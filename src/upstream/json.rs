use crate::{error::AppError, r#const::http_headers};
use hickory_proto::{
    op::{Message, MessageType, Query, ResponseCode},
    rr::{
        rdata::{self as HickoryRData, MX, SRV, TXT},
        Name, RData, Record, RecordType,
    },
};
use serde_json::{json, Value as JsonValue};
use std::net::{Ipv4Addr, Ipv6Addr};
use tracing::{debug, warn};

// DNS JSON 字段常量。
pub mod json_fields {
    // 请求字段
    pub const NAME: &str = "name";
    pub const TYPE: &str = "type";
    #[allow(dead_code)]
    pub const DO: &str = "do";
    pub const CD: &str = "cd";
    #[allow(dead_code)]
    pub const CT: &str = "ct";

    // 响应字段
    pub const TC: &str = "TC";
    pub const RD: &str = "RD";
    pub const RA: &str = "RA";
    pub const AD: &str = "AD";
    pub const STATUS: &str = "Status";
    pub const QUESTION: &str = "Question";
    pub const ANSWER: &str = "Answer";
    pub const AUTHORITY: &str = "Authority";
    pub const ADDITIONAL: &str = "Additional";
    pub const COMMENT: &str = "Comment";
    pub const TTL: &str = "TTL";
    pub const DATA: &str = "data";
    pub const EDNS_CLIENT_SUBNET: &str = "edns_client_subnet";
}

// DNS 常量。
#[allow(dead_code)]
pub const DNS_CLASS_IN: u16 = 1;

// DNS 状态码常量。
pub mod dns_status {
    pub const NO_ERROR: u64 = 0;
    pub const FORM_ERR: u64 = 1;
    pub const SERV_FAIL: u64 = 2;
    pub const NX_DOMAIN: u64 = 3;
    pub const NOT_IMP: u64 = 4;
    pub const REFUSED: u64 = 5;
}

// DNS 记录分区名称。
pub mod dns_section {
    pub const ANSWER: &str = "Answer";
    pub const AUTHORITY: &str = "Authority";
    pub const ADDITIONAL: &str = "Additional";
}

pub struct JsonConverter;

fn normalize_txt_data(raw: &str) -> String {
    // Google DoH JSON 的 TXT data 通常是一个字符串，有时会包含外层引号，或者使用 `""` 表示拼接。
    // 这里尽量做保守归一化：去掉首尾引号，并把相邻的双引号当作拼接符移除。
    let mut s = raw.trim().to_string();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s = s[1..s.len() - 1].to_string();
    }
    while s.contains("\"\"") {
        s = s.replace("\"\"", "");
    }
    s
}

impl JsonConverter {
    // 将 DNS 消息转换为 DNS JSON 格式。
    // 参考：https://developers.google.com/speed/public-dns/docs/doh/json
    #[allow(dead_code)]
    pub fn message_to_json(&self, query: &Message) -> Result<JsonValue, AppError> {
        // 创建一个 JSON 对象发送给 DoH 服务器。
        let query_param = match query.queries().first() {
            Some(q) => q,
            None => return Err(AppError::Internal("DNS query is empty".to_string())),
        };

        // 按照 Google DNS-over-HTTPS JSON API 的字段组织请求。
        let mut json_data = json!({
            json_fields::NAME: query_param.name().to_string(),
            json_fields::TYPE: u16::from(query_param.query_type()),
        });

        // 可选参数：当查询类别不是 IN(1) 时启用 DNSSEC。
        if u16::from(query_param.query_class()) != DNS_CLASS_IN {
            // `do` 参数表示 DNSSEC OK 标志。
            json_data[json_fields::DO] = json!(true);
        }

        // `cd` 参数表示关闭校验标志，默认 `false` 表示启用 DNSSEC 校验。
        json_data[json_fields::CD] = json!(false);

        // `ct` 参数声明期望返回 JSON 格式。
        json_data[json_fields::CT] = json!(http_headers::content_types::DNS_JSON);

        // 不显式添加 `edns_client_subnet`，沿用服务端默认行为。
        // 这里也不写入 `content-type`，由调用方统一设置 HTTP 头。

        Ok(json_data)
    }

