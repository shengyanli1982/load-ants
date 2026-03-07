use crate::config::{validate_idle_timeout, validate_keepalive, validate_socket_addr};
use crate::r#const::{
    bootstrap_dns_limits, cache_limits, dns_client_limits, http_client_limits, server_defaults,
    timeout_limits,
};
use serde::{Deserialize, Serialize};
use validator::{Validate, ValidationError};

// DNS 客户端配置（传统 UDP/TCP 上游）
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub struct DnsConfig {
    #[validate(range(
        min = dns_client_limits::MIN_CONNECT_TIMEOUT,
        max = dns_client_limits::MAX_CONNECT_TIMEOUT,
        message = "Connection timeout must be between {} and {} seconds"
    ))]
    pub connect_timeout: u64,

    #[validate(range(
        min = dns_client_limits::MIN_REQUEST_TIMEOUT,
        max = dns_client_limits::MAX_REQUEST_TIMEOUT,
        message = "Request timeout must be between {} and {} seconds"
    ))]
    pub request_timeout: u64,

    #[serde(default = "default_dns_prefer_tcp")]
    pub prefer_tcp: bool,

    #[serde(default = "default_dns_tcp_reconnect")]
    pub tcp_reconnect: bool,
}

fn default_dns_prefer_tcp() -> bool {
    dns_client_limits::DEFAULT_PREFER_TCP
}

fn default_dns_tcp_reconnect() -> bool {
    dns_client_limits::DEFAULT_TCP_RECONNECT
}

impl Default for DnsConfig {
    fn default() -> Self {
        Self {
            connect_timeout: dns_client_limits::DEFAULT_CONNECT_TIMEOUT,
            request_timeout: dns_client_limits::DEFAULT_REQUEST_TIMEOUT,
            prefer_tcp: default_dns_prefer_tcp(),
            tcp_reconnect: default_dns_tcp_reconnect(),
        }
    }
}

// Bootstrap DNS 配置：用于解析 upstream/proxy hostname（不用于普通转发）
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub struct BootstrapDnsConfig {
    #[validate(length(min = 1, message = "Bootstrap groups cannot be empty"))]
    pub groups: Vec<String>,

    #[serde(default = "default_bootstrap_timeout")]
    #[validate(range(
        min = bootstrap_dns_limits::MIN_TIMEOUT,
        max = bootstrap_dns_limits::MAX_TIMEOUT,
        message = "Bootstrap timeout must be between 1 and 30 seconds"
    ))]
    pub timeout: u64,

    #[serde(default = "default_bootstrap_cache_ttl")]
    #[validate(range(
        min = bootstrap_dns_limits::MIN_CACHE_TTL,
        max = bootstrap_dns_limits::MAX_CACHE_TTL,
        message = "Bootstrap cache_ttl must be between 0 and 86400 seconds"
    ))]
    pub cache_ttl: u64,

    #[serde(default = "default_bootstrap_prefer_ipv6")]
    pub prefer_ipv6: bool,

    #[serde(default = "default_bootstrap_use_system_resolver")]
    pub use_system_resolver: bool,
}

fn default_bootstrap_timeout() -> u64 {
    bootstrap_dns_limits::DEFAULT_TIMEOUT
}

fn default_bootstrap_cache_ttl() -> u64 {
    bootstrap_dns_limits::DEFAULT_CACHE_TTL
}

fn default_bootstrap_prefer_ipv6() -> bool {
    bootstrap_dns_limits::DEFAULT_PREFER_IPV6
}

fn default_bootstrap_use_system_resolver() -> bool {
    bootstrap_dns_limits::DEFAULT_USE_SYSTEM_RESOLVER
}

