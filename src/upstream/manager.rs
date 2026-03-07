use crate::{
    balancer::{LoadBalancer, RandomBalancer, RoundRobinBalancer, WeightedBalancer},
    config::{
        DnsConfig, DnsTransportMode, FailoverConfig, FailoverRcode, HttpConfig,
        LoadBalancingPolicy, RetryConfig, UpstreamEndpointConfig, UpstreamGroupConfig,
        UpstreamProtocol,
    },
    error::AppError,
    metrics::METRICS,
    r#const::{
        error_labels, protocol_labels, upstream_labels, upstream_protocol_labels,
        upstream_transport_labels,
    },
    upstream::{doh::DoHClient, http_client::HttpClient},
};
use hickory_proto::op::{Message, ResponseCode};
use reqwest_middleware::ClientWithMiddleware;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};
use tracing::{debug, error, info};

use super::dns_client::{DnsClient, DnsTransport};

// 上游管理器
pub struct UpstreamManager {
    // 上游组负载均衡器
    groups: HashMap<String, Arc<dyn LoadBalancer>>,
    // 上游组协议
    group_protocols: HashMap<String, UpstreamProtocol>,
    // 上游组客户端
    group_clients: HashMap<String, ClientWithMiddleware>,
    // 组级 fallback 配置（本期仅支持单个）
    group_fallbacks: HashMap<String, String>,
    // 组级 failover 配置
    group_failover: HashMap<String, FailoverConfig>,
    // failover 默认总截止时间（毫秒）
    default_failover_total_time_ms: u64,
    // DNS 客户端（用于 scheme=dns 的组）
    dns_client: DnsClient,
}

impl UpstreamManager {
    // 创建新的上游管理器
    pub async fn new(
        groups: Vec<UpstreamGroupConfig>,
        http_config: HttpConfig,
        dns_config: DnsConfig,
    ) -> Result<Self, AppError> {
        Self::new_with_bootstrap(groups, http_config, dns_config, None).await
    }

    pub async fn new_with_bootstrap(
        groups: Vec<UpstreamGroupConfig>,
        http_config: HttpConfig,
        dns_config: DnsConfig,
        bootstrap_dns: Option<crate::config::BootstrapDnsConfig>,
    ) -> Result<Self, AppError> {
        let mut group_map = HashMap::with_capacity(groups.len());
        let mut group_protocols = HashMap::with_capacity(groups.len());
        let mut group_clients = HashMap::new();
        let mut group_fallbacks = HashMap::new();
        let mut group_failover = HashMap::new();
        let mut doh_http_settings: Vec<(String, Option<String>, Option<RetryConfig>)> = Vec::new();

        let default_failover_total_time_ms =
            std::cmp::min(http_config.request_timeout, dns_config.request_timeout) * 1000;
        let dns_client = DnsClient::new(dns_config);

        // 先为每个组创建负载均衡器并记录 DoH 组的 HTTP 配置（HTTP client 在后续统一创建）
        for group in groups {
            let UpstreamGroupConfig {
                name,
                protocol,
                policy,
                endpoints,
                fallback,
                failover,
                health,
                retry,
                proxy,
            } = group;

            if let Some(fallback) = fallback {
                group_fallbacks.insert(name.clone(), fallback);
            }
            if let Some(failover) = failover {
                group_failover.insert(name.clone(), failover);
            }

            let lb: Arc<dyn LoadBalancer> = match policy {
                LoadBalancingPolicy::RoundRobin => {
                    Arc::new(RoundRobinBalancer::new(endpoints, health))
                }
                LoadBalancingPolicy::Weighted => Arc::new(WeightedBalancer::new(endpoints, health)),
                LoadBalancingPolicy::Random => Arc::new(RandomBalancer::new(endpoints, health)),
            };

            if matches!(protocol, UpstreamProtocol::Doh) {
                doh_http_settings.push((name.clone(), proxy, retry));
            }

            group_protocols.insert(name.clone(), protocol);
            group_map.insert(name, lb);
        }

        let bootstrap_resolver = bootstrap_dns
            .map(|cfg| {
                let mut bootstrap_groups = Vec::with_capacity(cfg.groups.len());
                for group_name in &cfg.groups {
                    let Some(protocol) = group_protocols.get(group_name) else {
                        return Err(AppError::UpstreamGroupNotFound(group_name.clone()));
                    };
                    if !matches!(protocol, UpstreamProtocol::Dns) {
                        return Err(AppError::Upstream(format!(
                            "bootstrap_dns group {} must be protocol=dns",
                            group_name
                        )));
                    }
                    let Some(lb) = group_map.get(group_name) else {
                        return Err(AppError::UpstreamGroupNotFound(group_name.clone()));
                    };
                    bootstrap_groups.push((group_name.clone(), Arc::clone(lb)));
                }

                Ok(Arc::new(super::bootstrap_dns::BootstrapDnsResolver::new(
                    cfg,
                    bootstrap_groups,
                    dns_client.clone(),
                )))
            })
            .transpose()?;

        for (group_name, proxy, retry) in doh_http_settings {
            let client = HttpClient::create(
                &http_config,
                proxy.as_deref(),
                retry.as_ref(),
                bootstrap_resolver.clone(),
            )?;
            group_clients.insert(group_name, client);
        }

        info!("Initialized {} upstream groups", group_map.len());

        Ok(Self {
            groups: group_map,
            group_protocols,
            group_clients,
            group_fallbacks,
            group_failover,
            default_failover_total_time_ms,
            dns_client,
        })
    }

