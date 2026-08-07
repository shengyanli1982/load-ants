use criterion::{criterion_group, criterion_main, Criterion};
use hickory_proto::{
    op::{Edns, Message, MessageType, OpCode, Query, ResponseCode},
    rr::{
        rdata::{A, AAAA, CNAME},
        Name, RData, Record, RecordType,
    },
};
use loadants::coalesce::CoalesceEntry;
use loadants::config::{DnsUpstreamServerConfig, UpstreamServerConfig};
use loadants::{
    build_cname_chain, filter_response_records, CacheKey, CoalescingMap, DnsCache, LoadBalancer,
    MatchType, RateLimiter, RequestHandler, RoundRobinBalancer, RouteAction, RouteRuleConfig,
    Router, UpstreamManager,
};
use mimalloc::MiMalloc;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

#[global_allocator]
static GLOBAL: MiMalloc = mimalloc::MiMalloc;

fn build_realistic_rules() -> Vec<RouteRuleConfig> {
    let mut rules = Vec::new();

    let block_domains = [
        "ads.example.com",
        "tracker.malware.net",
        "spam.badsite.org",
        "phishing.evil.com",
        "crypto.miner.xyz",
    ];
    for d in &block_domains {
        rules.push(RouteRuleConfig {
            match_type: MatchType::Exact,
            patterns: vec![d.to_string()],
            action: RouteAction::Block,
            target: None,
        });
    }

    let forward_pairs = [
        ("corp.internal", "internal-dns"),
        ("dev.corp.local", "internal-dns"),
        ("api.service.io", "cloud-doh"),
        ("cdn.fast.com", "cloud-doh"),
        ("mail.secure.org", "secure-dns"),
    ];
    for (d, group) in &forward_pairs {
        rules.push(RouteRuleConfig {
            match_type: MatchType::Exact,
            patterns: vec![d.to_string()],
            action: RouteAction::Forward,
            target: Some(group.to_string()),
        });
    }

    let wildcard_pairs = [
        ("*.corp.local", "internal-dns"),
        ("*.dev.internal", "internal-dns"),
        ("*.cdn.fast.com", "cloud-doh"),
        ("*.api.service.io", "cloud-doh"),
        ("*.mail.secure.org", "secure-dns"),
    ];
    for (wc, group) in &wildcard_pairs {
        rules.push(RouteRuleConfig {
            match_type: MatchType::Wildcard,
            patterns: vec![wc.to_string()],
            action: RouteAction::Forward,
            target: Some(group.to_string()),
        });
    }

    rules.push(RouteRuleConfig {
        match_type: MatchType::Regex,
        patterns: vec!["^api\\..+\\.com$".to_string()],
        action: RouteAction::Forward,
        target: Some("cloud-doh".to_string()),
    });
    rules.push(RouteRuleConfig {
        match_type: MatchType::Regex,
        patterns: vec!["^cdn\\..+\\.net$".to_string()],
        action: RouteAction::Forward,
        target: Some("cloud-doh".to_string()),
    });
    rules.push(RouteRuleConfig {
        match_type: MatchType::Regex,
        patterns: vec!["^mail\\..+\\.org$".to_string()],
        action: RouteAction::Forward,
        target: Some("secure-dns".to_string()),
    });

    rules.push(RouteRuleConfig {
        match_type: MatchType::Wildcard,
        patterns: vec!["*".to_string()],
        action: RouteAction::Forward,
        target: Some("default-dns".to_string()),
    });

    rules
}

