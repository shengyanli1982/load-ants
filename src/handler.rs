use crate::{
    metrics::{normalize_response_code, query_type_index, METRICS},
    r#const::{cache_op_indices, error_indices, processing_indices},
    AppError, CacheKey, CacheResult, CoalescingMap, DnsCache, RouteAction, Router, UpstreamManager,
};
use hickory_proto::op::{Edns, Message, MessageType, ResponseCode};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, error};

struct CoalesceGuard<'a> {
    map: &'a CoalescingMap,
    key: CacheKey,
    entry: Arc<crate::coalesce::CoalesceEntry>,
}

impl Drop for CoalesceGuard<'_> {
    fn drop(&mut self) {
        self.map.remove(&self.key);
        // D-19: leader 提前返回（路由错误、panic、任务取消）时 cell 可能未设置，
        // 兜底补写 Err，避免 follower 收到误导性的 "Coalescing cell not set"
        // 或空等满超时。与 BackgroundCoalesceGuard 行为对齐。
        if self.entry.cell.get().is_none() {
            self.entry.cell.set(Err("leader aborted".to_string())).ok();
        }
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
    /// D-04: follower 的 coalesce 等待时长，构造时从运行时配置派生：
    /// 所有上游组最坏转发总 deadline（request_timeout × 服务器台数的最大值）+ margin，
    /// 保证等待时长 ≥ leader 最坏转发时长，不再使用 5s 编译期常量。
    coalesce_wait_timeout: Duration,
}

