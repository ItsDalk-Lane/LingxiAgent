//! R08-T04 LegacyImporter 引擎（B1：迁移/清单/计划/簿记/收据骨架）。
//!
//! 离线单写者：importer 是短命 CLI 进程，直接持有 rusqlite Connection，
//! 不走 RunDatabase 的 async queue（该队列服务常驻服务的多消费者；此处
//! 整个进程生命周期即导入事务边界）。迁移与应用检查复用同一
//! `migrations::apply_all`（收据指纹校验、append-only、DatabaseTooNew
//! fail-closed 全部保留）。
//!
//! 目标根 fail-closed 规则（Lead 绑定条件③）：
//! - target 不存在或为空 → 新建；
//! - target 非空且无 runs.db → 拒绝（不认识的内容，不覆盖）；
//! - target 有 runs.db → 必须 `--resume <runId>` 且该 run 可恢复
//!   （state=interrupted，或 completed 但仍挂 planned convert 批次的分阶段续跑），
//!   否则拒绝。绝不静默开新 run 造成双簿记。

use rusqlite::Connection;
use std::fmt;
use std::path::{Path, PathBuf};

/// R08-T05：stillpoint 枚举与协调器复用 69 类注册表（pub 导出）。
pub mod classes;
pub(crate) mod convert_sessions;
pub(crate) mod convert_sidecars;
/// R08-T05：协调器预检/静止点双遍清单复用 T04 manifest builder（pub 导出）。
pub mod manifest;
/// 切片 B：旧 model-observability sqlite 只读映射（pub：service 端点薄壳
/// 与 CLI 阶段共用同一实现）。
pub mod observability;
pub(crate) mod runtime_state;
pub(crate) mod plan;
pub(crate) mod receipts;
pub(crate) mod session_jsonl;

use manifest::{build_manifest, manifest_json, source_root_sha256, Manifest, ManifestError};
use plan::{build_plan, PlannedBatch};

/// runs.db 在目标新根内的位置（镜像服务正式布局，T05 采用时零搬迁）。
pub const TARGET_DB_RELPATH: &str = "lingxi-service/data/runs.db";
/// 收据三件套目录。
pub const RECEIPT_DIR_RELPATH: &str = "legacy-import";

#[derive(Debug)]
pub enum ImportError {
    SourceMissing { path: PathBuf },
    /// 防御纵深：源根解析后恰为真实 $HOME/.lingxi → 无条件拒绝。
    SourceIsRealLingxiHome { path: PathBuf },
    TargetNotEmptyUnknown { path: PathBuf },
    ResumeRequired { path: PathBuf },
    RunNotResumable { run_id: String, state: String },
    RunNotFound { run_id: String },
    /// 响亮拒绝：形状/版本不受支持（sidecar、manifest 等）。
    Refused { reason: String },
    Manifest(ManifestError),
    Db(String),
    Io { path: PathBuf, message: String },
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::SourceMissing { path } => {
                write!(f, "source root does not exist or is not a directory: {}", path.display())
            }
            ImportError::SourceIsRealLingxiHome { path } => write!(
                f,
                "refusing to use the real ~/.lingxi as an import source: {}",
                path.display()
            ),
            ImportError::TargetNotEmptyUnknown { path } => write!(
                f,
                "target is non-empty but holds no runs.db; refusing to touch unknown content: {}",
                path.display()
            ),
            ImportError::ResumeRequired { path } => write!(
                f,
                "target already holds an import database; pass --resume <runId> (fail-closed, no silent new run): {}",
                path.display()
            ),
            ImportError::RunNotResumable { run_id, state } => write!(
                f,
                "run {run_id} is not resumable (state={state}, no pending planned batches)"
            ),
            ImportError::RunNotFound { run_id } => write!(f, "run {run_id} not found in target"),
            ImportError::Manifest(err) => write!(f, "source manifest: {err}"),
            ImportError::Refused { reason } => write!(f, "refused: {reason}"),
            ImportError::Db(message) => write!(f, "sqlite: {message}"),
            ImportError::Io { path, message } => write!(f, "{}: {message}", path.display()),
        }
    }
}

impl std::error::Error for ImportError {}

pub struct ImportArgs {
    pub source: PathBuf,
    pub target: PathBuf,
    pub resume: Option<String>,
    /// A08 测试注入（批准设计⑤）：执行 k 个批次后在批次边界干净退出，
    /// run 态落 interrupted。None = 全量跑完。
    pub stop_after_batch: Option<u64>,
    /// 调用方已持有目标根单写者实例锁的凭证（service instance.rs）。
    /// kill -9 复核路径只在持锁时把 running 判为 interrupted。
    pub instance_lock_held: bool,
}

/// 一次 CLI 调用的结果摘要（stdout JSON 用）。
#[derive(Debug, serde::Serialize)]
pub struct EngineOutcome {
    pub run_id: String,
    pub resumed: bool,
    pub source_root_sha256: String,
    pub source_unchanged: bool,
    pub batches_total: usize,
    pub batches_done: usize,
    pub batches_pending_convert: usize,
    pub issues: usize,
    pub status: &'static str,
    /// true = --stop-after-batch 在批次边界干净停下（run 态 interrupted）。
    pub interrupted: bool,
    pub receipt_path: PathBuf,
}

/// --stop-after-batch 预算：每执行一个批次前 consume 一次；返回 true =
/// 预算耗尽，当前批次不得开始（停在前一批次边界）。
struct BatchBudget {
    remaining: Option<u64>,
    stopped: bool,
}

impl BatchBudget {
    fn new(stop_after_batch: Option<u64>) -> Self {
        Self {
            remaining: stop_after_batch,
            stopped: false,
        }
    }

    fn consume(&mut self) -> bool {
        match self.remaining {
            Some(0) => {
                self.stopped = true;
                true
            }
            Some(n) => {
                self.remaining = Some(n - 1);
                false
            }
            None => false,
        }
    }
}

pub(crate) fn now_unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn refuse_real_lingxi_home(source: &Path) -> Result<(), ImportError> {
    if let Ok(home) = std::env::var("HOME") {
        let real = Path::new(&home).join(".lingxi");
        let canonical_source = source.canonicalize().unwrap_or_else(|_| source.to_path_buf());
        let canonical_real = real.canonicalize().unwrap_or(real);
        if canonical_source == canonical_real {
            return Err(ImportError::SourceIsRealLingxiHome {
                path: source.to_path_buf(),
            });
        }
    }
    Ok(())
}

struct RunRow {
    state: String,
}

fn load_run(conn: &Connection, run_id: &str) -> Result<Option<RunRow>, ImportError> {
    let mut stmt = conn
        .prepare("SELECT state FROM import_runs WHERE run_id = ?1")
        .map_err(|e| ImportError::Db(e.to_string()))?;
    let mut rows = stmt
        .query(rusqlite::params![run_id])
        .map_err(|e| ImportError::Db(e.to_string()))?;
    match rows.next().map_err(|e| ImportError::Db(e.to_string()))? {
        Some(row) => Ok(Some(RunRow {
            state: row.get(0).map_err(|e| ImportError::Db(e.to_string()))?,
        })),
        None => Ok(None),
    }
}

