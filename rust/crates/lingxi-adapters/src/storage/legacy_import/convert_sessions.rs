//! B2 切片 A：session-jsonl 类转换器。
//!
//! 写入纪律（T04_DESIGN ①-6 的落实细化）：
//! - 会话/消息走**同一条已验证写路径**（`session_tree::seed_session` /
//!   `append_message`），importer 内建 current_thread tokio runtime 驱动；
//!   离线语义不变（无网络、无服务进程，仅复用 async 存储核心）。
//! - 设计稿的「500 行一事务」细化为**消息级事务**（每条 append 是 canonical
//!   写路径自身的原子提交）：kill 落点更小，恢复幂等键=(session_id,
//!   message_id) 由本条路径的主键天然承担。簿记进度按批更新。
//! - runBoundary 物化为 legacy runs 行（簿记连接直插，列映射 1:1；
//!   无 live run driver 参与，design ⑥-1：绝不发明用户任务）。
//! - 幂等重跑：会话存在→跳过计数；消息同键同内容→already-present，
//!   同键异内容→conflict（error，批次 failed，不覆盖不合并）。
//!
//! 条目映射（读面已取证 history_projection.rs）：
//! - user/assistant/system（或无 entryType 带 role）→ entry_type='message'，
//!   content_json = 原始条目全字段透传 + role/content 规范化（文本字符串
//!   归一为 content blocks；未知字段原样随行 → legacyFields 可见）。
//! - toolCall/toolResult 等已知非消息条目 → entry_type=原值，role='legacy'，
//!   content_json 全字段透传（不冒充聊天气泡，也不进 quarantine）。
//! - 未知 entryType → 原始行字节进 quarantine + 追加
//!   entry_type='legacy-unknown-block' 标记消息（opaque 块指向归档）。
//! - 缺 Run 边界的会话：全部消息 run_id=NULL + info 级
//!   missing-run-boundary 问题条目；声明了不存在 runId 的消息同样落 NULL。

use super::session_jsonl::parse_session_jsonl;
use super::{ImportError, RECEIPT_DIR_RELPATH};
use rusqlite::Connection;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

use crate::storage::run_store::RunDatabase;
use crate::storage::session_tree::{append_message, seed_session, AppendMessage, SeedSession};
use crate::storage::StoreOptions;

/// 一条 fork 边（父会话解析出 fork 条目时记录，批后统一落谱系列）。
#[derive(Debug, Clone)]
pub struct ForkEdge {
    pub parent_session: String,
    pub child_session: String,
    /// fork 点时父会话最后一条消息 id。
    pub fork_point_message_id: Option<String>,
}

/// 单会话转换计数。
#[derive(Debug, Default)]
pub struct ConvertStats {
    pub imported: u64,
    pub already_present: u64,
    pub quarantined_unknown: u64,
    pub corrupt_lines: u64,
    pub corrupt_tail: bool,
    pub boundaries: u64,
    pub conflicts: u64,
    pub missing_run_boundary: bool,
    /// 处理过的可解析条目总数（含 control/boundary/fork/meta）。
    pub processed: u64,
    /// 物理总行数（含空行与损坏行）→ 批次 rows_planned。
    pub physical_lines: u64,
    pub fork_edges: Vec<ForkEdge>,
}

pub struct SessionContext {
    pub session_id: String,
    pub agent_id: String,
    pub archived: bool,
}

/// 从批次 relpath 推导会话上下文。
/// 期望形状 `agents/{agent}/sessions/**/{session}.jsonl`；不合形状 → None
/// （调用方记 unmapped-shape 问题，跳过不猜）。
pub fn session_context_from_relpath(relpath: &str) -> Option<SessionContext> {
    let path = Path::new(relpath);
    let stem = path.file_stem()?.to_str()?.to_string();
    let mut comps = path.components();
    let agents = comps.next()?;
    if agents.as_os_str() != "agents" {
        return None;
    }
    let agent = comps.next()?.as_os_str().to_str()?.to_string();
    if comps.next()?.as_os_str() != "sessions" {
        return None;
    }
    let archived = relpath.contains("/archived/");
    Some(SessionContext {
        session_id: stem,
        agent_id: agent,
        archived,
    })
}

