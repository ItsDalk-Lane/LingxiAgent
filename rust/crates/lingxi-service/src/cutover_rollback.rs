//! R08-T06：rollback / reforward 动词编排器（docs/rust-tauri/R08/T06_DESIGN.md，
//! Lead 已批 J1–J8 + 附加三钉）。
//!
//! 纪律（与 migrate 编排器同构，但独立 journal —— J5：migrate 本体零改动）：
//! - journal 相位是恢复唯一权威；任一进相失败 = refused 终态 + 响亮退出码 2，
//!   双侧数据保留，绝不自动删除任何一侧（refused 终态不可 --resume）。
//! - 指针切换 = pointer::commit_pointer 原子 rename + 读回比对：切换要么没发生、
//!   要么完整发生；切换后验证失败不反向再切（J6 唯一例外：old_stack_probe 失败
//!   → 指针回指新根 + refused + 修复模式，双侧保留）。
//! - 哈希约定披露（Lead 裁决）：delta 包条目为真·内容 sha256；manifest 域哈希为
//!   内部双哈希约定（T04/T05 同域），仅用于域内自洽比对。lineage.json 与收据均
//!   带 hashConvention 标注；runbook 承担人读披露义务。
//! - N4 双钉（附加钉①）：mode B 的 epoch_adopt 复用 EpochAdoptionEvidence /
//!   run_epoch_transition，checkpoint_receipt["id"] == checkpoint_id 且 journal
//!   transitionId == evidence.cutover_id 的校验链原样生效。
//! - journal 文件按动词分离：rollback-journal.json / reforforward-journal.json，
//!   reforward 以只读方式消费 rollback journal 的 committed 事实（证据不覆写）。

use lingxi_adapters::storage::legacy_import::{
    classes,
    manifest::{self, Manifest},
    ImportArgs, RECEIPT_DIR_RELPATH, TARGET_DB_RELPATH,
};
use lingxi_adapters::storage::migrations;
use lingxi_adapters::storage::rollback_delta;
use lingxi_adapters::storage::statfs;
use lingxi_service::cutover::backup;
use lingxi_service::cutover::pointer;
use lingxi_service::cutover::probe;
use lingxi_service::cutover::selfcheck::{self, SelfCheckExpectations};
use lingxi_service::cutover::stillpoint;
use lingxi_service::epoch::{self, EpochAdoptionEvidence, StampFormat, StampRead};
use lingxi_service::paths;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::legacy_import_runner::{self, VerbFailure};

// ── 常量 ────────────────────────────────────────────────────────

pub const ROLLBACK_JOURNAL_FILE: &str = "rollback-journal.json";
pub const REFORWARD_JOURNAL_FILE: &str = "reforward-journal.json";
const JOURNAL_SCHEMA: &str = "t06-rollback-journal/v1";
pub const ROLLBACK_RECEIPT_SCHEMA: &str = "t06-rollback-receipt/v1";
pub const REFORWARD_RECEIPT_SCHEMA: &str = "t06-reforward-receipt/v1";
const NEWROOT_BACKUP_MANIFEST_SCHEMA: &str = "t06-newroot-backup/v1";
const DELTA_LINEAGE_SCHEMA: &str = "t06-delta-lineage/v1";
const OLD_ROOT_PREPARE_DIRNAME: &str = "old-root-prepare";
const HASH_CONVENTION_NOTE: &str =
    "manifest-domain sha256 values use the internal double-hash convention \
     (same domain as T04/T05 manifests, comparison-only); delta package payload \
     entries carry true content sha256 (shasum-verifiable)";
/// 空间余量：备份为全树副本、增量包 ≤ 全树，两侧各加固定余量防贴边。
const SPACE_SLACK_BYTES: u64 = 256 * 1024 * 1024;
/// 旧栈探测纪律。
const PROBE_READY_BUDGET: Duration = Duration::from_secs(60);
const PROBE_SHUTDOWN_BUDGET: Duration = Duration::from_secs(15);
/// 旧栈 stderr 中绝不允许出现的数据 epoch 门标记（server/index.ts 启动闸）。
const PROBE_FORBIDDEN_STDERR_MARKERS: [&str; 3] = [
    "LINGXI_DATA_EPOCH_BLOCKED",
    "LINGXI_DATA_EPOCH_TRANSITION_INCOMPLETE",
    "LINGXI_DATA_EPOCH_BASELINE_WARNING",
];
/// 旧栈探测允许的旧根写集（精确路径；生命周期对消件——启动写出/关停注销，
/// 探针窗内净变化可能为非零）。
const PROBE_ALLOWED_EXACT: [&str; 1] = ["server-info.json"];
/// 证据派生白名单（设计 §7.2a，Lead 裁决 2026-10-23）：探针相活跃写集的
/// 允许类集 = 控制探针实测写集（t06-control-probe-05，旧栈冷启动 on 未回滚
/// fixture 副本：added=55 / modified=4 / removed=0）按 classes.rs 归类后的
/// 类 id 全集（18 类），∪ 准备相件所属类（execution-leases / agent-profile，
/// 防御性：准备相写于探针快照 A 之前，正常不出现；旧栈若重触它们亦属其
/// 自有存储）。**集外类/未分类路径写入 = 真红**（控制集之外出现写入=回滚
/// 事务泄漏，停手上报）。类模式即泛化面：logs/{timestamp}.log（runtime-
/// diagnostics）、knowledge/indexes/**（knowledge-indexes）、skills/
/// {skillName} 目录（skill-state）、file-history 显式 wal/shm 模式等。
/// 注意 session-manifest-sqlite 是 Convert 类——控制实测确认旧栈冷启动
/// 会写自己的会话清单库，白名单不能只按 Register/Regenerable 收。
const PROBE_ALLOWED_CLASSES: &[&str] = &[
    // ── 控制实测 18 类（t06-control-probe-05 写集归类）──
    "agent-authored-records",      // user/user.md
    "agent-facts-sqlite",          // agents/{id}/memory/facts.db
    "agent-memory",                // agents/{id}/memory/{memory,week,…}.md
    "device-access-registries",    // devices*.json / pairing-sessions.json
    "file-history-sqlite",         // file-history/{hash}/history.sqlite{,-shm,-wal}
    "knowledge-database",          // knowledge/knowledge.db（冷启动迁移触写）
    "knowledge-indexes",           // knowledge/indexes/*.db{,-shm,-wal}（Regenerable）
    "model-observability-db",      // model-observability/observability.sqlite
    "provider-state",              // auth/models/models-store/provider-catalog.json
    "runtime-diagnostics",         // logs/{timestamp}.log
    "security-grants",             // security/grants.json
    "server-network-config",       // server-network.json
    "server-node-identity",        // server-node.json
    "session-manifest-sqlite",     // session-manifest.db（Convert；实测被改）
    "skill-state",                 // skills/{skillName}/**（首启播种技能包）
    "studio-mount-registry",      // studio-mounts.json
    "user-preferences",            // user/preferences.json
    "user-studio-registries",      // users.json / studios.json
    // ── 准备相件所属类（§7.2a「∪ 准备相件」，防御性）──
    "execution-leases",            // security/execution-leases.json（J9①）
    "agent-profile",               // agents/{agentId}/config.yaml（J9②）
];
/// 探测期间 data-epoch.json 条款（裁决①，2026-10-23）：旧栈清洁启动把
/// lastVersion 盖为自身版本（legacy-import → 0.1.43）并刷 updatedAt =
/// 生产忠实 provenance 盖戳；断言改为四个 epoch 字段（schemaVersion/epoch/
/// minimumReaderEpoch/committedDataEpoch）逐值冻结 + 仅 lastVersion/updatedAt
/// 允许变化；其余任何差异（新增/删除字段、四字段任一变化）仍 forbidden。
/// 证据列：before/after 六字段逐值 + provenanceStamped 判定入探针报告。
const EPOCH_STAMP_RELPATH: &str = "data-epoch.json";
const EPOCH_JOURNAL_RELPATH: &str = "data-epoch-transition.json";

const ROLLBACK_PHASES: [&str; 9] = [
    "preflight",
    "stop_new_writer",
    "backup_new_root",
    "delta_export",
    "export_verify",
    "old_root_prepare",
    "pointer_switch",
    "old_stack_probe",
    "committed",
];
const REFORWARD_A_PHASES: [&str; 5] = [
    "preflight",
    "seal_verify",
    "pointer_switch",
    "selfcheck",
    "committed",
];
const REFORWARD_B_PHASES: [&str; 9] = [
    "preflight",
    "backup_old_root",
    "import",
    "delta_import",
    "delta_verify",
    "epoch_adopt",
    "pointer_switch",
    "selfcheck",
    "committed",
];

// ── 参数 / 出口 ────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RollbackArgs {
    pub pointer: PathBuf,
    pub old_root: PathBuf,
    pub new_root: PathBuf,
    pub work_dir: PathBuf,
    pub backup_dir: PathBuf,
    pub delta_out: PathBuf,
    /// 旧栈启动器（scripts/launch.js 路径）与工作目录（旧 Node 仓库根）。
    pub old_stack_launcher: PathBuf,
    pub old_stack_cwd: PathBuf,
    pub resume: Option<String>,
    pub stop_after_phase: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ReforwardArgs {
    pub pointer: PathBuf,
    pub old_root: PathBuf,
    pub new_root: PathBuf,
    pub work_dir: PathBuf,
    /// mode B 必填。
    pub backup_dir: Option<PathBuf>,
    pub delta_package: Option<PathBuf>,
    pub target: Option<PathBuf>,
    /// --recover-new-root：mode B 开关（J4：默认 mode A 快回指）。
    pub recover_new_root: bool,
    pub resume: Option<String>,
    pub stop_after_phase: Option<String>,
}

/// 动词出口摘要（调用方打印 ROLLBACK_RESULT / REFORWARD_RESULT 行）。
#[derive(Debug)]
pub struct RollbackOutcome {
    pub id: String,
    pub phase: String,
    pub outcome: &'static str, // committed | already-committed | stopped
    pub detail: String,
}

fn verb_err(code: u8, line: String) -> VerbFailure {
    VerbFailure { code, line }
}

fn now_unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn new_id(prefix: &str) -> String {
    format!("{prefix}-{}-{}", now_unix_ms(), std::process::id())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    Ok(manifest::sha256_hex(&bytes))
}

fn canonical(path: &Path, what: &str) -> Result<PathBuf, String> {
    std::fs::canonicalize(path).map_err(|e| format!("canonicalize {what} {}: {e}", path.display()))
}

// ── journal（t06-rollback-journal/v1；镜像 journal.rs 纪律，独立文件）──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalHistoryEntry {
    pub phase: String,
    pub entered_at: String,
    pub outcome: String, // in-progress | ok | stopped | refused
    #[serde(default)]
    pub detail: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackJournal {
    pub schema_version: String,
    pub verb: String, // "rollback" | "reforward"
    pub id: String,
    pub inputs: Value,
    pub phases: Vec<String>,
    pub phase: String,
    pub attempts: u64,
    pub status: String, // in-progress | stopped | committed | refused
    #[serde(default)]
    pub history: Vec<JournalHistoryEntry>,
    #[serde(default)]
    pub facts: Map<String, Value>,
    #[serde(default)]
    pub refusal: Option<Value>,
}

impl RollbackJournal {
    fn path(work_dir: &Path, verb: &str) -> PathBuf {
        work_dir.join(if verb == "rollback" {
            ROLLBACK_JOURNAL_FILE
        } else {
            REFORWARD_JOURNAL_FILE
        })
    }

    /// 不存在 → Ok(None)；存在但损坏 → 响亮 Err（corrupt 绝不自动修复）。
    fn load(work_dir: &Path, verb: &str) -> Result<Option<Self>, String> {
        let path = Self::path(work_dir, verb);
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("read journal {}: {e}", path.display())),
        };
        let journal: Self = serde_json::from_str(&raw)
            .map_err(|e| format!("journal {} is corrupt: {e} (refusing to repair)", path.display()))?;
        if journal.schema_version != JOURNAL_SCHEMA {
            return Err(format!(
                "journal {} schemaVersion {:?} != {JOURNAL_SCHEMA:?}",
                path.display(),
                journal.schema_version
            ));
        }
        if journal.verb != verb {
            return Err(format!(
                "journal {} verb {:?} != {verb:?}",
                path.display(),
                journal.verb
            ));
        }
        Ok(Some(journal))
    }

    fn create(work_dir: &Path, verb: &str, id: &str, inputs: Value, phases: &[&str]) -> Result<Self, String> {
        let journal = Self {
            schema_version: JOURNAL_SCHEMA.to_string(),
            verb: verb.to_string(),
            id: id.to_string(),
            inputs,
            phases: phases.iter().map(|p| p.to_string()).collect(),
            phase: phases[0].to_string(),
            attempts: 1,
            status: "in-progress".to_string(),
            history: vec![JournalHistoryEntry {
                phase: phases[0].to_string(),
                entered_at: now_iso(),
                outcome: "in-progress".to_string(),
                detail: Value::Null,
            }],
            facts: Map::new(),
            refusal: None,
        };
        journal.save(work_dir)?;
        Ok(journal)
    }

    fn save(&self, work_dir: &Path) -> Result<(), String> {
        let path = Self::path(work_dir, &self.verb);
        let mut body = serde_json::to_string_pretty(self)
            .map_err(|e| format!("serialize journal: {e}"))?;
        body.push('\n');
        paths::atomic_write_private(&path, body.as_bytes())
            .map_err(|e| format!("write journal {}: {e}", path.display()))
    }

    /// 相位推进：当前相标记 ok，进入 next（原子落盘）。
    fn advance(mut self, work_dir: &Path, next: &str, detail: Value) -> Result<Self, String> {
        if let Some(last) = self.history.last_mut() {
            if last.outcome == "in-progress" {
                last.outcome = "ok".to_string();
            }
        }
        self.phase = next.to_string();
        self.history.push(JournalHistoryEntry {
            phase: next.to_string(),
            entered_at: now_iso(),
            outcome: "in-progress".to_string(),
            detail,
        });
        self.save(work_dir)?;
        Ok(self)
    }

    fn stop(mut self, work_dir: &Path) -> Result<Self, String> {
        if let Some(last) = self.history.last_mut() {
            if last.outcome == "in-progress" {
                last.outcome = "stopped".to_string();
            }
        }
        self.status = "stopped".to_string();
        self.save(work_dir)?;
        Ok(self)
    }

    fn refuse(mut self, work_dir: &Path, phase: &str, reason: &str, repair: Value) -> Result<Self, String> {
        if let Some(last) = self.history.last_mut() {
            if last.outcome == "in-progress" {
                last.outcome = "refused".to_string();
            }
        }
        self.status = "refused".to_string();
        self.refusal = Some(json!({
            "phase": phase,
            "reason": reason,
            "repairAction": repair,
            "refusedAt": now_iso(),
        }));
        self.save(work_dir)?;
        Ok(self)
    }

    fn commit(mut self, work_dir: &Path) -> Result<Self, String> {
        if let Some(last) = self.history.last_mut() {
            if last.outcome == "in-progress" {
                last.outcome = "ok".to_string();
            }
        }
        self.phase = "committed".to_string();
        self.status = "committed".to_string();
        self.save(work_dir)?;
        Ok(self)
    }

    fn fact(&self, key: &str) -> Result<&Value, String> {
        self.facts
            .get(key)
            .ok_or_else(|| format!("journal fact {key:?} missing (journal incomplete)"))
    }

    fn fact_str(&self, key: &str) -> Result<String, String> {
        self.fact(key)?
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| format!("journal fact {key:?} is not a string"))
    }
}