    // 创建一个空的上游管理器，用于测试
    pub fn empty() -> Result<Self, AppError> {
        Ok(Self {
            groups: HashMap::new(),
            group_protocols: HashMap::new(),
            group_clients: HashMap::new(),
            group_fallbacks: HashMap::new(),
            group_failover: HashMap::new(),
            default_failover_total_time_ms: 0,
            dns_client: DnsClient::new(DnsConfig::default()),
        })
    }

    // 转发查询到指定上游组
    pub async fn forward(&self, query: &Message, group_name: &str) -> Result<Message, AppError> {
        let enable_failover = self.group_fallbacks.contains_key(group_name)
            || self.group_failover.contains_key(group_name);

        if !enable_failover {
            return self.forward_single(query, group_name).await;
        }

        self.forward_with_failover(query, group_name).await
    }

    async fn forward_single(&self, query: &Message, group_name: &str) -> Result<Message, AppError> {
        debug!("Forwarding request to upstream group: {}", group_name);

        // 获取上游组的负载均衡器
        let load_balancer = match self.groups.get(group_name) {
            Some(lb) => lb,
            None => {
                error!("Upstream group not found: {}", group_name);
                return Err(AppError::UpstreamGroupNotFound(group_name.to_string()));
            }
        };

        let protocol = match self.group_protocols.get(group_name) {
            Some(protocol) => protocol,
            None => {
                error!("Upstream group protocol not found: {}", group_name);
                return Err(AppError::UpstreamGroupNotFound(group_name.to_string()));
            }
        };

        // 选择一个上游服务器
        let selected_server = match load_balancer.select_server().await {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to select upstream server: {}", e);

                // 记录上游错误指标
                let upstream_protocol = match protocol {
                    UpstreamProtocol::Doh => upstream_protocol_labels::DOH,
                    UpstreamProtocol::Dns => upstream_protocol_labels::DNS,
                };
                let upstream_transport = match protocol {
                    UpstreamProtocol::Doh => upstream_transport_labels::HTTP,
                    UpstreamProtocol::Dns => upstream_transport_labels::UNKNOWN,
                };
                METRICS
                    .upstream_errors_total()
                    .with_label_values(&[
                        upstream_protocol,
                        upstream_transport,
                        error_labels::SELECT_ERROR,
                        group_name,
                        upstream_labels::UNKNOWN,
                    ])
                    .inc();

                return Err(e);
            }
        };

