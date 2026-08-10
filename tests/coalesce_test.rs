use hickory_proto::{
    op::{Message, MessageType, OpCode, Query, ResponseCode},
    rr::{rdata::A, Name, RData, Record, RecordType},
};
use loadants::{
    config::{
        DnsClientConfig, DnsUpstreamServerConfig, HttpClientConfig, LoadBalancingStrategy,
        UpstreamGroupConfig, UpstreamScheme, UpstreamServerConfig,
    },
    DnsCache, MatchType, RequestHandler, RouteAction, RouteRuleConfig, Router, UpstreamManager,
};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::{Notify, RwLock};

fn make_query_msg(name: &str, id: u16) -> Message {
    let mut msg = Message::new();
    msg.set_id(id);
    msg.set_message_type(MessageType::Query);
    msg.set_op_code(OpCode::Query);
    msg.set_recursion_desired(true);
    msg.add_query(Query::query(Name::from_ascii(name).unwrap(), RecordType::A));
    msg
}

fn make_response(request: &Message, ip: Ipv4Addr, ttl: u32) -> Message {
    let mut msg = Message::new();
    msg.set_id(request.id());
    msg.set_message_type(MessageType::Response);
    msg.set_op_code(request.op_code());
    msg.set_recursion_desired(request.recursion_desired());
    msg.set_recursion_available(true);
    msg.set_response_code(ResponseCode::NoError);
    for q in request.queries() {
        msg.add_query(q.clone());
    }
    if let Some(q) = request.queries().first() {
        msg.add_answer(Record::from_rdata(q.name().clone(), ttl, RData::A(A(ip))));
    }
    let mut header = *msg.header();
    header.set_answer_count(msg.answers().len() as u16);
    msg.set_header(header);
    msg
}

fn first_answer_ip(msg: &Message) -> Option<Ipv4Addr> {
    msg.answers().first().and_then(|r| match r.data() {
        RData::A(a) => Some(a.0),
        _ => None,
    })
}

async fn make_upstream(addr: SocketAddr) -> Arc<UpstreamManager> {
    let groups = vec![UpstreamGroupConfig {
        name: "coalesce-group".to_string(),
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
    let dns_config = DnsClientConfig {
        connect_timeout: 2,
        request_timeout: 5,
        prefer_tcp: false,
        idle_connection_timeout: 30,
        tcp_reconnect: true,
        max_tcp_connections: 16,
    };
    Arc::new(
        UpstreamManager::new(groups, HttpClientConfig::default(), dns_config)
            .await
            .unwrap(),
    )
}

fn make_router(domain: &str) -> Arc<RwLock<Arc<Router>>> {
    Arc::new(RwLock::new(Arc::new(
        Router::new(vec![RouteRuleConfig {
            match_type: MatchType::Exact,
            patterns: vec![domain.to_string()],
            action: RouteAction::Forward,
            target: Some("coalesce-group".to_string()),
        }])
        .unwrap(),
    )))
}

async fn wait_for_map_empty(handler: &RequestHandler) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while handler.coalesce_pending() > 0 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        handler.coalesce_pending(),
        0,
        "coalesce map must drain after in-flight requests complete"
    );
}

struct GatedUpstream {
    addr: SocketAddr,
    query_received: Arc<Notify>,
    respond_gate: Arc<Notify>,
}

async fn spawn_gated_udp_upstream() -> GatedUpstream {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    let socket = Arc::new(socket);
    let query_received = Arc::new(Notify::new());
    let respond_gate = Arc::new(Notify::new());

    tokio::spawn({
        let socket = socket.clone();
        let query_received = query_received.clone();
        let respond_gate = respond_gate.clone();
        async move {
            let mut buf = vec![0u8; 2048];
            let (len, peer) = socket.recv_from(&mut buf).await.unwrap();
            let request = Message::from_vec(&buf[..len]).unwrap();
            query_received.notify_one();
            respond_gate.notified().await;
            let response = make_response(&request, Ipv4Addr::new(1, 2, 3, 4), 600);
            socket
                .send_to(&response.to_vec().unwrap(), peer)
                .await
                .unwrap();
        }
    });

    GatedUpstream {
        addr,
        query_received,
        respond_gate,
    }
}