fn pending_convert_batches(conn: &Connection, run_id: &str) -> Result<i64, ImportError> {
    conn.query_row(
        "SELECT COUNT(*) FROM import_batches WHERE run_id = ?1 AND state = 'planned' AND strategy = 'convert'",
        rusqlite::params![run_id],
        |row| row.get(0),
    )
    .map_err(|e| ImportError::Db(e.to_string()))
}

fn open_db(target: &Path) -> Result<(Connection, bool), ImportError> {
    let db_path = target.join(TARGET_DB_RELPATH);
    let existed = db_path.exists();
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ImportError::Io {
            path: parent.to_path_buf(),
            message: e.to_string(),
        })?;
    }
    let conn = Connection::open(&db_path).map_err(|e| ImportError::Db(e.to_string()))?;
    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;")
        .map_err(|e| ImportError::Db(e.to_string()))?;
    super::migrations::apply_all(&conn, "r08-t04-legacy-import", now_unix_ms())
        .map_err(|e| ImportError::Db(e.to_string()))?;
    Ok((conn, existed))
}

/// 「空目标」判定：service 侧在 run() 前获取实例锁会先行创建
/// `lingxi-service/{instance.lock,instance.json,instance.stale.json,tmp/}`
/// 脚手架（路径契约见 lingxi-service paths.rs/instance.rs）。只含这些
/// 自产物的目标根视同空；任何其他内容 + 无 runs.db → 未知内容响亮拒绝。
/// 目录名白名单与 TARGET_DB_RELPATH 同根（"lingxi-service"）。
fn target_holds_only_instance_scaffold(target: &Path) -> Result<bool, ImportError> {
    let read = |p: &Path| {
        p.read_dir().map_err(|e| ImportError::Io {
            path: p.to_path_buf(),
            message: e.to_string(),
        })
    };
    for entry in read(target)? {
        let entry = entry.map_err(|e| ImportError::Io {
            path: target.to_path_buf(),
            message: e.to_string(),
        })?;
        let is_runtime_dir = entry.file_name() == "lingxi-service"
            && entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if !is_runtime_dir {
            return Ok(false);
        }
    }
    let runtime = target.join("lingxi-service");
    if runtime.exists() {
        for entry in read(&runtime)? {
            let entry = entry.map_err(|e| ImportError::Io {
                path: runtime.clone(),
                message: e.to_string(),
            })?;
            let name = entry.file_name();
            let scaffold = name == "instance.lock"
                || name == "instance.json"
                || name == "instance.stale.json"
                || name == "tmp";
            if !scaffold {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// 目标根闸门：返回 (db, resume_run_id_or_none)。全部违规都是响亮拒绝。
fn gate_target(args: &ImportArgs) -> Result<(Connection, Option<String>), ImportError> {
    let target = &args.target;
    let db_path = target.join(TARGET_DB_RELPATH);
    if target.exists() && !db_path.exists() && !target_holds_only_instance_scaffold(target)? {
        return Err(ImportError::TargetNotEmptyUnknown {
            path: target.clone(),
        });
    }
    if !target.exists() {
        std::fs::create_dir_all(target).map_err(|e| ImportError::Io {
            path: target.clone(),
            message: e.to_string(),
        })?;
    }
    let (conn, db_existed) = open_db(target)?;
    if db_existed {
        let run_id = args
            .resume
            .clone()
            .ok_or_else(|| ImportError::ResumeRequired {
                path: target.clone(),
            })?;
        let row = load_run(&conn, &run_id)?
            .ok_or_else(|| ImportError::RunNotFound {
                run_id: run_id.clone(),
            })?;
        // kill -9 复核路径（批准设计⑤）：run 停在 running 但本进程持有目标根
        // 实例锁 → 不可能有活写者 → 判 interrupted 并如实记 issue。未持锁则
        // 维持响亮拒绝（无法区分活写者与 kill 遗留）。
        if row.state == "running" && args.instance_lock_held {
            conn.execute(
                "UPDATE import_runs SET state = 'interrupted' WHERE run_id = ?1",
                rusqlite::params![run_id],
            )
            .map_err(|e| ImportError::Db(e.to_string()))?;
            let seq: i64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(seq), 0) + 1 FROM import_issues WHERE run_id = ?1",
                    rusqlite::params![run_id],
                    |row| row.get(0),
                )
                .map_err(|e| ImportError::Db(e.to_string()))?;
            insert_issue(
                &conn,
                &run_id,
                seq,
                "run",
                "warning",
                "run-recovered-from-kill",
                Some(&run_id),
                "run found in state 'running' while this process holds the target instance lock; \
                 no live writer can exist; judging interrupted and resuming",
            )?;
        }
        let state = if row.state == "running" && args.instance_lock_held {
            "interrupted"
        } else {
            row.state.as_str()
        };
        let resumable = state == "interrupted"
            || (state == "completed" && pending_convert_batches(&conn, &run_id)? > 0);
        if !resumable {
            return Err(ImportError::RunNotResumable {
                run_id,
                state: row.state.clone(),
            });
        }
        Ok((conn, Some(run_id)))
    } else {
        if args.resume.is_some() {
            return Err(ImportError::RunNotFound {
                run_id: args.resume.clone().unwrap_or_default(),
            });
        }
        Ok((conn, None))
    }
}

fn insert_batch(conn: &Connection, run_id: &str, batch: &PlannedBatch) -> Result<(), ImportError> {
    conn.execute(
        "INSERT OR IGNORE INTO import_batches
         (run_id, batch_key, store_class, strategy, source_relpath, source_sha256, state)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'planned')",
        rusqlite::params![
            run_id,
            batch.batch_key,
            batch.store_class,
            batch.strategy.receipt_name(),
            batch.source_relpath,
            batch.source_sha256,
        ],
    )
    .map_err(|e| ImportError::Db(e.to_string()))?;
    Ok(())
}

pub(crate) fn insert_issue(
    conn: &Connection,
    run_id: &str,
    seq: i64,
    batch_key: &str,
    severity: &str,
    kind: &str,
    ref_id: Option<&str>,
    message: &str,
) -> Result<(), ImportError> {
    // 内容级幂等：resume 重建计划时会重新产出同一诊断；同 (kind, ref_id, message)
    // 已登记则跳过，避免同一问题随重跑翻倍。
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(
               SELECT 1 FROM import_issues
               WHERE run_id = ?1 AND kind = ?2 AND ref_id IS ?3 AND message = ?4
             )",
            rusqlite::params![run_id, kind, ref_id, message],
            |row| row.get(0),
        )
        .map_err(|e| ImportError::Db(e.to_string()))?;
    if exists {
        return Ok(());
    }
    conn.execute(
        "INSERT OR IGNORE INTO import_issues
         (run_id, seq, batch_key, severity, kind, ref_id, message)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![run_id, seq, batch_key, severity, kind, ref_id, message],
    )
    .map_err(|e| ImportError::Db(e.to_string()))?;
    Ok(())
}

