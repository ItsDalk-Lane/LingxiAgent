//! 静止点（T05_DESIGN §5.3；裁决点 P7）。
//!
//! - **全集枚举**：69 类从 T04 静态注册表（classes::CLASSES，由 R00
//!   STORES.json × R01 DATA_COMPATIBILITY_MATRIX.json 生成）全量展开，
//!   每类给出 T05 处置（disposition）。**计数与 R01 台账不符 = 拒绝**
//!   （宁可停摆，绝不带着「以为枚举全了」前进）。
//! - **静止窗口**：源根 T04 manifest 双遍扫描（pass A → 校验读取期 →
//!   pass B），两遍**逐条相等**（relpath+size+sha256 全等）才视为静止；
//!   备份完成后第三遍再对拍 pass A——漂移 = 响亮失败，绝不带病切换。
//! - stillpoint-manifest.json 落 `{work-dir}`（atomic_write_private）。

use lingxi_adapters::storage::legacy_import::classes::{self, Strategy};
use lingxi_adapters::storage::legacy_import::manifest::{self, Manifest};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

pub const STILLPOINT_FILE_NAME: &str = "stillpoint-manifest.json";

/// R01 台账钉死的类数与分策略计数（T05_DESIGN §5.3 表）。
pub const EXPECTED_TOTAL: usize = 69;
pub const EXPECTED_REGISTER: usize = 35; // preserve-format
pub const EXPECTED_REGENERABLE: usize = 13;
pub const EXPECTED_CONVERT: usize = 11; // switch-to-new-authority
pub const EXPECTED_EPOCH: usize = 4; // epoch-mechanism
pub const EXPECTED_SHELL: usize = 6; // shell-local

/// T05 处置（§5.3 五桶）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Disposition {
    /// 备份 + T04 转换进新根。
    #[serde(rename = "backup+import")]
    BackupImport,
    /// 备份 + 原样搬迁进新根。
    #[serde(rename = "backup+carry")]
    BackupCarry,
    /// 只备份 + 重建收据（数据本体不搬迁，启动时重建）。
    #[serde(rename = "backup-only-rebuild-receipt")]
    BackupOnlyRebuildReceipt,
    /// epoch 元数据：由 epoch 采用流程处理（T05 §7）。
    #[serde(rename = "epoch-meta-handled-by-adoption")]
    EpochMetaHandledByAdoption,
    /// 壳本地：不迁移。
    #[serde(rename = "out-of-scope-shell-local")]
    OutOfScopeShellLocal,
}

