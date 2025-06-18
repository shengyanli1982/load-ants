// src/doh/handlers.rs

use crate::doh::json::dns_message_to_json;
use crate::doh::state::AppState;
use crate::metrics::METRICS;
use crate::r#const::{http_headers, processing_labels, protocol_labels};
use axum::{
    body::Bytes,
    extract::{ConnectInfo, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hickory_proto::op::{Message, MessageType};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Instant;
use tracing::{debug, error, info, warn};

// 定义一个元组来包含错误信息
type DohError = (StatusCode, &'static str);

/// 处理 DNS 消息并生成响应
///
/// 这是一个内部辅助函数，用于处理 DNS 消息并生成响应，被 GET 和 POST 处理函数共用
async fn process_dns_message(
    state: &AppState,
    dns_message: &Message,
) -> Result<(Vec<u8>, String), DohError> {
    // 处理 DNS 请求
    let response = match state.handler.handle_request(dns_message).await {
        Ok(resp) => resp,
        Err(_) => {
            // 注意：这里的具体错误已经在 handler 内部记录，这里只向上传递错误类型
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                processing_labels::error_types::UPSTREAM_ERROR,
            ));
        }
    };

    // 获取查询类型
    let query_type = dns_message
        .queries()
        .first()
        .map(|q| q.query_type().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // 编码 DNS 响应消息
    // 预分配合理大小的缓冲区，大多数DNS响应小于512字节
    let response_bytes = match response.to_vec() {
        Ok(bytes) => bytes,
        Err(_) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                processing_labels::error_types::MESSAGE_ENCODE_ERROR,
            ));
        }
    };

    Ok((response_bytes, query_type))
}

/// 处理 RFC 8484 DoH GET 请求
///
/// 处理 DNS 查询，其中 DNS 消息通过 URL 参数传递（base64url 编码）
pub async fn handle_doh_get(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let start_time = Instant::now();
    info!("Received DoH GET request from {}", addr);

    let result: Result<(HeaderMap, Vec<u8>), DohError> = async {
        // 提取 DNS 查询参数
        let dns_param = params.get("dns").ok_or((
            StatusCode::BAD_REQUEST,
            processing_labels::error_types::BAD_REQUEST,
        ))?;

        // 解码 base64url DNS 消息
        let dns_bytes = URL_SAFE_NO_PAD.decode(dns_param).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                processing_labels::error_types::BAD_REQUEST,
            )
        })?;

        // 解析 DNS 消息
        let dns_message = Message::from_vec(&dns_bytes).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                processing_labels::error_types::BAD_REQUEST,
            )
        })?;

        // 处理 DNS 消息
        let (response_bytes, query_type) = process_dns_message(&state, &dns_message).await?;

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
        Err((status, error_type)) => {
            // 记录失败的指标
            record_doh_metrics(start_time, "unknown", addr, &Err(status), Some(error_type));
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
    info!("Received DoH POST request from {}", addr);

    let result: Result<(HeaderMap, Vec<u8>), DohError> = async {
        // 验证内容类型
        if let Some(content_type) = headers.get(header::CONTENT_TYPE) {
            if content_type != http_headers::content_types::DNS_MESSAGE {
                return Err((
                    StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    processing_labels::error_types::UNSUPPORTED_MEDIA_TYPE,
                ));
            }
        } else {
            return Err((
                StatusCode::BAD_REQUEST,
                processing_labels::error_types::BAD_REQUEST,
            ));
        }

        // 解析 DNS 消息
        let dns_message = Message::from_vec(&body).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                processing_labels::error_types::BAD_REQUEST,
            )
        })?;

        // 处理 DNS 消息
        let (response_bytes, query_type) = process_dns_message(&state, &dns_message).await?;

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
        Err((status, error_type)) => {
            // 记录失败的指标
            record_doh_metrics(
                start_time,
                protocol_labels::UNKNOWN,
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
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let start_time = Instant::now();
    info!("Received JSON DoH GET request from {}", addr);

    let result: Result<(HeaderMap, String), DohError> = async {
        // 提取必要的查询参数
        let name = params.get("name").ok_or((
            StatusCode::BAD_REQUEST,
            processing_labels::error_types::BAD_REQUEST,
        ))?;

        // 提取查询类型 (默认为 1 = A 记录)
        let default_type = String::from("1");
        let type_str = params.get("type").unwrap_or(&default_type);
        let query_type_u16 = type_str.parse::<u16>().map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                processing_labels::error_types::BAD_REQUEST,
            )
        })?;
        let record_type = hickory_proto::rr::RecordType::from(query_type_u16);
        let query_type_str = record_type.to_string();

        // 创建 DNS 查询消息
        let mut query = Message::new();
        query.set_message_type(MessageType::Query);
        query.set_recursion_desired(true);

        let name_result = hickory_proto::rr::Name::from_ascii(name).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                processing_labels::error_types::BAD_REQUEST,
            )
        })?;

        let q = hickory_proto::op::Query::query(name_result, record_type);
        query.add_query(q);

        // 处理 DNS 请求
        let response = state.handler.handle_request(&query).await.map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                processing_labels::error_types::UPSTREAM_ERROR,
            )
        })?;

        // 转换为 Google JSON 格式
        let json_response = dns_message_to_json(response).map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                processing_labels::error_types::JSON_SERIALIZATION_ERROR,
            )
        })?;

        // 构建 HTTP 响应
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static(http_headers::content_types::DNS_JSON),
        );

        record_doh_metrics(start_time, &query_type_str, addr, &Ok(StatusCode::OK), None);

        Ok((headers, json_response))
    }
    .await;

    match result {
        Ok((headers, body)) => (StatusCode::OK, headers, body).into_response(),
        Err((status, error_type)) => {
            record_doh_metrics(start_time, "unknown", addr, &Err(status), Some(error_type));
            status.into_response()
        }
    }
}

/// 记录 DoH 请求的指标和日志
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
    let status_str = status_code.as_str().to_string();

    // 记录请求总数和时长
    METRICS
        .http_requests_total()
        .with_label_values(&[&status_str])
        .inc();
    METRICS
        .http_request_duration_seconds()
        .with_label_values(&[query_type, &status_str])
        .observe(duration);

    // 根据结果记录日志和错误总数
    match result {
        Ok(status) => {
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
