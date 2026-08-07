use crate::error::AppError;
use crate::handler::RequestHandler as DnsRequestHandler;
use crate::metrics::{normalize_query_type_label, normalize_response_code, METRICS};
use crate::r#const::{error_labels, protocol_labels};
use crate::rate_limit::RateLimiter;
use hickory_proto::op::{Header, Message, MessageType, OpCode, ResponseCode};
use hickory_proto::rr::RecordType;
use hickory_server::authority::MessageResponseBuilder;
use hickory_server::server::{Request, RequestHandler, ResponseHandler, ResponseInfo};
use socket2::{Domain, Protocol, Socket, Type as SocketType};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::{TcpListener, UdpSocket};
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemHandle};
use tracing::{debug, error, info, warn};

pub struct HandlerAdapter {
    handler: Arc<DnsRequestHandler>,
    rate_limiter: Option<Arc<RateLimiter>>,
}

#[doc(hidden)]
pub fn parse_request_message(request: &Request) -> Result<Message, AppError> {
    let mut message = Message::new();
    message.set_id(request.id());
    message.set_message_type(request.message_type());
    message.set_op_code(request.op_code());
    message.set_authoritative(request.authoritative());
    message.set_authentic_data(request.authentic_data());
    message.set_checking_disabled(request.checking_disabled());
    message.set_recursion_desired(request.recursion_desired());
    message.set_recursion_available(request.recursion_available());
    message.set_truncated(request.truncated());

    if let Some(first_query) = request.queries().first() {
        message.add_query(first_query.original().clone());
    }

    if let Some(edns) = request.edns() {
        message.set_edns(edns.clone());
    }

    Ok(message)
}

impl HandlerAdapter {
    pub fn new(handler: Arc<DnsRequestHandler>, rate_limiter: Option<Arc<RateLimiter>>) -> Self {
        Self {
            handler,
            rate_limiter,
        }
    }
}

