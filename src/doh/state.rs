// Axum 路由共享状态定义。

use crate::handler::RequestHandler;
use crate::rate_limit::RateLimiter;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub handler: Arc<RequestHandler>,
    pub rate_limiter: Option<Arc<RateLimiter>>,
}
