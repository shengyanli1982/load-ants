use loadants::{
    build_router,
    doh::server::DoHServer,
    r#const::server_defaults,
    rate_limit::RateLimiter,
    remote_rule::{reload_once, run_panic_guarded},
    server::DnsServerConfig,
    subsystem_names, AdminServer, AppError, Args, Config, ConfigSummary, DnsCache, DnsServer,
    RemoteSourceStatus, RequestHandler, Router, UpstreamManager,
};
use mimalloc::MiMalloc;
use std::process;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::interval;
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemBuilder, Toplevel};
use tracing::{debug, error, info};

#[global_allocator]
static GLOBAL: MiMalloc = mimalloc::MiMalloc;

fn init_logging(args: &Args) {
    let default_level = if args.debug {
        "loadants=debug"
    } else {
        "loadants=info"
    };

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_level));

    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_line_number(false)
        .with_env_filter(env_filter)
        .init()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse_args();

    init_logging(&args);

    if let Err(e) = args.validation() {
        error!(
            error = &e as &dyn std::error::Error,
            "Failed to validate command line arguments"
        );
        process::exit(1);
    }

    if args.dump_schema {
        let schema = schemars::schema_for!(Config);
        let json_schema =
            serde_json::to_string_pretty(&schema).expect("Failed to serialize JSON schema");
        println!("{}", json_schema);
        return Ok(());
    }

    info!(version = env!("CARGO_PKG_VERSION"), "Starting loadants");

    let config = match Config::from_file(&args.config) {
        Ok(config) => {
            info!(path = %args.config.display(), "Configuration loaded");
            let cache_enabled = config.cache.as_ref().is_some_and(|c| c.enabled);
            let cache_max_entries = if cache_enabled {
                config.cache.as_ref().map_or(0, |c| c.max_size)
            } else {
                0
            };
            info!(
                listen_udp = %config.server.listen_udp,
                listen_tcp = %config.server.listen_tcp,
                listen_http = config.server.listen_http.as_deref().unwrap_or("none"),
                doh = if config.server.listen_http.is_some() { "on" } else { "off" },
                cache = if cache_enabled { "on" } else { "off" },
                cache_max_entries = cache_max_entries,
                upstream_groups = config.upstream_groups.as_ref().map_or(0, |g| g.len()),
                remote_rule_sources = config.remote_rules.sources.len(),
                rate_limit = if config.server.rate_limit.is_some() { "on" } else { "off" },
                "Configuration summary"
            );
            config
        }
        Err(e) => {
            error!(
                error = &e as &dyn std::error::Error,
                "Failed to load configuration file"
            );
            process::exit(1);
        }
    };

    if let Err(e) = config.validate_runtime_requirements() {
        error!(
            error = &e as &dyn std::error::Error,
            "Failed to validate configuration"
        );
        process::exit(1);
    }

    if args.test_config {
        info!("Configuration validation successful");
        return Ok(());
    }

    let components = match create_components(config).await {
        Ok(components) => components,
        Err(e) => {
            error!(
                error = &e as &dyn std::error::Error,
                "Failed to create application components"
            );
            process::exit(1);
        }
    };

    let shutdown_started_at: Arc<std::sync::Mutex<Option<Instant>>> =
        Arc::new(std::sync::Mutex::new(None));
    let shutdown_timer_state = Arc::clone(&shutdown_started_at);

    let toplevel = Toplevel::new(|s| async move {
        let dns_server = components.dns_server;
        s.start(SubsystemBuilder::new(
            subsystem_names::DNS_SERVER,
            dns_server.into_subsystem(),
        ));
        let admin_server = components.admin_server;
        s.start(SubsystemBuilder::new(
            subsystem_names::ADMIN_SERVER,
            admin_server.into_subsystem(),
        ));
        if let Some(doh_server) = components.doh_server {
            s.start(SubsystemBuilder::new(
                subsystem_names::DOH_SERVER,
                move |s| async move { doh_server.run(s).await },
            ));
        }
        s.start(SubsystemBuilder::new(
            subsystem_names::SHUTDOWN_TIMER,
            move |s| async move {
                s.on_shutdown_requested().await;
                *shutdown_timer_state.lock().unwrap() = Some(Instant::now());
                Ok::<(), AppError>(())
            },
        ));
    });

    info!("Service tasks dispatched");
    match toplevel
        .catch_signals()
        .handle_shutdown_requests(Duration::from_secs(args.shutdown_timeout))
        .await
    {
        Ok(_) => {
            let elapsed_ms = shutdown_elapsed_ms(&shutdown_started_at);
            info!(
                elapsed_ms = elapsed_ms,
                status = "graceful",
                "Shutdown complete"
            );
            Ok(())
        }
        Err(e) => {
            let elapsed_ms = shutdown_elapsed_ms(&shutdown_started_at);
            error!(
                elapsed_ms = elapsed_ms,
                status = "error",
                error = &e as &dyn std::error::Error,
                "Shutdown failed"
            );
            process::exit(1);
        }
    }
}

fn shutdown_elapsed_ms(started_at: &std::sync::Mutex<Option<Instant>>) -> u64 {
    started_at
        .lock()
        .unwrap()
        .map(|t| t.elapsed().as_millis() as u64)
        .unwrap_or(0)
}

struct AppComponents {
    doh_server: Option<DoHServer>,
    dns_server: DnsServer,
    admin_server: AdminServer,
}

