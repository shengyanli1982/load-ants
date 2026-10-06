use hickory_proto::{
    op::{Message, MessageType, OpCode, Query, ResponseCode},
    rr::{
        rdata::{A, SOA},
        Name, RData, Record, RecordType,
    },
};
use loadants::{
    build_cname_chain, cache::CacheEntry, filter_answer_records, filter_response_records, CacheKey,
    CacheResult, DnsCache,
};
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn make_query(name: &str, record_type: RecordType) -> Message {
    let mut msg = Message::new();
    msg.set_id(1);
    msg.set_message_type(MessageType::Query);
    msg.set_op_code(OpCode::Query);
    msg.set_recursion_desired(true);
    msg.add_query(Query::query(Name::from_ascii(name).unwrap(), record_type));
    msg
}

fn make_response(query: &Message, code: ResponseCode, answers: Vec<Record>) -> Message {
    let mut msg = Message::new();
    msg.set_id(query.id());
    msg.set_message_type(MessageType::Response);
    msg.set_op_code(query.op_code());
    msg.set_recursion_desired(query.recursion_desired());
    msg.set_recursion_available(true);
    msg.set_response_code(code);
    for q in query.queries() {
        msg.add_query(q.clone());
    }
    let answer_count = answers.len() as u16;
    for ans in answers {
        msg.add_answer(ans);
    }
    let mut header = *msg.header();
    header.set_answer_count(answer_count);
    msg.set_header(header);
    msg
}

fn make_a_record(name: &str, ttl: u32, ip: Ipv4Addr) -> Record {
    Record::from_rdata(Name::from_ascii(name).unwrap(), ttl, RData::A(A::from(ip)))
}

fn make_cname_record(name: &str, ttl: u32, target: &str) -> Record {
    Record::from_rdata(
        Name::from_ascii(name).unwrap(),
        ttl,
        RData::CNAME(hickory_proto::rr::rdata::CNAME(
            Name::from_ascii(target).unwrap(),
        )),
    )
}

#[tokio::test]
async fn test_cache_insert_and_get() {
    let cache = DnsCache::new(100, 10, 300, Some(60), None);

    let query = make_query("example.com.", RecordType::A);
    let answers = vec![make_a_record(
        "example.com.",
        300,
        Ipv4Addr::new(1, 2, 3, 4),
    )];
    let response = make_response(&query, ResponseCode::NoError, answers);

    cache.insert(&query, response).await.unwrap();

    let key = CacheKey::from_message(&query).unwrap();
    match cache.get(&key).await {
        CacheResult::Fresh(cached_response) => {
            assert_eq!(cached_response.response_code(), ResponseCode::NoError);
            assert_eq!(cached_response.answer_count(), 1);
            assert_eq!(cached_response.answers()[0].data().to_string(), "1.2.3.4");
        }
        CacheResult::Stale(_) => panic!("expected Fresh, got Stale"),
        CacheResult::Miss => panic!("expected cache hit, got Miss"),
    }
}

#[tokio::test]
async fn test_cache_negative_caching() {
    let negative_ttl = 60;
    let cache = DnsCache::new(100, 10, 300, Some(negative_ttl), None);

    let query = make_query("nx.example.com.", RecordType::A);
    let nxd_response = make_response(&query, ResponseCode::NXDomain, vec![]);
    cache.insert(&query, nxd_response).await.unwrap();

    let key = CacheKey::from_message(&query).unwrap();
    match cache.get(&key).await {
        CacheResult::Fresh(cached) => {
            assert_eq!(cached.response_code(), ResponseCode::NXDomain);
        }
        CacheResult::Stale(_) => panic!("expected Fresh, got Stale"),
        CacheResult::Miss => panic!("expected negative cache hit, got Miss"),
    }

    // P1-4 修复：ServFail 不应被缓存，避免上游临时故障持续影响
    let servfail_query = make_query("fail.example.com.", RecordType::A);
    let servfail_response = make_response(&servfail_query, ResponseCode::ServFail, vec![]);
    cache
        .insert(&servfail_query, servfail_response)
        .await
        .unwrap();

    let servfail_key = CacheKey::from_message(&servfail_query).unwrap();
    match cache.get(&servfail_key).await {
        CacheResult::Fresh(_) => panic!("ServFail should not be cached (P1-4 fix)"),
        CacheResult::Stale(_) => panic!("ServFail should not be cached (P1-4 fix)"),
        CacheResult::Miss => {} // 正确：ServFail 不缓存
    }

    let ttl = cache.calculate_min_ttl(&make_response(&query, ResponseCode::NXDomain, vec![]));
    assert_eq!(
        ttl, negative_ttl,
        "negative TTL should be used for NXDomain"
    );
}

