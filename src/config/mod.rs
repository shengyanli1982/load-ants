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
mod serde_utils;
pub mod upstream;

pub use common::*;
pub use core::*;
pub use rule::*;
pub use upstream::*;

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

pub fn validate_socket_addr(addr: &str) -> Result<(), ValidationError> {
    match SocketAddr::from_str(addr) {
        Ok(_) => Ok(()),
        Err(_) => Err(ValidationError::new("invalid_socket_addr")),
    }
}

pub fn validate_url(url_str: &str) -> Result<(), ValidationError> {
    match Url::parse(url_str) {
        Ok(_) => Ok(()),
        Err(_) => Err(ValidationError::new("invalid_url")),
    }
}

pub fn validate_unique_upstream_names(config: &Config) -> Result<(), ValidationError> {
    let Some(groups) = &config.upstreams else {
        return Ok(());
    };

    let mut names = HashSet::new();
    for group in groups {
        if !names.insert(group.name.clone()) {
            let mut err = ValidationError::new("duplicate_upstream_name");
            err.message = Some(Cow::from(format!(
                "Duplicate upstream group name: '{}'",
                group.name
            )));
            return Err(err);
        }
    }
    Ok(())
}

pub fn validate_upstream_references(config: &Config) -> Result<(), ValidationError> {
    let mut forward_targets: Vec<String> = Vec::new();

    for rule in &config.rules.r#static {
        if let RouteAction::Forward = rule.action {
            if let Some(upstream) = &rule.upstream {
                forward_targets.push(upstream.clone());
            }
        }
    }

    for rule in &config.rules.remote {
        if let RouteAction::Forward = rule.action {
            if let Some(upstream) = &rule.upstream {
                forward_targets.push(upstream.clone());
            }
        }
    }

    if forward_targets.is_empty() {
        return Ok(());
    }

    let upstreams = match &config.upstreams {
        Some(groups) if !groups.is_empty() => groups,
        _ => {
            let mut err = ValidationError::new("missing_upstreams_for_forward");
            err.message = Some(Cow::from(
                "Forward rules require 'upstreams' to be configured and non-empty".to_string(),
            ));
            return Err(err);
        }
    };

    let upstream_names: HashSet<_> = upstreams.iter().map(|g| g.name.clone()).collect();
    for target in forward_targets {
        if !upstream_names.contains(&target) {
            let mut err = ValidationError::new("non_existent_upstream_reference");
            err.message = Some(Cow::from(format!(
                "Route rule references non-existent upstream group: '{}'",
                target
            )));
            return Err(err);
        }
    }

    Ok(())
}

pub fn validate_bootstrap_and_fallback_references(config: &Config) -> Result<(), ValidationError> {
    let Some(groups) = &config.upstreams else {
        if config.bootstrap_dns.is_some() {
            let mut err = ValidationError::new("missing_upstreams_for_bootstrap_dns");
            err.message = Some(Cow::from(
                "bootstrap_dns requires 'upstreams' to be configured and non-empty".to_string(),
            ));
            return Err(err);
        }
        return Ok(());
    };

    let upstream_names: HashSet<_> = groups.iter().map(|g| g.name.as_str()).collect();

    for group in groups {
        if let Some(fallback) = &group.fallback {
            if fallback == &group.name {
                let mut err = ValidationError::new("invalid_fallback_reference");
                err.message = Some(Cow::from(format!(
                    "Upstream group '{}' fallback cannot reference itself",
                    group.name
                )));
                return Err(err);
            }
            if !upstream_names.contains(fallback.as_str()) {
                let mut err = ValidationError::new("non_existent_fallback_reference");
                err.message = Some(Cow::from(format!(
                    "Upstream group '{}' references non-existent fallback group: '{}'",
                    group.name, fallback
                )));
                return Err(err);
            }
        }
    }

    if let Some(bootstrap) = &config.bootstrap_dns {
        if bootstrap.groups.is_empty() {
            return Err(ValidationError::new("empty_bootstrap_groups"));
        }

        for bootstrap_group_name in &bootstrap.groups {
            let Some(bootstrap_group) = groups.iter().find(|g| g.name == *bootstrap_group_name)
            else {
                let mut err = ValidationError::new("non_existent_bootstrap_group_reference");
                err.message = Some(Cow::from(format!(
                    "bootstrap_dns references non-existent upstream group: '{}'",
                    bootstrap_group_name
                )));
                return Err(err);
            };

            if bootstrap_group.protocol != UpstreamProtocol::Dns {
                let mut err = ValidationError::new("invalid_bootstrap_group_protocol");
                err.message = Some(Cow::from(format!(
                    "bootstrap_dns referenced group '{}' must use protocol 'dns'",
                    bootstrap_group_name
                )));
                return Err(err);
            }
        }
    }

    Ok(())
}

