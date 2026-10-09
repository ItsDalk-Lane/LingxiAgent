//! R06-T03 会话树 / 分支 / fork / 重试 / 回退 / 检查点的存储操作。
//!
//! 现役对照锚点：
//! - fork 复制 root→boundary **保留消息 ID**，仅 parent 重链
//!   （`pi-coding-agent/dist/core/session-manager.js:1113`）。
//! - 深度上限 2（`server/routes/sessions.ts:1738`），超限响亮拒绝。
//! - 分支重置：分支头回移 + 追加 `hana-session-branch-reset` 标记，旧历史不删
//!   （`core/session-turn-actions.ts:454-571`）。
//! - 具名检查点 latest 覆盖/其余冲突/上限 200（`core/session-checkpoints.ts:14-16,74-101`）。
//! - 文件恢复冲突检测为对现役缺口的加固（报告差异 D1）。
//!
//! 全部写操作经单写者队列（`queue.submit`）+ `with_write_txn` 事务，与其他
//! 变更同锁；事件由调用方在提交成功后经 EventHub 发布（提交后发布契约）。

use rusqlite::OptionalExtension;

use lingxi_kernel::ports::StorageError;
use lingxi_kernel::session_tree;
use lingxi_protocol::canon;
use lingxi_protocol::{EventEnvelope, EventId, EventPayload, Seq, SessionId, StreamId};

use super::migrations;
use super::run_store::{with_write_txn, RunDatabase};

/// fork 源会话行快照（agent/owner/title/权限档/授权目录 JSON/谱系深度）。
type SourceSessionFacts = (String, String, String, Option<String>, String, i64);
/// fork 保留段消息行（id/旧 parent/run/角色/内容）。
/// fork 保留段消息行（全字段保真复制：对照现役 session-manager.js:1113
/// `createBranchedSession` 的 `{...entry, parentId}` 浅拷贝——除 session/seq/
/// parent 重链外，消息 ID、run 归属、模型调用溯源、提交时刻、条目类型原样保留）。
type RetainedMessageRow = (
    String,
    Option<String>,
    Option<String>,
    String,
    String,
    Option<String>,
    i64,
    String,
);

/// 种一个新会话行（v8 谱系/生命周期列）。测试与生产创建共用。
pub struct SeedSession<'a> {
    pub session_id: &'a str,
    pub agent_id: &'a str,
    pub owner_user_id: &'a str,
    pub title: &'a str,
    pub now_unix_ms: u64,
    pub permission_mode: Option<&'a str>,
    pub authorized_folders: Vec<String>,
}

/// 追加一条消息（`message` 或自定义标记条目）。
/// `run_id = None` 用于无 run 归属的条目（用户输入消息、reset 标记）——
/// v8 messages 的 `run_id REFERENCES runs(run_id)` 可空但保参照完整性
/// （REPAIR-R1 FINDING-06：幽灵 run 归属由库层响亮拒绝）。
pub struct AppendMessage<'a> {
    pub session_id: &'a str,
    pub message_id: &'a str,
    pub parent_message_id: Option<&'a str>,
    pub run_id: Option<&'a str>,
    pub role: &'a str,
    pub content_json: &'a str,
    pub entry_type: &'a str,
    /// 模型调用溯源 id（final 消息由 run_store 直写；此处供标记/测试条目携带）。
    pub model_call_id: Option<&'a str>,
    pub now_unix_ms: u64,
}

/// fork 请求。
pub struct ForkRequest<'a> {
    pub source_session_id: &'a str,
    pub new_session_id: &'a str,
    pub boundary_message_id: &'a str,
    pub now_unix_ms: u64,
}

/// fork 结果（新会话的谱系与权限快照）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkOutcome {
    pub session_id: String,
    pub parent_session_id: String,
    pub fork_point_message_id: String,
    pub lineage_depth: u32,
    pub permission_mode: Option<String>,
    pub authorized_folders: Vec<String>,
}

/// 一条消息行（投影/列举用；model_call_id/committed_at_unix_ms 随行返回，
/// fork 保真（REPAIR-R1 FINDING-04）与审计联查可观测）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageRow {
    pub message_id: String,
    pub parent_message_id: Option<String>,
    pub run_id: Option<String>,
    pub role: String,
    pub content_json: String,
    pub entry_type: String,
    pub seq: i64,
    pub model_call_id: Option<String>,
    pub committed_at_unix_ms: i64,
}

/// 具名检查点 upsert 请求（REPAIR-R1 FINDING-01：文件版本+内容随行携带，
/// 检查点行与子行在**同一写事务**内原子替换——latest 覆盖不留旧版本残留）。
pub struct UpsertCheckpoint<'a> {
    pub session_id: &'a str,
    pub name: &'a str,
    pub target_message_id: &'a str,
    /// 该检查点目标的回合输入消息（retry fileRollback 的锚点；
    /// 对照现役按 turnInputEntryId 取回合快照）。
    pub turn_input_message_id: Option<&'a str>,
    pub message_count: u32,
    pub now_unix_ms: u64,
    /// 检查点时刻的文件版本与内容（rewind/retry 内容级恢复的存档字节）。
    pub files: Vec<CheckpointFileSpec>,
}

/// 检查点记录的一个文件（版本哈希 + 内容字节；内容即恢复写回的存档）。
pub struct CheckpointFileSpec {
    pub file_path: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub content: Vec<u8>,
    /// 内容形态标记（对照现役 checkpoint-store 的 utf-8/base64 双态）；
    /// BLOB 始终存原始字节，该列仅描述内容形态供读取方参考。
    pub encoding: String,
}

/// 具名检查点行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointRow {
    pub checkpoint_id: String,
    pub session_id: String,
    pub name: String,
    pub target_message_id: String,
    pub message_count: i64,
    pub created_at_unix_ms: i64,
}

