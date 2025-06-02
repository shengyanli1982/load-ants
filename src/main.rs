mod admin;
mod args;
mod balancer;
mod cache;
mod config;
mod r#const;
mod error;
mod handler;
mod metrics;
mod remote_rule;
mod router;
mod server;
mod upstream;

use crate::admin::AdminServer;
use crate::args::Args;
use crate::cache::DnsCache;
use crate::config::Config;
use crate::config::MatchType::{Exact, Regex, Wildcard};
use crate::error::AppError;
use crate::handler::RequestHandler;
use crate::metrics::METRICS;
use crate::r#const::{rule_source_labels, rule_type_labels, subsystem_names};
use crate::router::Router;
use crate::server::DnsServer;
use crate::upstream::UpstreamManager;
use mimalloc::MiMalloc;
use std::process;
use std::sync::Arc;
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemBuilder, Toplevel};
use tracing::{error, info};

// 使用 mimalloc 分配器提高内存效率
#[global_allocator]
static GLOBAL: MiMalloc = mimalloc::MiMalloc;

fn init_logging(args: &Args) {
    let builder = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_line_number(false);

    // 如果启用调试模式，输出调试信息，否则只输出 info 及以上级别
    if args.debug {
        builder.with_max_level(tracing::Level::DEBUG)
    } else {
        builder.with_max_level(tracing::Level::INFO)
    }
    .init();
}

// 程序入口
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 解析命令行参数
    let args = Args::parse_args();

    // 初始化日志
    init_logging(&args);

    // 验证参数
    if let Err(e) = args.validation() {
        error!("Invalid command line arguments: {}", e);
        process::exit(1);
    }

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
        s.start(SubsystemBuilder::new(
            subsystem_names::DNS_SERVER,
            move |s| async move { dns_server.run(s).await },
        ));

        // 启动管理服务器子系统
        let admin_server = components.admin_server;
        s.start(SubsystemBuilder::new(
            subsystem_names::ADMIN_SERVER,
            move |s| async move { admin_server.run(s).await },
        ));
    });

    // 等待关闭
    info!("All services started, waiting for requests...");
    match toplevel
        .catch_signals()
        .handle_shutdown_requests(tokio::time::Duration::from_secs(args.shutdown_timeout))
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
    // 管理服务器
    admin_server: AdminServer,
}

// 创建应用组件
async fn create_components(config: Config) -> Result<AppComponents, AppError> {
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

    // 创建管理服务器
    let admin_listen_addr = config.admin.listen.parse().unwrap();
    let admin_server = AdminServer::new(admin_listen_addr).with_cache(Arc::clone(&cache));

    // 创建上游管理器 - 避免不必要的克隆
    let upstream = match UpstreamManager::new(
        config.upstream_groups.clone(),
        config.http_client.clone(),
    )
    .await
    {
        Ok(manager) => {
            info!("Upstream manager initialized successfully");
            Arc::new(manager)
        }
        Err(e) => {
            error!("Failed to initialize upstream manager: {}", e);
            return Err(e);
        }
    };

    // 加载远程规则并与静态规则合并
    let rules = if !config.remote_rules.is_empty() {
        info!(
            "Loading {} remote rule sources...",
            config.remote_rules.len()
        );
        match crate::remote_rule::load_and_merge_rules(
            &config.remote_rules,
            &config.static_rules,
            &config.http_client,
        )
        .await
        {
            Ok(merged_rules) => merged_rules,
            Err(e) => {
                error!(
                    "Failed to load remote rules: {}, falling back to static rules only",
                    e
                );
                config.static_rules.clone()
            }
        }
    } else {
        // 没有远程规则，直接使用静态规则
        config.static_rules.clone()
    };

    // 创建路由引擎 - 使用合并后的规则
    let router = match Router::new(rules.clone()) {
        Ok(router) => {
            info!("Routing engine initialized successfully");

            // 设置路由规则数量指标 - 考虑每个规则中的多个模式
            let mut exact_count_static = 0;
            let mut wildcard_count_static = 0;
            let mut regex_count_static = 0;
            let mut exact_count_remote = 0;
            let mut wildcard_count_remote = 0;
            let mut regex_count_remote = 0;

            // 静态规则数量
            for rule in &config.static_rules {
                match rule.match_type {
                    Exact => exact_count_static += rule.patterns.len(),
                    Wildcard => wildcard_count_static += rule.patterns.len(),
                    Regex => regex_count_static += rule.patterns.len(),
                }
            }

            // 远程规则数量 (总规则数 - 静态规则数)
            let remote_rules_count = rules.len() - config.static_rules.len();
            if remote_rules_count > 0 {
                // 计算远程规则中各类型的数量
                for rule in rules.iter().skip(config.static_rules.len()) {
                    match rule.match_type {
                        Exact => exact_count_remote += rule.patterns.len(),
                        Wildcard => wildcard_count_remote += rule.patterns.len(),
                        Regex => regex_count_remote += rule.patterns.len(),
                    }
                }
            }

            // 设置静态规则指标
            METRICS
                .route_rules_count()
                .with_label_values(&[rule_type_labels::EXACT, rule_source_labels::STATIC])
                .set(exact_count_static as i64);

            METRICS
                .route_rules_count()
                .with_label_values(&[rule_type_labels::WILDCARD, rule_source_labels::STATIC])
                .set(wildcard_count_static as i64);

            METRICS
                .route_rules_count()
                .with_label_values(&[rule_type_labels::REGEX, rule_source_labels::STATIC])
                .set(regex_count_static as i64);

            // 设置远程规则指标
            if remote_rules_count > 0 {
                METRICS
                    .route_rules_count()
                    .with_label_values(&[rule_type_labels::EXACT, rule_source_labels::REMOTE])
                    .set(exact_count_remote as i64);

                METRICS
                    .route_rules_count()
                    .with_label_values(&[rule_type_labels::WILDCARD, rule_source_labels::REMOTE])
                    .set(wildcard_count_remote as i64);

                METRICS
                    .route_rules_count()
                    .with_label_values(&[rule_type_labels::REGEX, rule_source_labels::REMOTE])
                    .set(regex_count_remote as i64);
            }

            info!(
                "Routing engine initialized successfully with {} rules ({} static, {} remote): {} exact, {} wildcard, {} regex",
                rules.len(),
                config.static_rules.len(),
                remote_rules_count,
                exact_count_static + exact_count_remote,
                wildcard_count_static + wildcard_count_remote,
                regex_count_static + regex_count_remote
            );

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
        admin_server,
    })
}