#[tokio::test]
async fn test_cache_ttl_bounds() {
    let min_ttl = 60;
    let max_ttl = 600;
    let negative_ttl = 30;
    let cache = DnsCache::new(100, min_ttl, max_ttl, Some(negative_ttl), None);

    let query = make_query("lowttl.example.com.", RecordType::A);
    let response_low = make_response(
        &query,
        ResponseCode::NoError,
        vec![make_a_record(
            "lowttl.example.com.",
            5,
            Ipv4Addr::new(1, 2, 3, 4),
        )],
    );
    let ttl = cache.calculate_min_ttl(&response_low);
    assert_eq!(
        ttl, min_ttl,
        "TTL below min_ttl should be clamped to min_ttl"
    );

    let response_high = make_response(
        &query,
        ResponseCode::NoError,
        vec![make_a_record(
            "highttl.example.com.",
            99999,
            Ipv4Addr::new(5, 6, 7, 8),
        )],
    );
    let ttl = cache.calculate_min_ttl(&response_high);
    assert_eq!(
        ttl, max_ttl,
        "TTL above max_ttl should be clamped to max_ttl"
    );

    let response_normal = make_response(
        &query,
        ResponseCode::NoError,
        vec![make_a_record(
            "normalttl.example.com.",
            200,
            Ipv4Addr::new(9, 10, 11, 12),
        )],
    );
    let ttl = cache.calculate_min_ttl(&response_normal);
    assert_eq!(ttl, 200, "TTL within bounds should remain unchanged");
}

#[tokio::test]
async fn test_cache_answer_domain_validation() {
    let query = make_query("example.com.", RecordType::A);
    let answers = vec![
        make_a_record("example.com.", 300, Ipv4Addr::new(1, 2, 3, 4)),
        make_cname_record("example.com.", 300, "alias.example.com."),
        make_a_record("alias.example.com.", 300, Ipv4Addr::new(5, 6, 7, 8)),
        make_a_record("unrelated.org.", 300, Ipv4Addr::new(9, 10, 11, 12)),
    ];
    let response = make_response(&query, ResponseCode::NoError, answers);
    let query_name = Name::from_ascii("example.com.").unwrap();
    let filtered = filter_answer_records(&query_name, response);

    assert_eq!(
        filtered.answers().len(),
        3,
        "unrelated domain records should be filtered out"
    );
    for record in filtered.answers() {
        let name_str = record.name().to_string();
        assert!(
            name_str.starts_with("example.com") || name_str.starts_with("alias.example.com"),
            "only CNAME chain domains should remain: got {}",
            name_str
        );
    }
}