    // 解析 DNS JSON 响应并还原为 DNS 消息。
    // 参考：https://developers.google.com/speed/public-dns/docs/doh/json
    pub fn json_to_message(
        &self,
        json_response: &[u8],
        query: &Message,
    ) -> Result<Message, AppError> {
        // 解析 JSON 响应。
        let json: JsonValue = serde_json::from_slice(json_response)
            .map_err(|e| AppError::Upstream(format!("Failed to parse JSON response: {}", e)))?;

        // 创建新的 DNS 响应消息。
        let mut response = Message::new();
        response.set_id(query.id());
        response.set_message_type(MessageType::Response);
        response.set_op_code(query.op_code());

        // 处理DNS标志位
        // TC - 是否截断
        if let Some(tc) = json.get(json_fields::TC).and_then(|tc| tc.as_bool()) {
            response.set_truncated(tc);
        }

        // RD - 递归期望
        if let Some(rd) = json.get(json_fields::RD).and_then(|rd| rd.as_bool()) {
            response.set_recursion_desired(rd);
        } else {
            // 缺省时沿用原始查询中的递归期望设置。
            response.set_recursion_desired(query.recursion_desired());
        }

        // RA - 递归可用
        if let Some(ra) = json.get(json_fields::RA).and_then(|ra| ra.as_bool()) {
            response.set_recursion_available(ra);
        } else {
            // 缺省时按 `true` 处理，符合 Google Public DNS 的常见行为。
            response.set_recursion_available(true);
        }

        // AD 表示认证数据标志，用于表达 DNSSEC 校验结果。
        if let Some(ad) = json.get(json_fields::AD).and_then(|ad| ad.as_bool()) {
            response.set_authentic_data(ad);
        }

        // CD - 禁用检查标志
        if let Some(cd) = json.get(json_fields::CD).and_then(|cd| cd.as_bool()) {
            response.set_checking_disabled(cd);
        }

        // 将 `Status` 字段映射为标准响应码。
        if let Some(status) = json.get(json_fields::STATUS).and_then(|s| s.as_u64()) {
            let rcode = match status {
                dns_status::NO_ERROR => ResponseCode::NoError,
                dns_status::FORM_ERR => ResponseCode::FormErr,
                dns_status::SERV_FAIL => ResponseCode::ServFail,
                dns_status::NX_DOMAIN => ResponseCode::NXDomain,
                dns_status::NOT_IMP => ResponseCode::NotImp,
                dns_status::REFUSED => ResponseCode::Refused,
                _ => ResponseCode::ServFail,
            };
            response.set_response_code(rcode);
        }

        // 优先使用响应里的 `Question` 段；若缺失或无法解析则回退到原始查询。
        let mut added_query = false;
        if let Some(questions) = json.get(json_fields::QUESTION).and_then(|q| q.as_array()) {
            for question in questions {
                if let (Some(name), Some(q_type)) = (
                    question.get(json_fields::NAME).and_then(|n| n.as_str()),
                    question.get(json_fields::TYPE).and_then(|t| t.as_u64()),
                ) {
                    if let Ok(domain) = Name::parse(name, None) {
                        let record_type = RecordType::from(q_type as u16);
                        response.add_query(Query::query(domain, record_type));
                        added_query = true;
                    }
                }
            }
        }
        if !added_query {
            for q in query.queries() {
                response.add_query(q.clone());
            }
        }

        // 非成功响应通常不再附带可用记录，此时保留已补齐的查询段即可返回。
        if response.response_code() != ResponseCode::NoError {
            // 如果响应包含 `Comment` 字段，则作为调试信息输出。
            if let Some(comment) = json.get(json_fields::COMMENT).and_then(|c| c.as_str()) {
                debug!("DNS JSON response comment: {}", comment);
            }

            return Ok(response);
        }

        // 处理记录的辅助函数
        let parse_record = |record: &JsonValue, section: &str| -> Option<Record> {
            // 获取记录的基本属性
            let name = record.get(json_fields::NAME).and_then(|n| n.as_str())?;
            let r_type = record.get(json_fields::TYPE).and_then(|t| t.as_u64())?;
            let ttl = record.get(json_fields::TTL).and_then(|t| t.as_u64())?;
            let data = record.get(json_fields::DATA).and_then(|d| d.as_str())?;

            // 解析域名
            let name = match Name::parse(name, None) {
                Ok(n) => n,
                Err(e) => {
                    warn!("Failed to parse {} record name {}: {}", section, name, e);
                    return None;
                }
            };

            // 记录类型
            let record_type = RecordType::from(r_type as u16);

            // 根据记录类型创建适当的RData
            match record_type {
                RecordType::A => match data.parse::<Ipv4Addr>() {
                    Ok(addr) => {
                        let octets = addr.octets();
                        let rdata =
                            HickoryRData::A::new(octets[0], octets[1], octets[2], octets[3]);
                        Some(Record::from_rdata(name, ttl as u32, RData::A(rdata)))
                    }
                    Err(e) => {
                        warn!("Failed to parse A record data {}: {}", data, e);
                        None
                    }
                },
                RecordType::AAAA => match data.parse::<Ipv6Addr>() {
                    Ok(addr) => {
                        let segments = addr.segments();
                        let rdata = HickoryRData::AAAA::new(
                            segments[0],
                            segments[1],
                            segments[2],
                            segments[3],
                            segments[4],
                            segments[5],
                            segments[6],
                            segments[7],
                        );
                        Some(Record::from_rdata(name, ttl as u32, RData::AAAA(rdata)))
                    }
                    Err(e) => {
                        warn!("Failed to parse AAAA record data {}: {}", data, e);
                        None
                    }
                },
                RecordType::CNAME => match Name::parse(data, None) {
                    Ok(target) => {
                        let rdata = HickoryRData::CNAME(target);
                        Some(Record::from_rdata(name, ttl as u32, RData::CNAME(rdata)))
                    }
                    Err(e) => {
                        warn!("Failed to parse CNAME record data {}: {}", data, e);
                        None
                    }
                },
                RecordType::MX => {
                    let parts: Vec<&str> = data.split_whitespace().collect();
                    if parts.len() == 2 {
                        if let (Ok(preference), Ok(exchange)) =
                            (parts[0].parse::<u16>(), Name::parse(parts[1], None))
                        {
                            Some(Record::from_rdata(
                                name,
                                ttl as u32,
                                RData::MX(MX::new(preference, exchange)),
                            ))
                        } else {
                            warn!("Failed to parse MX record data '{}'", data);
                            None
                        }
                    } else {
                        warn!("Invalid MX record data format '{}'", data);
                        None
                    }
                }
                RecordType::TXT => {
                    // Google 的 JSON 格式通常把 TXT 记录压成单个字符串，这里按单段 TXT 处理。
                    // 如果服务端返回多段 TXT，需要引入更复杂的拆分逻辑。
                    let txt_data = TXT::new(vec![normalize_txt_data(data)]);
                    Some(Record::from_rdata(name, ttl as u32, RData::TXT(txt_data)))
                }
                RecordType::SRV => {
                    let parts: Vec<&str> = data.split_whitespace().collect();
                    if parts.len() == 4 {
                        if let (Ok(priority), Ok(weight), Ok(port), Ok(target)) = (
                            parts[0].parse::<u16>(),
                            parts[1].parse::<u16>(),
                            parts[2].parse::<u16>(),
                            Name::parse(parts[3], None),
                        ) {
                            Some(Record::from_rdata(
                                name,
                                ttl as u32,
                                RData::SRV(SRV::new(priority, weight, port, target)),
                            ))
                        } else {
                            warn!("Failed to parse SRV record data '{}'", data);
                            None
                        }
                    } else {
                        warn!("Invalid SRV record data format '{}'", data);
                        None
                    }
                }
                RecordType::PTR => match Name::parse(data, None) {
                    Ok(ptrdname) => {
                        let rdata = HickoryRData::PTR(ptrdname);
                        Some(Record::from_rdata(name, ttl as u32, RData::PTR(rdata)))
                    }
                    Err(e) => {
                        warn!("Failed to parse PTR record data {}: {}", data, e);
                        None
                    }
                },
                RecordType::NS => match Name::parse(data, None) {
                    Ok(target) => {
                        let rdata = HickoryRData::NS(target);
                        Some(Record::from_rdata(name, ttl as u32, RData::NS(rdata)))
                    }
                    Err(e) => {
                        warn!("Failed to parse NS record data {}: {}", data, e);
                        None
                    }
                },
                _ => {
                    // 对于其他记录类型，尝试作为未知记录处理
                    warn!("Unsupported record type: {:?}, data: {}", record_type, data);
                    None
                }
            }
        };

        // 处理Answer部分
        if let Some(answers) = json.get(json_fields::ANSWER).and_then(|a| a.as_array()) {
            for answer in answers {
                if let Some(record) = parse_record(answer, dns_section::ANSWER) {
                    response.add_answer(record);
                }
            }
        }

        // 处理Authority部分
        if let Some(authority) = json.get(json_fields::AUTHORITY).and_then(|a| a.as_array()) {
            for auth in authority {
                if let Some(record) = parse_record(auth, dns_section::AUTHORITY) {
                    response.add_name_server(record);
                }
            }
        }

        // 处理Additional部分
        if let Some(additional) = json.get(json_fields::ADDITIONAL).and_then(|a| a.as_array()) {
            for add in additional {
                if let Some(record) = parse_record(add, dns_section::ADDITIONAL) {
                    response.add_additional(record);
                }
            }
        }

        // 处理 `edns_client_subnet` 字段。
        if let Some(ecs) = json
            .get(json_fields::EDNS_CLIENT_SUBNET)
            .and_then(|e| e.as_str())
        {
            debug!("EDNS Client Subnet from DNS JSON response: {}", ecs);
            // 这里可以添加EDNS处理代码，但由于复杂性，我们只记录不处理
            // 以后如果具体需求，可以添加处理代码
        }

        Ok(response)
    }
}
