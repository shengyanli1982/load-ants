use crate::config::{
    HttpClientConfig, MatchType, RemoteRuleConfig, RetryConfig, RouteRuleConfig, RuleFormat,
};
use crate::error::{AppError, HttpClientError, InvalidProxyConfig};
use crate::r#const::{retry_limits, rule_action_labels};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use retry_policies::Jitter;
use std::time::Duration;
use tracing::{debug, info};

use super::parser::{RuleParser, V2RayRuleParser};

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
            // RuleFormat::Clash => Box::new(ClashRuleParser),
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
                    Duration::from_secs(retry.delay as u64),
                    Duration::from_secs(retry_limits::MAX_DELAY as u64),
                )
                // 设置指数退避的基数, 记得这里一定要大于 1，要不然退避时间会一直不变大
                .base(2)
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
                patterns: exact_patterns,
                action: self.config.action,
                target: self.config.target.clone(),
            });
        }

        // 创建通配符匹配规则（如果有）
        if !wildcard_patterns.is_empty() {
            route_rules.push(RouteRuleConfig {
                match_type: MatchType::Wildcard,
                patterns: wildcard_patterns,
                action: self.config.action,
                target: self.config.target.clone(),
            });
        }

        // 创建正则表达式匹配规则（如果有）
        if !regex_patterns.is_empty() {
            route_rules.push(RouteRuleConfig {
                match_type: MatchType::Regex,
                patterns: regex_patterns,
                action: self.config.action,
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
