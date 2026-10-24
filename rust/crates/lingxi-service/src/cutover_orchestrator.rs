//! R08-T05 Phase E：migrate 动词编排器（设计 §3 十相状态机驱动 + §10
//! 单写者裁决记录落盘）。
//!
//! 纪律：
//! - journal 相位是恢复唯一权威；续跑逐相重验前置，失效即 `rewind_to`
//!   （绝不跳相、绝不降格为警告）。
//! - 任一进相失败 = `journal.refuse` 终态 + 响亮退出码 2。
//! - `--stop-after-phase <p>`：干净停在相边界（journal 记 stopped）。
//! - 源根零写：编排器只在 work-dir / backup-dir / target / pointer 落笔。
//! - selfcheck 相（9）与 committed 相（10）在常量切换（D2）前由 e2e 用
//!   `--stop-after-phase pointer_switch` 截断；实现本身完整。

use lingxi_adapters::storage::legacy_import::{
    manifest, ImportArgs, RECEIPT_DIR_RELPATH, TARGET_DB_RELPATH,
};
use lingxi_adapters::storage::migrations;
use lingxi_service::cutover::backup;
use lingxi_service::cutover::journal::{
    self, AdoptBookmark, CutoverInputs, CutoverJournal, ImportBookmark,
};
use lingxi_service::cutover::pointer;
use lingxi_service::cutover::preflight::{self, PreflightInputs};
use lingxi_service::cutover::probe;
use lingxi_service::cutover::selfcheck::{self, SelfCheckExpectations};
use lingxi_service::cutover::stillpoint::{self, StillpointManifestFile};
use lingxi_service::epoch::{self, EpochAdoptionEvidence, StampRead};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::legacy_import_runner::{self, VerbFailure};

/// 单写者裁决记录文件名（设计 §10）。
pub const RULING_FILE_NAME: &str = "single-writer-ruling.json";

/// migrate 动词入参（main.rs 解析后透传）。
pub struct MigrateArgs {
    pub source: PathBuf,
    pub target: PathBuf,
    pub work_dir: PathBuf,
    pub backup_dir: PathBuf,
    pub pointer: PathBuf,
    /// --resume <cutoverId>：续跑既有 journal；缺省 = 全新切割。
    pub resume: Option<String>,
    /// --stop-after-phase <phase>：相边界干净停（A09 恢复演练注入点）。
    pub stop_after_phase: Option<String>,
    /// --min-avail-bytes-override <n>：A10 小卷演练阈值注入。
    pub min_avail_bytes_override: Option<u64>,
}

/// 动词出口摘要（调用方打印 CUTOVER_RESULT 行）。
#[derive(Debug)]
pub struct MigrateOutcome {
    pub cutover_id: String,
    pub phase: String,
    /// committed | stopped | already-committed
    pub outcome: &'static str,
    pub detail: String,
}

fn verb_err(code: u8, line: String) -> VerbFailure {
    VerbFailure { code, line }
}

fn journal_fail(err: journal::JournalError) -> VerbFailure {
    verb_err(2, format!("error: migrate: journal: {err}"))
}

fn now_unix_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// cutoverId：`<unix_ms>-<pid>`，与 T04 runId 同族的人类可读唯一 id。
fn new_cutover_id() -> String {
    format!("cut-{}-{}", now_unix_ms(), std::process::id())
}

/// 目标根计数/版本探针（import_verify 相与 epoch verify 闭包共用）。
/// 目标根是己方产物，直接只读打开 runs.db；缺库/缺表 = 响亮 Err。
fn probe_target_counts(target: &Path) -> Result<(u64, u64, u64, u64), String> {
    let db_path = target.join(TARGET_DB_RELPATH);
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("open target runs.db {}: {e}", db_path.display()))?;
    let count = |sql: &str| -> Result<u64, String> {
        conn.query_row(sql, [], |row| row.get::<_, i64>(0))
            .map(|v| v as u64)
            .map_err(|e| format!("count query failed ({sql}): {e}"))
    };
    let sessions = count("SELECT COUNT(*) FROM sessions")?;
    let model_call_usage = count("SELECT COUNT(*) FROM model_call_usage")?;
    let cron_jobs = count("SELECT COUNT(*) FROM automation_jobs_v1 WHERE deleted_at IS NULL")?;
    let user_version: u64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map(|v: i64| v as u64)
        .map_err(|e| format!("PRAGMA user_version: {e}"))?;
    Ok((sessions, model_call_usage, cron_jobs, user_version))
}

/// A09 kill -9 恢复的发现结果（import 相内被杀时 journal 尚无书签——
/// 书签只在 run 结束后落盘；续跑须从目标 runs.db 发现在途 run）。
enum InflightImport {
    /// 目标 runs.db 不存在：全新导入。
    None,
    /// 库文件存在但可证为空壳（零用户表，或 import_runs 存在而零行）：
    /// kill 落在建库窗口，无任何用户数据；调用方删除 runs.db 三件套后全新导入。
    EmptyShell,
    /// 恰好一个在途 run（running=被 kill；interrupted=批次边界干净停）：续跑之。
    Run(String),
}

