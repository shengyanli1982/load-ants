//! D-04 / D-15 回归测试（upstream manager 层）。
//!
//! - D-04: `UpstreamManager::forward` 必须有覆盖全部 attempts 的总 deadline
//!   （= request_timeout × server_count，从运行时配置派生），超时返回 `AppError::Timeout`。
//! - D-15: `deny_answers` 非法 CIDR 必须在构造期 fail fast（Err），错误信息含非法值原文；
//!   合法配置零行为变化。

use hickory_proto::{
    op::{Message, MessageType, OpCode, Query},
    rr::{Name, RecordType},
};
use loadants::{
    config::{
        DnsClientConfig, DnsUpstreamServerConfig, DoHContentType, DoHMethod,
        DoHUpstreamServerConfig, HttpClientConfig, LoadBalancingStrategy, UpstreamGroupConfig,
        UpstreamScheme, UpstreamServerConfig,
    },
    r#const::coalesce_limits,
    AppError, UpstreamManager,
};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::net::UdpSocket;

fn make_query_msg(name: &str) -> Message {
    let mut msg = Message::new();
    msg.set_id(1);
    msg.set_message_type(MessageType::Query);
    msg.set_op_code(OpCode::Query);
    msg.set_recursion_desired(true);
    msg.add_query(Query::query(Name::from_ascii(name).unwrap(), RecordType::A));
    msg
}

/// 绑定一个从不回应的 UDP socket（黑洞上游）。
/// 必须保持 socket 绑定存活，否则 ICMP port-unreachable 会让请求立即失败，
/// 无法触发 hickory 的 per-request 超时路径。
async fn spawn_blackhole_udp() -> SocketAddr {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    // 泄漏 socket 到 'static 任务，保证测试期间端口始终有绑定但永不回应。
    tokio::spawn(async move {
        let _keep = socket;
        std::future::pending::<()>().await;
    });
    addr
}

fn make_dns_group(name: &str, servers: Vec<SocketAddr>) -> UpstreamGroupConfig {
    UpstreamGroupConfig {
        name: name.to_string(),
        scheme: UpstreamScheme::Dns,
        strategy: LoadBalancingStrategy::RoundRobin,
        servers: servers
            .into_iter()
            .map(|addr| UpstreamServerConfig::Dns(DnsUpstreamServerConfig { addr, weight: 1 }))
            .collect(),
        retry: None,
        proxy: None,
        tls_verify: None,
        deny_answers: vec![],
        case_randomization: false,
        case_randomization_strict: false,
    }
}

fn make_dns_config(request_timeout: u64) -> DnsClientConfig {
    DnsClientConfig {
        connect_timeout: 1,
        request_timeout,
        prefer_tcp: false,
        idle_connection_timeout: 30,
        tcp_reconnect: true,
        max_tcp_connections: 16,
    }
}

/// D-04 核心：2 台黑洞服务器 × request_timeout=1s。
///
/// 修复前无总 deadline：每台服务器最坏消耗 2×request_timeout（UDP 首发超时 + 重连重试），
/// forward 总耗时 ≈ 4s 后返回最后一台的错误。
/// 修复后总 deadline = request_timeout × server_count = 2s，超时即 Err(AppError::Timeout)。
#[tokio::test(start_paused = true)]
async fn test_forward_total_deadline_caps_all_attempts() {
    let addr1 = spawn_blackhole_udp().await;
    let addr2 = spawn_blackhole_udp().await;

    let manager = UpstreamManager::new(
        vec![make_dns_group("deadline-group", vec![addr1, addr2])],
        HttpClientConfig::default(),
        make_dns_config(1),
    )
    .await
    .expect("manager construction should succeed with valid config");

    let query = make_query_msg("deadline.test.");
    let start = tokio::time::Instant::now();
    let result = manager.forward(&query, "deadline-group").await;
    let elapsed = start.elapsed();

    assert!(
        result.is_err(),
        "forward against blackhole upstreams must fail, got: {:?}",
        result.as_ref().ok().map(|m| m.response_code())
    );
    assert!(
        elapsed < Duration::from_secs(3),
        "forward must be capped by the total deadline (request_timeout × server_count = 2s), \
         but took {elapsed:?} (unbounded per-server retry chain)"
    );
    assert!(
        matches!(result, Err(AppError::Timeout)),
        "total deadline expiry must surface as AppError::Timeout, got: {:?}",
        result.err()
    );
}

/// D-15：非法 CIDR 必须在构造期返回 Err（fail fast），错误信息包含非法值原文。
#[tokio::test]
async fn test_deny_answers_invalid_cidr_fails_fast() {
    let addr = spawn_blackhole_udp().await;
    let mut group = make_dns_group("deny-invalid", vec![addr]);
    group.deny_answers = vec!["10.0.0.0/8".to_string(), "1.1.1.1/33".to_string()];

    let result = UpstreamManager::new(
        vec![group],
        HttpClientConfig::default(),
        DnsClientConfig::default(),
    )
    .await;

    match result {
        Ok(_) => panic!(
            "invalid deny_answers CIDR '1.1.1.1/33' must be rejected at construction time \
             (fail fast), but UpstreamManager::new returned Ok"
        ),
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("1.1.1.1/33"),
                "error message must contain the offending CIDR literal, got: {msg}"
            );
            assert!(
                msg.contains("deny-invalid"),
                "error message must identify the upstream group, got: {msg}"
            );
        }
    }
}

