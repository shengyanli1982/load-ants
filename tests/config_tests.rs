use loadants::r#const::{remote_rule_limits, server_defaults};
use loadants::Config;
use std::io::Write;
use std::path::PathBuf;
use tempfile::NamedTempFile;

fn create_temp_config_file(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(content.as_bytes()).unwrap();
    file.flush().unwrap();
    file
}

#[test]
fn test_repo_config_files_loading_and_runtime_requirements() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for file in ["config.example.yaml", "config.default.yaml"] {
        let path = root.join(file);
        let result = Config::from_file(&path);
        assert!(
            result.is_ok(),
            "Failed to load {:?}: {:?}",
            path,
            result.err()
        );
        let config = result.unwrap();
        assert!(
            config.validate_runtime_requirements().is_ok(),
            "Runtime validation failed for {:?}",
            path
        );
    }
}

#[test]
fn test_basic_config_loading() {
    let config_content = r#"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
admin:
  listen: "127.0.0.1:8080"
rules:
  static:
    - match: "wildcard"
      patterns: ["*"]
      action: "block"
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());

    assert!(
        result.is_ok(),
        "Failed to load valid config: {:?}",
        result.err()
    );
    let config = result.unwrap();

    assert_eq!(config.listeners.udp, "127.0.0.1:53");
    assert_eq!(config.listeners.tcp, "127.0.0.1:53");
    assert_eq!(
        config.listeners.tcp_idle_timeout,
        server_defaults::DEFAULT_TCP_TIMEOUT
    );
    assert_eq!(
        config.listeners.http_idle_timeout,
        server_defaults::DEFAULT_HTTP_TIMEOUT
    );

    assert!(config.admin.is_some());
    assert_eq!(config.admin.as_ref().unwrap().listen, "127.0.0.1:8080");

    assert_eq!(config.rules.r#static.len(), 1);
    assert!(config.rules.remote.is_empty());
}

#[test]
fn test_required_parameters() {
    let config_content = r#"
listeners:
  tcp: "127.0.0.1:53"
"#;
    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(result.is_err());

    let config_content = r#"
listeners:
  udp: "127.0.0.1:53"
"#;
    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(result.is_err());
}

#[test]
fn test_parameter_range_validation() {
    let config_content = r#"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
  tcp_idle_timeout: 0
"#;
    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(result.is_err());

    let config_content = r#"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
  tcp_idle_timeout: 100000
"#;
    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(
        result.is_err(),
        "Expected error for tcp_idle_timeout too large"
    );

    let config_content = r#"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
cache:
  enabled: true
  size: 5
  ttl:
    min: 60
    max: 3600
    negative: 300
"#;
    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(
        result.is_err(),
        "Expected error for cache.size that is too small"
    );

    let config_content = r#"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
http:
  connect_timeout: 5
  request_timeout: 10
  idle_timeout: 1
"#;
    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(
        result.is_err(),
        "Expected error for http.idle_timeout too small"
    );
}

#[test]
fn test_dependency_validation() {
    let config_content = r#"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
upstreams:
  - name: "google"
    policy: "roundrobin"
    endpoints:
      - url: "https://dns.google/dns-query"
  - name: "google"
    policy: "random"
    endpoints:
      - url: "https://dns.google/dns-query"
"#;
    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(result.is_err());

    let config_content = r#"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
upstreams:
  - name: "google"
    policy: "roundrobin"
    endpoints:
      - url: "https://dns.google/dns-query"
rules:
  static:
    - match: "exact"
      patterns: ["example.com"]
      action: "forward"
      upstream: "cloudflare"
"#;
    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(result.is_err());

    let config_content = r#"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
cache:
  enabled: true
  size: 10000
  ttl:
    min: 3600
    max: 60
    negative: 300
"#;
    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(result.is_err());

    let config_content = r#"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
rules:
  static:
    - match: "exact"
      patterns: ["example.com"]
      action: "forward"
"#;
    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(result.is_err());
}

