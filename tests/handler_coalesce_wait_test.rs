//! D-04 / D-19 回归测试（RequestHandler coalesce 层）。
//!
//! - D-04: follower 的 coalesce 等待时长必须从运行时配置派生
//!   （= leader 最坏转发总 deadline + margin），而非 5s 编译期常量：
//!   * 配置派生值 > 5s 时，follower 在 6s 处仍应等待（修复前 5s 即 Timeout）；
//!   * 配置派生值 < 5s 时，follower 应在派生值处 Timeout（修复前固定等 5s）。
//! - D-19: leader 路由错误提前返回时必须把 Err 写入 coalesce cell，
//!   follower 立即得到相同错误，而非 "Coalescing cell not set"。
//!
//! 全部用例使用 `start_paused` 虚拟时间，且只依赖 tokio 定时器/锁同步
//! （leader 被路由写锁挂起，转发永不发生）——避免 paused time 的
//! auto-advance 与真实 socket IO 送达之间的竞态导致虚拟时钟被提前快进。
//! leader 成功后 follower 复用结果的端到端路径由 tests/coalesce_test.rs
//! （真实时间 + gated UDP 上游）覆盖。

use hickory_proto::{
    op::{Message, MessageType, OpCode, Query},
    rr::{Name, RecordType},
};
use loadants::{
    config::{
        DnsClientConfig, DnsUpstreamServerConfig, HttpClientConfig, LoadBalancingStrategy,
        UpstreamGroupConfig, UpstreamScheme, UpstreamServerConfig,
    },
    AppError, DnsCache, MatchType, RequestHandler, RouteAction, RouteRuleConfig, Router,
    UpstreamManager,
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// 永不被拨号的占位上游地址（leader 在路由阶段即被挂起/失败，不会真正转发）。
const DUMMY_UPSTREAM_ADDR: SocketAddr = SocketAddr::new(
    std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)),
    59999,
);

fn make_query_msg(name: &str, id: u16) -> Message {
    let mut msg = Message::new();
    msg.set_id(id);
    msg.set_message_type(MessageType::Query);
    msg.set_op_code(OpCode::Query);
    msg.set_recursion_desired(true);
    msg.add_query(Query::query(Name::from_ascii(name).unwrap(), RecordType::A));
    msg
}