/// B1 执行面：非 convert 批次（register/regenerable/out-of-scope）即刻登记
/// 为 done（它们的全部语义就是枚举+哈希+收据）；convert 批次保持 planned
/// 待 B2/B3 切片执行。逐批推进以支持 --stop-after-batch 批次边界停机。
fn execute_nonconvert_batches(
    conn: &Connection,
    run_id: &str,
    budget: &mut BatchBudget,
) -> Result<(), ImportError> {
    let pending: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT batch_key FROM import_batches
                 WHERE run_id = ?1 AND state = 'planned'
                   AND strategy IN ('register','regenerable','out-of-scope')
                 ORDER BY batch_key",
            )
            .map_err(|e| ImportError::Db(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![run_id], |row| row.get::<_, String>(0))
            .map_err(|e| ImportError::Db(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| ImportError::Db(e.to_string()))?
    };
    for batch_key in pending {
        if budget.consume() {
            return Ok(());
        }
        conn.execute(
            "UPDATE import_batches
             SET state = 'done', rows_planned = 1, rows_done = 1,
                 detail_json = json_object('sha256', source_sha256, 'note', 'registered (no conversion by design)')
             WHERE run_id = ?1 AND batch_key = ?2 AND state = 'planned'",
            rusqlite::params![run_id, batch_key],
        )
        .map_err(|e| ImportError::Db(e.to_string()))?;
    }
    conn.execute(
        "UPDATE import_batches
         SET detail_json = json_object('sha256', source_sha256, 'note', 'pending slice execution (B2/B3)')
         WHERE run_id = ?1 AND state = 'planned' AND strategy = 'convert' AND detail_json IS NULL",
        rusqlite::params![run_id],
    )
    .map_err(|e| ImportError::Db(e.to_string()))?;
    Ok(())
}

