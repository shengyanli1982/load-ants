use crate::config::validate_url;
use crate::r#const::remote_rule_limits;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use validator::{Validate, ValidationError};

use super::common::{AuthConfig, RetryConfig};

// 规则格式枚举
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuleFormat {
    // V2Ray 规则格式
    V2ray,
    // Clash 规则格式
    // Clash,
}

// 默认规则格式为 V2Ray
fn default_rule_format() -> RuleFormat {
    RuleFormat::V2ray
}

// 远程规则类型枚举
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RemoteRuleType {
    // URL类型规则
    Url,
}

// 默认最大规则文件大小
fn default_max_rule_size() -> usize {
    remote_rule_limits::DEFAULT_MAX_SIZE
}

// 自定义验证函数 - 验证Forward动作时必须有target
fn validate_forward_target(rule: &RemoteRuleConfig) -> Result<(), ValidationError> {
    if matches!(rule.action, RouteAction::Forward) && rule.target.is_none() {
        return Err(ValidationError::new("missing_target_for_forward"));
    }
    Ok(())
}

// 自定义验证函数 - 验证规则文件大小限制
fn validate_rule_max_size(max_size: usize) -> Result<(), ValidationError> {
    if !(remote_rule_limits::MIN_SIZE..=remote_rule_limits::MAX_SIZE).contains(&max_size) {
        return Err(ValidationError::new("invalid_rule_max_size"));
    }
    Ok(())
}

// 远程规则配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate)]
#[validate(schema(
    function = "validate_forward_target",
    message = "Forward action requires target field"
))]
#[serde(rename_all = "lowercase")]
pub struct RemoteRuleConfig {
    // 规则类型
    pub r#type: RemoteRuleType,
    // 规则URL
    #[validate(custom(function = "validate_url", message = "Invalid URL format"))]
    pub url: String,
    // 认证配置（可选）
    #[validate(nested)]
    pub auth: Option<AuthConfig>,
    // 规则格式（默认为v2ray）
    #[serde(default = "default_rule_format")]
    pub format: RuleFormat,
    // 路由动作
    pub action: RouteAction,
    // 目标上游组（当action为Forward时必须提供）
    pub target: Option<String>,
    // 重试配置（可选）
    #[validate(nested)]
    pub retry: Option<RetryConfig>,
    // 代理（可选）
    pub proxy: Option<String>,
    // 最大规则文件大小（字节，默认10MB）
    #[serde(default = "default_max_rule_size")]
    #[validate(custom(
        function = "validate_rule_max_size",
        message = "Invalid rule file size limit"
    ))]
    pub max_size: usize,
}

// 路由匹配类型枚举
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MatchType {
    // 精确匹配
    Exact,
    // 通配符匹配
    Wildcard,
    // 正则表达式匹配
    Regex,
}

// 路由动作枚举
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Copy)]
#[serde(rename_all = "lowercase")]
pub enum RouteAction {
    // 转发请求
    Forward,
    // 拦截请求
    Block,
}

// 自定义验证函数 - 验证规则匹配模式非空
fn validate_patterns_not_empty(patterns: &[String]) -> Result<(), ValidationError> {
    if patterns.is_empty() {
        return Err(ValidationError::new("empty_patterns"));
    }
    Ok(())
}

// 自定义验证函数 - 校验 RouteRuleConfig 的 patterns 与 match_type 语义一致
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

                // 允许配置里带尾随 '.'，与 Router 的 normalize 行为对齐
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

// 自定义验证函数 - 验证静态规则的Forward动作时必须有target
fn validate_static_forward_target(rule: &RouteRuleConfig) -> Result<(), ValidationError> {
    if matches!(rule.action, RouteAction::Forward) && rule.target.is_none() {
        return Err(ValidationError::new("missing_target_for_forward"));
    }
    Ok(())
}

// 路由规则配置
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
    // 匹配类型
    #[serde(rename = "match")]
    pub match_type: MatchType,
    // 匹配模式
    #[validate(custom(
        function = "validate_patterns_not_empty",
        message = "Patterns list cannot be empty"
    ))]
    pub patterns: Vec<String>,
    // 路由动作
    pub action: RouteAction,
    // 目标上游组（当action为Forward时必须提供）
    pub target: Option<String>,
}
