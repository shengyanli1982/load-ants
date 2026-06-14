use criterion::{black_box, criterion_group, criterion_main, Criterion};
use hickory_proto::rr::Name;
use loadants::{MatchType, RouteAction, RouteRuleConfig, Router};

fn create_test_router() -> Router {
    let mut rules = Vec::new();

    for i in 0..100 {
        rules.push(RouteRuleConfig {
            match_type: MatchType::Exact,
            patterns: vec![format!("example{}.com", i)],
            action: RouteAction::Forward,
            target: Some("default".to_string()),
        });
    }

    for i in 0..50 {
        rules.push(RouteRuleConfig {
            match_type: MatchType::Wildcard,
            patterns: vec![format!("*.test{}.org", i)],
            action: RouteAction::Block,
            target: None,
        });
    }

    for i in 0..20 {
        rules.push(RouteRuleConfig {
            match_type: MatchType::Regex,
            patterns: vec![format!(".*\\.regex{}.io", i)],
            action: RouteAction::Forward,
            target: Some("default".to_string()),
        });
    }

    rules.push(RouteRuleConfig {
        match_type: MatchType::Wildcard,
        patterns: vec!["*".to_string()],
        action: RouteAction::Forward,
        target: Some("default".to_string()),
    });

    Router::new(rules).unwrap()
}

fn router_exact_match(c: &mut Criterion) {
    let router = create_test_router();
    let domain = Name::from_ascii("example50.com").unwrap();

    c.bench_function("router_exact_match_hit", |b| {
        b.iter(|| router.find_match(black_box(&domain)).ok())
    });
}

fn router_wildcard_match(c: &mut Criterion) {
    let router = create_test_router();
    let domain = Name::from_ascii("sub.test25.org").unwrap();

    c.bench_function("router_wildcard_match_hit", |b| {
        b.iter(|| router.find_match(black_box(&domain)).ok())
    });
}

fn router_regex_match(c: &mut Criterion) {
    let router = create_test_router();
    let domain = Name::from_ascii("www.sub.regex10.io").unwrap();

    c.bench_function("router_regex_match_hit", |b| {
        b.iter(|| router.find_match(black_box(&domain)).ok())
    });
}

fn router_global_wildcard(c: &mut Criterion) {
    let router = create_test_router();
    let domain = Name::from_ascii("unknown.domain.com").unwrap();

    c.bench_function("router_global_wildcard_fallback", |b| {
        b.iter(|| router.find_match(black_box(&domain)).ok())
    });
}

fn router_no_match(c: &mut Criterion) {
    let router = create_test_router();
    let domain = Name::from_ascii("notfound.nomatch.xyz").unwrap();

    c.bench_function("router_no_match_fallback", |b| {
        b.iter(|| router.find_match(black_box(&domain)).ok())
    });
}

fn router_with_many_rules(c: &mut Criterion) {
    let mut rules = Vec::new();

    for i in 0..1000 {
        rules.push(RouteRuleConfig {
            match_type: MatchType::Exact,
            patterns: vec![format!("domain{}.com", i)],
            action: RouteAction::Forward,
            target: Some("default".to_string()),
        });
    }

    let router = Router::new(rules).unwrap();
    let domain = Name::from_ascii("domain500.com").unwrap();

    c.bench_function("router_1000_exact_rules", |b| {
        b.iter(|| router.find_match(black_box(&domain)).ok())
    });
}

criterion_group!(
    benches,
    router_exact_match,
    router_wildcard_match,
    router_regex_match,
    router_global_wildcard,
    router_no_match,
    router_with_many_rules,
);
criterion_main!(benches);
