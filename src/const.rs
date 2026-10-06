// 应用常量定义

//
// 配置参数限制常量
//

// 应用关闭等待时间限制
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
    pub const DEFAULT_SIZE: usize = 10000;
    pub const MIN_SIZE: usize = 10;
    pub const MAX_SIZE: usize = 1000000;
    pub const DEFAULT_NEGATIVE_TTL: u32 = 300;
    pub const MIN_TTL: u32 = 1;
    pub const MAX_TTL: u32 = 86400;
    pub const DEFAULT_STALE_WHILE_REVALIDATE: u64 = 0;
    pub const MAX_STALE_WHILE_REVALIDATE: u64 = 86400;
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

// coalesce 合并等待限制
pub mod coalesce_limits {
    // follower 等待时长在 leader 最坏转发总 deadline 之上预留的余量（秒）
    pub const WAIT_MARGIN_SECS: u64 = 1;
    // 无上游组时的兜底总转发 deadline（秒），与 HTTP 默认请求超时保持一致
    pub const FALLBACK_FORWARD_DEADLINE_SECS: u64 =
        super::http_client_limits::DEFAULT_REQUEST_TIMEOUT;
}

// DNS Client（传统 UDP/TCP 上游）配置限制
pub mod dns_client_limits {
    pub const DEFAULT_CONNECT_TIMEOUT: u64 = 2;
    pub const MIN_CONNECT_TIMEOUT: u64 = 1;
    pub const MAX_CONNECT_TIMEOUT: u64 = 120;
    pub const DEFAULT_REQUEST_TIMEOUT: u64 = 3;
    pub const MIN_REQUEST_TIMEOUT: u64 = 1;
    pub const MAX_REQUEST_TIMEOUT: u64 = 1200;
    pub const DEFAULT_PREFER_TCP: bool = false;
    pub const DEFAULT_IDLE_CONNECTION_TIMEOUT: u64 = 30;
    pub const MIN_IDLE_CONNECTION_TIMEOUT: u64 = 5;
    pub const MAX_IDLE_CONNECTION_TIMEOUT: u64 = 1800;
    pub const DEFAULT_TCP_RECONNECT: bool = true;
    pub const CLEANUP_INTERVAL_SECS: u64 = 60;
    pub const DEFAULT_MAX_TCP_CONNECTIONS: usize = 256;
    pub const MIN_MAX_TCP_CONNECTIONS: usize = 1;
    pub const MAX_MAX_TCP_CONNECTIONS: usize = 65535;
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
pub mod protocol_labels {
    // UDP协议
    pub const UDP: &str = "udp";
    // TCP协议
    pub const TCP: &str = "tcp";
    // 未知协议
    pub const UNKNOWN: &str = "unknown";
}

// 处理阶段标签
pub mod processing_labels {
    // 缓存命中
    pub const CACHED: &str = "cached";
    // 解析完成
    pub const RESOLVED: &str = "resolved";

