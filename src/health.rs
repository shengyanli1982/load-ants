// src/server/health.rs

use crate::error::AppError;
use crate::metrics;
use axum::{
    routing::get,
    Router,
};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio_graceful_shutdown::{SubsystemHandle, IntoSubsystem};
use tracing::{error, info};

/// 健康检查服务器
pub struct HealthServer {
    /// 监听地址
    listen_addr: SocketAddr,
}

impl HealthServer {
    /// 创建新的健康检查服务器
    pub fn new(listen_addr: SocketAddr) -> Self {
        Self { listen_addr }
    }
    
    /// 启动健康检查服务器
    pub async fn start(&self) -> Result<(), AppError> {
        // 组合健康检查和指标路由
        let app = Router::new()
            .route("/health", get(health_handler))
            .merge(metrics::metrics_routes());
            
        let listener = TcpListener::bind(self.listen_addr).await?;
        info!("Health check and metrics server listening on {}", self.listen_addr);
        
        axum::serve(listener, app).await?;
        
        Ok(())
    }
    
    /// 运行服务器（用于优雅关闭集成）
    pub async fn run(self) -> Result<(), AppError> {
        self.start().await
    }
}

#[async_trait::async_trait]
impl IntoSubsystem<AppError> for HealthServer {
    async fn run(self, subsys: SubsystemHandle) -> Result<(), AppError> {
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
                Ok(())
            }
        }
    }
}

/// 健康检查处理程序
async fn health_handler() -> &'static str {
    "OK"
} 
