// src/doh/json.rs

use hickory_proto::op::{Message, Query};
use hickory_proto::rr::{RData, Record};
use serde::ser::{SerializeSeq, SerializeStruct};
use serde::{Serialize, Serializer};
use std::fmt::Write;

/// 一个包装器结构，用于为 `hickory_proto::op::Message` 实现 `serde::Serialize`。
///
/// 这个结构体通过借用一个 `Message` 并为其实现 `Serialize` trait，
/// 允许我们将 DNS 消息直接流式序列化为 JSON，而无需创建中间的 `serde_json::Value`，
/// 从而显著提高性能并减少内存分配。
pub struct SerializableDnsMessage<'a>(pub &'a Message);

impl<'a> Serialize for SerializableDnsMessage<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // 估算字段数量以进行优化
        let mut field_count = 6;
        if !self.0.queries().is_empty() {
            field_count += 1;
        }
        if !self.0.answers().is_empty() {
            field_count += 1;
        }
        if !self.0.name_servers().is_empty() {
            field_count += 1;
        }
        if !self.0.additionals().is_empty() {
            field_count += 1;
        }

        let mut state = serializer.serialize_struct("DnsJsonResponse", field_count)?;

        // 序列化核心状态字段
        state.serialize_field("Status", &self.0.response_code().low())?;
        state.serialize_field("TC", &self.0.truncated())?;
        state.serialize_field("RD", &self.0.recursion_desired())?;
        state.serialize_field("RA", &self.0.recursion_available())?;
        state.serialize_field("AD", &self.0.authentic_data())?;
        state.serialize_field("CD", &self.0.checking_disabled())?;

        // 序列化问题部分
        if !self.0.queries().is_empty() {
            state.serialize_field("Question", &SerializableQueries(self.0.queries()))?;
        }

        // 序列化回答部分
        if !self.0.answers().is_empty() {
            state.serialize_field("Answer", &SerializableRecords(self.0.answers()))?;
        }

        // 序列化权威部分
        if !self.0.name_servers().is_empty() {
            state.serialize_field("Authority", &SerializableRecords(self.0.name_servers()))?;
        }

        // 序列化附加信息部分
        if !self.0.additionals().is_empty() {
            state.serialize_field("Additional", &SerializableRecords(self.0.additionals()))?;
        }

        state.end()
    }
}

// 辅助结构体和实现，用于序列化记录切片
struct SerializableRecords<'a>(&'a [Record]);

impl<'a> Serialize for SerializableRecords<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for record in self.0 {
            seq.serialize_element(&SerializableRecord(record))?;
        }
        seq.end()
    }
}

// 辅助结构体和实现，用于序列化单个记录
struct SerializableRecord<'a>(&'a Record);

impl<'a> Serialize for SerializableRecord<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("DnsRecord", 4)?;
        state.serialize_field("name", &self.0.name().to_string())?;
        state.serialize_field("type", &u16::from(self.0.record_type()))?;
        state.serialize_field("TTL", &self.0.ttl())?;

        // 专门处理 RData
        let rdata_string = rdata_to_string(self.0.data());
        state.serialize_field("data", &rdata_string)?;

        state.end()
    }
}

// 辅助结构体和实现，用于序列化问题切片
struct SerializableQueries<'a>(&'a [Query]);

// 辅助结构体和实现，用于序列化单个问题
struct SerializableQuery<'a>(&'a Query);

impl<'a> Serialize for SerializableQuery<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut q_map = serializer.serialize_struct("DnsQuestion", 2)?;
        q_map.serialize_field("name", &self.0.name().to_string())?;
        q_map.serialize_field("type", &u16::from(self.0.query_type()))?;
        q_map.end()
    }
}

impl<'a> Serialize for SerializableQueries<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for query in self.0 {
            seq.serialize_element(&SerializableQuery(query))?;
        }
        seq.end()
    }
}

/// 高效地将 `RData` 转换为字符串，尽可能减少内存分配。
fn rdata_to_string(rdata: Option<&RData>) -> String {
    let rdata = match rdata {
        Some(r) => r,
        None => return String::new(),
    };

    match rdata {
        RData::A(addr) => addr.to_string(),
        RData::AAAA(addr) => addr.to_string(),
        RData::CNAME(name) => name.to_string(),
        RData::NS(name) => name.to_string(),
        RData::PTR(name) => name.to_string(),
        RData::MX(mx) => {
            let mut s = String::with_capacity(mx.exchange().to_string().len() + 8);
            write!(s, "{} {}", mx.preference(), mx.exchange()).unwrap();
            s
        }
        RData::SOA(soa) => {
            let mut s = String::with_capacity(
                soa.mname().to_string().len() + soa.rname().to_string().len() + 40,
            );
            write!(
                s,
                "{} {} {} {} {} {} {}",
                soa.mname(),
                soa.rname(),
                soa.serial(),
                soa.refresh(),
                soa.retry(),
                soa.expire(),
                soa.minimum()
            )
            .unwrap();
            s
        }
        RData::SRV(srv) => {
            let mut s = String::with_capacity(srv.target().to_string().len() + 20);
            write!(
                s,
                "{} {} {} {}",
                srv.priority(),
                srv.weight(),
                srv.port(),
                srv.target()
            )
            .unwrap();
            s
        }
        RData::SVCB(svcb) => {
            // Handles both SVCB and HTTPS records (RFC 9460).
            // The format is: SvcPriority SvcDomainName SvcParams...
            // e.g., "1 example.com alpn=h2,h3 port=443"
            let mut s = String::with_capacity(svcb.target_name().to_string().len() + 48); // Pre-allocate
            write!(s, "{} {}", svcb.svc_priority(), svcb.target_name()).unwrap();

            // The Display trait for SvcParam is expected to format as "key=value".
            for (key, value) in svcb.svc_params() {
                write!(
                    s,
                    " {
                }={
                }",
                    key, value
                )
                .unwrap();
            }
            s
        }
        RData::TXT(txt) => {
            // from_utf8_lossy 是高效的，只有在需要修复非UTF8序列时才会分配
            txt.txt_data()
                .iter()
                .map(|bytes| String::from_utf8_lossy(bytes))
                .collect::<Vec<_>>()
                .join(" ") // Google JSON 通常将多个 TXT 块合并为一个字符串
        }
        // 对于其他所有类型，返回一个标准的字符串表示
        _ => rdata.to_string(),
    }
}
