use crate::cache::{CacheDumpResponse, CacheRestoreRequest, CacheRestoreStats, DnsCache};
use crate::config::AdminAuthConfig;
use crate::error::AppError;
use crate::metrics;
use crate::router::Router as RoutingEngine;
use crate::upstream::UpstreamManager;
use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::{from_fn_with_state, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use subtle::ConstantTimeEq;
use tokio::net::TcpListener;
use tokio::sync::{watch, RwLock};
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemHandle};
use tracing::{error, info};

#[derive(Clone)]
pub struct ConfigSummary {
    pub listen_udp: String,
    pub listen_tcp: String,
    pub listen_http: Option<String>,
    pub cache_enabled: bool,
    pub cache_max_size: usize,
    pub upstream_group_count: usize,
    pub remote_source_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteSourceStatus {
    pub url: String,
    pub status: String,
    pub last_updated: u64,
    pub rule_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

pub struct AdminState {
    pub cache: Option<Arc<DnsCache>>,
    pub upstream: Option<Arc<UpstreamManager>>,
    pub version: &'static str,
    pub startup_time: Instant,
    pub router: Option<Arc<RwLock<Arc<RoutingEngine>>>>,
    pub config_summary: Option<ConfigSummary>,
    pub remote_source_statuses: Option<Arc<RwLock<Vec<RemoteSourceStatus>>>>,
}

pub struct AdminServer {
    listen_addr: SocketAddr,
    shutdown_requested: watch::Sender<bool>,
    cache: Option<Arc<DnsCache>>,
    auth: Option<AdminAuthConfig>,
    upstream: Option<Arc<UpstreamManager>>,
    version: &'static str,
    router: Option<Arc<RwLock<Arc<RoutingEngine>>>>,
    config_summary: Option<ConfigSummary>,
    remote_source_statuses: Option<Arc<RwLock<Vec<RemoteSourceStatus>>>>,
}

impl AdminServer {
    pub fn new(listen_addr: SocketAddr) -> Self {
        Self {
            listen_addr,
            shutdown_requested: watch::channel(false).0,
            cache: None,
            auth: None,
            upstream: None,
            version: "unknown",
            router: None,
            config_summary: None,
            remote_source_statuses: None,
        }
    }

    pub fn with_cache(mut self, cache: Arc<DnsCache>) -> Self {
        self.cache = Some(cache);
        self
    }

    pub fn with_auth(mut self, auth: Option<AdminAuthConfig>) -> Self {
        self.auth = auth;
        self
    }

    pub fn with_upstream(mut self, upstream: Arc<UpstreamManager>) -> Self {
        self.upstream = Some(upstream);
        self
    }

    pub fn with_version(mut self, version: &'static str) -> Self {
        self.version = version;
        self
    }

    pub fn with_router(mut self, router: Arc<RwLock<Arc<RoutingEngine>>>) -> Self {
        self.router = Some(router);
        self
    }

    pub fn with_config_summary(mut self, config_summary: ConfigSummary) -> Self {
        self.config_summary = Some(config_summary);
        self
    }

    pub fn with_remote_source_statuses(
        mut self,
        statuses: Arc<RwLock<Vec<RemoteSourceStatus>>>,
    ) -> Self {
        self.remote_source_statuses = Some(statuses);
        self
    }

    pub fn shutdown(&self) {
        self.shutdown_requested.send_replace(true);
        info!("Admin server stop signal sent");
    }

    pub async fn start(&self) -> Result<(), AppError> {
        let state = Arc::new(AdminState {
            cache: self.cache.clone(),
            upstream: self.upstream.clone(),
            version: self.version,
            startup_time: Instant::now(),
            router: self.router.clone(),
            config_summary: self.config_summary.clone(),
            remote_source_statuses: self.remote_source_statuses.clone(),
        });

        let auth_state = self.auth.clone();

        let health_routes = Router::new()
            .route("/health/live", get(health_live_handler))
            .route("/health/ready", get(health_ready_handler))
            .with_state(state.clone());

        let protected_routes = Router::new()
            .route("/api/cache/clear", post(refresh_cache_handler))
            .route("/api/cache/dump", get(cache_dump_handler))
            .route("/api/cache/restore", post(cache_restore_handler))
            .route("/api/upstreams", get(upstreams_handler))
            .route("/api/routes", get(routes_handler))
            .route("/api/info", get(info_handler))
            .with_state(state)
            .merge(metrics::metrics_routes())
            .route_layer(from_fn_with_state(auth_state, auth_middleware));

        let app = Router::new().merge(health_routes).merge(protected_routes);

        let listener = TcpListener::bind(self.listen_addr).await?;
        info!("Admin server listening on {}", self.listen_addr);

        let mut shutdown_rx = self.shutdown_requested.subscribe();

        let server = axum::serve(listener, app);
        let server_with_graceful_shutdown = server.with_graceful_shutdown(async move {
            let _ = shutdown_rx.wait_for(|requested| *requested).await;
            info!("Admin server received shutdown signal");
        });

        server_with_graceful_shutdown.await?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl IntoSubsystem<AppError> for AdminServer {
    async fn run(mut self, subsys: SubsystemHandle) -> Result<(), AppError> {
        let result = tokio::try_join! {
            async {
                let result = self.start().await;
                subsys.request_local_shutdown();
                result
            },
            async {
                subsys.on_shutdown_requested().await;
                self.shutdown();
                Ok(())
            }
        };

        if let Err(err) = result {
            error!("Admin server error: {}", err);
            Err(err)
        } else {
            info!("Admin server stopped");
            Ok(())
        }
    }
}

async fn health_live_handler(State(state): State<Arc<AdminState>>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "version": state.version,
    }))
}

async fn health_ready_handler(State(state): State<Arc<AdminState>>) -> (StatusCode, Json<Value>) {
    let cache_info = match &state.cache {
        Some(cache) => json!({
            "enabled": cache.is_enabled(),
            "entries": cache.approximate_len(),
        }),
        None => json!({"enabled": false}),
    };

    let summary = match &state.upstream {
        Some(upstream) => upstream.health_summary(),
        None => std::collections::HashMap::new(),
    };

    let upstreams_json = {
        let mut map = serde_json::Map::new();
        for (name, info) in &summary {
            map.insert(
                name.clone(),
                json!({
                    "servers": info.servers,
                    "healthy": info.healthy,
                    "unhealthy": info.unhealthy,
                    "halfOpen": info.half_open,
                    "status": info.status,
                }),
            );
        }
        map
    };

    let (status_code, overall_status) = if summary.is_empty() {
        (StatusCode::OK, "ok")
    } else if summary.values().any(|g| g.servers > 0 && g.healthy == 0) {
        (StatusCode::SERVICE_UNAVAILABLE, "unhealthy")
    } else if summary.values().any(|g| g.status != "ok") {
        (StatusCode::OK, "degraded")
    } else {
        (StatusCode::OK, "ok")
    };

    let mut body = serde_json::Map::new();
    body.insert("status".to_string(), json!(overall_status));
    body.insert("version".to_string(), json!(state.version));
    body.insert("cacheInfo".to_string(), cache_info);
    if !summary.is_empty() {
        body.insert("upstreams".to_string(), Value::Object(upstreams_json));
    }

    (status_code, Json(Value::Object(body)))
}

async fn auth_middleware(
    State(auth_state): State<Option<AdminAuthConfig>>,
    request: Request,
    next: Next,
) -> Response {
    if let Some(auth_config) = auth_state {
        let auth_header = request
            .headers()
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok());

        let is_authenticated = auth_header.is_some_and(|header| {
            if let Some(token) = header.strip_prefix("Bearer ") {
                constant_time_eq(token, &auth_config.token)
            } else {
                false
            }
        });

        if !is_authenticated {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "status": "error",
                    "message": "Unauthorized"
                })),
            )
                .into_response();
        }
    }

    next.run(request).await
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let max_len = a_bytes.len().max(b_bytes.len());

    let mut result = a_bytes.len().ct_eq(&b_bytes.len());

    for i in 0..max_len {
        let x = if i < a_bytes.len() { a_bytes[i] } else { 0u8 };
        let y = if i < b_bytes.len() { b_bytes[i] } else { 0u8 };
        result &= x.ct_eq(&y);
    }

    result.into()
}

