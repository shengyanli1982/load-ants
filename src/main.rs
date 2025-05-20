mod args;
mod balancer;
mod cache;
mod config;
mod r#const;
mod error;
mod handler;
mod health;
mod metrics;
mod router;
mod server;
mod upstream;

use crate::args::Args;
use crate::cache::DnsCache;
use crate::config::Config;
use crate::config::MatchType::{Exact, Regex, Wildcard};
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
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemBuilder, Toplevel};
use tracing::{debug, error, info};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

// 使用 mimalloc 分配器提高内存效率
#[global_allocator]
static GLOBAL: MiMalloc = mimalloc::MiMalloc;

fn init_logging(args: &Args) {
    // 根据命令行参数决定日志级别，优先使用 --debug 参数
    let filter = if args.debug {
        // 启用调试模式，显示更详细的日志
        EnvFilter::new("loadants=debug,tower_http=debug,tokio_graceful_shutdown=debug,tokio=debug,axum=debug,reqwest=debug,reqwest_middleware=debug,reqwest_retry=debug,hickory-server=debug,hickory-proto=debug,moka=debug")
    } else if let Ok(filter) = EnvFilter::try_from_default_env() {
        // 如果没有设置 --debug，但设置了环境变量，则使用环境变量
        filter
    } else {
        // 正常模式，仅显示 info 级别及以上
        EnvFilter::new("loadants=info,tower_http=info,tokio_graceful_shutdown=info,tokio=info,axum=info,reqwest=info,reqwest_middleware=info,reqwest_retry=info,hickory-server=info,hickory-proto=info,moka=info")
    };

    // 创建日志格式化器
    let fmt_layer = fmt::layer()
        .with_target(true)
        .with_level(true)
        .with_ansi(false); // 关闭彩色输出

    // 注册日志订阅器
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .init();

    // 如果启用调试模式，输出调试信息
    if args.debug {
        debug!("Debug logging enabled - verbose output mode active");
    }
}

// 程序入口
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 解析命令行参数
    let args = Args::parse_args();

    // 初始化日志
    init_logging(&args);

    info!("Starting Load Ants DNS UDP/TCP to DoH Proxy");

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
    if args.test_config {
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
    let toplevel = Toplevel::new(|s| async move {
        // 启动DNS服务器子系统
        let dns_server = components.dns_server;
        s.start(SubsystemBuilder::new("dns_server", move |s| async move {
            dns_server.run(s).await
        }));

        // 启动健康检查服务器子系统
        let health_server = components.health_server;
        s.start(SubsystemBuilder::new(
            "health_server",
            move |s| async move { health_server.run(s).await },
        ));
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
        config.cache.min_ttl,
        Some(config.cache.negative_ttl),
    ));
    if config.cache.enabled {
        info!(
            "DNS cache enabled, size: {}, min TTL: {}s, negative TTL: {}s",
            config.cache.max_size, config.cache.min_ttl, config.cache.negative_ttl
        );

        // 设置缓存容量指标
        METRICS.cache_capacity().set(config.cache.max_size as i64);
    } else {
        info!("DNS cache disabled");
    }

    // 创建上游管理器 - 避免不必要的克隆
    let upstream =
        match UpstreamManager::new(config.upstream_groups.clone(), config.http_client).await {
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
                METRICS
                    .route_rules_count()
                    .with_label_values(&[rule_type])
                    .inc();
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

    // 创建DNS服务器配置
    let server_config = server::DnsServerConfig {
        udp_bind_addr: config.server.listen_udp.parse()?,
        tcp_bind_addr: config.server.listen_tcp.parse()?,
        tcp_timeout: config.server.tcp_timeout,
    };

    // 创建 DNS 服务器
    let dns_server = DnsServer::new(server_config, handler);
    info!(
        "DNS server initialized with UDP: {}, TCP: {}",
        config.server.listen_udp, config.server.listen_tcp
    );

    // 返回应用组件
    Ok(AppComponents {
        dns_server,
        health_server,
    })
}
