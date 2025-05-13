use crate::cache::DnsCache;
use crate::error::AppError;
use crate::router::Router;
use crate::upstream::UpstreamManager;
use crate::metrics::METRICS;
use crate::r#const::{protocol_labels, processing_labels, error_labels, cache_labels, rule_type_labels};
use hickory_proto::op::{Message, MessageType, ResponseCode};
use crate::config::RouteAction;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, error, warn};

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
    pub fn new(
        cache: Arc<DnsCache>,
        router: Arc<Router>,
        upstream: Arc<UpstreamManager>,
    ) -> Self {
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
        
        // 检查是否为查询请求
        if request.message_type() != MessageType::Query {
            return Err(AppError::Internal("Not a query request".to_string()));
        }
        
        // 获取查询
        let query = match request.queries().first() {
            Some(q) => q,
            None => {
                // 记录错误指标
                METRICS.dns_request_errors_total()
                    .with_label_values(&[error_labels::EMPTY_QUERY])
                    .inc();
                return Err(AppError::Internal("Empty query".to_string()));
            }
        };
        
        let query_name = query.name();
        let query_type = query.query_type();
        let query_class = query.query_class();
        
        // 记录查询类型指标
        METRICS.dns_query_type_total()
            .with_label_values(&[query_type.to_string().as_str()])
            .inc();
        
        debug!(
            "Processing DNS query: {} ({} {})",
            query_name.to_utf8(),
            query_type,
            query_class
        );
        
        // 检查缓存（如果启用）
        if self.cache.is_enabled() {
            if let Some(cached_response) = self.cache.get(request).await {
                debug!("Cache hit: {} ({} {})", query_name.to_utf8(), query_type, query_class);
                
                // 设置响应ID与请求ID相匹配
                let mut response = cached_response.clone();
                response.set_id(request.id());
                
                // 记录请求处理时间
                let duration = start_time.elapsed();
                METRICS.dns_request_duration_seconds()
                    .with_label_values(&[processing_labels::CACHED, query_type.to_string().as_str()])
                    .observe(duration.as_secs_f64());
                
                return Ok(response);
            }
        }
        
        // 查找路由规则
        let route_match = match self.router.find_match(query_name) {
            Ok(m) => m,
            Err(e) => {
                warn!("Route matching failed: {} - {}", query_name.to_utf8(), e);
                
                // 记录路由失败指标
                METRICS.dns_request_errors_total()
                    .with_label_values(&[error_labels::ROUTE_ERROR])
                    .inc();
                
                return self.create_error_response(request, ResponseCode::ServFail);
            }
        };
        
        // 记录路由匹配指标
        METRICS.route_matches_total()
            .with_label_values(&[&route_match.rule_type, route_match.target.as_deref().unwrap_or(rule_type_labels::NO_TARGET)])
            .inc();
        
        debug!(
            "Route match: {} -> Rule type: {}, Pattern: {}, Action: {:?}, Target: {:?}",
            query_name.to_utf8(),
            route_match.rule_type,
            route_match.pattern,
            route_match.action,
            route_match.target
        );
        
        // 根据路由动作处理请求
        let response = match route_match.action {
            RouteAction::Forward => {
                // 获取目标上游组
                let target_group = match route_match.target {
                    Some(ref group) => group, // 使用引用代替克隆
                    None => {
                        error!(
                            "Route rule configuration error: Forward action missing target group - {}",
                            query_name.to_utf8()
                        );
                        
                        // 记录错误指标
                        METRICS.dns_request_errors_total()
                            .with_label_values(&[error_labels::MISSING_TARGET])
                            .inc();
                        
                        return self.create_error_response(request, ResponseCode::ServFail);
                    }
                };
                
                // 转发到上游
                match self.upstream.forward(request, target_group).await {
                    Ok(response) => response,
                    Err(e) => {
                        error!("Upstream request failed: {} - {}", target_group, e);
                        
                        // 记录错误指标
                        METRICS.dns_request_errors_total()
                            .with_label_values(&[error_labels::UPSTREAM_ERROR])
                            .inc();
                        
                        return self.create_error_response(request, ResponseCode::ServFail);
                    }
                }
            }
            RouteAction::Block => {
                debug!("Blocking domain: {}", query_name.to_utf8());
                self.create_error_response(request, ResponseCode::NXDomain)?
            }
        };
        
        // 记录响应代码指标
        METRICS.dns_response_codes_total()
            .with_label_values(&[response.response_code().to_string().as_str()])
            .inc();
        
        // 缓存响应（只缓存成功响应，且缓存已启用）
        if self.cache.is_enabled() && response.response_code() == ResponseCode::NoError {
            if let Err(e) = self.cache.insert(request, response.clone()).await {
                warn!("Cache insertion failed: {}", e);
                
                // 记录错误指标
                METRICS.cache_operations_total()
                    .with_label_values(&[cache_labels::INSERT_ERROR])
                    .inc();
            } else {
                // 记录缓存插入指标
                METRICS.cache_operations_total()
                    .with_label_values(&[cache_labels::INSERT])
                    .inc();
                
                // 更新缓存条目计数
                METRICS.cache_entries().set(self.cache.len().await as i64);
            }
        }
        
        // 记录请求处理时间
        let duration = start_time.elapsed();
        METRICS.dns_request_duration_seconds()
            .with_label_values(&[processing_labels::RESOLVED, query_type.to_string().as_str()])
            .observe(duration.as_secs_f64());
        
        Ok(response)
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

// 处理 DNS 请求的入口函数
pub async fn handle_request(
    request: Message,
    handler: &Arc<RequestHandler>,
) -> Result<Message, AppError> {
    debug!("Received DNS request: {:?}", request);
    
    // 记录DNS请求总数
    METRICS.dns_requests_total()
        .with_label_values(&[protocol_labels::UNKNOWN]) // 此函数无法获知请求协议
        .inc();
    
    let response = handler.handle_request(&request).await?;
    
    debug!("Sending DNS response: {:?}", response);
    
    Ok(response)
}
