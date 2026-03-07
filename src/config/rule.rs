use crate::config::validate_url;
use crate::r#const::remote_rule_limits;
use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize};
use std::borrow::Cow;
use validator::{Validate, ValidationError};

use super::common::{AuthConfig, RetryConfig};

// 规则格式枚举
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuleFormat {
    V2ray,
}

impl<'de> Deserialize<'de> for RuleFormat {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        super::serde_utils::deserialize_string_enum(
            deserializer,
            |normalized| match normalized {
                "v2ray" => Some(Self::V2ray),
                _ => None,
            },
            &["v2ray"],
        )
    }
}

fn default_rule_format() -> RuleFormat {
    RuleFormat::V2ray
}

// 远程规则类型枚举
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RemoteRuleType {
    Http,
}

impl<'de> Deserialize<'de> for RemoteRuleType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        super::serde_utils::deserialize_string_enum(
            deserializer,
            |normalized| match normalized {
                "http" => Some(Self::Http),
                _ => None,
            },
            &["http"],
        )
    }
}

fn default_max_rule_size() -> usize {
    remote_rule_limits::DEFAULT_MAX_SIZE
}

fn validate_forward_upstream(rule: &RemoteRuleConfig) -> Result<(), ValidationError> {
    if matches!(rule.action, RouteAction::Forward) && rule.upstream.is_none() {
        return Err(ValidationError::new("missing_upstream_for_forward"));
    }
    Ok(())
}

fn validate_rule_max_size(max_size: usize) -> Result<(), ValidationError> {
    if !(remote_rule_limits::MIN_SIZE..=remote_rule_limits::MAX_SIZE).contains(&max_size) {
        return Err(ValidationError::new("invalid_rule_max_size"));
    }
    Ok(())
}

// 远程规则配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate)]
#[validate(schema(
    function = "validate_forward_upstream",
    message = "Forward action requires upstream field"
))]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub struct RemoteRuleConfig {
    pub r#type: RemoteRuleType,

    #[validate(custom(function = "validate_url", message = "Invalid URL format"))]
    pub url: String,

    #[validate(nested)]
    pub auth: Option<AuthConfig>,

    #[serde(default = "default_rule_format")]
    pub format: RuleFormat,

    pub action: RouteAction,

    pub upstream: Option<String>,

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

// 路由匹配类型枚举
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MatchType {
    Exact,
    Wildcard,
    Regex,
}

impl<'de> Deserialize<'de> for MatchType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        super::serde_utils::deserialize_string_enum(
            deserializer,
            |normalized| match normalized {
                "exact" => Some(Self::Exact),
                "wildcard" => Some(Self::Wildcard),
                "regex" => Some(Self::Regex),
                _ => None,
            },
            &["exact", "wildcard", "regex"],
        )
    }
}

// 路由动作枚举
#[derive(Debug, Serialize, Clone, PartialEq, Eq, Copy)]
#[serde(rename_all = "lowercase")]
pub enum RouteAction {
    Forward,
    Block,
}

impl<'de> Deserialize<'de> for RouteAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        super::serde_utils::deserialize_string_enum(
            deserializer,
            |normalized| match normalized {
                "forward" => Some(Self::Forward),
                "block" => Some(Self::Block),
                _ => None,
            },
            &["forward", "block"],
        )
    }
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

fn validate_static_forward_upstream(rule: &RouteRuleConfig) -> Result<(), ValidationError> {
    if matches!(rule.action, RouteAction::Forward) && rule.upstream.is_none() {
        return Err(ValidationError::new("missing_upstream_for_forward"));
    }
    Ok(())
}

// 路由规则配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate)]
#[validate(schema(
    function = "validate_static_forward_upstream",
    message = "Forward action requires upstream field"
))]
#[validate(schema(
    function = "validate_route_rule_patterns",
    message = "Invalid route rule patterns"
))]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub struct RouteRuleConfig {
    #[serde(rename = "match")]
    pub match_type: MatchType,

    #[validate(custom(
        function = "validate_patterns_not_empty",
        message = "Patterns list cannot be empty"
    ))]
    pub patterns: Vec<String>,

    pub action: RouteAction,

    pub upstream: Option<String>,
}

// vNext 规则顶层配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate, Default)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub struct RulesConfig {
    #[serde(default)]
    #[validate(nested)]
    pub r#static: Vec<RouteRuleConfig>,

    #[serde(default)]
    #[validate(nested)]
    pub remote: Vec<RemoteRuleConfig>,
}
