#[cfg(test)]
mod tests {
    use hickory_proto::rr::Name;
    use loadants::config::{MatchType, RouteAction, RouteRuleConfig};
    use loadants::error::ConfigError;
    use loadants::router::{RoutedRule, Router, RuleMetadata};
    use std::str::FromStr;

    fn create_test_rules() -> Vec<RouteRuleConfig> {
        vec![
            RouteRuleConfig {
                match_type: MatchType::Exact,
                patterns: vec![
                    "ads.example.com".to_string(),
                    "special.corp.com".to_string(),
                ],
                action: RouteAction::Forward,
                target: Some("cloudflare_secure".to_string()),
            },
            RouteRuleConfig {
                match_type: MatchType::Wildcard,
                patterns: vec!["*.corp.local".to_string(), "*.corp.com".to_string()],
                action: RouteAction::Forward,
                target: Some("internal_doh".to_string()),
            },
            RouteRuleConfig {
                match_type: MatchType::Exact,
                patterns: vec!["special.corp.com".to_string()],
                action: RouteAction::Block,
                target: None,
            },
            RouteRuleConfig {
                match_type: MatchType::Regex,
                patterns: vec!["^(api|service)\\..+\\.com$".to_string()],
                action: RouteAction::Forward,
                target: Some("google_public".to_string()),
            },
            RouteRuleConfig {
                match_type: MatchType::Regex,
                patterns: vec!["^api\\.service\\.com$".to_string()],
                action: RouteAction::Block,
                target: None,
            },
            RouteRuleConfig {
                match_type: MatchType::Wildcard,
                patterns: vec!["*".to_string()],
                action: RouteAction::Forward,
                target: Some("google_public".to_string()),
            },
        ]
    }

    #[test]
    fn test_exact_match_block_priority() {
        let rules = create_test_rules();
        let router = Router::new(rules).expect("Failed to create router");

        let query_name = Name::from_str("special.corp.com.").expect("Invalid name");
        let result = router
            .find_match(&query_name)
            .expect("Match should succeed");

        assert_eq!(result.action, RouteAction::Block);
        assert_eq!(result.rule_type, "exact");
    }

    #[test]
    fn test_regex_match_block_priority() {
        let rules = create_test_rules();
        let router = Router::new(rules).expect("Failed to create router");

        let query_name = Name::from_str("api.service.com.").expect("Invalid name");
        let result = router
            .find_match(&query_name)
            .expect("Match should succeed");

        assert_eq!(result.action, RouteAction::Block);
        assert_eq!(result.rule_type, "regex");
    }

    #[test]
    fn test_wildcard_forward_match() {
        let rules = create_test_rules();
        let router = Router::new(rules).expect("Failed to create router");

        let query_name = Name::from_str("dev.corp.local.").expect("Invalid name");
        let result = router
            .find_match(&query_name)
            .expect("Match should succeed");

        assert_eq!(result.action, RouteAction::Forward);
        assert_eq!(result.rule_type, "wildcard");
        assert_eq!(result.target, Some("internal_doh".to_string()));
    }

    #[test]
    fn test_exact_forward_match() {
        let rules = create_test_rules();
        let router = Router::new(rules).expect("Failed to create router");

        let query_name = Name::from_str("ads.example.com.").expect("Invalid name");
        let result = router
            .find_match(&query_name)
            .expect("Match should succeed");

        assert_eq!(result.action, RouteAction::Forward);
        assert_eq!(result.rule_type, "exact");
        assert_eq!(result.target, Some("cloudflare_secure".to_string()));
    }

    #[test]
    fn test_global_wildcard_match() {
        let rules = create_test_rules();
        let router = Router::new(rules).expect("Failed to create router");

        let query_name = Name::from_str("random.domain.org.").expect("Invalid name");
        let result = router
            .find_match(&query_name)
            .expect("Match should succeed");

        assert_eq!(result.action, RouteAction::Forward);
        assert_eq!(result.rule_type, "wildcard");
        assert_eq!(result.target, Some("google_public".to_string()));
    }

    #[test]
    fn test_overlapping_patterns() {
        let rules = vec![
            RouteRuleConfig {
                match_type: MatchType::Exact,
                patterns: vec!["overlap.example.com".to_string()],
                action: RouteAction::Forward,
                target: Some("cloudflare_secure".to_string()),
            },
            RouteRuleConfig {
                match_type: MatchType::Wildcard,
                patterns: vec!["*.example.com".to_string()],
                action: RouteAction::Forward,
                target: Some("google_public".to_string()),
            },
            RouteRuleConfig {
                match_type: MatchType::Exact,
                patterns: vec!["overlap.example.com".to_string()],
                action: RouteAction::Block,
                target: None,
            },
        ];

        let router = Router::new(rules).expect("Failed to create router");

        let query_name = Name::from_str("overlap.example.com.").expect("Invalid name");
        let result = router
            .find_match(&query_name)
            .expect("Match should succeed");

        assert_eq!(result.action, RouteAction::Block);
        assert_eq!(result.rule_type, "exact");
    }