/// B2 切片 A（续）：session-sidecars 与 session-manifest-sqlite 批次。
fn execute_misc_convert_batches(
    conn: &Connection,
    run_id: &str,
    args: &ImportArgs,
    seq: &mut i64,
    budget: &mut BatchBudget,
) -> Result<(), ImportError> {
    let pending: Vec<(String, String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT batch_key, source_relpath, store_class FROM import_batches
                 WHERE run_id = ?1 AND strategy = 'convert'
                   AND store_class IN ('session-sidecars','session-manifest-sqlite')
                   AND state IN ('planned','in_progress') ORDER BY batch_key",
            )
            .map_err(|e| ImportError::Db(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![run_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| ImportError::Db(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| ImportError::Db(e.to_string()))?
    };
    for (batch_key, relpath, store_class) in pending {
        if budget.consume() {
            return Ok(());
        }
        conn.execute(
            "UPDATE import_batches SET state = 'in_progress' WHERE run_id = ?1 AND batch_key = ?2",
            rusqlite::params![run_id, batch_key],
        )
        .map_err(|e| ImportError::Db(e.to_string()))?;
        let outcome: Result<(String, i64, i64, String), ImportError> =
            if store_class == "session-sidecars" {
                convert_sidecars::convert_sidecar_file(
                    conn, run_id, &batch_key, &args.source, &relpath, seq,
                )
                .map(|stats| {
                    (
                        "done".to_string(),
                        stats.entries as i64,
                        stats.entries as i64,
                        serde_json::json!({
                            "entries": stats.entries,
                            "titlesApplied": stats.titles_applied,
                            "archivedApplied": stats.archived_applied,
                            "orphans": stats.orphans,
                            "unhandledKeys": stats.unhandled_keys,
                            "registerOnly": stats.register_only,
                        })
                        .to_string(),
                    )
                })
            } else {
                convert_sidecars::validate_manifest_file(
                    conn, run_id, &batch_key, &args.source, &relpath, seq,
                )
                .map(|maybe| match maybe {
                    Some(stats) => (
                        "done".to_string(),
                        stats.manifest_rows as i64,
                        (stats.matched + stats.mismatched) as i64,
                        serde_json::json!({
                            "manifestRows": stats.manifest_rows,
                            "matched": stats.matched,
                            "mismatched": stats.mismatched,
                            "extraInTarget": stats.extra_in_target,
                        })
                        .to_string(),
                    ),
                    // wal/shm 伴随文件：响亮地 skipped。
                    None => (
                        "skipped".to_string(),
                        0,
                        0,
                        serde_json::json!({"note": "wal/shm companion; validated via main db open"})
                            .to_string(),
                    ),
                })
            };
        match outcome {
            Ok((state, planned, done, detail)) => {
                conn.execute(
                    "UPDATE import_batches SET state = ?3, rows_planned = ?4, rows_done = ?5, detail_json = ?6
                     WHERE run_id = ?1 AND batch_key = ?2",
                    rusqlite::params![run_id, batch_key, state, planned, done, detail],
                )
                .map_err(|e| ImportError::Db(e.to_string()))?;
            }
            Err(err) => {
                *seq += 1;
                insert_issue(
                    conn,
                    run_id,
                    *seq,
                    &batch_key,
                    "error",
                    "batch-error",
                    Some(&relpath),
                    &format!("{store_class} conversion aborted for this batch: {err}"),
                )?;
                conn.execute(
                    "UPDATE import_batches SET state = 'failed' WHERE run_id = ?1 AND batch_key = ?2",
                    rusqlite::params![run_id, batch_key],
                )
                .map_err(|e| ImportError::Db(e.to_string()))?;
            }
        }
    }
    Ok(())
}

/// B2 切片 A 执行面：session-jsonl convert 批次。
/// planned + in_progress（半程 kill 遗留）都重跑——幂等由
/// (session_id,message_id) 同键同内容跳过保证。
fn execute_session_jsonl_batches(
    conn: &Connection,
    run_id: &str,
    args: &ImportArgs,
    seq: &mut i64,
    budget: &mut BatchBudget,
) -> Result<(), ImportError> {
    let pending: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT batch_key, source_relpath FROM import_batches
                 WHERE run_id = ?1 AND strategy = 'convert' AND store_class = 'session-jsonl'
                   AND state IN ('planned','in_progress') ORDER BY batch_key",
            )
            .map_err(|e| ImportError::Db(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![run_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| ImportError::Db(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| ImportError::Db(e.to_string()))?
    };
    if pending.is_empty() {
        return Ok(());
    }
    let (rt, db) = convert_sessions::open_target_store(&args.target)?;
    let mut edges: Vec<convert_sessions::ForkEdge> = Vec::new();
    for (batch_key, relpath) in pending {
        if budget.consume() {
            break;
        }
        conn.execute(
            "UPDATE import_batches SET state = 'in_progress' WHERE run_id = ?1 AND batch_key = ?2",
            rusqlite::params![run_id, batch_key],
        )
        .map_err(|e| ImportError::Db(e.to_string()))?;
        match convert_sessions::convert_session_file(
            conn,
            &rt,
            &db,
            run_id,
            &batch_key,
            &args.source,
            &relpath,
            &args.target,
            seq,
        ) {
            Ok(stats) => {
                edges.extend(stats.fork_edges.iter().cloned());
                let rows_done =
                    stats.processed + stats.corrupt_lines + u64::from(stats.corrupt_tail);
                let state = if stats.conflicts > 0 { "failed" } else { "done" };
                let detail = serde_json::json!({
                    "imported": stats.imported,
                    "alreadyPresent": stats.already_present,
                    "quarantinedUnknown": stats.quarantined_unknown,
                    "corruptLines": stats.corrupt_lines,
                    "corruptTail": stats.corrupt_tail,
                    "boundaries": stats.boundaries,
                    "conflicts": stats.conflicts,
                    "missingRunBoundary": stats.missing_run_boundary,
                    "forkEdges": stats.fork_edges.len(),
                    "blankLines": stats.physical_lines.saturating_sub(rows_done),
                })
                .to_string();
                conn.execute(
                    "UPDATE import_batches SET state = ?3, rows_planned = ?4, rows_done = ?5, detail_json = ?6
                     WHERE run_id = ?1 AND batch_key = ?2",
                    rusqlite::params![
                        run_id,
                        batch_key,
                        state,
                        stats.physical_lines as i64,
                        rows_done as i64,
                        detail
                    ],
                )
                .map_err(|e| ImportError::Db(e.to_string()))?;
            }
            Err(err) => {
                *seq += 1;
                insert_issue(
                    conn,
                    run_id,
                    *seq,
                    &batch_key,
                    "error",
                    "batch-error",
                    Some(&relpath),
                    &format!("session-jsonl conversion aborted for this batch: {err}"),
                )?;
                conn.execute(
                    "UPDATE import_batches SET state = 'failed' WHERE run_id = ?1 AND batch_key = ?2",
                    rusqlite::params![run_id, batch_key],
                )
                .map_err(|e| ImportError::Db(e.to_string()))?;
            }
        }
    }
    convert_sessions::apply_fork_edges(conn, run_id, &edges, seq)?;
    Ok(())
}

pub fn run(args: &ImportArgs) -> Result<EngineOutcome, ImportError> {
    if !args.source.is_dir() {
        return Err(ImportError::SourceMissing {
            path: args.source.clone(),
        });
    }
    refuse_real_lingxi_home(&args.source)?;

    let pre_manifest = build_manifest(&args.source).map_err(ImportError::Manifest)?;
    let root_sha = source_root_sha256(&pre_manifest);
    let plan = build_plan(&pre_manifest);

    let (conn, resume_run) = gate_target(args)?;
    let resumed = resume_run.is_some();
    let now = now_unix_ms();
    let run_id = resume_run
        .unwrap_or_else(|| format!("import:{}:{now}", &root_sha[..8.min(root_sha.len())]));

    if !resumed {
        conn.execute(
            "INSERT INTO import_runs (run_id, source_root, source_root_sha256, state, created_at_unix_ms)
             VALUES (?1, ?2, ?3, 'running', ?4)",
            rusqlite::params![
                run_id,
                args.source.to_string_lossy().as_ref(),
                root_sha,
                now
            ],
        )
        .map_err(|e| ImportError::Db(e.to_string()))?;
    } else {
        conn.execute(
            "UPDATE import_runs SET state = 'running', finished_at_unix_ms = NULL WHERE run_id = ?1",
            rusqlite::params![run_id],
        )
        .map_err(|e| ImportError::Db(e.to_string()))?;
    }

    for batch in &plan.batches {
        insert_batch(&conn, &run_id, batch)?;
    }
    // done 批次的源哈希复核（A08：源在变就必须响亮重跑该批次）。
    let stale: Vec<(String, Option<String>)> = {
        let mut stmt = conn
            .prepare(
                "SELECT batch_key, source_sha256 FROM import_batches
                 WHERE run_id = ?1 AND state = 'done'",
            )
            .map_err(|e| ImportError::Db(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![run_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .map_err(|e| ImportError::Db(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| ImportError::Db(e.to_string()))?
    };
    let current: std::collections::HashMap<&str, &str> = plan
        .batches
        .iter()
        .map(|b| (b.batch_key.as_str(), b.source_sha256.as_str()))
        .collect();
    let mut seq: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM import_issues WHERE run_id = ?1",
            rusqlite::params![run_id],
            |row| row.get(0),
        )
        .map_err(|e| ImportError::Db(e.to_string()))?;
    for (batch_key, old_sha) in stale {
        match current.get(batch_key.as_str()) {
            Some(new_sha) if old_sha.as_deref() == Some(*new_sha) => {}
            Some(new_sha) => {
                seq += 1;
                conn.execute(
                    "UPDATE import_batches SET state = 'planned', rows_done = NULL WHERE run_id = ?1 AND batch_key = ?2",
                    rusqlite::params![run_id, batch_key],
                )
                .map_err(|e| ImportError::Db(e.to_string()))?;
                insert_issue(
                    &conn,
                    &run_id,
                    seq,
                    &batch_key,
                    "warning",
                    "source-changed-between-runs",
                    None,
                    &format!(
                        "source sha256 changed between runs ({}… → {}…); batch reset to planned",
                        old_sha.unwrap_or_default().chars().take(8).collect::<String>(),
                        new_sha.chars().take(8).collect::<String>()
                    ),
                )?;
            }
            None => {
                seq += 1;
                insert_issue(
                    &conn,
                    &run_id,
                    seq,
                    &batch_key,
                    "warning",
                    "source-vanished-between-runs",
                    None,
                    "batch source file no longer present; batch left done for the earlier run",
                )?;
            }
        }
    }

    for unmapped in &plan.unmapped {
        seq += 1;
        insert_issue(
            &conn,
            &run_id,
            seq,
            "unmapped",
            "warning",
            "unmapped-class",
            Some(&unmapped.relpath),
            &format!(
                "file matches no registry class ({} bytes, sha256 {}…)",
                unmapped.size,
                unmapped.sha256.chars().take(8).collect::<String>()
            ),
        )?;
    }
    for skipped in &pre_manifest.skipped_special {
        seq += 1;
        insert_issue(
            &conn,
            &run_id,
            seq,
            "unmapped",
            "warning",
            "special-file-skipped",
            Some(&skipped.relpath),
            &format!("{} not followed/read during import", skipped.kind),
        )?;
    }

    let mut budget = BatchBudget::new(args.stop_after_batch);
    execute_nonconvert_batches(&conn, &run_id, &mut budget)?;
    execute_session_jsonl_batches(&conn, &run_id, args, &mut seq, &mut budget)?;
    execute_misc_convert_batches(&conn, &run_id, args, &mut seq, &mut budget)?;

    // 导入后源根复算（A08 原料：前后 manifest 比对）。
    let post_manifest: Manifest = build_manifest(&args.source).map_err(ImportError::Manifest)?;
    let source_unchanged = post_manifest == pre_manifest;

    let receipt_dir = args.target.join(RECEIPT_DIR_RELPATH);
    std::fs::create_dir_all(&receipt_dir).map_err(|e| ImportError::Io {
        path: receipt_dir.clone(),
        message: e.to_string(),
    })?;
    std::fs::write(
        receipt_dir.join("source-manifest.before.json"),
        serde_json::to_string_pretty(&manifest_json(&pre_manifest)).unwrap_or_default(),
    )
    .map_err(|e| ImportError::Io {
        path: receipt_dir.join("source-manifest.before.json"),
        message: e.to_string(),
    })?;
    std::fs::write(
        receipt_dir.join("source-manifest.after.json"),
        serde_json::to_string_pretty(&manifest_json(&post_manifest)).unwrap_or_default(),
    )
    .map_err(|e| ImportError::Io {
        path: receipt_dir.join("source-manifest.after.json"),
        message: e.to_string(),
    })?;

    let mut outcome = receipts::write_receipts(
        &conn,
        &receipt_dir,
        &run_id,
        &root_sha,
        source_unchanged,
        resumed,
        args.resume.as_deref(),
        now,
    )?;

    if budget.stopped {
        // A08：批次边界干净退出——run 态落 interrupted，等 --resume 续跑。
        conn.execute(
            "UPDATE import_runs SET state = 'interrupted' WHERE run_id = ?1",
            rusqlite::params![run_id],
        )
        .map_err(|e| ImportError::Db(e.to_string()))?;
        outcome.interrupted = true;
        return Ok(outcome);
    }

    conn.execute(
        "UPDATE import_runs SET state = 'completed', finished_at_unix_ms = ?2 WHERE run_id = ?1",
        rusqlite::params![run_id, now_unix_ms()],
    )
    .map_err(|e| ImportError::Db(e.to_string()))?;

    Ok(outcome)
}

// ── A07 附件实转（sessionfiles 扩展编排面）──
// 目录级 convert 批次由 service 侧 SessionFileService::import_legacy_sidecar_v2
// 执行（canonical 写路径唯一）；这里只提供「待执行目录查询」与「结果落账 +
// 收据重写」两端，绝不直写 session_files 行。

/// 待执行的 session-files 目录级批次（planned/in_progress），按 batch_key 排序。
/// 返回 (batch_key, dir_relpath)——dir_relpath 形如 "session-files/{hash}"。
pub fn pending_session_files_dirs(
    target: &Path,
    run_id: &str,
) -> Result<Vec<(String, String)>, ImportError> {
    let conn = Connection::open(target.join(TARGET_DB_RELPATH))
        .map_err(|e| ImportError::Db(e.to_string()))?;
    let mut stmt = conn
        .prepare(
            "SELECT batch_key, source_relpath FROM import_batches
             WHERE run_id = ?1 AND store_class = 'session-files' AND strategy = 'convert'
               AND state IN ('planned', 'in_progress')
             ORDER BY batch_key",
        )
        .map_err(|e| ImportError::Db(e.to_string()))?;
    let rows = stmt
        .query_map(rusqlite::params![run_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| ImportError::Db(e.to_string()))?;
    let mut out = Vec::new();
    for row in rows {
        let (batch_key, relpath) = row.map_err(|e| ImportError::Db(e.to_string()))?;
        // source_relpath 以 '/' 结尾标记目录（见 plan.rs）。
        out.push((batch_key, relpath.trim_end_matches('/').to_string()));
    }
    Ok(out)
}

/// 单个 session-files 目录的导入结果（由 service 侧 ImportLegacyReportV2 映射）。
pub struct SessionFilesDirOutcome {
    pub batch_key: String,
    pub imported: usize,
    pub skipped_existing: usize,
    pub broken: Vec<(String, String)>, // (file_id, 受控坏因)
    /// Some → 批次 failed + batch-error 问题（响亮，不吞错）。
    pub error: Option<String>,
}

/// 落账 + bad-attachment 问题清单 + 收据重写。批次必须存在（自身不变量，
/// 缺失 → Refused 响亮失败）。已 done 且明细一致的批次仅重写收据（幂等）。
pub fn finalize_session_files(
    args: &ImportArgs,
    run_id: &str,
    resumed: bool,
    outcomes: &[SessionFilesDirOutcome],
) -> Result<EngineOutcome, ImportError> {
    let conn = Connection::open(args.target.join(TARGET_DB_RELPATH))
        .map_err(|e| ImportError::Db(e.to_string()))?;
    let mut seq: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM import_issues WHERE run_id = ?1",
            rusqlite::params![run_id],
            |row| row.get(0),
        )
        .map_err(|e| ImportError::Db(e.to_string()))?;
    for outcome in outcomes {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM import_batches WHERE run_id = ?1 AND batch_key = ?2
                 )",
                rusqlite::params![run_id, outcome.batch_key],
                |row| row.get(0),
            )
            .map_err(|e| ImportError::Db(e.to_string()))?;
        if !exists {
            return Err(ImportError::Refused {
                reason: format!(
                    "session-files finalize targets unknown batch {}",
                    outcome.batch_key
                ),
            });
        }
        if let Some(error) = &outcome.error {
            seq += 1;
            insert_issue(
                &conn,
                run_id,
                seq,
                &outcome.batch_key,
                "error",
                "batch-error",
                Some(&outcome.batch_key),
                &format!("session-files payload import aborted for this batch: {error}"),
            )?;
            conn.execute(
                "UPDATE import_batches SET state = 'failed'
                 WHERE run_id = ?1 AND batch_key = ?2",
                rusqlite::params![run_id, outcome.batch_key],
            )
            .map_err(|e| ImportError::Db(e.to_string()))?;
            continue;
        }
        let broken_json = outcome
            .broken
            .iter()
            .map(|(file_id, reason)| serde_json::json!({"fileId": file_id, "reason": reason}))
            .collect::<Vec<_>>();
        let detail = serde_json::json!({
            "imported": outcome.imported,
            "skippedExisting": outcome.skipped_existing,
            "brokenCount": outcome.broken.len(),
            "broken": broken_json,
        })
        .to_string();
        // 账目口径：payload-missing/payload-truncated 的行仍被登记（status
        // missing/broken），已计入 imported；sidecar-row-invalid /
        // unsupported-version 未插行，须单独计入 planned。绝不重复计数。
        let broken_uninserted = outcome
            .broken
            .iter()
            .filter(|(_, reason)| {
                reason == "sidecar-row-invalid" || reason == "unsupported-version"
            })
            .count();
        let rows_planned =
            (outcome.imported + outcome.skipped_existing + broken_uninserted) as i64;
        let rows_done = (outcome.imported + outcome.skipped_existing) as i64;
        conn.execute(
            "UPDATE import_batches SET state = 'done', rows_planned = ?3, rows_done = ?4,
             detail_json = ?5
             WHERE run_id = ?1 AND batch_key = ?2",
            rusqlite::params![run_id, outcome.batch_key, rows_planned, rows_done, detail],
        )
        .map_err(|e| ImportError::Db(e.to_string()))?;
        for (file_id, reason) in &outcome.broken {
            seq += 1;
            // 词表内的 canonical kind；坏行进词表原因写入 message，不吞。
            insert_issue(
                &conn,
                run_id,
                seq,
                &outcome.batch_key,
                "warning",
                "bad-attachment",
                Some(file_id),
                &format!("attachment payload unusable: {reason}; source bytes left untouched"),
            )?;
        }
    }
    // 收据重写：源前后 manifest 复算（A08 原料不变）。
    let pre_manifest = build_manifest(&args.source).map_err(ImportError::Manifest)?;
    let post_manifest: Manifest = build_manifest(&args.source).map_err(ImportError::Manifest)?;
    let source_unchanged = post_manifest == pre_manifest;
    let root_sha = source_root_sha256(&pre_manifest);
    let receipt_dir = args.target.join(RECEIPT_DIR_RELPATH);
    std::fs::create_dir_all(&receipt_dir).map_err(|e| ImportError::Io {
        path: receipt_dir.clone(),
        message: e.to_string(),
    })?;
    let outcome =
        receipts::write_receipts(&conn, &receipt_dir, run_id, &root_sha, source_unchanged, resumed, if resumed { Some(run_id) } else { None }, now_unix_ms())?;
    conn.execute(
        "UPDATE import_runs SET state = 'completed', finished_at_unix_ms = ?2 WHERE run_id = ?1",
        rusqlite::params![run_id, now_unix_ms()],
    )
    .map_err(|e| ImportError::Db(e.to_string()))?;
    Ok(outcome)
}

// ── B3 切片 B/C：扩展收据（V28 import_run_extensions）──
// legacyTrace / legacyCron / runtimeState / knowledgeEvidence / legacyPlugins
// 全部是「register 批次字节收据之外」的转换/登记收据载体；幂等 upsert，收据
// 重写时按 ext_key 排序原样带出。空表 → 收据无 extensions 键（B2 形状不变）。

/// 待 trace 导入的旧 observability 主库：register done 批次里 class=
/// model-observability-db 且 relpath 以 .sqlite 结尾的主文件（-wal/-shm
/// 天然不匹配）。返回 (batch_key, source_relpath)。
pub fn pending_legacy_trace_stores(
    target: &Path,
    run_id: &str,
) -> Result<Vec<(String, String)>, ImportError> {
    let conn = Connection::open(target.join(TARGET_DB_RELPATH))
        .map_err(|e| ImportError::Db(e.to_string()))?;
    let mut stmt = conn
        .prepare(
            "SELECT batch_key, source_relpath FROM import_batches
             WHERE run_id = ?1 AND store_class = 'model-observability-db'
               AND strategy = 'register' AND state = 'done'
               AND source_relpath LIKE '%.sqlite'
             ORDER BY batch_key",
        )
        .map_err(|e| ImportError::Db(e.to_string()))?;
    let rows = stmt
        .query_map(rusqlite::params![run_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| ImportError::Db(e.to_string()))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| ImportError::Db(e.to_string()))?);
    }
    Ok(out)
}

/// 阶段用 issue 记录助手：seq 自取 max+1，词表 kind 由调用方控制。
/// 返回新 seq。响亮失败（Db 错误上抛），绝不静默吞。
pub fn record_issue(
    target: &Path,
    run_id: &str,
    batch_key: &str,
    severity: &str,
    kind: &str,
    ref_id: Option<&str>,
    message: &str,
) -> Result<(), ImportError> {
    let conn = Connection::open(target.join(TARGET_DB_RELPATH))
        .map_err(|e| ImportError::Db(e.to_string()))?;
    let seq: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM import_issues WHERE run_id = ?1",
            rusqlite::params![run_id],
            |row| row.get(0),
        )
        .map_err(|e| ImportError::Db(e.to_string()))?;
    insert_issue(&conn, run_id, seq, batch_key, severity, kind, ref_id, message)
}

/// 指定类是否存在 state=done 的 register 批次（阶段门控）。
pub fn has_register_done_batches(
    target: &Path,
    run_id: &str,
    store_class: &str,
) -> Result<bool, ImportError> {
    let conn = Connection::open(target.join(TARGET_DB_RELPATH))
        .map_err(|e| ImportError::Db(e.to_string()))?;
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM import_batches
             WHERE run_id = ?1 AND store_class = ?2 AND strategy = 'register' AND state = 'done'",
            rusqlite::params![run_id, store_class],
            |row| row.get(0),
        )
        .map_err(|e| ImportError::Db(e.to_string()))?;
    Ok(n > 0)
}

