use crate::r#const::{
    cache_op_indices, error_indices, processing_indices, protocol_indices, ttl_source_indices,
};
use axum::http::{header, StatusCode};
use axum::{routing::get, Router};
use hickory_proto::op::ResponseCode;
use hickory_proto::rr::RecordType;
use once_cell::sync::Lazy;
use prometheus::{
    opts, Histogram, HistogramVec, IntCounter, IntCounterVec, IntGauge, IntGaugeVec, Registry,
};
use tracing::error;

pub static METRICS: Lazy<DnsMetrics> = Lazy::new(DnsMetrics::new);

/// query_type 标签的有界枚举（13 值），与 [`query_type_index`] 的下标一一对应。
/// OPT-2 预解析句柄按此顺序物化，导出口径与 normalize_query_type_label 完全一致。
pub const QUERY_TYPE_LABELS: [&str; QUERY_TYPE_LABEL_COUNT] = [
    "A", "AAAA", "ANAME", "CNAME", "MX", "NS", "PTR", "SOA", "SRV", "TXT", "HTTPS", "SVCB", "OTHER",
];
/// query_type 有界标签集大小
pub const QUERY_TYPE_LABEL_COUNT: usize = 13;

/// RecordType → [`QUERY_TYPE_LABELS`] 下标（O(1) 匹配，供预解析句柄索引）。
pub fn query_type_index(record_type: RecordType) -> usize {
    match record_type {
        RecordType::A => 0,
        RecordType::AAAA => 1,
        RecordType::ANAME => 2,
        RecordType::CNAME => 3,
        RecordType::MX => 4,
        RecordType::NS => 5,
        RecordType::PTR => 6,
        RecordType::SOA => 7,
        RecordType::SRV => 8,
        RecordType::TXT => 9,
        RecordType::HTTPS => 10,
        RecordType::SVCB => 11,
        _ => 12,
    }
}

/// RecordType → 有界 query_type 标签（未知类型统一收敛为 "OTHER"）。
pub fn normalize_query_type_label(record_type: RecordType) -> &'static str {
    QUERY_TYPE_LABELS[query_type_index(record_type)]
}

/// rcode 标签的有界枚举（7 值），与 [`response_code_index`] 的下标一一对应。
pub const RESPONSE_CODE_LABELS: [&str; RESPONSE_CODE_LABEL_COUNT] = [
    "NOERROR", "FORMERR", "SERVFAIL", "NXDOMAIN", "NOTIMP", "REFUSED", "OTHER",
];
/// rcode 有界标签集大小
pub const RESPONSE_CODE_LABEL_COUNT: usize = 7;

/// ResponseCode → [`RESPONSE_CODE_LABELS`] 下标（O(1) 匹配，供预解析句柄索引）。
pub fn response_code_index(rc: ResponseCode) -> usize {
    match rc {
        ResponseCode::NoError => 0,
        ResponseCode::FormErr => 1,
        ResponseCode::ServFail => 2,
        ResponseCode::NXDomain => 3,
        ResponseCode::NotImp => 4,
        ResponseCode::Refused => 5,
        _ => 6,
    }
}

