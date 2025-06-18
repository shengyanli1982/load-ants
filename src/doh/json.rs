// src/doh/json.rs

use hickory_proto::op::Message;
use hickory_proto::rr::RData;
use serde_json::{json, Value as JsonValue};

/// 将 DNS 消息转换为 Google JSON 格式
///
/// 按照 Google 公共 DNS API 规范转换 DNS 消息
/// https://developers.google.com/speed/public-dns/docs/doh/json
pub fn dns_message_to_json(message: Message) -> Result<String, serde_json::Error> {
    // 创建基础 JSON 结构
    let mut response = json!({
        "Status": message.response_code().low(),
        "TC": message.truncated(),
        "RD": message.recursion_desired(),
        "RA": message.recursion_available(),
        "AD": message.authentic_data(),
        "CD": message.checking_disabled(),
    });

    // 处理问题部分
    if !message.queries().is_empty() {
        let query = &message.queries()[0];
        response["Question"] = json!([{
            "name": query.name().to_string(),
            "type": u16::from(query.query_type())
        }]);
    }

    // 处理回答部分
    if !message.answers().is_empty() {
        let mut answers = Vec::with_capacity(message.answers().len());
        for record in message.answers() {
            let answer = record_to_json_object(record);
            answers.push(answer);
        }
        response["Answer"] = JsonValue::Array(answers);
    }

    // 处理权威部分
    if !message.name_servers().is_empty() {
        let mut authorities = Vec::with_capacity(message.name_servers().len());
        for record in message.name_servers() {
            let authority = record_to_json_object(record);
            authorities.push(authority);
        }
        response["Authority"] = JsonValue::Array(authorities);
    }

    // 处理附加信息部分
    if !message.additionals().is_empty() {
        let mut additionals = Vec::with_capacity(message.additionals().len());
        for record in message.additionals() {
            let additional = record_to_json_object(record);
            additionals.push(additional);
        }
        response["Additional"] = JsonValue::Array(additionals);
    }

    // 序列化为 JSON 字符串
    serde_json::to_string(&response)
}

/// 将 DNS 记录转换为 JSON 对象
pub fn record_to_json_object(record: &hickory_proto::rr::Record) -> serde_json::Value {
    let mut answer = json!({
        "name": record.name().to_string(),
        "type": u16::from(record.record_type()),
        "TTL": record.ttl(),
    });

    // 根据记录类型设置数据字段
    match record.data() {
        Some(RData::A(addr)) => {
            answer["data"] = JsonValue::String(addr.to_string());
        }
        Some(RData::AAAA(addr)) => {
            answer["data"] = JsonValue::String(addr.to_string());
        }
        Some(RData::CNAME(name)) => {
            answer["data"] = JsonValue::String(name.to_string());
        }
        Some(RData::MX(mx)) => {
            // 使用预分配的String来减少内存分配
            let mut data = String::with_capacity(10 + mx.exchange().to_string().len());
            data.push_str(&mx.preference().to_string());
            data.push(' ');
            data.push_str(&mx.exchange().to_string());
            answer["data"] = JsonValue::String(data);
        }
        Some(RData::NS(name)) => {
            answer["data"] = JsonValue::String(name.to_string());
        }
        Some(RData::PTR(name)) => {
            answer["data"] = JsonValue::String(name.to_string());
        }
        Some(RData::SOA(soa)) => {
            // 预估SOA记录字符串长度，减少内存重分配
            let estimated_len = soa.mname().to_string().len() + soa.rname().to_string().len() + 50; // 为数字和空格预留空间

            let mut data = String::with_capacity(estimated_len);
            data.push_str(&soa.mname().to_string());
            data.push(' ');
            data.push_str(&soa.rname().to_string());
            data.push(' ');
            data.push_str(&soa.serial().to_string());
            data.push(' ');
            data.push_str(&soa.refresh().to_string());
            data.push(' ');
            data.push_str(&soa.retry().to_string());
            data.push(' ');
            data.push_str(&soa.expire().to_string());
            data.push(' ');
            data.push_str(&soa.minimum().to_string());

            answer["data"] = JsonValue::String(data);
        }
        Some(RData::SRV(srv)) => {
            // 预估SRV记录字符串长度
            let estimated_len = srv.target().to_string().len() + 20; // 为数字和空格预留空间

            let mut data = String::with_capacity(estimated_len);
            data.push_str(&srv.priority().to_string());
            data.push(' ');
            data.push_str(&srv.weight().to_string());
            data.push(' ');
            data.push_str(&srv.port().to_string());
            data.push(' ');
            data.push_str(&srv.target().to_string());

            answer["data"] = JsonValue::String(data);
        }
        Some(RData::TXT(txt)) => {
            // 优化TXT记录处理，减少内存分配
            let txt_data = match txt.txt_data().len() {
                0 => String::new(),
                1 => String::from_utf8_lossy(&txt.txt_data()[0]).into_owned(),
                _ => {
                    // 计算总长度以预分配足够空间
                    let total_len = txt.txt_data().iter().map(|bytes| bytes.len()).sum();
                    let mut result = String::with_capacity(total_len);

                    for bytes in txt.txt_data() {
                        result.push_str(&String::from_utf8_lossy(bytes));
                    }
                    result
                }
            };
            answer["data"] = JsonValue::String(txt_data);
        }
        _ => {
            // 对于其他记录类型，使用静态字符串
            answer["data"] = JsonValue::String("Unsupported record type".to_string());
        }
    }

    answer
}
