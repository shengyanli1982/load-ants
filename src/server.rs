use crate::error::AppError;
use crate::handler::RequestHandler as DnsRequestHandler;
use crate::metrics::METRICS;
use crate::r#const::{protocol_labels, error_labels};
use hickory_proto::op::{Header, Message, MessageType, OpCode, ResponseCode, Query};
use hickory_proto::rr::Record;
use hickory_server::server::{Request, RequestHandler, ResponseHandler, ResponseInfo};
use hickory_server::authority::MessageResponseBuilder;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::oneshot;
use tokio_graceful_shutdown::{SubsystemHandle, IntoSubsystem};
use tracing::{debug, error, info};

// DNS 服务器请求处理适配器
// 将我们的 RequestHandler 适配到 hickory-server 的 RequestHandler trait
pub struct HandlerAdapter {
    // 内部请求处理器
    handler: Arc<DnsRequestHandler>,
}

impl HandlerAdapter {
    // 创建新的处理器适配器
    pub fn new(handler: Arc<DnsRequestHandler>) -> Self {
        Self { handler }
    }
}

#[async_trait::async_trait]
impl RequestHandler for HandlerAdapter {
    async fn handle_request<R: ResponseHandler>(
        &self,
        request: &Request,
        mut response_handler: R,
    ) -> ResponseInfo {
        // 记录开始时间
        let start_time = Instant::now();
        
        // 记录协议类型
        let protocol = match request.src().ip() {
            std::net::IpAddr::V4(_) => protocol_labels::UDP,
            std::net::IpAddr::V6(_) => protocol_labels::TCP,
        };
        
        // 增加请求计数
        METRICS.dns_requests_total()
            .with_label_values(&[protocol])
            .inc();
        
        // 检查是否为查询请求或支持的操作码
        if request.op_code() != OpCode::Query {
            debug!("Unsupported operation code: {:?}", request.op_code());
            
            // 记录错误
            METRICS.dns_request_errors_total()
                .with_label_values(&[error_labels::UNSUPPORTED_OPCODE])
                .inc();
            
            let mut header = Header::new();
            header.set_id(request.id());
            header.set_op_code(request.op_code());
            header.set_response_code(ResponseCode::NotImp);
            
            let builder = MessageResponseBuilder::from_message_request(request);
            let response = builder.error_msg(&header, ResponseCode::NotImp);
            
            return response_handler.send_response(response).await
                .unwrap_or_else(|e| {
                    error!("Error sending response: {}", e);
                    ResponseInfo::from(header)
                });
        }

        if request.message_type() != MessageType::Query {
            debug!("Unsupported message type: {:?}", request.message_type());
            
            // 记录错误
            METRICS.dns_request_errors_total()
                .with_label_values(&[error_labels::UNSUPPORTED_MESSAGE_TYPE])
                .inc();
            
            let mut header = Header::new();
            header.set_id(request.id());
            header.set_op_code(request.op_code());
            header.set_response_code(ResponseCode::NotImp);
            
            let builder = MessageResponseBuilder::from_message_request(request);
            let response = builder.error_msg(&header, ResponseCode::NotImp);
            
            return response_handler.send_response(response).await
                .unwrap_or_else(|e| {
                    error!("Error sending response: {}", e);
                    ResponseInfo::from(header)
                });
        }

        // 获取请求的查询
        let query = request.query();
        let query_name = query.name();
        let query_type = query.query_type();
        
        debug!(
            "Received query request: {} {:?} from {}",
            query_name,
            query_type,
            request.src()
        );
        
        // 创建一个消息用于内部处理
        let mut message = Message::new();
        message.set_id(request.id());
        message.set_op_code(request.op_code());
        message.set_message_type(MessageType::Query);
        message.set_recursion_desired(request.recursion_desired());
        
        // 添加查询
        // 创建一个新的 Query 对象，因为 request.query() 返回的是 LowerQuery
        let mut temp_query = Query::new();
        temp_query.set_name(query.name().clone().into())
                  .set_query_type(query.query_type())
                  .set_query_class(query.query_class());
        message.add_query(temp_query);

        // 异步处理请求
        match self.handler.handle_request(&message).await {
            Ok(result) => {
                // 构建响应
                let header = *result.header();
                
                // 记录响应码指标
                METRICS.dns_response_codes_total()
                    .with_label_values(&[header.response_code().to_string().as_str()])
                    .inc();
                
                // 将Record引用转换为独立引用而不是双重引用
                let answers: Vec<Record> = result.answers().to_vec();
                let name_servers: Vec<Record> = result.name_servers().to_vec();
                let additionals: Vec<Record> = result.additionals().to_vec();
                
                // 创建独立的迭代器
                let answers_iter = answers.iter();
                let name_servers_iter = name_servers.iter();
                let additionals_iter = additionals.iter();
                
                let builder = MessageResponseBuilder::from_message_request(request);
                let response = builder.build(
                    header,
                    answers_iter,
                    name_servers_iter,
                    additionals_iter,
                    None // 不传递扩展信息
                );
                
                // 记录处理时间
                let duration = start_time.elapsed();
                METRICS.dns_request_duration_seconds()
                    .with_label_values(&[protocol, query_type.to_string().as_str()])
                    .observe(duration.as_secs_f64());
                
                response_handler.send_response(response).await
                    .unwrap_or_else(|e| {
                        let mut err_header = Header::new();
                        err_header.set_response_code(ResponseCode::ServFail);
                        error!("Error sending response: {}", e);
                        ResponseInfo::from(err_header)
                    })
            },
            Err(e) => {
                error!("Error processing DNS request: {}", e);
                
                // 记录错误
                METRICS.dns_request_errors_total()
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
                METRICS.dns_request_duration_seconds()
                    .with_label_values(&[protocol, query_type.to_string().as_str()])
                    .observe(duration.as_secs_f64());
                
                response_handler.send_response(response).await
                    .unwrap_or_else(|e| {
                        error!("Error sending response: {}", e);
                        ResponseInfo::from(header)
                    })
            }
        }
    }
}

