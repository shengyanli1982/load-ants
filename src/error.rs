use hickory_proto::error::ProtoError;
use std::io;
use thiserror::Error;
use crate::upstream::{InvalidProxyConfig, HttpClientError};

// Unified error type
#[derive(Error, Debug)]
pub enum AppError {
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("DNS resolution error: {0}")]
    DnsProto(#[from] ProtoError),

    #[error("HTTP request error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("HTTP middleware error: {0}")]
    HttpMiddleware(String),

    #[error("Upstream error: {0}")]
    Upstream(String),

    #[error("Router error: {0}")]
    Router(String),

    #[error("Cache error: {0}")]
    Cache(String),

    #[error("Timeout error")]
    Timeout,

    #[error("No available upstream servers")]
    NoUpstreamAvailable,

    #[error("Upstream group not found: {0}")]
    UpstreamGroupNotFound(String),

    #[error("Internal error: {0}")]
    Internal(String),
    
    #[error("JSON serialization/deserialization error: {0}")]
    JsonError(String),

    #[error("No matching route rule: {0}")]
    NoRouteMatch(String),

    #[error("Invalid proxy configuration: {0}")]
    InvalidProxy(#[from] InvalidProxyConfig),

    #[error("HTTP client error: {0}")]
    HttpError(#[from] HttpClientError),
}

impl From<reqwest_middleware::Error> for AppError {
    fn from(err: reqwest_middleware::Error) -> Self {
        match err {
            reqwest_middleware::Error::Reqwest(e) => Self::Http(e),
            _ => Self::HttpMiddleware(err.to_string()),
        }
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        Self::JsonError(err.to_string())
    }
}

// Configuration error type
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to load configuration file: {0}")]
    LoadError(#[from] io::Error),

    #[error("YAML parsing error: {0}")]
    ParseError(#[from] serde_yaml::Error),

    #[error("Invalid server listen address: {0}")]
    InvalidListenAddress(String),

    #[error("Invalid upstream URL: {0}")]
    InvalidUpstreamUrl(String),

    #[error("Invalid upstream group name: {0}")]
    InvalidGroupName(String),

    #[error("Missing required configuration: {0}")]
    MissingRequiredConfig(String),

    #[error("Invalid load balancing strategy: {0}")]
    InvalidLoadBalancingStrategy(String),

    #[error("Invalid weight configuration: {0}")]
    InvalidWeightConfig(String),

    #[error("Invalid authentication configuration: {0}")]
    InvalidAuthConfig(String),

    #[error("Invalid route rule: {0}")]
    InvalidRouteRule(String),

    #[error("Invalid regular expression: {0}")]
    InvalidRegex(#[from] regex::Error),

    #[error("Invalid cache configuration: {0}")]
    InvalidCacheConfig(String),

    #[error("Invalid HTTP client configuration: {0}")]
    InvalidHttpClientConfig(String),

    #[error("Duplicate upstream group name: {0}")]
    DuplicateGroupName(String),

    #[error("Route rule references non-existent upstream group: {0}")]
    NonExistentGroupReference(String),

    #[error("Configuration validation error: {0}")]
    ValidationError(String),
} 