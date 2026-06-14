use criterion::{black_box, criterion_group, criterion_main, Criterion};
use hickory_proto::op::{Message, Query};
use hickory_proto::rr::{Name, RecordType};
use loadants::DnsCache;
use std::sync::Arc;
use tokio::runtime::Runtime;

fn create_test_message(domain: &str, record_type: RecordType) -> Message {
    let mut message = Message::new();
    message.set_id(12345);
    message.set_recursion_desired(true);

    let query_name = Name::from_ascii(domain).unwrap();
    let query = Query::query(query_name, record_type);
    message.add_query(query);

    message
}

fn create_test_response(domain: &str, record_type: RecordType) -> Message {
    let mut response = Message::new();
    response.set_id(12345);
    response.set_recursion_desired(true);
    response.set_recursion_available(true);

    let query_name = Name::from_ascii(domain).unwrap();
    let query = Query::query(query_name, record_type);
    response.add_query(query);

    response
}

fn create_cache() -> Arc<DnsCache> {
    Arc::new(DnsCache::new(10000, 60, Some(300)))
}

fn cache_insert(c: &mut Criterion) {
    let cache = create_cache();
    let message = create_test_message("test.example.com", RecordType::A);
    let rt = Runtime::new().unwrap();

    c.bench_function("cache_insert", |b| {
        b.iter(|| {
            let response = create_test_response("test.example.com", RecordType::A);
            rt.block_on(cache.insert(black_box(&message), black_box(response)))
        })
    });
}

fn cache_hit(c: &mut Criterion) {
    let cache = create_cache();
    let message = create_test_message("cachehit.example.com", RecordType::A);
    let rt = Runtime::new().unwrap();

    rt.block_on(async {
        let response = create_test_response("cachehit.example.com", RecordType::A);
        cache.insert(&message, response).await.ok();
    });

    c.bench_function("cache_hit", |b| {
        b.iter(|| rt.block_on(cache.get(black_box(&message))))
    });
}

fn cache_miss(c: &mut Criterion) {
    let cache = create_cache();
    let message = create_test_message("miss.example.com", RecordType::A);
    let rt = Runtime::new().unwrap();

    c.bench_function("cache_miss", |b| {
        b.iter(|| rt.block_on(cache.get(black_box(&message))))
    });
}

criterion_group!(benches, cache_insert, cache_hit, cache_miss);
criterion_main!(benches);
