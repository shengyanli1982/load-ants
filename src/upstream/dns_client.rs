use crate::config::DnsClientConfig;
use crate::error::AppError;
use crate::metrics::METRICS;
use dashmap::DashMap;
use futures_util::StreamExt;
use hickory_proto::op::{Message, Query};
use hickory_proto::rr::Name;
use hickory_proto::runtime::TokioRuntimeProvider;
use hickory_proto::xfer::Protocol;
use hickory_proto::xfer::{DnsHandle, DnsRequest, DnsRequestOptions};
use hickory_server::resolver::config::{NameServerConfig, ResolverOpts};
use hickory_server::resolver::name_server::{
    ConnectionProvider, GenericConnection, GenericConnector,
};
use rand::Rng;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::AbortHandle;
use tokio::time::{self, Duration as TokioDuration};
use tracing::warn;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsTransport {
    Udp,
    Tcp,
}

#[derive(Debug, Clone)]
pub struct DnsClientAttempt {
    pub transport: DnsTransport,
    pub duration: Duration,
    /// 上次响应是否被截断（TC 标志），用于调试和日志
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
struct PooledConnection {
    connection: GenericConnection,
    last_used: Instant,
}

impl PooledConnection {
    fn new(connection: GenericConnection) -> Self {
        Self {
            connection,
            last_used: Instant::now(),
        }
    }
}

pub struct DnsClient {
    config: DnsClientConfig,
    connector: GenericConnector<TokioRuntimeProvider>,
    opts: ResolverOpts,
    tcp_conns: Arc<DashMap<SocketAddr, PooledConnection>>,
    udp_conns: Arc<DashMap<SocketAddr, PooledConnection>>,
    cleanup_handle: Option<AbortHandle>,
    /// TCP 连接池最大连接数上限，超限时淘汰最老连接
    max_tcp_conns: usize,
}

impl DnsClient {
    pub fn new(config: DnsClientConfig) -> Self {
        let mut opts = ResolverOpts::default();
        opts.timeout = Duration::from_secs(config.request_timeout);

        let max_tcp_conns = config.max_tcp_connections;
        let tcp_conns = Arc::new(DashMap::new());
        let udp_conns = Arc::new(DashMap::new());
        let cleanup_handle = Self::spawn_cleanup_task(
            tcp_conns.clone(),
            udp_conns.clone(),
            Duration::from_secs(config.idle_connection_timeout),
        );

        Self {
            config,
            connector: GenericConnector::new(TokioRuntimeProvider::new()),
            opts,
            tcp_conns,
            udp_conns,
            cleanup_handle: Some(cleanup_handle),
            max_tcp_conns,
        }
    }

