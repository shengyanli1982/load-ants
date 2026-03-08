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
