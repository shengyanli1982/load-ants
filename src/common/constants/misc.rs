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
    // Content-Type 头
    pub const CONTENT_TYPE: &str = "Content-Type";
    // Accept 头
    pub const ACCEPT: &str = "Accept";
    // Authorization 头
    pub const AUTHORIZATION: &str = "Authorization";

    // 内容类型常量
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