/// 从目标 runs.db 发现在途导入 run。任何歧义一律响亮 Err（绝不静默重跑）：
/// - 多个在途 run；或
/// - 已有 completed run 而 journal 无 completed 书签（不该发生，拒绝猜测）。
fn discover_inflight_import(target: &Path) -> Result<InflightImport, String> {
    let db_path = target.join(TARGET_DB_RELPATH);
    if !db_path.exists() {
        return Ok(InflightImport::None);
    }
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("open target runs.db {}: {e}", db_path.display()))?;
    let user_tables: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("inspect sqlite_master in {}: {e}", db_path.display()))?;
    if user_tables == 0 {
        return Ok(InflightImport::EmptyShell);
    }
    let has_runs_table: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='import_runs'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|e| format!("inspect import_runs in {}: {e}", db_path.display()))?
        > 0;
    if !has_runs_table {
        return Ok(InflightImport::EmptyShell);
    }
    let mut stmt = conn
        .prepare("SELECT run_id, state FROM import_runs ORDER BY rowid")
        .map_err(|e| format!("read import_runs in {}: {e}", db_path.display()))?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| format!("read import_runs in {}: {e}", db_path.display()))?
        .collect::<Result<_, _>>()
        .map_err(|e| format!("read import_runs in {}: {e}", db_path.display()))?;
    let inflight: Vec<String> = rows
        .iter()
        .filter(|(_, state)| state == "running" || state == "interrupted")
        .map(|(id, _)| id.clone())
        .collect();
    let completed = rows.iter().filter(|(_, state)| state == "completed").count();
    match (inflight.len(), completed) {
        (1, _) => Ok(InflightImport::Run(inflight[0].clone())),
        (0, 0) => Ok(InflightImport::EmptyShell),
        (0, _) => Err(format!(
            "target runs.db {} has {completed} completed run(s) but journal has no import bookmark; ambiguous, refusing",
            db_path.display()
        )),
        (n, _) => Err(format!(
            "target runs.db {} has {n} in-flight runs; ambiguous, refusing",
            db_path.display()
        )),
    }
}

/// import_verify 相核心（全只读，设计 §6.3）：收据 + 计数 + tip。
/// 返回可复述 detail（同时是 selfcheck 相 expectations 的载体）。
fn verify_import_core(target: &Path) -> Result<serde_json::Value, String> {
    let receipt_path = target
        .join(RECEIPT_DIR_RELPATH)
        .join("migration-receipt.json");
    let raw = std::fs::read_to_string(&receipt_path)
        .map_err(|e| format!("read receipt {}: {e}", receipt_path.display()))?;
    let receipt: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| format!("parse receipt {}: {e}", receipt_path.display()))?;
    let status = receipt
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("<missing>");
    if status != "complete" {
        return Err(format!(
            "migration receipt status is {status:?}, expected \"complete\""
        ));
    }
    let issues = receipt
        .pointer("/issues/count")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "receipt issues.count missing".to_string())?;
    let run_id = receipt
        .get("runId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let (sessions, model_call_usage, cron_jobs, user_version) = probe_target_counts(target)?;
    let tip = migrations::MIGRATIONS.len() as u64;
    if user_version != tip {
        return Err(format!(
            "target runs.db user_version {user_version} != schema tip {tip}"
        ));
    }
    Ok(serde_json::json!({
        "runId": run_id,
        "issues": issues,
        "sessions": sessions,
        "modelCallUsage": model_call_usage,
        "cronJobs": cron_jobs,
        "userVersion": user_version,
    }))
}

/// epoch verify 闭包：重跑 import_verify 只读核心（receipt + 计数 + tip）。
fn epoch_verify_closure(target: &Path) -> Result<(), String> {
    verify_import_core(target).map(|_| ())
}

/// 续跑重验（设计 §3.3）：按进入前置逐相检查，首个失效相 = rewind 目标。
/// 返回（可能 rewind 过的）journal。
fn reverify_prerequisites(
    mut journal: CutoverJournal,
    args: &MigrateArgs,
) -> Result<CutoverJournal, VerbFailure> {
    let current = match journal.current_phase_index() {
        Some(idx) => idx,
        None => return Ok(journal), // 终态由调用方先行处理
    };
    // 有序前置检查：(相位 idx, 失效原因)。首个失效即 rewind 并停止。
    let mut failed: Option<(usize, String)> = None;
    for idx in 1..=current {
        let reason = match idx {
            // 进入 stop_write_probe：preflight 报告在盘且 passed。
            1 => {
                let report_path = args.work_dir.join(preflight::PREFLIGHT_REPORT_FILE);
                match std::fs::read_to_string(&report_path) {
                    Ok(raw) => match serde_json::from_str::<serde_json::Value>(&raw) {
                        Ok(v) if v.get("outcome").and_then(|o| o.as_str()) == Some("passed") => {
                            None
                        }
                        _ => Some("preflight report missing passed outcome".to_string()),
                    },
                    Err(e) => Some(format!("preflight report unreadable: {e}")),
                }
            }
            // 进入 backup：静止点声明在盘且与 journal 登记的源清单哈希一致。
            3 => {
                let path = stillpoint::stillpoint_path(&args.work_dir);
                match std::fs::read_to_string(&path) {
                    Ok(raw) => match serde_json::from_str::<StillpointManifestFile>(&raw) {
                        Ok(file) => match &journal.inputs.source_manifest_hash {
                            Some(hash) if *hash == file.pass_a_hash => None,
                            Some(_) => {
                                Some("stillpoint hash diverges from journal record".to_string())
                            }
                            None => Some("journal lacks sourceManifestHash".to_string()),
                        },
                        Err(e) => Some(format!("stillpoint manifest corrupt: {e}")),
                    },
                    Err(e) => Some(format!("stillpoint manifest unreadable: {e}")),
                }
            }
            // 进入 import：备份 manifest 在盘。
            4 => {
                let manifest_path = backup::backup_root_for(&args.backup_dir, &journal.cutover_id)
                    .join(backup::BACKUP_MANIFEST_FILE);
                if manifest_path.is_file() {
                    None
                } else {
                    Some(format!(
                        "backup manifest missing at {}",
                        manifest_path.display()
                    ))
                }
            }
            // 进入 import_verify：import 书签 completed。
            5 => match &journal.import {
                Some(bookmark) if bookmark.status == "completed" => None,
                _ => Some("import bookmark not completed".to_string()),
            },
            // 进入 epoch_adopt：迁移收据在盘（import_verify 为纯只读可重跑）。
            6 => {
                let receipt = args
                    .target
                    .join(RECEIPT_DIR_RELPATH)
                    .join("migration-receipt.json");
                if receipt.is_file() {
                    None
                } else {
                    Some("migration receipt missing".to_string())
                }
            }
            // 进入 pointer_switch：目标根 stamp 已 committed {2,2}。
            7 => match epoch::read_stamp(&args.target) {
                StampRead::Ok(stamp)
                    if stamp.committed_data_epoch == 2 && stamp.minimum_reader_epoch == 2 =>
                {
                    None
                }
                StampRead::Ok(stamp) => Some(format!(
                    "target stamp not committed at 2 (minimum={}, committed={})",
                    stamp.minimum_reader_epoch, stamp.committed_data_epoch
                )),
                StampRead::Missing => Some("target stamp missing".to_string()),
                StampRead::Corrupt { detail } => Some(format!("target stamp corrupt: {detail}")),
            },
            // 进入 selfcheck：指针在盘且指向 target。
            8 => match pointer::read_pointer(&args.pointer) {
                Ok(Some(p)) => {
                    let canonical_target =
                        std::fs::canonicalize(&args.target).unwrap_or_else(|_| args.target.clone());
                    if PathBuf::from(&p.active_data_root) == canonical_target {
                        None
                    } else {
                        Some("pointer does not name the target root".to_string())
                    }
                }
                Ok(None) => Some("pointer file missing".to_string()),
                Err(e) => Some(format!("pointer unreadable: {e}")),
            },
            _ => None,
        };
        if let Some(reason) = reason {
            failed = Some((idx, reason));
            break;
        }
    }
    if let Some((idx, reason)) = failed {
        // 相位 idx 的进入前置失效 = 其产物相（idx-1）必须重进修复。
        // rewind 只向后/原地；同相重进由 advance 承担。
        let phase = journal::CUTOVER_PHASES[idx - 1];
        journal = journal
            .rewind_to(&args.work_dir, phase, &reason)
            .map_err(journal_fail)?;
    }
    Ok(journal)
}

