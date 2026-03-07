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
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LoadBalancingPolicy {
    RoundRobin,
    Weighted,
    Random,
}

impl<'de> Deserialize<'de> for LoadBalancingPolicy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        super::serde_utils::deserialize_string_enum(
            deserializer,
            |normalized| match normalized {
                "roundrobin" => Some(Self::RoundRobin),
                "weighted" => Some(Self::Weighted),
                "random" => Some(Self::Random),
                _ => None,
            },
            &["roundrobin", "weighted", "random"],
        )
    }
}

// 上游协议
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UpstreamProtocol {
    Doh,
    Dns,
}

impl<'de> Deserialize<'de> for UpstreamProtocol {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        super::serde_utils::deserialize_string_enum(
            deserializer,
            |normalized| match normalized {
                "doh" => Some(Self::Doh),
                "dns" => Some(Self::Dns),
                _ => None,
            },
            &["doh", "dns"],
        )
    }
}

fn default_upstream_protocol() -> UpstreamProtocol {
    UpstreamProtocol::Doh
}

// DoH 请求方法枚举
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DoHMethod {
    Get,
    Post,
}

impl<'de> Deserialize<'de> for DoHMethod {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        super::serde_utils::deserialize_string_enum(
            deserializer,
            |normalized| match normalized {
                "get" => Some(Self::Get),
                "post" => Some(Self::Post),
                _ => None,
            },
            &["get", "post"],
        )
    }
}

// DoH 内容类型枚举
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DoHContentType {
    Message,
    Json,
}

impl<'de> Deserialize<'de> for DoHContentType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        super::serde_utils::deserialize_string_enum(
            deserializer,
            |normalized| match normalized {
                "message" => Some(Self::Message),
                "json" => Some(Self::Json),
                _ => None,
            },
            &["message", "json"],
        )
    }
}

fn deserialize_url<'de, D>(deserializer: D) -> Result<Url, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Url::parse(&s).map_err(de::Error::custom)
}

fn validate_url_scheme(url: &Url) -> Result<(), ValidationError> {
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(ValidationError::new("invalid_url_scheme"));
    }
    Ok(())
}

fn validate_url_host(url: &Url) -> Result<(), ValidationError> {
    if url.host_str().is_none_or(str::is_empty) {
        return Err(ValidationError::new("missing_url_hostname"));
    }
    Ok(())
}

fn validate_url_path(url: &Url) -> Result<(), ValidationError> {
    if url.path().is_empty() || url.path() == "/" {
        return Err(ValidationError::new("invalid_url_path"));
    }
    Ok(())
}

fn validate_weight(weight: u32) -> Result<(), ValidationError> {
    if !(weight_limits::MIN_WEIGHT..=weight_limits::MAX_WEIGHT).contains(&weight) {
        return Err(ValidationError::new("invalid_weight"));
    }
    Ok(())
}

fn default_doh_method() -> DoHMethod {
    DoHMethod::Post
}

fn default_content_type() -> DoHContentType {
    DoHContentType::Message
}

fn default_weight() -> u32 {
    1
}

// DoH 上游端点
#[derive(Debug, Serialize, Deserialize, Validate)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub struct DoHUpstreamEndpointConfig {
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

    #[serde(default = "default_weight")]
    #[validate(custom(
        function = "validate_weight",
        message = "Weight must be between 1-65535"
    ))]
    pub weight: u32,

    #[serde(default = "default_doh_method")]
    pub method: DoHMethod,

    #[serde(default = "default_content_type")]
    pub content_type: DoHContentType,

    #[validate(nested)]
    pub auth: Option<AuthConfig>,
}

