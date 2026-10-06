use crate::{config::MatchType, error::AppError};

/// 规则解析器特征，定义解析不同格式规则文件的接口
pub trait RuleParser: Send + Sync {
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
                // 裸域名规则（默认）: V2Ray 语义 = apex 本身 + 所有子域
                let domain = line.trim().to_string();
                if !domain.is_empty() {
                    if domain == "*" {
                        rules.push((domain, MatchType::Wildcard));
                    } else {
                        // D-01: `*.domain` 通配符只覆盖真正的子域（trie 匹配保留
                        // 至少 1 个 label），必须额外生成一条 exact 规则覆盖 apex
                        // 本身，否则裸域名列表对 apex 查询永不命中。
                        if !domain.starts_with('*') {
                            rules.push((domain.clone(), MatchType::Exact));
                        }
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
