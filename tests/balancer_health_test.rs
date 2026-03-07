use loadants::config::{DnsUpstreamEndpointConfig, HealthConfig, UpstreamEndpointConfig};
use loadants::{LoadBalancer, RandomBalancer, RoundRobinBalancer, WeightedBalancer};
use std::net::SocketAddr;
use tokio::time::{advance, Duration};

fn dns_endpoint(addr: SocketAddr, weight: u32) -> UpstreamEndpointConfig {
    UpstreamEndpointConfig::Dns(DnsUpstreamEndpointConfig {
        addr,
        weight,
        transport: None,
    })
}

#[tokio::test(start_paused = true)]
async fn test_round_robin_cooldown_skip_and_recover() {
    let a = dns_endpoint("127.0.0.1:53".parse().unwrap(), 1);
    let b = dns_endpoint("127.0.0.2:53".parse().unwrap(), 1);

    let balancer = RoundRobinBalancer::new(
        vec![a.clone(), b.clone()],
        Some(HealthConfig {
            failure_threshold: 1,
            cooldown_seconds: 10,
            success_reset: false,
        }),
    );

    let s1 = balancer.select_server().await.unwrap();
    assert_eq!(s1, &a);

    balancer.report_failure(&a).await;

    let s2 = balancer.select_server().await.unwrap();
    assert_eq!(s2, &b);

    // cooldown 未过期时，a 不可选
    let s3 = balancer.select_server().await.unwrap();
    assert_eq!(s3, &b);

    // 推进时间让 cooldown 过期，a 恢复可选
    advance(Duration::from_secs(11)).await;
    let s4 = balancer.select_server().await.unwrap();
    assert_eq!(s4, &a);
}

#[tokio::test(start_paused = true)]
async fn test_success_reset_clears_cooldown() {
    let a = dns_endpoint("127.0.0.1:53".parse().unwrap(), 1);

    let balancer = RoundRobinBalancer::new(
        vec![a.clone()],
        Some(HealthConfig {
            failure_threshold: 1,
            cooldown_seconds: 60,
            success_reset: true,
        }),
    );

    let s1 = balancer.select_server().await.unwrap();
    assert_eq!(s1, &a);

    balancer.report_failure(&a).await;
    assert!(balancer.select_server().await.is_err());

    // success_reset 应立即解除 cooldown
    balancer.report_success(&a).await;
    let s2 = balancer.select_server().await.unwrap();
    assert_eq!(s2, &a);
}

#[tokio::test(start_paused = true)]
async fn test_weighted_and_random_skip_cooldown() {
    let a = dns_endpoint("127.0.0.1:53".parse().unwrap(), 10);
    let b = dns_endpoint("127.0.0.2:53".parse().unwrap(), 1);

    let health = Some(HealthConfig {
        failure_threshold: 1,
        cooldown_seconds: 60,
        success_reset: false,
    });

    // Weighted：初次应选 weight 更高的 a；a 进入 cooldown 后应选 b
    let weighted = WeightedBalancer::new(vec![a.clone(), b.clone()], health.clone());
    assert_eq!(weighted.select_server().await.unwrap(), &a);
    weighted.report_failure(&a).await;
    assert_eq!(weighted.select_server().await.unwrap(), &b);

    // Random：a 进入 cooldown 后只能选到 b（确定性）
    let random = RandomBalancer::new(vec![a.clone(), b.clone()], health.clone());
    random.report_failure(&a).await;
    for _ in 0..5 {
        assert_eq!(random.select_server().await.unwrap(), &b);
    }
}
