//! R08-T06：回滚增量包（delta package）——旧根冻结身份集推导 + 新根差集
//! 导出/回灌。纯存储逻辑；编排（journal/相位/指针/探针）在 service 侧
//! `cutover_rollback`。
//!
//! 判据（裁决 J3）：增量 = 新根内容 − 旧根冻结身份集 L。L 只从旧根真实
//! 内容推导（manifest + T04 同一分类表 + converter 同一身份规则），绝不
//! 凭空断言。推导宁可不足不可过头：漏推导的身份只会多进 payload，且在
//! B 模态回灌时被 already_present 交叉验证兜住（对账不符=响亮拒）；
//! 过推导（把非旧根身份说成旧根身份）被结构性杜绝（L 的每一项都来自
//! 旧根实际文件内容）。
//!
//! 身份规则（与 convert_sessions.rs 逐行同构，行号锚定 2026-10-23）：
//! - 会话：jsonl 文件 stem（session_context_from_relpath，不合形状跳过
//!   不猜，同 converter L219-234）。
//! - 消息：`entry.id` 原值，缺省 `legacy-{session}-{line}`（L364-367 /
//!   L410-413）；未知 entryType 标记 `legacy-unknown-block-{s}-{line}`
//!   （L477）。KNOWN_CONTROL_TYPES（runBoundary/sessionMeta/fork）不产生
//!   消息行（L319-330）。
//! - runs：runBoundary 条目的 runId（L283；start/end 双侧都物化，L290-308）。
//! - model_call_usage：旧 model-observability 库 call_id 原值
//!   （observability::read_legacy_observability_rows → record.model_call_id）。
//! - 文件级：session-files/**（register，内容不可变——同 relpath 异 sha
//!   = 响亮拒）、knowledge/** register 类（保格式库，整文件级差分）、
//!   user/preferences.json（settings-preserved-format）。
//!
//! 回灌语义：单事务 PRAGMA foreign_keys=OFF → 逐行幂等判定 →
//! foreign_key_check 违规响亮回滚。messages/runs/model_call_usage 等不可
//! 改账表：同身份同内容 already_present / 异内容 conflict 响亮拒；
//! sessions 发散行 INSERT OR REPLACE 计 session_rows_updated（设计裁决：
//! 会话元数据可变，消息/账簿不可变）。二次回灌必须 applied=0（零增长，
//! 裁决 J4 的 secondImportZeroGrowth 证明原料）。

use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use super::legacy_import::convert_sessions::session_context_from_relpath;
use super::legacy_import::manifest::{build_manifest, sha256_hex, Manifest, ManifestEntry};
use super::legacy_import::{classes, observability, session_jsonl};

/// 增量包 schema 版本（manifest.json 的 schema 字段）。
pub const DELTA_PACKAGE_SCHEMA: &str = "t06-delta-package/v1";

/// V27/V28 导入簿记表：不回滚（它们是导入链自身的账，回滚语义不涵盖）。
/// schema_migrations 同理补列（t06-d3-drill-04 红实证）：目标库自跑迁移产生
/// 自己的收据行（applied_at 时间戳必然与本根不同），导入既错时序也无意义。
const BOOKKEEPING_TABLES: [&str; 6] = [
    "import_runs",
    "import_batches",
    "import_issues",
    "import_run_extensions",
    "legacy_runtime_state_registry",
    "schema_migrations",
];

const MESSAGE_ENTRY_TYPES: [&str; 3] = ["user", "assistant", "system"];
const KNOWN_CONTROL_TYPES: [&str; 3] = ["runBoundary", "sessionMeta", "fork"];
const KNOWN_NONMESSAGE_TYPES: [&str; 2] = ["toolCall", "toolResult"];

#[derive(Debug)]
pub enum DeltaError {
    Io { path: PathBuf, message: String },
    Db(String),
    /// 结构性拒绝（形状不符/不可变内容被改写/对账不符）——零静默。
    Refused { reason: String },
    /// 同身份异内容冲突（不可改账表）。
    Conflict { table: String, identity: String },
}

impl fmt::Display for DeltaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeltaError::Io { path, message } => {
                write!(f, "delta io error at {}: {message}", path.display())
            }
            DeltaError::Db(m) => write!(f, "delta db error: {m}"),
            DeltaError::Refused { reason } => write!(f, "delta refused: {reason}"),
            DeltaError::Conflict { table, identity } => {
                write!(f, "delta conflict in {table} at identity {identity}: same identity, different content; immutable ledger not overwritten")
            }
        }
    }
}

impl std::error::Error for DeltaError {}

fn io_err(path: &Path, e: impl fmt::Display) -> DeltaError {
    DeltaError::Io {
        path: path.to_path_buf(),
        message: e.to_string(),
    }
}

// ── 旧根冻结身份集 ────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct LegacyIdentitySet {
    pub sessions: BTreeSet<String>,
    /// (session_id, message_id) —— messages_v8 复合主键序。
    pub messages: BTreeSet<(String, String)>,
    pub runs: BTreeSet<String>,
    pub model_call_ids: BTreeSet<String>,
    /// session-files/**：relpath → sha256（register，内容不可变）。
    /// 哈希为 T04 manifest 约定（file_sha256 口径，见 FileDeltaEntry::sha256
    /// 的约定隔离注释）——只用于与新根 manifest 条目的同口径比对，
    /// 不写出包、不作外部证据。
    pub session_files: BTreeMap<String, String>,
    /// knowledge/** register 类：relpath → sha256（保格式库整文件差分）。
    /// 同上：manifest 约定哈希，仅内部比对。
    pub knowledge_files: BTreeMap<String, String>,
    /// user/preferences.json 的 sha256（settings-preserved-format）。
    /// 同上：manifest 约定哈希，仅内部比对。
    pub preferences_sha256: Option<String>,
    /// 其余 Register 策略（保格式登记）类文件：relpath → sha256。覆盖
    /// agents/{id}/config.yaml 等 agent-profile/agent-memory/agent-authored-
    /// records/agent-phone/desk-activity/cron-automation/channels/file-history
    /// 族（R08 收尾 B②/B④：切后新助手与其持久化文件此前不在差集面；按
    /// T04 分类表系统性纳入，不逐个特例）。排除 session-files/knowledge（各
    /// 有专属映射）与 device-access-registries（T06 §4.2 设备身份安全域，
    /// 旧根自带原身份，明示不导出）。同上：manifest 约定哈希，仅内部比对。
    pub register_files: BTreeMap<String, String>,
    /// 不合 agents/{agent}/sessions/** 形状的 jsonl（converter 同规则跳过）。
    pub skipped_jsonl: Vec<String>,
}

/// Register 类文件是否进通用差集面（B②/B④）：排除专属映射类与安全域类。
fn is_generic_register_file(rel: &str) -> bool {
    if rel.starts_with("session-files/") || rel.starts_with("knowledge/") || rel == "user/preferences.json" {
        return false;
    }
    match classes::classify(rel) {
        Some((def, _)) => {
            def.strategy == classes::Strategy::Register
                && !def.external
                && def.id != "device-access-registries"
        }
        None => false,
    }
}

impl LegacyIdentitySet {
    /// 规范化身份集哈希：排序集合逐行 sha256（确定性复证原料）。
    pub fn identity_set_sha256(&self) -> String {
        let mut canon = String::new();
        for s in &self.sessions {
            canon.push_str(&format!("session {s}\n"));
        }
        for (s, m) in &self.messages {
            canon.push_str(&format!("message {s} {m}\n"));
        }
        for r in &self.runs {
            canon.push_str(&format!("run {r}\n"));
        }
        for c in &self.model_call_ids {
            canon.push_str(&format!("model_call {c}\n"));
        }
        for (rel, sha) in &self.session_files {
            canon.push_str(&format!("session_file {rel} {sha}\n"));
        }
        for (rel, sha) in &self.knowledge_files {
            canon.push_str(&format!("knowledge_file {rel} {sha}\n"));
        }
        if let Some(sha) = &self.preferences_sha256 {
            canon.push_str(&format!("preferences {sha}\n"));
        }
        for (rel, sha) in &self.register_files {
            canon.push_str(&format!("register_file {rel} {sha}\n"));
        }
        for rel in &self.skipped_jsonl {
            canon.push_str(&format!("skipped_jsonl {rel}\n"));
        }
        sha256_hex(canon.as_bytes())
    }

    pub fn counts_json(&self) -> Value {
        json!({
            "sessions": self.sessions.len(),
            "messages": self.messages.len(),
            "runs": self.runs.len(),
            "modelCallIds": self.model_call_ids.len(),
            "sessionFiles": self.session_files.len(),
            "knowledgeFiles": self.knowledge_files.len(),
            "preferencesPresent": self.preferences_sha256.is_some(),
            "registerFiles": self.register_files.len(),
            "skippedUnmappedJsonl": self.skipped_jsonl.len(),
        })
    }
}

