use crate::balancer::ServerState;
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
pub struct GroupHealthInfo {
    pub servers: usize,
    pub healthy: usize,
    pub unhealthy: usize,
    pub half_open: usize,
    pub status: &'static str,
}

pub struct GroupServerSummary {
    pub url: String,
    pub status: &'static str,
    pub weight: u32,
    pub failure_count: u32,
}

pub struct GroupDetailSummary {
    pub name: String,
    pub scheme: &'static str,
    pub strategy: &'static str,
    pub servers: Vec<GroupServerSummary>,
}

use hickory_proto::op::{Message, ResponseCode};
use hickory_proto::rr::{RData, RecordType};
use ipnet::IpNet;
use reqwest_middleware::ClientWithMiddleware;
use std::{collections::HashMap, net::IpAddr, sync::Arc, time::Instant};
use tracing::{debug, error, warn};

use super::dns_client::{DnsClient, DnsTransport};

struct UpstreamGroupState {
    lb: Arc<dyn LoadBalancer>,
    scheme: UpstreamScheme,
    strategy: LoadBalancingStrategy,
    client: Option<ClientWithMiddleware>,
    deny_cidrs: Vec<IpNet>,
    case_randomization: bool,
    case_randomization_strict: bool,
}

pub struct UpstreamManager {
    groups: HashMap<String, UpstreamGroupState>,
    dns_client: DnsClient,
}

impl UpstreamManager {
    pub async fn new(
        groups: Vec<UpstreamGroupConfig>,
        http_config: HttpClientConfig,
        dns_config: DnsClientConfig,
    ) -> Result<Self, AppError> {
        let mut group_map = HashMap::with_capacity(groups.len());
        let dns_client = DnsClient::new(dns_config);

        for UpstreamGroupConfig {
            name,
            scheme,
            strategy,
            servers,
            retry,
            proxy,
            tls_verify,
            deny_answers,
            case_randomization,
            case_randomization_strict,
        } in groups
        {
            let lb: Arc<dyn LoadBalancer> = match strategy {
                LoadBalancingStrategy::RoundRobin => {
                    Arc::new(RoundRobinBalancer::new(&name, servers))
                }
                LoadBalancingStrategy::Weighted => Arc::new(WeightedBalancer::new(&name, servers)),
                LoadBalancingStrategy::Random => Arc::new(RandomBalancer::new(&name, servers)),
            };

            let client = if matches!(scheme, UpstreamScheme::Doh) {
                Some(HttpClient::create(
                    &http_config,
                    proxy.as_deref(),
                    retry.as_ref(),
                    tls_verify,
                )?)
            } else {
                None
            };

            let deny_cidrs: Vec<IpNet> = if deny_answers.is_empty() {
                Vec::new()
            } else {
                deny_answers
                    .iter()
                    .filter_map(|s| {
                        s.parse::<IpNet>()
                            .map_err(|e| {
                                warn!(
                                    "Invalid CIDR in deny_answers for group '{}': {} - {}",
                                    name, s, e
                                );
                            })
                            .ok()
                    })
                    .collect()
            };

            group_map.insert(
                name,
                UpstreamGroupState {
                    lb,
                    scheme,
                    strategy,
                    client,
                    deny_cidrs,
                    case_randomization,
                    case_randomization_strict,
                },
            );
        }

        Ok(Self {
            groups: group_map,
            dns_client,
        })
    }

    pub fn empty() -> Result<Self, AppError> {
        Ok(Self {
            groups: HashMap::new(),
            dns_client: DnsClient::new(DnsClientConfig::default()),
        })
    }

    fn is_denied(ip: IpAddr, deny_cidrs: &[IpNet]) -> bool {
        deny_cidrs.iter().any(|cidr| cidr.contains(&ip))
    }

    fn filter_denied_answers(response: &mut Message, deny_cidrs: &[IpNet]) -> bool {
        let original_addr_count = response
            .answers()
            .iter()
            .filter(|r| matches!(r.record_type(), RecordType::A | RecordType::AAAA))
            .count();

        if original_addr_count == 0 {
            return true;
        }

        let answers: Vec<_> = response
            .answers()
            .iter()
            .filter(|record| match record.data() {
                RData::A(a) => !Self::is_denied(IpAddr::V4(a.0), deny_cidrs),
                RData::AAAA(aaaa) => !Self::is_denied(IpAddr::V6(aaaa.0), deny_cidrs),
                _ => true,
            })
            .cloned()
            .collect();

        let remaining_addr_count = answers
            .iter()
            .filter(|r| matches!(r.record_type(), RecordType::A | RecordType::AAAA))
            .count();

        *response.answers_mut() = answers;

        remaining_addr_count > 0
    }

