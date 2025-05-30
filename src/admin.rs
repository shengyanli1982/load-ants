// src/server/admin.rs

use crate::cache::DnsCache;
use crate::error::AppError;
use crate::metrics;
use axum::{
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemHandle};
use tracing::{error, info};

// 管理服务器
pub struct AdminServer {
    // 监听地址
    listen_addr: SocketAddr,
    // 停止信号接收端
    shutdown_rx: Option<oneshot::Receiver<()>>,
    // 停止信号发送端
    shutdown_tx: Option<oneshot::Sender<()>>,
    // DNS缓存引用
    cache: Option<Arc<DnsCache>>,
}

impl AdminServer {
    // 创建新的管理服务器
    pub fn new(listen_addr: SocketAddr) -> Self {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        Self {
            listen_addr,
            shutdown_rx: Some(shutdown_rx),
            shutdown_tx: Some(shutdown_tx),
            cache: None,
        }
    }

    // 设置DNS缓存引用
    pub fn with_cache(mut self, cache: Arc<DnsCache>) -> Self {
        self.cache = Some(cache);
        self
    }

    // 停止管理服务器
    pub fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
            info!("Admin server stop signal sent");
        }
    }

    // 启动管理服务器
    pub async fn start(&mut self) -> Result<(), AppError> {
        // 组合健康检查和指标路由
        let app = Router::new()
            .route("/health", get(health_handler))
            .route("/api/cache/refresh", post(refresh_cache_handler))
            .with_state(self.cache.clone())
            .merge(metrics::metrics_routes());

        let listener = TcpListener::bind(self.listen_addr).await?;
        info!("Admin server listening on {}", self.listen_addr);

        let shutdown_rx = self
            .shutdown_rx
            .take()
            .expect("Admin server already started");

        let server = axum::serve(listener, app);
        let server_with_graceful_shutdown = server.with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
            info!("Admin server received shutdown signal");
        });

        server_with_graceful_shutdown.await?;

        Ok(())
    }

    // 运行服务器（用于优雅关闭集成）
    #[allow(dead_code)]
    pub async fn start_server(mut self) -> Result<(), AppError> {
        self.start().await
    }
}

#[async_trait::async_trait]
impl IntoSubsystem<AppError> for AdminServer {
    async fn run(mut self, subsys: SubsystemHandle) -> Result<(), AppError> {
        tokio::select! {
            res = self.start() => {
                if let Err(err) = res {
                    error!("Admin server error: {}", err);
                    Err(err)
                } else {
                    info!("Admin server stopped");
                    Ok(())
                }
            }
            _ = subsys.on_shutdown_requested() => {
                info!("Received subsystem shutdown request, admin server is stopping");
                self.shutdown();
                Ok(())
            }
        }
    }
}

// 健康检查处理程序
async fn health_handler() -> &'static str {
    "OK"
}

// 缓存刷新处理程序
async fn refresh_cache_handler(
    cache: axum::extract::State<Option<Arc<DnsCache>>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match cache.0 {
        Some(cache) => {
            if cache.is_enabled() {
                // 清空缓存
                cache.clear().await;

                info!("DNS cache has been cleared");

                // 返回成功响应
                Ok(Json(json!({
                    "status": "success",
                    "message": "DNS cache has been cleared"
                })))
            } else {
                // 缓存未启用
                Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "status": "error",
                        "message": "DNS cache is not enabled"
                    })),
                ))
            }
        }
        None => {
            // 缓存未配置
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "status": "error",
                    "message": "DNS cache is not configured"
                })),
            ))
        }
    }
}
