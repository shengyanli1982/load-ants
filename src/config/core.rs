use crate::config::{validate_idle_timeout, validate_keepalive, validate_socket_addr};
use crate::r#const::{
    cache_limits, dns_client_limits, http_client_limits, rate_limit_defaults, server_defaults,
    timeout_limits,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use validator::{Validate, ValidationError};

// DNS Client 配置（传统 UDP/TCP 上游）
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub struct DnsClientConfig {
    #[validate(range(
        min = "dns_client_limits::MIN_CONNECT_TIMEOUT",
        max = "dns_client_limits::MAX_CONNECT_TIMEOUT",
        message = "Connection timeout must be between {} and {} seconds"
    ))]
    pub connect_timeout: u64,
    #[validate(range(
        min = "dns_client_limits::MIN_REQUEST_TIMEOUT",
        max = "dns_client_limits::MAX_REQUEST_TIMEOUT",
        message = "Request timeout must be between {} and {} seconds"
    ))]
    pub request_timeout: u64,
    #[serde(default = "default_dns_client_prefer_tcp")]
    pub prefer_tcp: bool,
    #[serde(default = "default_dns_client_idle_connection_timeout")]
    #[validate(range(
        min = "dns_client_limits::MIN_IDLE_CONNECTION_TIMEOUT",
        max = "dns_client_limits::MAX_IDLE_CONNECTION_TIMEOUT",
        message = "Idle connection timeout must be between {} and {} seconds"
    ))]
    pub idle_connection_timeout: u64,
    #[serde(default = "default_dns_client_tcp_reconnect")]
    pub tcp_reconnect: bool,
    /// TCP 连接池最大连接数（所有上游共享）
    #[serde(default = "default_max_tcp_connections")]
    #[validate(range(
        min = "dns_client_limits::MIN_MAX_TCP_CONNECTIONS",
        max = "dns_client_limits::MAX_MAX_TCP_CONNECTIONS",
        message = "Max TCP connections must be between 1 and 65535"
    ))]
    pub max_tcp_connections: usize,
}

fn default_dns_client_prefer_tcp() -> bool {
    dns_client_limits::DEFAULT_PREFER_TCP
}

fn default_dns_client_idle_connection_timeout() -> u64 {
    dns_client_limits::DEFAULT_IDLE_CONNECTION_TIMEOUT
}

fn default_dns_client_tcp_reconnect() -> bool {
    dns_client_limits::DEFAULT_TCP_RECONNECT
}

fn default_max_tcp_connections() -> usize {
    dns_client_limits::DEFAULT_MAX_TCP_CONNECTIONS
}

impl Default for DnsClientConfig {
    fn default() -> Self {
        Self {
            connect_timeout: dns_client_limits::DEFAULT_CONNECT_TIMEOUT,
            request_timeout: dns_client_limits::DEFAULT_REQUEST_TIMEOUT,
            prefer_tcp: default_dns_client_prefer_tcp(),
            idle_connection_timeout: default_dns_client_idle_connection_timeout(),
            tcp_reconnect: default_dns_client_tcp_reconnect(),
            max_tcp_connections: default_max_tcp_connections(),
        }
    }
}

// HTTP客户端配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub struct HttpClientConfig {
    // 连接超时（秒）
    #[validate(range(
        min = "http_client_limits::MIN_CONNECT_TIMEOUT",
        max = "http_client_limits::MAX_CONNECT_TIMEOUT",
        message = "Connection timeout must be between {} and {} seconds"
    ))]
    pub connect_timeout: u64,
    // 请求超时（秒）
    #[validate(range(
        min = "http_client_limits::MIN_REQUEST_TIMEOUT",
        max = "http_client_limits::MAX_REQUEST_TIMEOUT",
        message = "Request timeout must be between {} and {} seconds"
    ))]
    pub request_timeout: u64,
    // 空闲连接超时（秒）（可选）
    #[validate(custom(
        function = "validate_idle_timeout",
        message = "Idle timeout must be between minimum and maximum values"
    ))]
    pub idle_timeout: Option<u64>,
    // TCP Keepalive（秒）（可选）
    #[validate(custom(
        function = "validate_keepalive",
        message = "Keepalive must be between minimum and maximum values"
    ))]
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

// 速率限制配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub struct RateLimitConfig {
    #[validate(range(
        min = "rate_limit_defaults::MIN_MAX_REQUESTS_PER_SECOND",
        max = "rate_limit_defaults::MAX_MAX_REQUESTS_PER_SECOND",
        message = "Max requests per second must be between 1 and 100000"
    ))]
    pub max_requests_per_second: u32,
    #[serde(default = "default_per_ip_max_requests_per_second")]
    #[validate(range(
        min = "rate_limit_defaults::MIN_MAX_REQUESTS_PER_SECOND",
        max = "rate_limit_defaults::MAX_MAX_REQUESTS_PER_SECOND",
        message = "Per-IP max requests per second must be between 1 and 100000"
    ))]
    pub per_ip_max_requests_per_second: u32,
}

