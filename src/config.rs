use crate::error::ConfigError;
use crate::r#const::{
    cache_limits, http_client_limits, retry_limits, server_defaults, upstream_defaults,
    weight_limits,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, net::SocketAddr, path::Path, str::FromStr, time::Duration};
use tracing::debug;
use url::Url;

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

// 认证类型枚举
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuthType {
    // HTTP基本认证
    Basic,
    // Bearer令牌认证
    Bearer,
}

// 认证配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct AuthConfig {
    // 认证类型（basic/bearer）
    pub r#type: AuthType,
    // 用户名（仅用于basic认证）
    pub username: Option<String>,
    // 密码（仅用于basic认证）
    pub password: Option<String>,
    // 令牌（仅用于bearer认证）
    pub token: Option<String>,
}

// 重试配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct RetryConfig {
    // 重试次数
    pub attempts: u32,
    // 重试初始延迟（秒）
    pub delay: u32,
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

// 上游服务器配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct UpstreamServerConfig {
    // DoH服务器URL
    pub url: String,
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
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
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

// HTTP客户端配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct HttpClientConfig {
    // 连接超时（秒）
    pub connect_timeout: u64,
    // 请求超时（秒）
    pub request_timeout: u64,
    // 空闲连接超时（秒）（可选）
    pub idle_timeout: Option<u64>,
    // TCP Keepalive（秒）（可选）
    pub keepalive: Option<u32>,
    // HTTP用户代理（可选）
    pub agent: Option<String>,
}

impl Default for HttpClientConfig {
    fn default() -> Self {
        Self {
            connect_timeout: http_client_limits::DEFAULT_CONNECT_TIMEOUT,
            request_timeout: http_client_limits::DEFAULT_REQUEST_TIMEOUT,
            idle_timeout: Some(http_client_limits::DEFAULT_IDLE_TIMEOUT),
            keepalive: Some(http_client_limits::DEFAULT_KEEPALIVE),
            agent: None,
        }
    }
}

// 服务器配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    // UDP监听地址
    pub listen_udp: String,
    // TCP监听地址
    pub listen_tcp: String,
    // TCP连接空闲超时（秒）
    #[serde(default = "default_tcp_timeout")]
    pub tcp_timeout: u64,
}

fn default_tcp_timeout() -> u64 {
    server_defaults::DEFAULT_TCP_TIMEOUT
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen_udp: server_defaults::DEFAULT_DNS_LISTEN.to_string(),
            listen_tcp: server_defaults::DEFAULT_DNS_LISTEN.to_string(),
            tcp_timeout: default_tcp_timeout(),
        }
    }
}

// 缓存配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct CacheConfig {
    // 是否启用缓存
    pub enabled: bool,
    // 最大缓存条目数
    pub max_size: usize,
    // 最小TTL（秒）
    pub min_ttl: u32,
    // 最大TTL（秒）
    pub max_ttl: u32,
    // 负面缓存TTL（秒）
    pub negative_ttl: u32,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_size: cache_limits::DEFAULT_SIZE,
            min_ttl: cache_limits::MIN_TTL,
            max_ttl: cache_limits::MAX_TTL,
            negative_ttl: cache_limits::DEFAULT_NEGATIVE_TTL,
        }
    }
}

// 管理服务器配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct AdminConfig {
    // 管理服务器监听地址
    pub listen: String,
}

impl Default for AdminConfig {
    fn default() -> Self {
        Self {
            listen: server_defaults::DEFAULT_ADMIN_LISTEN.to_string(),
        }
    }
}

// 应用配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct Config {
    // 服务器配置
    pub server: ServerConfig,
    // 管理服务器配置
    pub admin: AdminConfig,
    // 缓存配置
    pub cache: CacheConfig,
    // HTTP客户端配置
    pub http_client: HttpClientConfig,
    // 上游组配置
    pub upstream_groups: Vec<UpstreamGroupConfig>,
    // 路由规则配置
    pub static_rules: Vec<RouteRuleConfig>,
}

