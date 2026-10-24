//! 预检六项（T05_DESIGN §4）：fail-closed，逐项留证据，报告落
//! `{work-dir}/preflight-report.json`（预检拒绝时 work-dir 是唯一允许的
//! 写入目标——三目录分离铁律）。
//!
//! 1. version-identity：源根确为可识别的 Lingxi home（身份标记存在）。
//! 2. disk-space：backup-dir 卷与 target 父卷可用空间 ≥ 需求
//!    （源清单字节 + runs.db 估计 + 256MiB 余量；`--min-avail-bytes-override`
//!    供 A10 小卷演练钉死阈值）。
//! 3. writability：backup-dir / target 父目录 / work-dir 写探针
//!    （tmp 文件建即删）；**源根永远不做写探针**。
//! 4. old-instance-state：双锁探针（旧栈 server-info 四态 + 新栈 OS 锁）。
//! 5. data-consistency：源 SQLite copy-set 到 scratch 后对**副本**做
//!    `PRAGMA quick_check`（P6：源库永不直接打开）。
//! 6. backup-target：备份落点不得位于 source 或 target 之内（反之亦然）。

#[cfg(test)]
use lingxi_adapters::storage::legacy_import::manifest;
use lingxi_adapters::storage::legacy_import::manifest::Manifest;
use lingxi_adapters::storage::statfs;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

use super::probe;

pub const PREFLIGHT_REPORT_FILE: &str = "preflight-report.json";
pub const SLACK_BYTES: u64 = 256 * 1024 * 1024; // 256MiB 余量

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PreflightCheck {
    pub id: String,
    /// ok | refused
    pub status: String,
    pub detail: String,
    pub evidence: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightReport {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u64,
    #[serde(rename = "ranAt")]
    pub ran_at: String,
    /// passed | refused
    pub outcome: String,
    pub checks: Vec<PreflightCheck>,
}

#[derive(Debug)]
pub enum PreflightError {
    /// 任一项 refused：报告已落盘，调用方须 journal.refuse 后退出。
    Refused {
        report_path: PathBuf,
    },
    Io {
        path: PathBuf,
        detail: String,
    },
}

impl fmt::Display for PreflightError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PreflightError::Refused { report_path } => {
                write!(f, "preflight refused; report at {}", report_path.display())
            }
            PreflightError::Io { path, detail } => {
                write!(f, "preflight io at {}: {detail}", path.display())
            }
        }
    }
}

impl std::error::Error for PreflightError {}

pub struct PreflightInputs<'a> {
    pub source: &'a Path,
    pub target: &'a Path,
    pub work_dir: &'a Path,
    pub backup_dir: &'a Path,
    pub source_manifest: &'a Manifest,
    /// A10 演练用：钉死最低可用字节阈值（0 = 不启用）。
    pub min_avail_bytes_override: Option<u64>,
}

fn ok(id: &str, detail: String, evidence: serde_json::Value) -> PreflightCheck {
    PreflightCheck {
        id: id.to_string(),
        status: "ok".into(),
        detail,
        evidence,
    }
}

fn refused(id: &str, detail: String, evidence: serde_json::Value) -> PreflightCheck {
    PreflightCheck {
        id: id.to_string(),
        status: "refused".into(),
        detail,
        evidence,
    }
}

/// 检查 1：版本身份——源根须含 Lingxi home 身份标记（三取一）。
fn check_version_identity(source: &Path) -> PreflightCheck {
    let markers = ["data-epoch.json", "server-info.json", "user/user.md"];
    let present: Vec<&str> = markers
        .iter()
        .copied()
        .filter(|m| source.join(m).is_file())
        .collect();
    let evidence = serde_json::json!({"markersChecked": markers, "markersPresent": present});
    if present.is_empty() {
        refused(
            "version-identity",
            format!(
                "{} does not look like a Lingxi home (no identity markers)",
                source.display()
            ),
            evidence,
        )
    } else {
        ok(
            "version-identity",
            format!("identity markers present: {}", present.join(", ")),
            evidence,
        )
    }
}

