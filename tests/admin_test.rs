use hickory_proto::op::{Message, OpCode, Query};
use hickory_proto::rr::{Name, RecordType};
use loadants::config::{
    DnsClientConfig, DoHContentType, DoHMethod, DoHUpstreamServerConfig, HttpClientConfig,
    LoadBalancingStrategy, UpstreamGroupConfig, UpstreamScheme, UpstreamServerConfig,
};
use loadants::UpstreamManager;
use loadants::{AdminAuthConfig, AdminServer, DnsCache, RemoteSourceStatus};
use reqwest::Url;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::RwLock;

fn test_token() -> String {
    "test-secret-token".to_string()
}

async fn get_free_port() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap()
}

#[tokio::test]
async fn test_health_accessible_without_auth() {
    let addr = get_free_port().await;

    let server = AdminServer::new(addr)
        .with_cache(Arc::new(DnsCache::new(0, 0, 0, Some(0), None)))
        .with_auth(Some(AdminAuthConfig {
            token: test_token(),
        }));

    let handle = tokio::spawn(async move { server.start().await });

    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{}/health/ready", addr))
        .send()
        .await;
    assert!(resp.is_ok());
    let resp = resp.unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert!(body.get("cacheInfo").is_some());
    assert!(
        body.get("upstreams").is_none(),
        "upstreams field must be omitted when no upstream is configured"
    );

    handle.abort();
}

fn doh_group(name: &str, url: &str) -> UpstreamGroupConfig {
    UpstreamGroupConfig {
        name: name.to_string(),
        scheme: UpstreamScheme::Doh,
        strategy: LoadBalancingStrategy::RoundRobin,
        servers: vec![UpstreamServerConfig::Doh(DoHUpstreamServerConfig {
            url: Url::parse(url).expect("valid URL"),
            weight: 1,
            method: DoHMethod::Get,
            content_type: DoHContentType::Message,
            auth: None,
        })],
        retry: None,
        proxy: None,
        tls_verify: None,
        deny_answers: vec![],
        case_randomization: false,
        case_randomization_strict: false,
    }
}

fn dns_query_message() -> Message {
    let mut message = Message::new();
    message.set_id(4321);
    message.set_op_code(OpCode::Query);
    message.set_recursion_desired(true);
    let name = Name::from_ascii("health-check.example.com.").expect("valid name");
    message.add_query(Query::query(name, RecordType::A));
    message
}

#[tokio::test]
async fn test_health_ready_returns_503_when_any_group_fully_unhealthy() {
    let dead_addr = get_free_port().await;
    let groups = vec![
        doh_group("down_group", &format!("http://{}/dns-query", dead_addr)),
        doh_group("healthy_group", "https://healthy.example/dns-query"),
    ];
    let manager = UpstreamManager::new(
        groups,
        HttpClientConfig::default(),
        DnsClientConfig::default(),
    )
    .await
    .expect("upstream manager should build");

    let query = dns_query_message();
    for _ in 0..3 {
        let result = manager.forward(&query, "down_group").await;
        assert!(result.is_err(), "forwarding to the dead group must fail");
    }
    let summary = manager.health_summary();
    assert_eq!(summary["down_group"].healthy, 0);
    assert_eq!(summary["healthy_group"].healthy, 1);

    let addr = get_free_port().await;
    let server = AdminServer::new(addr)
        .with_cache(Arc::new(DnsCache::new(10, 60, 86400, Some(30), None)))
        .with_upstream(Arc::new(manager));

    let handle = tokio::spawn(async move { server.start().await });

    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{}/health/ready", addr))
        .send()
        .await
        .expect("health/ready request should succeed");
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::SERVICE_UNAVAILABLE,
        "one fully unhealthy group must force 503"
    );
    let body: serde_json::Value = resp.json().await.expect("JSON body");
    assert_eq!(body["status"], "unhealthy");
    assert_eq!(body["upstreams"]["down_group"]["healthy"], 0);
    assert_eq!(body["upstreams"]["healthy_group"]["status"], "ok");

    handle.abort();
}

#[tokio::test]
async fn test_cache_clear_no_token_returns_401() {
    let addr = get_free_port().await;

    let server = AdminServer::new(addr)
        .with_cache(Arc::new(DnsCache::new(10, 60, 86400, Some(30), None)))
        .with_auth(Some(AdminAuthConfig {
            token: test_token(),
        }));

    let handle = tokio::spawn(async move { server.start().await });

    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{}/api/cache/clear", addr))
        .send()
        .await;
    assert!(resp.is_ok());
    assert_eq!(resp.unwrap().status(), reqwest::StatusCode::UNAUTHORIZED);

    handle.abort();
}

