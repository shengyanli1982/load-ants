use crate::{
    error::ConfigError, metrics::METRICS, r#const::router::wildcards, rule_action_labels,
    rule_source_labels, rule_type_labels, AppError, MatchType, RouteAction, RouteRuleConfig,
};
use hickory_proto::rr::Name;
use memchr::memmem::Finder;
use prometheus::IntCounter;
use regex::Regex;
use rustc_hash::FxBuildHasher;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::debug;

const REGEX_SPECIAL_CHARS: [char; 15] = [
    '\\', '^', '$', '.', '|', '?', '*', '+', '(', ')', '[', ']', '{', '}', '-',
];

const MAX_DOMAIN_STACK_LEN: usize = 253;

/// 在栈缓冲上构造全小写域名，结果与 `to_ascii()` + 去尾点 + 小写化逐字节一致，
/// 用于消除 `find_match` 热路径上 `Name::to_ascii()` 的堆分配。
/// 当名字包含需要转义的字节或超出缓冲容量时返回 `None`，由调用方回退堆路径。
fn build_lowercase_domain<'buf>(
    query_name: &Name,
    buf: &'buf mut [u8; MAX_DOMAIN_STACK_LEN],
) -> Option<&'buf str> {
    let mut pos = 0usize;
    for (label_index, label) in query_name.iter().enumerate() {
        let separator_len = usize::from(label_index > 0);
        if pos + separator_len + label.len() > buf.len() {
            return None;
        }
        if separator_len == 1 {
            buf[pos] = b'.';
            pos += 1;
        }
        for (byte_index, &byte) in label.iter().enumerate() {
            if !is_label_byte_passthrough(byte, byte_index == 0) {
                return None;
            }
            buf[pos] = byte.to_ascii_lowercase();
            pos += 1;
        }
    }
    std::str::from_utf8(&buf[..pos]).ok()
}

/// 判定 label 字节在 `Label::write_ascii` 中是否会被原样写出（不触发转义）。
/// 语义须与 hickory-proto `is_safe_ascii(c, is_first, for_encoding = true)` 保持一致。
fn is_label_byte_passthrough(byte: u8, is_first: bool) -> bool {
    byte.is_ascii_alphanumeric()
        || byte == b'_'
        || (is_first && byte == b'*')
        || (!is_first && byte == b'-')
}

/// 回退路径：保留原 `to_ascii()` + 去尾点 + 小写化逻辑（用于含转义字节或超长名字）。
fn domain_string_fallback(query_name: &Name) -> String {
    let mut domain = query_name.to_ascii();
    if domain.ends_with('.') {
        domain.pop();
    }
    if domain.is_ascii() {
        domain.make_ascii_lowercase();
    } else {
        domain = domain.to_lowercase();
    }
    domain
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
    /// 规则加载时预解析的 route_matches_total 句柄，命中时直接 inc()，
    /// 避免热路径上做 with_label_values 标签哈希查找
    match_counter: IntCounter,
}

/// 单条精确匹配规则
#[derive(Clone)]
struct ExactRule {
    value: RuleValue,
    /// 构建时 normalize 后的 pattern（同时也是 HashMap 的 key 来源）
    pattern: String,
}

/// 同一 pattern 对应的 Block + Forward 两条规则（同一 action 类型在构建时已拒绝重复）。
/// `find_match` 中单次 HashMap 查找拿到本结构，先检查 `block`，再检查 `forward`。
#[derive(Clone, Default)]
struct ExactRulePair {
    block: Option<ExactRule>,
    forward: Option<ExactRule>,
}

struct CompiledRegexRule {
    pattern: String,
    regex: Regex,
    /// 规则动作类型，用于调试和日志
    #[allow(dead_code)]
    action: RouteAction,
    value: RuleValue,
}

/// 正则预筛选项：规则加载时预编译的关键词子串搜索器 + 该关键词对应的候选规则索引。
/// 匹配时用 `Finder::find` 做子串判断，避免每查询重建 TwoWay 搜索器。
struct RegexPrefilterEntry {
    finder: Finder<'static>,
    rule_indices: Vec<usize>,
}

#[derive(Clone)]
struct WildcardRule {
    value: RuleValue,
    pattern: String,
}

/// Label-based trie node for wildcard rule lookup.
///
/// Each level represents one DNS label (right-to-left from root).
/// For `*.corp.example`, labels `["example", "corp"]` are inserted into the trie,
/// and the rule is stored at the leaf node.
struct WildcardTrieNode {
    children: HashMap<String, WildcardTrieNode, FxBuildHasher>,
    rule: Option<WildcardRule>,
}