/// 从冻结旧根推导身份集 L。目录遍历用 T04 同一 manifest builder；
/// 分类用 T04 同一注册表；jsonl 解析用 T04 同一解析器。
pub fn derive_legacy_identity_set(old_root: &Path) -> Result<LegacyIdentitySet, DeltaError> {
    let manifest = build_manifest(old_root).map_err(|e| DeltaError::Io {
        path: old_root.join(&e.relpath),
        message: e.message,
    })?;
    let mut set = LegacyIdentitySet::default();
    for entry in &manifest.entries {
        let rel = entry.relpath.as_str();
        if rel.starts_with("session-files/") {
            set.session_files.insert(rel.to_string(), entry.sha256.clone());
        }
        if rel.starts_with("knowledge/") {
            // register 类保格式库进 L；Regenerable（索引等）与未映射文件不
            // 进 L（新根侧差分按「不在 L = 增量」处理，方向安全）。
            if let Some((def, _)) = classes::classify(rel) {
                if def.strategy == classes::Strategy::Register {
                    set.knowledge_files
                        .insert(rel.to_string(), entry.sha256.clone());
                }
            }
        }
        if rel == "user/preferences.json" {
            set.preferences_sha256 = Some(entry.sha256.clone());
        }
        if is_generic_register_file(rel) {
            set.register_files.insert(rel.to_string(), entry.sha256.clone());
        }
        let Some((def, _)) = classes::classify(rel) else {
            continue;
        };
        match def.id {
            "session-jsonl" => {
                derive_session_identities(old_root, rel, &mut set)?;
            }
            "model-observability-db" => {
                // 主库才读。P6 纪律（T06 评审 N3 移交）：旧根库先复制
                // db+(-wal/-shm) 到临时目录再开副本——READ_ONLY 直开 WAL 模式
                // 库会在旧根静默建 -shm/-wal（t06-d2-drill-01 同根因），冻结旧根
                // 必须零触碰。
                if rel.ends_with(".sqlite") || rel.ends_with(".db") {
                    let staged = stage_quiesced_copy(&old_root.join(rel))?;
                    let rows = observability::read_legacy_observability_rows(&staged)
                        .map_err(|e| DeltaError::Refused {
                            reason: format!("legacy observability store {rel} unreadable: {e}"),
                        });
                    let _ = std::fs::remove_dir_all(staged.parent().unwrap_or(Path::new("/nonexistent")));
                    for row in rows? {
                        set.model_call_ids.insert(row.record.model_call_id.clone());
                    }
                }
            }
            _ => {}
        }
    }
    Ok(set)
}

/// 单会话 jsonl 的身份推导（converter 同构，见模块头行号锚）。
fn derive_session_identities(
    old_root: &Path,
    relpath: &str,
    set: &mut LegacyIdentitySet,
) -> Result<(), DeltaError> {
    let Some(ctx) = session_context_from_relpath(relpath) else {
        set.skipped_jsonl.push(relpath.to_string());
        return Ok(());
    };
    let path = old_root.join(relpath);
    let bytes = std::fs::read(&path).map_err(|e| io_err(&path, e))?;
    let parsed = session_jsonl::parse_session_jsonl(&bytes);
    set.sessions.insert(ctx.session_id.clone());
    for entry in &parsed.entries {
        let etype = entry.entry_type.as_deref();
        if KNOWN_CONTROL_TYPES.contains(&etype.unwrap_or("")) {
            if etype == Some("runBoundary") {
                if let Some(rid) = entry.run_id.clone() {
                    set.runs.insert(rid);
                }
            }
            continue;
        }
        let is_message = match etype {
            Some(t) => MESSAGE_ENTRY_TYPES.contains(&t),
            None => entry.role.is_some(),
        };
        if is_message || KNOWN_NONMESSAGE_TYPES.contains(&etype.unwrap_or("")) {
            let message_id = entry
                .id
                .clone()
                .unwrap_or_else(|| format!("legacy-{}-{}", ctx.session_id, entry.line_no));
            set.messages
                .insert((ctx.session_id.clone(), message_id));
        } else {
            // 未知 entryType → 标记消息（converter L477）。
            set.messages.insert((
                ctx.session_id.clone(),
                format!("legacy-unknown-block-{}-{}", ctx.session_id, entry.line_no),
            ));
        }
    }
    Ok(())
}

// ── 新根 runs.db 差集导出 ─────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct DeltaRow {
    /// 主键列值数组（PRAGMA table_info 的 pk 序号升序）。
    pub identity: Vec<Value>,
    /// 全行（列名 → 值；blob 编码为 {"$blobBase64": ...}）。
    pub row: BTreeMap<String, Value>,
    pub row_sha256: String,
    /// 仅 sessions 表：false = 该会话无旧根来源（含 scaffold-seed，J7）。
    pub old_ui_readable: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableDelta {
    pub table: String,
    pub pk_columns: Vec<String>,
    pub rows: Vec<DeltaRow>,
}

fn sql_value_to_json(v: rusqlite::types::ValueRef<'_>) -> Value {
    use base64::Engine;
    use rusqlite::types::ValueRef;
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => json!(i),
        ValueRef::Real(r) => json!(r),
        ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => json!({ "$blobBase64": base64::engine::general_purpose::STANDARD.encode(b) }),
    }
}

fn json_to_sql_value(v: &Value) -> Result<rusqlite::types::Value, DeltaError> {
    use base64::Engine;
    use rusqlite::types::Value as SqlValue;
    Ok(match v {
        Value::Null => SqlValue::Null,
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                SqlValue::Integer(i)
            } else if let Some(r) = n.as_f64() {
                SqlValue::Real(r)
            } else {
                return Err(DeltaError::Refused {
                    reason: format!("unrepresentable number in delta row: {n}"),
                });
            }
        }
        Value::String(s) => SqlValue::Text(s.clone()),
        Value::Object(o) if o.len() == 1 && o.contains_key("$blobBase64") => {
            let b64 = o["$blobBase64"].as_str().ok_or_else(|| DeltaError::Refused {
                reason: "$blobBase64 is not a string".into(),
            })?;
            SqlValue::Blob(
                base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .map_err(|e| DeltaError::Refused {
                        reason: format!("invalid blob base64: {e}"),
                    })?,
            )
        }
        other => {
            return Err(DeltaError::Refused {
                reason: format!("unsupported delta value shape: {other}"),
            })
        }
    })
}

fn canonical_row_string(row: &BTreeMap<String, Value>) -> String {
    // BTreeMap 已按列名字典序；serde_json 对 Map<String,_> 保序输出。
    Value::Object(row.clone().into_iter().collect()).to_string()
}

