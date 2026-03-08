use std::sync::Arc;

use hickory_proto::op::{Message, MessageType, OpCode, Query, ResponseCode};
use hickory_proto::rr::{Name, RData, Record, RecordType};
use loadants::cache::DnsCache;
use loadants::config::{
    DnsConfig, DoHContentType, DoHMethod, DoHUpstreamEndpointConfig, HttpConfig,
    LoadBalancingPolicy, MatchType, RouteAction, RouteRuleConfig, UpstreamEndpointConfig,
    UpstreamGroupConfig, UpstreamProtocol,
};
use loadants::handler::RequestHandler;
use loadants::upstream::UpstreamManager;
use reqwest::Url;
use std::net::Ipv4Addr;
use std::str::FromStr;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

fn create_test_dns_query(id: u16, domain: &str, record_type: RecordType) -> Message {
    let mut message = Message::new();
    message.set_id(id);
    message.set_op_code(OpCode::Query);
    message.set_message_type(MessageType::Query);
    message.set_recursion_desired(true);

    let name = Name::from_str(&format!("{}.", domain)).unwrap();
    let query = Query::query(name, record_type);
    message.add_query(query);

    message
}

fn create_test_dns_response_bytes(domain: &str, record_type: RecordType) -> Vec<u8> {
    let mut response = Message::new();
    response.set_id(9999);
    response.set_message_type(MessageType::Response);
    response.set_recursion_desired(true);
    response.set_recursion_available(true);
    response.set_op_code(OpCode::Query);
    response.set_response_code(ResponseCode::NoError);

    let name = Name::from_str(&format!("{}.", domain)).unwrap();
    response.add_query(Query::query(name.clone(), record_type));

    let record = Record::from_rdata(
        name,
        300,
        match record_type {
            RecordType::A => RData::A(hickory_proto::rr::rdata::A(Ipv4Addr::new(93, 184, 216, 34))),
            _ => panic!("unsupported record type for this test"),
        },
    );
    response.add_answer(record);

    response.to_vec().unwrap()
}

#[tokio::test]
async fn handler_cache_hit_skips_upstream_and_overwrites_response_id() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/dns-query"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("Content-Type", "application/dns-message")
                .set_body_bytes(create_test_dns_response_bytes("example.com", RecordType::A)),
        )
        .mount(&mock_server)
        .await;

    let groups = vec![UpstreamGroupConfig {
        name: "test_group".to_string(),
        protocol: UpstreamProtocol::Doh,
        policy: LoadBalancingPolicy::RoundRobin,
        max_concurrent: None,
        endpoints: vec![UpstreamEndpointConfig::Doh(DoHUpstreamEndpointConfig {
            url: Url::parse(&format!("{}/dns-query", mock_server.uri())).unwrap(),
            weight: 1,
            method: DoHMethod::Get,
            content_type: DoHContentType::Message,
            auth: None,
        })],
        fallback: None,
        failover: None,
        health: None,
        retry: None,
        proxy: None,
    }];

    let upstream = Arc::new(
        UpstreamManager::new(groups, HttpConfig::default(), DnsConfig::default())
            .await
            .unwrap(),
    );

    let rules = vec![RouteRuleConfig {
        match_type: MatchType::Exact,
        patterns: vec!["example.com".to_string()],
        action: RouteAction::Forward,
        upstream: Some("test_group".to_string()),
    }];
    let router = Arc::new(loadants::router::Router::new(rules).unwrap());

    let cache = Arc::new(DnsCache::new(64, 1, Some(60)));
    let handler = RequestHandler::new(Arc::clone(&cache), router, upstream);

    let query1 = create_test_dns_query(1111, "example.com", RecordType::A);
    let resp1 = handler.handle_request(&query1).await.unwrap();
    assert_eq!(resp1.id(), 1111);
    assert_eq!(resp1.response_code(), ResponseCode::NoError);

    let query2 = create_test_dns_query(2222, "example.com", RecordType::A);
    let resp2 = handler.handle_request(&query2).await.unwrap();
    assert_eq!(resp2.id(), 2222);
    assert_eq!(resp2.response_code(), ResponseCode::NoError);

    let received = mock_server
        .received_requests()
        .await
        .expect("wiremock request recording should be enabled by default");
    assert_eq!(
        received.len(),
        1,
        "expected upstream to be called once; second request should be served from cache"
    );
}