fn bench_router_find_match(c: &mut Criterion) {
    let router = Router::new(build_realistic_rules()).unwrap();

    let exact_block_name = Name::from_str("ads.example.com.").unwrap();
    c.bench_function("router/exact_block_hit", |b| {
        b.iter(|| router.find_match(&exact_block_name).ok())
    });

    let exact_fwd_name = Name::from_str("api.service.io.").unwrap();
    c.bench_function("router/exact_forward_hit", |b| {
        b.iter(|| router.find_match(&exact_fwd_name).ok())
    });

    let wildcard_name = Name::from_str("sub.dev.internal.").unwrap();
    c.bench_function("router/wildcard_hit", |b| {
        b.iter(|| router.find_match(&wildcard_name).ok())
    });

    let regex_name = Name::from_str("api.something.com.").unwrap();
    c.bench_function("router/regex_hit", |b| {
        b.iter(|| router.find_match(&regex_name).ok())
    });

    let global_wc_name = Name::from_str("random.unknown.org.").unwrap();
    c.bench_function("router/global_wildcard_hit", |b| {
        b.iter(|| router.find_match(&global_wc_name).ok())
    });

    c.bench_function("router/no_match", |b| {
        let name =
            Name::from_str("a.b.c.d.e.f.g.h.i.j.k.l.m.n.o.p.q.r.s.t.u.v.w.x.y.z.example.invalid.")
                .unwrap();
        b.iter(|| router.find_match(&name).ok())
    });
}

fn bench_cache_key_from_message(c: &mut Criterion) {
    let mut msg = Message::new();
    msg.set_id(1);
    msg.set_message_type(MessageType::Query);
    msg.set_op_code(OpCode::Query);
    msg.set_recursion_desired(true);
    msg.add_query(Query::query(
        Name::from_ascii("www.example.com.").unwrap(),
        RecordType::A,
    ));

    c.bench_function("cache/key_from_message", |b| {
        b.iter(|| CacheKey::from_message(&msg))
    });
}

fn bench_cache_operations(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let cache = DnsCache::new(10000, 60, 300, Some(30), Some(300));

    let query = {
        let mut msg = Message::new();
        msg.set_id(1);
        msg.set_message_type(MessageType::Query);
        msg.set_op_code(OpCode::Query);
        msg.set_recursion_desired(true);
        msg.add_query(Query::query(
            Name::from_ascii("cached.example.com.").unwrap(),
            RecordType::A,
        ));
        msg
    };

    let response = {
        let mut msg = Message::new();
        msg.set_id(query.id());
        msg.set_message_type(MessageType::Response);
        msg.set_op_code(OpCode::Query);
        msg.set_recursion_desired(true);
        msg.set_recursion_available(true);
        msg.set_response_code(ResponseCode::NoError);
        for q in query.queries() {
            msg.add_query(q.clone());
        }
        let record = Record::from_rdata(
            Name::from_ascii("cached.example.com.").unwrap(),
            300,
            RData::A(A::from(Ipv4Addr::new(1, 2, 3, 4))),
        );
        msg.add_answer(record);
        msg
    };

    rt.block_on(cache.insert(&query, response.clone())).unwrap();

    let cache_key = CacheKey::from_message(&query).unwrap();

    c.bench_function("cache/get_hit", |b| {
        b.to_async(&rt).iter(|| async {
            let _ = cache.get(&cache_key).await;
        })
    });

    let miss_query = {
        let mut msg = Message::new();
        msg.set_id(2);
        msg.set_message_type(MessageType::Query);
        msg.set_op_code(OpCode::Query);
        msg.set_recursion_desired(true);
        msg.add_query(Query::query(
            Name::from_ascii("notcached.example.com.").unwrap(),
            RecordType::A,
        ));
        msg
    };

    let miss_cache_key = CacheKey::from_message(&miss_query).unwrap();

    c.bench_function("cache/get_miss", |b| {
        b.to_async(&rt).iter(|| async {
            let _ = cache.get(&miss_cache_key).await;
        })
    });

    c.bench_function("cache/insert", |b| {
        b.to_async(&rt).iter(|| async {
            cache.insert(&query, response.clone()).await.unwrap();
        })
    });
}

