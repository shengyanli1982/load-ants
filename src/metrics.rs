use axum::http::{header, StatusCode};
use axum::{routing::get, Router};
use hickory_proto::op::ResponseCode;
use hickory_proto::rr::RecordType;
use once_cell::sync::Lazy;
use prometheus::{opts, HistogramVec, IntCounterVec, IntGauge, IntGaugeVec, Registry};
use tracing::error;

pub static METRICS: Lazy<DnsMetrics> = Lazy::new(DnsMetrics::new);
pub fn normalize_query_type_label(record_type: RecordType) -> &'static str {
    match record_type {
        RecordType::A => "A",
        RecordType::AAAA => "AAAA",
        RecordType::ANAME => "ANAME",
        RecordType::CNAME => "CNAME",
        RecordType::MX => "MX",
        RecordType::NS => "NS",
        RecordType::PTR => "PTR",
        RecordType::SOA => "SOA",
        RecordType::SRV => "SRV",
        RecordType::TXT => "TXT",
        RecordType::HTTPS => "HTTPS",
        RecordType::SVCB => "SVCB",
        _ => "OTHER",
    }
}

pub fn normalize_response_code(rc: ResponseCode) -> &'static str {
    match rc {
        ResponseCode::NoError => "NOERROR",
        ResponseCode::FormErr => "FORMERR",
        ResponseCode::ServFail => "SERVFAIL",
        ResponseCode::NXDomain => "NXDOMAIN",
        ResponseCode::NotImp => "NOTIMP",
        ResponseCode::Refused => "REFUSED",
        _ => "OTHER",
    }
}

pub struct DnsMetrics {
    #[doc(hidden)]
    pub registry: Registry,
    #[doc(hidden)]
    pub dns_requests_total: IntCounterVec,
    #[doc(hidden)]
    pub dns_request_duration_seconds: HistogramVec,
    #[doc(hidden)]
    pub dns_request_processing_duration_seconds: HistogramVec,
    #[doc(hidden)]
    pub dns_request_errors_total: IntCounterVec,
    #[doc(hidden)]
    pub http_requests_total: IntCounterVec,
    #[doc(hidden)]
    pub http_request_duration_seconds: HistogramVec,
    #[doc(hidden)]
    pub http_request_errors_total: IntCounterVec,
    #[doc(hidden)]
    pub cache_entries: IntGauge,
    #[doc(hidden)]
    pub cache_capacity: IntGauge,
    #[doc(hidden)]
    pub cache_operations_total: IntCounterVec,
    #[doc(hidden)]
    pub cache_ttl_seconds: HistogramVec,
    #[doc(hidden)]
    pub dns_query_type_total: IntCounterVec,
    #[doc(hidden)]
    pub dns_response_codes_total: IntCounterVec,
    #[doc(hidden)]
    pub upstream_requests_total: IntCounterVec,
    #[doc(hidden)]
    pub upstream_errors_total: IntCounterVec,
    #[doc(hidden)]
    pub upstream_duration_seconds: HistogramVec,
    #[doc(hidden)]
    pub route_matches_total: IntCounterVec,
    #[doc(hidden)]
    pub route_rules_count: IntGaugeVec,
    #[doc(hidden)]
    pub circuit_breaker_transitions_total: IntCounterVec,
    #[doc(hidden)]
    pub upstream_health_state: IntGaugeVec,
    #[doc(hidden)]
    pub inflight_requests: IntGauge,
    #[doc(hidden)]
    pub stale_fallback_total: IntCounterVec,
    #[doc(hidden)]
    pub tcp_pool_connections: IntGauge,
}

impl Default for DnsMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl DnsMetrics {
    pub fn new() -> Self {
        let registry = Registry::new();

        let dns_requests_total = IntCounterVec::new(
            opts!(
                "loadants_dns_requests_total",
                "Total DNS requests processed by the proxy, classified by protocol (UDP/TCP)"
            ),
            &["protocol"],
        )
        .expect("metric name format valid");

        let dns_request_duration_seconds = HistogramVec::new(
            prometheus::histogram_opts!(
                "loadants_dns_request_duration_seconds",
                "DNS request processing duration in seconds, classified by protocol and query type",
                vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0]
            ),
            &["protocol", "query_type"],
        )
        .expect("metric name format valid");

