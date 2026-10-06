use crate::error::AppError;
use crate::handler::RequestHandler as DnsRequestHandler;
use crate::metrics::{query_type_index, response_code_index, METRICS};
use crate::r#const::{error_indices, protocol_indices, protocol_labels, subsystem_names};
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

        // 记录协议类型（protocol_idx 供 OPT-2 预解析 metric 句柄索引）
        let (protocol, protocol_idx) = match request.protocol() {
            hickory_proto::xfer::Protocol::Udp => (protocol_labels::UDP, protocol_indices::UDP),
            hickory_proto::xfer::Protocol::Tcp => (protocol_labels::TCP, protocol_indices::TCP),
            _ => (protocol_labels::UNKNOWN, protocol_indices::UNKNOWN),
        };

        if let Some(limiter) = &self.rate_limiter {
            let src_ip = request.src().ip();
            if !limiter.check(src_ip) {
                debug!(protocol = %protocol, "Rate limit hit");
                METRICS.request_errors[error_indices::REQUEST_ERROR].inc();

                // D-09: 实际发送 REFUSED 而非静默丢包——静默丢包会触发客户端
                // 超时重试放大入站流量，且与 DoH 侧 429 显式拒绝语义不一致。
                // UDP send_to / TCP 长度前缀帧由 ResponseHandler 实现负责，
                // 与正常回包共用同一路径。
                let mut header = Header::new();
                header.set_id(request.id());
                header.set_response_code(ResponseCode::Refused);

                let builder = MessageResponseBuilder::from_message_request(request);
                let response = builder.error_msg(&header, ResponseCode::Refused);

                return response_handler
                    .send_response(response)
                    .await
                    .unwrap_or_else(|_| {
                        debug!(
                            client_ip = %request.src().ip(),
                            protocol = %protocol,
                            "Failed to send response"
                        );
                        ResponseInfo::from(header)
                    });
            }
        }

        METRICS.requests_by_protocol[protocol_idx].inc();

        // 检查是否为查询请求或支持的操作码
        if request.op_code() != OpCode::Query {
            debug!(opcode = ?request.op_code(), "Unsupported operation code");

            // 记录错误
            METRICS.request_errors[error_indices::UNSUPPORTED_OPCODE].inc();

            let mut header = Header::new();
            header.set_id(request.id());
            header.set_op_code(request.op_code());
            header.set_response_code(ResponseCode::NotImp);

            let builder = MessageResponseBuilder::from_message_request(request);
            let response = builder.error_msg(&header, ResponseCode::NotImp);

            return response_handler
                .send_response(response)
                .await
                .unwrap_or_else(|_| {
                    debug!(
                        client_ip = %request.src().ip(),
                        protocol = %protocol,
                        "Failed to send response"
                    );
                    ResponseInfo::from(header)
                });
        }

        if request.message_type() != MessageType::Query {
            debug!(
                message_type = ?request.message_type(),
                "Unsupported message type"
            );

            // 记录错误
            METRICS.request_errors[error_indices::UNSUPPORTED_MESSAGE_TYPE].inc();

            let mut header = Header::new();
            header.set_id(request.id());
            header.set_op_code(request.op_code());
            header.set_response_code(ResponseCode::NotImp);

            let builder = MessageResponseBuilder::from_message_request(request);
            let response = builder.error_msg(&header, ResponseCode::NotImp);

            return response_handler
                .send_response(response)
                .await
                .unwrap_or_else(|_| {
                    debug!(
                        client_ip = %request.src().ip(),
                        protocol = %protocol,
                        "Failed to send response"
                    );
                    ResponseInfo::from(header)
                });
        }

        // 获取请求的查询
        let query_type = request
            .queries()
            .first()
            .map(|q| q.query_type())
            .unwrap_or(RecordType::A);
        let query_type_idx = query_type_index(query_type);

        debug!(protocol = %protocol, "DNS request received");

        let message = match parse_request_message(request) {
            Ok(message) => message,
            Err(e) => {
                debug!(
                    client_ip = %request.src().ip(),
                    protocol = %protocol,
                    error = %e,
                    "Failed to parse request message"
                );

                METRICS.request_errors[error_indices::REQUEST_ERROR].inc();

                let mut header = Header::new();
                header.set_id(request.id());
                header.set_op_code(request.op_code());
                header.set_response_code(ResponseCode::ServFail);

                let builder = MessageResponseBuilder::from_message_request(request);
                let response = builder.error_msg(&header, ResponseCode::ServFail);

                // 记录处理时间
                let duration = start_time.elapsed();
                METRICS.request_durations[protocol_idx][query_type_idx]
                    .observe(duration.as_secs_f64());

                return response_handler
                    .send_response(response)
                    .await
                    .unwrap_or_else(|_| {
                        debug!(
                            client_ip = %request.src().ip(),
                            protocol = %protocol,
                            "Failed to send response"
                        );
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
                METRICS.response_codes[response_code_index(header.response_code())].inc();

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
                METRICS.request_durations[protocol_idx][query_type_idx]
                    .observe(duration.as_secs_f64());

                response_handler
                    .send_response(response)
                    .await
                    .unwrap_or_else(|_| {
                        let mut err_header = Header::new();
                        err_header.set_response_code(ResponseCode::ServFail);
                        debug!(
                            client_ip = %request.src().ip(),
                            protocol = %protocol,
                            "Failed to send response"
                        );
                        ResponseInfo::from(err_header)
                    })
            }
            Err(e) => {
                debug!(error = %e, "Failed to process request");

                // 记录错误
                METRICS.request_errors[error_indices::HANDLER_ERROR].inc();

                let mut header = Header::new();
                header.set_id(request.id());
                header.set_op_code(request.op_code());
                header.set_response_code(ResponseCode::ServFail);

                let builder = MessageResponseBuilder::from_message_request(request);
                let response = builder.error_msg(&header, ResponseCode::ServFail);

                // 记录处理时间
                let duration = start_time.elapsed();
                METRICS.request_durations[protocol_idx][query_type_idx]
                    .observe(duration.as_secs_f64());

                response_handler
                    .send_response(response)
                    .await
                    .unwrap_or_else(|_| {
                        debug!(
                            client_ip = %request.src().ip(),
                            protocol = %protocol,
                            "Failed to send response"
                        );
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

/// 将 `ServerFuture::block_until_done` 的结果映射为子系统运行结果。
/// 提取为独立函数以便对"运行期错误必须向上传播"这一语义进行单元测试（D-17）。
///
/// D-17: 运行期错误必须向上传播（与 admin 子系统对齐），使 toplevel 感知失败、
/// 进程以非零退出码结束；吞掉错误会让 systemd/docker 重启策略与监控失效。
/// 错误属子系统生命周期事件，按规范记 ERROR 一次。
#[doc(hidden)]
pub fn dns_server_result(result: Result<(), hickory_proto::ProtoError>) -> Result<(), AppError> {
    result.map_err(|e| {
        error!(error = %e, "Failed to run DNS server");
        AppError::DnsProto(e)
    })
}

/// 计算当前平台生效的 UDP socket 数量（D-18 seam，便于单元测试）。
///
/// unix: 保持配置值（SO_REUSEPORT 支持多 socket 安全地同端口绑定）。
/// Windows: 无 SO_REUSEPORT，多 socket 同端口绑定只能依赖 SO_REUSEADDR，
/// 而 Windows 的 SO_REUSEADDR 允许本机其他进程劫持已绑定端口（实测验证），
/// 且收包分发不可控，因此降级为单 socket（Windows 默认 bind 即排他）。
#[doc(hidden)]
pub fn effective_udp_socket_count(configured: usize) -> usize {
    let count = configured.max(1);
    #[cfg(windows)]
    if count > 1 {
        warn!(
            requested = count,
            effective = 1,
            "udp_socket_count > 1 is unsupported on Windows (no SO_REUSEPORT, SO_REUSEADDR is hijack-prone); falling back to a single socket"
        );
        return 1;
    }
    count
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

        let socket_count = effective_udp_socket_count(self.config.udp_socket_count);

        for i in 0..socket_count {
            let addr = self.config.udp_bind_addr;
            let domain = if addr.is_ipv4() {
                Domain::IPV4
            } else {
                Domain::IPV6
            };
            let s = Socket::new(domain, SocketType::DGRAM, Some(Protocol::UDP)).map_err(|e| {
                error!(addr = %addr, error = %e, "Failed to create UDP socket");
                AppError::Io(e)
            })?;
            s.set_recv_buffer_size(self.config.udp_recv_buffer)
                .map_err(|e| {
                    error!(addr = %addr, error = %e, "Failed to set recv buffer size");
                    AppError::Io(e)
                })?;
            s.set_send_buffer_size(self.config.udp_send_buffer)
                .map_err(|e| {
                    error!(addr = %addr, error = %e, "Failed to set send buffer size");
                    AppError::Io(e)
                })?;
            // D-18: 仅 unix 需要（且能安全支持）多 socket 同端口绑定。
            // Windows 已在 effective_udp_socket_count 降级为单 socket，此块不编译；
            // 且 Windows 的 SO_REUSEADDR 允许端口被其他进程劫持，绝不可设置。
            #[cfg(unix)]
            if socket_count > 1 {
                s.set_reuse_address(true).map_err(|e| {
                    error!(addr = %addr, error = %e, "Failed to set SO_REUSEADDR");
                    AppError::Io(e)
                })?;
                s.set_reuse_port(true).map_err(|e| {
                    error!(addr = %addr, error = %e, "Failed to set SO_REUSEPORT");
                    AppError::Io(e)
                })?;
            }
            s.bind(&addr.into()).map_err(|e| {
                error!(addr = %addr, error = %e, "Failed to bind UDP socket");
                AppError::Io(e)
            })?;
            s.set_nonblocking(true).map_err(|e| {
                error!(addr = %addr, error = %e, "Failed to set nonblocking");
                AppError::Io(e)
            })?;
            let std_socket: std::net::UdpSocket = s.into();
            let socket = UdpSocket::from_std(std_socket);

            match socket {
                Ok(s) => {
                    debug!(addr = %addr, index = i + 1, "UDP socket bound");
                    server.register_socket(s);
                }
                Err(e) => {
                    error!(addr = %addr, error = %e, "Failed to register UDP socket");
                    return Err(AppError::Io(e));
                }
            }
        }
        info!(
            transport = "udp",
            addr = %self.config.udp_bind_addr,
            sockets = socket_count,
            "Listener ready"
        );

        // 绑定 TCP 端口
        let tcp_listener = match TcpListener::bind(self.config.tcp_bind_addr).await {
            Ok(listener) => {
                info!(transport = "tcp", addr = %self.config.tcp_bind_addr, "Listener ready");
                listener
            }
            Err(e) => {
                error!(addr = %self.config.tcp_bind_addr, error = %e, "Failed to bind TCP listener");
                return Err(AppError::Io(e));
            }
        };

        // 设置TCP超时
        let tcp_timeout = std::time::Duration::from_secs(self.config.tcp_timeout);
        server.register_listener(tcp_listener, tcp_timeout);

        // 使用tokio::select!监听服务器和关闭信号
        tokio::select! {
            result = server.block_until_done() => match dns_server_result(result) {
                Ok(()) => {
                    info!("DNS server completed");
                    Ok(())
                }
                Err(e) => Err(e),
            },
            _ = subsys.on_shutdown_requested() => {
                info!(subsystem = subsystem_names::DNS_SERVER, "Shutdown requested");

                // 使用timeout包装graceful shutdown
                match tokio::time::timeout(
                    std::time::Duration::from_secs(15),
                    server.shutdown_gracefully()
                ).await {
                    Ok(Ok(_)) => info!(status = "graceful", "DNS server stopped"),
                    Ok(Err(e)) => warn!(status = "error", error = %e, "DNS server stopped"),
                    Err(_) => warn!(status = "timeout", "DNS server stopped")
                }

                Ok(())
            }
        }
    }
}
