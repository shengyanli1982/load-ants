use crate::r#const::remote_rule_limits;
use serde::{Deserialize, Serialize};

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

// 远程规则配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct RemoteRuleConfig {
    // 规则类型
    pub r#type: RemoteRuleType,
    // 规则URL
    pub url: String,
    // 认证配置（可选）
    pub auth: Option<AuthConfig>,
    // 规则格式（默认为v2ray）
    #[serde(default = "default_rule_format")]
    pub format: RuleFormat,
    // 路由动作
    pub action: RouteAction,
    // 目标上游组（当action为Forward时必须提供）
    pub target: Option<String>,
    // 重试配置（可选）
    pub retry: Option<RetryConfig>,
    // 代理（可选）
    pub proxy: Option<String>,
    // 最大规则文件大小（字节，默认10MB）
    #[serde(default = "default_max_rule_size")]
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

// 路由规则配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct RouteRuleConfig {
    // 匹配类型
    #[serde(rename = "match")]
    pub match_type: MatchType,
    // 匹配模式
    pub patterns: Vec<String>,
    // 路由动作
    pub action: RouteAction,
    // 目标上游组（当action为Forward时必须提供）
    pub target: Option<String>,
}
