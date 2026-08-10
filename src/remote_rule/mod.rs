mod loader;
mod parser;
mod snapshot;

pub use self::loader::RemoteRuleLoader;
pub use self::parser::{ClashRuleParser, RuleParser, V2RayRuleParser};
pub use self::snapshot::{RemoteRuleSnapshotEnvelope, RemoteRuleSnapshotStore};

use crate::config::{
    HttpClientConfig, RemoteRuleConfig, RemoteRuleFailurePolicy, RemoteRuleSnapshotConfig,
    RouteAction, RouteRuleConfig,
};
use crate::error::AppError;
use crate::router::{RoutedRule, RuleMetadata};
use tracing::{error, warn};

pub type RemoteRuleResult = Result<RemoteRuleLoadSummary, AppError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRuleLoadFailure {
    pub url: String,
    pub action: RouteAction,
    pub failure_policy: RemoteRuleFailurePolicy,
    pub error: String,
    // 预留给后续“最近一次成功快照”回退链路使用。
    pub fallback_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRuleLoadSummary {
    pub merged_rules: Vec<RoutedRule>,
    pub successful_sources: Vec<String>,
    pub failed_sources: Vec<RemoteRuleLoadFailure>,
}

impl RemoteRuleLoadSummary {
    /// 是否存在任何失败来源，但尚未达到阻断启动的程度。
    pub fn partial_failure(&self) -> bool {
        !self.failed_sources.is_empty()
    }

    /// 是否存在严格策略且无法回退的失败来源。
    pub fn has_blocking_failures(&self) -> bool {
        self.failed_sources.iter().any(|failure| {
            failure.failure_policy == RemoteRuleFailurePolicy::Strict
                && failure.fallback_hint.is_none()
        })
    }
}

/// 根据汇总结果决定远程规则失败是否应阻断启动流程。
pub fn evaluate_remote_rule_startup(
    summary: RemoteRuleLoadSummary,
) -> Result<RemoteRuleLoadSummary, AppError> {
    if !summary.has_blocking_failures() {
        return Ok(summary);
    }

    let blocking_failures = summary
        .failed_sources
        .iter()
        .filter(|failure| failure.failure_policy == RemoteRuleFailurePolicy::Strict)
        .map(|failure| format!("{} ({})", failure.url, failure.error))
        .collect::<Vec<_>>()
        .join("; ");

    Err(AppError::Upstream(format!(
        "strict remote rule sources failed: {blocking_failures}"
    )))
}

/// 加载远程规则并与静态规则合并，同时处理快照持久化与回退。
pub async fn load_and_merge_rules(
    remote_configs: &[RemoteRuleConfig],
    static_rules: &[RouteRuleConfig],
    http_config: &HttpClientConfig,
    snapshot_config: &RemoteRuleSnapshotConfig,
) -> RemoteRuleResult {
    let mut merged_rules = Vec::with_capacity(static_rules.len() + remote_configs.len() * 3);
    merged_rules.extend(static_rules.iter().cloned().map(RoutedRule::from_static));

    let mut successful_sources = Vec::new();
    let mut failed_sources = Vec::new();
    let snapshot_store = RemoteRuleSnapshotStore::new(snapshot_config);
    let active_source_urls = remote_configs
        .iter()
        .map(|config| config.url.clone())
        .collect::<Vec<_>>();

    if let Err(snapshot_error) = snapshot_store
        .sync_active_sources(&active_source_urls)
        .await
    {
        warn!(
            error = %snapshot_error,
            "Failed to synchronize remote rule snapshot directory"
        );
    }

    for config in remote_configs {
        let source_url = config.url.clone();
        let source_metadata = RuleMetadata::remote(source_url.clone());

        match RemoteRuleLoader::new(config.clone(), http_config.clone()) {
            Ok(loader) => match loader.load().await {
                Ok(remote_rules) => {
                    merged_rules.extend(
                        remote_rules
                            .iter()
                            .cloned()
                            .map(|rule| RoutedRule::new(rule, source_metadata.clone())),
                    );

                    if let Err(snapshot_error) =
                        snapshot_store.save(&source_url, &remote_rules).await
                    {
                        warn!(
                            url = %source_url,
                            error = %snapshot_error,
                            "Failed to persist last-known-good remote rule snapshot"
                        );
                    }

                    successful_sources.push(source_url);
                }
                Err(err) => {
                    let mut failure = RemoteRuleLoadFailure {
                        url: source_url.clone(),
                        action: config.action,
                        failure_policy: config.failure_policy.clone(),
                        error: err.to_string(),
                        fallback_hint: None,
                    };

                    match snapshot_store.load(&source_url).await {
                        Ok(Some(snapshot)) => {
                            merged_rules.extend(
                                snapshot
                                    .rules
                                    .into_iter()
                                    .map(|rule| RoutedRule::new(rule, source_metadata.clone())),
                            );
                            failure.fallback_hint = Some(format!(
                                "used last-known-good snapshot '{}'",
                                snapshot_store.snapshot_path(&source_url).display()
                            ));
                        }
                        Ok(None) => {}
                        Err(snapshot_error) => {
                            failure.error = format!(
                                "{}; snapshot restore failed: {}",
                                failure.error, snapshot_error
                            );
                        }
                    }

                    error!(
                        url = %failure.url,
                        action = %<&'static str>::from(failure.action),
                        failure_policy = %failure.failure_policy,
                        error = %failure.error,
                        fallback_hint = failure.fallback_hint.as_deref().unwrap_or("none"),
                        "Failed to load remote rule source"
                    );
                    failed_sources.push(failure);
                }
            },
            Err(err) => {
                let mut failure = RemoteRuleLoadFailure {
                    url: source_url.clone(),
                    action: config.action,
                    failure_policy: config.failure_policy.clone(),
                    error: err.to_string(),
                    fallback_hint: None,
                };

                match snapshot_store.load(&source_url).await {
                    Ok(Some(snapshot)) => {
                        merged_rules.extend(
                            snapshot
                                .rules
                                .into_iter()
                                .map(|rule| RoutedRule::new(rule, source_metadata.clone())),
                        );
                        failure.fallback_hint = Some(format!(
                            "used last-known-good snapshot '{}'",
                            snapshot_store.snapshot_path(&source_url).display()
                        ));
                    }
                    Ok(None) => {}
                    Err(snapshot_error) => {
                        failure.error = format!(
                            "{}; snapshot restore failed: {}",
                            failure.error, snapshot_error
                        );
                    }
                }

                error!(
                    url = %failure.url,
                    action = %<&'static str>::from(failure.action),
                    failure_policy = %failure.failure_policy,
                    error = %failure.error,
                    fallback_hint = failure.fallback_hint.as_deref().unwrap_or("none"),
                    "Failed to create remote rule loader"
                );
                failed_sources.push(failure);
            }
        }
    }

    if !failed_sources.is_empty() {
        warn!(
            successful_sources = successful_sources.len(),
            failed_sources = failed_sources.len(),
            "Remote rule loading completed with degraded sources"
        );
    }

    Ok(RemoteRuleLoadSummary {
        merged_rules,
        successful_sources,
        failed_sources,
    })
}
