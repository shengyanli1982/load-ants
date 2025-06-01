#[cfg(test)]
mod tests {
    use hickory_proto::rr::Name;
    use loadants::config::{MatchType, RouteAction, RouteRuleConfig};
    use loadants::router::Router;
    use std::str::FromStr;

    // 创建测试规则集
    fn create_test_rules() -> Vec<RouteRuleConfig> {
        vec![
            // 精确匹配的 forward 规则
            RouteRuleConfig {
                match_type: MatchType::Exact,
                patterns: vec![
                    "ads.example.com".to_string(),
                    "special.corp.com".to_string(),
                ],
                action: RouteAction::Forward,
                target: Some("cloudflare_secure".to_string()),
            },
            // 通配符 forward 规则
            RouteRuleConfig {
                match_type: MatchType::Wildcard,
                patterns: vec!["*.corp.local".to_string(), "*.corp.com".to_string()],
                action: RouteAction::Forward,
                target: Some("internal_doh".to_string()),
            },
            // 精确匹配的 block 规则 - 应覆盖上面的 forward 规则
            RouteRuleConfig {
                match_type: MatchType::Exact,
                patterns: vec!["special.corp.com".to_string()],
                action: RouteAction::Block,
                target: None,
            },
            // 正则匹配的 forward 规则
            RouteRuleConfig {
                match_type: MatchType::Regex,
                patterns: vec!["^(api|service)\\..+\\.com$".to_string()],
                action: RouteAction::Forward,
                target: Some("google_public".to_string()),
            },
            // 正则匹配的 block 规则 - 应覆盖上面的 forward 规则
            RouteRuleConfig {
                match_type: MatchType::Regex,
                patterns: vec!["^api\\.service\\.com$".to_string()],
                action: RouteAction::Block,
                target: None,
            },
            // 全局通配符 forward 规则（默认规则）
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
        // 准备测试数据
        let rules = create_test_rules();
        let router = Router::new(rules).expect("Failed to create router");

        // 测试精确匹配的 block 规则优先于 forward 规则
        let query_name = Name::from_str("special.corp.com.").expect("Invalid name");
        let result = router
            .find_match(&query_name)
            .expect("Match should succeed");

        assert_eq!(result.action, RouteAction::Block);
        assert_eq!(result.rule_type, "exact");
    }

    #[test]
    fn test_regex_match_block_priority() {
        // 准备测试数据
        let rules = create_test_rules();
        let router = Router::new(rules).expect("Failed to create router");

        // 测试正则匹配的 block 规则优先于 forward 规则
        let query_name = Name::from_str("api.service.com.").expect("Invalid name");
        let result = router
            .find_match(&query_name)
            .expect("Match should succeed");

        assert_eq!(result.action, RouteAction::Block);
        assert_eq!(result.rule_type, "regex");
    }

    #[test]
    fn test_wildcard_forward_match() {
        // 准备测试数据
        let rules = create_test_rules();
        let router = Router::new(rules).expect("Failed to create router");

        // 测试通配符 forward 规则匹配
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
        // 准备测试数据
        let rules = create_test_rules();
        let router = Router::new(rules).expect("Failed to create router");

        // 测试精确匹配的 forward 规则
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
        // 准备测试数据
        let rules = create_test_rules();
        let router = Router::new(rules).expect("Failed to create router");

        // 测试全局通配符规则匹配
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
        // 创建重叠规则
        let rules = vec![
            // 精确匹配规则 - forward
            RouteRuleConfig {
                match_type: MatchType::Exact,
                patterns: vec!["overlap.example.com".to_string()],
                action: RouteAction::Forward,
                target: Some("cloudflare_secure".to_string()),
            },
            // 通配符规则 - forward
            RouteRuleConfig {
                match_type: MatchType::Wildcard,
                patterns: vec!["*.example.com".to_string()],
                action: RouteAction::Forward,
                target: Some("google_public".to_string()),
            },
            // 精确匹配规则 - block (应该优先)
            RouteRuleConfig {
                match_type: MatchType::Exact,
                patterns: vec!["overlap.example.com".to_string()],
                action: RouteAction::Block,
                target: None,
            },
        ];

        let router = Router::new(rules).expect("Failed to create router");

        // 测试重叠模式下 block 规则优先
        let query_name = Name::from_str("overlap.example.com.").expect("Invalid name");
        let result = router
            .find_match(&query_name)
            .expect("Match should succeed");

        assert_eq!(result.action, RouteAction::Block);
        assert_eq!(result.rule_type, "exact");
    }
}
