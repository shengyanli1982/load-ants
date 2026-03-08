use loadants::{
    doh::server::DoHServer, metrics::METRICS, r#const::server_defaults, rule_source_labels,
    rule_type_labels, server::DnsServerConfig, subsystem_names, AdminServer, AppError, Args,
    Config, DnsCache, DnsConfig, DnsServer, HttpConfig, MatchType, RequestHandler, RouteRuleConfig,
    Router, UpstreamManager,
};
use mimalloc::MiMalloc;
use std::process;
use std::sync::Arc;
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemBuilder, SubsystemHandle, Toplevel};
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
    let AppComponents {
        doh_server,
        dns_server,
        admin_server,
    } = components;

    let toplevel = Toplevel::new(async move |s: &mut SubsystemHandle| {
        // 启动DNS服务器子系统
        s.start(SubsystemBuilder::new(
            subsystem_names::DNS_SERVER,
            dns_server.into_subsystem(),
        ));

        // 启动管理服务器子系统
        s.start(SubsystemBuilder::new(
            subsystem_names::ADMIN_SERVER,
            admin_server.into_subsystem(),
        ));

        // 启动DoH服务器子系统
        if let Some(doh_server) = doh_server {
            s.start(SubsystemBuilder::new(
                subsystem_names::DOH_SERVER,
                async move |s: &mut SubsystemHandle| doh_server.run(s).await,
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

fn build_dns_cache(config: &Config) -> Arc<DnsCache> {
    if let Some(cache_config) = &config.cache {
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
        info!("Cache configuration not provided, cache disabled");
        Arc::new(DnsCache::new(0, 0, Some(0)))
    }
}

fn build_admin_server(config: &Config, cache: Arc<DnsCache>) -> Result<AdminServer, AppError> {
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

    Ok(AdminServer::new(admin_listen_addr).with_cache(cache))
}

fn prepare_http_client_config(config: &Config) -> HttpConfig {
    config.http.clone().unwrap_or_default()
}

fn prepare_dns_client_config(config: &Config) -> DnsConfig {
    config.dns.clone().unwrap_or_default()
}

async fn build_upstream_manager(
    config: &Config,
    http_client_config: HttpConfig,
    dns_client_config: DnsConfig,
) -> Result<Arc<UpstreamManager>, AppError> {
    match UpstreamManager::new_with_bootstrap(
        config.upstreams.clone().unwrap_or_default(),
        http_client_config,
        dns_client_config,
        config.bootstrap_dns.clone(),
    )
    .await
    {
        Ok(manager) => {
            info!("Upstream manager initialized successfully");
            Ok(Arc::new(manager))
        }
        Err(e) => {
            error!("Failed to initialize upstream manager: {}", e);
            Err(e)
        }
    }
}

async fn load_merged_rules(
    config: &Config,
    static_rules: &[RouteRuleConfig],
    http_client_config: &HttpConfig,
) -> Vec<RouteRuleConfig> {
    if config.rules.remote.is_empty() {
        return static_rules.to_vec();
    }

    info!(
        "Loading {} remote rule sources...",
        config.rules.remote.len()
    );
    match loadants::remote_rule::load_and_merge_rules(
        &config.rules.remote,
        static_rules,
        http_client_config,
    )
    .await
    {
        Ok(merged_rules) => merged_rules,
        Err(e) => {
            error!(
                "Failed to load remote rules: {}, falling back to static rules only",
                e
            );
            static_rules.to_vec()
        }
    }
}

fn record_route_rules_count_metrics(static_rules: &[RouteRuleConfig], rules: &[RouteRuleConfig]) {
    let mut exact_count_static = 0;
    let mut wildcard_count_static = 0;
    let mut regex_count_static = 0;
    let mut exact_count_remote = 0;
    let mut wildcard_count_remote = 0;
    let mut regex_count_remote = 0;

    for rule in static_rules {
        match &rule.match_type {
            MatchType::Exact => exact_count_static += rule.patterns.len(),
            MatchType::Wildcard => wildcard_count_static += rule.patterns.len(),
            MatchType::Regex => regex_count_static += rule.patterns.len(),
        }
    }

    let static_rules_len = static_rules.len();
    if static_rules_len < rules.len() {
        for rule in rules.iter().skip(static_rules_len) {
            match &rule.match_type {
                MatchType::Exact => exact_count_remote += rule.patterns.len(),
                MatchType::Wildcard => wildcard_count_remote += rule.patterns.len(),
                MatchType::Regex => regex_count_remote += rule.patterns.len(),
            }
        }
    }

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

    let remote_rules_count = rules.len().saturating_sub(static_rules_len);
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
}

fn build_router(
    static_rules: &[RouteRuleConfig],
    rules: Vec<RouteRuleConfig>,
) -> Result<Arc<Router>, AppError> {
    match Router::new(rules.clone()) {
        Ok(router) => {
            record_route_rules_count_metrics(static_rules, &rules);
            Ok(Arc::new(router))
        }
        Err(e) => {
            error!("Failed to initialize routing engine: {}", e);
            Err(AppError::Config(e))
        }
    }
}

fn build_dns_server_config(config: &Config) -> Result<DnsServerConfig, AppError> {
    Ok(DnsServerConfig {
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
    })
}

fn build_dns_server(config: &Config, handler: Arc<RequestHandler>) -> Result<DnsServer, AppError> {
    let server_config = build_dns_server_config(config)?;
    Ok(DnsServer::new(server_config, handler))
}

fn build_doh_server(
    config: &Config,
    handler: Arc<RequestHandler>,
) -> Result<Option<DoHServer>, AppError> {
    if let Some(ref listen_doh) = config.listeners.doh {
        info!(
            "DNS server initialized with UDP: {:?}, TCP: {:?}, HTTP: {:?}",
            config.listeners.udp, config.listeners.tcp, listen_doh
        );
        Ok(Some(DoHServer::new(
            listen_doh.parse()?,
            config.listeners.http_idle_timeout,
            handler,
        )))
    } else {
        info!(
            "DNS server initialized with UDP: {:?}, TCP: {:?}",
            config.listeners.udp, config.listeners.tcp
        );
        Ok(None)
    }
}

// 创建应用组件
async fn create_components(config: Config) -> Result<AppComponents, AppError> {
    let cache = build_dns_cache(&config);
    let admin_server = build_admin_server(&config, Arc::clone(&cache))?;

    let http_client_config = prepare_http_client_config(&config);
    let dns_client_config = prepare_dns_client_config(&config);
    let upstream =
        build_upstream_manager(&config, http_client_config.clone(), dns_client_config).await?;

    let static_rules = config.rules.r#static.clone();
    let rules = load_merged_rules(&config, &static_rules, &http_client_config).await;
    let router = build_router(&static_rules, rules)?;

    let handler = Arc::new(RequestHandler::new(cache, router, upstream));
    let dns_server = build_dns_server(&config, handler.clone())?;
    let doh_server = build_doh_server(&config, handler)?;

    // 返回应用组件
    Ok(AppComponents {
        doh_server,
        dns_server,
        admin_server,
    })
}