/// D-15 回归守卫：合法 CIDR 配置零行为变化（构造成功）。
#[tokio::test]
async fn test_deny_answers_valid_cidrs_still_accepted() {
    let addr = spawn_blackhole_udp().await;
    let mut group = make_dns_group("deny-valid", vec![addr]);
    group.deny_answers = vec!["1.1.1.1/32".to_string(), "10.0.0.0/8".to_string()];

    UpstreamManager::new(
        vec![group],
        HttpClientConfig::default(),
        DnsClientConfig::default(),
    )
    .await
    .expect("valid deny_answers CIDRs must be accepted without behavior change");
}

// ============================================================================
// 以下测试为本轮新增回归测试（D-04 总转发 deadline 派生），并非迁移自 src
// 内嵌测试（src/upstream/manager.rs 从无内嵌测试）。
// ============================================================================

fn dns_group(name: &str, server_count: usize) -> UpstreamGroupConfig {
    UpstreamGroupConfig {
        name: name.to_string(),
        scheme: UpstreamScheme::Dns,
        strategy: LoadBalancingStrategy::RoundRobin,
        servers: (0..server_count)
            .map(|i| {
                UpstreamServerConfig::Dns(DnsUpstreamServerConfig {
                    addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 5300 + i as u16),
                    weight: 1,
                })
            })
            .collect(),
        retry: None,
        proxy: None,
        tls_verify: None,
        deny_answers: vec![],
        case_randomization: false,
        case_randomization_strict: false,
    }
}

fn doh_group(name: &str, server_count: usize) -> UpstreamGroupConfig {
    UpstreamGroupConfig {
        name: name.to_string(),
        scheme: UpstreamScheme::Doh,
        strategy: LoadBalancingStrategy::RoundRobin,
        servers: (0..server_count)
            .map(|i| {
                UpstreamServerConfig::Doh(DoHUpstreamServerConfig {
                    url: format!("https://192.0.2.{}", i + 1).parse().unwrap(),
                    weight: 1,
                    method: DoHMethod::Get,
                    content_type: DoHContentType::Message,
                    auth: None,
                })
            })
            .collect(),
        retry: None,
        proxy: None,
        tls_verify: None,
        deny_answers: vec![],
        case_randomization: false,
        case_randomization_strict: false,
    }
}

fn dns_config_with_timeout(request_timeout: u64) -> DnsClientConfig {
    DnsClientConfig {
        request_timeout,
        ..DnsClientConfig::default()
    }
}

fn http_config_with_timeout(request_timeout: u64) -> HttpClientConfig {
    HttpClientConfig {
        request_timeout,
        ..HttpClientConfig::default()
    }
}

/// D-04: DNS 组 deadline = dns_client.request_timeout × 服务器台数。
#[tokio::test]
async fn forward_deadline_derives_from_dns_request_timeout_times_servers() {
    let manager = UpstreamManager::new(
        vec![dns_group("dns-g", 2)],
        HttpClientConfig::default(),
        dns_config_with_timeout(3),
    )
    .await
    .unwrap();
    assert_eq!(manager.max_forward_deadline(), Duration::from_secs(6));
    assert_eq!(
        manager.groups["dns-g"].forward_deadline,
        Duration::from_secs(6)
    );
}

/// D-04: DoH 组 deadline = http_client.request_timeout × 服务器台数；
/// 多组时取最坏值（coalesce 等待按最坏组派生）。
#[tokio::test]
async fn forward_deadline_takes_worst_group_across_schemes() {
    let manager = UpstreamManager::new(
        vec![doh_group("doh-g", 4), dns_group("dns-g", 2)],
        http_config_with_timeout(10),
        dns_config_with_timeout(3),
    )
    .await
    .unwrap();
    assert_eq!(
        manager.groups["doh-g"].forward_deadline,
        Duration::from_secs(40)
    );
    assert_eq!(
        manager.groups["dns-g"].forward_deadline,
        Duration::from_secs(6)
    );
    assert_eq!(manager.max_forward_deadline(), Duration::from_secs(40));
}

/// 无上游组时返回兜底 deadline（与旧 HTTP 默认请求超时一致），
/// 保证 empty()/空配置下 coalesce 行为与修复前兼容。
#[tokio::test]
async fn forward_deadline_falls_back_without_groups() {
    let fallback = Duration::from_secs(coalesce_limits::FALLBACK_FORWARD_DEADLINE_SECS);
    assert_eq!(fallback, Duration::from_secs(5));

    let empty = UpstreamManager::empty().unwrap();
    assert_eq!(empty.max_forward_deadline(), fallback);

    let no_groups = UpstreamManager::new(
        vec![],
        HttpClientConfig::default(),
        DnsClientConfig::default(),
    )
    .await
    .unwrap();
    assert_eq!(no_groups.max_forward_deadline(), fallback);
}
