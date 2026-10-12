//! R06-T03 会话管理面的存储操作：创建 / 改名 / 置顶 / 归档 / 恢复 / 永久删除 /
//! 清理 / 搜索 / 记忆开关。
//!
//! 现役对照锚点（逐条亲验）：
//! - rename：`server/routes/sessions.ts:2493`（title 非空校验在路由层）。
//! - cleanup：`:2517`（maxAgeDays 默认 90，按归档时间截止线永久删除）。
//! - archived 列表：`:2555`。
//! - archive：`:2569-2660`（子对话存在且无 childMode → 409 child_sessions_present；
//!   detach_children 只摘直接子对话的谱系指针；archive_children 递归归档、
//!   流式中的子对话跳过并计数）。
//! - restore：`:2813`（仅归档态可恢复）。
//! - archived/delete：`:2882`（仅归档态可永久删除）。
//! - pin：`:1014`；pin-order：`:1052`（完整有序列表整体重编号，重复 id 响亮 400）。
//! - search：`:857`（查询上限 `SESSION_SEARCH_QUERY_MAX_LENGTH = 512`，
//!   title/content 两阶段）。
//! - memory GET/PATCH：`:1103`/`:1135`。
//!
//! 全部写操作经单写者队列 + `with_write_txn`；永久删除按 FK 依赖序逐表清理
//! （foreign_keys=ON，queue.rs:592），顺序在本文件 `delete_archived_session`
//! 的注释中逐条列出。

use rusqlite::OptionalExtension;

use lingxi_kernel::ports::StorageError;

use super::migrations;
use super::run_store::{with_write_txn, RunDatabase};

fn map_err(err: rusqlite::Error) -> StorageError {
    migrations::map_rusqlite(err)
}

/// 归档的子对话处置策略（现役 archive 路由 childMode，sessions.ts:2586-2590）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildMode {
    /// 递归归档全部后代；流式中的子对话跳过并计数。
    ArchiveChildren,
    /// 直接子对话谱系指针清空（释放到顶层），仅归档自身。
    DetachChildren,
}

/// 归档结果计数（对照现役响应字段 archivedChildren/detachedChildren/
/// skippedStreamingChildren）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveOutcome {
    pub archived_children: usize,
    pub detached_children: usize,
    pub skipped_streaming_children: usize,
}

/// 恢复 / 删除的结局（三态，调用方映射 404/409/200，绝不静默成功）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleOutcome {
    Done,
    NotFound,
    /// 会话存在但生命周期态不允许该操作（如未归档却请求永久删除）。
    WrongLifecycle,
}

/// 管理面会话摘要行（归档列表 / 搜索投影）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummaryRow {
    pub session_id: String,
    pub agent_id: String,
    pub owner_user_id: String,
    pub title: String,
    pub lifecycle: String,
    pub created_at_unix_ms: i64,
    pub archived_at_unix_ms: Option<i64>,
    pub last_activity_unix_ms: Option<i64>,
}

/// 创建一个新会话行（顶层：lineage_depth=0，lifecycle=active）。
/// fork 不走这里（fork_session 自带谱系列）。
#[allow(clippy::too_many_arguments)]
pub async fn create_session(
    db: &RunDatabase,
    session_id: &str,
    agent_id: &str,
    owner_user_id: &str,
    title: &str,
    permission_mode: Option<&str>,
    authorized_folders: &[String],
    memory_enabled: bool,
    now_unix_ms: u64,
) -> Result<(), StorageError> {
    let folders_json =
        serde_json::to_string(authorized_folders).map_err(|e| StorageError::Internal {
            detail: e.to_string(),
        })?;
    let sid = session_id.to_string();
    let aid = agent_id.to_string();
    let owner = owner_user_id.to_string();
    let ttl = title.to_string();
    let perm = permission_mode.map(str::to_string);
    let mem = i64::from(memory_enabled);
    let now = now_unix_ms as i64;
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                // 撞库预检（REPAIR-R1 观察项同类入口）：session_id 已存在 →
                // 响亮 Conflict（生产链映射 409），不落 Internal 500
                // （对照现役 sessions.ts:587 active_session_conflict 409）。
                let id_taken: bool = conn
                    .query_row(
                        "SELECT COUNT(*) FROM sessions WHERE session_id=?1",
                        [&sid],
                        |r| r.get::<_, i64>(0),
                    )
                    .map_err(map_err)?
                    > 0;
                if id_taken {
                    return Err(StorageError::Conflict {
                        detail: format!("session_exists:{sid}"),
                    });
                }
                conn.execute(
                    "INSERT INTO sessions \
                 (session_id, agent_id, owner_user_id, title, created_at_unix_ms, \
                  lifecycle, lineage_depth, permission_mode, authorized_folders_json, \
                  memory_enabled, last_activity_unix_ms) \
                 VALUES (?1,?2,?3,?4,?5,'active',0,?6,?7,?8,?5)",
                    rusqlite::params![sid, aid, owner, ttl, now, perm, folders_json, mem],
                )
                .map_err(map_err)?;
                Ok(())
            })
        })
        .await?;
    Ok(())
}

