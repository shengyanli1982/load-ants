use loadants::build_router;
use loadants::config::{
    AuthConfig, AuthType, Config, HttpClientConfig, MatchType, RemoteRuleConfig,
    RemoteRuleFailurePolicy, RemoteRuleSnapshotConfig, RemoteRulesConfig, RetryConfig, RouteAction,
    RouteRuleConfig,
};
use loadants::error::AppError;
use loadants::r#const::remote_rule_limits;
use loadants::remote_rule::{
    evaluate_remote_rule_startup, load_and_merge_rules, reload_once, run_panic_guarded,
    ClashRuleParser, RemoteRuleLoader, RemoteRuleSnapshotStore, RuleParser, V2RayRuleParser,
};
use loadants::router::Router;
use loadants::RemoteSourceStatus;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tempfile::tempdir;
use tokio::sync::RwLock;
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
    // D-01: 裸域名产出双规则——apex 本身的 exact + 子域的 wildcard
    assert_eq!(rules.len(), 4);
    assert_eq!(rules[0], ("example.com".to_string(), MatchType::Exact));
    assert_eq!(
        rules[1],
        (".*\\.example\\.com$".to_string(), MatchType::Regex)
    );
    assert_eq!(
        rules[2],
        ("another.domain.com".to_string(), MatchType::Exact),
        "裸域名必须额外产出覆盖 apex 本身的 exact 规则"
    );
    assert_eq!(
        rules[3],
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
        r#type: "url".to_string(),
        url: format!("{}/rules.txt", mock_server.uri()),
        format: "v2ray".to_string(),
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
        tls_verify: None,
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
                // D-01: 裸域名 `sub.domain.org` 的 apex 本身进入 Exact 组
                assert_eq!(rule.patterns.len(), 3);
                assert!(rule.patterns.contains(&"example.com".to_string()));
                assert!(rule.patterns.contains(&"test.example.com".to_string()));
                assert!(
                    rule.patterns.contains(&"sub.domain.org".to_string()),
                    "裸域名的 apex exact 规则必须与 full: 条目一起归入 Exact 组"
                );
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
        r#type: "url".to_string(),
        url: format!("{}/auth-rules.txt", mock_server.uri()),
        format: "v2ray".to_string(),
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
        tls_verify: None,
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
            r#type: "url".to_string(),
            url: format!("{}/block-rules.txt", mock_server1.uri()),
            format: "v2ray".to_string(),
            failure_policy: RemoteRuleFailurePolicy::Strict,
            action: RouteAction::Block,
            target: None,
            auth: None,
            retry: None,
            proxy: None,
            max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
            tls_verify: None,
        },
        RemoteRuleConfig {
            r#type: "url".to_string(),
            url: format!("{}/forward-rules.txt", mock_server2.uri()),
            format: "v2ray".to_string(),
            failure_policy: RemoteRuleFailurePolicy::Strict,
            action: RouteAction::Forward,
            target: Some("test-target".to_string()),
            auth: None,
            retry: None,
            proxy: None,
            max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
            tls_verify: None,
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
        r#type: "url".to_string(),
        url: format!("{}/not-found.txt", mock_server.uri()),
        format: "v2ray".to_string(),
        failure_policy: RemoteRuleFailurePolicy::Strict,
        action: RouteAction::Block,
        target: None,
        auth: None,
        retry: None,
        proxy: None,
        max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
        tls_verify: None,
    };

    let loader = RemoteRuleLoader::new(not_found_config, HttpClientConfig::default()).unwrap();
    let result = loader.load().await;
    assert!(result.is_err());

    let large_file_config = RemoteRuleConfig {
        r#type: "url".to_string(),
        url: format!("{}/large-file.txt", mock_server.uri()),
        format: "v2ray".to_string(),
        failure_policy: RemoteRuleFailurePolicy::Strict,
        action: RouteAction::Block,
        target: None,
        auth: None,
        retry: None,
        proxy: None,
        max_size: 100,
        tls_verify: None,
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
            r#type: "url".to_string(),
            url: format!("{}/ok.txt", success_server.uri()),
            format: "v2ray".to_string(),
            failure_policy: RemoteRuleFailurePolicy::Strict,
            action: RouteAction::Block,
            target: None,
            auth: None,
            retry: None,
            proxy: None,
            max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
            tls_verify: None,
        },
        RemoteRuleConfig {
            r#type: "url".to_string(),
            url: format!("{}/missing.txt", failure_server.uri()),
            format: "v2ray".to_string(),
            failure_policy: RemoteRuleFailurePolicy::Lenient,
            action: RouteAction::Block,
            target: None,
            auth: None,
            retry: None,
            proxy: None,
            max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
            tls_verify: None,
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
            r#type: "url".to_string(),
            url: format!("{}/ok.txt", success_server.uri()),
            format: "v2ray".to_string(),
            failure_policy: RemoteRuleFailurePolicy::Lenient,
            action: RouteAction::Block,
            target: None,
            auth: None,
            retry: None,
            proxy: None,
            max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
            tls_verify: None,
        },
        RemoteRuleConfig {
            r#type: "url".to_string(),
            url: format!("{}/missing.txt", failure_server.uri()),
            format: "v2ray".to_string(),
            failure_policy: RemoteRuleFailurePolicy::Strict,
            action: RouteAction::Forward,
            target: Some("test-target".to_string()),
            auth: None,
            retry: None,
            proxy: None,
            max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
            tls_verify: None,
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
        r#type: "url".to_string(),
        url: format!("{}/rules.txt", mock_server.uri()),
        format: "v2ray".to_string(),
        failure_policy: RemoteRuleFailurePolicy::Strict,
        action: RouteAction::Block,
        target: None,
        auth: None,
        retry: None,
        proxy: None,
        max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
        tls_verify: None,
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
        .await
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
        .await
        .expect("stale snapshot should be seeded");
    let stale_snapshot_path = snapshot_store.snapshot_path(&stale_url);
    let stale_tmp_path = snapshot_temp_path(&snapshot_store, &stale_url, 2001);
    fs::write(&stale_tmp_path, b"stale tmp").expect("stale tmp should be created");

    let remote_configs = vec![RemoteRuleConfig {
        r#type: "url".to_string(),
        url: format!("{}/rules.txt", active_server.uri()),
        format: "v2ray".to_string(),
        failure_policy: RemoteRuleFailurePolicy::Strict,
        action: RouteAction::Block,
        target: None,
        auth: None,
        retry: None,
        proxy: None,
        max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
        tls_verify: None,
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
        .await
        .expect("snapshot should be seeded");

    let remote_configs = vec![RemoteRuleConfig {
        r#type: "url".to_string(),
        url: remote_url.clone(),
        format: "v2ray".to_string(),
        failure_policy: RemoteRuleFailurePolicy::Strict,
        action: RouteAction::Block,
        target: None,
        auth: None,
        retry: None,
        proxy: None,
        max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
        tls_verify: None,
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
        remote_rules: RemoteRulesConfig {
            reload_interval_secs: 3600,
            snapshot: RemoteRuleSnapshotConfig {
                enabled: false,
                path: ".unused".to_string(),
            },
            sources: vec![
                RemoteRuleConfig {
                    r#type: "url".to_string(),
                    url: format!("{}/ok.txt", success_server.uri()),
                    format: "v2ray".to_string(),
                    failure_policy: RemoteRuleFailurePolicy::Strict,
                    action: RouteAction::Block,
                    target: None,
                    auth: None,
                    retry: None,
                    proxy: None,
                    max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
                    tls_verify: None,
                },
                RemoteRuleConfig {
                    r#type: "url".to_string(),
                    url: format!("{}/missing.txt", failure_server.uri()),
                    format: "v2ray".to_string(),
                    failure_policy: RemoteRuleFailurePolicy::Lenient,
                    action: RouteAction::Block,
                    target: None,
                    auth: None,
                    retry: None,
                    proxy: None,
                    max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
                    tls_verify: None,
                },
            ],
        },
        ..Default::default()
    };

    let router = build_router(&config)
        .await
        .expect("lenient failure should still build router");
    let matched = router
        .read()
        .await
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
        remote_rules: RemoteRulesConfig {
            reload_interval_secs: 3600,
            snapshot: RemoteRuleSnapshotConfig {
                enabled: false,
                path: ".unused".to_string(),
            },
            sources: vec![RemoteRuleConfig {
                r#type: "url".to_string(),
                url: format!("{}/missing.txt", failure_server.uri()),
                format: "v2ray".to_string(),
                failure_policy: RemoteRuleFailurePolicy::Strict,
                action: RouteAction::Block,
                target: None,
                auth: None,
                retry: None,
                proxy: None,
                max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
                tls_verify: None,
            }],
        },
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
        .await
        .expect("snapshot should be seeded");

    let config = Config {
        remote_rules: RemoteRulesConfig {
            reload_interval_secs: 3600,
            snapshot: snapshot_config,
            sources: vec![RemoteRuleConfig {
                r#type: "url".to_string(),
                url: remote_url,
                format: "v2ray".to_string(),
                failure_policy: RemoteRuleFailurePolicy::Strict,
                action: RouteAction::Block,
                target: None,
                auth: None,
                retry: None,
                proxy: None,
                max_size: remote_rule_limits::DEFAULT_MAX_SIZE,
                tls_verify: None,
            }],
        },
        ..Default::default()
    };

    let router = build_router(&config)
        .await
        .expect("strict remote failure should recover from snapshot");
    let matched = router
        .read()
        .await
        .find_match(&hickory_proto::rr::Name::from_ascii("strict-fallback.example.").unwrap())
        .expect("snapshot-backed remote rule should be compiled into router");
    assert_eq!(matched.action, RouteAction::Block);
    assert_eq!(matched.rule_source, "remote");
}

// ============================================================================
// 以下测试为本轮新增回归测试（D-01 裸域名 apex+wildcard 规则派生），并非迁移
// 自 src 内嵌测试（src/remote_rule/parser.rs 从无内嵌测试）。
// ============================================================================

/// D-01: V2Ray 裸域名 `example.com` 语义 = apex 本身 + 所有子域，
/// 必须同时产出 exact（apex）与 wildcard（子域）两条规则。
#[test]
fn bare_domain_emits_apex_exact_and_subdomain_wildcard() {
    let parser = V2RayRuleParser;
    let rules = parser.parse("example.com").unwrap();

    assert_eq!(rules.len(), 2, "bare domain must yield exactly 2 rules");
    assert!(
        rules.contains(&("example.com".to_string(), MatchType::Exact)),
        "missing apex exact rule, got {rules:?}"
    );
    assert!(
        rules.contains(&("*.example.com".to_string(), MatchType::Wildcard)),
        "missing subdomain wildcard rule, got {rules:?}"
    );
}

/// 验收标准②：带前缀规则（full:/regexp:）、全局通配 `*`、注释与空行语义保持不变。
#[test]
fn prefixed_and_global_rules_keep_existing_semantics() {
    let parser = V2RayRuleParser;
    let content = "# comment\n\nfull:exact.example.com\nregexp:.*\\.example\\.com$\n*";
    let rules = parser.parse(content).unwrap();

    assert_eq!(
        rules,
        vec![
            ("exact.example.com".to_string(), MatchType::Exact),
            (".*\\.example\\.com$".to_string(), MatchType::Regex),
            ("*".to_string(), MatchType::Wildcard),
        ]
    );
}

// ============================================================================
// 以下测试迁移自 src/main.rs 内嵌 #[cfg(test)] mod tests（测试语义零变化）。
// 被测函数 run_panic_guarded / reload_once 已平移至 loadants::remote_rule。
// ============================================================================

#[tokio::test]
async fn test_reload_guard_contains_iteration_panic() {
    // 单次迭代 panic 不得向外传播，否则刷新循环会静默退出且永不恢复
    run_panic_guarded("test", async {
        panic!("simulated remote rule parse panic");
    })
    .await;
}

#[tokio::test]
async fn test_reload_guard_runs_normal_iteration() {
    let completed = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&completed);
    run_panic_guarded("test", async move {
        flag.store(true, Ordering::SeqCst);
    })
    .await;
    assert!(
        completed.load(Ordering::SeqCst),
        "normal iteration should run to completion"
    );
}

#[tokio::test]
async fn test_reload_once_swaps_router_and_updates_statuses() {
    let mut remote_rules_config = RemoteRulesConfig::default();
    remote_rules_config.snapshot.enabled = false;

    let static_rules = vec![RouteRuleConfig {
        match_type: MatchType::Wildcard,
        patterns: vec!["*".to_string()],
        action: RouteAction::Forward,
        target: Some("default".to_string()),
    }];

    let router: Arc<RwLock<Arc<Router>>> = Arc::new(RwLock::new(Arc::new(
        Router::new(Vec::new()).expect("empty router should build"),
    )));
    let statuses = Arc::new(RwLock::new(vec![RemoteSourceStatus {
        url: "https://example.com/rules.txt".to_string(),
        status: "pending".to_string(),
        last_updated: 0,
        rule_count: 0,
        error_message: None,
    }]));

    reload_once(
        remote_rules_config,
        static_rules,
        HttpClientConfig::default(),
        Arc::clone(&router),
        Arc::clone(&statuses),
    )
    .await;

    assert_eq!(
        router.read().await.rule_count(),
        1,
        "router should be rebuilt from merged rules"
    );
    assert!(
        statuses.read().await.is_empty(),
        "statuses should be rebuilt from source list"
    );
}
