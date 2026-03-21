use crate::config::{RemoteRuleSnapshotConfig, RouteRuleConfig};
use crate::error::AppError;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

const SNAPSHOT_SCHEMA_VERSION: u32 = 1;

/// 远程规则快照的持久化封装，记录来源与规则内容。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteRuleSnapshotEnvelope {
    pub schema_version: u32,
    pub source_url: String,
    pub rules: Vec<RouteRuleConfig>,
}

impl RemoteRuleSnapshotEnvelope {
    /// 创建符合当前版本的快照内容。
    pub fn new(source_url: &str, rules: Vec<RouteRuleConfig>) -> Self {
        Self {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            source_url: source_url.to_string(),
            rules,
        }
    }
}

/// 负责远程规则快照的读写与原子替换。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRuleSnapshotStore {
    enabled: bool,
    root_dir: PathBuf,
}

impl RemoteRuleSnapshotStore {
    /// 根据配置创建快照存储。
    pub fn new(config: &RemoteRuleSnapshotConfig) -> Self {
        Self {
            enabled: config.enabled,
            root_dir: PathBuf::from(&config.path),
        }
    }

    /// 返回快照功能当前是否启用。
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// 按来源地址加载最近一次成功快照。
    pub fn load(&self, source_url: &str) -> Result<Option<RemoteRuleSnapshotEnvelope>, AppError> {
        if !self.enabled {
            return Ok(None);
        }

        let Some(path) = self.resolve_load_path(source_url) else {
            return Ok(None);
        };

        let content = fs::read_to_string(&path).map_err(|error| {
            AppError::Cache(format!(
                "failed to read remote rule snapshot '{}': {}",
                path.display(),
                error
            ))
        })?;
        let snapshot: RemoteRuleSnapshotEnvelope =
            serde_json::from_str(&content).map_err(|error| {
                AppError::Cache(format!(
                    "failed to parse remote rule snapshot '{}': {}",
                    path.display(),
                    error
                ))
            })?;

        if snapshot.schema_version != SNAPSHOT_SCHEMA_VERSION {
            return Err(AppError::Cache(format!(
                "remote rule snapshot '{}' uses unsupported schema version {}",
                path.display(),
                snapshot.schema_version
            )));
        }

        if snapshot.source_url != source_url {
            return Err(AppError::Cache(format!(
                "remote rule snapshot '{}' source URL mismatch: expected '{}', got '{}'",
                path.display(),
                source_url,
                snapshot.source_url
            )));
        }

        Ok(Some(snapshot))
    }

    /// 将指定来源的规则保存为最近一次成功快照。
    pub fn save(&self, source_url: &str, rules: &[RouteRuleConfig]) -> Result<(), AppError> {
        if !self.enabled {
            return Ok(());
        }

        fs::create_dir_all(&self.root_dir).map_err(|error| {
            AppError::Cache(format!(
                "failed to create remote rule snapshot directory '{}': {}",
                self.root_dir.display(),
                error
            ))
        })?;

        let snapshot = RemoteRuleSnapshotEnvelope::new(source_url, rules.to_vec());
        let payload = serde_json::to_vec_pretty(&snapshot).map_err(|error| {
            AppError::Cache(format!(
                "failed to serialize remote rule snapshot for '{}': {}",
                source_url, error
            ))
        })?;

        let path = self.snapshot_path(source_url);
        let temp_path = self.temp_snapshot_path(source_url);
        fs::write(&temp_path, payload).map_err(|error| {
            AppError::Cache(format!(
                "failed to write remote rule snapshot '{}': {}",
                temp_path.display(),
                error
            ))
        })?;

        if let Err(error) = self.promote_snapshot(&temp_path, &path) {
            let _ = fs::remove_file(&temp_path);
            return Err(error);
        }

        let legacy_path = self.legacy_snapshot_path(source_url);
        if legacy_path != path && legacy_path.exists() {
            fs::remove_file(&legacy_path).map_err(|error| {
                AppError::Cache(format!(
                    "failed to remove legacy remote rule snapshot '{}': {}",
                    legacy_path.display(),
                    error
                ))
            })?;
        }

        Ok(())
    }

