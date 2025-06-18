pub mod admin;
pub mod args;
pub mod balancer;
pub mod cache;
pub mod config;
pub mod r#const;
pub mod doh;
pub mod error;
pub mod handler;
pub mod metrics;
pub mod remote_rule;
pub mod router;
pub mod server;
pub mod upstream;

// 重导出常用组件
pub use admin::AdminServer;
pub use args::Args;
pub use balancer::{LoadBalancer, RandomBalancer, RoundRobinBalancer, WeightedBalancer};
pub use cache::DnsCache;
pub use config::Config;
pub use doh::DoHServer;
pub use error::AppError;
pub use handler::RequestHandler;
pub use metrics::DnsMetrics;
pub use remote_rule::{ClashRuleParser, RemoteRuleLoader, RuleParser, V2RayRuleParser};
pub use router::Router;
pub use server::DnsServer;
pub use upstream::UpstreamManager;

// 重导出常用常量组
pub use r#const::{
    cache_labels, error_labels, processing_labels, protocol_labels, rule_action_labels,
    rule_source_labels, rule_type_labels, subsystem_names,
};

// 重导出常用配置类型
pub use config::{
    AdminConfig, CacheConfig, HttpClientConfig, MatchType, RemoteRuleConfig, RouteAction,
    RouteRuleConfig, ServerConfig, UpstreamGroupConfig,
};
