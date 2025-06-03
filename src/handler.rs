use crate::cache::DnsCache;
use crate::config::RouteAction;
use crate::error::AppError;
use crate::metrics::METRICS;
use crate::r#const::{
    cache_labels, error_labels, processing_labels, protocol_labels, rule_action_labels,
    rule_source_labels, rule_type_labels,
};
use crate::router::Router;
use crate::upstream::UpstreamManager;
use hickory_proto::op::{Message, MessageType, ResponseCode};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, error, info, warn};

// DNS 请求处理器
pub struct RequestHandler {
    // DNS 缓存
    cache: Arc<DnsCache>,
    // 路由引擎
    router: Arc<Router>,
    // 上游管理器
    upstream: Arc<UpstreamManager>,
}

impl RequestHandler {
    // 创建 DNS 请求处理器
    pub fn new(cache: Arc<DnsCache>, router: Arc<Router>, upstream: Arc<UpstreamManager>) -> Self {
        Self {
            cache,
            router,
            upstream,
        }
    }

    // 处理 DNS 请求
    pub async fn handle_request(&self, request: &Message) -> Result<Message, AppError> {
        // 记录请求开始时间
        let start_time = Instant::now();

        // 检查是否为查询请求并获取查询内容
        let query = self.validate_request(request)?;
        let query_name = query.name();
        let query_type = query.query_type();
        let query_class = query.query_class();

        // 记录查询类型指标
        METRICS
            .dns_query_type_total()
            .with_label_values(&[query_type.to_string().as_str()])
            .inc();

        debug!(
            "Processing DNS query: {} ({} {})",
            query_name.to_utf8(),
            query_type,
            query_class
        );

        // 尝试从缓存获取响应
        if let Some(response) = self
            .check_cache(request, query_name, query_type, &start_time)
            .await
        {
            return Ok(response);
        }

        // 查找路由规则
        let route_match = self.find_route_match(query_name).await?;

        // 根据路由动作处理请求
        let response = match route_match.action {
            RouteAction::Forward => {
                self.handle_forward(request, &route_match, query_name)
                    .await?
            }
            RouteAction::Block => {
                debug!("Blocking domain: {}", query_name.to_utf8());
                self.create_error_response(request, ResponseCode::NXDomain)?
            }
        };

        // 记录响应代码指标
        METRICS
            .dns_response_codes_total()
            .with_label_values(&[response.response_code().to_string().as_str()])
            .inc();

        // 缓存响应
        self.cache_response(request, response.clone(), query_name)
            .await;

        // 记录请求处理时间
        let duration = start_time.elapsed();
        METRICS
            .dns_request_duration_seconds()
            .with_label_values(&[processing_labels::RESOLVED, query_type.to_string().as_str()])
            .observe(duration.as_secs_f64());

        info!(
            "DNS request processed in {:?} - {}",
            duration,
            query_name.to_utf8()
        );

        Ok(response)
    }

