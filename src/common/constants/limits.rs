pub mod shutdown_timeout {
    // 默认值
    pub const DEFAULT: u64 = 30;
    // 最小值
    pub const MIN: u64 = 1;
    // 最大值
    pub const MAX: u64 = 120;
}

// 缓存配置限制
pub mod cache_limits {
    // 默认缓存大小
    pub const DEFAULT_SIZE: usize = 10000;
    // 最小缓存大小
    pub const MIN_SIZE: usize = 10;
    // 最大缓存大小
    pub const MAX_SIZE: usize = 1000000;
    // 默认负面缓存TTL值（秒）
    pub const DEFAULT_NEGATIVE_TTL: u32 = 300;
    // 最小TTL值（秒）
    pub const MIN_TTL: u32 = 1;
    // 最大TTL值（秒）
    pub const MAX_TTL: u32 = 86400;
}

// HTTP客户端配置限制
pub mod http_client_limits {
    // 默认连接超时（秒）
    pub const DEFAULT_CONNECT_TIMEOUT: u64 = 3;
    // 最小连接超时（秒）
    pub const MIN_CONNECT_TIMEOUT: u64 = 1;
    // 最大连接超时（秒）
    pub const MAX_CONNECT_TIMEOUT: u64 = 120;
    // 默认请求超时（秒）
    pub const DEFAULT_REQUEST_TIMEOUT: u64 = 5;
    // 最小请求超时（秒）
    pub const MIN_REQUEST_TIMEOUT: u64 = 1;
    // 最大请求超时（秒）
    pub const MAX_REQUEST_TIMEOUT: u64 = 1200;
    // 默认空闲超时（秒）
    pub const DEFAULT_IDLE_TIMEOUT: u64 = 10;
    // 最小空闲超时（秒）
    pub const MIN_IDLE_TIMEOUT: u64 = 5;
    // 最大空闲超时（秒）
    pub const MAX_IDLE_TIMEOUT: u64 = 1800;
    // 默认keepalive时间（秒）
    pub const DEFAULT_KEEPALIVE: u32 = 30;
    // 最小keepalive时间（秒）
    pub const MIN_KEEPALIVE: u32 = 5;
    // 最大keepalive时间（秒）
    pub const MAX_KEEPALIVE: u32 = 600;
}

// DNS Client（传统 UDP/TCP 上游）配置限制
pub mod dns_client_limits {
    // 默认连接超时（秒）
    pub const DEFAULT_CONNECT_TIMEOUT: u64 = 2;
    // 最小连接超时（秒）
    pub const MIN_CONNECT_TIMEOUT: u64 = 1;
    // 最大连接超时（秒）
    pub const MAX_CONNECT_TIMEOUT: u64 = 120;
    // 默认请求超时（秒）
    pub const DEFAULT_REQUEST_TIMEOUT: u64 = 3;
    // 最小请求超时（秒）
    pub const MIN_REQUEST_TIMEOUT: u64 = 1;
    // 最大请求超时（秒）
    pub const MAX_REQUEST_TIMEOUT: u64 = 1200;
    // 默认 prefer_tcp
    pub const DEFAULT_PREFER_TCP: bool = false;
    // 默认 tcp_reconnect
    pub const DEFAULT_TCP_RECONNECT: bool = true;
}

// Bootstrap DNS（DoH upstream hostname 解析）配置限制
pub mod bootstrap_dns_limits {
    // 默认请求超时（秒）
    pub const DEFAULT_TIMEOUT: u64 = 5;
    // 最小请求超时（秒）
    pub const MIN_TIMEOUT: u64 = 1;
    // 最大请求超时（秒）
    pub const MAX_TIMEOUT: u64 = 30;

    // 默认缓存TTL（秒）
    pub const DEFAULT_CACHE_TTL: u64 = 300;
    // 最小缓存TTL（秒）
    pub const MIN_CACHE_TTL: u64 = 0;
    // 最大缓存TTL（秒）
    pub const MAX_CACHE_TTL: u64 = 86400;

    // 默认 prefer_ipv6
    pub const DEFAULT_PREFER_IPV6: bool = false;
    // 默认 use_system_resolver（当 bootstrap_dns 块存在但未显式配置时）
    pub const DEFAULT_USE_SYSTEM_RESOLVER: bool = false;
}

// 重试配置限制
pub mod retry_limits {
    // 默认重试次数
    pub const DEFAULT_ATTEMPTS: u32 = 3;
    // 最小重试次数
    pub const MIN_ATTEMPTS: u32 = 1;
    // 最大重试次数
    pub const MAX_ATTEMPTS: u32 = 100;
    // 默认重试延迟（秒）
    pub const DEFAULT_DELAY: u32 = 2;
    // 最小重试延迟（秒）
    pub const MIN_DELAY: u32 = 1;
    // 最大重试延迟（秒）
    pub const MAX_DELAY: u32 = 120;
}

// 权重配置限制
pub mod weight_limits {
    // 最小权重值
    pub const MIN_WEIGHT: u32 = 1;
    // 最大权重值
    pub const MAX_WEIGHT: u32 = 65535;
}

// 远程规则文件大小限制
pub mod remote_rule_limits {
    // 默认最大文件大小（字节）- 10MB
    pub const DEFAULT_MAX_SIZE: usize = 10 * 1024 * 1024;
    // 最小文件大小（字节）- 1KB
    pub const MIN_SIZE: usize = 1024;
    // 最大文件大小（字节）- 50MB
    pub const MAX_SIZE: usize = 50 * 1024 * 1024;
}

// 端口限制
pub mod timeout_limits {
    // 最小超时
    pub const MIN_TIMEOUT: u64 = 1;
    // 最大超时
    pub const MAX_TIMEOUT: u64 = 65535;
}

//
// 指标标签常量
//

// 协议类型标签
