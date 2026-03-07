use hickory_proto::op::{Message, MessageType, OpCode, Query, ResponseCode};
use hickory_proto::rr::{Name, RecordType};
use loadants::config::{
    DnsConfig, DnsTransportMode, DnsUpstreamEndpointConfig, HttpConfig, LoadBalancingPolicy,
    UpstreamEndpointConfig, UpstreamGroupConfig, UpstreamProtocol,
};
use loadants::upstream::UpstreamManager;
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

async fn build_dns_manager(
    addr: SocketAddr,
    dns_config: DnsConfig,
    transport: Option<DnsTransportMode>,
) -> UpstreamManager {
    let groups = vec![UpstreamGroupConfig {
        name: "dns_group".to_string(),
        protocol: UpstreamProtocol::Dns,
        policy: LoadBalancingPolicy::RoundRobin,
        endpoints: vec![UpstreamEndpointConfig::Dns(DnsUpstreamEndpointConfig {
            addr,
            weight: 1,
            transport,
        })],
        fallback: None,
        failover: None,
        health: None,
        retry: None,
        proxy: None,
    }];

    UpstreamManager::new(groups, HttpConfig::default(), dns_config)
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
        DnsConfig {
            connect_timeout: 1,
            request_timeout: 2,
            prefer_tcp: true,
            tcp_reconnect: true,
        },
        None,
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
        DnsConfig {
            connect_timeout: 1,
            request_timeout: 2,
            prefer_tcp: false,
            tcp_reconnect: true,
        },
        None,
    )
    .await;

    let query = create_dns_query(201, "example.com");
    let response = manager.forward(&query, "dns_group").await.unwrap();
    assert_eq!(response.response_code(), ResponseCode::NoError);
    assert_eq!(udp_count.load(Ordering::SeqCst), 1);
    assert_eq!(tcp_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_dns_transport_udp_does_not_retry_tcp_on_tc() {
    let tcp_count = Arc::new(AtomicUsize::new(0));
    let udp_count = Arc::new(AtomicUsize::new(0));

    let tcp_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = tcp_listener.local_addr().unwrap().port();
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);

    let _tcp = spawn_tcp_server(tcp_listener, tcp_count.clone(), ResponseCode::NoError).await;
    let _udp = spawn_udp_server(addr, udp_count.clone(), ResponseCode::NoError, true).await;

    let manager = build_dns_manager(
        addr,
        DnsConfig {
            connect_timeout: 1,
            request_timeout: 2,
            prefer_tcp: false,
            tcp_reconnect: true,
        },
        Some(DnsTransportMode::Udp),
    )
    .await;

    let query = create_dns_query(202, "example.com");
    let response = manager.forward(&query, "dns_group").await.unwrap();
    assert_eq!(response.response_code(), ResponseCode::NoError);
    assert!(
        response.truncated(),
        "Expected UDP transport to return truncated response"
    );
    assert_eq!(udp_count.load(Ordering::SeqCst), 1);
    assert_eq!(tcp_count.load(Ordering::SeqCst), 0);
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
        DnsConfig {
            connect_timeout: 1,
            request_timeout: 2,
            prefer_tcp: false,
            tcp_reconnect: true,
        },
        None,
    )
    .await;

    let query = create_dns_query(202, "nxdomain.example");
    let response = manager.forward(&query, "dns_group").await.unwrap();
    assert_eq!(response.response_code(), ResponseCode::NXDomain);
    assert_eq!(udp_count.load(Ordering::SeqCst), 1);
}
