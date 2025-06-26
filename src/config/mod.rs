use crate::error::ConfigError;
use crate::r#const::{http_client_limits, retry_limits, upstream_defaults};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, net::SocketAddr, path::Path, str::FromStr};
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
                return Err(ValidationError::new("duplicate_group_name"));
            }
        }
    }
    Ok(())
}

// 自定义验证函数 - 验证规则引用的上游组存在
pub fn validate_group_references(config: &Config) -> Result<(), ValidationError> {
    // 如果没有上游组配置，则跳过验证
    let upstream_groups = match &config.upstream_groups {
        Some(groups) => groups,
        None => return Ok(()),
    };

    // 收集所有上游组名称
    let group_names: HashSet<_> = upstream_groups.iter().map(|g| g.name.clone()).collect();

    // 检查静态规则
    if let Some(static_rules) = &config.static_rules {
        for rule in static_rules {
            if let RouteAction::Forward = rule.action {
                if let Some(target) = &rule.target {
                    if !group_names.contains(target) {
                        return Err(ValidationError::new("non_existent_group_reference"));
                    }
                }
            }
        }
    }

    // 检查远程规则
    for rule in &config.remote_rules {
        if let RouteAction::Forward = rule.action {
            if let Some(target) = &rule.target {
                if !group_names.contains(target) {
                    return Err(ValidationError::new("non_existent_group_reference"));
                }
            }
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
pub fn validate_idle_timeout(idle_timeout: &Option<u64>) -> Result<(), ValidationError> {
    if let Some(timeout) = idle_timeout {
        if *timeout < http_client_limits::MIN_IDLE_TIMEOUT
            || *timeout > http_client_limits::MAX_IDLE_TIMEOUT
        {
            return Err(ValidationError::new("invalid_idle_timeout"));
        }
    }
    Ok(())
}

// 自定义验证函数 - 验证Keepalive
pub fn validate_keepalive(keepalive: &Option<u32>) -> Result<(), ValidationError> {
    if let Some(value) = keepalive {
        if *value < http_client_limits::MIN_KEEPALIVE || *value > http_client_limits::MAX_KEEPALIVE
        {
            return Err(ValidationError::new("invalid_keepalive"));
        }
    }
    Ok(())
}

// 应用配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate)]
#[validate(schema(
    function = "validate_unique_group_names",
    message = "Upstream group names must be unique"
))]
#[validate(schema(
    function = "validate_group_references",
    message = "Rules reference non-existent upstream groups"
))]
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
                    url: reqwest::Url::parse(upstream_defaults::DEFAULT_DOH_SERVER).unwrap(),
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
