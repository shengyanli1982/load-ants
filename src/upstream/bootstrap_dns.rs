use crate::{
    balancer::LoadBalancer,
    config::BootstrapDnsConfig,
    error::AppError,
    metrics::METRICS,
};
use dashmap::DashMap;
use hickory_proto::{
    op::{Message, OpCode, Query, ResponseCode},
    rr::{
        rdata::{A, AAAA},
        Name, RData, RecordType,
    },
};
use reqwest::dns::{Addrs, Name as ReqwestName, Resolve, Resolving};
use std::{
    collections::HashSet,
    io,
    net::{IpAddr, SocketAddr},
    str::FromStr,
    sync::Arc,
};
use tokio::time::{timeout, Duration, Instant};
use tracing::{debug, warn};

use super::dns_client::DnsClient;

const BOOTSTRAP_METRIC_HIT: &str = "hit";
const BOOTSTRAP_METRIC_MISS: &str = "miss";
const BOOTSTRAP_METRIC_SYSTEM: &str = "system";
const BOOTSTRAP_METRIC_ERROR: &str = "error";

#[derive(Clone)]
pub struct BootstrapDnsResolver {
    inner: Arc<BootstrapDnsResolverInner>,
}

struct BootstrapDnsResolverInner {
    groups: Vec<BootstrapDnsGroup>,
    dns_client: DnsClient,
    timeout: Duration,
    cache_ttl: Duration,
    prefer_ipv6: bool,
    use_system_resolver: bool,
    cache: DashMap<String, CachedAddrs>,
}

#[derive(Clone)]
struct BootstrapDnsGroup {
    name: String,
    lb: Arc<dyn LoadBalancer>,
}

#[derive(Clone)]
struct CachedAddrs {
    addrs: Vec<IpAddr>,
    expires_at: Instant,
}

impl BootstrapDnsResolver {
    pub fn new(
        cfg: BootstrapDnsConfig,
        groups: Vec<(String, Arc<dyn LoadBalancer>)>,
        dns_client: DnsClient,
    ) -> Self {
        let groups = groups
            .into_iter()
            .map(|(name, lb)| BootstrapDnsGroup { name, lb })
            .collect();

        Self {
            inner: Arc::new(BootstrapDnsResolverInner {
                groups,
                dns_client,
                timeout: Duration::from_secs(cfg.timeout),
                cache_ttl: Duration::from_secs(cfg.cache_ttl),
                prefer_ipv6: cfg.prefer_ipv6,
                use_system_resolver: cfg.use_system_resolver,
                cache: DashMap::new(),
            }),
        }
    }

    async fn resolve_host(
        &self,
        host: &str,
    ) -> Result<Vec<IpAddr>, Box<dyn std::error::Error + Send + Sync>> {
        if let Ok(ip) = host.parse::<IpAddr>() {
            return Ok(vec![ip]);
        }

        let now = Instant::now();
        if !self.inner.cache_ttl.is_zero() {
            if let Some(entry) = self.inner.cache.get(host) {
                if entry.expires_at > now {
                    METRICS
                        .bootstrap_dns_queries_total()
                        .with_label_values(&[BOOTSTRAP_METRIC_HIT])
                        .inc();
                    return Ok(entry.addrs.clone());
                }
                self.inner.cache.remove(host);
            }
        }

        METRICS
            .bootstrap_dns_queries_total()
            .with_label_values(&[BOOTSTRAP_METRIC_MISS])
            .inc();

        let resolve_fut = self.resolve_via_bootstrap(host);
        let resolved = match timeout(self.inner.timeout, resolve_fut).await {
            Ok(Ok(resolved)) => Some(resolved),
            Ok(Err(e)) => {
                warn!("Bootstrap DNS failed for host {}: {}", host, e);
                None
            }
            Err(_) => {
                warn!("Bootstrap DNS timed out for host {}", host);
                None
            }
        };

        if let Some((addrs, ttl)) = resolved {
            self.maybe_cache(host, addrs.clone(), ttl);
            return Ok(addrs);
        }

        if self.inner.use_system_resolver {
            METRICS
                .bootstrap_dns_queries_total()
                .with_label_values(&[BOOTSTRAP_METRIC_SYSTEM])
                .inc();
            let addrs = tokio::net::lookup_host((host, 0)).await?;
            let ips: Vec<IpAddr> = addrs.map(|a| a.ip()).collect();
            if !ips.is_empty() {
                self.maybe_cache(host, ips.clone(), None);
                return Ok(ips);
            }
        }

        METRICS
            .bootstrap_dns_queries_total()
            .with_label_values(&[BOOTSTRAP_METRIC_ERROR])
            .inc();

        Err(Box::new(io::Error::new(
            io::ErrorKind::Other,
            format!("Bootstrap DNS resolution failed for host {}", host),
        )))
    }

    fn maybe_cache(&self, host: &str, addrs: Vec<IpAddr>, ttl: Option<u32>) {
        if self.inner.cache_ttl.is_zero() {
            return;
        }

        let max_ttl = self.inner.cache_ttl;
        let ttl = ttl
            .map(|t| Duration::from_secs(t as u64))
            .unwrap_or(max_ttl);
        let ttl = std::cmp::min(ttl, max_ttl);
        if ttl.is_zero() {
            return;
        }

        self.inner.cache.insert(
            host.to_string(),
            CachedAddrs {
                addrs,
                expires_at: Instant::now() + ttl,
            },
        );
    }

