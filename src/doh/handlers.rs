// src/doh/handlers.rs

use crate::doh::json::SerializableDnsMessage;
use crate::doh::state::AppState;
use crate::metrics::METRICS;
use crate::r#const::{http_headers, processing_labels, protocol_labels};
use axum::{
    body::Bytes,
    extract::{ConnectInfo, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hickory_proto::op::{Message, MessageType};
use hickory_proto::rr::{Name, RecordType};
use serde::Deserialize;
use std::borrow::Cow;
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Instant;
use tracing::{error, info, warn};

// 定义一个元组来包含错误信息
type DohError = (StatusCode, &'static str);

/// 根据记录类型高效地返回一个 Cow<'static, str>
/// 对于常见类型，它借用一个静态字符串，避免了分配。
/// 对于不常见的类型，它会分配一个新的字符串。
#[inline(always)]
fn record_type_to_cow_str(record_type: RecordType) -> Cow<'static, str> {
    match record_type {
        RecordType::A => Cow::Borrowed("A"),
        RecordType::AAAA => Cow::Borrowed("AAAA"),
        RecordType::ANAME => Cow::Borrowed("ANAME"),
        RecordType::CNAME => Cow::Borrowed("CNAME"),
        RecordType::MX => Cow::Borrowed("MX"),
        RecordType::NS => Cow::Borrowed("NS"),
        RecordType::PTR => Cow::Borrowed("PTR"),
        RecordType::SOA => Cow::Borrowed("SOA"),
        RecordType::SRV => Cow::Borrowed("SRV"),
        RecordType::TXT => Cow::Borrowed("TXT"),
        RecordType::HTTPS => Cow::Borrowed("HTTPS"),
        RecordType::SVCB => Cow::Borrowed("SVCB"),
        other => Cow::Owned(other.to_string()),
    }
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
    /// CD (Checking Disabled) 标志，用于控制是否禁用 DNSSEC 验证
    /// 使用 cd=1 或 cd=true 禁用 DNSSEC 验证；使用 cd=0，cd=false 或不提供 cd 参数启用验证
    #[serde(default)]
    pub cd: Option<String>,
    /// DO (DNSSEC OK) 标志，用于控制是否包含 DNSSEC 记录
    /// 使用 do=1 或 do=true 包含 DNSSEC 记录；使用 do=0，do=false 或不提供 do 参数忽略 DNSSEC 记录
    #[serde(rename = "do", default)]
    pub do_flag: Option<String>,
    /// 内容类型选项，用于指定响应的内容类型
    /// 使用 ct=application/dns-message 接收二进制 DNS 消息；使用 ct=application/x-javascript 或不提供 ct 参数接收 JSON 文本
    #[serde(default)]
    pub ct: Option<String>,
}

/// 处理 DNS 消息并生成响应
///
/// 这是一个内部辅助函数，用于处理 DNS 消息并生成响应，被 GET 和 POST 处理函数共用
#[inline(always)]
async fn process_dns_message(state: &AppState, dns_message: &Message) -> Result<Message, DohError> {
    // 处理 DNS 请求
    match state.handler.handle_request(dns_message).await {
        Ok(resp) => Ok(resp),
        Err(_) => {
            // 注意：这里的具体错误已经在 handler 内部记录，这里只向上传递错误类型
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                processing_labels::error_types::UPSTREAM_ERROR,
            ))
        }
    }
}

/// 处理 RFC 8484 DoH GET 请求
///
/// 处理 DNS 查询，其中 DNS 消息通过 URL 参数传递（base64url 编码）
pub async fn handle_doh_get(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(params): Query<DohGetParams>,
) -> impl IntoResponse {
    let start_time = Instant::now();

    let result: Result<(HeaderMap, Vec<u8>), (StatusCode, &'static str, Cow<'static, str>)> =
        async {
            // 提取 DNS 查询参数
            let dns_param = &params.dns;

            // 解码 base64url DNS 消息
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

            // 构建 HTTP 响应
            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_static(http_headers::content_types::DNS_MESSAGE),
            );

            // 记录成功的指标
            record_doh_metrics(start_time, &query_type, addr, &Ok(StatusCode::OK), None);

            Ok((headers, response_bytes))
        }
        .await;

    match result {
        Ok((headers, body)) => (StatusCode::OK, headers, body).into_response(),
        Err((status, error_type, query_type)) => {
            // 记录失败的指标
            record_doh_metrics(
                start_time,
                &query_type,
                addr,
                &Err(status),
                Some(error_type),
            );
            status.into_response()
        }
    }
}

