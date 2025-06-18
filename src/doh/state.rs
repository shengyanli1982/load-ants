// src/doh/state.rs

use crate::handler::RequestHandler;
use std::sync::Arc;

/// 应用程序状态结构体
#[derive(Clone)]
pub struct AppState {
    /// DNS 请求处理器
    pub handler: Arc<RequestHandler>,
}
