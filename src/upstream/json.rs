use crate::error::AppError;
use hickory_proto::rr::rdata as HickoryRData;
use hickory_proto::{
    op::{Message, MessageType, ResponseCode},
    rr::{Name, RData, Record, RecordType},
};
use serde_json::{json, Value as JsonValue};
use std::net::{Ipv4Addr, Ipv6Addr};
use tracing::{debug, warn};

// JSON字段常量
pub mod json_fields {
    // 请求字段
    pub const NAME: &str = "name";
    pub const TYPE: &str = "type";
    pub const DO: &str = "do";
    pub const CD: &str = "cd";

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

// DNS常量
pub const DNS_CLASS_IN: u16 = 1;

// DNS状态码常量
pub mod dns_status {
    pub const NO_ERROR: u64 = 0;
    pub const FORM_ERR: u64 = 1;
    pub const SERV_FAIL: u64 = 2;
    pub const NX_DOMAIN: u64 = 3;
    pub const NOT_IMP: u64 = 4;
    pub const REFUSED: u64 = 5;
}

// DNS记录段名称
pub mod dns_section {
    pub const ANSWER: &str = "Answer";
    pub const AUTHORITY: &str = "Authority";
    pub const ADDITIONAL: &str = "Additional";
}

pub struct JsonConverter;

impl JsonConverter {
    // 将DNS消息转换为DNS JSON格式
    // https://developers.google.com/speed/public-dns/docs/doh/json
    pub fn message_to_json(&self, query: &Message) -> Result<JsonValue, AppError> {
        // 创建一个JSON对象以发送给DoH服务器
        let query_param = match query.queries().first() {
            Some(q) => q,
            None => return Err(AppError::Internal("DNS query is empty".to_string())),
        };

        // 基于Google DNS-over-HTTPS JSON API格式
        let mut json_data = json!({
            json_fields::NAME: query_param.name().to_string(),
            json_fields::TYPE: u16::from(query_param.query_type()),
        });

        // 可选参数: 当查询类别不是IN(1)时启用DNSSEC
        if u16::from(query_param.query_class()) != DNS_CLASS_IN {
            // do参数: DNSSEC OK 标志
            json_data[json_fields::DO] = json!(true);
        }

        // cd参数: Checking Disabled 标志，默认为false (启用DNSSEC验证)
        json_data[json_fields::CD] = json!(false);

        // 不添加edns_client_subnet参数，使用默认值
        // 可选: 添加 random_padding 参数以使所有请求大小相同
        // 此处不添加content-type参数，由调用方在HTTP头中设置

        Ok(json_data)
    }

    // 解析DNS JSON响应为DNS消息
    // https://developers.google.com/speed/public-dns/docs/doh/json
    pub fn json_to_message(
        &self,
        json_response: &[u8],
        query: &Message,
    ) -> Result<Message, AppError> {
        // 解析JSON响应
        let json: JsonValue = serde_json::from_slice(json_response)
            .map_err(|e| AppError::Upstream(format!("Failed to parse JSON response: {}", e)))?;

        // 创建新的DNS响应消息
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
            // 默认使用查询中的递归期望设置
            response.set_recursion_desired(query.recursion_desired());
        }

        // RA - 递归可用
        if let Some(ra) = json.get(json_fields::RA).and_then(|ra| ra.as_bool()) {
            response.set_recursion_available(ra);
        } else {
            // 默认为true，Google Public DNS总是支持递归
            response.set_recursion_available(true);
        }

        // AD - 认证数据标志 (DNSSEC验证)
        if let Some(ad) = json.get(json_fields::AD).and_then(|ad| ad.as_bool()) {
            response.set_authentic_data(ad);
        }

        // CD - 禁用检查标志
        if let Some(cd) = json.get(json_fields::CD).and_then(|cd| cd.as_bool()) {
            response.set_checking_disabled(cd);
        }

        // 复制查询部分
        for q in query.queries() {
            response.add_query(q.clone());
        }

        // 处理Status字段，映射到响应码
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

        // 如果状态不是成功，可能不需要进一步处理（但处理Question部分）
        if response.response_code() != ResponseCode::NoError {
            // 即使有错误，Question部分也可能存在
            if let Some(questions) = json.get(json_fields::QUESTION).and_then(|q| q.as_array()) {
                for question in questions {
                    // 只处理第一个Question，因为DNS消息通常只有一个查询
                    if let (Some(name), Some(q_type)) = (
                        question.get(json_fields::NAME).and_then(|n| n.as_str()),
                        question.get(json_fields::TYPE).and_then(|t| t.as_u64()),
                    ) {
                        // 尝试解析域名
                        if let Ok(domain) = Name::parse(name, None) {
                            let record_type = RecordType::from(q_type as u16);
                            // 重新创建查询
                            let query_record = hickory_proto::op::Query::query(domain, record_type);
                            response.add_query(query_record);
                        }
                    }
                }
            }

            // 如果JSON包含Comment字段，记录为调试信息
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
                    // MX记录格式通常为"优先级 主机名"
                    let parts: Vec<&str> = data.split_whitespace().collect();
                    if parts.len() >= 2 {
                        match (parts[0].parse::<u16>(), Name::parse(parts[1], None)) {
                            (Ok(preference), Ok(exchange)) => {
                                let rdata = HickoryRData::MX::new(preference, exchange);
                                Some(Record::from_rdata(name, ttl as u32, RData::MX(rdata)))
                            }
                            _ => {
                                warn!("Failed to parse MX record data {}", data);
                                None
                            }
                        }
                    } else {
                        warn!("Invalid MX record format {}", data);
                        None
                    }
                }
                RecordType::TXT => {
                    // TXT记录可能包含多个引号部分
                    // 处理诸如 "v=spf1 -all" 或 "k=rsa; p=MIGfMA0..." "更多数据"

                    // 去除首尾引号，处理多部分TXT记录
                    let mut txt_data = String::new();
                    let mut in_quotes = false;
                    let mut escaped = false;

                    for c in data.chars() {
                        match c {
                            '"' if !escaped => {
                                in_quotes = !in_quotes;
                                // 不将引号添加到实际数据中
                            }
                            '\\' if !escaped => {
                                escaped = true;
                            }
                            _ => {
                                if !(!in_quotes && c == ' ') {
                                    txt_data.push(c);
                                }
                                escaped = false;
                            }
                        }
                    }

                    // 创建TXT记录
                    let txt_strings = vec![txt_data];
                    let rdata = HickoryRData::TXT::new(txt_strings);
                    Some(Record::from_rdata(name, ttl as u32, RData::TXT(rdata)))
                }
                RecordType::SRV => {
                    // SRV记录格式为"优先级 权重 端口 目标主机名"
                    let parts: Vec<&str> = data.split_whitespace().collect();
                    if parts.len() >= 4 {
                        match (
                            parts[0].parse::<u16>(),     // 优先级
                            parts[1].parse::<u16>(),     // 权重
                            parts[2].parse::<u16>(),     // 端口
                            Name::parse(parts[3], None), // 目标主机名
                        ) {
                            (Ok(priority), Ok(weight), Ok(port), Ok(target)) => {
                                let rdata = HickoryRData::SRV::new(priority, weight, port, target);
                                Some(Record::from_rdata(name, ttl as u32, RData::SRV(rdata)))
                            }
                            _ => {
                                warn!("Failed to parse SRV record data {}", data);
                                None
                            }
                        }
                    } else {
                        warn!("Invalid SRV record format {}", data);
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

        // 处理edns_client_subnet字段
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