/// 文件版本记录（rewind 冲突检测）——按 `(checkpoint_id, file_path)` 幂等覆盖。
/// 生产路径（service create_checkpoint）使用 `upsert_checkpoint` 的组合载荷
/// 在同事务内原子写入；本入口保留给逐条补记场景与定点探针复跑。
pub struct RecordFileVersion<'a> {
    pub checkpoint_id: &'a str,
    pub session_id: &'a str,
    pub file_path: &'a str,
    pub sha256: &'a str,
    pub size_bytes: u64,
    pub now_unix_ms: u64,
}

/// 分支头回移 + 追加 `hana-session-branch-reset` 标记（同事务；旧历史不删）。
/// `new_head_message_id = None` 表示重置到根回合之前（现役
/// retryBranchParentId = null，分支投影只剩 reset 标记）。
/// 标记 id 在事务内铸造（`reset:{session}:{now}:{seq}`——单写者下 seq
/// 单调，天然唯一），由返回值交给调用方用于事件关联。
pub struct ResetBranchHead<'a> {
    pub session_id: &'a str,
    pub new_head_message_id: Option<&'a str>,
    pub reason: &'a str,
    /// 触发重置的回合输入消息（现役 reset 记录的 sourceEntryId）。
    pub source_message_id: Option<&'a str>,
    pub now_unix_ms: u64,
}

fn map_err(err: rusqlite::Error) -> StorageError {
    migrations::map_rusqlite(err)
}

/// 种会话行。
pub async fn seed_session(db: &RunDatabase, req: SeedSession<'_>) -> Result<(), StorageError> {
    let folders_json =
        serde_json::to_string(&req.authorized_folders).map_err(|e| StorageError::Internal {
            detail: e.to_string(),
        })?;
    let session_id = req.session_id.to_string();
    let agent_id = req.agent_id.to_string();
    let owner = req.owner_user_id.to_string();
    let title = req.title.to_string();
    let perm = req.permission_mode.map(str::to_string);
    let now = req.now_unix_ms as i64;
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                conn.execute(
                    "INSERT INTO sessions \
                 (session_id, agent_id, owner_user_id, title, created_at_unix_ms, \
                  permission_mode, authorized_folders_json, lifecycle, lineage_depth) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,'active',0)",
                    rusqlite::params![session_id, agent_id, owner, title, now, perm, folders_json],
                )
                .map_err(map_err)?;
                Ok(())
            })
        })
        .await?;
    Ok(())
}

/// 追加消息（message 或标记条目）。seq 由会话内 MAX(seq)+1 分配。
pub async fn append_message(db: &RunDatabase, req: AppendMessage<'_>) -> Result<(), StorageError> {
    let session_id = req.session_id.to_string();
    let message_id = req.message_id.to_string();
    let parent = req.parent_message_id.map(str::to_string);
    let run_id = req.run_id.map(str::to_string);
    let role = req.role.to_string();
    let content = req.content_json.to_string();
    let entry_type = req.entry_type.to_string();
    let model_call_id = req.model_call_id.map(str::to_string);
    let now = req.now_unix_ms as i64;
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                let seq: i64 = conn
                    .query_row(
                        "SELECT COALESCE(MAX(seq),0)+1 FROM messages WHERE session_id=?1",
                        [&session_id],
                        |r| r.get(0),
                    )
                    .map_err(map_err)?;
                conn.execute(
                    "INSERT INTO messages \
                 (message_id, session_id, run_id, role, content_json, model_call_id, \
                  committed_at_unix_ms, seq, parent_message_id, entry_type) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                    rusqlite::params![
                        message_id,
                        session_id,
                        run_id,
                        role,
                        content,
                        model_call_id,
                        now,
                        seq,
                        parent,
                        entry_type
                    ],
                )
                .map_err(map_err)?;
                // 追加推进分支头（append-only 的当前叶；分支重置另行回移）。
                conn.execute(
                    "INSERT INTO session_branch_heads \
                 (session_id, head_message_id, observed_tail_message_id, revision, \
                  head_resolution, updated_at_unix_ms) \
                 VALUES (?1,?2,?2,0,'persisted_head',?3) \
                 ON CONFLICT(session_id) DO UPDATE SET \
                   head_message_id=excluded.head_message_id, \
                   revision=session_branch_heads.revision+1, \
                   head_resolution='persisted_head', \
                   updated_at_unix_ms=excluded.updated_at_unix_ms",
                    rusqlite::params![session_id, message_id, now],
                )
                .map_err(map_err)?;
                Ok(())
            })
        })
        .await?;
    Ok(())
}

