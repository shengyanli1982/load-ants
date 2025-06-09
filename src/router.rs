use crate::config::{MatchType, RouteAction, RouteRuleConfig};
use crate::error::{AppError, ConfigError};
use crate::metrics::METRICS;
use crate::r#const::{router::wildcards, rule_action_labels, rule_source_labels, rule_type_labels};
use hickory_proto::rr::Name;
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use tracing::debug;

// 定义正则表达式特殊字符常量
lazy_static! {
    static ref REGEX_SPECIAL_CHARS: [char; 15] =
        ['\\', '^', '$', '.', '|', '?', '*', '+', '(', ')', '[', ']', '{', '}', '-',];
}

// 编译后的正则表达式规则
struct CompiledRegexRule {
    // 原始模式
    pattern: String,
    // 编译后的正则表达式
    regex: Regex,
    // 路由动作
    #[allow(dead_code)]
    action: RouteAction,
    // 目标上游组
    target: Option<Arc<String>>,
}

// 添加类型别名用于简化复杂类型
/// 路由规则元组类型，包含(模式, 动作, 目标)
pub type RouteRuleTuple = (Option<String>, RouteAction, Option<Arc<String>>);

// DNS请求路由引擎
// 实现特点：
// 1. 分离存储：block规则和forward规则分开存储，确保block规则始终具有更高优先级
// 2. 查询优化：在匹配算法中，始终先检查所有类型的block规则，再检查forward规则
// 3. 规则排序：维持了原有的精确匹配>通配符匹配>正则匹配>全局通配符的类型优先级
// 4. 性能保障：保留了高效的查询机制，如使用HashMap进行精确匹配，BTreeMap进行后缀树匹配，以及正则表达式预筛选
pub struct Router {
    // 精确匹配规则 - 分离block和forward规则
    exact_block_rules: HashMap<String, Option<Arc<String>>>,
    exact_forward_rules: HashMap<String, Option<Arc<String>>>,

    // 通配符匹配规则树 - 分离block和forward规则
    // 键为反转后的域名后缀，值为(目标,原始模式)
    wildcard_block_rules: BTreeMap<String, Option<Arc<String>>>,
    wildcard_forward_rules: BTreeMap<String, Option<Arc<String>>>,

    // 全局通配符规则（模式为 "*"）- 分离block和forward规则
    global_wildcard_block_rule: Option<(Option<Arc<String>>, String)>,
    global_wildcard_forward_rule: Option<(Option<Arc<String>>, String)>,

    // 正则表达式匹配规则 - 分离block和forward规则
    regex_block_rules: Vec<CompiledRegexRule>,
    regex_forward_rules: Vec<CompiledRegexRule>,

    // 正则表达式预筛选映射
    regex_block_prefilter: HashMap<String, HashSet<usize>>,
    regex_forward_prefilter: HashMap<String, HashSet<usize>>,
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
    pub rule_type: &'static str,
    // 匹配的模式
    pub pattern: String,
}

impl Router {
    // 反转域名标签，例如 "example.com" -> "com.example"
    // 这个函数在 `find_match` 方法中被多次调用，因此使用 `#[inline(always)]` 优化
    #[inline(always)]
    fn reverse_domain_labels(domain_suffix: &str) -> String {
        // 优化思路：
        // 1. 如果没有点，直接返回克隆，这是最快路径，避免不必要的操作。
        // 2. 使用 `rsplit` 从后往前遍历各部分，这天然地符合反转的需求。
        // 3. 预先为新字符串分配足够的容量。
        // 4. 手动遍历迭代器并拼接字符串，以避免使用 `.join()` 时可能产生的额外开销。
        if !domain_suffix.contains(wildcards::DOT) {
            return domain_suffix.to_string();
        }

        let mut reversed = String::with_capacity(domain_suffix.len());
        let mut parts = domain_suffix.rsplit(wildcards::DOT);

        // `rsplit`返回的迭代器会先给出原始字符串的最后一部分，我们首先添加它。
        if let Some(part) = parts.next() {
            reversed.push_str(part);
        }

        // 遍历剩余的部分，在每个部分前加上点号。
        for part in parts {
            reversed.push(wildcards::DOT);
            reversed.push_str(part);
        }

        reversed
    }

