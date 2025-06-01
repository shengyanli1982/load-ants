use crate::config::{MatchType, RouteAction, RouteRuleConfig};
use crate::error::{AppError, ConfigError};
use crate::metrics::METRICS;
use crate::r#const::{router::wildcards, rule_type_labels};
use hickory_proto::rr::Name;
use regex::Regex;
use std::collections::{BTreeMap, HashMap, HashSet};
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

// DNS请求路由引擎
// 实现特点：
// 1. 分离存储：block规则和forward规则分开存储，确保block规则始终具有更高优先级
// 2. 查询优化：在匹配算法中，始终先检查所有类型的block规则，再检查forward规则
// 3. 规则排序：维持了原有的精确匹配>通配符匹配>正则匹配>全局通配符的类型优先级
// 4. 性能保障：保留了高效的查询机制，如使用HashMap进行精确匹配，BTreeMap进行后缀树匹配，以及正则表达式预筛选
pub struct Router {
    // 精确匹配规则 - 分离block和forward规则
    exact_block_rules: HashMap<String, Option<String>>,
    exact_forward_rules: HashMap<String, Option<String>>,

    // 通配符匹配规则树 - 分离block和forward规则
    // 键为反转后的域名后缀，值为(目标,原始模式)
    wildcard_block_rules: BTreeMap<String, (Option<String>, String)>,
    wildcard_forward_rules: BTreeMap<String, (Option<String>, String)>,

    // 全局通配符规则（模式为 "*"）- 分离block和forward规则
    global_wildcard_block_rule: Option<(Option<String>, String)>,
    global_wildcard_forward_rule: Option<(Option<String>, String)>,

    // 正则表达式匹配规则 - 分离block和forward规则
    regex_block_rules: Vec<CompiledRegexRule>,
    regex_forward_rules: Vec<CompiledRegexRule>,

    // 正则表达式预筛选映射
    regex_block_prefilter: HashMap<String, HashSet<usize>>,
    regex_forward_prefilter: HashMap<String, HashSet<usize>>,

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
    pub rule_type: &'static str,
    // 匹配的模式
    pub pattern: String,
}

impl Router {
    // 反转域名标签，例如 "example.com" -> "com.example"
    // 这个函数在 `find_match` 方法中被多次调用，因此使用 `#[inline(always)]` 优化
    #[inline(always)]
    fn reverse_domain_labels(domain_suffix: &str) -> String {
        if domain_suffix.is_empty() {
            return String::new();
        }

        // 预先分配足够空间 (最坏情况需要额外的点号)
        let mut result = String::with_capacity(domain_suffix.len() + 1);

        // 计算段数以确定何时添加分隔符
        let segments: Vec<(usize, usize)> = domain_suffix
            .char_indices()
            .filter(|(_, c)| *c == wildcards::DOT)
            .map(|(i, _)| i)
            .fold(Vec::with_capacity(10), |mut acc, i| {
                if let Some(&(_, last)) = acc.last() {
                    acc.push((last + 1, i));
                } else {
                    acc.push((0, i));
                }
                acc
            });

        // 处理最后一段
        if let Some(&(start, _)) = segments.last() {
            result.push_str(&domain_suffix[start..]);
        } else {
            // 没有点号，直接返回原字符串
            return domain_suffix.to_string();
        }

        // 反向处理其他段
        for i in (0..segments.len() - 1).rev() {
            let (start, end) = segments[i];
            result.push(wildcards::DOT);
            result.push_str(&domain_suffix[start..end]);
        }

        result
    }