/// 当前分支的有序消息 id 链（root→…→head）。经 parent_message_id 从当前头回溯。
/// `include_markers`：是否把 `hana-session-branch-reset` 标记条目纳入投影
/// （分支历史投影 = true，reset 测试要看标记；fork 内容链 = false，fork 复制
/// 与追加只认真实 message 条目）。
async fn branch_id_chain(
    db: &RunDatabase,
    session_id: &str,
    include_markers: bool,
) -> Result<Vec<String>, StorageError> {
    let sid = session_id.to_string();
    db.queue()
        .submit(move |conn| {
            // 当前头：branch_heads 表，缺省回退到「seq 最大的 message 条目」
            // （legacy_tail：跳过 branch-reset 等标记条目，落到真实历史尾）。
            let head: Option<String> = conn
                .query_row(
                    "SELECT head_message_id FROM session_branch_heads WHERE session_id=?1",
                    [&sid],
                    |r| r.get(0),
                )
                .optional()
                .map_err(map_err)?
                .flatten();
            let head = match head {
                Some(h) => h,
                None => {
                    // legacy_tail：seq 最大的真实 message（不含标记条目）。
                    let t: Option<String> = conn
                        .query_row(
                            "SELECT message_id FROM messages WHERE session_id=?1 \
                         AND entry_type='message' ORDER BY seq DESC LIMIT 1",
                            [&sid],
                            |r| r.get(0),
                        )
                        .optional()
                        .map_err(map_err)?;
                    match t {
                        Some(t) => t,
                        None => return Ok(Vec::new()),
                    }
                }
            };
            // 从 head 回溯到 root；`include_markers=false` 时跳过 branch-reset
            // 标记（标记不是历史内容条目）。
            let mut chain = vec![head];
            loop {
                let cur = chain.last().unwrap().clone();
                let parent: Option<String> = conn
                .query_row(
                    "SELECT parent_message_id FROM messages WHERE session_id=?1 AND message_id=?2",
                    rusqlite::params![sid, cur],
                    |r| r.get(0),
                )
                .optional()
                .map_err(map_err)?
                .flatten();
                match parent {
                    Some(p) => chain.push(p),
                    None => break,
                }
            }
            chain.reverse();
            if !include_markers {
                chain.retain(|id| {
                    // 标记条目 id 不进 fork/追加的内容链。判定：查 entry_type。
                    conn.query_row(
                        "SELECT entry_type FROM messages WHERE session_id=?1 AND message_id=?2",
                        rusqlite::params![sid, id],
                        |r| r.get::<_, String>(0),
                    )
                    .map(|t| t == "message")
                    .unwrap_or(false)
                });
            }
            Ok(chain)
        })
        .await
}

/// fork：复制 root→boundary 保留段（保留消息 ID，parent 重链）到新会话。
/// 深度闸在事务内重查（防并发绕过）。
pub async fn fork_session(
    db: &RunDatabase,
    req: ForkRequest<'_>,
) -> Result<ForkOutcome, StorageError> {
    // 先读源分支链与源会话权限快照（只读）。
    let chain = branch_id_chain(db, req.source_session_id, false).await?;
    let retained = session_tree::plan_fork_retained_ids(chain, req.boundary_message_id).ok_or(
        StorageError::InvalidRequest {
            detail: format!(
                "fork target {} is not on the current branch of {}",
                req.boundary_message_id, req.source_session_id
            ),
        },
    )?;

    let src = req.source_session_id.to_string();
    let src_label = req.source_session_id.to_string();
    let found: Option<SourceSessionFacts> = db
        .queue()
        .submit(move |conn| {
            conn.query_row(
                "SELECT agent_id, owner_user_id, title, permission_mode, \
                 authorized_folders_json, lineage_depth FROM sessions WHERE session_id=?1",
                [&src],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .optional()
            .map_err(map_err)
        })
        .await?;
    let (agent_id, owner, title, perm, folders_json, depth) =
        found.ok_or(StorageError::InvalidRequest {
            detail: format!("source session {src_label} not found"),
        })?;

    // 深度闸（纯逻辑）：来源已在第 2 层则拒绝。
    let child_depth =
        session_tree::check_fork_depth(depth as u32).ok_or(StorageError::InvalidRequest {
            detail: format!(
                "fork_depth_limit: source {} is at lineage depth {} (max {})",
                req.source_session_id,
                depth,
                session_tree::MAX_FORK_LINEAGE_DEPTH
            ),
        })?;

    // 读取保留段的完整消息行（全字段保真：ID、run 归属、模型调用溯源、
    // 提交时刻、条目类型原样保留；仅 parent 重链——对照现役浅拷贝语义）。
    let src2 = req.source_session_id.to_string();
    let retained_clone = retained.clone();
    let rows: Vec<RetainedMessageRow> = db
        .queue()
        .submit(move |conn| {
            let mut out = Vec::new();
            for id in &retained_clone {
                let row = conn
                    .query_row(
                        "SELECT message_id, parent_message_id, run_id, role, content_json, \
                         model_call_id, committed_at_unix_ms, entry_type \
                         FROM messages WHERE session_id=?1 AND message_id=?2",
                        rusqlite::params![src2, id],
                        |r| {
                            Ok((
                                r.get(0)?,
                                r.get(1)?,
                                r.get(2)?,
                                r.get(3)?,
                                r.get(4)?,
                                r.get(5)?,
                                r.get(6)?,
                                r.get(7)?,
                            ))
                        },
                    )
                    .map_err(map_err)?;
                out.push(row);
            }
            Ok(out)
        })
        .await?;

    let new_id = req.new_session_id.to_string();
    let boundary = req.boundary_message_id.to_string();
    let src3 = req.source_session_id.to_string();
    let now = req.now_unix_ms as i64;
    let perm2 = perm.clone();
    let folders_json2 = folders_json.clone();
    let folders: Vec<String> =
        serde_json::from_str(&folders_json).map_err(|e| StorageError::Corrupted {
            detail: e.to_string(),
        })?;

    let busy = db.options().busy_timeout_ms;
    let _: () = db
        .queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                // 事务内重查深度闸（防并发 fork 绕过）。
                let cur_depth: i64 = conn
                    .query_row(
                        "SELECT lineage_depth FROM sessions WHERE session_id=?1",
                        [&src3],
                        |r| r.get(0),
                    )
                    .map_err(map_err)?;
                if session_tree::check_fork_depth(cur_depth as u32).is_none() {
                    return Err(StorageError::InvalidRequest {
                        detail: format!("fork_depth_limit: source {src3} at depth {cur_depth}"),
                    });
                }
                // 撞库预检（REPAIR-R1 观察项）：new_session_id 已存在 → 响亮
                // Conflict（生产链映射 409），不再落 Internal 500
                // （对照现役 sessions.ts:587 active_session_conflict 409）。
                let id_taken: bool = conn
                    .query_row(
                        "SELECT COUNT(*) FROM sessions WHERE session_id=?1",
                        [&new_id],
                        |r| r.get::<_, i64>(0),
                    )
                    .map_err(map_err)?
                    > 0;
                if id_taken {
                    return Err(StorageError::Conflict {
                        detail: format!("session_exists:{new_id}"),
                    });
                }
                // 新会话行（继承权限快照，绝不扩大）。
                conn.execute(
                    "INSERT INTO sessions \
                 (session_id, agent_id, owner_user_id, title, created_at_unix_ms, \
                  parent_session_id, fork_point_message_id, lineage_depth, lifecycle, \
                  permission_mode, authorized_folders_json) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'active',?9,?10)",
                    rusqlite::params![
                        new_id,
                        agent_id,
                        owner,
                        title,
                        now,
                        src3,
                        boundary,
                        child_depth as i64,
                        perm2,
                        folders_json2
                    ],
                )
                .map_err(map_err)?;
                // 复制保留段：除 run_id 外全字段保真（ID/模型调用溯源/提交时刻/
                // 条目类型原样），仅 seq 重排、parent 重链（root 的 parent 置 NULL）。
                // run_id 置 NULL（REPAIR-R1 裁决）：副本归属新会话，不持有指向源
                // 会话 runs 行的活引用——否则源会话永久删除时 runs 行被副本引用
                // 而 FK 拒绝（现役是按会话分文件存储，天然无此跨会话耦合）；源 run
                // 的可追溯性由消息 id（user:{run_id} / {run_id}-final）原样保留。
                let mut prev: Option<String> = None;
                for (
                    idx,
                    (
                        mid,
                        _old_parent,
                        _run_id,
                        role,
                        content,
                        model_call_id,
                        committed,
                        entry_type,
                    ),
                ) in rows.iter().enumerate()
                {
                    let seq = (idx + 1) as i64;
                    conn.execute(
                        "INSERT INTO messages \
                     (message_id, session_id, run_id, role, content_json, model_call_id, \
                      committed_at_unix_ms, seq, parent_message_id, entry_type) \
                     VALUES (?1,?2,NULL,?3,?4,?5,?6,?7,?8,?9)",
                        rusqlite::params![
                            mid,
                            new_id,
                            role,
                            content,
                            model_call_id,
                            committed,
                            seq,
                            prev,
                            entry_type
                        ],
                    )
                    .map_err(map_err)?;
                    prev = Some(mid.clone());
                }
                // 新会话分支头 = boundary。
                conn.execute(
                    "INSERT INTO session_branch_heads \
                 (session_id, head_message_id, observed_tail_message_id, revision, \
                  head_resolution, updated_at_unix_ms) \
                 VALUES (?1,?2,?2,0,'persisted_head',?3)",
                    rusqlite::params![new_id, boundary, now],
                )
                .map_err(map_err)?;
                Ok(())
            })
        })
        .await?;

    Ok(ForkOutcome {
        session_id: req.new_session_id.to_string(),
        parent_session_id: req.source_session_id.to_string(),
        fork_point_message_id: req.boundary_message_id.to_string(),
        lineage_depth: child_depth,
        permission_mode: perm,
        authorized_folders: folders,
    })
}