    async fn resolve_via_bootstrap(
        &self,
        host: &str,
    ) -> Result<(Vec<IpAddr>, Option<u32>), AppError> {
        let order = if self.inner.prefer_ipv6 {
            [RecordType::AAAA, RecordType::A]
        } else {
            [RecordType::A, RecordType::AAAA]
        };

        let mut results: Vec<IpAddr> = Vec::new();
        let mut min_ttl: Option<u32> = None;
        let mut last_error: Option<AppError> = None;

        for record_type in order {
            match self.resolve_record_type(host, record_type).await {
                Ok((ips, ttl)) => {
                    if !ips.is_empty() {
                        min_ttl = match (min_ttl, ttl) {
                            (Some(a), Some(b)) => Some(a.min(b)),
                            (Some(a), None) => Some(a),
                            (None, Some(b)) => Some(b),
                            (None, None) => None,
                        };
                        results.extend(ips);
                    }
                }
                Err(e) => {
                    last_error = Some(e);
                }
            }
        }

        if results.is_empty() {
            return Err(last_error.unwrap_or_else(|| {
                AppError::Upstream(format!("bootstrap dns returned no A/AAAA records for {}", host))
            }));
        }

        Ok((results, min_ttl))
    }

    async fn resolve_record_type(
        &self,
        host: &str,
        record_type: RecordType,
    ) -> Result<(Vec<IpAddr>, Option<u32>), AppError> {
        let mut last_error: Option<AppError> = None;
        let mut saw_noerror_response = false;

        for group in &self.inner.groups {
            let mut attempted: HashSet<String> = HashSet::new();
            for _ in 0..2 {
                let endpoint = match group.lb.select_server().await {
                    Ok(s) => s,
                    Err(e) => {
                        last_error = Some(e);
                        break;
                    }
                };

                let Some(server) = endpoint.as_dns() else {
                    last_error = Some(AppError::Upstream(format!(
                        "bootstrap group {} selected non-dns endpoint",
                        group.name
                    )));
                    break;
                };

                if !attempted.insert(server.addr.to_string()) {
                    continue;
                }

                let query = build_dns_query(host, record_type)?;
                let response = match timeout(
                    self.inner.timeout,
                    self.inner
                        .dns_client
                        .send_to(server.addr, &query, server.transport),
                )
                .await
                {
                    Ok(Ok(r)) => {
                        group.lb.report_success(endpoint).await;
                        r.message
                    }
                    Ok(Err(e)) => {
                        group.lb.report_failure(endpoint).await;
                        last_error = Some(e.error);
                        continue;
                    }
                    Err(_) => {
                        group.lb.report_failure(endpoint).await;
                        last_error = Some(AppError::Timeout);
                        continue;
                    }
                };

                if response.response_code() != ResponseCode::NoError {
                    debug!(
                        "bootstrap dns rcode for host {} type {:?}: {:?}",
                        host,
                        record_type,
                        response.response_code()
                    );
                    last_error = Some(AppError::Upstream(format!(
                        "bootstrap dns rcode {:?}",
                        response.response_code()
                    )));
                    continue;
                }

                saw_noerror_response = true;
                let (ips, ttl) = extract_ips(&response, record_type);
                if !ips.is_empty() {
                    return Ok((ips, ttl));
                }
            }
        }

        if saw_noerror_response {
            Ok((Vec::new(), None))
        } else {
            Err(last_error.unwrap_or(AppError::NoUpstreamAvailable))
        }
    }
}

impl Resolve for BootstrapDnsResolver {
    fn resolve(&self, name: ReqwestName) -> Resolving {
        let host = name.as_str().to_string();
        debug!(host = %host, "bootstrap_dns resolver invoked");
        let resolver = self.clone();
        Box::pin(async move {
            let ips = resolver.resolve_host(&host).await?;
            let addrs: Vec<SocketAddr> = ips.into_iter().map(|ip| SocketAddr::new(ip, 0)).collect();
            let addrs: Addrs = Box::new(addrs.into_iter());
            Ok(addrs)
        })
    }
}

fn build_dns_query(host: &str, record_type: RecordType) -> Result<Message, AppError> {
    let mut message = Message::new();
    message.set_id(0);
    message.set_op_code(OpCode::Query);
    message.set_recursion_desired(true);

    let qname = if host.ends_with('.') {
        host.to_string()
    } else {
        format!("{}.", host)
    };
    let name = Name::from_str(&qname).map_err(|e| AppError::DnsProto(e.into()))?;
    let query = Query::query(name, record_type);
    message.add_query(query);
    Ok(message)
}

fn extract_ips(message: &Message, record_type: RecordType) -> (Vec<IpAddr>, Option<u32>) {
    let mut ips: Vec<IpAddr> = Vec::new();
    let mut min_ttl: Option<u32> = None;

    for record in message.answers() {
        if record.record_type() != record_type {
            continue;
        }
        let ttl = record.ttl();
        min_ttl = Some(min_ttl.map(|t| t.min(ttl)).unwrap_or(ttl));

        if let Some(RData::A(A(ip))) = record.data() {
            ips.push(IpAddr::V4(*ip));
        }
        if let Some(RData::AAAA(AAAA(ip))) = record.data() {
            ips.push(IpAddr::V6(*ip));
        }
    }

    (ips, min_ttl)
}
