use std::sync::Arc;

use hickory_proto::op::{Message, MessageType, OpCode, Query, ResponseCode};
use hickory_proto::rr::{Name, RecordType};
use loadants::cache::DnsCache;
use loadants::error::AppError;
use std::str::FromStr;

fn create_query(id: u16, domain: &str, record_type: RecordType) -> Message {
    let mut message = Message::new();
    message.set_id(id);
    message.set_op_code(OpCode::Query);
    message.set_message_type(MessageType::Query);
    message.set_recursion_desired(true);

    let name = Name::from_str(&format!("{}.", domain)).unwrap();
    message.add_query(Query::query(name, record_type));
    message
}

fn create_response_with_query(
    id: u16,
    domain: &str,
    record_type: RecordType,
    rcode: ResponseCode,
) -> Message {
    let mut response = Message::new();
    response.set_id(id);
    response.set_message_type(MessageType::Response);
    response.set_op_code(OpCode::Query);
    response.set_recursion_desired(true);
    response.set_recursion_available(true);
    response.set_response_code(rcode);

    let name = Name::from_str(&format!("{}.", domain)).unwrap();
    response.add_query(Query::query(name, record_type));
    response
}

#[tokio::test]
async fn cache_get_returns_none_when_query_has_no_question() {
    let cache = DnsCache::new(16, 1, Some(60));
    let empty_query = Message::new();
    assert!(cache.get(&empty_query).await.is_none());
}

#[tokio::test]
async fn cache_insert_errors_when_query_has_no_question_but_response_has_query() {
    let cache = DnsCache::new(16, 1, Some(60));

    let empty_query = Message::new();
    let response =
        create_response_with_query(1, "example.com", RecordType::A, ResponseCode::NoError);

    let err = cache.insert(&empty_query, response).await.unwrap_err();
    assert!(matches!(err, AppError::Cache(_)));
}

#[tokio::test]
async fn cache_insert_ignores_response_without_query_section() {
    let cache = Arc::new(DnsCache::new(16, 1, Some(60)));

    let query = create_query(1, "example.com", RecordType::A);

    let mut response = Message::new();
    response.set_id(2);
    response.set_message_type(MessageType::Response);
    response.set_response_code(ResponseCode::NoError);
    // response.queries() 为空 -> is_cacheable 返回 false，应被忽略

    cache.insert(&query, response).await.unwrap();
    assert_eq!(cache.len().await, 0);
    assert!(cache.get(&query).await.is_none());
}

#[tokio::test]
async fn cache_can_store_and_return_nxdomain_responses() {
    let cache = DnsCache::new(16, 1, Some(60));

    let query = create_query(1, "example.com", RecordType::A);
    let response =
        create_response_with_query(2, "example.com", RecordType::A, ResponseCode::NXDomain);

    cache.insert(&query, response).await.unwrap();
    let cached = cache
        .get(&query)
        .await
        .expect("expected NXDOMAIN cached response");
    assert_eq!(cached.response_code(), ResponseCode::NXDomain);
}