/// 当前分支的有序消息（root→…→head）。
pub async fn list_branch_messages(
    db: &RunDatabase,
    session_id: &str,
) -> Result<Vec<MessageRow>, StorageError> {
    let chain = branch_id_chain(db, session_id, true).await?;
    let sid = session_id.to_string();
    db.queue()
        .submit(move |conn| {
            let mut out = Vec::new();
            for id in &chain {
                let row = conn
                    .query_row(
                        "SELECT message_id, parent_message_id, run_id, role, content_json, \
                     entry_type, seq, model_call_id, committed_at_unix_ms \
                     FROM messages WHERE session_id=?1 AND message_id=?2",
                        rusqlite::params![sid, id],
                        |r| {
                            Ok((
                                r.get(0)?,
                                r.get(1)?,
                                r.get(2)?,
                                r.get(3)?,
                                r.get(4)?,
                                r.get(5)?,
                                r.get(6)?,
                                r.get(7)?,
                                r.get(8)?,
                            ))
                        },
                    )
                    .map_err(map_err)?;
                out.push(MessageRow {
                    message_id: row.0,
                    parent_message_id: row.1,
                    run_id: row.2,
                    role: row.3,
                    content_json: row.4,
                    entry_type: row.5,
                    seq: row.6,
                    model_call_id: row.7,
                    committed_at_unix_ms: row.8,
                });
            }
            Ok(out)
        })
        .await
}

/// 会话的全部消息（含被分支丢弃的旧历史；append-only 不删）。
pub async fn list_all_messages(
    db: &RunDatabase,
    session_id: &str,
) -> Result<Vec<MessageRow>, StorageError> {
    let sid = session_id.to_string();
    db.queue()
        .submit(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT message_id, parent_message_id, run_id, role, content_json, \
                 entry_type, seq, model_call_id, committed_at_unix_ms \
                 FROM messages WHERE session_id=?1 ORDER BY seq",
                )
                .map_err(map_err)?;
            let rows = stmt
                .query_map([&sid], |r| {
                    Ok(MessageRow {
                        message_id: r.get(0)?,
                        parent_message_id: r.get(1)?,
                        run_id: r.get(2)?,
                        role: r.get(3)?,
                        content_json: r.get(4)?,
                        entry_type: r.get(5)?,
                        seq: r.get(6)?,
                        model_call_id: r.get(7)?,
                        committed_at_unix_ms: r.get(8)?,
                    })
                })
                .map_err(map_err)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r.map_err(map_err)?);
            }
            Ok(out)
        })
        .await
}

