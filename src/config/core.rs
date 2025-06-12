use crate::r#const::{cache_limits, http_client_limits, server_defaults};
use serde::{Deserialize, Serialize};

// HTTP客户端配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct HttpClientConfig {
    // 连接超时（秒）
    pub connect_timeout: u64,
    // 请求超时（秒）
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

// 服务器配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    // UDP监听地址
    pub listen_udp: String,
    // TCP监听地址
    pub listen_tcp: String,
    // TCP连接空闲超时（秒）
    #[serde(default = "default_tcp_timeout")]
    pub tcp_timeout: u64,
}

fn default_tcp_timeout() -> u64 {
    server_defaults::DEFAULT_TCP_TIMEOUT
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen_udp: server_defaults::DEFAULT_DNS_LISTEN.to_string(),
            listen_tcp: server_defaults::DEFAULT_DNS_LISTEN.to_string(),
            tcp_timeout: default_tcp_timeout(),
        }
    }
}

// 缓存配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct CacheConfig {
    // 是否启用缓存
    pub enabled: bool,
    // 最大缓存条目数
    pub max_size: usize,
    // 最小TTL（秒）
    pub min_ttl: u32,
    // 最大TTL（秒）
    pub max_ttl: u32,
    // 负面缓存TTL（秒）
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
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct AdminConfig {
    // 管理服务器监听地址
    pub listen: String,
}

impl Default for AdminConfig {
    fn default() -> Self {
        Self {
            listen: server_defaults::DEFAULT_ADMIN_LISTEN.to_string(),
        }
    }
}