/// 需求字节 = 源清单总字节 + runs.db 估计（含 wal/shm）+ 256MiB 余量。
pub fn required_bytes(source: &Path, source_manifest: &Manifest) -> u64 {
    let manifest_bytes: u64 = source_manifest.entries.iter().map(|e| e.size).sum();
    let runs_estimate: u64 = [
        "lingxi-service/runs.db",
        "lingxi-service/runs.db-wal",
        "lingxi-service/runs.db-shm",
    ]
    .iter()
    .filter_map(|rel| std::fs::metadata(source.join(rel)).ok().map(|m| m.len()))
    .sum();
    manifest_bytes + runs_estimate + SLACK_BYTES
}

/// 检查 2：磁盘空间（backup-dir 卷 + target 父卷各自 >= 需求）。
fn check_disk_space(inputs: &PreflightInputs) -> PreflightCheck {
    let required = inputs
        .min_avail_bytes_override
        .unwrap_or_else(|| required_bytes(inputs.source, inputs.source_manifest));
    let target_anchor = inputs.target.parent().unwrap_or(inputs.target);
    let volumes = [
        ("backupDirVolume", inputs.backup_dir),
        ("targetParentVolume", target_anchor),
    ];
    let mut measurements = Vec::new();
    let mut shortfalls = Vec::new();
    for (label, path) in volumes {
        match statfs::available_bytes(path) {
            Ok(avail) => {
                measurements.push(serde_json::json!({
                    "volume": label, "anchor": path.display().to_string(),
                    "availableBytes": avail, "requiredBytes": required,
                }));
                if avail < required {
                    shortfalls.push(format!("{label}: avail {avail} < required {required}"));
                }
            }
            Err(err) => {
                shortfalls.push(format!("{label}: statvfs failed loudly: {err}"));
            }
        }
    }
    let evidence = serde_json::json!({
        "requiredBytes": required,
        "override": inputs.min_avail_bytes_override,
        "volumes": measurements,
    });
    if shortfalls.is_empty() {
        ok(
            "disk-space",
            format!("all volumes >= required {required} bytes"),
            evidence,
        )
    } else {
        refused("disk-space", shortfalls.join("; "), evidence)
    }
}

/// 检查 3：可写性探针（源根永不写探针）。
fn check_writability(inputs: &PreflightInputs) -> PreflightCheck {
    let target_anchor = inputs.target.parent().unwrap_or(inputs.target);
    let dirs = [
        ("backupDir", inputs.backup_dir),
        ("targetParent", target_anchor),
        ("workDir", inputs.work_dir),
    ];
    let mut probed = Vec::new();
    let mut failures = Vec::new();
    for (label, dir) in dirs {
        let probe_file = dir.join(format!(".t05-preflight-probe-{}", std::process::id()));
        let result = std::fs::create_dir_all(dir)
            .and_then(|_| std::fs::write(&probe_file, b"probe"))
            .and_then(|_| std::fs::remove_file(&probe_file));
        match result {
            Ok(()) => probed.push(serde_json::json!({"dir": label, "path": dir.display().to_string(), "writable": true})),
            Err(err) => {
                probed.push(serde_json::json!({"dir": label, "path": dir.display().to_string(), "writable": false}));
                failures.push(format!("{label} {}: {err}", dir.display()));
            }
        }
    }
    let evidence = serde_json::json!({
        "probed": probed,
        "sourceWriteProbe": "never (source root is write-probe-free by rule)",
    });
    if failures.is_empty() {
        ok(
            "writability",
            "backup/target-parent/work dirs writable".into(),
            evidence,
        )
    } else {
        refused("writability", failures.join("; "), evidence)
    }
}