/// ResponseCode → 有界 rcode 标签（未知响应码统一收敛为 "OTHER"）。
pub fn normalize_response_code(rc: ResponseCode) -> &'static str {
    RESPONSE_CODE_LABELS[response_code_index(rc)]
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

    // ── OPT-2: 有界标签集预解析 child 句柄 ──
    // 构造期一次性 with_label_values 解析（IntCounter/Histogram 是底层 child 的
    // 廉价 Arc 克隆，与上方 *Vec 内部为同一 metric 对象——增量对同一原子计数生效，
    // /metrics 导出口径不变）。热路径直接索引，消除每 op 的
    // RwLock + SipHash + Arc clone/drop 查找开销（Round1 profile: 占每 op ~85%）。
    // 红线：仅收录有界标签维度；stale_fallback_total{group}、upstream_*{group,server}、
    // route_matches_total{target_group} 等无界维度严禁预解析，保持 with_label_values。
    /// dns_requests_total，按 protocol_indices（udp/tcp/unknown）索引
    pub requests_by_protocol: [IntCounter; protocol_indices::ALL.len()],
    /// dns_request_duration_seconds，按 [protocol_indices][query_type_index] 索引
    pub request_durations: [[Histogram; QUERY_TYPE_LABEL_COUNT]; protocol_indices::ALL.len()],
    /// dns_request_processing_duration_seconds，按 [processing_indices][query_type_index] 索引
    pub processing_durations: [[Histogram; QUERY_TYPE_LABEL_COUNT]; processing_indices::ALL.len()],
    /// dns_request_errors_total，按 error_indices 索引（仅收录该 metric 实际使用的有界值）
    pub request_errors: [IntCounter; error_indices::ALL.len()],
    /// dns_response_codes_total，按 response_code_index() 索引
    pub response_codes: [IntCounter; RESPONSE_CODE_LABEL_COUNT],
    /// dns_query_type_total，按 query_type_index() 索引
    pub query_types: [IntCounter; QUERY_TYPE_LABEL_COUNT],
    /// cache_operations_total，按 cache_op_indices 索引
    pub cache_operations: [IntCounter; cache_op_indices::ALL.len()],
    /// cache_ttl_seconds，按 ttl_source_indices 索引
    pub cache_ttl_sources: [Histogram; ttl_source_indices::ALL.len()],
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

        // OPT-2: 构造期一次性预解析有界标签集的 child 句柄。
        // with_label_values 返回的 IntCounter/Histogram 是 *Vec 内部 child 的 Arc 克隆
        // （同一原子计数对象），因此注册表、metric 对象与导出口径完全不变；
        // 代价是全部有界组合在启动时即物化为零值序列（数量固定，见
        // tests/metrics_preresolved_export_test.rs 护栏）。
        let requests_by_protocol: [IntCounter; protocol_indices::ALL.len()] =
            std::array::from_fn(|i| {
                dns_requests_total.with_label_values(&[protocol_indices::ALL[i]])
            });
        let request_durations: [[Histogram; QUERY_TYPE_LABEL_COUNT]; protocol_indices::ALL.len()] =
            std::array::from_fn(|p| {
                std::array::from_fn(|q| {
                    dns_request_duration_seconds
                        .with_label_values(&[protocol_indices::ALL[p], QUERY_TYPE_LABELS[q]])
                })
            });
        let processing_durations: [[Histogram; QUERY_TYPE_LABEL_COUNT];
            processing_indices::ALL.len()] = std::array::from_fn(|s| {
            std::array::from_fn(|q| {
                dns_request_processing_duration_seconds
                    .with_label_values(&[processing_indices::ALL[s], QUERY_TYPE_LABELS[q]])
            })
        });
        let request_errors: [IntCounter; error_indices::ALL.len()] = std::array::from_fn(|i| {
            dns_request_errors_total.with_label_values(&[error_indices::ALL[i]])
        });
        let response_codes: [IntCounter; RESPONSE_CODE_LABEL_COUNT] = std::array::from_fn(|i| {
            dns_response_codes_total.with_label_values(&[RESPONSE_CODE_LABELS[i]])
        });
        let query_types: [IntCounter; QUERY_TYPE_LABEL_COUNT] = std::array::from_fn(|i| {
            dns_query_type_total.with_label_values(&[QUERY_TYPE_LABELS[i]])
        });
        let cache_operations: [IntCounter; cache_op_indices::ALL.len()] =
            std::array::from_fn(|i| {
                cache_operations_total.with_label_values(&[cache_op_indices::ALL[i]])
            });
        let cache_ttl_sources: [Histogram; ttl_source_indices::ALL.len()] =
            std::array::from_fn(|i| {
                cache_ttl_seconds.with_label_values(&[ttl_source_indices::ALL[i]])
            });

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
            requests_by_protocol,
            request_durations,
            processing_durations,
            request_errors,
            response_codes,
            query_types,
            cache_operations,
            cache_ttl_sources,
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
                error!(
                    context = "export_metrics",
                    error = %e,
                    "Failed to encode Prometheus metrics"
                );
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
                    error!(
                        context = "metrics_route",
                        error = %e,
                        "Failed to encode Prometheus metrics"
                    );
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
