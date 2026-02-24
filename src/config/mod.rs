use crate::error::ConfigError;
use crate::r#const::{http_client_limits, retry_limits, upstream_defaults};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, collections::HashSet, fs, net::SocketAddr, path::Path, str::FromStr};
use tracing::debug;
use url::Url;
use validator::{Validate, ValidationError, ValidationErrors};

pub mod common;
pub mod core;
pub mod rule;
pub mod upstream;

pub use common::*;
pub use core::*;
pub use rule::*;
pub use upstream::*;

// 配置结果类型别名
pub type ConfigResult<T> = Result<T, ConfigError>;

static DEFAULT_DOH_URL: Lazy<reqwest::Url> = Lazy::new(|| {
    reqwest::Url::parse(upstream_defaults::DEFAULT_DOH_SERVER).unwrap_or_else(|e| {
        panic!(
            "Invalid upstream_defaults::DEFAULT_DOH_SERVER '{}': {}",
            upstream_defaults::DEFAULT_DOH_SERVER,
            e
        )
    })
});

// 自定义验证函数 - 验证Socket地址格式
pub fn validate_socket_addr(addr: &str) -> Result<(), ValidationError> {
    match SocketAddr::from_str(addr) {
        Ok(_) => Ok(()),
        Err(_) => Err(ValidationError::new("invalid_socket_addr")),
    }
}

// 自定义验证函数 - 验证URL格式
pub fn validate_url(url_str: &str) -> Result<(), ValidationError> {
    match Url::parse(url_str) {
        Ok(_) => Ok(()),
        Err(_) => Err(ValidationError::new("invalid_url")),
    }
}

// 自定义验证函数 - 验证Forward动作时必须有target
pub fn validate_forward_target(rule: &RouteRuleConfig) -> Result<(), ValidationError> {
    if matches!(rule.action, RouteAction::Forward) && rule.target.is_none() {
        return Err(ValidationError::new("missing_target_for_forward"));
    }
    Ok(())
}

// 自定义验证函数 - 验证上游组名称唯一性
pub fn validate_unique_group_names(config: &Config) -> Result<(), ValidationError> {
    if let Some(groups) = &config.upstream_groups {
        let mut names = HashSet::new();
        for group in groups {
            if !names.insert(group.name.clone()) {
                let mut err = ValidationError::new("duplicate_group_name");
                err.message = Some(Cow::from(format!(
                    "Duplicate upstream group name: '{}'",
                    group.name
                )));
                return Err(err);
            }
        }
    }
    Ok(())
}

// 自定义验证函数 - 验证规则引用的上游组存在
pub fn validate_group_references(config: &Config) -> Result<(), ValidationError> {
    let mut forward_targets: Vec<String> = Vec::new();

    // 收集静态规则中的 Forward targets
    if let Some(static_rules) = &config.static_rules {
        for rule in static_rules {
            if let RouteAction::Forward = rule.action {
                if let Some(target) = &rule.target {
                    forward_targets.push(target.clone());
                }
            }
        }
    }

    // 收集远程规则中的 Forward targets
    for rule in &config.remote_rules {
        if let RouteAction::Forward = rule.action {
            if let Some(target) = &rule.target {
                forward_targets.push(target.clone());
            }
        }
    }

    // 没有任何 Forward 规则，则不需要 upstream_groups 参与校验
    if forward_targets.is_empty() {
        return Ok(());
    }

    // 存在 Forward 规则：必须配置 upstream_groups，且至少有一个组
    let upstream_groups = match &config.upstream_groups {
        Some(groups) if !groups.is_empty() => groups,
        _ => {
            let mut err = ValidationError::new("missing_upstream_groups_for_forward");
            err.message = Some(Cow::from(
                "Forward rules require 'upstream_groups' to be configured and non-empty"
                    .to_string(),
            ));
            return Err(err);
        }
    };

    // 收集所有上游组名称
    let group_names: HashSet<_> = upstream_groups.iter().map(|g| g.name.clone()).collect();

    // 校验每个 target 都必须存在
    for target in forward_targets {
        if !group_names.contains(&target) {
            let mut err = ValidationError::new("non_existent_group_reference");
            err.message = Some(Cow::from(format!(
                "Route rule references non-existent upstream group: '{}'",
                target
            )));
            return Err(err);
        }
    }

    Ok(())
}