#[tokio::test]
async fn insert_rejects_caching_when_all_answers_filtered_out() {
    // P1-1 回归：全部答案均不在查询 CNAME chain 内（上游投毒/名称不匹配）时，
    // filter_response_records 会全拒；insert() 必须走 SEC-1 投毒分支
    // （warn + 放弃缓存），不得把空 NOERROR 响应以原始 answers 的长 TTL 缓存。
    let cache = DnsCache::new(100, 10, 300, Some(60), None);

    let query = make_query("example.com.", RecordType::A);
    let answers = vec![
        make_a_record("unrelated.org.", 300, Ipv4Addr::new(9, 10, 11, 12)),
        make_a_record("evil.example.net.", 300, Ipv4Addr::new(13, 14, 15, 16)),
    ];
    let response = make_response(&query, ResponseCode::NoError, answers);

    // 前置条件：header 计数与记录段同步（make_response 保证），且过滤确实全拒
    assert_eq!(response.answer_count(), 2);
    assert_eq!(response.answers().len(), 2);
    let query_name = Name::from_ascii("example.com.").unwrap();
    assert!(
        filter_response_records(&query_name, response.clone())
            .answers()
            .is_empty(),
        "前置条件：链外答案应被全部拒绝"
    );

    cache.insert(&query, response).await.unwrap();

    let key = CacheKey::from_message(&query).unwrap();
    assert!(
        matches!(cache.get(&key).await, CacheResult::Miss),
        "全拒的投毒响应不得写入缓存"
    );
}

#[tokio::test]
async fn test_cache_disabled_size_zero() {
    let cache = DnsCache::new(0, 10, 300, Some(60), None);
    assert!(!cache.is_enabled(), "cache with size=0 should be disabled");

    let cache_enabled = DnsCache::new(100, 10, 300, Some(60), None);
    assert!(
        cache_enabled.is_enabled(),
        "cache with size>0 should be enabled"
    );
}

#[tokio::test]
async fn test_cache_not_cacheable_no_query() {
    let cache = DnsCache::new(100, 10, 300, Some(60), None);

    let response = Message::new();
    assert!(
        !cache.is_cacheable(&response),
        "response with no queries should not be cacheable"
    );
}

#[tokio::test]
async fn test_build_cname_chain_simple() {
    let query_name = Name::from_ascii("www.example.com.").unwrap();
    let answers = vec![
        make_cname_record("www.example.com.", 300, "cdn.example.com."),
        make_a_record("cdn.example.com.", 300, Ipv4Addr::new(1, 2, 3, 4)),
    ];

    let chain = build_cname_chain(&query_name, &answers);
    assert!(chain.contains(&Name::from_ascii("www.example.com.").unwrap()));
    assert!(chain.contains(&Name::from_ascii("cdn.example.com.").unwrap()));
    assert!(!chain.contains(&Name::from_ascii("other.com.").unwrap()));
}

#[tokio::test]
async fn test_build_cname_chain_multi_hop() {
    let query_name = Name::from_ascii("a.example.com.").unwrap();
    let answers = vec![
        make_cname_record("a.example.com.", 300, "b.example.com."),
        make_cname_record("b.example.com.", 300, "c.example.com."),
        make_a_record("c.example.com.", 300, Ipv4Addr::new(1, 2, 3, 4)),
    ];

    let chain = build_cname_chain(&query_name, &answers);
    assert!(chain.contains(&Name::from_ascii("a.example.com.").unwrap()));
    assert!(chain.contains(&Name::from_ascii("b.example.com.").unwrap()));
    assert!(chain.contains(&Name::from_ascii("c.example.com.").unwrap()));
}

#[tokio::test]
async fn test_get_stale_entry_returns_stale_entry() {
    let stale_grace = 60u64;
    let cache = DnsCache::new(100, 10, 300, Some(60), Some(stale_grace));

    let query = make_query("stale-fallback.example.com.", RecordType::A);
    let answers = vec![make_a_record(
        "stale-fallback.example.com.",
        10,
        Ipv4Addr::new(10, 0, 0, 1),
    )];
    let response = make_response(&query, ResponseCode::NoError, answers);

    cache.insert(&query, response).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let result = cache.get_stale_entry(&query).await;
    assert!(
        result.is_some(),
        "get_stale_entry should return entry when stale_while_revalidate > 0"
    );
    let msg = result.unwrap();
    assert_eq!(msg.response_code(), ResponseCode::NoError);
    assert_eq!(msg.answer_count(), 1);
}

#[tokio::test]
async fn test_get_stale_entry_returns_none_when_no_entry() {
    let cache = DnsCache::new(100, 10, 300, Some(60), Some(60));

    let query = make_query("nonexistent.example.com.", RecordType::A);
    let result = cache.get_stale_entry(&query).await;
    assert!(
        result.is_none(),
        "get_stale_entry should return None when no entry"
    );
}

