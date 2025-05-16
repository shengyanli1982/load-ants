use crate::config::{AuthConfig, AuthType, DoHContentType, DoHMethod, HttpClientConfig, LoadBalancingStrategy, RetryConfig, UpstreamGroupConfig, UpstreamServerConfig};
use crate::error::AppError;
use crate::metrics::METRICS;
use crate::r#const::{error_labels, upstream_labels};
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hickory_proto::{
    op::{Message, MessageType, ResponseCode},
    rr::{Name, Record, RecordType, RData},
};
use rand::{seq::SliceRandom, thread_rng};
use reqwest::Url;
use reqwest_middleware::ClientWithMiddleware;
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use serde_json::{json, Value as JsonValue};
use std::{
    collections::HashMap,
    net::{Ipv4Addr, Ipv6Addr},
    sync::{Arc, atomic::{AtomicUsize, Ordering}},
    time::Duration,
};
use tracing::{debug, error, info, warn};
use hickory_proto::rr::rdata as HickoryRData;
use retry_policies::{Jitter};

// 负载均衡器特性
#[async_trait]
pub trait LoadBalancer: Send + Sync {
    // 选择一个上游服务器
    async fn select_server(&self) -> Result<UpstreamServerConfig, AppError>;
    
    // 报告服务器失败
    async fn report_failure(&self, server: &UpstreamServerConfig);
}

// 轮询负载均衡器
pub struct RoundRobinBalancer {
    // 服务器列表
    servers: Vec<UpstreamServerConfig>,
    // 当前索引（原子操作）
    current: AtomicUsize,
}