    fn spawn_cleanup_task(
        tcp_conns: Arc<DashMap<SocketAddr, PooledConnection>>,
        udp_conns: Arc<DashMap<SocketAddr, PooledConnection>>,
        idle_timeout: Duration,
    ) -> AbortHandle {
        let interval =
            TokioDuration::from_secs(crate::r#const::dns_client_limits::CLEANUP_INTERVAL_SECS);
        let handle = tokio::spawn(async move {
            let mut ticker = time::interval(interval);
            loop {
                ticker.tick().await;
                let now = Instant::now();
                let tcp_len_before = tcp_conns.len();
                tcp_conns.retain(|_, pooled| now.duration_since(pooled.last_used) < idle_timeout);
                udp_conns.retain(|_, pooled| now.duration_since(pooled.last_used) < idle_timeout);
                let tcp_len_after = tcp_conns.len();
                if tcp_len_before != tcp_len_after {
                    METRICS.tcp_pool_connections.set(tcp_len_after as i64);
                }
            }
        });
        handle.abort_handle()
    }

    /// 对查询消息中的域名进行 0x20 大小写随机化。
    /// 返回随机化后的消息副本，以及随机化后的域名 ASCII 字符串（用于响应验证）。
    fn randomize_query_case(message: &Message) -> (Message, Option<String>) {
        let mut rng = rand::thread_rng();
        let mut msg = message.clone();

        let randomized_name = if let Some(query) = msg.queries().first() {
            let original = query.name().to_ascii();
            // 对每个字节随机决定大小写
            let randomized: String = original
                .bytes()
                .map(|b| {
                    if b.is_ascii_alphabetic() {
                        if rng.gen::<bool>() {
                            b.to_ascii_uppercase() as char
                        } else {
                            b.to_ascii_lowercase() as char
                        }
                    } else {
                        b as char
                    }
                })
                .collect();

            match Name::from_ascii(&randomized) {
                Ok(randomized_name) => {
                    // 替换 queries 中的域名
                    let queries: Vec<Query> = msg
                        .take_queries()
                        .into_iter()
                        .enumerate()
                        .map(|(i, mut q)| {
                            if i == 0 {
                                q.set_name(randomized_name.clone());
                            }
                            q
                        })
                        .collect();
                    for q in queries {
                        msg.add_query(q);
                    }
                    Some(randomized)
                }
                Err(e) => {
                    warn!("Failed to create randomized DNS name: {}", e);
                    None
                }
            }
        } else {
            None
        };

        (msg, randomized_name)
    }

    /// 验证响应中的域名大小写是否与发送时一致（0x20 验证）。
    /// 返回 true 表示验证通过，false 表示验证失败（大小写不匹配或缺失 question
    /// section，可能是伪造响应）。
    fn verify_response_case(response: &Message, sent_name: &str) -> bool {
        match response.queries().first() {
            Some(q) => {
                let response_name = q.name().to_ascii();
                if response_name != sent_name {
                    warn!(
                        sent = %sent_name,
                        received = %response_name,
                        "0x20 case verification failed: response domain case mismatch"
                    );
                    false
                } else {
                    true
                }
            }
            // 缺失 question section 无法完成 0x20 校验，按校验失败处理，
            // 由调用方决定 strict 回退 TCP 或非 strict warn-only 放行。
            None => {
                warn!(
                    sent = %sent_name,
                    "0x20 case verification failed: response missing question section"
                );
                false
            }
        }
    }

    pub async fn send_to(
        &self,
        addr: SocketAddr,
        message: &Message,
        case_randomization: bool,
        case_randomization_strict: bool,
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

        // UDP 路径：可选 0x20 大小写随机化
        let (udp_query, sent_name) = if case_randomization {
            let (randomized, name) = Self::randomize_query_case(message);
            (randomized, name)
        } else {
            (message.clone(), None)
        };

        let start = Instant::now();
        let udp_result = self.send_udp(addr, &udp_query).await;
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

        // 0x20 响应验证
        // strict 模式：验证失败时丢弃 UDP 响应，回退到 TCP
        // 非 strict 模式：验证失败仅记录警告日志（warn-only），仍接受 UDP 响应
        if case_randomization {
            if let Some(ref name) = sent_name {
                let verified = Self::verify_response_case(&udp_response, name);
                if !verified && case_randomization_strict {
                    warn!("0x20 strict mode: discarding UDP response and falling back to TCP");
                    // 丢弃 UDP 响应，直接触发 TCP fallback
                    let start = Instant::now();
                    let tcp_result = self.send_tcp(addr, message).await;
                    let tcp_duration = start.elapsed();

                    return match tcp_result {
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
                    };
                }
            }
        }

        if !udp_response.truncated() {
            return Ok(DnsClientResponse {
                message: udp_response,
                attempts,
            });
        }

        // UDP 截断，回退到 TCP（TCP 不需要 0x20）
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
        let connect_future = self
            .connector
            .new_connection(&name_server, &self.opts)
            .map_err(|e| AppError::Upstream(e.to_string()))?;

        let conn_result: Result<GenericConnection, AppError> = if protocol == Protocol::Tcp {
            let connect_timeout = TokioDuration::from_secs(self.config.connect_timeout);
            match time::timeout(connect_timeout, connect_future).await {
                Ok(Ok(conn)) => Ok(conn),
                Ok(Err(e)) => Err(AppError::Upstream(e.to_string())),
                Err(_) => Err(AppError::Timeout),
            }
        } else {
            connect_future
                .await
                .map_err(|e| AppError::Upstream(e.to_string()))
        };

        conn_result
    }

    async fn send_udp(&self, addr: SocketAddr, message: &Message) -> Result<Message, AppError> {
        // 不持有 DashMap 锁跨 connect().await：先在锁外完成建连再插入。
        // 并发重复建连时后写覆盖先写（与 TCP 连接池语义一致），各调用方持有
        // 各自的连接克隆，被覆盖的连接不影响在途请求。
        let conn = if let Some(mut pooled) = self.udp_conns.get_mut(&addr) {
            pooled.last_used = Instant::now();
            pooled.connection.clone()
        } else {
            let conn = self.connect(addr, Protocol::Udp).await?;
            self.udp_conns
                .insert(addr, PooledConnection::new(conn.clone()));
            conn
        };

        let result = self.send_with_conn(conn, message).await;
        if result.is_err() {
            self.udp_conns.remove(&addr);
            let new_conn = self.connect(addr, Protocol::Udp).await?;
            let retry_result = self.send_with_conn(new_conn.clone(), message).await;
            if retry_result.is_ok() {
                self.udp_conns.insert(addr, PooledConnection::new(new_conn));
            }
            return retry_result;
        }
        result
    }

    async fn send_tcp(&self, addr: SocketAddr, message: &Message) -> Result<Message, AppError> {
        // 尝试复用已有连接
        let conn = if let Some(mut entry) = self.tcp_conns.get_mut(&addr) {
            entry.last_used = Instant::now();
            entry.connection.clone()
        } else {
            // 检查容量上限，超限时淘汰最老连接
            if self.tcp_conns.len() >= self.max_tcp_conns {
                self.evict_oldest_tcp_conn();
            }
            let conn = self.connect(addr, Protocol::Tcp).await?;
            self.tcp_conns
                .insert(addr, PooledConnection::new(conn.clone()));
            METRICS
                .tcp_pool_connections
                .set(self.tcp_conns.len() as i64);
            conn
        };

        let result = self.send_with_conn(conn, message).await;
        if result.is_err() {
            self.tcp_conns.remove(&addr);
            METRICS
                .tcp_pool_connections
                .set(self.tcp_conns.len() as i64);
            if self.config.tcp_reconnect {
                // 重连前再次检查容量（因为刚刚已移除一条，此时 len < max 必然成立）
                let new_conn = self.connect(addr, Protocol::Tcp).await?;
                let retry_result = self.send_with_conn(new_conn.clone(), message).await;
                if retry_result.is_ok() {
                    self.tcp_conns.insert(addr, PooledConnection::new(new_conn));
                    METRICS
                        .tcp_pool_connections
                        .set(self.tcp_conns.len() as i64);
                }
                return retry_result;
            }
        }
        result
    }

    /// 淘汰 TCP 连接池中 last_used 最老的一条连接。
    fn evict_oldest_tcp_conn(&self) {
        let oldest_key = self
            .tcp_conns
            .iter()
            .min_by_key(|entry| entry.value().last_used)
            .map(|entry| *entry.key());
        if let Some(key) = oldest_key {
            self.tcp_conns.remove(&key);
            METRICS
                .tcp_pool_connections
                .set(self.tcp_conns.len() as i64);
            warn!(
                evicted_addr = %key,
                pool_size = self.tcp_conns.len(),
                "TCP connection pool full, evicted oldest connection"
            );
        }
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

impl Drop for DnsClient {
    fn drop(&mut self) {
        if let Some(handle) = self.cleanup_handle.take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DnsClientConfig;
    use hickory_proto::op::MessageType;
    use hickory_proto::rr::RecordType;
    use std::net::Ipv4Addr;
    use std::str::FromStr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::net::UdpSocket;

    fn response_with_question(name_ascii: &str) -> Message {
        let mut response = Message::new();
        response.set_message_type(MessageType::Response);
        let name = Name::from_ascii(name_ascii).expect("valid DNS name");
        response.add_query(Query::query(name, RecordType::A));
        response
    }

    #[test]
    fn verify_response_case_fails_when_question_section_missing() {
        let response = Message::new();
        assert!(
            !DnsClient::verify_response_case(&response, "Example.com."),
            "missing question section must fail 0x20 verification"
        );
    }

    #[test]
    fn verify_response_case_passes_on_exact_case_match() {
        let response = response_with_question("Example.com.");
        assert!(DnsClient::verify_response_case(&response, "Example.com."));
    }

    #[test]
    fn verify_response_case_fails_on_case_mismatch() {
        let response = response_with_question("example.com.");
        assert!(!DnsClient::verify_response_case(&response, "Example.com."));
    }

    #[tokio::test]
    async fn send_udp_concurrent_first_connect_keeps_all_queries_successful() {
        let server_socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind test UDP server");
        let server_addr = server_socket.local_addr().expect("server addr");
        let received = Arc::new(AtomicUsize::new(0));
        {
            let received = received.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                loop {
                    let Ok((len, peer)) = server_socket.recv_from(&mut buf).await else {
                        break;
                    };
                    received.fetch_add(1, Ordering::SeqCst);
                    let Ok(query) = Message::from_vec(&buf[..len]) else {
                        continue;
                    };
                    let mut response = Message::new();
                    response.set_id(query.id());
                    response.set_message_type(MessageType::Response);
                    if let Some(q) = query.queries().first() {
                        response.add_query(q.clone());
                    }
                    let Ok(bytes) = response.to_vec() else {
                        continue;
                    };
                    let _ = server_socket.send_to(&bytes, peer).await;
                }
            });
        }

        let client = Arc::new(DnsClient::new(DnsClientConfig::default()));
        let barrier = Arc::new(tokio::sync::Barrier::new(8));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let client = client.clone();
            let barrier = barrier.clone();
            handles.push(tokio::spawn(async move {
                let mut query = Message::new();
                query.set_recursion_desired(true);
                let name = Name::from_str("concurrent.example.com.").expect("valid name");
                query.add_query(Query::query(name, RecordType::A));
                barrier.wait().await;
                client.send_udp(server_addr, &query).await
            }));
        }

        for handle in handles {
            let response = handle
                .await
                .expect("task join")
                .expect("concurrent first-connect send_udp must succeed");
            assert_eq!(response.queries().len(), 1);
            assert_eq!(
                response.queries()[0].name().to_ascii(),
                "concurrent.example.com."
            );
        }
        assert_eq!(received.load(Ordering::SeqCst), 8);
    }
}