#[tokio::test]
async fn test_get_stale_entry_returns_none_when_stale_disabled() {
    let cache = DnsCache::new(100, 10, 300, Some(60), Some(0));

    let query = make_query("no-stale.example.com.", RecordType::A);
    let answers = vec![make_a_record(
        "no-stale.example.com.",
        10,
        Ipv4Addr::new(10, 0, 0, 2),
    )];
    let response = make_response(&query, ResponseCode::NoError, answers);
    cache.insert(&query, response).await.unwrap();

    let result = cache.get_stale_entry(&query).await;
    assert!(
        result.is_none(),
        "get_stale_entry should return None when stale_while_revalidate is 0"
    );
}

#[tokio::test]
async fn test_cache_dump_and_restore_round_trip() {
    // 创建源缓存
    let source = DnsCache::new(100, 10, 3600, Some(60), Some(120));

    // 插入两条记录
    let q1 = make_query("a.example.com.", RecordType::A);
    let r1 = make_response(
        &q1,
        ResponseCode::NoError,
        vec![make_a_record(
            "a.example.com.",
            300,
            Ipv4Addr::new(1, 2, 3, 4),
        )],
    );
    source.insert(&q1, r1).await.unwrap();

    let q2 = make_query("b.example.com.", RecordType::A);
    let r2 = make_response(
        &q2,
        ResponseCode::NoError,
        vec![make_a_record(
            "b.example.com.",
            600,
            Ipv4Addr::new(5, 6, 7, 8),
        )],
    );
    source.insert(&q2, r2).await.unwrap();

    // dump
    let dump_entries = source.iter_entries();
    assert_eq!(dump_entries.len(), 2, "should dump 2 entries");

    // 校验 dump 格式
    for e in &dump_entries {
        assert!(!e.name.is_empty());
        assert!(!e.query_type.is_empty());
        assert!(!e.response_hex.is_empty());
        assert!(e.expires_at > 0);
    }

    // 清空目标缓存
    let target = DnsCache::new(100, 10, 3600, Some(60), Some(120));

    // restore
    for entry in dump_entries {
        let stats = target.insert_restored(entry).await;
        assert_eq!(stats.loaded, 1);
        assert_eq!(stats.skipped_expired, 0);
        assert_eq!(stats.failed, 0);
    }

    // 验证 restored 缓存可被命中
    let result_a = target.get(&CacheKey::from_message(&q1).unwrap()).await;
    match result_a {
        CacheResult::Fresh(_) | CacheResult::Stale(_) => {}
        CacheResult::Miss => panic!("restored entry should be found"),
    }

    let result_b = target.get(&CacheKey::from_message(&q2).unwrap()).await;
    match result_b {
        CacheResult::Fresh(_) | CacheResult::Stale(_) => {}
        CacheResult::Miss => panic!("restored entry should be found"),
    }
}

#[tokio::test]
async fn test_cache_restore_skips_expired_entries() {
    let cache = DnsCache::new(100, 10, 3600, Some(60), Some(120));

    use loadants::CacheDumpEntry;

    let expired_entry = CacheDumpEntry {
        name: "expired.example.com.".to_string(),
        query_type: "A".to_string(),
        response_hex: String::new(), // 无效 hex, 但应在过期判断前先返回
        expires_at: 1,               // 很久之前，已经过期
        subnet: "0.0.0.0/0".to_string(),
    };

    let stats = cache.insert_restored(expired_entry).await;
    assert_eq!(stats.skipped_expired, 1);
    assert_eq!(stats.loaded, 0);
    assert_eq!(stats.failed, 0);
}