// 自定义验证函数 - 验证重试配置
pub fn validate_retry_config(retry: &RetryConfig) -> Result<(), ValidationError> {
    if retry.attempts < retry_limits::MIN_ATTEMPTS || retry.attempts > retry_limits::MAX_ATTEMPTS {
        return Err(ValidationError::new("invalid_retry_attempts"));
    }

    if retry.delay < retry_limits::MIN_DELAY || retry.delay > retry_limits::MAX_DELAY {
        return Err(ValidationError::new("invalid_retry_delay"));
    }

    Ok(())
}

// 自定义验证函数 - 验证空闲超时
//
// validator 对 Option<T> 字段会在 Some(T) 时把 T（按值）传入自定义校验函数，None 会被跳过。
pub fn validate_idle_timeout(timeout: u64) -> Result<(), ValidationError> {
    if !(http_client_limits::MIN_IDLE_TIMEOUT..=http_client_limits::MAX_IDLE_TIMEOUT)
        .contains(&timeout)
    {
        return Err(ValidationError::new("invalid_idle_timeout"));
    }
    Ok(())
}

// 自定义验证函数 - 验证Keepalive
pub fn validate_keepalive(value: u32) -> Result<(), ValidationError> {
    if !(http_client_limits::MIN_KEEPALIVE..=http_client_limits::MAX_KEEPALIVE).contains(&value) {
        return Err(ValidationError::new("invalid_keepalive"));
    }
    Ok(())
}

// 应用配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate)]
#[validate(schema(function = "validate_unique_group_names"))]
#[validate(schema(function = "validate_group_references"))]
#[serde(rename_all = "lowercase")]
pub struct Config {
    // 服务器配置
    #[validate(nested)]
    pub server: ServerConfig,
    // 管理服务器配置（可选）
    #[serde(default)]
    #[validate(nested)]
    pub admin: Option<AdminConfig>,
    // 缓存配置（可选）
    #[serde(default)]
    #[validate(nested)]
    pub cache: Option<CacheConfig>,
    // HTTP客户端配置（可选）
    #[serde(default)]
    #[validate(nested)]
    pub http_client: Option<HttpClientConfig>,
    // 上游组配置（可选）
    #[serde(default)]
    #[validate(nested)]
    pub upstream_groups: Option<Vec<UpstreamGroupConfig>>,
    // 路由规则配置（可选）
    #[serde(default)]
    #[validate(nested)]
    pub static_rules: Option<Vec<RouteRuleConfig>>,
    // 远程规则配置（可选）
    #[serde(default)]
    #[validate(nested)]
    pub remote_rules: Vec<RemoteRuleConfig>,
}

impl Config {
    // 从文件加载配置
    pub fn from_file<P: AsRef<Path>>(path: P) -> ConfigResult<Self> {
        debug!("Loading configuration file: {:?}", path.as_ref());
        let content = fs::read_to_string(path).map_err(ConfigError::LoadError)?;
        let config: Config = serde_yaml::from_str(&content).map_err(ConfigError::ParseError)?;
        config.validate()?;
        Ok(config)
    }

    // 创建一个带有默认值的配置
    #[allow(dead_code)]
    pub fn new_with_defaults() -> Self {
        Self::default()
    }

    // 验证配置有效性
    pub fn validate(&self) -> ConfigResult<()> {
        // 使用 validator 库进行验证
        if let Err(errors) = Validate::validate(self) {
            return Err(ConfigError::ValidationError(format_validation_errors(
                &errors,
            )));
        }
        Ok(())
    }