impl Config {
    // 从文件加载配置
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        debug!("Loading configuration file: {:?}", path.as_ref());
        let content = fs::read_to_string(path).map_err(ConfigError::LoadError)?;
        let config: Config = serde_yaml::from_str(&content).map_err(ConfigError::ParseError)?;
        config.validate()?;
        Ok(config)
    }

    // 创建一个带有默认值的配置
    #[allow(dead_code)]
    pub fn new_with_defaults() -> Self {
        Self::default()
    }

    // 验证配置有效性
    pub fn validate(&self) -> Result<(), ConfigError> {
        // 验证服务器配置
        self.validate_server_config()?;

        // 验证管理服务器配置
        self.validate_admin_config()?;

        // 验证缓存配置
        self.validate_cache_config()?;

        // 验证HTTP客户端配置
        self.validate_http_client_config()?;

        // 验证上游组配置
        self.validate_upstream_groups()?;

        // 验证路由规则配置
        self.validate_static_rules()?;

        Ok(())
    }

    // 验证服务器配置
    fn validate_server_config(&self) -> Result<(), ConfigError> {
        // 验证UDP监听地址
        SocketAddr::from_str(&self.server.listen_udp)
            .map_err(|_| ConfigError::InvalidListenAddress(self.server.listen_udp.clone()))?;

        // 验证TCP监听地址
        SocketAddr::from_str(&self.server.listen_tcp)
            .map_err(|_| ConfigError::InvalidListenAddress(self.server.listen_tcp.clone()))?;

        Ok(())
    }

    // 验证管理服务器配置
    fn validate_admin_config(&self) -> Result<(), ConfigError> {
        // 验证管理服务器监听地址
        SocketAddr::from_str(&self.admin.listen)
            .map_err(|_| ConfigError::InvalidListenAddress(self.admin.listen.clone()))?;

        Ok(())
    }

    // 验证缓存配置
    fn validate_cache_config(&self) -> Result<(), ConfigError> {
        if self.cache.enabled {
            // 验证最大缓存条目数
            if self.cache.max_size == 0 {
                return Err(ConfigError::InvalidCacheConfig(
                    "max_size must be greater than 0".to_string(),
                ));
            }

            // 验证缓存大小是否在合理范围内
            if self.cache.max_size < cache_limits::MIN_SIZE
                || self.cache.max_size > cache_limits::MAX_SIZE
            {
                return Err(ConfigError::InvalidCacheConfig(format!(
                    "max_size must be between {} and {}",
                    cache_limits::MIN_SIZE,
                    cache_limits::MAX_SIZE
                )));
            }

            // 验证TTL配置
            if self.cache.min_ttl > self.cache.max_ttl {
                return Err(ConfigError::InvalidCacheConfig(
                    "min_ttl cannot be greater than max_ttl".to_string(),
                ));
            }

            // 验证min_ttl是否在合理范围内
            if self.cache.min_ttl < cache_limits::MIN_TTL
                || self.cache.min_ttl > cache_limits::MAX_TTL
            {
                return Err(ConfigError::InvalidCacheConfig(format!(
                    "min_ttl must be between {} and {} seconds",
                    cache_limits::MIN_TTL,
                    cache_limits::MAX_TTL
                )));
            }

            // 验证max_ttl是否在合理范围内
            if self.cache.max_ttl < cache_limits::MIN_TTL
                || self.cache.max_ttl > cache_limits::MAX_TTL
            {
                return Err(ConfigError::InvalidCacheConfig(format!(
                    "max_ttl must be between {} and {} seconds",
                    cache_limits::MIN_TTL,
                    cache_limits::MAX_TTL
                )));
            }

            // 验证negative_ttl是否在合理范围内
            if self.cache.negative_ttl < cache_limits::MIN_TTL
                || self.cache.negative_ttl > cache_limits::MAX_TTL
            {
                return Err(ConfigError::InvalidCacheConfig(format!(
                    "negative_ttl must be between {} and {} seconds",
                    cache_limits::MIN_TTL,
                    cache_limits::MAX_TTL
                )));
            }
        }

        Ok(())
    }

    // 验证HTTP客户端配置
    fn validate_http_client_config(&self) -> Result<(), ConfigError> {
        // 验证连接超时
        if self.http_client.connect_timeout == 0 {
            return Err(ConfigError::InvalidHttpClientConfig(
                "connect_timeout must be greater than 0".into(),
            ));
        }

        // 验证连接超时是否在合理范围内
        if self.http_client.connect_timeout < http_client_limits::MIN_CONNECT_TIMEOUT
            || self.http_client.connect_timeout > http_client_limits::MAX_CONNECT_TIMEOUT
        {
            return Err(ConfigError::InvalidHttpClientConfig(format!(
                "connect_timeout must be between {} and {} seconds",
                http_client_limits::MIN_CONNECT_TIMEOUT,
                http_client_limits::MAX_CONNECT_TIMEOUT
            )));
        }

        // 验证请求超时
        if self.http_client.request_timeout == 0 {
            return Err(ConfigError::InvalidHttpClientConfig(
                "request_timeout must be greater than 0".into(),
            ));
        }

        // 验证请求超时是否在合理范围内
        if self.http_client.request_timeout < http_client_limits::MIN_REQUEST_TIMEOUT
            || self.http_client.request_timeout > http_client_limits::MAX_REQUEST_TIMEOUT
        {
            return Err(ConfigError::InvalidHttpClientConfig(format!(
                "request_timeout must be between {} and {} seconds",
                http_client_limits::MIN_REQUEST_TIMEOUT,
                http_client_limits::MAX_REQUEST_TIMEOUT
            )));
        }

        // 验证空闲超时（如果提供）
        if let Some(idle_timeout) = self.http_client.idle_timeout {
            if !(http_client_limits::MIN_IDLE_TIMEOUT..=http_client_limits::MAX_IDLE_TIMEOUT)
                .contains(&idle_timeout)
            {
                return Err(ConfigError::InvalidHttpClientConfig(format!(
                    "idle_timeout must be between {} and {} seconds",
                    http_client_limits::MIN_IDLE_TIMEOUT,
                    http_client_limits::MAX_IDLE_TIMEOUT
                )));
            }
        }

        // 验证keepalive（如果提供）
        if let Some(keepalive) = self.http_client.keepalive {
            if !(http_client_limits::MIN_KEEPALIVE..=http_client_limits::MAX_KEEPALIVE)
                .contains(&keepalive)
            {
                return Err(ConfigError::InvalidHttpClientConfig(format!(
                    "keepalive must be between {} and {} seconds",
                    http_client_limits::MIN_KEEPALIVE,
                    http_client_limits::MAX_KEEPALIVE
                )));
            }
        }

        // 验证用户代理（如果提供）
        if let Some(agent) = &self.http_client.agent {
            if agent.trim().is_empty() {
                return Err(ConfigError::InvalidHttpClientConfig(
                    "agent cannot be empty if provided".into(),
                ));
            }
        }

        Ok(())
    }

    // 验证URL格式
    fn validate_url(url_str: &str, context: &str) -> Result<(), ConfigError> {
        match Url::parse(url_str) {
            Ok(url) => {
                // 验证URL方案
                if url.scheme() != "http" && url.scheme() != "https" {
                    return Err(ConfigError::InvalidUpstreamUrl(format!(
                        "URL '{}' must use http or https scheme (current: {})",
                        url_str,
                        url.scheme()
                    )));
                }

                // 验证主机名存在
                if url.host_str().is_none() || url.host_str().unwrap().is_empty() {
                    return Err(ConfigError::InvalidUpstreamUrl(format!(
                        "URL '{}' must contain a valid hostname",
                        url_str
                    )));
                }

                // 验证路径非空
                if url.path().is_empty() || url.path() == "/" {
                    return Err(ConfigError::InvalidUpstreamUrl(format!(
                        "URL '{}' must contain a valid path",
                        url_str
                    )));
                }

                Ok(())
            }
            Err(e) => Err(ConfigError::InvalidUpstreamUrl(format!(
                "Invalid URL '{}' in {}: {}",
                url_str, context, e
            ))),
        }
    }

    // 验证上游组配置
    fn validate_upstream_groups(&self) -> Result<(), ConfigError> {
        let mut group_names = HashSet::with_capacity(self.upstream_groups.len());

        for group in &self.upstream_groups {
            // 验证组名称唯一性
            if !group_names.insert(&group.name) {
                return Err(ConfigError::DuplicateGroupName(group.name.clone()));
            }

            // 验证组名称非空
            if group.name.trim().is_empty() {
                return Err(ConfigError::InvalidGroupName(
                    "Upstream group name cannot be empty".to_string(),
                ));
            }

            // 验证服务器列表非空
            if group.servers.is_empty() {
                return Err(ConfigError::InvalidGroupName(format!(
                    "Server list for group '{}' cannot be empty",
                    group.name
                )));
            }

            // 验证代理URL格式（如果提供）
            if let Some(proxy) = &group.proxy {
                if !proxy.starts_with("http://")
                    && !proxy.starts_with("https://")
                    && !proxy.starts_with("socks5://")
                {
                    return Err(ConfigError::InvalidGroupName(format!(
                        "Invalid proxy URL format for group '{}', should start with http://, https:// or socks5://",
                        group.name
                    )));
                }
            }

            // 验证负载均衡策略与配置是否一致
            match group.strategy {
                LoadBalancingStrategy::Weighted => {
                    // 验证加权策略中所有服务器是否都设置了权重
                    let sum_weights: u32 = group.servers.iter().map(|s| s.weight).sum();

                    if sum_weights == 0 {
                        return Err(ConfigError::InvalidWeightConfig(format!(
                            "Group '{}' uses weighted strategy, but the sum of all server weights is 0",
                            group.name
                        )));
                    }

                    // 检查有无权重为0的服务器
                    if group.servers.iter().any(|s| s.weight == 0) {
                        return Err(ConfigError::InvalidWeightConfig(format!(
                            "Group '{}' contains servers with weight 0",
                            group.name
                        )));
                    }
                }
                _ => {
                    // 其他策略不需要验证权重
                }
            }

            // 验证每个服务器的URL和认证配置
            for (i, server) in group.servers.iter().enumerate() {
                // 严格验证URL格式
                Self::validate_url(
                    &server.url,
                    &format!("Server #{} in group '{}'", i + 1, group.name),
                )?;

                // 验证服务器权重是否在合理范围内
                if server.weight > 0
                    && (server.weight < weight_limits::MIN_WEIGHT
                        || server.weight > weight_limits::MAX_WEIGHT)
                {
                    return Err(ConfigError::InvalidWeightConfig(format!(
                        "Server weight must be between {} and {}",
                        weight_limits::MIN_WEIGHT,
                        weight_limits::MAX_WEIGHT
                    )));
                }

                // 验证认证配置（如果提供）
                if let Some(auth) = &server.auth {
                    match auth.r#type {
                        AuthType::Basic => {
                            // Basic认证必须提供用户名和密码
                            if auth.username.is_none() || auth.password.is_none() {
                                return Err(ConfigError::InvalidAuthConfig(
                                    "Basic authentication requires username and password".into(),
                                ));
                            }

                            // 验证用户名非空
                            if let Some(username) = &auth.username {
                                if username.trim().is_empty() {
                                    return Err(ConfigError::InvalidAuthConfig(
                                        "Username for Basic authentication cannot be empty".into(),
                                    ));
                                }
                            }

                            // 验证密码非空
                            if let Some(password) = &auth.password {
                                if password.trim().is_empty() {
                                    return Err(ConfigError::InvalidAuthConfig(
                                        "Password for Basic authentication cannot be empty".into(),
                                    ));
                                }
                            }
                        }
                        AuthType::Bearer => {
                            // Bearer认证必须提供令牌
                            if auth.token.is_none() {
                                return Err(ConfigError::InvalidAuthConfig(
                                    "Bearer authentication requires token".into(),
                                ));
                            }

                            // 验证令牌非空
                            if let Some(token) = &auth.token {
                                if token.trim().is_empty() {
                                    return Err(ConfigError::InvalidAuthConfig(
                                        "Token for Bearer authentication cannot be empty".into(),
                                    ));
                                }
                            }
                        }
                    }
                }
            }

            // 验证重试配置（如果提供）
            if let Some(retry) = &group.retry {
                // 验证重试次数
                if retry.attempts == 0 {
                    return Err(ConfigError::ValidationError(format!(
                        "Retry attempts for group '{}' must be greater than 0",
                        group.name
                    )));
                }

                // 验证重试次数是否在合理范围内
                if retry.attempts < retry_limits::MIN_ATTEMPTS
                    || retry.attempts > retry_limits::MAX_ATTEMPTS
                {
                    return Err(ConfigError::ValidationError(format!(
                        "Retry attempts for group '{}' must be between {} and {}",
                        group.name,
                        retry_limits::MIN_ATTEMPTS,
                        retry_limits::MAX_ATTEMPTS
                    )));
                }

                // 验证重试延迟是否在合理范围内
                if retry.delay < retry_limits::MIN_DELAY || retry.delay > retry_limits::MAX_DELAY {
                    return Err(ConfigError::ValidationError(format!(
                        "Retry delay for group '{}' must be between {} and {} seconds",
                        group.name,
                        retry_limits::MIN_DELAY,
                        retry_limits::MAX_DELAY
                    )));
                }
            }
        }

        Ok(())
    }

    // 验证路由规则配置
    fn validate_static_rules(&self) -> Result<(), ConfigError> {
        // 获取所有上游组名称 - 预分配容量
        let group_names: HashSet<_> = self.upstream_groups.iter().map(|g| &g.name).collect();

        for (i, rule) in self.static_rules.iter().enumerate() {
            // 验证匹配模式非空
            if rule.patterns.is_empty() {
                return Err(ConfigError::InvalidRouteRule(format!(
                    "Match rule #{} cannot be empty",
                    i + 1
                )));
            }

            // 验证匹配模式
            for (j, pattern) in rule.patterns.iter().enumerate() {
                if pattern.trim().is_empty() {
                    return Err(ConfigError::InvalidRouteRule(format!(
                        "Match pattern for rule #{} cannot be empty",
                        j + 1
                    )));
                }
                match rule.match_type {
                    MatchType::Exact => {
                        // 确保精确匹配的域名不包含通配符
                        if pattern.contains('*') {
                            return Err(ConfigError::InvalidRouteRule(format!(
                                "Exact match pattern '{}' (rule #{}) should not contain wildcards (*)",
                                pattern,
                                i + 1
                            )));
                        }
                    }
                    MatchType::Wildcard => {
                        // 验证通配符格式
                        if pattern != "*" && !pattern.starts_with("*.") {
                            return Err(ConfigError::InvalidRouteRule(format!(
                                "Wildcard pattern '{}' (rule #{}) is invalid, should be in format '*' or '*.domain.com'",
                                pattern, i + 1
                            )));
                        }

                        // 确保通配符后面有内容（对于*.domain.com格式）
                        if pattern.starts_with("*.") && pattern.len() <= 2 {
                            return Err(ConfigError::InvalidRouteRule(format!(
                                "Wildcard pattern '{}' (rule #{}) is invalid, must have content after '*.'",
                                pattern, i + 1
                            )));
                        }
                    }
                    MatchType::Regex => {
                        // 验证正则表达式
                        match Regex::new(pattern) {
                            Ok(_) => (), // 正则表达式有效
                            Err(e) => {
                                return Err(ConfigError::InvalidRouteRule(format!(
                                    "Regular expression '{}' (rule #{}) is invalid: {}",
                                    pattern,
                                    i + 1,
                                    e
                                )));
                            }
                        }
                    }
                }
            }

            // 验证动作和目标
            match rule.action {
                RouteAction::Forward => {
                    // 转发动作必须提供目标上游组
                    match &rule.target {
                        Some(target) => {
                            // 验证目标上游组是否存在
                            if !group_names.contains(target) {
                                return Err(ConfigError::NonExistentGroupReference(format!(
                                    "Rule #{} references non-existent upstream group '{}'",
                                    i + 1,
                                    target
                                )));
                            }
                        }
                        None => {
                            return Err(ConfigError::InvalidRouteRule(format!(
                                "Rule #{} with Forward action must provide a target field",
                                i + 1
                            )));
                        }
                    }
                }
                RouteAction::Block => {
                    // Block动作不需要目标上游组，但如果提供了，应检查其值是否有效
                    if let Some(target) = &rule.target {
                        if !target.trim().is_empty() && !group_names.contains(target) {
                            return Err(ConfigError::InvalidRouteRule(
                                format!("Rule #{} with Block action references non-existent upstream group '{}'. Block action does not need a target.", 
                                        i + 1, target)
                            ));
                        }
                    }
                }
            }
        }

        Ok(())
    }

    // 解析 keepalive 配置
    #[allow(dead_code)]
    pub fn parse_keepalive(&self) -> Option<Duration> {
        self.http_client
            .keepalive
            .map(|seconds| Duration::from_secs(seconds as u64))
    }
}

// 默认配置实现
impl Default for Config {
    fn default() -> Self {
        Config {
            server: ServerConfig::default(),
            admin: AdminConfig::default(),
            cache: CacheConfig::default(),
            http_client: HttpClientConfig::default(),
            upstream_groups: vec![UpstreamGroupConfig {
                name: upstream_defaults::DEFAULT_GROUP_NAME.to_string(),
                strategy: LoadBalancingStrategy::RoundRobin,
                servers: vec![UpstreamServerConfig {
                    url: upstream_defaults::DEFAULT_DOH_SERVER.to_string(),
                    weight: upstream_defaults::DEFAULT_WEIGHT,
                    method: DoHMethod::Post,
                    content_type: DoHContentType::Message,
                    auth: None,
                }],
                retry: Some(RetryConfig {
                    attempts: 3,
                    delay: 1,
                }),
                proxy: None,
            }],
            static_rules: vec![RouteRuleConfig {
                match_type: MatchType::Wildcard,
                patterns: vec!["*".to_string()],
                action: RouteAction::Forward,
                target: Some(upstream_defaults::DEFAULT_GROUP_NAME.to_string()),
            }],
        }
    }
}