impl Clone for DoHUpstreamEndpointConfig {
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

impl PartialEq for DoHUpstreamEndpointConfig {
    fn eq(&self, other: &Self) -> bool {
        self.url.as_str() == other.url.as_str()
            && self.weight == other.weight
            && self.method == other.method
            && self.content_type == other.content_type
            && self.auth == other.auth
    }
}

impl Eq for DoHUpstreamEndpointConfig {}

// DNS（UDP/TCP）上游端点
#[derive(Debug, Serialize, Deserialize, Validate, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub struct DnsUpstreamEndpointConfig {
    pub addr: SocketAddr,

    #[serde(default = "default_weight")]
    #[validate(custom(
        function = "validate_weight",
        message = "Weight must be between 1-65535"
    ))]
    pub weight: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum UpstreamEndpointConfig {
    Doh(DoHUpstreamEndpointConfig),
    Dns(DnsUpstreamEndpointConfig),
}

impl UpstreamEndpointConfig {
    pub fn weight(&self) -> u32 {
        match self {
            Self::Doh(s) => s.weight,
            Self::Dns(s) => s.weight,
        }
    }

    pub fn as_doh(&self) -> Option<&DoHUpstreamEndpointConfig> {
        match self {
            Self::Doh(s) => Some(s),
            Self::Dns(_) => None,
        }
    }

    pub fn as_dns(&self) -> Option<&DnsUpstreamEndpointConfig> {
        match self {
            Self::Doh(_) => None,
            Self::Dns(s) => Some(s),
        }
    }
}

impl Validate for UpstreamEndpointConfig {
    fn validate(&self) -> Result<(), ValidationErrors> {
        match self {
            Self::Doh(s) => s.validate(),
            Self::Dns(s) => s.validate(),
        }
    }
}

fn validate_endpoints_not_empty(
    endpoints: &[UpstreamEndpointConfig],
) -> Result<(), ValidationError> {
    if endpoints.is_empty() {
        return Err(ValidationError::new("empty_endpoints"));
    }
    Ok(())
}

fn validate_group_protocol(group: &UpstreamGroupConfig) -> Result<(), ValidationError> {
    match group.protocol {
        UpstreamProtocol::Doh => {
            for endpoint in &group.endpoints {
                if endpoint.as_doh().is_none() {
                    let mut err = ValidationError::new("invalid_endpoint_variant_for_protocol");
                    err.message = Some(Cow::from(
                        "Upstream group protocol 'doh' requires endpoints to use 'url' entries"
                            .to_string(),
                    ));
                    return Err(err);
                }
            }
            Ok(())
        }
        UpstreamProtocol::Dns => {
            if group.retry.is_some() {
                let mut err = ValidationError::new("dns_group_retry_not_supported");
                err.message = Some(Cow::from(
                    "Upstream group protocol 'dns' does not support 'retry'".to_string(),
                ));
                return Err(err);
            }
            if group.proxy.is_some() {
                let mut err = ValidationError::new("dns_group_proxy_not_supported");
                err.message = Some(Cow::from(
                    "Upstream group protocol 'dns' does not support 'proxy'".to_string(),
                ));
                return Err(err);
            }
            for endpoint in &group.endpoints {
                if endpoint.as_dns().is_none() {
                    let mut err = ValidationError::new("invalid_endpoint_variant_for_protocol");
                    err.message = Some(Cow::from(
                        "Upstream group protocol 'dns' requires endpoints to use 'addr' entries"
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
    function = "validate_group_protocol",
    message = "Upstream group protocol validation failed"
))]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub struct UpstreamGroupConfig {
    #[validate(length(min = 1, message = "Group name cannot be empty"))]
    pub name: String,

    #[serde(default = "default_upstream_protocol")]
    pub protocol: UpstreamProtocol,

    pub policy: LoadBalancingPolicy,

    #[validate(custom(
        function = "validate_endpoints_not_empty",
        message = "Endpoint list cannot be empty"
    ))]
    #[validate(nested)]
    pub endpoints: Vec<UpstreamEndpointConfig>,

    #[validate(nested)]
    pub retry: Option<RetryConfig>,

    pub proxy: Option<String>,
}
