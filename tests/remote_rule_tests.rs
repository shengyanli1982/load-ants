use loadants::config::{
    AuthConfig, AuthType, HttpClientConfig, MatchType, RemoteRuleConfig, RemoteRuleType,
    RetryConfig, RouteAction, RouteRuleConfig, RuleFormat,
};
use loadants::error::AppError;
use loadants::r#const::remote_rule_limits;
use loadants::remote_rule::{
    load_and_merge_rules, ClashRuleParser, RemoteRuleLoader, RuleParser, V2RayRuleParser,
};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

#[tokio::test]
async fn test_v2ray_rule_parser() {
    // 测试V2Ray规则解析器
    let parser = V2RayRuleParser;

    // 测试空内容
    let empty_content = "";
    let result = parser.parse(empty_content);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 0);

    // 测试注释和空行
    let comment_content = "# 这是注释\n\n# 另一个注释";
    let result = parser.parse(comment_content);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 0);

    // 测试不同类型的规则
    let mixed_content = r#"
# 这是一个测试规则文件
full:example.com
regexp:.*\.example\.com$
another.domain.com
    "#;

    let result = parser.parse(mixed_content);
    assert!(result.is_ok());

    let rules = result.unwrap();
    assert_eq!(rules.len(), 3);

    // 检查规则类型和内容
    assert_eq!(rules[0], ("example.com".to_string(), MatchType::Exact));
    assert_eq!(
        rules[1],
        (".*\\.example\\.com$".to_string(), MatchType::Regex)
    );
    assert_eq!(
        rules[2],
        ("*.another.domain.com".to_string(), MatchType::Wildcard)
    );
}

