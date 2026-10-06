//! OPT-2 护栏测试：有界标签集预解析 child 句柄。
//!
//! 验证四点：
//! 1. 同一对象：预解析句柄与 with_label_values 返回的 child 是同一 metric 对象
//!    （经句柄的增量在 with_label_values 侧可见）——/metrics 导出口径不变；
//! 2. 映射正确：query_type_index / response_code_index 与 normalize_* 标签函数
//!    一一对应（含 OTHER 兜底桶），索引常量与 *_indices::ALL 顺序一致；
//! 3. 文本一致：同一等效操作序列，"全经预解析句柄"与"全经 with_label_values
//!    （优化前路径）"两种访问方式的 /metrics 导出文本逐字节一致；
//! 4. 基数有界：预解析物化的序列集合恰为有界枚举，无额外序列。
//!
//! 红线复核：无界标签维度（stale_fallback_total{group}、upstream_*{group,server}、
//! route_matches_total{target_group}、circuit_breaker_transitions_total{group,..}、
//! upstream_health_state{group,server}）不在预解析之列，保持 with_label_values。

use hickory_proto::op::ResponseCode;
use hickory_proto::rr::RecordType;
use loadants::metrics::{
    normalize_query_type_label, normalize_response_code, query_type_index, response_code_index,
    DnsMetrics, QUERY_TYPE_LABELS, RESPONSE_CODE_LABELS,
};
use loadants::r#const::{
    cache_labels, cache_op_indices, error_indices, error_labels, processing_indices,
    processing_labels, protocol_indices, protocol_labels, ttl_source_indices, ttl_source_labels,
};

/// 统计 counter 型 metric 的序列数（`name{labels} value` 行）
fn count_counter_series(output: &str, metric: &str) -> usize {
    let prefix = format!("{metric}{{");
    output.lines().filter(|l| l.starts_with(&prefix)).count()
}

/// 统计 histogram 型 metric 的序列数（每序列恰好一行 `_count{labels}`）
fn count_histogram_series(output: &str, metric: &str) -> usize {
    let prefix = format!("{metric}_count{{");
    output.lines().filter(|l| l.starts_with(&prefix)).count()
}

