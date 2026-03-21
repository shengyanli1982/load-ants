use crate::config::Config;
use crate::error::AppError;
use crate::remote_rule::{evaluate_remote_rule_startup, load_and_merge_rules};
use crate::Router;
use std::sync::Arc;
use tracing::{info, warn};

/// 根据配置构建路由器，并在需要时加载远程规则。
pub async fn build_router(config: &Config) -> Result<Arc<Router>, AppError> {
    let http_client_config = config.http_client.clone().unwrap_or_default();
    let static_rules = config.static_rules.clone().unwrap_or_default();

    if !config.remote_rules.is_empty() {
        // 存在远程规则源时，先拉取并与本地静态规则合并。
        info!(
            "Loading {} remote rule sources...",
            config.remote_rules.len()
        );
        let load_summary = load_and_merge_rules(
            &config.remote_rules,
            &static_rules,
            &http_client_config,
            &config.remote_rule_snapshot,
        )
        .await?;

        let load_summary = evaluate_remote_rule_startup(load_summary)?;

        if load_summary.partial_failure() {
            // 启动期间允许部分降级，但会逐条记录失败来源与回退信息。
            for failure in &load_summary.failed_sources {
                warn!(
                    url = %failure.url,
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
        info!(
            "Routing engine initialized successfully with {} rules ({} static, {} remote)",
            total_rules,
            static_rules.len(),
            remote_rules_count
        );
        Ok(Arc::new(router))
    } else {
        // 没有远程规则时直接使用静态规则初始化路由器。
        let router = Router::new(static_rules.clone())?;
        info!(
            "Routing engine initialized successfully with {} static rules",
            static_rules.len()
        );
        Ok(Arc::new(router))
    }
}
