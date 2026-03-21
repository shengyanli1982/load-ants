use crate::config::{RemoteRuleSnapshotConfig, RouteRuleConfig};
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fmt::Write;
use std::fs;
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

        let path = self.snapshot_path(source_url);
        if !path.exists() {
            return Ok(None);
        }

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
            let snapshot_key = Self::snapshot_key(source_url);
            active_json_files.insert(format!("{snapshot_key}.json"));
            active_tmp_prefixes.insert(format!("{snapshot_key}."));
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
        self.root_dir
            .join(format!("{}.json", Self::snapshot_key(source_url)))
    }

    fn temp_snapshot_path(&self, source_url: &str) -> PathBuf {
        self.root_dir.join(format!(
            "{}.{}.tmp",
            Self::snapshot_key(source_url),
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

    fn snapshot_key(source_url: &str) -> String {
        let digest = Sha256::digest(source_url.as_bytes());
        let mut output = String::with_capacity(digest.len() * 2);
        for byte in digest {
            let _ = write!(&mut output, "{byte:02x}");
        }
        output
    }
}