fn bench_rate_limiter_check(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    // `new_with_per_ip` spawns a cleanup task via `tokio::spawn`, requiring runtime context.
    let _guard = rt.enter();
    let limiter = RateLimiter::new_with_per_ip(1_000_000, 100_000);

    let ips: Vec<IpAddr> = (0..10)
        .map(|i| IpAddr::V4(Ipv4Addr::new(10, 0, 0, i)))
        .collect();

    c.bench_function("rate_limiter/check_10ips", |b| {
        b.iter(|| {
            for ip in &ips {
                let _ = limiter.check(*ip);
            }
        })
    });
}

fn bench_create_error_response(c: &mut Criterion) {
    let mut msg = Message::new();
    msg.set_id(42);
    msg.set_message_type(MessageType::Query);
    msg.set_op_code(OpCode::Query);
    msg.set_recursion_desired(true);
    msg.add_query(Query::query(
        Name::from_ascii("www.example.com.").unwrap(),
        RecordType::A,
    ));
    // Include EDNS0 OPT to exercise the EDNS copy path in create_error_response.
    let mut edns = Edns::new();
    edns.set_max_payload(1232);
    edns.set_version(0);
    msg.set_edns(edns);

    c.bench_function("handler/create_error_response_refused", |b| {
        b.iter(|| RequestHandler::create_error_response(&msg, ResponseCode::Refused))
    });
}

fn bench_handler_cache_hit_path(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    // Enter the runtime so `tokio::spawn` inside UpstreamManager/RateLimiter construction works.
    let _guard = rt.enter();

    let cache = Arc::new(DnsCache::new(10_000, 60, 300, Some(30), Some(300)));
    let router = Arc::new(Router::new(build_realistic_rules()).unwrap());
    let router_lock = Arc::new(RwLock::new(router));
    // `UpstreamManager::empty()` provides a zero-config stub; cache-hit path never reaches it.
    let upstream = Arc::new(UpstreamManager::empty().unwrap());

    let handler = RequestHandler::new(cache.clone(), router_lock, upstream);

    let query = {
        let mut msg = Message::new();
        msg.set_id(1);
        msg.set_message_type(MessageType::Query);
        msg.set_op_code(OpCode::Query);
        msg.set_recursion_desired(true);
        msg.add_query(Query::query(
            Name::from_ascii("cached-handler.example.com.").unwrap(),
            RecordType::A,
        ));
        msg
    };

    let response = {
        let mut msg = Message::new();
        msg.set_id(1);
        msg.set_message_type(MessageType::Response);
        msg.set_op_code(OpCode::Query);
        msg.set_recursion_desired(true);
        msg.set_recursion_available(true);
        msg.set_response_code(ResponseCode::NoError);
        msg.add_query(Query::query(
            Name::from_ascii("cached-handler.example.com.").unwrap(),
            RecordType::A,
        ));
        let record = Record::from_rdata(
            Name::from_ascii("cached-handler.example.com.").unwrap(),
            300,
            RData::A(A::from(Ipv4Addr::new(93, 184, 216, 34))),
        );
        msg.add_answer(record);
        msg
    };

    rt.block_on(cache.insert(&query, response)).unwrap();

    c.bench_function("handler/cache_hit_path", |b| {
        b.to_async(&rt).iter(|| async {
            let _ = handler.handle_request(&query).await;
        })
    });
}

fn bench_coalesce_entry_ops(c: &mut Criterion) {
    let map = CoalescingMap::new();

    let keys: Vec<CacheKey> = (0..10)
        .map(|i| {
            let mut msg = Message::new();
            msg.set_id(i as u16);
            msg.set_message_type(MessageType::Query);
            msg.set_op_code(OpCode::Query);
            msg.set_recursion_desired(true);
            msg.add_query(Query::query(
                Name::from_ascii(format!("coalesce-key-{}.example.com.", i).as_str()).unwrap(),
                RecordType::A,
            ));
            CacheKey::from_message(&msg).unwrap()
        })
        .collect();

    c.bench_function("coalesce/entry_insert_remove_cycle", |b| {
        b.iter(|| {
            for key in &keys {
                // entry() + or_insert_with() inserts a fresh CoalesceEntry when the slot is vacant.
                // `let _ = ...` drops the RefMut immediately, releasing the shard lock before remove.
                let _ = map
                    .entry(key.clone())
                    .or_insert_with(|| Arc::new(CoalesceEntry::new()));
                map.remove(key);
            }
        })
    });
}