/// upsert 具名检查点（latest 覆盖/其余冲突/上限 200）。
/// REPAIR-R1 FINDING-01：检查点行与文件版本/内容子行在**同一写事务**内
/// 原子完成——Overwrite 先删旧子行再写新子行（同名文件可重录、异名文件
/// 不残留）；窗口裁减先清子行再删检查点行（带版本的检查点被裁不再 FK 失败）。
pub async fn upsert_checkpoint(
    db: &RunDatabase,
    req: UpsertCheckpoint<'_>,
) -> Result<CheckpointRow, StorageError> {
    let sid = req.session_id.to_string();
    let name = req.name.to_string();
    let target = req.target_message_id.to_string();
    let turn_input = req.turn_input_message_id.map(str::to_string);
    let now = req.now_unix_ms as i64;
    let count = req.message_count as i64;
    let cp_id = format!("{}:{}", sid, name);
    let cp_id_for_write = cp_id.clone();
    let files: Vec<(String, String, i64, Vec<u8>, String)> = req
        .files
        .into_iter()
        .map(|f| {
            (
                f.file_path,
                f.sha256,
                f.size_bytes as i64,
                f.content,
                f.encoding,
            )
        })
        .collect();
    let busy = db.options().busy_timeout_ms;
    let _: () = db
        .queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                let exists: bool = conn
                    .query_row(
                        "SELECT COUNT(*) FROM checkpoints WHERE session_id=?1 AND name=?2",
                        rusqlite::params![sid, name],
                        |r| r.get::<_, i64>(0),
                    )
                    .map_err(map_err)?
                    > 0;
                match session_tree::decide_checkpoint_upsert(&name, exists) {
                    session_tree::CheckpointUpsert::Conflict => {
                        return Err(StorageError::Conflict {
                            detail: format!(
                                "checkpoint {name} already exists on session {sid} (only \
                             'latest' may be overwritten)"
                            ),
                        })
                    }
                    session_tree::CheckpointUpsert::Overwrite => {
                        // latest 覆盖 = 检查点整体指向新状态：旧文件版本与内容
                        // 子行随行清除（同事务），再写入新集合。
                        conn.execute(
                            "DELETE FROM checkpoint_file_versions WHERE checkpoint_id=?1",
                            [&cp_id_for_write],
                        )
                        .map_err(map_err)?;
                        conn.execute(
                            "DELETE FROM file_checkpoints WHERE checkpoint_id=?1",
                            [&cp_id_for_write],
                        )
                        .map_err(map_err)?;
                        conn.execute(
                        "UPDATE checkpoints SET target_message_id=?3, turn_input_message_id=?4, \
                         message_count=?5, created_at_unix_ms=?6 \
                         WHERE session_id=?1 AND name=?2",
                        rusqlite::params![sid, name, target, turn_input, count, now],
                    )
                    .map_err(map_err)?;
                    }
                    session_tree::CheckpointUpsert::Insert => {
                        conn.execute(
                            "INSERT INTO checkpoints \
                         (checkpoint_id, session_id, name, kind, target_message_id, \
                          turn_input_message_id, message_count, created_at_unix_ms) \
                         VALUES (?1,?2,?3,'named',?4,?5,?6,?7)",
                            rusqlite::params![
                                cp_id_for_write,
                                sid,
                                name,
                                target,
                                turn_input,
                                count,
                                now
                            ],
                        )
                        .map_err(map_err)?;
                    }
                }
                // 文件版本 + 内容子行（同事务随行写入；rewind/retry 的恢复存档）。
                for (file_path, sha256, size_bytes, content, encoding) in &files {
                    conn.execute(
                        "INSERT INTO checkpoint_file_versions \
                     (checkpoint_id, file_path, sha256, size_bytes, recorded_at_unix_ms) \
                     VALUES (?1,?2,?3,?4,?5)",
                        rusqlite::params![cp_id_for_write, file_path, sha256, size_bytes, now],
                    )
                    .map_err(map_err)?;
                    conn.execute(
                        "INSERT INTO file_checkpoints \
                     (checkpoint_id, session_id, file_path, content, encoding, reason, \
                      created_at_unix_ms) \
                     VALUES (?1,?2,?3,?4,?5,'checkpoint',?6)",
                        rusqlite::params![cp_id_for_write, sid, file_path, content, encoding, now],
                    )
                    .map_err(map_err)?;
                }
                // 窗口化裁减（上限 200，裁最老非 latest）。先清子行再删检查点行
                // （FK 顺序与 delete_checkpoint 同标准）。
                let mut stmt = conn
                    .prepare(
                        "SELECT name FROM checkpoints WHERE session_id=?1 \
                     ORDER BY created_at_unix_ms ASC",
                    )
                    .map_err(map_err)?;
                let names: Vec<String> = stmt
                    .query_map([&sid], |r| r.get(0))
                    .map_err(map_err)?
                    .collect::<Result<_, _>>()
                    .map_err(map_err)?;
                drop(stmt);
                for evict in session_tree::plan_checkpoint_eviction(&names) {
                    let evict_id = format!("{sid}:{evict}");
                    conn.execute(
                        "DELETE FROM checkpoint_file_versions WHERE checkpoint_id=?1",
                        [&evict_id],
                    )
                    .map_err(map_err)?;
                    conn.execute(
                        "DELETE FROM file_checkpoints WHERE checkpoint_id=?1",
                        [&evict_id],
                    )
                    .map_err(map_err)?;
                    conn.execute(
                        "DELETE FROM checkpoints WHERE checkpoint_id=?1",
                        [&evict_id],
                    )
                    .map_err(map_err)?;
                }
                Ok(())
            })
        })
        .await?;
    Ok(CheckpointRow {
        checkpoint_id: cp_id,
        session_id: req.session_id.to_string(),
        name: req.name.to_string(),
        target_message_id: req.target_message_id.to_string(),
        message_count: req.message_count as i64,
        created_at_unix_ms: req.now_unix_ms as i64,
    })
}

