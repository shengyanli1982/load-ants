use crate::{
    config::{DoHContentType, DoHMethod, UpstreamServerConfig},
    error::AppError,
    r#const::http_headers,
    upstream::{http_client::HttpClient, json::JsonConverter},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hickory_proto::op::Message;
use reqwest_middleware::ClientWithMiddleware;

pub struct DoHClient<'a> {
    client: &'a ClientWithMiddleware,
    json_converter: JsonConverter,
}

impl<'a> DoHClient<'a> {
    pub fn new(client: &'a ClientWithMiddleware) -> Self {
        Self {
            client,
            json_converter: JsonConverter,
        }
    }

    // 发送DoH请求的入口方法
    pub async fn send_request(
        &self,
        query: &Message,
        server: &UpstreamServerConfig,
    ) -> Result<Message, AppError> {
        // 根据配置的方法选择GET或POST
        match &server.method {
            DoHMethod::Get => self.send_doh_request_get(query, server).await,
            DoHMethod::Post => self.send_doh_request_post(query, server).await,
        }
    }

    // 发送DoH POST请求
    async fn send_doh_request_post(
        &self,
        query: &Message,
        server: &UpstreamServerConfig,
    ) -> Result<Message, AppError> {
        // 创建请求URL
        let url = server.url.clone();

        // 根据内容类型处理
        match server.content_type {
            DoHContentType::Message => {
                // 将DNS查询编码为二进制数据
                let query_data = query.to_vec()?;

                // 创建POST请求
                let mut request = self
                    .client
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
                request = HttpClient::add_auth_to_request(request, &server.auth)?;

                // 发送请求并返回响应体
                let response_data = HttpClient::send_request(request).await?;

                // 解析二进制响应为DNS消息
                let mut message = Message::from_vec(&response_data)?;

                // 复制请求ID
                message.set_id(query.id());

                Ok(message)
            }
            DoHContentType::Json => {
                // 将DNS查询转换为JSON
                let json_data = self.json_converter.message_to_json(query)?;
                let json_string = serde_json::to_string(&json_data)?;

                // 创建POST请求
                let request = self
                    .client
                    .post(url)
                    .header(http_headers::ACCEPT, http_headers::content_types::DNS_JSON)
                    .header(
                        http_headers::CONTENT_TYPE,
                        http_headers::content_types::DNS_JSON,
                    )
                    .body(json_string);

                // 添加认证信息
                let request = HttpClient::add_auth_to_request(request, &server.auth)?;

                // 发送请求并返回响应体
                let response_data = HttpClient::send_request(request).await?;

                // 解析JSON响应为DNS消息
                self.json_converter.json_to_message(&response_data, query)
            }
        }
    }

    // 发送DoH GET请求
    async fn send_doh_request_get(
        &self,
        query: &Message,
        server: &UpstreamServerConfig,
    ) -> Result<Message, AppError> {
        // 创建请求URL
        let mut url = server.url.clone();

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
                let mut request = self.client.get(url).header(
                    http_headers::ACCEPT,
                    http_headers::content_types::DNS_MESSAGE,
                );

                // 添加认证信息
                request = HttpClient::add_auth_to_request(request, &server.auth)?;

                // 发送请求并返回响应体
                let response_data = HttpClient::send_request(request).await?;

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
                let mut request = self
                    .client
                    .get(url)
                    .header(http_headers::ACCEPT, http_headers::content_types::DNS_JSON);

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