fn default_per_ip_max_requests_per_second() -> u32 {
    rate_limit_defaults::DEFAULT_PER_IP_MAX_REQUESTS_PER_SECOND
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests_per_second: rate_limit_defaults::DEFAULT_MAX_REQUESTS_PER_SECOND,
            per_ip_max_requests_per_second:
                rate_limit_defaults::DEFAULT_PER_IP_MAX_REQUESTS_PER_SECOND,
        }
    }
}

// 服务器配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate, JsonSchema)]
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
    // TLS 证书文件路径（可选，配置后 DoH 入站使用 HTTPS）
    pub tls_cert: Option<String>,
    // TLS 私钥文件路径（可选，配置后 DoH 入站使用 HTTPS）
    pub tls_key: Option<String>,
    // TCP连接空闲超时（秒）
    #[serde(default = "default_tcp_timeout")]
    #[validate(range(
        min = "timeout_limits::MIN_TIMEOUT",
        max = "timeout_limits::MAX_TIMEOUT",
        message = "TCP timeout must be between 1 and 65535 seconds"
    ))]
    pub tcp_timeout: u64,
    // HTTP连接空闲超时（秒）
    #[serde(default = "default_http_timeout")]
    #[validate(range(
        min = "timeout_limits::MIN_TIMEOUT",
        max = "timeout_limits::MAX_TIMEOUT",
        message = "HTTP timeout must be between 1 and 65535 seconds"
    ))]
    pub http_timeout: u64,
    #[serde(default)]
    #[validate(nested)]
    pub rate_limit: Option<RateLimitConfig>,
    #[serde(default, rename = "udp")]
    #[validate(nested)]
    pub udp_socket: UdpSocketConfig,
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
            tls_cert: None,
            tls_key: None,
            tcp_timeout: default_tcp_timeout(),
            http_timeout: default_http_timeout(),
            rate_limit: None,
            udp_socket: UdpSocketConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub struct UdpSocketConfig {
    #[serde(default = "default_udp_recv_buffer")]
    pub recv_buffer: usize,
    #[serde(default = "default_udp_send_buffer")]
    pub send_buffer: usize,
    #[serde(default = "default_udp_socket_count")]
    #[validate(range(
        min = 1,
        max = 256,
        message = "UDP socket count must be between 1 and 256"
    ))]
    pub socket_count: usize,
}

fn default_udp_recv_buffer() -> usize {
    server_defaults::DEFAULT_UDP_RECV_BUFFER
}

fn default_udp_send_buffer() -> usize {
    server_defaults::DEFAULT_UDP_SEND_BUFFER
}

fn default_udp_socket_count() -> usize {
    server_defaults::DEFAULT_UDP_SOCKET_COUNT
}

impl Default for UdpSocketConfig {
    fn default() -> Self {
        Self {
            recv_buffer: default_udp_recv_buffer(),
            send_buffer: default_udp_send_buffer(),
            socket_count: default_udp_socket_count(),
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
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate, JsonSchema)]
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
        min = "cache_limits::MIN_SIZE",
        max = "cache_limits::MAX_SIZE",
        message = "Cache size must be between 10 and 1000000"
    ))]
    pub max_size: usize,
    // 最小TTL（秒）
    #[validate(range(
        min = "cache_limits::MIN_TTL",
        max = "cache_limits::MAX_TTL",
        message = "Minimum TTL must be between 1 and 86400 seconds"
    ))]
    pub min_ttl: u32,
    // 最大TTL（秒）
    #[validate(range(
        min = "cache_limits::MIN_TTL",
        max = "cache_limits::MAX_TTL",
        message = "Maximum TTL must be between 1 and 86400 seconds"
    ))]
    pub max_ttl: u32,
    // 负面缓存TTL（秒）
    #[validate(range(
        min = "cache_limits::MIN_TTL",
        max = "cache_limits::MAX_TTL",
        message = "Negative cache TTL must be between 1 and 86400 seconds"
    ))]
    pub negative_ttl: u32,
    #[serde(default)]
    #[validate(range(
        min = 0,
        max = "cache_limits::MAX_STALE_WHILE_REVALIDATE",
        message = "Stale-while-revalidate must be between 0 and 86400 seconds"
    ))]
    pub stale_while_revalidate: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_size: cache_limits::DEFAULT_SIZE,
            min_ttl: cache_limits::MIN_TTL,
            max_ttl: cache_limits::MAX_TTL,
            negative_ttl: cache_limits::DEFAULT_NEGATIVE_TTL,
            stale_while_revalidate: cache_limits::DEFAULT_STALE_WHILE_REVALIDATE,
        }
    }
}

// 管理服务器认证配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub struct AdminAuthConfig {
    pub token: String,
}

// 管理服务器配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub struct AdminConfig {
    // 管理服务器监听地址
    #[validate(custom(
        function = "validate_socket_addr",
        message = "Invalid admin server listen address format"
    ))]
    pub listen: String,
    // 认证配置（可选，配置后管理端点需要 Bearer token）
    pub auth: Option<AdminAuthConfig>,
}

impl Default for AdminConfig {
    fn default() -> Self {
        Self {
            listen: server_defaults::DEFAULT_ADMIN_LISTEN.to_string(),
            auth: None,
        }
    }
}