    // 构建正则表达式预筛选映射
    fn build_regex_prefilter(rules: &[CompiledRegexRule]) -> HashMap<String, HashSet<usize>> {
        let mut prefilter = HashMap::new();

        // 为每个正则表达式规则提取关键词
        for (i, rule) in rules.iter().enumerate() {
            // 提取不包含正则表达式特殊字符的子字符串作为预筛选关键词
            let pattern = &rule.pattern;
            let mut current_segment = String::with_capacity(pattern.len() / 2);
            let mut segments = Vec::with_capacity(5);

            for c in pattern.chars() {
                if REGEX_SPECIAL_CHARS.contains(&c) {
                    if !current_segment.is_empty() {
                        segments.push(std::mem::take(&mut current_segment));
                    }
                } else {
                    current_segment.push(c);
                }
            }

            if !current_segment.is_empty() {
                segments.push(current_segment);
            }

            // 选择最长的子字符串作为预筛选关键词（更具体更好）
            if let Some(longest_segment) = segments
                .iter()
                .filter(|s| s.len() >= 2) // 忽略太短的字符串
                .max_by_key(|s| s.len())
            {
                prefilter
                    .entry(longest_segment.to_lowercase())
                    .or_insert_with(HashSet::new)
                    .insert(i);
            }
        }

        prefilter
    }