        let dns_request_processing_duration_seconds = HistogramVec::new(
            prometheus::histogram_opts!(
                "loadants_dns_request_processing_duration_seconds",
                "DNS request processing duration in seconds by processing stage (cached, resolved) and query type",
                vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0]
            ),
            &["processing_stage", "query_type"],
        )
        .expect("metric name format valid");

        let dns_request_errors_total = IntCounterVec::new(
            opts!(
                "loadants_dns_request_errors_total",
                "Total DNS request processing errors, classified by error type"
            ),
            &["error_type"],
        )
        .expect("metric name format valid");

        let http_requests_total = IntCounterVec::new(
            opts!(
                "loadants_http_requests_total",
                "Total DNS over HTTP requests processed by the proxy, classified by status code"
            ),
            &["status_code"],
        )
        .expect("metric name format valid");

        let http_request_duration_seconds = HistogramVec::new(
            prometheus::histogram_opts!(
                "loadants_http_request_duration_seconds",
                "DNS over HTTP request processing duration in seconds, classified by query type and status code",
                vec![0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 20.0]
            ),
            &["query_type", "status_code"],
        )
        .expect("metric name format valid");

        let http_request_errors_total = IntCounterVec::new(
            opts!(
                "loadants_http_request_errors_total",
                "Total DNS over HTTP request processing errors, classified by error type"
            ),
            &["error_type"],
        )
        .expect("metric name format valid");

        let cache_entries = IntGauge::new(
            "loadants_cache_entries",
            "Current number of DNS cache entries",
        )
        .expect("metric name format valid");

        let cache_capacity = IntGauge::new(
            "loadants_cache_capacity",
            "Maximum capacity of the DNS cache",
        )
        .expect("metric name format valid");

        let cache_operations_total = IntCounterVec::new(
            opts!(
                "loadants_cache_operations_total",
                "Total cache operations, classified by operation type (hit, miss, stale, insert, insert_error, clear)"
            ),
            &["operation"]
        ).expect("metric name format valid");

        let cache_ttl_seconds = HistogramVec::new(
            prometheus::histogram_opts!(
                "loadants_cache_ttl_seconds",
                "TTL distribution of DNS cache entries in seconds",
                vec![1.0, 5.0, 10.0, 30.0, 60.0, 300.0, 600.0, 1800.0, 3600.0]
            ),
            &["source"],
        )
        .expect("metric name format valid");

        let dns_query_type_total = IntCounterVec::new(
            opts!(
                "loadants_dns_query_type_total",
                "Total DNS queries by record type (A, AAAA, MX, etc.)"
            ),
            &["type"],
        )
        .expect("metric name format valid");

        let dns_response_codes_total = IntCounterVec::new(
            opts!(
                "loadants_dns_response_codes_total",
                "Total DNS responses by response code (RCODE)"
            ),
            &["rcode"],
        )
        .expect("metric name format valid");

        let upstream_requests_total = IntCounterVec::new(
            opts!(
                "loadants_upstream_requests_total",
                "Total requests sent to upstream resolvers, classified by protocol, transport, group and server"
            ),
            &["upstream_protocol", "upstream_transport", "group", "server"],
        )
        .expect("metric name format valid");

        let upstream_errors_total = IntCounterVec::new(
            opts!(
                "loadants_upstream_errors_total",
                "Total upstream resolver errors, classified by protocol, transport, error type, group and server"
            ),
            &[
                "upstream_protocol",
                "upstream_transport",
                "error_type",
                "group",
                "server",
            ],
        )
        .expect("metric name format valid");

        let upstream_duration_seconds = HistogramVec::new(
            prometheus::histogram_opts!(
                "loadants_upstream_duration_seconds",
                "Upstream query duration in seconds, classified by protocol, transport, group and server",
                vec![0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
            ),
            &["upstream_protocol", "upstream_transport", "group", "server"],
        )
        .expect("metric name format valid");

        let route_matches_total = IntCounterVec::new(
            opts!("loadants_route_matches_total", "Total routing rule matches, classified by rule type, target group, rule source and action"),
            &["rule_type", "target_group", "rule_source", "action"]
        ).expect("metric name format valid");

        let route_rules_count = IntGaugeVec::new(
            opts!(
                "loadants_route_rules_count",
                "Current number of active routing rules, classified by rule type and rule source"
            ),
            &["rule_type", "rule_source"],
        )
        .expect("metric name format valid");

        let circuit_breaker_transitions_total = IntCounterVec::new(
            opts!(
                "loadants_circuit_breaker_transitions_total",
                "Total state transitions (Healthy->Unhealthy, Unhealthy->HalfOpen, HalfOpen->Healthy/Unhealthy) per upstream group"
            ),
            &["group", "state_from", "state_to"],
        )
        .expect("metric name format valid");

        let upstream_health_state = IntGaugeVec::new(
            opts!(
                "loadants_upstream_health_state",
                "Current health state of upstream server (0=Unhealthy, 1=HalfOpen, 2=Healthy)"
            ),
            &["group", "server"],
        )
        .expect("metric name format valid");

        let inflight_requests = IntGauge::new(
            "loadants_inflight_requests",
            "Current number of in-flight DNS requests being processed",
        )
        .expect("metric name format valid");

        let stale_fallback_total = IntCounterVec::new(
            opts!(
                "loadants_stale_fallback_total",
                "Total stale cache responses served when upstream forwarding failed"
            ),
            &["group"],
        )
        .expect("metric name format valid");

        let tcp_pool_connections = IntGauge::new(
            "loadants_tcp_pool_connections",
            "Current number of pooled TCP connections",
        )
        .expect("metric name format valid");

        let metrics = DnsMetrics {
            registry,
            dns_requests_total,
            dns_request_duration_seconds,
            dns_request_processing_duration_seconds,
            dns_request_errors_total,
            http_requests_total,
            http_request_duration_seconds,
            http_request_errors_total,
            cache_entries,
            cache_capacity,
            cache_operations_total,
            cache_ttl_seconds,
            dns_query_type_total,
            dns_response_codes_total,
            upstream_requests_total,
            upstream_errors_total,
            upstream_duration_seconds,
            route_matches_total,
            route_rules_count,
            circuit_breaker_transitions_total,
            upstream_health_state,
            inflight_requests,
            stale_fallback_total,
            tcp_pool_connections,
        };

        let collectors: Vec<Box<dyn prometheus::core::Collector + Send + Sync>> = vec![
            Box::new(metrics.dns_requests_total.clone()),
            Box::new(metrics.dns_request_duration_seconds.clone()),
            Box::new(metrics.dns_request_processing_duration_seconds.clone()),
            Box::new(metrics.dns_request_errors_total.clone()),
            Box::new(metrics.http_requests_total.clone()),
            Box::new(metrics.http_request_duration_seconds.clone()),
            Box::new(metrics.http_request_errors_total.clone()),
            Box::new(metrics.cache_entries.clone()),
            Box::new(metrics.cache_capacity.clone()),
            Box::new(metrics.cache_operations_total.clone()),
            Box::new(metrics.cache_ttl_seconds.clone()),
            Box::new(metrics.dns_query_type_total.clone()),
            Box::new(metrics.dns_response_codes_total.clone()),
            Box::new(metrics.upstream_requests_total.clone()),
            Box::new(metrics.upstream_errors_total.clone()),
            Box::new(metrics.upstream_duration_seconds.clone()),
            Box::new(metrics.route_matches_total.clone()),
            Box::new(metrics.route_rules_count.clone()),
            Box::new(metrics.circuit_breaker_transitions_total.clone()),
            Box::new(metrics.upstream_health_state.clone()),
            Box::new(metrics.inflight_requests.clone()),
            Box::new(metrics.stale_fallback_total.clone()),
            Box::new(metrics.tcp_pool_connections.clone()),
        ];
        for m in collectors {
            metrics
                .registry
                .register(m)
                .expect("metric registration failed");
        }

        metrics
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn export_metrics(&self) -> String {
        let encoder = prometheus::TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = String::new();
        encoder
            .encode_utf8(&metric_families, &mut buffer)
            .unwrap_or_else(|e| {
                error!("Failed to encode Prometheus metrics: {}", e);
            });
        buffer
    }
}

// 提供指标导出路由
pub fn metrics_routes() -> Router {
    Router::new().route(
        "/metrics",
        get(|| async {
            let encoder = prometheus::TextEncoder::new();
            let metric_families = METRICS.registry().gather();
            let mut buffer = String::new();
            encoder
                .encode_utf8(&metric_families, &mut buffer)
                .unwrap_or_else(|e| {
                    error!("Failed to encode Prometheus metrics: {}", e);
                    buffer = String::new();
                });
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, prometheus::TEXT_FORMAT)],
                buffer,
            )
        }),
    )
}
