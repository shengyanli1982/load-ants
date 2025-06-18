// tests/http_doh_tests.rs

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hickory_proto::{
    op::{Message, MessageType, OpCode, Query},
    rr::{Name, RecordType},
};
use reqwest::{Client, StatusCode};
use std::net::TcpListener;
use std::str::FromStr;
use std::time::Duration;
use tokio::process::Command;

// 测试 RFC 8484 DoH GET 请求
#[tokio::test]
async fn test_doh_get_request() {
    // 启动测试服务器
    let _server_guard = start_test_server().await;

    // 创建 DNS 查询消息
    let query = build_test_query("example.com", RecordType::A);

    // 创建 HTTP 客户端
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    // 编码查询消息为 base64url
    let mut query_bytes = Vec::with_capacity(512);
    query.to_vec(&mut query_bytes).unwrap();
    let query_base64 = URL_SAFE_NO_PAD.encode(&query_bytes);

    // 发送 DoH GET 请求
    let response = client
        .get(format!(
            "http://127.0.0.1:8053/dns-query?dns={}",
            query_base64
        ))
        .header("accept", "application/dns-message")
        .send()
        .await
        .unwrap();

    // 检查响应状态码
    assert_eq!(response.status(), StatusCode::OK);

    // 检查内容类型
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "application/dns-message"
    );

    // 读取响应体
    let response_body = response.bytes().await.unwrap();
    assert!(!response_body.is_empty());

    // 解析 DNS 响应消息
    let dns_response = Message::from_vec(&response_body).expect("Failed to parse DNS response");
    assert_eq!(dns_response.message_type(), MessageType::Response);
    assert_eq!(dns_response.op_code(), OpCode::Query);
    assert_eq!(
        dns_response.queries().first().map(|q| q.name().to_string()),
        Some("example.com.".to_string())
    );
}

// 测试 RFC 8484 DoH POST 请求
#[tokio::test]
async fn test_doh_post_request() {
    // 启动测试服务器
    let _server_guard = start_test_server().await;

    // 创建 DNS 查询消息
    let query = build_test_query("example.org", RecordType::A);

    // 创建 HTTP 客户端
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    // 将查询消息转换为字节
    let mut query_bytes = Vec::with_capacity(512);
    query.to_vec(&mut query_bytes).unwrap();

    // 发送 DoH POST 请求
    let response = client
        .post("http://127.0.0.1:8053/dns-query")
        .header("content-type", "application/dns-message")
        .header("accept", "application/dns-message")
        .body(query_bytes)
        .send()
        .await
        .unwrap();

    // 检查响应状态码
    assert_eq!(response.status(), StatusCode::OK);

    // 检查内容类型
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "application/dns-message"
    );

    // 读取响应体
    let response_body = response.bytes().await.unwrap();
    assert!(!response_body.is_empty());

    // 解析 DNS 响应消息
    let dns_response = Message::from_vec(&response_body).expect("Failed to parse DNS response");
    assert_eq!(dns_response.message_type(), MessageType::Response);
    assert_eq!(
        dns_response.queries().first().map(|q| q.name().to_string()),
        Some("example.org.".to_string())
    );
}

// 测试 Google JSON DoH 请求
#[tokio::test]
async fn test_json_doh_request() {
    // 启动测试服务器
    let _server_guard = start_test_server().await;

    // 创建 HTTP 客户端
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    // 发送 JSON DoH 请求
    let response = client
        .get("http://127.0.0.1:8053/resolve?name=example.net&type=1")
        .header("accept", "application/dns-json")
        .send()
        .await
        .unwrap();

    // 检查响应状态码
    assert_eq!(response.status(), StatusCode::OK);

    // 检查内容类型
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "application/dns-json"
    );

    // 读取并解析 JSON 响应
    let json_response: serde_json::Value = response.json().await.unwrap();

    // 基本验证
    assert!(json_response.is_object());
    assert!(json_response.get("Status").is_some());
    assert!(json_response.get("Question").is_some());

    // 检查查询内容
    let question = &json_response["Question"][0];
    assert_eq!(question["name"], "example.net.");
    assert_eq!(question["type"], 1);
}

// 构建测试用 DNS 查询
fn build_test_query(domain: &str, record_type: RecordType) -> Message {
    let mut query = Message::new();
    query.set_message_type(MessageType::Query);
    query.set_op_code(OpCode::Query);
    query.set_recursion_desired(true);

    let name = Name::from_str(&format!("{}.", domain)).unwrap();
    let q = Query::query(name, record_type);
    query.add_query(q);

    query
}

// 查找可用的端口
#[allow(dead_code)]
fn find_available_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind to random port");
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

// 启动测试服务器
async fn start_test_server() -> impl Drop {
    // 创建测试配置
    let test_config = format!(
        r#"
server:
  listen_udp: "127.0.0.1:8053"
  listen_tcp: "127.0.0.1:8053"
  listen_http: "127.0.0.1:8053"
  tcp_timeout: 10
  http_timeout: 10

cache:
  enabled: true
  max_size: 1000
  min_ttl: 60
  max_ttl: 3600
  negative_ttl: 300

# HTTP client settings
http_client:
  connect_timeout: 5
  request_timeout: 5
  idle_timeout: 30
  keepalive: 30

# Default upstream group
upstream_groups:
  - name: "default"
    strategy: "random"
    servers:
      - url: "https://dns.google/dns-query"
      - url: "https://cloudflare-dns.com/dns-query"

# Routing rules
rules:
  - match: "*"
    action: "forward"
    target: "default"
"#
    );

    // 写入临时配置文件
    let tmp_dir = tempfile::tempdir().unwrap();
    let config_path = tmp_dir.path().join("test_config.yaml");
    tokio::fs::write(&config_path, test_config).await.unwrap();

    // 启动服务器进程
    let mut server_process = Command::new(env!("CARGO_BIN_EXE_load-ants"))
        .arg("--config")
        .arg(&config_path)
        .kill_on_drop(true)
        .spawn()
        .expect("Failed to start test server");

    // 等待服务器启动
    tokio::time::sleep(Duration::from_secs(2)).await;

    // 返回一个 guard，当它被丢弃时将关闭服务器
    struct ServerGuard {
        _tmp_dir: tempfile::TempDir,
        _process: tokio::process::Child,
    }

    impl Drop for ServerGuard {
        fn drop(&mut self) {
            let _ = self._process.start_kill();
        }
    }

    ServerGuard {
        _tmp_dir: tmp_dir,
        _process: server_process,
    }
}