#[test]
fn index_constants_align_with_label_constants() {
    // protocol
    assert_eq!(
        protocol_indices::ALL[protocol_indices::UDP],
        protocol_labels::UDP
    );
    assert_eq!(
        protocol_indices::ALL[protocol_indices::TCP],
        protocol_labels::TCP
    );
    assert_eq!(
        protocol_indices::ALL[protocol_indices::UNKNOWN],
        protocol_labels::UNKNOWN
    );
    // processing stage
    assert_eq!(
        processing_indices::ALL[processing_indices::CACHED],
        processing_labels::CACHED
    );
    assert_eq!(
        processing_indices::ALL[processing_indices::RESOLVED],
        processing_labels::RESOLVED
    );
    // error type（仅 dns_request_errors_total 实际使用的 8 个有界值）
    assert_eq!(
        error_indices::ALL[error_indices::EMPTY_QUERY],
        error_labels::EMPTY_QUERY
    );
    assert_eq!(
        error_indices::ALL[error_indices::ROUTE_ERROR],
        error_labels::ROUTE_ERROR
    );
    assert_eq!(
        error_indices::ALL[error_indices::MISSING_TARGET],
        error_labels::MISSING_TARGET
    );
    assert_eq!(
        error_indices::ALL[error_indices::UPSTREAM_ERROR],
        error_labels::UPSTREAM_ERROR
    );
    assert_eq!(
        error_indices::ALL[error_indices::UNSUPPORTED_OPCODE],
        error_labels::UNSUPPORTED_OPCODE
    );
    assert_eq!(
        error_indices::ALL[error_indices::UNSUPPORTED_MESSAGE_TYPE],
        error_labels::UNSUPPORTED_MESSAGE_TYPE
    );
    assert_eq!(
        error_indices::ALL[error_indices::HANDLER_ERROR],
        error_labels::HANDLER_ERROR
    );
    assert_eq!(
        error_indices::ALL[error_indices::REQUEST_ERROR],
        error_labels::REQUEST_ERROR
    );
    // cache operation
    assert_eq!(
        cache_op_indices::ALL[cache_op_indices::HIT],
        cache_labels::HIT
    );
    assert_eq!(
        cache_op_indices::ALL[cache_op_indices::MISS],
        cache_labels::MISS
    );
    assert_eq!(
        cache_op_indices::ALL[cache_op_indices::INSERT_ERROR],
        cache_labels::INSERT_ERROR
    );
    assert_eq!(
        cache_op_indices::ALL[cache_op_indices::INSERT],
        cache_labels::INSERT
    );
    assert_eq!(
        cache_op_indices::ALL[cache_op_indices::CLEAR],
        cache_labels::CLEAR
    );
    assert_eq!(
        cache_op_indices::ALL[cache_op_indices::STALE],
        cache_labels::STALE
    );
    // ttl source
    assert_eq!(
        ttl_source_indices::ALL[ttl_source_indices::ORIGINAL],
        ttl_source_labels::ORIGINAL
    );
    assert_eq!(
        ttl_source_indices::ALL[ttl_source_indices::MIN_TTL],
        ttl_source_labels::MIN_TTL
    );
    assert_eq!(
        ttl_source_indices::ALL[ttl_source_indices::MAX_TTL],
        ttl_source_labels::MAX_TTL
    );
    assert_eq!(
        ttl_source_indices::ALL[ttl_source_indices::ADJUSTED],
        ttl_source_labels::ADJUSTED
    );
    assert_eq!(
        ttl_source_indices::ALL[ttl_source_indices::NEGATIVE_TTL],
        ttl_source_labels::NEGATIVE_TTL
    );
}

#[test]
fn preresolved_handles_are_the_same_objects_as_with_label_values() {
    let m = DnsMetrics::new();

    // query_type 维度：13 个具体类型 + OTHER 兜底，索引函数与 normalize 一致
    let record_types = [
        RecordType::A,
        RecordType::AAAA,
        RecordType::ANAME,
        RecordType::CNAME,
        RecordType::MX,
        RecordType::NS,
        RecordType::PTR,
        RecordType::SOA,
        RecordType::SRV,
        RecordType::TXT,
        RecordType::HTTPS,
        RecordType::SVCB,
        RecordType::from(9999), // 未知类型 → OTHER 兜底桶
    ];
    for rt in record_types {
        let label = normalize_query_type_label(rt);
        let before = m.dns_query_type_total.with_label_values(&[label]).get();
        m.query_types[query_type_index(rt)].inc();
        assert_eq!(
            m.dns_query_type_total.with_label_values(&[label]).get(),
            before + 1,
            "query_types[query_type_index({rt:?})] 与标签 {label} 不是同一 child"
        );
    }
    // 兜底桶与已知桶互不串扰
    assert_eq!(
        m.dns_query_type_total.with_label_values(&["OTHER"]).get(),
        1
    );
    assert_eq!(m.dns_query_type_total.with_label_values(&["A"]).get(), 1);

    // rcode 维度：6 个具名响应码 + OTHER 兜底
    let rcodes = [
        ResponseCode::NoError,
        ResponseCode::FormErr,
        ResponseCode::ServFail,
        ResponseCode::NXDomain,
        ResponseCode::NotImp,
        ResponseCode::Refused,
        ResponseCode::YXDomain, // 未在具名列表 → OTHER 兜底桶
    ];
    for rc in rcodes {
        let label = normalize_response_code(rc);
        let before = m.dns_response_codes_total.with_label_values(&[label]).get();
        m.response_codes[response_code_index(rc)].inc();
        assert_eq!(
            m.dns_response_codes_total.with_label_values(&[label]).get(),
            before + 1,
            "response_codes[response_code_index({rc:?})] 与标签 {label} 不是同一 child"
        );
    }

    // protocol 维（counter）
    m.requests_by_protocol[protocol_indices::UDP].inc();
    assert_eq!(m.dns_requests_total.with_label_values(&["udp"]).get(), 1);

    // protocol × query_type 维（histogram）
    m.request_durations[protocol_indices::TCP][query_type_index(RecordType::AAAA)].observe(0.25);
    assert_eq!(
        m.dns_request_duration_seconds
            .with_label_values(&["tcp", "AAAA"])
            .get_sample_count(),
        1
    );

    // processing_stage × query_type 维（histogram）
    m.processing_durations[processing_indices::CACHED][query_type_index(RecordType::A)]
        .observe(0.001);
    m.processing_durations[processing_indices::RESOLVED][query_type_index(RecordType::A)]
        .observe(0.002);
    assert_eq!(
        m.dns_request_processing_duration_seconds
            .with_label_values(&["cached", "A"])
            .get_sample_count(),
        1
    );
    assert_eq!(
        m.dns_request_processing_duration_seconds
            .with_label_values(&["resolved", "A"])
            .get_sample_count(),
        1
    );

    // error_type 维
    m.request_errors[error_indices::REQUEST_ERROR].inc();
    assert_eq!(
        m.dns_request_errors_total
            .with_label_values(&[error_labels::REQUEST_ERROR])
            .get(),
        1
    );

    // cache operation 维
    m.cache_operations[cache_op_indices::HIT].inc();
    assert_eq!(
        m.cache_operations_total
            .with_label_values(&[cache_labels::HIT])
            .get(),
        1
    );

    // ttl source 维（histogram）
    m.cache_ttl_sources[ttl_source_indices::ADJUSTED].observe(300.0);
    assert_eq!(
        m.cache_ttl_seconds
            .with_label_values(&[ttl_source_labels::ADJUSTED])
            .get_sample_count(),
        1
    );
}