    /// 根据当前活跃来源同步快照目录，删除孤儿快照与遗留临时文件。
    pub fn sync_active_sources(&self, active_urls: &[String]) -> Result<(), AppError> {
        if !self.enabled {
            return Ok(());
        }

        fs::create_dir_all(&self.root_dir).map_err(|error| {
            AppError::Cache(format!(
                "failed to create remote rule snapshot directory '{}': {}",
                self.root_dir.display(),
                error
            ))
        })?;

        let mut active_json_files = HashSet::new();
        let mut active_tmp_prefixes = HashSet::new();
        for source_url in active_urls {
            let current_stem = Self::current_snapshot_stem(source_url);
            let legacy_stem = Self::legacy_snapshot_key(source_url);

            active_json_files.insert(format!("{current_stem}.json"));
            active_json_files.insert(format!("{legacy_stem}.json"));
            active_tmp_prefixes.insert(format!("{current_stem}."));
            active_tmp_prefixes.insert(format!("{legacy_stem}."));
        }

        for entry in fs::read_dir(&self.root_dir).map_err(|error| {
            AppError::Cache(format!(
                "failed to read remote rule snapshot directory '{}': {}",
                self.root_dir.display(),
                error
            ))
        })? {
            let entry = entry.map_err(|error| {
                AppError::Cache(format!(
                    "failed to inspect remote rule snapshot directory '{}': {}",
                    self.root_dir.display(),
                    error
                ))
            })?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };

            let should_keep = if file_name.ends_with(".json") {
                active_json_files.contains(file_name)
            } else if file_name.ends_with(".tmp") {
                active_tmp_prefixes
                    .iter()
                    .any(|prefix| file_name.starts_with(prefix))
            } else {
                true
            };

            if !should_keep {
                fs::remove_file(&path).map_err(|error| {
                    AppError::Cache(format!(
                        "failed to remove stale remote rule snapshot '{}': {}",
                        path.display(),
                        error
                    ))
                })?;
            }
        }

        Ok(())
    }

    /// 计算某个来源地址对应的快照文件路径。
    pub fn snapshot_path(&self, source_url: &str) -> PathBuf {
        self.current_snapshot_path(source_url)
    }

    fn temp_snapshot_path(&self, source_url: &str) -> PathBuf {
        self.root_dir.join(format!(
            "{}.{}.tmp",
            Self::current_snapshot_stem(source_url),
            std::process::id()
        ))
    }

    fn promote_snapshot(&self, temp_path: &Path, final_path: &Path) -> Result<(), AppError> {
        match fs::rename(temp_path, final_path) {
            Ok(()) => Ok(()),
            Err(rename_error) if final_path.exists() => {
                fs::remove_file(final_path).map_err(|error| {
                    AppError::Cache(format!(
                        "failed to replace remote rule snapshot '{}': {}",
                        final_path.display(),
                        error
                    ))
                })?;
                fs::rename(temp_path, final_path).map_err(|error| {
                    AppError::Cache(format!(
                        "failed to promote remote rule snapshot '{}' after replacement fallback: {} (initial rename error: {})",
                        final_path.display(),
                        error,
                        rename_error
                    ))
                })
            }
            Err(error) => Err(AppError::Cache(format!(
                "failed to promote remote rule snapshot '{}': {}",
                final_path.display(),
                error
            ))),
        }
    }

    fn resolve_load_path(&self, source_url: &str) -> Option<PathBuf> {
        let current_path = self.current_snapshot_path(source_url);
        if current_path.exists() {
            return Some(current_path);
        }

        let legacy_path = self.legacy_snapshot_path(source_url);
        if legacy_path.exists() {
            return Some(legacy_path);
        }

        None
    }

    fn current_snapshot_path(&self, source_url: &str) -> PathBuf {
        self.root_dir
            .join(format!("{}.json", Self::current_snapshot_stem(source_url)))
    }

    fn legacy_snapshot_path(&self, source_url: &str) -> PathBuf {
        self.root_dir
            .join(format!("{}.json", Self::legacy_snapshot_key(source_url)))
    }

    fn current_snapshot_stem(source_url: &str) -> String {
        format!(
            "{}-{}",
            Self::source_slug(source_url),
            Self::stable_digest(source_url)
        )
    }

    fn source_slug(source_url: &str) -> String {
        let preferred_text = Url::parse(source_url)
            .ok()
            .map(|url| {
                let mut parts = Vec::new();
                if let Some(host) = url.host_str() {
                    parts.push(host.to_string());
                }
                if let Some(segments) = url.path_segments() {
                    for segment in segments {
                        if !segment.is_empty() {
                            parts.push(segment.to_string());
                        }
                    }
                }
                if parts.is_empty() {
                    source_url.to_string()
                } else {
                    parts.join("-")
                }
            })
            .unwrap_or_else(|| source_url.to_string());

        let mut slug = String::with_capacity(preferred_text.len());
        let mut previous_was_separator = false;
        for ch in preferred_text.chars() {
            if ch.is_ascii_alphanumeric() {
                slug.push(ch.to_ascii_lowercase());
                previous_was_separator = false;
            } else if !previous_was_separator {
                slug.push('-');
                previous_was_separator = true;
            }
        }

        let slug = slug.trim_matches('-');
        if slug.is_empty() {
            "remote-rule".to_string()
        } else if slug.len() > 80 {
            slug[..80].trim_matches('-').to_string()
        } else {
            slug.to_string()
        }
    }

    fn stable_digest(source_url: &str) -> String {
        const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;

        let mut hash = FNV_OFFSET_BASIS;
        for byte in source_url.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }

        format!("{hash:016x}")
    }

    fn legacy_snapshot_key(source_url: &str) -> String {
        let mut hasher = DefaultHasher::new();
        source_url.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
}
