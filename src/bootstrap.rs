use crate::config::Config;
use crate::error::AppError;
use crate::remote_rule::{evaluate_remote_rule_startup, load_and_merge_rules, strip_url_userinfo};
use crate::Router;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

pub async fn build_router(config: &Config) -> Result<Arc<RwLock<Arc<Router>>>, AppError> {
    let http_client_config = config.http_client.clone().unwrap_or_default();
    let static_rules = config.static_rules.clone().unwrap_or_default();

    let (router, total_rules, static_rules_count, remote_rules_count) =
        if !config.remote_rules.sources.is_empty() {
            // 存在远程规则源时，先拉取并与本地静态规则合并。
            info!(
                sources = config.remote_rules.sources.len(),
                "Loading remote rule sources"
            );
            let load_summary = load_and_merge_rules(
                &config.remote_rules.sources,
                &static_rules,
                &http_client_config,
                &config.remote_rules.snapshot,
            )
            .await?;

            let load_summary = evaluate_remote_rule_startup(load_summary)?;

            if load_summary.partial_failure() {
                // 启动期间允许部分降级，但会逐条记录失败来源与回退信息。
                for failure in &load_summary.failed_sources {
                    warn!(
                        url = %strip_url_userinfo(failure.url.as_str()),
                        action = %<&'static str>::from(failure.action),
                        failure_policy = %failure.failure_policy,
                        error = %failure.error,
                        fallback_hint = failure.fallback_hint.as_deref().unwrap_or("none"),
                        "Remote rule source degraded during startup"
                    );
                }
            }

            let total_rules = load_summary.merged_rules.len();
            let remote_rules_count = total_rules.saturating_sub(static_rules.len());

            let router = Router::new_with_metadata(load_summary.merged_rules)?;
            (router, total_rules, static_rules.len(), remote_rules_count)
        } else {
            let total_rules = static_rules.len();
            let router = Router::new(static_rules.clone())?;
            (router, total_rules, total_rules, 0)
        };

    if total_rules == 0 {
        error!("Routing table is empty, all queries will be refused");
    }
    info!(
        rules = total_rules,
        static_rules = static_rules_count,
        remote_rules = remote_rules_count,
        "Routing engine ready"
    );
    Ok(Arc::new(RwLock::new(Arc::new(router))))
}