async fn make_dns_upstream(group_name: &str, request_timeout: u64) -> Arc<UpstreamManager> {
    let groups = vec![UpstreamGroupConfig {
        name: group_name.to_string(),
        scheme: UpstreamScheme::Dns,
        strategy: LoadBalancingStrategy::RoundRobin,
        servers: vec![UpstreamServerConfig::Dns(DnsUpstreamServerConfig {
            addr: DUMMY_UPSTREAM_ADDR,
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
        connect_timeout: 1,
        request_timeout,
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

fn make_router(domain: &str, group_name: &str) -> Arc<RwLock<Arc<Router>>> {
    Arc::new(RwLock::new(Arc::new(
        Router::new(vec![RouteRuleConfig {
            match_type: MatchType::Exact,
            patterns: vec![domain.to_string()],
            action: RouteAction::Forward,
            target: Some(group_name.to_string()),
        }])
        .unwrap(),
    )))
}

/// 轮询等待 leader 在 coalesce map 中注册（虚拟时间下用短 sleep 让出执行权；
/// 主任务始终持有最早的定时器，auto-advance 不会越过它快进）。
async fn wait_until_leader_registered(handler: &RequestHandler) {
    for _ in 0..100 {
        if handler.coalesce_pending() == 1 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "leader never registered in coalesce map (pending = {})",
        handler.coalesce_pending()
    );
}

/// D-04（正向）：request_timeout=10s、1 台服务器 → 派生 coalesce 等待 = 10s + margin = 11s > 5s。
///
/// leader 被路由写锁挂起（cell 未设置）。修复前 follower 固定 5s 超时：
/// 虚拟时间推进到 6.1s 时 follower 已完成并返回 AppError::Timeout
/// （恰在最需要合并保护时失效）。修复后 follower 在 6.1s 处仍在等待；
/// 锁释放后 leader 以路由错误结束，follower 立即通过 cell 收到镜像错误
/// （而非自身的等待超时）。
#[tokio::test(start_paused = true)]
async fn test_follower_waits_beyond_legacy_5s_when_config_derives_longer() {
    let upstream = make_dns_upstream("slow-group", 10).await;
    let cache = Arc::new(DnsCache::new(100, 60, 300, Some(60), None));
    // 路由表不含查询域名：写锁释放后 leader 将以路由错误快速结束。
    let router = make_router("unrelated.test", "slow-group");

    let router_write = router.write().await;
    let handler = Arc::new(RequestHandler::new(cache, router.clone(), upstream));

    let leader_request = make_query_msg("slow-leader.test.", 101);
    let leader_handler = handler.clone();
    let leader = tokio::spawn(async move { leader_handler.handle_request(&leader_request).await });

    wait_until_leader_registered(&handler).await;

    let follower_request = make_query_msg("slow-leader.test.", 202);
    let follower_handler = handler.clone();
    let follower =
        tokio::spawn(async move { follower_handler.handle_request(&follower_request).await });

    // 让 follower 完成注册并进入等待。
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 越过旧常量 5s：修复前 follower 此刻已 Timeout 结束；修复后应仍在等待 leader。
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert!(
        !follower.is_finished(),
        "follower must keep waiting past the legacy 5s constant when the config-derived \
         coalesce wait (request_timeout × servers + margin = 11s) has not elapsed"
    );
    assert!(
        !leader.is_finished(),
        "leader must still be blocked on the router write lock at t≈6.1s"
    );

    // 释放写锁 → leader 路由错误结束并写 cell → follower 立即收到镜像错误
    // （早于自身 11s 派生等待，证明结果经由 cell 复用而非超时）。
    drop(router_write);

    let (leader_result, follower_result) =
        tokio::time::timeout(Duration::from_secs(5), async move {
            (leader.await.unwrap(), follower.await.unwrap())
        })
        .await
        .expect("leader/follower must both finish promptly after the route error");

    assert!(
        leader_result.is_err(),
        "leader should fail with a route error after the lock is released"
    );
    match follower_result {
        Err(AppError::Internal(msg)) => {
            assert!(
                msg.contains("Route matching failed"),
                "follower must receive the leader's mirrored route error, got: {msg}"
            );
        }
        Err(AppError::Timeout) => {
            panic!("follower must not resolve via its own wait timeout (derived wait = 11s)")
        }
        other => panic!("follower should get Internal error mirroring the leader, got: {other:?}"),
    }
    assert_eq!(handler.coalesce_pending(), 0);
}

/// D-04（反向）：request_timeout=1s、1 台服务器 → 派生 coalesce 等待 = 1s + margin ≈ 2s < 5s。
///
/// leader 被路由写锁挂起（不经过 forward，因此不受 forward deadline 约束）。
/// 修复后 follower 应在派生等待（≈2s）处 Timeout；修复前固定等 5s，
/// 虚拟时间 2.5s 处 follower 仍在等待 → 证明等待时长确实随配置派生值变化。
#[tokio::test(start_paused = true)]
async fn test_follower_times_out_at_derived_wait_shorter_than_legacy() {
    let upstream = make_dns_upstream("derived-wait-group", 1).await;
    let cache = Arc::new(DnsCache::new(100, 60, 300, Some(60), None));
    // 路由表中没有该域名的规则：锁释放后 leader 将以路由错误快速结束。
    let router = make_router("unrelated.test", "derived-wait-group");

    // 持有写锁，把 leader 挂在 find_route_match 的 router.read() 上。
    let router_write = router.write().await;

    let handler = Arc::new(RequestHandler::new(cache, router.clone(), upstream));

    let leader_request = make_query_msg("derived-wait.test.", 301);
    let leader_handler = handler.clone();
    let leader = tokio::spawn(async move { leader_handler.handle_request(&leader_request).await });

    wait_until_leader_registered(&handler).await;

    let follower_request = make_query_msg("derived-wait.test.", 401);
    let follower_handler = handler.clone();
    let follower =
        tokio::spawn(async move { follower_handler.handle_request(&follower_request).await });

    // 让 follower 完成注册并进入等待。
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 派生等待 = 1s(deadline) + 1s(margin) = 2s：推进到 2.5s。
    // 修复后 follower 已在 ≈2s 处 Timeout；修复前（5s 常量）此刻仍在等待。
    tokio::time::sleep(Duration::from_millis(2400)).await;
    assert!(
        follower.is_finished(),
        "follower must time out at the config-derived wait (≈2s), not the legacy 5s constant"
    );
    let follower_result = follower.await.unwrap();
    assert!(
        matches!(follower_result, Err(AppError::Timeout)),
        "follower timeout must surface as AppError::Timeout, got: {:?}",
        follower_result.err()
    );

    // 释放写锁让 leader 走完（路由错误路径），避免任务悬挂。
    drop(router_write);
    let leader_result = tokio::time::timeout(Duration::from_secs(5), leader)
        .await
        .expect("leader must finish after the write lock is released")
        .unwrap();
    assert!(
        leader_result.is_err(),
        "leader should fail with a route error after the lock is released"
    );
    assert_eq!(handler.coalesce_pending(), 0);
}

/// D-19：leader 因路由错误提前返回时，必须把 Err 写入 coalesce cell，
/// follower 立即收到相同错误；修复前 cell 未设置，follower 收到误导性的
/// "Coalescing cell not set"（或空等满超时）。
#[tokio::test(start_paused = true)]
async fn test_leader_route_error_propagates_to_follower() {
    let upstream = make_dns_upstream("route-err-group", 5).await;
    let cache = Arc::new(DnsCache::new(100, 60, 300, Some(60), None));
    // 路由表不含目标域名 → find_match 返回 Err(NoRouteMatch) → leader 提前返回。
    let router = make_router("unrelated.test", "route-err-group");

    let router_write = router.write().await;
    let handler = Arc::new(RequestHandler::new(cache, router.clone(), upstream));

    let leader_request = make_query_msg("no-route.test.", 501);
    let leader_handler = handler.clone();
    let leader = tokio::spawn(async move { leader_handler.handle_request(&leader_request).await });

    wait_until_leader_registered(&handler).await;

    let follower_request = make_query_msg("no-route.test.", 601);
    let follower_handler = handler.clone();
    let follower =
        tokio::spawn(async move { follower_handler.handle_request(&follower_request).await });

    // follower 进入等待后再放行 leader 的路由错误。
    tokio::time::sleep(Duration::from_millis(100)).await;
    drop(router_write);

    let (leader_result, follower_result) =
        tokio::time::timeout(Duration::from_secs(5), async move {
            (leader.await.unwrap(), follower.await.unwrap())
        })
        .await
        .expect("leader/follower must both finish promptly after the route error");

    let leader_err = leader_result.expect_err("leader must fail with a route error");
    let follower_err = follower_result.expect_err("follower must receive the same failure");

    let leader_msg = leader_err.to_string();
    assert!(
        leader_msg.contains("Route matching failed"),
        "leader error should be the route failure, got: {leader_msg}"
    );

    let follower_msg = match &follower_err {
        AppError::Internal(msg) => msg.clone(),
        other => panic!("follower should get Internal error mirroring the leader, got: {other:?}"),
    };
    assert!(
        !follower_msg.contains("Coalescing cell not set"),
        "follower must not receive the misleading 'Coalescing cell not set' error, got: {follower_msg}"
    );
    assert!(
        follower_msg.contains("Route matching failed"),
        "follower must immediately receive the leader's route error, got: {follower_msg}"
    );
    assert_eq!(handler.coalesce_pending(), 0);
}
