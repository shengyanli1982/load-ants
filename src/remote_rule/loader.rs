use crate::config::{
    HttpClientConfig, MatchType, RemoteRuleConfig, RouteRuleConfig, RuleFormat,
};
use crate::error::AppError;
use crate::r#const::rule_action_labels;
use crate::upstream::HttpClient;
use reqwest_middleware::ClientWithMiddleware;
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
            HttpClient::create(&http_config, config.proxy.as_deref(), config.retry.as_ref())?;

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

        // 预检 Content-Length，避免读取过大的响应体
        if let Some(content_length) = response.content_length() {
            if content_length as usize > self.config.max_size {
                return Err(AppError::Upstream(format!(
                    "Remote rule Content-Length ({}) exceeds configured limit ({})",
                    content_length, self.config.max_size
                )));
            }
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

        // 先计数再预分配容量，避免按 parsed_rules.len() 三倍预分配造成不必要的内存峰值。
        let mut exact_count = 0;
        let mut wildcard_count = 0;
        let mut regex_count = 0;
        for (_, match_type) in &parsed_rules {
            match match_type {
                MatchType::Exact => exact_count += 1,
                MatchType::Wildcard => wildcard_count += 1,
                MatchType::Regex => regex_count += 1,
            }
        }

        // 根据匹配类型分组规则（直接 move pattern，避免 clone）
        let mut exact_patterns = Vec::with_capacity(exact_count);
        let mut wildcard_patterns = Vec::with_capacity(wildcard_count);
        let mut regex_patterns = Vec::with_capacity(regex_count);

        for (pattern, match_type) in parsed_rules {
            match match_type {
                MatchType::Exact => exact_patterns.push(pattern),
                MatchType::Wildcard => wildcard_patterns.push(pattern),
                MatchType::Regex => regex_patterns.push(pattern),
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
            exact_count + wildcard_count + regex_count,
            self.config.url,
            action_label,
            exact_count,
            wildcard_count,
            regex_count
        );

        Ok(route_rules)
    }
}