#[tokio::test]
async fn test_cache_clear_wrong_token_returns_401() {
    let addr = get_free_port().await;

    let server = AdminServer::new(addr)
        .with_cache(Arc::new(DnsCache::new(10, 60, 86400, Some(30), None)))
        .with_auth(Some(AdminAuthConfig {
            token: test_token(),
        }));

    let handle = tokio::spawn(async move { server.start().await });

    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{}/api/cache/clear", addr))
        .header("Authorization", "Bearer wrong-token")
        .send()
        .await;
    assert!(resp.is_ok());
    assert_eq!(resp.unwrap().status(), reqwest::StatusCode::UNAUTHORIZED);

    handle.abort();
}

#[tokio::test]
async fn test_cache_clear_valid_token_succeeds() {
    let addr = get_free_port().await;

    let server = AdminServer::new(addr)
        .with_cache(Arc::new(DnsCache::new(10, 60, 86400, Some(30), None)))
        .with_auth(Some(AdminAuthConfig {
            token: test_token(),
        }));

    let handle = tokio::spawn(async move { server.start().await });

    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{}/api/cache/clear", addr))
        .header("Authorization", format!("Bearer {}", test_token()))
        .send()
        .await;
    assert!(resp.is_ok());
    let resp = resp.unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body = resp.text().await.unwrap();
    assert!(body.contains("success"));

    handle.abort();
}

#[tokio::test]
async fn test_cache_clear_no_auth_config_no_token_needed() {
    let addr = get_free_port().await;

    let server = AdminServer::new(addr)
        .with_cache(Arc::new(DnsCache::new(10, 60, 86400, Some(30), None)))
        .with_auth(None);

    let handle = tokio::spawn(async move { server.start().await });

    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{}/api/cache/clear", addr))
        .send()
        .await;
    assert!(resp.is_ok());
    let resp = resp.unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body = resp.text().await.unwrap();
    assert!(body.contains("success"));

    handle.abort();
}

#[tokio::test]
async fn test_cache_clear_invalid_auth_format_returns_401() {
    let addr = get_free_port().await;

    let server = AdminServer::new(addr)
        .with_cache(Arc::new(DnsCache::new(10, 60, 86400, Some(30), None)))
        .with_auth(Some(AdminAuthConfig {
            token: test_token(),
        }));

    let handle = tokio::spawn(async move { server.start().await });

    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{}/api/cache/clear", addr))
        .header("Authorization", "Basic user:pass")
        .send()
        .await;
    assert!(resp.is_ok());
    assert_eq!(resp.unwrap().status(), reqwest::StatusCode::UNAUTHORIZED);

    handle.abort();
}

#[tokio::test]
async fn test_routes_returns_total_rule_count() {
    let addr = get_free_port().await;

    let server = AdminServer::new(addr)
        .with_cache(Arc::new(DnsCache::new(10, 60, 86400, Some(30), None)))
        .with_remote_source_statuses(Arc::new(RwLock::new(Vec::new())));

    let handle = tokio::spawn(async move { server.start().await });

    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{}/api/routes", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.get("totalRuleCount").is_some());
    assert!(body.get("staticRuleCount").is_none());
    assert_eq!(body["totalRuleCount"], 0);
    assert!(body["remoteSources"].as_array().unwrap().is_empty());

    handle.abort();
}

#[tokio::test]
async fn test_routes_includes_remote_source_statuses() {
    let addr = get_free_port().await;

    let statuses = vec![
        RemoteSourceStatus {
            url: "https://example.com/rules.txt".to_string(),
            status: "ok".to_string(),
            last_updated: 1700000000,
            rule_count: 42,
            error_message: None,
        },
        RemoteSourceStatus {
            url: "https://broken.source/rules.txt".to_string(),
            status: "failed".to_string(),
            last_updated: 1700000100,
            rule_count: 0,
            error_message: Some("connection timeout".to_string()),
        },
    ];

    let server = AdminServer::new(addr)
        .with_cache(Arc::new(DnsCache::new(10, 60, 86400, Some(30), None)))
        .with_remote_source_statuses(Arc::new(RwLock::new(statuses)));

    let handle = tokio::spawn(async move { server.start().await });

    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{}/api/routes", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();

    let sources = body["remoteSources"].as_array().unwrap();
    assert_eq!(sources.len(), 2);

    let ok_source = &sources[0];
    assert_eq!(ok_source["url"], "https://example.com/rules.txt");
    assert_eq!(ok_source["status"], "ok");
    assert_eq!(ok_source["lastUpdated"], 1700000000);
    assert_eq!(ok_source["ruleCount"], 42);
    assert!(ok_source.get("errorMessage").is_none());

    let failed_source = &sources[1];
    assert_eq!(failed_source["url"], "https://broken.source/rules.txt");
    assert_eq!(failed_source["status"], "failed");
    assert_eq!(failed_source["lastUpdated"], 1700000100);
    assert_eq!(failed_source["ruleCount"], 0);
    assert_eq!(failed_source["errorMessage"], "connection timeout");

    handle.abort();
}
