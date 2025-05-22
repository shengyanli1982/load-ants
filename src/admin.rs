// src/server/health.rs

use crate::error::AppError;
use crate::metrics;
use axum::{routing::get, Router};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemHandle};
use tracing::{error, info};

// 健康检查服务器
pub struct AdminServer {
    // 监听地址
    listen_addr: SocketAddr,
    // 停止信号接收端
    shutdown_rx: Option<oneshot::Receiver<()>>,
    // 停止信号发送端
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl AdminServer {
    // 创建新的健康检查服务器
    pub fn new(listen_addr: SocketAddr) -> Self {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        Self {
            listen_addr,
            shutdown_rx: Some(shutdown_rx),
            shutdown_tx: Some(shutdown_tx),
        }
    }

    // 停止健康检查服务器
    pub fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
            info!("Health check server stop signal sent");
        }
    }

    // 启动健康检查服务器
    pub async fn start(&mut self) -> Result<(), AppError> {
        // 组合健康检查和指标路由
        let app = Router::new()
            .route("/health", get(health_handler))
            .merge(metrics::metrics_routes());

        let listener = TcpListener::bind(self.listen_addr).await?;
        info!(
            "Health check and metrics server listening on {}",
            self.listen_addr
        );

        let shutdown_rx = self
            .shutdown_rx
            .take()
            .expect("Health check server already started");

        let server = axum::serve(listener, app);
        let server_with_graceful_shutdown = server.with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
            info!("Health check server received shutdown signal");
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
                    error!("Health check server error: {}", err);
                    Err(err)
                } else {
                    info!("Health check server stopped");
                    Ok(())
                }
            }
            _ = subsys.on_shutdown_requested() => {
                info!("Received subsystem shutdown request, health check server is stopping");
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
