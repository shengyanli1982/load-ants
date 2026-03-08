pub mod cli;
pub mod common;
pub mod core;
pub mod servers;

pub mod config;
pub mod remote_rule;
pub mod upstream;

// 兼容旧模块路径（tests/外部使用方可能依赖）
pub use cli::args;
pub use common::constants as r#const;
pub use common::error;
pub use common::metrics;
pub use core::balancer;
pub use core::cache;
pub use core::handler;
pub use core::router;
pub use servers::admin;
pub use servers::dns as server;
pub use servers::doh;

// 重导出常用组件
pub use admin::AdminServer;
pub use args::Args;
pub use balancer::{LoadBalancer, RandomBalancer, RoundRobinBalancer, WeightedBalancer};
pub use cache::DnsCache;
pub use config::Config;
pub use doh::DoHServer;
pub use error::{AppError, ConfigError};
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
    AdminConfig, CacheConfig, DnsConfig, HttpConfig, ListenersConfig, MatchType, RemoteRuleConfig,
    RouteAction, RouteRuleConfig, RulesConfig, UpstreamEndpointConfig, UpstreamGroupConfig,
};