/// 相完成后处理 --stop-after-phase：停在相边界（journal 记 stopped）。
fn maybe_stop(
    journal: CutoverJournal,
    args: &MigrateArgs,
    completed_phase: &str,
) -> Result<Option<MigrateOutcome>, VerbFailure> {
    if args.stop_after_phase.as_deref() == Some(completed_phase) {
        let journal = journal.mark_stopped(&args.work_dir).map_err(journal_fail)?;
        return Ok(Some(MigrateOutcome {
            cutover_id: journal.cutover_id.clone(),
            phase: journal.phase.clone(),
            outcome: "stopped",
            detail: format!("clean stop after phase {completed_phase} (--stop-after-phase)"),
        }));
    }
    Ok(None)
}

macro_rules! refuse {
    ($journal:expr, $args:expr, $check:expr, $reason:expr, $report:expr) => {{
        $journal
            .refuse(&$args.work_dir, $check, &$reason, $report)
            .map_err(journal_fail)?;
        return Err(verb_err(
            2,
            format!("error: migrate: refused [{}]: {}", $check, $reason),
        ));
    }};
}

/// 十相驱动主循环。
pub async fn run_migrate(args: &MigrateArgs) -> Result<MigrateOutcome, VerbFailure> {
    // ── 0. journal 开门（create / resume / 幂等空跑）────────────────────
    let mut journal = match journal::load(&args.work_dir).map_err(journal_fail)? {
        None => {
            if args.resume.is_some() {
                return Err(verb_err(
                    2,
                    format!(
                        "error: migrate: --resume given but no journal at {}",
                        journal::journal_path(&args.work_dir).display()
                    ),
                ));
            }
            let cutover_id = new_cutover_id();
            let inputs = CutoverInputs {
                source: args.source.display().to_string(),
                target: args.target.display().to_string(),
                work_dir: args.work_dir.display().to_string(),
                backup_dir: args.backup_dir.display().to_string(),
                pointer: args.pointer.display().to_string(),
                source_manifest_hash: None,
            };
            journal::CutoverJournal::create(&args.work_dir, &cutover_id, inputs)
                .map_err(journal_fail)?
        }
        Some(existing) => {
            let resume_id = match &args.resume {
                Some(id) => id.clone(),
                None => {
                    return Err(verb_err(
                        2,
                        format!(
                            "error: migrate: journal {} already exists (cutoverId {:?}, phase {:?}); pass --resume <cutoverId> to continue (double-run guard)",
                            journal::journal_path(&args.work_dir).display(),
                            existing.cutover_id,
                            existing.phase
                        ),
                    ));
                }
            };
            let j = existing
                .reopen_for_resume(&args.work_dir, &resume_id)
                .map_err(journal_fail)?;
            if j.phase == "committed" {
                return Ok(MigrateOutcome {
                    cutover_id: j.cutover_id.clone(),
                    phase: j.phase.clone(),
                    outcome: "already-committed",
                    detail: "cutover already committed; idempotent no-op".to_string(),
                });
            }
            if j.phase == journal::PHASE_REFUSED {
                return Err(verb_err(
                    2,
                    format!(
                        "error: migrate: cutover {:?} is terminally refused ({}); start a fresh work-dir",
                        j.cutover_id,
                        j.refusal
                            .as_ref()
                            .map(|r| format!("{}: {}", r.check, r.reason))
                            .unwrap_or_default()
                    ),
                ));
            }
            reverify_prerequisites(j, args)?
        }
    };

    // ── 主循环：按 journal.phase 逐相执行 ────────────────────────────────
    // A09 测试注入钩子（文档化、缺省为零=生产无行为变化）：每相执行前睡眠，
    // 扩宽真实 kill -9 的定时窗口（设计 §3.4「定时窗口」的确定性化）。
    let phase_delay_ms: u64 = std::env::var("LINGXI_CUTOVER_DELAY_BEFORE_PHASE_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    loop {
        if phase_delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(phase_delay_ms));
        }
        match journal.phase.as_str() {
            // ── 1. preflight ────────────────────────────────────────────
            "preflight" => {
                let source_manifest = stillpoint::scan_pass(&args.source).map_err(|e| {
                    verb_err(2, format!("error: migrate: source manifest scan: {e}"))
                })?;
                let source_hash = manifest::source_root_sha256(&source_manifest);
                journal = journal
                    .record_source_manifest_hash(&args.work_dir, source_hash.clone())
                    .map_err(journal_fail)?;
                let inputs = PreflightInputs {
                    source: &args.source,
                    target: &args.target,
                    work_dir: &args.work_dir,
                    backup_dir: &args.backup_dir,
                    source_manifest: &source_manifest,
                    min_avail_bytes_override: args.min_avail_bytes_override,
                };
                match preflight::run_preflight(&inputs) {
                    Ok(report) => {
                        journal = journal
                            .advance(
                                &args.work_dir,
                                "stop_write_probe",
                                Some(serde_json::json!({
                                    "report": args.work_dir
                                        .join(preflight::PREFLIGHT_REPORT_FILE)
                                        .display().to_string(),
                                    "outcome": report.outcome,
                                    "sourceManifestHash": source_hash,
                                })),
                            )
                            .map_err(journal_fail)?;
                    }
                    Err(preflight::PreflightError::Refused { report_path }) => {
                        let report: serde_json::Value = serde_json::from_str(
                            &std::fs::read_to_string(&report_path).unwrap_or_default(),
                        )
                        .unwrap_or_default();
                        let check = report
                            .get("checks")
                            .and_then(|c| c.as_array())
                            .and_then(|arr| {
                                arr.iter().find(|c| {
                                    c.get("status").and_then(|s| s.as_str()) == Some("refused")
                                })
                            })
                            .and_then(|c| c.get("id"))
                            .and_then(|i| i.as_str())
                            .unwrap_or("preflight")
                            .to_string();
                        let reason = format!(
                            "preflight refused (check={check}); report at {}",
                            report_path.display()
                        );
                        refuse!(
                            journal,
                            args,
                            check.as_str(),
                            reason,
                            Some(report_path.display().to_string())
                        );
                    }
                    Err(e) => {
                        return Err(verb_err(2, format!("error: migrate: preflight io: {e}")))
                    }
                }
            }
            // ── 2. stop_write_probe（§5.1/§5.2 双探针，纯探针可重跑）─────
            "stop_write_probe" => {
                let legacy = probe::probe_legacy_server_info(&args.source)
                    .map_err(|e| verb_err(2, format!("error: migrate: legacy probe: {e}")))?;
                let rust_lock = probe::probe_rust_instance_lock(&args.source)
                    .map_err(|e| verb_err(2, format!("error: migrate: rust lock probe: {e}")))?;
                let evidence = serde_json::json!({
                    "legacyServerInfo": legacy,
                    "rustInstanceLock": rust_lock,
                });
                if legacy.probe.is_blocking() {
                    let reason = format!(
                        "old instance still alive ({:?}): {}",
                        legacy.probe, legacy.detail
                    );
                    refuse!(journal, args, "stop-write-probe", reason, None);
                }
                if rust_lock.probe.is_blocking() {
                    let reason = format!("rust instance lock held at source: {}", rust_lock.detail);
                    refuse!(journal, args, "stop-write-probe", reason, None);
                }
                journal = journal
                    .advance(&args.work_dir, "quiesce_snapshot", Some(evidence))
                    .map_err(journal_fail)?;
            }
            // ── 3. quiesce_snapshot（静止点双遍声明，§5.3）──────────────
            "quiesce_snapshot" => match stillpoint::establish(&args.source, &args.work_dir) {
                Ok(file) => {
                    journal = journal
                        .advance(
                            &args.work_dir,
                            "backup",
                            Some(serde_json::json!({
                                "stillpoint": stillpoint::stillpoint_path(&args.work_dir)
                                    .display().to_string(),
                                "passAHash": file.pass_a_hash,
                                "stores": file.stores.len(),
                            })),
                        )
                        .map_err(journal_fail)?;
                }
                Err(e) => {
                    let reason = format!("stillness window violated or registry mismatch: {e}");
                    refuse!(journal, args, "quiesce-snapshot", reason, None);
                }
            },
            // ── 4. backup（§5.4；备份后第三遍对拍）───────────────────────
            "backup" => {
                let stillpoint_file: StillpointManifestFile = (|| {
                    let raw = std::fs::read_to_string(stillpoint::stillpoint_path(&args.work_dir))?;
                    serde_json::from_str(&raw)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
                })()
                .map_err(|e| verb_err(2, format!("error: migrate: reload stillpoint: {e}")))?;
                let source_manifest = stillpoint::scan_pass(&args.source)
                    .map_err(|e| verb_err(2, format!("error: migrate: source rescan: {e}")))?;
                match backup::run_backup(
                    &args.source,
                    &args.backup_dir,
                    &args.work_dir,
                    &journal.cutover_id,
                    &stillpoint_file.stores,
                    &source_manifest,
                ) {
                    Ok(report) => {
                        if let Err(e) =
                            stillpoint::verify_post_backup(&args.source, &stillpoint_file)
                        {
                            let reason = format!("source drifted during backup window: {e}");
                            refuse!(journal, args, "backup", reason, None);
                        }
                        journal = journal
                            .advance(
                                &args.work_dir,
                                "import",
                                Some(serde_json::json!({
                                    "backupRoot": report.backup_root,
                                    "files": report.files.len(),
                                })),
                            )
                            .map_err(journal_fail)?;
                    }
                    Err(e) => {
                        let reason = format!("backup failed: {e}");
                        refuse!(journal, args, "backup", reason, None);
                    }
                }
            }
            // ── 5. import（T04 链；目标根锁在相作用域内持有）─────────────
            "import" => {
                let _target_lock = legacy_import_runner::acquire_target_lock(&args.target)
                    .map_err(|(code, line)| verb_err(code, line))?;
                // A09 缺口缝合：kill -9 可能落在「书签落盘之后、advance 之前」——
                // 书签 completed 而相位仍 import。幂等放行，绝不重跑导入。
                if let Some(b) = journal.import.as_ref().filter(|b| b.status == "completed") {
                    let run_id = b.run_id.clone();
                    journal = journal
                        .advance(
                            &args.work_dir,
                            "import_verify",
                            Some(serde_json::json!({
                                "runId": run_id,
                                "resumed": "bookmark-completed",
                            })),
                        )
                        .map_err(journal_fail)?;
                    continue;
                }
                let mut empty_shell_removed: Option<String> = None;
                // kill -9 落在相内时 journal 无书签（书签在 run 结束后才落盘）。
                // 仅在续跑（attempts>1）时从目标 runs.db 发现在途 run；首次尝试
                // 保持 resume=None，T04 门（ResumeRequired）继续守既有库。
                let resume_run_id = match &journal.import {
                    Some(b) => Some(b.run_id.clone()),
                    None if journal.attempts > 1 => {
                        match discover_inflight_import(&args.target)
                            .map_err(|e| verb_err(2, format!("error: migrate: import recovery: {e}")))?
                        {
                            InflightImport::Run(id) => Some(id),
                            InflightImport::EmptyShell => {
                                // 可证空壳（零用户表/零 run 行）：删除 runs.db
                                // 三件套后全新导入；删除动作进 advance 细节。
                                let db_path = args.target.join(TARGET_DB_RELPATH);
                                for suffix in ["", "-wal", "-shm"] {
                                    let p = PathBuf::from(format!("{}{}", db_path.display(), suffix));
                                    if p.exists() {
                                        std::fs::remove_file(&p).map_err(|e| {
                                            verb_err(
                                                2,
                                                format!("error: migrate: remove empty runs.db shell {}: {e}", p.display()),
                                            )
                                        })?;
                                    }
                                }
                                empty_shell_removed = Some(db_path.display().to_string());
                                None
                            }
                            InflightImport::None => None,
                        }
                    }
                    None => None,
                };
                let import_args = ImportArgs {
                    source: args.source.clone(),
                    target: args.target.clone(),
                    resume: resume_run_id,
                    stop_after_batch: None,
                    instance_lock_held: true,
                };
                let outcome = match legacy_import_runner::run_import(import_args).await {
                    Ok(outcome) => outcome,
                    Err(failure) => {
                        let reason = failure.line.clone();
                        refuse!(journal, args, "import", reason, None);
                    }
                };
                if outcome.interrupted {
                    journal = journal
                        .record_import(
                            &args.work_dir,
                            ImportBookmark {
                                run_id: outcome.run_id.clone(),
                                status: "interrupted".to_string(),
                            },
                        )
                        .map_err(journal_fail)?;
                    journal = journal.mark_stopped(&args.work_dir).map_err(journal_fail)?;
                    return Ok(MigrateOutcome {
                        cutover_id: journal.cutover_id.clone(),
                        phase: journal.phase.clone(),
                        outcome: "stopped",
                        detail: format!(
                            "import interrupted at batch boundary (runId {}); resume via --resume",
                            outcome.run_id
                        ),
                    });
                }
                if outcome.status != "complete" {
                    let reason = format!(
                        "import run {} finished with status {:?} (issues {})",
                        outcome.run_id, outcome.status, outcome.issues
                    );
                    refuse!(journal, args, "import", reason, None);
                }
                journal = journal
                    .record_import(
                        &args.work_dir,
                        ImportBookmark {
                            run_id: outcome.run_id.clone(),
                            status: "completed".to_string(),
                        },
                    )
                    .map_err(journal_fail)?;
                journal = journal
                    .advance(
                        &args.work_dir,
                        "import_verify",
                        Some(serde_json::json!({
                            "runId": outcome.run_id,
                            "issues": outcome.issues,
                            "batchesDone": outcome.batches_done,
                            "receipt": outcome.receipt_path.display().to_string(),
                            "emptyShellRemoved": empty_shell_removed,
                        })),
                    )
                    .map_err(journal_fail)?;
            }
            // ── 6. import_verify（全只读，§6.3）─────────────────────────
            "import_verify" => match verify_import_core(&args.target) {
                Ok(detail) => {
                    journal = journal
                        .advance(&args.work_dir, "epoch_adopt", Some(detail))
                        .map_err(journal_fail)?;
                }
                Err(reason) => {
                    refuse!(journal, args, "import-verify", reason, None);
                }
            },
            // ── 7. epoch_adopt（§7.3 注册迁移路径 1→2）───────────────────
            "epoch_adopt" => {
                let evidence = match build_adoption_evidence(&journal, args) {
                    Ok(ev) => ev,
                    Err(reason) => {
                        refuse!(journal, args, "epoch-adopt", reason, None);
                    }
                };
                let target = args.target.clone();
                let result = tokio::task::spawn_blocking(move || {
                    epoch::run_epoch_transition(
                        &target,
                        epoch::MIGRATION_R08_LEGACY_CUTOVER.id,
                        &evidence,
                        &epoch_verify_closure,
                        now_unix_ms(),
                    )
                })
                .await
                .map_err(|e| verb_err(2, format!("error: migrate: epoch task join: {e}")))?;
                match result {
                    Ok(()) => {
                        let cutover_id = journal.cutover_id.clone();
                        journal = journal
                            .record_adopt(
                                &args.work_dir,
                                AdoptBookmark {
                                    transition_id: cutover_id.clone(),
                                    phase: "committed".to_string(),
                                },
                            )
                            .map_err(journal_fail)?;
                        journal = journal
                            .advance(
                                &args.work_dir,
                                "pointer_switch",
                                Some(serde_json::json!({
                                    "transitionId": cutover_id,
                                    "migrationId": epoch::MIGRATION_R08_LEGACY_CUTOVER.id,
                                })),
                            )
                            .map_err(journal_fail)?;
                    }
                    Err(reason) => {
                        refuse!(journal, args, "epoch-adopt", reason, None);
                    }
                }
            }
            // ── 8. pointer_switch（§8 rename-atomic + 读回校验）─────────
            "pointer_switch" => {
                match epoch::read_stamp(&args.target) {
                    StampRead::Ok(stamp)
                        if stamp.committed_data_epoch == 2 && stamp.minimum_reader_epoch == 2 => {}
                    other => {
                        let reason = format!(
                            "target stamp not committed at epoch 2 before pointer switch: {other:?}"
                        );
                        refuse!(journal, args, "pointer-switch", reason, None);
                    }
                }
                let canonical_target = std::fs::canonicalize(&args.target).map_err(|e| {
                    verb_err(
                        2,
                        format!(
                            "error: migrate: canonicalize target {}: {e}",
                            args.target.display()
                        ),
                    )
                })?;
                match pointer::commit_pointer(&args.pointer, &canonical_target, &journal.cutover_id)
                {
                    Ok(committed) => {
                        journal = journal
                            .advance(
                                &args.work_dir,
                                "selfcheck",
                                Some(serde_json::json!({
                                    "pointer": args.pointer.display().to_string(),
                                    "activeDataRoot": committed.active_data_root,
                                    "committedAt": committed.committed_at,
                                })),
                            )
                            .map_err(journal_fail)?;
                    }
                    Err(e) => {
                        let reason = format!("pointer commit failed: {e}");
                        refuse!(journal, args, "pointer-switch", reason, None);
                    }
                }
            }
            // ── 9. selfcheck（§9 子进程模式起服务 + 只读探针 + SIGTERM）──
            "selfcheck" => {
                let expectations = match import_verify_expectations(&journal) {
                    Ok(exp) => exp,
                    Err(reason) => {
                        refuse!(journal, args, "selfcheck", reason, None);
                    }
                };
                let binary = std::env::current_exe()
                    .map_err(|e| verb_err(2, format!("error: migrate: current_exe: {e}")))?;
                match selfcheck::run_self_check(
                    &binary,
                    &args.target,
                    &expectations,
                    Duration::from_secs(60),
                    Duration::from_secs(30),
                )
                .await
                {
                    Ok(report) => {
                        journal = journal
                            .advance(
                                &args.work_dir,
                                "committed",
                                Some(serde_json::json!({
                                    "addr": report.addr,
                                    "counts": {
                                        "sessions": report.counts.sessions,
                                        "modelCallUsage": report.counts.model_call_usage,
                                        "cronJobs": report.counts.cron_jobs,
                                        "userVersion": report.counts.user_version,
                                    },
                                    "lockReleased": report.lock_released,
                                })),
                            )
                            .map_err(journal_fail)?;
                    }
                    Err(e) => {
                        let reason = format!("self-check failed: {e}");
                        refuse!(journal, args, "selfcheck", reason, None);
                    }
                }
            }
            // ── 10. committed（§10 裁决记录落盘 + 终态）──────────────────
            "committed" => {
                let ruling_path = write_single_writer_ruling(&journal, args)?;
                return Ok(MigrateOutcome {
                    cutover_id: journal.cutover_id.clone(),
                    phase: "committed".to_string(),
                    outcome: "committed",
                    detail: format!("cutover committed; ruling at {}", ruling_path.display()),
                });
            }
            other => {
                return Err(verb_err(
                    2,
                    format!("error: migrate: unexpected journal phase {other:?}"),
                ))
            }
        }
        // 相边界干净停判定（在换相完成之后）。
        let completed = journal::CUTOVER_PHASES
            [journal.current_phase_index().unwrap_or(1).saturating_sub(1)]
        .to_string();
        if let Some(outcome) = maybe_stop(journal.clone(), args, &completed)? {
            return Ok(outcome);
        }
    }
}

