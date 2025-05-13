use crate::config::{MatchType, RouteAction, RouteRuleConfig};
use crate::error::{AppError, ConfigError};
use crate::metrics::METRICS;
use crate::r#const::rule_type_labels;
use hickory_proto::rr::Name;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use tracing::debug;

// 编译后的正则表达式规则
struct CompiledRegexRule {
    // 原始模式
    pattern: String,
    // 编译后的正则表达式
    regex: Regex,
    // 路由动作
    action: RouteAction,
    // 目标上游组
    target: Option<String>,
}

// 通配符规则，以特定性排序
struct WildcardRule {
    // 原始模式
    pattern: String,
    // 特定性（通配符后域名部分的段数，越多越具体）
    specificity: usize,
    // 路由动作
    action: RouteAction,
    // 目标上游组
    target: Option<String>,
}

// DNS请求路由引擎
pub struct Router {
    // 精确匹配规则
    exact_rules: HashMap<String, (RouteAction, Option<String>)>,
    // 通配符匹配规则
    wildcard_rules: Vec<WildcardRule>,
    // 正则表达式匹配规则
    regex_rules: Vec<CompiledRegexRule>,
    // 正则表达式预筛选映射
    regex_prefilter: HashMap<String, HashSet<usize>>,
    // 已排序的规则列表（按优先级）
    sorted_rules: Vec<(RouteAction, Option<String>)>,
}

// 路由匹配结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteMatch {
    // 匹配的域名
    pub domain: String,
    // 路由动作
    pub action: RouteAction,
    // 目标上游组
    pub target: Option<String>,
    // 匹配规则类型
    pub rule_type: String,
    // 匹配的模式
    pub pattern: String,
}

impl Router {
    // 创建新的路由引擎
    pub fn new(rules: Vec<RouteRuleConfig>) -> Result<Self, ConfigError> {
        let mut exact_rules = HashMap::new();
        let mut wildcard_rules = Vec::new();
        let mut regex_rules = Vec::new();
        
        // 处理所有规则
        for rule in rules {
            match rule.match_type {
                MatchType::Exact => {
                    // 添加精确匹配规则
                    exact_rules.insert(
                        rule.pattern.clone(),
                        (rule.action, rule.target),
                    );
                }
                MatchType::Wildcard => {
                    // 计算通配符规则的特定性
                    let specificity = if rule.pattern == "*" {
                        0 // 全局通配符最不具体
                    } else {
                        // 计算通配符后部分的域名段数，越多越具体
                        rule.pattern[2..].split('.').count() // 去掉"*."前缀
                    };
                    
                    // 添加通配符规则
                    wildcard_rules.push(WildcardRule {
                        pattern: rule.pattern,
                        specificity,
                        action: rule.action,
                        target: rule.target,
                    });
                }
                MatchType::Regex => {
                    // 编译正则表达式
                    let regex = Regex::new(&rule.pattern)?;
                    
                    // 添加正则表达式规则
                    regex_rules.push(CompiledRegexRule {
                        pattern: rule.pattern,
                        regex,
                        action: rule.action,
                        target: rule.target,
                    });
                }
            }
        }
        
        // 对通配符规则按特定性排序（从高到低）
        wildcard_rules.sort_by(|a, b| b.specificity.cmp(&a.specificity));
        
        // 创建正则表达式预筛选映射
        let regex_prefilter = Self::build_regex_prefilter(&regex_rules);
        
        // 更新路由规则数量指标
        METRICS.route_rules_count().with_label_values(&[rule_type_labels::EXACT]).set(exact_rules.len() as f64);
        METRICS.route_rules_count().with_label_values(&[rule_type_labels::WILDCARD]).set(wildcard_rules.len() as f64);
        METRICS.route_rules_count().with_label_values(&[rule_type_labels::REGEX]).set(regex_rules.len() as f64);
        
        let mut router = Self {
            exact_rules,
            wildcard_rules,
            regex_rules,
            regex_prefilter,
            sorted_rules: Vec::new(),
        };
        
        // 对所有规则进行排序，确保它们按照正确的优先级顺序处理
        let sorted_rules = router.sort_rules()?;
        debug!("Sort {} routing rules in total", sorted_rules.len());
        
        // 保存排序后的规则
        router.sorted_rules = sorted_rules;
        
        Ok(router)
    }

    // 构建正则表达式预筛选映射
    fn build_regex_prefilter(regex_rules: &[CompiledRegexRule]) -> HashMap<String, HashSet<usize>> {
        let mut prefilter = HashMap::new();
        
        for (index, rule) in regex_rules.iter().enumerate() {
            // 尝试从正则表达式中提取固定子串作为预筛选键
            // 这是一个简单的启发式方法，实际上可以使用更复杂的算法
            let pattern = &rule.pattern;
            
            // 提取顶级域名和二级域名作为特征
            if let Some(tld_pos) = pattern.rfind('.') {
                if let Some(domain_part) = pattern.get(tld_pos..) {
                    // 使用顶级域名作为特征（如 .com, .org 等）
                    prefilter
                        .entry(domain_part.to_string())
                        .or_insert_with(HashSet::new)
                        .insert(index);
                }
                
                // 尝试提取二级域名
                if let Some(sld_pos) = pattern[..tld_pos].rfind('.') {
                    if let Some(domain_part) = pattern.get(sld_pos..) {
                        // 使用二级域名作为特征（如 .example.com）
                        prefilter
                            .entry(domain_part.to_string())
                            .or_insert_with(HashSet::new)
                            .insert(index);
                    }
                }
            }
            
            // 为所有正则表达式规则添加一个全局键，以确保所有规则都能被尝试
            prefilter
                .entry("*".to_string())
                .or_insert_with(HashSet::new)
                .insert(index);
        }
        
        prefilter
    }

