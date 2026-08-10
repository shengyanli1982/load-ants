use crate::error::AppError;
use crate::metrics::METRICS;
use crate::r#const::{cache_labels, cache_limits, ttl_source_labels};
use hickory_proto::{
    op::{Message, ResponseCode},
    rr::{
        rdata::opt::{EdnsCode, EdnsOption},
        DNSClass, Name, RData, Record, RecordType,
    },
};
use moka::future::Cache;
use moka::policy::Expiry;
use rand::{seq::SliceRandom, thread_rng};
use rustc_hash::FxBuildHasher;
use serde::{Deserialize, Serialize};
use std::{
    net::{Ipv4Addr, Ipv6Addr},
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tracing::{debug, info, warn};

pub enum CacheResult {
    Fresh(Message),
    Stale(Message),
    Miss,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    name: Name,
    record_type: RecordType,
    class: DNSClass,
    ecs_subnet: Option<String>,
}

fn extract_ecs_from_message(message: &Message) -> Option<String> {
    let edns = message.extensions().as_ref()?;
    let edns_option = edns.option(EdnsCode::Subnet)?;

    match edns_option {
        EdnsOption::Subnet(subnet) => {
            let bytes: Vec<u8> = subnet.try_into().ok()?;
            if bytes.len() < 4 {
                return None;
            }

            let family = u16::from_be_bytes([bytes[0], bytes[1]]);
            let source_prefix = bytes[2];

            let addr_bytes = &bytes[4..];

            match family {
                1 => {
                    let mut full_addr = [0u8; 4];
                    let copy_len = addr_bytes.len().min(4);
                    full_addr[..copy_len].copy_from_slice(&addr_bytes[..copy_len]);
                    let ip = Ipv4Addr::from(full_addr);
                    Some(format!("{}/{}", ip, source_prefix))
                }
                2 => {
                    let mut full_addr = [0u8; 16];
                    let copy_len = addr_bytes.len().min(16);
                    full_addr[..copy_len].copy_from_slice(&addr_bytes[..copy_len]);
                    let ip = Ipv6Addr::from(full_addr);
                    Some(format!("{}/{}", ip, source_prefix))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

impl CacheKey {
    pub fn from_message(message: &Message) -> Option<Self> {
        let query = message.queries().first()?;

        Some(Self {
            name: query.name().clone(),
            record_type: query.query_type(),
            class: query.query_class(),
            ecs_subnet: extract_ecs_from_message(message),
        })
    }

    pub fn with_ecs(self, ecs_subnet: String) -> Self {
        Self {
            ecs_subnet: Some(ecs_subnet),
            ..self
        }
    }

    pub fn name(&self) -> &Name {
        &self.name
    }

    pub fn record_type(&self) -> RecordType {
        self.record_type
    }

    pub fn ecs_subnet(&self) -> Option<&str> {
        self.ecs_subnet.as_deref()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CacheEntry {
    pub(crate) message: Arc<Message>,
    pub(crate) timestamp: Instant,
    pub(crate) effective_ttl: u32,
}

// ---- Cache dump/restore DTOs ----

/// 单条缓存转储条目，用于 dump/restore API 的 JSON 序列化
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheDumpEntry {
    /// DNS 查询域名
    pub name: String,
    /// DNS 查询类型（如 "A", "AAAA", "CNAME" 等）
    pub query_type: String,
    /// DNS 响应 wire format 的 hex 编码
    pub response_hex: String,
    /// 过期时间戳（Unix epoch seconds）
    pub expires_at: u64,
    /// EDNS Client Subnet 子网信息（无 ECS 时为 "0.0.0.0/0"）
    pub subnet: String,
}

/// 缓存 restore 操作的统计结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheRestoreStats {
    pub loaded: u64,
    pub skipped_expired: u64,
    pub failed: u64,
}

/// 缓存 dump 响应格式
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheDumpResponse {
    pub entries: Vec<CacheDumpEntry>,
}

/// 缓存 restore 请求体格式
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheRestoreRequest {
    pub entries: Vec<CacheDumpEntry>,
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(s, "{:02x}", b).expect("write hex failed");
    }
    s
}

fn hex_nibble(byte: u8, position: usize) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(format!(
            "invalid hex byte 0x{:02X} at position {}",
            byte, position
        )),
    }
}

fn from_hex(s: &str) -> Result<Vec<u8>, String> {
    let bytes = s.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return Err("hex string has odd length".to_string());
    }
    bytes
        .chunks_exact(2)
        .enumerate()
        .map(|(index, pair)| {
            let position = index * 2;
            let high = hex_nibble(pair[0], position)?;
            let low = hex_nibble(pair[1], position + 1)?;
            Ok((high << 4) | low)
        })
        .collect()
}

struct CacheEntryExpiry {
    stale_while_revalidate_secs: u64,
}

impl Expiry<CacheKey, CacheEntry> for CacheEntryExpiry {
    fn expire_after_create(
        &self,
        _key: &CacheKey,
        value: &CacheEntry,
        _created_at: Instant,
    ) -> Option<Duration> {
        let total = value.effective_ttl as u64 + self.stale_while_revalidate_secs;
        Some(Duration::from_secs(total))
    }

    fn expire_after_read(
        &self,
        _key: &CacheKey,
        _value: &CacheEntry,
        _read_at: Instant,
        duration_until_expiry: Option<Duration>,
        _last_modified_at: Instant,
    ) -> Option<Duration> {
        duration_until_expiry
    }

    fn expire_after_update(
        &self,
        _key: &CacheKey,
        value: &CacheEntry,
        _updated_at: Instant,
        _duration_until_expiry: Option<Duration>,
    ) -> Option<Duration> {
        let total = value.effective_ttl as u64 + self.stale_while_revalidate_secs;
        Some(Duration::from_secs(total))
    }
}

pub struct DnsCache {
    cache: Cache<CacheKey, CacheEntry, FxBuildHasher>,
    size: usize,
    min_ttl: u32,
    max_ttl: u32,
    negative_ttl: u32,
    stale_while_revalidate: u64,
}

impl DnsCache {
    pub fn new(
        size: usize,
        min_ttl: u32,
        max_ttl: u32,
        negative_ttl: Option<u32>,
        stale_while_revalidate: Option<u64>,
    ) -> Self {
        let size = if size == 0 {
            0
        } else {
            size.clamp(cache_limits::MIN_SIZE, cache_limits::MAX_SIZE)
        };
        let min_ttl = min_ttl.clamp(cache_limits::MIN_TTL, cache_limits::MAX_TTL);
        let max_ttl = max_ttl.clamp(cache_limits::MIN_TTL, cache_limits::MAX_TTL);
        let negative_ttl = negative_ttl
            .unwrap_or(cache_limits::DEFAULT_NEGATIVE_TTL)
            .clamp(cache_limits::MIN_TTL, cache_limits::MAX_TTL);
        let stale_while_revalidate = stale_while_revalidate
            .unwrap_or(cache_limits::DEFAULT_STALE_WHILE_REVALIDATE)
            .clamp(0, cache_limits::MAX_STALE_WHILE_REVALIDATE);

        let expiry = CacheEntryExpiry {
            stale_while_revalidate_secs: stale_while_revalidate,
        };

        let hard_ttl_cap = if stale_while_revalidate > 0 {
            cache_limits::MAX_TTL as u64 + stale_while_revalidate
        } else {
            cache_limits::MAX_TTL as u64
        };

        let cache = Cache::builder()
            .max_capacity(size as u64)
            .expire_after(expiry)
            .time_to_live(Duration::from_secs(hard_ttl_cap))
            .build_with_hasher(FxBuildHasher);

        info!(
            "Creating DNS cache - Size: {}, Min TTL: {}s, Max TTL: {}s, Negative TTL: {}s, Stale-while-revalidate: {}s",
            size, min_ttl, max_ttl, negative_ttl, stale_while_revalidate
        );

        METRICS.cache_capacity.set(size as i64);
        METRICS.cache_entries.set(0);

        Self {
            cache,
            size,
            min_ttl,
            max_ttl,
            negative_ttl,
            stale_while_revalidate,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.size > 0
    }

    pub fn stale_while_revalidate_enabled(&self) -> bool {
        self.stale_while_revalidate > 0
    }

    pub async fn get(&self, key: &CacheKey) -> CacheResult {
        let entry = match self.cache.get(key).await {
            Some(e) => e,
            None => {
                return CacheResult::Miss;
            }
        };

        let elapsed_secs = entry.timestamp.elapsed().as_secs();

        if elapsed_secs < entry.effective_ttl as u64 {
            let mut response = entry.message.as_ref().clone();
            // Optimization: skip adjust_message_ttl for trivially small responses.
            // All cached records share the same effective_ttl (normalized at insert),
            // so the adjustment is uniform; skipping it for 0-1 answers avoids the
            // iteration overhead on the cache hit hot path.
            if entry.message.answer_count() > 1 {
                self.adjust_message_ttl(&mut response, &entry);
            }

            if key.record_type == RecordType::A || key.record_type == RecordType::AAAA {
                self.shuffle_message_records(&mut response, key.record_type);
            }

            METRICS
                .cache_operations_total
                .with_label_values(&[cache_labels::HIT])
                .inc();

            CacheResult::Fresh(response)
        } else if self.stale_while_revalidate > 0
            && elapsed_secs < entry.effective_ttl as u64 + self.stale_while_revalidate
        {
            let mut response = entry.message.as_ref().clone();

            let stale_ttl = entry
                .effective_ttl
                .saturating_sub(elapsed_secs as u32)
                .max(1);

            for record in response.answers_mut() {
                record.set_ttl(stale_ttl);
            }
            for record in response.name_servers_mut() {
                record.set_ttl(stale_ttl);
            }
            for record in response.additionals_mut() {
                if record.record_type() == RecordType::OPT {
                    continue;
                }
                record.set_ttl(stale_ttl);
            }

            if key.record_type == RecordType::A || key.record_type == RecordType::AAAA {
                self.shuffle_message_records(&mut response, key.record_type);
            }

            METRICS
                .cache_operations_total
                .with_label_values(&[cache_labels::STALE])
                .inc();

            CacheResult::Stale(response)
        } else {
            self.cache.invalidate(key).await;
            METRICS
                .cache_operations_total
                .with_label_values(&[cache_labels::MISS])
                .inc();
            CacheResult::Miss
        }
    }

    pub async fn get_stale_entry(&self, query: &Message) -> Option<Message> {
        if self.stale_while_revalidate == 0 {
            return None;
        }

        let key = CacheKey::from_message(query)?;
        let entry = self.cache.get(&key).await?;
        let elapsed_secs = entry.timestamp.elapsed().as_secs();

        if elapsed_secs < entry.effective_ttl as u64 {
            let mut response = entry.message.as_ref().clone();
            if entry.message.answer_count() > 1 {
                self.adjust_message_ttl(&mut response, &entry);
            }
            if key.record_type == RecordType::A || key.record_type == RecordType::AAAA {
                self.shuffle_message_records(&mut response, key.record_type);
            }
            return Some(response);
        }

        if elapsed_secs < entry.effective_ttl as u64 + self.stale_while_revalidate {
            let mut response = entry.message.as_ref().clone();
            let stale_ttl = entry
                .effective_ttl
                .saturating_sub(elapsed_secs as u32)
                .max(1);
            for record in response.answers_mut() {
                record.set_ttl(stale_ttl);
            }
            for record in response.name_servers_mut() {
                record.set_ttl(stale_ttl);
            }
            for record in response.additionals_mut() {
                if record.record_type() == RecordType::OPT {
                    continue;
                }
                record.set_ttl(stale_ttl);
            }
            if key.record_type == RecordType::A || key.record_type == RecordType::AAAA {
                self.shuffle_message_records(&mut response, key.record_type);
            }
            return Some(response);
        }

        None
    }

    fn shuffle_message_records(&self, message: &mut Message, record_type: RecordType) {
        let answers = message.answers_mut();

        if answers.len() < 2 {
            return;
        }

        let mut target_end = 0;
        for i in 0..answers.len() {
            if answers[i].record_type() == record_type {
                answers.swap(i, target_end);
                target_end += 1;
            }
        }

        if target_end < 2 {
            return;
        }

        answers[0..target_end].shuffle(&mut thread_rng());
    }

    pub async fn insert(&self, query: &Message, response: Message) -> Result<(), AppError> {
        if !self.is_cacheable(&response) {
            debug!("Response not cacheable");
            return Ok(());
        }

        let key = match CacheKey::from_message(query) {
            Some(k) => k,
            None => {
                debug!("Cannot create cache key from query");
                METRICS
                    .cache_operations_total
                    .with_label_values(&[cache_labels::INSERT_ERROR])
                    .inc();
                return Err(AppError::Cache(
                    "Cannot create cache key from query".to_string(),
                ));
            }
        };

        // `from_message` already validated that a query exists and populated
        // `key.name`. Reuse it directly to avoid re-extracting from the query.
        let query_name = &key.name;

        let original_answer_count = response.answer_count();

        let mut filtered_response = filter_response_records(query_name, response);

        if filtered_response.answer_count() == 0 && original_answer_count > 0 {
            warn!(
                "Cache poisoning detected: all answer records rejected for query {} - domain mismatch",
                query_name.to_utf8()
            );
            return Ok(());
        }

        let ttl = self
            .calculate_min_ttl(&filtered_response)
            .clamp(cache_limits::MIN_TTL, cache_limits::MAX_TTL);

        METRICS
            .cache_ttl_seconds
            .with_label_values(&[ttl_source_labels::ADJUSTED])
            .observe(ttl as f64);

        // P1-3: 存入缓存前将所有记录 TTL 规范化为 effective_ttl，
        // 确保 adjust_message_ttl() 的减法与缓存过期时间对齐。
        for record in filtered_response.answers_mut() {
            record.set_ttl(ttl);
        }
        for record in filtered_response.name_servers_mut() {
            record.set_ttl(ttl);
        }
        for record in filtered_response.additionals_mut() {
            if record.record_type() != RecordType::OPT {
                record.set_ttl(ttl);
            }
        }

        let entry = CacheEntry {
            message: Arc::new(filtered_response),
            timestamp: Instant::now(),
            effective_ttl: ttl,
        };

        debug!("Added to cache - {} ({:?})", key.name, key.record_type);
        self.cache.insert(key, entry).await;

        METRICS
            .cache_operations_total
            .with_label_values(&[cache_labels::INSERT])
            .inc();

        Ok(())
    }

    pub async fn clear(&self) {
        debug!("Clearing DNS cache");
        self.cache.invalidate_all();

        METRICS
            .cache_operations_total
            .with_label_values(&[cache_labels::CLEAR])
            .inc();
        METRICS.cache_entries.set(0);
    }

    pub fn is_cacheable(&self, response: &Message) -> bool {
        // 不缓存错误响应（SERVFAIL、REFUSED 等），避免上游临时故障持续影响
        if matches!(
            response.response_code(),
            ResponseCode::ServFail
                | ResponseCode::Refused
                | ResponseCode::FormErr
                | ResponseCode::NotImp
        ) {
            debug!(
                "Response not cacheable: error response code {:?}",
                response.response_code()
            );
            return false;
        }

        if response.queries().is_empty() {
            debug!("Response contains no query, not caching");
            return false;
        }

        if response.response_code() == ResponseCode::NoError && response.answer_count() > 0 {
            let raw_min_ttl = response
                .answers()
                .iter()
                .map(|r| r.ttl())
                .min()
                .unwrap_or(u32::MAX);
            if raw_min_ttl == 0 {
                debug!("Response has TTL=0, not cacheable per RFC 2308");
                return false;
            }
        }

        true
    }

    pub fn calculate_min_ttl(&self, response: &Message) -> u32 {
        if response.response_code() == ResponseCode::NXDomain {
            let soa_min_ttl = response
                .name_servers()
                .iter()
                .filter_map(|r| match r.data() {
                    RData::SOA(soa) => Some(soa.minimum()),
                    _ => None,
                })
                .min();

            let ttl = match soa_min_ttl {
                Some(soa_ttl) => soa_ttl.min(self.negative_ttl),
                None => self.negative_ttl,
            };

            METRICS
                .cache_ttl_seconds
                .with_label_values(&[ttl_source_labels::NEGATIVE_TTL])
                .observe(ttl as f64);

            debug!(
                "Using negative cache TTL ({} seconds) for NXDOMAIN, SOA minimum: {:?}",
                ttl, soa_min_ttl
            );

            return ttl;
        }

        if response.response_code() == ResponseCode::NoError && response.answer_count() == 0 {
            let soa_min_ttl = response
                .name_servers()
                .iter()
                .filter_map(|r| match r.data() {
                    RData::SOA(soa) => Some(soa.minimum()),
                    _ => None,
                })
                .min();
            let ttl = soa_min_ttl
                .map(|v| v.min(self.negative_ttl))
                .unwrap_or(self.negative_ttl);

            METRICS
                .cache_ttl_seconds
                .with_label_values(&[ttl_source_labels::NEGATIVE_TTL])
                .observe(ttl as f64);

            debug!(
                "Using TTL ({} seconds) for NODATA response, SOA minimum: {:?}",
                ttl, soa_min_ttl
            );

            return ttl;
        }

        let mut min_ttl = u32::MAX;

        for record in response.answers() {
            min_ttl = min_ttl.min(record.ttl());
        }

        if min_ttl != u32::MAX {
            METRICS
                .cache_ttl_seconds
                .with_label_values(&[ttl_source_labels::ORIGINAL])
                .observe(min_ttl as f64);
        }

        if min_ttl == u32::MAX {
            min_ttl = self.min_ttl;

            METRICS
                .cache_ttl_seconds
                .with_label_values(&[ttl_source_labels::MIN_TTL])
                .observe(min_ttl as f64);
        } else {
            let before_adjustment = min_ttl;
            min_ttl = min_ttl.max(self.min_ttl);

            if before_adjustment != min_ttl {
                METRICS
                    .cache_ttl_seconds
                    .with_label_values(&[ttl_source_labels::MIN_TTL])
                    .observe(min_ttl as f64);
            }

            let before_max = min_ttl;
            min_ttl = min_ttl.min(self.max_ttl);

            if before_max != min_ttl {
                METRICS
                    .cache_ttl_seconds
                    .with_label_values(&[ttl_source_labels::MAX_TTL])
                    .observe(min_ttl as f64);
            }
        }

        min_ttl
    }

    fn adjust_message_ttl(&self, message: &mut Message, entry: &CacheEntry) {
        let elapsed = entry.timestamp.elapsed().as_secs().min(u32::MAX as u64) as u32;

        fn adjust_record_ttl(record: &mut Record, elapsed: u32) {
            let original_ttl = record.ttl();
            record.set_ttl(if original_ttl > elapsed {
                original_ttl - elapsed
            } else {
                1
            });
        }

        for record in message.answers_mut() {
            adjust_record_ttl(record, elapsed);
        }

        for record in message.name_servers_mut() {
            adjust_record_ttl(record, elapsed);
        }

        for record in message.additionals_mut() {
            if record.record_type() == RecordType::OPT {
                continue;
            }
            adjust_record_ttl(record, elapsed);
        }
    }

    pub fn approximate_len(&self) -> usize {
        self.cache.entry_count() as usize
    }

    /// 遍历缓存中所有未过期条目，返回可序列化的 dump 格式列表
    pub fn iter_entries(&self) -> Vec<CacheDumpEntry> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.cache
            .iter()
            .filter_map(|(key, entry)| {
                let instant_now = Instant::now();
                let elapsed = if entry.timestamp <= instant_now {
                    instant_now.duration_since(entry.timestamp).as_secs()
                } else {
                    0
                };
                // 跳过已超出有效 TTL 的条目（含 stale window）
                let total_ttl = entry.effective_ttl as u64 + self.stale_while_revalidate;
                if elapsed >= total_ttl {
                    return None;
                }
                let expires_at = now + total_ttl.saturating_sub(elapsed);
                let wire = entry.message.to_vec().ok()?;
                Some(CacheDumpEntry {
                    name: key.name().to_utf8(),
                    query_type: key.record_type().to_string(),
                    response_hex: to_hex(&wire),
                    expires_at,
                    subnet: key.ecs_subnet().unwrap_or("0.0.0.0/0").to_string(),
                })
            })
            .collect()
    }

    /// 从 dump 数据恢复缓存条目，返回加载统计
    pub async fn insert_restored(&self, entry: CacheDumpEntry) -> CacheRestoreStats {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // 跳过已过期条目
        if entry.expires_at <= now {
            debug!(
                "Skipping expired cache entry during restore: {} (expired at {})",
                entry.name, entry.expires_at
            );
            return CacheRestoreStats {
                loaded: 0,
                skipped_expired: 1,
                failed: 0,
            };
        }

        // 解析 DNS 名称
        let name = match Name::from_str(&entry.name) {
            Ok(n) => n,
            Err(e) => {
                warn!(
                    "Failed to parse name '{}' during restore: {}",
                    entry.name, e
                );
                return CacheRestoreStats {
                    loaded: 0,
                    skipped_expired: 0,
                    failed: 1,
                };
            }
        };

        // 解析查询类型
        let record_type = match RecordType::from_str(&entry.query_type) {
            Ok(rt) => rt,
            Err(e) => {
                warn!(
                    "Failed to parse record type '{}' during restore: {}",
                    entry.query_type, e
                );
                return CacheRestoreStats {
                    loaded: 0,
                    skipped_expired: 0,
                    failed: 1,
                };
            }
        };

        // 解码响应 wire format
        let wire_bytes = match from_hex(&entry.response_hex) {
            Ok(b) => b,
            Err(e) => {
                warn!(
                    "Failed to decode hex response for '{}' during restore: {}",
                    entry.name, e
                );
                return CacheRestoreStats {
                    loaded: 0,
                    skipped_expired: 0,
                    failed: 1,
                };
            }
        };

        let message = match Message::from_vec(&wire_bytes) {
            Ok(m) => m,
            Err(e) => {
                warn!(
                    "Failed to parse DNS message for '{}' during restore: {}",
                    entry.name, e
                );
                return CacheRestoreStats {
                    loaded: 0,
                    skipped_expired: 0,
                    failed: 1,
                };
            }
        };

        // 计算剩余有效 TTL；需减去 stale window 以与 Expiry trait 对齐
        // moka 的 total TTL = effective_ttl + stale_while_revalidate_secs
        // 因此 effective_ttl = remaining - stale_window
        let remaining = entry.expires_at - now;
        let effective_ttl = remaining.saturating_sub(self.stale_while_revalidate).max(1);

        let key = CacheKey {
            name,
            record_type,
            class: DNSClass::IN,
            ecs_subnet: if entry.subnet == "0.0.0.0/0" {
                None
            } else {
                Some(entry.subnet)
            },
        };

        let cache_entry = CacheEntry {
            message: Arc::new(message),
            timestamp: Instant::now(),
            effective_ttl: effective_ttl as u32,
        };

        debug!("Restored cache entry: {} ({})", key.name, key.record_type);
        self.cache.insert(key, cache_entry).await;

        CacheRestoreStats {
            loaded: 1,
            skipped_expired: 0,
            failed: 0,
        }
    }
}

const MAX_CNAME_DEPTH: usize = 16;

pub fn build_cname_chain(query_name: &Name, answers: &[Record]) -> Vec<Name> {
    let mut chain = Vec::new();
    chain.push(query_name.clone());

    // Fast path: no CNAME records means the chain is just the query name.
    // Avoid the second Name clone and the full scanning loop.
    if !answers.iter().any(|r| r.record_type() == RecordType::CNAME) {
        return chain;
    }

    let mut current = query_name.clone();
    let mut depth = 0;
    let mut changed = true;
    while changed && depth < MAX_CNAME_DEPTH {
        changed = false;
        depth += 1;
        for record in answers {
            if record.record_type() == RecordType::CNAME && record.name() == &current {
                if let RData::CNAME(cname) = record.data() {
                    let target = cname.0.clone();
                    if !chain.contains(&target) {
                        chain.push(target.clone());
                        current = target;
                        changed = true;
                        break; // 找到一个即止，避免同名多条 CNAME 触发额外迭代
                    }
                }
            }
        }
    }

    if depth >= MAX_CNAME_DEPTH && changed {
        warn!(
            "CNAME chain depth limit ({}) reached for {}",
            MAX_CNAME_DEPTH,
            query_name.to_utf8()
        );
    }

    chain
}

pub fn filter_answer_records(query_name: &Name, mut response: Message) -> Message {
    let chain = build_cname_chain(query_name, response.answers());
    let original_count = response.answer_count();
    let answers = response.take_answers();
    let valid: Vec<Record> = answers
        .into_iter()
        .filter(|record| chain.contains(record.name()))
        .collect();
    let rejected_count = original_count as usize - valid.len();
    if rejected_count > 0 {
        warn!(
            "Rejected {} answer record(s) with mismatched domain for query {}",
            rejected_count,
            query_name.to_utf8()
        );
    }
    response.insert_answers(valid);
    response
}

pub fn filter_response_records(query_name: &Name, mut response: Message) -> Message {
    let chain = build_cname_chain(query_name, response.answers());

    let original_answer_count = response.answer_count();
    let answers = response.take_answers();
    let valid_answers: Vec<Record> = answers
        .into_iter()
        .filter(|record| chain.contains(record.name()))
        .collect();
    let rejected_answer_count = original_answer_count as usize - valid_answers.len();
    if rejected_answer_count > 0 {
        warn!(
            "Rejected {} answer record(s) with mismatched domain for query {}",
            rejected_answer_count,
            query_name.to_utf8()
        );
    }

    // Fast path: if there are no authority or additional records (common for
    // simple A/AAAA responses), skip the name-list construction required for
    // domain validation on those sections.
    if response.name_servers().is_empty() && response.additionals().is_empty() {
        response.insert_answers(valid_answers);
        return response;
    }

    // Valid answer names are a subset of the chain (enforced by the filter
    // above), so the chain alone seeds the validation list.
    let mut valid_names: Vec<Name> = chain;

    let original_ns_count = response.name_servers().len();
    let ns_records = response.take_name_servers();
    let valid_ns: Vec<Record> = ns_records
        .into_iter()
        .filter(|record| valid_names.contains(record.name()))
        .collect();
    for record in &valid_ns {
        let name = record.name();
        if !valid_names.contains(name) {
            valid_names.push(name.clone());
        }
    }
    let rejected_ns_count = original_ns_count - valid_ns.len();
    if rejected_ns_count > 0 {
        warn!(
            "Rejected {} NS record(s) with mismatched domain for query {}",
            rejected_ns_count,
            query_name.to_utf8()
        );
    }

    let original_additional_count = response.additionals().len();
    let additional_records = response.take_additionals();
    let valid_additional: Vec<Record> = additional_records
        .into_iter()
        .filter(|record| {
            record.record_type() == RecordType::OPT || valid_names.contains(record.name())
        })
        .collect();
    let rejected_additional_count = original_additional_count - valid_additional.len();
    if rejected_additional_count > 0 {
        warn!(
            "Rejected {} additional record(s) with mismatched domain for query {}",
            rejected_additional_count,
            query_name.to_utf8()
        );
    }

    response.insert_answers(valid_answers);
    response.insert_name_servers(valid_ns);
    response.insert_additionals(valid_additional);
    response
}