    // 构建新的路由引擎
    pub fn new(rules: Vec<RouteRuleConfig>) -> Result<Self, ConfigError> {
        let mut exact_block_rules = HashMap::new();
        let mut exact_forward_rules = HashMap::new();
        let mut wildcard_block_rules = BTreeMap::new();
        let mut wildcard_forward_rules = BTreeMap::new();
        let mut global_wildcard_block_rule = None;
        let mut global_wildcard_forward_rule = None;
        let mut regex_block_rules = Vec::new();
        let mut regex_forward_rules = Vec::new();

        // 处理所有规则
        for rule in rules {
            // 将目标字符串转换为 Arc 以便共享
            let target = rule.target.map(Arc::new);

            match rule.match_type {
                MatchType::Exact => {
                    // 添加精确匹配规则，根据动作类型分别存储
                    for pattern in rule.patterns {
                        match rule.action {
                            RouteAction::Block => {
                                exact_block_rules.insert(pattern, target.clone());
                            }
                            RouteAction::Forward => {
                                exact_forward_rules.insert(pattern, target.clone());
                            }
                        }
                    }
                }
                MatchType::Wildcard => {
                    for pattern in rule.patterns {
                        if pattern == wildcards::GLOBAL {
                            // 根据动作类型存储全局通配符规则
                            match rule.action {
                                RouteAction::Block => {
                                    if global_wildcard_block_rule.is_some() {
                                        debug!("Multiple definitions of global wildcard block rule '*', using the last one");
                                    }
                                    global_wildcard_block_rule = Some((target.clone(), pattern));
                                }
                                RouteAction::Forward => {
                                    if global_wildcard_forward_rule.is_some() {
                                        debug!("Multiple definitions of global wildcard forward rule '*', using the last one");
                                    }
                                    global_wildcard_forward_rule = Some((target.clone(), pattern));
                                }
                            }
                        } else {
                            // 处理特定通配符规则：*.domain.tld
                            // 为了支持反向后缀匹配，将域名部分反转存储
                            let suffix = &pattern[2..]; // 移除前导*.
                            let reversed_suffix = Self::reverse_domain_labels(suffix);

                            // 根据动作类型存储通配符规则
                            match rule.action {
                                RouteAction::Block => {
                                    wildcard_block_rules.insert(reversed_suffix, target.clone());
                                }
                                RouteAction::Forward => {
                                    wildcard_forward_rules.insert(reversed_suffix, target.clone());
                                }
                            }
                        }
                    }
                }
                MatchType::Regex => {
                    let action = rule.action;
                    for pattern in rule.patterns {
                        // 编译正则表达式
                        let regex = Regex::new(&pattern)?;

                        // 根据动作类型添加正则表达式规则
                        match action {
                            RouteAction::Block => {
                                regex_block_rules.push(CompiledRegexRule {
                                    pattern,
                                    regex,
                                    action: RouteAction::Block,
                                    target: target.clone(),
                                });
                            }
                            RouteAction::Forward => {
                                regex_forward_rules.push(CompiledRegexRule {
                                    pattern,
                                    regex,
                                    action: RouteAction::Forward,
                                    target: target.clone(),
                                });
                            }
                        }
                    }
                }
            }
        }

        // 创建正则表达式预筛选映射
        let regex_block_prefilter = Self::build_regex_prefilter(&regex_block_rules);
        let regex_forward_prefilter = Self::build_regex_prefilter(&regex_forward_rules);

        // 更新路由规则数量指标
        METRICS
            .route_rules_count()
            .with_label_values(&[rule_type_labels::EXACT, rule_source_labels::STATIC])
            .set((exact_block_rules.len() + exact_forward_rules.len()) as i64);

        let wildcard_count = wildcard_block_rules.len()
            + wildcard_forward_rules.len()
            + global_wildcard_block_rule.is_some() as usize
            + global_wildcard_forward_rule.is_some() as usize;

        METRICS
            .route_rules_count()
            .with_label_values(&[rule_type_labels::WILDCARD, rule_source_labels::STATIC])
            .set(wildcard_count as i64);

        METRICS
            .route_rules_count()
            .with_label_values(&[rule_type_labels::REGEX, rule_source_labels::STATIC])
            .set((regex_block_rules.len() + regex_forward_rules.len()) as i64);

        let router = Self {
            exact_block_rules,
            exact_forward_rules,
            wildcard_block_rules,
            wildcard_forward_rules,
            global_wildcard_block_rule,
            global_wildcard_forward_rule,
            regex_block_rules,
            regex_forward_rules,
            regex_block_prefilter,
            regex_forward_prefilter,
        };

        Ok(router)
    }

    // 尝试精确匹配规则
    fn try_exact_match(&self, domain: &str, action: RouteAction) -> Option<RouteMatch> {
        let rules = match action {
            RouteAction::Block => &self.exact_block_rules,
            RouteAction::Forward => &self.exact_forward_rules,
        };

        if let Some(target) = rules.get(domain) {
            let target_default = rule_type_labels::NO_TARGET;
            let target_str = target
                .as_ref()
                .map(|s| s.as_str())
                .unwrap_or(target_default);

            debug!(
                "Rule match: Exact {:?} match '{}' -> Target: {}",
                action, domain, target_str
            );

            // 记录路由匹配指标
            METRICS
                .route_matches_total()
                .with_label_values(&[
                    rule_type_labels::EXACT,
                    target_str,
                    rule_source_labels::STATIC,
                    <&'static str>::from(action),
                ])
                .inc();

            return Some(RouteMatch {
                domain: domain.to_string(),
                action,
                target: target.as_ref().map(|arc_str| arc_str.to_string()),
                rule_type: rule_type_labels::EXACT,
                pattern: domain.to_string(),
            });
        }

        None
    }