/// 读取一个具名检查点。
pub async fn get_checkpoint(
    db: &RunDatabase,
    session_id: &str,
    name: &str,
) -> Result<Option<CheckpointRow>, StorageError> {
    let sid = session_id.to_string();
    let nm = name.to_string();
    db.queue()
        .submit(move |conn| {
            conn.query_row(
                "SELECT checkpoint_id, session_id, name, target_message_id, message_count, \
             created_at_unix_ms FROM checkpoints WHERE session_id=?1 AND name=?2",
                rusqlite::params![sid, nm],
                |r| {
                    Ok(CheckpointRow {
                        checkpoint_id: r.get(0)?,
                        session_id: r.get(1)?,
                        name: r.get(2)?,
                        target_message_id: r.get(3)?,
                        message_count: r.get(4)?,
                        created_at_unix_ms: r.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(map_err)
        })
        .await
}

/// 记录检查点处的文件版本（rewind 冲突检测基线）。按 (checkpoint_id, file_path)
/// 幂等覆盖（ON CONFLICT DO UPDATE）——重复记录同文件响亮成功而非 PK 冲突。
pub async fn record_file_version(
    db: &RunDatabase,
    req: RecordFileVersion<'_>,
) -> Result<(), StorageError> {
    let cp = req.checkpoint_id.to_string();
    let fp = req.file_path.to_string();
    let hash = req.sha256.to_string();
    let size = req.size_bytes as i64;
    let now = req.now_unix_ms as i64;
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                conn.execute(
                    "INSERT INTO checkpoint_file_versions \
                 (checkpoint_id, file_path, sha256, size_bytes, recorded_at_unix_ms) \
                 VALUES (?1,?2,?3,?4,?5) \
                 ON CONFLICT(checkpoint_id, file_path) DO UPDATE SET \
                   sha256=excluded.sha256, size_bytes=excluded.size_bytes, \
                   recorded_at_unix_ms=excluded.recorded_at_unix_ms",
                    rusqlite::params![cp, fp, hash, size, now],
                )
                .map_err(map_err)?;
                Ok(())
            })
        })
        .await?;
    Ok(())
}

/// 分支头回移 + 追加 `hana-session-branch-reset` 标记（同事务；旧历史不删）。
/// 标记 id 事务内铸造为 `reset:{session_id}:{now}:{seq}`（单写者下 seq 单调，
/// 天然唯一）并返回，供调用方用于事件关联；`new_head = None` 表示重置到
/// 根回合之前（现役 retryBranchParentId = null）。
pub async fn reset_branch_head(
    db: &RunDatabase,
    req: ResetBranchHead<'_>,
) -> Result<String, StorageError> {
    let sid = req.session_id.to_string();
    let new_head: Option<String> = req.new_head_message_id.map(str::to_string);
    let source_id: Option<String> = req.source_message_id.map(str::to_string);
    let reason = req.reason.to_string();
    let now = req.now_unix_ms as i64;
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                // 1) 追加 reset 标记（parent=新头，append-only）。
                let seq: i64 = conn
                    .query_row(
                        "SELECT COALESCE(MAX(seq),0)+1 FROM messages WHERE session_id=?1",
                        [&sid],
                        |r| r.get(0),
                    )
                    .map_err(map_err)?;
                // 单写者事务内：seq 单调，id 无竞争（不依赖调用方铸造，杜绝重复 id）。
                let marker_id = format!("reset:{sid}:{now}:{seq}");
                // 对照现役 SESSION_BRANCH_RESET_RECORD_TYPE 记录字段：reason/to/sourceEntryId。
                let mut marker_content = serde_json::Map::new();
                marker_content.insert("reason".into(), reason.clone().into());
                marker_content.insert(
                    "to".into(),
                    match &new_head {
                        Some(h) => h.clone().into(),
                        None => serde_json::Value::Null,
                    },
                );
                if let Some(src) = &source_id {
                    marker_content.insert("sourceEntryId".into(), src.clone().into());
                }
                let marker_json = serde_json::Value::Object(marker_content).to_string();
                // run_id = NULL：reset 标记无 run 归属（v8 run_id 可空但保 FK——
                // REPAIR-R1 FINDING-06；空串会被外键拒绝）。
                conn.execute(
                    "INSERT INTO messages \
                 (message_id, session_id, run_id, role, content_json, model_call_id, \
                  committed_at_unix_ms, seq, parent_message_id, entry_type) \
                 VALUES (?1,?2,NULL,'system',?3,NULL,?4,?5,?6,'hana-session-branch-reset')",
                    rusqlite::params![marker_id, sid, marker_json, now, seq, new_head],
                )
                .map_err(map_err)?;
                // 2) 分支头指向 reset 标记（持久化头，revision 递增）。
                conn.execute(
                    "INSERT INTO session_branch_heads \
                 (session_id, head_message_id, observed_tail_message_id, revision, \
                  head_resolution, updated_at_unix_ms) \
                 VALUES (?1,?2,?2,1,'persisted_head',?3) \
                 ON CONFLICT(session_id) DO UPDATE SET \
                   head_message_id=excluded.head_message_id, \
                   revision=session_branch_heads.revision+1, \
                   head_resolution='persisted_head', \
                   updated_at_unix_ms=excluded.updated_at_unix_ms",
                    rusqlite::params![sid, marker_id, now],
                )
                .map_err(map_err)?;
                Ok(marker_id)
            })
        })
        .await
}