/// 改名。返回 false = 会话不存在（调用方 404）。
pub async fn rename_session(
    db: &RunDatabase,
    session_id: &str,
    title: &str,
) -> Result<bool, StorageError> {
    let sid = session_id.to_string();
    let ttl = title.to_string();
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                let n = conn
                    .execute(
                        "UPDATE sessions SET title=?2 WHERE session_id=?1",
                        rusqlite::params![sid, ttl],
                    )
                    .map_err(map_err)?;
                Ok(n > 0)
            })
        })
        .await
}

/// 置顶 / 取消置顶（现役 pin：置顶时 pinned_at=now 且排到置顶区尾）。
/// 返回 None = 会话不存在；否则返回 (pinned_at, pin_order)。
pub async fn set_pinned(
    db: &RunDatabase,
    session_id: &str,
    pinned: bool,
    now_unix_ms: u64,
) -> Result<Option<(Option<i64>, Option<i64>)>, StorageError> {
    let sid = session_id.to_string();
    let now = now_unix_ms as i64;
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                let exists: bool = conn
                    .query_row(
                        "SELECT COUNT(*) FROM sessions WHERE session_id=?1",
                        [&sid],
                        |r| r.get::<_, i64>(0),
                    )
                    .map_err(map_err)?
                    > 0;
                if !exists {
                    return Ok(None);
                }
                if pinned {
                    let next_order: i64 = conn
                        .query_row(
                            "SELECT COALESCE(MAX(pin_order),0)+1 FROM sessions \
                             WHERE pinned_at_unix_ms IS NOT NULL",
                            [],
                            |r| r.get(0),
                        )
                        .map_err(map_err)?;
                    conn.execute(
                        "UPDATE sessions SET pinned_at_unix_ms=?2, pin_order=?3 \
                         WHERE session_id=?1",
                        rusqlite::params![sid, now, next_order],
                    )
                    .map_err(map_err)?;
                    Ok(Some((Some(now), Some(next_order))))
                } else {
                    conn.execute(
                        "UPDATE sessions SET pinned_at_unix_ms=NULL, pin_order=NULL \
                         WHERE session_id=?1",
                        [&sid],
                    )
                    .map_err(map_err)?;
                    Ok(Some((None, None)))
                }
            })
        })
        .await
}

/// 置顶区整体重编号（现役 pin-order：提交完整有序列表，服务端按序赋 1..n）。
/// 重复 id 由调用方（路由/服务）先行拒绝；这里对不在列表中的已置顶会话
/// 不动（现役 setSessionPinOrder 只重排提交进来的集合）。
pub async fn set_pin_order(
    db: &RunDatabase,
    ordered_session_ids: &[String],
    now_unix_ms: u64,
) -> Result<(), StorageError> {
    let ids: Vec<String> = ordered_session_ids.to_vec();
    let now = now_unix_ms as i64;
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                for (idx, sid) in ids.iter().enumerate() {
                    let n = conn
                        .execute(
                            "UPDATE sessions SET pin_order=?2, \
                             pinned_at_unix_ms=COALESCE(pinned_at_unix_ms,?3) \
                             WHERE session_id=?1",
                            rusqlite::params![sid, (idx + 1) as i64, now],
                        )
                        .map_err(map_err)?;
                    if n == 0 {
                        return Err(StorageError::InvalidRequest {
                            detail: format!("pin_order session {sid} not found"),
                        });
                    }
                }
                Ok(())
            })
        })
        .await?;
    Ok(())
}

