use crate::{
    cache_labels, error_labels,
    metrics::{normalize_query_type_label, METRICS},
    processing_labels, AppError, CacheKey, CacheResult, CoalescingMap, DnsCache, RouteAction,
    Router, UpstreamManager,
};
use hickory_proto::op::{Edns, Message, MessageType, ResponseCode};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

const COALESCE_WAIT_TIMEOUT: Duration =
    Duration::from_secs(crate::r#const::http_client_limits::DEFAULT_REQUEST_TIMEOUT);

struct CoalesceGuard<'a> {
    map: &'a CoalescingMap,
    key: CacheKey,
    entry: Arc<crate::coalesce::CoalesceEntry>,
}

impl Drop for CoalesceGuard<'_> {
    fn drop(&mut self) {
        self.map.remove(&self.key);
        self.entry.notify.notify_waiters();
    }
}

/// Owned RAII guard for background refresh tasks.
/// Ensures coalesce map cleanup, error cell setting, and waiter notification on drop.
struct BackgroundCoalesceGuard {
    map: CoalescingMap,
    key: CacheKey,
    entry: Arc<crate::coalesce::CoalesceEntry>,
}

impl Drop for BackgroundCoalesceGuard {
    fn drop(&mut self) {
        self.map.remove(&self.key);
        if self.entry.cell.get().is_none() {
            self.entry
                .cell
                .set(Err("background refresh aborted".to_string()))
                .ok();
        }
        self.entry.notify.notify_waiters();
    }
}

struct InflightGuard;

impl Drop for InflightGuard {
    fn drop(&mut self) {
        METRICS.inflight_requests.dec();
    }
}

pub struct RequestHandler {
    cache: Arc<DnsCache>,
    router: Arc<RwLock<Arc<Router>>>,
    upstream: Arc<UpstreamManager>,
    coalesce: CoalescingMap,
}

impl RequestHandler {
    pub fn new(
        cache: Arc<DnsCache>,
        router: Arc<RwLock<Arc<Router>>>,
        upstream: Arc<UpstreamManager>,
    ) -> Self {
        Self {
            cache,
            router,
            upstream,
            coalesce: CoalescingMap::new(),
        }
    }

    pub fn coalesce_pending(&self) -> usize {
        self.coalesce.len()
    }