// DNS 服务器
pub struct DnsServer {
    // 绑定地址
    bind_addr: SocketAddr,
    // 请求处理器
    handler: Arc<DnsRequestHandler>,
    // 停止信号接收端
    shutdown_rx: Option<oneshot::Receiver<()>>,
    // 停止信号发送端
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl DnsServer {
    // 创建新的 DNS 服务器
    pub fn new(
        bind_addr: SocketAddr,
        handler: Arc<DnsRequestHandler>,
    ) -> Self {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        
        Self {
            bind_addr,
            handler,
            shutdown_rx: Some(shutdown_rx),
            shutdown_tx: Some(shutdown_tx),
        }
    }
    
    // 停止 DNS 服务器
    pub fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
            info!("DNS server stop signal sent");
        }
    }

    // 运行服务器（用于优雅关闭集成）
    pub async fn run(mut self) -> Result<(), AppError> {
        let adapter = HandlerAdapter::new(self.handler.clone());
        let mut server = hickory_server::ServerFuture::new(adapter);
        
        // 绑定 UDP 端口
        let udp_socket = UdpSocket::bind(self.bind_addr).await?;
        info!("DNS server UDP listening on {}", self.bind_addr);
        server.register_socket(udp_socket);
        
        // 绑定 TCP 端口
        let tcp_listener = TcpListener::bind(self.bind_addr).await?;
        info!("DNS server TCP listening on {}", self.bind_addr);
        server.register_listener(tcp_listener, std::time::Duration::from_secs(10));
        
        // 运行服务器直到收到停止信号
        let shutdown_rx = self.shutdown_rx.take()
            .expect("DNS 服务器已经启动");
            
        tokio::select! {
            _ = server.block_until_done() => {
                info!("DNS server task completed");
                Ok(())
            }
            _ = shutdown_rx => {
                info!("DNS server received shutdown signal");
                Ok(())
            }
        }
    }
}

#[async_trait::async_trait]
impl IntoSubsystem<AppError> for DnsServer {
    async fn run(mut self, subsys: SubsystemHandle) -> Result<(), AppError> {
        let adapter = HandlerAdapter::new(self.handler.clone());
        let mut server = hickory_server::ServerFuture::new(adapter);
        
        // 绑定 UDP 端口
        let udp_socket = UdpSocket::bind(self.bind_addr).await?;
        info!("DNS server UDP listening on {}", self.bind_addr);
        server.register_socket(udp_socket);
        
        // 绑定 TCP 端口
        let tcp_listener = TcpListener::bind(self.bind_addr).await?;
        info!("DNS server TCP listening on {}", self.bind_addr);
        server.register_listener(tcp_listener, std::time::Duration::from_secs(10));
        
        // 运行服务器直到收到停止信号
        let shutdown_rx = self.shutdown_rx.take()
            .expect("DNS 服务器已经启动");
            
        tokio::select! {
            _ = server.block_until_done() => {
                info!("DNS server task completed");
                Ok(())
            }
            _ = shutdown_rx => {
                info!("DNS server received shutdown signal");
                Ok(())
            }
            _ = subsys.on_shutdown_requested() => {
                info!("Received subsystem shutdown request, stopping DNS server");
                self.shutdown();
                Ok(())
            }
        }
    }
}
