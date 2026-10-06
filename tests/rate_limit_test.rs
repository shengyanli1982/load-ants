use loadants::{RateLimitConfig, RateLimiter};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[tokio::test]
async fn test_rate_limiter_allows_within_limit() {
    let limiter = RateLimiter::new(10);
    let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    for _ in 0..10 {
        assert!(limiter.check(ip));
    }
}

#[tokio::test]
async fn test_rate_limiter_rejects_over_limit() {
    let limiter = RateLimiter::new(5);
    let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    for _ in 0..5 {
        assert!(limiter.check(ip));
    }
    assert!(!limiter.check(ip));
}

#[tokio::test]
async fn test_rate_limiter_per_ip_independent() {
    let limiter = RateLimiter::new(10);
    let ip1 = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    let ip2 = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2));
    assert!(limiter.check(ip1));
    assert!(limiter.check(ip1));
    assert!(limiter.check(ip2));
    assert!(limiter.check(ip2));
    assert!(limiter.check(ip1));
    assert!(limiter.check(ip2));
    assert!(limiter.check(ip1));
    assert!(limiter.check(ip2));
    assert!(limiter.check(ip1));
    assert!(limiter.check(ip2));
    assert!(!limiter.check(ip1));
    assert!(!limiter.check(ip2));
}

#[tokio::test]
async fn test_rate_limiter_ipv6() {
    let limiter = RateLimiter::new(3);
    let ip = IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1));
    for _ in 0..3 {
        assert!(limiter.check(ip));
    }
    assert!(!limiter.check(ip));
}

#[test]
fn test_rate_limit_config_default() {
    let config = RateLimitConfig::default();
    assert_eq!(config.max_requests_per_second, 100);
}

#[tokio::test]
async fn test_max_per_second_accessor() {
    let limiter = RateLimiter::new(200);
    assert_eq!(limiter.max_per_second(), 200);
}

/// D-07: 被 per-IP 限制拒绝的请求不得消耗全局配额。
/// 否则单个 IP 的滥用流量会先耗尽全局计数，令其他合法客户端全部被拒，
/// per-IP 限制形同虚设，限速器反而成为 DoS 放大器。
#[tokio::test]
async fn test_per_ip_rejections_do_not_consume_global_quota() {
    // 全局 100/s，per-IP 2/s
    let limiter = RateLimiter::new_with_per_ip(100, 2);
    let abuser = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));

    // abuser 前 2 次通过，消耗全局配额 2
    assert!(limiter.check(abuser));
    assert!(limiter.check(abuser));
    // abuser 后续请求被 per-IP 限制拒绝，且不应再消耗全局配额
    for _ in 0..50 {
        assert!(!limiter.check(abuser));
    }
    // 98 个不同的受害 IP（各自 per-IP 配额充足）应仍可使用剩余的 98 个全局配额
    for i in 1..=98u8 {
        let victim = IpAddr::V4(Ipv4Addr::new(10, 0, 2, i));
        assert!(limiter.check(victim), "victim 10.0.2.{i} should pass");
    }
    // 全局配额恰好耗尽：第三方 IP 被拒绝
    let other = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3));
    assert!(!limiter.check(other));
}

/// D-07 独立性（全局方向）: 全局配额耗尽时，即使 per-IP 配额充足也拒绝所有请求。
/// 回归护栏——验证 check() 顺序调整后全局限速语义不变。
#[tokio::test]
async fn test_global_limit_rejects_across_ips_when_exhausted() {
    // 全局 3/s，per-IP 100/s：全局是瓶颈
    let limiter = RateLimiter::new_with_per_ip(3, 100);
    for i in 1..=3u8 {
        assert!(limiter.check(IpAddr::V4(Ipv4Addr::new(10, 1, 0, i))));
    }
    // 新 IP 的 per-IP 配额充足，但全局已耗尽 → 仍拒绝
    assert!(!limiter.check(IpAddr::V4(Ipv4Addr::new(10, 1, 0, 4))));
}

/// D-07 独立性（per-IP 方向）: per-IP 配额耗尽时仅拒绝该 IP，全局有余量则其他 IP 不受影响。
/// 回归护栏——验证 check() 顺序调整后 per-IP 限速语义不变。
#[tokio::test]
async fn test_per_ip_limit_rejects_while_global_has_room() {
    let limiter = RateLimiter::new_with_per_ip(1000, 2);
    let ip = IpAddr::V4(Ipv4Addr::new(10, 2, 0, 1));
    assert!(limiter.check(ip));
    assert!(limiter.check(ip));
    // per-IP 耗尽，全局仍有大量余量
    assert!(!limiter.check(ip));
    // 其他 IP 不受影响
    assert!(limiter.check(IpAddr::V4(Ipv4Addr::new(10, 2, 0, 2))));
}