    pub async fn handle_request(&self, request: &Message) -> Result<Message, AppError> {
        let start_time = Instant::now();
        let _inflight = {
            METRICS.inflight_requests.inc();
            InflightGuard
        };

        let query = self.validate_request(request)?;
        let query_name = query.name();
        let query_type = query.query_type();
        let query_type_label = normalize_query_type_label(query_type);

        METRICS
            .dns_query_type_total
            .with_label_values(&[query_type_label])
            .inc();

        debug!(
            "Received DNS query: {} ({})",
            query_name.to_utf8(),
            query_type
        );

        // Only allocate a CacheKey when caching is enabled; when disabled,
        // skip cache lookup, coalescing and key construction entirely.
        let cache_key = if self.cache.is_enabled() {
            match CacheKey::from_message(request) {
                Some(k) => Some(k),
                None => {
                    debug!("Cannot create cache key from message");
                    None
                }
            }
        } else {
            None
        };

        let cache_result = self
            .check_cache(
                request,
                cache_key.as_ref(),
                query_name,
                query_type,
                query_type_label,
                &start_time,
            )
            .await;

        match cache_result {
            CacheResult::Fresh(response) => return Ok(response),
            CacheResult::Stale(response) => {
                self.spawn_background_refresh(request);
                return Ok(response);
            }
            CacheResult::Miss => {}
        }

        // When cache_key is unavailable (cache disabled or key creation failed),
        // skip coalescing and forward directly.
        let cache_key = match cache_key {
            Some(k) => k,
            None => {
                let route_match = self.find_route_match(query_name).await?;
                let response = match route_match.action {
                    RouteAction::Forward => {
                        self.handle_forward(request, &route_match, query_name)
                            .await?
                    }
                    RouteAction::Block => {
                        debug!("Blocking domain: {}", query_name.to_utf8());
                        Self::create_error_response(request, ResponseCode::Refused)?
                    }
                };

                let duration = start_time.elapsed();
                METRICS
                    .dns_request_processing_duration_seconds
                    .with_label_values(&[processing_labels::RESOLVED, query_type_label])
                    .observe(duration.as_secs_f64());

                debug!(
                    duration = ?duration,
                    query = %query_name.to_utf8(),
                    "DNS request resolved"
                );

                return Ok(response);
            }
        };

        let (coalesce_entry, is_leader) = {
            match self.coalesce.entry(cache_key.clone()) {
                dashmap::mapref::entry::Entry::Occupied(e) => (e.get().clone(), false),
                dashmap::mapref::entry::Entry::Vacant(e) => {
                    let coalesce_entry = Arc::new(crate::coalesce::CoalesceEntry::new());
                    e.insert(coalesce_entry.clone());
                    (coalesce_entry, true)
                }
            }
        };

        if is_leader {
            let _guard = CoalesceGuard {
                map: &self.coalesce,
                key: cache_key,
                entry: coalesce_entry.clone(),
            };

            let route_match = self.find_route_match(query_name).await?;

            let mut response = match route_match.action {
                RouteAction::Forward => {
                    self.handle_forward(request, &route_match, query_name)
                        .await?
                }
                RouteAction::Block => {
                    debug!("Blocking domain: {}", query_name.to_utf8());
                    Self::create_error_response(request, ResponseCode::Refused)?
                }
            };
            response.set_id(request.id());

            self.cache_response(request, response.clone(), query_name)
                .await;

            let result = if response.response_code() == ResponseCode::ServFail {
                Err(format!(
                    "Upstream returned SERVFAIL for {}",
                    query_name.to_utf8()
                ))
            } else {
                Ok(response.clone())
            };
            coalesce_entry.cell.set(result).ok();

            let duration = start_time.elapsed();
            METRICS
                .dns_request_processing_duration_seconds
                .with_label_values(&[processing_labels::RESOLVED, query_type_label])
                .observe(duration.as_secs_f64());

            debug!(
                duration = ?duration,
                query = %query_name.to_utf8(),
                "DNS request resolved"
            );

            Ok(response)
        } else {
            let notified = coalesce_entry.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();

            if coalesce_entry.cell.get().is_none()
                && tokio::time::timeout(COALESCE_WAIT_TIMEOUT, notified.as_mut())
                    .await
                    .is_err()
            {
                return Err(AppError::Timeout);
            }

            match coalesce_entry.cell.get() {
                Some(Ok(response)) => {
                    let mut response = response.clone();
                    response.set_id(request.id());
                    Ok(response)
                }
                Some(Err(err_msg)) => Err(AppError::Internal(err_msg.clone())),
                None => Err(AppError::Internal("Coalescing cell not set".to_string())),
            }
        }
    }