#[tokio::test]
async fn test_remote_rule_loader() {
    // 启动mock服务器
    let mock_server = MockServer::start().await;

    // 创建一个简单的V2Ray规则文件响应
    let rule_content = r#"
# 测试规则
full:example.com
full:test.example.com
regexp:.*\.example\.net$
sub.domain.org
    "#;

    // 设置mock响应
    Mock::given(method("GET"))
        .and(path("/rules.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string(rule_content))
        .mount(&mock_server)
        .await;

    // 创建远程规则配置
    let config = RemoteRuleConfig {
        r#type: RemoteRuleType::Url,
        url: format!("{}/rules.txt", mock_server.uri()),
        format: RuleFormat::V2ray,
        action: RouteAction::Block,
        target: None,
        auth: None,
        retry: Some(RetryConfig {
            attempts: 3,
            delay: 1,
        }),
        proxy: None,
        max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
    };

    // 创建HTTP客户端配置
    let http_config = HttpClientConfig {
        connect_timeout: 5,
        request_timeout: 10,
        idle_timeout: Some(60),
        keepalive: Some(30),
        agent: Some("Test-Agent".to_string()),
    };

    // 创建远程规则加载器
    let loader = RemoteRuleLoader::new(config, http_config).unwrap();

    // 加载规则
    let rules = loader.load().await;
    assert!(rules.is_ok());

    let route_rules = rules.unwrap();
    assert_eq!(route_rules.len(), 3); // 应该有3种不同类型的规则（精确、通配符、正则）

    // 检查规则内容
    let mut has_exact = false;
    let mut has_wildcard = false;
    let mut has_regex = false;

    for rule in &route_rules {
        match rule.match_type {
            MatchType::Exact => {
                has_exact = true;
                assert_eq!(rule.patterns.len(), 2); // 两个精确匹配规则
                assert!(rule.patterns.contains(&"example.com".to_string()));
                assert!(rule.patterns.contains(&"test.example.com".to_string()));
            }
            MatchType::Wildcard => {
                has_wildcard = true;
                assert_eq!(rule.patterns.len(), 1); // 一个通配符规则
                assert!(rule.patterns.contains(&"*.sub.domain.org".to_string()));
            }
            MatchType::Regex => {
                has_regex = true;
                assert_eq!(rule.patterns.len(), 1); // 一个正则表达式规则
                assert!(rule.patterns.contains(&".*\\.example\\.net$".to_string()));
            }
        }
        // 检查动作和目标
        assert_eq!(rule.action, RouteAction::Block);
        assert_eq!(rule.target, None);
    }

    assert!(has_exact);
    assert!(has_wildcard);
    assert!(has_regex);
}

#[tokio::test]
async fn test_remote_rule_with_auth() {
    // 启动mock服务器
    let mock_server = MockServer::start().await;

    // 创建一个简单的规则文件响应
    let rule_content = "full:auth-example.com";

    // 设置mock响应，验证Bearer认证头
    Mock::given(method("GET"))
        .and(path("/auth-rules.txt"))
        .and(wiremock::matchers::header(
            "Authorization",
            "Bearer test-token",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(rule_content))
        .mount(&mock_server)
        .await;

    // 创建远程规则配置，带认证信息
    let config = RemoteRuleConfig {
        r#type: RemoteRuleType::Url,
        url: format!("{}/auth-rules.txt", mock_server.uri()),
        format: RuleFormat::V2ray,
        action: RouteAction::Forward,
        target: Some("test-target".to_string()),
        auth: Some(AuthConfig {
            r#type: AuthType::Bearer,
            username: None,
            password: None,
            token: Some("test-token".to_string()),
        }),
        retry: None,
        proxy: None,
        max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
    };

    // 创建HTTP客户端配置
    let http_config = HttpClientConfig::default();

    // 创建远程规则加载器
    let loader = RemoteRuleLoader::new(config, http_config).unwrap();

    // 加载规则
    let rules = loader.load().await;
    assert!(rules.is_ok());

    let route_rules = rules.unwrap();
    assert_eq!(route_rules.len(), 1); // 应该只有一种类型的规则（精确）

    // 检查规则内容
    assert_eq!(route_rules[0].match_type, MatchType::Exact);
    assert_eq!(route_rules[0].patterns.len(), 1);
    assert_eq!(route_rules[0].patterns[0], "auth-example.com");
    assert_eq!(route_rules[0].action, RouteAction::Forward);
    assert_eq!(route_rules[0].target, Some("test-target".to_string()));
}

#[tokio::test]
async fn test_load_and_merge_rules() {
    // 启动两个mock服务器，模拟不同的规则源
    let mock_server1 = MockServer::start().await;
    let mock_server2 = MockServer::start().await;

    // 第一个规则源（阻止规则）
    let block_rules = "full:blocked.example.com";
    Mock::given(method("GET"))
        .and(path("/block-rules.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string(block_rules))
        .mount(&mock_server1)
        .await;

    // 第二个规则源（转发规则）
    let forward_rules = "full:forward.example.com";
    Mock::given(method("GET"))
        .and(path("/forward-rules.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string(forward_rules))
        .mount(&mock_server2)
        .await;

    // 创建远程规则配置列表
    let remote_configs = vec![
        RemoteRuleConfig {
            r#type: RemoteRuleType::Url,
            url: format!("{}/block-rules.txt", mock_server1.uri()),
            format: RuleFormat::V2ray,
            action: RouteAction::Block,
            target: None,
            auth: None,
            retry: None,
            proxy: None,
            max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
        },
        RemoteRuleConfig {
            r#type: RemoteRuleType::Url,
            url: format!("{}/forward-rules.txt", mock_server2.uri()),
            format: RuleFormat::V2ray,
            action: RouteAction::Forward,
            target: Some("test-target".to_string()),
            auth: None,
            retry: None,
            proxy: None,
            max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
        },
    ];

    // 创建静态规则
    let static_rules = vec![RouteRuleConfig {
        match_type: MatchType::Exact,
        patterns: vec!["static.example.com".to_string()],
        action: RouteAction::Block,
        target: None,
    }];

    // 创建HTTP客户端配置
    let http_config = HttpClientConfig::default();

    // 加载并合并规则
    let merged_rules = load_and_merge_rules(&remote_configs, &static_rules, &http_config).await;

    assert!(merged_rules.is_ok());

    let rules = merged_rules.unwrap();
    assert_eq!(rules.len(), 3); // 2个远程规则 + 1个静态规则

    // 验证规则内容和顺序（静态规则应该在最后）

    // 找到阻止规则
    let block_rule = rules.iter().find(|r| {
        r.match_type == MatchType::Exact
            && r.action == RouteAction::Block
            && r.patterns.contains(&"blocked.example.com".to_string())
    });
    assert!(block_rule.is_some());

    // 找到转发规则
    let forward_rule = rules.iter().find(|r| {
        r.match_type == MatchType::Exact
            && r.action == RouteAction::Forward
            && r.patterns.contains(&"forward.example.com".to_string())
    });
    assert!(forward_rule.is_some());
    assert_eq!(
        forward_rule.unwrap().target,
        Some("test-target".to_string())
    );

    // 找到静态规则
    let static_rule = rules.iter().find(|r| {
        r.match_type == MatchType::Exact && r.patterns.contains(&"static.example.com".to_string())
    });
    assert!(static_rule.is_some());

    // 验证静态规则顺序
    assert_eq!(rules.first().unwrap().patterns[0], "static.example.com");
    assert_eq!(rules.last().unwrap().patterns[0], "forward.example.com");
}

#[tokio::test]
async fn test_error_handling() {
    // 启动mock服务器
    let mock_server = MockServer::start().await;

    // 设置一个失败的响应
    Mock::given(method("GET"))
        .and(path("/not-found.txt"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    // 设置一个超大响应，超过最大大小限制
    let large_content = "full:example.com\n".repeat(1000); // 创建一个大文件
    Mock::given(method("GET"))
        .and(path("/large-file.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string(large_content))
        .mount(&mock_server)
        .await;

    // 创建HTTP客户端配置
    let http_config = HttpClientConfig::default();

    // 测试404错误
    let not_found_config = RemoteRuleConfig {
        r#type: RemoteRuleType::Url,
        url: format!("{}/not-found.txt", mock_server.uri()),
        format: RuleFormat::V2ray,
        action: RouteAction::Block,
        target: None,
        auth: None,
        retry: None,
        proxy: None,
        max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
    };

    let loader = RemoteRuleLoader::new(not_found_config, http_config.clone()).unwrap();
    let result = loader.load().await;
    assert!(result.is_err());

    // 测试文件大小限制
    let large_file_config = RemoteRuleConfig {
        r#type: RemoteRuleType::Url,
        url: format!("{}/large-file.txt", mock_server.uri()),
        format: RuleFormat::V2ray,
        action: RouteAction::Block,
        target: None,
        auth: None,
        retry: None,
        proxy: None,
        max_size: 100, // 设置一个很小的限制
    };

    let loader = RemoteRuleLoader::new(large_file_config, http_config).unwrap();
    let result = loader.load().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_clash_rule_parser_not_implemented() {
    // 直接测试ClashRuleParser
    let parser = ClashRuleParser;
    let result = parser.parse("some content");

    // 检查是否是NotImplemented错误
    match result {
        Err(AppError::NotImplemented(_)) => {
            // 预期的错误
        }
        _ => {
            panic!("Expected NotImplemented error, got: {:?}", result);
        }
    }
}
