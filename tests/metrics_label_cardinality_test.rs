use std::{net::SocketAddr, sync::Arc};
use tokio::sync::RwLock;

use axum::{
    extract::{ConnectInfo, Query, State},
    response::IntoResponse,
};
use hickory_proto::rr::{Name, RecordType};
use loadants::{
    cache::DnsCache,
    config::{MatchType, RouteAction, RouteRuleConfig},
    doh::{
        handlers::{handle_json_get, DohJsonGetParams},
        state::AppState,
    },
    handler::RequestHandler,
    metrics::{normalize_query_type_label, METRICS},
    router::{RoutedRule, Router, RuleMetadata},
    UpstreamManager,
};

fn count_query_type_series(metrics_output: &str) -> usize {
    metrics_output
        .lines()
        .filter(|line| line.starts_with("loadants_dns_query_type_total{") && line.contains("type="))
        .count()
}

fn has_metric_line(metrics_output: &str, metric_name: &str, fragments: &[&str]) -> bool {
    metrics_output.lines().any(|line| {
        line.starts_with(metric_name) && fragments.iter().all(|fragment| line.contains(fragment))
    })
}

fn create_test_handler() -> Arc<RequestHandler> {
    let cache = Arc::new(DnsCache::new(0, 0, 0, None, None));
    let router = Arc::new(RwLock::new(Arc::new(
        Router::new(Vec::new()).expect("Failed to create Router"),
    )));
    let upstream = Arc::new(UpstreamManager::empty().expect("Failed to create empty upstream"));

    Arc::new(RequestHandler::new(cache, router, upstream))
}

#[test]
fn metrics_query_type_labels_collapse_unknown_record_types_into_other() {
    for raw in 1000u16..1100u16 {
        let record_type = RecordType::from(raw);
        let label = normalize_query_type_label(record_type);

        METRICS
            .dns_query_type_total
            .with_label_values(&[label])
            .inc();
    }

    let output = METRICS.export_metrics();

    assert!(output.contains("type=\"OTHER\""));
    assert!(count_query_type_series(&output) >= 1);
}

#[tokio::test]
async fn metrics_recording_paths_use_bounded_query_type_labels() {
    let handler = create_test_handler();
    let app_state = AppState {
        handler,
        rate_limiter: None,
    };
    let addr: SocketAddr = "127.0.0.1:18080".parse().unwrap();
    let uncommon_types: Vec<u16> = (2000u16..2010u16).collect();

    for raw in &uncommon_types {
        let response = handle_json_get(
            State(app_state.clone()),
            ConnectInfo(addr),
            Query(DohJsonGetParams {
                name: "example.com".to_string(),
                r#type: Some(raw.to_string()),
                cd: None,
                do_flag: None,
                ct: None,
                ecs: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status().as_u16(), 200);
    }

    let output = METRICS.export_metrics();

    for raw in uncommon_types {
        let dynamic_label = format!("type=\"{}\"", RecordType::from(raw));
        assert!(
            !output.contains(&dynamic_label),
            "unexpected dynamic label found in metrics output: {dynamic_label}"
        );
    }

    assert!(output.contains("loadants_dns_query_type_total{type=\"OTHER\"}"));
}

#[test]
fn metrics_stale_fallback_is_labeled_by_upstream_group() {
    let before = METRICS
        .stale_fallback_total
        .with_label_values(&["stale-fallback-cardinality-group"])
        .get();

    METRICS
        .stale_fallback_total
        .with_label_values(&["stale-fallback-cardinality-group"])
        .inc();

    let output = METRICS.export_metrics();

    assert!(
        output
            .contains("loadants_stale_fallback_total{group=\"stale-fallback-cardinality-group\"}"),
        "stale_fallback_total must expose a group label, got:\n{}",
        output
            .lines()
            .filter(|line| line.contains("stale_fallback_total"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(
        !output.contains("stale_fallback_total{query_type="),
        "stale_fallback_total must not carry a query_type label"
    );
    assert_eq!(
        METRICS
            .stale_fallback_total
            .with_label_values(&["stale-fallback-cardinality-group"])
            .get(),
        before + 1
    );
}

#[test]
fn metrics_route_rule_source_labels_follow_rule_metadata() {
    let router = Router::new_with_metadata(vec![
        RoutedRule::from_static(RouteRuleConfig {
            match_type: MatchType::Exact,
            patterns: vec!["static-metrics.example".to_string()],
            action: RouteAction::Block,
            target: None,
        }),
        RoutedRule::new(
            RouteRuleConfig {
                match_type: MatchType::Exact,
                patterns: vec!["remote-metrics.example".to_string()],
                action: RouteAction::Forward,
                target: Some("metrics-upstream".to_string()),
            },
            RuleMetadata::remote("https://example.com/metrics-remote.txt"),
        ),
    ])
    .expect("router with metadata should build");

    router
        .find_match(&Name::from_ascii("static-metrics.example.").unwrap())
        .expect("static match should succeed");
    router
        .find_match(&Name::from_ascii("remote-metrics.example.").unwrap())
        .expect("remote match should succeed");

    let output = METRICS.export_metrics();

    assert!(has_metric_line(
        &output,
        "loadants_route_matches_total{",
        &[
            "rule_source=\"static\"",
            "rule_type=\"exact\"",
            "action=\"block\"",
        ],
    ));
    assert!(has_metric_line(
        &output,
        "loadants_route_matches_total{",
        &[
            "rule_source=\"remote\"",
            "rule_type=\"exact\"",
            "action=\"forward\"",
            "target_group=\"metrics-upstream\"",
        ],
    ));
    assert!(has_metric_line(
        &output,
        "loadants_route_rules_count{",
        &["rule_source=\"static\"", "rule_type=\"exact\""],
    ));
    assert!(has_metric_line(
        &output,
        "loadants_route_rules_count{",
        &["rule_source=\"remote\"", "rule_type=\"exact\""],
    ));
}