/// 幂等 upsert 一条扩展收据。
pub fn record_extension(
    target: &Path,
    run_id: &str,
    ext_key: &str,
    payload: &serde_json::Value,
) -> Result<(), ImportError> {
    let conn = Connection::open(target.join(TARGET_DB_RELPATH))
        .map_err(|e| ImportError::Db(e.to_string()))?;
    conn.execute(
        "INSERT INTO import_run_extensions (run_id, ext_key, payload_json, updated_at_unix_ms)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(run_id, ext_key) DO UPDATE SET
           payload_json = excluded.payload_json,
           updated_at_unix_ms = excluded.updated_at_unix_ms",
        rusqlite::params![run_id, ext_key, payload.to_string(), now_unix_ms()],
    )
    .map_err(|e| ImportError::Db(e.to_string()))?;
    Ok(())
}

/// 扩展落账后重写收据三件套（含 extensions），返回新引擎摘要。
/// source_unchanged 取真实比对：run 建档时锁定的 source_root_sha256
/// （import_runs 簿记）vs 当前源清单根哈希——不一致即 false，响亮反映在
/// 收据，绝不硬编码 true。
pub fn finalize_extensions(
    args: &ImportArgs,
    run_id: &str,
    resumed: bool,
) -> Result<EngineOutcome, ImportError> {
    let conn = Connection::open(args.target.join(TARGET_DB_RELPATH))
        .map_err(|e| ImportError::Db(e.to_string()))?;
    let recorded_sha: String = conn
        .query_row(
            "SELECT source_root_sha256 FROM import_runs WHERE run_id = ?1",
            rusqlite::params![run_id],
            |row| row.get(0),
        )
        .map_err(|e| ImportError::Db(e.to_string()))?;
    let post_manifest: Manifest = build_manifest(&args.source).map_err(ImportError::Manifest)?;
    let root_sha = source_root_sha256(&post_manifest);
    let source_unchanged = root_sha == recorded_sha;
    let receipt_dir = args.target.join(RECEIPT_DIR_RELPATH);
    std::fs::create_dir_all(&receipt_dir).map_err(|e| ImportError::Io {
        path: receipt_dir.clone(),
        message: e.to_string(),
    })?;
    receipts::write_receipts(
        &conn,
        &receipt_dir,
        run_id,
        &root_sha,
        source_unchanged,
        resumed,
        if resumed { Some(run_id) } else { None },
        now_unix_ms(),
    )
}

