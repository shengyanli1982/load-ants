use crate::{
    balancer::{LoadBalancer, RandomBalancer, RoundRobinBalancer, WeightedBalancer},
    config::{
        DnsClientConfig, HttpClientConfig, LoadBalancingStrategy, UpstreamGroupConfig,
        UpstreamScheme,
    },
    error::AppError,
    metrics::METRICS,
    r#const::{
        error_labels, protocol_labels, upstream_labels, upstream_protocol_labels,
        upstream_transport_labels,
    },
    upstream::{doh::DoHClient, http_client::HttpClient},
};
use hickory_proto::op::Message;
use reqwest_middleware::ClientWithMiddleware;
use std::{collections::HashMap, sync::Arc, time::Instant};
use tracing::{debug, error, info};

use super::dns_client::{DnsClient, DnsTransport};

// 上游管理器
pub struct UpstreamManager {
    // 上游组负载均衡器
    groups: HashMap<String, Arc<dyn LoadBalancer>>,
    // 上游组 scheme
    group_schemes: HashMap<String, UpstreamScheme>,
    // 上游组客户端
    group_clients: HashMap<String, ClientWithMiddleware>,
    // DNS 客户端（用于 scheme=dns 的组）
    dns_client: DnsClient,
}

impl UpstreamManager {
    // 创建新的上游管理器
    pub async fn new(
        groups: Vec<UpstreamGroupConfig>,
        http_config: HttpClientConfig,
        dns_config: DnsClientConfig,
    ) -> Result<Self, AppError> {
        let mut group_map = HashMap::with_capacity(groups.len());
        let mut group_schemes = HashMap::with_capacity(groups.len());
        let mut group_clients = HashMap::new();
        let dns_client = DnsClient::new(dns_config);

        // 为每个组创建负载均衡器和HTTP客户端
        for UpstreamGroupConfig {
            name,
            scheme,
            strategy,
            servers,
            retry,
            proxy,
        } in groups
        {
            let lb: Arc<dyn LoadBalancer> = match strategy {
                LoadBalancingStrategy::RoundRobin => Arc::new(RoundRobinBalancer::new(servers)),
                LoadBalancingStrategy::Weighted => Arc::new(WeightedBalancer::new(servers)),
                LoadBalancingStrategy::Random => Arc::new(RandomBalancer::new(servers)),
            };

            if matches!(scheme, UpstreamScheme::Doh) {
                // 创建该组的HTTP客户端
                let client = HttpClient::create(&http_config, proxy.as_deref(), retry.as_ref())?;
                group_clients.insert(name.clone(), client);
            }

            group_schemes.insert(name.clone(), scheme);
            group_map.insert(name, lb);
        }

        info!("Initialized {} upstream groups", group_map.len());

        Ok(Self {
            groups: group_map,
            group_schemes,
            group_clients,
            dns_client,
        })
    }

    // 创建一个空的上游管理器，用于测试
    pub fn empty() -> Result<Self, AppError> {
        Ok(Self {
            groups: HashMap::new(),
            group_schemes: HashMap::new(),
            group_clients: HashMap::new(),
            dns_client: DnsClient::new(DnsClientConfig::default()),
        })
    }

    // 转发查询到指定上游组
    pub async fn forward(&self, query: &Message, group_name: &str) -> Result<Message, AppError> {
        debug!("Forwarding request to upstream group: {}", group_name);

        // 获取上游组的负载均衡器
        let load_balancer = match self.groups.get(group_name) {
            Some(lb) => lb,
            None => {
                error!("Upstream group not found: {}", group_name);
                return Err(AppError::UpstreamGroupNotFound(group_name.to_string()));
            }
        };

        let scheme = match self.group_schemes.get(group_name) {
            Some(scheme) => scheme,
            None => {
                error!("Upstream group scheme not found: {}", group_name);
                return Err(AppError::UpstreamGroupNotFound(group_name.to_string()));
            }
        };

        // 选择一个上游服务器
        let selected_server = match load_balancer.select_server().await {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to select upstream server: {}", e);

                // 记录上游错误指标
                let upstream_protocol = match scheme {
                    UpstreamScheme::Doh => upstream_protocol_labels::DOH,
                    UpstreamScheme::Dns => upstream_protocol_labels::DNS,
                };
                let upstream_transport = match scheme {
                    UpstreamScheme::Doh => upstream_transport_labels::HTTP,
                    UpstreamScheme::Dns => upstream_transport_labels::UNKNOWN,
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

        match scheme {
            UpstreamScheme::Doh => {
                let Some(server) = selected_server.as_doh() else {
                    error!("Invalid upstream server type for group: {}", group_name);
                    return Err(AppError::Upstream(
                        "Invalid upstream server type for this group".to_string(),
                    ));
                };

                let server_host = server.url.host_str().unwrap_or(protocol_labels::UNKNOWN);
                debug!("Selected upstream server: {}", server.url.as_str());

                // 记录上游请求指标
                METRICS
                    .upstream_requests_total()
                    .with_label_values(&[
                        upstream_protocol_labels::DOH,
                        upstream_transport_labels::HTTP,
                        group_name,
                        server_host,
                    ])
                    .inc();

                // 记录开始时间
                let start_time = Instant::now();

                // 获取组的HTTP客户端
                let client = match self.group_clients.get(group_name) {
                    Some(c) => c,
                    None => {
                        error!("HTTP client not found for group: {}", group_name);
                        return Err(AppError::UpstreamGroupNotFound(group_name.to_string()));
                    }
                };

                // 发送请求（通过reqwest-retry中间件处理重试）
                let doh_client = DoHClient::new(client);
                match doh_client.send_request(query, server).await {
                    Ok(response) => {
                        // 记录上游请求耗时
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

                        // 报告上游失败
                        load_balancer.report_failure(selected_server).await;

                        // 记录上游错误指标
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
            UpstreamScheme::Dns => {
                let Some(server) = selected_server.as_dns() else {
                    error!("Invalid upstream server type for group: {}", group_name);
                    return Err(AppError::Upstream(
                        "Invalid upstream server type for this group".to_string(),
                    ));
                };

                let server_host = server.addr.ip().to_string();
                debug!("Selected upstream server: {}", server.addr);

                match self.dns_client.send_to(server.addr, query).await {
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
}
