use axum::http::{header, StatusCode};
use axum::{routing::get, Router};
use once_cell::sync::Lazy;
use prometheus::{opts, GaugeVec, HistogramVec, IntCounterVec, IntGauge, Registry};

// 全局静态指标实例
pub static METRICS: Lazy<DnsMetrics> = Lazy::new(DnsMetrics::new);

// DNS 代理性能指标
pub struct DnsMetrics {
    registry: Registry,

    // 1. 请求处理和性能指标
    dns_requests_total: IntCounterVec,
    dns_request_duration_seconds: HistogramVec,
    dns_request_errors_total: IntCounterVec,

    // 2. 缓存效率和状态指标
    cache_entries: IntGauge,
    cache_capacity: IntGauge,
    cache_operations_total: IntCounterVec,
    cache_ttl_seconds: HistogramVec,

    // 3. DNS 查询统计指标
    dns_query_type_total: IntCounterVec,
    dns_response_codes_total: IntCounterVec,

    // 4. 上游 DoH 解析器指标
    upstream_requests_total: IntCounterVec,
    upstream_errors_total: IntCounterVec,
    upstream_duration_seconds: HistogramVec,

    // 5. 路由策略指标
    route_matches_total: IntCounterVec,
    route_rules_count: GaugeVec,
}

impl Default for DnsMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl DnsMetrics {
    // 创建新的指标收集器
    pub fn new() -> Self {
        let registry = Registry::new();

        // 1. 请求处理和性能指标
        let dns_requests_total = IntCounterVec::new(
            opts!(
                "loadants_dns_requests_total",
                "Total DNS requests processed by the proxy, classified by protocol (UDP/TCP)"
            ),
            &["protocol"],
        )
        .unwrap();

        let dns_request_duration_seconds = HistogramVec::new(
            prometheus::histogram_opts!(
                "loadants_dns_request_duration_seconds",
                "DNS request processing duration in seconds, classified by protocol and query type",
                vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0]
            ),
            &["protocol", "query_type"],
        )
        .unwrap();

        let dns_request_errors_total = IntCounterVec::new(
            opts!(
                "loadants_dns_request_errors_total",
                "Total DNS request processing errors, classified by error type"
            ),
            &["error_type"],
        )
        .unwrap();

        // 2. 缓存效率和状态指标
        let cache_entries = IntGauge::new(
            "loadants_cache_entries",
            "Current number of DNS cache entries",
        )
        .unwrap();

        let cache_capacity = IntGauge::new(
            "loadants_cache_capacity",
            "Maximum capacity of the DNS cache",
        )
        .unwrap();

        let cache_operations_total = IntCounterVec::new(
            opts!("loadants_cache_operations_total", "Total cache operations, classified by operation type (hit, miss, insert, evict, expire)"),
            &["operation"]
        ).unwrap();

        let cache_ttl_seconds = HistogramVec::new(
            prometheus::histogram_opts!(
                "loadants_cache_ttl_seconds",
                "TTL distribution of DNS cache entries in seconds",
                vec![1.0, 5.0, 10.0, 30.0, 60.0, 300.0, 600.0, 1800.0, 3600.0]
            ),
            &["source"],
        )
        .unwrap();

        // 3. DNS 查询统计指标
        let dns_query_type_total = IntCounterVec::new(
            opts!(
                "loadants_dns_query_type_total",
                "Total DNS queries by record type (A, AAAA, MX, etc.)"
            ),
            &["type"],
        )
        .unwrap();

        let dns_response_codes_total = IntCounterVec::new(
            opts!(
                "loadants_dns_response_codes_total",
                "Total DNS responses by response code (RCODE)"
            ),
            &["rcode"],
        )
        .unwrap();

        // 4. 上游 DoH 解析器指标
        let upstream_requests_total = IntCounterVec::new(
            opts!(
                "loadants_upstream_requests_total",
                "Total requests sent to upstream DoH resolvers, classified by group and server"
            ),
            &["group", "server"],
        )
        .unwrap();

        let upstream_errors_total = IntCounterVec::new(
            opts!(
                "loadants_upstream_errors_total",
                "Total upstream DoH resolver errors, classified by error type, group and server"
            ),
            &["error_type", "group", "server"],
        )
        .unwrap();

        let upstream_duration_seconds = HistogramVec::new(
            prometheus::histogram_opts!(
                "loadants_upstream_duration_seconds",
                "Upstream DoH query duration in seconds, classified by group and server",
                vec![0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
            ),
            &["group", "server"],
        )
        .unwrap();

        // 5. 路由策略指标
        let route_matches_total = IntCounterVec::new(
            opts!("loadants_route_matches_total", "Total routing rule matches, classified by rule type (exact, wildcard, regex) and target group"),
            &["rule_type", "target_group"]
        ).unwrap();

        let route_rules_count = GaugeVec::new(
            opts!("loadants_route_rules_count", "Current number of active routing rules, classified by rule type (exact, wildcard, regex)"),
            &["rule_type"]
        ).unwrap();

        // 创建指标实例
        let metrics = DnsMetrics {
            registry,
            dns_requests_total,
            dns_request_duration_seconds,
            dns_request_errors_total,
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
        };

        // 注册所有指标
        metrics.register_all_metrics();

        metrics
    }

