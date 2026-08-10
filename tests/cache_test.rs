use hickory_proto::{
    op::{Message, MessageType, OpCode, Query, ResponseCode},
    rr::{rdata::A, Name, RData, Record, RecordType},
};
use loadants::{build_cname_chain, filter_answer_records, CacheKey, CacheResult, DnsCache};
use std::net::Ipv4Addr;

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
