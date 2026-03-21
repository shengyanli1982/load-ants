use loadants::config::{MatchType, RemoteRuleSnapshotConfig, RouteAction, RouteRuleConfig};
use loadants::remote_rule::RemoteRuleSnapshotStore;
use serde_json::to_vec_pretty;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use tempfile::tempdir;

fn snapshot_store(enabled: bool) -> (tempfile::TempDir, RemoteRuleSnapshotStore) {
    let directory = tempdir().expect("应成功创建临时目录");
    let store = RemoteRuleSnapshotStore::new(&RemoteRuleSnapshotConfig {
        enabled,
        path: directory.path().to_string_lossy().to_string(),
    });
    (directory, store)
}

fn legacy_snapshot_path(directory: &std::path::Path, source_url: &str) -> std::path::PathBuf {
    let mut hasher = DefaultHasher::new();
    source_url.hash(&mut hasher);
    directory.join(format!("{:016x}.json", hasher.finish()))
}

fn temp_snapshot_path(
    store: &RemoteRuleSnapshotStore,
    source_url: &str,
    pid: u32,
) -> std::path::PathBuf {
    let snapshot_path = store.snapshot_path(source_url);
    let stem = snapshot_path
        .file_stem()
        .expect("快照路径应包含 stem")
        .to_string_lossy();
    snapshot_path.with_file_name(format!("{stem}.{pid}.tmp"))
}

fn snapshot_rules() -> Vec<RouteRuleConfig> {
    vec![RouteRuleConfig {
        match_type: MatchType::Exact,
        patterns: vec!["snapshot.example".to_string()],
        action: RouteAction::Block,
        target: None,
    }]
}

#[test]
fn snapshot_store_round_trip() {
    let (_directory, store) = snapshot_store(true);
    let source_url = "https://example.com/rules.txt";
    let rules = snapshot_rules();

    store.save(source_url, &rules).expect("应成功写入快照");
    let snapshot = store
        .load(source_url)
        .expect("应成功读取快照")
        .expect("快照文件应存在");

    assert_eq!(snapshot.source_url, source_url);
    assert_eq!(snapshot.rules, rules);
}

#[test]
fn snapshot_store_is_noop_when_disabled() {
    let (_directory, store) = snapshot_store(false);

    store
        .save("https://example.com/rules.txt", &[])
        .expect("禁用快照功能时保存应为无操作");
    assert!(store
        .load("https://example.com/rules.txt")
        .expect("禁用快照功能时读取不应报错")
        .is_none());
}

#[test]
fn snapshot_path_is_stable_and_human_readable() {
    let (_directory, store) = snapshot_store(true);
    let source_url = "https://rules.example.com/a/b.txt?token=secret";

    let path = store.snapshot_path(source_url);
    let file_name = path
        .file_name()
        .expect("快照路径应包含文件名")
        .to_string_lossy()
        .to_string();

    assert_eq!(path, store.snapshot_path(source_url));
    assert!(file_name.ends_with(".json"));
    assert!(file_name.contains("rules-example-com-a-b-txt"));
    assert!(!file_name.contains("secret"));
}

#[test]
fn load_falls_back_to_legacy_hashed_snapshot() {
    let (directory, store) = snapshot_store(true);
    let source_url = "https://example.com/rules.txt";
    let rules = snapshot_rules();
    let payload = to_vec_pretty(&loadants::remote_rule::RemoteRuleSnapshotEnvelope::new(
        source_url,
        rules.clone(),
    ))
    .expect("应成功序列化快照");

    fs::write(legacy_snapshot_path(directory.path(), source_url), payload)
        .expect("应成功写入旧格式快照");

    let snapshot = store
        .load(source_url)
        .expect("应成功读取旧格式快照")
        .expect("旧格式快照应存在");

    assert_eq!(snapshot.source_url, source_url);
    assert_eq!(snapshot.rules, rules);
}

#[test]
fn save_replaces_legacy_snapshot_for_same_source() {
    let (directory, store) = snapshot_store(true);
    let source_url = "https://example.com/rules.txt";
    let rules = snapshot_rules();
    let legacy_path = legacy_snapshot_path(directory.path(), source_url);

    let payload = to_vec_pretty(&loadants::remote_rule::RemoteRuleSnapshotEnvelope::new(
        source_url,
        vec![RouteRuleConfig {
            match_type: MatchType::Exact,
            patterns: vec!["old.example".to_string()],
            action: RouteAction::Block,
            target: None,
        }],
    ))
    .expect("应成功序列化旧快照");
    fs::write(&legacy_path, payload).expect("应成功写入旧格式快照");

    store.save(source_url, &rules).expect("应成功覆盖保存快照");

    assert!(
        !legacy_path.exists(),
        "保存新命名快照后应移除同源旧纯哈希文件"
    );
    assert!(
        store.snapshot_path(source_url).exists(),
        "应写入新的可读命名快照文件"
    );
}

#[test]
fn sync_active_sources_removes_orphan_snapshots_and_tmp_files() {
    let (directory, store) = snapshot_store(true);
    let active_url = "https://active.example.com/rules.txt".to_string();
    let stale_url = "https://stale.example.com/rules.txt".to_string();

    store
        .save(&active_url, &snapshot_rules())
        .expect("应成功写入活跃来源快照");
    store
        .save(&stale_url, &snapshot_rules())
        .expect("应成功写入陈旧来源快照");

    let active_tmp = temp_snapshot_path(&store, &active_url, 1001);
    let stale_tmp = temp_snapshot_path(&store, &stale_url, 1002);
    fs::write(&active_tmp, b"active tmp").expect("应成功写入活跃来源临时文件");
    fs::write(&stale_tmp, b"stale tmp").expect("应成功写入陈旧来源临时文件");

    let extra_file = directory.path().join("notes.txt");
    fs::write(&extra_file, b"keep").expect("应成功写入额外文件");

    store
        .sync_active_sources(std::slice::from_ref(&active_url))
        .expect("应成功同步活跃来源");

    assert!(store.snapshot_path(&active_url).exists());
    assert!(active_tmp.exists(), "活跃来源临时文件应被保留");
    assert!(
        !store.snapshot_path(&stale_url).exists(),
        "非活跃来源正式快照应被清理"
    );
    assert!(!stale_tmp.exists(), "非活跃来源临时文件应被清理");
    assert!(extra_file.exists(), "非快照文件不应被误删");
}