impl RoundRobinBalancer {
    // 创建新的轮询负载均衡器
    pub fn new(servers: Vec<UpstreamServerConfig>) -> Self {
        Self {
            servers,
            current: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl LoadBalancer for RoundRobinBalancer {
    async fn select_server(&self) -> Result<UpstreamServerConfig, AppError> {
        if self.servers.is_empty() {
            return Err(AppError::NoUpstreamAvailable);
        }
        
        let current = self.current.fetch_add(1, Ordering::SeqCst) % self.servers.len();
        Ok(self.servers[current].clone())
    }
    
    async fn report_failure(&self, _server: &UpstreamServerConfig) {
        // 轮询策略下不需要特殊处理失败
    }
}

// 加权轮询负载均衡器
pub struct WeightedBalancer {
    // 服务器列表，按权重复制
    servers: Vec<UpstreamServerConfig>,
    // 当前索引（原子操作）
    current: AtomicUsize,
}

impl WeightedBalancer {
    // 创建新的加权轮询负载均衡器
    pub fn new(servers: Vec<UpstreamServerConfig>) -> Self {
        // 根据权重复制服务器
        let mut weighted_servers = Vec::with_capacity(servers.iter().map(|s| s.weight as usize).sum());
        
        for server in servers {
            // 对于每个服务器，按其权重添加多个副本
            for _ in 0..server.weight {
                weighted_servers.push(server.clone());
            }
        }
        
        Self {
            servers: weighted_servers,
            current: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl LoadBalancer for WeightedBalancer {
    async fn select_server(&self) -> Result<UpstreamServerConfig, AppError> {
        if self.servers.is_empty() {
            return Err(AppError::NoUpstreamAvailable);
        }
        
        let current = self.current.fetch_add(1, Ordering::SeqCst) % self.servers.len();
        Ok(self.servers[current].clone())
    }
    
    async fn report_failure(&self, _server: &UpstreamServerConfig) {
        // 加权轮询策略下不需要特殊处理失败
    }
}

// 随机负载均衡器
pub struct RandomBalancer {
    // 服务器列表
    servers: Vec<UpstreamServerConfig>,
}

impl RandomBalancer {
    // 创建新的随机负载均衡器
    pub fn new(servers: Vec<UpstreamServerConfig>) -> Self {
        Self { servers }
    }
}

#[async_trait]
impl LoadBalancer for RandomBalancer {
    async fn select_server(&self) -> Result<UpstreamServerConfig, AppError> {
        if self.servers.is_empty() {
            return Err(AppError::NoUpstreamAvailable);
        }
        
        let server = self.servers.choose(&mut thread_rng()).ok_or(AppError::NoUpstreamAvailable)?;
        Ok(server.clone())
    }
    
    async fn report_failure(&self, _server: &UpstreamServerConfig) {
        // 随机策略下不需要特殊处理失败
    }
}

// 上游管理器
pub struct UpstreamManager {
    // 上游组负载均衡器
    groups: HashMap<String, Arc<dyn LoadBalancer>>,
    // 上游组客户端
    group_clients: HashMap<String, ClientWithMiddleware>,
}

impl UpstreamManager {
    // 创建新的上游管理器
    pub async fn new(
        groups: Vec<UpstreamGroupConfig>,
        http_config: HttpClientConfig,
    ) -> Result<Self, AppError> {
        let mut group_map = HashMap::with_capacity(groups.len());
        let mut group_clients = HashMap::new();
        
        // 为每个组创建负载均衡器和HTTP客户端
        for group in groups {
            let lb: Arc<dyn LoadBalancer> = match group.strategy {
                LoadBalancingStrategy::RoundRobin => {
                    Arc::new(RoundRobinBalancer::new(group.servers.clone()))
                }
                LoadBalancingStrategy::Weighted => {
                    Arc::new(WeightedBalancer::new(group.servers.clone()))
                }
                LoadBalancingStrategy::Random => {
                    Arc::new(RandomBalancer::new(group.servers.clone()))
                }
            };
            
            // 创建该组的HTTP客户端
            let client = Self::create_http_client(&http_config, group.proxy.as_deref(), group.retry.as_ref())?;
            group_clients.insert(group.name.clone(), client);
            
            group_map.insert(group.name, lb);
        }
        
        info!("Initialized {} upstream groups", group_map.len());
        
        Ok(Self {
            groups: group_map,
            group_clients,
        })
    }

    // 创建HTTP客户端
    fn create_http_client(config: &HttpClientConfig, proxy: Option<&str>, retry_config: Option<&RetryConfig>) -> Result<ClientWithMiddleware, AppError> {
        debug!("Creating HTTP client, config: {:?}, proxy: {:?}, retry_config: {:?}", config, proxy, retry_config);
        
        // 创建客户端构建器
        let mut client_builder = reqwest::ClientBuilder::new()
            .danger_accept_invalid_certs(true) // 允许无效证书，用于内部自签名证书
            .connect_timeout(Duration::from_secs(config.connect_timeout))
            .timeout(Duration::from_secs(config.request_timeout));
        
        // 配置TCP keepalive
        if let Some(ref keepalive) = config.keepalive {
            client_builder = client_builder.tcp_keepalive(Duration::from_secs(*keepalive as u64));
        }
        
        // 配置空闲连接超时
        if let Some(idle_timeout) = config.idle_timeout {
            client_builder = client_builder.pool_idle_timeout(Duration::from_secs(idle_timeout));
        }
        
        // 配置用户代理
        if let Some(ref agent) = config.agent {
            client_builder = client_builder.user_agent(agent);
        }
        
        // 配置代理
        if let Some(proxy_url) = proxy {
            client_builder = client_builder.proxy(reqwest::Proxy::all(proxy_url).map_err(|e| {
                AppError::InvalidProxy(InvalidProxyConfig(format!("Proxy configuration error: {}", e)))
            })?);
        }
        
        // 创建基础HTTP客户端
        let client = client_builder.build().map_err(|e| {
            AppError::HttpError(HttpClientError(format!("Failed to create HTTP client: {}", e)))
        })?;
        
        // 配置重试策略（根据组的重试配置）
        let middleware_client = if let Some(retry) = retry_config {
            // 使用指数退避策略，基于组的重试配置
            let retry_policy = ExponentialBackoff::builder()
                // 设置指数退避的基数
                .base(retry.delay)
                // 使用有界抖动来避免多个客户端同时重试
                .jitter(Jitter::Bounded)
                // 配置最大重试次数
                .build_with_max_retries(retry.attempts);
            
            reqwest_middleware::ClientBuilder::new(client)
                .with(RetryTransientMiddleware::new_with_policy(retry_policy))
                .build()
        } else {
            // 不进行重试
            reqwest_middleware::ClientBuilder::new(client)
                .build()
        };
        
        Ok(middleware_client)
    }

    // 转发查询到指定上游组
    pub async fn forward(&self, query: &Message, group_name: &str) -> Result<Message, AppError> {
        debug!("Forwarding request to upstream group: {}", group_name);
        
        // 获取上游组的负载均衡器
        let load_balancer = match self.groups.get(group_name) {
            Some(lb) => lb,
            None => {
                error!("Upstream group not found: {}", group_name);
                return Err(AppError::UpstreamGroupNotFound(group_name.to_string()));
            }
        };

        // 选择一个上游服务器
        let server = match load_balancer.select_server().await {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to select upstream server: {}", e);
                
                // 记录上游错误指标
                METRICS.upstream_errors_total()
                    .with_label_values(&[error_labels::SELECT_ERROR, group_name, upstream_labels::UNKNOWN])
                    .inc();
                
                return Err(e);
            }
        };
        
        debug!("Selected upstream server: {}", server.url);
        
        // 记录上游请求指标
        METRICS.upstream_requests_total()
            .with_label_values(&[group_name, &server.url])
            .inc();
            
        // 记录开始时间
        let start_time = std::time::Instant::now();
        
        // 发送请求（通过reqwest-retry中间件处理重试）
        match self.send_doh_request(query, &server, group_name).await {
            Ok(response) => {
                // 记录上游请求耗时
                let duration = start_time.elapsed();
                METRICS.upstream_duration_seconds()
                    .with_label_values(&[group_name, &server.url])
                    .observe(duration.as_secs_f64());
                
                Ok(response)
            }
            Err(e) => {
                error!("Upstream request failed: {} - {}", server.url, e);
                
                // 报告上游失败
                load_balancer.report_failure(&server).await;
                
                // 记录上游错误指标
                METRICS.upstream_errors_total()
                    .with_label_values(&[error_labels::REQUEST_ERROR, group_name, &server.url])
                    .inc();
                
                Err(e)
            }
        }
    }

    // 发送DoH请求
    async fn send_doh_request(
        &self,
        query: &Message,
        server: &UpstreamServerConfig,
        group_name: &str,
    ) -> Result<Message, AppError> {
        // 根据配置的方法选择GET或POST
        match server.method.clone() {
            DoHMethod::Get => self.send_doh_request_get(query, server, group_name).await,
            DoHMethod::Post => self.send_doh_request_post(query, server, group_name).await,
        }
    }

    // 发送DoH POST请求
    async fn send_doh_request_post(
        &self,
        query: &Message,
        server: &UpstreamServerConfig,
        group_name: &str,
    ) -> Result<Message, AppError> {
        // 获取组的HTTP客户端
        let client = match self.group_clients.get(group_name) {
            Some(c) => c,
            None => {
                error!("HTTP client not found for group: {}", group_name);
                return Err(AppError::UpstreamGroupNotFound(group_name.to_string()));
            }
        };

        // 创建请求URL
        let url = Url::parse(&server.url).map_err(|e| {
            AppError::Upstream(format!("无效的上游URL: {} - {}", server.url, e))
        })?;
        
        // 根据内容类型处理
        match server.content_type {
            DoHContentType::Message => {
                // 将DNS查询编码为二进制数据
                let query_data = query.to_vec()?;
                
                // 创建POST请求
                let mut request = client
                    .post(url)
                    .header("Accept", "application/dns-message")
                    .header("Content-Type", "application/dns-message")
                    .body(query_data);
                
                // 添加认证信息
                request = self.add_auth_to_middleware_request(request, &server.auth)?;
                
                // 发送请求并返回响应体
                let response_data = self.send_middleware_request(request).await?;
                
                // 解析二进制响应为DNS消息
                let mut message = Message::from_vec(&response_data)?;
                
                // 复制请求ID
                message.set_id(query.id());
                
                Ok(message)
            },
            DoHContentType::Json => {
                // 将DNS查询转换为JSON
                let json_data = self.message_to_json(query)?;
                let json_string = serde_json::to_string(&json_data)?;
                
                // 创建POST请求
                let request = client
                    .post(url)
                    .header("Accept", "application/dns-json")
                    .header("Content-Type", "application/dns-json")
                    .body(json_string);
                
                // 添加认证信息
                let request = self.add_auth_to_middleware_request(request, &server.auth)?;
                
                // 发送请求并返回响应体
                let response_data = self.send_middleware_request(request).await?;
                
                // 解析JSON响应为DNS消息
                self.json_to_message(&response_data, query)
            }
        }
    }

    // 发送DoH GET请求
    async fn send_doh_request_get(
        &self,
        query: &Message,
        server: &UpstreamServerConfig,
        group_name: &str,
    ) -> Result<Message, AppError> {
        // 获取组的HTTP客户端
        let client = match self.group_clients.get(group_name) {
            Some(c) => c,
            None => {
                error!("HTTP client not found for group: {}", group_name);
                return Err(AppError::UpstreamGroupNotFound(group_name.to_string()));
            }
        };
        
        // 创建请求URL
        let mut url = Url::parse(&server.url).map_err(|e| {
            AppError::Upstream(format!("无效的上游URL: {} - {}", server.url, e))
        })?;
        
        // 根据内容类型处理
        match server.content_type {
            DoHContentType::Message => {
                // 将DNS查询编码为二进制数据
                let query_data = query.to_vec()?;
                
                // Base64Url编码
                let b64_data = URL_SAFE_NO_PAD.encode(&query_data);
                
                // 添加查询参数
                url.query_pairs_mut().append_pair("dns", &b64_data);
                
                // 创建GET请求
                let mut request = client
                    .get(url)
                    .header("Accept", "application/dns-message");
                
                // 添加认证信息
                request = self.add_auth_to_middleware_request(request, &server.auth)?;
                
                // 发送请求并返回响应体
                let response_data = self.send_middleware_request(request).await?;
                
                // 解析二进制响应为DNS消息
                let mut message = Message::from_vec(&response_data)?;
                
                // 复制请求ID
                message.set_id(query.id());
                
                Ok(message)
            },
            DoHContentType::Json => {
                // 从查询中提取参数
                let query_param = match query.queries().first() {
                    Some(q) => q,
                    None => return Err(AppError::Internal("DNS query is empty".to_string())),
                };
                
                // 添加查询参数
                url.query_pairs_mut()
                    .append_pair("name", &query_param.name().to_string())
                    .append_pair("type", &(u16::from(query_param.query_type())).to_string());
                
                if u16::from(query_param.query_class()) != 1 {
                    url.query_pairs_mut().append_pair("dnssec_data", "true");
                }
                
                // 创建GET请求
                let mut request = client
                    .get(url)
                    .header("Accept", "application/dns-json");
                
                // 添加认证信息
                request = self.add_auth_to_middleware_request(request, &server.auth)?;
                
                // 发送请求并返回响应体
                let response_data = self.send_middleware_request(request).await?;
                
                // 解析JSON响应为DNS消息
                self.json_to_message(&response_data, query)
            }
        }
    }

    // 处理认证头添加
    fn add_auth_to_middleware_request(
        &self, 
        request: reqwest_middleware::RequestBuilder, 
        auth: &Option<AuthConfig>
    ) -> Result<reqwest_middleware::RequestBuilder, AppError> {
        let mut req = request;
        
        // 添加认证信息（如果有）
        if let Some(ref auth) = auth {
            req = match auth.r#type {
                AuthType::Basic => {
                    let username = auth.username.as_ref().ok_or_else(|| {
                        AppError::Upstream("Missing username for Basic authentication".to_string())
                    })?;
                    let password = auth.password.as_ref().ok_or_else(|| {
                        AppError::Upstream("Missing password for Basic authentication".to_string())
                    })?;
                    req.basic_auth(username, Some(password))
                }
                AuthType::Bearer => {
                    let token = auth.token.as_ref().ok_or_else(|| {
                        AppError::Upstream("Missing token for Bearer authentication".to_string())
                    })?;
                    req.header("Authorization", format!("Bearer {}", token))
                }
            };
        }
        
        Ok(req)
    }

    // 发送middleware请求并读取响应体
    async fn send_middleware_request(
        &self,
        request: reqwest_middleware::RequestBuilder
    ) -> Result<bytes::Bytes, AppError> {
        // 发送请求
        let response = request.send().await?;
        
        // 检查状态码
        if !response.status().is_success() {
            return Err(AppError::Upstream(format!(
                "Upstream server returned error: {}",
                response.status()
            )));
        }
        
        // 读取响应体
        let response_data = response.bytes().await?;
        
        Ok(response_data)
    }

    // 将DNS消息转换为DNS JSON格式
    fn message_to_json(&self, query: &Message) -> Result<JsonValue, AppError> {
        // 创建一个JSON对象以发送给DoH服务器
        let query_param = match query.queries().first() {
            Some(q) => q,
            None => return Err(AppError::Internal("DNS query is empty".to_string())),
        };
        
        // 基于google/cloudflare DNS-over-HTTPS JSON API格式
        Ok(json!({
            "name": query_param.name().to_string(),
            "type": u16::from(query_param.query_type()),
            "dnssec_data": u16::from(query_param.query_class()) != 1, // 非IN类查询可能需要DNSSEC数据
            "do": false,  // 是否需要DNSSEC
            "cd": false,  // 禁用DNSSEC验证
        }))
    }
    
    // 解析DNS JSON响应为DNS消息
    fn json_to_message(&self, json_response: &[u8], query: &Message) -> Result<Message, AppError> {
        // 解析JSON响应
        let json: JsonValue = serde_json::from_slice(json_response)
            .map_err(|e| AppError::Upstream(format!("Failed to parse JSON response: {}", e)))?;
        
        // 创建新的DNS响应消息
        let mut response = Message::new();
        response.set_id(query.id());
        response.set_message_type(MessageType::Response);
        response.set_op_code(query.op_code());
        response.set_recursion_desired(query.recursion_desired());
        response.set_recursion_available(true);
        
        // 设置响应码（默认为NoError）
        response.set_response_code(ResponseCode::NoError);
        
        // 复制查询部分
        for q in query.queries() {
            response.add_query(q.clone());
        }
        
        // 处理Status字段，映射到响应码
        if let Some(status) = json.get("Status").and_then(|s| s.as_u64()) {
            let rcode = match status {
                0 => ResponseCode::NoError,
                1 => ResponseCode::FormErr,
                2 => ResponseCode::ServFail,
                3 => ResponseCode::NXDomain,
                4 => ResponseCode::NotImp,
                5 => ResponseCode::Refused,
                _ => ResponseCode::ServFail,
            };
            response.set_response_code(rcode);
        }
        
        // 如果状态不是成功，可能不需要进一步处理
        if response.response_code() != ResponseCode::NoError {
            return Ok(response);
        }
        
        // 处理Answer部分（记录转换为hickory记录）
        if let Some(answers) = json.get("Answer").and_then(|a| a.as_array()) {
            for answer in answers {
                if let (Some(name), Some(r_type), Some(ttl), Some(data)) = (
                    answer.get("name").and_then(|n| n.as_str()),
                    answer.get("type").and_then(|t| t.as_u64()),
                    answer.get("TTL").and_then(|t| t.as_u64()),
                    answer.get("data").and_then(|d| d.as_str()),
                ) {
                    // 尝试将JSON记录转换为hickory记录
                    let name = match Name::parse(name, None) {
                        Ok(n) => n,
                        Err(e) => {
                            // 记录错误但继续处理其他记录
                            warn!("Failed to parse record name {}: {}", name, e);
                            continue;
                        }
                    };
                    
                    // 根据记录类型创建适当的RData
                    let record_type = RecordType::from(r_type as u16);
                    
                    // 尝试构建记录
                    // 这里简化处理，实际上根据记录类型有不同的解析方式
                    let record = match record_type {
                        RecordType::A => {
                            match data.parse::<Ipv4Addr>() {
                                Ok(addr) => {
                                    let octets = addr.octets();
                                    let rdata = HickoryRData::A::new(octets[0], octets[1], octets[2], octets[3]);
                                    Record::from_rdata(
                                        name.clone(),
                                        ttl as u32,
                                        RData::A(rdata)
                                    )
                                }
                                Err(e) => {
                                    warn!("Failed to parse A record data {}: {}", data, e);
                                    continue;
                                }
                            }
                        }
                        RecordType::AAAA => {
                            match data.parse::<Ipv6Addr>() {
                                Ok(addr) => {
                                    let segments = addr.segments();
                                    let rdata = HickoryRData::AAAA::new(
                                        segments[0], segments[1], segments[2], segments[3],
                                        segments[4], segments[5], segments[6], segments[7]
                                    );
                                    Record::from_rdata(
                                        name.clone(),
                                        ttl as u32,
                                        RData::AAAA(rdata)
                                    )
                                }
                                Err(e) => {
                                    warn!("Failed to parse AAAA record data {}: {}", data, e);
                                    continue;
                                }
                            }
                        }
                        RecordType::CNAME => {
                            match Name::parse(data, None) {
                                Ok(target) => {
                                    let rdata = HickoryRData::CNAME(target);
                                    Record::from_rdata(
                                        name.clone(),
                                        ttl as u32,
                                        RData::CNAME(rdata)
                                    )
                                }
                                Err(e) => {
                                    warn!("Failed to parse CNAME record data {}: {}", data, e);
                                    continue;
                                }
                            }
                        }
                        RecordType::MX => {
                            // MX记录格式通常为"优先级 主机名"
                            let parts: Vec<&str> = data.split_whitespace().collect();
                            if parts.len() >= 2 {
                                match (parts[0].parse::<u16>(), Name::parse(parts[1], None)) {
                                    (Ok(preference), Ok(exchange)) => {
                                        let rdata = HickoryRData::MX::new(preference, exchange);
                                        Record::from_rdata(
                                            name.clone(),
                                            ttl as u32,
                                            RData::MX(rdata)
                                        )
                                    }
                                    _ => {
                                        warn!("Failed to parse MX record data {}", data);
                                        continue;
                                    }
                                }
                            } else {
                                warn!("Invalid MX record format {}", data);
                                continue;
                            }
                        }
                        RecordType::TXT => {
                            let txt_strings = vec![data.to_string()];
                            let rdata = HickoryRData::TXT::new(txt_strings);
                            Record::from_rdata(
                                name.clone(),
                                ttl as u32,
                                RData::TXT(rdata)
                            )
                        }
                        _ => {
                            // 对于其他记录类型，我们可能需要更复杂的解析
                            warn!("Unsupported record type: {:?}", record_type);
                            continue;
                        }
                    };
                    
                    // 添加记录到响应
                    response.add_answer(record);
                }
            }
        }
        
        Ok(response)
    }
}

// 无效的代理配置错误
#[derive(Debug, thiserror::Error)]
#[error("Proxy configuration error: {0}")]
pub struct InvalidProxyConfig(pub String);

// HTTP客户端错误
#[derive(Debug, thiserror::Error)]
#[error("HTTP client error: {0}")]
pub struct HttpClientError(pub String);
