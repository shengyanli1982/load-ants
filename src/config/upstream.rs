use reqwest::Url;
use serde::{
    de::{self, Deserializer},
    Deserialize, Serialize,
};

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
    // 支持Message和Json内容类型
    Get,
    // POST请求方法
    // 只支持Message内容类型，不支持Json内容类型
    Post,
}

// DoH内容类型枚举
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DoHContentType {
    // application/dns-message格式
    Message,
    // application/dns-json格式
    // 注意：根据Google Public DNS规范，JSON格式只支持GET方法
    // 参考：https://developers.google.com/speed/public-dns/docs/doh/json
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

// 上游服务器配置
#[derive(Debug, Serialize, Deserialize)]
pub struct UpstreamServerConfig {
    // DoH服务器URL
    #[serde(deserialize_with = "deserialize_url")]
    pub url: Url,
    // 权重（仅用于加权负载均衡）
    #[serde(default)]
    pub weight: u32,
    // DoH请求方法（GET/POST），默认为POST
    #[serde(default = "default_doh_method")]
    pub method: DoHMethod,
    // DoH内容类型，默认为Message
    #[serde(default = "default_content_type")]
    pub content_type: DoHContentType,
    // 认证配置（可选）
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

// 上游组配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct UpstreamGroupConfig {
    // 组名称
    pub name: String,
    // 负载均衡策略
    pub strategy: LoadBalancingStrategy,
    // 服务器列表
    pub servers: Vec<UpstreamServerConfig>,
    // 重试配置（可选）
    pub retry: Option<RetryConfig>,
    // 代理（可选）
    pub proxy: Option<String>,
}