/// 打开或创建 journal，套用 --resume 纪律（镜像 migrate 动词）：
/// 无 journal + --resume → 退出 2；有 journal 无 --resume → 退出 2；
/// id 不符 → 退出 2；committed → 幂等放行；refused → 终态退出 2。
fn open_journal(
    work_dir: &Path,
    verb: &str,
    id_prefix: &str,
    inputs: Value,
    phases: &[&str],
    resume: &Option<String>,
) -> Result<(RollbackJournal, Option<&'static str>), String> {
    match RollbackJournal::load(work_dir, verb)? {
        None => {
            if resume.is_some() {
                return Err(format!(
                    "--resume given but no {verb} journal exists in {}",
                    work_dir.display()
                ));
            }
            let journal = RollbackJournal::create(work_dir, verb, &new_id(id_prefix), inputs, phases)?;
            Ok((journal, None))
        }
        Some(mut journal) => {
            let resume_id = resume.as_deref().ok_or_else(|| {
                format!(
                    "a {verb} journal already exists (id {}); pass --resume {} to continue it",
                    journal.id, journal.id
                )
            })?;
            if resume_id != journal.id {
                return Err(format!(
                    "--resume id {resume_id} does not match journal id {}",
                    journal.id
                ));
            }
            match journal.status.as_str() {
                "committed" => Ok((journal, Some("already-committed"))),
                "refused" => Err(format!(
                    "{verb} {} is terminally refused (phase {}): {} — see journal repairAction; \
                     both roots are preserved",
                    journal.id,
                    journal
                        .refusal
                        .as_ref()
                        .and_then(|r| r.get("phase"))
                        .and_then(|p| p.as_str())
                        .unwrap_or("?"),
                    journal
                        .refusal
                        .as_ref()
                        .and_then(|r| r.get("reason"))
                        .and_then(|p| p.as_str())
                        .unwrap_or("?"),
                )),
                "stopped" | "in-progress" => {
                    journal.attempts += 1;
                    journal.save(work_dir)?;
                    Ok((journal, None))
                }
                other => Err(format!("journal has unknown status {other:?}")),
            }
        }
    }
}

// ── 共享小工具 ──────────────────────────────────────────────────

/// probe_target_counts 的三计数版（orchestrator 私有函数的本模块等价物）。
fn probe_counts(root: &Path) -> Result<(u64, u64, u64), String> {
    let db_path = root.join(TARGET_DB_RELPATH);
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("open runs.db {}: {e}", db_path.display()))?;
    let count = |sql: &str| -> Result<u64, String> {
        conn.query_row(sql, [], |row| row.get::<_, i64>(0))
            .map(|v| v as u64)
            .map_err(|e| format!("count query failed ({sql}): {e}"))
    };
    let sessions = count("SELECT COUNT(*) FROM sessions")?;
    let model_call_usage = count("SELECT COUNT(*) FROM model_call_usage")?;
    let cron_jobs = count("SELECT COUNT(*) FROM automation_jobs_v1 WHERE deleted_at IS NULL")?;
    Ok((sessions, model_call_usage, cron_jobs))
}

/// verify_import_core 的本模块等价物（mode B epoch 验证闭包）。
fn verify_recovered_root(target: &Path) -> Result<(), String> {
    let receipt_path = target.join(RECEIPT_DIR_RELPATH).join("migration-receipt.json");
    let raw = std::fs::read_to_string(&receipt_path)
        .map_err(|e| format!("read receipt {}: {e}", receipt_path.display()))?;
    let receipt: Value = serde_json::from_str(&raw)
        .map_err(|e| format!("parse receipt {}: {e}", receipt_path.display()))?;
    let status = receipt.get("status").and_then(|v| v.as_str()).unwrap_or("<missing>");
    if status != "complete" {
        return Err(format!("migration receipt status is {status:?}, expected \"complete\""));
    }
    let db_path = target.join(TARGET_DB_RELPATH);
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("open runs.db {}: {e}", db_path.display()))?;
    let user_version: u64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map(|v: i64| v as u64)
        .map_err(|e| format!("PRAGMA user_version: {e}"))?;
    let tip = migrations::MIGRATIONS.len() as u64;
    if user_version != tip {
        return Err(format!("target runs.db user_version {user_version} != schema tip {tip}"));
    }
    Ok(())
}

/// 全树确定性副本（backup_new_root 专用；T04 classes 表为 legacy 域，
/// 新根布局不在其分类面内，故不复用 backup::run_backup —— 见台账偏离记录）。
/// 返回 (文件数, 总字节)。symlink/特殊文件 = 响亮拒绝。
fn copy_tree_full(source: &Path, dest: &Path) -> Result<(u64, u64), String> {
    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        let src_dir = source.join(&rel);
        let mut entries: Vec<_> = std::fs::read_dir(&src_dir)
            .map_err(|e| format!("read dir {}: {e}", src_dir.display()))?
            .collect::<Result<_, _>>()
            .map_err(|e| format!("read dir entry {}: {e}", src_dir.display()))?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let child_rel = rel.join(entry.file_name());
            let file_type = entry
                .file_type()
                .map_err(|e| format!("file type {}: {e}", entry.path().display()))?;
            if file_type.is_dir() {
                std::fs::create_dir_all(dest.join(&child_rel))
                    .map_err(|e| format!("mkdir {}: {e}", dest.join(&child_rel).display()))?;
                stack.push(child_rel);
            } else if file_type.is_file() {
                let meta = entry
                    .metadata()
                    .map_err(|e| format!("metadata {}: {e}", entry.path().display()))?;
                std::fs::copy(entry.path(), dest.join(&child_rel))
                    .map_err(|e| format!("copy {}: {e}", entry.path().display()))?;
                files += 1;
                bytes += meta.len();
            } else {
                return Err(format!(
                    "special file (symlink/fifo/…) in new root, refusing full-tree copy: {}",
                    child_rel.display()
                ));
            }
        }
    }
    Ok((files, bytes))
}

/// 旧根写集分类（证据派生白名单，§7.2a）：允许 = PROBE_ALLOWED_EXACT ∪
/// PROBE_ALLOWED_CLASSES 类匹配 ∪ {-shm/-wal 兄弟泛化} ∪ data-epoch.json
/// 裁决①条款；data-epoch-transition.json 永远禁止；集外 = forbidden（真红）。
/// 返回 (allowed, forbidden, epoch_evidence)；epoch_evidence 仅当印章出现在
/// 写集时非空（裁决①证据列）。
fn classify_write_set(
    before: &Manifest,
    after: &Manifest,
    epoch_before: Option<&str>,
    epoch_after: Option<&str>,
) -> (Vec<String>, Vec<String>, Value) {
    let to_map = |m: &Manifest| -> Map<String, Value> {
        m.entries
            .iter()
            .map(|e| (e.relpath.clone(), json!([e.size, e.sha256])))
            .collect()
    };
    let before_map = to_map(before);
    let after_map = to_map(after);
    let mut changed: Vec<String> = Vec::new();
    for (rel, sig) in after_map.iter() {
        match before_map.get(rel) {
            Some(old) if old == sig => {}
            _ => changed.push(rel.clone()),
        }
    }
    for rel in before_map.keys() {
        if !after_map.contains_key(rel) {
            changed.push(rel.clone());
        }
    }
    changed.sort();
    let class_allowed = |rel: &str| -> bool {
        if let Some((def, _)) = classes::classify(rel) {
            return PROBE_ALLOWED_CLASSES.contains(&def.id);
        }
        false
    };
    let mut allowed = Vec::new();
    let mut forbidden = Vec::new();
    let mut epoch_evidence = Value::Null;
    for rel in changed {
        if rel == EPOCH_JOURNAL_RELPATH {
            forbidden.push(rel);
        } else if rel == EPOCH_STAMP_RELPATH {
            // 裁决①条款：四字段逐值冻结 + lastVersion/updatedAt 盖戳允许。
            match epoch_stamp_clause(epoch_before, epoch_after) {
                Ok(ev) => {
                    epoch_evidence = ev;
                    allowed.push(rel);
                }
                Err(reason) => {
                    epoch_evidence = json!({"verdict": "forbidden", "reason": reason});
                    forbidden.push(rel);
                }
            }
        } else if PROBE_ALLOWED_EXACT.contains(&rel.as_str()) || class_allowed(&rel) {
            allowed.push(rel);
        } else if (rel.ends_with("-shm") || rel.ends_with("-wal"))
            && class_allowed(rel.trim_end_matches("-shm").trim_end_matches("-wal"))
        {
            // {db}-shm/-wal 兄弟泛化：wal/shm 属其主库存储身份；主库类在
            // 允许集内则兄弟文件允许（知识索引/文件史模式已显式覆盖，此处
            // 兜底其余 sqlite 存储的连接生命周期兄弟件）。
            allowed.push(rel);
        } else {
            forbidden.push(rel);
        }
    }
    (allowed, forbidden, epoch_evidence)
}

/// 裁决①：四个 epoch 字段逐值冻结；lastVersion/updatedAt 为 provenance
/// 盖戳允许；其余任何差异（含字段新增/删除/类型变化）拒绝。
/// 返回证据 JSON（before/after 六字段 + provenanceStamped 判定）。
fn epoch_stamp_clause(before: Option<&str>, after: Option<&str>) -> Result<Value, String> {
    let (b, a) = match (before, after) {
        (Some(b), Some(a)) => (b, a),
        _ => return Err("data-epoch.json missing before/after content".to_string()),
    };
    let parse = |raw: &str| -> Result<Map<String, Value>, String> {
        serde_json::from_str::<Value>(raw)
            .map_err(|e| format!("stamp json parse: {e}"))?
            .as_object()
            .cloned()
            .ok_or_else(|| "stamp is not a json object".to_string())
    };
    let bm = parse(b)?;
    let am = parse(a)?;
    const FROZEN: [&str; 4] = [
        "schemaVersion",
        "epoch",
        "minimumReaderEpoch",
        "committedDataEpoch",
    ];
    const PROVENANCE: [&str; 2] = ["lastVersion", "updatedAt"];
    for field in FROZEN {
        if bm.get(field) != am.get(field) {
            return Err(format!(
                "epoch field {field} changed: {:?} -> {:?}",
                bm.get(field), am.get(field)
            ));
        }
    }
    let b_keys: std::collections::BTreeSet<&String> = bm.keys().collect();
    let a_keys: std::collections::BTreeSet<&String> = am.keys().collect();
    if b_keys != a_keys {
        let added: Vec<&str> = a_keys.difference(&b_keys).map(|k| k.as_str()).collect();
        let removed: Vec<&str> = b_keys.difference(&a_keys).map(|k| k.as_str()).collect();
        return Err(format!(
            "stamp field set changed (added={added:?}, removed={removed:?}) beyond provenance stamping"
        ));
    }
    let mut changed_provenance: Vec<&str> = Vec::new();
    for field in PROVENANCE {
        if bm.get(field) != am.get(field) {
            changed_provenance.push(field);
        }
    }
    Ok(json!({
        "verdict": "allowed",
        "frozenFields": {
            "schemaVersion": bm.get("schemaVersion"),
            "epoch": bm.get("epoch"),
            "minimumReaderEpoch": bm.get("minimumReaderEpoch"),
            "committedDataEpoch": bm.get("committedDataEpoch"),
        },
        "before": {"lastVersion": bm.get("lastVersion"), "updatedAt": bm.get("updatedAt")},
        "after": {"lastVersion": am.get("lastVersion"), "updatedAt": am.get("updatedAt")},
        "provenanceStamped": changed_provenance,
    }))
}

/// 最小环回 HTTP GET（无新依赖）：返回状态码。
/// /api/health 就绪相需鉴权（server/http/route-security.ts L58 AUTHENTICATED_ONLY；
/// 仅 503 引导相免鉴权，server/index.ts L389）——token 取自 server-info.json。
fn http_get_status(port: u16, path: &str, timeout: Duration, token: Option<&str>) -> Result<u16, String> {
    use std::io::{Read, Write};
    let addr: std::net::SocketAddr = ([127, 0, 0, 1], port).into();
    let mut stream = std::net::TcpStream::connect_timeout(&addr, timeout)
        .map_err(|e| format!("connect 127.0.0.1:{port}: {e}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| format!("set timeout: {e}"))?;
    let auth = match token {
        Some(t) if !t.is_empty() => format!("Authorization: Bearer {t}\r\n"),
        _ => String::new(),
    };
    let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{auth}Connection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("write request: {e}"))?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
                if buf.len() > 64 * 1024 {
                    return Err("response head exceeded 64KiB".to_string());
                }
            }
            Err(e) => return Err(format!("read response: {e}")),
        }
    }
    let head = String::from_utf8_lossy(&buf);
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| format!("malformed status line: {:?}", head.lines().next().unwrap_or("")))?;
    Ok(status)
}

/// 旧栈探测报告（J6：任一断言失败 → 指针回指新根 + refused）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OldStackProbeReport {
    port: u16,
    pid: u32,
    server_info_seen: bool,
    http_status: u16,
    stderr_markers_clean: bool,
    new_root_unchanged: bool,
    old_root_write_set_allowed: Vec<String>,
    old_root_write_set_forbidden: Vec<String>,
    /// 裁决①证据列：data-epoch.json 前后六字段逐值 + provenanceStamped 判定
    /// （印章出现在写集时非空；四字段冻结断言的取值可追溯）。
    epoch_stamp_evidence: Value,
    shutdown_clean: bool,
}

// ── rollback：相位实现 ─────────────────────────────────────────

