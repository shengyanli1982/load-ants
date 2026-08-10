use crate::doh::json::SerializableDnsMessage;
use crate::doh::state::AppState;
use crate::metrics::{normalize_query_type_label, normalize_response_code, METRICS};
use crate::r#const::{error_labels, http_headers, processing_labels, protocol_labels};
use axum::{
    body::Bytes,
    extract::{ConnectInfo, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Json, Response},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hickory_proto::op::{Edns, Message, MessageType, ResponseCode};
use hickory_proto::rr::{
    rdata::opt::{ClientSubnet, EdnsOption},
    Name, RData, RecordType,
};
use serde::Deserialize;
use std::borrow::Cow;
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Instant;
use tracing::{debug, error, warn};

const DEFAULT_CACHE_MAX_AGE: u32 = 60;

/// RFC 8484 Section 5.1: 从DNS响应中提取最小TTL，用于设置Cache-Control max-age。
/// max-age MUST NOT 大于Answer section中的最小TTL。
fn extract_min_ttl(response: &Message) -> u32 {
    match response.response_code() {
        ResponseCode::ServFail
        | ResponseCode::Refused
        | ResponseCode::FormErr
        | ResponseCode::NotImp => return 0,
        _ => {}
    }

    let answers = response.answers();
    if !answers.is_empty() {
        answers
            .iter()
            .map(|r| r.ttl())
            .min()
            .unwrap_or(DEFAULT_CACHE_MAX_AGE)
    } else if !response.name_servers().is_empty() {
        let soa_min = response
            .name_servers()
            .iter()
            .filter_map(|r| match r.data() {
                RData::SOA(soa) => Some(soa.minimum()),
                _ => None,
            })
            .min();
        soa_min.unwrap_or(DEFAULT_CACHE_MAX_AGE)
    } else {
        DEFAULT_CACHE_MAX_AGE
    }
}

// 定义一个元组来包含错误信息
type DohError = (StatusCode, &'static str);
type DohQueryType = Cow<'static, str>;
type DohHandlerError = (StatusCode, &'static str, DohQueryType);
type DohBinaryHandlerResult = Result<(HeaderMap, Vec<u8>), DohHandlerError>;
type DohResponseHandlerResult = Result<Response, DohHandlerError>;

/// 根据记录类型高效返回 `Cow<'static, str>`。
/// 常见类型直接借用静态字符串，避免额外分配；
/// 不常见类型则按需分配新的字符串。
#[inline(always)]
fn record_type_to_cow_str(record_type: RecordType) -> Cow<'static, str> {
    Cow::Borrowed(normalize_query_type_label(record_type))
}

/// 定义 `handle_doh_get` 的查询参数结构体
#[derive(Deserialize)]
pub struct DohGetParams {
    pub dns: String,
}

/// 定义 `handle_json_get` 的查询参数结构体
#[derive(Deserialize)]
pub struct DohJsonGetParams {
    pub name: String,
    #[serde(rename = "type")]
    pub r#type: Option<String>,
    /// CD 标志（Checking Disabled），用于控制是否禁用 DNSSEC 验证。
    /// 使用 cd=1 或 cd=true 禁用 DNSSEC 验证；使用 cd=0，cd=false 或不提供 cd 参数启用验证
    #[serde(default)]
    pub cd: Option<String>,
    /// DO 标志（DNSSEC OK），用于控制是否包含 DNSSEC 记录。
    /// 使用 do=1 或 do=true 包含 DNSSEC 记录；使用 do=0，do=false 或不提供 do 参数忽略 DNSSEC 记录
    #[serde(rename = "do", default)]
    pub do_flag: Option<String>,
    /// 内容类型选项，用于指定响应的内容类型
    /// 使用 ct=application/dns-message 接收二进制 DNS 消息；使用 ct=application/x-javascript 或不提供 ct 参数接收 JSON 文本
    #[serde(default)]
    pub ct: Option<String>,
    /// EDNS Client Subnet 参数（Google JSON API），格式为 IP/prefix（如 1.2.3.4/24）。
    #[serde(rename = "edns_client_subnet", default)]
    pub ecs: Option<String>,
}

/// 处理 DNS 消息并生成响应
///
/// 这是一个内部辅助函数，用于处理 DNS 消息并生成响应，被 GET 和 POST 处理函数共用
#[inline(always)]
async fn process_dns_message(state: &AppState, dns_message: &Message) -> Result<Message, DohError> {
    match state.handler.handle_request(dns_message).await {
        Ok(resp) => Ok(resp),
        Err(e) => {
            // RFC 8484 Section 4.2.1: DNS 处理失败必须以 HTTP 200 + DNS SERVFAIL 返回，
            // 而非 HTTP 5xx。HTTP 错误码仅用于 HTTP 协议层面的错误。
            warn!(
                request_id = dns_message.id(),
                error = %e,
                "Upstream processing failed, returning DNS SERVFAIL per RFC 8484"
            );

            let mut response = Message::new();
            response.set_id(dns_message.id());
            response.set_message_type(MessageType::Response);
            response.set_op_code(dns_message.op_code());
            response.set_recursion_desired(dns_message.recursion_desired());
            response.set_recursion_available(true);
            response.set_response_code(ResponseCode::ServFail);

            for query in dns_message.queries() {
                response.add_query(query.clone());
            }

            // RFC 6891 §7 — 若请求包含 EDNS0 OPT，响应也必须包含 OPT 记录
            if let Some(req_edns) = dns_message.extensions() {
                let mut edns = Edns::new();
                edns.set_max_payload(req_edns.max_payload());
                edns.set_version(0);
                // 错误响应不携带 DO bit，不复制 DNSSEC 相关选项
                response.set_edns(edns);
            }

            Ok(response)
        }
    }
}

#[tracing::instrument(
    name = "doh_query",
    skip(state, params),
    fields(client_ip = %addr.ip(), method = "GET")
)]
pub async fn handle_doh_get(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(params): Query<DohGetParams>,
) -> impl IntoResponse {
    let start_time = Instant::now();

    if let Some(limiter) = &state.rate_limiter {
        if !limiter.check(addr.ip()) {
            METRICS
                .http_request_errors_total
                .with_label_values(&[error_labels::REQUEST_ERROR])
                .inc();
            METRICS
                .http_requests_total
                .with_label_values(&[StatusCode::TOO_MANY_REQUESTS.as_str()])
                .inc();
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [(header::RETRY_AFTER, HeaderValue::from_static("1"))],
            )
                .into_response();
        }
    }

    let result: DohBinaryHandlerResult = async {
        let dns_param = &params.dns;

        // 解码 base64url 编码的 DNS 消息。
        let dns_bytes = URL_SAFE_NO_PAD.decode(dns_param).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                processing_labels::error_types::BAD_REQUEST,
                Cow::from(protocol_labels::UNKNOWN),
            )
        })?;

        // 解析 DNS 消息
        let dns_message = Message::from_vec(&dns_bytes).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                processing_labels::error_types::BAD_REQUEST,
                Cow::from(protocol_labels::UNKNOWN),
            )
        })?;

        // 提前获取查询类型，以便在错误日志中也能使用
        let query_type = dns_message
            .queries()
            .first()
            .map(|q| record_type_to_cow_str(q.query_type()))
            .unwrap_or(Cow::from(protocol_labels::UNKNOWN));

        // 处理 DNS 消息
        let response = process_dns_message(&state, &dns_message)
            .await
            .map_err(|(status, err_type)| (status, err_type, query_type.clone()))?;

        // 编码 DNS 响应消息
        let response_bytes = response.to_vec().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                processing_labels::error_types::MESSAGE_ENCODE_ERROR,
                query_type.clone(),
            )
        })?;

        // 构建 HTTP 响应。
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static(http_headers::content_types::DNS_MESSAGE),
        );

        // RFC 8484 Section 5.1: 设置Cache-Control max-age为最小TTL
        let min_ttl = extract_min_ttl(&response);
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_str(&format!("max-age={}", min_ttl))
                .unwrap_or_else(|_| HeaderValue::from_static("max-age=60")),
        );

        let rcode = normalize_response_code(response.response_code());
        record_doh_metrics(
            start_time,
            &query_type,
            addr,
            &Ok(StatusCode::OK),
            None,
            Some(rcode),
        );

        Ok((headers, response_bytes))
    }
    .await;

    match result {
        Ok((headers, body)) => (StatusCode::OK, headers, body).into_response(),
        Err((status, error_type, query_type)) => {
            record_doh_metrics(
                start_time,
                &query_type,
                addr,
                &Err(status),
                Some(error_type),
                None,
            );
            status.into_response()
        }
    }
}

