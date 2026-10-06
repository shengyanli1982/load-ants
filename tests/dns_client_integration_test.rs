use hickory_proto::op::{Message, MessageType, OpCode, Query, ResponseCode};
use hickory_proto::rr::{Name, RecordType};
use loadants::config::{
    DnsClientConfig, DnsUpstreamServerConfig, HttpClientConfig, LoadBalancingStrategy,
    UpstreamGroupConfig, UpstreamScheme, UpstreamServerConfig,
};
use loadants::upstream::{DnsClient, UpstreamManager};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};
use tokio::task::JoinHandle;
use tokio::time::{self, Duration};

fn create_dns_query(id: u16, domain: &str) -> Message {
    let mut message = Message::new();
    message.set_id(id);
    message.set_op_code(OpCode::Query);
    message.set_recursion_desired(true);

    let name = Name::from_str(&format!("{}.", domain)).unwrap();
    let query = Query::query(name, RecordType::A);
    message.add_query(query);

    message
}

fn build_response_from_query(
    query: &Message,
    response_code: ResponseCode,
    truncated: bool,
) -> Message {
    let mut response = Message::new();
    response
        .set_id(query.id())
        .set_message_type(MessageType::Response)
        .set_recursion_desired(query.recursion_desired())
        .set_recursion_available(true)
        .set_op_code(query.op_code())
        .set_response_code(response_code)
        .set_truncated(truncated);

    if let Some(q) = query.queries().first() {
        response.add_query(q.clone());
    }

    response
}

async fn spawn_udp_server(
    addr: SocketAddr,
    udp_count: Arc<AtomicUsize>,
    response_code: ResponseCode,
    truncated: bool,
) -> JoinHandle<()> {
    let socket = UdpSocket::bind(addr).await.unwrap();
    tokio::spawn(async move {
        let mut buf = [0u8; 2048];
        let recv = time::timeout(Duration::from_secs(2), socket.recv_from(&mut buf)).await;
        if let Ok(Ok((len, peer))) = recv {
            udp_count.fetch_add(1, Ordering::SeqCst);
            let query = Message::from_vec(&buf[..len]).unwrap();
            let response = build_response_from_query(&query, response_code, truncated);
            let bytes = response.to_vec().unwrap();
            let _ = socket.send_to(&bytes, peer).await;
        }
    })
}

async fn spawn_tcp_server(
    listener: TcpListener,
    tcp_count: Arc<AtomicUsize>,
    response_code: ResponseCode,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let accept = time::timeout(Duration::from_secs(2), listener.accept()).await;
        let Ok(Ok((mut stream, _))) = accept else {
            return;
        };

        tcp_count.fetch_add(1, Ordering::SeqCst);

        let mut len_buf = [0u8; 2];
        if stream.read_exact(&mut len_buf).await.is_err() {
            return;
        }
        let msg_len = u16::from_be_bytes(len_buf) as usize;

        let mut msg_buf = vec![0u8; msg_len];
        if stream.read_exact(&mut msg_buf).await.is_err() {
            return;
        }

        let query = Message::from_vec(&msg_buf).unwrap();
        let response = build_response_from_query(&query, response_code, false);
        let bytes = response.to_vec().unwrap();

        let len_prefix = (bytes.len() as u16).to_be_bytes();
        let _ = stream.write_all(&len_prefix).await;
        let _ = stream.write_all(&bytes).await;
    })
}

async fn build_dns_manager(addr: SocketAddr, dns_config: DnsClientConfig) -> UpstreamManager {
    let groups = vec![UpstreamGroupConfig {
        name: "dns_group".to_string(),
        scheme: UpstreamScheme::Dns,
        strategy: LoadBalancingStrategy::RoundRobin,
        servers: vec![UpstreamServerConfig::Dns(DnsUpstreamServerConfig {
            addr,
            weight: 1,
        })],
        retry: None,
        proxy: None,
        tls_verify: None,
        deny_answers: vec![],
        case_randomization: false,
        case_randomization_strict: false,
    }];

    UpstreamManager::new(groups, HttpClientConfig::default(), dns_config)
        .await
        .unwrap()
}