async fn refresh_cache_handler(
    State(state): State<Arc<AdminState>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match &state.cache {
        Some(cache) => {
            if cache.is_enabled() {
                cache.clear().await;

                info!("DNS cache has been cleared");

                Ok(Json(json!({
                    "status": "success",
                    "message": "DNS cache has been cleared"
                })))
            } else {
                Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "status": "error",
                        "message": "DNS cache is not enabled"
                    })),
                ))
            }
        }
        None => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "status": "error",
                "message": "DNS cache is not configured"
            })),
        )),
    }
}

async fn upstreams_handler(State(state): State<Arc<AdminState>>) -> Json<Value> {
    let groups = match &state.upstream {
        Some(upstream) => upstream.group_summary(),
        None => Vec::new(),
    };

    let groups_json: Vec<Value> = groups
        .into_iter()
        .map(|g| {
            let servers_json: Vec<Value> = g
                .servers
                .into_iter()
                .map(|s| {
                    json!({
                        "url": s.url,
                        "status": s.status,
                        "weight": s.weight,
                        "failureCount": s.failure_count,
                    })
                })
                .collect();
            json!({
                "name": g.name,
                "scheme": g.scheme,
                "strategy": g.strategy,
                "servers": servers_json,
            })
        })
        .collect();

    Json(json!({ "groups": groups_json }))
}

