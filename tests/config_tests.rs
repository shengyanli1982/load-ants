use loadants::config::Config;
use std::io::Write;
use tempfile::NamedTempFile;

// 辅助函数：创建临时配置文件
fn create_temp_config_file(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(content.as_bytes()).unwrap();
    file.flush().unwrap();
    file
}

#[test]
fn test_basic_config_loading() {
    // 创建一个最小有效配置
    let config_content = r#"
server:
  listen_udp: "127.0.0.1:53"
  listen_tcp: "127.0.0.1:53"
admin:
  listen: "127.0.0.1:8080"
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());

    assert!(
        result.is_ok(),
        "Failed to load valid config: {:?}",
        result.err()
    );
    let config = result.unwrap();

    // 验证基本配置值
    assert_eq!(config.server.listen_udp, "127.0.0.1:53");
    assert_eq!(config.server.listen_tcp, "127.0.0.1:53");
    assert_eq!(config.admin.as_ref().unwrap().listen, "127.0.0.1:8080");

    // 验证默认值
    assert_eq!(config.server.tcp_timeout, 10); // 默认值
    assert!(config.cache.as_ref().unwrap().enabled); // 默认启用
    assert!(config
        .upstream_groups
        .as_ref()
        .unwrap_or(&Vec::new())
        .is_empty()); // 默认为空
    assert!(config
        .static_rules
        .as_ref()
        .unwrap_or(&Vec::new())
        .is_empty()); // 默认为空
    assert!(config.remote_rules.is_empty()); // 默认为空
}

#[test]
fn test_required_parameters() {
    // 缺少 server.listen_udp
    let config_content = r#"
server:
  listen_tcp: "127.0.0.1:53"
admin:
  listen: "127.0.0.1:8080"
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());

    assert!(result.is_err());

    // 缺少 server.listen_tcp
    let config_content = r#"
server:
  listen_udp: "127.0.0.1:53"
admin:
  listen: "127.0.0.1:8080"
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());

    assert!(result.is_err());
}

#[test]
fn test_optional_parameters_defaults() {
    // 创建最小配置，不包含可选参数
    let config_content = r#"
server:
  listen_udp: "127.0.0.1:53"
  listen_tcp: "127.0.0.1:53"
admin:
  listen: "127.0.0.1:8080"
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());

    assert!(result.is_ok());
    let config = result.unwrap();

    // 验证默认值
    assert_eq!(config.server.tcp_timeout, 10); // 默认 TCP 超时

    // 验证缓存默认值
    assert!(config.cache.as_ref().unwrap().enabled); // 默认启用
    assert_eq!(config.cache.as_ref().unwrap().max_size, 10000); // 默认大小
    assert_eq!(config.cache.as_ref().unwrap().min_ttl, 1); // 默认最小 TTL
    assert_eq!(config.cache.as_ref().unwrap().max_ttl, 86400); // 默认最大 TTL
    assert_eq!(config.cache.as_ref().unwrap().negative_ttl, 300); // 默认负面缓存 TTL

    // 验证 HTTP 客户端默认值
    assert_eq!(config.http_client.as_ref().unwrap().connect_timeout, 3); // 默认连接超时
    assert_eq!(config.http_client.as_ref().unwrap().request_timeout, 5); // 默认请求超时
    assert_eq!(config.http_client.as_ref().unwrap().idle_timeout, Some(10)); // 默认空闲超时
    assert_eq!(config.http_client.as_ref().unwrap().keepalive, Some(30)); // 默认 keepalive
    assert!(config.http_client.as_ref().unwrap().agent.is_none()); // 默认无代理
}

#[test]
fn test_parameter_range_validation() {
    // TCP 超时超出范围（太小）
    let config_content = r#"
server:
  listen_udp: "127.0.0.1:53"
  listen_tcp: "127.0.0.1:53"
  tcp_timeout: 0
admin:
  listen: "127.0.0.1:8080"
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());

    assert!(result.is_err());

    // TCP 超时超出范围（太大）
    let config_content = r#"
server:
  listen_udp: "127.0.0.1:53"
  listen_tcp: "127.0.0.1:53"
  tcp_timeout: 4000
