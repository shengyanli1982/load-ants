use crate::config::validate_url;
use crate::r#const::remote_rule_limits;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt;
use validator::{Validate, ValidationError};

use super::common::{AuthConfig, RetryConfig};

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuleFormat {
    V2ray,
}

fn default_rule_format() -> RuleFormat {
    RuleFormat::V2ray
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RemoteRuleType {
    Url,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RemoteRuleFailurePolicy {
    Strict,
    Lenient,
}

impl fmt::Display for RemoteRuleFailurePolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Strict => f.write_str("strict"),
            Self::Lenient => f.write_str("lenient"),
        }
    }
}

fn default_remote_rule_failure_policy() -> RemoteRuleFailurePolicy {
    RemoteRuleFailurePolicy::Strict
}

fn default_max_rule_size() -> usize {
    remote_rule_limits::DEFAULT_MAX_SIZE
}

fn default_remote_rule_snapshot_enabled() -> bool {
    true
}

fn default_remote_rule_snapshot_path() -> String {
    ".load-ants/remote-rule-snapshots".to_string()
}

fn validate_forward_target(rule: &RemoteRuleConfig) -> Result<(), ValidationError> {
    if matches!(rule.action, RouteAction::Forward) && rule.target.is_none() {
        return Err(ValidationError::new("missing_target_for_forward"));
    }
    Ok(())
}

fn validate_rule_max_size(max_size: usize) -> Result<(), ValidationError> {
    if !(remote_rule_limits::MIN_SIZE..=remote_rule_limits::MAX_SIZE).contains(&max_size) {
        return Err(ValidationError::new("invalid_rule_max_size"));
    }
    Ok(())
}

fn validate_snapshot_path(path: &str) -> Result<(), ValidationError> {
    if path.trim().is_empty() {
        return Err(ValidationError::new("invalid_snapshot_path"));
    }
    Ok(())
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate)]
#[serde(rename_all = "lowercase")]
pub struct RemoteRuleSnapshotConfig {
    #[serde(default = "default_remote_rule_snapshot_enabled")]
    pub enabled: bool,
    #[serde(default = "default_remote_rule_snapshot_path")]
    #[validate(custom(
        function = "validate_snapshot_path",
        message = "Snapshot path cannot be empty"
    ))]
    pub path: String,
}

impl Default for RemoteRuleSnapshotConfig {
    fn default() -> Self {
        Self {
            enabled: default_remote_rule_snapshot_enabled(),
            path: default_remote_rule_snapshot_path(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate)]
#[validate(schema(
    function = "validate_forward_target",
    message = "Forward action requires target field"
))]
#[serde(rename_all = "lowercase")]
pub struct RemoteRuleConfig {
    pub r#type: RemoteRuleType,
    #[validate(custom(function = "validate_url", message = "Invalid URL format"))]
    pub url: String,
    #[validate(nested)]
    pub auth: Option<AuthConfig>,
    #[serde(default = "default_rule_format")]
    pub format: RuleFormat,
    #[serde(default = "default_remote_rule_failure_policy")]
    pub failure_policy: RemoteRuleFailurePolicy,
    pub action: RouteAction,
    pub target: Option<String>,
    #[validate(nested)]
    pub retry: Option<RetryConfig>,
    pub proxy: Option<String>,
    #[serde(default = "default_max_rule_size")]
    #[validate(custom(
        function = "validate_rule_max_size",
        message = "Invalid rule file size limit"
    ))]
    pub max_size: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MatchType {
    Exact,
    Wildcard,
    Regex,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Copy)]
#[serde(rename_all = "lowercase")]
pub enum RouteAction {
    Forward,
    Block,
}

fn validate_patterns_not_empty(patterns: &[String]) -> Result<(), ValidationError> {
    if patterns.is_empty() {
        return Err(ValidationError::new("empty_patterns"));
    }
    Ok(())
}

fn validate_route_rule_patterns(rule: &RouteRuleConfig) -> Result<(), ValidationError> {
    match rule.match_type {
        MatchType::Exact => Ok(()),
        MatchType::Wildcard => {
            for pattern in &rule.patterns {
                if pattern == "*" {
                    continue;
                }

                let Some(suffix) = pattern.strip_prefix("*.") else {
                    let mut err = ValidationError::new("invalid_wildcard_pattern");
                    err.message = Some(Cow::from(format!(
                        "Invalid wildcard pattern '{}': expected '*' or '*.domain.tld'",
                        pattern
                    )));
                    return Err(err);
                };

                let suffix = suffix.trim_end_matches('.');
                if suffix.is_empty() || suffix.starts_with('.') || suffix.contains("..") {
                    let mut err = ValidationError::new("invalid_wildcard_pattern");
                    err.message = Some(Cow::from(format!(
                        "Invalid wildcard pattern '{}': invalid domain suffix",
                        pattern
                    )));
                    return Err(err);
                }
            }
            Ok(())
        }
        MatchType::Regex => {
            for pattern in &rule.patterns {
                if let Err(e) = Regex::new(pattern) {
                    let mut err = ValidationError::new("invalid_regex_pattern");
                    err.message = Some(Cow::from(format!(
                        "Invalid regex pattern '{}': {}",
                        pattern, e
                    )));
                    return Err(err);
                }
            }
            Ok(())
        }
    }
}

fn validate_static_forward_target(rule: &RouteRuleConfig) -> Result<(), ValidationError> {
    if matches!(rule.action, RouteAction::Forward) && rule.target.is_none() {
        return Err(ValidationError::new("missing_target_for_forward"));
    }
    Ok(())
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate)]
#[validate(schema(
    function = "validate_static_forward_target",
    message = "Forward action requires target field"
))]
#[validate(schema(
    function = "validate_route_rule_patterns",
    message = "Invalid route rule patterns"
))]
#[serde(rename_all = "lowercase")]
pub struct RouteRuleConfig {
    #[serde(rename = "match")]
    pub match_type: MatchType,
    #[validate(custom(
        function = "validate_patterns_not_empty",
        message = "Patterns list cannot be empty"
    ))]
    pub patterns: Vec<String>,
    pub action: RouteAction,
    pub target: Option<String>,
}
