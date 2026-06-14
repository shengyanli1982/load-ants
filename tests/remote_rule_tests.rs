use loadants::build_router;
use loadants::config::{
    AuthConfig, AuthType, Config, HttpClientConfig, MatchType, RemoteRuleConfig,
    RemoteRuleFailurePolicy, RemoteRuleSnapshotConfig, RemoteRuleType, RetryConfig, RouteAction,
    RouteRuleConfig, RuleFormat,
};
use loadants::error::AppError;
use loadants::r#const::remote_rule_limits;
use loadants::remote_rule::{
    evaluate_remote_rule_startup, load_and_merge_rules, ClashRuleParser, RemoteRuleLoader,
    RemoteRuleSnapshotStore, RuleParser, V2RayRuleParser,
};
use std::fs;
use tempfile::tempdir;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

fn snapshot_temp_path(
    store: &RemoteRuleSnapshotStore,
    source_url: &str,
    pid: u32,
) -> std::path::PathBuf {
    let snapshot_path = store.snapshot_path(source_url);
    let stem = snapshot_path
        .file_stem()
        .expect("snapshot path should contain stem")
        .to_string_lossy();
    snapshot_path.with_file_name(format!("{stem}.{pid}.tmp"))
}

#[tokio::test]
async fn test_v2ray_rule_parser() {
    let parser = V2RayRuleParser;

    let empty_content = "";
    let result = parser.parse(empty_content);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 0);

    let comment_content = "# comment\n\n# another comment";
    let result = parser.parse(comment_content);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 0);

    let mixed_content = r#"
# comment
full:example.com
regexp:.*\.example\.com$
another.domain.com
    "#;

    let result = parser.parse(mixed_content);
    assert!(result.is_ok());

    let rules = result.unwrap();
    assert_eq!(rules.len(), 3);
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
    let mock_server = MockServer::start().await;

    let rule_content = r#"