/// epoch_adopt 证据装配（§7.3：全部字段非空、store ids 唯一）。
fn build_adoption_evidence(
    journal: &CutoverJournal,
    args: &MigrateArgs,
) -> Result<EpochAdoptionEvidence, String> {
    let stillpoint_raw = std::fs::read(&stillpoint::stillpoint_path(&args.work_dir))
        .map_err(|e| format!("read stillpoint manifest: {e}"))?;
    let stillpoint_file: StillpointManifestFile = serde_json::from_slice(&stillpoint_raw)
        .map_err(|e| format!("parse stillpoint manifest: {e}"))?;
    let affected_store_ids: Vec<String> = stillpoint_file
        .stores
        .iter()
        .map(|s| s.class_id.clone())
        .collect();
    let stillpoint_manifest_sha256 = sha256_hex(&stillpoint_raw);

    let backup_manifest_path = backup::backup_root_for(&args.backup_dir, &journal.cutover_id)
        .join(backup::BACKUP_MANIFEST_FILE);
    let backup_raw = std::fs::read(&backup_manifest_path).map_err(|e| {
        format!(
            "read backup manifest {}: {e}",
            backup_manifest_path.display()
        )
    })?;
    let backup_manifest_sha256 = sha256_hex(&backup_raw);
    let mut checkpoint_receipt: serde_json::Value =
        serde_json::from_slice(&backup_raw).map_err(|e| format!("parse backup manifest: {e}"))?;
    let checkpoint_id = format!("r08-cutover-{}", journal.cutover_id);
    checkpoint_receipt["id"] = serde_json::Value::String(checkpoint_id.clone());

    let import_run_id = journal
        .import
        .as_ref()
        .filter(|b| b.status == "completed")
        .map(|b| b.run_id.clone())
        .ok_or_else(|| "import bookmark missing or not completed".to_string())?;
    let source_identity_sha256 = journal
        .inputs
        .source_manifest_hash
        .clone()
        .ok_or_else(|| "journal lacks sourceManifestHash".to_string())?;

    Ok(EpochAdoptionEvidence {
        cutover_id: journal.cutover_id.clone(),
        affected_store_ids,
        checkpoint_id,
        checkpoint_receipt,
        import_run_id,
        backup_manifest_id: backup_manifest_sha256,
        source_identity_sha256,
        stillpoint_manifest_sha256,
        own_version: lingxi_service::server_version().to_string(),
    })
}