/// OPT-1a 并发正确性抽查（同 IP）：get-first 改造把命中路径从 entry() 独占写锁
/// 换成 get() 共享读锁 + AtomicU64 计数。本测试用多线程猛压同一 IP，验证并发
/// 原子计数不丢增量——若丢更新，放行数会逼近总尝试数（远超限额）；正确计数下
/// 放行数被 per-IP 限额约束（至多因跨秒窗口翻转放大数倍，仍远小于总尝试数）。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_concurrent_same_ip_count_integrity() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    const PER_IP_LIMIT: u32 = 1_000;
    const THREADS: usize = 8;
    const ATTEMPTS_PER_THREAD: usize = 2_500;
    const TOTAL_ATTEMPTS: usize = THREADS * ATTEMPTS_PER_THREAD; // 20_000 == 20 * limit

    // 全局配额设为 u32::MAX，隔离出纯 per-IP 计数路径。
    let limiter = RateLimiter::new_with_per_ip(u32::MAX, PER_IP_LIMIT);
    let ip = IpAddr::V4(Ipv4Addr::new(10, 9, 0, 1));
    let allowed = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(THREADS));

    std::thread::scope(|s| {
        for _ in 0..THREADS {
            let limiter = Arc::clone(&limiter);
            let allowed = Arc::clone(&allowed);
            let barrier = Arc::clone(&barrier);
            s.spawn(move || {
                barrier.wait(); // 同步起跑，最大化 shard 争用
                for _ in 0..ATTEMPTS_PER_THREAD {
                    if limiter.check(ip) {
                        allowed.fetch_add(1, Ordering::Relaxed);
                    }
                }
            });
        }
    });

    let allowed = allowed.load(Ordering::Relaxed);
    // 至少放行到限额（未过度拒绝）。
    assert!(
        allowed >= PER_IP_LIMIT as usize,
        "allowed {allowed} should be >= per-IP limit {PER_IP_LIMIT}"
    );
    // 远小于总尝试数：丢更新会使 allowed 逼近 TOTAL_ATTEMPTS(=20*limit)。
    // 阈值取 TOTAL_ATTEMPTS/2(=10*limit)，正确计数即使跨多秒窗口翻转也不会触及。
    assert!(
        allowed <= TOTAL_ATTEMPTS / 2,
        "allowed {allowed} indicates lost updates (total attempts {TOTAL_ATTEMPTS})"
    );
}

/// OPT-1a 并发正确性抽查（不同 IP）：多线程各自使用独立 IP 猛压 check，覆盖
/// get 未命中→entry() 独占插入的回退路径（新 IP 首查竞窗）。验证无 panic/死锁，
/// 且高配额下全部放行（并发插入不产生伪拒绝）。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_concurrent_distinct_ips_no_panic() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    const THREADS: usize = 8;
    const ATTEMPTS_PER_THREAD: usize = 2_000;

    // per-IP 与全局配额都拉满，任何 check 都应放行。
    let limiter = RateLimiter::new_with_per_ip(u32::MAX, u32::MAX);
    let allowed = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(THREADS));

    std::thread::scope(|s| {
        for t in 0..THREADS {
            let limiter = Arc::clone(&limiter);
            let allowed = Arc::clone(&allowed);
            let barrier = Arc::clone(&barrier);
            s.spawn(move || {
                barrier.wait();
                for i in 0..ATTEMPTS_PER_THREAD {
                    // 每线程独占 10.{t}.x.x 段，IP 互不重叠 → 全部走新 IP 插入回退路径。
                    let ip = IpAddr::V4(Ipv4Addr::new(
                        10,
                        t as u8,
                        ((i >> 8) & 0xff) as u8,
                        (i & 0xff) as u8,
                    ));
                    if limiter.check(ip) {
                        allowed.fetch_add(1, Ordering::Relaxed);
                    }
                }
            });
        }
    });

    let total = THREADS * ATTEMPTS_PER_THREAD;
    assert_eq!(
        allowed.load(Ordering::Relaxed),
        total,
        "all checks should be allowed under max quota (no spurious rejection from concurrent insert)"
    );
}
