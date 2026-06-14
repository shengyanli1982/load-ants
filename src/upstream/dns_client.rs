use crate::config::DnsClientConfig;
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
    config: DnsClientConfig,
    connector: GenericConnector<TokioRuntimeProvider>,
    opts: ResolverOpts,
    tcp_conns: Arc<DashMap<SocketAddr, (GenericConnection, Instant)>>,
}

impl DnsClient {
    pub fn new(config: DnsClientConfig) -> Self {
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
    ) -> Result<DnsClientResponse, DnsClientSendError> {
        let mut attempts = Vec::new();

        if self.config.prefer_tcp {
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
                    return Ok(DnsClientResponse {
                        message: response,
                        attempts,
                    });
                }
                Err(error) => {
                    attempts.push(DnsClientAttempt {
                        transport: DnsTransport::Tcp,
                        duration,
                        truncated: false,
                    });
                    return Err(DnsClientSendError { error, attempts });
                }
            }
        }

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
        let idle_timeout = Duration::from_secs(self.config.tcp_idle_timeout);

        // 惰性检查：获取缓存连接时检查是否已空闲超时
        let conn = match self.tcp_conns.get(&addr) {
            Some(entry) => {
                let (ref cached_conn, last_used) = *entry;
                if Instant::now().duration_since(last_used) > idle_timeout {
                    // 连接已超时，丢弃并重新连接
                    drop(entry);
                    self.tcp_conns.remove(&addr);
                    let new_conn = self.connect(addr, Protocol::Tcp).await?;
                    self.tcp_conns.insert(addr, (new_conn.clone(), Instant::now()));
                    new_conn
                } else {
                    cached_conn.clone()
                }
            }
            None => {
                let conn = self.connect(addr, Protocol::Tcp).await?;
                self.tcp_conns.insert(addr, (conn.clone(), Instant::now()));
                conn
            }
        };

        let result = self.send_with_conn(conn, message).await;
        match &result {
            Ok(_) => {
                // 成功时更新最近使用时间
                if let Some(mut entry) = self.tcp_conns.get_mut(&addr) {
                    entry.1 = Instant::now();
                }
            }
            Err(_) => {
                if self.config.tcp_reconnect {
                    self.tcp_conns.remove(&addr);
                }
            }
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
        let request_timeout = TokioDuration::from_secs(self.config.request_timeout);
        let response = match time::timeout(request_timeout, stream.next()).await {
            Ok(Some(result)) => result.map_err(|e| AppError::Upstream(e.to_string()))?,
            Ok(None) => {
                return Err(AppError::Upstream(
                    "empty response stream".to_string(),
                ))
            }
            Err(_) => return Err(AppError::Timeout),
        };

        Ok(response.into_message())
    }
}
