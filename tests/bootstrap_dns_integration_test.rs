use hickory_proto::op::{Message, OpCode, Query, ResponseCode};
use hickory_proto::rr::rdata::A;
use hickory_proto::rr::{Name, RData, Record, RecordType};
use loadants::config::{
    BootstrapDnsConfig, DnsConfig, DnsTransportMode, DnsUpstreamEndpointConfig, DoHContentType,
    DoHMethod, DoHUpstreamEndpointConfig, HttpConfig, LoadBalancingPolicy, UpstreamEndpointConfig,
    UpstreamGroupConfig, UpstreamProtocol,
};
use loadants::upstream::UpstreamManager;
use reqwest::Url;
use std::net::{Ipv4Addr, SocketAddr};
use std::str::FromStr;
use tokio::net::UdpSocket;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

fn create_test_dns_query(domain: &str, record_type: RecordType) -> Message {
    let mut message = Message::new();
    message.set_id(1234);
    message.set_op_code(OpCode::Query);
    message.set_recursion_desired(true);

    let name = Name::from_str(&format!("{}.", domain)).unwrap();
    let query = Query::query(name, record_type);
    message.add_query(query);

    message
}

fn create_test_dns_response(id: u16) -> Vec<u8> {
    let mut response = Message::new();
    response.set_id(id);
    response.set_recursion_desired(true);
    response.set_recursion_available(true);
    response.set_op_code(OpCode::Query);
    response.set_response_code(ResponseCode::NoError);

    let name = Name::from_str("example.com.").unwrap();
    let query = Query::query(name.clone(), RecordType::A);
    response.add_query(query);

    let mut record = Record::with(name, RecordType::A, 300);
    record.set_data(Some(RData::A(A(Ipv4Addr::new(93, 184, 216, 34)))));
    response.add_answer(record);

    response.to_vec().unwrap()
}

#[tokio::test]
async fn test_bootstrap_dns_resolves_doh_hostname_without_system_resolver() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter("debug")
        .try_init();

    // 避免本机环境变量代理（HTTP_PROXY/HTTPS_PROXY/ALL_PROXY）干扰：该测试需要直连 upstream
    for key in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
    ] {
        std::env::remove_var(key);
    }

    let bootstrap_hostname = "bootstrap.test.invalid";

    // 1) 启动本地 UDP DNS stub：回答 bootstrap_hostname 的 A 记录为 127.0.0.1
    let udp_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let dns_addr = udp_socket.local_addr().unwrap();

    let qname = format!("{}.", bootstrap_hostname);
    tokio::spawn(async move {
        let mut buf = [0u8; 2048];
        loop {
            let Ok((len, peer)) = udp_socket.recv_from(&mut buf).await else {
                break;
            };

            let Ok(query) = Message::from_vec(&buf[..len]) else {
                continue;
            };

            let mut response = Message::new();
            response.set_id(query.id());
            response.set_message_type(hickory_proto::op::MessageType::Response);
            response.set_recursion_desired(query.recursion_desired());
            response.set_recursion_available(true);
            response.set_op_code(query.op_code());
            response.set_response_code(ResponseCode::NoError);

            if let Some(q) = query.queries().first() {
                response.add_query(q.clone());
                if q.name().to_utf8() == qname && q.query_type() == RecordType::A {
                    let mut record = Record::with(q.name().clone(), RecordType::A, 60);
                    record.set_data(Some(RData::A(A(Ipv4Addr::new(127, 0, 0, 1)))));
                    response.add_answer(record);
                }
            }

            let bytes = response.to_vec().unwrap();
            let _ = udp_socket.send_to(&bytes, peer).await;
        }
    });

    // 2) 启动 wiremock 作为 DoH upstream（绑定 127.0.0.1:PORT）
    let mock_server = MockServer::start().await;
    let base = Url::parse(&mock_server.uri()).unwrap();
    let port = base.port_or_known_default().unwrap();

    Mock::given(method("GET"))
        .and(path("/dns-query"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("Content-Type", "application/dns-message")
                .set_body_bytes(create_test_dns_response(1234)),
        )
        .mount(&mock_server)
        .await;

    // 3) 组装 UpstreamManager：bootstrap DNS 组 + hostname DoH 组
    let groups = vec![
        UpstreamGroupConfig {
            name: "bootstrap_dns_group".to_string(),
            protocol: UpstreamProtocol::Dns,
            policy: LoadBalancingPolicy::RoundRobin,
            endpoints: vec![UpstreamEndpointConfig::Dns(DnsUpstreamEndpointConfig {
                addr: SocketAddr::from(dns_addr),
                weight: 1,
                transport: Some(DnsTransportMode::Udp),
            })],
            fallback: None,
            failover: None,
            health: None,
            retry: None,
            proxy: None,
        },
        UpstreamGroupConfig {
            name: "doh_group".to_string(),
            protocol: UpstreamProtocol::Doh,
            policy: LoadBalancingPolicy::RoundRobin,
            endpoints: vec![UpstreamEndpointConfig::Doh(DoHUpstreamEndpointConfig {
                url: Url::parse(&format!(
                    "http://{}:{}/dns-query",
                    bootstrap_hostname, port
                ))
                .unwrap(),
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
        },
    ];

    let manager = UpstreamManager::new_with_bootstrap(
        groups,
        HttpConfig::default(),
        DnsConfig::default(),
        Some(BootstrapDnsConfig {
            groups: vec!["bootstrap_dns_group".to_string()],
            timeout: 2,
            cache_ttl: 300,
            prefer_ipv6: false,
            use_system_resolver: false,
        }),
    )
    .await
    .unwrap();

    let query = create_test_dns_query("example.com", RecordType::A);
    let response = manager.forward(&query, "doh_group").await.unwrap();
    assert_eq!(response.response_code(), ResponseCode::NoError);
}