fn build_dns_servers(count: u8) -> Vec<UpstreamServerConfig> {
    (1..=count)
        .map(|i| {
            UpstreamServerConfig::Dns(DnsUpstreamServerConfig {
                addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 1, i)), 53),
                weight: 1,
            })
        })
        .collect()
}

fn bench_balancer_select(c: &mut Criterion) {
    let servers = build_dns_servers(4);

    let all_healthy = RoundRobinBalancer::new("bench-all-healthy", servers.clone());
    c.bench_function("balancer/round_robin_all_healthy", |b| {
        b.iter(|| all_healthy.select_server(&[]).ok())
    });

    // max_failures=1 + long cooldown keeps the failed server Unhealthy for the whole bench.
    let one_down = RoundRobinBalancer::with_params("bench-one-down", servers, 1, 3600);
    let failed = one_down.servers()[0].clone();
    one_down.report_failure(&failed, "bench-one-down");
    c.bench_function("balancer/round_robin_one_down", |b| {
        b.iter(|| one_down.select_server(&[]).ok())
    });
}

fn build_cname_chain_answers() -> Vec<Record> {
    vec![
        Record::from_rdata(
            Name::from_ascii("www.example.com.").unwrap(),
            300,
            RData::CNAME(CNAME(Name::from_ascii("alias.example.com.").unwrap())),
        ),
        Record::from_rdata(
            Name::from_ascii("alias.example.com.").unwrap(),
            300,
            RData::CNAME(CNAME(Name::from_ascii("target.example.com.").unwrap())),
        ),
        Record::from_rdata(
            Name::from_ascii("target.example.com.").unwrap(),
            300,
            RData::A(A::from(Ipv4Addr::new(1, 2, 3, 4))),
        ),
    ]
}

fn bench_cname_chain(c: &mut Criterion) {
    let query_name = Name::from_ascii("www.example.com.").unwrap();
    let answers = build_cname_chain_answers();

    c.bench_function("cache/build_cname_chain", |b| {
        b.iter(|| build_cname_chain(&query_name, &answers))
    });
}

fn bench_filter_response_records(c: &mut Criterion) {
    let query_name = Name::from_ascii("www.example.com.").unwrap();

    let response = {
        let mut msg = Message::new();
        msg.set_id(1);
        msg.set_message_type(MessageType::Response);
        msg.set_op_code(OpCode::Query);
        msg.set_recursion_desired(true);
        msg.set_recursion_available(true);
        msg.set_response_code(ResponseCode::NoError);
        msg.add_query(Query::query(query_name.clone(), RecordType::A));
        for record in build_cname_chain_answers() {
            msg.add_answer(record);
        }
        // Unrelated records that the filter must reject.
        msg.add_answer(Record::from_rdata(
            Name::from_ascii("unrelated.example.net.").unwrap(),
            300,
            RData::AAAA(AAAA::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1)),
        ));
        msg.add_answer(Record::from_rdata(
            Name::from_ascii("other.example.org.").unwrap(),
            300,
            RData::AAAA(AAAA::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 2)),
        ));
        msg
    };

    c.bench_function("cache/filter_response_records", |b| {
        b.iter(|| filter_response_records(&query_name, response.clone()))
    });
}

criterion_group!(
    benches,
    bench_router_find_match,
    bench_cache_key_from_message,
    bench_cache_operations,
    bench_rate_limiter_check,
    bench_create_error_response,
    bench_handler_cache_hit_path,
    bench_coalesce_entry_ops,
    bench_balancer_select,
    bench_cname_chain,
    bench_filter_response_records,
);
criterion_main!(benches);