    #[test]
    fn test_match_preserves_remote_rule_source_metadata() {
        let router = Router::new_with_metadata(vec![
            RoutedRule::from_static(RouteRuleConfig {
                match_type: MatchType::Exact,
                patterns: vec!["static-source.example".to_string()],
                action: RouteAction::Block,
                target: None,
            }),
            RoutedRule::new(
                RouteRuleConfig {
                    match_type: MatchType::Exact,
                    patterns: vec!["remote-source.example".to_string()],
                    action: RouteAction::Forward,
                    target: Some("remote-upstream".to_string()),
                },
                RuleMetadata::remote("https://example.com/router-remote.txt"),
            ),
        ])
        .expect("router with metadata should build");

        let static_match = router
            .find_match(&Name::from_str("static-source.example.").unwrap())
            .expect("static rule should match");
        assert_eq!(static_match.rule_source, "static");
        assert_eq!(static_match.source_id, None);

        let remote_match = router
            .find_match(&Name::from_str("remote-source.example.").unwrap())
            .expect("remote rule should match");
        assert_eq!(remote_match.rule_source, "remote");
        assert_eq!(
            remote_match.source_id,
            Some("https://example.com/router-remote.txt".to_string())
        );
        assert_eq!(remote_match.target, Some("remote-upstream".to_string()));
    }

    #[test]
    fn test_duplicate_exact_pattern_is_rejected() {
        let rules = vec![
            RouteRuleConfig {
                match_type: MatchType::Exact,
                patterns: vec!["dup.example.com".to_string()],
                action: RouteAction::Forward,
                target: Some("google_public".to_string()),
            },
            RouteRuleConfig {
                match_type: MatchType::Exact,
                patterns: vec!["dup.example.com".to_string()],
                action: RouteAction::Forward,
                target: Some("internal_doh".to_string()),
            },
        ];

        let error = match Router::new(rules) {
            Ok(_) => panic!("duplicate exact pattern should fail"),
            Err(error) => error,
        };
        match error {
            ConfigError::RuleConflict(message) => {
                assert!(message.contains("exact"));
                assert!(message.contains("dup.example.com"));
            }
            other => panic!("expected RuleConflict, got {other:?}"),
        }
    }

    #[test]
    fn test_duplicate_wildcard_pattern_is_rejected() {
        let rules = vec![
            RouteRuleConfig {
                match_type: MatchType::Wildcard,
                patterns: vec!["*.corp.example".to_string()],
                action: RouteAction::Forward,
                target: Some("google_public".to_string()),
            },
            RouteRuleConfig {
                match_type: MatchType::Wildcard,
                patterns: vec!["*.corp.example".to_string()],
                action: RouteAction::Forward,
                target: Some("internal_doh".to_string()),
            },
        ];

        let error = match Router::new(rules) {
            Ok(_) => panic!("duplicate wildcard pattern should fail"),
            Err(error) => error,
        };
        match error {
            ConfigError::RuleConflict(message) => {
                assert!(message.contains("wildcard"));
                assert!(message.contains("*.corp.example"));
            }
            other => panic!("expected RuleConflict, got {other:?}"),
        }
    }

    #[test]
    fn test_duplicate_global_wildcard_is_rejected() {
        let rules = vec![
            RouteRuleConfig {
                match_type: MatchType::Wildcard,
                patterns: vec!["*".to_string()],
                action: RouteAction::Forward,
                target: Some("google_public".to_string()),
            },
            RouteRuleConfig {
                match_type: MatchType::Wildcard,
                patterns: vec!["*".to_string()],
                action: RouteAction::Forward,
                target: Some("internal_doh".to_string()),
            },
        ];

        let error = match Router::new(rules) {
            Ok(_) => panic!("duplicate global wildcard should fail"),
            Err(error) => error,
        };
        match error {
            ConfigError::RuleConflict(message) => {
                assert!(message.contains("global wildcard"));
                assert!(message.contains("*"));
            }
            other => panic!("expected RuleConflict, got {other:?}"),
        }
    }

    #[test]
    fn test_duplicate_regex_pattern_is_rejected() {
        let rules = vec![
            RouteRuleConfig {
                match_type: MatchType::Regex,
                patterns: vec!["^dup\\.example\\.com$".to_string()],
                action: RouteAction::Forward,
                target: Some("google_public".to_string()),
            },
            RouteRuleConfig {
                match_type: MatchType::Regex,
                patterns: vec!["^dup\\.example\\.com$".to_string()],
                action: RouteAction::Forward,
                target: Some("internal_doh".to_string()),
            },
        ];

        let error = match Router::new(rules) {
            Ok(_) => panic!("duplicate regex pattern should fail"),
            Err(error) => error,
        };

        match error {
            ConfigError::RuleConflict(message) => {
                assert!(message.contains("regex"));
                assert!(message.contains("^dup\\.example\\.com$"));
            }
            other => panic!("expected RuleConflict, got {other:?}"),
        }
    }

