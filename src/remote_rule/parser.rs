use crate::{
    config::MatchType,
    error::{AppError, NotImplementedError},
};

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
        Err(AppError::NotImplemented(NotImplementedError(
            "ClashRuleParser has not been implemented yet, it will be supported in future versions"
                .to_string(),
        )))
    }
}