pub fn validate_retry_config(retry: &RetryConfig) -> Result<(), ValidationError> {
    if retry.attempts < retry_limits::MIN_ATTEMPTS || retry.attempts > retry_limits::MAX_ATTEMPTS {
        return Err(ValidationError::new("invalid_retry_attempts"));
    }

    if retry.delay < retry_limits::MIN_DELAY || retry.delay > retry_limits::MAX_DELAY {
        return Err(ValidationError::new("invalid_retry_delay"));
    }

    Ok(())
}

pub fn validate_idle_timeout(timeout: u64) -> Result<(), ValidationError> {
    if !(http_client_limits::MIN_IDLE_TIMEOUT..=http_client_limits::MAX_IDLE_TIMEOUT)
        .contains(&timeout)
    {
        return Err(ValidationError::new("invalid_idle_timeout"));
    }
    Ok(())
}

pub fn validate_keepalive(value: u32) -> Result<(), ValidationError> {
    if !(http_client_limits::MIN_KEEPALIVE..=http_client_limits::MAX_KEEPALIVE).contains(&value) {
        return Err(ValidationError::new("invalid_keepalive"));
    }
    Ok(())
}

// vNext 应用配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate)]
#[validate(schema(function = "validate_unique_upstream_names"))]
#[validate(schema(function = "validate_upstream_references"))]
#[validate(schema(function = "validate_bootstrap_and_fallback_references"))]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub struct Config {
    #[validate(nested)]
    pub listeners: ListenersConfig,

    #[serde(default)]
    #[validate(nested)]
    pub admin: Option<AdminConfig>,

    #[serde(default)]
    #[validate(nested)]
    pub cache: Option<CacheConfig>,

    #[serde(default)]
    #[validate(nested)]
    pub http: Option<HttpConfig>,

    #[serde(default)]
    #[validate(nested)]
    pub dns: Option<DnsConfig>,

    #[serde(default)]
    #[validate(nested)]
    pub bootstrap_dns: Option<BootstrapDnsConfig>,

    #[serde(default)]
    #[validate(nested)]
    pub upstreams: Option<Vec<UpstreamGroupConfig>>,

    #[serde(default)]
    #[validate(nested)]
    pub rules: RulesConfig,
}

impl Config {
    pub fn from_file<P: AsRef<Path>>(path: P) -> ConfigResult<Self> {
        debug!("Loading configuration file: {:?}", path.as_ref());
        let content = fs::read_to_string(path).map_err(ConfigError::LoadError)?;
        let config: Config = serde_yaml::from_str(&content).map_err(ConfigError::ParseError)?;
        config.validate()?;
        Ok(config)
    }

    #[allow(dead_code)]
    pub fn new_with_defaults() -> Self {
        Self::default()
    }

    pub fn validate(&self) -> ConfigResult<()> {
        if let Err(errors) = Validate::validate(self) {
            return Err(ConfigError::ValidationError(format_validation_errors(
                &errors,
            )));
        }
        Ok(())
    }

    pub fn validate_runtime_requirements(&self) -> ConfigResult<()> {
        let static_rules_count = self.rules.r#static.len();
        let remote_rules_count = self.rules.remote.len();

        if static_rules_count == 0 && remote_rules_count == 0 {
            return Err(ConfigError::ValidationError(
                "No routing rules configured: please configure 'rules.static' and/or 'rules.remote'"
                    .to_string(),
            ));
        }

        let has_forward_in_static = self
            .rules
            .r#static
            .iter()
            .any(|rule| matches!(rule.action, RouteAction::Forward));
        let has_forward_in_remote = self
            .rules
            .remote
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

fn format_validation_errors(errors: &ValidationErrors) -> String {
    let mut messages = Vec::new();

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

impl Default for Config {
    fn default() -> Self {
        Config {
            listeners: ListenersConfig::default(),
            admin: Some(AdminConfig::default()),
            cache: Some(CacheConfig::default()),
            http: Some(HttpConfig::default()),
            dns: Some(DnsConfig::default()),
            bootstrap_dns: None,
            upstreams: Some(vec![UpstreamGroupConfig {
                name: upstream_defaults::DEFAULT_GROUP_NAME.to_string(),
                protocol: UpstreamProtocol::Doh,
                policy: LoadBalancingPolicy::RoundRobin,
                endpoints: vec![UpstreamEndpointConfig::Doh(DoHUpstreamEndpointConfig {
                    url: DEFAULT_DOH_URL.clone(),
                    weight: upstream_defaults::DEFAULT_WEIGHT,
                    method: DoHMethod::Post,
                    content_type: DoHContentType::Message,
                    auth: None,
                })],
                fallback: None,
                failover: None,
                health: None,
                retry: Some(RetryConfig {
                    attempts: retry_limits::DEFAULT_ATTEMPTS,
                    delay: retry_limits::DEFAULT_DELAY,
                }),
                proxy: None,
            }]),
            rules: RulesConfig {
                r#static: vec![RouteRuleConfig {
                    match_type: MatchType::Wildcard,
                    patterns: vec!["*".to_string()],
                    action: RouteAction::Forward,
                    upstream: Some(upstream_defaults::DEFAULT_GROUP_NAME.to_string()),
                }],
                remote: Vec::new(),
            },
        }
    }
}