#[test]
fn test_bootstrap_fallback_failover_transport_validation() {
    let config_content = r#"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
bootstrap_dns:
  groups: ["public_dns"]
upstreams:
  - name: "public_dns"
    protocol: "dns"
    policy: "roundrobin"
    endpoints:
      - addr: 223.5.5.5:53
"#;
    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(result.is_ok(), "Expected valid bootstrap_dns config");

    let config_content = r#"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
bootstrap_dns:
  groups: ["missing_group"]
upstreams:
  - name: "public_dns"
    protocol: "dns"
    policy: "roundrobin"
    endpoints:
      - addr: 223.5.5.5:53
"#;
    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(
        result.is_err(),
        "Expected error for missing bootstrap group"
    );

    let config_content = r#"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
bootstrap_dns:
  groups: ["public"]
upstreams:
  - name: "public"
    protocol: "doh"
    policy: "roundrobin"
    endpoints:
      - url: "https://dns.google/dns-query"
"#;
    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(
        result.is_err(),
        "Expected error when bootstrap_dns references non-dns upstream"
    );

    let config_content = r#"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
upstreams:
  - name: "secure"
    protocol: "doh"
    policy: "roundrobin"
    endpoints:
      - url: "https://dns.google/dns-query"
    fallback: "missing"
"#;
    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(result.is_err(), "Expected error for missing fallback group");

    let config_content = r#"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
upstreams:
  - name: "secure"
    protocol: "doh"
    policy: "roundrobin"
    endpoints:
      - url: "https://dns.google/dns-query"
    fallback: "secure"
"#;
    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(
        result.is_err(),
        "Expected error for self-referencing fallback"
    );

    let config_content = r#"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
upstreams:
  - name: "public_dns"
    protocol: "dns"
    policy: "roundrobin"
    endpoints:
      - addr: 223.5.5.5:53
        transport: "bogus"
"#;
    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(result.is_err(), "Expected error for invalid dns transport");

    let config_content = r#"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
upstreams:
  - name: "secure"
    protocol: "doh"
    policy: "roundrobin"
    endpoints:
      - url: "https://dns.google/dns-query"
    failover:
      on_rcode: ["bogus"]
"#;
    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(
        result.is_err(),
        "Expected error for invalid failover.on_rcode"
    );

    let config_content = r#"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
upstreams:
  - name: "secure"
    protocol: "doh"
    policy: "roundrobin"
    endpoints:
      - url: "https://dns.google/dns-query"
    failover:
      on_rcode: ["SERVFAIL"]
"#;
    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(result.is_ok(), "Expected normalized SERVFAIL to parse");
}

#[test]
fn test_complete_valid_config() {
    let config_content = r#"
listeners:
  udp: "0.0.0.0:53"
  tcp: "0.0.0.0:53"
  doh: "0.0.0.0:8080"
  tcp_idle_timeout: 15
  http_idle_timeout: 30
admin:
  listen: "0.0.0.0:9000"
cache:
  enabled: true
  size: 20000
  ttl:
    min: 120
    max: 7200
    negative: 600
http:
  connect_timeout: 5
  request_timeout: 10
  idle_timeout: 60
  keepalive: 60
  user_agent: "LoadAnts/1.0"
dns:
  connect_timeout: 2
  request_timeout: 3
  prefer_tcp: false
  tcp_reconnect: true
upstreams:
  - name: "google"
    protocol: "doh"
    policy: "roundrobin"
    endpoints:
      - url: "https://dns.google/dns-query"
        method: "post"
        content_type: "message"
      - url: "https://8.8.8.8/dns-query"
        method: "get"
        content_type: "json"
    retry:
      attempts: 3
      delay: 1
  - name: "cloudflare"
    protocol: "doh"
    policy: "random"
    endpoints:
      - url: "https://cloudflare-dns.com/dns-query"
        weight: 2
      - url: "https://1.1.1.1/dns-query"
        weight: 1
rules:
  static:
    - match: "exact"
      patterns: ["example.com", "example.org"]
      action: "block"
    - match: "wildcard"
      patterns: ["*.google.com"]
      action: "forward"
      upstream: "google"
    - match: "regex"
      patterns: ["^.*\\.cloudflare\\.com$"]
      action: "forward"
      upstream: "cloudflare"
  remote:
    - type: "http"
      url: "https://example.com/rules.txt"
      format: "v2ray"
      action: "block"
      max_size: 1048576
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());

    assert!(
        result.is_ok(),
        "Failed to load valid config: {:?}",
        result.err()
    );
    let config = result.unwrap();

    assert_eq!(config.listeners.udp, "0.0.0.0:53");
    assert_eq!(config.listeners.tcp, "0.0.0.0:53");
    assert_eq!(config.listeners.tcp_idle_timeout, 15);

    assert_eq!(config.admin.as_ref().unwrap().listen, "0.0.0.0:9000");

    assert!(config.cache.as_ref().unwrap().enabled);
    assert_eq!(config.cache.as_ref().unwrap().size, 20000);
    assert_eq!(config.cache.as_ref().unwrap().ttl.min, 120);
    assert_eq!(config.cache.as_ref().unwrap().ttl.max, 7200);
    assert_eq!(config.cache.as_ref().unwrap().ttl.negative, 600);

    assert_eq!(config.http.as_ref().unwrap().connect_timeout, 5);
    assert_eq!(config.http.as_ref().unwrap().request_timeout, 10);
    assert_eq!(config.http.as_ref().unwrap().idle_timeout, Some(60));
    assert_eq!(config.http.as_ref().unwrap().keepalive, Some(60));
    assert_eq!(
        config.http.as_ref().unwrap().user_agent,
        Some("LoadAnts/1.0".to_string())
    );

    assert_eq!(config.upstreams.as_ref().unwrap().len(), 2);
    assert_eq!(config.rules.r#static.len(), 3);
    assert_eq!(config.rules.remote.len(), 1);
}

