use loadants::config::{
    DnsClientConfig, DnsUpstreamServerConfig, DoHContentType, DoHMethod, DoHUpstreamServerConfig,
    HttpClientConfig, LoadBalancingStrategy, RetryConfig, UpstreamGroupConfig, UpstreamScheme,
    UpstreamServerConfig,
};
use loadants::error::AppError;
use loadants::upstream::UpstreamManager;
use reqwest::Url;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

fn doh_config() -> UpstreamGroupConfig {
    UpstreamGroupConfig {
        name: "test-doh".to_string(),
        scheme: UpstreamScheme::Doh,
        strategy: LoadBalancingStrategy::RoundRobin,
        servers: vec![UpstreamServerConfig::Doh(DoHUpstreamServerConfig {
            url: Url::parse("https://doh.example.com/dns-query").unwrap(),
            weight: 1,
            method: DoHMethod::Post,
            content_type: DoHContentType::Message,
            auth: None,
        })],
        retry: Some(RetryConfig { attempts: 1, delay: 0 }),
        proxy: None,
    }
}

fn dns_config() -> UpstreamGroupConfig {
    UpstreamGroupConfig {
        name: "test-dns".to_string(),
        scheme: UpstreamScheme::Dns,
        strategy: LoadBalancingStrategy::RoundRobin,
        servers: vec![UpstreamServerConfig::Dns(DnsUpstreamServerConfig {
            addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 53),
            weight: 1,
        })],
        retry: Some(RetryConfig { attempts: 1, delay: 0 }),
        proxy: None,
    }
}

#[tokio::test]
async fn test_upstream_manager_creates_doh_and_dns_groups() {
    let manager = UpstreamManager::new(
        vec![doh_config(), dns_config()],
        HttpClientConfig::default(),
        DnsClientConfig::default(),
    )
    .await
    .expect("Should create UpstreamManager with both DoH and DNS groups");

    // 验证已知组存在：对不存在的组转发应返回 UpstreamGroupNotFound
    let query = {
        use hickory_proto::op::{Message, OpCode, Query};
        use hickory_proto::rr::{Name, RecordType};
        use std::str::FromStr;
        let mut msg = Message::new();
        msg.set_id(1);
        msg.set_op_code(OpCode::Query);
        msg.set_recursion_desired(true);
        let name = Name::from_str("example.com.").unwrap();
        msg.add_query(Query::query(name, RecordType::A));
        msg
    };

    // 不存在的组应返回 UpstreamGroupNotFound
    let result = manager.forward(&query, "non-existent").await;
    assert!(matches!(result, Err(AppError::UpstreamGroupNotFound(_))));

    // 已知组（test-doh / test-dns）存在，即使转发失败（无真实服务器），错误也不是 UpstreamGroupNotFound
    let result_doh = manager.forward(&query, "test-doh").await;
    assert!(!matches!(result_doh, Err(AppError::UpstreamGroupNotFound(_))));

    let result_dns = manager.forward(&query, "test-dns").await;
    assert!(!matches!(result_dns, Err(AppError::UpstreamGroupNotFound(_))));
}