/// 相 1：preflight（纯只读 + 目录创建）。任一前置不满足 → 响亮拒绝。
fn rb_preflight(args: &RollbackArgs) -> Result<Vec<(String, Value)>, String> {
    if !args.old_root.is_dir() {
        return Err(format!("old root {} is not a directory", args.old_root.display()));
    }
    if !args.new_root.is_dir() {
        return Err(format!("new root {} is not a directory", args.new_root.display()));
    }
    if !args.old_stack_launcher.is_file() {
        return Err(format!(
            "old stack launcher {} not found (--old-stack-launcher)",
            args.old_stack_launcher.display()
        ));
    }
    if !args.old_stack_cwd.is_dir() {
        return Err(format!(
            "old stack cwd {} is not a directory (--old-stack-cwd)",
            args.old_stack_cwd.display()
        ));
    }
    // 旧栈印章：Corrupt = 响亮拒绝（诊断证据，绝不猜测）。
    match epoch::read_stamp(&args.old_root) {
        StampRead::Corrupt { detail } => {
            return Err(format!("old root data-epoch stamp is corrupt: {detail}"));
        }
        StampRead::Missing | StampRead::Ok(_) => {}
    }
    // 新根印章：必须已提交 epoch 2（切割完成的证据）。
    match epoch::read_stamp(&args.new_root) {
        StampRead::Ok(stamp)
            if stamp.format == StampFormat::V2 && stamp.committed_data_epoch == 2 => {}
        StampRead::Ok(stamp) => {
            return Err(format!(
                "new root stamp is {:?} committedDataEpoch={} — expected v2 epoch 2 (cutover not committed?)",
                stamp.format, stamp.committed_data_epoch
            ));
        }
        StampRead::Missing => {
            return Err("new root has no data-epoch stamp — cutover not committed?".to_string());
        }
        StampRead::Corrupt { detail } => {
            return Err(format!("new root data-epoch stamp is corrupt: {detail}"));
        }
    }
    // 指针：必须存在且指向新根；cutoverId 即 lineage sourceCutoverId。
    let pointer = pointer::read_pointer(&args.pointer)
        .map_err(|e| format!("read pointer {}: {e}", args.pointer.display()))?
        .ok_or_else(|| format!("pointer {} does not exist", args.pointer.display()))?;
    let new_canon = canonical(&args.new_root, "new root")?;
    let pointed = PathBuf::from(&pointer.active_data_root);
    let pointed_canon = canonical(&pointed, "pointer activeDataRoot")?;
    if pointed_canon != new_canon {
        return Err(format!(
            "pointer activeDataRoot {} does not equal new root {} (rollback requires the pointer to be on the new root)",
            pointer.active_data_root,
            args.new_root.display()
        ));
    }
    // 前向迁移收据：lineage 需要；缺失 = 响亮拒绝。
    let receipt_path = args
        .new_root
        .join(RECEIPT_DIR_RELPATH)
        .join("migration-receipt.json");
    let receipt_raw = std::fs::read_to_string(&receipt_path)
        .map_err(|e| format!("forward migration receipt {} unreadable: {e}", receipt_path.display()))?;
    let receipt: Value = serde_json::from_str(&receipt_raw)
        .map_err(|e| format!("parse forward receipt: {e}"))?;
    if receipt.get("status").and_then(|v| v.as_str()) != Some("complete") {
        return Err(format!(
            "forward migration receipt status is {:?}, expected \"complete\"",
            receipt.get("status")
        ));
    }
    let receipt_sha = manifest::sha256_hex(receipt_raw.as_bytes());
    // 写者探针：新根 Rust 锁不可持有；旧根旧栈不可存活。
    let rust_lock = probe::probe_rust_instance_lock(&args.new_root)
        .map_err(|e| format!("probe rust instance lock: {e}"))?;
    if matches!(rust_lock.probe, probe::RustLockProbe::Held) {
        return Err(format!(
            "new root has a live Rust writer ({}); stop it before rollback",
            rust_lock.lock_path
        ));
    }
    let legacy = probe::probe_legacy_server_info(&args.old_root)
        .map_err(|e| format!("probe legacy server info: {e}"))?;
    if legacy.probe.is_blocking() {
        return Err(format!(
            "old root has a live legacy stack ({}); stop it before rollback",
            legacy.detail
        ));
    }
    // 空间：备份（全树×2 + 余量）与增量包（全树 + 余量）。
    std::fs::create_dir_all(&args.work_dir).map_err(|e| format!("mkdir work-dir: {e}"))?;
    std::fs::create_dir_all(&args.backup_dir).map_err(|e| format!("mkdir backup-dir: {e}"))?;
    std::fs::create_dir_all(&args.delta_out).map_err(|e| format!("mkdir delta-out: {e}"))?;
    let scan = stillpoint::scan_pass(&args.new_root)
        .map_err(|e| format!("scan new root for space estimate: {e}"))?;
    let total_bytes: u64 = scan.entries.iter().map(|e| e.size).sum();
    let backup_need = total_bytes.saturating_mul(2).saturating_add(SPACE_SLACK_BYTES);
    let backup_avail = statfs::available_bytes(&args.backup_dir)
        .map_err(|e| format!("statfs backup-dir: {e}"))?;
    if backup_avail < backup_need {
        return Err(format!(
            "insufficient space on backup volume: need {backup_need} bytes (2×tree+slack), available {backup_avail}"
        ));
    }
    let delta_need = total_bytes.saturating_add(SPACE_SLACK_BYTES);
    let delta_avail =
        statfs::available_bytes(&args.delta_out).map_err(|e| format!("statfs delta-out: {e}"))?;
    if delta_avail < delta_need {
        return Err(format!(
            "insufficient space on delta volume: need {delta_need} bytes (tree+slack), available {delta_avail}"
        ));
    }
    let source_cutover_id = pointer.cutover_id.clone();
    Ok(vec![
        ("sourceCutoverId".into(), json!(source_cutover_id)),
        (
            "forwardMigrationReceipt".into(),
            json!({"path": receipt_path.display().to_string(), "sha256": receipt_sha}),
        ),
        ("pointerBefore".into(), json!(pointer.active_data_root)),
        (
            "preflight".into(),
            json!({
                "newRootTreeBytes": total_bytes,
                "backupNeed": backup_need, "backupAvail": backup_avail,
                "deltaNeed": delta_need, "deltaAvail": delta_avail,
            }),
        ),
    ])
}

/// 相 2：stop_new_writer —— 幂等再验（写者停干净是后续所有相的前提）。
fn rb_stop_new_writer(args: &RollbackArgs) -> Result<Vec<(String, Value)>, String> {
    let rust_lock = probe::probe_rust_instance_lock(&args.new_root)
        .map_err(|e| format!("probe rust instance lock: {e}"))?;
    if matches!(rust_lock.probe, probe::RustLockProbe::Held) {
        return Err(format!(
            "new root still has a live Rust writer ({}): {}",
            rust_lock.lock_path, rust_lock.detail
        ));
    }
    let legacy = probe::probe_legacy_server_info(&args.new_root)
        .map_err(|e| format!("probe legacy server info on new root: {e}"))?;
    if legacy.probe.is_blocking() {
        return Err(format!("new root has a live legacy stack: {}", legacy.detail));
    }
    Ok(vec![(
        "stopNewWriter".into(),
        json!({"rustLock": format!("{:?}", rust_lock.probe), "detail": rust_lock.detail}),
    )])
}

/// 相 3：backup_new_root —— 全树确定性副本 + manifest 双域比对。
/// （不复用 backup::run_backup：T04 classes 表是 legacy 域，新根布局不在其
/// 分类面内 —— 设计锁定计划的偏离已记台账。）
fn rb_backup_new_root(args: &RollbackArgs, journal: &RollbackJournal) -> Result<Vec<(String, Value)>, String> {
    let source_scan = stillpoint::scan_pass(&args.new_root)
        .map_err(|e| format!("scan new root: {e}"))?;
    if !source_scan.skipped_special.is_empty() {
        let list: Vec<String> = source_scan
            .skipped_special
            .iter()
            .map(|s| format!("{} ({})", s.relpath, s.kind))
            .collect();
        return Err(format!(
            "new root contains special files, refusing full-tree backup: {}",
            list.join(", ")
        ));
    }
    let dest = args.backup_dir.join(format!("rollback-{}", journal.id));
    if dest.exists() {
        // 续跑幂等：dest 命名空间为本 journal 所有，半成品直接清掉重来。
        std::fs::remove_dir_all(&dest)
            .map_err(|e| format!("remove partial backup {}: {e}", dest.display()))?;
    }
    std::fs::create_dir_all(&dest).map_err(|e| format!("mkdir {}: {e}", dest.display()))?;
    let (files, bytes) = copy_tree_full(&args.new_root, &dest)?;
    // 副本对拍：同一 manifest 域（内部双哈希约定）根哈希必须相等。
    let dest_scan = stillpoint::scan_pass(&dest).map_err(|e| format!("scan backup: {e}"))?;
    let source_hash = manifest::source_root_sha256(&source_scan);
    let dest_hash = manifest::source_root_sha256(&dest_scan);
    if source_hash != dest_hash {
        return Err(format!(
            "backup verification failed: source root hash {source_hash} != backup {dest_hash} (manifest convention)"
        ));
    }
    let backup_manifest = json!({
        "schema": NEWROOT_BACKUP_MANIFEST_SCHEMA,
        "id": journal.id,
        "source": args.new_root.display().to_string(),
        "fileCount": files,
        "totalBytes": bytes,
        "rootSha256": source_hash,
        "hashConvention": HASH_CONVENTION_NOTE,
        "copiedAt": now_iso(),
    });
    let mut body = serde_json::to_string_pretty(&backup_manifest)
        .map_err(|e| format!("serialize backup manifest: {e}"))?;
    body.push('\n');
    paths::atomic_write_private(&dest.join(backup::BACKUP_MANIFEST_FILE), body.as_bytes())
        .map_err(|e| format!("write backup manifest: {e}"))?;
    Ok(vec![
        (
            "newRootBackup".into(),
            json!({
                "path": dest.display().to_string(),
                "rootSha256": source_hash,
                "fileCount": files,
                "totalBytes": bytes,
            }),
        ),
        ("newRootSealHash".into(), json!(source_hash)),
    ])
}

/// 相 4：delta_export —— 身份集差集导出（冻结身份集，J3）。
fn rb_delta_export(args: &RollbackArgs, journal: &RollbackJournal) -> Result<Vec<(String, Value)>, String> {
    let legacy = rollback_delta::derive_legacy_identity_set(&args.old_root)
        .map_err(|e| format!("derive legacy identity set: {e}"))?;
    let new_manifest = stillpoint::scan_pass(&args.new_root)
        .map_err(|e| format!("scan new root: {e}"))?;
    let seal = journal.fact_str("newRootSealHash")?;
    let current_hash = manifest::source_root_sha256(&new_manifest);
    if current_hash != seal {
        return Err(format!(
            "new root drifted between backup and delta export: {seal} → {current_hash} (a writer is still active?)"
        ));
    }
    let db_path = args.new_root.join(TARGET_DB_RELPATH);
    let tables = rollback_delta::export_db_delta(&db_path, &legacy)
        .map_err(|e| format!("export db delta: {e}"))?;
    let files = rollback_delta::diff_files(&args.new_root, &new_manifest, &legacy)
        .map_err(|e| format!("diff files: {e}"))?;
    let lineage = json!({
        "schema": DELTA_LINEAGE_SCHEMA,
        "rollbackId": journal.id,
        "sourceCutoverId": journal.fact_str("sourceCutoverId")?,
        "createdAt": now_iso(),
        "newRootSealHash": seal,
        "identitySetSha256": legacy.identity_set_sha256(),
        "legacyCounts": legacy.counts_json(),
        "hashConvention": HASH_CONVENTION_NOTE,
        // B①：原新根路径（原样 + 规范化两形态）——恢复式再前滚回灌时据此把
        // managed_cache 附件行的绝对路径重基到恢复根（身份与路径重新关联）。
        "originNewRoot": args.new_root.display().to_string(),
        "originNewRootCanonical": std::fs::canonicalize(&args.new_root)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| args.new_root.display().to_string()),
    });
    let package_dir = args.delta_out.join(&journal.id);
    if package_dir.exists() {
        // 续跑幂等：包目录为本 journal 所有；确定性重导，半成品清掉重来。
        std::fs::remove_dir_all(&package_dir)
            .map_err(|e| format!("remove partial package {}: {e}", package_dir.display()))?;
    }
    let pkg = rollback_delta::write_delta_package(
        &package_dir,
        &tables,
        &files,
        &args.new_root,
        &lineage,
        &legacy,
    )
    .map_err(|e| format!("write delta package: {e}"))?;
    let table_counts: Map<String, Value> = tables
        .iter()
        .map(|t| (t.table.clone(), json!(t.rows.len())))
        .collect();
    Ok(vec![(
        "deltaPackage".into(),
        json!({
            "path": package_dir.display().to_string(),
            "sha256": pkg.package_sha256,
            "identitySetSha256": pkg.identity_set_sha256,
            "legacyCounts": legacy.counts_json(),
            "perCategoryCounts": {
                "tables": Value::Object(table_counts),
                "files": pkg.files_count,
            },
        }),
    )])
}

/// 相 5：export_verify —— 全链复验 + 确定性重导比对（J3 对账）。
fn rb_export_verify(args: &RollbackArgs, journal: &RollbackJournal) -> Result<Vec<(String, Value)>, String> {
    // ① 旧根身份集重导：sha 必须与包内一致（旧根在导出后被碰 = 拒）。
    let legacy = rollback_delta::derive_legacy_identity_set(&args.old_root)
        .map_err(|e| format!("re-derive legacy identity set: {e}"))?;
    let pkg_fact = journal.fact("deltaPackage")?.clone();
    let want_identity = pkg_fact
        .get("identitySetSha256")
        .and_then(|v| v.as_str())
        .ok_or("deltaPackage.identitySetSha256 missing")?;
    if legacy.identity_set_sha256() != want_identity {
        return Err(format!(
            "legacy identity set drifted after export: {} → {} (old root was touched mid-rollback)",
            want_identity,
            legacy.identity_set_sha256()
        ));
    }
    // ② 新根封缄复验。
    let seal = journal.fact_str("newRootSealHash")?;
    let rescan = stillpoint::scan_pass(&args.new_root).map_err(|e| format!("rescan new root: {e}"))?;
    let current = manifest::source_root_sha256(&rescan);
    if current != seal {
        return Err(format!("new root seal broken: {seal} → {current}"));
    }
    // ③ 包自校验（read_delta_package 逐文件 sha256/size 复核）。
    let package_dir = args.delta_out.join(&journal.id);
    let (pkg, _, _) = rollback_delta::read_delta_package(&package_dir)
        .map_err(|e| format!("re-read delta package: {e}"))?;
    let want_pkg_sha = pkg_fact
        .get("sha256")
        .and_then(|v| v.as_str())
        .ok_or("deltaPackage.sha256 missing")?;
    if pkg.package_sha256 != want_pkg_sha {
        return Err(format!(
            "package sha256 drifted: {want_pkg_sha} → {}",
            pkg.package_sha256
        ));
    }
    // ④ 确定性重导：同输入 → manifest.json 与 payload 逐字节相等。
    //    lineage.json 含 createdAt provenance 时间戳，本身不在确定性门内；但
    //    manifest.json 内嵌 lineageSha256（并折入 packageSha256）——重导必须复用
    //    首次导出的 lineage.json 字节作为输入，否则时间戳差异会经该哈希泄入
    //    manifest 而把导出器确定性门误判为红（t06-d1-drill-04 真红根因）。
    //    lineage 是 provenance 输入而非导出器行为，复用它使比对精确落在
    //    「同新根 + 同身份集 + 同 lineage → 同包」上。
    let scratch = args.work_dir.join("scratch").join(format!("reexport-{}", journal.id));
    if scratch.exists() {
        std::fs::remove_dir_all(&scratch).map_err(|e| format!("clean scratch: {e}"))?;
    }
    let tables = rollback_delta::export_db_delta(&args.new_root.join(TARGET_DB_RELPATH), &legacy)
        .map_err(|e| format!("re-export db delta: {e}"))?;
    let files = rollback_delta::diff_files(&args.new_root, &rescan, &legacy)
        .map_err(|e| format!("re-diff files: {e}"))?;
    let lineage_path = package_dir.join("lineage.json");
    let lineage: Value = serde_json::from_slice(
        &std::fs::read(&lineage_path).map_err(|e| format!("read {}: {e}", lineage_path.display()))?,
    )
    .map_err(|e| format!("parse first-export lineage.json: {e}"))?;
    rollback_delta::write_delta_package(&scratch, &tables, &files, &args.new_root, &lineage, &legacy)
        .map_err(|e| format!("re-write delta package: {e}"))?;
    let compare = |rel: &str| -> Result<(), String> {
        let a = std::fs::read(package_dir.join(rel))
            .map_err(|e| format!("read {}: {e}", package_dir.join(rel).display()))?;
        let b = std::fs::read(scratch.join(rel))
            .map_err(|e| format!("read {}: {e}", scratch.join(rel).display()))?;
        if a != b {
            return Err(format!("determinism check failed: {rel} differs between exports"));
        }
        Ok(())
    };
    compare("manifest.json")?;
    for (table, _rows, relpath, _sha) in &pkg.tables {
        let _ = table;
        compare(relpath)?;
    }
    if let Some(files_rel) = pkg_fact.get("perCategoryCounts").and_then(|_| pkg.to_json().pointer("/files/path").cloned()) {
        if let Some(rel) = files_rel.as_str() {
            compare(rel)?;
        }
    }
    // payload/files/*：逐 FileDeltaEntry 重比对（files 数量小，直接全比）。
    let (_, _, file_entries) = rollback_delta::read_delta_package(&package_dir)
        .map_err(|e| format!("re-read package for payload compare: {e}"))?;
    for entry in &file_entries {
        // 包内文件按层级存储（write_delta_package L787 payload/files/<relpath>）；
        // 早期扁平名 replace('/',"__") 约定残留导致读错路径（t06-d1-drill-05 红根因）。
        compare(&format!("payload/files/{}", entry.relpath))
            .map_err(|e| format!("payload {}: {e}", entry.relpath))?;
    }
    std::fs::remove_dir_all(&scratch).map_err(|e| format!("clean scratch: {e}"))?;
    Ok(vec![(
        "exportVerify".into(),
        json!({
            "identitySetStable": true,
            "sealStable": true,
            "packageSelfVerified": true,
            "deterministicReexport": true,
        }),
    )])
}