/// 用户表清单（剔除 sqlite 内部表与 V27/V28 簿记表；名称升序 = 确定性）。
fn list_delta_tables(conn: &rusqlite::Connection) -> Result<Vec<String>, DeltaError> {
    let mut stmt = conn
        .prepare(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' \
             ORDER BY name",
        )
        .map_err(|e| DeltaError::Db(format!("list tables: {e}")))?;
    let names = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| DeltaError::Db(format!("list tables: {e}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| DeltaError::Db(format!("list tables: {e}")))?;
    Ok(names
        .into_iter()
        .filter(|n| !BOOKKEEPING_TABLES.contains(&n.as_str()))
        .collect())
}

/// 表的主键列（pk 序号升序）。无主键 = 未核定形状，响亮拒（不猜身份）。
fn pk_columns(conn: &rusqlite::Connection, table: &str) -> Result<Vec<String>, DeltaError> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info(\"{table}\")"))
        .map_err(|e| DeltaError::Db(format!("table_info {table}: {e}")))?;
    let mut cols: Vec<(i64, String)> = stmt
        .query_map([], |row| Ok((row.get::<_, i64>(5)?, row.get::<_, String>(1)?)))
        .map_err(|e| DeltaError::Db(format!("table_info {table}: {e}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| DeltaError::Db(format!("table_info {table}: {e}")))?;
    cols.retain(|(pk, _)| *pk > 0);
    cols.sort_by_key(|(pk, _)| *pk);
    if cols.is_empty() {
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |r| r.get::<_, i64>(0).map(|n| n > 0),
            )
            .unwrap_or(false);
        return Err(DeltaError::Refused {
            reason: if exists {
                format!("table {table} has no PRIMARY KEY; delta identity undefined (refuse to guess)")
            } else {
                format!("table {table} does not exist in target; refusing to import non-empty delta for an absent table")
            },
        });
    }
    Ok(cols.into_iter().map(|(_, name)| name).collect())
}

/// 该身份是否属于旧根冻结集 L（按表名+主键列名映射；其余表 L=∅）。
fn identity_in_legacy(
    table: &str,
    pk_cols: &[String],
    identity: &[Value],
    legacy: &LegacyIdentitySet,
) -> bool {
    let get = |name: &str| -> Option<&Value> {
        pk_cols
            .iter()
            .position(|c| c == name)
            .and_then(|i| identity.get(i))
    };
    match table {
        "sessions" => get("session_id")
            .and_then(Value::as_str)
            .map(|s| legacy.sessions.contains(s))
            .unwrap_or(false),
        "messages" => match (
            get("session_id").and_then(Value::as_str),
            get("message_id").and_then(Value::as_str),
        ) {
            (Some(s), Some(m)) => legacy.messages.contains(&(s.to_string(), m.to_string())),
            _ => false,
        },
        "runs" => get("run_id")
            .and_then(Value::as_str)
            .map(|r| legacy.runs.contains(r))
            .unwrap_or(false),
        "model_call_usage" | "observability_legacy_trace" => get("model_call_id")
            .and_then(Value::as_str)
            .map(|c| legacy.model_call_ids.contains(c))
            .unwrap_or(false),
        _ => false,
    }
}

/// 导出全部差集行。P6 纪律（源库绝不直开）：READ_ONLY 连接对 WAL 库仍会在
/// 源目录创建 -shm/-wal（t06-d2-drill-01 红实证：封缄根被导出器污染 →
/// export_verify 印章破裂）——先复制 db+(-wal/-shm) 到临时目录再开**副本**
/// （副本上正常恢复语义，wal 内已提交事务可见），封缄源字节零改动。
pub fn export_db_delta(
    new_root_db: &Path,
    legacy: &LegacyIdentitySet,
) -> Result<Vec<TableDelta>, DeltaError> {
    let staged = stage_quiesced_copy(new_root_db)?;
    let conn = rusqlite::Connection::open(&staged)
        .map_err(|e| DeltaError::Db(format!("open staged copy of {}: {e}", new_root_db.display())))?;
    let out = (|| -> Result<Vec<TableDelta>, DeltaError> {
        let mut out = Vec::new();
        for table in list_delta_tables(&conn)? {
        let pk_cols = pk_columns(&conn, &table)?;
        let quoted = format!("\"{table}\"");
        let mut stmt = conn
            .prepare(&format!("SELECT * FROM {quoted}"))
            .map_err(|e| DeltaError::Db(format!("scan {table}: {e}")))?;
        let col_names: Vec<String> =
            stmt.column_names().iter().map(|s| s.to_string()).collect();
        let pk_idx: Vec<usize> = pk_cols
            .iter()
            .map(|c| {
                col_names
                    .iter()
                    .position(|n| n == c)
                    .ok_or_else(|| DeltaError::Refused {
                        reason: format!("pk column {c} of {table} not in SELECT * projection"),
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut rows = stmt
            .query([])
            .map_err(|e| DeltaError::Db(format!("scan {table}: {e}")))?;
        let mut delta_rows = Vec::new();
        while let Some(row) = rows
            .next()
            .map_err(|e| DeltaError::Db(format!("scan {table}: {e}")))?
        {
            let mut obj = BTreeMap::new();
            for (i, name) in col_names.iter().enumerate() {
                let v = row
                    .get_ref(i)
                    .map_err(|e| DeltaError::Db(format!("scan {table}: {e}")))?;
                obj.insert(name.clone(), sql_value_to_json(v));
            }
            let identity: Vec<Value> = pk_idx.iter().map(|i| obj[&col_names[*i]].clone()).collect();
            let legacy_identity = identity_in_legacy(&table, &pk_cols, &identity, legacy);
            // B③（R08 收尾）：sessions 是可变元数据表（标题/置顶/归档/项目归属
            // 等切后合法更新）。旧会话按身份整行跳过会把切后改名等更新丢在
            // 新根里，恢复退回旧标题——改为旧身份行也入包：回灌侧对同身份行
            // 逐列比对，相同 already_present、发散 session_rows_updated（设计
            // 裁决原文：会话元数据可变）。账簿表（messages/runs/model_call_
            // usage/observability）仍按身份跳过（不可改账，旧数据保留）。
            if legacy_identity && table != "sessions" {
                continue;
            }
            let row_sha256 = sha256_hex(canonical_row_string(&obj).as_bytes());
            delta_rows.push(DeltaRow {
                identity,
                // 旧身份会话行对旧 UI 可读（旧根原有会话）；新身份会话行不可读。
                old_ui_readable: if table == "sessions" { Some(legacy_identity) } else { None },
                row: obj,
                row_sha256,
            });
        }
        delta_rows.sort_by(|a, b| canonical_identity(a).cmp(&canonical_identity(b)));
        out.push(TableDelta {
            table,
            pk_columns: pk_cols,
            rows: delta_rows,
        });
    }
    Ok(out)
    })();
    drop(conn);
    let _ = std::fs::remove_dir_all(staged.parent().unwrap_or(Path::new("/nonexistent")));
    out
}

/// 把静止血源库（db + -wal/-shm 兄弟若存在）复制到临时目录，返回副本路径。
/// 调用方负责删除副本父目录。源根零写（P6：源库绝不直开）。
pub fn stage_quiesced_copy(db: &Path) -> Result<PathBuf, DeltaError> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "t06-delta-export-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).map_err(|e| DeltaError::Db(format!("mkdir staging {}: {e}", dir.display())))?;
    let name = db.file_name().and_then(|n| n.to_str()).ok_or_else(|| DeltaError::Db(format!("db path not utf-8: {}", db.display())))?;
    for suffix in ["", "-wal", "-shm"] {
        let src = db.with_file_name(format!("{name}{suffix}"));
        if src.exists() {
            let dst = dir.join(format!("{name}{suffix}"));
            std::fs::copy(&src, &dst).map_err(|e| DeltaError::Db(format!("stage copy {}: {e}", src.display())))?;
        }
    }
    Ok(dir.join(name))
}

fn canonical_identity(row: &DeltaRow) -> String {
    Value::Array(row.identity.clone()).to_string()
}

// ── 新根文件差集（session-files / knowledge / preferences）─────────

#[derive(Debug, Clone, PartialEq)]
pub struct FileDeltaEntry {
    pub relpath: String,
    /// 真·内容 sha256（单次 `Sha256::digest`，外部 `shasum -a 256` 可对拍）。
    /// 注意与 T04 `manifest.rs` 的条目哈希约定不同：后者是
    /// `sha256(sha256_raw(content))`（file_sha256 流式 finalize 后再经
    /// sha256_hex 二次哈希）。manifest 约定已锁进 T05 封存对拍锚
    /// （r08_t05_refactor_parity.mjs 的 80a8d2c2…feaa），不得"修复"；
    /// 本包层只用真·内容哈希，两套约定在此显式隔离、互不混用。
    pub sha256: String,
    pub size: u64,
    /// session-files | knowledge | preferences | register（其余 Register 类，B②/B④）
    pub category: &'static str,
}

/// 新根 manifest → 文件差集。session-files 同 relpath 异 sha = 响亮拒
/// （register 内容不可变）；knowledge/preferences 异 sha = 增量（整文件）。
/// 差集判定用 manifest 约定哈希（新旧同口径，自洽）；产出条目的 sha256
/// 字段另取真·内容哈希（读本文件一次），并复验 size 与 manifest 一致
/// （不一致 = 扫描与读取之间文件被改写，响亮拒）。
pub fn diff_files(
    new_root: &Path,
    new_manifest: &Manifest,
    legacy: &LegacyIdentitySet,
) -> Result<Vec<FileDeltaEntry>, DeltaError> {
    // 差集判定（manifest 约定，自洽比对）→ 命中即读本文件取真·内容哈希。
    let mut out = Vec::new();
    let mut emit = |entry: &ManifestEntry,
                    category: &'static str|
     -> Result<(), DeltaError> {
        let path = new_root.join(&entry.relpath);
        let bytes = std::fs::read(&path).map_err(|e| io_err(&path, e))?;
        if bytes.len() as u64 != entry.size {
            return Err(DeltaError::Refused {
                reason: format!(
                    "{} mutated between manifest scan and delta read: {} (size {} → {})",
                    category,
                    entry.relpath,
                    entry.size,
                    bytes.len()
                ),
            });
        }
        out.push(FileDeltaEntry {
            relpath: entry.relpath.clone(),
            sha256: sha256_hex(&bytes),
            size: entry.size,
            category,
        });
        Ok(())
    };
    for entry in &new_manifest.entries {
        let rel = entry.relpath.as_str();
        if rel.starts_with("session-files/") {
            match legacy.session_files.get(rel) {
                Some(old_sha) if old_sha != &entry.sha256 => {
                    return Err(DeltaError::Refused {
                        reason: format!(
                            "session-files register content mutated in new root: {rel} \
                             (old {old_sha} → new {})",
                            entry.sha256
                        ),
                    });
                }
                Some(_) => {}
                None => emit(entry, "session-files")?,
            }
        } else if rel.starts_with("knowledge/") {
            if legacy.knowledge_files.get(rel) == Some(&entry.sha256) {
                continue;
            }
            emit(entry, "knowledge")?;
        } else if rel == "user/preferences.json" {
            if legacy.preferences_sha256.as_deref() == Some(entry.sha256.as_str()) {
                continue;
            }
            emit(entry, "preferences")?;
        } else if is_generic_register_file(rel) {
            // B②/B④：其余 Register 类（agents/{id}/config.yaml 等 agent 持久化
            // 文件、channels、file-history…）同 knowledge 语义——不在 L 或异
            // sha 即整文件入包；回灌侧发散=整文件覆盖（切后新根为权威）。
            if legacy.register_files.get(rel) == Some(&entry.sha256) {
                continue;
            }
            emit(entry, "register")?;
        }
    }
    out.sort_by(|a, b| a.relpath.cmp(&b.relpath));
    Ok(out)
}

// ── 包落盘/读取 ──────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct PackageManifest {
    pub identity_set_sha256: String,
    pub legacy_counts: Value,
    /// table → (rows, payload relpath, payload sha256)
    pub tables: Vec<(String, u64, String, String)>,
    /// files-manifest 的 (count, sha256)
    pub files_count: u64,
    pub files_manifest_sha256: String,
    pub lineage_sha256: String,
    pub package_sha256: String,
}

impl PackageManifest {
    pub fn to_json(&self) -> Value {
        let tables: Vec<Value> = self
            .tables
            .iter()
            .map(|(t, rows, rel, sha)| {
                json!({"table": t, "rows": rows, "path": rel, "sha256": sha})
            })
            .collect();
        json!({
            "schema": DELTA_PACKAGE_SCHEMA,
            "identitySetSha256": self.identity_set_sha256,
            "legacyCounts": self.legacy_counts,
            "tables": tables,
            "files": {"path": "payload/files-manifest.json", "count": self.files_count, "sha256": self.files_manifest_sha256},
            "lineageSha256": self.lineage_sha256,
            "packageSha256": self.package_sha256,
        })
    }

    pub fn from_json(v: &Value) -> Result<Self, DeltaError> {
        let bad = |m: &str| DeltaError::Refused {
            reason: format!("delta package manifest invalid: {m}"),
        };
        if v.get("schema").and_then(Value::as_str) != Some(DELTA_PACKAGE_SCHEMA) {
            return Err(bad("schema mismatch"));
        }
        let mut tables = Vec::new();
        for t in v
            .get("tables")
            .and_then(Value::as_array)
            .ok_or_else(|| bad("tables missing"))?
        {
            tables.push((
                t.get("table")
                    .and_then(Value::as_str)
                    .ok_or_else(|| bad("table name"))?
                    .to_string(),
                t.get("rows")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| bad("rows"))?,
                t.get("path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| bad("path"))?
                    .to_string(),
                t.get("sha256")
                    .and_then(Value::as_str)
                    .ok_or_else(|| bad("sha256"))?
                    .to_string(),
            ));
        }
        let files = v.get("files").ok_or_else(|| bad("files missing"))?;
        Ok(PackageManifest {
            identity_set_sha256: v
                .get("identitySetSha256")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("identitySetSha256"))?
                .to_string(),
            legacy_counts: v.get("legacyCounts").cloned().unwrap_or(Value::Null),
            tables,
            files_count: files
                .get("count")
                .and_then(Value::as_u64)
                .ok_or_else(|| bad("files.count"))?,
            files_manifest_sha256: files
                .get("sha256")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("files.sha256"))?
                .to_string(),
            lineage_sha256: v
                .get("lineageSha256")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("lineageSha256"))?
                .to_string(),
            package_sha256: v
                .get("packageSha256")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("packageSha256"))?
                .to_string(),
        })
    }
}

