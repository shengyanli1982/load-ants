// src/doh/server.rs

use crate::doh::handlers::{handle_doh_get, handle_doh_post, handle_json_get};
use crate::doh::state::AppState;
use crate::error::AppError;
use crate::handler::RequestHandler;
use axum::{routing::get, Router};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::{error, info};

const DOH_QUERY_PATH: &str = "/dns-query";
const JSON_QUERY_PATH: &str = "/resolve";

/// DoH 服务器结构体
pub struct DoHServer {
    /// 监听地址
    bind_addr: SocketAddr,
    /// DNS 请求处理器
    handler: Arc<RequestHandler>,
    /// 关闭信号发送端
    shutdown_tx: oneshot::Sender<()>,
    /// 关闭信号接收端
    shutdown_rx: oneshot::Receiver<()>,
}

impl DoHServer {
    /// 创建新的 DoH 服务器
    pub fn new(bind_addr: SocketAddr, _timeout: u64, handler: Arc<RequestHandler>) -> Self {
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        Self {
            bind_addr,
            handler,
            shutdown_tx,
            shutdown_rx,
        }
    }

    /// 创建应用路由
    fn create_router(&self) -> Router {
        // 创建应用程序状态
        let app_state = AppState {
            handler: self.handler.clone(),
        };

        // 创建路由
        Router::new()
            // RFC 8484 DoH 端点
            .route(DOH_QUERY_PATH, get(handle_doh_get).post(handle_doh_post))
            // Google JSON DoH 端点
            .route(JSON_QUERY_PATH, get(handle_json_get))
            // 添加应用程序状态
            .with_state(app_state)
    }

    /// 启动 DoH 服务器
    pub async fn run(self, subsys: SubsystemHandle) -> Result<(), AppError> {
        // 创建路由
        let app = self.create_router();

        // 创建 TCP 监听器
        let listener = match TcpListener::bind(self.bind_addr).await {
            Ok(listener) => {
                info!("DoH server listening on {}", self.bind_addr);
                listener
            }
            Err(e) => {
                error!("Failed to bind DoH server: {}", e);
                return Err(AppError::Io(e));
            }
        };

        // 获取关闭信号接收端
        let shutdown_rx = self.shutdown_rx;

        // 启动 HTTP 服务器
        tokio::select! {
            result = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>()
            )
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
                info!("DoH server received shutdown signal");
            }) => {
                if let Err(e) = result {
                    error!("DoH server error: {}", e);
                } else {
                    info!("DoH server completed normally");
                }
                Ok(())
            }
            _ = subsys.on_shutdown_requested() => {
                info!("Shutdown requested, stopping DoH server");
                let _ = self.shutdown_tx.send(());
                Ok(())
            }
        }
    }
}
