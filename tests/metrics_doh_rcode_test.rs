use std::sync::Arc;

use axum::extract::{ConnectInfo, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hickory_proto::op::{Message, MessageType, ResponseCode};
use loadants::doh::handlers::{handle_doh_get, DohGetParams};
use loadants::doh::state::AppState;
use loadants::metrics::METRICS;
use loadants::{
    cache::DnsCache, handler::RequestHandler, router::Router, upstream::UpstreamManager,
};

fn create_test_dns_query() -> Message {
    let mut query = Message::new();
    query.set_id(1234);
    query.set_message_type(MessageType::Query);
    query.set_recursion_desired(true);

    let mut q = hickory_proto::op::Query::new();
    q.set_name(hickory_proto::rr::Name::from_ascii("example.com.").unwrap());
    q.set_query_type(hickory_proto::rr::RecordType::A);
    query.add_query(q);

    query
}

fn create_test_dns_response() -> Message {
    let mut response = Message::new();
    response.set_id(1234);
    response.set_message_type(MessageType::Response);
    response.set_recursion_desired(true);
    response.set_recursion_available(true);
    response.set_response_code(ResponseCode::NoError);

    let mut q = hickory_proto::op::Query::new();
    q.set_name(hickory_proto::rr::Name::from_ascii("example.com.").unwrap());
    q.set_query_type(hickory_proto::rr::RecordType::A);
    response.add_query(q);

    response
}

#[tokio::test]
async fn doh_get_records_dns_response_code_metric() {
    let cache = Arc::new(DnsCache::new(10, 1, Some(1)));
    let router = Arc::new(Router::new(Vec::new()).expect("failed to create router"));
    let upstream = Arc::new(UpstreamManager::empty().expect("failed to create empty upstream"));
    let handler = Arc::new(RequestHandler::new(Arc::clone(&cache), router, upstream));

    let query = create_test_dns_query();
    cache
        .insert(&query, create_test_dns_response())
        .await
        .expect("cache insert should succeed");

    let dns_param = URL_SAFE_NO_PAD.encode(query.to_vec().expect("encode query"));
    let app_state = AppState { handler };
    let addr = "127.0.0.1:8080".parse().unwrap();

    let response = handle_doh_get(
        State(app_state),
        ConnectInfo(addr),
        Query(DohGetParams { dns: dns_param }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);

    let expected_rcode = ResponseCode::NoError.to_string();
    let metrics_text = METRICS.export_metrics();
    assert!(
        metrics_text.contains(&format!(
            "loadants_dns_response_codes_total{{rcode=\"{}\"}}",
            expected_rcode
        )),
        "DoH success path should record dns_response_codes_total"
    );
}
