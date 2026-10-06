use hickory_proto::op::{Edns, Message, MessageType, OpCode, Query, ResponseCode};
use hickory_proto::rr::{Name, Record, RecordType};
use hickory_proto::serialize::binary::{BinDecodable, BinDecoder, BinEncoder};
use hickory_proto::xfer::Protocol;
use hickory_server::authority::{MessageRequest, MessageResponse};
use hickory_server::server::RequestHandler as HickoryRequestHandler;
use hickory_server::server::{Request, ResponseHandler, ResponseInfo};
use loadants::cache::DnsCache;
use loadants::handler::RequestHandler;
use loadants::router::Router;
use loadants::server::{parse_request_message, HandlerAdapter};
use loadants::RateLimiter;
use loadants::UpstreamManager;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

#[test]
fn test_parse_request_message_preserves_edns() {
    let mut message = Message::new();
    message
        .set_id(10)
        .set_message_type(MessageType::Query)
        .set_op_code(OpCode::Query)
        .set_recursion_desired(true);

    let mut query = Query::new();
    query.set_name(Name::from_ascii("www.example.com.").unwrap());
    query.set_query_type(RecordType::A);
    message.add_query(query);

    let mut edns = Edns::new();
    edns.set_max_payload(1232);
    message.set_edns(edns);

    let bytes = message.to_vec().unwrap();
    let mut decoder = BinDecoder::new(&bytes);
    let message_request = MessageRequest::read(&mut decoder).unwrap();

    let src = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 5533);
    let request = Request::new(message_request, src, Protocol::Udp);

    let parsed = parse_request_message(&request).unwrap();
    assert!(parsed.extensions().is_some());
    assert_eq!(parsed.extensions().as_ref().unwrap().max_payload(), 1232);
}

/// 记录型 ResponseHandler：捕获 send_response 序列化后的 wire 字节，
/// 用于断言限速分支实际发出了响应（D-09 seam 测试）。
#[derive(Clone, Default)]
struct RecordingResponseHandler {
    sent: Arc<Mutex<Vec<Vec<u8>>>>,
}

#[async_trait::async_trait]
impl ResponseHandler for RecordingResponseHandler {
    async fn send_response<'a>(
        &mut self,
        response: MessageResponse<
            '_,
            'a,
            impl Iterator<Item = &'a Record> + Send + 'a,
            impl Iterator<Item = &'a Record> + Send + 'a,
            impl Iterator<Item = &'a Record> + Send + 'a,
            impl Iterator<Item = &'a Record> + Send + 'a,
        >,
    ) -> io::Result<ResponseInfo> {
        let mut buffer = Vec::with_capacity(512);
        let mut encoder = BinEncoder::new(&mut buffer);
        let info = response
            .destructive_emit(&mut encoder)
            .map_err(io::Error::other)?;
        self.sent.lock().unwrap().push(buffer);
        Ok(info)
    }
}

fn create_test_handler() -> Arc<RequestHandler> {
    let cache = Arc::new(DnsCache::new(0, 0, 0, None, None));
    let router = Arc::new(RwLock::new(Arc::new(
        Router::new(Vec::new()).expect("Failed to create Router"),
    )));
    let upstream = Arc::new(UpstreamManager::empty().expect("Failed to create empty upstream"));
    Arc::new(RequestHandler::new(cache, router, upstream))
}

const CLIENT_ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 7)), 40000);

fn build_query_request(id: u16, protocol: Protocol) -> Request {
    let mut message = Message::new();
    message
        .set_id(id)
        .set_message_type(MessageType::Query)
        .set_op_code(OpCode::Query)
        .set_recursion_desired(true);

    let mut query = Query::new();
    query.set_name(Name::from_ascii("rate-limit.example.com.").unwrap());
    query.set_query_type(RecordType::A);
    message.add_query(query);

    let bytes = message.to_vec().unwrap();
    let mut decoder = BinDecoder::new(&bytes);
    let message_request = MessageRequest::read(&mut decoder).unwrap();
    Request::new(message_request, CLIENT_ADDR, protocol)
}

fn decode_response(bytes: &[u8]) -> Message {
    let mut decoder = BinDecoder::new(bytes);
    Message::read(&mut decoder).expect("captured response must be valid wire message")
}

/// D-09: 限速命中时必须通过 response_handler 实际发送 REFUSED 响应（UDP 路径），
/// 而非静默丢包导致客户端超时重试放大流量。
#[tokio::test]
async fn test_rate_limited_udp_request_receives_refused_response() {
    let limiter = RateLimiter::new(1);
    // 预先耗尽该 IP 的配额，令 adapter 内的 check 命中限速
    assert!(limiter.check(CLIENT_ADDR.ip()));

    let adapter = HandlerAdapter::new(create_test_handler(), Some(limiter));
    let request = build_query_request(0x1234, Protocol::Udp);
    let recorder = RecordingResponseHandler::default();

    adapter.handle_request(&request, recorder.clone()).await;

    let sent = recorder.sent.lock().unwrap();
    assert_eq!(sent.len(), 1, "rate-limited request must send a response");
    let response = decode_response(&sent[0]);
    assert_eq!(response.id(), 0x1234);
    assert_eq!(response.message_type(), MessageType::Response);
    assert_eq!(response.response_code(), ResponseCode::Refused);
    assert_eq!(response.queries().len(), 1, "query section must be echoed");
}

/// D-09: TCP 路径同样必须发送 REFUSED（与 UDP 共用 handle_request 分支；
/// wire 层长度前缀帧由 hickory ResponseHandle 负责，与正常回包同一路径）。
#[tokio::test]
async fn test_rate_limited_tcp_request_receives_refused_response() {
    let limiter = RateLimiter::new(1);
    assert!(limiter.check(CLIENT_ADDR.ip()));

    let adapter = HandlerAdapter::new(create_test_handler(), Some(limiter));
    let request = build_query_request(0x4321, Protocol::Tcp);
    let recorder = RecordingResponseHandler::default();

    adapter.handle_request(&request, recorder.clone()).await;

    let sent = recorder.sent.lock().unwrap();
    assert_eq!(sent.len(), 1, "rate-limited request must send a response");
    let response = decode_response(&sent[0]);
    assert_eq!(response.id(), 0x4321);
    assert_eq!(response.message_type(), MessageType::Response);
    assert_eq!(response.response_code(), ResponseCode::Refused);
}

/// D-09 回归护栏: 限速未命中时请求正常进入 handler 路径并回包（非 REFUSED），
/// 保证修复不影响既有正常回包行为。
#[tokio::test]
async fn test_unlimited_request_still_reaches_handler() {
    let limiter = RateLimiter::new(100);
    let adapter = HandlerAdapter::new(create_test_handler(), Some(limiter));
    let request = build_query_request(0x5678, Protocol::Udp);
    let recorder = RecordingResponseHandler::default();

    adapter.handle_request(&request, recorder.clone()).await;

    let sent = recorder.sent.lock().unwrap();
    assert_eq!(sent.len(), 1, "normal request must send a response");
    let response = decode_response(&sent[0]);
    assert_eq!(response.id(), 0x5678);
    assert_ne!(
        response.response_code(),
        ResponseCode::Refused,
        "request within limit must not be refused"
    );
}