    // 尝试通配符匹配规则
    fn try_wildcard_match(&self, domain: &str, action: RouteAction) -> Option<RouteMatch> {
        let rules = match action {
            RouteAction::Block => &self.wildcard_block_rules,
            RouteAction::Forward => &self.wildcard_forward_rules,
        };

        let target_default = rule_type_labels::NO_TARGET;
        let mut current_part = domain;

        // 首先检查完整域名，然后逐步缩短
        loop {
            // 将当前域名部分反转以匹配wildcard规则
            let reversed_suffix = Self::reverse_domain_labels(current_part);

            // 检查当前反转后缀是否匹配规则
            if let Some(target) = rules.get(&reversed_suffix) {
                let target_str = target
                    .as_ref()
                    .map(|s| s.as_str())
                    .unwrap_or(target_default);

                debug!(
                    "Rule match: Wildcard {:?} match '{}' -> Pattern: '{}', Target: {}",
                    action, domain, reversed_suffix, target_str
                );

                // 记录路由匹配指标
                METRICS
                    .route_matches_total()
                    .with_label_values(&[
                        rule_type_labels::WILDCARD,
                        target_str,
                        rule_source_labels::STATIC,
                        <&'static str>::from(action),
                    ])
                    .inc();

                return Some(RouteMatch {
                    domain: domain.to_string(),
                    action,
                    target: target.as_ref().map(|arc_str| arc_str.to_string()),
                    rule_type: rule_type_labels::WILDCARD,
                    pattern: reversed_suffix,
                });
            }

            // 找到当前部分的第一个点号
            if let Some(dot_pos) = current_part.find('.') {
                // 去掉最左边的标签进行下一次匹配
                current_part = &current_part[dot_pos + 1..];
            } else {
                // 没有更多的点号，结束循环
                break;
            }
        }

        None
    }

    // 尝试正则表达式匹配规则
    fn try_regex_match(&self, domain: &str, action: RouteAction) -> Option<RouteMatch> {
        let (rules, prefilter) = match action {
            RouteAction::Block => (&self.regex_block_rules, &self.regex_block_prefilter),
            RouteAction::Forward => (&self.regex_forward_rules, &self.regex_forward_prefilter),
        };

        if rules.is_empty() {
            return None;
        }

        let target_default = rule_type_labels::NO_TARGET;
        let domain_parts: Vec<&str> = domain.split('.').collect();

        // 使用预筛选优化正则表达式匹配
        let mut potential_rules = HashSet::new();

        // 收集所有可能匹配的规则
        for segment in &domain_parts {
            if segment.len() < 2 {
                continue; // 跳过太短的片段
            }

            let segment_lower = segment.to_lowercase();
            if let Some(rule_indices) = prefilter.get(&segment_lower) {
                for &idx in rule_indices {
                    potential_rules.insert(idx);
                }
            }
        }

        // 然后检查每个潜在匹配的规则
        for &rule_idx in &potential_rules {
            let rule = &rules[rule_idx];
            if rule.regex.is_match(domain) {
                let target_str = rule
                    .target
                    .as_ref()
                    .map(|s| s.as_str())
                    .unwrap_or(target_default);

                debug!(
                    "Rule match: Regex {:?} match '{}' -> Pattern: '{}', Target: {}",
                    action, domain, rule.pattern, target_str
                );

                // 记录路由匹配指标
                METRICS
                    .route_matches_total()
                    .with_label_values(&[
                        rule_type_labels::REGEX,
                        target_str,
                        rule_source_labels::STATIC,
                        <&'static str>::from(action),
                    ])
                    .inc();

                return Some(RouteMatch {
                    domain: domain.to_string(),
                    action,
                    target: rule.target.as_ref().map(|arc_str| arc_str.to_string()),
                    rule_type: rule_type_labels::REGEX,
                    pattern: rule.pattern.clone(),
                });
            }
        }

        None
    }