#[tracing::instrument(
    name = "doh_query",
    skip(body, state, headers),
    fields(client_ip = %addr.ip(), method = "POST")
)]
pub async fn handle_doh_post(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let start_time = Instant::now();

    if let Some(limiter) = &state.rate_limiter {
        if !limiter.check(addr.ip()) {
            METRICS
                .http_request_errors_total
                .with_label_values(&[error_labels::REQUEST_ERROR])
                .inc();
            METRICS
                .http_requests_total
                .with_label_values(&[StatusCode::TOO_MANY_REQUESTS.as_str()])
                .inc();
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [(header::RETRY_AFTER, HeaderValue::from_static("1"))],
            )
                .into_response();
        }
    }

    let result: DohBinaryHandlerResult = async {
        if let Some(content_type) = headers.get(header::CONTENT_TYPE) {
            if content_type != http_headers::content_types::DNS_MESSAGE {
                return Err((
                    StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    processing_labels::error_types::UNSUPPORTED_MEDIA_TYPE,
                    Cow::from(protocol_labels::UNKNOWN),
                ));
            }
        } else {
            return Err((
                StatusCode::BAD_REQUEST,
                processing_labels::error_types::BAD_REQUEST,
                Cow::from(protocol_labels::UNKNOWN),
            ));
        }

        // 解析 DNS 消息
        let dns_message = Message::from_vec(&body).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                processing_labels::error_types::BAD_REQUEST,
                Cow::from(protocol_labels::UNKNOWN),
            )
        })?;

        // 提前获取查询类型，以便在错误日志中也能使用
        let query_type = dns_message
            .queries()
            .first()
            .map(|q| record_type_to_cow_str(q.query_type()))
            .unwrap_or(Cow::from(protocol_labels::UNKNOWN));

        // 处理 DNS 消息
        let response = process_dns_message(&state, &dns_message)
            .await
            .map_err(|(status, err_type)| (status, err_type, query_type.clone()))?;

        // 编码 DNS 响应消息
        let response_bytes = response.to_vec().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                processing_labels::error_types::MESSAGE_ENCODE_ERROR,
                query_type.clone(),
            )
        })?;

        // 构建 HTTP 响应。
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static(http_headers::content_types::DNS_MESSAGE),
        );

        // RFC 8484 Section 5.1: 设置Cache-Control max-age为最小TTL
        let min_ttl = extract_min_ttl(&response);
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_str(&format!("max-age={}", min_ttl))
                .unwrap_or_else(|_| HeaderValue::from_static("max-age=60")),
        );

        let rcode = normalize_response_code(response.response_code());
        record_doh_metrics(
            start_time,
            &query_type,
            addr,
            &Ok(StatusCode::OK),
            None,
            Some(rcode),
        );

        Ok((headers, response_bytes))
    }
    .await;

    match result {
        Ok((headers, body)) => (StatusCode::OK, headers, body).into_response(),
        Err((status, error_type, query_type)) => {
            record_doh_metrics(
                start_time,
                &query_type,
                addr,
                &Err(status),
                Some(error_type),
                None,
            );
            status.into_response()
        }
    }
}

