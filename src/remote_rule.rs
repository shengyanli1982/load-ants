use crate::config::{
    HttpClientConfig, MatchType, RemoteRuleConfig, RetryConfig, RouteRuleConfig, RuleFormat,
};
use crate::error::{AppError, HttpClientError, InvalidProxyConfig};
use crate::r#const::{retry_limits, rule_action_labels};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use retry_policies::Jitter;
use std::time::Duration;
use tracing::{debug, error, info};

/// 规则解析器特征，定义解析不同格式规则文件的接口
pub trait RuleParser {
    /// 解析规则内容，返回(域名模式, 匹配类型)的列表
    fn parse(&self, content: &str) -> Result<Vec<(String, MatchType)>, AppError>;
}

/// V2Ray规则解析器
pub struct V2RayRuleParser;

impl RuleParser for V2RayRuleParser {
    fn parse(&self, content: &str) -> Result<Vec<(String, MatchType)>, AppError> {
        let mut rules = Vec::new();

        for line in content.lines() {
            // 跳过空行和注释
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // 处理不同类型的规则
            if let Some(stripped) = line.strip_prefix("full:") {
                // 精确匹配规则: full:example.com
                let domain = stripped.trim().to_string();
                if !domain.is_empty() {
                    rules.push((domain, MatchType::Exact));
                }
            } else if let Some(stripped) = line.strip_prefix("regexp:") {
                // 正则表达式匹配规则: regexp:.*\.example\.com$
                let pattern = stripped.trim().to_string();
                if !pattern.is_empty() {
                    rules.push((pattern, MatchType::Regex));
                }
            } else {
                // 通配符匹配规则（默认）: example.com -> *.example.com
                let domain = line.trim().to_string();
                if !domain.is_empty() {
                    // 如果域名不是以*开头，转换为*.domain.com格式
                    if domain == "*" {
                        rules.push((domain, MatchType::Wildcard));
                    } else {
                        rules.push((format!("*.{}", domain), MatchType::Wildcard));
                    }
                }
            }
        }

        Ok(rules)
    }
}

/// Clash规则解析器（为未来扩展预留）
pub struct ClashRuleParser;

impl RuleParser for ClashRuleParser {
    fn parse(&self, _content: &str) -> Result<Vec<(String, MatchType)>, AppError> {
        Err(AppError::NotImplemented(
            "ClashRuleParser has not been implemented yet, it will be supported in future versions"
                .to_string(),
        ))
    }
}

/// 远程规则加载器
pub struct RemoteRuleLoader {
    client: ClientWithMiddleware,
    config: RemoteRuleConfig,
    parser: Box<dyn RuleParser>,
}

impl RemoteRuleLoader {
    /// 创建新的远程规则加载器
    pub fn new(config: RemoteRuleConfig, http_config: HttpClientConfig) -> Result<Self, AppError> {
        let client =
            Self::create_http_client(&http_config, config.proxy.as_deref(), config.retry.as_ref())?;

        // 根据配置的格式选择解析器
        let parser: Box<dyn RuleParser> = match config.format {
            RuleFormat::V2ray => Box::new(V2RayRuleParser),
            RuleFormat::Clash => Box::new(ClashRuleParser),
        };

        Ok(Self {
            client,
            config,
            parser,
        })
    }

    /// 创建HTTP客户端
    fn create_http_client(
        config: &HttpClientConfig,
        proxy: Option<&str>,
        retry_config: Option<&RetryConfig>,
    ) -> Result<ClientWithMiddleware, AppError> {
        debug!(
            "Creating HTTP client for remote rule, config: {:?}, proxy: {:?}, retry_config: {:?}",
            config, proxy, retry_config
        );

        // 创建客户端构建器
        let mut client_builder = reqwest::ClientBuilder::new()
            .danger_accept_invalid_certs(true) // 允许无效证书，用于内部自签名证书
            .connect_timeout(Duration::from_secs(config.connect_timeout))
            .timeout(Duration::from_secs(config.request_timeout));

        // 配置TCP keepalive
        if let Some(ref keepalive) = config.keepalive {
            client_builder = client_builder.tcp_keepalive(Duration::from_secs(*keepalive as u64));
        }

        // 配置空闲连接超时
        if let Some(idle_timeout) = config.idle_timeout {
            client_builder = client_builder.pool_idle_timeout(Duration::from_secs(idle_timeout));
        }

        // 配置用户代理
        if let Some(ref agent) = config.agent {
            client_builder = client_builder.user_agent(agent);
        }

        // 配置代理
        if let Some(proxy_url) = proxy {
            client_builder = client_builder.proxy(reqwest::Proxy::all(proxy_url).map_err(|e| {
                AppError::InvalidProxy(InvalidProxyConfig(format!(
                    "Proxy configuration error: {}",
                    e
                )))
            })?);
        }

        // 创建基础HTTP客户端
        let client = client_builder.build().map_err(|e| {
            AppError::HttpError(HttpClientError(format!(
                "Failed to create HTTP client: {}",
                e
            )))
        })?;

        // 配置重试策略（根据组的重试配置）
        let middleware_client = if let Some(retry) = retry_config {
            // 使用指数退避策略，基于组的重试配置
            let retry_policy = ExponentialBackoff::builder()
                // 设置重试时间间隔的上下限
                .retry_bounds(
                    Duration::from_secs(retry_limits::MIN_DELAY as u64),
                    Duration::from_secs(retry_limits::MAX_DELAY as u64),
                )
                // 设置指数退避的基数
                .base(retry.delay)
                // 使用有界抖动来避免多个客户端同时重试
                .jitter(Jitter::Bounded)
                // 配置最大重试次数
                .build_with_max_retries(retry.attempts);

            ClientBuilder::new(client)
                .with(RetryTransientMiddleware::new_with_policy(retry_policy))
                .build()
        } else {
            // 不进行重试
            ClientBuilder::new(client).build()
        };

        Ok(middleware_client)
    }