impl Disposition {
    pub fn from_strategy(strategy: Strategy) -> Self {
        match strategy {
            Strategy::Convert => Self::BackupImport,
            Strategy::Register => Self::BackupCarry,
            Strategy::Regenerable => Self::BackupOnlyRebuildReceipt,
            Strategy::OutOfScopeEpoch => Self::EpochMetaHandledByAdoption,
            Strategy::OutOfScopeShell => Self::OutOfScopeShellLocal,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoreRow {
    #[serde(rename = "classId")]
    pub class_id: String,
    pub strategy: String,
    pub disposition: Disposition,
    pub external: bool,
    pub patterns: Vec<String>,
}

#[derive(Debug)]
pub enum StillpointError {
    /// 注册表与 R01 台账不符 = 拒绝（缺类 = 拒绝的硬实现）。
    RegistryMismatch {
        detail: String,
    },
    Manifest {
        detail: String,
    },
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl fmt::Display for StillpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StillpointError::RegistryMismatch { detail } => {
                write!(f, "stillpoint registry mismatch (refuse): {detail}")
            }
            StillpointError::Manifest { detail } => write!(f, "source manifest error: {detail}"),
            StillpointError::Io { path, source } => {
                write!(f, "stillpoint io error at {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for StillpointError {}

fn strategy_name(strategy: Strategy) -> &'static str {
    match strategy {
        Strategy::Convert => "switch-to-new-authority",
        Strategy::Register => "preserve-format",
        Strategy::Regenerable => "regenerable",
        Strategy::OutOfScopeEpoch => "epoch-mechanism",
        Strategy::OutOfScopeShell => "shell-local",
    }
}

/// 全集枚举 + R01 计数对拍。任何不符 = RegistryMismatch 拒绝。
pub fn enumerate_stores() -> Result<Vec<StoreRow>, StillpointError> {
    let mut rows = Vec::with_capacity(classes::CLASSES.len());
    let mut count_register = 0usize;
    let mut count_regenerable = 0usize;
    let mut count_convert = 0usize;
    let mut count_epoch = 0usize;
    let mut count_shell = 0usize;
    for def in classes::CLASSES {
        match def.strategy {
            Strategy::Register => count_register += 1,
            Strategy::Regenerable => count_regenerable += 1,
            Strategy::Convert => count_convert += 1,
            Strategy::OutOfScopeEpoch => count_epoch += 1,
            Strategy::OutOfScopeShell => count_shell += 1,
        }
        rows.push(StoreRow {
            class_id: def.id.to_string(),
            strategy: strategy_name(def.strategy).to_string(),
            disposition: Disposition::from_strategy(def.strategy),
            external: def.external,
            patterns: def.patterns.iter().map(|p| p.to_string()).collect(),
        });
    }
    let mismatches = [
        (rows.len(), EXPECTED_TOTAL, "total"),
        (count_register, EXPECTED_REGISTER, "preserve-format"),
        (count_regenerable, EXPECTED_REGENERABLE, "regenerable"),
        (count_convert, EXPECTED_CONVERT, "switch-to-new-authority"),
        (count_epoch, EXPECTED_EPOCH, "epoch-mechanism"),
        (count_shell, EXPECTED_SHELL, "shell-local"),
    ]
    .into_iter()
    .filter(|(actual, expected, _)| actual != expected)
    .map(|(actual, expected, name)| format!("{name}: {actual} != {expected}"))
    .collect::<Vec<_>>();
    if !mismatches.is_empty() {
        return Err(StillpointError::RegistryMismatch {
            detail: mismatches.join("; "),
        });
    }
    Ok(rows)
}

pub fn stillpoint_path(work_dir: &Path) -> PathBuf {
    work_dir.join(STILLPOINT_FILE_NAME)
}

/// 一遍源根清单（T04 builder：确定性排序，IO 错误响亮）。
pub fn scan_pass(source: &Path) -> Result<Manifest, StillpointError> {
    manifest::build_manifest(source).map_err(|err| StillpointError::Manifest {
        detail: format!("{}: {}", err.relpath, err.message),
    })
}

/// 双遍相等性：逐条 relpath+size+sha256 全等（Manifest PartialEq 即逐条）。
pub fn passes_equal(a: &Manifest, b: &Manifest) -> bool {
    a == b
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StillpointManifestFile {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u64,
    #[serde(rename = "capturedAt")]
    pub captured_at: String,
    pub source: String,
    #[serde(rename = "passAHash")]
    pub pass_a_hash: String,
    #[serde(rename = "passBHash")]
    pub pass_b_hash: String,
    #[serde(rename = "passesEqual")]
    pub passes_equal: bool,
    pub stores: Vec<StoreRow>,
    /// 静止窗口内的完整文件清单（pass A 内容，逐行 sha256 行）。
    pub files: Vec<serde_json::Value>,
    #[serde(rename = "skippedSpecial")]
    pub skipped_special: Vec<serde_json::Value>,
}

/// 建立静止窗口：枚举 + pass A + pass B + 相等性断言 + 落盘。
/// 两遍不等 = Err（漂移，fail-closed）。
pub fn establish(
    source: &Path,
    work_dir: &Path,
) -> Result<StillpointManifestFile, StillpointError> {
    let stores = enumerate_stores()?;
    let pass_a = scan_pass(source)?;
    let pass_b = scan_pass(source)?;
    let equal = passes_equal(&pass_a, &pass_b);
    let manifest_json = manifest::manifest_json(&pass_a);
    let files = manifest_json["files"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let skipped_special = manifest_json["skippedSpecial"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let file = StillpointManifestFile {
        schema_version: 1,
        captured_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        source: source.display().to_string(),
        pass_a_hash: manifest::source_root_sha256(&pass_a),
        pass_b_hash: manifest::source_root_sha256(&pass_b),
        passes_equal: equal,
        stores,
        files,
        skipped_special,
    };
    let path = stillpoint_path(work_dir);
    let mut body =
        serde_json::to_string_pretty(&file).map_err(|err| StillpointError::Manifest {
            detail: format!("serialize stillpoint: {err}"),
        })?;
    body.push('\n');
    crate::paths::atomic_write_private(&path, body.as_bytes()).map_err(|source| {
        StillpointError::Io {
            path: path.clone(),
            source,
        }
    })?;
    if !equal {
        return Err(StillpointError::Manifest {
            detail: format!(
                "source drifted inside stillness window: passA {} != passB {}",
                file.pass_a_hash, file.pass_b_hash
            ),
        });
    }
    Ok(file)
}

/// 备份后第三遍对拍（§5.3）：仍须与 pass A 逐条相等。
pub fn verify_post_backup(
    source: &Path,
    established: &StillpointManifestFile,
) -> Result<(), StillpointError> {
    let third = scan_pass(source)?;
    let third_hash = manifest::source_root_sha256(&third);
    if third_hash != established.pass_a_hash {
        return Err(StillpointError::Manifest {
            detail: format!(
                "source drifted during backup: passA {} != postBackup {}",
                established.pass_a_hash, third_hash
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_dir(tag: &str) -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "t05-stillpoint-test-{}-{}-{tag}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn registry_enumeration_matches_r01_ledger() {
        let rows = enumerate_stores().unwrap();
        assert_eq!(rows.len(), 69);
        let by = |d: Disposition| rows.iter().filter(|r| r.disposition == d).count();
        assert_eq!(by(Disposition::BackupCarry), 35);
        assert_eq!(by(Disposition::BackupOnlyRebuildReceipt), 13);
        assert_eq!(by(Disposition::BackupImport), 11);
        assert_eq!(by(Disposition::EpochMetaHandledByAdoption), 4);
        assert_eq!(by(Disposition::OutOfScopeShellLocal), 6);
        // 无重复 classId。
        let mut ids: Vec<_> = rows.iter().map(|r| r.class_id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 69, "duplicate class ids present");
    }

    #[test]
    fn still_window_on_quiet_source_establishes_and_verifies() {
        let src = temp_dir("quiet-src");
        let work = temp_dir("quiet-work");
        std::fs::create_dir_all(src.join("user")).unwrap();
        std::fs::write(src.join("user/user.md"), b"# user\n").unwrap();
        std::fs::write(src.join("server-info.json"), b"{}").unwrap();
        let established = establish(&src, &work).unwrap();
        assert!(established.passes_equal);
        assert_eq!(established.stores.len(), 69);
        assert!(stillpoint_path(&work).exists());
        verify_post_backup(&src, &established).unwrap();
        std::fs::remove_dir_all(&src).unwrap();
        std::fs::remove_dir_all(&work).unwrap();
    }

    #[test]
    fn post_backup_drift_is_loud_failure() {
        let src = temp_dir("drift-src");
        let work = temp_dir("drift-work");
        std::fs::write(src.join("a.txt"), b"one\n").unwrap();
        let established = establish(&src, &work).unwrap();
        std::fs::write(src.join("a.txt"), b"two\n").unwrap();
        let err = verify_post_backup(&src, &established).unwrap_err();
        assert!(err.to_string().contains("drifted during backup"), "{err}");
        std::fs::remove_dir_all(&src).unwrap();
        std::fs::remove_dir_all(&work).unwrap();
    }
}