fn table_payload_jsonl(delta: &TableDelta) -> String {
    let mut out = String::new();
    for row in &delta.rows {
        let mut envelope = serde_json::Map::new();
        envelope.insert("identity".into(), Value::Array(row.identity.clone()));
        envelope.insert("rowSha256".into(), json!(row.row_sha256));
        if let Some(readable) = row.old_ui_readable {
            envelope.insert("oldUiReadable".into(), json!(readable));
        }
        envelope.insert(
            "row".into(),
            Value::Object(row.row.clone().into_iter().collect()),
        );
        out.push_str(&Value::Object(envelope).to_string());
        out.push('\n');
    }
    out
}

fn parse_table_payload(
    table: &str,
    pk_columns: Vec<String>,
    bytes: &[u8],
) -> Result<TableDelta, DeltaError> {
    let text = String::from_utf8(bytes.to_vec()).map_err(|e| DeltaError::Refused {
        reason: format!("payload {table} not utf-8: {e}"),
    })?;
    let mut rows = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(line).map_err(|e| DeltaError::Refused {
            reason: format!("payload {table} line {}: {e}", idx + 1),
        })?;
        let identity = v
            .get("identity")
            .and_then(Value::as_array)
            .ok_or_else(|| DeltaError::Refused {
                reason: format!("payload {table} line {} lacks identity", idx + 1),
            })?
            .clone();
        let row: BTreeMap<String, Value> = v
            .get("row")
            .and_then(Value::as_object)
            .ok_or_else(|| DeltaError::Refused {
                reason: format!("payload {table} line {} lacks row", idx + 1),
            })?
            .clone()
            .into_iter()
            .collect();
        let row_sha256 = v
            .get("rowSha256")
            .and_then(Value::as_str)
            .ok_or_else(|| DeltaError::Refused {
                reason: format!("payload {table} line {} lacks rowSha256", idx + 1),
            })?
            .to_string();
        // 落盘回读复验：行哈希必须自洽（篡改即响）。
        let recomputed = sha256_hex(canonical_row_string(&row).as_bytes());
        if recomputed != row_sha256 {
            return Err(DeltaError::Refused {
                reason: format!(
                    "payload {table} line {} rowSha256 mismatch (tamper or corruption)",
                    idx + 1
                ),
            });
        }
        rows.push(DeltaRow {
            identity,
            row,
            row_sha256,
            old_ui_readable: v.get("oldUiReadable").and_then(Value::as_bool),
        });
    }
    Ok(TableDelta {
        table: table.to_string(),
        pk_columns,
        rows,
    })
}

