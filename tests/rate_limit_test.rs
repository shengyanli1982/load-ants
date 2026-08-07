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