/// 相 6：old_root_prepare（J1：archive-before-write + sha256 复验 + 逐写 journal
/// + 收据 prepareWrites[]）。生产诚实路径：印章已合规 → 零写 verify-only 记录。
fn rb_old_root_prepare(args: &RollbackArgs, journal: &RollbackJournal) -> Result<Vec<(String, Value)>, String> {
    let package_dir = args.delta_out.join(&journal.id);
    let archive_dir = package_dir.join(OLD_ROOT_PREPARE_DIRNAME);
    let mut writes: Vec<Value> = Vec::new();
    // ── 印章 ──
    let stamp_path = args.old_root.join(EPOCH_STAMP_RELPATH);
    let before_bytes = std::fs::read(&stamp_path).ok();
    let before_sha = before_bytes.as_ref().map(|b| manifest::sha256_hex(b));
    let conforming = matches!(
        epoch::read_stamp(&args.old_root),
        StampRead::Ok(ref s)
            if s.format == StampFormat::V2
                && s.minimum_reader_epoch == 1
                && s.committed_data_epoch == 1
    );
    let after_sha;
    if conforming {
        // 生产诚实路径：零写，verify-only 记录。
        after_sha = before_sha.clone();
        writes.push(json!({
            "relpath": EPOCH_STAMP_RELPATH,
            "action": "verified-already-conforming",
            "beforeSha256": before_sha,
            "afterSha256": after_sha,
        }));
    } else {
        std::fs::create_dir_all(&archive_dir).map_err(|e| format!("mkdir archive dir: {e}"))?;
        let mut archived_to = Value::Null;
        if let Some(bytes) = &before_bytes {
            let archive_path = archive_dir.join("data-epoch.json.orig");
            paths::atomic_write_private(&archive_path, bytes)
                .map_err(|e| format!("archive original stamp: {e}"))?;
            // 归档回读复验（J1）。
            let reread = sha256_file(&archive_path)?;
            if Some(&reread) != before_sha.as_ref() {
                return Err(format!(
                    "archive verification failed: {} sha256 {} != original {:?}",
                    archive_path.display(),
                    reread,
                    before_sha
                ));
            }
            archived_to = json!(format!("{OLD_ROOT_PREPARE_DIRNAME}/data-epoch.json.orig"));
        }
        // J8：印章目标 = v2 {schemaVersion:2, epoch:1, minimumReaderEpoch:1,
        // committedDataEpoch:1}（Node 同构）；复用 T05 同一写入函数。
        epoch::write_legacy_import_stamp(&args.old_root, now_unix_ms())
            .map_err(|e| format!("write v2 epoch-1 stamp: {e}"))?;
        match epoch::read_stamp(&args.old_root) {
            StampRead::Ok(ref s)
                if s.format == StampFormat::V2
                    && s.minimum_reader_epoch == 1
                    && s.committed_data_epoch == 1 => {}
            other => {
                return Err(format!(
                    "post-write stamp verification failed: {other:?} (original archived at {archived_to})"
                ));
            }
        }
        after_sha = Some(sha256_file(&stamp_path)?);
        writes.push(json!({
            "relpath": EPOCH_STAMP_RELPATH,
            "action": "rewritten-to-v2-epoch1",
            "beforeSha256": before_sha,
            "afterSha256": after_sha,
            "archivedTo": archived_to,
        }));
    }
    // ── 过渡 journal：存在即归档 + 移除（旧栈遇未完成过渡会拒启）。──
    let journal_path = args.old_root.join(EPOCH_JOURNAL_RELPATH);
    if let Ok(bytes) = std::fs::read(&journal_path) {
        let sha = manifest::sha256_hex(&bytes);
        std::fs::create_dir_all(&archive_dir).map_err(|e| format!("mkdir archive dir: {e}"))?;
        let archive_path = archive_dir.join("data-epoch-transition.json.orig");
        paths::atomic_write_private(&archive_path, &bytes)
            .map_err(|e| format!("archive transition journal: {e}"))?;
        let reread = sha256_file(&archive_path)?;
        if reread != sha {
            return Err(format!(
                "archive verification failed: {} sha256 {} != original {}",
                archive_path.display(),
                reread,
                sha
            ));
        }
        std::fs::remove_file(&journal_path)
            .map_err(|e| format!("remove transition journal {}: {e}", journal_path.display()))?;
        writes.push(json!({
            "relpath": EPOCH_JOURNAL_RELPATH,
            "action": "archived-and-removed",
            "beforeSha256": sha,
            "archivedTo": format!("{OLD_ROOT_PREPARE_DIRNAME}/data-epoch-transition.json.orig"),
        }));
    }
    // ── J9① execution-leases.json → 生产空注册表形状（封闭修复集，J1 纪律）。──
    // 夹具 {version:1, leases:[{id,holder}]} 行是 T04 注册类覆盖样本、非语义
    // 有效 lease；现役 core/execution-lease-registry.ts 逐字段强校验 → 旧栈冷
    // 启动必炸。修复=重写为生产空注册表形状（createEmptyExecutionLeaseRegistry
    // 逐字段，execution-lease-registry.ts:83-90：schemaVersion/leases/createdAt/
    // updatedAt 四键）。生产诚实路径：已合规 → verify-only 零写（resume 幂等）。
    let leases_note;
    if let Ok(bytes) = std::fs::read(args.old_root.join("security/execution-leases.json")) {
        let conforming = serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|v| v.as_object().cloned())
            .map(|obj| {
                let mut keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
                keys.sort_unstable();
                keys == ["createdAt", "leases", "schemaVersion", "updatedAt"]
                    && obj.get("schemaVersion").and_then(|v| v.as_i64()) == Some(1)
                    && obj.get("leases").and_then(|v| v.as_array()).map(|a| a.is_empty()).unwrap_or(false)
                    && obj.get("createdAt").and_then(|v| v.as_str()).is_some()
                    && obj.get("updatedAt").and_then(|v| v.as_str()).is_some()
            })
            .unwrap_or(false);
        if conforming {
            leases_note = json!({"action": "verified-already-conforming"});
        } else {
            let sha = manifest::sha256_hex(&bytes);
            std::fs::create_dir_all(&archive_dir).map_err(|e| format!("mkdir archive dir: {e}"))?;
            let archive_path = archive_dir.join("execution-leases.json.orig");
            paths::atomic_write_private(&archive_path, &bytes)
                .map_err(|e| format!("archive execution-leases: {e}"))?;
            let reread = sha256_file(&archive_path)?;
            if reread != sha {
                return Err(format!(
                    "archive verification failed: {} sha256 {} != original {}",
                    archive_path.display(),
                    reread,
                    sha
                ));
            }
            let now_iso = iso_now();
            let payload = format!(
                "{}\n",
                serde_json::to_string_pretty(&json!({
                    "schemaVersion": 1,
                    "leases": [],
                    "createdAt": now_iso,
                    "updatedAt": now_iso,
                }))
                .map_err(|e| format!("serialize leases registry: {e}"))?
            );
            paths::atomic_write_private(&args.old_root.join("security/execution-leases.json"), payload.as_bytes())
                .map_err(|e| format!("rewrite execution-leases: {e}"))?;
            let after = sha256_file(&args.old_root.join("security/execution-leases.json"))?;
            leases_note = json!({"action": "rewritten-empty-registry", "beforeSha256": sha, "afterSha256": after,
                "archivedTo": format!("{OLD_ROOT_PREPARE_DIRNAME}/execution-leases.json.orig")});
        }
        writes.push(json!({"relpath": "security/execution-leases.json", "note": leases_note.clone()}));
    }
    // ── J9② agents/{agentId}/config.yaml 最小有效配置（封闭修复集）。──
    // 无 config.yaml 的 agent 目录会被旧栈 first-run 判无效并另建默认助手
    // lingxi（掏空探针断言⑤「旧 UI 可读范围=旧根既有集」的取证价值）。
    // 修复=按生产模板补最小键（lib/config.example.yaml L7-9 agent 块 +
    // core/first-run.ts:181-199 seedDefaultAgent 播种链；agent.name/agent.yuan，
    // 0600）。name 取 agentId 大写（夹具 a1 → "A1"，与控制探针验证字节同构）。
    // 生产诚实路径：已有 config.yaml → 零写（resume 幂等）。
    let mut agent_configs_created: Vec<Value> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(args.old_root.join("agents")) {
        let mut agent_ids: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        agent_ids.sort();
        for agent_id in agent_ids {
            let config_path = args.old_root.join("agents").join(&agent_id).join("config.yaml");
            if config_path.exists() {
                continue;
            }
            let name = agent_id.to_uppercase();
            let content = format!(
                "# R08-T06 J9 夹具同构修复：形状引生产模板 lib/config.example.yaml L7-9\n\
                 # + core/first-run.ts:181-199 seedDefaultAgent 播种链；仅 agent 块最小键。\n\
                 agent:\n  name: \"{name}\"\n  yuan: \"lingxi\"\n"
            );
            paths::atomic_write_private(&config_path, content.as_bytes())
                .map_err(|e| format!("write minimal agent config {}: {e}", config_path.display()))?;
            let sha = sha256_file(&config_path)?;
            agent_configs_created.push(json!({
                "relpath": format!("agents/{agent_id}/config.yaml"),
                "action": "created-minimal",
                "afterSha256": sha,
            }));
        }
    }
    for entry in &agent_configs_created {
        writes.push(entry.clone());
    }
    // ── ⑤a knowledge.db 合成表 RENAME 保序（封闭修复集，单事务 + integrity）。──
    // 夹具合成表 documents/processing_artifacts 与生产 v16 表同名但列形为合成
    // 最小形；迁移链 CREATE TABLE IF NOT EXISTS 跳过建表后引用缺失列
    // content_snapshot_id 而崩。判别式：documents 表存在即合成（生产全 schema
    // 从不建该表）；processing_artifacts 缺 content_snapshot_id 列即合成。
    // RENAME 保序（行零丢失、字节仍在库内）→ 迁移链自建生产空表。纪律：原库
    // 字节先归档 + sha256 复验；RENAME 单事务；术后 PRAGMA integrity_check；
    // prepareWrites/readable-scope 披露「knowledge 视图为空」。生产诚实路径：
    // 无合成表 → verify-only 零写（resume 幂等：改名后判别式不再命中）。
    let knowledge_note;
    let knowledge_path = args.old_root.join("knowledge/knowledge.db");
    if knowledge_path.exists() {
        let original_bytes = std::fs::read(&knowledge_path)
            .map_err(|e| format!("read knowledge.db: {e}"))?;
        let original_sha = manifest::sha256_hex(&original_bytes);
        let synth = detect_synthetic_knowledge_tables(&knowledge_path)?;
        if synth.is_empty() {
            knowledge_note = json!({"action": "verified-no-synthetic-tables"});
        } else {
            std::fs::create_dir_all(&archive_dir).map_err(|e| format!("mkdir archive dir: {e}"))?;
            let archive_path = archive_dir.join("knowledge.db.orig");
            paths::atomic_write_private(&archive_path, &original_bytes)
                .map_err(|e| format!("archive knowledge.db: {e}"))?;
            let reread = sha256_file(&archive_path)?;
            if reread != original_sha {
                return Err(format!(
                    "archive verification failed: {} sha256 {} != original {}",
                    archive_path.display(),
                    reread,
                    original_sha
                ));
            }
            let (renamed, preserved_rows, integrity) = rename_synthetic_knowledge_tables(&knowledge_path, &synth)?;
            let after_sha = sha256_file(&knowledge_path)?;
            knowledge_note = json!({
                "action": "renamed-synthetic-tables",
                "beforeSha256": original_sha,
                "afterSha256": after_sha,
                "archivedTo": format!("{OLD_ROOT_PREPARE_DIRNAME}/knowledge.db.orig"),
                "renamed": renamed,
                "preservedRows": preserved_rows,
                "integrityCheck": integrity,
                "disclosure": "knowledge 视图为空（合成表已改名保序，行零丢失）",
            });
        }
        writes.push(json!({"relpath": "knowledge/knowledge.db", "note": knowledge_note.clone()}));
    }
    Ok(vec![
        ("oldRootStampBeforeSha256".into(), json!(before_sha)),
        ("oldRootStampAfterSha256".into(), json!(after_sha)),
        ("prepareWrites".into(), Value::Array(writes)),
        // 封闭修复集披露（J9 + ⑤a 裁决）：集外新阻断 = 停手上报，不扩集。
        (
            "prepareFixSetClosure".into(),
            json!({
                "closedSet": [
                    "data-epoch.json 印章重写 v2{1,1,1}",
                    "data-epoch-transition.json 归档移除",
                    "security/execution-leases.json → 生产空注册表形状",
                    "agents/{agentId}/config.yaml 最小有效配置",
                    "knowledge/knowledge.db 合成表 RENAME fixture_synthetic_*",
                ],
                "rule": "集外新阻断停手上报，不得自行扩集（J9 裁决）",
            }),
        ),
    ])
}

/// ⑤a 判别式：返回需改名的合成表名列表。`documents` 表存在即合成（生产
/// 全 schema 无此表）；`processing_artifacts` 存在且缺 content_snapshot_id
/// 列即合成（生产 v16 该列 NOT NULL）。只读打开。
fn detect_synthetic_knowledge_tables(db_path: &std::path::Path) -> Result<Vec<String>, String> {
    // P6 纪律（T06 评审 N3 移交）：判别只读副本——READ_ONLY 直开 WAL 模式库会在
    // 旧根静默建 -shm/-wal；准备相的判别步骤对旧根必须零触碰（RENAME 步骤才是
    // J1 授权的唯一写入点）。
    let staged = rollback_delta::stage_quiesced_copy(db_path)
        .map_err(|e| format!("stage knowledge.db copy for detection: {e}"))?;
    let result = detect_synthetic_knowledge_tables_in(&staged);
    let _ = std::fs::remove_dir_all(staged.parent().unwrap_or(std::path::Path::new("/nonexistent")));
    result
}

fn detect_synthetic_knowledge_tables_in(db_path: &std::path::Path) -> Result<Vec<String>, String> {
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| format!("open knowledge.db copy read-only: {e}"))?;
    let table_exists = |name: &str| -> Result<bool, String> {
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .map(|n| n > 0)
        .map_err(|e| format!("sqlite_master probe {name}: {e}"))
    };
    let column_exists = |table: &str, column: &str| -> Result<bool, String> {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .map_err(|e| format!("table_info {table}: {e}"))?;
        let mut names = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| format!("table_info rows {table}: {e}"))?;
        while let Some(row) = names.next() {
            let name = row.map_err(|e| format!("table_info step {table}: {e}"))?;
            if name == column {
                return Ok(true);
            }
        }
        Ok(false)
    };
    let mut synthetic = Vec::new();
    if table_exists("documents")? {
        synthetic.push("documents".to_string());
    }
    if table_exists("processing_artifacts")? && !column_exists("processing_artifacts", "content_snapshot_id")? {
        synthetic.push("processing_artifacts".to_string());
    }
    Ok(synthetic)
}

/// ⑤a 执行：单事务 RENAME fixture_synthetic_* + 行数保全记录 + integrity_check。
/// 返回 (renamed pairs, preserved row counts, integrity result)。
fn rename_synthetic_knowledge_tables(
    db_path: &std::path::Path,
    synthetic: &[String],
) -> Result<(Vec<(String, String)>, std::collections::BTreeMap<String, i64>, String), String> {
    let conn = rusqlite::Connection::open(db_path)
        .map_err(|e| format!("open knowledge.db read-write: {e}"))?;
    conn.execute_batch("BEGIN")
        .map_err(|e| format!("begin rename transaction: {e}"))?;
    let mut renamed: Vec<(String, String)> = Vec::new();
    let result = (|| -> Result<std::collections::BTreeMap<String, i64>, String> {
        let mut preserved = std::collections::BTreeMap::new();
        for from in synthetic {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM \"{from}\""), [], |row| row.get(0))
                .map_err(|e| format!("count {from}: {e}"))?;
            let to = format!("fixture_synthetic_{from}");
            conn.execute_batch(&format!("ALTER TABLE \"{from}\" RENAME TO \"{to}\""))
                .map_err(|e| format!("rename {from}: {e}"))?;
            let after: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM \"{to}\""), [], |row| row.get(0))
                .map_err(|e| format!("count {to}: {e}"))?;
            if after != count {
                return Err(format!("row loss on rename {from}: {count} -> {after}"));
            }
            preserved.insert(to.clone(), after);
            renamed.push((from.clone(), to));
        }
        conn.execute_batch("COMMIT")
            .map_err(|e| format!("commit rename transaction: {e}"))?;
        Ok(preserved)
    })();
    let preserved = match result {
        Ok(p) => p,
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(format!("synthetic table rename failed (rolled back): {e}"));
        }
    };
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|e| format!("integrity_check: {e}"))?;
    if integrity != "ok" {
        return Err(format!("integrity_check after rename: {integrity}"));
    }
    Ok((renamed, preserved, integrity))
}