    // 注册所有指标
    fn register_all_metrics(&self) {
        // 1. 请求处理和性能指标
        self.registry
            .register(Box::new(self.dns_requests_total.clone()))
            .unwrap();
        self.registry
            .register(Box::new(self.dns_request_duration_seconds.clone()))
            .unwrap();
        self.registry
            .register(Box::new(self.dns_request_errors_total.clone()))
            .unwrap();

        // 2. 缓存效率和状态指标
        self.registry
            .register(Box::new(self.cache_entries.clone()))
            .unwrap();
        self.registry
            .register(Box::new(self.cache_capacity.clone()))
            .unwrap();
        self.registry
            .register(Box::new(self.cache_operations_total.clone()))
            .unwrap();
        self.registry
            .register(Box::new(self.cache_ttl_seconds.clone()))
            .unwrap();

        // 3. DNS 查询统计指标
        self.registry
            .register(Box::new(self.dns_query_type_total.clone()))
            .unwrap();
        self.registry
            .register(Box::new(self.dns_response_codes_total.clone()))
            .unwrap();

        // 4. 上游 DoH 解析器指标
        self.registry
            .register(Box::new(self.upstream_requests_total.clone()))
            .unwrap();
        self.registry
            .register(Box::new(self.upstream_errors_total.clone()))
            .unwrap();
        self.registry
            .register(Box::new(self.upstream_duration_seconds.clone()))
            .unwrap();

        // 5. 路由策略指标
        self.registry
            .register(Box::new(self.route_matches_total.clone()))
            .unwrap();
        self.registry
            .register(Box::new(self.route_rules_count.clone()))
            .unwrap();
    }

    // 获取 Prometheus 注册表
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    // 导出所有指标为输出字符串
    #[allow(dead_code)]
    pub fn export_metrics(&self) -> String {
        let encoder = prometheus::TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = String::new();
        encoder.encode_utf8(&metric_families, &mut buffer).unwrap();
        buffer
    }

    // 下面是各个指标的getter方法，用于其他模块增加计数或设置值

    // 1. 请求处理和性能指标
    pub fn dns_requests_total(&self) -> &IntCounterVec {
        &self.dns_requests_total
    }

    pub fn dns_request_duration_seconds(&self) -> &HistogramVec {
        &self.dns_request_duration_seconds
    }

    pub fn dns_request_errors_total(&self) -> &IntCounterVec {
        &self.dns_request_errors_total
    }

    // 2. 缓存效率和状态指标
    pub fn cache_entries(&self) -> &IntGauge {
        &self.cache_entries
    }

    pub fn cache_capacity(&self) -> &IntGauge {
        &self.cache_capacity
    }

    pub fn cache_operations_total(&self) -> &IntCounterVec {
        &self.cache_operations_total
    }

    pub fn cache_ttl_seconds(&self) -> &HistogramVec {
        &self.cache_ttl_seconds
    }

    // 3. DNS 查询统计指标
    pub fn dns_query_type_total(&self) -> &IntCounterVec {
        &self.dns_query_type_total
    }

    pub fn dns_response_codes_total(&self) -> &IntCounterVec {
        &self.dns_response_codes_total
    }

    // 4. 上游 DoH 解析器指标
    pub fn upstream_requests_total(&self) -> &IntCounterVec {
        &self.upstream_requests_total
    }

    pub fn upstream_errors_total(&self) -> &IntCounterVec {
        &self.upstream_errors_total
    }

    pub fn upstream_duration_seconds(&self) -> &HistogramVec {
        &self.upstream_duration_seconds
    }

    // 5. 路由策略指标
    pub fn route_matches_total(&self) -> &IntCounterVec {
        &self.route_matches_total
    }

    pub fn route_rules_count(&self) -> &GaugeVec {
        &self.route_rules_count
    }
}

// 提供指标导出路由
pub fn metrics_routes() -> Router {
    Router::new().route(
        "/metrics",
        get(|| async {
            let encoder = prometheus::TextEncoder::new();

            // 直接从全局METRICS获取所有注册的指标
            let metric_families = METRICS.registry().gather();

            // 编码为文本格式
            let mut buffer = String::new();
            encoder.encode_utf8(&metric_families, &mut buffer).unwrap();

            // 返回响应
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, prometheus::TEXT_FORMAT)],
                buffer,
            )
        }),
    )
}
