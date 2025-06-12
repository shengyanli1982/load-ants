use crate::balancer::{LoadBalancer, RandomBalancer, RoundRobinBalancer, WeightedBalancer};
use crate::config::{
    AuthConfig, AuthType, DoHContentType, DoHMethod, HttpClientConfig, LoadBalancingStrategy,
    RetryConfig, UpstreamGroupConfig, UpstreamServerConfig,
};
use crate::error::{AppError, HttpClientError, InvalidProxyConfig};
use crate::metrics::METRICS;
use crate::r#const::{error_labels, http_headers, retry_limits, upstream_labels};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hickory_proto::rr::rdata as HickoryRData;
use hickory_proto::{
    op::{Message, MessageType, ResponseCode},
    rr::{Name, RData, Record, RecordType},
};
use reqwest::Url;
use reqwest_middleware::ClientWithMiddleware;
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use retry_policies::Jitter;
use serde_json::{json, Value as JsonValue};
use std::{
    collections::HashMap,
    net::{Ipv4Addr, Ipv6Addr},
    sync::Arc,
    time::Duration,
};
use tracing::{debug, error, info, warn};

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
        for UpstreamGroupConfig {
            name,
            strategy,
            servers,
            retry,
            proxy,
        } in groups
        {
            let lb: Arc<dyn LoadBalancer> = match strategy {
                LoadBalancingStrategy::RoundRobin => Arc::new(RoundRobinBalancer::new(servers)),
                LoadBalancingStrategy::Weighted => Arc::new(WeightedBalancer::new(servers)),
                LoadBalancingStrategy::Random => Arc::new(RandomBalancer::new(servers)),
            };

            // 创建该组的HTTP客户端
            let client = Self::create_http_client(&http_config, proxy.as_deref(), retry.as_ref())?;
            group_clients.insert(name.clone(), client);

            group_map.insert(name, lb);
        }

        info!("Initialized {} upstream groups", group_map.len());

        Ok(Self {
            groups: group_map,
            group_clients,
        })
    }

    // 创建HTTP客户端
    fn create_http_client(
        config: &HttpClientConfig,
        proxy: Option<&str>,
        retry_config: Option<&RetryConfig>,
    ) -> Result<ClientWithMiddleware, AppError> {
        debug!(
            "Creating HTTP client for upstream, config: {:?}, proxy: {:?}, retry_config: {:?}",
            config, proxy, retry_config
        );

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
                AppError::InvalidProxy(InvalidProxyConfig(format!(
                    "Proxy configuration error: {}",
                    e
                )))
            })?);
        }

        // 创建基础HTTP客户端
        let client = client_builder.build().map_err(|e| {
            AppError::HttpError(HttpClientError(format!(
                "Failed to create HTTP client: {}",
                e
            )))
        })?;

        // 配置重试策略（根据组的重试配置）
        let middleware_client = if let Some(retry) = retry_config {
            // 使用指数退避策略，基于组的重试配置
            let retry_policy = ExponentialBackoff::builder()
                // 设置重试时间间隔的上下限
                .retry_bounds(
                    Duration::from_secs(retry_limits::MIN_DELAY as u64),
                    Duration::from_secs(retry_limits::MAX_DELAY as u64),
                )
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
            reqwest_middleware::ClientBuilder::new(client).build()
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
                METRICS
                    .upstream_errors_total()
                    .with_label_values(&[
                        error_labels::SELECT_ERROR,
                        group_name,
                        upstream_labels::UNKNOWN,
                    ])
                    .inc();

                return Err(e);
            }
        };

        debug!("Selected upstream server: {}", server.url);

        // 记录上游请求指标
        METRICS
            .upstream_requests_total()
            .with_label_values(&[group_name, &server.url])
            .inc();

        // 记录开始时间
        let start_time = std::time::Instant::now();

        // 发送请求（通过reqwest-retry中间件处理重试）
        match self.send_doh_request(query, server, group_name).await {
            Ok(response) => {
                // 记录上游请求耗时
                let duration = start_time.elapsed();
                METRICS
                    .upstream_duration_seconds()
                    .with_label_values(&[group_name, &server.url])
                    .observe(duration.as_secs_f64());

                Ok(response)
            }
            Err(e) => {
                error!("Upstream request failed: {} - {}", server.url, e);

                // 报告上游失败
                load_balancer.report_failure(server).await;

                // 记录上游错误指标
                METRICS
                    .upstream_errors_total()
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
        match &server.method {
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
            AppError::Upstream(format!("Invalid upstream URL: {} - {}", server.url, e))
        })?;

        // 根据内容类型处理
        match server.content_type {
            DoHContentType::Message => {
                // 将DNS查询编码为二进制数据
                let query_data = query.to_vec()?;

                // 创建POST请求
                let mut request = client
                    .post(url)
                    .header(
                        http_headers::ACCEPT,
                        http_headers::content_types::DNS_MESSAGE,
                    )
                    .header(
                        http_headers::CONTENT_TYPE,
                        http_headers::content_types::DNS_MESSAGE,
                    )
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
            }
            DoHContentType::Json => {
                // 将DNS查询转换为JSON
                let json_data = self.message_to_json(query)?;
                let json_string = serde_json::to_string(&json_data)?;

                // 创建POST请求
                let request = client
                    .post(url)
                    .header(http_headers::ACCEPT, http_headers::content_types::DNS_JSON)
                    .header(
                        http_headers::CONTENT_TYPE,
                        http_headers::content_types::DNS_JSON,
                    )
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
            AppError::Upstream(format!("Invalid upstream URL: {} - {}", server.url, e))
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
                let mut request = client.get(url).header(
                    http_headers::ACCEPT,
                    http_headers::content_types::DNS_MESSAGE,
                );

                // 添加认证信息
                request = self.add_auth_to_middleware_request(request, &server.auth)?;

                // 发送请求并返回响应体
                let response_data = self.send_middleware_request(request).await?;

                // 解析二进制响应为DNS消息
                let mut message = Message::from_vec(&response_data)?;

                // 复制请求ID
                message.set_id(query.id());

                Ok(message)
            }
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
                    .header(http_headers::ACCEPT, http_headers::content_types::DNS_JSON);

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
        auth: &Option<AuthConfig>,
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
                    req.header(
                        http_headers::AUTHORIZATION,
                        format!("{}{}", http_headers::auth::BEARER_PREFIX, token),
                    )
                }
            };
        }

        Ok(req)
    }

    // 发送middleware请求并读取响应体
    async fn send_middleware_request(
        &self,
        request: reqwest_middleware::RequestBuilder,
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
    // https://developers.google.com/speed/public-dns/docs/doh/json
    fn message_to_json(&self, query: &Message) -> Result<JsonValue, AppError> {
        // 创建一个JSON对象以发送给DoH服务器
        let query_param = match query.queries().first() {
            Some(q) => q,
            None => return Err(AppError::Internal("DNS query is empty".to_string())),
        };

        // 基于Google DNS-over-HTTPS JSON API格式
        let mut json_data = json!({
            "name": query_param.name().to_string(),
            "type": u16::from(query_param.query_type()),
        });

        // 可选参数: 当查询类别不是IN(1)时启用DNSSEC
        if u16::from(query_param.query_class()) != 1 {
            // do参数: DNSSEC OK 标志
            json_data["do"] = json!(true);
        }

        // cd参数: Checking Disabled 标志，默认为false (启用DNSSEC验证)
        json_data["cd"] = json!(false);

        // 不添加edns_client_subnet参数，使用默认值
        // 可选: 添加 random_padding 参数以使所有请求大小相同
        // 此处不添加content-type参数，由调用方在HTTP头中设置

        Ok(json_data)
    }

    // 解析DNS JSON响应为DNS消息
    // https://developers.google.com/speed/public-dns/docs/doh/json
    fn json_to_message(&self, json_response: &[u8], query: &Message) -> Result<Message, AppError> {
        // 解析JSON响应
        let json: JsonValue = serde_json::from_slice(json_response)
            .map_err(|e| AppError::Upstream(format!("Failed to parse JSON response: {}", e)))?;

        // 创建新的DNS响应消息
        let mut response = Message::new();
        response.set_id(query.id());
        response.set_message_type(MessageType::Response);
        response.set_op_code(query.op_code());

        // 处理DNS标志位
        // TC - 是否截断
        if let Some(tc) = json.get("TC").and_then(|tc| tc.as_bool()) {
            response.set_truncated(tc);
        }

        // RD - 递归期望
        if let Some(rd) = json.get("RD").and_then(|rd| rd.as_bool()) {
            response.set_recursion_desired(rd);
        } else {
            // 默认使用查询中的递归期望设置
            response.set_recursion_desired(query.recursion_desired());
        }

        // RA - 递归可用
        if let Some(ra) = json.get("RA").and_then(|ra| ra.as_bool()) {
            response.set_recursion_available(ra);
        } else {
            // 默认为true，Google Public DNS总是支持递归
            response.set_recursion_available(true);
        }

        // AD - 认证数据标志 (DNSSEC验证)
        if let Some(ad) = json.get("AD").and_then(|ad| ad.as_bool()) {
            response.set_authentic_data(ad);
        }

        // CD - 禁用检查标志
        if let Some(cd) = json.get("CD").and_then(|cd| cd.as_bool()) {
            response.set_checking_disabled(cd);
        }

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

        // 如果状态不是成功，可能不需要进一步处理（但处理Question部分）
        if response.response_code() != ResponseCode::NoError {
            // 即使有错误，Question部分也可能存在
            if let Some(questions) = json.get("Question").and_then(|q| q.as_array()) {
                for question in questions {
                    // 只处理第一个Question，因为DNS消息通常只有一个查询
                    if let (Some(name), Some(q_type)) = (
                        question.get("name").and_then(|n| n.as_str()),
                        question.get("type").and_then(|t| t.as_u64()),
                    ) {
                        // 尝试解析域名
                        if let Ok(domain) = Name::parse(name, None) {
                            let record_type = RecordType::from(q_type as u16);
                            // 重新创建查询
                            let query_record = hickory_proto::op::Query::query(domain, record_type);
                            response.add_query(query_record);
                        }
                    }
                }
            }

            // 如果JSON包含Comment字段，记录为调试信息
            if let Some(comment) = json.get("Comment").and_then(|c| c.as_str()) {
                debug!("DNS JSON response comment: {}", comment);
            }

            return Ok(response);
        }

        // 处理记录的辅助函数
        let parse_record = |record: &JsonValue, section: &str| -> Option<Record> {
            // 获取记录的基本属性
            let name = record.get("name").and_then(|n| n.as_str())?;
            let r_type = record.get("type").and_then(|t| t.as_u64())?;
            let ttl = record.get("TTL").and_then(|t| t.as_u64())?;
            let data = record.get("data").and_then(|d| d.as_str())?;

            // 解析域名
            let name = match Name::parse(name, None) {
                Ok(n) => n,
                Err(e) => {
                    warn!("Failed to parse {} record name {}: {}", section, name, e);
                    return None;
                }
            };

            // 记录类型
            let record_type = RecordType::from(r_type as u16);

            // 根据记录类型创建适当的RData
            match record_type {
                RecordType::A => match data.parse::<Ipv4Addr>() {
                    Ok(addr) => {
                        let octets = addr.octets();
                        let rdata =
                            HickoryRData::A::new(octets[0], octets[1], octets[2], octets[3]);
                        Some(Record::from_rdata(name, ttl as u32, RData::A(rdata)))
                    }
                    Err(e) => {
                        warn!("Failed to parse A record data {}: {}", data, e);
                        None
                    }
                },
                RecordType::AAAA => match data.parse::<Ipv6Addr>() {
                    Ok(addr) => {
                        let segments = addr.segments();
                        let rdata = HickoryRData::AAAA::new(
                            segments[0],
                            segments[1],
                            segments[2],
                            segments[3],
                            segments[4],
                            segments[5],
                            segments[6],
                            segments[7],
                        );
                        Some(Record::from_rdata(name, ttl as u32, RData::AAAA(rdata)))
                    }
                    Err(e) => {
                        warn!("Failed to parse AAAA record data {}: {}", data, e);
                        None
                    }
                },
                RecordType::CNAME => match Name::parse(data, None) {
                    Ok(target) => {
                        let rdata = HickoryRData::CNAME(target);
                        Some(Record::from_rdata(name, ttl as u32, RData::CNAME(rdata)))
                    }
                    Err(e) => {
                        warn!("Failed to parse CNAME record data {}: {}", data, e);
                        None
                    }
                },
                RecordType::MX => {
                    // MX记录格式通常为"优先级 主机名"
                    let parts: Vec<&str> = data.split_whitespace().collect();
                    if parts.len() >= 2 {
                        match (parts[0].parse::<u16>(), Name::parse(parts[1], None)) {
                            (Ok(preference), Ok(exchange)) => {
                                let rdata = HickoryRData::MX::new(preference, exchange);
                                Some(Record::from_rdata(name, ttl as u32, RData::MX(rdata)))
                            }
                            _ => {
                                warn!("Failed to parse MX record data {}", data);
                                None
                            }
                        }
                    } else {
                        warn!("Invalid MX record format {}", data);
                        None
                    }
                }
                RecordType::TXT => {
                    // TXT记录可能包含多个引号部分
                    // 处理诸如 "v=spf1 -all" 或 "k=rsa; p=MIGfMA0..." "更多数据"

                    // 去除首尾引号，处理多部分TXT记录
                    let mut txt_data = String::new();
                    let mut in_quotes = false;
                    let mut escaped = false;

                    for c in data.chars() {
                        match c {
                            '"' if !escaped => {
                                in_quotes = !in_quotes;
                                // 不将引号添加到实际数据中
                            }
                            '\\' if !escaped => {
                                escaped = true;
                            }
                            _ => {
                                if in_quotes || (!in_quotes && c != ' ') {
                                    txt_data.push(c);
                                }
                                escaped = false;
                            }
                        }
                    }

                    // 创建TXT记录
                    let txt_strings = vec![txt_data];
                    let rdata = HickoryRData::TXT::new(txt_strings);
                    Some(Record::from_rdata(name, ttl as u32, RData::TXT(rdata)))
                }
                RecordType::SRV => {
                    // SRV记录格式为"优先级 权重 端口 目标主机名"
                    let parts: Vec<&str> = data.split_whitespace().collect();
                    if parts.len() >= 4 {
                        match (
                            parts[0].parse::<u16>(),     // 优先级
                            parts[1].parse::<u16>(),     // 权重
                            parts[2].parse::<u16>(),     // 端口
                            Name::parse(parts[3], None), // 目标主机名
                        ) {
                            (Ok(priority), Ok(weight), Ok(port), Ok(target)) => {
                                let rdata = HickoryRData::SRV::new(priority, weight, port, target);
                                Some(Record::from_rdata(name, ttl as u32, RData::SRV(rdata)))
                            }
                            _ => {
                                warn!("Failed to parse SRV record data {}", data);
                                None
                            }
                        }
                    } else {
                        warn!("Invalid SRV record format {}", data);
                        None
                    }
                }
                RecordType::PTR => match Name::parse(data, None) {
                    Ok(ptrdname) => {
                        let rdata = HickoryRData::PTR(ptrdname);
                        Some(Record::from_rdata(name, ttl as u32, RData::PTR(rdata)))
                    }
                    Err(e) => {
                        warn!("Failed to parse PTR record data {}: {}", data, e);
                        None
                    }
                },
                RecordType::NS => match Name::parse(data, None) {
                    Ok(target) => {
                        let rdata = HickoryRData::NS(target);
                        Some(Record::from_rdata(name, ttl as u32, RData::NS(rdata)))
                    }
                    Err(e) => {
                        warn!("Failed to parse NS record data {}: {}", data, e);
                        None
                    }
                },
                _ => {
                    // 对于其他记录类型，尝试作为未知记录处理
                    warn!("Unsupported record type: {:?}, data: {}", record_type, data);
                    None
                }
            }
        };

        // 处理Answer部分
        if let Some(answers) = json.get("Answer").and_then(|a| a.as_array()) {
            for answer in answers {
                if let Some(record) = parse_record(answer, "Answer") {
                    response.add_answer(record);
                }
            }
        }

        // 处理Authority部分
        if let Some(authority) = json.get("Authority").and_then(|a| a.as_array()) {
            for auth in authority {
                if let Some(record) = parse_record(auth, "Authority") {
                    response.add_name_server(record);
                }
            }
        }

        // 处理Additional部分
        if let Some(additional) = json.get("Additional").and_then(|a| a.as_array()) {
            for add in additional {
                if let Some(record) = parse_record(add, "Additional") {
                    response.add_additional(record);
                }
            }
        }

        // 处理edns_client_subnet字段
        if let Some(ecs) = json.get("edns_client_subnet").and_then(|e| e.as_str()) {
            debug!("EDNS Client Subnet from DNS JSON response: {}", ecs);
            // 这里可以添加EDNS处理代码，但由于复杂性，我们只记录不处理
        }

        Ok(response)
    }
}