#[tokio::test]
async fn test_cache_restore_rejects_invalid_hex() {
    let cache = DnsCache::new(100, 10, 3600, Some(60), Some(120));

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let bad_entry = loadants::CacheDumpEntry {
        name: "bad.example.com.".to_string(),
        query_type: "A".to_string(),
        response_hex: "zzz".to_string(), // 无效 hex
        expires_at: now + 3600,
        subnet: "0.0.0.0/0".to_string(),
    };

    let stats = cache.insert_restored(bad_entry).await;
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.loaded, 0);
}

#[tokio::test]
async fn test_cache_restore_rejects_multibyte_utf8_hex_without_panic() {
    // P2-A: 多字节 UTF-8 输入的字节长度为偶数（如 "€€" 为 6 字节），
    // 不得在字符串切片时 panic，必须返回 failed 统计
    let cache = DnsCache::new(100, 10, 3600, Some(60), Some(120));

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // "€€" 6 字节（偶数），"0€" 4 字节（偶数且切片点落在字符内部）
    for bad_hex in ["€€", "0€"] {
        let entry = loadants::CacheDumpEntry {
            name: "multibyte.example.com.".to_string(),
            query_type: "A".to_string(),
            response_hex: bad_hex.to_string(),
            expires_at: now + 3600,
            subnet: "0.0.0.0/0".to_string(),
        };

        let stats = cache.insert_restored(entry).await;
        assert_eq!(
            stats.failed, 1,
            "multibyte UTF-8 hex {:?} should fail gracefully",
            bad_hex
        );
        assert_eq!(stats.loaded, 0);
        assert_eq!(stats.skipped_expired, 0);
    }
}

#[tokio::test]
async fn test_cache_restore_rejects_even_length_non_hex_ascii() {
    let cache = DnsCache::new(100, 10, 3600, Some(60), Some(120));

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    for bad_hex in ["zz", "0x", "gg", "  "] {
        let entry = loadants::CacheDumpEntry {
            name: "nonhex.example.com.".to_string(),
            query_type: "A".to_string(),
            response_hex: bad_hex.to_string(),
            expires_at: now + 3600,
            subnet: "0.0.0.0/0".to_string(),
        };

        let stats = cache.insert_restored(entry).await;
        assert_eq!(
            stats.failed, 1,
            "even-length non-hex input {:?} should be rejected",
            bad_hex
        );
        assert_eq!(stats.loaded, 0);
    }
}

#[tokio::test]
async fn test_cache_restore_accepts_uppercase_hex() {
    let cache = DnsCache::new(100, 10, 3600, Some(60), Some(120));

    let query = make_query("upper.example.com.", RecordType::A);
    let response = make_response(
        &query,
        ResponseCode::NoError,
        vec![make_a_record(
            "upper.example.com.",
            300,
            Ipv4Addr::new(1, 2, 3, 4),
        )],
    );
    let wire = response.to_vec().unwrap();
    let upper_hex: String = wire.iter().map(|b| format!("{:02X}", b)).collect();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let entry = loadants::CacheDumpEntry {
        name: "upper.example.com.".to_string(),
        query_type: "A".to_string(),
        response_hex: upper_hex,
        expires_at: now + 3600,
        subnet: "0.0.0.0/0".to_string(),
    };

    let stats = cache.insert_restored(entry).await;
    assert_eq!(stats.failed, 0);
    assert_eq!(stats.loaded, 1);

    match cache.get(&CacheKey::from_message(&query).unwrap()).await {
        CacheResult::Fresh(_) | CacheResult::Stale(_) => {}
        CacheResult::Miss => panic!("entry restored from uppercase hex should be found"),
    }
}

// ============================================================================
// 以下测试为本轮新增回归测试（D-02 TTL 扣减 / D-03 负缓存 SOA / D-20 header
// 计数失同步容错 派生），并非迁移自 src 内嵌测试（src/cache.rs 从无内嵌测试）。
// ============================================================================

fn make_soa_record(name: &str, minimum: u32) -> Record {
    Record::from_rdata(
        Name::from_ascii(name).unwrap(),
        3600,
        RData::SOA(SOA::new(
            Name::from_ascii("ns1.example.com.").unwrap(),
            Name::from_ascii("admin.example.com.").unwrap(),
            1,
            3600,
            600,
            86400,
            minimum,
        )),
    )
}