/// selfcheck expectations：journal 历史中 import_verify 相的计数快照。
fn import_verify_expectations(journal: &CutoverJournal) -> Result<SelfCheckExpectations, String> {
    let detail = journal
        .history
        .iter()
        .rev()
        .find(|h| h.phase == "import_verify" && h.outcome == "ok")
        .and_then(|h| h.detail.clone())
        .ok_or_else(|| "no completed import_verify history entry with counts".to_string())?;
    let get = |key: &str| -> Result<u64, String> {
        detail
            .get(key)
            .and_then(|v| v.as_u64())
            .ok_or_else(|| format!("import_verify detail lacks {key}"))
    };
    Ok(SelfCheckExpectations {
        sessions: get("sessions")?,
        model_call_usage: get("modelCallUsage")?,
        cron_jobs: get("cronJobs")?,
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// §10 单写者裁决记录：work-dir 主件 + target 收据目录副本。
/// 证据取自 journal 历史（stop_write_probe 探针四态 + 各相出口）。
fn write_single_writer_ruling(
    journal: &CutoverJournal,
    args: &MigrateArgs,
) -> Result<PathBuf, VerbFailure> {
    let probe_evidence = journal
        .history
        .iter()
        .rev()
        .find(|h| h.phase == "stop_write_probe" && h.outcome == "ok")
        .and_then(|h| h.detail.clone())
        .unwrap_or_else(
            || serde_json::json!({"note": "probe evidence not found in journal history"}),
        );
    let ruling = serde_json::json!({
        "schemaVersion": 1,
        "cutoverId": journal.cutover_id,
        "decidedAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "strategy": "separated-roots",
        "sharedWrites": "forbidden",
        "enforcement": [
            "path-binding: 旧二进制继续写旧根（其启动参数绑定），新栈只服务 pointer 指名的 target；两根文件系统级分离，无共享文件",
            "epoch-barrier: stamp {minimumReaderEpoch:2, committedDataEpoch:2} 使任何 epoch-1 内核（含旧二进制）打开新根即拒启；旧根永远保持 epoch 1，新栈 epoch-2 门拒绝服务未采用根",
            "os-instance-lock: 导入、自检、正式服务共用同一把目标根 OS 实例锁（instance.rs），同一目标根同时只有一个写者",
        ],
        "probes": probe_evidence,
        "residualRisk": "旧程序继续运行时写旧根——两根分离后旧根的后续写入不再进入新根；该域属 T06/运营域处置，T05 不消费、不裁决。",
    });
    let mut body = serde_json::to_string_pretty(&ruling)
        .map_err(|e| verb_err(2, format!("error: migrate: serialize ruling: {e}")))?;
    body.push('\n');
    let work_dir_path = args.work_dir.join(RULING_FILE_NAME);
    lingxi_service::paths::atomic_write_private(&work_dir_path, body.as_bytes())
        .map_err(|e| verb_err(2, format!("error: migrate: write ruling: {e}")))?;
    // 副本入 target 收据目录（设计 §10）。
    let target_copy = args.target.join(RECEIPT_DIR_RELPATH).join(RULING_FILE_NAME);
    if let Some(parent) = target_copy.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| verb_err(2, format!("error: migrate: ruling target dir: {e}")))?;
    }
    lingxi_service::paths::atomic_write_private(&target_copy, body.as_bytes())
        .map_err(|e| verb_err(2, format!("error: migrate: write ruling copy: {e}")))?;
    Ok(work_dir_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "t05-orch-test-{}-{}-{}",
            tag,
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_args(base: &Path) -> MigrateArgs {
        MigrateArgs {
            source: base.join("source"),
            target: base.join("target"),
            work_dir: base.join("work"),
            backup_dir: base.join("backup"),
            pointer: base.join("pointer.json"),
            resume: None,
            stop_after_phase: None,
            min_avail_bytes_override: None,
        }
    }

    fn write_fake_source(source: &Path) {
        std::fs::create_dir_all(source).unwrap();
        std::fs::write(
            source.join("data-epoch.json"),
            "{\"epoch\": 1, \"stampedAt\": 1760000000000}\n",
        )
        .unwrap();
    }

    #[tokio::test]
    async fn fresh_run_requires_no_resume_and_writes_journal() {
        let base = temp_dir("fresh");
        let args = make_args(&base);
        write_fake_source(&args.source);
        // 全套跑到 committed 需要真实导入/采用——此处仅验证 create 门：
        // 预检通过后 --stop-after-phase preflight 干净停。
        let args = MigrateArgs {
            stop_after_phase: Some("preflight".to_string()),
            ..make_args(&base)
        };
        write_fake_source(&args.source);
        let outcome = run_migrate(&args).await.unwrap();
        assert_eq!(outcome.outcome, "stopped");
        let journal = journal::load(&args.work_dir).unwrap().unwrap();
        assert_eq!(journal.phase, "stop_write_probe");
        assert_eq!(
            journal.history.last().unwrap().outcome,
            "stopped",
            "stop-after-phase must close the next phase entry as stopped"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[tokio::test]
    async fn double_run_guard_refuses_without_resume() {
        let base = temp_dir("guard");
        let args = MigrateArgs {
            stop_after_phase: Some("preflight".to_string()),
            ..make_args(&base)
        };
        write_fake_source(&args.source);
        let first = run_migrate(&args).await.unwrap();
        assert_eq!(first.outcome, "stopped");
        // 第二次不带 --resume：响亮拒绝。
        let second = run_migrate(&args).await.unwrap_err();
        assert_eq!(second.code, 2);
        assert!(second.line.contains("--resume"), "{}", second.line);
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[tokio::test]
    async fn resume_with_wrong_id_is_loud() {
        let base = temp_dir("wrongid");
        let args = MigrateArgs {
            stop_after_phase: Some("preflight".to_string()),
            ..make_args(&base)
        };
        write_fake_source(&args.source);
        run_migrate(&args).await.unwrap();
        let args = MigrateArgs {
            resume: Some("cut-wrong-id".to_string()),
            ..make_args(&base)
        };
        let err = run_migrate(&args).await.unwrap_err();
        assert_eq!(err.code, 2);
        assert!(err.line.contains("does not match"), "{}", err.line);
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[tokio::test]
    async fn resume_without_journal_is_loud() {
        let base = temp_dir("nojournal");
        let args = MigrateArgs {
            resume: Some("cut-anything".to_string()),
            ..make_args(&base)
        };
        let err = run_migrate(&args).await.unwrap_err();
        assert_eq!(err.code, 2);
        assert!(err.line.contains("no journal"), "{}", err.line);
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[tokio::test]
    async fn stop_after_phase_threads_each_boundary() {
        let base = temp_dir("boundaries");
        let source = base.join("source");
        write_fake_source(&source);
        // 逐相停：preflight → stop_write_probe → quiesce_snapshot → backup。
        // （backup 需要完整备份链；此处验证到 quiesce_snapshot 边界 +
        // resume 续跑到 backup 前。）
        let args = MigrateArgs {
            stop_after_phase: Some("stop_write_probe".to_string()),
            ..make_args(&base)
        };
        let outcome = run_migrate(&args).await.unwrap();
        assert_eq!(outcome.outcome, "stopped");
        let journal = journal::load(&args.work_dir).unwrap().unwrap();
        assert_eq!(journal.phase, "quiesce_snapshot");
        // resume 续跑一相。
        let args = MigrateArgs {
            resume: Some(journal.cutover_id.clone()),
            stop_after_phase: Some("quiesce_snapshot".to_string()),
            ..make_args(&base)
        };
        let outcome = run_migrate(&args).await.unwrap();
        assert_eq!(outcome.outcome, "stopped");
        let journal = journal::load(&args.work_dir).unwrap().unwrap();
        assert_eq!(journal.phase, "backup");
        assert!(journal
            .history
            .iter()
            .any(|h| h.phase == "stop_write_probe" && h.outcome == "ok"));
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn ruling_shape_is_pinned() {
        let base = temp_dir("ruling");
        let args = make_args(&base);
        std::fs::create_dir_all(&args.work_dir).unwrap();
        let inputs = CutoverInputs {
            source: args.source.display().to_string(),
            target: args.target.display().to_string(),
            work_dir: args.work_dir.display().to_string(),
            backup_dir: args.backup_dir.display().to_string(),
            pointer: args.pointer.display().to_string(),
            source_manifest_hash: Some("a".repeat(64)),
        };
        let journal = CutoverJournal::create(&args.work_dir, "cut-test-ruling", inputs).unwrap();
        let path = write_single_writer_ruling(&journal, &args).unwrap();
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["schemaVersion"], 1);
        assert_eq!(raw["strategy"], "separated-roots");
        assert_eq!(raw["sharedWrites"], "forbidden");
        assert_eq!(raw["enforcement"].as_array().unwrap().len(), 3);
        assert!(raw["residualRisk"].as_str().unwrap().contains("T06"));
        // target 副本落盘。
        assert!(args
            .target
            .join(RECEIPT_DIR_RELPATH)
            .join(RULING_FILE_NAME)
            .is_file());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn reverify_rewinds_when_stillpoint_missing() {
        let base = temp_dir("rewind");
        let args = make_args(&base);
        std::fs::create_dir_all(&args.work_dir).unwrap();
        let inputs = CutoverInputs {
            source: args.source.display().to_string(),
            target: args.target.display().to_string(),
            work_dir: args.work_dir.display().to_string(),
            backup_dir: args.backup_dir.display().to_string(),
            pointer: args.pointer.display().to_string(),
            source_manifest_hash: Some("b".repeat(64)),
        };
        let journal = CutoverJournal::create(&args.work_dir, "cut-test-rewind", inputs).unwrap();
        // 伪造 preflight 报告 passed + advance 到 backup（跳过 stillpoint 落盘）。
        std::fs::write(
            args.work_dir.join(preflight::PREFLIGHT_REPORT_FILE),
            "{\"schemaVersion\":1,\"ranAt\":\"t\",\"outcome\":\"passed\",\"checks\":[]}\n",
        )
        .unwrap();
        let journal = journal
            .advance(&args.work_dir, "stop_write_probe", None)
            .unwrap()
            .advance(&args.work_dir, "quiesce_snapshot", None)
            .unwrap()
            .advance(&args.work_dir, "backup", None)
            .unwrap();
        assert_eq!(journal.phase, "backup");
        // 静止点文件缺失 → rewind 到产物相 quiesce_snapshot 重进。
        let journal = reverify_prerequisites(journal, &args).unwrap();
        assert_eq!(journal.phase, "quiesce_snapshot");
        assert!(journal
            .history
            .iter()
            .any(|h| h.outcome == "reverify-failed"));
        std::fs::remove_dir_all(&base).unwrap();
    }
}