/// 处理 RFC 8484 DoH POST 请求
///
/// 处理 DNS 查询，其中 DNS 消息在 HTTP 请求体中传递
pub async fn handle_doh_post(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let start_time = Instant::now();

    let result: Result<(HeaderMap, Vec<u8>), (StatusCode, &'static str, Cow<'static, str>)> =
        async {
            // 验证内容类型
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

            // 构建 HTTP 响应
            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_static(http_headers::content_types::DNS_MESSAGE),
            );
            // 记录成功的指标
            record_doh_metrics(start_time, &query_type, addr, &Ok(StatusCode::OK), None);

            Ok((headers, response_bytes))
        }
        .await;

    match result {
        Ok((headers, body)) => (StatusCode::OK, headers, body).into_response(),
        Err((status, error_type, query_type)) => {
            // 记录失败的指标
            record_doh_metrics(
                start_time,
                &query_type,
                addr,
                &Err(status),
                Some(error_type),
            );
            status.into_response()
        }
    }
}

/// 处理 Google JSON 格式的 DoH GET 请求
///
/// 处理 Google 格式的 DNS 查询，参数通过 URL 查询字符串传递
pub async fn handle_json_get(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(params): Query<DohJsonGetParams>,
) -> impl IntoResponse {
    let start_time = Instant::now();

    let result: Result<Response, (StatusCode, &'static str, Cow<'static, str>)> = async {
        // 提取必要的查询参数
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

        // 提取查询类型 (默认为 "1" = A 记录)
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

        // 处理 CD 标志 (Checking Disabled)
        let checking_disabled = match params.cd.as_deref() {
            Some("1") | Some("true") => true,
            Some("0") | Some("false") | None => false,
            _ => false, // 无效值默认为 false
        };

        // 处理 DO 标志 (DNSSEC OK)
        let _dnssec_ok = match params.do_flag.as_deref() {
            Some("1") | Some("true") => true,
            Some("0") | Some("false") | None => false,
            _ => false, // 无效值默认为 false
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

        // 处理 DNS 请求
        let response = process_dns_message(&state, &query)
            .await
            .map_err(|(status, err_type)| (status, err_type, query_type.clone()))?;

        // 构建 HTTP 响应
        let mut headers = HeaderMap::new();

        // 处理内容类型 (Content Type)
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
                (headers, Json(SerializableDnsMessage(&response))).into_response()
            }
        };

        // 记录成功的指标
        record_doh_metrics(start_time, &query_type, addr, &Ok(StatusCode::OK), None);

        Ok(response)
    }
    .await;

    match result {
        Ok(response) => response,
        Err((status, error_type, query_type)) => {
            // 记录失败的指标
            record_doh_metrics(
                start_time,
                &query_type,
                addr,
                &Err(status),
                Some(error_type),
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
) {
    let duration = start_time.elapsed().as_secs_f64();
    let status_code = match result {
        Ok(s) | Err(s) => *s,
    };
    let status_str = status_code.as_str();

    // 记录请求总数和时长
    METRICS
        .http_requests_total()
        .with_label_values(&[status_str])
        .inc();

    // 根据结果记录日志和错误总数
    match result {
        Ok(status) => {
            METRICS
                .http_request_duration_seconds()
                .with_label_values(&[query_type, status_str])
                .observe(duration);
            info!(
                client_ip = %client_addr,
                status_code = %status,
                duration = ?start_time.elapsed(),
                "Finished processing DoH request"
            );
        }
        Err(status) => {
            if let Some(err_type) = error_type {
                METRICS
                    .http_request_errors_total()
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
