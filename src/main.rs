use loadants::{
    build_router, doh::server::DoHServer, r#const::server_defaults, server::DnsServerConfig,
    subsystem_names, AdminServer, AppError, Args, Config, DnsCache, DnsServer, RequestHandler,
    UpstreamManager,
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
    let cache = if let Some(cache_config) = &config.cache {
        let cache_size = if cache_config.enabled {
            cache_config.max_size
        } else {
            0
        };
        let cache = Arc::new(DnsCache::new(
            cache_size,
            cache_config.min_ttl,
            Some(cache_config.negative_ttl),
        ));
        if cache_config.enabled {
            info!(
                "DNS cache enabled, size: {}, min TTL: {}s, negative TTL: {}s",
                cache_config.max_size, cache_config.min_ttl, cache_config.negative_ttl
            );
        } else {
            info!("DNS cache disabled");
        }
        cache
    } else {
        info!("Cache configuration not provided, cache disabled");
        Arc::new(DnsCache::new(0, 0, Some(0)))
    };

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

    let http_client_config = config.http_client.clone().unwrap_or_default();
    let dns_client_config = config.dns_client.clone().unwrap_or_default();

    let upstream = match UpstreamManager::new(
        config.upstream_groups.clone().unwrap_or_default(),
        http_client_config.clone(),
        dns_client_config.clone(),
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

    let router = match build_router(&config).await {
        Ok(router) => router,
        Err(e) => {
            error!("Failed to initialize routing engine: {}", e);
            return Err(e);
        }
    };

    let handler = Arc::new(RequestHandler::new(cache, router, upstream));

    let server_config = DnsServerConfig {
        udp_bind_addr: config.server.listen_udp.parse()?,
        tcp_bind_addr: config.server.listen_tcp.parse()?,
        tcp_timeout: config.server.tcp_timeout,
        http_bind_addr: config
            .server
            .listen_http
            .as_deref()
            .unwrap_or("127.0.0.1:0")
            .parse()?,
        http_timeout: config.server.http_timeout,
    };

    let dns_server = DnsServer::new(server_config, handler.clone());

    let doh_server = if let Some(ref listen_http) = config.server.listen_http {
        info!(
            "DNS server initialized with UDP: {:?}, TCP: {:?}, HTTP: {:?}",
            config.server.listen_udp, config.server.listen_tcp, config.server.listen_http
        );
        Some(DoHServer::new(
            listen_http.parse()?,
            config.server.http_timeout,
            handler,
        ))
    } else {
        info!(
            "DNS server initialized with UDP: {:?}, TCP: {:?}",
            config.server.listen_udp, config.server.listen_tcp
        );
        None
    };

    Ok(AppComponents {
        doh_server,
        dns_server,
        admin_server,
    })
}