# test rules
full:example.com
full:test.example.com
regexp:.*\.example\.net$
sub.domain.org
    "#;

    Mock::given(method("GET"))
        .and(path("/rules.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string(rule_content))
        .mount(&mock_server)
        .await;

    let config = RemoteRuleConfig {
        r#type: RemoteRuleType::Url,
        url: format!("{}/rules.txt", mock_server.uri()),
        format: RuleFormat::V2ray,
        failure_policy: RemoteRuleFailurePolicy::Strict,
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

    let http_config = HttpClientConfig {
        connect_timeout: 5,
        request_timeout: 10,
        idle_timeout: Some(60),
        keepalive: Some(30),
        agent: Some("Test-Agent".to_string()),
    };

    let loader = RemoteRuleLoader::new(config, http_config).unwrap();
    let rules = loader.load().await;
    assert!(rules.is_ok());

    let route_rules = rules.unwrap();
    assert_eq!(route_rules.len(), 3);

    let mut has_exact = false;
    let mut has_wildcard = false;
    let mut has_regex = false;

    for rule in &route_rules {
        match rule.match_type {
            MatchType::Exact => {
                has_exact = true;
                assert_eq!(rule.patterns.len(), 2);
                assert!(rule.patterns.contains(&"example.com".to_string()));
                assert!(rule.patterns.contains(&"test.example.com".to_string()));
            }
            MatchType::Wildcard => {
                has_wildcard = true;
                assert_eq!(rule.patterns.len(), 1);
                assert!(rule.patterns.contains(&"*.sub.domain.org".to_string()));
            }
            MatchType::Regex => {
                has_regex = true;
                assert_eq!(rule.patterns.len(), 1);
                assert!(rule.patterns.contains(&".*\\.example\\.net$".to_string()));
            }
        }

        assert_eq!(rule.action, RouteAction::Block);
        assert_eq!(rule.target, None);
    }

    assert!(has_exact);
    assert!(has_wildcard);
    assert!(has_regex);
}

#[tokio::test]
async fn test_remote_rule_with_auth() {
    let mock_server = MockServer::start().await;

    let rule_content = "full:auth-example.com";

    Mock::given(method("GET"))
        .and(path("/auth-rules.txt"))
        .and(wiremock::matchers::header(
            "Authorization",
            "Bearer test-token",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(rule_content))
        .mount(&mock_server)
        .await;

    let config = RemoteRuleConfig {
        r#type: RemoteRuleType::Url,
        url: format!("{}/auth-rules.txt", mock_server.uri()),
        format: RuleFormat::V2ray,
        failure_policy: RemoteRuleFailurePolicy::Strict,
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

    let loader = RemoteRuleLoader::new(config, HttpClientConfig::default()).unwrap();
    let rules = loader.load().await;
    assert!(rules.is_ok());

    let route_rules = rules.unwrap();
    assert_eq!(route_rules.len(), 1);
    assert_eq!(route_rules[0].match_type, MatchType::Exact);
    assert_eq!(route_rules[0].patterns.len(), 1);
    assert_eq!(route_rules[0].patterns[0], "auth-example.com");
    assert_eq!(route_rules[0].action, RouteAction::Forward);
    assert_eq!(route_rules[0].target, Some("test-target".to_string()));
}

#[tokio::test]
async fn test_load_and_merge_rules() {
    let mock_server1 = MockServer::start().await;
    let mock_server2 = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/block-rules.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("full:blocked.example.com"))
        .mount(&mock_server1)
        .await;

    Mock::given(method("GET"))
        .and(path("/forward-rules.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("full:forward.example.com"))
        .mount(&mock_server2)
        .await;

    let remote_configs = vec![
        RemoteRuleConfig {
            r#type: RemoteRuleType::Url,
            url: format!("{}/block-rules.txt", mock_server1.uri()),
            format: RuleFormat::V2ray,
            failure_policy: RemoteRuleFailurePolicy::Strict,
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
            failure_policy: RemoteRuleFailurePolicy::Strict,
            action: RouteAction::Forward,
            target: Some("test-target".to_string()),
            auth: None,
            retry: None,
            proxy: None,
            max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
        },
    ];

    let static_rules = vec![RouteRuleConfig {
        match_type: MatchType::Exact,
        patterns: vec!["static.example.com".to_string()],
        action: RouteAction::Block,
        target: None,
    }];

    let merged_rules = load_and_merge_rules(
        &remote_configs,
        &static_rules,
        &HttpClientConfig::default(),
        &RemoteRuleSnapshotConfig {
            enabled: false,
            path: ".unused".to_string(),
        },
    )
    .await;
    assert!(merged_rules.is_ok());

    let rules = merged_rules.unwrap();
    assert_eq!(rules.merged_rules.len(), 3);
    assert!(!rules.partial_failure());
    assert_eq!(rules.successful_sources.len(), 2);
    assert!(rules.failed_sources.is_empty());

    let block_rule = rules.merged_rules.iter().find(|r| {
        r.rule.match_type == MatchType::Exact
            && r.rule.action == RouteAction::Block
            && r.rule.patterns.contains(&"blocked.example.com".to_string())
    });
    assert!(block_rule.is_some());

    let forward_rule = rules.merged_rules.iter().find(|r| {
        r.rule.match_type == MatchType::Exact
            && r.rule.action == RouteAction::Forward
            && r.rule.patterns.contains(&"forward.example.com".to_string())
    });
    assert!(forward_rule.is_some());
    assert_eq!(
        forward_rule.unwrap().rule.target,
        Some("test-target".to_string())
    );

    let static_rule = rules.merged_rules.iter().find(|r| {
        r.rule.match_type == MatchType::Exact
            && r.rule.patterns.contains(&"static.example.com".to_string())
    });
    assert!(static_rule.is_some());

    assert_eq!(
        rules.merged_rules.first().unwrap().rule.patterns[0],
        "static.example.com"
    );
    assert_eq!(
        rules.merged_rules.last().unwrap().rule.patterns[0],
        "forward.example.com"
    );
}

#[tokio::test]
async fn test_error_handling() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/not-found.txt"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    let large_content = "full:example.com\n".repeat(1000);
    Mock::given(method("GET"))
        .and(path("/large-file.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string(large_content))
        .mount(&mock_server)
        .await;

    let not_found_config = RemoteRuleConfig {
        r#type: RemoteRuleType::Url,
        url: format!("{}/not-found.txt", mock_server.uri()),
        format: RuleFormat::V2ray,
        failure_policy: RemoteRuleFailurePolicy::Strict,
        action: RouteAction::Block,
        target: None,
        auth: None,
        retry: None,
        proxy: None,
        max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
    };

    let loader = RemoteRuleLoader::new(not_found_config, HttpClientConfig::default()).unwrap();
    let result = loader.load().await;
    assert!(result.is_err());

    let large_file_config = RemoteRuleConfig {
        r#type: RemoteRuleType::Url,
        url: format!("{}/large-file.txt", mock_server.uri()),
        format: RuleFormat::V2ray,
        failure_policy: RemoteRuleFailurePolicy::Strict,
        action: RouteAction::Block,
        target: None,
        auth: None,
        retry: None,
        proxy: None,
        max_size: 100,
    };

    let loader = RemoteRuleLoader::new(large_file_config, HttpClientConfig::default()).unwrap();
    let result = loader.load().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_clash_rule_parser_not_implemented() {
    let parser = ClashRuleParser;
    let result = parser.parse("some content");

    match result {
        Err(AppError::NotImplemented(_)) => {}
        _ => {
            panic!("Expected NotImplemented error, got: {:?}", result);
        }
    }
}

#[tokio::test]
async fn test_load_and_merge_rules_reports_lenient_failures() {
    let success_server = MockServer::start().await;
    let failure_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/ok.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("full:remote-ok.example"))
        .mount(&success_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/missing.txt"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&failure_server)
        .await;

    let remote_configs = vec![
        RemoteRuleConfig {
            r#type: RemoteRuleType::Url,
            url: format!("{}/ok.txt", success_server.uri()),
            format: RuleFormat::V2ray,
            failure_policy: RemoteRuleFailurePolicy::Strict,
            action: RouteAction::Block,
            target: None,
            auth: None,
            retry: None,
            proxy: None,
            max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
        },
        RemoteRuleConfig {
            r#type: RemoteRuleType::Url,
            url: format!("{}/missing.txt", failure_server.uri()),
            format: RuleFormat::V2ray,
            failure_policy: RemoteRuleFailurePolicy::Lenient,
            action: RouteAction::Block,
            target: None,
            auth: None,
            retry: None,
            proxy: None,
            max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
        },
    ];

    let static_rules = vec![RouteRuleConfig {
        match_type: MatchType::Exact,
        patterns: vec!["static.example.com".to_string()],
        action: RouteAction::Block,
        target: None,
    }];

    let summary = load_and_merge_rules(
        &remote_configs,
        &static_rules,
        &HttpClientConfig::default(),
        &RemoteRuleSnapshotConfig {
            enabled: false,
            path: ".unused".to_string(),
        },
    )
    .await
    .expect("remote rule load summary should be returned");

    assert!(summary.partial_failure());
    assert_eq!(summary.successful_sources.len(), 1);
    assert_eq!(summary.failed_sources.len(), 1);
    assert_eq!(
        summary.failed_sources[0].failure_policy,
        RemoteRuleFailurePolicy::Lenient
    );
    assert_eq!(summary.failed_sources[0].fallback_hint, None);
    assert_eq!(summary.merged_rules.len(), 2);

    let startup = evaluate_remote_rule_startup(summary)
        .expect("lenient failures should allow degraded startup");
    assert!(startup.partial_failure());
    assert_eq!(startup.failed_sources.len(), 1);
}

#[tokio::test]
async fn test_evaluate_remote_rule_startup_rejects_strict_failures() {
    let success_server = MockServer::start().await;
    let failure_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/ok.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("full:remote-ok.example"))
        .mount(&success_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/missing.txt"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&failure_server)
        .await;

    let remote_configs = vec![
        RemoteRuleConfig {
            r#type: RemoteRuleType::Url,
            url: format!("{}/ok.txt", success_server.uri()),
            format: RuleFormat::V2ray,
            failure_policy: RemoteRuleFailurePolicy::Lenient,
            action: RouteAction::Block,
            target: None,
            auth: None,
            retry: None,
            proxy: None,
            max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
        },
        RemoteRuleConfig {
            r#type: RemoteRuleType::Url,
            url: format!("{}/missing.txt", failure_server.uri()),
            format: RuleFormat::V2ray,
            failure_policy: RemoteRuleFailurePolicy::Strict,
            action: RouteAction::Forward,
            target: Some("test-target".to_string()),
            auth: None,
            retry: None,
            proxy: None,
            max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
        },
    ];

    let static_rules = vec![RouteRuleConfig {
        match_type: MatchType::Exact,
        patterns: vec!["static.example.com".to_string()],
        action: RouteAction::Block,
        target: None,
    }];

    let summary = load_and_merge_rules(
        &remote_configs,
        &static_rules,
        &HttpClientConfig::default(),
        &RemoteRuleSnapshotConfig {
            enabled: false,
            path: ".unused".to_string(),
        },
    )
    .await
    .expect("remote rule load summary should be returned");

    let err = evaluate_remote_rule_startup(summary)
        .expect_err("strict failures must reject startup before router creation");
    assert!(
        matches!(err, AppError::Upstream(ref message) if message.contains("strict") && message.contains("/missing.txt")),
        "unexpected strict failure error: {err:?}"
    );
}

#[tokio::test]
async fn test_load_and_merge_rules_writes_snapshot_on_success() {
    let directory = tempdir().expect("snapshot directory should be created");
    let snapshot_config = RemoteRuleSnapshotConfig {
        enabled: true,
        path: directory.path().to_string_lossy().to_string(),
    };
    let snapshot_store = RemoteRuleSnapshotStore::new(&snapshot_config);
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/rules.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("full:snapshot-write.example"))
        .mount(&mock_server)
        .await;

    let remote_configs = vec![RemoteRuleConfig {
        r#type: RemoteRuleType::Url,
        url: format!("{}/rules.txt", mock_server.uri()),
        format: RuleFormat::V2ray,
        failure_policy: RemoteRuleFailurePolicy::Strict,
        action: RouteAction::Block,
        target: None,
        auth: None,
        retry: None,
        proxy: None,
        max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
    }];

    let summary = load_and_merge_rules(
        &remote_configs,
        &[],
        &HttpClientConfig::default(),
        &snapshot_config,
    )
    .await
    .expect("remote rule load summary should be returned");

    assert!(summary.failed_sources.is_empty());
    let snapshot = snapshot_store
        .load(&remote_configs[0].url)
        .expect("snapshot should be readable")
        .expect("snapshot should exist");
    assert_eq!(snapshot.rules.len(), 1);
    assert_eq!(
        snapshot.rules[0].patterns,
        vec!["snapshot-write.example".to_string()]
    );
}

#[tokio::test]
async fn test_load_and_merge_rules_prunes_stale_snapshot_files() {
    let directory = tempdir().expect("snapshot directory should be created");
    let snapshot_config = RemoteRuleSnapshotConfig {
        enabled: true,
        path: directory.path().to_string_lossy().to_string(),
    };
    let snapshot_store = RemoteRuleSnapshotStore::new(&snapshot_config);
    let active_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/rules.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("full:active.example"))
        .mount(&active_server)
        .await;

    let stale_url = "https://stale.example.com/rules.txt".to_string();
    snapshot_store
        .save(
            &stale_url,
            &[RouteRuleConfig {
                match_type: MatchType::Exact,
                patterns: vec!["stale.example".to_string()],
                action: RouteAction::Block,
                target: None,
            }],
        )
        .expect("stale snapshot should be seeded");
    let stale_snapshot_path = snapshot_store.snapshot_path(&stale_url);
    let stale_tmp_path = snapshot_temp_path(&snapshot_store, &stale_url, 2001);
    fs::write(&stale_tmp_path, b"stale tmp").expect("stale tmp should be created");

    let remote_configs = vec![RemoteRuleConfig {
        r#type: RemoteRuleType::Url,
        url: format!("{}/rules.txt", active_server.uri()),
        format: RuleFormat::V2ray,
        failure_policy: RemoteRuleFailurePolicy::Strict,
        action: RouteAction::Block,
        target: None,
        auth: None,
        retry: None,
        proxy: None,
        max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
    }];

    let summary = load_and_merge_rules(
        &remote_configs,
        &[],
        &HttpClientConfig::default(),
        &snapshot_config,
    )
    .await
    .expect("remote rule load summary should be returned");

    assert!(summary.failed_sources.is_empty());
    assert!(
        !stale_snapshot_path.exists(),
        "已移除来源的快照文件应被清理"
    );
    assert!(!stale_tmp_path.exists(), "已移除来源的临时文件应被清理");
    assert!(
        snapshot_store
            .snapshot_path(&remote_configs[0].url)
            .exists(),
        "当前活跃来源应写入新快照"
    );
}

#[tokio::test]
async fn test_load_and_merge_rules_uses_snapshot_fallback_for_strict_failures() {
    let directory = tempdir().expect("snapshot directory should be created");
    let snapshot_config = RemoteRuleSnapshotConfig {
        enabled: true,
        path: directory.path().to_string_lossy().to_string(),
    };
    let snapshot_store = RemoteRuleSnapshotStore::new(&snapshot_config);
    let failure_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/missing.txt"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&failure_server)
        .await;

    let remote_url = format!("{}/missing.txt", failure_server.uri());
    snapshot_store
        .save(
            &remote_url,
            &[RouteRuleConfig {
                match_type: MatchType::Exact,
                patterns: vec!["fallback.example".to_string()],
                action: RouteAction::Block,
                target: None,
            }],
        )
        .expect("snapshot should be seeded");

    let remote_configs = vec![RemoteRuleConfig {
        r#type: RemoteRuleType::Url,
        url: remote_url.clone(),
        format: RuleFormat::V2ray,
        failure_policy: RemoteRuleFailurePolicy::Strict,
        action: RouteAction::Block,
        target: None,
        auth: None,
        retry: None,
        proxy: None,
        max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
    }];

    let summary = load_and_merge_rules(
        &remote_configs,
        &[],
        &HttpClientConfig::default(),
        &snapshot_config,
    )
    .await
    .expect("remote rule load summary should be returned");

    assert!(summary.partial_failure());
    assert!(!summary.has_blocking_failures());
    assert_eq!(summary.failed_sources.len(), 1);
    assert!(summary.failed_sources[0]
        .fallback_hint
        .as_deref()
        .unwrap_or_default()
        .contains("last-known-good"));
    assert!(summary
        .merged_rules
        .iter()
        .any(|rule| rule.rule.patterns.contains(&"fallback.example".to_string())));

    let startup =
        evaluate_remote_rule_startup(summary).expect("strict fallback should allow startup");
    assert_eq!(startup.failed_sources.len(), 1);
}

#[tokio::test]
async fn test_build_router_allows_lenient_remote_failures() {
    let success_server = MockServer::start().await;
    let failure_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/ok.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("full:lenient-ok.example"))
        .mount(&success_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/missing.txt"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&failure_server)
        .await;

    let config = Config {
        remote_rule_snapshot: RemoteRuleSnapshotConfig {
            enabled: false,
            path: ".unused".to_string(),
        },
        remote_rules: vec![
            RemoteRuleConfig {
                r#type: RemoteRuleType::Url,
                url: format!("{}/ok.txt", success_server.uri()),
                format: RuleFormat::V2ray,
                failure_policy: RemoteRuleFailurePolicy::Strict,
                action: RouteAction::Block,
                target: None,
                auth: None,
                retry: None,
                proxy: None,
                max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
            },
            RemoteRuleConfig {
                r#type: RemoteRuleType::Url,
                url: format!("{}/missing.txt", failure_server.uri()),
                format: RuleFormat::V2ray,
                failure_policy: RemoteRuleFailurePolicy::Lenient,
                action: RouteAction::Block,
                target: None,
                auth: None,
                retry: None,
                proxy: None,
                max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
            },
        ],
        ..Default::default()
    };

    let router = build_router(&config)
        .await
        .expect("lenient failure should still build router");
    let matched = router
        .find_match(&hickory_proto::rr::Name::from_ascii("lenient-ok.example.").unwrap())
        .expect("successful remote rule should be compiled into router");
    assert_eq!(matched.action, RouteAction::Block);
    assert_eq!(matched.rule_source, "remote");
}

#[tokio::test]
async fn test_build_router_rejects_strict_remote_failures() {
    let failure_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/missing.txt"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&failure_server)
        .await;

    let config = Config {
        remote_rule_snapshot: RemoteRuleSnapshotConfig {
            enabled: false,
            path: ".unused".to_string(),
        },
        remote_rules: vec![RemoteRuleConfig {
            r#type: RemoteRuleType::Url,
            url: format!("{}/missing.txt", failure_server.uri()),
            format: RuleFormat::V2ray,
            failure_policy: RemoteRuleFailurePolicy::Strict,
            action: RouteAction::Block,
            target: None,
            auth: None,
            retry: None,
            proxy: None,
            max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
        }],
        ..Default::default()
    };

    let error = match build_router(&config).await {
        Ok(_) => panic!("strict failure should abort router build"),
        Err(error) => error,
    };
    assert!(
        matches!(error, AppError::Upstream(ref message) if message.contains("strict") && message.contains("/missing.txt")),
        "unexpected build_router error: {error:?}"
    );
}

#[tokio::test]
async fn test_build_router_allows_strict_remote_failures_with_snapshot_fallback() {
    let directory = tempdir().expect("snapshot directory should be created");
    let failure_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/missing.txt"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&failure_server)
        .await;

    let remote_url = format!("{}/missing.txt", failure_server.uri());
    let snapshot_config = RemoteRuleSnapshotConfig {
        enabled: true,
        path: directory.path().to_string_lossy().to_string(),
    };
    let snapshot_store = RemoteRuleSnapshotStore::new(&snapshot_config);
    snapshot_store
        .save(
            &remote_url,
            &[RouteRuleConfig {
                match_type: MatchType::Exact,
                patterns: vec!["strict-fallback.example".to_string()],
                action: RouteAction::Block,
                target: None,
            }],
        )
        .expect("snapshot should be seeded");

    let config = Config {
        remote_rule_snapshot: snapshot_config,
        remote_rules: vec![RemoteRuleConfig {
            r#type: RemoteRuleType::Url,
            url: remote_url,
            format: RuleFormat::V2ray,
            failure_policy: RemoteRuleFailurePolicy::Strict,
            action: RouteAction::Block,
            target: None,
            auth: None,
            retry: None,
            proxy: None,
            max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
        }],
        ..Default::default()
    };

    let router = build_router(&config)
        .await
        .expect("strict remote failure should recover from snapshot");
    let matched = router
        .find_match(&hickory_proto::rr::Name::from_ascii("strict-fallback.example.").unwrap())
        .expect("snapshot-backed remote rule should be compiled into router");
    assert_eq!(matched.action, RouteAction::Block);
    assert_eq!(matched.rule_source, "remote");
}