    /// 校验程序运行所需的“配置语义约束”。
    ///
    /// 注意：该校验不属于结构化字段校验（`validator`），因此不会在 `from_file()` 中自动触发，
    /// 由二进制入口（例如 `src/main.rs`）在启动阶段显式调用，以保持现有库接口与测试兼容。
    pub fn validate_runtime_requirements(&self) -> ConfigResult<()> {
        let static_rules_count = self.static_rules.as_ref().map_or(0, |rules| rules.len());
        let remote_rules_count = self.remote_rules.len();

        if static_rules_count == 0 && remote_rules_count == 0 {
            return Err(ConfigError::ValidationError(
                "No routing rules configured: please configure 'static_rules' and/or 'remote_rules'"
                    .to_string(),
            ));
        }

        let has_forward_in_static = self.static_rules.as_ref().is_some_and(|rules| {
            rules
                .iter()
                .any(|rule| matches!(rule.action, RouteAction::Forward))
        });
        let has_forward_in_remote = self
            .remote_rules
            .iter()
            .any(|rule| matches!(rule.action, RouteAction::Forward));

        if !(has_forward_in_static || has_forward_in_remote) {
            return Err(ConfigError::ValidationError(
                "No forward rules configured: please add at least one rule with action 'forward'"
                    .to_string(),
            ));
        }

        Ok(())
    }
}

// 将 ValidationErrors 转换为友好的错误信息
fn format_validation_errors(errors: &ValidationErrors) -> String {
    let mut messages = Vec::new();

    // 格式化字段错误
    for (field, error_kind) in errors.errors() {
        match error_kind {
            validator::ValidationErrorsKind::Field(field_errors) => {
                for error in field_errors {
                    let message = error
                        .message
                        .as_ref()
                        .map(|m| m.to_string())
                        .unwrap_or_else(|| error.code.to_string());
                    messages.push(format!("Field '{}': {}", field, message));
                }
            }
            validator::ValidationErrorsKind::Struct(struct_errors) => {
                messages.push(format!(
                    "Struct '{}' validation failed: {}",
                    field,
                    format_validation_errors(struct_errors)
                ));
            }
            validator::ValidationErrorsKind::List(list_errors) => {
                for (index, err) in list_errors {
                    messages.push(format!(
                        "List '{}' at index {}: {}",
                        field,
                        index,
                        format_validation_errors(err)
                    ));
                }
            }
        }
    }

    if messages.is_empty() {
        "Unknown validation error".to_string()
    } else {
        messages.join("\n")
    }
}

// 默认配置实现
impl Default for Config {
    fn default() -> Self {
        Config {
            server: ServerConfig::default(),
            admin: Some(AdminConfig::default()),
            cache: Some(CacheConfig::default()),
            http_client: Some(HttpClientConfig::default()),
            upstream_groups: Some(vec![UpstreamGroupConfig {
                name: upstream_defaults::DEFAULT_GROUP_NAME.to_string(),
                strategy: LoadBalancingStrategy::RoundRobin,
                servers: vec![UpstreamServerConfig {
                    url: DEFAULT_DOH_URL.clone(),
                    weight: upstream_defaults::DEFAULT_WEIGHT,
                    method: DoHMethod::Post,
                    content_type: DoHContentType::Message,
                    auth: None,
                }],
                retry: Some(RetryConfig {
                    attempts: retry_limits::DEFAULT_ATTEMPTS,
                    delay: retry_limits::DEFAULT_DELAY,
                }),
                proxy: None,
            }]),
            static_rules: Some(vec![RouteRuleConfig {
                match_type: MatchType::Wildcard,
                patterns: vec!["*".to_string()],
                action: RouteAction::Forward,
                target: Some(upstream_defaults::DEFAULT_GROUP_NAME.to_string()),
            }]),
            remote_rules: Vec::new(),
        }
    }
}