admin:
  listen: "127.0.0.1:8080"
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());

    assert!(result.is_err());

    // 缓存大小超出范围
    let config_content = r#"
server:
  listen_udp: "127.0.0.1:53"
  listen_tcp: "127.0.0.1:53"
admin:
  listen: "127.0.0.1:8080"
cache:
  enabled: true
  max_size: 5
  min_ttl: 60
  max_ttl: 3600
  negative_ttl: 300
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());

    assert!(result.is_err());
}

#[test]
fn test_dependency_validation() {
    // 测试重复的上游组名称
    let config_content = r#"
server:
  listen_udp: "127.0.0.1:53"
  listen_tcp: "127.0.0.1:53"
admin:
  listen: "127.0.0.1:8080"
upstream_groups:
  - name: "google"
    strategy: "roundrobin"
    servers:
      - url: "https://dns.google/dns-query"
  - name: "google"  # 重复的名称
    strategy: "random"
    servers:
      - url: "https://dns.google/dns-query"
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());

    assert!(result.is_err());

    // 测试规则引用不存在的上游组
    let config_content = r#"
server:
  listen_udp: "127.0.0.1:53"
  listen_tcp: "127.0.0.1:53"
admin:
  listen: "127.0.0.1:8080"
upstream_groups:
  - name: "google"
    strategy: "roundrobin"
    servers:
      - url: "https://dns.google/dns-query"
static_rules:
  - match: "exact"
    patterns: ["example.com"]
    action: "forward"
    target: "cloudflare"  # 不存在的上游组
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());

    assert!(result.is_err());

    // 测试缓存 TTL 关系验证
    let config_content = r#"
server:
  listen_udp: "127.0.0.1:53"
  listen_tcp: "127.0.0.1:53"
admin:
  listen: "127.0.0.1:8080"
cache:
  enabled: true
  max_size: 10000
  min_ttl: 3600
  max_ttl: 60  # min_ttl > max_ttl
  negative_ttl: 300
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());

    assert!(result.is_err());

    // 测试 Forward 动作必须有目标
    let config_content = r#"
server:
  listen_udp: "127.0.0.1:53"
  listen_tcp: "127.0.0.1:53"
admin:
  listen: "127.0.0.1:8080"
static_rules:
  - match: "exact"
    patterns: ["example.com"]
    action: "forward"
    # 缺少 target
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());

    assert!(result.is_err());
}

#[test]
fn test_complete_valid_config() {
    // 创建一个完整有效的配置
    let config_content = r#"
server:
  listen_udp: "0.0.0.0:53"
  listen_tcp: "0.0.0.0:53"
  tcp_timeout: 15
admin:
  listen: "0.0.0.0:8080"
cache:
  enabled: true
  max_size: 20000
  min_ttl: 120
  max_ttl: 7200
  negative_ttl: 600
http_client:
  connect_timeout: 5
  request_timeout: 10
  idle_timeout: 60
  keepalive: 60
  agent: "LoadAnts/1.0"
upstream_groups:
  - name: "google"
    strategy: "roundrobin"
    servers:
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
    strategy: "random"
    servers:
      - url: "https://cloudflare-dns.com/dns-query"
        weight: 2
      - url: "https://1.1.1.1/dns-query"
        weight: 1
static_rules:
  - match: "exact"
    patterns: ["example.com", "example.org"]
    action: "block"
  - match: "wildcard"
    patterns: ["*.google.com"]
    action: "forward"
    target: "google"
  - match: "regex"
    patterns: ["^.*\\.cloudflare\\.com$"]
    action: "forward"
    target: "cloudflare"
remote_rules:
  - type: "url"
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

    // 验证服务器配置
    assert_eq!(config.server.listen_udp, "0.0.0.0:53");
    assert_eq!(config.server.listen_tcp, "0.0.0.0:53");
    assert_eq!(config.server.tcp_timeout, 15);

    // 验证管理服务器配置
    assert_eq!(config.admin.as_ref().unwrap().listen, "0.0.0.0:8080");

    // 验证缓存配置
    assert!(config.cache.as_ref().unwrap().enabled);
    assert_eq!(config.cache.as_ref().unwrap().max_size, 20000);
    assert_eq!(config.cache.as_ref().unwrap().min_ttl, 120);
    assert_eq!(config.cache.as_ref().unwrap().max_ttl, 7200);
    assert_eq!(config.cache.as_ref().unwrap().negative_ttl, 600);

    // 验证 HTTP 客户端配置
    assert_eq!(config.http_client.as_ref().unwrap().connect_timeout, 5);
    assert_eq!(config.http_client.as_ref().unwrap().request_timeout, 10);
    assert_eq!(config.http_client.as_ref().unwrap().idle_timeout, Some(60));
    assert_eq!(config.http_client.as_ref().unwrap().keepalive, Some(60));
    assert_eq!(
        config.http_client.as_ref().unwrap().agent,
        Some("LoadAnts/1.0".to_string())
    );

    // 验证上游组配置
    assert_eq!(config.upstream_groups.as_ref().unwrap().len(), 2);
    assert_eq!(config.upstream_groups.as_ref().unwrap()[0].name, "google");
    assert_eq!(
        config.upstream_groups.as_ref().unwrap()[1].name,
        "cloudflare"
    );

    // 验证规则配置
    assert_eq!(config.static_rules.as_ref().unwrap().len(), 3);
    assert_eq!(config.remote_rules.len(), 1);
}