/// 直接子对话 id 列表（parent_session_id = 本会话）。
pub async fn list_child_sessions(
    db: &RunDatabase,
    session_id: &str,
) -> Result<Vec<String>, StorageError> {
    let sid = session_id.to_string();
    db.queue()
        .submit(move |conn| {
            let mut stmt = conn
                .prepare("SELECT session_id FROM sessions WHERE parent_session_id=?1")
                .map_err(map_err)?;
            let rows = stmt.query_map([&sid], |r| r.get(0)).map_err(map_err)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r.map_err(map_err)?);
            }
            Ok(out)
        })
        .await
}

/// 全部后代 id 列表（BFS；现役 archive_children 的递归队列同义）。
pub async fn list_descendant_sessions(
    db: &RunDatabase,
    session_id: &str,
) -> Result<Vec<String>, StorageError> {
    let root = session_id.to_string();
    db.queue()
        .submit(move |conn| {
            let mut out = Vec::new();
            let mut seen = std::collections::HashSet::new();
            let mut queue = vec![root.clone()];
            seen.insert(root);
            while let Some(cur) = queue.pop() {
                let mut stmt = conn
                    .prepare("SELECT session_id FROM sessions WHERE parent_session_id=?1")
                    .map_err(map_err)?;
                let rows = stmt
                    .query_map([&cur], |r| r.get::<_, String>(0))
                    .map_err(map_err)?;
                for r in rows {
                    let child = r.map_err(map_err)?;
                    if seen.insert(child.clone()) {
                        out.push(child.clone());
                        queue.push(child);
                    }
                }
            }
            Ok(out)
        })
        .await
}

/// 归档（含子对话策略）。`skip_ids`：流式中的后代（服务层按 supervisor 状态
/// 提供），archive_children 时跳过并计数。直接子对话存在且无 childMode →
/// `StorageError::Conflict{detail:"child_sessions_present:<n>"}`（对照现役 409）。
pub async fn archive_session(
    db: &RunDatabase,
    session_id: &str,
    child_mode: Option<ChildMode>,
    skip_ids: &[String],
    now_unix_ms: u64,
) -> Result<ArchiveOutcome, StorageError> {
    let sid = session_id.to_string();
    let mode = child_mode;
    let skips: Vec<String> = skip_ids.to_vec();
    let now = now_unix_ms as i64;
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                let lifecycle: Option<String> = conn
                    .query_row(
                        "SELECT lifecycle FROM sessions WHERE session_id=?1",
                        [&sid],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(map_err)?;
                let Some(lifecycle) = lifecycle else {
                    return Err(StorageError::InvalidRequest {
                        detail: format!("session {sid} not found"),
                    });
                };
                if lifecycle != "active" {
                    return Err(StorageError::Conflict {
                        detail: format!(
                            "session {sid} lifecycle is {lifecycle}, archive requires active"
                        ),
                    });
                }
                let mut stmt = conn
                    .prepare("SELECT session_id FROM sessions WHERE parent_session_id=?1")
                    .map_err(map_err)?;
                let direct: Vec<String> = stmt
                    .query_map([&sid], |r| r.get(0))
                    .map_err(map_err)?
                    .collect::<Result<_, _>>()
                    .map_err(map_err)?;
                drop(stmt);
                if !direct.is_empty() && mode.is_none() {
                    return Err(StorageError::Conflict {
                        detail: format!("child_sessions_present:{}", direct.len()),
                    });
                }
                let mut outcome = ArchiveOutcome {
                    archived_children: 0,
                    detached_children: 0,
                    skipped_streaming_children: 0,
                };
                match mode {
                    Some(ChildMode::DetachChildren) => {
                        let n = conn
                            .execute(
                                "UPDATE sessions SET parent_session_id=NULL \
                                 WHERE parent_session_id=?1",
                                [&sid],
                            )
                            .map_err(map_err)?;
                        outcome.detached_children = n;
                    }
                    Some(ChildMode::ArchiveChildren) => {
                        // BFS 收集后代（含间接），跳过 skip_ids。
                        let mut descendants = Vec::new();
                        let mut seen = std::collections::HashSet::new();
                        let mut queue = direct.clone();
                        for d in &queue {
                            seen.insert(d.clone());
                        }
                        while let Some(cur) = queue.pop() {
                            descendants.push(cur.clone());
                            let mut stmt = conn
                                .prepare(
                                    "SELECT session_id FROM sessions WHERE parent_session_id=?1",
                                )
                                .map_err(map_err)?;
                            let rows = stmt
                                .query_map([&cur], |r| r.get::<_, String>(0))
                                .map_err(map_err)?;
                            for r in rows {
                                let child = r.map_err(map_err)?;
                                if seen.insert(child.clone()) {
                                    queue.push(child);
                                }
                            }
                        }
                        for d in &descendants {
                            if skips.contains(d) {
                                outcome.skipped_streaming_children += 1;
                                continue;
                            }
                            // REPAIR-R1 FINDING-08：只计真实翻转的后代
                            // （UPDATE 带 lifecycle='active' 过滤；已归档的
                            // 不计入——对照现役逐子成功才递增 archivedChildren）。
                            let flipped = conn
                                .execute(
                                    "UPDATE sessions SET lifecycle='archived', \
                                 archived_at_unix_ms=?2 WHERE session_id=?1 \
                                 AND lifecycle='active'",
                                    rusqlite::params![d, now],
                                )
                                .map_err(map_err)?;
                            if flipped > 0 {
                                outcome.archived_children += 1;
                            }
                        }
                    }
                    None => {}
                }
                conn.execute(
                    "UPDATE sessions SET lifecycle='archived', archived_at_unix_ms=?2 \
                     WHERE session_id=?1",
                    rusqlite::params![sid, now],
                )
                .map_err(map_err)?;
                Ok(outcome)
            })
        })
        .await
}

