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
    // 并发超限（本地拒绝，不代表上游端点失败）
    pub const OVERLOADED: &str = "overloaded";
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
}

// 服务器默认值