    // DoH 错误类型标签
    pub mod error_types {
        // 上游错误
        pub const UPSTREAM_ERROR: &str = "upstream_error";
        // 消息编码错误
        pub const MESSAGE_ENCODE_ERROR: &str = "message_encode_error";
        // 错误的请求
        pub const BAD_REQUEST: &str = "bad_request";
        // 不支持的媒体类型
        pub const UNSUPPORTED_MEDIA_TYPE: &str = "unsupported_media_type";
        // JSON序列化错误
        pub const JSON_SERIALIZATION_ERROR: &str = "json_serialization_error";
    }
}

// 错误类型标签
pub mod error_labels {
    // 空查询错误
    pub const EMPTY_QUERY: &str = "empty_query";
    // 路由错误
    pub const ROUTE_ERROR: &str = "route_error";
    // 缺少目标
    pub const MISSING_TARGET: &str = "missing_target";
    // 上游错误
    pub const UPSTREAM_ERROR: &str = "upstream_error";
    // 不支持的操作码
    pub const UNSUPPORTED_OPCODE: &str = "unsupported_opcode";
    // 不支持的消息类型
    pub const UNSUPPORTED_MESSAGE_TYPE: &str = "unsupported_message_type";
    // 处理器错误
    pub const HANDLER_ERROR: &str = "handler_error";
    // 选择错误
    pub const SELECT_ERROR: &str = "select_error";
    // 请求错误
    pub const REQUEST_ERROR: &str = "request_error";
}

// 缓存操作标签
pub mod cache_labels {
    pub const HIT: &str = "hit";
    pub const MISS: &str = "miss";
    pub const INSERT_ERROR: &str = "insert_error";
    pub const INSERT: &str = "insert";
    pub const CLEAR: &str = "clear";
    pub const STALE: &str = "stale";
}

pub mod ttl_source_labels {
    pub const ORIGINAL: &str = "original";
    pub const MIN_TTL: &str = "min_ttl";
    pub const MAX_TTL: &str = "max_ttl";
    pub const ADJUSTED: &str = "adjusted";
    pub const NEGATIVE_TTL: &str = "negative_ttl";
}

//
// OPT-2: 有界标签集预解析下标。
// 每个 *_indices 模块与对应 *_labels 模块一一对应：`ALL[i]` 即下标 `i` 的标签值，
// metrics.rs 构造期按 `ALL` 顺序一次性 with_label_values 预解析 child 句柄，
// 热路径以下标直接索引，消除每 op 的 RwLock + 哈希 + Arc clone/drop 查找开销。
// 约束：仅允许收录有界标签维度；ALL 顺序与下标常量必须保持一致
// （tests/metrics_preresolved_export_test.rs 有一致性护栏）。
//

// 协议下标（dns_requests_total / dns_request_duration_seconds 的 protocol 维）
pub mod protocol_indices {
    use super::protocol_labels;
    pub const UDP: usize = 0;
    pub const TCP: usize = 1;
    pub const UNKNOWN: usize = 2;
    pub const ALL: [&str; 3] = [
        protocol_labels::UDP,
        protocol_labels::TCP,
        protocol_labels::UNKNOWN,
    ];
}

// 处理阶段下标（dns_request_processing_duration_seconds 的 processing_stage 维）
pub mod processing_indices {
    use super::processing_labels;
    pub const CACHED: usize = 0;
    pub const RESOLVED: usize = 1;
    pub const ALL: [&str; 2] = [processing_labels::CACHED, processing_labels::RESOLVED];
}

// 错误类型下标（dns_request_errors_total 的 error_type 维）。
// 仅收录该 metric 实际使用的有界值；SELECT_ERROR 只流入 upstream_errors_total
// （无界维度、保持 with_label_values），不在此预解析、不物化零值序列。
pub mod error_indices {
    use super::error_labels;
    pub const EMPTY_QUERY: usize = 0;
    pub const ROUTE_ERROR: usize = 1;
    pub const MISSING_TARGET: usize = 2;
    pub const UPSTREAM_ERROR: usize = 3;
    pub const UNSUPPORTED_OPCODE: usize = 4;
    pub const UNSUPPORTED_MESSAGE_TYPE: usize = 5;
    pub const HANDLER_ERROR: usize = 6;
    pub const REQUEST_ERROR: usize = 7;
    pub const ALL: [&str; 8] = [
        error_labels::EMPTY_QUERY,
        error_labels::ROUTE_ERROR,
        error_labels::MISSING_TARGET,
        error_labels::UPSTREAM_ERROR,
        error_labels::UNSUPPORTED_OPCODE,
        error_labels::UNSUPPORTED_MESSAGE_TYPE,
        error_labels::HANDLER_ERROR,
        error_labels::REQUEST_ERROR,
    ];
}

// 缓存操作下标（cache_operations_total 的 operation 维）
pub mod cache_op_indices {
    use super::cache_labels;
    pub const HIT: usize = 0;
    pub const MISS: usize = 1;
    pub const INSERT_ERROR: usize = 2;
    pub const INSERT: usize = 3;
    pub const CLEAR: usize = 4;
    pub const STALE: usize = 5;
    pub const ALL: [&str; 6] = [
        cache_labels::HIT,
        cache_labels::MISS,
        cache_labels::INSERT_ERROR,
        cache_labels::INSERT,
        cache_labels::CLEAR,
        cache_labels::STALE,
    ];
}

// TTL 来源下标（cache_ttl_seconds 的 source 维）
pub mod ttl_source_indices {
    use super::ttl_source_labels;
    pub const ORIGINAL: usize = 0;
    pub const MIN_TTL: usize = 1;
    pub const MAX_TTL: usize = 2;
    pub const ADJUSTED: usize = 3;
    pub const NEGATIVE_TTL: usize = 4;
    pub const ALL: [&str; 5] = [
        ttl_source_labels::ORIGINAL,
        ttl_source_labels::MIN_TTL,
        ttl_source_labels::MAX_TTL,
        ttl_source_labels::ADJUSTED,
        ttl_source_labels::NEGATIVE_TTL,
    ];
}

// 上游标签
pub mod upstream_labels {
    pub const UNKNOWN: &str = "unknown";
}

// 上游协议标签
pub mod upstream_protocol_labels {
    // DoH 上游
    pub const DOH: &str = "doh";
    // DNS（UDP/TCP）上游
    pub const DNS: &str = "dns";
    // 未知上游
    pub const UNKNOWN: &str = "unknown";
}

// 上游传输标签
pub mod upstream_transport_labels {
    // DoH 使用 HTTP
    pub const HTTP: &str = "http";
    // DNS 使用 UDP
    pub const UDP: &str = "udp";
    // DNS 使用 TCP
    pub const TCP: &str = "tcp";
    // 未知传输
    pub const UNKNOWN: &str = "unknown";
}

// 路由规则类型标签
pub mod rule_type_labels {
    // 精确匹配
    pub const EXACT: &str = "exact";
    // 通配符匹配
    pub const WILDCARD: &str = "wildcard";
    // 正则表达式匹配
    pub const REGEX: &str = "regex";
    // 未指定目标
    pub const NO_TARGET: &str = "none";
}

// 规则来源标签
pub mod rule_source_labels {
    // 静态规则
    pub const STATIC: &str = "static";
    // 远程规则
    pub const REMOTE: &str = "remote";
}

// 规则动作标签
pub mod rule_action_labels {
    // 转发动作
    pub const FORWARD: &str = "forward";
    // 阻止动作
    pub const BLOCK: &str = "block";
}

// 子系统名称
pub mod subsystem_names {
    // DNS服务器子系统
    pub const DNS_SERVER: &str = "dns_server";
    // 管理服务器子系统
    pub const ADMIN_SERVER: &str = "admin_server";
    // DoH服务器子系统
    pub const DOH_SERVER: &str = "doh_server";
    pub const SHUTDOWN_TIMER: &str = "shutdown_timer";
}

// 速率限制配置常量
pub mod rate_limit_defaults {
    // 默认每秒最大请求数
    pub const DEFAULT_MAX_REQUESTS_PER_SECOND: u32 = 100;
    // 最小每秒请求数
    pub const MIN_MAX_REQUESTS_PER_SECOND: u32 = 1;
    // 最大每秒请求数
    pub const MAX_MAX_REQUESTS_PER_SECOND: u32 = 100000;
    // 默认单IP每秒最大请求数
    pub const DEFAULT_PER_IP_MAX_REQUESTS_PER_SECOND: u32 = 100;
}

// 服务器默认值
pub mod server_defaults {
    // 默认TCP超时（秒）
    pub const DEFAULT_TCP_TIMEOUT: u64 = 10;
    // 默认HTTP超时（秒）
    pub const DEFAULT_HTTP_TIMEOUT: u64 = 30;
    // 默认DNS监听地址
    pub const DEFAULT_DNS_LISTEN: &str = "127.0.0.1:53";
    // 默认HTTP监听地址
    pub const DEFAULT_HTTP_LISTEN: &str = "127.0.0.1:8080";
    // 默认管理服务器监听地址
    pub const DEFAULT_ADMIN_LISTEN: &str = "127.0.0.1:9000";
    pub const DEFAULT_UDP_RECV_BUFFER: usize = 4 * 1024 * 1024;
    pub const DEFAULT_UDP_SEND_BUFFER: usize = 4 * 1024 * 1024;
    // 默认UDP socket数量（1）
    pub const DEFAULT_UDP_SOCKET_COUNT: usize = 1;
}

// 上游默认值
pub mod upstream_defaults {
    // 默认上游组名称
    pub const DEFAULT_GROUP_NAME: &str = "default";
    // 默认DoH服务器
    pub const DEFAULT_DOH_SERVER: &str = "https://dns.google/dns-query";
    // 默认权重
    pub const DEFAULT_WEIGHT: u32 = 1;
}

// 路由器常量
pub mod router {
    // 通配符常量
    pub mod wildcards {
        // 全局通配符
        pub const GLOBAL: &str = "*";
        // 前缀通配符
        pub const PREFIX: &str = "*.";
        // 点分隔符
        pub const DOT: char = '.';
    }
}

// HTTP头常量
pub mod http_headers {
    pub mod content_types {
        // DNS消息内容类型
        pub const DNS_MESSAGE: &str = "application/dns-message";
        // DNS JSON内容类型
        pub const DNS_JSON: &str = "application/dns-json";
    }

    // 认证常量
    pub mod auth {
        // Bearer前缀
        pub const BEARER_PREFIX: &str = "Bearer ";
    }
}