fn make_response_with_authority(
    query: &Message,
    code: ResponseCode,
    answers: Vec<Record>,
    authority: Vec<Record>,
) -> Message {
    let mut msg = Message::new();
    msg.set_id(query.id());
    msg.set_message_type(MessageType::Response);
    msg.set_op_code(query.op_code());
    msg.set_recursion_desired(query.recursion_desired());
    msg.set_recursion_available(true);
    msg.set_response_code(code);
    for q in query.queries() {
        msg.add_query(q.clone());
    }
    let answer_count = answers.len() as u16;
    let ns_count = authority.len() as u16;
    for ans in answers {
        msg.add_answer(ans);
    }
    for ns in authority {
        msg.add_name_server(ns);
    }
    let mut header = *msg.header();
    header.set_answer_count(answer_count);
    header.set_name_server_count(ns_count);
    msg.set_header(header);
    msg
}

/// 构造一个已过去 `secs` 秒的时间戳，用于确定性地测试 TTL 扣减。
/// 缓存内部使用 `std::time::Instant`（不受 tokio 虚拟时钟影响），
/// 因此直接回拨条目时间戳而非真实 sleep。
fn backdated_timestamp(secs: u64) -> Instant {
    Instant::now()
        .checked_sub(Duration::from_secs(secs))
        .expect("system uptime shorter than test backdate window")
}

/// 构造一个"已存在 120 秒"的单 A 记录缓存条目并返回其 key。
/// 模拟 `insert()` 的产物：记录 TTL 已归一化为 `effective_ttl`（300）。
async fn insert_backdated_single_a_entry(cache: &DnsCache, name: &str) -> CacheKey {
    let query = make_query(name, RecordType::A);
    let response = make_response_with_authority(
        &query,
        ResponseCode::NoError,
        vec![make_a_record(name, 300, Ipv4Addr::new(1, 2, 3, 4))],
        vec![],
    );
    let key = CacheKey::from_message(&query).unwrap();
    let entry = CacheEntry {
        message: Arc::new(response),
        timestamp: backdated_timestamp(120),
        effective_ttl: 300,
    };
    cache.cache.insert(key.clone(), entry).await;
    key
}

// ---- D-02: 缓存命中必须按已消耗时间扣减 TTL（answer ≤ 1 同样扣减）----

#[tokio::test]
async fn get_deducts_elapsed_ttl_for_single_answer_hit() {
    let cache = DnsCache::new(100, 10, 300, Some(60), Some(0));
    let key = insert_backdated_single_a_entry(&cache, "single-a.example.com.").await;

    match cache.get(&key).await {
        CacheResult::Fresh(msg) => assert_eq!(
            msg.answers()[0].ttl(),
            180,
            "single-answer cache hit must return effective_ttl(300) - elapsed(120) = 180"
        ),
        CacheResult::Stale(_) => panic!("expected Fresh, got Stale"),
        CacheResult::Miss => panic!("expected cache hit, got Miss"),
    }
}

#[tokio::test]
async fn get_stale_entry_deducts_elapsed_ttl_for_single_answer_hit() {
    // 与 get() 行为一致：fresh 分支同样按已消耗时间扣减 TTL
    let cache = DnsCache::new(100, 10, 300, Some(60), Some(60));
    let query = make_query("stale-consistent.example.com.", RecordType::A);
    insert_backdated_single_a_entry(&cache, "stale-consistent.example.com.").await;

    let msg = cache
        .get_stale_entry(&query)
        .await
        .expect("fresh entry should be returned");
    assert_eq!(
        msg.answers()[0].ttl(),
        180,
        "get_stale_entry fresh branch must return effective_ttl(300) - elapsed(120) = 180, \
         consistent with get()"
    );
}

// ---- D-03: 负缓存 TTL 必须取 min(SOA.MINIMUM, negative_ttl)（RFC 2308）----