async fn spawn_udp_upstream(answer_ip: Ipv4Addr, ttl: u32) -> SocketAddr {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    let socket = Arc::new(socket);

    tokio::spawn({
        let socket = socket.clone();
        async move {
            let mut buf = vec![0u8; 2048];
            loop {
                let Ok((len, peer)) = socket.recv_from(&mut buf).await else {
                    return;
                };
                let Ok(request) = Message::from_vec(&buf[..len]) else {
                    return;
                };
                let response = make_response(&request, answer_ip, ttl);
                if socket
                    .send_to(&response.to_vec().unwrap(), peer)
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
    });

    addr
}

// multi_thread flavor is required: with the pre-fix deadlock one tokio worker
// blocks synchronously, so the watchdog timeout needs a second worker to fire.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_leader_follower_concurrent_queries_both_succeed() {
    let gated = spawn_gated_udp_upstream().await;
    let upstream = make_upstream(gated.addr).await;
    let cache = Arc::new(DnsCache::new(100, 60, 300, Some(60), None));
    let router = make_router("coalesce-a.test");
    let handler = Arc::new(RequestHandler::new(cache, router, upstream));

    let request1 = make_query_msg("coalesce-a.test.", 101);
    let leader_handler = handler.clone();
    let leader = tokio::spawn(async move { leader_handler.handle_request(&request1).await });

    gated.query_received.notified().await;

    let request2 = make_query_msg("coalesce-a.test.", 202);
    let follower_handler = handler.clone();
    let follower = tokio::spawn(async move { follower_handler.handle_request(&request2).await });

    tokio::time::sleep(Duration::from_millis(100)).await;
    gated.respond_gate.notify_one();

    let (leader_result, follower_result) =
        tokio::time::timeout(Duration::from_secs(8), async move {
            (leader.await.unwrap(), follower.await.unwrap())
        })
        .await
        .expect("concurrent leader/follower requests must not deadlock");

    let leader_response = leader_result.expect("leader request should succeed");
    let follower_response = follower_result.expect("follower request should succeed");

    assert_eq!(leader_response.response_code(), ResponseCode::NoError);
    assert_eq!(follower_response.response_code(), ResponseCode::NoError);
    assert_eq!(leader_response.id(), 101);
    assert_eq!(follower_response.id(), 202);
    assert_eq!(
        first_answer_ip(&leader_response),
        Some(Ipv4Addr::new(1, 2, 3, 4))
    );
    assert_eq!(
        first_answer_ip(&follower_response),
        Some(Ipv4Addr::new(1, 2, 3, 4))
    );

    wait_for_map_empty(&handler).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_background_refresh_insert_refresh_cleanup_then_no_hang() {
    let upstream_addr = spawn_udp_upstream(Ipv4Addr::new(2, 2, 2, 2), 60).await;
    let upstream = make_upstream(upstream_addr).await;
    let cache = Arc::new(DnsCache::new(100, 1, 10, Some(60), Some(60)));
    let router = make_router("refresh-b.test");

    let seed_request = make_query_msg("refresh-b.test.", 1);
    let seed_response = make_response(&seed_request, Ipv4Addr::new(1, 1, 1, 1), 1);
    cache.insert(&seed_request, seed_response).await.unwrap();

    tokio::time::sleep(Duration::from_millis(1200)).await;

    let handler = RequestHandler::new(cache, router, upstream);

    let stale_response = handler
        .handle_request(&make_query_msg("refresh-b.test.", 11))
        .await
        .expect("stale entry should be served immediately");
    assert_eq!(stale_response.response_code(), ResponseCode::NoError);
    assert_eq!(
        first_answer_ip(&stale_response),
        Some(Ipv4Addr::new(1, 1, 1, 1)),
        "first response should be the stale cached entry"
    );

    wait_for_map_empty(&handler).await;

    let followup = tokio::time::timeout(
        Duration::from_secs(3),
        handler.handle_request(&make_query_msg("refresh-b.test.", 22)),
    )
    .await
    .expect("request after background refresh must not hang")
    .expect("request after background refresh should succeed");
    assert_eq!(followup.response_code(), ResponseCode::NoError);
    assert_eq!(
        first_answer_ip(&followup),
        Some(Ipv4Addr::new(2, 2, 2, 2)),
        "follow-up response should come from the completed background refresh"
    );
    assert_eq!(handler.coalesce_pending(), 0);
}
