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

// 重试配置限制
pub mod retry_limits {
    // 最小重试次数
    pub const MIN_ATTEMPTS: u32 = 1;
    // 最大重试次数
    pub const MAX_ATTEMPTS: u32 = 100;
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
    #[allow(dead_code)]
    pub const UNKNOWN: &str = "unknown";
}

// 处理阶段标签
pub mod processing_labels {
    // 缓存命中
    pub const CACHED: &str = "cached";
    // 解析完成
    pub const RESOLVED: &str = "resolved";
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
    // 缓存命中
    pub const HIT: &str = "hit";
    // 缓存未命中
    #[allow(dead_code)]
    pub const MISS: &str = "miss";
    // 插入错误
    pub const INSERT_ERROR: &str = "insert_error";
    // 插入成功
    pub const INSERT: &str = "insert";
    // 清空缓存
    #[allow(dead_code)]
    pub const CLEAR: &str = "clear";
    // 原始TTL
    #[allow(dead_code)]
    pub const ORIGINAL: &str = "original";
    // 调整后TTL
    pub const ADJUSTED: &str = "adjusted";
}

// TTL源标签
pub mod ttl_source_labels {
    // 记录原始TTL
    pub const ORIGINAL: &str = "original";
    // 最小TTL配置
    pub const MIN_TTL: &str = "min_ttl";
    // TTL已调整
    pub const ADJUSTED: &str = "adjusted";
    // 负面缓存TTL
    pub const NEGATIVE_TTL: &str = "negative_ttl";
}

// 上游标签
pub mod upstream_labels {
    // 未知上游
    pub const UNKNOWN: &str = "unknown";
    // 重试
    #[allow(dead_code)]
    pub const RETRY: &str = "retry";
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
