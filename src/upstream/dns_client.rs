use crate::config::{DnsConfig, DnsTransportMode};
use crate::error::AppError;
use dashmap::DashMap;
use futures_util::StreamExt;
use hickory_proto::op::Message;
use hickory_proto::xfer::{DnsHandle, DnsRequest, DnsRequestOptions};
use hickory_server::resolver::config::{NameServerConfig, Protocol, ResolverOpts};
use hickory_server::resolver::name_server::{
    ConnectionProvider, GenericConnection, GenericConnector, TokioRuntimeProvider,
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::{self, Duration as TokioDuration};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsTransport {
    Udp,
    Tcp,
}

#[derive(Debug, Clone)]
pub struct DnsClientAttempt {
    pub transport: DnsTransport,
    pub duration: Duration,
    #[allow(dead_code)]
    pub truncated: bool,
}

#[derive(Debug)]
pub struct DnsClientResponse {
    pub message: Message,
    pub attempts: Vec<DnsClientAttempt>,
}

#[derive(Debug)]
pub struct DnsClientSendError {
    pub error: AppError,
    pub attempts: Vec<DnsClientAttempt>,
}

#[derive(Clone)]
pub struct DnsClient {
    config: DnsConfig,
    connector: GenericConnector<TokioRuntimeProvider>,
    opts: ResolverOpts,
    tcp_conns: Arc<DashMap<SocketAddr, GenericConnection>>,
}

impl DnsClient {
    pub fn new(config: DnsConfig) -> Self {
        let mut opts = ResolverOpts::default();
        opts.timeout = Duration::from_secs(config.request_timeout);

        Self {
            config,
            connector: GenericConnector::new(TokioRuntimeProvider::new()),
            opts,
            tcp_conns: Arc::new(DashMap::new()),
        }
    }

    pub async fn send_to(
        &self,
        addr: SocketAddr,
        message: &Message,
        transport: Option<DnsTransportMode>,
    ) -> Result<DnsClientResponse, DnsClientSendError> {
        let mut attempts = Vec::new();

        let mode = match transport {
            Some(m) => m,
            None => {
                if self.config.prefer_tcp {
                    DnsTransportMode::Tcp
                } else {
                    DnsTransportMode::Auto
                }
            }
        };

        match mode {
            DnsTransportMode::Tcp => {
                let start = Instant::now();
                let result = self.send_tcp(addr, message).await;
                let duration = start.elapsed();

                match result {
                    Ok(response) => {
                        attempts.push(DnsClientAttempt {
                            transport: DnsTransport::Tcp,
                            duration,
                            truncated: response.truncated(),
                        });
                        Ok(DnsClientResponse {
                            message: response,
                            attempts,
                        })
                    }
                    Err(error) => {
                        attempts.push(DnsClientAttempt {
                            transport: DnsTransport::Tcp,
                            duration,
                            truncated: false,
                        });
                        Err(DnsClientSendError { error, attempts })
                    }
                }
            }
            DnsTransportMode::Udp => {
                let start = Instant::now();
                let result = self.send_udp(addr, message).await;
                let duration = start.elapsed();

                match result {
                    Ok(response) => {
                        attempts.push(DnsClientAttempt {
                            transport: DnsTransport::Udp,
                            duration,
                            truncated: response.truncated(),
                        });
                        Ok(DnsClientResponse {
                            message: response,
                            attempts,
                        })
                    }
                    Err(error) => {
                        attempts.push(DnsClientAttempt {
                            transport: DnsTransport::Udp,
                            duration,
                            truncated: false,
                        });
                        Err(DnsClientSendError { error, attempts })
                    }
                }
            }
            DnsTransportMode::Auto => {
                let start = Instant::now();
                let udp_result = self.send_udp(addr, message).await;
                let udp_duration = start.elapsed();

                let udp_response = match udp_result {
                    Ok(response) => {
                        let truncated = response.truncated();
                        attempts.push(DnsClientAttempt {
                            transport: DnsTransport::Udp,
                            duration: udp_duration,
                            truncated,
                        });
                        response
                    }
                    Err(error) => {
                        attempts.push(DnsClientAttempt {
                            transport: DnsTransport::Udp,
                            duration: udp_duration,
                            truncated: false,
                        });
                        return Err(DnsClientSendError { error, attempts });
                    }
                };

                if !udp_response.truncated() {
                    return Ok(DnsClientResponse {
                        message: udp_response,
                        attempts,
                    });
                }

                let start = Instant::now();
                let tcp_result = self.send_tcp(addr, message).await;
                let tcp_duration = start.elapsed();

                match tcp_result {
                    Ok(response) => {
                        attempts.push(DnsClientAttempt {
                            transport: DnsTransport::Tcp,
                            duration: tcp_duration,
                            truncated: response.truncated(),
                        });
                        Ok(DnsClientResponse {
                            message: response,
                            attempts,
                        })
                    }
                    Err(error) => {
                        attempts.push(DnsClientAttempt {
                            transport: DnsTransport::Tcp,
                            duration: tcp_duration,
                            truncated: false,
                        });
                        Err(DnsClientSendError { error, attempts })
                    }
                }
            }
        }
    }

    async fn connect(
        &self,
        addr: SocketAddr,
        protocol: Protocol,
    ) -> Result<GenericConnection, AppError> {
        let name_server = NameServerConfig::new(addr, protocol);
        let connect_future = self.connector.new_connection(&name_server, &self.opts);

        let conn_result = if protocol == Protocol::Tcp {
            let connect_timeout = TokioDuration::from_secs(self.config.connect_timeout);
            match time::timeout(connect_timeout, connect_future).await {
                Ok(conn) => conn,
                Err(_) => return Err(AppError::Timeout),
            }
        } else {
            connect_future.await
        };

        conn_result.map_err(|e| AppError::Upstream(e.to_string()))
    }

    async fn send_udp(&self, addr: SocketAddr, message: &Message) -> Result<Message, AppError> {
        let conn = self.connect(addr, Protocol::Udp).await?;
        self.send_with_conn(conn, message).await
    }

    async fn send_tcp(&self, addr: SocketAddr, message: &Message) -> Result<Message, AppError> {
        let conn = match self.tcp_conns.get(&addr) {
            Some(conn) => conn.clone(),
            None => {
                let conn = self.connect(addr, Protocol::Tcp).await?;
                self.tcp_conns.insert(addr, conn.clone());
                conn
            }
        };

        let result = self.send_with_conn(conn, message).await;
        if result.is_err() && self.config.tcp_reconnect {
            self.tcp_conns.remove(&addr);
        }
        result
    }

    async fn send_with_conn(
        &self,
        conn: GenericConnection,
        message: &Message,
    ) -> Result<Message, AppError> {
        let mut options = DnsRequestOptions::default();
        options.use_edns = message.extensions().is_some();
        options.recursion_desired = message.recursion_desired();
        let request = DnsRequest::new(message.clone(), options);

        let mut stream = conn.send(request);
        let response = stream
            .next()
            .await
            .ok_or_else(|| AppError::Upstream("empty response stream".to_string()))?
            .map_err(|e| AppError::Upstream(e.to_string()))?;

        Ok(response.into_message())
    }
}