// HTTP 客户端配置（全局）
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub struct HttpConfig {
    #[validate(range(
        min = http_client_limits::MIN_CONNECT_TIMEOUT,
        max = http_client_limits::MAX_CONNECT_TIMEOUT,
        message = "Connection timeout must be between {} and {} seconds"
    ))]
    pub connect_timeout: u64,

    #[validate(range(
        min = http_client_limits::MIN_REQUEST_TIMEOUT,
        max = http_client_limits::MAX_REQUEST_TIMEOUT,
        message = "Request timeout must be between {} and {} seconds"
    ))]
    pub request_timeout: u64,

    #[validate(custom(
        function = "validate_idle_timeout",
        message = "Idle timeout must be between minimum and maximum values"
    ))]
    pub idle_timeout: Option<u64>,

    #[validate(custom(
        function = "validate_keepalive",
        message = "Keepalive must be between minimum and maximum values"
    ))]
    pub keepalive: Option<u32>,

    pub user_agent: Option<String>,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            connect_timeout: http_client_limits::DEFAULT_CONNECT_TIMEOUT,
            request_timeout: http_client_limits::DEFAULT_REQUEST_TIMEOUT,
            idle_timeout: Some(http_client_limits::DEFAULT_IDLE_TIMEOUT),
            keepalive: Some(http_client_limits::DEFAULT_KEEPALIVE),
            user_agent: None,
        }
    }
}

// 监听配置（服务端）
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub struct ListenersConfig {
    #[validate(custom(
        function = "validate_socket_addr",
        message = "Invalid UDP listen address format"
    ))]
    pub udp: String,

    #[validate(custom(
        function = "validate_socket_addr",
        message = "Invalid TCP listen address format"
    ))]
    pub tcp: String,

    #[validate(custom(
        function = "validate_socket_addr",
        message = "Invalid DoH listen address format"
    ))]
    pub doh: Option<String>,

    #[serde(default = "default_tcp_idle_timeout")]
    #[validate(range(
        min = timeout_limits::MIN_TIMEOUT,
        max = timeout_limits::MAX_TIMEOUT,
        message = "TCP idle timeout must be between 1 and 65535 seconds"
    ))]
    pub tcp_idle_timeout: u64,

    #[serde(default = "default_http_idle_timeout")]
    #[validate(range(
        min = timeout_limits::MIN_TIMEOUT,
        max = timeout_limits::MAX_TIMEOUT,
        message = "HTTP idle timeout must be between 1 and 65535 seconds"
    ))]
    pub http_idle_timeout: u64,
}

fn default_tcp_idle_timeout() -> u64 {
    server_defaults::DEFAULT_TCP_TIMEOUT
}

fn default_http_idle_timeout() -> u64 {
    server_defaults::DEFAULT_HTTP_TIMEOUT
}

impl Default for ListenersConfig {
    fn default() -> Self {
        Self {
            udp: server_defaults::DEFAULT_DNS_LISTEN.to_string(),
            tcp: server_defaults::DEFAULT_DNS_LISTEN.to_string(),
            doh: None,
            tcp_idle_timeout: default_tcp_idle_timeout(),
            http_idle_timeout: default_http_idle_timeout(),
        }
    }
}

// 缓存 TTL 子配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub struct CacheTtlConfig {
    #[validate(range(
        min = cache_limits::MIN_TTL,
        max = cache_limits::MAX_TTL,
        message = "Minimum TTL must be between 1 and 86400 seconds"
    ))]
    pub min: u32,

    #[validate(range(
        min = cache_limits::MIN_TTL,
        max = cache_limits::MAX_TTL,
        message = "Maximum TTL must be between 1 and 86400 seconds"
    ))]
    pub max: u32,

    #[validate(range(
        min = cache_limits::MIN_TTL,
        max = cache_limits::MAX_TTL,
        message = "Negative cache TTL must be between 1 and 86400 seconds"
    ))]
    pub negative: u32,
}

impl Default for CacheTtlConfig {
    fn default() -> Self {
        Self {
            min: cache_limits::MIN_TTL,
            max: cache_limits::MAX_TTL,
            negative: cache_limits::DEFAULT_NEGATIVE_TTL,
        }
    }
}

pub fn validate_cache_ttl(cache: &CacheConfig) -> Result<(), ValidationError> {
    if cache.enabled && cache.ttl.min > cache.ttl.max {
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
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub struct CacheConfig {
    pub enabled: bool,

    #[validate(range(
        min = cache_limits::MIN_SIZE,
        max = cache_limits::MAX_SIZE,
        message = "Cache size must be between 10 and 1000000"
    ))]
    pub size: usize,

    #[validate(nested)]
    pub ttl: CacheTtlConfig,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            size: cache_limits::DEFAULT_SIZE,
            ttl: CacheTtlConfig::default(),
        }
    }
}

// 管理服务器配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub struct AdminConfig {
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