    pub fn validate_request<'a>(
        &self,
        request: &'a Message,
    ) -> Result<&'a hickory_proto::op::Query, AppError> {
        if request.message_type() != MessageType::Query {
            return Err(AppError::Internal("Not a query request".to_string()));
        }

        match request.queries().first() {
            Some(q) => Ok(q),
            None => {
                METRICS
                    .dns_request_errors_total
                    .with_label_values(&[error_labels::EMPTY_QUERY])
                    .inc();
                Err(AppError::Internal("Empty query".to_string()))
            }
        }
    }

    async fn check_cache(
        &self,
        request: &Message,
        cache_key: Option<&CacheKey>,
        query_name: &hickory_proto::rr::Name,
        query_type: hickory_proto::rr::RecordType,
        query_type_label: &'static str,
        start_time: &Instant,
    ) -> CacheResult {
        if !self.cache.is_enabled() {
            return CacheResult::Miss;
        }

        let key = match cache_key {
            Some(k) => k,
            None => return CacheResult::Miss,
        };

        let cache_check_time = Instant::now();
        let result = self.cache.get(key).await;

        match result {
            CacheResult::Fresh(cached_response) => {
                debug!(
                    query = %query_name.to_utf8(),
                    query_type = %query_type,
                    "Cache hit"
                );

                let mut response = cached_response;
                response.set_id(request.id());

                let duration = start_time.elapsed();
                METRICS
                    .dns_request_processing_duration_seconds
                    .with_label_values(&[processing_labels::CACHED, query_type_label])
                    .observe(duration.as_secs_f64());

                CacheResult::Fresh(response)
            }
            CacheResult::Stale(stale_response) => {
                debug!(
                    query = %query_name.to_utf8(),
                    query_type = %query_type,
                    "Cache stale, serving stale response, revalidating in background"
                );

                let mut response = stale_response;
                response.set_id(request.id());

                let duration = start_time.elapsed();
                METRICS
                    .dns_request_processing_duration_seconds
                    .with_label_values(&[processing_labels::CACHED, query_type_label])
                    .observe(duration.as_secs_f64());

                CacheResult::Stale(response)
            }
            CacheResult::Miss => {
                METRICS
                    .cache_operations_total
                    .with_label_values(&[cache_labels::MISS])
                    .inc();
                // 同步 cache entries gauge（moka 可能已惰性驱逐条目，
                // miss 时同步真实计数可修正 insert-only 更新导致的漂移）
                METRICS
                    .cache_entries
                    .set(self.cache.approximate_len() as i64);
                debug!(
                    query = %query_name.to_utf8(),
                    cache_check_duration = ?cache_check_time.elapsed(),
                    "Cache miss"
                );
                CacheResult::Miss
            }
        }
    }

    fn spawn_background_refresh(&self, request: &Message) {
        let cache_key = match CacheKey::from_message(request) {
            Some(k) => k,
            None => {
                warn!("Background refresh: cannot create cache key");
                return;
            }
        };

        match self.coalesce.entry(cache_key.clone()) {
            dashmap::mapref::entry::Entry::Occupied(_) => {
                debug!("Background refresh already pending for key, skipping duplicate");
            }
            dashmap::mapref::entry::Entry::Vacant(e) => {
                let coalesce_entry = Arc::new(crate::coalesce::CoalesceEntry::new());
                e.insert(coalesce_entry.clone());

                let request = request.clone();
                let cache = self.cache.clone();
                let router = self.router.clone();
                let upstream = self.upstream.clone();
                let coalesce = self.coalesce.clone();

                tokio::spawn(async move {
                    let _guard = BackgroundCoalesceGuard {
                        map: coalesce,
                        key: cache_key,
                        entry: coalesce_entry,
                    };

                    let query = match request.queries().first() {
                        Some(q) => q,
                        None => {
                            warn!("Background refresh: no query in request");
                            return;
                        }
                    };
                    let query_name = query.name();

                    debug!(
                        "Background revalidation started for {}",
                        query_name.to_utf8()
                    );

                    let route_match = {
                        let router_guard = router.read().await;
                        match router_guard.find_match(query_name) {
                            Ok(m) => m,
                            Err(e) => {
                                warn!(
                                    "Background revalidation route failed: {} - {}",
                                    query_name.to_utf8(),
                                    e
                                );
                                _guard
                                    .entry
                                    .cell
                                    .set(Err(format!("Route match failed: {}", e)))
                                    .ok();
                                return;
                            }
                        }
                    };

                    if route_match.action == RouteAction::Block {
                        debug!(
                            "Background revalidation: blocking domain {}",
                            query_name.to_utf8()
                        );
                        return;
                    }

                    let target_group = match &route_match.target {
                        Some(g) => g,
                        None => {
                            warn!(
                                "Background revalidation: forward action missing target - {}",
                                query_name.to_utf8()
                            );
                            return;
                        }
                    };

                    let result = upstream.forward(&request, target_group).await;
                    match result {
                        Ok(response) => {
                            if let Err(e) = cache.insert(&request, response.clone()).await {
                                warn!("Background revalidation cache insert failed: {}", e);
                            } else {
                                info!(
                                    "Background revalidation completed for {}",
                                    query_name.to_utf8()
                                );
                            }
                            METRICS.cache_entries.set(cache.approximate_len() as i64);
                            _guard.entry.cell.set(Ok(response)).ok();
                        }
                        Err(e) => {
                            warn!(
                                "Background revalidation upstream failed: {} - {}",
                                target_group, e
                            );
                            _guard
                                .entry
                                .cell
                                .set(Err(format!("Upstream failed: {}", e)))
                                .ok();
                        }
                    }
                });
            }
        }
    }

    async fn find_route_match(
        &self,
        query_name: &hickory_proto::rr::Name,
    ) -> Result<crate::router::RouteMatch, AppError> {
        let route_match_time = Instant::now();
        let router = self.router.read().await;
        let route_match = match router.find_match(query_name) {
            Ok(m) => {
                debug!(
                    query = %query_name.to_utf8(),
                    route_match_duration = ?route_match_time.elapsed(),
                    "Route match found"
                );
                m
            }
            Err(e) => {
                warn!("Route matching failed: {} - {}", query_name.to_utf8(), e);

                METRICS
                    .dns_request_errors_total
                    .with_label_values(&[error_labels::ROUTE_ERROR])
                    .inc();

                return Err(AppError::Internal(format!("Route matching failed: {}", e)));
            }
        };

        debug!(
            query = %query_name.to_utf8(),
            rule_type = %route_match.rule_type,
            pattern = %route_match.pattern,
            action = %<&'static str>::from(route_match.action),
            target = route_match.target.as_deref().unwrap_or("None"),
            "Route match details"
        );

        Ok(route_match)
    }

    async fn handle_forward(
        &self,
        request: &Message,
        route_match: &crate::router::RouteMatch,
        query_name: &hickory_proto::rr::Name,
    ) -> Result<Message, AppError> {
        let target_group = match &route_match.target {
            Some(group) => group,
            None => {
                error!(
                    "Route rule configuration error: Forward action missing target group - {}",
                    query_name.to_utf8()
                );

                METRICS
                    .dns_request_errors_total
                    .with_label_values(&[error_labels::MISSING_TARGET])
                    .inc();

                return Self::create_error_response(request, ResponseCode::ServFail);
            }
        };

        let upstream_time = Instant::now();
        let result = self.upstream.forward(request, target_group).await;
        debug!(
            target_group = %target_group,
            query = %query_name.to_utf8(),
            upstream_duration = ?upstream_time.elapsed(),
            "Upstream forwarding completed"
        );

        match result {
            Ok(response) => Ok(response),
            Err(e) => {
                error!("Upstream request failed: {} - {}", target_group, e);

                METRICS
                    .dns_request_errors_total
                    .with_label_values(&[error_labels::UPSTREAM_ERROR])
                    .inc();

                if self.cache.is_enabled() && self.cache.stale_while_revalidate_enabled() {
                    if let Some(stale_response) = self.cache.get_stale_entry(request).await {
                        METRICS
                            .stale_fallback_total
                            .with_label_values(&[target_group])
                            .inc();

                        warn!(
                            query = %query_name.to_utf8(),
                            "Upstream failure, serving stale cached response"
                        );

                        let mut response = stale_response;
                        response.set_id(request.id());
                        return Ok(response);
                    }
                }

                Self::create_error_response(request, ResponseCode::ServFail)
            }
        }
    }

    async fn cache_response(
        &self,
        request: &Message,
        response: Message,
        query_name: &hickory_proto::rr::Name,
    ) {
        if !self.cache.is_enabled() {
            return;
        }

        let cache_insert_time = Instant::now();
        if let Err(e) = self.cache.insert(request, response).await {
            warn!("Cache insertion failed: {}", e);
        } else {
            debug!(
                query = %query_name.to_utf8(),
                cache_insert_duration = ?cache_insert_time.elapsed(),
                "Cache insertion completed"
            );
        }

        METRICS
            .cache_entries
            .set(self.cache.approximate_len() as i64);
    }

    pub fn create_error_response(
        request: &Message,
        response_code: ResponseCode,
    ) -> Result<Message, AppError> {
        let mut response = Message::new();
        response.set_id(request.id());
        response.set_message_type(MessageType::Response);
        response.set_op_code(request.op_code());
        response.set_recursion_desired(request.recursion_desired());
        response.set_recursion_available(true);
        response.set_response_code(response_code);

        for query in request.queries() {
            response.add_query(query.clone());
        }

        // P1-2: RFC 6891 §7 — 若请求包含 EDNS0 OPT，响应也必须包含 OPT 记录
        if let Some(req_edns) = request.extensions() {
            let mut edns = Edns::new();
            edns.set_max_payload(req_edns.max_payload());
            edns.set_version(0);
            // 错误响应不携带 DO bit，不复制 DNSSEC 相关选项
            response.set_edns(edns);
        }

        Ok(response)
    }
}