#[async_trait::async_trait]
impl RequestHandler for HandlerAdapter {
    #[tracing::instrument(
        name = "dns_query",
        skip(self, request, response_handler),
        fields(
            client_ip = %request.src().ip(),
            query = %request.queries().first().map(|q| q.name().to_utf8()).unwrap_or_default(),
            qtype = %request.queries().first().map(|q| q.query_type()).unwrap_or(RecordType::A)
        )
    )]
    async fn handle_request<R: ResponseHandler>(
        &self,
        request: &Request,
        mut response_handler: R,
    ) -> ResponseInfo {
        // 记录开始时间
        let start_time = Instant::now();

        // 记录协议类型
        let protocol = match request.protocol() {
            hickory_proto::xfer::Protocol::Udp => protocol_labels::UDP,
            hickory_proto::xfer::Protocol::Tcp => protocol_labels::TCP,
            _ => "unknown",
        };

        if let Some(limiter) = &self.rate_limiter {
            let src_ip = request.src().ip();
            if !limiter.check(src_ip) {
                warn!(
                    client_ip = %request.src(),
                    "DNS request rate limited"
                );
                METRICS
                    .dns_request_errors_total
                    .with_label_values(&[error_labels::REQUEST_ERROR])
                    .inc();
                let mut header = Header::new();
                header.set_id(request.id());
                header.set_response_code(ResponseCode::Refused);
                return ResponseInfo::from(header);
            }
        }

        METRICS
            .dns_requests_total
            .with_label_values(&[protocol])
            .inc();

        // 检查是否为查询请求或支持的操作码
        if request.op_code() != OpCode::Query {
            debug!("Unsupported operation code: {:?}", request.op_code());

            // 记录错误
            METRICS
                .dns_request_errors_total
                .with_label_values(&[error_labels::UNSUPPORTED_OPCODE])
                .inc();

            let mut header = Header::new();
            header.set_id(request.id());
            header.set_op_code(request.op_code());
            header.set_response_code(ResponseCode::NotImp);

            let builder = MessageResponseBuilder::from_message_request(request);
            let response = builder.error_msg(&header, ResponseCode::NotImp);

            return response_handler
                .send_response(response)
                .await
                .unwrap_or_else(|e| {
                    error!("Error sending response: {}", e);
                    ResponseInfo::from(header)
                });
        }

        if request.message_type() != MessageType::Query {
            debug!("Unsupported message type: {:?}", request.message_type());

            // 记录错误
            METRICS
                .dns_request_errors_total
                .with_label_values(&[error_labels::UNSUPPORTED_MESSAGE_TYPE])
                .inc();

            let mut header = Header::new();
            header.set_id(request.id());
            header.set_op_code(request.op_code());
            header.set_response_code(ResponseCode::NotImp);

            let builder = MessageResponseBuilder::from_message_request(request);
            let response = builder.error_msg(&header, ResponseCode::NotImp);

            return response_handler
                .send_response(response)
                .await
                .unwrap_or_else(|e| {
                    error!("Error sending response: {}", e);
                    ResponseInfo::from(header)
                });
        }

        // 获取请求的查询
        let query_type = request
            .queries()
            .first()
            .map(|q| q.query_type())
            .unwrap_or(RecordType::A);
        let query_type_label = normalize_query_type_label(query_type);

        debug!(
            protocol = %protocol,
            client = %request.src(),
            query_type = %query_type,
            "DNS query request received"
        );

        let message = match parse_request_message(request) {
            Ok(message) => message,
            Err(e) => {
                error!("Failed to parse request message: {}", e);

                METRICS
                    .dns_request_errors_total
                    .with_label_values(&[error_labels::REQUEST_ERROR])
                    .inc();

                let mut header = Header::new();
                header.set_id(request.id());
                header.set_op_code(request.op_code());
                header.set_response_code(ResponseCode::ServFail);

                let builder = MessageResponseBuilder::from_message_request(request);
                let response = builder.error_msg(&header, ResponseCode::ServFail);

                // 记录处理时间
                let duration = start_time.elapsed();
                METRICS
                    .dns_request_duration_seconds
                    .with_label_values(&[protocol, query_type_label])
                    .observe(duration.as_secs_f64());

                return response_handler
                    .send_response(response)
                    .await
                    .unwrap_or_else(|e| {
                        error!("Error sending response: {}", e);
                        ResponseInfo::from(header)
                    });
            }
        };

        // 异步处理请求
        match self.handler.handle_request(&message).await {
            Ok(result) => {
                // 构建响应
                let header = *result.header();

                // 记录响应码指标
                METRICS
                    .dns_response_codes_total
                    .with_label_values(&[normalize_response_code(header.response_code())])
                    .inc();

                let mut builder = MessageResponseBuilder::from_message_request(request);

                if let Some(response_edns) = result.extensions() {
                    builder.edns(response_edns.clone());
                }

                let additionals: Vec<&hickory_proto::rr::Record> = result
                    .additionals()
                    .iter()
                    .filter(|r| r.record_type() != RecordType::OPT)
                    .collect();

                let response = builder.build(
                    header,
                    result.answers().iter(),
                    result.name_servers().iter(),
                    additionals,
                    None,
                );

                // 记录处理时间
                let duration = start_time.elapsed();
                METRICS
                    .dns_request_duration_seconds
                    .with_label_values(&[protocol, query_type_label])
                    .observe(duration.as_secs_f64());

                response_handler
                    .send_response(response)
                    .await
                    .unwrap_or_else(|e| {
                        let mut err_header = Header::new();
                        err_header.set_response_code(ResponseCode::ServFail);
                        error!("Error sending response: {}", e);
                        ResponseInfo::from(err_header)
                    })
            }
            Err(e) => {
                error!("Error processing DNS request: {}", e);

                // 记录错误
                METRICS
                    .dns_request_errors_total
                    .with_label_values(&[error_labels::HANDLER_ERROR])
                    .inc();

                let mut header = Header::new();
                header.set_id(request.id());
                header.set_op_code(request.op_code());
                header.set_response_code(ResponseCode::ServFail);

                let builder = MessageResponseBuilder::from_message_request(request);
                let response = builder.error_msg(&header, ResponseCode::ServFail);

                // 记录处理时间
                let duration = start_time.elapsed();
                METRICS
                    .dns_request_duration_seconds
                    .with_label_values(&[protocol, query_type_label])
                    .observe(duration.as_secs_f64());

                response_handler
                    .send_response(response)
                    .await
                    .unwrap_or_else(|e| {
                        error!("Error sending response: {}", e);
                        ResponseInfo::from(header)
                    })
            }
        }
    }
}

// DNS 服务器配置
pub struct DnsServerConfig {
    pub udp_bind_addr: SocketAddr,
    pub tcp_bind_addr: SocketAddr,
    pub http_bind_addr: SocketAddr,
    pub tcp_timeout: u64,
    pub http_timeout: u64,
    pub rate_limiter: Option<Arc<RateLimiter>>,
    pub udp_recv_buffer: usize,
    pub udp_send_buffer: usize,
    pub udp_socket_count: usize,
}

