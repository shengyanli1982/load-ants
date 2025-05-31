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
pub struct Router {
    // 精确匹配规则
    exact_rules: HashMap<String, (RouteAction, Option<String>)>,
    // 通配符匹配规则树 - 键为反转后的域名后缀，值为(动作,目标,原始模式)
    wildcard_rules: BTreeMap<String, (RouteAction, Option<String>, String)>,
    // 全局通配符规则（模式为 "*"）
    global_wildcard_rule: Option<(RouteAction, Option<String>, String)>,
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
            result.push('.');
            result.push_str(&domain_suffix[start..end]);
        }

        result
    }

    // 创建新的路由引擎
    pub fn new(rules: Vec<RouteRuleConfig>) -> Result<Self, ConfigError> {
        let mut exact_rules = HashMap::new();
        let mut wildcard_rules = BTreeMap::new();
        let mut global_wildcard_rule = None;
        let mut regex_rules = Vec::new();

        // 处理所有规则
        for rule in rules {
            match rule.match_type {
                MatchType::Exact => {
                    // 添加精确匹配规则
                    for pattern in rule.patterns {
                        exact_rules
                            .insert(pattern.clone(), (rule.action.clone(), rule.target.clone()));
                    }
                }
                MatchType::Wildcard => {
                    for pattern in rule.patterns {
                        if pattern == wildcards::GLOBAL {
                            // 存储全局通配符规则
                            if global_wildcard_rule.is_some() {
                                debug!("Multiple definitions of global wildcard rule '*', using the last one");
                            }
                            global_wildcard_rule =
                                Some((rule.action.clone(), rule.target.clone(), pattern.clone()));
                        } else if let Some(suffix) = pattern.strip_prefix(wildcards::PREFIX) {
                            // 验证通配符格式正确（必须是 *.suffix 格式）
                            if suffix.is_empty() || suffix.starts_with(wildcards::DOT) {
                                return Err(ConfigError::InvalidPattern(format!(
                                    "Invalid wildcard rule format: {}",
                                    pattern
                                )));
                            }

                            // 生成反转后缀键
                            let reversed_key = Self::reverse_domain_labels(suffix);

                            // 检查是否存在冲突
                            if wildcard_rules.contains_key(&reversed_key) {
                                debug!("Wildcard rule '{}' conflicts with existing rule, using the last one", pattern);
                            }

                            // 插入到通配符匹配树中
                            wildcard_rules.insert(
                                reversed_key,
                                (rule.action.clone(), rule.target.clone(), pattern.clone()),
                            );
                        } else {
                            return Err(ConfigError::InvalidPattern(format!(
                                "Invalid wildcard rule format: {}",
                                pattern
                            )));
                        }
                    }
                }
                MatchType::Regex => {
                    for pattern in rule.patterns {
                        // 编译正则表达式
                        let regex = Regex::new(&pattern)?;

                        // 添加正则表达式规则
                        regex_rules.push(CompiledRegexRule {
                            pattern,
                            regex,
                            action: rule.action.clone(),
                            target: rule.target.clone(),
                        });
                    }
                }
            }
        }

        // 创建正则表达式预筛选映射
        let regex_prefilter = Self::build_regex_prefilter(&regex_rules);

        // 更新路由规则数量指标
        METRICS
            .route_rules_count()
            .with_label_values(&[rule_type_labels::EXACT])
            .set(exact_rules.len() as f64);
        METRICS
            .route_rules_count()
            .with_label_values(&[rule_type_labels::WILDCARD])
            .set(
                wildcard_rules.len() as f64
                    + if global_wildcard_rule.is_some() {
                        1.0
                    } else {
                        0.0
                    },
            );
        METRICS
            .route_rules_count()
            .with_label_values(&[rule_type_labels::REGEX])
            .set(regex_rules.len() as f64);

        let mut router = Self {
            exact_rules,
            wildcard_rules,
            global_wildcard_rule,
            regex_rules,
            regex_prefilter,
            sorted_rules: Vec::new(),
        };

        // 对所有规则进行排序，确保它们按照正确的优先级顺序处理
        let sorted_rules = router.sort_rules()?;
        debug!("Sort {} routing rules in total", sorted_rules.len());
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
                .entry(wildcards::GLOBAL.to_string())
                .or_insert_with(HashSet::new)
                .insert(index);
        }

        prefilter
    }

    // 查找匹配的规则
    pub fn find_match(&self, query_name: &Name) -> Result<RouteMatch, AppError> {
        // 将查询名称转换为小写字符串，便于匹配
        let mut domain = query_name.to_string().to_lowercase();

        // 移除末尾可能存在的点，确保正则表达式等能正确匹配
        if domain.ends_with('.') {
            domain.pop();
        }

        // 为结果准备一些重用变量
        let target_default = rule_type_labels::NO_TARGET;

        // 尝试精确匹配 - 使用引用而非克隆
        if let Some((action, target)) = self.exact_rules.get(&domain) {
            let target_str = target.as_deref().unwrap_or(target_default);

            debug!(
                "Rule match: Exact match '{}' -> Target: {}",
                domain, target_str
            );

            // 记录路由匹配指标
            METRICS
                .route_matches_total()
                .with_label_values(&[rule_type_labels::EXACT, target_str])
                .inc();

            return Ok(RouteMatch {
                domain: domain.clone(),
                action: action.clone(),
                target: target.clone(),
                rule_type: rule_type_labels::EXACT,
                pattern: domain.clone(),
            });
        }

        // 预先分析域名部分 - 只分割一次，后续复用
        if !domain.is_empty() {
            // 只有当域名非空时才进行处理
            let domain_labels: Vec<&str> = domain.split('.').filter(|s| !s.is_empty()).collect();
            let num_labels = domain_labels.len();

            if num_labels > 0 {
                // 尝试通配符匹配
                // 从最长可能的后缀开始，避免了字符串复制
                // 预分配缓冲区以减少内存分配
                let mut current_suffix = String::with_capacity(domain.len());

                for k in (1..=num_labels).rev() {
                    // 重置缓冲区而非创建新的
                    current_suffix.clear();

                    // 构建当前后缀
                    let start_idx = num_labels - k;
                    for (i, label) in domain_labels[start_idx..].iter().enumerate() {
                        if i > 0 {
                            current_suffix.push('.');
                        }
                        current_suffix.push_str(label);
                    }

                    // 计算查找键（反转后缀）
                    let lookup_key = Self::reverse_domain_labels(&current_suffix);

                    // 在通配符匹配树中查找
                    if let Some((action, target, pattern)) = self.wildcard_rules.get(&lookup_key) {
                        let target_str = target.as_deref().unwrap_or(target_default);

                        debug!(
                            "Rule match: Wildcard match '{}' -> Pattern: '{}', Target: {}",
                            domain, pattern, target_str
                        );

                        // 记录路由匹配指标
                        METRICS
                            .route_matches_total()
                            .with_label_values(&[rule_type_labels::WILDCARD, target_str])
                            .inc();

                        return Ok(RouteMatch {
                            domain: domain.clone(),
                            action: action.clone(),
                            target: target.clone(),
                            rule_type: rule_type_labels::WILDCARD,
                            pattern: pattern.clone(),
                        });
                    }
                }

                // 尝试正则表达式匹配 (使用预筛选优化)
                // 直接复用已有的 domain_labels 而非重新分割

                // 查找符合条件的候选键，使用小容量预分配
                let mut candidate_keys = Vec::with_capacity(3);
                candidate_keys.push(wildcards::GLOBAL.to_string()); // 通配符键

                // 复用域名部分提取，避免重复查找点号位置
                if num_labels >= 1 {
                    // 添加顶级域名作为候选键
                    let tld = domain_labels[num_labels - 1];
                    let tld_key = format!(".{}", tld);
                    candidate_keys.push(tld_key); // 存储整个字符串而非引用

                    if num_labels >= 2 {
                        // 添加二级域名作为候选键
                        let sld = domain_labels[num_labels - 2];
                        // 构建 .sld.tld，避免重复字符串操作
                        let sld_tld = format!(".{}.{}", sld, tld);
                        candidate_keys.push(sld_tld); // 存储整个字符串而非引用
                    }
                }

                // 改进匹配候选正则表达式，减少HashSet操作
                // 先检查是否有可能匹配的规则
                let mut matched_rule_index = None;

                // 首先统计所有候选索引
                for key in &candidate_keys {
                    if let Some(indices) = self.regex_prefilter.get(key) {
                        // 尝试直接匹配，避免临时构建哈希集
                        for &index in indices {
                            let rule = &self.regex_rules[index];
                            if rule.regex.is_match(&domain) {
                                matched_rule_index = Some(index);
                                break;
                            }
                        }

                        if matched_rule_index.is_some() {
                            break;
                        }
                    }
                }

                // 如果找到匹配的规则，处理匹配结果
                if let Some(index) = matched_rule_index {
                    let rule = &self.regex_rules[index];
                    let target_str = rule.target.as_deref().unwrap_or(target_default);

                    debug!(
                        "Rule match: Regex match '{}' -> Pattern: '{}', Target: {}",
                        domain, rule.pattern, target_str
                    );

                    // 记录路由匹配指标
                    METRICS
                        .route_matches_total()
                        .with_label_values(&[rule_type_labels::REGEX, target_str])
                        .inc();

                    return Ok(RouteMatch {
                        domain: domain.clone(),
                        action: rule.action.clone(),
                        target: rule.target.clone(),
                        rule_type: rule_type_labels::REGEX,
                        pattern: rule.pattern.clone(),
                    });
                }

                // 最后尝试全局通配符匹配
                if let Some((action, target, pattern)) = &self.global_wildcard_rule {
                    let target_str = target.as_deref().unwrap_or(target_default);

                    debug!(
                        "Rule match: Global wildcard match '{}' -> Pattern: '{}', Target: {}",
                        domain, pattern, target_str
                    );

                    // 记录路由匹配指标
                    METRICS
                        .route_matches_total()
                        .with_label_values(&[rule_type_labels::WILDCARD, target_str])
                        .inc();

                    return Ok(RouteMatch {
                        domain: domain.clone(),
                        action: action.clone(),
                        target: target.clone(),
                        rule_type: rule_type_labels::WILDCARD,
                        pattern: pattern.clone(),
                    });
                }
            }
        } else if let Some((action, target, pattern)) = &self.global_wildcard_rule {
            // 空域名只检查全局通配符
            let target_str = target.as_deref().unwrap_or(target_default);

            debug!(
                "Rule match: Global wildcard match '{}' -> Pattern: '{}', Target: {}",
                domain, pattern, target_str
            );

            // 记录路由匹配指标
            METRICS
                .route_matches_total()
                .with_label_values(&[rule_type_labels::WILDCARD, target_str])
                .inc();

            return Ok(RouteMatch {
                domain: domain.clone(),
                action: action.clone(),
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
    // 1. 精确匹配规则 (最高优先级)
    // 2. 通配符规则 (按特定性排序)
    // 3. 正则表达式规则
    // 4. 全局通配符规则 (最低优先级)
    //
    // 返回的列表包含所有路由动作及其可能的目标上游组
    pub fn sort_rules(&self) -> Result<Vec<(RouteAction, Option<String>)>, ConfigError> {
        let mut rules = Vec::new();

        // 添加精确匹配规则
        for rule in self.exact_rules.values() {
            rules.push(rule.clone());
        }

        // 收集通配符规则并按特定性排序
        let mut wildcard_rules = Vec::new();

        // 添加特定通配符规则
        for rule in self.wildcard_rules.values() {
            wildcard_rules.push((
                rule.0.clone(),
                rule.1.clone(),
                rule.2.clone(),
                rule.2[2..].split('.').count(), // 计算特定性
            ));
        }

        // 按特定性从高到低排序
        wildcard_rules.sort_by(|a, b| b.3.cmp(&a.3));

        // 将排序后的规则添加到输出列表
        for rule in wildcard_rules {
            rules.push((rule.0, rule.1));
        }

        // 添加正则表达式规则
        for rule in &self.regex_rules {
            rules.push((rule.action.clone(), rule.target.clone()));
        }

        // 添加全局通配符规则（如果存在）
        if let Some((action, target, _)) = &self.global_wildcard_rule {
            rules.push((action.clone(), target.clone()));
        }

        Ok(rules)
    }

    // 获取排序后的规则列表
    #[allow(dead_code)]
    pub fn get_sorted_rules(&self) -> &Vec<(RouteAction, Option<String>)> {
        &self.sorted_rules
    }
}