    /// 加载远程规则
    pub async fn load(&self) -> Result<Vec<RouteRuleConfig>, AppError> {
        debug!("Loading domains from URL: {:?}", self.config.url);

        // 构建请求
        let mut request = self.client.get(&self.config.url);

        // 添加认证信息
        if let Some(auth) = &self.config.auth {
            request = match auth.r#type {
                crate::config::AuthType::Basic => {
                    let username = auth.username.as_deref().unwrap_or("");
                    let password = auth.password.as_deref().unwrap_or("");
                    request.basic_auth(username, Some(password))
                }
                crate::config::AuthType::Bearer => {
                    let token = auth.token.as_deref().unwrap_or("");
                    request.bearer_auth(token)
                }
            };
        }

        // 发送请求并获取响应
        let response = request.send().await?;

        // 检查响应状态
        if !response.status().is_success() {
            return Err(AppError::Upstream(format!(
                "Failed to fetch remote rules, status: {}",
                response.status()
            )));
        }

        // 获取响应内容
        let content = response.text().await?;

        // 检查规则文件大小
        let content_size = content.len();
        if content_size > self.config.max_size {
            return Err(AppError::Upstream(format!(
                "Remote rule file size ({} bytes) exceeds configured limit ({} bytes)",
                content_size, self.config.max_size
            )));
        }

        // 解析规则
        let parsed_rules = self.parser.parse(&content)?;

        // 将解析后的规则转换为RouteRuleConfig
        let mut route_rules = Vec::new();

        // 根据匹配类型分组规则
        let mut exact_patterns = Vec::new();
        let mut wildcard_patterns = Vec::new();
        let mut regex_patterns = Vec::new();

        // 计数器，用于记录各类型规则数量
        let mut exact_count = 0;
        let mut wildcard_count = 0;
        let mut regex_count = 0;

        // 遍历解析后的规则，按类型分组
        for (pattern, match_type) in &parsed_rules {
            match match_type {
                MatchType::Exact => {
                    exact_patterns.push(pattern.clone());
                    exact_count += 1;
                }
                MatchType::Wildcard => {
                    wildcard_patterns.push(pattern.clone());
                    wildcard_count += 1;
                }
                MatchType::Regex => {
                    regex_patterns.push(pattern.clone());
                    regex_count += 1;
                }
            }
        }

        // 获取规则动作标签
        let action_label = match self.config.action {
            crate::config::RouteAction::Forward => rule_action_labels::FORWARD,
            crate::config::RouteAction::Block => rule_action_labels::BLOCK,
        };

        // 创建精确匹配规则（如果有）
        if !exact_patterns.is_empty() {
            route_rules.push(RouteRuleConfig {
                match_type: MatchType::Exact,
                patterns: exact_patterns.clone(), // 合并精确匹配规则，存放于patterns中
                action: self.config.action.clone(),
                target: self.config.target.clone(),
            });
        }

        // 创建通配符匹配规则（如果有）
        if !wildcard_patterns.is_empty() {
            route_rules.push(RouteRuleConfig {
                match_type: MatchType::Wildcard,
                patterns: wildcard_patterns.clone(), // 合并通配符匹配规则，存放于patterns中
                action: self.config.action.clone(),
                target: self.config.target.clone(),
            });
        }

        // 创建正则表达式匹配规则（如果有）
        if !regex_patterns.is_empty() {
            route_rules.push(RouteRuleConfig {
                match_type: MatchType::Regex,
                patterns: regex_patterns.clone(), // 合并正则表达式匹配规则，存放于patterns中
                action: self.config.action.clone(),
                target: self.config.target.clone(),
            });
        }

        info!(
            "Loaded {} domains from {:?} ({}): {} exact, {} wildcard, {} regex",
            parsed_rules.len(),
            self.config.url,
            action_label,
            exact_count,
            wildcard_count,
            regex_count
        );

        Ok(route_rules)
    }
}

/// 加载所有远程规则并与本地规则合并
pub async fn load_and_merge_rules(
    remote_configs: &[RemoteRuleConfig],
    static_rules: &[RouteRuleConfig],
    http_config: &HttpClientConfig,
) -> Result<Vec<RouteRuleConfig>, AppError> {
    // 创建一个规则列表
    let mut merged_rules = static_rules.to_vec();

    // 加载每个远程规则
    for config in remote_configs {
        match RemoteRuleLoader::new(config.clone(), http_config.clone()) {
            Ok(loader) => {
                match loader.load().await {
                    Ok(remote_rules) => {
                        // 将远程规则添加到合并规则列表
                        merged_rules.extend(remote_rules.to_vec());
                    }
                    Err(e) => {
                        // 记录错误但继续处理其他规则
                        error!("Failed to load domains from {:?}: {}", config.url, e);
                    }
                }
            }
            Err(e) => {
                // 记录错误但继续处理其他规则
                error!(
                    "Failed to create remote rule loader for {}: {}",
                    config.url, e
                );
            }
        }
    }

    Ok(merged_rules)
}