/// 检查 4：旧实例状态（双锁探针；阻断态 = refused，探针 io 故障也 = refused）。
fn check_old_instance_state(source: &Path) -> PreflightCheck {
    let legacy = probe::probe_legacy_server_info(source);
    let rust_lock = probe::probe_rust_instance_lock(source);
    let evidence = serde_json::json!({
        "legacy": legacy.as_ref().map(|o| serde_json::to_value(o).unwrap_or_default()).unwrap_or_else(|e| serde_json::json!({"error": e.to_string()})),
        "rustLock": rust_lock.as_ref().map(|o| serde_json::to_value(o).unwrap_or_default()).unwrap_or_else(|e| serde_json::json!({"error": e.to_string()})),
    });
    let mut blocks = Vec::new();
    match &legacy {
        Ok(outcome) if outcome.probe.is_blocking() => blocks.push(format!(
            "legacy stack probe = {:?}: {}",
            outcome.probe, outcome.detail
        )),
        Err(err) => blocks.push(format!("legacy probe failed loudly: {err}")),
        _ => {}
    }
    match &rust_lock {
        Ok(outcome) if outcome.probe.is_blocking() => {
            blocks.push(format!("rust instance.lock held: {}", outcome.detail))
        }
        Err(err) => blocks.push(format!("rust lock probe failed loudly: {err}")),
        _ => {}
    }
    if blocks.is_empty() {
        ok(
            "old-instance-state",
            "no live writer detected on either stack".into(),
            evidence,
        )
    } else {
        refused("old-instance-state", blocks.join("; "), evidence)
    }
}

/// 检查 5：数据一致性——源 SQLite copy-set 到 scratch 后对**副本** quick_check。
fn check_data_consistency(
    source: &Path,
    work_dir: &Path,
    source_manifest: &Manifest,
) -> PreflightCheck {
    let scratch = work_dir.join("preflight-scratch");
    let mut checked = Vec::new();
    let mut failures = Vec::new();
    let clean = |path: &Path| {
        if path.exists() {
            let _ = std::fs::remove_dir_all(path);
        }
    };
    for entry in &source_manifest.entries {
        let rel = entry.relpath.as_str();
        if !(rel.ends_with(".db") || rel.ends_with(".sqlite") || rel.ends_with(".sqlite3")) {
            continue;
        }
        let scratch_db = scratch.join("check.db");
        clean(&scratch);
        if let Err(err) = std::fs::create_dir_all(&scratch) {
            failures.push(format!("{rel}: scratch: {err}"));
            continue;
        }
        let mut copy_ok = true;
        for suffix in ["", "-wal", "-shm"] {
            let src = source.join(format!("{rel}{suffix}"));
            let dst = scratch.join(format!("check.db{suffix}"));
            match std::fs::copy(&src, &dst) {
                Ok(_) => {}
                Err(err) if suffix.is_empty() || err.kind() != std::io::ErrorKind::NotFound => {
                    failures.push(format!("{rel}{suffix}: copy-set: {err}"));
                    copy_ok = false;
                    break;
                }
                Err(_) => {}
            }
        }
        if !copy_ok {
            continue;
        }
        let verdict = rusqlite::Connection::open(&scratch_db).and_then(|conn| {
            conn.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
        });
        match verdict {
            Ok(v) if v == "ok" => checked.push(rel.to_string()),
            Ok(v) => failures.push(format!("{rel}: quick_check = {v}")),
            Err(err) => failures.push(format!("{rel}: open/quick_check on copy: {err}")),
        }
    }
    clean(&scratch);
    let evidence = serde_json::json!({
        "sqliteCopiesChecked": checked,
        "note": "source DBs never opened directly; quick_check ran on scratch copies (P6)",
    });
    if failures.is_empty() {
        ok(
            "data-consistency",
            format!("{} sqlite copies passed quick_check", checked.len()),
            evidence,
        )
    } else {
        refused("data-consistency", failures.join("; "), evidence)
    }
}

