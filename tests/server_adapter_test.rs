use hickory_proto::op::{Edns, Message, MessageType, OpCode, Query};
use hickory_proto::rr::{Name, RecordType};
use hickory_proto::serialize::binary::{BinDecodable, BinDecoder};
use hickory_server::authority::MessageRequest;
use hickory_server::server::{Protocol, Request};
use loadants::server::parse_request_message;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

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