#[tokio::test]
async fn insert_nxdomain_ttl_uses_soa_minimum_below_negative_ttl() {
    // SOA MINIMUM=5 < negative_ttl=60 → effective_ttl 应为 5 而非配置值 60。
    // SOA owner 是 zone apex（example.com.），不在查询名的 CNAME chain 内，
    // 会被 filter_response_records 剥除，因此 TTL 必须在过滤前用原始 response 计算。
    let cache = DnsCache::new(100, 10, 300, Some(60), Some(0));
    let query = make_query("nx.example.com.", RecordType::A);
    let response = make_response_with_authority(
        &query,
        ResponseCode::NXDomain,
        vec![],
        vec![make_soa_record("example.com.", 5)],
    );

    cache.insert(&query, response).await.unwrap();

    let key = CacheKey::from_message(&query).unwrap();
    let entry = cache
        .cache
        .get(&key)
        .await
        .expect("NXDOMAIN response should be cached");
    assert_eq!(
        entry.effective_ttl, 5,
        "negative cache TTL must be min(SOA.MINIMUM=5, negative_ttl=60) = 5 (RFC 2308)"
    );
}

#[tokio::test]
async fn insert_nodata_ttl_uses_soa_minimum_below_negative_ttl() {
    // NODATA（NOERROR + 0 answer）与 NXDOMAIN 同病：SOA 在 TTL 计算前被过滤剥除
    let cache = DnsCache::new(100, 10, 300, Some(60), Some(0));
    let query = make_query("nodata.example.com.", RecordType::A);
    let response = make_response_with_authority(
        &query,
        ResponseCode::NoError,
        vec![],
        vec![make_soa_record("example.com.", 5)],
    );

    cache.insert(&query, response).await.unwrap();

    let key = CacheKey::from_message(&query).unwrap();
    let entry = cache
        .cache
        .get(&key)
        .await
        .expect("NODATA response should be cached");
    assert_eq!(
        entry.effective_ttl, 5,
        "NODATA cache TTL must be min(SOA.MINIMUM=5, negative_ttl=60) = 5 (RFC 2308)"
    );
}

// ---- D-20: 过滤函数必须容忍 header 计数与实际记录段失同步 ----

/// 构造 header.answer_count 与 answer 记录段失同步的合成响应。
/// hickory-proto 0.25.2 的 `add_answer` 不更新 header（且 `Message::answer_count()`
/// 经 `Deref<Target=Header>` 读的是 header 计数），合成 Message 不经 wire 往返
/// 即天然失同步：header=0、实际记录=1 → `header_count - valid_len` 减法溢出。
fn make_header_desynced_response(name: &str) -> Message {
    let mut msg = Message::new();
    msg.set_message_type(MessageType::Response);
    msg.set_response_code(ResponseCode::NoError);
    msg.add_query(Query::query(Name::from_ascii(name).unwrap(), RecordType::A));
    msg.add_answer(make_a_record(name, 300, Ipv4Addr::new(1, 2, 3, 4)));
    assert_eq!(msg.answer_count(), 0, "前置条件：header 计数应失同步为 0");
    assert_eq!(msg.answers().len(), 1, "前置条件：answer 段实际有 1 条记录");
    msg
}

#[test]
fn filter_answer_records_tolerates_header_count_desync() {
    let query_name = Name::from_ascii("desync-a.example.com.").unwrap();
    let response = make_header_desynced_response("desync-a.example.com.");

    let filtered = filter_answer_records(&query_name, response);

    assert_eq!(
        filtered.answers().len(),
        1,
        "过滤结果必须按实际记录段计算，链内有效记录应保留"
    );
}

#[test]
fn filter_response_records_tolerates_header_count_desync() {
    let query_name = Name::from_ascii("desync-r.example.com.").unwrap();
    let response = make_header_desynced_response("desync-r.example.com.");

    let filtered = filter_response_records(&query_name, response);

    assert_eq!(
        filtered.answers().len(),
        1,
        "过滤结果必须按实际记录段计算，链内有效记录应保留"
    );
}