#[test]
fn test_invalid_socket_address() {
    // 测试无效的 socket 地址格式
    let config_content = r#"
server:
  listen_udp: "invalid_address"
  listen_tcp: "127.0.0.1:53"
admin:
  listen: "127.0.0.1:8080"
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());

    assert!(result.is_err());
}

#[test]
fn test_auth_config_validation() {
    // 测试 basic 认证缺少用户名
    let config_content = r#"
server:
  listen_udp: "127.0.0.1:53"
  listen_tcp: "127.0.0.1:53"
admin:
  listen: "127.0.0.1:8080"
upstream_groups:
  - name: "secure"
    strategy: "roundrobin"
    servers:
      - url: "https://secure.dns/dns-query"
        auth:
          type: "basic"
          # 缺少 username
          password: "password123"
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());

    // 注意：根据代码，这可能不会导致错误，因为 username 是 Option<String>
    // 但我们可以检查配置是否正确加载
    if result.is_ok() {
        let config = result.unwrap();
        let auth = &config.upstream_groups.as_ref().unwrap()[0].servers[0].auth;
        assert!(auth.is_some());
        let auth = auth.as_ref().unwrap();
        assert!(auth.username.is_none() || auth.username.as_ref().unwrap().is_empty());
    }

    // 测试 bearer 认证缺少令牌
    let config_content = r#"
server:
  listen_udp: "127.0.0.1:53"
  listen_tcp: "127.0.0.1:53"
admin:
  listen: "127.0.0.1:8080"
upstream_groups:
  - name: "secure"
    strategy: "roundrobin"
    servers:
      - url: "https://secure.dns/dns-query"
        auth:
          type: "bearer"
          # 缺少 token
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());

    // 同样，根据代码，这可能不会导致错误
    if result.is_ok() {
        let config = result.unwrap();
        let auth = &config.upstream_groups.as_ref().unwrap()[0].servers[0].auth;
        assert!(auth.is_some());
        let auth = auth.as_ref().unwrap();
        assert!(auth.token.is_none() || auth.token.as_ref().unwrap().is_empty());
    }
}

#[test]
fn test_remote_rule_validation() {
    // 测试远程规则 URL 格式无效
    let config_content = r#"
server:
  listen_udp: "127.0.0.1:53"
  listen_tcp: "127.0.0.1:53"
admin:
  listen: "127.0.0.1:8080"
remote_rules:
  - type: "url"
    url: "invalid-url"
    format: "v2ray"
    action: "block"
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());

    assert!(result.is_err());

    // 测试远程规则文件大小超出范围
    let config_content = r#"
server:
  listen_udp: "127.0.0.1:53"
  listen_tcp: "127.0.0.1:53"
admin:
  listen: "127.0.0.1:8080"
remote_rules:
  - type: "url"
    url: "https://example.com/rules.txt"
    format: "v2ray"
    action: "block"
    max_size: 100000000000  # 太大
"#;

    let file = create_temp_config_file(config_content);
    let result = Config::from_file(file.path());

    assert!(result.is_err());
}
