use hickory_proto::rr::Name;
use loadants::config::{MatchType, RouteAction, RouteRuleConfig};
use loadants::router::Router;
use std::str::FromStr;

fn query(name: &str) -> Name {
    Name::from_str(&format!("{}.", name)).expect("Invalid name")
}

fn rules_with_conflicts() -> Vec<RouteRuleConfig> {
    vec![
        RouteRuleConfig {
            match_type: MatchType::Exact,
            patterns: vec!["domain.example.com".to_string()],
            action: RouteAction::Block,
            target: None,
        },
        RouteRuleConfig {
            match_type: MatchType::Exact,
            patterns: vec!["domain.example.com".to_string()],
            action: RouteAction::Forward,
            target: Some("upstream".to_string()),
        },
        RouteRuleConfig {
            match_type: MatchType::Wildcard,
            patterns: vec!["*.wildcard.com".to_string()],
            action: RouteAction::Block,
            target: None,
        },
        RouteRuleConfig {
            match_type: MatchType::Wildcard,
            patterns: vec!["*.wildcard.com".to_string()],
            action: RouteAction::Forward,
            target: Some("upstream".to_string()),
        },
        RouteRuleConfig {
            match_type: MatchType::Regex,
            patterns: vec!["^regex\\.match\\.com$".to_string()],
            action: RouteAction::Block,
            target: None,
        },
        RouteRuleConfig {
            match_type: MatchType::Regex,
            patterns: vec!["^regex\\.match\\.com$".to_string()],
            action: RouteAction::Forward,
            target: Some("upstream".to_string()),
        },
        RouteRuleConfig {
            match_type: MatchType::Wildcard,
            patterns: vec!["*".to_string()],
            action: RouteAction::Block,
            target: None,
        },
        RouteRuleConfig {
            match_type: MatchType::Wildcard,
            patterns: vec!["*".to_string()],
            action: RouteAction::Forward,
            target: Some("upstream".to_string()),
        },
    ]
}

#[test]
fn test_exact_block_overrides_forward_when_same_domain() {
    let router = Router::new(rules_with_conflicts()).unwrap();
    let result = router.find_match(&query("domain.example.com")).unwrap();
    assert_eq!(result.action, RouteAction::Block);
    assert_eq!(result.rule_type, "exact");
}

#[test]
fn test_wildcard_block_overrides_forward_when_same_domain() {
    let router = Router::new(rules_with_conflicts()).unwrap();
    let result = router.find_match(&query("sub.wildcard.com")).unwrap();
    assert_eq!(result.action, RouteAction::Block);
    assert_eq!(result.rule_type, "wildcard");
}

#[test]
fn test_regex_block_overrides_forward_when_same_pattern() {
    let router = Router::new(rules_with_conflicts()).unwrap();
    let result = router.find_match(&query("regex.match.com")).unwrap();
    assert_eq!(result.action, RouteAction::Block);
    assert_eq!(result.rule_type, "regex");
}

#[test]
fn test_global_wildcard_block_overrides_forward_when_no_specific_match() {
    let router = Router::new(rules_with_conflicts()).unwrap();
    let result = router.find_match(&query("any.other.domain.com")).unwrap();
    assert_eq!(result.action, RouteAction::Block);
    assert_eq!(result.rule_type, "wildcard");
    assert_eq!(result.pattern, "*");
}

#[test]
fn test_block_priority_over_forward() {
    let rules = vec![
        RouteRuleConfig {
            match_type: MatchType::Exact,
            patterns: vec!["domain.com".to_string()],
            action: RouteAction::Block,
            target: None,
        },
        RouteRuleConfig {
            match_type: MatchType::Exact,
            patterns: vec!["domain.com".to_string()],
            action: RouteAction::Forward,
            target: Some("upstream".to_string()),
        },
    ];
    let router = Router::new(rules).unwrap();
    let result = router.find_match(&query("domain.com")).unwrap();
    assert_eq!(result.action, RouteAction::Block);
    assert_eq!(result.rule_type, "exact");
}

#[test]
fn test_exact_forward_wins_over_wildcard_forward() {
    let rules = vec![
        RouteRuleConfig {
            match_type: MatchType::Wildcard,
            patterns: vec!["*.example.com".to_string()],
            action: RouteAction::Forward,
            target: Some("wildcard_target".to_string()),
        },
        RouteRuleConfig {
            match_type: MatchType::Exact,
            patterns: vec!["exact.example.com".to_string()],
            action: RouteAction::Forward,
            target: Some("exact_target".to_string()),
        },
    ];
    let router = Router::new(rules).unwrap();
    let result = router.find_match(&query("exact.example.com")).unwrap();
    assert_eq!(result.rule_type, "exact");
    assert_eq!(result.action, RouteAction::Forward);
    assert_eq!(result.target.as_deref(), Some("exact_target"));
}

#[test]
fn test_wildcard_forward_wins_over_regex_forward() {
    let rules = vec![
        RouteRuleConfig {
            match_type: MatchType::Regex,
            patterns: vec![".*\\.test\\.com".to_string()],
            action: RouteAction::Forward,
            target: Some("regex_target".to_string()),
        },
        RouteRuleConfig {
            match_type: MatchType::Wildcard,
            patterns: vec!["*.test.com".to_string()],
            action: RouteAction::Forward,
            target: Some("wildcard_target".to_string()),
        },
    ];
    let router = Router::new(rules).unwrap();
    let result = router.find_match(&query("sub.test.com")).unwrap();
    assert_eq!(result.rule_type, "wildcard");
    assert_eq!(result.action, RouteAction::Forward);
    assert_eq!(result.target.as_deref(), Some("wildcard_target"));
}