fn quarantine_dir(target: &Path) -> PathBuf {
    target
        .join(RECEIPT_DIR_RELPATH)
        .join("quarantine")
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// 消息 content_json：全字段透传 + role/content 规范化。
fn build_message_content(raw: &Value, role: &str) -> String {
    let mut obj: Map<String, Value> = raw.as_object().cloned().unwrap_or_default();
    obj.insert("role".into(), json!(role));
    // T07 F-T07-1 修复（加性）：真实旧栈 Pi SDK 条目的 content 嵌于
    // message.content —— 顶层 content 缺省时提升嵌套值为规范化来源
    // （message 原文继续全字段透传）；夹具条目恒有顶层 content，零变化。
    let effective = obj
        .get("content")
        .cloned()
        .or_else(|| {
            obj.get("message")
                .and_then(|m| m.get("content"))
                .cloned()
        });
    match effective {
        Some(Value::Array(_)) if obj.contains_key("content") => {}
        Some(Value::Array(items)) => {
            obj.insert("content".into(), Value::Array(items));
        }
        Some(Value::String(text)) => {
            obj.insert("content".into(), json!([{ "type": "text", "text": text }]));
        }
        Some(other) => {
            // 非字符串非数组的 content：原值保留进 legacyContent，读面仍可见。
            obj.insert("legacyContent".into(), other);
            obj.insert("content".into(), json!([]));
        }
        None => {
            obj.insert("content".into(), json!([]));
        }
    }
    Value::Object(obj).to_string()
}

const MESSAGE_ENTRY_TYPES: [&str; 3] = ["user", "assistant", "system"];
const KNOWN_CONTROL_TYPES: [&str; 3] = ["runBoundary", "sessionMeta", "fork"];
const KNOWN_NONMESSAGE_TYPES: [&str; 2] = ["toolCall", "toolResult"];

fn insert_legacy_run(
    conn: &Connection,
    run_id: &str,
    session_id: &str,
    agent_id: &str,
    terminal: bool,
    ts: i64,
) -> Result<bool, ImportError> {
    // 幂等：同 id 存在 → 校验归属一致，冲突响亮。
    let existing: Option<(String,)> = conn
        .query_row(
            "SELECT session_id FROM runs WHERE run_id = ?1",
            [run_id],
            |row| Ok((row.get::<_, String>(0)?,)),
        )
        .optional()
        .map_err(|e| ImportError::Db(e.to_string()))?;
    if let Some((owner_session,)) = existing {
        if owner_session != session_id {
            return Err(ImportError::Db(format!(
                "legacy run {run_id} already bound to session {owner_session}, refuse rebind to {session_id}"
            )));
        }
        return Ok(false);
    }
    conn.execute(
        "INSERT INTO runs \
         (run_id, session_id, owner_kind, owner_subject, principal_id, attempt_count, \
          status, terminal_reason, generation, created_at_unix_ms, updated_at_unix_ms, last_event_seq) \
         VALUES (?1,?2,'legacy-import',?3,'legacy-import',0,?4,?5,0,?6,?6,0)",
        rusqlite::params![
            run_id,
            session_id,
            agent_id,
            if terminal { "completed" } else { "unknown" },
            if terminal {
                Some("legacy-boundary-end")
            } else {
                None
            },
            ts
        ],
    )
    .map_err(|e| ImportError::Db(e.to_string()))?;
    Ok(true)
}

fn session_exists(conn: &Connection, session_id: &str) -> Result<bool, ImportError> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sessions WHERE session_id = ?1)",
        [session_id],
        |row| row.get(0),
    )
    .map_err(|e| ImportError::Db(e.to_string()))
}

