use hickory_proto::{
    op::{Message, MessageType, OpCode, Query, ResponseCode},
    rr::{Name, RecordType},
};
use loadants::{
    DnsCache, MatchType, RequestHandler, RouteAction, RouteRuleConfig, Router, UpstreamManager,
};
use std::sync::Arc;
use tokio::sync::RwLock;

async fn make_handler() -> RequestHandler {
    let cache = Arc::new(DnsCache::new(100, 10, 300, Some(60), None));
    let router = Arc::new(RwLock::new(Arc::new(
        Router::new(vec![
            RouteRuleConfig {
                match_type: MatchType::Exact,
                patterns: vec!["blocked.com".to_string()],
                action: RouteAction::Block,
                target: None,
            },
            RouteRuleConfig {
                match_type: MatchType::Wildcard,
                patterns: vec!["*.forward.com".to_string()],
                action: RouteAction::Forward,
                target: Some("test-group".to_string()),
            },
        ])
        .unwrap(),
    )));
    let upstream = Arc::new(UpstreamManager::empty().unwrap());
    RequestHandler::new(cache, router, upstream)
}

fn make_query_msg(name: &str, record_type: RecordType) -> Message {
    let mut msg = Message::new();
    msg.set_id(1);
    msg.set_message_type(MessageType::Query);
    msg.set_op_code(OpCode::Query);
    msg.set_recursion_desired(true);
    msg.add_query(Query::query(Name::from_ascii(name).unwrap(), record_type));
    msg
}

#[tokio::test]
async fn test_block_response_refused() {
    let handler = make_handler().await;
    let request = make_query_msg("blocked.com.", RecordType::A);

    let result = handler.handle_request(&request).await.unwrap();
    assert_eq!(
        result.response_code(),
        ResponseCode::Refused,
        "block action should return Refused"
    );
    assert_eq!(result.message_type(), MessageType::Response);
    assert_eq!(result.id(), request.id());
    assert_eq!(result.queries().len(), 1);
}

#[tokio::test]
async fn test_validate_request_rejects_non_query() {
    let handler = make_handler().await;
    let mut msg = Message::new();
    msg.set_id(1);
    msg.set_message_type(MessageType::Response);
    msg.set_op_code(OpCode::Query);
    msg.add_query(Query::query(
        Name::from_ascii("example.com.").unwrap(),
        RecordType::A,
    ));

    let result = handler.validate_request(&msg);
    assert!(result.is_err(), "non-query message should be rejected");
}

#[tokio::test]
async fn test_validate_request_rejects_empty_query() {
    let handler = make_handler().await;
    let mut msg = Message::new();
    msg.set_id(1);
    msg.set_message_type(MessageType::Query);
    msg.set_op_code(OpCode::Query);

    let result = handler.validate_request(&msg);
    assert!(
        result.is_err(),
        "message with no queries should be rejected"
    );
}

#[tokio::test]
async fn test_create_error_response_servfail() {
    let request = make_query_msg("example.com.", RecordType::A);

    let result = RequestHandler::create_error_response(&request, ResponseCode::ServFail).unwrap();
    assert_eq!(result.response_code(), ResponseCode::ServFail);
    assert_eq!(result.message_type(), MessageType::Response);
    assert_eq!(result.id(), request.id());
    assert!(result.recursion_available());
    assert_eq!(result.queries().len(), 1);
}

fn make_response(
    query: &Message,
    code: ResponseCode,
    answers: Vec<hickory_proto::rr::Record>,
) -> Message {
    let mut msg = Message::new();
    msg.set_id(query.id());
    msg.set_message_type(MessageType::Response);
    msg.set_op_code(query.op_code());
    msg.set_recursion_desired(query.recursion_desired());
    msg.set_recursion_available(true);
    msg.set_response_code(code);
    for q in query.queries() {
        msg.add_query(q.clone());
    }
    let answer_count = answers.len() as u16;
    for ans in answers {
        msg.add_answer(ans);
    }
    let mut header = *msg.header();
    header.set_answer_count(answer_count);
    msg.set_header(header);
    msg
}