/// UTC ISO-8601 毫秒精度（与 Node new Date().toISOString() 同形）。
fn iso_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// 相 7：pointer_switch —— 原子 rename + 读回比对（commit_pointer 自带）。
fn rb_pointer_switch(args: &RollbackArgs, journal: &RollbackJournal) -> Result<Vec<(String, Value)>, String> {
    // 切换前断言：旧根印章必须为已备形态（双保险，防 resume 乱序）。
    match epoch::read_stamp(&args.old_root) {
        StampRead::Ok(ref s)
            if s.format == StampFormat::V2
                && s.minimum_reader_epoch == 1
                && s.committed_data_epoch == 1 => {}
        other => {
            return Err(format!(
                "old root stamp not in prepared v2 epoch-1 form at pointer_switch: {other:?}"
            ));
        }
    }
    let before = pointer::read_pointer(&args.pointer)
        .map_err(|e| format!("read pointer: {e}"))?
        .map(|p| p.active_data_root)
        .unwrap_or_else(|| "<absent>".to_string());
    let committed = pointer::commit_pointer(&args.pointer, &args.old_root, &journal.id)
        .map_err(|e| format!("commit pointer: {e}"))?;
    Ok(vec![(
        "pointer".into(),
        json!({"before": before, "after": committed.active_data_root, "cutoverId": committed.cutover_id}),
    )])
}

/// 相 8：old_stack_probe —— 真旧栈冷启动断言组（J6：失败 → 指针回指新根）。
async fn rb_old_stack_probe(args: &RollbackArgs, journal: &RollbackJournal) -> Result<Vec<(String, Value)>, String> {
    let probe_result = old_stack_probe_inner(args, journal).await;
    match probe_result {
        Ok(report) => Ok(vec![("probe".into(), serde_json::to_value(&report).unwrap_or(Value::Null))]),
        Err(reason) => {
            // J6：指针回指新根（恢复已知良好态），然后以 refused 终态上报。
            let restore = pointer::commit_pointer(
                &args.pointer,
                &args.new_root,
                &format!("rb-restore-{}", journal.id),
            );
            let restore_note = match &restore {
                Ok(p) => format!("pointer restored to new root ({})", p.active_data_root),
                Err(e) => format!("POINTER RESTORE ALSO FAILED: {e} — pointer state unknown, inspect manually"),
            };
            Err(format!("old stack probe failed: {reason}; {restore_note}"))
        }
    }
}