#[test]
fn preresolved_ops_export_identical_text_as_with_label_values() {
    // A/B 双实例：构造方式完全相同（同一 DnsMetrics::new），A 全经预解析句柄操作，
    // B 全经 with_label_values（优化前访问路径）执行等效操作序列。
    // 导出文本逐字节一致 ⇒ 预解析未改变任何导出口径（名称/标签/值/格式）。
    let a = DnsMetrics::new();
    let b = DnsMetrics::new();

    let protocol_values = [11u64, 7, 3];
    let error_values = [5u64, 6, 7, 8, 9, 10, 11, 12];
    let cache_op_values = [3u64, 4, 5, 6, 7, 8];
    let ttl_values = [30.0f64, 60.0, 90.0, 120.0, 150.0];

    // ── A：全经预解析句柄 ──
    for (i, c) in a.requests_by_protocol.iter().enumerate() {
        c.inc_by(protocol_values[i]);
    }
    for (i, c) in a.query_types.iter().enumerate() {
        c.inc_by(i as u64 + 1);
    }
    for (i, c) in a.response_codes.iter().enumerate() {
        c.inc_by(2 * i as u64 + 1);
    }
    for (i, c) in a.request_errors.iter().enumerate() {
        c.inc_by(error_values[i]);
    }
    for (i, c) in a.cache_operations.iter().enumerate() {
        c.inc_by(cache_op_values[i]);
    }
    for (p, row) in a.request_durations.iter().enumerate() {
        for (q, h) in row.iter().enumerate() {
            h.observe(0.001 * (p as f64 + 1.0) * (q as f64 + 1.0));
        }
    }
    for (s, row) in a.processing_durations.iter().enumerate() {
        for (q, h) in row.iter().enumerate() {
            h.observe(0.002 * (s as f64 + 1.0) + 0.0001 * q as f64);
        }
    }
    for (i, h) in a.cache_ttl_sources.iter().enumerate() {
        h.observe(ttl_values[i]);
    }

    // ── B：全经 with_label_values（优化前路径），操作序列与 A 等效 ──
    for (i, label) in protocol_indices::ALL.iter().enumerate() {
        b.dns_requests_total
            .with_label_values(&[label])
            .inc_by(protocol_values[i]);
    }
    for (i, label) in QUERY_TYPE_LABELS.iter().enumerate() {
        b.dns_query_type_total
            .with_label_values(&[label])
            .inc_by(i as u64 + 1);
    }
    for (i, label) in RESPONSE_CODE_LABELS.iter().enumerate() {
        b.dns_response_codes_total
            .with_label_values(&[label])
            .inc_by(2 * i as u64 + 1);
    }
    for (i, label) in error_indices::ALL.iter().enumerate() {
        b.dns_request_errors_total
            .with_label_values(&[label])
            .inc_by(error_values[i]);
    }
    for (i, label) in cache_op_indices::ALL.iter().enumerate() {
        b.cache_operations_total
            .with_label_values(&[label])
            .inc_by(cache_op_values[i]);
    }
    for (p, p_label) in protocol_indices::ALL.iter().enumerate() {
        for (q, q_label) in QUERY_TYPE_LABELS.iter().enumerate() {
            b.dns_request_duration_seconds
                .with_label_values(&[p_label, q_label])
                .observe(0.001 * (p as f64 + 1.0) * (q as f64 + 1.0));
        }
    }
    for (s, s_label) in processing_indices::ALL.iter().enumerate() {
        for (q, q_label) in QUERY_TYPE_LABELS.iter().enumerate() {
            b.dns_request_processing_duration_seconds
                .with_label_values(&[s_label, q_label])
                .observe(0.002 * (s as f64 + 1.0) + 0.0001 * q as f64);
        }
    }
    for (i, label) in ttl_source_indices::ALL.iter().enumerate() {
        b.cache_ttl_seconds
            .with_label_values(&[label])
            .observe(ttl_values[i]);
    }

    assert_eq!(
        a.export_metrics(),
        b.export_metrics(),
        "预解析句柄与 with_label_values 的导出文本不一致"
    );
}