impl RequestHandler {
    pub fn new(
        cache: Arc<DnsCache>,
        router: Arc<RwLock<Arc<Router>>>,
        upstream: Arc<UpstreamManager>,
    ) -> Self {
        let coalesce_wait_timeout = upstream.max_forward_deadline()
            + Duration::from_secs(crate::r#const::coalesce_limits::WAIT_MARGIN_SECS);
        Self {
            cache,
            router,
            upstream,
            coalesce: CoalescingMap::new(),
            coalesce_wait_timeout,
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
        let query_type_idx = query_type_index(query_type);

        METRICS.query_types[query_type_idx].inc();

        debug!("Query received");

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
            .check_cache(request, cache_key.as_ref(), query_type_idx, &start_time)
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
                        debug!("Query blocked");
                        Self::create_error_response(request, ResponseCode::Refused)?
                    }
                };

                let duration = start_time.elapsed();
                METRICS.processing_durations[processing_indices::RESOLVED][query_type_idx]
                    .observe(duration.as_secs_f64());

                debug!(
                    duration_ms = duration.as_secs_f64() * 1000.0,
                    rcode = normalize_response_code(response.response_code()),
                    answers = response.answer_count(),
                    "Query resolved"
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

            // D-19: 路由错误提前返回前，把相同错误写入 cell，
            // 让已注册的 follower 立即失败而不是空等/收到误导性错误。
            let route_match = match self.find_route_match(query_name).await {
                Ok(route_match) => route_match,
                Err(e) => {
                    coalesce_entry.cell.set(Err(e.to_string())).ok();
                    return Err(e);
                }
            };

            let mut response = match route_match.action {
                RouteAction::Forward => {
                    self.handle_forward(request, &route_match, query_name)
                        .await?
                }
                RouteAction::Block => {
                    debug!("Query blocked");
                    Self::create_error_response(request, ResponseCode::Refused)?
                }
            };
            response.set_id(request.id());

            self.cache_response(request, response.clone()).await;

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
            METRICS.processing_durations[processing_indices::RESOLVED][query_type_idx]
                .observe(duration.as_secs_f64());

            debug!(
                duration_ms = duration.as_secs_f64() * 1000.0,
                rcode = normalize_response_code(response.response_code()),
                answers = response.answer_count(),
                "Query resolved"
            );

            Ok(response)
        } else {
            let notified = coalesce_entry.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();

            if coalesce_entry.cell.get().is_none()
                && tokio::time::timeout(self.coalesce_wait_timeout, notified.as_mut())
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
                METRICS.request_errors[error_indices::EMPTY_QUERY].inc();
                Err(AppError::Internal("Empty query".to_string()))
            }
        }
    }

    async fn check_cache(
        &self,
        request: &Message,
        cache_key: Option<&CacheKey>,
        query_type_idx: usize,
        start_time: &Instant,
    ) -> CacheResult {
        if !self.cache.is_enabled() {
            return CacheResult::Miss;
        }

        let key = match cache_key {
            Some(k) => k,
            None => return CacheResult::Miss,
        };

        let result = self.cache.get(key).await;

        match result {
            CacheResult::Fresh(cached_response) => {
                debug!("Cache hit");

                let mut response = cached_response;
                response.set_id(request.id());

                let duration = start_time.elapsed();
                METRICS.processing_durations[processing_indices::CACHED][query_type_idx]
                    .observe(duration.as_secs_f64());

                CacheResult::Fresh(response)
            }
            CacheResult::Stale(stale_response) => {
                debug!("Cache stale served, background revalidation scheduled");

                let mut response = stale_response;
                response.set_id(request.id());

                let duration = start_time.elapsed();
                METRICS.processing_durations[processing_indices::CACHED][query_type_idx]
                    .observe(duration.as_secs_f64());

                CacheResult::Stale(response)
            }
            CacheResult::Miss => {
                METRICS.cache_operations[cache_op_indices::MISS].inc();
                // 同步 cache entries gauge（moka 可能已惰性驱逐条目，
                // miss 时同步真实计数可修正 insert-only 更新导致的漂移）
                METRICS
                    .cache_entries
                    .set(self.cache.approximate_len() as i64);
                // OPT-3: 不再为计时 cache.get 单独构造 Instant（hit 路径纯浪费），
                // duration_ms 随用随取 start_time.elapsed()（请求路径耗时，与
                // "Query resolved" 口径一致；tracing 字段惰性求值，debug 关闭零成本）
                debug!(
                    duration_ms = start_time.elapsed().as_secs_f64() * 1000.0,
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
                let query = request
                    .queries()
                    .first()
                    .map(|q| q.name().to_utf8())
                    .unwrap_or_default();
                debug!(
                    query = %query,
                    reason = "no_cache_key",
                    "Background revalidation failed"
                );
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
                            debug!(reason = "no_query", "Background revalidation failed");
                            return;
                        }
                    };
                    let query_name = query.name();

                    debug!(
                        query = %query_name.to_utf8(),
                        "Background revalidation started"
                    );

                    let route_match = {
                        let router_guard = router.read().await;
                        match router_guard.find_match(query_name) {
                            Ok(m) => m,
                            Err(e) => {
                                debug!(
                                    query = %query_name.to_utf8(),
                                    reason = "route_match_failed",
                                    error = %e,
                                    "Background revalidation failed"
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
                            query = %query_name.to_utf8(),
                            reason = "blocked",
                            "Background revalidation skipped"
                        );
                        return;
                    }

                    let target_group = match &route_match.target {
                        Some(g) => g,
                        None => {
                            debug!(
                                query = %query_name.to_utf8(),
                                reason = "missing_target",
                                "Background revalidation failed"
                            );
                            return;
                        }
                    };

                    let result = upstream.forward(&request, target_group).await;
                    match result {
                        Ok(response) => {
                            if let Err(e) = cache.insert(&request, response.clone()).await {
                                debug!(
                                    query = %query_name.to_utf8(),
                                    reason = "cache_insert_failed",
                                    error = %e,
                                    "Background revalidation failed"
                                );
                            } else {
                                debug!(
                                    query = %query_name.to_utf8(),
                                    "Background revalidation completed"
                                );
                            }
                            METRICS.cache_entries.set(cache.approximate_len() as i64);
                            _guard.entry.cell.set(Ok(response)).ok();
                        }
                        Err(e) => {
                            debug!(
                                query = %query_name.to_utf8(),
                                group = %target_group,
                                reason = "upstream_failed",
                                error = %e,
                                "Background revalidation failed"
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
                    duration_ms = route_match_time.elapsed().as_secs_f64() * 1000.0,
                    "Route match found"
                );
                m
            }
            Err(e) => {
                debug!(
                    query = %query_name.to_utf8(),
                    error = %e,
                    "Failed to match route"
                );

                METRICS.request_errors[error_indices::ROUTE_ERROR].inc();

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
                debug!(
                    query = %query_name.to_utf8(),
                    reason = "target_missing",
                    "Failed to resolve forward target"
                );

                METRICS.request_errors[error_indices::MISSING_TARGET].inc();

                return Self::create_error_response(request, ResponseCode::ServFail);
            }
        };

        let upstream_time = Instant::now();
        let result = self.upstream.forward(request, target_group).await;
        debug!(
            group = %target_group,
            duration_ms = upstream_time.elapsed().as_secs_f64() * 1000.0,
            "Upstream forwarding completed"
        );

        match result {
            Ok(response) => Ok(response),
            Err(e) => {
                METRICS.request_errors[error_indices::UPSTREAM_ERROR].inc();

                if self.cache.is_enabled() && self.cache.stale_while_revalidate_enabled() {
                    if let Some(stale_response) = self.cache.get_stale_entry(request).await {
                        // 红线：group 为无界标签（配置/远程规则来源），严禁预解析
                        METRICS
                            .stale_fallback_total
                            .with_label_values(&[target_group])
                            .inc();

                        debug!(
                            group = %target_group,
                            error = %e,
                            "Stale response served after upstream failure"
                        );

                        let mut response = stale_response;
                        response.set_id(request.id());
                        return Ok(response);
                    }
                }

                error!(
                    group = %target_group,
                    error = &e as &dyn std::error::Error,
                    "Failed to forward request to upstream"
                );

                Self::create_error_response(request, ResponseCode::ServFail)
            }
        }
    }

    async fn cache_response(&self, request: &Message, response: Message) {
        if !self.cache.is_enabled() {
            return;
        }

        let cache_insert_time = Instant::now();
        if let Err(e) = self.cache.insert(request, response).await {
            debug!(error = %e, "Failed to insert cache entry");
        } else {
            debug!(
                duration_ms = cache_insert_time.elapsed().as_secs_f64() * 1000.0,
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