/// R06-T03：提交一条会话树管理事件（`session_created` / `session_branch_reset`
/// 等）到 key_events，并返回提交成功的封套供调用方发布（提交后发布契约）。
/// 事件负载经规范 JSON 序列化；`stream_id` 取会话 id（与 run 事件同流）。
pub async fn stage_key_event(
    db: &RunDatabase,
    session_id: &str,
    event_id: &str,
    payload: EventPayload,
    now_unix_ms: u64,
) -> Result<EventEnvelope, StorageError> {
    let sid = session_id.to_string();
    let eid = event_id.to_string();
    let now = now_unix_ms as i64;
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                let current: Option<i64> = conn
                    .query_row(
                        "SELECT MAX(seq) FROM key_events WHERE stream_id=?1",
                        [&sid],
                        |r| r.get(0),
                    )
                    .map_err(map_err)?;
                let seq = current.unwrap_or(0) + 1;
                let payload_json = canon::canonical_string(&payload);
                let event_type = payload.event_type().to_string();
                conn.execute(
                    "INSERT INTO key_events \
                 (event_id, stream_id, seq, session_id, run_id, attempt, event_type, \
                  payload_json, committed_at_unix_ms) \
                 VALUES (?1,?2,?3,?4,NULL,NULL,?5,?6,?7)",
                    rusqlite::params![eid, sid, seq, sid, event_type, payload_json, now],
                )
                .map_err(map_err)?;
                Ok(EventEnvelope::new(
                    EventId::new(eid),
                    StreamId::new(sid.clone()),
                    Seq::new(seq as u64),
                    SessionId::new(sid),
                    None,
                    None,
                    payload,
                ))
            })
        })
        .await
}

/// 构造一个 Unknown 透传事件负载（`lingxi.wire` v1 开放词汇的显式扩展面）。
pub fn unknown_payload(
    event_type: &str,
    fields: serde_json::Map<String, serde_json::Value>,
) -> EventPayload {
    let mut raw = fields;
    raw.insert(
        "type".to_string(),
        serde_json::Value::String(event_type.to_string()),
    );
    EventPayload::Unknown(lingxi_protocol::UnknownEventPayload {
        event_type: event_type.to_string(),
        raw,
    })
}

/// 一个检查点处记录的文件版本行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileVersionRow {
    pub checkpoint_id: String,
    pub file_path: String,
    pub sha256: String,
    pub size_bytes: i64,
}

/// 列出检查点记录的全部文件版本（rewind 冲突检测输入）。
pub async fn list_file_versions(
    db: &RunDatabase,
    checkpoint_id: &str,
) -> Result<Vec<FileVersionRow>, StorageError> {
    let cp = checkpoint_id.to_string();
    db.queue()
        .submit(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT checkpoint_id, file_path, sha256, size_bytes \
                 FROM checkpoint_file_versions WHERE checkpoint_id=?1 ORDER BY file_path",
                )
                .map_err(map_err)?;
            let rows = stmt
                .query_map([&cp], |r| {
                    Ok(FileVersionRow {
                        checkpoint_id: r.get(0)?,
                        file_path: r.get(1)?,
                        sha256: r.get(2)?,
                        size_bytes: r.get(3)?,
                    })
                })
                .map_err(map_err)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r.map_err(map_err)?);
            }
            Ok(out)
        })
        .await
}

/// 会话行的 v8 全列投影（谱系/生命周期/置顶/记忆/权限快照）。
/// 路由归属闸、生命周期闸与检查点授权目录闸的共同输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTreeRow {
    pub session_id: String,
    pub agent_id: String,
    pub owner_user_id: String,
    pub title: String,
    pub created_at_unix_ms: i64,
    pub parent_session_id: Option<String>,
    pub fork_point_message_id: Option<String>,
    pub lineage_depth: i64,
    pub lifecycle: String,
    pub pinned_at_unix_ms: Option<i64>,
    pub pin_order: Option<i64>,
    pub memory_enabled: bool,
    pub authorized_folders: Vec<String>,
    pub permission_mode: Option<String>,
    pub archived_at_unix_ms: Option<i64>,
    pub last_activity_unix_ms: Option<i64>,
}

/// 读取一个会话行的 v8 全列投影。
pub async fn get_session_row(
    db: &RunDatabase,
    session_id: &str,
) -> Result<Option<SessionTreeRow>, StorageError> {
    let sid = session_id.to_string();
    db.queue()
        .submit(move |conn| {
            conn.query_row(
                "SELECT session_id, agent_id, owner_user_id, title, created_at_unix_ms, \
                 parent_session_id, fork_point_message_id, lineage_depth, lifecycle, \
                 pinned_at_unix_ms, pin_order, memory_enabled, authorized_folders_json, \
                 permission_mode, archived_at_unix_ms, last_activity_unix_ms \
                 FROM sessions WHERE session_id=?1",
                [&sid],
                |r| {
                    let folders_json: String = r.get(12)?;
                    let folders: Vec<String> =
                        serde_json::from_str(&folders_json).unwrap_or_default();
                    Ok(SessionTreeRow {
                        session_id: r.get(0)?,
                        agent_id: r.get(1)?,
                        owner_user_id: r.get(2)?,
                        title: r.get(3)?,
                        created_at_unix_ms: r.get(4)?,
                        parent_session_id: r.get(5)?,
                        fork_point_message_id: r.get(6)?,
                        lineage_depth: r.get(7)?,
                        lifecycle: r.get(8)?,
                        pinned_at_unix_ms: r.get(9)?,
                        pin_order: r.get(10)?,
                        memory_enabled: r.get::<_, i64>(11)? != 0,
                        authorized_folders: folders,
                        permission_mode: r.get(13)?,
                        archived_at_unix_ms: r.get(14)?,
                        last_activity_unix_ms: r.get(15)?,
                    })
                },
            )
            .optional()
            .map_err(map_err)
        })
        .await
}

