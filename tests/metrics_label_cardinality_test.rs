use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::{ConnectInfo, Query, State},
    response::IntoResponse,
};
use hickory_proto::rr::RecordType;
use loadants::{
    cache::DnsCache,
    doh::{handlers::{handle_json_get, DohJsonGetParams}, state::AppState},
    metrics::{normalize_query_type_label, DnsMetrics, METRICS},
    handler::RequestHandler,
    router::Router,
    UpstreamManager,
};

fn count_query_type_series(metrics_output: &str) -> usize {
    metrics_output
        .lines()
        .filter(|line| {
            line.starts_with("loadants_dns_query_type_total{") && line.contains("type=")
        })
        .count()
}

fn create_test_handler() -> Arc<RequestHandler> {
    let cache = Arc::new(DnsCache::new(0, 0, None));
    let router = Arc::new(Router::new(Vec::new()).expect("Failed to create Router"));
    let upstream = Arc::new(UpstreamManager::empty().expect("Failed to create empty upstream"));

    Arc::new(RequestHandler::new(cache, router, upstream))
}

#[test]
fn metrics_query_type_labels_collapse_unknown_record_types_into_other() {
    let metrics = DnsMetrics::new();

    for raw in 1000u16..1100u16 {
        let record_type = RecordType::from(raw);
        let label = normalize_query_type_label(record_type);

        metrics.dns_query_type_total().with_label_values(&[label]).inc();
    }

    let output = metrics.export_metrics();

    assert!(output.contains("type=\"OTHER\""));
    assert_eq!(count_query_type_series(&output), 1);
}

#[tokio::test]
async fn metrics_recording_paths_use_bounded_query_type_labels() {
    let handler = create_test_handler();
    let app_state = AppState { handler };
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
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status().as_u16(), 500);
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