async fn old_stack_probe_inner(args: &RollbackArgs, _journal: &RollbackJournal) -> Result<OldStackProbeReport, String> {
    // 自由环回端口：bind :0 取号即放（竞态可接受，失败响亮）。
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .map_err(|e| format!("bind probe port: {e}"))?;
        listener
            .local_addr()
            .map_err(|e| format!("probe port addr: {e}"))?
            .port()
    };
    // 前置快照：新根封缄 + 旧根 manifest（写集比对基线）+ 印章字节（裁决①
    // 证据列：四字段冻结断言需前后内容，非仅哈希）。
    let new_before = manifest::source_root_sha256(
        &stillpoint::scan_pass(&args.new_root).map_err(|e| format!("scan new root: {e}"))?,
    );
    let old_before = stillpoint::scan_pass(&args.old_root).map_err(|e| format!("scan old root: {e}"))?;
    let epoch_before = std::fs::read_to_string(args.old_root.join(EPOCH_STAMP_RELPATH)).ok();
    // 冷启动旧栈：LINGXI_HOME=old_root 直给（旧 Node 栈不读 active-data-root.json
    // —— shared/ 与 server/ 零引用，设计 §7.2 已核）。
    let mut cmd = std::process::Command::new("node");
    cmd.arg(&args.old_stack_launcher)
        .arg("server")
        .current_dir(&args.old_stack_cwd)
        .env("LINGXI_HOME", &args.old_root)
        .env("LINGXI_PORT", port.to_string())
        .env_remove("ELECTRON_RUN_AS_NODE")
        .env_remove("LINGXI_TOKEN")
        .env_remove("LINGXI_SERVER_OWNER")
        .env_remove("LINGXI_SERVER_OWNER_PID")
        .env_remove("LINGXI_ALLOW_DATA_DOWNGRADE")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("spawn old stack: {e}"))?;
    let pid = child.id();
    let stderr_pipe = child.stderr.take().ok_or("stderr pipe missing")?;
    let stderr_handle = std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = String::new();
        let mut reader = std::io::BufReader::new(stderr_pipe);
        let _ = reader.read_to_string(&mut buf);
        buf
    });
    // 就绪轮询：server-info.json 端口匹配 + /api/health 200（Bearer 鉴权，
    // token 取自 server-info.json —— route-security.ts L58；503 引导相免鉴权
    // 见 server/index.ts L389。设计文本的“环回 HTTP 应答”不变量以此既有端点落实）。
    let server_info_path = args.old_root.join("server-info.json");
    let started = Instant::now();
    let mut server_info: Option<Value> = None;
    let mut http_status = 0u16;
    loop {
        if let Some(status) = child.try_wait().map_err(|e| format!("poll child: {e}"))? {
            let stderr = stderr_handle.join().unwrap_or_default();
            return Err(format!(
                "old stack exited before readiness (status {status}, server_info_seen={}, last_http_status={http_status}); stderr tail: {}",
                server_info.is_some(),
                tail_lines(&stderr, 40)
            ));
        }
        if started.elapsed() > PROBE_READY_BUDGET {
            terminate_old_stack(&mut child, server_info_pid(&server_info));
            let stderr = stderr_handle.join().unwrap_or_default();
            return Err(format!(
                "old stack not ready within {:?} (server_info_seen={}, last_http_status={http_status}); stderr tail: {}",
                PROBE_READY_BUDGET,
                server_info.is_some(),
                tail_lines(&stderr, 40)
            ));
        }
        if server_info.is_none() {
            if let Ok(raw) = std::fs::read_to_string(&server_info_path) {
                if let Ok(info) = serde_json::from_str::<Value>(&raw) {
                    if info.get("port").and_then(|v| v.as_u64()) == Some(port as u64)
                        && info.get("token").and_then(|v| v.as_str()).is_some()
                    {
                        server_info = Some(info);
                    }
                }
            }
        }
        if let Some(info) = &server_info {
            let token = info.get("token").and_then(|v| v.as_str());
            match http_get_status(port, "/api/health", Duration::from_secs(3), token) {
                Ok(200) => {
                    http_status = 200;
                    break;
                }
                Ok(other) => http_status = other,
                Err(_) => {}
            }
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    // 正常关停：SIGTERM **server-info.json 的 owned-pid**（launch.js 不转发信号，
    // 杀 launcher 会孤儿 server —— t06-control-probe-03 取证确认）；预算内退出
    // 否则 SIGKILL 兜底。杀前刚验过 /api/health 200（所杀即所答）。
    terminate_old_stack(&mut child, server_info_pid(&server_info));
    let shutdown_deadline = Instant::now() + PROBE_SHUTDOWN_BUDGET;
    let shutdown_clean = loop {
        if let Some(_status) = child.try_wait().map_err(|e| format!("wait child: {e}"))? {
            break true;
        }
        if Instant::now() > shutdown_deadline {
            // 预算耗尽：双边 SIGKILL（owned-pid 优先），绝不孤儿 server。
            if let Some(pid) = server_info_pid(&server_info) {
                let _ = std::process::Command::new("kill")
                    .arg("-KILL")
                    .arg(pid.to_string())
                    .status();
            }
            let _ = child.kill();
            let _ = child.wait();
            break false;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let stderr = stderr_handle.join().unwrap_or_default();
    // 断言组（§7.2）：三条 epoch 门标记绝不允许出现。
    let stderr_markers_clean = !PROBE_FORBIDDEN_STDERR_MARKERS
        .iter()
        .any(|m| stderr.contains(m));
    // 新根零变化（无排除项 —— 全树哈希比对）。
    let new_after = manifest::source_root_sha256(
        &stillpoint::scan_pass(&args.new_root).map_err(|e| format!("rescan new root: {e}"))?,
    );
    let new_root_unchanged = new_before == new_after;
    // 旧根写集白名单比对（证据派生白名单 + 裁决①印章条款）。
    let old_after = stillpoint::scan_pass(&args.old_root).map_err(|e| format!("rescan old root: {e}"))?;
    let epoch_after = std::fs::read_to_string(args.old_root.join(EPOCH_STAMP_RELPATH)).ok();
    let (allowed, forbidden, epoch_stamp_evidence) =
        classify_write_set(&old_before, &old_after, epoch_before.as_deref(), epoch_after.as_deref());
    let report = OldStackProbeReport {
        port,
        pid,
        server_info_seen: server_info.is_some(),
        http_status,
        stderr_markers_clean,
        new_root_unchanged,
        old_root_write_set_allowed: allowed,
        old_root_write_set_forbidden: forbidden,
        epoch_stamp_evidence,
        shutdown_clean,
    };
    if !report.stderr_markers_clean {
        return Err(format!(
            "forbidden data-epoch marker in old stack stderr; tail: {}",
            tail_lines(&stderr, 40)
        ));
    }
    if !report.new_root_unchanged {
        return Err("new root changed during old stack probe (zero-exclusion hash compare)".to_string());
    }
    if !report.old_root_write_set_forbidden.is_empty() {
        return Err(format!(
            "old root write set outside whitelist: {:?}",
            report.old_root_write_set_forbidden
        ));
    }
    if !report.shutdown_clean {
        return Err("old stack did not shut down within budget (SIGKILL fallback used)".to_string());
    }
    Ok(report)
}

/// server-info.json 中的 owned-pid（真 server 进程；launch.js 是其父、不转发信号）。
fn server_info_pid(server_info: &Option<Value>) -> Option<u64> {
    server_info
        .as_ref()
        .and_then(|info| info.get("pid"))
        .and_then(|v| v.as_u64())
}

/// 终止纪律：优先 SIGTERM server-info.json 的 owned-pid（真 server），
/// 再 SIGTERM launcher（其随 grandchild 退出而自退，此调用是兜底）；
/// launch.js 不转发信号（t06-control-probe-03 取证），绝不允许只杀 launcher。
fn terminate_old_stack(child: &mut std::process::Child, owned_pid: Option<u64>) {
    if let Some(pid) = owned_pid {
        let _ = std::process::Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .status();
    }
    let _ = std::process::Command::new("kill")
        .arg("-TERM")
        .arg(child.id().to_string())
        .status();
}

fn tail_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

/// 相 9：写回滚收据（t06-rollback-receipt/v1）。
fn rb_write_receipt(args: &RollbackArgs, journal: &RollbackJournal) -> Result<Vec<(String, Value)>, String> {
    let f = &journal.facts;
    let get = |k: &str| -> Result<Value, String> {
        f.get(k)
            .cloned()
            .ok_or_else(|| format!("journal fact {k:?} missing for receipt"))
    };
    let receipt = json!({
        "schema": ROLLBACK_RECEIPT_SCHEMA,
        "rollbackId": journal.id,
        "generatedAt": now_iso(),
        "lineage": {
            "sourceCutoverId": get("sourceCutoverId")?,
            "forwardMigrationReceipt": get("forwardMigrationReceipt")?,
        },
        "pointer": get("pointer")?,
        "oldRoot": {
            "path": args.old_root.display().to_string(),
            "stampBeforeSha256": get("oldRootStampBeforeSha256")?,
            "stampAfterSha256": get("oldRootStampAfterSha256")?,
            "prepareWrites": get("prepareWrites")?,
        },
        "newRoot": {
            "path": args.new_root.display().to_string(),
            "sha256ManifestRef": get("newRootSealHash")?,
            "sealed": true,
            "backup": get("newRootBackup")?,
        },
        "deltaPackage": get("deltaPackage")?,
        "exportVerify": get("exportVerify")?,
        "probe": get("probe")?,
        "hashConvention": HASH_CONVENTION_NOTE,
        "phaseHistory": serde_json::to_value(&journal.history).unwrap_or(Value::Null),
    });
    let receipt_path = args.work_dir.join("rollback-receipt.json");
    let mut body = serde_json::to_string_pretty(&receipt).map_err(|e| format!("serialize receipt: {e}"))?;
    body.push('\n');
    paths::atomic_write_private(&receipt_path, body.as_bytes())
        .map_err(|e| format!("write receipt {}: {e}", receipt_path.display()))?;
    let sha = sha256_file(&receipt_path)?;
    Ok(vec![(
        "receipt".into(),
        json!({"path": receipt_path.display().to_string(), "sha256": sha}),
    )])
}

/// rollback 相位修复指引（refusal.repairAction）。
fn rb_repair_action(phase: &str, args: &RollbackArgs, reason: &str) -> Value {
    let base = json!({
        "journal": RollbackJournal::path(&args.work_dir, "rollback").display().to_string(),
        "resume": format!("lingxi-service rollback --resume <id> （修正前置后续跑；refused 终态不可续跑，需人工裁决）"),
        "bothSidesPreserved": true,
    });
    let specific = match phase {
        "preflight" => format!("修正 preflight 列出的前置后重跑：{reason}"),
        "stop_new_writer" => "停掉活写者（新根 Rust 实例/旧栈）后重跑".to_string(),
        "backup_new_root" => "双侧保留；释放备份卷空间或修 IO 后重跑（备份半成品已自清）".to_string(),
        "delta_export" => "双侧保留；排查导出错误后重跑（包半成品已自清）".to_string(),
        "export_verify" => format!(
            "包不可信：删除 {} 后重跑以重新导出；双侧保留",
            args.delta_out.display()
        ),
        "old_root_prepare" => format!(
            "原始件已归档于增量包 {OLD_ROOT_PREPARE_DIRNAME}/ 子目录；按归档恢复或修正后重跑：{reason}"
        ),
        "pointer_switch" => "指针原子 rename 失败：检查 --pointer 路径与文件系统；双侧根未动".to_string(),
        "old_stack_probe" => "指针已回指新根（新根恢复为活跃根）；旧根保留；按 probe 失败详情修复后重跑".to_string(),
        _ => "见 journal refusal.reason；双侧保留".to_string(),
    };
    json!({ "action": specific, "context": base })
}

// ── rollback：状态机 ───────────────────────────────────────────

pub async fn run_rollback(args: &RollbackArgs) -> Result<RollbackOutcome, VerbFailure> {
    let inputs = json!({
        "pointer": args.pointer.display().to_string(),
        "oldRoot": args.old_root.display().to_string(),
        "newRoot": args.new_root.display().to_string(),
        "workDir": args.work_dir.display().to_string(),
        "backupDir": args.backup_dir.display().to_string(),
        "deltaOut": args.delta_out.display().to_string(),
        "oldStackLauncher": args.old_stack_launcher.display().to_string(),
        "oldStackCwd": args.old_stack_cwd.display().to_string(),
    });
    std::fs::create_dir_all(&args.work_dir)
        .map_err(|e| verb_err(2, format!("error: rollback: mkdir work-dir: {e}")))?;
    let (mut journal, prior) = open_journal(
        &args.work_dir,
        "rollback",
        "rb",
        inputs,
        &ROLLBACK_PHASES,
        &args.resume,
    )
    .map_err(|e| verb_err(2, format!("error: rollback: {e}")))?;
    if prior == Some("already-committed") {
        return Ok(RollbackOutcome {
            id: journal.id.clone(),
            phase: "committed".to_string(),
            outcome: "already-committed",
            detail: "rollback already committed; nothing to do".to_string(),
        });
    }
    loop {
        let phase = journal.phase.clone();
        if phase == "committed" {
            let facts = rb_write_receipt(args, &journal).map_err(|e| {
                verb_err(2, format!("error: rollback: write receipt: {e}"))
            })?;
            for (k, v) in facts {
                journal.facts.insert(k, v);
            }
            journal = journal
                .commit(&args.work_dir)
                .map_err(|e| verb_err(2, format!("error: rollback: journal commit: {e}")))?;
            return Ok(RollbackOutcome {
                id: journal.id.clone(),
                phase: "committed".to_string(),
                outcome: "committed",
                detail: format!(
                    "rollback committed; receipt {}",
                    journal
                        .fact("receipt")
                        .ok()
                        .and_then(|r| r.get("path"))
                        .and_then(|p| p.as_str())
                        .unwrap_or("?")
                ),
            });
        }
        let result = match phase.as_str() {
            "preflight" => rb_preflight(args),
            "stop_new_writer" => rb_stop_new_writer(args),
            "backup_new_root" => rb_backup_new_root(args, &journal),
            "delta_export" => rb_delta_export(args, &journal),
            "export_verify" => rb_export_verify(args, &journal),
            "old_root_prepare" => rb_old_root_prepare(args, &journal),
            "pointer_switch" => rb_pointer_switch(args, &journal),
            "old_stack_probe" => rb_old_stack_probe(args, &journal).await,
            other => Err(format!("unknown phase {other:?} in journal")),
        };
        let facts = match result {
            Ok(facts) => facts,
            Err(reason) => {
                let repair = rb_repair_action(&phase, args, &reason);
                journal
                    .refuse(&args.work_dir, &phase, &reason, repair)
                    .map_err(|e| verb_err(2, format!("error: rollback: journal refuse: {e}")))?;
                return Err(verb_err(
                    2,
                    format!("error: rollback: phase {phase} refused: {reason}"),
                ));
            }
        };
        for (k, v) in facts {
            journal.facts.insert(k, v);
        }
        let idx = ROLLBACK_PHASES.iter().position(|p| p == &phase).unwrap_or(0);
        let next = ROLLBACK_PHASES[idx + 1];
        journal = journal
            .advance(&args.work_dir, next, json!({"completedPhase": phase}))
            .map_err(|e| verb_err(2, format!("error: rollback: journal advance: {e}")))?;
        if args.stop_after_phase.as_deref() == Some(phase.as_str()) {
            journal = journal
                .stop(&args.work_dir)
                .map_err(|e| verb_err(2, format!("error: rollback: journal stop: {e}")))?;
            return Ok(RollbackOutcome {
                id: journal.id.clone(),
                phase: phase.clone(),
                outcome: "stopped",
                detail: format!("clean stop after phase {phase} (--stop-after-phase)"),
            });
        }
    }
}

// ── reforward：相位实现 ────────────────────────────────────────

/// 相 1（A/B 共干 + 模态分支）：preflight。
fn rf_preflight(args: &ReforwardArgs, mode_b: bool) -> Result<Vec<(String, Value)>, String> {
    if !args.old_root.is_dir() {
        return Err(format!("old root {} is not a directory", args.old_root.display()));
    }
    // 指针必须在旧根上（reforward 的前提是 rollback 已切走）。
    let pointer = pointer::read_pointer(&args.pointer)
        .map_err(|e| format!("read pointer: {e}"))?
        .ok_or_else(|| format!("pointer {} does not exist", args.pointer.display()))?;
    let old_canon = canonical(&args.old_root, "old root")?;
    let pointed_canon = canonical(
        &PathBuf::from(&pointer.active_data_root),
        "pointer activeDataRoot",
    )?;
    if pointed_canon != old_canon {
        return Err(format!(
            "pointer activeDataRoot {} != old root {} (reforward requires the pointer to be on the old root)",
            pointer.active_data_root,
            args.old_root.display()
        ));
    }
    // 旧根印章必须为 rollback prepare 后的 v2 epoch-1 形态。
    match epoch::read_stamp(&args.old_root) {
        StampRead::Ok(ref s)
            if s.format == StampFormat::V2
                && s.minimum_reader_epoch == 1
                && s.committed_data_epoch == 1 => {}
        other => {
            return Err(format!(
                "old root stamp is not the prepared v2 epoch-1 form: {other:?} (rollback prepare incomplete?)"
            ));
        }
    }
    // 只读消费 rollback journal：必须存在且 committed。
    let rb = RollbackJournal::load(&args.work_dir, "rollback")?
        .ok_or_else(|| format!("no rollback journal in {} (run rollback first)", args.work_dir.display()))?;
    if rb.status != "committed" {
        return Err(format!(
            "rollback {} is not committed (status {}); reforward requires a committed rollback",
            rb.id, rb.status
        ));
    }
    // 旧栈写者必须已停。
    let legacy = probe::probe_legacy_server_info(&args.old_root)
        .map_err(|e| format!("probe legacy server info: {e}"))?;
    if legacy.probe.is_blocking() {
        return Err(format!("old root has a live legacy stack: {}", legacy.detail));
    }
    let mut facts = vec![
        ("rollbackId".into(), json!(rb.id)),
        ("sourceCutoverId".into(), rb.fact("sourceCutoverId").cloned().unwrap_or(Value::Null)),
        ("newRootSealHash".into(), rb.fact("newRootSealHash").cloned().unwrap_or(Value::Null)),
        ("deltaPackage".into(), rb.fact("deltaPackage").cloned().unwrap_or(Value::Null)),
        ("rollbackReceipt".into(), rb.fact("receipt").cloned().unwrap_or(Value::Null)),
        ("pointerBefore".into(), json!(pointer.active_data_root)),
    ];
    if mode_b {
        // mode B 专属前置：包/备份目录/目标根。
        let delta_package = args
            .delta_package
            .clone()
            .ok_or("--recover-new-root requires --delta-package <dir>")?;
        if args.backup_dir.is_none() {
            return Err("--recover-new-root requires --backup-dir <dir>".to_string());
        }
        let (pkg, _t, _f) = rollback_delta::read_delta_package(&delta_package)
            .map_err(|e| format!("delta package fails self-verification: {e}"))?;
        // lineage 绑定：包必须属于本 work-dir 的这次 rollback。
        let lineage_raw = std::fs::read_to_string(delta_package.join("lineage.json"))
            .map_err(|e| format!("read package lineage.json: {e}"))?;
        let lineage: Value = serde_json::from_str(&lineage_raw)
            .map_err(|e| format!("parse lineage.json: {e}"))?;
        let lineage_rb = lineage.get("rollbackId").and_then(|v| v.as_str()).unwrap_or("");
        if lineage_rb != rb.id {
            return Err(format!(
                "delta package lineage rollbackId {lineage_rb:?} != committed rollback {:?}",
                rb.id
            ));
        }
        let target = args
            .target
            .clone()
            .unwrap_or_else(|| args.work_dir.join("recovered-root"));
        if target.exists() {
            let mut entries = std::fs::read_dir(&target)
                .map_err(|e| format!("read target {}: {e}", target.display()))?;
            if entries.next().is_some() {
                return Err(format!(
                    "recovery target {} already exists and is not empty (pass a fresh --target)",
                    target.display()
                ));
            }
        }
        facts.push(("deltaPackageSha256".into(), json!(pkg.package_sha256)));
        facts.push(("targetRoot".into(), json!(target.display().to_string())));
        facts.push(("mode".into(), json!("recovery-import")));
    } else {
        // mode A 专属前置：新根必须存在且无活写者。
        if !args.new_root.is_dir() {
            return Err(format!(
                "new root {} is not a directory (mode A requires the sealed new root)",
                args.new_root.display()
            ));
        }
        let rust_lock = probe::probe_rust_instance_lock(&args.new_root)
            .map_err(|e| format!("probe rust instance lock: {e}"))?;
        if matches!(rust_lock.probe, probe::RustLockProbe::Held) {
            return Err(format!("new root has a live Rust writer: {}", rust_lock.lock_path));
        }
        facts.push(("mode".into(), json!("fast-repoint")));
    }
    Ok(facts)
}

/// 相 2A：seal_verify —— 新根封缄哈希必须与 rollback 收据一致。
fn rf_a_seal_verify(args: &ReforwardArgs, journal: &RollbackJournal) -> Result<Vec<(String, Value)>, String> {
    let seal = journal.fact_str("newRootSealHash")?;
    let scan = stillpoint::scan_pass(&args.new_root).map_err(|e| format!("scan new root: {e}"))?;
    let current = manifest::source_root_sha256(&scan);
    if current != seal {
        return Err(format!(
            "new root drifted since rollback: sealed {seal}, now {current} — use --recover-new-root (mode B) instead"
        ));
    }
    Ok(vec![(
        "sealVerify".into(),
        json!({"sealHashMatched": true, "verifiedHash": current}),
    )])
}

/// 相 2B：backup_old_root —— 复用 T04 run_backup（旧根 = legacy 布局，类表面适用）。
fn rf_b_backup_old_root(args: &ReforwardArgs, journal: &RollbackJournal) -> Result<Vec<(String, Value)>, String> {
    let backup_dir = args.backup_dir.clone().ok_or("backup-dir missing")?;
    std::fs::create_dir_all(&backup_dir).map_err(|e| format!("mkdir backup-dir: {e}"))?;
    let sf = stillpoint::establish(&args.old_root, &args.work_dir)
        .map_err(|e| format!("establish stillpoint: {e}"))?;
    let source_manifest = stillpoint::scan_pass(&args.old_root)
        .map_err(|e| format!("scan old root: {e}"))?;
    backup::run_backup(
        &args.old_root,
        &backup_dir,
        &args.work_dir,
        &journal.id,
        &sf.stores,
        &source_manifest,
    )
    .map_err(|e| format!("run backup: {e}"))?;
    stillpoint::verify_post_backup(&args.old_root, &sf)
        .map_err(|e| format!("verify post backup: {e}"))?;
    let checkpoint_dir = backup::backup_root_for(&backup_dir, &journal.id);
    let manifest_path = checkpoint_dir.join(backup::BACKUP_MANIFEST_FILE);
    let raw = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("read backup manifest {}: {e}", manifest_path.display()))?;
    let backup_manifest_sha = manifest::sha256_hex(raw.as_bytes());
    // N4 双钉（附加钉①）：checkpoint_receipt["id"] == checkpoint_id，与
    // orchestrator build_adoption_evidence 同款注入。
    let checkpoint_id = format!("r08-reforward-{}", journal.id);
    let mut checkpoint_receipt: Value =
        serde_json::from_str(&raw).map_err(|e| format!("parse backup manifest: {e}"))?;
    checkpoint_receipt["id"] = json!(checkpoint_id);
    let stillpoint_path = stillpoint::stillpoint_path(&args.work_dir);
    let stillpoint_sha = sha256_file(&stillpoint_path)?;
    let affected: Vec<String> = sf.stores.iter().map(|s| s.class_id.clone()).collect();
    // source_identity 用 manifest 域（内部双哈希约定）——与 T05 epoch 证据同域，
    // 属内部自洽比对，非人读内容哈希（哈希披露义务在 runbook）。
    let source_identity = manifest::source_root_sha256(&source_manifest);
    Ok(vec![
        (
            "oldRootBackup".into(),
            json!({
                "path": checkpoint_dir.display().to_string(),
                "manifestSha256": backup_manifest_sha,
            }),
        ),
        ("checkpointId".into(), json!(checkpoint_id)),
        ("checkpointReceipt".into(), checkpoint_receipt),
        ("backupManifestId".into(), json!(backup_manifest_sha)),
        ("stillpointManifestSha256".into(), json!(stillpoint_sha)),
        ("sourceIdentitySha256".into(), json!(source_identity)),
        ("affectedStoreIds".into(), json!(affected)),
    ])
}

/// 相 3B：import —— 复用 T04 run_import（旧根 → 恢复根）。
async fn rf_b_import(args: &ReforwardArgs, journal: &RollbackJournal) -> Result<Vec<(String, Value)>, String> {
    let target = PathBuf::from(journal.fact_str("targetRoot")?);
    if target.exists() && journal.attempts > 1 {
        // 续跑幂等：目标为本 drill 新建产物（preflight 保证首轮不存在），
        // 半成品 = 本动词在途输出，清除重导并留痕。
        std::fs::remove_dir_all(&target)
            .map_err(|e| format!("remove partial recovery target {}: {e}", target.display()))?;
    }
    std::fs::create_dir_all(&target).map_err(|e| format!("mkdir target: {e}"))?;
    let _target_lock = legacy_import_runner::acquire_target_lock(&target)
        .map_err(|(code, line)| format!("acquire target lock (exit {code}): {line}"))?;
    let outcome = legacy_import_runner::run_import(ImportArgs {
        source: args.old_root.clone(),
        target: target.clone(),
        resume: None,
        stop_after_batch: None,
        instance_lock_held: true,
    })
    .await
    .map_err(|vf| vf.line)?;
    legacy_import_runner::print_outcome(&outcome);
    if outcome.status != "complete" || outcome.interrupted {
        return Err(format!(
            "import did not complete (status {}, interrupted={}); see receipt {}",
            outcome.status,
            outcome.interrupted,
            outcome.receipt_path.display()
        ));
    }
    Ok(vec![
        ("importRunId".into(), json!(outcome.run_id)),
        (
            "import".into(),
            json!({
                "runId": outcome.run_id,
                "status": outcome.status,
                "receiptPath": outcome.receipt_path.display().to_string(),
                "batchesTotal": outcome.batches_total,
                "batchesDone": outcome.batches_done,
                "issues": outcome.issues,
            }),
        ),
    ])
}

/// 增量包内 outbox 哨兵行取证（附加钉②：禁 queued 行是 D1 夹具属性；
/// 此处如实摘出行身份与状态，queued 续投语义进 runbook）。
fn collect_outbox_sentinels(tables: &[rollback_delta::TableDelta]) -> Value {
    let mut out = Vec::new();
    for table in tables {
        if !table.table.contains("outbox") {
            continue;
        }
        for row in &table.rows {
            out.push(json!({
                "table": table.table,
                "identity": row.identity,
                "status": row.row.get("status").cloned().unwrap_or(Value::Null),
            }));
        }
    }
    Value::Array(out)
}

/// 相 4B：delta_import —— 增量回放进恢复根。
/// B①：从包 lineage.json 读原新根（原样 + 规范化）并构造到恢复根的路径重基。
/// 旧包（无 originNewRoot 字段）如实返回 None——不猜原根，附件行按原样回灌
/// 并在收据里以 pathRebased=0 可见。
fn delta_path_rebase(delta_package: &Path, target: &Path) -> Result<Option<rollback_delta::PathRebase>, String> {
    let lineage_path = delta_package.join("lineage.json");
    let lineage: Value = serde_json::from_slice(
        &std::fs::read(&lineage_path).map_err(|e| format!("read {}: {e}", lineage_path.display()))?,
    )
    .map_err(|e| format!("parse lineage.json: {e}"))?;
    let mut origins: Vec<PathBuf> = Vec::new();
    for key in ["originNewRoot", "originNewRootCanonical"] {
        if let Some(value) = lineage.get(key).and_then(Value::as_str) {
            let path = PathBuf::from(value);
            if !origins.contains(&path) {
                origins.push(path);
            }
        }
    }
    if origins.is_empty() {
        return Ok(None);
    }
    let target_canonical = std::fs::canonicalize(target)
        .map_err(|e| format!("canonicalize target root {}: {e}", target.display()))?;
    Ok(Some(rollback_delta::PathRebase {
        origins,
        target: target_canonical,
    }))
}

fn rf_b_delta_import(args: &ReforwardArgs, journal: &RollbackJournal) -> Result<Vec<(String, Value)>, String> {
    let delta_package = args.delta_package.clone().ok_or("delta-package missing")?;
    let target = PathBuf::from(journal.fact_str("targetRoot")?);
    let (pkg, tables, files) = rollback_delta::read_delta_package(&delta_package)
        .map_err(|e| format!("read delta package: {e}"))?;
    let rebase = delta_path_rebase(&delta_package, &target)?;
    let mut stats = rollback_delta::apply_db_delta(&target.join(TARGET_DB_RELPATH), &tables, rebase.as_ref())
        .map_err(|e| format!("apply db delta: {e}"))?;
    rollback_delta::apply_files_delta(&delta_package, &target, &files, &mut stats)
        .map_err(|e| format!("apply files delta: {e}"))?;
    Ok(vec![
        ("deltaImport".into(), json!({
            "packageSha256": pkg.package_sha256,
            "stats": stats.totals_json(),
            "pathRebase": rebase.as_ref().map(|r| json!({
                "origins": r.origins.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                "target": r.target.display().to_string(),
            })),
        })),
        ("sentinelReceipts".into(), collect_outbox_sentinels(&tables)),
    ])
}

/// 相 5B：delta_verify —— 二次回放零增长（行级身份+内容比对由 apply 的
/// 冲突响亮路径免费提供；J4：secondImportZeroGrowth 落收据）。
fn rf_b_delta_verify(args: &ReforwardArgs, journal: &RollbackJournal) -> Result<Vec<(String, Value)>, String> {
    let delta_package = args.delta_package.clone().ok_or("delta-package missing")?;
    let target = PathBuf::from(journal.fact_str("targetRoot")?);
    let (_pkg, tables, files) = rollback_delta::read_delta_package(&delta_package)
        .map_err(|e| format!("read delta package: {e}"))?;
    let rebase = delta_path_rebase(&delta_package, &target)?;
    let mut stats = rollback_delta::apply_db_delta(&target.join(TARGET_DB_RELPATH), &tables, rebase.as_ref())
        .map_err(|e| format!("second apply db delta: {e}"))?;
    rollback_delta::apply_files_delta(&delta_package, &target, &files, &mut stats)
        .map_err(|e| format!("second apply files delta: {e}"))?;
    if !stats.zero_growth() {
        return Err(format!(
            "second delta apply was not zero-growth: {}",
            stats.totals_json()
        ));
    }
    Ok(vec![(
        "deltaVerify".into(),
        json!({
            "secondImportZeroGrowth": true,
            "secondApplyTotals": stats.totals_json(),
        }),
    )])
}

/// 相 6B：epoch_adopt —— 复用 run_epoch_transition（N4 双钉链原样保持）。
async fn rf_b_epoch_adopt(args: &ReforwardArgs, journal: &RollbackJournal) -> Result<Vec<(String, Value)>, String> {
    let target = PathBuf::from(journal.fact_str("targetRoot")?);
    let affected: Vec<String> = journal
        .fact("affectedStoreIds")?
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    let evidence = EpochAdoptionEvidence {
        cutover_id: journal.id.clone(),
        affected_store_ids: affected,
        checkpoint_id: journal.fact_str("checkpointId")?,
        checkpoint_receipt: journal.fact("checkpointReceipt")?.clone(),
        import_run_id: journal.fact_str("importRunId")?,
        backup_manifest_id: journal.fact_str("backupManifestId")?,
        source_identity_sha256: journal.fact_str("sourceIdentitySha256")?,
        stillpoint_manifest_sha256: journal.fact_str("stillpointManifestSha256")?,
        own_version: lingxi_service::server_version().to_string(),
    };
    let _ = args;
    let home = target.clone();
    let result = tokio::task::spawn_blocking(move || {
        epoch::run_epoch_transition(
            &home,
            epoch::MIGRATION_R08_LEGACY_CUTOVER.id,
            &evidence,
            &verify_recovered_root,
            now_unix_ms(),
        )
    })
    .await
    .map_err(|e| format!("join epoch transition task: {e}"))?;
    result.map_err(|e| format!("epoch transition refused: {e}"))?;
    Ok(vec![(
        "epochAdopt".into(),
        json!({
            "migrationId": epoch::MIGRATION_R08_LEGACY_CUTOVER.id,
            "stamp": "v2 committedDataEpoch=2",
        }),
    )])
}

/// 相 7B / 相 3A：pointer_switch。
fn rf_pointer_switch(args: &ReforwardArgs, journal: &RollbackJournal, mode_b: bool) -> Result<Vec<(String, Value)>, String> {
    let target = if mode_b {
        PathBuf::from(journal.fact_str("targetRoot")?)
    } else {
        args.new_root.clone()
    };
    let before = pointer::read_pointer(&args.pointer)
        .map_err(|e| format!("read pointer: {e}"))?
        .map(|p| p.active_data_root)
        .unwrap_or_else(|| "<absent>".to_string());
    let committed = pointer::commit_pointer(&args.pointer, &target, &journal.id)
        .map_err(|e| format!("commit pointer: {e}"))?;
    Ok(vec![(
        "pointer".into(),
        json!({"before": before, "after": committed.active_data_root, "cutoverId": committed.cutover_id}),
    )])
}

/// 相 4A / 相 8B：selfcheck —— 期值取自目标根 runs.db 活读
/// （服务面 vs 磁盘一致性门；锁定计划口径）。
async fn rf_selfcheck(root: &Path, journal: &RollbackJournal) -> Result<Vec<(String, Value)>, String> {
    let (sessions, model_call_usage, cron_jobs) = probe_counts(root)?;
    let expectations = SelfCheckExpectations {
        sessions,
        model_call_usage,
        cron_jobs,
    };
    let binary = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let report = selfcheck::run_self_check(
        &binary,
        root,
        &expectations,
        Duration::from_secs(60),
        Duration::from_secs(30),
    )
    .await
    .map_err(|e| format!("self-check failed: {e}"))?;
    let _ = journal;
    Ok(vec![(
        "selfcheck".into(),
        json!({
            "addr": report.addr,
            "expectationsSource": "live-probed target runs.db",
            "counts": {
                "sessions": report.counts.sessions,
                "modelCallUsage": report.counts.model_call_usage,
                "cronJobs": report.counts.cron_jobs,
            },
        }),
    )])
}

/// 相 9：写 reforward 收据（t06-reforward-receipt/v1）。
fn rf_write_receipt(args: &ReforwardArgs, journal: &RollbackJournal, mode_b: bool) -> Result<Vec<(String, Value)>, String> {
    let f = &journal.facts;
    let get = |k: &str| -> Result<Value, String> {
        f.get(k)
            .cloned()
            .ok_or_else(|| format!("journal fact {k:?} missing for receipt"))
    };
    let verify = if mode_b {
        json!({
            "perCategoryCounts": get("deltaImport")?.get("stats").cloned().unwrap_or(Value::Null),
            "sentinelReceipts": get("sentinelReceipts")?,
            // 行级身份+内容比对 = 二次回放零增长（apply 对既有身份的冲突响亮）。
            "sentinelReceiptsUnchanged": true,
            "secondImportZeroGrowth": true,
            "secondApplyTotals": get("deltaVerify")?.get("secondApplyTotals").cloned().unwrap_or(Value::Null),
            "selfcheck": get("selfcheck")?,
        })
    } else {
        json!({
            "sealHashMatched": true,
            "sealVerify": get("sealVerify")?,
            "selfcheck": get("selfcheck")?,
        })
    };
    let receipt = json!({
        "schema": REFORWARD_RECEIPT_SCHEMA,
        "reforwardId": journal.id,
        "mode": get("mode")?,
        "generatedAt": now_iso(),
        "lineage": {
            "rollbackReceiptId": get("rollbackId")?,
            "sourceCutoverId": get("sourceCutoverId")?,
            "deltaPackageSha256": if mode_b {
                get("deltaPackageSha256")?
            } else {
                get("deltaPackage")?.get("sha256").cloned().unwrap_or(Value::Null)
            },
        },
        "verify": verify,
        "pointer": get("pointer")?,
        "hashConvention": HASH_CONVENTION_NOTE,
        "phaseHistory": serde_json::to_value(&journal.history).unwrap_or(Value::Null),
        "outcome": "committed",
    });
    let receipt_path = args.work_dir.join("reforward-receipt.json");
    let mut body = serde_json::to_string_pretty(&receipt).map_err(|e| format!("serialize receipt: {e}"))?;
    body.push('\n');
    paths::atomic_write_private(&receipt_path, body.as_bytes())
        .map_err(|e| format!("write receipt {}: {e}", receipt_path.display()))?;
    let sha = sha256_file(&receipt_path)?;
    Ok(vec![(
        "receipt".into(),
        json!({"path": receipt_path.display().to_string(), "sha256": sha}),
    )])
}

fn rf_repair_action(phase: &str, args: &ReforwardArgs, reason: &str) -> Value {
    let specific = match phase {
        "preflight" => format!("修正 preflight 前置后重跑：{reason}"),
        "seal_verify" => "新根已漂移：按设计改用 --recover-new-root（mode B）".to_string(),
        "backup_old_root" => "旧根与新根均保留；排查备份错误后 --resume".to_string(),
        "import" => format!(
            "导入在途失败：恢复目标 {} 为本 drill 新建产物（首轮 preflight 保证不存在），删除后 --resume；旧根与指针未动",
            args.work_dir.join("recovered-root").display()
        ),
        "delta_import" | "delta_verify" => "增量回放/复验失败：恢复根与旧根保留；排查后 --resume".to_string(),
        "epoch_adopt" => "epoch 采纳被拒（响亮）：恢复根与旧根保留，证据见恢复根 data-epoch*.json".to_string(),
        "pointer_switch" => "指针原子 rename 失败：检查 --pointer；双侧根未动".to_string(),
        "selfcheck" => "指针已切但自检失败：按设计不反向再切；人工裁决（selfcheck 证据见 journal）".to_string(),
        _ => "见 journal refusal.reason；双侧保留".to_string(),
    };
    json!({
        "action": specific,
        "context": {
            "journal": RollbackJournal::path(&args.work_dir, "reforward").display().to_string(),
            "bothSidesPreserved": true,
        },
    })
}

// ── reforward：状态机 ──────────────────────────────────────────

pub async fn run_reforward(args: &ReforwardArgs) -> Result<RollbackOutcome, VerbFailure> {
    let mode_b = args.recover_new_root;
    let phases: &[&str] = if mode_b { &REFORWARD_B_PHASES } else { &REFORWARD_A_PHASES };
    let inputs = json!({
        "pointer": args.pointer.display().to_string(),
        "oldRoot": args.old_root.display().to_string(),
        "newRoot": args.new_root.display().to_string(),
        "workDir": args.work_dir.display().to_string(),
        "backupDir": args.backup_dir.as_ref().map(|p| p.display().to_string()),
        "deltaPackage": args.delta_package.as_ref().map(|p| p.display().to_string()),
        "target": args.target.as_ref().map(|p| p.display().to_string()),
        "recoverNewRoot": args.recover_new_root,
    });
    std::fs::create_dir_all(&args.work_dir)
        .map_err(|e| verb_err(2, format!("error: reforward: mkdir work-dir: {e}")))?;
    let (mut journal, prior) = open_journal(
        &args.work_dir,
        "reforward",
        "rf",
        inputs,
        phases,
        &args.resume,
    )
    .map_err(|e| verb_err(2, format!("error: reforward: {e}")))?;
    if prior == Some("already-committed") {
        return Ok(RollbackOutcome {
            id: journal.id.clone(),
            phase: "committed".to_string(),
            outcome: "already-committed",
            detail: "reforward already committed; nothing to do".to_string(),
        });
    }
    loop {
        let phase = journal.phase.clone();
        if phase == "committed" {
            let facts = rf_write_receipt(args, &journal, mode_b)
                .map_err(|e| verb_err(2, format!("error: reforward: write receipt: {e}")))?;
            for (k, v) in facts {
                journal.facts.insert(k, v);
            }
            journal = journal
                .commit(&args.work_dir)
                .map_err(|e| verb_err(2, format!("error: reforward: journal commit: {e}")))?;
            return Ok(RollbackOutcome {
                id: journal.id.clone(),
                phase: "committed".to_string(),
                outcome: "committed",
                detail: "reforward committed".to_string(),
            });
        }
        let result = match phase.as_str() {
            "preflight" => rf_preflight(args, mode_b),
            "seal_verify" if !mode_b => rf_a_seal_verify(args, &journal),
            "backup_old_root" if mode_b => rf_b_backup_old_root(args, &journal),
            "import" if mode_b => rf_b_import(args, &journal).await,
            "delta_import" if mode_b => rf_b_delta_import(args, &journal),
            "delta_verify" if mode_b => rf_b_delta_verify(args, &journal),
            "epoch_adopt" if mode_b => rf_b_epoch_adopt(args, &journal).await,
            "pointer_switch" => rf_pointer_switch(args, &journal, mode_b),
            "selfcheck" => {
                let root = if mode_b {
                    PathBuf::from(journal.fact_str("targetRoot").unwrap_or_default())
                } else {
                    args.new_root.clone()
                };
                rf_selfcheck(&root, &journal).await
            }
            other => Err(format!("unknown phase {other:?} in journal")),
        };
        let facts = match result {
            Ok(facts) => facts,
            Err(reason) => {
                let repair = rf_repair_action(&phase, args, &reason);
                journal
                    .refuse(&args.work_dir, &phase, &reason, repair)
                    .map_err(|e| verb_err(2, format!("error: reforward: journal refuse: {e}")))?;
                return Err(verb_err(
                    2,
                    format!("error: reforward: phase {phase} refused: {reason}"),
                ));
            }
        };
        for (k, v) in facts {
            journal.facts.insert(k, v);
        }
        let idx = phases.iter().position(|p| p == &phase).unwrap_or(0);
        let next = phases[idx + 1];
        journal = journal
            .advance(&args.work_dir, next, json!({"completedPhase": phase}))
            .map_err(|e| verb_err(2, format!("error: reforward: journal advance: {e}")))?;
        if args.stop_after_phase.as_deref() == Some(phase.as_str()) {
            journal = journal
                .stop(&args.work_dir)
                .map_err(|e| verb_err(2, format!("error: reforward: journal stop: {e}")))?;
            return Ok(RollbackOutcome {
                id: journal.id.clone(),
                phase: phase.clone(),
                outcome: "stopped",
                detail: format!("clean stop after phase {phase} (--stop-after-phase)"),
            });
        }
    }
}

// ── 单元测试 ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lingxi_adapters::storage::legacy_import::manifest::ManifestEntry;

    fn tmp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "t06-rollback-test-{}-{}-{}",
            tag,
            std::process::id(),
            now_unix_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn journal_lifecycle_create_advance_stop_resume() {
        let dir = tmp_dir("journal");
        let j = RollbackJournal::create(&dir, "rollback", "rb-test-1", json!({}), &ROLLBACK_PHASES).unwrap();
        assert_eq!(j.phase, "preflight");
        assert_eq!(j.status, "in-progress");
        let j = j.advance(&dir, "stop_new_writer", json!({})).unwrap();
        assert_eq!(j.phase, "stop_new_writer");
        assert_eq!(j.history[0].outcome, "ok");
        let j = j.stop(&dir).unwrap();
        assert_eq!(j.status, "stopped");
        // 落盘回读：内容一致。
        let loaded = RollbackJournal::load(&dir, "rollback").unwrap().unwrap();
        assert_eq!(loaded.id, "rb-test-1");
        assert_eq!(loaded.status, "stopped");
        // 损坏 journal = 响亮 Err（绝不自动修复）。
        std::fs::write(dir.join(ROLLBACK_JOURNAL_FILE), "{not json").unwrap();
        assert!(RollbackJournal::load(&dir, "rollback").is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn journal_refuse_is_terminal_and_persisted() {
        let dir = tmp_dir("refuse");
        let j = RollbackJournal::create(&dir, "reforward", "rf-test-1", json!({}), &REFORWARD_A_PHASES).unwrap();
        let j = j.refuse(&dir, "preflight", "boom", json!({"action": "fix"})).unwrap();
        assert_eq!(j.status, "refused");
        let loaded = RollbackJournal::load(&dir, "reforward").unwrap().unwrap();
        assert_eq!(loaded.refusal.as_ref().unwrap()["reason"], "boom");
        // open_journal 对 refused = 终态 Err。
        let err = open_journal(&dir, "reforward", "rf", json!({}), &REFORWARD_A_PHASES, &Some("rf-test-1".into()))
            .unwrap_err();
        assert!(err.contains("terminally refused"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn open_journal_resume_discipline() {
        let dir = tmp_dir("resume");
        // 无 journal + --resume → Err。
        assert!(open_journal(&dir, "rollback", "rb", json!({}), &ROLLBACK_PHASES, &Some("rb-x".into())).is_err());
        // 新建 → 再进无 --resume → Err。
        let (j, _) = open_journal(&dir, "rollback", "rb", json!({}), &ROLLBACK_PHASES, &None).unwrap();
        assert!(open_journal(&dir, "rollback", "rb", json!({}), &ROLLBACK_PHASES, &None).is_err());
        // 错 id → Err。
        assert!(open_journal(&dir, "rollback", "rb", json!({}), &ROLLBACK_PHASES, &Some("rb-wrong".into())).is_err());
        // 正确 id → attempts+1。
        let id = j.id.clone();
        let (j2, prior) = open_journal(&dir, "rollback", "rb", json!({}), &ROLLBACK_PHASES, &Some(id)).unwrap();
        assert!(prior.is_none());
        assert_eq!(j2.attempts, 2);
        // committed → already-committed 幂等。
        let j2 = j2.commit(&dir).unwrap();
        let (_j3, prior) = open_journal(&dir, "rollback", "rb", json!({}), &ROLLBACK_PHASES, &Some(j2.id)).unwrap();
        assert_eq!(prior, Some("already-committed"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_set_classification_whitelist() {
        let mk = |pairs: &[(&str, u64, &str)]| Manifest {
            entries: pairs
                .iter()
                .map(|(r, s, h)| ManifestEntry {
                    relpath: r.to_string(),
                    size: *s,
                    sha256: h.to_string(),
                })
                .collect(),
            skipped_special: vec![],
        };
        let epoch_ok_before = r#"{"schemaVersion":2,"epoch":1,"minimumReaderEpoch":1,"committedDataEpoch":1,"lastVersion":"legacy-import","updatedAt":"2026-10-23T10:00:00.000Z"}"#;
        let epoch_ok_after = r#"{"schemaVersion":2,"epoch":1,"minimumReaderEpoch":1,"committedDataEpoch":1,"lastVersion":"0.1.43","updatedAt":"2026-10-23T10:06:35.658Z"}"#;
        let epoch_bad_after = r#"{"schemaVersion":2,"epoch":2,"minimumReaderEpoch":1,"committedDataEpoch":1,"lastVersion":"0.1.43","updatedAt":"2026-10-23T10:06:35.658Z"}"#;
        let before = mk(&[
            ("knowledge/note.md", 10, "aaa"), // 未分类 → forbidden
            ("server-info.json", 5, "old"),
            ("data-epoch.json", 20, "stamp1"),
            ("agents/a1/sessions/old.jsonl", 30, "stable"),
        ]);
        let after = mk(&[
            ("knowledge/note.md", 11, "bbb"),          // 未分类改动 → forbidden
            ("server-info.json", 6, "new"),            // 精确白名单
            ("data-epoch.json", 21, "stamp2"),         // 裁决①条款判定
            ("agents/a1/sessions/old.jsonl", 30, "stable"), // 不变 → 不在写集
            ("knowledge/indexes/idx.bin", 100, "ix"),  // knowledge-indexes 类
            ("models-store.json", 2, "ms"),            // provider-state 类（§4b 增补）
            ("logs/2026-10-23_18-06-35.log", 50, "lg"), // runtime-diagnostics 类
            ("agents/a1/sessions/new.jsonl", 40, "nw"), // session-jsonl（Convert∉集）→ forbidden
            ("agents/a1/memory/facts.db-wal", 7, "wal"), // 兄弟泛化（agent-facts-sqlite）
            ("totally/unknown.bin", 9, "unk"),         // 未分类 → forbidden
        ]);
        // 印章条款通过腿：
        let (allowed, forbidden, ev) =
            classify_write_set(&before, &after, Some(epoch_ok_before), Some(epoch_ok_after));
        assert_eq!(
            allowed,
            vec![
                "agents/a1/memory/facts.db-wal",
                "data-epoch.json",
                "knowledge/indexes/idx.bin",
                "logs/2026-10-23_18-06-35.log",
                "models-store.json",
                "server-info.json",
            ]
        );
        assert_eq!(
            forbidden,
            vec!["agents/a1/sessions/new.jsonl", "knowledge/note.md", "totally/unknown.bin"]
        );
        assert_eq!(ev["verdict"], "allowed");
        assert_eq!(ev["frozenFields"]["epoch"], 1);
        assert_eq!(ev["provenanceStamped"], json!(["lastVersion", "updatedAt"]));
        assert_eq!(ev["before"]["lastVersion"], "legacy-import");
        assert_eq!(ev["after"]["lastVersion"], "0.1.43");
        // 印章条款拒绝腿：epoch 字段变化 → forbidden + 证据列明示原因。
        let (allowed2, forbidden2, ev2) =
            classify_write_set(&before, &after, Some(epoch_ok_before), Some(epoch_bad_after));
        assert_eq!(allowed2, vec!["agents/a1/memory/facts.db-wal", "knowledge/indexes/idx.bin", "logs/2026-10-23_18-06-35.log", "models-store.json", "server-info.json"]);
        assert_eq!(
            forbidden2,
            vec!["agents/a1/sessions/new.jsonl", "data-epoch.json", "knowledge/note.md", "totally/unknown.bin"]
        );
        assert_eq!(ev2["verdict"], "forbidden");
        assert!(ev2["reason"].as_str().unwrap().contains("epoch field epoch changed"));
    }

    /// §7.2a 钉死测试：控制探针实测写集（t06-control-probe-05，added=55/
    /// modified=4/removed=0）逐路径必须全部落入证据派生白名单（data-epoch.json
    /// 按裁决①条款通过腿处理）。此测试把控制实测钉进代码——白名单收紧到
    /// 容不下控制写集时在此响亮红。
    #[test]
    fn control_probe_write_set_pinned_allows_evidence_derived_whitelist() {
        let mk = |pairs: &[(&str, u64, &str)]| Manifest {
            entries: pairs
                .iter()
                .map(|(r, s, h)| ManifestEntry {
                    relpath: r.to_string(),
                    size: *s,
                    sha256: h.to_string(),
                })
                .collect(),
            skipped_special: vec![],
        };
        // 控制实测 added=55（逐字面路径；logs 时间戳即实测值——类模式泛化由
        // runtime-diagnostics 承担，此处用字面值另证精确路径亦通过）。
        let added: &[&str] = &[
            "agents/a1/memory/facts.db",
            "agents/a1/memory/facts.md",
            "agents/a1/memory/memory.md",
            "agents/a1/memory/navigation.md",
            "agents/a1/memory/week.md",
            "auth.json",
            "device-credentials.json",
            "devices.json",
            "file-history/7a9cb13342a99252/history.sqlite",
            "file-history/7a9cb13342a99252/history.sqlite-shm",
            "file-history/7a9cb13342a99252/history.sqlite-wal",
            "knowledge/indexes/knowledge-ann.db",
            "knowledge/indexes/knowledge-ann.db-shm",
            "knowledge/indexes/knowledge-ann.db-wal",
            "knowledge/indexes/knowledge-fts.db",
            "knowledge/indexes/knowledge-fts.db-shm",
            "knowledge/indexes/knowledge-fts.db-wal",
            "knowledge/indexes/knowledge-vector.db",
            "knowledge/indexes/knowledge-vector.db-shm",
            "knowledge/indexes/knowledge-vector.db-wal",
            "logs/2026-10-23_18-06-35.log",
            "models-store.json",
            "models.json",
            "pairing-sessions.json",
            "provider-catalog.json",
            "security/grants.json",
            "server-network.json",
            "server-node.json",
            "skills/character-creator/SKILL.md",
            "skills/character-creator/references/anti-slop.md",
            "skills/character-creator/references/card-format.md",
            "skills/character-creator/references/character-craft.md",
            "skills/quiet-musing/SKILL.md",
            "skills/skill-creator/LICENSE.txt",
            "skills/skill-creator/SKILL.md",
            "skills/skill-creator/assets/eval_review.html",
            "skills/skill-creator/eval-viewer/generate_review.py",
            "skills/skill-creator/eval-viewer/viewer.html",
            "skills/skill-creator/references/schemas.md",
            "skills/skill-creator/scripts/__init__.py",
            "skills/skill-creator/scripts/aggregate_benchmark.py",
            "skills/skill-creator/scripts/check_env.mjs",
            "skills/skill-creator/scripts/generate_report.py",
            "skills/skill-creator/scripts/improve_description.py",
            "skills/skill-creator/scripts/package_skill.py",
            "skills/skill-creator/scripts/quick_validate.py",
            "skills/skill-creator/scripts/run_eval.py",
            "skills/skill-creator/scripts/run_loop.py",
            "skills/skill-creator/scripts/utils.py",
            "skills/user-guide/SKILL.md",
            "studio-mounts.json",
            "studios.json",
            "user/preferences.json",
            "user/user.md",
            "users.json",
        ];
        // 控制实测 modified=3（非印章）。
        let modified: &[&str] = &[
            "knowledge/knowledge.db",
            "model-observability/observability.sqlite",
            "session-manifest.db",
        ];
        let mut before_pairs: Vec<(&str, u64, &str)> =
            modified.iter().map(|p| (*p, 1, "old")).collect();
        before_pairs.push(("data-epoch.json", 20, "stamp1"));
        let mut after_pairs: Vec<(&str, u64, &str)> = modified
            .iter()
            .chain(std::iter::once(&"data-epoch.json"))
            .map(|p| (*p, 2, "new"))
            .collect();
        for p in added {
            after_pairs.push((p, 3, "added"));
        }
        let before = mk(&before_pairs);
        let after = mk(&after_pairs);
        // 裁决①通过腿印章（控制实测：legacy-import → 0.1.43 + updatedAt 盖戳）。
        let epoch_before = r#"{"schemaVersion":2,"epoch":1,"minimumReaderEpoch":1,"committedDataEpoch":1,"lastVersion":"legacy-import","updatedAt":"2026-10-23T10:00:00.000Z"}"#;
        let epoch_after = r#"{"schemaVersion":2,"epoch":1,"minimumReaderEpoch":1,"committedDataEpoch":1,"lastVersion":"0.1.43","updatedAt":"2026-10-23T10:06:35.658Z"}"#;
        let (allowed, forbidden, ev) =
            classify_write_set(&before, &after, Some(epoch_before), Some(epoch_after));
        assert!(forbidden.is_empty(), "control write set must be fully allowed, forbidden: {forbidden:?}");
        assert_eq!(allowed.len(), added.len() + modified.len() + 1);
        assert_eq!(ev["verdict"], "allowed");
    }

    /// ⑤a：合成表判别 + 单事务改名保序 + 幂等 + 生产形状不误伤。
    #[test]
    fn knowledge_synthetic_detection_rename_idempotent() {
        let dir = tmp_dir("kdb-synth");
        std::fs::create_dir_all(dir.join("knowledge")).unwrap();
        let db = dir.join("knowledge/knowledge.db");
        {
            let conn = rusqlite::Connection::open(&db).unwrap();
            // 合成最小形（夹具 v3 同构）。
            conn.execute_batch(
                "CREATE TABLE documents (doc_id TEXT PRIMARY KEY, title TEXT NOT NULL, source_path TEXT, created_at INTEGER NOT NULL);
                 INSERT INTO documents VALUES ('d1','t1','p1',1);
                 CREATE TABLE processing_artifacts (artifact_id TEXT PRIMARY KEY, doc_id TEXT NOT NULL, parser TEXT NOT NULL, locator_map TEXT NOT NULL);
                 INSERT INTO processing_artifacts VALUES ('a1','d1','p','{}');",
            )
            .unwrap();
        }
        // 判别：两表皆合成。
        let synth = detect_synthetic_knowledge_tables(&db).unwrap();
        assert_eq!(synth, vec!["documents", "processing_artifacts"]);
        // 改名：行零丢失 + integrity ok。
        let (renamed, preserved, integrity) = rename_synthetic_knowledge_tables(&db, &synth).unwrap();
        assert_eq!(
            renamed,
            vec![
                ("documents".to_string(), "fixture_synthetic_documents".to_string()),
                ("processing_artifacts".to_string(), "fixture_synthetic_processing_artifacts".to_string()),
            ]
        );
        assert_eq!(preserved.get("fixture_synthetic_documents"), Some(&1));
        assert_eq!(preserved.get("fixture_synthetic_processing_artifacts"), Some(&1));
        assert_eq!(integrity, "ok");
        // 幂等：改名后判别式不再命中。
        let resynth = detect_synthetic_knowledge_tables(&db).unwrap();
        assert!(resynth.is_empty(), "residual synthetic: {resynth:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// ⑤a：生产形状 processing_artifacts（含 content_snapshot_id）不误伤。
    #[test]
    fn knowledge_production_shape_not_flagged() {
        let dir = tmp_dir("kdb-prod");
        std::fs::create_dir_all(dir.join("knowledge")).unwrap();
        let db = dir.join("knowledge/knowledge.db");
        {
            let conn = rusqlite::Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE processing_artifacts (id TEXT PRIMARY KEY, content_snapshot_id TEXT NOT NULL, processor_id TEXT NOT NULL, processor_version TEXT NOT NULL, processor_config_hash TEXT NOT NULL, status TEXT NOT NULL, locator_map_json TEXT NOT NULL DEFAULT '{}', warnings_json TEXT NOT NULL DEFAULT '[]', created_at TEXT NOT NULL);",
            )
            .unwrap();
        }
        let synth = detect_synthetic_knowledge_tables(&db).unwrap();
        assert!(synth.is_empty(), "production shape must not be flagged: {synth:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn copy_tree_full_copies_regular_files_and_refuses_symlinks() {
        let src = tmp_dir("copy-src");
        let dst = tmp_dir("copy-dst");
        std::fs::create_dir_all(src.join("a/b")).unwrap();
        std::fs::write(src.join("a/b/f.txt"), b"hello").unwrap();
        std::fs::write(src.join("top.txt"), b"world!").unwrap();
        let (files, bytes) = copy_tree_full(&src, &dst).unwrap();
        assert_eq!(files, 2);
        assert_eq!(bytes, 11);
        assert_eq!(std::fs::read(dst.join("a/b/f.txt")).unwrap(), b"hello");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(src.join("top.txt"), src.join("link")).unwrap();
            assert!(copy_tree_full(&src, &dst).is_err());
            std::fs::remove_file(src.join("link")).unwrap();
        }
        std::fs::remove_dir_all(&src).ok();
        std::fs::remove_dir_all(&dst).ok();
    }

    #[test]
    fn http_get_status_parses_status_line() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).unwrap();
            let request = String::from_utf8_lossy(&buf[..n]);
            assert!(
                request.contains("Authorization: Bearer test-token"),
                "request must carry Bearer token: {request}"
            );
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .unwrap();
        });
        let status = http_get_status(port, "/api/health", Duration::from_secs(3), Some("test-token")).unwrap();
        assert_eq!(status, 200);
        handle.join().unwrap();
    }
}