/// 列出会话的全部具名检查点（创建时间升序——与裁减窗口同序）。
pub async fn list_checkpoints(
    db: &RunDatabase,
    session_id: &str,
) -> Result<Vec<CheckpointRow>, StorageError> {
    let sid = session_id.to_string();
    db.queue()
        .submit(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT checkpoint_id, session_id, name, target_message_id, message_count, \
                     created_at_unix_ms FROM checkpoints WHERE session_id=?1 \
                     ORDER BY created_at_unix_ms ASC",
                )
                .map_err(map_err)?;
            let rows = stmt
                .query_map([&sid], |r| {
                    Ok(CheckpointRow {
                        checkpoint_id: r.get(0)?,
                        session_id: r.get(1)?,
                        name: r.get(2)?,
                        target_message_id: r.get(3)?,
                        message_count: r.get(4)?,
                        created_at_unix_ms: r.get(5)?,
                    })
                })
                .map_err(map_err)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r.map_err(map_err)?);
            }
            Ok(out)
        })
        .await
}

/// 删除一个具名检查点（FK 顺序：先文件版本与内容子行，后检查点行）。
/// 返回是否确有该行被删（false → 调用方映射 404，不静默成功）。
pub async fn delete_checkpoint(
    db: &RunDatabase,
    session_id: &str,
    name: &str,
) -> Result<bool, StorageError> {
    let sid = session_id.to_string();
    let nm = name.to_string();
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                let cp_id: Option<String> = conn
                    .query_row(
                        "SELECT checkpoint_id FROM checkpoints WHERE session_id=?1 AND name=?2",
                        rusqlite::params![sid, nm],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(map_err)?;
                let Some(cp_id) = cp_id else {
                    return Ok(false);
                };
                conn.execute(
                    "DELETE FROM checkpoint_file_versions WHERE checkpoint_id=?1",
                    [&cp_id],
                )
                .map_err(map_err)?;
                conn.execute(
                    "DELETE FROM file_checkpoints WHERE checkpoint_id=?1",
                    [&cp_id],
                )
                .map_err(map_err)?;
                conn.execute("DELETE FROM checkpoints WHERE checkpoint_id=?1", [&cp_id])
                    .map_err(map_err)?;
                Ok(true)
            })
        })
        .await
}

/// 检查点处存档的一个文件内容（rewind/retry 内容级恢复的写回字节）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileContentRow {
    pub file_path: String,
    pub content: Vec<u8>,
    pub encoding: String,
}

/// 读取检查点存档的全部文件内容（REPAIR-R1 FINDING-05/07：file_checkpoints
/// 从死表激活为恢复语义的存档承载）。
pub async fn list_file_contents(
    db: &RunDatabase,
    checkpoint_id: &str,
) -> Result<Vec<FileContentRow>, StorageError> {
    let cp = checkpoint_id.to_string();
    db.queue()
        .submit(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT file_path, content, encoding FROM file_checkpoints \
                 WHERE checkpoint_id=?1 ORDER BY file_path",
                )
                .map_err(map_err)?;
            let rows = stmt
                .query_map([&cp], |r| {
                    Ok(FileContentRow {
                        file_path: r.get(0)?,
                        content: r.get(1)?,
                        encoding: r.get(2)?,
                    })
                })
                .map_err(map_err)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r.map_err(map_err)?);
            }
            Ok(out)
        })
        .await
}

/// 会话全部检查点见证过的 (file_path, sha256) 集合（rewind 三档判定的
/// 「系统见证」输入：当前内容哈希在此集合内 = 检查点之后的改动曾被系统
/// 记录过，如模型自己的后续回合 → 可安全写回存档；不在 = 外部修改 → 拒绝覆盖）。
pub async fn list_session_file_hashes(
    db: &RunDatabase,
    session_id: &str,
) -> Result<Vec<(String, String)>, StorageError> {
    let sid = session_id.to_string();
    db.queue()
        .submit(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT v.file_path, v.sha256 FROM checkpoint_file_versions v \
                 JOIN checkpoints c ON c.checkpoint_id = v.checkpoint_id \
                 WHERE c.session_id=?1",
                )
                .map_err(map_err)?;
            let rows = stmt
                .query_map([&sid], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(map_err)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r.map_err(map_err)?);
            }
            Ok(out)
        })
        .await
}

/// 按回合输入消息定位检查点（retry fileRollback 的锚点——对照现役按
/// turnInputEntryId 取该回合开始时的快照；多个命中取最新创建者）。
pub async fn find_checkpoint_by_turn_input(
    db: &RunDatabase,
    session_id: &str,
    turn_input_message_id: &str,
) -> Result<Option<CheckpointRow>, StorageError> {
    let sid = session_id.to_string();
    let ti = turn_input_message_id.to_string();
    db.queue()
        .submit(move |conn| {
            conn.query_row(
                "SELECT checkpoint_id, session_id, name, target_message_id, message_count, \
                 created_at_unix_ms FROM checkpoints \
                 WHERE session_id=?1 AND turn_input_message_id=?2 \
                 ORDER BY created_at_unix_ms DESC, rowid DESC LIMIT 1",
                rusqlite::params![sid, ti],
                |r| {
                    Ok(CheckpointRow {
                        checkpoint_id: r.get(0)?,
                        session_id: r.get(1)?,
                        name: r.get(2)?,
                        target_message_id: r.get(3)?,
                        message_count: r.get(4)?,
                        created_at_unix_ms: r.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(map_err)
        })
        .await
}