/// 恢复归档会话为 active（现役 restore：仅归档态允许）。
pub async fn restore_session(
    db: &RunDatabase,
    session_id: &str,
    now_unix_ms: u64,
) -> Result<LifecycleOutcome, StorageError> {
    let sid = session_id.to_string();
    let now = now_unix_ms as i64;
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                let lifecycle: Option<String> = conn
                    .query_row(
                        "SELECT lifecycle FROM sessions WHERE session_id=?1",
                        [&sid],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(map_err)?;
                let Some(lifecycle) = lifecycle else {
                    return Ok(LifecycleOutcome::NotFound);
                };
                if lifecycle != "archived" {
                    return Ok(LifecycleOutcome::WrongLifecycle);
                }
                conn.execute(
                    "UPDATE sessions SET lifecycle='active', archived_at_unix_ms=NULL, \
                     last_activity_unix_ms=?2 WHERE session_id=?1",
                    rusqlite::params![sid, now],
                )
                .map_err(map_err)?;
                Ok(LifecycleOutcome::Done)
            })
        })
        .await
}

/// 永久删除一个**已归档**会话及其全部派生数据。
///
/// FK 依赖序（foreign_keys=ON；逐表亲验列名于 migrations.rs v1-v8）：
/// 1. checkpoint_file_versions ← checkpoints(checkpoint_id)
/// 2. checkpoints ← sessions
/// 3. file_checkpoints（session_id 无 FK，按会话清）
/// 4. session_branch_heads ← sessions
/// 5. messages ← sessions
/// 6. key_events（无 FK，按 session_id 清）
/// 7. stale_result_audit（无 FK，按 session_id 清）
/// 8. invocation_journal ← runs(run_id)
/// 9. run_lineage ← runs(run_id)
/// 10. invocations ← runs(run_id)
/// 11. run_attempts ← runs(run_id)
/// 12. model_call_usage（无 FK，按 session_id 清）
/// 13. runs ← sessions
/// 14. sessions
pub async fn delete_archived_session(
    db: &RunDatabase,
    session_id: &str,
) -> Result<LifecycleOutcome, StorageError> {
    let sid = session_id.to_string();
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                let lifecycle: Option<String> = conn
                    .query_row(
                        "SELECT lifecycle FROM sessions WHERE session_id=?1",
                        [&sid],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(map_err)?;
                let Some(lifecycle) = lifecycle else {
                    return Ok(LifecycleOutcome::NotFound);
                };
                if lifecycle != "archived" {
                    return Ok(LifecycleOutcome::WrongLifecycle);
                }
                let cp_ids: Vec<String> = {
                    let mut stmt = conn
                        .prepare("SELECT checkpoint_id FROM checkpoints WHERE session_id=?1")
                        .map_err(map_err)?;
                    let rows = stmt.query_map([&sid], |r| r.get(0)).map_err(map_err)?;
                    let mut v = Vec::new();
                    for r in rows {
                        v.push(r.map_err(map_err)?);
                    }
                    v
                };
                for cp in &cp_ids {
                    conn.execute(
                        "DELETE FROM checkpoint_file_versions WHERE checkpoint_id=?1",
                        [cp],
                    )
                    .map_err(map_err)?;
                }
                // FK 顺序：子表 file_checkpoints（REFERENCES checkpoints）必须先于
                // checkpoints 删除（REPAIR-R1 FINDING-07 激活 FK 后的硬性顺序）。
                conn.execute("DELETE FROM file_checkpoints WHERE session_id=?1", [&sid])
                    .map_err(map_err)?;
                conn.execute("DELETE FROM checkpoints WHERE session_id=?1", [&sid])
                    .map_err(map_err)?;
                conn.execute(
                    "DELETE FROM session_branch_heads WHERE session_id=?1",
                    [&sid],
                )
                .map_err(map_err)?;
                conn.execute("DELETE FROM messages WHERE session_id=?1", [&sid])
                    .map_err(map_err)?;
                conn.execute("DELETE FROM key_events WHERE session_id=?1", [&sid])
                    .map_err(map_err)?;
                conn.execute("DELETE FROM stale_result_audit WHERE session_id=?1", [&sid])
                    .map_err(map_err)?;
                let run_ids: Vec<String> = {
                    let mut stmt = conn
                        .prepare("SELECT run_id FROM runs WHERE session_id=?1")
                        .map_err(map_err)?;
                    let rows = stmt.query_map([&sid], |r| r.get(0)).map_err(map_err)?;
                    let mut v = Vec::new();
                    for r in rows {
                        v.push(r.map_err(map_err)?);
                    }
                    v
                };
                for rid in &run_ids {
                    conn.execute("DELETE FROM invocation_journal WHERE run_id=?1", [rid])
                        .map_err(map_err)?;
                    conn.execute("DELETE FROM run_lineage WHERE run_id=?1", [rid])
                        .map_err(map_err)?;
                    conn.execute("DELETE FROM invocations WHERE run_id=?1", [rid])
                        .map_err(map_err)?;
                    conn.execute("DELETE FROM run_attempts WHERE run_id=?1", [rid])
                        .map_err(map_err)?;
                }
                conn.execute("DELETE FROM model_call_usage WHERE session_id=?1", [&sid])
                    .map_err(map_err)?;
                conn.execute("DELETE FROM runs WHERE session_id=?1", [&sid])
                    .map_err(map_err)?;
                conn.execute("DELETE FROM sessions WHERE session_id=?1", [&sid])
                    .map_err(map_err)?;
                Ok(LifecycleOutcome::Done)
            })
        })
        .await
}