/// 写增量包：manifest.json 本体无时间戳（时间戳进 lineage.json），但内嵌
/// `lineageSha256`（并折入 `packageSha256`）——故 manifest 的确定性以「同
/// lineage 输入」为条件：export_verify 相重导时必须复用首次导出的 lineage.json
/// 字节，比对才精确落在导出器行为上（t06-d1-drill-04 真红根因披露）。
pub fn write_delta_package(
    package_dir: &Path,
    tables: &[TableDelta],
    file_entries: &[FileDeltaEntry],
    new_root: &Path,
    lineage_json: &Value,
    legacy: &LegacyIdentitySet,
) -> Result<PackageManifest, DeltaError> {
    std::fs::create_dir_all(package_dir.join("payload/db"))
        .map_err(|e| io_err(package_dir, e))?;
    let lineage_bytes = serde_json::to_vec_pretty(lineage_json)
        .map_err(|e| DeltaError::Refused {
            reason: format!("lineage serialize: {e}"),
        })?;
    std::fs::write(package_dir.join("lineage.json"), &lineage_bytes)
        .map_err(|e| io_err(&package_dir.join("lineage.json"), e))?;
    let lineage_sha256 = sha256_hex(&lineage_bytes);

    let mut table_entries = Vec::new();
    for delta in tables {
        let rel = format!("payload/db/{}.jsonl", delta.table);
        let body = table_payload_jsonl(delta);
        let path = package_dir.join(&rel);
        std::fs::write(&path, body.as_bytes()).map_err(|e| io_err(&path, e))?;
        table_entries.push((
            delta.table.clone(),
            delta.rows.len() as u64,
            rel,
            sha256_hex(body.as_bytes()),
        ));
    }

    let mut files_manifest_lines = String::new();
    for entry in file_entries {
        let src = new_root.join(&entry.relpath);
        let dst = package_dir.join("payload/files").join(&entry.relpath);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io_err(parent, e))?;
        }
        std::fs::copy(&src, &dst).map_err(|e| io_err(&src, e))?;
        // 复制完整性复验：拷贝后字节哈希必须等于 diff_files 记录的
        // 真·内容哈希——不等即拷贝受损或源文件在读取与拷贝之间被改写，响亮拒。
        let copied = std::fs::read(&dst).map_err(|e| io_err(&dst, e))?;
        let copied_sha = sha256_hex(&copied);
        if copied_sha != entry.sha256 || copied.len() as u64 != entry.size {
            return Err(DeltaError::Refused {
                reason: format!(
                    "payload copy integrity failure for {} (recorded sha={} size={}; copied sha={} size={})",
                    entry.relpath,
                    entry.sha256,
                    entry.size,
                    copied_sha,
                    copied.len()
                ),
            });
        }
        files_manifest_lines.push_str(&format!(
            "{}  {}  {}  {}\n",
            entry.sha256, entry.size, entry.category, entry.relpath
        ));
    }
    let fm_path = package_dir.join("payload/files-manifest.json");
    std::fs::write(&fm_path, files_manifest_lines.as_bytes()).map_err(|e| io_err(&fm_path, e))?;
    let files_manifest_sha256 = sha256_hex(files_manifest_lines.as_bytes());

    let mut manifest = PackageManifest {
        identity_set_sha256: legacy.identity_set_sha256(),
        legacy_counts: legacy.counts_json(),
        tables: table_entries,
        files_count: file_entries.len() as u64,
        files_manifest_sha256,
        lineage_sha256,
        package_sha256: String::new(),
    };
    // packageSha256 = 除自身字段外的 canonical manifest JSON 哈希。
    let canon = canonical_manifest_string(&manifest);
    manifest.package_sha256 = sha256_hex(canon.as_bytes());
    let m_path = package_dir.join("manifest.json");
    std::fs::write(
        &m_path,
        serde_json::to_string_pretty(&manifest.to_json())
            .map_err(|e| DeltaError::Refused {
                reason: format!("manifest serialize: {e}"),
            })?
            .as_bytes(),
    )
    .map_err(|e| io_err(&m_path, e))?;
    Ok(manifest)
}

fn canonical_manifest_string(m: &PackageManifest) -> String {
    let mut v = m.to_json();
    v.as_object_mut().map(|o| o.remove("packageSha256"));
    v.to_string()
}

/// 读回增量包并整包复验（行级 sha + 文件 sha + manifest packageSha256）。
pub fn read_delta_package(
    package_dir: &Path,
) -> Result<(PackageManifest, Vec<TableDelta>, Vec<FileDeltaEntry>), DeltaError> {
    let m_path = package_dir.join("manifest.json");
    let m_bytes = std::fs::read(&m_path).map_err(|e| io_err(&m_path, e))?;
    let m_json: Value = serde_json::from_slice(&m_bytes).map_err(|e| DeltaError::Refused {
        reason: format!("manifest.json parse: {e}"),
    })?;
    let manifest = PackageManifest::from_json(&m_json)?;
    // 包自校验的有效门 = 下方 packageSha256 重算（实体字段全覆盖）；原 canonical
    // 形态比对空 if 死块已删（T06 评审 N2）。
    let recomputed_pkg = sha256_hex(canonical_manifest_string(&manifest).as_bytes());
    if recomputed_pkg != manifest.package_sha256 {
        return Err(DeltaError::Refused {
            reason: "manifest.json packageSha256 mismatch (tamper or corruption)".into(),
        });
    }
    let lineage_path = package_dir.join("lineage.json");
    let lineage_bytes = std::fs::read(&lineage_path).map_err(|e| io_err(&lineage_path, e))?;
    if sha256_hex(&lineage_bytes) != manifest.lineage_sha256 {
        return Err(DeltaError::Refused {
            reason: "lineage.json sha256 mismatch".into(),
        });
    }
    let fm_path = package_dir.join("payload/files-manifest.json");
    let fm_bytes = std::fs::read(&fm_path).map_err(|e| io_err(&fm_path, e))?;
    if sha256_hex(&fm_bytes) != manifest.files_manifest_sha256 {
        return Err(DeltaError::Refused {
            reason: "files-manifest.json sha256 mismatch".into(),
        });
    }
    let fm_text = String::from_utf8(fm_bytes).map_err(|e| DeltaError::Refused {
        reason: format!("files-manifest not utf-8: {e}"),
    })?;
    let mut file_entries = Vec::new();
    for line in fm_text.lines() {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(4, "  ");
        let sha = parts.next().unwrap_or_default().to_string();
        let size: u64 = parts
            .next()
            .and_then(|s| s.trim().parse().ok())
            .ok_or_else(|| DeltaError::Refused {
                reason: format!("files-manifest line malformed: {line}"),
            })?;
        let category = parts.next().unwrap_or_default().trim().to_string();
        let relpath = parts.next().unwrap_or_default().trim().to_string();
        if relpath.is_empty() {
            return Err(DeltaError::Refused {
                reason: format!("files-manifest line malformed: {line}"),
            });
        }
        let f_path = package_dir.join("payload/files").join(&relpath);
        let bytes = std::fs::read(&f_path).map_err(|e| io_err(&f_path, e))?;
        let actual_sha = sha256_hex(&bytes);
        if actual_sha != sha || bytes.len() as u64 != size {
            return Err(DeltaError::Refused {
                reason: format!(
                    "payload file {relpath} sha256/size mismatch (manifest sha={sha} size={size}; actual sha={actual_sha} size={})",
                    bytes.len()
                ),
            });
        }
        let category: &'static str = match category.as_str() {
            "session-files" => "session-files",
            "knowledge" => "knowledge",
            "preferences" => "preferences",
            "register" => "register",
            other => {
                return Err(DeltaError::Refused {
                    reason: format!("unknown file category {other}"),
                })
            }
        };
        file_entries.push(FileDeltaEntry {
            relpath,
            sha256: sha,
            size,
            category,
        });
    }
    if file_entries.len() as u64 != manifest.files_count {
        return Err(DeltaError::Refused {
            reason: "files-manifest count mismatch".into(),
        });
    }
    let mut tables = Vec::new();
    for (table, rows, rel, sha) in &manifest.tables {
        let t_path = package_dir.join(rel);
        let bytes = std::fs::read(&t_path).map_err(|e| io_err(&t_path, e))?;
        if sha256_hex(&bytes) != *sha {
            return Err(DeltaError::Refused {
                reason: format!("table payload {rel} sha256 mismatch"),
            });
        }
        // pk_columns 在回灌时以目标库 schema 为准（read 处先占位，apply 复验一致）。
        let delta = parse_table_payload(table, Vec::new(), &bytes)?;
        if delta.rows.len() as u64 != *rows {
            return Err(DeltaError::Refused {
                reason: format!("table payload {rel} row count mismatch"),
            });
        }
        tables.push(delta);
    }
    Ok((manifest, tables, file_entries))
}

// ── 回灌（B 模态 delta_import）────────────────────────────────────

#[derive(Debug, Default, Clone, PartialEq)]
pub struct TableApplyStats {
    pub applied: u64,
    pub already_present: u64,
    pub session_rows_updated: u64,
    /// session_files 族身份冲突且内容发散时保留目标行（根作用域路径正确性
    /// 优先）的计数——显式降级披露，不静默。
    pub path_scoped_preserved: u64,
    /// B①：新身份 session_files 行（managed_cache）插入时 file_path/real_path
    /// 由原新根前缀重基到恢复根的计数（路径与身份重新关联，不再指向被隔离
    /// 的原新根）。
    pub path_rebased: u64,
}

/// B①（R08 收尾）：附件行路径重基。包内 session_files 行携带**原新根**的绝对
/// file_path/real_path（managed_cache 存储形态 `{home}/session-files/…`），而
/// 字节经包 files 面恢复到**恢复根**同 relpath——原样 INSERT 会让行指向已被
/// 隔离/丢失的原新根（身份与路径脱钩）。回灌时对 managed_cache 行把任一
/// 原根前缀（原样/规范化两形态）替换为恢复根；external 存储形态（用户自有
/// 文件，不在根内）不改写。
#[derive(Debug, Clone, PartialEq)]
pub struct PathRebase {
    pub origins: Vec<PathBuf>,
    pub target: PathBuf,
}