#[tokio::test]
async fn test_dns_prefer_tcp_true_uses_tcp_only() {
    let tcp_count = Arc::new(AtomicUsize::new(0));
    let udp_count = Arc::new(AtomicUsize::new(0));

    let tcp_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = tcp_listener.local_addr().unwrap().port();
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);

    let _tcp = spawn_tcp_server(tcp_listener, tcp_count.clone(), ResponseCode::NoError).await;
    let _udp = spawn_udp_server(addr, udp_count.clone(), ResponseCode::NoError, false).await;

    let manager = build_dns_manager(
        addr,
        DnsClientConfig {
            connect_timeout: 1,
            request_timeout: 2,
            prefer_tcp: true,
            idle_connection_timeout: 30,
            tcp_reconnect: true,
            max_tcp_connections: 256,
        },
    )
    .await;

    let query = create_dns_query(200, "example.com");
    let response = manager.forward(&query, "dns_group").await.unwrap();
    assert_eq!(response.response_code(), ResponseCode::NoError);
    assert_eq!(tcp_count.load(Ordering::SeqCst), 1);
    time::sleep(Duration::from_millis(50)).await;
    assert_eq!(udp_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn test_dns_udp_tc_triggers_tcp_retry() {
    let tcp_count = Arc::new(AtomicUsize::new(0));
    let udp_count = Arc::new(AtomicUsize::new(0));

    let tcp_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = tcp_listener.local_addr().unwrap().port();
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);

    let _tcp = spawn_tcp_server(tcp_listener, tcp_count.clone(), ResponseCode::NoError).await;
    let _udp = spawn_udp_server(addr, udp_count.clone(), ResponseCode::NoError, true).await;

    let manager = build_dns_manager(
        addr,
        DnsClientConfig {
            connect_timeout: 1,
            request_timeout: 2,
            prefer_tcp: false,
            idle_connection_timeout: 30,
            tcp_reconnect: true,
            max_tcp_connections: 256,
        },
    )
    .await;

    let query = create_dns_query(201, "example.com");
    let response = manager.forward(&query, "dns_group").await.unwrap();
    assert_eq!(response.response_code(), ResponseCode::NoError);
    assert_eq!(udp_count.load(Ordering::SeqCst), 1);
    assert_eq!(tcp_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_dns_nxdomain_transparent() {
    let udp_count = Arc::new(AtomicUsize::new(0));

    let udp_socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let addr = udp_socket.local_addr().unwrap();
    drop(udp_socket);

    let _udp = spawn_udp_server(addr, udp_count.clone(), ResponseCode::NXDomain, false).await;

    let manager = build_dns_manager(
        addr,
        DnsClientConfig {
            connect_timeout: 1,
            request_timeout: 2,
            prefer_tcp: false,
            idle_connection_timeout: 30,
            tcp_reconnect: true,
            max_tcp_connections: 256,
        },
    )
    .await;

    let query = create_dns_query(202, "nxdomain.example");
    let response = manager.forward(&query, "dns_group").await.unwrap();
    assert_eq!(response.response_code(), ResponseCode::NXDomain);
    assert_eq!(udp_count.load(Ordering::SeqCst), 1);
}

// ============================================================================
// 以下测试迁移自 src/upstream/dns_client.rs 内嵌 #[cfg(test)] mod tests（测试语义零变化）。
// ============================================================================

fn response_with_question(name_ascii: &str) -> Message {
    let mut response = Message::new();
    response.set_message_type(MessageType::Response);
    let name = Name::from_ascii(name_ascii).expect("valid DNS name");
    response.add_query(Query::query(name, RecordType::A));
    response
}

#[test]
fn verify_response_case_fails_when_question_section_missing() {
    let response = Message::new();
    assert!(
        !DnsClient::verify_response_case(&response, "Example.com."),
        "missing question section must fail 0x20 verification"
    );
}

#[test]
fn verify_response_case_passes_on_exact_case_match() {
    let response = response_with_question("Example.com.");
    assert!(DnsClient::verify_response_case(&response, "Example.com."));
}

#[test]
fn verify_response_case_fails_on_case_mismatch() {
    let response = response_with_question("example.com.");
    assert!(!DnsClient::verify_response_case(&response, "Example.com."));
}

#[tokio::test]
async fn send_udp_concurrent_first_connect_keeps_all_queries_successful() {
    let server_socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind test UDP server");
    let server_addr = server_socket.local_addr().expect("server addr");
    let received = Arc::new(AtomicUsize::new(0));
    {
        let received = received.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 2048];
            loop {
                let Ok((len, peer)) = server_socket.recv_from(&mut buf).await else {
                    break;
                };
                received.fetch_add(1, Ordering::SeqCst);
                let Ok(query) = Message::from_vec(&buf[..len]) else {
                    continue;
                };
                let mut response = Message::new();
                response.set_id(query.id());
                response.set_message_type(MessageType::Response);
                if let Some(q) = query.queries().first() {
                    response.add_query(q.clone());
                }
                let Ok(bytes) = response.to_vec() else {
                    continue;
                };
                let _ = server_socket.send_to(&bytes, peer).await;
            }
        });
    }

    let client = Arc::new(DnsClient::new(DnsClientConfig::default()));
    let barrier = Arc::new(tokio::sync::Barrier::new(8));
    let mut handles = Vec::new();
    for _ in 0..8 {
        let client = client.clone();
        let barrier = barrier.clone();
        handles.push(tokio::spawn(async move {
            let mut query = Message::new();
            query.set_recursion_desired(true);
            let name = Name::from_str("concurrent.example.com.").expect("valid name");
            query.add_query(Query::query(name, RecordType::A));
            barrier.wait().await;
            client.send_udp(server_addr, &query).await
        }));
    }

    for handle in handles {
        let response = handle
            .await
            .expect("task join")
            .expect("concurrent first-connect send_udp must succeed");
        assert_eq!(response.queries().len(), 1);
        assert_eq!(
            response.queries()[0].name().to_ascii(),
            "concurrent.example.com."
        );
    }
    assert_eq!(received.load(Ordering::SeqCst), 8);
}
