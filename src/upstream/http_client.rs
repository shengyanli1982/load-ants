use crate::config::{AuthConfig, AuthType, HttpClientConfig, RetryConfig};
use crate::error::{AppError, HttpClientError, InvalidProxyConfig};
use crate::r#const::{http_headers, retry_limits};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, RequestBuilder};
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use retry_policies::Jitter;
use std::time::Duration;
use tracing::debug;

pub struct HttpClient;

impl HttpClient {
    // 创建HTTP客户端
    pub fn create(
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

            ClientBuilder::new(client)
                .with(RetryTransientMiddleware::new_with_policy(retry_policy))
                .build()
        } else {
            // 不进行重试
            ClientBuilder::new(client).build()
        };

        Ok(middleware_client)
    }

    // 处理认证头添加
    pub fn add_auth_to_request(
        request: RequestBuilder,
        auth: &Option<AuthConfig>,
    ) -> Result<RequestBuilder, AppError> {
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
    pub async fn send_request(request: RequestBuilder) -> Result<bytes::Bytes, AppError> {
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
}