impl PathRebase {
    fn rebase(&self, value: &str) -> Option<String> {
        let path = Path::new(value);
        for origin in &self.origins {
            if let Ok(rest) = path.strip_prefix(origin) {
                return Some(self.target.join(rest).to_string_lossy().into_owned());
            }
        }
        None
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ApplyStats {
    pub per_table: BTreeMap<String, TableApplyStats>,
    pub files_applied: u64,
    pub files_already_present: u64,
}

impl ApplyStats {
    pub fn zero_growth(&self) -> bool {
        self.files_applied == 0
            && self
                .per_table
                .values()
                .all(|t| t.applied == 0 && t.session_rows_updated == 0)
    }

    pub fn totals_json(&self) -> Value {
        let per: serde_json::Map<String, Value> = self
            .per_table
            .iter()
            .map(|(t, s)| {
                (
                    t.clone(),
                    json!({"applied": s.applied, "alreadyPresent": s.already_present, "sessionRowsUpdated": s.session_rows_updated, "pathScopedPreserved": s.path_scoped_preserved, "pathRebased": s.path_rebased}),
                )
            })
            .collect();
        json!({
            "tables": Value::Object(per),
            "filesApplied": self.files_applied,
            "filesAlreadyPresent": self.files_already_present,
        })
    }
}

/// 回灌 DB 差集：单事务 + FK 关停 + 提交前 foreign_key_check。目标库 pk
/// 形状须与包内行身份元数一致（fresh legacy import 与导出端同版迁移，
/// 不一致=形状漂移响亮拒）。
pub fn apply_db_delta(
    target_db: &Path,
    tables: &[TableDelta],
    rebase: Option<&PathRebase>,
) -> Result<ApplyStats, DeltaError> {
    let mut conn = rusqlite::Connection::open(target_db)
        .map_err(|e| DeltaError::Db(format!("open {} rw: {e}", target_db.display())))?;
    conn.execute_batch("PRAGMA foreign_keys=OFF")
        .map_err(|e| DeltaError::Db(format!("fk off: {e}")))?;
    let tx = conn
        .transaction()
        .map_err(|e| DeltaError::Db(format!("begin tx: {e}")))?;
    let mut stats = ApplyStats::default();
    for delta in tables {
        // 零行表无可回灌，直接跳过：源侧存在而目标侧不存在的空表属正常
        // （subagent_* 等运行时惰性建表族——服务首次使用才 CREATE，不在
        // migrations 链内；t06-d3-drill-06 红实证）。有行而目标缺表仍会在
        // 下方 pk_columns 响亮拒。
        if delta.rows.is_empty() {
            continue;
        }
        let pk_cols = pk_columns(&tx, &delta.table)?;
        if !delta.rows.is_empty() && pk_cols.len() != delta.identity_len() {
            return Err(DeltaError::Refused {
                reason: format!(
                    "table {} pk arity {} != package identity arity {}",
                    delta.table,
                    pk_cols.len(),
                    delta.identity_len()
                ),
            });
        }
        let table_stats = stats.per_table.entry(delta.table.clone()).or_default();
        for row in &delta.rows {
            apply_row(&tx, delta, &pk_cols, row, table_stats, rebase)?;
        }
    }
    // 提交前 FK 全量校验：违规=回滚响亮拒。
    let violations: i64 = tx
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .map_err(|e| DeltaError::Db(format!("fk check: {e}")))?;
    if violations > 0 {
        return Err(DeltaError::Refused {
            reason: format!("delta import left {violations} foreign-key violations; rolled back"),
        });
    }
    tx.commit().map_err(|e| DeltaError::Db(format!("commit: {e}")))?;
    Ok(stats)
}

impl TableDelta {
    fn identity_len(&self) -> usize {
        self.rows
            .first()
            .map(|r| r.identity.len())
            .unwrap_or(self.pk_columns.len())
    }
}

fn apply_row(
    tx: &rusqlite::Transaction<'_>,
    delta: &TableDelta,
    pk_cols: &[String],
    row: &DeltaRow,
    stats: &mut TableApplyStats,
    rebase: Option<&PathRebase>,
) -> Result<(), DeltaError> {
    let table = &delta.table;
    // B①：有效行 = 包行经路径重基（仅 session_files 的 managed_cache 行）。
    // 比对与插入都用有效行：二次回灌时目标行已是重基路径，与有效行相等 →
    // already_present（零增长成立），而不是误落 path_scoped_preserved。
    let mut effective_row = row.row.clone();
    let mut rebased_here = 0u64;
    if table == "session_files" {
        if let Some(rebase) = rebase {
            let managed = effective_row
                .get("storage_kind")
                .and_then(Value::as_str)
                == Some("managed_cache");
            if managed {
                for column in ["file_path", "real_path"] {
                    let Some(Value::String(current)) = effective_row.get(column) else {
                        continue;
                    };
                    if let Some(next) = rebase.rebase(current) {
                        if next != *current {
                            effective_row.insert(column.to_string(), Value::String(next));
                            rebased_here += 1;
                        }
                    }
                }
            }
        }
    }
    let where_clause = pk_cols
        .iter()
        .enumerate()
        .map(|(i, c)| format!("\"{c}\" = ?{}", i + 1))
        .collect::<Vec<_>>()
        .join(" AND ");
    let pk_params: Vec<rusqlite::types::Value> = row
        .identity
        .iter()
        .map(json_to_sql_value)
        .collect::<Result<Vec<_>, _>>()?;
    let existing: Option<BTreeMap<String, Value>> = {
        let mut stmt = tx
            .prepare(&format!("SELECT * FROM \"{table}\" WHERE {where_clause}"))
            .map_err(|e| DeltaError::Db(format!("apply {table}: {e}")))?;
        let col_names: Vec<String> =
            stmt.column_names().iter().map(|s| s.to_string()).collect();
        let mut rows = stmt
            .query(rusqlite::params_from_iter(pk_params.iter()))
            .map_err(|e| DeltaError::Db(format!("apply {table}: {e}")))?;
        match rows
            .next()
            .map_err(|e| DeltaError::Db(format!("apply {table}: {e}")))?
        {
            Some(r) => {
                let mut obj = BTreeMap::new();
                for (i, name) in col_names.iter().enumerate() {
                    let v = r
                        .get_ref(i)
                        .map_err(|e| DeltaError::Db(format!("apply {table}: {e}")))?;
                    obj.insert(name.clone(), sql_value_to_json(v));
                }
                Some(obj)
            }
            None => None,
        }
    };
    match existing {
        None => {
            let cols: Vec<&String> = effective_row.keys().collect();
            let insert = format!(
                "INSERT INTO \"{table}\" ({}) VALUES ({})",
                cols.iter()
                    .map(|c| format!("\"{c}\""))
                    .collect::<Vec<_>>()
                    .join(","),
                (1..=cols.len())
                    .map(|i| format!("?{i}"))
                    .collect::<Vec<_>>()
                    .join(",")
            );
            let params: Vec<rusqlite::types::Value> = cols
                .iter()
                .map(|c| json_to_sql_value(&effective_row[*c]))
                .collect::<Result<Vec<_>, _>>()?;
            tx.execute(&insert, rusqlite::params_from_iter(params.iter()))
                .map_err(|e| DeltaError::Db(format!("apply insert {table}: {e}")))?;
            stats.applied += 1;
            if rebased_here > 0 {
                stats.path_rebased += 1;
            }
        }
        Some(existing_row) => {
            if canonical_row_string(&existing_row) == canonical_row_string(&effective_row) {
                stats.already_present += 1;
                return Ok(());
            }
            // session_files 族：恢复导入已用**本根作用域路径**重建行；包行携带
            // 原新根绝对路径，整体替换会把指向旧根的路径写进恢复根（活性错误）。
            // 裁决：身份冲突且发散 → 目标行胜，计数 path_scoped_preserved（显式
            // 降级披露；新身份附件行不受影响照常导入，字节经包 files 面恢复）。
            // （t06-d3-drill-05 红实证：file-broken-1 路径/生命周期双发散。）
            if table == "session_files"
                || table == "session_file_aliases"
                || table == "session_file_refs"
            {
                stats.path_scoped_preserved += 1;
                return Ok(());
            }
            // 可变元数据/运行态表：新根为切后权威（标题/生命周期/调度簿记
            // next_run_at/updated_at 等），发散行整体替换并计数。sessions 是
            // 设计原文；automation_* 族系 D3 红驱动精化（t06-d3-drill-02/03：
            // legacy 导入会建 legacy-job-* 行、新根服务调度器又合法改写调度
            // 字段——「导入零写入 → L=∅」假设证伪；账本表 messages/runs/
            // model_call_usage/bridge_* 永不到此分支，维持不可改账）。
            if table == "sessions" || table.starts_with("automation_") {
                // 设计裁决：会话元数据可变（标题/生命周期等），发散行整体
                // 替换并计数；消息/账簿表永不到此分支。占位符全程编号：
                // SET 段 ?1..?n（行值），WHERE 段 ?n+1..?n+k（主键值），
                // 不与匿名 ? 混用（SQLite 索引会错位）。
                let set_clause = row
                    .row
                    .keys()
                    .enumerate()
                    .map(|(i, c)| format!("\"{c}\" = ?{}", i + 1))
                    .collect::<Vec<_>>()
                    .join(", ");
                let update_where = pk_cols
                    .iter()
                    .enumerate()
                    .map(|(j, c)| format!("\"{c}\" = ?{}", row.row.len() + j + 1))
                    .collect::<Vec<_>>()
                    .join(" AND ");
                let mut params: Vec<rusqlite::types::Value> = row
                    .row
                    .values()
                    .map(json_to_sql_value)
                    .collect::<Result<Vec<_>, _>>()?;
                params.extend(pk_params.iter().cloned());
                tx.execute(
                    &format!("UPDATE \"{table}\" SET {set_clause} WHERE {update_where}"),
                    rusqlite::params_from_iter(params.iter()),
                )
                .map_err(|e| DeltaError::Db(format!("apply update {table}: {e}")))?;
                stats.session_rows_updated += 1;
            } else {
                return Err(DeltaError::Conflict {
                    table: table.clone(),
                    identity: canonical_identity(row),
                });
            }
        }
    }
    Ok(())
}

/// 回灌文件差集：session-files 同 relpath 异内容=响亮拒；knowledge/
/// preferences 发散=整文件覆盖计数（设计语义：保格式库/设置整文件级）。
pub fn apply_files_delta(
    package_dir: &Path,
    target_root: &Path,
    file_entries: &[FileDeltaEntry],
    stats: &mut ApplyStats,
) -> Result<(), DeltaError> {
    for entry in file_entries {
        let src = package_dir.join("payload/files").join(&entry.relpath);
        let dst = target_root.join(&entry.relpath);
        if dst.exists() {
            let dst_bytes = std::fs::read(&dst).map_err(|e| io_err(&dst, e))?;
            let dst_sha = sha256_hex(&dst_bytes);
            if dst_sha == entry.sha256 {
                stats.files_already_present += 1;
                continue;
            }
            if entry.category == "session-files" {
                return Err(DeltaError::Refused {
                    reason: format!(
                        "session-files payload {} conflicts with different existing content in target; register content is immutable",
                        entry.relpath
                    ),
                });
            }
        }
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io_err(parent, e))?;
        }
        std::fs::copy(&src, &dst).map_err(|e| io_err(&src, e))?;
        stats.files_applied += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_root(tag: &str) -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "t06-delta-test-{}-{}-{tag}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_old_root() -> PathBuf {
        let root = temp_root("old");
        std::fs::create_dir_all(root.join("agents/a1/sessions")).unwrap();
        std::fs::write(
            root.join("agents/a1/sessions/s1.jsonl"),
            concat!(
                "{\"entryType\":\"sessionMeta\",\"createdAt\":1700000000000}\n",
                "{\"entryType\":\"runBoundary\",\"runId\":\"r1\",\"phase\":\"start\",\"ts\":1700000000001}\n",
                "{\"id\":\"m1\",\"role\":\"user\",\"content\":\"hi\",\"ts\":1700000000002}\n",
                "{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"yo\"}]}\n",
                "{\"entryType\":\"toolCall\",\"id\":\"t1\",\"runId\":\"r1\"}\n",
                "{\"entryType\":\"weirdFutureBlock\",\"x\":1}\n",
                "{\"entryType\":\"runBoundary\",\"runId\":\"r1\",\"phase\":\"end\",\"ts\":1700000000009}\n"
            ),
        )
        .unwrap();
        std::fs::create_dir_all(root.join("session-files/ab")).unwrap();
        std::fs::write(root.join("session-files/ab/abcdef"), b"payload-bytes").unwrap();
        std::fs::create_dir_all(root.join("knowledge")).unwrap();
        std::fs::write(root.join("knowledge/knowledge.db"), b"kdb-bytes").unwrap();
        std::fs::create_dir_all(root.join("user")).unwrap();
        std::fs::write(root.join("user/preferences.json"), b"{\"theme\":\"dark\"}").unwrap();
        root
    }

