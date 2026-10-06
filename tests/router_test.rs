#[cfg(test)]
mod tests {
    use hickory_proto::rr::Name;
    use loadants::config::{MatchType, RouteAction, RouteRuleConfig};
    use loadants::error::ConfigError;
    use loadants::router::{
        build_lowercase_domain, RoutedRule, Router, RuleMetadata, MAX_DOMAIN_STACK_LEN,
    };
    use loadants::{rule_type_labels, AppError};
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

    // ========================================================================
    // 以下测试迁移自 src/router.rs 内嵌 #[cfg(test)] mod tests（测试语义零变化）。
    // ========================================================================

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
        assert_eq!(matched.domain.as_str(), "ads.example.com");

        let matched = router
            .find_match(&Name::from_ascii("DeV.Corp.LOCAL.").unwrap())
            .unwrap();
        assert_eq!(matched.action, RouteAction::Forward);
        assert_eq!(matched.rule_type, rule_type_labels::WILDCARD);
        assert_eq!(matched.domain.as_str(), "dev.corp.local");

        let matched = router
            .find_match(&Name::from_ascii("API.Service.Com.").unwrap())
            .unwrap();
        assert_eq!(matched.action, RouteAction::Block);
        assert_eq!(matched.rule_type, rule_type_labels::REGEX);
        assert_eq!(matched.domain.as_str(), "api.service.com");

        let matched = router
            .find_match(&Name::from_ascii("Ads.Example.COM").unwrap())
            .unwrap();
        assert_eq!(matched.rule_type, rule_type_labels::EXACT);
        assert_eq!(matched.domain.as_str(), "ads.example.com");
    }

    /// 模拟 `RemoteRuleLoader::load` 的分组行为：将 V2Ray 解析输出按 MatchType
    /// 聚合为 RouteRuleConfig 列表（同一来源共享 action/target）。
    fn group_v2ray_rules(
        parsed: Vec<(String, MatchType)>,
        action: RouteAction,
        target: Option<&str>,
    ) -> Vec<RouteRuleConfig> {
        let mut groups: Vec<RouteRuleConfig> = Vec::new();
        for (pattern, match_type) in parsed {
            if let Some(group) = groups.iter_mut().find(|g| g.match_type == match_type) {
                group.patterns.push(pattern);
            } else {
                groups.push(RouteRuleConfig {
                    match_type,
                    patterns: vec![pattern],
                    action,
                    target: target.map(String::from),
                });
            }
        }
        groups
    }

    /// D-01 验收标准①（reject-list 场景）：裸域名规则 `example.com` 对 apex 查询
    /// `example.com.` 与子域查询 `sub.example.com.` 都必须命中且 action 一致。
    #[test]
    fn v2ray_bare_domain_rules_block_apex_and_subdomains() {
        use loadants::remote_rule::{RuleParser, V2RayRuleParser};

        let parsed = V2RayRuleParser.parse("example.com").unwrap();
        let router = Router::new(group_v2ray_rules(parsed, RouteAction::Block, None)).unwrap();

        let apex = router
            .find_match(&Name::from_ascii("example.com.").unwrap())
            .expect("apex query must be matched by bare-domain rule");
        assert_eq!(apex.action, RouteAction::Block);
        assert_eq!(apex.domain.as_str(), "example.com");

        let sub = router
            .find_match(&Name::from_ascii("sub.example.com.").unwrap())
            .expect("subdomain query must be matched by bare-domain rule");
        assert_eq!(sub.action, RouteAction::Block);
        assert_eq!(sub.domain.as_str(), "sub.example.com");
    }

    /// D-01 验收标准①（proxy-list 场景）：apex 不得落到默认路由，
    /// 且 apex 与子域命中后的 action/target 必须一致。
    #[test]
    fn v2ray_bare_domain_rules_forward_apex_and_subdomain_to_same_target() {
        use loadants::remote_rule::{RuleParser, V2RayRuleParser};

        let parsed = V2RayRuleParser.parse("proxy-target.example.org").unwrap();
        let router = Router::new(group_v2ray_rules(
            parsed,
            RouteAction::Forward,
            Some("proxy-group"),
        ))
        .unwrap();

        let apex = router
            .find_match(&Name::from_ascii("proxy-target.example.org.").unwrap())
            .expect("apex query must be forwarded, not fall through to default route");
        let sub = router
            .find_match(&Name::from_ascii("www.proxy-target.example.org.").unwrap())
            .expect("subdomain query must be forwarded");

        assert_eq!(apex.action, RouteAction::Forward);
        assert_eq!(sub.action, RouteAction::Forward);
        assert_eq!(apex.target.as_deref(), Some("proxy-group"));
        assert_eq!(
            apex.target, sub.target,
            "apex and subdomain must share target"
        );
    }

    /// D-01 边界：裸域名规则不得误命中相似域名（exact 全等 + wildcard 按 label
    /// 边界匹配，`notexample.com` / `example.com.evil.net` 均不得命中）。
    #[test]
    fn v2ray_bare_domain_rules_do_not_match_similar_domains() {
        use loadants::remote_rule::{RuleParser, V2RayRuleParser};

        let parsed = V2RayRuleParser.parse("example.com").unwrap();
        let router = Router::new(group_v2ray_rules(parsed, RouteAction::Block, None)).unwrap();

        for similar in ["notexample.com.", "myexample.com.", "example.com.evil.net."] {
            let result = router.find_match(&Name::from_ascii(similar).unwrap());
            assert!(
                matches!(result, Err(AppError::NoRouteMatch(_))),
                "similar domain {similar} must not match bare-domain rule, got {result:?}"
            );
        }
    }
}