    #[test]
    fn test_duplicate_exact_pattern_across_sources_is_rejected() {
        let error = match Router::new_with_metadata(vec![
            RoutedRule::from_static(RouteRuleConfig {
                match_type: MatchType::Exact,
                patterns: vec!["cross-source.example".to_string()],
                action: RouteAction::Block,
                target: None,
            }),
            RoutedRule::new(
                RouteRuleConfig {
                    match_type: MatchType::Exact,
                    patterns: vec!["cross-source.example".to_string()],
                    action: RouteAction::Block,
                    target: None,
                },
                RuleMetadata::remote("https://example.com/remote-rules.txt"),
            ),
        ]) {
            Ok(_) => panic!("cross-source duplicate exact pattern should fail"),
            Err(error) => error,
        };

        match error {
            ConfigError::RuleConflict(message) => {
                assert!(message.contains("cross-source.example"));
                assert!(message.contains("https://example.com/remote-rules.txt"));
            }
            other => panic!("expected RuleConflict, got {other:?}"),
        }
    }

    #[test]
    fn test_duplicate_regex_pattern_across_sources_is_rejected() {
        let error = match Router::new_with_metadata(vec![
            RoutedRule::from_static(RouteRuleConfig {
                match_type: MatchType::Regex,
                patterns: vec!["^cross-source\\.example$".to_string()],
                action: RouteAction::Block,
                target: None,
            }),
            RoutedRule::new(
                RouteRuleConfig {
                    match_type: MatchType::Regex,
                    patterns: vec!["^cross-source\\.example$".to_string()],
                    action: RouteAction::Block,
                    target: None,
                },
                RuleMetadata::remote("https://example.com/remote-regex-rules.txt"),
            ),
        ]) {
            Ok(_) => panic!("cross-source duplicate regex pattern should fail"),
            Err(error) => error,
        };

        match error {
            ConfigError::RuleConflict(message) => {
                assert!(message.contains("^cross-source\\.example$"));
                assert!(message.contains("https://example.com/remote-regex-rules.txt"));
            }
            other => panic!("expected RuleConflict, got {other:?}"),
        }
    }

    #[test]
    fn test_distinct_regex_patterns_remain_allowed() {
        let rules = vec![
            RouteRuleConfig {
                match_type: MatchType::Regex,
                patterns: vec!["^(api|service)\\.example\\.com$".to_string()],
                action: RouteAction::Forward,
                target: Some("google_public".to_string()),
            },
            RouteRuleConfig {
                match_type: MatchType::Regex,
                patterns: vec!["^api\\..+\\.com$".to_string()],
                action: RouteAction::Forward,
                target: Some("google_public".to_string()),
            },
        ];

        let router = Router::new(rules).expect("distinct regex patterns should remain allowed");
        let matched = router
            .find_match(&Name::from_str("api.example.com.").unwrap())
            .expect("regex rule should still match");

        assert_eq!(matched.action, RouteAction::Forward);
        assert_eq!(matched.rule_type, "regex");
    }

    fn build_regex_only_router(pattern: &str, action: RouteAction) -> Router {
        let rules = vec![RouteRuleConfig {
            match_type: MatchType::Regex,
            patterns: vec![pattern.to_string()],
            action,
            target: match action {
                RouteAction::Forward => Some("google_public".to_string()),
                RouteAction::Block => None,
            },
        }];
        Router::new(rules).expect("regex-only router should build")
    }

    #[test]
    fn test_regex_match_keyword_is_substring_of_hyphenated_label() {
        let router = build_regex_only_router(".*my-tracking\\.com$", RouteAction::Block);

        let query_name = Name::from_str("x.my-tracking.com.").expect("Invalid name");
        let matched = router
            .find_match(&query_name)
            .expect("regex rule must match when keyword is a substring of a hyphenated label");

        assert_eq!(matched.action, RouteAction::Block);
        assert_eq!(matched.rule_type, "regex");
    }

    #[test]
    fn test_regex_forward_match_keyword_is_substring_of_long_label() {
        let router = build_regex_only_router(".*admanager\\.com$", RouteAction::Forward);

        let query_name = Name::from_str("googleadmanager.com.").expect("Invalid name");
        let matched = router
            .find_match(&query_name)
            .expect("regex rule must match when keyword is a substring of a longer label");

        assert_eq!(matched.action, RouteAction::Forward);
        assert_eq!(matched.rule_type, "regex");
        assert_eq!(matched.target, Some("google_public".to_string()));
    }
}
