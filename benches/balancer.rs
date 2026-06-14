use criterion::{criterion_group, criterion_main, Criterion};
use loadants::config::{DoHContentType, DoHMethod, DoHUpstreamServerConfig, UpstreamServerConfig};
use std::sync::atomic::{AtomicUsize, Ordering};

fn create_doh_servers(count: usize) -> Vec<UpstreamServerConfig> {
    (0..count)
        .map(|i| {
            UpstreamServerConfig::Doh(DoHUpstreamServerConfig {
                url: format!("https://dns{}.example.com/dns-query", i)
                    .parse()
                    .unwrap(),
                weight: 1,
                method: DoHMethod::Get,
                content_type: DoHContentType::Message,
                auth: None,
            })
        })
        .collect()
}

fn round_robin_select(c: &mut Criterion) {
    let servers = create_doh_servers(10);
    let current = AtomicUsize::new(0);

    c.bench_function("round_robin_select_10_servers", |b| {
        b.iter(|| {
            let idx = current.fetch_add(1, Ordering::Relaxed) % servers.len();
            let _ = &servers[idx];
        })
    });
}

fn weighted_select(c: &mut Criterion) {
    let servers = [
        UpstreamServerConfig::Doh(DoHUpstreamServerConfig {
            url: "https://dns1.example.com/dns-query".parse().unwrap(),
            weight: 3,
            method: DoHMethod::Get,
            content_type: DoHContentType::Message,
            auth: None,
        }),
        UpstreamServerConfig::Doh(DoHUpstreamServerConfig {
            url: "https://dns2.example.com/dns-query".parse().unwrap(),
            weight: 2,
            method: DoHMethod::Get,
            content_type: DoHContentType::Message,
            auth: None,
        }),
        UpstreamServerConfig::Doh(DoHUpstreamServerConfig {
            url: "https://dns3.example.com/dns-query".parse().unwrap(),
            weight: 1,
            method: DoHMethod::Get,
            content_type: DoHContentType::Message,
            auth: None,
        }),
    ];
    let total_weight: usize = servers.iter().map(|s| s.weight() as usize).sum();
    let current_weights: Vec<AtomicUsize> = servers.iter().map(|_| AtomicUsize::new(0)).collect();

    c.bench_function("weighted_select_3_servers", |b| {
        b.iter(|| {
            let mut max_weight = 0;
            let mut max_index = 0;

            for (i, weight_atomic) in current_weights.iter().enumerate() {
                let weight = servers[i].weight() as usize;
                let current = weight_atomic.fetch_add(weight, Ordering::Relaxed) + weight;

                if current > max_weight {
                    max_weight = current;
                    max_index = i;
                }
            }

            current_weights[max_index].fetch_sub(total_weight, Ordering::Relaxed);
            let _ = &servers[max_index];
        })
    });
}

fn weighted_select_many_servers(c: &mut Criterion) {
    let servers = create_doh_servers(20);
    let total_weight: usize = servers.iter().map(|s| s.weight() as usize).sum();
    let current_weights: Vec<AtomicUsize> = servers.iter().map(|_| AtomicUsize::new(0)).collect();

    c.bench_function("weighted_select_20_servers", |b| {
        b.iter(|| {
            let mut max_weight = 0;
            let mut max_index = 0;

            for (i, weight_atomic) in current_weights.iter().enumerate() {
                let weight = servers[i].weight() as usize;
                let current = weight_atomic.fetch_add(weight, Ordering::Relaxed) + weight;

                if current > max_weight {
                    max_weight = current;
                    max_index = i;
                }
            }

            current_weights[max_index].fetch_sub(total_weight, Ordering::Relaxed);
            let _ = &servers[max_index];
        })
    });
}

fn random_select(c: &mut Criterion) {
    use rand::seq::SliceRandom;
    use rand::thread_rng;

    let servers = create_doh_servers(10);

    c.bench_function("random_select_10_servers", |b| {
        b.iter(|| {
            let server = servers.choose(&mut thread_rng()).unwrap();
            let _ = server;
        })
    });
}

criterion_group!(
    benches,
    round_robin_select,
    weighted_select,
    random_select,
    weighted_select_many_servers,
);
criterion_main!(benches);
