// src/doh/handlers.rs

use crate::doh::json::dns_message_to_json;
use crate::doh::state::AppState;
use crate::metrics::METRICS;
use crate::r#const::{http_headers, processing_labels};
use axum::{
    body::Bytes,
    extract::{Query, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hickory_proto::op::{Message, MessageType};
use std::collections::HashMap;
use std::time::Instant;
use tracing::{debug, error};

/// 处理 DNS 消息并生成响应
///
/// 这是一个内部辅助函数，用于处理 DNS 消息并生成响应，被 GET 和 POST 处理函数共用
async fn process_dns_message(
    state: &AppState,
    dns_message: &Message,
) -> Result<(Vec<u8>, String), StatusCode> {
    // 处理 DNS 请求
    let response = match state.handler.handle_request(dns_message).await {
        Ok(resp) => resp,
        Err(e) => {
            error!("Error processing DNS request: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
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
        Err(e) => {
            error!("Failed to encode DNS response: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    Ok((response_bytes, query_type))
}

/// 处理 RFC 8484 DoH GET 请求
///
/// 处理 DNS 查询，其中 DNS 消息通过 URL 参数传递（base64url 编码）
pub async fn handle_doh_get(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, StatusCode> {
    // 记录请求开始时间
    let start_time = Instant::now();

    // 提取 DNS 查询参数
    let dns_param = match params.get("dns") {
        Some(param) => param,
        None => {
            error!("Missing 'dns' parameter in DoH GET request");
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    // 解码 base64url DNS 消息
    let dns_bytes = match URL_SAFE_NO_PAD.decode(dns_param) {
        Ok(bytes) => bytes,
        Err(e) => {
            error!("Failed to decode DNS message: {}", e);
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    // 解析 DNS 消息
    let dns_message = match Message::from_vec(&dns_bytes) {
        Ok(msg) => msg,
        Err(e) => {
            error!("Failed to parse DNS message: {}", e);
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    // 处理 DNS 消息
    let (response_bytes, query_type) = match process_dns_message(&state, &dns_message).await {
        Ok(result) => result,
        Err(status) => return Err(status),
    };

    // 记录处理时间
    let duration = start_time.elapsed();
    METRICS
        .dns_request_duration_seconds()
        .with_label_values(&[processing_labels::RESOLVED, &query_type])
        .observe(duration.as_secs_f64());

    debug!("DoH GET request processed in {:?}", duration);

    // 构建 HTTP 响应
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static(http_headers::content_types::DNS_MESSAGE),
    );

    Ok((headers, response_bytes))
}

/// 处理 RFC 8484 DoH POST 请求
///
/// 处理 DNS 查询，其中 DNS 消息在 HTTP 请求体中传递
pub async fn handle_doh_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, StatusCode> {
    // 记录请求开始时间
    let start_time = Instant::now();

    // 验证内容类型
    if let Some(content_type) = headers.get(header::CONTENT_TYPE) {
        if content_type != http_headers::content_types::DNS_MESSAGE {
            error!("Invalid content type: {:?}", content_type);
            return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
        }
    } else {
        error!("Missing content type header");
        return Err(StatusCode::BAD_REQUEST);
    }

    // 解析 DNS 消息
    let dns_message = match Message::from_vec(&body) {
        Ok(msg) => msg,
        Err(e) => {
            error!("Failed to parse DNS message: {}", e);
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    // 处理 DNS 消息
    let (response_bytes, query_type) = match process_dns_message(&state, &dns_message).await {
        Ok(result) => result,
        Err(status) => return Err(status),
    };

    // 记录处理时间
    let duration = start_time.elapsed();
    METRICS
        .dns_request_duration_seconds()
        .with_label_values(&[processing_labels::RESOLVED, &query_type])
        .observe(duration.as_secs_f64());

    debug!("DoH POST request processed in {:?}", duration);

    // 构建 HTTP 响应
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static(http_headers::content_types::DNS_MESSAGE),
    );

    Ok((headers, response_bytes))
}

/// 处理 Google JSON 格式的 DoH GET 请求
///
/// 处理 Google 格式的 DNS 查询，参数通过 URL 查询字符串传递
pub async fn handle_json_get(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, StatusCode> {
    // 记录请求开始时间
    let start_time = Instant::now();

    // 提取必要的查询参数
    let name = match params.get("name") {
        Some(name) => name,
        None => {
            error!("Missing 'name' parameter in JSON DoH request");
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    // 提取查询类型 (默认为 1 = A 记录)
    let default_type = String::from("1");
    let type_str = params.get("type").unwrap_or(&default_type);
    let query_type = match type_str.parse::<u16>() {
        Ok(t) => t,
        Err(_) => {
            error!("Invalid 'type' parameter: {}", type_str);
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    // 创建 DNS 查询消息
    let mut query = Message::new();
    query.set_message_type(MessageType::Query);
    query.set_recursion_desired(true);

    // 添加查询问题
    let name_result = match hickory_proto::rr::Name::from_ascii(name) {
        Ok(n) => n,
        Err(e) => {
            error!("Failed to parse domain name: {}", e);
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    let record_type = hickory_proto::rr::RecordType::from(query_type);

    // 创建并添加查询
    let q = hickory_proto::op::Query::query(name_result, record_type);
    query.add_query(q);

    // 处理 DNS 请求
    let response = match state.handler.handle_request(&query).await {
        Ok(resp) => resp,
        Err(e) => {
            error!("Error processing DNS request: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // 转换为 Google JSON 格式
    let json_response = match dns_message_to_json(response) {
        Ok(json) => json,
        Err(e) => {
            error!("Failed to convert DNS message to JSON: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // 记录处理时间
    let duration = start_time.elapsed();
    METRICS
        .dns_request_duration_seconds()
        .with_label_values(&[processing_labels::RESOLVED, &record_type.to_string()])
        .observe(duration.as_secs_f64());

    debug!("JSON DoH request processed in {:?}", duration);

    // 构建 HTTP 响应
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static(http_headers::content_types::DNS_JSON),
    );

    Ok((headers, json_response))
}