/// 检查 6：备份落点不得与 source/target 互相嵌套。
fn check_backup_target(inputs: &PreflightInputs) -> PreflightCheck {
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let backup = canon(inputs.backup_dir);
    let source = canon(inputs.source);
    let target = canon(inputs.target);
    let evidence = serde_json::json!({
        "backupDir": backup.display().to_string(),
        "source": source.display().to_string(),
        "target": target.display().to_string(),
    });
    let mut problems = Vec::new();
    if backup.starts_with(&source) {
        problems.push("backup-dir is inside source");
    }
    if backup.starts_with(&target) || target.starts_with(&backup) {
        problems.push("backup-dir and target nest each other");
    }
    if backup == source || backup == target {
        problems.push("backup-dir equals source/target");
    }
    if source == target {
        problems.push("source equals target");
    }
    if problems.is_empty() {
        ok(
            "backup-target",
            "backup target separated from source and target".into(),
            evidence,
        )
    } else {
        refused("backup-target", problems.join("; "), evidence)
    }
}

/// 跑六项预检；任一 refused → 报告落盘后返回 Err(Refused)。
pub fn run_preflight(inputs: &PreflightInputs) -> Result<PreflightReport, PreflightError> {
    let checks = vec![
        check_version_identity(inputs.source),
        check_disk_space(inputs),
        check_writability(inputs),
        check_old_instance_state(inputs.source),
        check_data_consistency(inputs.source, inputs.work_dir, inputs.source_manifest),
        check_backup_target(inputs),
    ];
    let outcome = if checks.iter().any(|c| c.status == "refused") {
        "refused"
    } else {
        "passed"
    };
    let report = PreflightReport {
        schema_version: 1,
        ran_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        outcome: outcome.to_string(),
        checks,
    };
    std::fs::create_dir_all(inputs.work_dir).map_err(|err| PreflightError::Io {
        path: inputs.work_dir.to_path_buf(),
        detail: format!("create_dir_all: {err}"),
    })?;
    let report_path = inputs.work_dir.join(PREFLIGHT_REPORT_FILE);
    let mut body = serde_json::to_string_pretty(&report).map_err(|err| PreflightError::Io {
        path: report_path.clone(),
        detail: format!("serialize: {err}"),
    })?;
    body.push('\n');
    crate::paths::atomic_write_private(&report_path, body.as_bytes()).map_err(|err| {
        PreflightError::Io {
            path: report_path.clone(),
            detail: format!("atomic write: {err}"),
        }
    })?;
    if outcome == "refused" {
        Err(PreflightError::Refused { report_path })
    } else {
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_dir(tag: &str) -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "t05-preflight-test-{}-{}-{tag}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    struct Fixture {
        source: PathBuf,
        target: PathBuf,
        work: PathBuf,
        backup: PathBuf,
        manifest: Manifest,
    }

    fn fixture(tag: &str) -> Fixture {
        let source = temp_dir(&format!("{tag}-src"));
        let target = temp_dir(&format!("{tag}-tgt")).join("new-root"); // 尚不存在
        let work = temp_dir(&format!("{tag}-work"));
        let backup = temp_dir(&format!("{tag}-bak"));
        std::fs::create_dir_all(source.join("user")).unwrap();
        std::fs::write(source.join("user/user.md"), b"# u\n").unwrap();
        std::fs::write(source.join("data-epoch.json"), b"{}").unwrap();
        let manifest = manifest::build_manifest(&source).unwrap();
        Fixture {
            source,
            target,
            work,
            backup,
            manifest,
        }
    }

    fn inputs<'a>(f: &'a Fixture) -> PreflightInputs<'a> {
        PreflightInputs {
            source: &f.source,
            target: &f.target,
            work_dir: &f.work,
            backup_dir: &f.backup,
            source_manifest: &f.manifest,
            min_avail_bytes_override: None,
        }
    }

    impl Fixture {
        fn cleanup(&self) {
            for d in [&self.source, &self.work, &self.backup] {
                let _ = std::fs::remove_dir_all(d);
            }
            if let Some(parent) = self.target.parent() {
                let _ = std::fs::remove_dir_all(parent);
            }
        }
    }

    #[test]
    fn healthy_home_passes_all_six() {
        let f = fixture("pass");
        let report = run_preflight(&inputs(&f)).unwrap();
        assert_eq!(report.outcome, "passed");
        assert_eq!(report.checks.len(), 6);
        assert!(
            report.checks.iter().all(|c| c.status == "ok"),
            "{:?}",
            report.checks
        );
        assert!(f.work.join(PREFLIGHT_REPORT_FILE).exists());
        f.cleanup();
    }

    #[test]
    fn unrecognizable_home_refuses_version_identity() {
        let f = fixture("noidentity");
        std::fs::remove_file(f.source.join("user/user.md")).unwrap();
        std::fs::remove_file(f.source.join("data-epoch.json")).unwrap();
        let err = run_preflight(&inputs(&f)).unwrap_err();
        assert!(err.to_string().contains("preflight refused"), "{err}");
        let body = std::fs::read_to_string(f.work.join(PREFLIGHT_REPORT_FILE)).unwrap();
        assert!(body.contains("version-identity"));
        f.cleanup();
    }

    #[test]
    fn disk_override_forces_loud_refusal() {
        let f = fixture("a10");
        let mut inp = inputs(&f);
        inp.min_avail_bytes_override = Some(u64::MAX / 2);
        let err = run_preflight(&inp).unwrap_err();
        assert!(err.to_string().contains("preflight refused"), "{err}");
        let body = std::fs::read_to_string(f.work.join(PREFLIGHT_REPORT_FILE)).unwrap();
        assert!(body.contains("disk-space"), "{body}");
        // A10 红线：target 未被创建，源零变化。
        assert!(!f.target.exists(), "target must not be created on refusal");
        let after = manifest::build_manifest(&f.source).unwrap();
        assert_eq!(after, f.manifest, "source mutated by preflight");
        f.cleanup();
    }

    #[test]
    fn backup_inside_source_is_refused() {
        let f = fixture("nest");
        let nested = f.source.join("backups");
        let mut inp = inputs(&f);
        inp.backup_dir = &nested;
        let err = run_preflight(&inp).unwrap_err();
        assert!(err.to_string().contains("preflight refused"), "{err}");
        let body = std::fs::read_to_string(f.work.join(PREFLIGHT_REPORT_FILE)).unwrap();
        assert!(body.contains("backup-target"), "{body}");
        f.cleanup();
    }

    #[test]
    fn held_rust_lock_refuses_old_instance_state() {
        let f = fixture("heldlock");
        let lock_dir = f.source.join("lingxi-service");
        std::fs::create_dir_all(&lock_dir).unwrap();
        let holder = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(lock_dir.join("instance.lock"))
            .unwrap();
        holder.try_lock().unwrap();
        let err = run_preflight(&inputs(&f)).unwrap_err();
        assert!(err.to_string().contains("preflight refused"), "{err}");
        let body = std::fs::read_to_string(f.work.join(PREFLIGHT_REPORT_FILE)).unwrap();
        assert!(body.contains("old-instance-state"), "{body}");
        drop(holder);
        f.cleanup();
    }

    #[test]
    fn corrupt_sqlite_copy_fails_consistency_loudly() {
        let f = fixture("corruptdb");
        let db_dir = f.source.join("agents/a1/memory");
        std::fs::create_dir_all(&db_dir).unwrap();
        std::fs::write(db_dir.join("facts.db"), b"not a sqlite file at all").unwrap();
        let manifest = manifest::build_manifest(&f.source).unwrap();
        let mut inp = inputs(&f);
        inp.source_manifest = &manifest;
        let err = run_preflight(&inp).unwrap_err();
        assert!(err.to_string().contains("preflight refused"), "{err}");
        let body = std::fs::read_to_string(f.work.join(PREFLIGHT_REPORT_FILE)).unwrap();
        assert!(body.contains("data-consistency"), "{body}");
        f.cleanup();
    }
}