    // 尝试全局通配符匹配规则
    fn try_global_wildcard_match(&self, domain: &str, action: RouteAction) -> Option<RouteMatch> {
        let global_rule = match action {
            RouteAction::Block => &self.global_wildcard_block_rule,
            RouteAction::Forward => &self.global_wildcard_forward_rule,
        };

        if let Some((target, pattern)) = global_rule {
            let target_default = rule_type_labels::NO_TARGET;
            let target_str = target
                .as_ref()
                .map(|s| s.as_str())
                .unwrap_or(target_default);

            debug!(
                "Rule match: Global wildcard {:?} match '{}' -> Pattern: '{}', Target: {}",
                action, domain, pattern, target_str
            );

            // 记录路由匹配指标
            METRICS
                .route_matches_total()
                .with_label_values(&[
                    rule_type_labels::WILDCARD,
                    target_str,
                    rule_source_labels::STATIC,
                    <&'static str>::from(action),
                ])
                .inc();

            return Some(RouteMatch {
                domain: domain.to_string(),
                action,
                target: target.as_ref().map(|arc_str| arc_str.to_string()),
                rule_type: rule_type_labels::WILDCARD,
                pattern: pattern.clone(),
            });
        }

        None
    }

    // 查找匹配规则
    //
    // 查找顺序（优先级从高到低）：
    // 1. 精确匹配 block 规则
    // 2. 通配符 block 规则（按特定性从高到低）
    // 3. 正则表达式 block 规则
    // 4. 全局通配符 block 规则
    // 5. 精确匹配 forward 规则
    // 6. 通配符 forward 规则（按特定性从高到低）
    // 7. 正则表达式 forward 规则
    // 8. 全局通配符 forward 规则
    //
    // 这种优先级顺序确保：
    // - 所有 block 规则优先于所有 forward 规则
    // - 在同类规则中，遵循精确匹配 > 通配符匹配 > 正则匹配 > 全局通配符的优先级
    //
    // 整体查找匹配规则
    // 精确匹配 block > 通配符 block > 正则 block > 全局通配符 block > 精确匹配 forward > 通配符 forward > 正则 forward > 全局通配符 forward
    pub fn find_match(&self, query_name: &Name) -> Result<RouteMatch, AppError> {
        // 将查询名称转换为小写字符串，便于匹配
        let mut domain = query_name.to_string().to_lowercase();

        // 移除末尾可能存在的点，确保正则表达式等能正确匹配
        if domain.ends_with('.') {
            domain.pop();
        }

        if domain.is_empty() {
            return Err(AppError::NoRouteMatch(domain));
        }

        // 1. 先检查所有 Block 规则
        // 精确匹配 block > 通配符 block > 正则 block > 全局通配符 block
        if let Some(match_result) = self.try_exact_match(&domain, RouteAction::Block) {
            return Ok(match_result);
        }

        if let Some(match_result) = self.try_wildcard_match(&domain, RouteAction::Block) {
            return Ok(match_result);
        }

        if let Some(match_result) = self.try_regex_match(&domain, RouteAction::Block) {
            return Ok(match_result);
        }

        if let Some(match_result) = self.try_global_wildcard_match(&domain, RouteAction::Block) {
            return Ok(match_result);
        }

        // 2. 再检查所有 Forward 规则
        // 精确匹配 forward > 通配符 forward > 正则 forward > 全局通配符 forward
        if let Some(match_result) = self.try_exact_match(&domain, RouteAction::Forward) {
            return Ok(match_result);
        }

        if let Some(match_result) = self.try_wildcard_match(&domain, RouteAction::Forward) {
            return Ok(match_result);
        }

        if let Some(match_result) = self.try_regex_match(&domain, RouteAction::Forward) {
            return Ok(match_result);
        }

        if let Some(match_result) = self.try_global_wildcard_match(&domain, RouteAction::Forward) {
            return Ok(match_result);
        }

        // 没有匹配的规则
        Err(AppError::NoRouteMatch(domain))
    }
}

// 实现路由动作标签转换
impl From<RouteAction> for &'static str {
    fn from(action: RouteAction) -> Self {
        match action {
            RouteAction::Block => rule_action_labels::BLOCK,
            RouteAction::Forward => rule_action_labels::FORWARD,
        }
    }
}