    #[test]
    fn identity_derivation_matches_converter_rules() {
        let old = make_old_root();
        let set = derive_legacy_identity_set(&old).unwrap();
        assert!(set.sessions.contains("s1"));
        assert!(set.runs.contains("r1"));
        assert!(set.messages.contains(&("s1".into(), "m1".into())));
        // 缺 id → fallback legacy-s1-4（第 4 物理行）。
        assert!(set.messages.contains(&("s1".into(), "legacy-s1-4".into())));
        assert!(set.messages.contains(&("s1".into(), "t1".into())));
        // 未知 entryType → 标记消息（第 6 行）。
        assert!(set
            .messages
            .contains(&("s1".into(), "legacy-unknown-block-s1-6".into())));
        assert_eq!(set.session_files.len(), 1);
        assert_eq!(set.knowledge_files.len(), 1);
        assert!(set.preferences_sha256.is_some());
        // 确定性：两次推导同哈希。
        let again = derive_legacy_identity_set(&old).unwrap();
        assert_eq!(set.identity_set_sha256(), again.identity_set_sha256());
        std::fs::remove_dir_all(&old).unwrap();
    }

    #[test]
    fn unmapped_jsonl_shape_skipped_not_guessed() {
        let root = temp_root("unmapped");
        std::fs::create_dir_all(root.join("stray")).unwrap();
        // 不在 agents/*/sessions/** 下，但撞上 session-jsonl 类模式的情况：
        // 直接用 agents 外路径验证 session_context 门（类模式不命中时分类为
        // 其它类/未映射，跳过）。
        std::fs::create_dir_all(root.join("agents/a1")).unwrap();
        std::fs::write(root.join("agents/a1/notes.jsonl"), b"{\"id\":\"x\"}\n").unwrap();
        let set = derive_legacy_identity_set(&root).unwrap();
        assert!(!set.sessions.contains("notes"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    fn open_test_db(path: &Path) -> rusqlite::Connection {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (session_id TEXT PRIMARY KEY, title TEXT, n INTEGER);
             CREATE TABLE runs (run_id TEXT PRIMARY KEY, session_id TEXT REFERENCES sessions(session_id));
             CREATE TABLE messages (message_id TEXT NOT NULL, session_id TEXT NOT NULL REFERENCES sessions(session_id), body TEXT, PRIMARY KEY (session_id, message_id));
             CREATE TABLE import_runs (run_id TEXT PRIMARY KEY);
             CREATE TABLE bridge_outbox_v1 (delivery_id TEXT PRIMARY KEY, status TEXT);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn export_filters_legacy_identities_and_bookkeeping() {
        let dir = temp_root("export");
        let db = dir.join("runs.db");
        let conn = open_test_db(&db);
        conn.execute_batch(
            "INSERT INTO sessions VALUES ('s1','legacy session',1),('s-new','new session',2);
             INSERT INTO runs VALUES ('r1','s1'),('r-new','s-new');
             INSERT INTO messages VALUES ('m1','s1','old'),('m-new','s-new','new');
             INSERT INTO import_runs VALUES ('bookkeeping');
             INSERT INTO bridge_outbox_v1 VALUES ('d1','confirmed');",
        )
        .unwrap();
        drop(conn);
        let mut legacy = LegacyIdentitySet::default();
        legacy.sessions.insert("s1".into());
        legacy.messages.insert(("s1".into(), "m1".into()));
        legacy.runs.insert("r1".into());
        let tables = export_db_delta(&db, &legacy).unwrap();
        let by_name: BTreeMap<_, _> = tables.iter().map(|t| (t.table.as_str(), t)).collect();
        // 簿记表不出现。
        assert!(!by_name.contains_key("import_runs"));
        // B③：sessions 为可变元数据表——旧身份行也入包（回灌侧按内容判
        // already_present / session_rows_updated），新身份行照常入包。
        assert_eq!(by_name["sessions"].rows.len(), 2);
        let session_row = |id: &str| {
            by_name["sessions"]
                .rows
                .iter()
                .find(|r| r.identity == vec![json!(id)])
                .unwrap_or_else(|| panic!("sessions row {id} missing"))
        };
        // J7：新根新建会话 oldUiReadable=false；旧根原有会话 oldUiReadable=true。
        assert_eq!(session_row("s-new").old_ui_readable, Some(false));
        assert_eq!(session_row("s1").old_ui_readable, Some(true));
        // 账簿表（messages/runs）仍按旧身份过滤：不可改账，旧数据保留。
        assert_eq!(by_name["messages"].rows.len(), 1);
        assert_eq!(by_name["messages"].rows[0].identity, vec![json!("s-new"), json!("m-new")]);
        assert_eq!(by_name["runs"].rows.len(), 1);
        // L=∅ 表全行为增量。
        assert_eq!(by_name["bridge_outbox_v1"].rows.len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn package_roundtrip_and_tamper_detection() {
        let dir = temp_root("pkg");
        let new_root = dir.join("new");
        std::fs::create_dir_all(new_root.join("session-files/zz")).unwrap();
        std::fs::write(new_root.join("session-files/zz/newfile"), b"new-bytes").unwrap();
        let db = dir.join("runs.db");
        let conn = open_test_db(&db);
        conn.execute_batch("INSERT INTO sessions VALUES ('s-x','t',0);").unwrap();
        drop(conn);
        let legacy = LegacyIdentitySet::default();
        let tables = export_db_delta(&db, &legacy).unwrap();
        let new_manifest = build_manifest(&new_root).unwrap();
        let files = diff_files(&new_root, &new_manifest, &legacy).unwrap();
        assert_eq!(files.len(), 1);
        let pkg = dir.join("pkg");
        let lineage = json!({"rollbackId":"rb-1","createdAt":"2026-10-23T00:00:00Z"});
        let manifest =
            write_delta_package(&pkg, &tables, &files, &new_root, &lineage, &legacy).unwrap();
        let (back, tables_back, files_back) = read_delta_package(&pkg).unwrap();
        assert_eq!(back, manifest);
        assert_eq!(tables_back.len(), tables.len());
        assert_eq!(files_back, files);
        // 篡改行 payload → 响亮拒。
        let sessions_rel = manifest
            .tables
            .iter()
            .find(|(t, _, _, _)| t == "sessions")
            .map(|(_, _, rel, _)| rel.clone())
            .unwrap();
        let p = pkg.join(&sessions_rel);
        let mut bytes = std::fs::read(&p).unwrap();
        bytes[10] = b'X';
        std::fs::write(&p, bytes).unwrap();
        let err = read_delta_package(&pkg).unwrap_err().to_string();
        assert!(err.contains("sha256 mismatch"), "unexpected: {err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn apply_is_idempotent_and_conflicts_loudly() {
        let dir = temp_root("apply");
        let db = dir.join("runs.db");
        let conn = open_test_db(&db);
        conn.execute_batch(
            "INSERT INTO sessions VALUES ('s1','legacy title',1);
             INSERT INTO runs VALUES ('r1','s1');
             INSERT INTO messages VALUES ('m1','s1','original');",
        )
        .unwrap();
        drop(conn);
        let tables = vec![
            TableDelta {
                table: "sessions".into(),
                pk_columns: vec!["session_id".into()],
                rows: vec![DeltaRow {
                    identity: vec![json!("s1")],
                    row: [("session_id".into(), json!("s1")), ("title".into(), json!("renamed post-cutover")), ("n".into(), json!(1))].into_iter().collect(),
                    row_sha256: String::new(),
                    old_ui_readable: Some(false),
                }],
            },
            TableDelta {
                table: "messages".into(),
                pk_columns: vec!["session_id".into(), "message_id".into()],
                rows: vec![
                    DeltaRow {
                        identity: vec![json!("s1"), json!("m1")],
                        row: [("message_id".into(), json!("m1")), ("session_id".into(), json!("s1")), ("body".into(), json!("original"))].into_iter().collect(),
                        row_sha256: String::new(),
                        old_ui_readable: None,
                    },
                    DeltaRow {
                        identity: vec![json!("s1"), json!("m-new")],
                        row: [("message_id".into(), json!("m-new")), ("session_id".into(), json!("s1")), ("body".into(), json!("new body"))].into_iter().collect(),
                        row_sha256: String::new(),
                        old_ui_readable: None,
                    },
                ],
            },
        ];
        let first = apply_db_delta(&db, &tables, None).unwrap();
        assert_eq!(first.per_table["sessions"].session_rows_updated, 1);
        assert_eq!(first.per_table["messages"].applied, 1);
        assert_eq!(first.per_table["messages"].already_present, 1);
        // 二次回灌 = 零增长（J4 证明原料）。
        let second = apply_db_delta(&db, &tables, None).unwrap();
        assert!(second.zero_growth(), "second apply: {second:?}");
        // 异内容消息 → conflict 响亮拒（不可改账）。
        let mut conflict = tables.clone();
        conflict[1].rows[1]
            .row
            .insert("body".into(), json!("rewritten"));
        let err = apply_db_delta(&db, &conflict, None).unwrap_err().to_string();
        assert!(err.contains("conflict"), "unexpected: {err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn session_files_rows_rebase_managed_cache_paths_only() {
        // B①：managed_cache 行 file_path/real_path 由原新根重基到恢复根；
        // external 行不改写；二次回灌同包 already_present（零增长）。
        let dir = temp_root("rebase");
        let db = dir.join("runs.db");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE session_files (file_id TEXT PRIMARY KEY, storage_kind TEXT, file_path TEXT, real_path TEXT);",
        )
        .unwrap();
        drop(conn);
        let origin = PathBuf::from("/tmp/origin-root");
        let origin_canonical = PathBuf::from("/private/tmp/origin-root");
        let target = PathBuf::from("/private/tmp/recover-root");
        let row = |id: &str, kind: &str, p: &str, r: &str| DeltaRow {
            identity: vec![json!(id)],
            row: [
                ("file_id".into(), json!(id)),
                ("storage_kind".into(), json!(kind)),
                ("file_path".into(), json!(p)),
                ("real_path".into(), json!(r)),
            ]
            .into_iter()
            .collect(),
            row_sha256: String::new(),
            old_ui_readable: None,
        };
        let tables = vec![TableDelta {
            table: "session_files".into(),
            pk_columns: vec!["file_id".into()],
            rows: vec![
                row("sf_managed", "managed_cache", "/tmp/origin-root/session-files/ab/x.bin", "/private/tmp/origin-root/session-files/ab/x.bin"),
                row("sf_external", "external", "/Users/u/Documents/own.pdf", "/Users/u/Documents/own.pdf"),
            ],
        }];
        let rebase = PathRebase { origins: vec![origin, origin_canonical], target: target.clone() };
        let first = apply_db_delta(&db, &tables, Some(&rebase)).unwrap();
        assert_eq!(first.per_table["session_files"].applied, 2);
        assert_eq!(first.per_table["session_files"].path_rebased, 1);
        let conn = rusqlite::Connection::open(&db).unwrap();
        let (fp, rp): (String, String) = conn
            .query_row("SELECT file_path, real_path FROM session_files WHERE file_id='sf_managed'", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(fp, "/private/tmp/recover-root/session-files/ab/x.bin");
        assert_eq!(rp, "/private/tmp/recover-root/session-files/ab/x.bin");
        let ext: String = conn
            .query_row("SELECT file_path FROM session_files WHERE file_id='sf_external'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ext, "/Users/u/Documents/own.pdf");
        drop(conn);
        let second = apply_db_delta(&db, &tables, Some(&rebase)).unwrap();
        assert!(second.zero_growth(), "second apply: {second:?}");
        assert_eq!(second.per_table["session_files"].already_present, 2);
        assert_eq!(second.per_table["session_files"].path_scoped_preserved, 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn fk_violation_rolls_back_loudly() {
        let dir = temp_root("fk");
        let db = dir.join("runs.db");
        let conn = open_test_db(&db);
        conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
        drop(conn);
        let tables = vec![TableDelta {
            table: "messages".into(),
            pk_columns: vec!["session_id".into(), "message_id".into()],
            rows: vec![DeltaRow {
                identity: vec![json!("ghost"), json!("m-x")],
                row: [("message_id".into(), json!("m-x")), ("session_id".into(), json!("ghost")), ("body".into(), json!("orphan"))].into_iter().collect(),
                row_sha256: String::new(),
                old_ui_readable: None,
            }],
        }];
        let err = apply_db_delta(&db, &tables, None).unwrap_err().to_string();
        assert!(err.contains("foreign-key"), "unexpected: {err}");
        // 回滚后行不存在。
        let conn = rusqlite::Connection::open(&db).unwrap();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn session_files_mutation_refused() {
        let dir = temp_root("sfx");
        let new_root = dir.join("new");
        std::fs::create_dir_all(new_root.join("session-files/ab")).unwrap();
        std::fs::write(new_root.join("session-files/ab/abcdef"), b"tampered").unwrap();
        let mut legacy = LegacyIdentitySet::default();
        legacy
            .session_files
            .insert("session-files/ab/abcdef".into(), "different-sha".into());
        let new_manifest = build_manifest(&new_root).unwrap();
        let err = diff_files(&new_root, &new_manifest, &legacy)
            .unwrap_err()
            .to_string();
        assert!(err.contains("mutated"), "unexpected: {err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