    pub async fn forward(&self, query: &Message, group_name: &str) -> Result<Message, AppError> {
        debug!("Forwarding request to upstream group: {}", group_name);

        let group_state = match self.groups.get(group_name) {
            Some(state) => state,
            None => {
                error!("Upstream group not found: {}", group_name);
                return Err(AppError::UpstreamGroupNotFound(group_name.to_string()));
            }
        };

        let load_balancer = &group_state.lb;
        let scheme = &group_state.scheme;

        let max_attempts = load_balancer.server_count().max(1);
        let mut last_error: Option<AppError> = None;
        let mut tried_indices: Vec<usize> = Vec::new();

        for attempt in 0..max_attempts {
            let selected_server = match load_balancer.select_server(&tried_indices) {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to select upstream server: {}", e);

                    let upstream_protocol = match scheme {
                        UpstreamScheme::Doh => upstream_protocol_labels::DOH,
                        UpstreamScheme::Dns => upstream_protocol_labels::DNS,
                    };
                    let upstream_transport = match scheme {
                        UpstreamScheme::Doh => upstream_transport_labels::HTTP,
                        UpstreamScheme::Dns => upstream_transport_labels::UNKNOWN,
                    };
                    METRICS
                        .upstream_errors_total
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

            if let Some(idx) = load_balancer
                .servers()
                .iter()
                .position(|s| std::ptr::eq(s, selected_server))
            {
                tried_indices.push(idx);
            }

            match scheme {
                UpstreamScheme::Doh => {
                    let Some(server) = selected_server.as_doh() else {
                        error!("Invalid upstream server type for group: {}", group_name);
                        return Err(AppError::Upstream(
                            "Invalid upstream server type for this group".to_string(),
                        ));
                    };

                    let server_host = server.url.host_str().unwrap_or(protocol_labels::UNKNOWN);
                    debug!(
                        attempt = attempt + 1,
                        max_attempts,
                        server = %server.url.as_str(),
                        "Selected upstream DoH server"
                    );

                    METRICS
                        .upstream_requests_total
                        .with_label_values(&[
                            upstream_protocol_labels::DOH,
                            upstream_transport_labels::HTTP,
                            group_name,
                            server_host,
                        ])
                        .inc();

                    let start_time = Instant::now();

                    let client = match &group_state.client {
                        Some(c) => c,
                        None => {
                            error!("HTTP client not found for group: {}", group_name);
                            return Err(AppError::UpstreamGroupNotFound(group_name.to_string()));
                        }
                    };

                    let doh_client = DoHClient::new(client);
                    match doh_client.send_request(query, server).await {
                        Ok(mut response) => {
                            load_balancer.report_success(selected_server, group_name);

                            let duration = start_time.elapsed();
                            METRICS
                                .upstream_duration_seconds
                                .with_label_values(&[
                                    upstream_protocol_labels::DOH,
                                    upstream_transport_labels::HTTP,
                                    group_name,
                                    server_host,
                                ])
                                .observe(duration.as_secs_f64());

                            if !group_state.deny_cidrs.is_empty()
                                && !Self::filter_denied_answers(
                                    &mut response,
                                    &group_state.deny_cidrs,
                                )
                            {
                                let query_name = query
                                    .queries()
                                    .first()
                                    .map(|q| q.name().to_string())
                                    .unwrap_or_default();
                                warn!(
                                    group = group_name,
                                    query = %query_name,
                                    "All A/AAAA answers filtered by deny_answers, returning SERVFAIL"
                                );
                                response.set_response_code(ResponseCode::ServFail);
                                response.take_answers();
                            }

                            return Ok(response);
                        }
                        Err(e) => {
                            debug!(
                                server = %server.url.as_str(),
                                attempt = attempt + 1,
                                max_attempts,
                                error = %e,
                                "Upstream DoH request failed, will retry if possible"
                            );

                            load_balancer.report_failure(selected_server, group_name);

                            METRICS
                                .upstream_errors_total
                                .with_label_values(&[
                                    upstream_protocol_labels::DOH,
                                    upstream_transport_labels::HTTP,
                                    error_labels::REQUEST_ERROR,
                                    group_name,
                                    server_host,
                                ])
                                .inc();

                            last_error = Some(e);
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
                    debug!(
                        attempt = attempt + 1,
                        max_attempts,
                        server = %server.addr,
                        "Selected upstream DNS server"
                    );

                    match self
                        .dns_client
                        .send_to(
                            server.addr,
                            query,
                            group_state.case_randomization,
                            group_state.case_randomization_strict,
                        )
                        .await
                    {
                        Ok(response) => {
                            load_balancer.report_success(selected_server, group_name);

                            for attempt in &response.attempts {
                                let upstream_transport = match attempt.transport {
                                    DnsTransport::Udp => upstream_transport_labels::UDP,
                                    DnsTransport::Tcp => upstream_transport_labels::TCP,
                                };

                                METRICS
                                    .upstream_requests_total
                                    .with_label_values(&[
                                        upstream_protocol_labels::DNS,
                                        upstream_transport,
                                        group_name,
                                        server_host.as_str(),
                                    ])
                                    .inc();
                                METRICS
                                    .upstream_duration_seconds
                                    .with_label_values(&[
                                        upstream_protocol_labels::DNS,
                                        upstream_transport,
                                        group_name,
                                        server_host.as_str(),
                                    ])
                                    .observe(attempt.duration.as_secs_f64());
                            }

                            let mut msg = response.message;
                            if !group_state.deny_cidrs.is_empty()
                                && !Self::filter_denied_answers(&mut msg, &group_state.deny_cidrs)
                            {
                                let query_name = query
                                    .queries()
                                    .first()
                                    .map(|q| q.name().to_string())
                                    .unwrap_or_default();
                                warn!(
                                    group = group_name,
                                    query = %query_name,
                                    "All A/AAAA answers filtered by deny_answers, returning SERVFAIL"
                                );
                                msg.set_response_code(ResponseCode::ServFail);
                                msg.take_answers();
                            }

                            return Ok(msg);
                        }
                        Err(e) => {
                            debug!(
                                server = %server.addr,
                                attempt = attempt + 1,
                                max_attempts,
                                error = %e.error,
                                "Upstream DNS request failed, will retry if possible"
                            );

                            for dns_attempt in &e.attempts {
                                let upstream_transport = match dns_attempt.transport {
                                    DnsTransport::Udp => upstream_transport_labels::UDP,
                                    DnsTransport::Tcp => upstream_transport_labels::TCP,
                                };

                                METRICS
                                    .upstream_requests_total
                                    .with_label_values(&[
                                        upstream_protocol_labels::DNS,
                                        upstream_transport,
                                        group_name,
                                        server_host.as_str(),
                                    ])
                                    .inc();
                                METRICS
                                    .upstream_duration_seconds
                                    .with_label_values(&[
                                        upstream_protocol_labels::DNS,
                                        upstream_transport,
                                        group_name,
                                        server_host.as_str(),
                                    ])
                                    .observe(dns_attempt.duration.as_secs_f64());
                            }

                            if let Some(last_dns_attempt) = e.attempts.last() {
                                let upstream_transport = match last_dns_attempt.transport {
                                    DnsTransport::Udp => upstream_transport_labels::UDP,
                                    DnsTransport::Tcp => upstream_transport_labels::TCP,
                                };

                                METRICS
                                    .upstream_errors_total
                                    .with_label_values(&[
                                        upstream_protocol_labels::DNS,
                                        upstream_transport,
                                        error_labels::REQUEST_ERROR,
                                        group_name,
                                        server_host.as_str(),
                                    ])
                                    .inc();
                            }

                            load_balancer.report_failure(selected_server, group_name);
                            last_error = Some(e.error);
                        }
                    }
                }
            }
        }

        Err(last_error.unwrap_or(AppError::NoUpstreamAvailable))
    }

    pub fn health_summary(&self) -> HashMap<String, GroupHealthInfo> {
        let mut result = HashMap::with_capacity(self.groups.len());
        for (name, group) in &self.groups {
            let health_states = group.lb.health_states();
            let total = health_states.len();
            let mut healthy = 0usize;
            let mut unhealthy = 0usize;
            let mut half_open = 0usize;
            for h in health_states {
                match h.state() {
                    ServerState::Healthy => healthy += 1,
                    ServerState::Unhealthy => unhealthy += 1,
                    ServerState::HalfOpen => half_open += 1,
                }
            }
            let status = if total == 0 || healthy == total {
                "ok"
            } else if unhealthy == total {
                "unhealthy"
            } else {
                "degraded"
            };
            result.insert(
                name.clone(),
                GroupHealthInfo {
                    servers: total,
                    healthy,
                    unhealthy,
                    half_open,
                    status,
                },
            );
        }
        result
    }

    pub fn group_summary(&self) -> Vec<GroupDetailSummary> {
        let mut result = Vec::with_capacity(self.groups.len());
        for (name, group) in &self.groups {
            let servers_list = group.lb.servers();
            let health_states = group.lb.health_states();

            let servers: Vec<GroupServerSummary> = servers_list
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let (url, weight) = match s {
                        crate::config::UpstreamServerConfig::Doh(doh) => {
                            (doh.url.as_str().to_string(), doh.weight)
                        }
                        crate::config::UpstreamServerConfig::Dns(dns) => {
                            (dns.addr.to_string(), dns.weight)
                        }
                    };
                    let status = if i < health_states.len() {
                        match health_states[i].state() {
                            ServerState::Healthy => "healthy",
                            ServerState::Unhealthy => "unhealthy",
                            ServerState::HalfOpen => "half_open",
                        }
                    } else {
                        "unknown"
                    };
                    let failure_count = if i < health_states.len() {
                        health_states[i].failure_count()
                    } else {
                        0
                    };
                    GroupServerSummary {
                        url,
                        status,
                        weight,
                        failure_count,
                    }
                })
                .collect();

            let scheme_str = match &group.scheme {
                UpstreamScheme::Doh => "doh",
                UpstreamScheme::Dns => "dns",
            };

            let strategy_str = match &group.strategy {
                LoadBalancingStrategy::RoundRobin => "roundrobin",
                LoadBalancingStrategy::Weighted => "weighted",
                LoadBalancingStrategy::Random => "random",
            };

            result.push(GroupDetailSummary {
                name: name.clone(),
                scheme: scheme_str,
                strategy: strategy_str,
                servers,
            });
        }

        result.sort_by(|a, b| a.name.cmp(&b.name));
        result
    }
}