    // 构建正则表达式预筛选映射
    fn build_regex_prefilter(rules: &[CompiledRegexRule]) -> HashMap<String, HashSet<usize>> {
        let mut prefilter = HashMap::new();

        // 为每个正则表达式规则提取关键词
        for (i, rule) in rules.iter().enumerate() {
            // 提取不包含正则表达式特殊字符的子字符串作为预筛选关键词
            let pattern = &rule.pattern;
            let mut current_segment = String::new();
            let mut segments = Vec::new();

            for c in pattern.chars() {
                if "\\^$.|?*+()[]{}-".contains(c) {
                    if !current_segment.is_empty() {
                        segments.push(current_segment);
                        current_segment = String::new();
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
                .filter(|s| !s.is_empty()) // 忽略空字符串
                .max_by_key(|s| s.len())
            {
                // 确保关键词长度大于等于2，避免过多的假阳性
                if longest_segment.len() >= 2 {
                    let entry = prefilter
                        .entry(longest_segment.to_lowercase())
                        .or_insert_with(HashSet::new);
                    entry.insert(i);
                }
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
            match rule.match_type {
                MatchType::Exact => {
                    // 添加精确匹配规则，根据动作类型分别存储
                    for pattern in rule.patterns {
                        match rule.action {
                            RouteAction::Block => {
                                exact_block_rules.insert(pattern.clone(), rule.target.clone());
                            }
                            RouteAction::Forward => {
                                exact_forward_rules.insert(pattern.clone(), rule.target.clone());
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
                                    global_wildcard_block_rule =
                                        Some((rule.target.clone(), pattern.clone()));
                                }
                                RouteAction::Forward => {
                                    if global_wildcard_forward_rule.is_some() {
                                        debug!("Multiple definitions of global wildcard forward rule '*', using the last one");
                                    }
                                    global_wildcard_forward_rule =
                                        Some((rule.target.clone(), pattern.clone()));
                                }
                            }
                        } else {
                            // 处理特定通配符规则：*.domain.tld
                            // 为了支持反向后缀匹配，将域名部分反转存储
                            let suffix = pattern[2..].to_string(); // 移除前导*.
                            let reversed_suffix = Self::reverse_domain_labels(&suffix);

                            // 根据动作类型存储通配符规则
                            match rule.action {
                                RouteAction::Block => {
                                    wildcard_block_rules.insert(
                                        reversed_suffix,
                                        (rule.target.clone(), pattern.clone()),
                                    );
                                }
                                RouteAction::Forward => {
                                    wildcard_forward_rules.insert(
                                        reversed_suffix,
                                        (rule.target.clone(), pattern.clone()),
                                    );
                                }
                            }
                        }
                    }
                }
                MatchType::Regex => {
                    for pattern in rule.patterns {
                        // 编译正则表达式
                        let regex = Regex::new(&pattern)?;

                        // 根据动作类型添加正则表达式规则
                        match rule.action {
                            RouteAction::Block => {
                                regex_block_rules.push(CompiledRegexRule {
                                    pattern,
                                    regex,
                                    action: rule.action.clone(),
                                    target: rule.target.clone(),
                                });
                            }
                            RouteAction::Forward => {
                                regex_forward_rules.push(CompiledRegexRule {
                                    pattern,
                                    regex,
                                    action: rule.action.clone(),
                                    target: rule.target.clone(),
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
            .with_label_values(&[rule_type_labels::EXACT])
            .set((exact_block_rules.len() + exact_forward_rules.len()) as f64);

        let wildcard_count = wildcard_block_rules.len()
            + wildcard_forward_rules.len()
            + if global_wildcard_block_rule.is_some() {
                1
            } else {
                0
            }
            + if global_wildcard_forward_rule.is_some() {
                1
            } else {
                0
            };

        METRICS
            .route_rules_count()
            .with_label_values(&[rule_type_labels::WILDCARD])
            .set(wildcard_count as f64);

        METRICS
            .route_rules_count()
            .with_label_values(&[rule_type_labels::REGEX])
            .set((regex_block_rules.len() + regex_forward_rules.len()) as f64);

        let mut router = Self {
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
            sorted_rules: Vec::new(),
        };

        // 对所有规则进行排序，确保它们按照正确的优先级顺序处理
        let sorted_rules = router.sort_rules()?;
        debug!("Sort {} routing rules in total", sorted_rules.len());
        router.sorted_rules = sorted_rules;

        Ok(router)
    }

    // 查找匹配规则
    //
    // 查找顺序（优先级从高到低）：
    // 1. 精确匹配 block 规则
    // 2. 精确匹配 forward 规则
    // 3. 通配符 block 规则（按特定性从高到低）
    // 4. 通配符 forward 规则（按特定性从高到低）
    // 5. 正则表达式 block 规则
    // 6. 正则表达式 forward 规则
    // 7. 全局通配符 block 规则
    // 8. 全局通配符 forward 规则
    //
    // 这种优先级顺序确保：
    // - 在同一匹配类型内，block 规则始终优先于 forward 规则
    // - 在不同匹配类型间，保持精确匹配 > 通配符匹配 > 正则匹配 > 全局通配符的优先级
    //
    // 整体查找匹配规则
    // 精确匹配 block > 精确匹配 forward > 通配符 block > 通配符 forward > 正则 block > 正则 forward > 全局通配符 block > 全局通配符 forward
    pub fn find_match(&self, query_name: &Name) -> Result<RouteMatch, AppError> {
        // 将查询名称转换为小写字符串，便于匹配
        let mut domain = query_name.to_string().to_lowercase();

        // 移除末尾可能存在的点，确保正则表达式等能正确匹配
        if domain.ends_with('.') {
            domain.pop();
        }

        // 为结果准备一些重用变量
        let target_default = rule_type_labels::NO_TARGET;

        // 1. 首先尝试精确匹配的block规则 - 最高优先级
        if let Some(target) = self.exact_block_rules.get(&domain) {
            let target_str = target.as_deref().unwrap_or(target_default);

            debug!(
                "Rule match: Exact block match '{}' -> Target: {}",
                domain, target_str
            );

            // 记录路由匹配指标
            METRICS
                .route_matches_total()
                .with_label_values(&[rule_type_labels::EXACT, target_str])
                .inc();

            return Ok(RouteMatch {
                domain: domain.clone(),
                action: RouteAction::Block,
                target: target.clone(),
                rule_type: rule_type_labels::EXACT,
                pattern: domain.clone(),
            });
        }

        // 2. 然后尝试精确匹配的forward规则
        if let Some(target) = self.exact_forward_rules.get(&domain) {
            let target_str = target.as_deref().unwrap_or(target_default);

            debug!(
                "Rule match: Exact forward match '{}' -> Target: {}",
                domain, target_str
            );

            // 记录路由匹配指标
            METRICS
                .route_matches_total()
                .with_label_values(&[rule_type_labels::EXACT, target_str])
                .inc();

            return Ok(RouteMatch {
                domain: domain.clone(),
                action: RouteAction::Forward,
                target: target.clone(),
                rule_type: rule_type_labels::EXACT,
                pattern: domain.clone(),
            });
        }

        // 预先分析域名部分 - 只分割一次，后续复用
        if !domain.is_empty() {
            // 分割域名部分以便通配符匹配
            let domain_parts: Vec<&str> = domain.split('.').collect();

            if !domain_parts.is_empty() {
                // 通配符匹配 - 从最特定到最不特定
                // 策略：
                // 1. 首先检查完整域名的反转后缀
                // 2. 然后逐步移除最左边的标签，检查更短的后缀
                // 3. 先检查所有可能的block规则，再检查forward规则
                // 4. 这样可以确保最特定的匹配（最长的后缀）具有最高优先级
                // 5. 在每个特定级别上，block规则始终优先于forward规则

                // 3. 先尝试block规则的通配符匹配
                let mut current_part = domain.clone();

                // 首先检查完整域名，然后逐步缩短
                loop {
                    // 将当前域名部分反转以匹配wildcard规则
                    let reversed_suffix = Self::reverse_domain_labels(&current_part);

                    // 检查当前反转后缀是否匹配block规则
                    if let Some((target, pattern)) = self.wildcard_block_rules.get(&reversed_suffix)
                    {
                        let target_str = target.as_deref().unwrap_or(target_default);

                        debug!(
                            "Rule match: Wildcard block match '{}' -> Pattern: '{}', Target: {}",
                            domain, pattern, target_str
                        );

                        // 记录路由匹配指标
                        METRICS
                            .route_matches_total()
                            .with_label_values(&[rule_type_labels::WILDCARD, target_str])
                            .inc();

                        return Ok(RouteMatch {
                            domain: domain.clone(),
                            action: RouteAction::Block,
                            target: target.clone(),
                            rule_type: rule_type_labels::WILDCARD,
                            pattern: pattern.clone(),
                        });
                    }

                    // 找到当前部分的第一个点号
                    if let Some(dot_pos) = current_part.find('.') {
                        // 去掉最左边的标签进行下一次匹配
                        current_part = current_part[dot_pos + 1..].to_string();
                    } else {
                        // 没有更多的点号，结束循环
                        break;
                    }
                }

                // 4. 然后尝试forward规则的通配符匹配
                current_part = domain.clone();

                // 首先检查完整域名，然后逐步缩短
                loop {
                    // 将当前域名部分反转以匹配wildcard规则
                    let reversed_suffix = Self::reverse_domain_labels(&current_part);

                    // 检查当前反转后缀是否匹配forward规则
                    if let Some((target, pattern)) =
                        self.wildcard_forward_rules.get(&reversed_suffix)
                    {
                        let target_str = target.as_deref().unwrap_or(target_default);

                        debug!(
                            "Rule match: Wildcard forward match '{}' -> Pattern: '{}', Target: {}",
                            domain, pattern, target_str
                        );

                        // 记录路由匹配指标
                        METRICS
                            .route_matches_total()
                            .with_label_values(&[rule_type_labels::WILDCARD, target_str])
                            .inc();

                        return Ok(RouteMatch {
                            domain: domain.clone(),
                            action: RouteAction::Forward,
                            target: target.clone(),
                            rule_type: rule_type_labels::WILDCARD,
                            pattern: pattern.clone(),
                        });
                    }

                    // 找到当前部分的第一个点号
                    if let Some(dot_pos) = current_part.find('.') {
                        // 去掉最左边的标签进行下一次匹配
                        current_part = current_part[dot_pos + 1..].to_string();
                    } else {
                        // 没有更多的点号，结束循环
                        break;
                    }
                }

                // 5. 尝试regex block规则
                let mut matched_rule_index = None;

                // 使用预筛选优化正则表达式匹配
                let mut potential_rules = HashSet::new();

                // 先收集所有可能匹配的block规则
                for segment in domain.split('.') {
                    if segment.len() < 2 {
                        continue; // 跳过太短的片段
                    }

                    if let Some(indices) = self.regex_block_prefilter.get(&segment.to_lowercase()) {
                        for &idx in indices {
                            potential_rules.insert(idx);
                        }
                    }
                }

                // 如果没有匹配的预筛选关键词，仍然需要尝试所有regex规则
                if potential_rules.is_empty() && !self.regex_block_rules.is_empty() {
                    potential_rules = (0..self.regex_block_rules.len()).collect();
                }

                // 检查所有潜在匹配的block规则
                for &rule_idx in &potential_rules {
                    let rule = &self.regex_block_rules[rule_idx];
                    if rule.regex.is_match(&domain) {
                        matched_rule_index = Some(rule_idx);
                        break;
                    }
                }

                // 如果找到匹配的block规则，返回结果
                if let Some(index) = matched_rule_index {
                    let rule = &self.regex_block_rules[index];
                    let target_str = rule.target.as_deref().unwrap_or(target_default);

                    debug!(
                        "Rule match: Regex block match '{}' -> Pattern: '{}', Target: {}",
                        domain, rule.pattern, target_str
                    );

                    // 记录路由匹配指标
                    METRICS
                        .route_matches_total()
                        .with_label_values(&[rule_type_labels::REGEX, target_str])
                        .inc();

                    return Ok(RouteMatch {
                        domain: domain.clone(),
                        action: RouteAction::Block,
                        target: rule.target.clone(),
                        rule_type: rule_type_labels::REGEX,
                        pattern: rule.pattern.clone(),
                    });
                }

                // 6. 尝试regex forward规则
                matched_rule_index = None;
                potential_rules.clear();

                // 收集所有可能匹配的forward规则
                for segment in domain.split('.') {
                    if segment.len() < 2 {
                        continue;
                    }

                    if let Some(indices) = self.regex_forward_prefilter.get(&segment.to_lowercase())
                    {
                        for &idx in indices {
                            potential_rules.insert(idx);
                        }
                    }
                }

                // 如果没有匹配的预筛选关键词，仍然需要尝试所有regex规则
                if potential_rules.is_empty() && !self.regex_forward_rules.is_empty() {
                    potential_rules = (0..self.regex_forward_rules.len()).collect();
                }

                // 检查所有潜在匹配的forward规则
                for &rule_idx in &potential_rules {
                    let rule = &self.regex_forward_rules[rule_idx];
                    if rule.regex.is_match(&domain) {
                        matched_rule_index = Some(rule_idx);
                        break;
                    }
                }

                // 如果找到匹配的forward规则，返回结果
                if let Some(index) = matched_rule_index {
                    let rule = &self.regex_forward_rules[index];
                    let target_str = rule.target.as_deref().unwrap_or(target_default);

                    debug!(
                        "Rule match: Regex forward match '{}' -> Pattern: '{}', Target: {}",
                        domain, rule.pattern, target_str
                    );

                    // 记录路由匹配指标
                    METRICS
                        .route_matches_total()
                        .with_label_values(&[rule_type_labels::REGEX, target_str])
                        .inc();

                    return Ok(RouteMatch {
                        domain: domain.clone(),
                        action: RouteAction::Forward,
                        target: rule.target.clone(),
                        rule_type: rule_type_labels::REGEX,
                        pattern: rule.pattern.clone(),
                    });
                }

                // 7. 尝试全局通配符 block 规则
                if let Some((target, pattern)) = &self.global_wildcard_block_rule {
                    let target_str = target.as_deref().unwrap_or(target_default);

                    debug!(
                        "Rule match: Global wildcard block match '{}' -> Pattern: '{}', Target: {}",
                        domain, pattern, target_str
                    );

                    // 记录路由匹配指标
                    METRICS
                        .route_matches_total()
                        .with_label_values(&[rule_type_labels::WILDCARD, target_str])
                        .inc();

                    return Ok(RouteMatch {
                        domain: domain.clone(),
                        action: RouteAction::Block,
                        target: target.clone(),
                        rule_type: rule_type_labels::WILDCARD,
                        pattern: pattern.clone(),
                    });
                }

                // 8. 最后尝试全局通配符 forward 规则
                if let Some((target, pattern)) = &self.global_wildcard_forward_rule {
                    let target_str = target.as_deref().unwrap_or(target_default);

                    debug!(
                        "Rule match: Global wildcard forward match '{}' -> Pattern: '{}', Target: {}",
                        domain, pattern, target_str
                    );

                    // 记录路由匹配指标
                    METRICS
                        .route_matches_total()
                        .with_label_values(&[rule_type_labels::WILDCARD, target_str])
                        .inc();

                    return Ok(RouteMatch {
                        domain: domain.clone(),
                        action: RouteAction::Forward,
                        target: target.clone(),
                        rule_type: rule_type_labels::WILDCARD,
                        pattern: pattern.clone(),
                    });
                }
            }
        } else if let Some((target, pattern)) = &self.global_wildcard_block_rule {
            // 空域名先检查全局通配符block规则
            let target_str = target.as_deref().unwrap_or(target_default);

            debug!(
                "Rule match: Global wildcard block match '{}' -> Pattern: '{}', Target: {}",
                domain, pattern, target_str
            );

            // 记录路由匹配指标
            METRICS
                .route_matches_total()
                .with_label_values(&[rule_type_labels::WILDCARD, target_str])
                .inc();

            return Ok(RouteMatch {
                domain: domain.clone(),
                action: RouteAction::Block,
                target: target.clone(),
                rule_type: rule_type_labels::WILDCARD,
                pattern: pattern.clone(),
            });
        } else if let Some((target, pattern)) = &self.global_wildcard_forward_rule {
            // 然后检查全局通配符forward规则
            let target_str = target.as_deref().unwrap_or(target_default);

            debug!(
                "Rule match: Global wildcard forward match '{}' -> Pattern: '{}', Target: {}",
                domain, pattern, target_str
            );

            // 记录路由匹配指标
            METRICS
                .route_matches_total()
                .with_label_values(&[rule_type_labels::WILDCARD, target_str])
                .inc();

            return Ok(RouteMatch {
                domain: domain.clone(),
                action: RouteAction::Forward,
                target: target.clone(),
                rule_type: rule_type_labels::WILDCARD,
                pattern: pattern.clone(),
            });
        }

        // 没有匹配的规则
        Err(AppError::NoRouteMatch(domain))
    }

    // 返回所有规则的组合列表，按照优先级顺序排列
    //
    // 此方法在应用启动后加载规则配置时应被调用，用于确保路由规则按正确的优先级顺序处理：
    // 1. 精确匹配 block 规则 (最高优先级)
    // 2. 精确匹配 forward 规则
    // 3. 通配符 block 规则 (按特定性排序)
    // 4. 通配符 forward 规则 (按特定性排序)
    // 5. 正则表达式 block 规则
    // 6. 正则表达式 forward 规则
    // 7. 全局通配符 block 规则
    // 8. 全局通配符 forward 规则 (最低优先级)
    //
    // 返回的列表包含所有路由动作及其可能的目标上游组
    pub fn sort_rules(&self) -> Result<Vec<(RouteAction, Option<String>)>, ConfigError> {
        let mut rules = Vec::new();

        // 1. 添加精确匹配 block 规则
        for target in self.exact_block_rules.values() {
            rules.push((RouteAction::Block, target.clone()));
        }

        // 2. 添加精确匹配 forward 规则
        for target in self.exact_forward_rules.values() {
            rules.push((RouteAction::Forward, target.clone()));
        }

        // 3. 收集通配符 block 规则并按特定性排序
        let mut wildcard_block_rules = Vec::new();
        for (reversed_suffix, (target, pattern)) in &self.wildcard_block_rules {
            wildcard_block_rules.push((
                target.clone(),
                pattern.clone(),
                reversed_suffix.split('.').count(), // 计算特定性
            ));
        }
        // 按特定性从高到低排序
        wildcard_block_rules.sort_by(|a, b| b.2.cmp(&a.2));
        // 将排序后的 block 规则添加到输出列表
        for rule in wildcard_block_rules {
            rules.push((RouteAction::Block, rule.0));
        }

        // 4. 收集通配符 forward 规则并按特定性排序
        let mut wildcard_forward_rules = Vec::new();
        for (reversed_suffix, (target, pattern)) in &self.wildcard_forward_rules {
            wildcard_forward_rules.push((
                target.clone(),
                pattern.clone(),
                reversed_suffix.split('.').count(), // 计算特定性
            ));
        }
        // 按特定性从高到低排序
        wildcard_forward_rules.sort_by(|a, b| b.2.cmp(&a.2));
        // 将排序后的 forward 规则添加到输出列表
        for rule in wildcard_forward_rules {
            rules.push((RouteAction::Forward, rule.0));
        }

        // 5. 添加正则表达式 block 规则
        for rule in &self.regex_block_rules {
            rules.push((rule.action.clone(), rule.target.clone()));
        }

        // 6. 添加正则表达式 forward 规则
        for rule in &self.regex_forward_rules {
            rules.push((rule.action.clone(), rule.target.clone()));
        }

        // 7. 添加全局通配符 block 规则（如果存在）
        if let Some((target, _)) = &self.global_wildcard_block_rule {
            rules.push((RouteAction::Block, target.clone()));
        }

        // 8. 添加全局通配符 forward 规则（如果存在）
        if let Some((target, _)) = &self.global_wildcard_forward_rule {
            rules.push((RouteAction::Forward, target.clone()));
        }

        Ok(rules)
    }
}