impl WildcardTrieNode {
    fn new() -> Self {
        Self {
            children: HashMap::with_hasher(FxBuildHasher),
            rule: None,
        }
    }

    /// Inserts a wildcard rule using reversed label path.
    /// `suffix` is the normalized domain part after `*.` (e.g. `corp.example`).
    fn insert(&mut self, suffix: &str, rule: WildcardRule) {
        let mut current = self;
        for label in suffix.rsplit('.') {
            current = current
                .children
                .entry(label.to_string())
                .or_insert_with(WildcardTrieNode::new);
        }
        current.rule = Some(rule);
    }

    /// Counts the number of rules stored in the trie.
    fn rule_count(&self) -> usize {
        self.rule.is_some() as usize
            + self
                .children
                .values()
                .map(|c| c.rule_count())
                .sum::<usize>()
    }
}

pub struct Router {
    /// 精确匹配规则（合并 Block + Forward 到一个 HashMap），减少一次 HashMap 查找
    exact_rules: HashMap<String, ExactRulePair, FxBuildHasher>,
    wildcard_block_trie: WildcardTrieNode,
    wildcard_forward_trie: WildcardTrieNode,
    global_wildcard_block_rule: Option<WildcardRule>,
    global_wildcard_forward_rule: Option<WildcardRule>,
    regex_block_rules: Vec<CompiledRegexRule>,
    regex_forward_rules: Vec<CompiledRegexRule>,
    regex_block_prefilter: Vec<RegexPrefilterEntry>,
    regex_forward_prefilter: Vec<RegexPrefilterEntry>,
    /// 无法从模式提取预筛选关键词的正则规则索引，匹配时始终检查
    regex_block_always_check: Vec<usize>,
    /// 无法从模式提取预筛选关键词的正则规则索引，匹配时始终检查
    regex_forward_always_check: Vec<usize>,
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
    seen: &mut HashMap<String, RuleMetadata, FxBuildHasher>,
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