    // 查找匹配的规则
    pub fn find_match(&self, query_name: &Name) -> Result<RouteMatch, AppError> {
        // 将查询名称转换为小写字符串，便于匹配
        let domain = query_name.to_string().to_lowercase();
        
        // 尝试精确匹配
        if let Some((action, target)) = self.exact_rules.get(&domain) {
            debug!(
                "Rule match: Exact match '{}' -> target: {:?}",
                domain, target
            );
            
            // 记录路由匹配指标
            METRICS.route_matches_total()
                .with_label_values(&[rule_type_labels::EXACT, target.as_deref().unwrap_or(rule_type_labels::NO_TARGET)])
                .inc();
            
            return Ok(RouteMatch {
                domain: domain.clone(),
                action: action.clone(),
                target: target.clone(),
                rule_type: "exact".to_string(),
                pattern: domain.clone(),
            });
        }
        
        // 尝试通配符匹配（按特定性从高到低）
        for rule in &self.wildcard_rules {
            if self.match_wildcard(&domain, &rule.pattern) {
                debug!(
                    "Rule match: Wildcard match '{}' -> pattern: '{}', target: {:?}",
                    domain, rule.pattern, rule.target
                );
                
                // 记录路由匹配指标
                METRICS.route_matches_total()
                    .with_label_values(&[rule_type_labels::WILDCARD, rule.target.as_deref().unwrap_or(rule_type_labels::NO_TARGET)])
                    .inc();
                
                return Ok(RouteMatch {
                    domain: domain.clone(),
                    action: rule.action.clone(),
                    target: rule.target.clone(),
                    rule_type: "wildcard".to_string(),
                    pattern: rule.pattern.clone(),
                });
            }
        }
        
        // 尝试正则表达式匹配 (使用预筛选优化)
        let domain_parts: Vec<&str> = domain.split('.').collect();
        let _domain_len = domain_parts.len();
        
        // 查找符合条件的候选键
        let mut candidate_keys = Vec::new();
        candidate_keys.push("*".to_string()); // 通配符键
        
        if let Some(tld_pos) = domain.rfind('.') {
            if let Some(tld) = domain.get(tld_pos..) {
                candidate_keys.push(tld.to_string());
            }
            
            if let Some(sld_pos) = domain[..tld_pos].rfind('.') {
                if let Some(sld) = domain.get(sld_pos..) {
                    candidate_keys.push(sld.to_string());
                }
            }
        }
        
        // 收集所有可能匹配的规则索引
        let mut candidate_indices: HashSet<usize> = HashSet::new();
        
        for key in candidate_keys {
            if let Some(indices) = self.regex_prefilter.get(&key) {
                candidate_indices.extend(indices);
            }
        }
        
        // 尝试匹配候选正则表达式
        for &index in &candidate_indices {
            let rule: &CompiledRegexRule = &self.regex_rules[index];
            if rule.regex.is_match(&domain) {
                debug!(
                    "Rule match: Regex match '{}' -> pattern: '{}', target: {:?}",
                    domain, rule.pattern, rule.target
                );
                
                // 记录路由匹配指标
                METRICS.route_matches_total()
                    .with_label_values(&[rule_type_labels::REGEX, rule.target.as_deref().unwrap_or(rule_type_labels::NO_TARGET)])
                    .inc();
                
                return Ok(RouteMatch {
                    domain: domain.clone(),
                    action: rule.action.clone(),
                    target: rule.target.clone(),
                    rule_type: "regex".to_string(),
                    pattern: rule.pattern.clone(),
                });
            }
        }
        
        // 没有匹配的规则
        Err(AppError::NoRouteMatch(domain))
    }

    // 返回所有规则的组合列表，按照优先级顺序排列
    // 返回所有规则的组合列表，按照优先级顺序排列
    // 
    // 此方法在应用启动后加载规则配置时应被调用，用于确保路由规则按正确的优先级顺序处理：
    // 1. 精确匹配规则 (最高优先级)
    // 2. 通配符规则 (按特定性排序)
    // 3. 正则表达式规则
    // 
    // 返回的列表包含所有路由动作及其可能的目标上游组
    pub fn sort_rules(&self) -> Result<Vec<(RouteAction, Option<String>)>, ConfigError> {
        let mut rules = Vec::new();
        
        // 添加精确匹配规则
        for rule in self.exact_rules.values() {
            rules.push(rule.clone());
        }
        
        // 添加通配符规则
        for rule in &self.wildcard_rules {
            rules.push((rule.action.clone(), rule.target.clone()));
        }
        
        // 添加正则表达式规则
        for rule in &self.regex_rules {
            rules.push((rule.action.clone(), rule.target.clone()));
        }
        
        Ok(rules)
    }

    // 获取排序后的规则列表
    #[allow(dead_code)]
    pub fn get_sorted_rules(&self) -> &Vec<(RouteAction, Option<String>)> {
        &self.sorted_rules
    }

    // 检查域名是否匹配通配符模式
    fn match_wildcard(&self, domain: &str, pattern: &str) -> bool {
        if pattern == "*" {
            // 全局通配符匹配所有域名
            return true;
        }
        
        // 对于 "*.example.com" 格式的模式
        if let Some(suffix) = pattern.strip_prefix("*.") {
            domain.ends_with(suffix) && {
                // 确保匹配的是整个域名或子域名
                let prefix_len = domain.len() - suffix.len();
                prefix_len == 0 || domain.as_bytes()[prefix_len - 1] == b'.'
            }
        } else {
            false
        }
    }
}