#[tokio::test]
async fn test_upstream_failure_stale_disabled_returns_servfail() {
    let cache = Arc::new(DnsCache::new(100, 10, 300, Some(60), Some(0)));
    let router = Arc::new(RwLock::new(Arc::new(
        Router::new(vec![RouteRuleConfig {
            match_type: MatchType::Exact,
            patterns: vec!["stale-test.com.".to_string()],
            action: RouteAction::Forward,
            target: Some("nonexistent-group".to_string()),
        }])
        .unwrap(),
    )));
    let upstream = Arc::new(UpstreamManager::empty().unwrap());
    let handler = RequestHandler::new(cache, router, upstream);

    let request = make_query_msg("stale-test.com.", RecordType::A);
    let result = handler.handle_request(&request).await.unwrap();
    assert_eq!(
        result.response_code(),
        ResponseCode::ServFail,
        "upstream failure with stale disabled should return ServFail"
    );
}

#[tokio::test]
async fn test_upstream_failure_stale_enabled_no_entry_returns_servfail() {
    let cache = Arc::new(DnsCache::new(100, 10, 300, Some(60), Some(300)));
    let router = Arc::new(RwLock::new(Arc::new(
        Router::new(vec![RouteRuleConfig {
            match_type: MatchType::Exact,
            patterns: vec!["stale-test2.com.".to_string()],
            action: RouteAction::Forward,
            target: Some("nonexistent-group".to_string()),
        }])
        .unwrap(),
    )));
    let upstream = Arc::new(UpstreamManager::empty().unwrap());
    let handler = RequestHandler::new(cache, router, upstream);

    let request = make_query_msg("stale-test2.com.", RecordType::A);
    let result = handler.handle_request(&request).await.unwrap();
    assert_eq!(
        result.response_code(),
        ResponseCode::ServFail,
        "upstream failure with stale enabled but no cached entry should return ServFail"
    );
}

#[tokio::test]
async fn test_stale_entry_served_on_upstream_failure() {
    use hickory_proto::rr::{rdata::A, RData, Record};
    use std::net::Ipv4Addr;

    let cache = Arc::new(DnsCache::new(100, 1, 300, Some(60), Some(300)));

    let query = make_query_msg("stale-fallback.com.", RecordType::A);
    let record = Record::from_rdata(
        Name::from_ascii("stale-fallback.com.").unwrap(),
        1,
        RData::A(A::from(Ipv4Addr::new(1, 2, 3, 4))),
    );
    let response = make_response(&query, ResponseCode::NoError, vec![record]);
    cache.insert(&query, response).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    let router = Arc::new(RwLock::new(Arc::new(
        Router::new(vec![RouteRuleConfig {
            match_type: MatchType::Exact,
            patterns: vec!["stale-fallback.com.".to_string()],
            action: RouteAction::Forward,
            target: Some("nonexistent-group".to_string()),
        }])
        .unwrap(),
    )));
    let upstream = Arc::new(UpstreamManager::empty().unwrap());
    let handler = RequestHandler::new(cache, router, upstream);

    let request = make_query_msg("stale-fallback.com.", RecordType::A);
    let result = handler.handle_request(&request).await.unwrap();
    assert_eq!(
        result.response_code(),
        ResponseCode::NoError,
        "stale entry should be served instead of ServFail when available"
    );
    assert_eq!(
        result.answer_count(),
        1,
        "stale response should contain the cached answer"
    );
}

#[tokio::test]
async fn test_forward_missing_target_returns_servfail() {
    let cache = Arc::new(DnsCache::new(0, 10, 300, Some(60), None));
    let router = Arc::new(RwLock::new(Arc::new(
        Router::new(vec![RouteRuleConfig {
            match_type: MatchType::Exact,
            patterns: vec!["no-target.com".to_string()],
            action: RouteAction::Forward,
            target: None,
        }])
        .unwrap(),
    )));
    let upstream = Arc::new(UpstreamManager::empty().unwrap());
    let handler = RequestHandler::new(cache, router, upstream);

    let request = make_query_msg("no-target.com.", RecordType::A);
    let result = handler.handle_request(&request).await.unwrap();
    assert_eq!(
        result.response_code(),
        ResponseCode::ServFail,
        "forward with no target should return ServFail"
    );
}