async fn create_components(config: Config) -> Result<AppComponents, AppError> {
    let cache = if let Some(cache_config) = &config.cache {
        let cache_size = if cache_config.enabled {
            cache_config.max_size
        } else {
            0
        };
        let stale_while_revalidate = if cache_config.stale_while_revalidate > 0 {
            Some(cache_config.stale_while_revalidate)
        } else {
            None
        };
        Arc::new(DnsCache::new(
            cache_size,
            cache_config.min_ttl,
            cache_config.max_ttl,
            Some(cache_config.negative_ttl),
            stale_while_revalidate,
        ))
    } else {
        Arc::new(DnsCache::new(0, 0, 0, Some(0), None))
    };

    let http_client_config = config.http_client.clone().unwrap_or_default();
    let dns_client_config = config.dns_client.clone().unwrap_or_default();

    let upstream = match UpstreamManager::new(
        config.upstream_groups.clone().unwrap_or_default(),
        http_client_config.clone(),
        dns_client_config.clone(),
    )
    .await
    {
        Ok(manager) => Arc::new(manager),
        Err(e) => {
            error!(
                error = &e as &dyn std::error::Error,
                "Failed to initialize upstream manager"
            );
            return Err(e);
        }
    };

    let router = match build_router(&config).await {
        Ok(router) => router,
        Err(e) => {
            error!(
                error = &e as &dyn std::error::Error,
                "Failed to initialize routing engine"
            );
            return Err(e);
        }
    };

    let admin_listen_addr = match &config.admin {
        Some(admin_config) => admin_config.listen.parse()?,
        None => {
            info!(
                addr = server_defaults::DEFAULT_ADMIN_LISTEN,
                "Admin listen address defaulted"
            );
            server_defaults::DEFAULT_ADMIN_LISTEN.parse()?
        }
    };
    let admin_auth = config.admin.as_ref().and_then(|a| a.auth.clone());
    let config_summary = ConfigSummary {
        listen_udp: config.server.listen_udp.clone(),
        listen_tcp: config.server.listen_tcp.clone(),
        listen_http: config.server.listen_http.clone(),
        cache_enabled: config.cache.as_ref().is_some_and(|c| c.enabled),
        cache_max_size: config.cache.as_ref().map_or(0, |c| c.max_size),
        upstream_group_count: config.upstream_groups.as_ref().map_or(0, |g| g.len()),
        remote_source_count: config.remote_rules.sources.len(),
    };
    let remote_source_statuses: Arc<RwLock<Vec<RemoteSourceStatus>>> = Arc::new(RwLock::new(
        config
            .remote_rules
            .sources
            .iter()
            .map(|src| RemoteSourceStatus {
                url: src.url.clone(),
                status: "pending".to_string(),
                last_updated: 0,
                rule_count: 0,
                error_message: None,
            })
            .collect(),
    ));

    let admin_server = AdminServer::new(admin_listen_addr)
        .with_cache(Arc::clone(&cache))
        .with_auth(admin_auth)
        .with_upstream(Arc::clone(&upstream))
        .with_version(env!("CARGO_PKG_VERSION"))
        .with_router(Arc::clone(&router))
        .with_config_summary(config_summary)
        .with_remote_source_statuses(Arc::clone(&remote_source_statuses));

    if !config.remote_rules.sources.is_empty() {
        let reload_router = Arc::clone(&router);
        let reload_statuses = Arc::clone(&remote_source_statuses);
        let remote_rules_config = config.remote_rules.clone();
        let static_rules = config.static_rules.clone().unwrap_or_default();
        let http_client_config = config.http_client.clone().unwrap_or_default();

        tokio::spawn(async move {
            run_reload_task(
                remote_rules_config,
                static_rules,
                http_client_config,
                reload_router,
                reload_statuses,
            )
            .await;
        });
    }

    let handler = Arc::new(RequestHandler::new(cache, router, upstream));

    let rate_limiter = config.server.rate_limit.as_ref().map(|rl| {
        info!(
            max_rps = rl.max_requests_per_second,
            per_ip_rps = rl.per_ip_max_requests_per_second,
            "Rate limiter ready"
        );
        RateLimiter::new_with_per_ip(
            rl.max_requests_per_second,
            rl.per_ip_max_requests_per_second,
        )
    });

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
        rate_limiter: rate_limiter.clone(),
        udp_recv_buffer: config.server.udp_socket.recv_buffer,
        udp_send_buffer: config.server.udp_socket.send_buffer,
        udp_socket_count: config.server.udp_socket.socket_count,
    };

    let dns_server = DnsServer::new(server_config, handler.clone());

    let doh_server = if let Some(ref listen_http) = config.server.listen_http {
        let tls_cert = config.server.tls_cert.clone();
        let tls_key = config.server.tls_key.clone();
        Some(DoHServer::new(
            listen_http.parse()?,
            handler.clone(),
            rate_limiter,
            tls_cert,
            tls_key,
        ))
    } else {
        None
    };

    Ok(AppComponents {
        doh_server,
        dns_server,
        admin_server,
    })
}

async fn run_reload_task(
    remote_rules_config: loadants::RemoteRulesConfig,
    static_rules: Vec<loadants::RouteRuleConfig>,
    http_client_config: loadants::HttpClientConfig,
    router: Arc<RwLock<Arc<Router>>>,
    statuses: Arc<RwLock<Vec<RemoteSourceStatus>>>,
) {
    let mut ticker = interval(Duration::from_secs(
        remote_rules_config.reload_interval_secs,
    ));
    ticker.tick().await;

    loop {
        ticker.tick().await;
        debug!("Remote rules reload started");

        let iteration_config = remote_rules_config.clone();
        let iteration_rules = static_rules.clone();
        let iteration_client = http_client_config.clone();
        let iteration_router = Arc::clone(&router);
        let iteration_statuses = Arc::clone(&statuses);

        run_panic_guarded(
            "Remote rules reload",
            reload_once(
                iteration_config,
                iteration_rules,
                iteration_client,
                iteration_router,
                iteration_statuses,
            ),
        )
        .await;
    }
}
