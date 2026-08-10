use hickory_proto::op::{Message, MessageType, OpCode, Query, ResponseCode};
use hickory_proto::rr::{Name, RecordType};
use loadants::config::{
    DnsClientConfig, DnsUpstreamServerConfig, HttpClientConfig, LoadBalancingStrategy,
    UpstreamGroupConfig, UpstreamScheme, UpstreamServerConfig,
};
use loadants::upstream::UpstreamManager;
use std::net::{Ipv4Addr, SocketAddr};
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

/// 模拟伪造响应的 UDP 服务器：仅回显事务 ID，不携带 question section。
async fn spawn_udp_server_without_question(
    addr: SocketAddr,
    udp_count: Arc<AtomicUsize>,
) -> JoinHandle<()> {
    let socket = UdpSocket::bind(addr).await.unwrap();
    tokio::spawn(async move {
        let mut buf = [0u8; 2048];
        let recv = time::timeout(Duration::from_secs(2), socket.recv_from(&mut buf)).await;
        if let Ok(Ok((len, peer))) = recv {
            udp_count.fetch_add(1, Ordering::SeqCst);
            let query = Message::from_vec(&buf[..len]).unwrap();
            let mut response = Message::new();
            response
                .set_id(query.id())
                .set_message_type(MessageType::Response)
                .set_recursion_desired(true)
                .set_recursion_available(true)
                .set_op_code(query.op_code())
                .set_response_code(ResponseCode::NoError);
            let bytes = response.to_vec().unwrap();
            let _ = socket.send_to(&bytes, peer).await;
        }
    })
}

async fn spawn_tcp_server(listener: TcpListener, tcp_count: Arc<AtomicUsize>) -> JoinHandle<()> {
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
        let mut response = Message::new();
        response
            .set_id(query.id())
            .set_message_type(MessageType::Response)
            .set_recursion_desired(query.recursion_desired())
            .set_recursion_available(true)
            .set_op_code(query.op_code())
            .set_response_code(ResponseCode::NoError);
        if let Some(q) = query.queries().first() {
            response.add_query(q.clone());
        }
        let bytes = response.to_vec().unwrap();

        let len_prefix = (bytes.len() as u16).to_be_bytes();
        let _ = stream.write_all(&len_prefix).await;
        let _ = stream.write_all(&bytes).await;
    })
}

async fn build_dns_manager(
    addr: SocketAddr,
    dns_config: DnsClientConfig,
    case_randomization: bool,
    case_randomization_strict: bool,
) -> UpstreamManager {
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
        case_randomization,
        case_randomization_strict,
    }];

    UpstreamManager::new(groups, HttpClientConfig::default(), dns_config)
        .await
        .unwrap()
}

fn default_dns_config() -> DnsClientConfig {
    DnsClientConfig {
        connect_timeout: 1,
        request_timeout: 2,
        prefer_tcp: false,
        idle_connection_timeout: 30,
        tcp_reconnect: true,
        max_tcp_connections: 256,
    }
}

#[tokio::test]
async fn test_0x20_strict_missing_question_falls_back_to_tcp() {
    let tcp_count = Arc::new(AtomicUsize::new(0));
    let udp_count = Arc::new(AtomicUsize::new(0));

    let udp_socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let addr = udp_socket.local_addr().unwrap();
    drop(udp_socket);

    let tcp_listener = TcpListener::bind(addr).await.unwrap();

    let _udp = spawn_udp_server_without_question(addr, udp_count.clone()).await;
    let _tcp = spawn_tcp_server(tcp_listener, tcp_count.clone()).await;

    let manager = build_dns_manager(addr, default_dns_config(), true, true).await;

    let query = create_dns_query(300, "example.com");
    let response = manager.forward(&query, "dns_group").await.unwrap();

    assert_eq!(response.response_code(), ResponseCode::NoError);
    assert_eq!(udp_count.load(Ordering::SeqCst), 1);
    assert_eq!(
        tcp_count.load(Ordering::SeqCst),
        1,
        "missing question section must fail 0x20 verification and trigger TCP fallback"
    );
}

#[tokio::test]
async fn test_0x20_non_strict_missing_question_accepts_udp_response() {
    let tcp_count = Arc::new(AtomicUsize::new(0));
    let udp_count = Arc::new(AtomicUsize::new(0));

    let udp_socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let addr = udp_socket.local_addr().unwrap();
    drop(udp_socket);

    let _udp = spawn_udp_server_without_question(addr, udp_count.clone()).await;

    let manager = build_dns_manager(addr, default_dns_config(), true, false).await;

    let query = create_dns_query(301, "example.com");
    let response = manager.forward(&query, "dns_group").await.unwrap();

    assert_eq!(response.response_code(), ResponseCode::NoError);
    assert_eq!(udp_count.load(Ordering::SeqCst), 1);
    assert_eq!(
        tcp_count.load(Ordering::SeqCst),
        0,
        "non-strict mode must accept the UDP response without TCP fallback"
    );
}
