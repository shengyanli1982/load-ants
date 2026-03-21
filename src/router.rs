use crate::{
    error::ConfigError, metrics::METRICS, r#const::router::wildcards, rule_action_labels,
    rule_source_labels, rule_type_labels, AppError, MatchType, RouteAction, RouteRuleConfig,
};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleSourceType {
    Static,
    Remote,
}

impl RuleSourceType {
    pub fn as_label(&self) -> &'static str {
        match self {
            Self::Static => rule_source_labels::STATIC,
            Self::Remote => rule_source_labels::REMOTE,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleMetadata {
    pub source_type: RuleSourceType,
    pub source_id: Option<Arc<String>>,
}

impl RuleMetadata {
    pub fn static_rule() -> Self {
        Self {
            source_type: RuleSourceType::Static,
            source_id: None,
        }
    }

    pub fn remote<S: Into<String>>(source_id: S) -> Self {
        Self {
            source_type: RuleSourceType::Remote,
            source_id: Some(Arc::new(source_id.into())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedRule {
    pub rule: RouteRuleConfig,
    pub metadata: RuleMetadata,
}

impl RoutedRule {
    pub fn new(rule: RouteRuleConfig, metadata: RuleMetadata) -> Self {
        Self { rule, metadata }
    }

    pub fn from_static(rule: RouteRuleConfig) -> Self {
        Self::new(rule, RuleMetadata::static_rule())
    }
}

#[derive(Clone)]
struct RuleValue {
    target: Option<Arc<String>>,
    metadata: RuleMetadata,
}

struct CompiledRegexRule {
    pattern: String,
    regex: Regex,
    #[allow(dead_code)]
    action: RouteAction,
    value: RuleValue,
}

#[derive(Clone)]
struct WildcardRule {
    value: RuleValue,
    pattern: String,
}

// 添加类型别名用于简化复杂类型
/// 路由规则元组类型，包含(模式, 动作, 目标)
pub type RouteRuleTuple = (Option<String>, RouteAction, Option<Arc<String>>);

pub struct Router {
    exact_block_rules: HashMap<String, RuleValue>,
    exact_forward_rules: HashMap<String, RuleValue>,
    wildcard_block_rules: BTreeMap<String, WildcardRule>,
    wildcard_forward_rules: BTreeMap<String, WildcardRule>,
    global_wildcard_block_rule: Option<WildcardRule>,
    global_wildcard_forward_rule: Option<WildcardRule>,
    regex_block_rules: Vec<CompiledRegexRule>,
    regex_forward_rules: Vec<CompiledRegexRule>,
    regex_block_prefilter: HashMap<String, HashSet<usize>>,
    regex_forward_prefilter: HashMap<String, HashSet<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteMatch {
    pub domain: String,
    pub action: RouteAction,
    pub target: Option<String>,
    pub rule_type: &'static str,
    pub pattern: String,
    pub rule_source: &'static str,
    pub source_id: Option<String>,
}

fn describe_rule_source(metadata: &RuleMetadata) -> String {
    match metadata.source_id.as_ref() {
        Some(source_id) => format!("{} ({})", metadata.source_type.as_label(), source_id),
        None => metadata.source_type.as_label().to_string(),
    }
}

fn build_rule_conflict_error(
    rule_kind: &'static str,
    action: RouteAction,
    pattern: &str,
    existing: &RuleMetadata,
    incoming: &RuleMetadata,
) -> ConfigError {
    ConfigError::RuleConflict(format!(
        "Duplicate {} {} rule for pattern '{}': first defined by {}, duplicated by {}",
        <&'static str>::from(action),
        rule_kind,
        pattern,
        describe_rule_source(existing),
        describe_rule_source(incoming),
    ))
}

fn track_pattern_conflict(
    seen: &mut HashMap<String, RuleMetadata>,
    pattern: String,
    rule_kind: &'static str,
    action: RouteAction,
    metadata: &RuleMetadata,
) -> Result<(), ConfigError> {
    if let Some(existing) = seen.get(&pattern) {
        return Err(build_rule_conflict_error(
            rule_kind, action, &pattern, existing, metadata,
        ));
    }

    seen.insert(pattern, metadata.clone());
    Ok(())
}

fn track_global_wildcard_conflict(
    seen: &mut Option<RuleMetadata>,
    action: RouteAction,
    metadata: &RuleMetadata,
) -> Result<(), ConfigError> {
    if let Some(existing) = seen.as_ref() {
        return Err(build_rule_conflict_error(
            "global wildcard",
            action,
            wildcards::GLOBAL,
            existing,
            metadata,
        ));
    }

    *seen = Some(metadata.clone());
    Ok(())
}

impl Router {
    #[inline(always)]
    fn normalize_domain_like(mut s: String) -> String {
        if s.ends_with('.') {
            s.pop();
        }
        if s.is_ascii() {
            s.make_ascii_lowercase();
        } else {
            s = s.to_lowercase();
        }
        s
    }

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

    pub fn new(rules: Vec<RouteRuleConfig>) -> Result<Self, ConfigError> {
        let tracked_rules = rules.into_iter().map(RoutedRule::from_static).collect();
        Self::new_with_metadata(tracked_rules)
    }

    pub(crate) fn validate_rule_conflicts(rules: &[RoutedRule]) -> Result<(), ConfigError> {
        let mut exact_block_rules = HashMap::new();
        let mut exact_forward_rules = HashMap::new();
        let mut wildcard_block_rules = HashMap::new();
        let mut wildcard_forward_rules = HashMap::new();
        let mut regex_block_rules = HashMap::new();
        let mut regex_forward_rules = HashMap::new();
        let mut global_wildcard_block_rule = None;
        let mut global_wildcard_forward_rule = None;

        for routed_rule in rules {
            let RoutedRule { rule, metadata } = routed_rule;

            match rule.match_type {
                MatchType::Exact => {
                    for pattern in &rule.patterns {
                        let pattern = Self::normalize_domain_like(pattern.clone());
                        match rule.action {
                            RouteAction::Block => track_pattern_conflict(
                                &mut exact_block_rules,
                                pattern,
                                rule_type_labels::EXACT,
                                rule.action,
                                metadata,
                            )?,
                            RouteAction::Forward => track_pattern_conflict(
                                &mut exact_forward_rules,
                                pattern,
                                rule_type_labels::EXACT,
                                rule.action,
                                metadata,
                            )?,
                        }
                    }
                }
                MatchType::Wildcard => {
                    for pattern in &rule.patterns {
                        if pattern == wildcards::GLOBAL {
                            match rule.action {
                                RouteAction::Block => track_global_wildcard_conflict(
                                    &mut global_wildcard_block_rule,
                                    rule.action,
                                    metadata,
                                )?,
                                RouteAction::Forward => track_global_wildcard_conflict(
                                    &mut global_wildcard_forward_rule,
                                    rule.action,
                                    metadata,
                                )?,
                            }
                            continue;
                        }

                        let suffix = pattern.strip_prefix("*.").ok_or_else(|| {
                            ConfigError::InvalidRouteRule(format!(
                                "Invalid wildcard pattern '{}': expected '*.domain.tld' or '*'",
                                pattern
                            ))
                        })?;
                        let suffix = Self::normalize_domain_like(suffix.to_string());
                        let normalized_pattern = format!("*.{}", suffix);

                        match rule.action {
                            RouteAction::Block => track_pattern_conflict(
                                &mut wildcard_block_rules,
                                normalized_pattern,
                                rule_type_labels::WILDCARD,
                                rule.action,
                                metadata,
                            )?,
                            RouteAction::Forward => track_pattern_conflict(
                                &mut wildcard_forward_rules,
                                normalized_pattern,
                                rule_type_labels::WILDCARD,
                                rule.action,
                                metadata,
                            )?,
                        }
                    }
                }
                MatchType::Regex => {
                    for pattern in &rule.patterns {
                        match rule.action {
                            RouteAction::Block => track_pattern_conflict(
                                &mut regex_block_rules,
                                pattern.clone(),
                                rule_type_labels::REGEX,
                                rule.action,
                                metadata,
                            )?,
                            RouteAction::Forward => track_pattern_conflict(
                                &mut regex_forward_rules,
                                pattern.clone(),
                                rule_type_labels::REGEX,
                                rule.action,
                                metadata,
                            )?,
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub fn new_with_metadata(rules: Vec<RoutedRule>) -> Result<Self, ConfigError> {
        Self::validate_rule_conflicts(&rules)?;

        let mut exact_block_rules = HashMap::new();
        let mut exact_forward_rules = HashMap::new();
        let mut wildcard_block_rules = BTreeMap::new();
        let mut wildcard_forward_rules = BTreeMap::new();
        let mut global_wildcard_block_rule = None;
        let mut global_wildcard_forward_rule = None;
        let mut regex_block_rules = Vec::new();
        let mut regex_forward_rules = Vec::new();

        let mut exact_static_count = 0_i64;
        let mut exact_remote_count = 0_i64;
        let mut wildcard_static_count = 0_i64;
        let mut wildcard_remote_count = 0_i64;
        let mut regex_static_count = 0_i64;
        let mut regex_remote_count = 0_i64;

        for routed_rule in rules {
            let RoutedRule { rule, metadata } = routed_rule;
            let target = rule.target.map(Arc::new);
            let pattern_count = rule.patterns.len() as i64;

            match (&rule.match_type, &metadata.source_type) {
                (MatchType::Exact, RuleSourceType::Static) => exact_static_count += pattern_count,
                (MatchType::Exact, RuleSourceType::Remote) => exact_remote_count += pattern_count,
                (MatchType::Wildcard, RuleSourceType::Static) => {
                    wildcard_static_count += pattern_count
                }
                (MatchType::Wildcard, RuleSourceType::Remote) => {
                    wildcard_remote_count += pattern_count
                }
                (MatchType::Regex, RuleSourceType::Static) => regex_static_count += pattern_count,
                (MatchType::Regex, RuleSourceType::Remote) => regex_remote_count += pattern_count,
            }

            match rule.match_type {
                MatchType::Exact => {
                    for pattern in rule.patterns {
                        let pattern = Self::normalize_domain_like(pattern);
                        let value = RuleValue {
                            target: target.clone(),
                            metadata: metadata.clone(),
                        };

                        match rule.action {
                            RouteAction::Block => {
                                exact_block_rules.insert(pattern, value);
                            }
                            RouteAction::Forward => {
                                exact_forward_rules.insert(pattern, value);
                            }
                        }
                    }
                }
                MatchType::Wildcard => {
                    for pattern in rule.patterns {
                        if pattern == wildcards::GLOBAL {
                            let rule_value = RuleValue {
                                target: target.clone(),
                                metadata: metadata.clone(),
                            };

                            match rule.action {
                                RouteAction::Block => {
                                    if global_wildcard_block_rule.is_some() {
                                        debug!("Multiple definitions of global wildcard block rule '*', using the last one");
                                    }
                                    global_wildcard_block_rule = Some(WildcardRule {
                                        value: rule_value,
                                        pattern,
                                    });
                                }
                                RouteAction::Forward => {
                                    if global_wildcard_forward_rule.is_some() {
                                        debug!("Multiple definitions of global wildcard forward rule '*', using the last one");
                                    }
                                    global_wildcard_forward_rule = Some(WildcardRule {
                                        value: rule_value,
                                        pattern,
                                    });
                                }
                            }
                        } else {
                            let suffix = pattern.strip_prefix("*.").ok_or_else(|| {
                                ConfigError::InvalidRouteRule(format!(
                                    "Invalid wildcard pattern '{}': expected '*.domain.tld' or '*'",
                                    pattern
                                ))
                            })?;
                            let suffix = Self::normalize_domain_like(suffix.to_string());
                            let normalized_pattern = format!("*.{}", suffix);
                            let reversed_suffix = Self::reverse_domain_labels(&suffix);
                            let rule_value = RuleValue {
                                target: target.clone(),
                                metadata: metadata.clone(),
                            };

                            match rule.action {
                                RouteAction::Block => {
                                    wildcard_block_rules.insert(
                                        reversed_suffix,
                                        WildcardRule {
                                            value: rule_value,
                                            pattern: normalized_pattern,
                                        },
                                    );
                                }
                                RouteAction::Forward => {
                                    wildcard_forward_rules.insert(
                                        reversed_suffix,
                                        WildcardRule {
                                            value: rule_value,
                                            pattern: normalized_pattern,
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
                MatchType::Regex => {
                    let action = rule.action;
                    for pattern in rule.patterns {
                        let regex = Regex::new(&pattern)?;
                        let rule_value = RuleValue {
                            target: target.clone(),
                            metadata: metadata.clone(),
                        };

                        match action {
                            RouteAction::Block => {
                                regex_block_rules.push(CompiledRegexRule {
                                    pattern,
                                    regex,
                                    action: RouteAction::Block,
                                    value: rule_value,
                                });
                            }
                            RouteAction::Forward => {
                                regex_forward_rules.push(CompiledRegexRule {
                                    pattern,
                                    regex,
                                    action: RouteAction::Forward,
                                    value: rule_value,
                                });
                            }
                        }
                    }
                }
            }
        }

        let regex_block_prefilter = Self::build_regex_prefilter(&regex_block_rules);
        let regex_forward_prefilter = Self::build_regex_prefilter(&regex_forward_rules);

        METRICS
            .route_rules_count()
            .with_label_values(&[rule_type_labels::EXACT, rule_source_labels::STATIC])
            .set(exact_static_count);
        METRICS
            .route_rules_count()
            .with_label_values(&[rule_type_labels::EXACT, rule_source_labels::REMOTE])
            .set(exact_remote_count);
        METRICS
            .route_rules_count()
            .with_label_values(&[rule_type_labels::WILDCARD, rule_source_labels::STATIC])
            .set(wildcard_static_count);
        METRICS
            .route_rules_count()
            .with_label_values(&[rule_type_labels::WILDCARD, rule_source_labels::REMOTE])
            .set(wildcard_remote_count);
        METRICS
            .route_rules_count()
            .with_label_values(&[rule_type_labels::REGEX, rule_source_labels::STATIC])
            .set(regex_static_count);
        METRICS
            .route_rules_count()
            .with_label_values(&[rule_type_labels::REGEX, rule_source_labels::REMOTE])
            .set(regex_remote_count);

        Ok(Self {
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
        })
    }

    fn try_exact_match(&self, domain: &str, action: RouteAction) -> Option<RouteMatch> {
        let rules = match action {
            RouteAction::Block => &self.exact_block_rules,
            RouteAction::Forward => &self.exact_forward_rules,
        };

        if let Some(rule) = rules.get(domain) {
            let target_default = rule_type_labels::NO_TARGET;
            let target_str = rule
                .target
                .as_ref()
                .map(|s| s.as_str())
                .unwrap_or(target_default);
            let source_label = rule.metadata.source_type.as_label();

            debug!(
                "Rule match: Exact {:?} match '{}' -> Target: {}, Source: {}",
                action, domain, target_str, source_label
            );

            METRICS
                .route_matches_total()
                .with_label_values(&[
                    rule_type_labels::EXACT,
                    target_str,
                    source_label,
                    <&'static str>::from(action),
                ])
                .inc();

            return Some(RouteMatch {
                domain: domain.to_string(),
                action,
                target: rule.target.as_ref().map(|arc_str| arc_str.to_string()),
                rule_type: rule_type_labels::EXACT,
                pattern: domain.to_string(),
                rule_source: source_label,
                source_id: rule
                    .metadata
                    .source_id
                    .as_ref()
                    .map(|value| value.to_string()),
            });
        }

        None
    }

    fn try_wildcard_match(&self, domain: &str, action: RouteAction) -> Option<RouteMatch> {
        let rules = match action {
            RouteAction::Block => &self.wildcard_block_rules,
            RouteAction::Forward => &self.wildcard_forward_rules,
        };

        let target_default = rule_type_labels::NO_TARGET;
        let reversed = Self::reverse_domain_labels(domain);
        let mut end = reversed.len();

        loop {
            let key = &reversed[..end];

            if let Some(rule) = rules.get(key) {
                let target_str = rule
                    .value
                    .target
                    .as_ref()
                    .map(|s| s.as_str())
                    .unwrap_or(target_default);
                let source_label = rule.value.metadata.source_type.as_label();

                debug!(
                    "Rule match: Wildcard {:?} match '{}' -> Pattern: '{}', Target: {}, Source: {}",
                    action,
                    domain,
                    rule.pattern.as_str(),
                    target_str,
                    source_label
                );

                METRICS
                    .route_matches_total()
                    .with_label_values(&[
                        rule_type_labels::WILDCARD,
                        target_str,
                        source_label,
                        <&'static str>::from(action),
                    ])
                    .inc();

                return Some(RouteMatch {
                    domain: domain.to_string(),
                    action,
                    target: rule
                        .value
                        .target
                        .as_ref()
                        .map(|arc_str| arc_str.to_string()),
                    rule_type: rule_type_labels::WILDCARD,
                    pattern: rule.pattern.clone(),
                    rule_source: source_label,
                    source_id: rule
                        .value
                        .metadata
                        .source_id
                        .as_ref()
                        .map(|value| value.to_string()),
                });
            }

            match key.rfind('.') {
                Some(pos) => end = pos,
                None => break,
            }
        }

        None
    }

    fn try_regex_match(&self, domain: &str, action: RouteAction) -> Option<RouteMatch> {
        let (rules, prefilter) = match action {
            RouteAction::Block => (&self.regex_block_rules, &self.regex_block_prefilter),
            RouteAction::Forward => (&self.regex_forward_rules, &self.regex_forward_prefilter),
        };

        if rules.is_empty() {
            return None;
        }

        let target_default = rule_type_labels::NO_TARGET;
        let mut candidates: Vec<usize> = Vec::new();
        for segment in domain.split('.') {
            if segment.len() < 2 {
                continue;
            }

            if let Some(rule_indices) = prefilter.get(segment) {
                candidates.extend(rule_indices.iter().copied());
            }
        }

        if candidates.is_empty() {
            return None;
        }

        candidates.sort_unstable();
        candidates.dedup();

        for &rule_idx in candidates.iter().rev() {
            let rule = &rules[rule_idx];
            if rule.regex.is_match(domain) {
                let target_str = rule
                    .value
                    .target
                    .as_ref()
                    .map(|s| s.as_str())
                    .unwrap_or(target_default);
                let source_label = rule.value.metadata.source_type.as_label();

                debug!(
                    "Rule match: Regex {:?} match '{}' -> Pattern: '{}', Target: {}, Source: {}",
                    action,
                    domain,
                    rule.pattern.as_str(),
                    target_str,
                    source_label
                );

                METRICS
                    .route_matches_total()
                    .with_label_values(&[
                        rule_type_labels::REGEX,
                        target_str,
                        source_label,
                        <&'static str>::from(action),
                    ])
                    .inc();

                return Some(RouteMatch {
                    domain: domain.to_string(),
                    action,
                    target: rule
                        .value
                        .target
                        .as_ref()
                        .map(|arc_str| arc_str.to_string()),
                    rule_type: rule_type_labels::REGEX,
                    pattern: rule.pattern.clone(),
                    rule_source: source_label,
                    source_id: rule
                        .value
                        .metadata
                        .source_id
                        .as_ref()
                        .map(|value| value.to_string()),
                });
            }
        }

        None
    }

    fn try_global_wildcard_match(&self, domain: &str, action: RouteAction) -> Option<RouteMatch> {
        let global_rule = match action {
            RouteAction::Block => &self.global_wildcard_block_rule,
            RouteAction::Forward => &self.global_wildcard_forward_rule,
        };

        if let Some(rule) = global_rule {
            let target_default = rule_type_labels::NO_TARGET;
            let target_str = rule
                .value
                .target
                .as_ref()
                .map(|s| s.as_str())
                .unwrap_or(target_default);
            let source_label = rule.value.metadata.source_type.as_label();

            debug!(
            "Rule match: Global wildcard {:?} match '{}' -> Pattern: '{}', Target: {}, Source: {}",
            action,
            domain,
            rule.pattern.as_str(),
            target_str,
            source_label
        );

            METRICS
                .route_matches_total()
                .with_label_values(&[
                    rule_type_labels::WILDCARD,
                    target_str,
                    source_label,
                    <&'static str>::from(action),
                ])
                .inc();

            return Some(RouteMatch {
                domain: domain.to_string(),
                action,
                target: rule
                    .value
                    .target
                    .as_ref()
                    .map(|arc_str| arc_str.to_string()),
                rule_type: rule_type_labels::WILDCARD,
                pattern: rule.pattern.clone(),
                rule_source: source_label,
                source_id: rule
                    .value
                    .metadata
                    .source_id
                    .as_ref()
                    .map(|value| value.to_string()),
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
        // 将查询名称转换为字符串（可能包含非 ASCII label），再做大小写归一化以便匹配。
        //
        // 性能：绝大多数域名是 ASCII（punycode 亦为 ASCII），这里用 make_ascii_lowercase 原地转换，
        // 避免 `to_lowercase()` 产生的新 String 分配；非 ASCII 则回退 Unicode lower 以保持语义。
        let mut domain = query_name.to_string();

        // 移除末尾可能存在的点，确保正则表达式等能正确匹配
        if domain.ends_with('.') {
            domain.pop();
        }

        if domain.is_empty() {
            return Err(AppError::NoRouteMatch(domain));
        }

        if domain.is_ascii() {
            domain.make_ascii_lowercase();
        } else {
            domain = domain.to_lowercase();
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