/// 列出已归档会话（归档时间倒序；对照现役 listArchivedSessions）。
/// `owner = None` 不过滤（本地主人视野）；`Some(uid)` 只看该用户。
pub async fn list_archived_sessions(
    db: &RunDatabase,
    owner: Option<&str>,
) -> Result<Vec<SessionSummaryRow>, StorageError> {
    let owner: Option<String> = owner.map(str::to_string);
    db.queue()
        .submit(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT session_id, agent_id, owner_user_id, title, lifecycle, \
                     created_at_unix_ms, archived_at_unix_ms, last_activity_unix_ms \
                     FROM sessions WHERE lifecycle='archived' \
                     AND (?1 IS NULL OR owner_user_id=?1) \
                     ORDER BY archived_at_unix_ms DESC",
                )
                .map_err(map_err)?;
            let rows = stmt
                .query_map([&owner], |r| {
                    Ok(SessionSummaryRow {
                        session_id: r.get(0)?,
                        agent_id: r.get(1)?,
                        owner_user_id: r.get(2)?,
                        title: r.get(3)?,
                        lifecycle: r.get(4)?,
                        created_at_unix_ms: r.get(5)?,
                        archived_at_unix_ms: r.get(6)?,
                        last_activity_unix_ms: r.get(7)?,
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

/// 记忆开关（现役 memory GET/PATCH；inert 数据位，不构成安全边界）。
/// 返回 None = 会话不存在；否则返回新值。
pub async fn set_memory_enabled(
    db: &RunDatabase,
    session_id: &str,
    enabled: bool,
) -> Result<Option<bool>, StorageError> {
    let sid = session_id.to_string();
    let val = i64::from(enabled);
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                let n = conn
                    .execute(
                        "UPDATE sessions SET memory_enabled=?2 WHERE session_id=?1",
                        rusqlite::params![sid, val],
                    )
                    .map_err(map_err)?;
                Ok(if n > 0 { Some(enabled) } else { None })
            })
        })
        .await
}

/// 触碰最近活动时间（执行/提交路径与归档共用）。
pub async fn touch_last_activity(
    db: &RunDatabase,
    session_id: &str,
    now_unix_ms: u64,
) -> Result<(), StorageError> {
    let sid = session_id.to_string();
    let now = now_unix_ms as i64;
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                conn.execute(
                    "UPDATE sessions SET last_activity_unix_ms=?2 WHERE session_id=?1",
                    rusqlite::params![sid, now],
                )
                .map_err(map_err)?;
                Ok(())
            })
        })
        .await?;
    Ok(())
}

