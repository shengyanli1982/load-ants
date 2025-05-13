mod args;
mod cache;
mod config;
mod error;
mod handler;
mod health;
mod metrics;
mod router;
mod server;
mod upstream;
mod r#const;

use crate::args::Args;
use crate::cache::DnsCache;
use crate::config::Config;
use crate::error::AppError;
use crate::handler::RequestHandler;
use crate::health::HealthServer;
use crate::metrics::METRICS;
use crate::router::Router;
use crate::server::DnsServer;
use crate::upstream::UpstreamManager;
use mimalloc::MiMalloc;
use std::process;
use std::sync::Arc;
use tokio_graceful_shutdown::{Toplevel, SubsystemHandle, SubsystemBuilder};
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};
use crate::config::MatchType::{Exact, Wildcard, Regex};

// 使用 mimalloc 分配器提高内存效率
#[global_allocator]
static GLOBAL: MiMalloc = mimalloc::MiMalloc;

// 程序入口
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化日志
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    info!("Starting Load Ants DNS UDP/TCP to DoH Proxy");

    // 解析命令行参数
    let args = Args::parse_args();

    // 加载配置
    let config = match Config::from_file(&args.config) {
        Ok(config) => {
            info!("Successfully loaded configuration: {:?}", args.config);
            config
        }
        Err(e) => {
            error!("Failed to load configuration file: {}", e);
            process::exit(1);
        }
    };

    // 如果是测试模式，成功验证配置后退出
    if args.test {
        info!("Configuration file validation successful");
        return Ok(());
    }

    // 创建应用组件
    let components = match create_components(config).await {
        Ok(components) => components,
        Err(e) => {
            error!("Failed to create application components: {}", e);
            process::exit(1);
        }
    };

    // 创建优雅关闭顶层管理器
    let toplevel = Toplevel::new(|s: SubsystemHandle<AppError>| async move {
        s.start(SubsystemBuilder::new("DNS Server", |_| async {
            components.dns_server.run().await
        }));
        s.start(SubsystemBuilder::new("Health Server", |_| async {
            components.health_server.run().await
        }));
    });

    // 等待关闭
    info!("All services started, waiting for requests...");
    match toplevel
        .catch_signals()
        .handle_shutdown_requests(tokio::time::Duration::from_secs(30))
        .await
    {
        Ok(_) => {
            info!("Application gracefully shut down");
            Ok(())
        }
        Err(e) => {
            error!("Application shutdown error: {}", e);
            process::exit(1);
        }
    }
}

// 应用组件
struct AppComponents {
    // DNS 服务器
    dns_server: DnsServer,
    // 健康检查服务器
    health_server: HealthServer,
}

// 创建应用组件
async fn create_components(config: Config) -> Result<AppComponents, AppError> {
    // 创建健康检查服务器
    let health_listen_addr = config.health.listen.parse().unwrap();
    let health_server = HealthServer::new(health_listen_addr);
    
    // 创建 DNS 缓存
    let cache = Arc::new(DnsCache::new(
        config.cache.max_size,
        config.cache.min_ttl
    ));
    if config.cache.enabled {
        info!("DNS cache enabled, size: {}", config.cache.max_size);
        
        // 设置缓存容量指标
        METRICS.cache_capacity().set(config.cache.max_size as i64);
    } else {
        info!("DNS cache disabled");
    }

    // 创建上游管理器 - 避免不必要的克隆
    let upstream = match UpstreamManager::new(
        config.upstream_groups.clone(),
        config.http_client,
    ).await {
        Ok(manager) => {
            info!("Upstream manager initialized successfully");
            Arc::new(manager)
        }
        Err(e) => {
            error!("Failed to initialize upstream manager: {}", e);
            return Err(e);
        }
    };

    // 创建路由引擎 - 避免不必要的克隆
    let router = match Router::new(config.routing_rules.clone()) {
        Ok(router) => {
            info!("Routing engine initialized successfully");
            
            // 设置路由规则数量指标
            for rule in &config.routing_rules {
                let rule_type = match rule.match_type {
                    Exact => "exact",
                    Wildcard => "wildcard",
                    Regex => "regex",
                };
                METRICS.route_rules_count().with_label_values(&[rule_type]).inc();
            }
            
            Arc::new(router)
        }
        Err(e) => {
            error!("Failed to initialize routing engine: {}", e);
            return Err(AppError::Config(e));
        }
    };

    // 创建请求处理器
    let handler = Arc::new(RequestHandler::new(cache, router, upstream));

    // 创建 DNS 服务器
    let bind_addr = config.server.listen_udp.parse().unwrap();
    let dns_server = DnsServer::new(bind_addr, handler);

    // 返回应用组件
    Ok(AppComponents {
        dns_server,
        health_server,
    })
}