    // 验证请求有效性并获取查询
    fn validate_request<'a>(
        &self,
        request: &'a Message,
    ) -> Result<&'a hickory_proto::op::Query, AppError> {
        // 检查是否为查询请求
        if request.message_type() != MessageType::Query {
            return Err(AppError::Internal("Not a query request".to_string()));
        }

        // 获取查询
        match request.queries().first() {
            Some(q) => Ok(q),
            None => {
                // 记录错误指标
                METRICS
                    .dns_request_errors_total()
                    .with_label_values(&[error_labels::EMPTY_QUERY])
                    .inc();
                Err(AppError::Internal("Empty query".to_string()))
            }
        }
    }

    // 检查缓存中是否有响应
    async fn check_cache(
        &self,
        request: &Message,
        query_name: &hickory_proto::rr::Name,
        query_type: hickory_proto::rr::RecordType,
        start_time: &Instant,
    ) -> Option<Message> {
        if !self.cache.is_enabled() {
            return None;
        }

        let cache_check_time = Instant::now();
        if let Some(cached_response) = self.cache.get(request).await {
            debug!("Cache hit: {} ({})", query_name.to_utf8(), query_type);

            // 设置响应ID与请求ID相匹配
            let mut response = cached_response.clone();
            response.set_id(request.id());

            // 记录请求处理时间
            let duration = start_time.elapsed();
            METRICS
                .dns_request_duration_seconds()
                .with_label_values(&[processing_labels::CACHED, query_type.to_string().as_str()])
                .observe(duration.as_secs_f64());

            info!(
                "Cache hit: {} processed in {:?}",
                query_name.to_utf8(),
                duration
            );

            return Some(response);
        } else {
            // 记录缓存未命中指标
            METRICS
                .cache_operations_total()
                .with_label_values(&[cache_labels::MISS])
                .inc();
            info!(
                "Cache check for {} took {:?}",
                query_name.to_utf8(),
                cache_check_time.elapsed()
            );
        }

        None
    }

    // 查找路由规则
    async fn find_route_match(
        &self,
        query_name: &hickory_proto::rr::Name,
    ) -> Result<crate::router::RouteMatch, AppError> {
        let route_match_time = Instant::now();
        let route_match = match self.router.find_match(query_name) {
            Ok(m) => m,
            Err(e) => {
                warn!("Route matching failed: {} - {}", query_name.to_utf8(), e);

                // 记录路由失败指标
                METRICS
                    .dns_request_errors_total()
                    .with_label_values(&[error_labels::ROUTE_ERROR])
                    .inc();

                return Err(AppError::Internal(format!("Route matching failed: {}", e)));
            }
        };
        info!(
            "Route matching for {} took {:?}",
            query_name.to_utf8(),
            route_match_time.elapsed()
        );

        // 记录路由匹配指标
        METRICS
            .route_matches_total()
            .with_label_values(&[
                route_match.rule_type,
                route_match
                    .target
                    .as_deref()
                    .unwrap_or(rule_type_labels::NO_TARGET),
                rule_source_labels::STATIC,
                match route_match.action {
                    RouteAction::Forward => rule_action_labels::FORWARD,
                    RouteAction::Block => rule_action_labels::BLOCK,
                },
            ])
            .inc();

        debug!(
            "Route match: {} -> Rule type: '{}', Pattern: '{}', Action: {:?}, Target: {}",
            query_name.to_utf8(),
            route_match.rule_type,
            route_match.pattern,
            route_match.action,
            route_match.target.as_deref().unwrap_or("None")
        );

        Ok(route_match)
    }

    // 处理转发请求
    async fn handle_forward(
        &self,
        request: &Message,
        route_match: &crate::router::RouteMatch,
        query_name: &hickory_proto::rr::Name,
    ) -> Result<Message, AppError> {
        // 获取目标上游组
        let target_group = match &route_match.target {
            Some(group) => group,
            None => {
                error!(
                    "Route rule configuration error: Forward action missing target group - {}",
                    query_name.to_utf8()
                );

                // 记录错误指标
                METRICS
                    .dns_request_errors_total()
                    .with_label_values(&[error_labels::MISSING_TARGET])
                    .inc();

                return self.create_error_response(request, ResponseCode::ServFail);
            }
        };

        // 转发到上游
        let upstream_time = Instant::now();
        let result = self.upstream.forward(request, target_group).await;
        info!(
            "Upstream forwarding to {} for {} took {:?}",
            target_group,
            query_name.to_utf8(),
            upstream_time.elapsed()
        );

        match result {
            Ok(response) => Ok(response),
            Err(e) => {
                error!("Upstream request failed: {} - {}", target_group, e);

                // 记录错误指标
                METRICS
                    .dns_request_errors_total()
                    .with_label_values(&[error_labels::UPSTREAM_ERROR])
                    .inc();

                self.create_error_response(request, ResponseCode::ServFail)
            }
        }
    }

    // 缓存响应
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

            // 记录错误指标
            METRICS
                .cache_operations_total()
                .with_label_values(&[cache_labels::INSERT_ERROR])
                .inc();
        } else {
            info!(
                "Cache insertion for {} took {:?}",
                query_name.to_utf8(),
                cache_insert_time.elapsed()
            );
        }

        // 更新缓存条目计数
        METRICS.cache_entries().set(self.cache.len().await as i64);
    }

    // 创建错误响应
    fn create_error_response(
        &self,
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

        // 复制查询部分到响应
        for query in request.queries() {
            response.add_query(query.clone());
        }

        Ok(response)
    }
}

// 处理DNS请求（仅用于测试）
#[allow(dead_code)]
pub async fn handle_request(
    request: Message,
    handler: &Arc<RequestHandler>,
) -> Result<Message, AppError> {
    debug!("Received DNS request: {:?}", request);

    // 记录DNS请求总数
    METRICS
        .dns_requests_total()
        .with_label_values(&[protocol_labels::UNKNOWN]) // 此函数无法获知请求协议
        .inc();

    let response = handler.handle_request(&request).await?;

    debug!("Sending DNS response: {:?}", response);

    Ok(response)
}
