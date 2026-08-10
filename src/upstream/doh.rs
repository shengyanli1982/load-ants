use crate::{
    config::{DoHContentType, DoHMethod, DoHUpstreamServerConfig},
    error::AppError,
    r#const::http_headers,
    upstream::{http_client::HttpClient, json::JsonConverter},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hickory_proto::{
    op::Message,
    serialize::binary::{BinEncodable, BinEncoder},
};
use reqwest_middleware::ClientWithMiddleware;

pub struct DoHClient<'a> {
    client: &'a ClientWithMiddleware,
    json_converter: JsonConverter,
}

impl<'a> DoHClient<'a> {
    /// 创建一个复用现有 HTTP 客户端的 DoH 客户端包装器。
    pub fn new(client: &'a ClientWithMiddleware) -> Self {
        Self {
            client,
            json_converter: JsonConverter,
        }
    }

    // 发送 DoH 请求的统一入口。
    pub async fn send_request(
        &self,
        query: &Message,
        server: &DoHUpstreamServerConfig,
    ) -> Result<Message, AppError> {
        // 根据配置的方法选择GET或POST
        match &server.method {
            DoHMethod::Get => self.send_doh_request_get(query, server).await,
            DoHMethod::Post => self.send_doh_request_post(query, server).await,
        }
    }

    // 发送 DoH POST 请求。
    async fn send_doh_request_post(
        &self,
        query: &Message,
        server: &DoHUpstreamServerConfig,
    ) -> Result<Message, AppError> {
        let original_id = query.id();

        // 创建请求URL
        let url = server.url.clone();

        // 根据内容类型处理
        match server.content_type {
            DoHContentType::Message => {
                // RFC 8484 Section 4.1: DoH客户端发送请求时应将DNS ID设置为0，
                // 以最大化HTTP缓存友好性。
                let mut query_with_zero_id = query.clone();
                query_with_zero_id.set_id(0);

                // 创建一个可复用的缓冲区。
                let mut buffer = Vec::with_capacity(512);
                let mut encoder = BinEncoder::new(&mut buffer);
                query_with_zero_id.emit(&mut encoder)?;

                // 创建POST请求
                let mut request = self
                    .client
                    .post(url)
                    .header("Accept", http_headers::content_types::DNS_MESSAGE)
                    .header("Content-Type", http_headers::content_types::DNS_MESSAGE)
                    .body(buffer);

                // 添加认证信息
                request = HttpClient::add_auth_to_request(request, &server.auth)?;

                // 发送请求并返回响应体
                let response_data = HttpClient::send_request(request).await?;

                // 解析二进制响应为DNS消息
                let mut message = Message::from_vec(&response_data)?;

                // 恢复原始请求ID
                message.set_id(original_id);

                Ok(message)
            }
            DoHContentType::Json => {
                // JSON 格式不支持 POST 方法，直接返回错误。
                Err(AppError::Upstream(
                    "JSON content type is not supported with POST method. Use GET method instead."
                        .to_string(),
                ))
            }
        }
    }

    // 发送 DoH GET 请求。
    async fn send_doh_request_get(
        &self,
        query: &Message,
        server: &DoHUpstreamServerConfig,
    ) -> Result<Message, AppError> {
        let original_id = query.id();

        // 创建请求URL
        let mut url = server.url.clone();

        // 根据内容类型处理
        match server.content_type {
            DoHContentType::Message => {
                // RFC 8484 Section 4.1: DoH客户端发送请求时应将DNS ID设置为0，
                // 以最大化HTTP缓存友好性。
                let mut query_with_zero_id = query.clone();
                query_with_zero_id.set_id(0);

                // 创建一个可复用的缓冲区
                let mut buffer = Vec::with_capacity(2048);
                let mut encoder = BinEncoder::new(&mut buffer);
                query_with_zero_id.emit(&mut encoder)?;

                // 按 base64url 规则编码查询报文。
                let b64_data = URL_SAFE_NO_PAD.encode(&buffer);

                // 添加查询参数
                url.query_pairs_mut().append_pair("dns", &b64_data);

                // 创建GET请求
                let mut request = self
                    .client
                    .get(url)
                    .header("Accept", http_headers::content_types::DNS_MESSAGE);

                // 添加认证信息
                request = HttpClient::add_auth_to_request(request, &server.auth)?;

                // 发送请求并返回响应体
                let response_data = HttpClient::send_request(request).await?;

                // 解析二进制响应为DNS消息
                let mut message = Message::from_vec(&response_data)?;

                // 恢复原始请求ID
                message.set_id(original_id);

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

                let dnssec_ok = query
                    .extensions()
                    .as_ref()
                    .map(|e| e.flags().dnssec_ok)
                    .unwrap_or(false);
                if dnssec_ok {
                    url.query_pairs_mut().append_pair("do", "true");
                }

                if query.checking_disabled() {
                    url.query_pairs_mut().append_pair("cd", "true");
                }

                // 创建GET请求
                let mut request = self
                    .client
                    .get(url)
                    .header("Accept", http_headers::content_types::DNS_JSON);

                // 添加认证信息
                request = HttpClient::add_auth_to_request(request, &server.auth)?;

                // 发送请求并返回响应体
                let response_data = HttpClient::send_request(request).await?;

                // 解析JSON响应为DNS消息
                self.json_converter.json_to_message(&response_data, query)
            }
        }
    }
}
