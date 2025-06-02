pub mod admin;
pub mod args;
pub mod balancer;
pub mod cache;
pub mod config;
pub mod r#const;
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
pub use error::AppError;
pub use handler::RequestHandler;
pub use metrics::DnsMetrics;
pub use remote_rule::{ClashRuleParser, RemoteRuleLoader, RuleParser, V2RayRuleParser};
pub use router::Router;
pub use server::DnsServer;
pub use upstream::UpstreamManager;