#[test]
fn test_invalid_socket_address() {
    let config_content = r#"
listeners:
  udp: "invalid_address"
  tcp: "127.0.0.1:53"
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(result.is_err());
}

#[test]
fn test_auth_config_validation() {
    let config_content = r#"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
upstreams:
  - name: "secure"
    policy: "roundrobin"
    endpoints:
      - url: "https://secure.dns/dns-query"
        auth:
          type: "basic"
          password: "password123"
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(
        result.is_err(),
        "Expected error for basic auth missing username"
    );

    let config_content = r#"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
upstreams:
  - name: "secure"
    policy: "roundrobin"
    endpoints:
      - url: "https://secure.dns/dns-query"
        auth:
          type: "bearer"
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(
        result.is_err(),
        "Expected error for bearer auth missing token"
    );
}

#[test]
fn test_remote_rule_validation() {
    let config_content = r#"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
rules:
  remote:
    - type: "http"
      url: "invalid-url"
      format: "v2ray"
      action: "block"
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(result.is_err());

    let config_content = format!(
        r#"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
rules:
  remote:
    - type: "http"
      url: "https://example.com/rules.txt"
      format: "v2ray"
      action: "block"
      max_size: {}
"#,
        remote_rule_limits::MAX_SIZE + 1
    );

    let file = create_temp_config_file(&config_content);
    let result = Config::from_file(file.path());
    assert!(result.is_err());
}

#[test]
fn test_enum_value_normalization() {
    let config_content = r#"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
upstreams:
  - name: "public"
    protocol: "DoH"
    policy: "round_robin"
    endpoints:
      - url: "https://dns.google/dns-query"
        method: "POST"
        content_type: "MESSAGE"
rules:
  static:
    - match: "wild-card"
      patterns: ["*"]
      action: "FORWARD"
      upstream: "public"
  remote:
    - type: "HTTP"
      url: "https://example.com/rules.txt"
      format: "V2Ray"
      action: "BLOCK"
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(result.is_ok(), "Expected normalized enum values to parse");

    let config = result.unwrap();
    assert_eq!(config.upstreams.as_ref().unwrap()[0].name, "public");
    assert_eq!(config.rules.r#static.len(), 1);
    assert_eq!(config.rules.remote.len(), 1);
}

#[test]
fn test_deny_unknown_fields() {
    let config_content = r#"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
  unexpected: "nope"
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(
        result.is_err(),
        "Expected error for unknown field in listeners"
    );

    let config_content = r#"
server:
  listen_udp: "127.0.0.1:53"
  listen_tcp: "127.0.0.1:53"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(
        result.is_err(),
        "Expected error for unknown top-level field"
    );
}

#[test]
fn test_remote_rule_type_url_rejected() {
    let config_content = r#"
listeners:
  udp: "127.0.0.1:53"
  tcp: "127.0.0.1:53"
rules:
  remote:
    - type: "url"
      url: "https://example.com/rules.txt"
      format: "v2ray"
      action: "block"
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());
    assert!(
        result.is_err(),
        "Expected RemoteRuleType=url to be rejected"
    );
}