/// B3 切片 C 阶段入口（service 主进程在 async 阶段前调用，同步即可）：
/// 8 个运行态类逐字节归档 + 键登记，结果写入扩展收据 runtimeState。
/// 无可处理批次返回 None（不重写收据）。
pub fn convert_runtime_state_phase(
    args: &ImportArgs,
    run_id: &str,
    resumed: bool,
) -> Result<Option<EngineOutcome>, ImportError> {
    let conn = Connection::open(args.target.join(TARGET_DB_RELPATH))
        .map_err(|e| ImportError::Db(e.to_string()))?;
    let mut seq: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM import_issues WHERE run_id = ?1",
            rusqlite::params![run_id],
            |row| row.get(0),
        )
        .map_err(|e| ImportError::Db(e.to_string()))?;
    let Some((total, per_class)) = runtime_state::convert_runtime_state(&conn, run_id, args, &mut seq)?
    else {
        return Ok(None);
    };
    record_extension(
        &args.target,
        run_id,
        "runtimeState",
        &serde_json::json!({
            "entries": total,
            "classes": per_class,
            "archiveRoot": runtime_state::ARCHIVE_DIR,
            "label": "已登记、消费端待对应任务",
        }),
    )?;
    finalize_extensions(args, run_id, resumed).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_pair(tag: &str) -> (PathBuf, PathBuf) {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir().join(format!(
            "t04i-engine-test-{}-{}-{tag}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        let source = base.join("source");
        let target = base.join("target");
        std::fs::create_dir_all(&source).unwrap();
        (source, target)
    }

    fn seed_fixture(source: &Path) {
        std::fs::create_dir_all(source.join("agents/a1/sessions")).unwrap();
        std::fs::write(
            source.join("agents/a1/sessions/s1.jsonl"),
            b"{\"type\":\"user\"}\n",
        )
        .unwrap();
        // 真实 sqlite manifest（垃圾字节会被只读校验响亮拒绝——那是另一个测试面）。
        let manifest = Connection::open(source.join("session-manifest.db")).unwrap();
        manifest
            .execute_batch(
                "CREATE TABLE sessions (session_id TEXT PRIMARY KEY, agent_id TEXT, updated_at INTEGER);
                 INSERT INTO sessions VALUES ('s1', 'a1', 1700000000000);",
            )
            .unwrap();
        drop(manifest);
        std::fs::create_dir_all(source.join("knowledge/indexes")).unwrap();
        std::fs::write(source.join("knowledge/indexes/x.bin"), b"cache").unwrap();
        std::fs::write(source.join("mystery.bin"), b"?").unwrap();
    }

    fn args(source: &Path, target: &Path, resume: Option<String>) -> ImportArgs {
        ImportArgs {
            source: source.to_path_buf(),
            target: target.to_path_buf(),
            resume,
            stop_after_batch: None,
            instance_lock_held: false,
        }
    }

    #[test]
    fn fresh_run_plans_batches_and_writes_receipt() {
        let (source, target) = temp_pair("fresh");
        seed_fixture(&source);
        let outcome = run(&args(&source, &target, None)).unwrap();
        assert!(!outcome.resumed);
        assert_eq!(outcome.batches_total, 3);
        // register/regenerable + session-jsonl + session-manifest-sqlite 全部即刻落地。
        assert_eq!(outcome.batches_done, 3);
        assert_eq!(outcome.batches_pending_convert, 0);
        assert_eq!(outcome.status, "complete");
        assert!(outcome.source_unchanged);
        assert!(outcome.receipt_path.exists());
        assert!(target.join(TARGET_DB_RELPATH).exists());
        assert!(target
            .join(RECEIPT_DIR_RELPATH)
            .join("issues.jsonl")
            .exists());
        std::fs::remove_dir_all(source.parent().unwrap()).unwrap();
    }

    #[test]
    fn rerun_without_resume_is_refused_fail_closed() {
        let (source, target) = temp_pair("refuse");
        seed_fixture(&source);
        run(&args(&source, &target, None)).unwrap();
        let err = run(&args(&source, &target, None)).unwrap_err();
        assert!(
            matches!(err, ImportError::ResumeRequired { .. }),
            "want ResumeRequired, got {err}"
        );
        std::fs::remove_dir_all(source.parent().unwrap()).unwrap();
    }

    #[test]
    fn nonempty_target_without_db_is_refused() {
        let (source, target) = temp_pair("unknown");
        seed_fixture(&source);
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("stranger.dat"), b"not ours").unwrap();
        let err = run(&args(&source, &target, None)).unwrap_err();
        assert!(
            matches!(err, ImportError::TargetNotEmptyUnknown { .. }),
            "want TargetNotEmptyUnknown, got {err}"
        );
        std::fs::remove_dir_all(source.parent().unwrap()).unwrap();
    }

    #[test]
    fn resume_continues_same_run_without_duplicate_batches() {
        let (source, target) = temp_pair("resume");
        seed_fixture(&source);
        let first = run(&args(&source, &target, None)).unwrap();
        // 全部批次终态后 completed → 不可续跑（fail-closed）；拨回一个 planned 模拟半程。
        Connection::open(target.join(TARGET_DB_RELPATH))
            .unwrap()
            .execute(
                "UPDATE import_batches SET state='planned' WHERE batch_key='session-jsonl/agents/a1/sessions/s1.jsonl'",
                [],
            )
            .unwrap();
        let second = run(&args(&source, &target, Some(first.run_id.clone()))).unwrap();
        assert!(second.resumed);
        assert_eq!(second.run_id, first.run_id);
        assert_eq!(second.batches_total, 3, "no duplicate batch rows");
        let conn = Connection::open(target.join(TARGET_DB_RELPATH)).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM import_batches WHERE run_id = ?1",
                rusqlite::params![first.run_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 3);
        std::fs::remove_dir_all(source.parent().unwrap()).unwrap();
    }

    #[test]
    fn resume_with_unknown_run_is_refused() {
        let (source, target) = temp_pair("badresume");
        seed_fixture(&source);
        run(&args(&source, &target, None)).unwrap();
        let err = run(&args(&source, &target, Some("import:deadbeef:0".into()))).unwrap_err();
        assert!(matches!(err, ImportError::RunNotFound { .. }), "got {err}");
        std::fs::remove_dir_all(source.parent().unwrap()).unwrap();
    }

    /// A08：--stop-after-batch 在批次边界干净退出（interrupted + partial 收据），
    /// --resume 续跑完成且不重复批次；收据写 resumedFromRunId 链。
    #[test]
    fn stop_after_batch_interrupts_cleanly_then_resume_finishes() {
        let (source, target) = temp_pair("stopafter");
        seed_fixture(&source);
        let mut a = args(&source, &target, None);
        a.stop_after_batch = Some(1);
        let first = run(&a).unwrap();
        assert!(first.interrupted, "budget stop must mark interrupted");
        assert_eq!(first.batches_done, 1);
        assert_eq!(first.status, "partial");
        let conn = Connection::open(target.join(TARGET_DB_RELPATH)).unwrap();
        let state: String = conn
            .query_row(
                "SELECT state FROM import_runs WHERE run_id = ?1",
                rusqlite::params![first.run_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(state, "interrupted");
        drop(conn);
        let second = run(&args(&source, &target, Some(first.run_id.clone()))).unwrap();
        assert!(second.resumed);
        assert!(!second.interrupted);
        assert_eq!(second.status, "complete");
        assert_eq!(second.batches_done, 3, "no duplicate batch rows");
        let receipt = std::fs::read_to_string(
            target
                .join(RECEIPT_DIR_RELPATH)
                .join("migration-receipt.json"),
        )
        .unwrap();
        assert!(
            receipt.contains(&format!("\"resumedFromRunId\": \"{}\"", first.run_id)),
            "resume chain missing: {receipt}"
        );
        std::fs::remove_dir_all(source.parent().unwrap()).unwrap();
    }

    /// A08 kill -9 复核路径：run 停在 running 时，持锁（instance_lock_held）
    /// 才判 interrupted 续跑并记 issue；未持锁维持响亮拒绝。
    #[test]
    fn kill_recovery_requires_instance_lock() {
        let (source, target) = temp_pair("kill9");
        seed_fixture(&source);
        let first = run(&args(&source, &target, None)).unwrap();
        // 模拟 kill -9 遗留：run 态停在 running。
        Connection::open(target.join(TARGET_DB_RELPATH))
            .unwrap()
            .execute(
                "UPDATE import_runs SET state='running' WHERE run_id = ?1",
                rusqlite::params![first.run_id],
            )
            .unwrap();
        let err = run(&args(&source, &target, Some(first.run_id.clone()))).unwrap_err();
        assert!(
            matches!(err, ImportError::RunNotResumable { .. }),
            "no lock → refuse, got {err}"
        );
        let mut a = args(&source, &target, Some(first.run_id.clone()));
        a.instance_lock_held = true;
        let second = run(&a).unwrap();
        assert!(second.resumed);
        let conn = Connection::open(target.join(TARGET_DB_RELPATH)).unwrap();
        let recovered: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM import_issues
                 WHERE run_id = ?1 AND kind = 'run-recovered-from-kill'",
                rusqlite::params![first.run_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(recovered, 1, "recovery must be recorded as issue");
        std::fs::remove_dir_all(source.parent().unwrap()).unwrap();
    }

    /// B2 切片 A 引擎级验收：fork 对、未知块、截断尾、缺边界、幂等续跑。
    fn seed_b2_fixture(source: &Path) {
        std::fs::create_dir_all(source.join("agents/a1/sessions")).unwrap();
        std::fs::write(
            source.join("agents/a1/sessions/parent.jsonl"),
            concat!(
                "{\"entryType\":\"sessionMeta\",\"title\":\"父会话\",\"createdAt\":1700000000000}\n",
                "{\"entryType\":\"runBoundary\",\"phase\":\"start\",\"runId\":\"run-p1\",\"ts\":1700000000100}\n",
                "{\"entryType\":\"user\",\"id\":\"p-m1\",\"runId\":\"run-p1\",\"content\":\"问\",\"ts\":1700000000200}\n",
                "{\"entryType\":\"assistant\",\"id\":\"p-m2\",\"runId\":\"run-p1\",\"content\":\"答\",\"ts\":1700000000300,\"customVendorField\":{\"v\":7}}\n",
                "{\"entryType\":\"runBoundary\",\"phase\":\"end\",\"runId\":\"run-p1\",\"ts\":1700000000400}\n",
                "{\"entryType\":\"fork\",\"childSessionId\":\"child\",\"ts\":1700000000500}\n"
            ),
        )
        .unwrap();
        std::fs::write(
            source.join("agents/a1/sessions/child.jsonl"),
            concat!(
                "{\"entryType\":\"user\",\"id\":\"c-m1\",\"content\":\"子问\",\"ts\":1700000000600}\n",
                "{\"entryType\":\"assistant\",\"id\":\"c-m2\",\"content\":\"子答\",\"ts\":1700000000700}\n"
            ),
        )
        .unwrap();
        std::fs::write(
            source.join("agents/a1/sessions/blocks.jsonl"),
            concat!(
                "{\"entryType\":\"user\",\"id\":\"b-m1\",\"content\":\"hello\"}\n",
                "{\"entryType\":\"quantumEntangler\",\"id\":\"b-x1\",\"payload\":{\"spin\":\"up\"}}\n",
                "{\"entryType\":\"assistant\",\"id\":\"b-m2\",\"content\":\"world\"}\n"
            ),
        )
        .unwrap();
        let mut corrupt = b"{\"entryType\":\"user\",\"id\":\"t-m1\",\"content\":\"good\"}\n".to_vec();
        corrupt.extend_from_slice(b"{\"entryType\":\"assistant\",\"content\":\"cut of");
        std::fs::write(source.join("agents/a1/sessions/corrupt.jsonl"), corrupt).unwrap();
    }

    #[test]
    fn b2_session_jsonl_convert_end_to_end() {
        let (source, target) = temp_pair("b2e2e");
        seed_b2_fixture(&source);
        let outcome = run(&args(&source, &target, None)).unwrap();
        let conn = Connection::open(target.join(TARGET_DB_RELPATH)).unwrap();

        // 会话种子 + 谱系：child.parent = parent，depth=1，fork 点=父最后消息。
        let (parent_of, fork_point, depth): (Option<String>, Option<String>, i64) = conn
            .query_row(
                "SELECT parent_session_id, fork_point_message_id, lineage_depth FROM sessions WHERE session_id='child'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(parent_of.as_deref(), Some("parent"));
        assert_eq!(fork_point.as_deref(), Some("p-m2"));
        assert_eq!(depth, 1);

        // parent：边界物化为 legacy run，两条消息挂其下；未知字段透传。
        let run_status: String = conn
            .query_row("SELECT status FROM runs WHERE run_id='run-p1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(run_status, "completed");
        let cnt: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE session_id='parent' AND run_id='run-p1' AND entry_type='message'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cnt, 2);
        let content: String = conn
            .query_row(
                "SELECT content_json FROM messages WHERE session_id='parent' AND message_id='p-m2'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["customVendorField"]["v"], 7);
        assert_eq!(parsed["content"][0]["text"], "答");

        // child 无 runBoundary → run_id NULL + missing-run-boundary info。
        let null_runs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE session_id='child' AND run_id IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(null_runs, 2);
        let missing_info: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM import_issues WHERE kind='missing-run-boundary' AND ref_id='child'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(missing_info, 1);

        // 未知块：标记消息 + quarantine 原件 + warning。
        let markers: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE session_id='blocks' AND entry_type='legacy-unknown-block'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(markers, 1);
        let qdir = target
            .join(RECEIPT_DIR_RELPATH)
            .join("quarantine/unknown-blocks/blocks");
        let qfiles: Vec<_> = std::fs::read_dir(&qdir).unwrap().collect();
        assert_eq!(qfiles.len(), 1);
        let qbytes = std::fs::read(qfiles[0].as_ref().unwrap().path()).unwrap();
        assert!(String::from_utf8_lossy(&qbytes).contains("quantumEntangler"));

        // 截断尾：前缀导入 + corrupt-tail warning，会话不丢。
        let tail_msgs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE session_id='corrupt' AND entry_type='message'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tail_msgs, 1);
        let tail_issues: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM import_issues WHERE kind='corrupt-tail'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tail_issues, 1);

        // 无重复会话：四次种子调用（含续跑）不翻倍。
        let session_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(session_count, 4);

        // 幂等续跑：把一个 convert 批次拨回 planned（模拟 A08 --stop-after-batch
        // 半程落点），completed+planned → 可续跑；重跑后消息/问题不得翻倍。
        conn.execute(
            "UPDATE import_batches SET state='planned' WHERE batch_key='session-jsonl/agents/a1/sessions/corrupt.jsonl'",
            [],
        )
        .unwrap();
        let issues_before: i64 = conn
            .query_row("SELECT COUNT(*) FROM import_issues", [], |r| r.get(0))
            .unwrap();
        let resumed = run(&args(&source, &target, Some(outcome.run_id.clone()))).unwrap();
        assert!(resumed.resumed);
        let conn2 = Connection::open(target.join(TARGET_DB_RELPATH)).unwrap();
        let total_msgs: i64 = conn2
            .query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total_msgs, 8); // 2+2+3(含标记)+1
        let total_sessions: i64 = conn2
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total_sessions, 4);
        let issues_after: i64 = conn2
            .query_row("SELECT COUNT(*) FROM import_issues", [], |r| r.get(0))
            .unwrap();
        assert_eq!(issues_after, issues_before);

        std::fs::remove_dir_all(source.parent().unwrap()).unwrap();
    }
}