async fn routes_handler(State(state): State<Arc<AdminState>>) -> Json<Value> {
    let total_rule_count = match &state.router {
        Some(router_lock) => {
            let router_guard = router_lock.read().await;
            router_guard.rule_count()
        }
        None => 0,
    };

    let remote_sources = match &state.remote_source_statuses {
        Some(lock) => {
            let guards = lock.read().await;
            guards
                .iter()
                .map(|s| {
                    let mut map = serde_json::Map::new();
                    map.insert("url".to_string(), json!(s.url));
                    map.insert("status".to_string(), json!(s.status));
                    map.insert("lastUpdated".to_string(), json!(s.last_updated));
                    map.insert("ruleCount".to_string(), json!(s.rule_count));
                    if let Some(ref msg) = s.error_message {
                        map.insert("errorMessage".to_string(), json!(msg));
                    }
                    Value::Object(map)
                })
                .collect::<Vec<_>>()
        }
        None => vec![],
    };

    Json(json!({
        "totalRuleCount": total_rule_count,
        "remoteSources": remote_sources,
    }))
}

async fn info_handler(State(state): State<Arc<AdminState>>) -> Json<Value> {
    let uptime_seconds = state.startup_time.elapsed().as_secs();

    let config_summary_json = match &state.config_summary {
        Some(cs) => json!({
            "listenUdp": cs.listen_udp,
            "listenTcp": cs.listen_tcp,
            "listenHttp": cs.listen_http,
            "upstreamGroupCount": cs.upstream_group_count,
            "cacheEnabled": cs.cache_enabled,
            "cacheMaxSize": cs.cache_max_size,
        }),
        None => json!({}),
    };

    Json(json!({
        "version": state.version,
        "uptimeSeconds": uptime_seconds,
        "configSummary": config_summary_json,
    }))
}

async fn cache_dump_handler(
    State(state): State<Arc<AdminState>>,
) -> Result<Json<CacheDumpResponse>, (StatusCode, Json<Value>)> {
    match &state.cache {
        Some(cache) => {
            if !cache.is_enabled() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "status": "error",
                        "message": "DNS cache is not enabled"
                    })),
                ));
            }

            let entries = cache.iter_entries();
            info!("Cache dump: {} entries dumped", entries.len());
            Ok(Json(CacheDumpResponse { entries }))
        }
        None => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "status": "error",
                "message": "DNS cache is not configured"
            })),
        )),
    }
}

async fn cache_restore_handler(
    State(state): State<Arc<AdminState>>,
    Json(body): Json<CacheRestoreRequest>,
) -> Result<Json<CacheRestoreStats>, (StatusCode, Json<Value>)> {
    match &state.cache {
        Some(cache) => {
            if !cache.is_enabled() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "status": "error",
                        "message": "DNS cache is not enabled"
                    })),
                ));
            }

            let mut total_loaded = 0u64;
            let mut total_skipped_expired = 0u64;
            let mut total_failed = 0u64;

            for entry in body.entries {
                let stats = cache.insert_restored(entry).await;
                total_loaded += stats.loaded;
                total_skipped_expired += stats.skipped_expired;
                total_failed += stats.failed;
            }

            info!(
                "Cache restore completed: loaded={}, skipped_expired={}, failed={}",
                total_loaded, total_skipped_expired, total_failed
            );

            Ok(Json(CacheRestoreStats {
                loaded: total_loaded,
                skipped_expired: total_skipped_expired,
                failed: total_failed,
            }))
        }
        None => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "status": "error",
                "message": "DNS cache is not configured"
            })),
        )),
    }
}