        let result = self
            .forward_selected(query, group_name, protocol, load_balancer, selected_server)
            .await;
        if result.is_ok() {
            load_balancer.report_success(selected_server).await;
        }
        result
    }

    async fn forward_with_failover(
        &self,
        query: &Message,
        primary_group: &str,
    ) -> Result<Message, AppError> {
        let primary_failover_cfg = self.group_failover.get(primary_group);
        let max_groups = primary_failover_cfg.map(|c| c.max_groups).unwrap_or(2);
        let max_total_time_ms = primary_failover_cfg
            .and_then(|c| c.max_total_time_ms)
            .unwrap_or(self.default_failover_total_time_ms);

        let deadline = Instant::now() + Duration::from_millis(max_total_time_ms);

        let mut visited_groups: HashSet<String> = HashSet::new();
        let mut groups_tried: u8 = 0;
        let mut current_group = primary_group.to_string();

        let mut last_error: Option<AppError> = None;
        let mut last_truncated: Option<Message> = None;

        while groups_tried < max_groups {
            if Instant::now() >= deadline {
                break;
            }

            if !visited_groups.insert(current_group.clone()) {
                break;
            }

            let load_balancer = match self.groups.get(current_group.as_str()) {
                Some(lb) => lb,
                None => {
                    last_error = Some(AppError::UpstreamGroupNotFound(current_group.clone()));
                    break;
                }
            };

            let protocol = match self.group_protocols.get(current_group.as_str()) {
                Some(protocol) => protocol,
                None => {
                    last_error = Some(AppError::UpstreamGroupNotFound(current_group.clone()));
                    break;
                }
            };

            let group_failover_cfg = self.group_failover.get(current_group.as_str());
            let max_endpoints_per_group = group_failover_cfg
                .map(|c| c.max_endpoints_per_group)
                .unwrap_or(2);

            let mut attempted: HashSet<String> = HashSet::new();

            for attempt_idx in 0..max_endpoints_per_group {
                if Instant::now() >= deadline {
                    break;
                }

                let mut selected_server: Option<&UpstreamEndpointConfig> = None;
                for _ in 0..8 {
                    match load_balancer.select_server().await {
                        Ok(s) => {
                            let key = Self::endpoint_key(s);
                            if attempted.insert(key) {
                                selected_server = Some(s);
                                break;
                            }
                        }
                        Err(e) => {
                            error!("Failed to select upstream server: {}", e);

                            let upstream_protocol = match protocol {
                                UpstreamProtocol::Doh => upstream_protocol_labels::DOH,
                                UpstreamProtocol::Dns => upstream_protocol_labels::DNS,
                            };
                            let upstream_transport = match protocol {
                                UpstreamProtocol::Doh => upstream_transport_labels::HTTP,
                                UpstreamProtocol::Dns => upstream_transport_labels::UNKNOWN,
                            };
                            METRICS
                                .upstream_errors_total()
                                .with_label_values(&[
                                    upstream_protocol,
                                    upstream_transport,
                                    error_labels::SELECT_ERROR,
                                    current_group.as_str(),
                                    upstream_labels::UNKNOWN,
                                ])
                                .inc();

                            last_error = Some(e);
                            selected_server = None;
                            break;
                        }
                    }
                }

                let Some(selected_server) = selected_server else {
                    if last_error.is_none() {
                        last_error = Some(AppError::NoUpstreamAvailable);
                    }
                    break;
                };

                let (upstream_protocol, upstream_transport, server_label) =
                    Self::labels_for_attempt(protocol, selected_server);

                METRICS
                    .upstream_attempts_total()
                    .with_label_values(&[
                        upstream_protocol,
                        upstream_transport,
                        current_group.as_str(),
                        server_label.as_str(),
                    ])
                    .inc();

                debug!(
                    group = %current_group,
                    attempt_idx,
                    server = %server_label,
                    upstream_protocol = %upstream_protocol,
                    upstream_transport = %upstream_transport,
                    "Failover attempt"
                );

                match self
                    .forward_selected(
                        query,
                        current_group.as_str(),
                        protocol,
                        load_balancer,
                        selected_server,
                    )
                    .await
                {
                    Ok(response) => {
                        let rcode = response.response_code();

                        if matches!(protocol, UpstreamProtocol::Dns) {
                            if let Some(server) = selected_server.as_dns() {
                                if server.transport == Some(DnsTransportMode::Udp)
                                    && response.truncated()
                                {
                                    METRICS
                                        .upstream_failover_total()
                                        .with_label_values(&[
                                            "udp_truncated",
                                            current_group.as_str(),
                                            current_group.as_str(),
                                            upstream_protocol,
                                            upstream_transport,
                                            server_label.as_str(),
                                        ])
                                        .inc();
                                    last_truncated = Some(response.clone());
                                    load_balancer.report_failure(selected_server).await;
                                    last_error =
                                        Some(AppError::Upstream("udp_truncated".to_string()));
                                    continue;
                                }
                            }
                        }

                        match rcode {
                            ResponseCode::NoError | ResponseCode::NXDomain => {
                                load_balancer.report_success(selected_server).await;
                                return Ok(response);
                            }
                            ResponseCode::ServFail | ResponseCode::Refused => {
                                if Self::should_failover_on_rcode(group_failover_cfg, rcode) {
                                    let reason = match rcode {
                                        ResponseCode::ServFail => "rcode_servfail",
                                        ResponseCode::Refused => "rcode_refused",
                                        _ => "rcode",
                                    };
                                    METRICS
                                        .upstream_failover_total()
                                        .with_label_values(&[
                                            reason,
                                            current_group.as_str(),
                                            current_group.as_str(),
                                            upstream_protocol,
                                            upstream_transport,
                                            server_label.as_str(),
                                        ])
                                        .inc();
                                    load_balancer.report_failure(selected_server).await;
                                    last_error = Some(AppError::Upstream(format!(
                                        "rcode {} triggers failover",
                                        rcode
                                    )));
                                    continue;
                                }
                                load_balancer.report_success(selected_server).await;
                                return Ok(response);
                            }
                            _ => {
                                load_balancer.report_success(selected_server).await;
                                return Ok(response);
                            }
                        }
                    }
                    Err(e) => {
                        let reason = Self::failover_reason_for_error(&e);
                        METRICS
                            .upstream_failover_total()
                            .with_label_values(&[
                                reason,
                                current_group.as_str(),
                                current_group.as_str(),
                                upstream_protocol,
                                upstream_transport,
                                server_label.as_str(),
                            ])
                            .inc();
                        last_error = Some(e);
                        continue;
                    }
                }
            }

            groups_tried += 1;

            let Some(next_group) = self.group_fallbacks.get(current_group.as_str()) else {
                break;
            };

            if visited_groups.contains(next_group) {
                break;
            }

            METRICS
                .upstream_failover_total()
                .with_label_values(&[
                    "fallback_group",
                    current_group.as_str(),
                    next_group.as_str(),
                    match protocol {
                        UpstreamProtocol::Doh => upstream_protocol_labels::DOH,
                        UpstreamProtocol::Dns => upstream_protocol_labels::DNS,
                    },
                    match protocol {
                        UpstreamProtocol::Doh => upstream_transport_labels::HTTP,
                        UpstreamProtocol::Dns => upstream_transport_labels::UNKNOWN,
                    },
                    upstream_labels::UNKNOWN,
                ])
                .inc();

            current_group = next_group.clone();
        }

        if let Some(truncated) = last_truncated {
            return Ok(truncated);
        }

        Err(last_error.unwrap_or(AppError::NoUpstreamAvailable))
    }

    fn labels_for_attempt(
        protocol: &UpstreamProtocol,
        endpoint: &UpstreamEndpointConfig,
    ) -> (&'static str, &'static str, String) {
        match protocol {
            UpstreamProtocol::Doh => {
                let server = endpoint.as_doh();
                let server_label = server
                    .and_then(|s| s.url.host_str())
                    .unwrap_or(upstream_labels::UNKNOWN)
                    .to_string();
                (
                    upstream_protocol_labels::DOH,
                    upstream_transport_labels::HTTP,
                    server_label,
                )
            }
            UpstreamProtocol::Dns => {
                let server = endpoint.as_dns();
                let (transport, server_label) = match server {
                    Some(s) => {
                        let transport = match s.transport {
                            Some(DnsTransportMode::Udp) => upstream_transport_labels::UDP,
                            Some(DnsTransportMode::Tcp) => upstream_transport_labels::TCP,
                            Some(DnsTransportMode::Auto) | None => {
                                upstream_transport_labels::UNKNOWN
                            }
                        };
                        (transport, s.addr.ip().to_string())
                    }
                    None => (
                        upstream_transport_labels::UNKNOWN,
                        upstream_labels::UNKNOWN.to_string(),
                    ),
                };
                (upstream_protocol_labels::DNS, transport, server_label)
            }
        }
    }

    fn failover_reason_for_error(error: &AppError) -> &'static str {
        match error {
            AppError::Timeout => "timeout",
            AppError::Http(_) => "http_error",
            AppError::HttpMiddleware(_) => "http_middleware_error",
            AppError::Upstream(_) => "upstream_error",
            AppError::NoUpstreamAvailable => "no_upstream_available",
            AppError::UpstreamGroupNotFound(_) => "upstream_group_not_found",
            _ => "request_error",
        }
    }

    async fn forward_selected(
        &self,
        query: &Message,
        group_name: &str,
        protocol: &UpstreamProtocol,
        load_balancer: &Arc<dyn LoadBalancer>,
        selected_server: &UpstreamEndpointConfig,
    ) -> Result<Message, AppError> {
        match protocol {
            UpstreamProtocol::Doh => {
                let Some(server) = selected_server.as_doh() else {
                    error!("Invalid upstream server type for group: {}", group_name);
                    return Err(AppError::Upstream(
                        "Invalid upstream server type for this group".to_string(),
                    ));
                };

                let server_host = server.url.host_str().unwrap_or(protocol_labels::UNKNOWN);
                debug!("Selected upstream server: {}", server.url.as_str());

                METRICS
                    .upstream_requests_total()
                    .with_label_values(&[
                        upstream_protocol_labels::DOH,
                        upstream_transport_labels::HTTP,
                        group_name,
                        server_host,
                    ])
                    .inc();

                let start_time = Instant::now();

                let client = match self.group_clients.get(group_name) {
                    Some(c) => c,
                    None => {
                        error!("HTTP client not found for group: {}", group_name);
                        return Err(AppError::UpstreamGroupNotFound(group_name.to_string()));
                    }
                };

                let doh_client = DoHClient::new(client);
                match doh_client.send_request(query, server).await {
                    Ok(response) => {
                        let duration = start_time.elapsed();
                        METRICS
                            .upstream_duration_seconds()
                            .with_label_values(&[
                                upstream_protocol_labels::DOH,
                                upstream_transport_labels::HTTP,
                                group_name,
                                server_host,
                            ])
                            .observe(duration.as_secs_f64());

                        Ok(response)
                    }
                    Err(e) => {
                        error!("Upstream request failed: {} - {}", server.url.as_str(), e);

                        load_balancer.report_failure(selected_server).await;

                        METRICS
                            .upstream_errors_total()
                            .with_label_values(&[
                                upstream_protocol_labels::DOH,
                                upstream_transport_labels::HTTP,
                                error_labels::REQUEST_ERROR,
                                group_name,
                                server_host,
                            ])
                            .inc();

                        Err(e)
                    }
                }
            }
            UpstreamProtocol::Dns => {
                let Some(server) = selected_server.as_dns() else {
                    error!("Invalid upstream server type for group: {}", group_name);
                    return Err(AppError::Upstream(
                        "Invalid upstream server type for this group".to_string(),
                    ));
                };

                let server_host = server.addr.ip().to_string();
                debug!("Selected upstream server: {}", server.addr);

                match self
                    .dns_client
                    .send_to(server.addr, query, server.transport)
                    .await
                {
                    Ok(response) => {
                        for attempt in &response.attempts {
                            let upstream_transport = match attempt.transport {
                                DnsTransport::Udp => upstream_transport_labels::UDP,
                                DnsTransport::Tcp => upstream_transport_labels::TCP,
                            };

                            METRICS
                                .upstream_requests_total()
                                .with_label_values(&[
                                    upstream_protocol_labels::DNS,
                                    upstream_transport,
                                    group_name,
                                    server_host.as_str(),
                                ])
                                .inc();
                            METRICS
                                .upstream_duration_seconds()
                                .with_label_values(&[
                                    upstream_protocol_labels::DNS,
                                    upstream_transport,
                                    group_name,
                                    server_host.as_str(),
                                ])
                                .observe(attempt.duration.as_secs_f64());
                        }

                        Ok(response.message)
                    }
                    Err(e) => {
                        error!("Upstream request failed: {} - {}", server.addr, e.error);

                        for attempt in &e.attempts {
                            let upstream_transport = match attempt.transport {
                                DnsTransport::Udp => upstream_transport_labels::UDP,
                                DnsTransport::Tcp => upstream_transport_labels::TCP,
                            };

                            METRICS
                                .upstream_requests_total()
                                .with_label_values(&[
                                    upstream_protocol_labels::DNS,
                                    upstream_transport,
                                    group_name,
                                    server_host.as_str(),
                                ])
                                .inc();
                            METRICS
                                .upstream_duration_seconds()
                                .with_label_values(&[
                                    upstream_protocol_labels::DNS,
                                    upstream_transport,
                                    group_name,
                                    server_host.as_str(),
                                ])
                                .observe(attempt.duration.as_secs_f64());
                        }

                        if let Some(last_attempt) = e.attempts.last() {
                            let upstream_transport = match last_attempt.transport {
                                DnsTransport::Udp => upstream_transport_labels::UDP,
                                DnsTransport::Tcp => upstream_transport_labels::TCP,
                            };

                            METRICS
                                .upstream_errors_total()
                                .with_label_values(&[
                                    upstream_protocol_labels::DNS,
                                    upstream_transport,
                                    error_labels::REQUEST_ERROR,
                                    group_name,
                                    server_host.as_str(),
                                ])
                                .inc();
                        }

                        load_balancer.report_failure(selected_server).await;

                        Err(e.error)
                    }
                }
            }
        }
    }

    fn endpoint_key(endpoint: &UpstreamEndpointConfig) -> String {
        match endpoint {
            UpstreamEndpointConfig::Doh(s) => format!("doh:{}", s.url.as_str()),
            UpstreamEndpointConfig::Dns(s) => {
                let transport = match s.transport {
                    Some(DnsTransportMode::Auto) => "auto",
                    Some(DnsTransportMode::Udp) => "udp",
                    Some(DnsTransportMode::Tcp) => "tcp",
                    None => "default",
                };
                format!("dns:{}?transport={}", s.addr, transport)
            }
        }
    }

    fn should_failover_on_rcode(cfg: Option<&FailoverConfig>, rcode: ResponseCode) -> bool {
        let Some(cfg) = cfg else {
            return false;
        };

        cfg.on_rcode.iter().any(|r| match (r, rcode) {
            (FailoverRcode::ServFail, ResponseCode::ServFail) => true,
            (FailoverRcode::Refused, ResponseCode::Refused) => true,
            _ => false,
        })
    }
}