    // 构建正则表达式预筛选项（加载时预编译关键词子串搜索器），
    // 同时返回无法提取关键词的规则索引列表（always_check）
    fn build_regex_prefilter(
        rules: &[CompiledRegexRule],
    ) -> (Vec<RegexPrefilterEntry>, Vec<usize>) {
        let mut prefilter: HashMap<String, Vec<usize>, FxBuildHasher> =
            HashMap::with_hasher(FxBuildHasher);
        let mut always_check = Vec::new();

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
                    .or_default()
                    .push(i);
            } else {
                // P1-10: 无法提取关键词的规则，记入 always_check，匹配时始终检查
                always_check.push(i);
            }
        }

        let entries = prefilter
            .into_iter()
            .map(|(keyword, rule_indices)| RegexPrefilterEntry {
                finder: Finder::new(keyword.as_str()).into_owned(),
                rule_indices,
            })
            .collect();

        (entries, always_check)
    }

    /// 按 (rule_type, target, source, action) 标签组合预解析 route_matches_total 句柄，
    /// 命中热路径上只需 `inc()`，不再做标签哈希查找
    fn resolve_match_counter(
        rule_type: &'static str,
        action: RouteAction,
        target: Option<&str>,
        metadata: &RuleMetadata,
    ) -> IntCounter {
        METRICS.route_matches_total.with_label_values(&[
            rule_type,
            target.unwrap_or(rule_type_labels::NO_TARGET),
            metadata.source_type.as_label(),
            <&'static str>::from(action),
        ])
    }

    pub fn new(rules: Vec<RouteRuleConfig>) -> Result<Self, ConfigError> {
        let tracked_rules = rules.into_iter().map(RoutedRule::from_static).collect();
        Self::new_with_metadata(tracked_rules)
    }

    pub fn rule_count(&self) -> usize {
        self.exact_rules.len()
            + self.wildcard_block_trie.rule_count()
            + self.wildcard_forward_trie.rule_count()
            + self.global_wildcard_block_rule.iter().count()
            + self.global_wildcard_forward_rule.iter().count()
            + self.regex_block_rules.len()
            + self.regex_forward_rules.len()
    }

    pub(crate) fn validate_rule_conflicts(rules: &[RoutedRule]) -> Result<(), ConfigError> {
        // exact 规则按 action 分开追踪冲突：同一 pattern 可以同时拥有 Block 与 Forward，
        // 但同一 (pattern, action) 组合不允许重复。
        let mut exact_block_rules = HashMap::with_hasher(FxBuildHasher);
        let mut exact_forward_rules = HashMap::with_hasher(FxBuildHasher);
        let mut wildcard_block_rules = HashMap::with_hasher(FxBuildHasher);
        let mut wildcard_forward_rules = HashMap::with_hasher(FxBuildHasher);
        let mut regex_block_rules = HashMap::with_hasher(FxBuildHasher);
        let mut regex_forward_rules = HashMap::with_hasher(FxBuildHasher);
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

        let mut exact_rules = HashMap::with_hasher(FxBuildHasher);
        let mut wildcard_block_trie = WildcardTrieNode::new();
        let mut wildcard_forward_trie = WildcardTrieNode::new();
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
                    let action = rule.action;
                    let match_counter = Self::resolve_match_counter(
                        rule_type_labels::EXACT,
                        action,
                        target.as_deref().map(String::as_str),
                        &metadata,
                    );
                    for pattern in rule.patterns {
                        let pattern = Self::normalize_domain_like(pattern);
                        let value = RuleValue {
                            target: target.clone(),
                            metadata: metadata.clone(),
                            match_counter: match_counter.clone(),
                        };
                        let exact_rule = ExactRule {
                            value,
                            pattern: pattern.clone(),
                        };
                        let entry = exact_rules
                            .entry(pattern)
                            .or_insert_with(ExactRulePair::default);
                        match action {
                            RouteAction::Block => entry.block = Some(exact_rule),
                            RouteAction::Forward => entry.forward = Some(exact_rule),
                        }
                    }
                }
                MatchType::Wildcard => {
                    let match_counter = Self::resolve_match_counter(
                        rule_type_labels::WILDCARD,
                        rule.action,
                        target.as_deref().map(String::as_str),
                        &metadata,
                    );
                    for pattern in rule.patterns {
                        if pattern == wildcards::GLOBAL {
                            let rule_value = RuleValue {
                                target: target.clone(),
                                metadata: metadata.clone(),
                                match_counter: match_counter.clone(),
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
                            let rule_value = RuleValue {
                                target: target.clone(),
                                metadata: metadata.clone(),
                                match_counter: match_counter.clone(),
                            };
                            let wildcard_rule = WildcardRule {
                                value: rule_value,
                                pattern: normalized_pattern,
                            };

                            match rule.action {
                                RouteAction::Block => {
                                    wildcard_block_trie.insert(&suffix, wildcard_rule);
                                }
                                RouteAction::Forward => {
                                    wildcard_forward_trie.insert(&suffix, wildcard_rule);
                                }
                            }
                        }
                    }
                }
                MatchType::Regex => {
                    let action = rule.action;
                    let match_counter = Self::resolve_match_counter(
                        rule_type_labels::REGEX,
                        action,
                        target.as_deref().map(String::as_str),
                        &metadata,
                    );
                    for pattern in rule.patterns {
                        let regex = Regex::new(&pattern)?;
                        let rule_value = RuleValue {
                            target: target.clone(),
                            metadata: metadata.clone(),
                            match_counter: match_counter.clone(),
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

        let (regex_block_prefilter, regex_block_always_check) =
            Self::build_regex_prefilter(&regex_block_rules);
        let (regex_forward_prefilter, regex_forward_always_check) =
            Self::build_regex_prefilter(&regex_forward_rules);

        METRICS
            .route_rules_count
            .with_label_values(&[rule_type_labels::EXACT, rule_source_labels::STATIC])
            .set(exact_static_count);
        METRICS
            .route_rules_count
            .with_label_values(&[rule_type_labels::EXACT, rule_source_labels::REMOTE])
            .set(exact_remote_count);
        METRICS
            .route_rules_count
            .with_label_values(&[rule_type_labels::WILDCARD, rule_source_labels::STATIC])
            .set(wildcard_static_count);
        METRICS
            .route_rules_count
            .with_label_values(&[rule_type_labels::WILDCARD, rule_source_labels::REMOTE])
            .set(wildcard_remote_count);
        METRICS
            .route_rules_count
            .with_label_values(&[rule_type_labels::REGEX, rule_source_labels::STATIC])
            .set(regex_static_count);
        METRICS
            .route_rules_count
            .with_label_values(&[rule_type_labels::REGEX, rule_source_labels::REMOTE])
            .set(regex_remote_count);

        Ok(Self {
            exact_rules,
            wildcard_block_trie,
            wildcard_forward_trie,
            global_wildcard_block_rule,
            global_wildcard_forward_rule,
            regex_block_rules,
            regex_forward_rules,
            regex_block_prefilter,
            regex_forward_prefilter,
            regex_block_always_check,
            regex_forward_always_check,
        })
    }

    fn build_route_match(
        &self,
        domain: &str,
        action: RouteAction,
        rule_type: &'static str,
        pattern: String,
        value: &RuleValue,
    ) -> RouteMatch {
        let target = value.target.as_deref().map(String::as_str);
        let target_for_label = target.unwrap_or(rule_type_labels::NO_TARGET);
        let source_label = value.metadata.source_type.as_label();

        debug!(
            "Rule match: {} {:?} match '{}' -> Pattern: '{}', Target: {}, Source: {}",
            rule_type, action, domain, pattern, target_for_label, source_label
        );

        value.match_counter.inc();

        RouteMatch {
            domain: domain.to_string(),
            action,
            target: target.map(|s| s.to_string()),
            rule_type,
            pattern,
            rule_source: source_label,
            source_id: value.metadata.source_id.as_ref().map(|v| v.to_string()),
        }
    }

    fn try_wildcard_match(
        &self,
        name: &Name,
        domain: &str,
        action: RouteAction,
    ) -> Option<RouteMatch> {
        let trie = match action {
            RouteAction::Block => &self.wildcard_block_trie,
            RouteAction::Forward => &self.wildcard_forward_trie,
        };

        if trie.children.is_empty() {
            return None;
        }

        let label_count = usize::from(name.num_labels());

        // 至少需要 2 个 label：1 个给 suffix 匹配，1 个留给 wildcard 前缀
        if label_count < 2 {
            return None;
        }

        // 最多消费 label_count - 1 个 label（从最右开始向左），
        // 保留至少 1 个 label 确保 domain 是 suffix 的真正子域名。
        let max_depth = label_count - 1;
        let mut current = trie;
        let mut best_match: Option<&WildcardRule> = None;

        for label_bytes in name.iter().rev().take(max_depth) {
            let label = std::str::from_utf8(label_bytes).unwrap_or("");
            // Name::iter() 返回的 labels 已保证小写；仅在罕见大写路径下分配
            let lookup = if label.as_bytes().iter().any(u8::is_ascii_uppercase) {
                std::borrow::Cow::Owned(label.to_ascii_lowercase())
            } else {
                std::borrow::Cow::Borrowed(label)
            };

            match current.children.get(lookup.as_ref()) {
                Some(child) => {
                    if child.rule.is_some() {
                        best_match = child.rule.as_ref();
                    }
                    current = child;
                }
                None => break,
            }
        }

        best_match.map(|rule| {
            self.build_route_match(
                domain,
                action,
                rule_type_labels::WILDCARD,
                rule.pattern.clone(),
                &rule.value,
            )
        })
    }

    fn try_regex_match(&self, domain: &str, action: RouteAction) -> Option<RouteMatch> {
        let (rules, prefilter, always_check) = match action {
            RouteAction::Block => (
                &self.regex_block_rules,
                &self.regex_block_prefilter,
                &self.regex_block_always_check,
            ),
            RouteAction::Forward => (
                &self.regex_forward_rules,
                &self.regex_forward_prefilter,
                &self.regex_forward_always_check,
            ),
        };

        if rules.is_empty() {
            return None;
        }

        // 关键词是正则 pattern 的字面子串，域名包含关键词是正则命中的必要条件。
        // 用子串包含检查收集候选：关键词可能只是某个 label 的一部分（如 `tracking`
        // 之于 `my-tracking`），按完整 label 精确查找会漏报。
        // 搜索器在规则加载时预编译，热路径只做字节级子串查找。
        let mut candidates: Vec<usize> = Vec::new();
        for entry in prefilter {
            if entry.finder.find(domain.as_bytes()).is_some() {
                candidates.extend(entry.rule_indices.iter().copied());
            }
        }

        // P1-10: 始终将无法提取预筛选关键词的规则加入候选列表
        candidates.extend_from_slice(always_check);

        if candidates.is_empty() {
            return None;
        }

        candidates.sort_unstable();
        candidates.dedup();

        // 后定义规则优先级更高，匹配时从后往前迭代
        for &rule_idx in candidates.iter().rev() {
            let rule = &rules[rule_idx];
            if rule.regex.is_match(domain) {
                return Some(self.build_route_match(
                    domain,
                    action,
                    rule_type_labels::REGEX,
                    rule.pattern.clone(),
                    &rule.value,
                ));
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
            return Some(self.build_route_match(
                domain,
                action,
                rule_type_labels::WILDCARD,
                rule.pattern.clone(),
                &rule.value,
            ));
        }

        None
    }

    // 查找匹配规则
    //
    // 查找顺序（优先级从高到低）：
    // 1. 精确匹配（Block 与 Forward 合并查找，一次哈希）
    // 2. 通配符 block 规则（按特定性从高到低）
    // 3. 正则表达式 block 规则
    // 4. 全局通配符 block 规则
    // 5. 通配符 forward 规则（按特定性从高到低）
    // 6. 正则表达式 forward 规则
    // 7. 全局通配符 forward 规则
    //
    // 这种优先级顺序确保：
    // - 所有 block 规则优先于所有 forward 规则
    // - 在同类规则中，遵循通配符匹配 > 正则匹配 > 全局通配符的优先级
    // - 精确匹配单次 HashMap 查找即可判定 block/forward（同 pattern 同时拥有两者时 block 优先）
    pub fn find_match(&self, query_name: &Name) -> Result<RouteMatch, AppError> {
        // 快路径：栈缓冲构造小写域名，消除 `Name::to_ascii()` 堆分配；
        // 含转义字节或超长名字回退 `domain_string_fallback`（原语义不变）。
        let mut stack_buf = [0u8; MAX_DOMAIN_STACK_LEN];
        let fallback_domain;
        let domain: &str = match build_lowercase_domain(query_name, &mut stack_buf) {
            Some(stack_domain) => stack_domain,
            None => {
                fallback_domain = domain_string_fallback(query_name);
                &fallback_domain
            }
        };

        if domain.is_empty() {
            return Err(AppError::NoRouteMatch(domain.to_string()));
        }

        // 1. 精确匹配：单次 HashMap 查找，block 优先于 forward
        if let Some(pair) = self.exact_rules.get(domain) {
            let (exact_rule, action) = match (pair.block.as_ref(), pair.forward.as_ref()) {
                (Some(r), _) => (r, RouteAction::Block),
                (_, Some(r)) => (r, RouteAction::Forward),
                (None, None) => return Err(AppError::NoRouteMatch(domain.to_string())),
            };
            return Ok(self.build_route_match(
                domain,
                action,
                rule_type_labels::EXACT,
                exact_rule.pattern.clone(),
                &exact_rule.value,
            ));
        }

        // 2. Block 规则：wildcard → regex → global wildcard
        if let Some(match_result) = self.try_wildcard_match(query_name, domain, RouteAction::Block)
        {
            return Ok(match_result);
        }

        if let Some(match_result) = self.try_regex_match(domain, RouteAction::Block) {
            return Ok(match_result);
        }

        if let Some(match_result) = self.try_global_wildcard_match(domain, RouteAction::Block) {
            return Ok(match_result);
        }

        // 3. Forward 规则：wildcard → regex → global wildcard
        if let Some(match_result) =
            self.try_wildcard_match(query_name, domain, RouteAction::Forward)
        {
            return Ok(match_result);
        }

        if let Some(match_result) = self.try_regex_match(domain, RouteAction::Forward) {
            return Ok(match_result);
        }

        if let Some(match_result) = self.try_global_wildcard_match(domain, RouteAction::Forward) {
            return Ok(match_result);
        }

        // 没有匹配的规则
        Err(AppError::NoRouteMatch(domain.to_string()))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_normalized_domain(query_name: &Name) -> String {
        let mut domain = query_name.to_ascii();
        if domain.ends_with('.') {
            domain.pop();
        }
        if domain.is_ascii() {
            domain.make_ascii_lowercase();
        } else {
            domain = domain.to_lowercase();
        }
        domain
    }

    fn fast_path_names() -> Vec<Name> {
        vec![
            Name::from_ascii("WWW.Example.COM.").unwrap(),
            Name::from_ascii("MiXeD.Case-Host.Org").unwrap(),
            Name::from_ascii("ads.example.com.").unwrap(),
            Name::from_ascii("A.B.C.D.E.F.G.H.I.J.Example.Invalid.").unwrap(),
            Name::from_ascii("_dmarc._domainkey.Example.COM.").unwrap(),
            Name::from_ascii("*.Example.COM.").unwrap(),
            Name::from_ascii("XN--ZS9H.Example.COM.").unwrap(),
            Name::from_ascii("localhost.").unwrap(),
            Name::from_ascii("LocalHost").unwrap(),
            Name::from_ascii("my-Tracking-Host.My-Example.Com").unwrap(),
            Name::root(),
            Name::from_labels((0..127).map(|_| b"a".to_vec())).unwrap(),
        ]
    }

    fn fallback_names() -> Vec<Name> {
        vec![
            Name::from_labels(vec![b"-Leading-Dash".to_vec(), b"Example".to_vec()]).unwrap(),
            Name::from_labels(vec![vec![0xE9u8, b'A'], b"Example".to_vec()]).unwrap(),
            Name::from_labels(vec![b"a*b".to_vec(), b"Example".to_vec()]).unwrap(),
            Name::from_labels(vec![b"a.b".to_vec(), b"Example".to_vec()]).unwrap(),
        ]
    }

    #[test]
    fn stack_domain_is_byte_identical_to_legacy_normalization() {
        for name in fast_path_names() {
            let mut buf = [0u8; MAX_DOMAIN_STACK_LEN];
            let stack_domain = build_lowercase_domain(&name, &mut buf)
                .unwrap_or_else(|| panic!("expected fast path for {name}"));
            assert_eq!(
                stack_domain,
                legacy_normalized_domain(&name),
                "name: {name}"
            );
        }
    }

    #[test]
    fn stack_domain_falls_back_for_escaped_or_oversized_names() {
        for name in fallback_names() {
            let mut buf = [0u8; MAX_DOMAIN_STACK_LEN];
            assert!(
                build_lowercase_domain(&name, &mut buf).is_none(),
                "expected fallback for {name}"
            );
        }
    }

    #[test]
    fn miss_path_matches_legacy_normalization_byte_for_byte() {
        let router = Router::new(Vec::new()).unwrap();
        for name in fast_path_names().into_iter().chain(fallback_names()) {
            let expected = legacy_normalized_domain(&name);
            match router.find_match(&name) {
                Err(AppError::NoRouteMatch(domain)) => {
                    assert_eq!(domain, expected, "name: {name}");
                }
                other => panic!("expected NoRouteMatch for {name}, got {other:?}"),
            }
        }
    }

    #[test]
    fn find_match_is_case_insensitive_for_mixed_case_queries() {
        let rules = vec![
            RouteRuleConfig {
                match_type: MatchType::Exact,
                patterns: vec!["Ads.Example.COM".to_string()],
                action: RouteAction::Block,
                target: None,
            },
            RouteRuleConfig {
                match_type: MatchType::Wildcard,
                patterns: vec!["*.Corp.Local".to_string()],
                action: RouteAction::Forward,
                target: Some("internal_doh".to_string()),
            },
            RouteRuleConfig {
                match_type: MatchType::Regex,
                patterns: vec!["^api\\.service\\.com$".to_string()],
                action: RouteAction::Block,
                target: None,
            },
        ];
        let router = Router::new(rules).unwrap();

        let matched = router
            .find_match(&Name::from_ascii("ADS.EXAMPLE.com.").unwrap())
            .unwrap();
        assert_eq!(matched.action, RouteAction::Block);
        assert_eq!(matched.rule_type, rule_type_labels::EXACT);
        assert_eq!(matched.domain, "ads.example.com");

        let matched = router
            .find_match(&Name::from_ascii("DeV.Corp.LOCAL.").unwrap())
            .unwrap();
        assert_eq!(matched.action, RouteAction::Forward);
        assert_eq!(matched.rule_type, rule_type_labels::WILDCARD);
        assert_eq!(matched.domain, "dev.corp.local");

        let matched = router
            .find_match(&Name::from_ascii("API.Service.Com.").unwrap())
            .unwrap();
        assert_eq!(matched.action, RouteAction::Block);
        assert_eq!(matched.rule_type, rule_type_labels::REGEX);
        assert_eq!(matched.domain, "api.service.com");

        let matched = router
            .find_match(&Name::from_ascii("Ads.Example.COM").unwrap())
            .unwrap();
        assert_eq!(matched.rule_type, rule_type_labels::EXACT);
        assert_eq!(matched.domain, "ads.example.com");
    }
}
