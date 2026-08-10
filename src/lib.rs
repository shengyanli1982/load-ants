pub mod admin;
pub mod args;
pub mod balancer;
pub mod bootstrap;
pub mod cache;
pub mod coalesce;
pub mod config;
pub mod r#const;
pub mod doh;
pub mod error;
pub mod handler;
pub mod metrics;
pub mod rate_limit;
pub mod remote_rule;
pub mod router;
pub mod server;
pub mod upstream;

// 重导出常用组件
pub use admin::{AdminServer, ConfigSummary, RemoteSourceStatus};
pub use args::Args;
pub use balancer::{LoadBalancer, RandomBalancer, RoundRobinBalancer, WeightedBalancer};
pub use bootstrap::build_router;
pub use cache::{
    CacheDumpEntry, CacheDumpResponse, CacheKey, CacheRestoreRequest, CacheRestoreStats,
    CacheResult, DnsCache,
};
pub use coalesce::CoalescingMap;
pub use config::Config;
pub use doh::DoHServer;
pub use error::AppError;
pub use handler::RequestHandler;
pub use metrics::DnsMetrics;
pub use rate_limit::RateLimiter;
pub use remote_rule::{
    evaluate_remote_rule_startup, load_and_merge_rules, ClashRuleParser, RemoteRuleLoadSummary,
    RemoteRuleLoader, RuleParser, V2RayRuleParser,
};
pub use router::Router;
pub use server::DnsServer;
pub use upstream::UpstreamManager;

// 重导出常用常量组
pub use r#const::{
    cache_labels, error_labels, processing_labels, protocol_labels, rule_action_labels,
    rule_source_labels, rule_type_labels, subsystem_names, ttl_source_labels,
};

// 重导出常用配置类型
pub use config::{
    AdminAuthConfig, AdminConfig, CacheConfig, DnsClientConfig, HttpClientConfig, MatchType,
    RateLimitConfig, RemoteRuleConfig, RemoteRuleFailurePolicy, RemoteRuleSnapshotConfig,
    RemoteRulesConfig, RouteAction, RouteRuleConfig, ServerConfig, UpstreamGroupConfig,
};

pub use cache::{build_cname_chain, filter_answer_records, filter_response_records};
