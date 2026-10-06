mod loader;
mod parser;
mod snapshot;

pub use self::loader::RemoteRuleLoader;
pub use self::parser::{ClashRuleParser, RuleParser, V2RayRuleParser};
pub use self::snapshot::{RemoteRuleSnapshotEnvelope, RemoteRuleSnapshotStore};

use crate::admin::RemoteSourceStatus;
use crate::config::{
    HttpClientConfig, RemoteRuleConfig, RemoteRuleFailurePolicy, RemoteRuleSnapshotConfig,
    RemoteRulesConfig, RouteAction, RouteRuleConfig,
};
use crate::error::AppError;
use crate::router::{RoutedRule, Router, RuleMetadata};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

pub(crate) fn strip_url_userinfo(url_str: &str) -> String {
    let Some(scheme_end) = url_str.find("://") else {
        return url_str.to_string();
    };
    let rest = &url_str[scheme_end + 3..];
    let authority_end = rest.find('/').map_or(rest.len(), |pos| pos);
    if let Some(at_pos) = rest[..authority_end].find('@') {
        return format!("{}{}", &url_str[..scheme_end + 3], &rest[at_pos + 1..]);
    }
    url_str.to_string()
}

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
        .map(|failure| {
            format!(
                "{} ({})",
                strip_url_userinfo(failure.url.as_str()),
                failure.error
            )
        })
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

                    warn!(
                        url = %strip_url_userinfo(failure.url.as_str()),
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

                warn!(
                    url = %strip_url_userinfo(failure.url.as_str()),
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

/// 运行一次后台任务迭代，捕获迭代内的 panic 使其不向外传播。
///
/// 单次迭代 panic 不得终止外层刷新循环，否则循环会静默退出且永不恢复。
/// （原位于 src/main.rs，为可测试性平移至 lib，逻辑零变化。）
pub async fn run_panic_guarded<F>(task_name: &str, task: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    match tokio::spawn(task).await {
        Ok(()) => {}
        Err(e) if e.is_panic() => {
            error!(
                task = %task_name,
                error = &e as &dyn std::error::Error,
                "Task iteration panicked"
            );
        }
        Err(e) => {
            warn!(task = %task_name, error = %e, "Task cancelled");
        }
    }
}

/// 执行一次远程规则重载：拉取并合并规则，成功后原子替换 router，
/// 并重建各来源状态列表；失败时将所有来源标记为 failed。
/// （原位于 src/main.rs，为可测试性平移至 lib，逻辑零变化。）
pub async fn reload_once(
    remote_rules_config: RemoteRulesConfig,
    static_rules: Vec<RouteRuleConfig>,
    http_client_config: HttpClientConfig,
    router: Arc<RwLock<Arc<Router>>>,
    statuses: Arc<RwLock<Vec<RemoteSourceStatus>>>,
) {
    let reload_started = Instant::now();

    let load_result = load_and_merge_rules(
        &remote_rules_config.sources,
        &static_rules,
        &http_client_config,
        &remote_rules_config.snapshot,
    )
    .await;

    let summary = match load_result {
        Ok(s) => s,
        Err(e) => {
            warn!(
                elapsed_ms = reload_started.elapsed().as_millis() as u64,
                error = %e,
                "Failed to reload remote rules"
            );
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let mut guards = statuses.write().await;
            for g in guards.iter_mut() {
                g.status = "failed".to_string();
                g.last_updated = now;
                g.rule_count = 0;
                g.error_message = Some(format!("{e}"));
            }
            return;
        }
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let new_statuses: Vec<RemoteSourceStatus> = remote_rules_config
        .sources
        .iter()
        .map(|config| {
            let url = &config.url;
            let is_successful = summary.successful_sources.contains(url);
            let failed_entry = summary.failed_sources.iter().find(|f| &f.url == url);
            let rule_count = summary
                .merged_rules
                .iter()
                .filter(|rule| {
                    rule.metadata.source_id.as_ref().map(|s| s.as_str()) == Some(url.as_str())
                })
                .count();
            let status = if is_successful { "ok" } else { "failed" };
            let error_message = failed_entry.map(|f| f.error.clone());
            RemoteSourceStatus {
                url: url.clone(),
                status: status.to_string(),
                last_updated: now,
                rule_count,
                error_message,
            }
        })
        .collect();

    {
        let mut guards = statuses.write().await;
        *guards = new_statuses;
    }

    match evaluate_remote_rule_startup(summary) {
        Ok(summary) => {
            let rule_count = summary.merged_rules.len();
            let sources_ok = summary.successful_sources.len();
            let sources_failed = summary.failed_sources.len();
            match Router::new_with_metadata(summary.merged_rules) {
                Ok(new_router) => {
                    let new_router = Arc::new(new_router);
                    let mut writer = router.write().await;
                    *writer = new_router;
                    info!(
                        rules = rule_count,
                        sources_ok = sources_ok,
                        sources_failed = sources_failed,
                        elapsed_ms = reload_started.elapsed().as_millis() as u64,
                        "Remote rules reload completed"
                    );
                }
                Err(e) => {
                    warn!(error = %e, "Failed to build router from reloaded rules");
                }
            }
        }
        Err(e) => {
            warn!(error = %e, "Failed to evaluate reloaded rules");
        }
    }
}