fn existing_message_content(
    conn: &Connection,
    session_id: &str,
    message_id: &str,
) -> Result<Option<String>, ImportError> {
    conn.query_row(
        "SELECT content_json FROM messages WHERE session_id = ?1 AND message_id = ?2",
        rusqlite::params![session_id, message_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| ImportError::Db(e.to_string()))
}

use rusqlite::OptionalExtension;

#[allow(clippy::too_many_arguments)]
pub fn convert_session_file(
    conn: &Connection,
    rt: &tokio::runtime::Runtime,
    db: &RunDatabase,
    run_id: &str,
    batch_key: &str,
    source_root: &Path,
    relpath: &str,
    target_root: &Path,
    issue_seq: &mut i64,
) -> Result<ConvertStats, ImportError> {
    let mut stats = ConvertStats::default();
    let ctx = match session_context_from_relpath(relpath) {
        Some(ctx) => ctx,
        None => {
            *issue_seq += 1;
            super::insert_issue(
                conn,
                run_id,
                *issue_seq,
                batch_key,
                "warning",
                "unmapped-shape",
                Some(relpath),
                "session-jsonl path does not match agents/{agent}/sessions/**; skipped without guessing",
            )?;
            return Ok(stats);
        }
    };
    let bytes = std::fs::read(source_root.join(relpath)).map_err(|e| ImportError::Io {
        path: source_root.join(relpath),
        message: e.to_string(),
    })?;
    let parsed = parse_session_jsonl(&bytes);
    stats.physical_lines = parsed.total_lines;

    // 会话种子（canonical 写路径）；已存在 → already-imported 计数（resume 幂等）。
    let now = super::now_unix_ms();
    let session_created = parsed
        .entries
        .iter()
        .find(|e| e.entry_type.as_deref() == Some("sessionMeta"))
        .and_then(|e| e.raw.get("createdAt").and_then(Value::as_i64))
        .unwrap_or(now);
    if session_exists(conn, &ctx.session_id)? {
        stats.already_present += 1;
    } else {
        rt.block_on(seed_session(
            db,
            SeedSession {
                session_id: &ctx.session_id,
                agent_id: &ctx.agent_id,
                owner_user_id: "legacy-import",
                title: &ctx.session_id,
                now_unix_ms: session_created.max(0) as u64,
                permission_mode: None,
                authorized_folders: vec![],
            },
        ))
        .map_err(|e| ImportError::Db(format!("seed_session {}: {e}", ctx.session_id)))?;
    }
    if ctx.archived {
        conn.execute(
            "UPDATE sessions SET lifecycle = 'archived', archived_at_unix_ms = COALESCE(archived_at_unix_ms, ?2) WHERE session_id = ?1",
            rusqlite::params![ctx.session_id, session_created],
        )
        .map_err(|e| ImportError::Db(e.to_string()))?;
    }

    // runBoundary 先行：建本文件声明的 legacy run 集合（消息归属判定用）。
    let mut known_runs: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut open_runs: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for entry in &parsed.entries {
        if entry.entry_type.as_deref() != Some("runBoundary") {
            continue;
        }
        let Some(rid) = entry.run_id.clone() else { continue };
        let phase = entry
            .raw
            .get("phase")
            .or_else(|| entry.raw.get("kind"))
            .and_then(Value::as_str);
        match phase {
            Some("start") => {
                open_runs.insert(rid.clone(), entry.ts.unwrap_or(session_created));
            }
            Some("end") => {
                let started = open_runs.remove(&rid).unwrap_or(entry.ts.unwrap_or(session_created));
                if insert_legacy_run(conn, &rid, &ctx.session_id, &ctx.agent_id, true, started)? {
                    stats.boundaries += 1;
                }
                known_runs.insert(rid);
            }
            _ => {}
        }
    }
    // 起始无结束：依然物化（status='unknown'，诚实呈现边界残缺）。
    for (rid, started) in open_runs {
        if insert_legacy_run(conn, &rid, &ctx.session_id, &ctx.agent_id, false, started)? {
            stats.boundaries += 1;
        }
        known_runs.insert(rid);
    }

    let has_boundaries = !known_runs.is_empty();
    let mut last_message_id: Option<String> = None;
    let mut reported_missing_runs: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    for entry in &parsed.entries {
        stats.processed += 1;
        let etype = entry.entry_type.as_deref();
        if KNOWN_CONTROL_TYPES.contains(&etype.unwrap_or("")) {
            if etype == Some("fork") {
                if let Some(child) = entry.raw.get("childSessionId").and_then(Value::as_str) {
                    stats.fork_edges.push(ForkEdge {
                        parent_session: ctx.session_id.clone(),
                        child_session: child.to_string(),
                        fork_point_message_id: last_message_id.clone(),
                    });
                }
            }
            continue;
        }
        let is_message = match etype {
            Some(t) => MESSAGE_ENTRY_TYPES.contains(&t),
            None => entry.role.is_some(),
        };
        let declared_run = entry.run_id.clone();
        let run_id_for_message = match &declared_run {
            Some(rid) if known_runs.contains(rid) => Some(rid.clone()),
            Some(rid) => {
                if reported_missing_runs.insert(rid.clone()) {
                    *issue_seq += 1;
                    super::insert_issue(
                        conn,
                        run_id,
                        *issue_seq,
                        batch_key,
                        "info",
                        "missing-run-boundary",
                        Some(rid),
                        &format!(
                            "message declares runId {rid} but no runBoundary materializes it; imported with run_id NULL (no invented run)"
                        ),
                    )?;
                }
                None
            }
            None => None,
        };
        if is_message {
            let role = entry
                .role
                .clone()
                .or_else(|| etype.map(str::to_string))
                .unwrap_or_else(|| "legacy".into());
            let message_id = entry
                .id
                .clone()
                .unwrap_or_else(|| format!("legacy-{}-{}", ctx.session_id, entry.line_no));
            let content = build_message_content(&entry.raw, &role);
            match existing_message_content(conn, &ctx.session_id, &message_id)? {
                Some(existing) if existing == content => {
                    stats.already_present += 1;
                    last_message_id = Some(message_id);
                    continue;
                }
                Some(_) => {
                    stats.conflicts += 1;
                    *issue_seq += 1;
                    super::insert_issue(
                        conn,
                        run_id,
                        *issue_seq,
                        batch_key,
                        "error",
                        "conflict",
                        Some(&message_id),
                        "same (session_id,message_id) with different content; not overwritten, batch failed",
                    )?;
                    continue;
                }
                None => {}
            }
            rt.block_on(append_message(
                db,
                AppendMessage {
                    session_id: &ctx.session_id,
                    message_id: &message_id,
                    parent_message_id: last_message_id.as_deref(),
                    run_id: run_id_for_message.as_deref(),
                    role: &role,
                    content_json: &content,
                    entry_type: "message",
                    model_call_id: None,
                    now_unix_ms: entry.ts.unwrap_or(session_created).max(0) as u64,
                },
            ))
            .map_err(|e| ImportError::Db(format!("append_message {message_id}: {e}")))?;
            stats.imported += 1;
            last_message_id = Some(message_id);
        } else if KNOWN_NONMESSAGE_TYPES.contains(&etype.unwrap_or("")) {
            let message_id = entry
                .id
                .clone()
                .unwrap_or_else(|| format!("legacy-{}-{}", ctx.session_id, entry.line_no));
            let content = build_message_content(&entry.raw, "legacy");
            match existing_message_content(conn, &ctx.session_id, &message_id)? {
                Some(existing) if existing == content => {
                    stats.already_present += 1;
                    last_message_id = Some(message_id);
                    continue;
                }
                Some(_) => {
                    stats.conflicts += 1;
                    *issue_seq += 1;
                    super::insert_issue(
                        conn,
                        run_id,
                        *issue_seq,
                        batch_key,
                        "error",
                        "conflict",
                        Some(&message_id),
                        "same (session_id,message_id) with different content; not overwritten, batch failed",
                    )?;
                    continue;
                }
                None => {}
            }
            rt.block_on(append_message(
                db,
                AppendMessage {
                    session_id: &ctx.session_id,
                    message_id: &message_id,
                    parent_message_id: last_message_id.as_deref(),
                    run_id: run_id_for_message.as_deref(),
                    role: "legacy",
                    content_json: &content,
                    entry_type: etype.unwrap_or("legacy"),
                    model_call_id: None,
                    now_unix_ms: entry.ts.unwrap_or(session_created).max(0) as u64,
                },
            ))
            .map_err(|e| ImportError::Db(format!("append_message {message_id}: {e}")))?;
            stats.imported += 1;
            last_message_id = Some(message_id);
        } else {
            // 未知 entryType：原始字节归档 + 标记消息（design ⑥-2）。
            let qdir = quarantine_dir(target_root)
                .join("unknown-blocks")
                .join(&ctx.session_id);
            std::fs::create_dir_all(&qdir).map_err(|e| ImportError::Io {
                path: qdir.clone(),
                message: e.to_string(),
            })?;
            let qpath = qdir.join(format!("{}.raw", entry.line_no));
            let raw_bytes = entry.raw.to_string();
            std::fs::write(&qpath, raw_bytes.as_bytes()).map_err(|e| ImportError::Io {
                path: qpath.clone(),
                message: e.to_string(),
            })?;
            let sha = sha256_hex(raw_bytes.as_bytes());
            let rel_q = qpath
                .strip_prefix(target_root)
                .unwrap_or(&qpath)
                .to_string_lossy()
                .to_string();
            let original_type = etype.unwrap_or("<none>");
            let marker_id = format!("legacy-unknown-block-{}-{}", ctx.session_id, entry.line_no);
            let marker_content = json!({
                "role": "system",
                "content": [{
                    "type": "opaque",
                    "provider": "legacy-import",
                    "data": {
                        "originalEntryType": original_type,
                        "line": entry.line_no,
                        "sha256": sha,
                        "quarantinePath": rel_q,
                    }
                }]
            })
            .to_string();
            if existing_message_content(conn, &ctx.session_id, &marker_id)?.is_none() {
                rt.block_on(append_message(
                    db,
                    AppendMessage {
                        session_id: &ctx.session_id,
                        message_id: &marker_id,
                        parent_message_id: last_message_id.as_deref(),
                        run_id: None,
                        role: "system",
                        content_json: &marker_content,
                        entry_type: "legacy-unknown-block",
                        model_call_id: None,
                        now_unix_ms: entry.ts.unwrap_or(session_created).max(0) as u64,
                    },
                ))
                .map_err(|e| ImportError::Db(format!("append marker {marker_id}: {e}")))?;
            }
            last_message_id = Some(marker_id);
            stats.quarantined_unknown += 1;
            *issue_seq += 1;
            super::insert_issue(
                conn,
                run_id,
                *issue_seq,
                batch_key,
                "warning",
                "unknown-block",
                Some(&format!("{}:{}", ctx.session_id, entry.line_no)),
                &format!(
                    "unknown entryType {original_type} quarantined to {rel_q} (sha256 {}…)",
                    sha.chars().take(8).collect::<String>()
                ),
            )?;
        }
    }

    // 触发条件统一按「会话是否有任何内容行」（导入/已存在/隔离未知块）判定：
    // 半程 kill 后重跑会把已落库的标记消息计入 already_present，若只看
    // imported+already_present，干净跑与重跑会产出不同的诊断集（A08 幂等缺口）。
    if !has_boundaries && stats.imported + stats.already_present + stats.quarantined_unknown > 0 {
        stats.missing_run_boundary = true;
        *issue_seq += 1;
        super::insert_issue(
            conn,
            run_id,
            *issue_seq,
            batch_key,
            "info",
            "missing-run-boundary",
            Some(&ctx.session_id),
            "session has no runBoundary entries; all messages imported with run_id NULL (legacy_unknown)",
        )?;
    }

    // 损坏行：中间行 + 截断尾行各自归档、各自问题条目（design ⑥-5）。
    for corrupt in &parsed.corrupt_lines {
        let qdir = quarantine_dir(target_root)
            .join("corrupt-lines")
            .join(&ctx.session_id);
        std::fs::create_dir_all(&qdir).map_err(|e| ImportError::Io {
            path: qdir.clone(),
            message: e.to_string(),
        })?;
        let qpath = qdir.join(format!("{}.raw", corrupt.line_no));
        std::fs::write(&qpath, &corrupt.bytes).map_err(|e| ImportError::Io {
            path: qpath.clone(),
            message: e.to_string(),
        })?;
        stats.corrupt_lines += 1;
        *issue_seq += 1;
        super::insert_issue(
            conn,
            run_id,
            *issue_seq,
            batch_key,
            "warning",
            "corrupt-line",
            Some(&format!("{}:{}", ctx.session_id, corrupt.line_no)),
            &format!(
                "mid-file line {} is not valid JSON; {} bytes quarantined",
                corrupt.line_no,
                corrupt.bytes.len()
            ),
        )?;
    }
    if let Some(tail) = &parsed.corrupt_tail {
        let qdir = quarantine_dir(target_root)
            .join("corrupt-tails")
            .join(&ctx.session_id);
        std::fs::create_dir_all(&qdir).map_err(|e| ImportError::Io {
            path: qdir.clone(),
            message: e.to_string(),
        })?;
        let qpath = qdir.join(format!("tail-{}.raw", tail.line_no));
        std::fs::write(&qpath, &tail.bytes).map_err(|e| ImportError::Io {
            path: qpath.clone(),
            message: e.to_string(),
        })?;
        stats.corrupt_tail = true;
        *issue_seq += 1;
        super::insert_issue(
            conn,
            run_id,
            *issue_seq,
            batch_key,
            "warning",
            "corrupt-tail",
            Some(&format!("{}:{}", ctx.session_id, tail.line_no)),
            &format!(
                "final line {} truncated mid-JSON; {} bytes quarantined; intact prefix imported",
                tail.line_no,
                tail.bytes.len()
            ),
        )?;
    }

    Ok(stats)
}

/// 批后谱系列落地：fork 边 → sessions.parent_session_id/fork_point/depth。
/// 子会话不在本次导入集合 → fork-target-missing（warning，不造假想）。
pub fn apply_fork_edges(
    conn: &Connection,
    run_id: &str,
    edges: &[ForkEdge],
    issue_seq: &mut i64,
) -> Result<(), ImportError> {
    for edge in edges {
        if !session_exists(conn, &edge.child_session)? {
            *issue_seq += 1;
            super::insert_issue(
                conn,
                run_id,
                *issue_seq,
                "session-jsonl",
                "warning",
                "fork-target-missing",
                Some(&edge.child_session),
                &format!(
                    "fork edge {} → {} names a child session not present in source; lineage left NULL",
                    edge.parent_session, edge.child_session
                ),
            )?;
            continue;
        }
        let parent_depth: i64 = conn
            .query_row(
                "SELECT lineage_depth FROM sessions WHERE session_id = ?1",
                [&edge.parent_session],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| ImportError::Db(e.to_string()))?
            .unwrap_or(0);
        conn.execute(
            "UPDATE sessions SET parent_session_id = ?2, fork_point_message_id = ?3, lineage_depth = ?4 \
             WHERE session_id = ?1 AND parent_session_id IS NULL",
            rusqlite::params![
                edge.child_session,
                edge.parent_session,
                edge.fork_point_message_id,
                parent_depth + 1
            ],
        )
        .map_err(|e| ImportError::Db(e.to_string()))?;
    }
    Ok(())
}

/// 打开目标根的 canonical 存储（复用 RunDatabase 的迁移/完整性校验/写队列）。
pub fn open_target_store(target: &Path) -> Result<(tokio::runtime::Runtime, RunDatabase), ImportError> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| ImportError::Db(format!("tokio runtime: {e}")))?;
    let db_path = target.join(super::TARGET_DB_RELPATH);
    let db = rt
        .block_on(RunDatabase::open(&db_path, StoreOptions::default()))
        .map_err(|e| ImportError::Db(format!("open target store: {e}")))?;
    Ok((rt, db))
}
