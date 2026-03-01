use crate::r#const::weight_limits;
use reqwest::Url;
use serde::{
    de::{self, Deserializer},
    Deserialize, Serialize,
};
use std::borrow::Cow;
use std::net::SocketAddr;
use validator::{Validate, ValidationError, ValidationErrors};

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

// 上游组 scheme
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UpstreamScheme {
    Doh,
    Dns,
}

fn default_upstream_scheme() -> UpstreamScheme {
    UpstreamScheme::Doh
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
    if url.host_str().is_none_or(str::is_empty) {
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

// DoH 上游服务器配置
#[derive(Debug, Serialize, Deserialize, Validate)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub struct DoHUpstreamServerConfig {
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

impl Clone for DoHUpstreamServerConfig {
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

impl PartialEq for DoHUpstreamServerConfig {
    fn eq(&self, other: &Self) -> bool {
        self.url.as_str() == other.url.as_str()
            && self.weight == other.weight
            && self.method == other.method
            && self.content_type == other.content_type
            && self.auth == other.auth
    }
}

impl Eq for DoHUpstreamServerConfig {}

// DNS（UDP/TCP）上游服务器配置
#[derive(Debug, Serialize, Deserialize, Validate, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub struct DnsUpstreamServerConfig {
    pub addr: SocketAddr,

    #[serde(default = "default_us_weight")]
    #[validate(custom(
        function = "validate_weight",
        message = "Weight must be between 1-65535"
    ))]
    pub weight: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum UpstreamServerConfig {
    Doh(DoHUpstreamServerConfig),
    Dns(DnsUpstreamServerConfig),
}

impl UpstreamServerConfig {
    pub fn weight(&self) -> u32 {
        match self {
            Self::Doh(s) => s.weight,
            Self::Dns(s) => s.weight,
        }
    }

    pub fn as_doh(&self) -> Option<&DoHUpstreamServerConfig> {
        match self {
            Self::Doh(s) => Some(s),
            Self::Dns(_) => None,
        }
    }

    pub fn as_dns(&self) -> Option<&DnsUpstreamServerConfig> {
        match self {
            Self::Doh(_) => None,
            Self::Dns(s) => Some(s),
        }
    }
}

impl Validate for UpstreamServerConfig {
    fn validate(&self) -> Result<(), ValidationErrors> {
        match self {
            Self::Doh(s) => s.validate(),
            Self::Dns(s) => s.validate(),
        }
    }
}

// 自定义验证函数 - 验证上游组服务器非空
fn validate_servers_not_empty(servers: &[UpstreamServerConfig]) -> Result<(), ValidationError> {
    if servers.is_empty() {
        return Err(ValidationError::new("empty_servers"));
    }
    Ok(())
}

fn validate_group_scheme(group: &UpstreamGroupConfig) -> Result<(), ValidationError> {
    match group.scheme {
        UpstreamScheme::Doh => {
            for server in &group.servers {
                if server.as_doh().is_none() {
                    let mut err = ValidationError::new("invalid_server_variant_for_scheme");
                    err.message = Some(Cow::from(
                        "Upstream group scheme 'doh' requires servers to use 'url' entries"
                            .to_string(),
                    ));
                    return Err(err);
                }
            }
            Ok(())
        }
        UpstreamScheme::Dns => {
            if group.retry.is_some() {
                let mut err = ValidationError::new("dns_group_retry_not_supported");
                err.message = Some(Cow::from(
                    "Upstream group scheme 'dns' does not support 'retry'".to_string(),
                ));
                return Err(err);
            }
            if group.proxy.is_some() {
                let mut err = ValidationError::new("dns_group_proxy_not_supported");
                err.message = Some(Cow::from(
                    "Upstream group scheme 'dns' does not support 'proxy'".to_string(),
                ));
                return Err(err);
            }
            for server in &group.servers {
                if server.as_dns().is_none() {
                    let mut err = ValidationError::new("invalid_server_variant_for_scheme");
                    err.message = Some(Cow::from(
                        "Upstream group scheme 'dns' requires servers to use 'addr' entries"
                            .to_string(),
                    ));
                    return Err(err);
                }
            }
            Ok(())
        }
    }
}

// 上游组配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate)]
#[validate(schema(
    function = "validate_group_scheme",
    message = "Upstream group scheme validation failed"
))]
#[serde(rename_all = "lowercase")]
pub struct UpstreamGroupConfig {
    // 组名称
    #[validate(length(min = 1, message = "Group name cannot be empty"))]
    pub name: String,

    // 上游组 scheme（doh|dns），缺省为 doh（兼容旧配置）
    #[serde(default = "default_upstream_scheme", alias = "protocol")]
    pub scheme: UpstreamScheme,

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
