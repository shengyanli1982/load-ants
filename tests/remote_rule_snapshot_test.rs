use loadants::config::{MatchType, RemoteRuleSnapshotConfig, RouteAction, RouteRuleConfig};
use loadants::remote_rule::RemoteRuleSnapshotStore;
use std::fs;
use tempfile::tempdir;

fn snapshot_store(enabled: bool) -> (tempfile::TempDir, RemoteRuleSnapshotStore) {
    let directory = tempdir().expect("temporary directory should be created");
    let store = RemoteRuleSnapshotStore::new(&RemoteRuleSnapshotConfig {
        enabled,
        path: directory.path().to_string_lossy().to_string(),
    });
    (directory, store)
}

fn temp_snapshot_path(
    store: &RemoteRuleSnapshotStore,
    source_url: &str,
    pid: u32,
) -> std::path::PathBuf {
    let snapshot_path = store.snapshot_path(source_url);
    let stem = snapshot_path
        .file_stem()
        .expect("snapshot path should contain a file stem")
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

#[tokio::test]
async fn snapshot_store_round_trip() {
    let (_directory, store) = snapshot_store(true);
    let source_url = "https://example.com/rules.txt";
    let rules = snapshot_rules();

    store
        .save(source_url, &rules)
        .await
        .expect("snapshot should be written");
    let snapshot = store
        .load(source_url)
        .await
        .expect("snapshot should be readable")
        .expect("snapshot file should exist");

    assert_eq!(snapshot.source_url, source_url);
    assert_eq!(snapshot.rules, rules);
}

#[tokio::test]
async fn snapshot_store_is_noop_when_disabled() {
    let (_directory, store) = snapshot_store(false);

    store
        .save("https://example.com/rules.txt", &[])
        .await
        .expect("disabled snapshot store should be a no-op on save");
    assert!(store
        .load("https://example.com/rules.txt")
        .await
        .expect("disabled snapshot store should not fail on load")
        .is_none());
}

#[test]
fn snapshot_path_uses_sha256_hex_name() {
    let (_directory, store) = snapshot_store(true);
    let source_url = "https://rules.example.com/a/b.txt?token=secret";

    let path = store.snapshot_path(source_url);
    let file_name = path
        .file_name()
        .expect("snapshot path should contain a file name")
        .to_string_lossy()
        .to_string();

    assert_eq!(path, store.snapshot_path(source_url));
    assert!(file_name.ends_with(".json"));
    assert_eq!(
        file_name.len(),
        69,
        "file name should be 64 hex characters plus .json"
    );
    assert!(
        file_name
            .strip_suffix(".json")
            .expect("file name should end with .json")
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase()),
        "file name body should be lowercase SHA-256 hex"
    );
}

#[tokio::test]
async fn save_overwrites_same_source_in_place() {
    let (_directory, store) = snapshot_store(true);
    let source_url = "https://example.com/rules.txt";
    let first_path = store.snapshot_path(source_url);

    store
        .save(source_url, &snapshot_rules())
        .await
        .expect("first save should succeed");
    store
        .save(
            source_url,
            &[RouteRuleConfig {
                match_type: MatchType::Exact,
                patterns: vec!["updated.example".to_string()],
                action: RouteAction::Block,
                target: None,
            }],
        )
        .await
        .expect("second save should succeed");

    assert_eq!(first_path, store.snapshot_path(source_url));
    let snapshot = store
        .load(source_url)
        .await
        .expect("overwritten snapshot should be readable")
        .expect("overwritten snapshot should exist");
    assert_eq!(
        snapshot.rules[0].patterns,
        vec!["updated.example".to_string()]
    );
}

#[tokio::test]
async fn sync_active_sources_removes_orphan_snapshots_and_tmp_files() {
    let (directory, store) = snapshot_store(true);
    let active_url = "https://active.example.com/rules.txt".to_string();
    let stale_url = "https://stale.example.com/rules.txt".to_string();

    store
        .save(&active_url, &snapshot_rules())
        .await
        .expect("active snapshot should be written");
    store
        .save(&stale_url, &snapshot_rules())
        .await
        .expect("stale snapshot should be written");

    let active_tmp = temp_snapshot_path(&store, &active_url, 1001);
    let stale_tmp = temp_snapshot_path(&store, &stale_url, 1002);
    fs::write(&active_tmp, b"active tmp").expect("active temp file should be created");
    fs::write(&stale_tmp, b"stale tmp").expect("stale temp file should be created");

    let extra_file = directory.path().join("notes.txt");
    fs::write(&extra_file, b"keep").expect("extra file should be created");

    store
        .sync_active_sources(std::slice::from_ref(&active_url))
        .await
        .expect("active sources should be synchronized");

    assert!(store.snapshot_path(&active_url).exists());
    assert!(active_tmp.exists(), "active temp file should be preserved");
    assert!(
        !store.snapshot_path(&stale_url).exists(),
        "stale snapshot file should be removed"
    );
    assert!(!stale_tmp.exists(), "stale temp file should be removed");
    assert!(
        extra_file.exists(),
        "non-snapshot files should not be removed"
    );
}
