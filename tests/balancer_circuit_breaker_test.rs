use loadants::config::{DoHContentType, DoHMethod, DoHUpstreamServerConfig, UpstreamServerConfig};
use loadants::metrics::METRICS;
use loadants::{LoadBalancer, RoundRobinBalancer};
use reqwest::Url;
use std::time::Duration;

const UNHEALTHY_STATE: i64 = 0;
const HALF_OPEN_STATE: i64 = 1;
const HEALTHY_STATE: i64 = 2;

fn doh_server(url: &str) -> UpstreamServerConfig {
    UpstreamServerConfig::Doh(DoHUpstreamServerConfig {
        url: Url::parse(url).expect("valid URL"),
        weight: 1,
        method: DoHMethod::Get,
        content_type: DoHContentType::Message,
        auth: None,
    })
}

fn transition_count(group: &str, from: &str, to: &str) -> u64 {
    METRICS
        .circuit_breaker_transitions_total
        .with_label_values(&[group, from, to])
        .get()
}

fn health_state_gauge(group: &str, server_label: &str) -> i64 {
    METRICS
        .upstream_health_state
        .with_label_values(&[group, server_label])
        .get()
}

#[tokio::test]
async fn healthy_to_unhealthy_transition_is_recorded_with_gauge_zero() {
    let group = "cb-test-h2u";
    let server_url = "https://cb-h2u.example/dns-query";
    let server = doh_server(server_url);
    let balancer = RoundRobinBalancer::with_params(group, vec![server.clone()], 2, 60);

    let before = transition_count(group, "healthy", "unhealthy");

    balancer.report_failure(&server, group);
    assert_eq!(
        transition_count(group, "healthy", "unhealthy"),
        before,
        "first failure must not trip the breaker"
    );

    balancer.report_failure(&server, group);
    assert_eq!(
        transition_count(group, "healthy", "unhealthy"),
        before + 1,
        "reaching max failures must record H->U"
    );
    assert_eq!(health_state_gauge(group, server_url), UNHEALTHY_STATE);
}

#[tokio::test]
async fn cooldown_probe_success_records_u_to_ho_then_ho_to_h() {
    let group = "cb-test-probe-ok";
    let server_url = "https://cb-probe-ok.example/dns-query";
    let server = doh_server(server_url);
    let balancer = RoundRobinBalancer::with_params(group, vec![server.clone()], 1, 1);

    let before_h2u = transition_count(group, "healthy", "unhealthy");
    let before_u2ho = transition_count(group, "unhealthy", "half_open");
    let before_ho2h = transition_count(group, "half_open", "healthy");
    let before_u2h = transition_count(group, "unhealthy", "healthy");

    balancer.report_failure(&server, group);
    assert_eq!(
        transition_count(group, "healthy", "unhealthy"),
        before_h2u + 1
    );
    assert_eq!(health_state_gauge(group, server_url), UNHEALTHY_STATE);

    tokio::time::sleep(Duration::from_millis(1300)).await;

    let selected = balancer
        .select_server(&[])
        .expect("half-open server must be selectable after cooldown");
    assert_eq!(
        transition_count(group, "unhealthy", "half_open"),
        before_u2ho + 1,
        "time-driven U->HO must be recorded once"
    );
    assert_eq!(health_state_gauge(group, server_url), HALF_OPEN_STATE);

    balancer.report_success(selected, group);
    assert_eq!(
        transition_count(group, "half_open", "healthy"),
        before_ho2h + 1,
        "successful probe must record HO->H with half_open as the from-state"
    );
    assert_eq!(
        transition_count(group, "unhealthy", "healthy"),
        before_u2h,
        "phantom U->H transition must not be recorded"
    );
    assert_eq!(health_state_gauge(group, server_url), HEALTHY_STATE);
}

#[tokio::test]
async fn cooldown_probe_failure_records_u_to_ho_then_ho_to_u() {
    let group = "cb-test-probe-fail";
    let server_url = "https://cb-probe-fail.example/dns-query";
    let server = doh_server(server_url);
    let balancer = RoundRobinBalancer::with_params(group, vec![server.clone()], 1, 1);

    let before_h2u = transition_count(group, "healthy", "unhealthy");
    let before_u2ho = transition_count(group, "unhealthy", "half_open");
    let before_ho2u = transition_count(group, "half_open", "unhealthy");

    balancer.report_failure(&server, group);
    assert_eq!(
        transition_count(group, "healthy", "unhealthy"),
        before_h2u + 1
    );

    tokio::time::sleep(Duration::from_millis(1300)).await;

    let selected = balancer
        .select_server(&[])
        .expect("half-open server must be selectable after cooldown");
    assert_eq!(
        transition_count(group, "unhealthy", "half_open"),
        before_u2ho + 1,
        "time-driven U->HO must be recorded once"
    );
    assert_eq!(health_state_gauge(group, server_url), HALF_OPEN_STATE);

    balancer.report_failure(selected, group);
    assert_eq!(
        transition_count(group, "half_open", "unhealthy"),
        before_ho2u + 1,
        "failed probe must record HO->U, not get lost as U->U"
    );
    assert_eq!(health_state_gauge(group, server_url), UNHEALTHY_STATE);
}
