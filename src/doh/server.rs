use crate::doh::handlers::{handle_doh_get, handle_doh_post, handle_json_get};
use crate::doh::state::AppState;
use crate::error::AppError;
use crate::handler::RequestHandler;
use crate::rate_limit::RateLimiter;
use axum::extract::DefaultBodyLimit;
use axum::{routing::get, Router};
use axum_server::tls_rustls::RustlsConfig;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::{error, info, warn};

const DOH_QUERY_PATH: &str = "/dns-query";
const JSON_QUERY_PATH: &str = "/resolve";

pub struct DoHServer {
    bind_addr: SocketAddr,
    handler: Arc<RequestHandler>,
    rate_limiter: Option<Arc<RateLimiter>>,
    shutdown_tx: oneshot::Sender<()>,
    shutdown_rx: oneshot::Receiver<()>,
    tls_cert: Option<String>,
    tls_key: Option<String>,
}

impl DoHServer {
    pub fn new(
        bind_addr: SocketAddr,
        handler: Arc<RequestHandler>,
        rate_limiter: Option<Arc<RateLimiter>>,
        tls_cert: Option<String>,
        tls_key: Option<String>,
    ) -> Self {
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        Self {
            bind_addr,
            handler,
            rate_limiter,
            shutdown_tx,
            shutdown_rx,
            tls_cert,
            tls_key,
        }
    }

    fn create_router(&self) -> Router {
        let app_state = AppState {
            handler: self.handler.clone(),
            rate_limiter: self.rate_limiter.clone(),
        };

        Router::new()
            .route(DOH_QUERY_PATH, get(handle_doh_get).post(handle_doh_post))
            .route(JSON_QUERY_PATH, get(handle_json_get))
            .layer(DefaultBodyLimit::max(65535))
            .with_state(app_state)
    }

    pub async fn run(self, subsys: SubsystemHandle) -> Result<(), AppError> {
        let app = self.create_router();

        let use_tls = self.tls_cert.is_some() && self.tls_key.is_some();

        if use_tls {
            let cert_path = self.tls_cert.as_ref().unwrap();
            let key_path = self.tls_key.as_ref().unwrap();

            let tls_config = match RustlsConfig::from_pem_file(cert_path, key_path).await {
                Ok(config) => {
                    info!("DoH TLS configuration loaded successfully");
                    config
                }
                Err(e) => {
                    error!("Failed to load DoH TLS configuration: {}", e);
                    return Err(AppError::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Failed to load TLS cert/key: {}", e),
                    )));
                }
            };

            info!("DoH server (HTTPS) listening on {}", self.bind_addr);

            let handle = axum_server::Handle::new();
            let shutdown_handle = handle.clone();

            let server = axum_server::bind_rustls(self.bind_addr, tls_config)
                .handle(handle)
                .serve(app.into_make_service_with_connect_info::<SocketAddr>());

            tokio::select! {
                result = server => {
                    if let Err(e) = result {
                        error!("DoH server error: {}", e);
                    } else {
                        info!("DoH server completed normally");
                    }
                    Ok(())
                }
                _ = subsys.on_shutdown_requested() => {
                    info!("Shutdown requested, stopping DoH server");
                    shutdown_handle.graceful_shutdown(Some(std::time::Duration::from_secs(30)));
                    let _ = self.shutdown_tx.send(());
                    Ok(())
                }
            }
        } else {
            warn!("DoH server running without TLS (HTTP only) — this violates RFC 8484 HTTPS requirement; use TLS for production deployments");

            let listener = match TcpListener::bind(self.bind_addr).await {
                Ok(listener) => {
                    info!("DoH server (HTTP) listening on {}", self.bind_addr);
                    listener
                }
                Err(e) => {
                    error!("Failed to bind DoH server: {}", e);
                    return Err(AppError::Io(e));
                }
            };

            let shutdown_rx = self.shutdown_rx;

            tokio::select! {
                result = axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<SocketAddr>()
                )
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                    info!("DoH server received shutdown signal");
                }) => {
                    if let Err(e) = result {
                        error!("DoH server error: {}", e);
                    } else {
                        info!("DoH server completed normally");
                    }
                    Ok(())
                }
                _ = subsys.on_shutdown_requested() => {
                    info!("Shutdown requested, stopping DoH server");
                    let _ = self.shutdown_tx.send(());
                    Ok(())
                }
            }
        }
    }
}