#[test]
fn preresolved_series_set_is_exactly_the_bounded_enumeration() {
    let m = DnsMetrics::new();
    let output = m.export_metrics();

    // counter 维：序列数 == 有界枚举大小，无多余物化
    assert_eq!(
        count_counter_series(&output, "loadants_dns_query_type_total"),
        QUERY_TYPE_LABELS.len()
    );
    assert_eq!(
        count_counter_series(&output, "loadants_dns_response_codes_total"),
        RESPONSE_CODE_LABELS.len()
    );
    assert_eq!(
        count_counter_series(&output, "loadants_dns_requests_total"),
        protocol_indices::ALL.len()
    );
    assert_eq!(
        count_counter_series(&output, "loadants_dns_request_errors_total"),
        error_indices::ALL.len()
    );
    assert_eq!(
        count_counter_series(&output, "loadants_cache_operations_total"),
        cache_op_indices::ALL.len()
    );
    // histogram 维
    assert_eq!(
        count_histogram_series(&output, "loadants_cache_ttl_seconds"),
        ttl_source_indices::ALL.len()
    );
    assert_eq!(
        count_histogram_series(&output, "loadants_dns_request_duration_seconds"),
        protocol_indices::ALL.len() * QUERY_TYPE_LABELS.len()
    );
    assert_eq!(
        count_histogram_series(&output, "loadants_dns_request_processing_duration_seconds"),
        processing_indices::ALL.len() * QUERY_TYPE_LABELS.len()
    );

    // 无界维度未被预解析物化：stale_fallback_total 不应出现任何序列
    assert_eq!(
        count_counter_series(&output, "loadants_stale_fallback_total"),
        0,
        "无界标签维度 stale_fallback_total 不得在构造期物化序列"
    );
    assert!(
        !output.contains("loadants_stale_fallback_total{"),
        "无界标签维度不得预解析"
    );
}