pub async fn handle_json_get(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(params): Query<DohJsonGetParams>,
) -> impl IntoResponse {
    let start_time = Instant::now();

    if let Some(limiter) = &state.rate_limiter {
        if !limiter.check(addr.ip()) {
            METRICS
                .http_request_errors_total
                .with_label_values(&[error_labels::REQUEST_ERROR])
                .inc();
            METRICS
                .http_requests_total
                .with_label_values(&[StatusCode::TOO_MANY_REQUESTS.as_str()])
                .inc();
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [(header::RETRY_AFTER, HeaderValue::from_static("1"))],
            )
                .into_response();
        }
    }

    let result: DohResponseHandlerResult = async {
        let name = &params.name;
        if name.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                processing_labels::error_types::BAD_REQUEST,
                Cow::from(protocol_labels::UNKNOWN),
            ));
        }

        // 验证 name 参数格式（长度和标签限制）
        if name.len() > 253 || name.contains("..") || name.starts_with('.') {
            return Err((
                StatusCode::BAD_REQUEST,
                processing_labels::error_types::BAD_REQUEST,
                Cow::from(protocol_labels::UNKNOWN),
            ));
        }

        // 提取查询类型，默认值 `"1"` 表示 A 记录。
        let type_str = params.r#type.as_deref().unwrap_or("1");

        // 尝试从字符串（如 "A", "AAAA"）或数字解析 RecordType
        let record_type = RecordType::from_str(type_str)
            .or_else(|_| type_str.parse::<u16>().map(RecordType::from))
            .map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    processing_labels::error_types::BAD_REQUEST,
                    Cow::from(protocol_labels::UNKNOWN),
                )
            })?;

        let query_type = record_type_to_cow_str(record_type);

        // 处理 CD 标志。
        let checking_disabled = match params.cd.as_deref() {
            Some("1") | Some("true") => true,
            Some("0") | Some("false") | None => false,
            _ => false, // 无效值默认为 false
        };

        // 处理 DO 标志。
        let dnssec_ok = match params.do_flag.as_deref() {
            Some("1") | Some("true") => true,
            Some("0") | Some("false") | None => false,
            _ => false,
        };

        // 创建 DNS 查询消息
        let mut query = Message::new();
        query.set_message_type(MessageType::Query);
        query.set_recursion_desired(true);
        query.set_checking_disabled(checking_disabled);

        let name_result = Name::from_ascii(name).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                processing_labels::error_types::BAD_REQUEST,
                query_type.clone(),
            )
        })?;

        let q = hickory_proto::op::Query::query(name_result, record_type);
        query.add_query(q);

        let ecs_subnet = params
            .ecs
            .as_deref()
            .and_then(|ecs_str| ClientSubnet::from_str(ecs_str).ok());

        if dnssec_ok || ecs_subnet.is_some() {
            let mut edns = Edns::new();
            if dnssec_ok {
                edns.set_dnssec_ok(true);
            }
            if let Some(client_subnet) = ecs_subnet {
                edns.options_mut().insert(EdnsOption::Subnet(client_subnet));
            }
            query.set_edns(edns);
        }

        // 处理 DNS 请求
        let response = process_dns_message(&state, &query)
            .await
            .map_err(|(status, err_type)| (status, err_type, query_type.clone()))?;

        // 构建 HTTP 响应
        let mut headers = HeaderMap::new();

        // RFC 8484 Section 5.1: 设置Cache-Control max-age为最小TTL
        let min_ttl = extract_min_ttl(&response);
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_str(&format!("max-age={}", min_ttl))
                .unwrap_or_else(|_| HeaderValue::from_static("max-age=60")),
        );

        // 提取 edns_client_subnet 参数（Google JSON API）
        let ecs = params.ecs.as_deref();

        let rcode = normalize_response_code(response.response_code());

        // 根据内容类型决定返回二进制 DNS 消息还是 JSON 文本。
        let response = match params.ct.as_deref() {
            Some("application/dns-message") => {
                // 返回二进制 DNS 消息
                headers.insert(
                    header::CONTENT_TYPE,
                    header::HeaderValue::from_static(http_headers::content_types::DNS_MESSAGE),
                );

                // 编码 DNS 响应消息
                let response_bytes = response.to_vec().map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        processing_labels::error_types::MESSAGE_ENCODE_ERROR,
                        query_type.clone(),
                    )
                })?;

                (headers, response_bytes).into_response()
            }
            _ => {
                // JSON 响应（默认）
                headers.insert(
                    header::CONTENT_TYPE,
                    header::HeaderValue::from_static(http_headers::content_types::DNS_JSON),
                );
                (
                    headers,
                    Json(SerializableDnsMessage {
                        message: &response,
                        comment: None,
                        ecs,
                    }),
                )
                    .into_response()
            }
        };

        record_doh_metrics(
            start_time,
            &query_type,
            addr,
            &Ok(StatusCode::OK),
            None,
            Some(rcode),
        );

        Ok(response)
    }
    .await;

    match result {
        Ok(response) => response,
        Err((status, error_type, query_type)) => {
            record_doh_metrics(
                start_time,
                &query_type,
                addr,
                &Err(status),
                Some(error_type),
                None,
            );
            status.into_response()
        }
    }
}

