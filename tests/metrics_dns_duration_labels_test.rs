use std::sync::Arc;

use hickory_proto::op::{Message, MessageType, ResponseCode};
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
async fn dns_request_duration_protocol_label_not_polluted_by_handler_stage() {
    let cache = Arc::new(DnsCache::new(10, 1, Some(1)));
    let router = Arc::new(Router::new(Vec::new()).expect("failed to create router"));
    let upstream = Arc::new(UpstreamManager::empty().expect("failed to create empty upstream"));
    let handler = RequestHandler::new(Arc::clone(&cache), router, upstream);

    let query = create_test_dns_query();
    cache
        .insert(&query, create_test_dns_response())
        .await
        .expect("cache insert should succeed");

    let _ = handler
        .handle_request(&query)
        .await
        .expect("handler request should succeed via cache hit");

    let metrics_text = METRICS.export_metrics();

    assert!(
        !metrics_text.contains("protocol=\"cached\""),
        "dns_request_duration_seconds' protocol label should not contain handler stage value (cached)"
    );
    assert!(
        !metrics_text.contains("protocol=\"resolved\""),
        "dns_request_duration_seconds' protocol label should not contain handler stage value (resolved)"
    );

    assert!(
        metrics_text.contains("loadants_dns_handler_duration_seconds_bucket"),
        "dns_handler_duration_seconds should output bucket series"
    );
    assert!(
        metrics_text.contains("stage=\"cached\""),
        "dns_handler_duration_seconds should record cached stage duration"
    );
}
