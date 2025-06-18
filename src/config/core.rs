use crate::config::validate_socket_addr;
use crate::r#const::{cache_limits, http_client_limits, server_defaults, timeout_limits};
use serde::{Deserialize, Serialize};
use validator::{Validate, ValidationError};

// HTTP客户端配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate)]
#[serde(rename_all = "lowercase")]
pub struct HttpClientConfig {
    // 连接超时（秒）
    #[validate(range(
        min = http_client_limits::MIN_CONNECT_TIMEOUT,
        max = http_client_limits::MAX_CONNECT_TIMEOUT,
        message = "Connection timeout must be between {} and {} seconds"
    ))]
    pub connect_timeout: u64,
    // 请求超时（秒）
    #[validate(range(
        min = http_client_limits::MIN_REQUEST_TIMEOUT,
        max = http_client_limits::MAX_REQUEST_TIMEOUT,
        message = "Request timeout must be between {} and {} seconds"
    ))]
    pub request_timeout: u64,
    // 空闲连接超时（秒）（可选）
    pub idle_timeout: Option<u64>,
    // TCP Keepalive（秒）（可选）
    pub keepalive: Option<u32>,
    // HTTP用户代理（可选）
    pub agent: Option<String>,
}

impl Default for HttpClientConfig {
    fn default() -> Self {
        Self {
            connect_timeout: http_client_limits::DEFAULT_CONNECT_TIMEOUT,
            request_timeout: http_client_limits::DEFAULT_REQUEST_TIMEOUT,
            idle_timeout: Some(http_client_limits::DEFAULT_IDLE_TIMEOUT),
            keepalive: Some(http_client_limits::DEFAULT_KEEPALIVE),
            agent: None,
        }
    }
}

// 为 HttpClientConfig 实现自定义验证逻辑
impl HttpClientConfig {
    // 验证可选字段
    pub fn validate_optional_fields(&self) -> Result<(), ValidationError> {
        // 验证空闲超时
        if let Some(idle_timeout) = self.idle_timeout {
            if idle_timeout < http_client_limits::MIN_IDLE_TIMEOUT
                || idle_timeout > http_client_limits::MAX_IDLE_TIMEOUT
            {
                return Err(ValidationError::new("idle_timeout_out_of_range"));
            }
        }

        // 验证keepalive
        if let Some(keepalive) = self.keepalive {
            if keepalive < http_client_limits::MIN_KEEPALIVE
                || keepalive > http_client_limits::MAX_KEEPALIVE
            {
                return Err(ValidationError::new("keepalive_out_of_range"));
            }
        }

        Ok(())
    }
}

// 服务器配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate)]
#[serde(rename_all = "lowercase")]
pub struct ServerConfig {
    // UDP监听地址
    #[validate(custom(
        function = "validate_socket_addr",
        message = "Invalid UDP listen address format"
    ))]
    pub listen_udp: String,
    // TCP监听地址
    #[validate(custom(
        function = "validate_socket_addr",
        message = "Invalid TCP listen address format"
    ))]
    pub listen_tcp: String,
    // HTTP监听地址
    #[validate(custom(
        function = "validate_socket_addr",
        message = "Invalid HTTP listen address format"
    ))]
    pub listen_http: Option<String>,
    // TCP连接空闲超时（秒）
    #[serde(default = "default_tcp_timeout")]
    #[validate(range(
        min = timeout_limits::MIN_TIMEOUT,
        max = timeout_limits::MAX_TIMEOUT,
        message = "TCP timeout must be between 1 and 65535 seconds"
    ))]
    pub tcp_timeout: u64,
    // HTTP连接空闲超时（秒）
    #[serde(default = "default_http_timeout")]
    #[validate(range(
        min = timeout_limits::MIN_TIMEOUT,
        max = timeout_limits::MAX_TIMEOUT,
        message = "HTTP timeout must be between 1 and 65535 seconds"
    ))]
    pub http_timeout: u64,
}

fn default_tcp_timeout() -> u64 {
    server_defaults::DEFAULT_TCP_TIMEOUT
}

fn default_http_timeout() -> u64 {
    server_defaults::DEFAULT_HTTP_TIMEOUT
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen_udp: server_defaults::DEFAULT_DNS_LISTEN.to_string(),
            listen_tcp: server_defaults::DEFAULT_DNS_LISTEN.to_string(),
            listen_http: None,
            tcp_timeout: default_tcp_timeout(),
            http_timeout: default_http_timeout(),
        }
    }
}

// 自定义验证函数 - 验证缓存TTL关系
pub fn validate_cache_ttl(cache: &CacheConfig) -> Result<(), ValidationError> {
    if cache.enabled && cache.min_ttl > cache.max_ttl {
        return Err(ValidationError::new("min_ttl_greater_than_max_ttl"));
    }
    Ok(())
}

// 缓存配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate)]
#[validate(schema(
    function = "validate_cache_ttl",
    message = "Minimum TTL cannot be greater than maximum TTL"
))]
#[serde(rename_all = "lowercase")]
pub struct CacheConfig {
    // 是否启用缓存
    pub enabled: bool,
    // 最大缓存条目数
    #[validate(range(
        min = cache_limits::MIN_SIZE,
        max = cache_limits::MAX_SIZE,
        message = "Cache size must be between 10 and 1000000"
    ))]
    pub max_size: usize,
    // 最小TTL（秒）
    #[validate(range(
        min = cache_limits::MIN_TTL,
        max = cache_limits::MAX_TTL,
        message = "Minimum TTL must be between 1 and 86400 seconds"
    ))]
    pub min_ttl: u32,
    // 最大TTL（秒）
    #[validate(range(
        min = cache_limits::MIN_TTL,
        max = cache_limits::MAX_TTL,
        message = "Maximum TTL must be between 1 and 86400 seconds"
    ))]
    pub max_ttl: u32,
    // 负面缓存TTL（秒）
    #[validate(range(
        min = cache_limits::MIN_TTL,
        max = cache_limits::MAX_TTL,
        message = "Negative cache TTL must be between 1 and 86400 seconds"
    ))]
    pub negative_ttl: u32,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_size: cache_limits::DEFAULT_SIZE,
            min_ttl: cache_limits::MIN_TTL,
            max_ttl: cache_limits::MAX_TTL,
            negative_ttl: cache_limits::DEFAULT_NEGATIVE_TTL,
        }
    }
}

// 管理服务器配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate)]
#[serde(rename_all = "lowercase")]
pub struct AdminConfig {
    // 管理服务器监听地址
    #[validate(custom(
        function = "validate_socket_addr",
        message = "Invalid admin server listen address format"
    ))]
    pub listen: String,
}

impl Default for AdminConfig {
    fn default() -> Self {
        Self {
            listen: server_defaults::DEFAULT_ADMIN_LISTEN.to_string(),
        }
    }
}