/// 记录 DoH 请求的指标和日志
#[inline]
fn record_doh_metrics(
    start_time: Instant,
    query_type: &str,
    client_addr: SocketAddr,
    result: &Result<StatusCode, StatusCode>,
    error_type: Option<&str>,
    response_code: Option<&str>,
) {
    let duration = start_time.elapsed().as_secs_f64();
    let status_code = match result {
        Ok(s) | Err(s) => *s,
    };
    let status_str = status_code.as_str();

    METRICS
        .http_requests_total
        .with_label_values(&[status_str])
        .inc();

    match result {
        Ok(status) => {
            METRICS
                .http_request_duration_seconds
                .with_label_values(&[query_type, status_str])
                .observe(duration);

            if let Some(rcode) = response_code {
                METRICS
                    .dns_response_codes_total
                    .with_label_values(&[rcode])
                    .inc();
            }

            debug!(
                client_ip = %client_addr,
                status_code = %status,
                duration = ?start_time.elapsed(),
                "Finished processing DoH request"
            );
        }
        Err(status) => {
            if let Some(err_type) = error_type {
                METRICS
                    .http_request_errors_total
                    .with_label_values(&[err_type])
                    .inc();
                error!(
                    client_ip = %client_addr,
                    status_code = %status,
                    error_type = err_type,
                    duration = ?start_time.elapsed(),
                    "Failed to process DoH request"
                );
            } else {
                warn!(
                    client_ip = %client_addr,
                    status_code = %status,
                    duration = ?start_time.elapsed(),
                    "Processed DoH request with an unspecified error"
                );
            }
        }
    }
}