/// 一次搜索命中的投影（title 或 content 阶段）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub session_id: String,
    pub title: String,
    /// content 阶段：命中消息内容截断（对照现役 firstMessage 截断 100）。
    pub snippet: String,
    pub match_kind: String,
    pub created_at_unix_ms: i64,
    pub last_activity_unix_ms: Option<i64>,
}

/// 会话搜索（对照现役 /sessions/search，sessions.ts:857）：
/// `phase=title` 匹配标题；`phase=content` 匹配消息内容。
/// LIKE 转义在调用方拼 pattern 时完成（`\` 转义 `%`/`_`）。
/// `owner = None` 不过滤（本地主人视野）。
pub async fn search_sessions(
    db: &RunDatabase,
    owner: Option<&str>,
    like_pattern: &str,
    phase: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, StorageError> {
    let owner: Option<String> = owner.map(str::to_string);
    let pat = like_pattern.to_string();
    let content_phase = phase == "content";
    let lim = limit as i64;
    db.queue()
        .submit(move |conn| {
            let mut out = Vec::new();
            if content_phase {
                let mut stmt = conn
                    .prepare(
                        "SELECT s.session_id, s.title, m.content_json, \
                         s.created_at_unix_ms, s.last_activity_unix_ms \
                         FROM sessions s JOIN messages m ON m.session_id = s.session_id \
                         WHERE (?1 IS NULL OR s.owner_user_id=?1) \
                         AND s.lifecycle='active' \
                         AND m.entry_type='message' \
                         AND m.content_json LIKE ?2 ESCAPE '\\' \
                         GROUP BY s.session_id ORDER BY s.last_activity_unix_ms DESC LIMIT ?3",
                    )
                    .map_err(map_err)?;
                let rows = stmt
                    .query_map(rusqlite::params![owner, pat, lim], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, i64>(3)?,
                            r.get::<_, Option<i64>>(4)?,
                        ))
                    })
                    .map_err(map_err)?;
                for r in rows {
                    let (sid, title, content, created, activity) = r.map_err(map_err)?;
                    out.push(SearchHit {
                        session_id: sid,
                        title,
                        snippet: content.chars().take(100).collect(),
                        match_kind: "content".to_string(),
                        created_at_unix_ms: created,
                        last_activity_unix_ms: activity,
                    });
                }
            } else {
                let mut stmt = conn
                    .prepare(
                        "SELECT session_id, title, created_at_unix_ms, last_activity_unix_ms \
                         FROM sessions WHERE (?1 IS NULL OR owner_user_id=?1) \
                         AND lifecycle='active' \
                         AND title LIKE ?2 ESCAPE '\\' \
                         ORDER BY last_activity_unix_ms DESC LIMIT ?3",
                    )
                    .map_err(map_err)?;
                let rows = stmt
                    .query_map(rusqlite::params![owner, pat, lim], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, i64>(2)?,
                            r.get::<_, Option<i64>>(3)?,
                        ))
                    })
                    .map_err(map_err)?;
                for r in rows {
                    let (sid, title, created, activity) = r.map_err(map_err)?;
                    out.push(SearchHit {
                        session_id: sid,
                        title,
                        snippet: String::new(),
                        match_kind: "title".to_string(),
                        created_at_unix_ms: created,
                        last_activity_unix_ms: activity,
                    });
                }
            }
            Ok(out)
        })
        .await
}