impl Default for DnsServerConfig {
    fn default() -> Self {
        Self {
            udp_bind_addr: "127.0.0.1:53".parse().unwrap(),
            tcp_bind_addr: "127.0.0.1:53".parse().unwrap(),
            http_bind_addr: "127.0.0.1:8080".parse().unwrap(),
            tcp_timeout: 10,
            http_timeout: 10,
            rate_limiter: None,
            udp_recv_buffer: 4 * 1024 * 1024,
            udp_send_buffer: 4 * 1024 * 1024,
            udp_socket_count: 1,
        }
    }
}

pub struct DnsServer {
    config: DnsServerConfig,
    handler: Arc<DnsRequestHandler>,
}

impl DnsServer {
    pub fn new(config: DnsServerConfig, handler: Arc<DnsRequestHandler>) -> Self {
        Self { config, handler }
    }
}

#[async_trait::async_trait]
impl IntoSubsystem<AppError> for DnsServer {
    async fn run(self, subsys: SubsystemHandle) -> Result<(), AppError> {
        let adapter = HandlerAdapter::new(self.handler.clone(), self.config.rate_limiter.clone());

        let mut server = hickory_server::ServerFuture::new(adapter);

        let socket_count = self.config.udp_socket_count.max(1);

        for i in 0..socket_count {
            let addr = self.config.udp_bind_addr;
            let domain = if addr.is_ipv4() {
                Domain::IPV4
            } else {
                Domain::IPV6
            };
            let s = Socket::new(domain, SocketType::DGRAM, Some(Protocol::UDP)).map_err(|e| {
                error!("Failed to create UDP socket with socket2: {}", e);
                AppError::Io(e)
            })?;
            s.set_recv_buffer_size(self.config.udp_recv_buffer)
                .map_err(|e| {
                    error!("Failed to set recv buffer size: {}", e);
                    AppError::Io(e)
                })?;
            s.set_send_buffer_size(self.config.udp_send_buffer)
                .map_err(|e| {
                    error!("Failed to set send buffer size: {}", e);
                    AppError::Io(e)
                })?;
            if socket_count > 1 {
                s.set_reuse_address(true).map_err(|e| {
                    error!("Failed to set SO_REUSEADDR: {}", e);
                    AppError::Io(e)
                })?;
                #[cfg(unix)]
                s.set_reuse_port(true).map_err(|e| {
                    error!("Failed to set SO_REUSEPORT: {}", e);
                    AppError::Io(e)
                })?;
            }
            s.bind(&addr.into()).map_err(|e| {
                error!("Failed to bind UDP socket: {}", e);
                AppError::Io(e)
            })?;
            s.set_nonblocking(true).map_err(|e| {
                error!("Failed to set nonblocking: {}", e);
                AppError::Io(e)
            })?;
            let std_socket: std::net::UdpSocket = s.into();
            let socket = UdpSocket::from_std(std_socket);

            match socket {
                Ok(s) => {
                    info!(
                        "DNS server UDP socket {}/{} listening on {}",
                        i + 1,
                        socket_count,
                        addr
                    );
                    server.register_socket(s);
                }
                Err(e) => {
                    error!("Failed to bind UDP socket: {}", e);
                    return Err(AppError::Io(e));
                }
            }
        }
        info!(
            "DNS server UDP listening on {} with {} socket(s)",
            self.config.udp_bind_addr, socket_count
        );

        // 绑定 TCP 端口
        let tcp_listener = match TcpListener::bind(self.config.tcp_bind_addr).await {
            Ok(listener) => {
                info!("DNS server TCP listening on {}", self.config.tcp_bind_addr);
                listener
            }
            Err(e) => {
                error!("Failed to bind TCP listener: {}", e);
                return Err(AppError::Io(e));
            }
        };

        // 设置TCP超时
        let tcp_timeout = std::time::Duration::from_secs(self.config.tcp_timeout);
        server.register_listener(tcp_listener, tcp_timeout);

        // 使用tokio::select!监听服务器和关闭信号
        tokio::select! {
            result = server.block_until_done() => {
                if let Err(e) = result {
                    error!("DNS server error: {}", e);
                } else {
                    info!("DNS server completed normally");
                }
                Ok(())
            }
            _ = subsys.on_shutdown_requested() => {
                info!("Shutdown requested, stopping DNS server");

                // 使用timeout包装graceful shutdown
                match tokio::time::timeout(
                    std::time::Duration::from_secs(15),
                    server.shutdown_gracefully()
                ).await {
                    Ok(Ok(_)) => info!("DNS server shutdown completed successfully"),
                    Ok(Err(e)) => warn!("DNS server shutdown error: {}", e),
                    Err(_) => warn!("DNS server shutdown timed out")
                }

                Ok(())
            }
        }
    }
}
