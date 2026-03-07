use loadants::{
    doh::server::DoHServer, metrics::METRICS, r#const::server_defaults, rule_source_labels,
    rule_type_labels, server::DnsServerConfig, subsystem_names, AdminServer, AppError, Args,
    Config, DnsCache, DnsServer, MatchType, RequestHandler, Router, UpstreamManager,
};
use mimalloc::MiMalloc;
use std::process;
use std::sync::Arc;
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemBuilder, Toplevel};
use tracing::{error, info, warn};

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

    if let Err(e) = config.validate_runtime_requirements() {
        error!("Invalid configuration: {}", e);
        process::exit(1);
    }

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
            dns_server.into_subsystem(),
        ));
        // 启动管理服务器子系统
        let admin_server = components.admin_server;
        s.start(SubsystemBuilder::new(
            subsystem_names::ADMIN_SERVER,
            admin_server.into_subsystem(),
        ));
        // 启动DoH服务器子系统
        if let Some(doh_server) = components.doh_server {
            s.start(SubsystemBuilder::new(
                subsystem_names::DOH_SERVER,
                move |s| async move { doh_server.run(s).await },
            ));
        }
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
    // DoH 服务器
    doh_server: Option<DoHServer>,
    // DNS 服务器
    dns_server: DnsServer,
    // 管理服务器
    admin_server: AdminServer,
}

// 创建应用组件
async fn create_components(config: Config) -> Result<AppComponents, AppError> {
    // 创建 DNS 缓存
    let cache = if let Some(cache_config) = &config.cache {
        let cache_size = if cache_config.enabled {
            cache_config.size
        } else {
            0
        };
        let cache = Arc::new(DnsCache::new(
            cache_size,
            cache_config.ttl.min,
            Some(cache_config.ttl.negative),
        ));
        if cache_config.enabled {
            info!(
                "DNS cache enabled, size: {}, min TTL: {}s, negative TTL: {}s",
                cache_config.size, cache_config.ttl.min, cache_config.ttl.negative
            );
        } else {
            info!("DNS cache disabled");
        }
        cache
    } else {
        // 如果没有提供缓存配置，创建一个默认的禁用缓存
        info!("Cache configuration not provided, cache disabled");
        Arc::new(DnsCache::new(0, 0, Some(0)))
    };

    // 创建管理服务器
    let admin_listen_addr = match &config.admin {
        Some(admin_config) => admin_config.listen.parse()?,
        None => {
            warn!(
                "Admin server configuration not provided, using default address {}",
                server_defaults::DEFAULT_ADMIN_LISTEN
            );
            server_defaults::DEFAULT_ADMIN_LISTEN.parse()?
        }
    };
    let admin_server = AdminServer::new(admin_listen_addr).with_cache(Arc::clone(&cache));

    // 准备HTTP客户端配置
    let http_client_config = config.http.clone().unwrap_or_default();
    // 准备 DNS 客户端配置
    let dns_client_config = config.dns.clone().unwrap_or_default();

    // 创建上游管理器 - 避免不必要的克隆
    let upstream = match UpstreamManager::new_with_bootstrap(
        config.upstreams.clone().unwrap_or_default(),
        http_client_config.clone(),
        dns_client_config.clone(),
        config.bootstrap_dns.clone(),
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

    // 获取静态规则（如果有）
    let static_rules = config.rules.r#static.clone();

    // 加载远程规则并与静态规则合并
    let rules = if !config.rules.remote.is_empty() {
        info!(
            "Loading {} remote rule sources...",
            config.rules.remote.len()
        );
        match loadants::remote_rule::load_and_merge_rules(
            &config.rules.remote,
            &static_rules,
            &http_client_config,
        )
        .await
        {
            Ok(merged_rules) => merged_rules,
            Err(e) => {
                error!(
                    "Failed to load remote rules: {}, falling back to static rules only",
                    e
                );
                static_rules.clone()
            }
        }
    } else {
        // 没有远程规则，直接使用静态规则
        static_rules.clone()
    };

    // 创建路由引擎 - 使用合并后的规则
    let router = match Router::new(rules.clone()) {
        Ok(router) => {
            // 设置路由规则数量指标 - 考虑每个规则中的多个模式
            let mut exact_count_static = 0;
            let mut wildcard_count_static = 0;
            let mut regex_count_static = 0;
            let mut exact_count_remote = 0;
            let mut wildcard_count_remote = 0;
            let mut regex_count_remote = 0;

            // 静态规则数量
            for rule in &static_rules {
                match &rule.match_type {
                    MatchType::Exact => exact_count_static += rule.patterns.len(),
                    MatchType::Wildcard => wildcard_count_static += rule.patterns.len(),
                    MatchType::Regex => regex_count_static += rule.patterns.len(),
                }
            }

            // 远程规则数量
            let static_rules_len = static_rules.len();
            if static_rules_len < rules.len() {
                // 计算远程规则中各类型的数量
                for rule in rules.iter().skip(static_rules_len) {
                    match &rule.match_type {
                        MatchType::Exact => exact_count_remote += rule.patterns.len(),
                        MatchType::Wildcard => wildcard_count_remote += rule.patterns.len(),
                        MatchType::Regex => regex_count_remote += rule.patterns.len(),
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
            let remote_rules_count = rules.len() - static_rules_len;
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
                static_rules_len,
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
    let server_config = DnsServerConfig {
        udp_bind_addr: config.listeners.udp.parse()?,
        tcp_bind_addr: config.listeners.tcp.parse()?,
        tcp_timeout: config.listeners.tcp_idle_timeout,
        // DNS 服务器当前并不依赖 HTTP 监听地址；若未配置，则使用一个占位地址避免 panic。
        http_bind_addr: config
            .listeners
            .doh
            .as_deref()
            .unwrap_or("127.0.0.1:0")
            .parse()?,
        http_timeout: config.listeners.http_idle_timeout,
    };

    // 创建 DNS 服务器
    let dns_server = DnsServer::new(server_config, handler.clone());

    // 启动 DoH 服务器
    let doh_server = if let Some(ref listen_doh) = config.listeners.doh {
        info!(
            "DNS server initialized with UDP: {:?}, TCP: {:?}, HTTP: {:?}",
            config.listeners.udp, config.listeners.tcp, config.listeners.doh
        );
        // 创建 DoH 服务器
        Some(DoHServer::new(
            listen_doh.parse()?,
            config.listeners.http_idle_timeout,
            handler,
        ))
    } else {
        info!(
            "DNS server initialized with UDP: {:?}, TCP: {:?}",
            config.listeners.udp, config.listeners.tcp
        );
        None
    };

    // 返回应用组件
    Ok(AppComponents {
        doh_server,
        dns_server,
        admin_server,
    })
}
