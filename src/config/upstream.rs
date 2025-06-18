use crate::r#const::weight_limits;
use reqwest::Url;
use serde::{
    de::{self, Deserializer},
    Deserialize, Serialize,
};
use validator::{Validate, ValidationError};

use super::common::{AuthConfig, RetryConfig};

// 负载均衡策略枚举
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LoadBalancingStrategy {
    // 轮询策略
    RoundRobin,
    // 加权轮询策略
    Weighted,
    // 随机策略
    Random,
}

// DoH请求方法枚举
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DoHMethod {
    // GET请求方法
    Get,
    // POST请求方法
    Post,
}

// DoH内容类型枚举
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DoHContentType {
    // application/dns-message格式
    Message,
    // application/dns-json格式
    Json,
}

// 自定义反序列化函数，用于将字符串解析为 reqwest::Url
fn deserialize_url<'de, D>(deserializer: D) -> Result<Url, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Url::parse(&s).map_err(de::Error::custom)
}

// 自定义验证函数 - 验证URL方案
fn validate_url_scheme(url: &Url) -> Result<(), ValidationError> {
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(ValidationError::new("invalid_url_scheme"));
    }
    Ok(())
}

// 自定义验证函数 - 验证URL主机名
fn validate_url_host(url: &Url) -> Result<(), ValidationError> {
    if url.host_str().is_none() || url.host_str().unwrap().is_empty() {
        return Err(ValidationError::new("missing_url_hostname"));
    }
    Ok(())
}

// 自定义验证函数 - 验证URL路径
fn validate_url_path(url: &Url) -> Result<(), ValidationError> {
    if url.path().is_empty() || url.path() == "/" {
        return Err(ValidationError::new("invalid_url_path"));
    }
    Ok(())
}

// 自定义验证函数 - 验证权重
fn validate_weight(weight: u32) -> Result<(), ValidationError> {
    if !(weight_limits::MIN_WEIGHT..=weight_limits::MAX_WEIGHT).contains(&weight) {
        return Err(ValidationError::new("invalid_weight"));
    }
    Ok(())
}

// 上游服务器配置
#[derive(Debug, Serialize, Deserialize, Validate)]
#[serde(rename_all = "lowercase")]
pub struct UpstreamServerConfig {
    // DoH服务器URL
    #[serde(deserialize_with = "deserialize_url")]
    #[validate(custom(
        function = "validate_url_scheme",
        message = "URL must use http or https scheme"
    ))]
    #[validate(custom(
        function = "validate_url_host",
        message = "URL must contain a valid hostname"
    ))]
    #[validate(custom(
        function = "validate_url_path",
        message = "URL must contain a valid path"
    ))]
    pub url: Url,

    // 权重（仅用于加权负载均衡）
    #[serde(default = "default_us_weight")]
    #[validate(custom(
        function = "validate_weight",
        message = "Weight must be between 1-65535"
    ))]
    pub weight: u32,

    // DoH请求方法（GET/POST），默认为POST
    #[serde(default = "default_doh_method")]
    pub method: DoHMethod,

    // DoH内容类型，默认为Message
    #[serde(default = "default_content_type")]
    pub content_type: DoHContentType,

    // 认证配置（可选）
    #[validate(nested)]
    pub auth: Option<AuthConfig>,
}

impl Clone for UpstreamServerConfig {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            weight: self.weight,
            method: self.method.clone(),
            content_type: self.content_type.clone(),
            auth: self.auth.clone(),
        }
    }
}

impl PartialEq for UpstreamServerConfig {
    fn eq(&self, other: &Self) -> bool {
        self.url.as_str() == other.url.as_str()
            && self.weight == other.weight
            && self.method == other.method
            && self.content_type == other.content_type
            && self.auth == other.auth
    }
}

impl Eq for UpstreamServerConfig {}

// 默认的DoH方法为POST
fn default_doh_method() -> DoHMethod {
    DoHMethod::Post
}

// 默认的内容类型为DNS消息格式
fn default_content_type() -> DoHContentType {
    DoHContentType::Message
}

// 默认的权重为1
fn default_us_weight() -> u32 {
    1
}

// 自定义验证函数 - 验证上游组服务器非空
fn validate_servers_not_empty(servers: &[UpstreamServerConfig]) -> Result<(), ValidationError> {
    if servers.is_empty() {
        return Err(ValidationError::new("empty_servers"));
    }
    Ok(())
}

// 上游组配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate)]
#[serde(rename_all = "lowercase")]
pub struct UpstreamGroupConfig {
    // 组名称
    #[validate(length(min = 1, message = "Group name cannot be empty"))]
    pub name: String,

    // 负载均衡策略
    pub strategy: LoadBalancingStrategy,

    // 服务器列表
    #[validate(custom(
        function = "validate_servers_not_empty",
        message = "Server list cannot be empty"
    ))]
    #[validate(nested)]
    pub servers: Vec<UpstreamServerConfig>,

    // 重试配置（可选）
    #[validate(nested)]
    pub retry: Option<RetryConfig>,

    // 代理（可选）
    pub proxy: Option<String>,
}
