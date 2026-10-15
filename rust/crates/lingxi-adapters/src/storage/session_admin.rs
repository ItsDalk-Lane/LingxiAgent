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
    pub errors: usize,
}

/// 失败仍然是错误；计数仅说明已经观察到的跳过项和整批回滚。
#[derive(Debug, Clone)]
pub struct ArchiveFailure {
    pub source: StorageError,
    pub outcome: ArchiveOutcome,
    pub rolled_back: bool,
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
    pub first_message: String,
}

#[derive(serde::Deserialize)]
struct StoredUserInput {
    text: String,
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
    create_session_prepared(
        db,
        SessionCreation {
            session_id,
            agent_id,
            owner_user_id,
            title,
            permission_mode,
            authorized_folders,
            memory_enabled,
            now_unix_ms,
            project_id: None,
            lineage: None,
        },
    )
    .await
}

/// 焦点创建在调用方持新会话许可时提交完整元数据，项目归属与会话行同事务。
pub struct SessionCreation<'a> {
    pub session_id: &'a str,
    pub agent_id: &'a str,
    pub owner_user_id: &'a str,
    pub title: &'a str,
    pub permission_mode: Option<&'a str>,
    pub authorized_folders: &'a [String],
    pub memory_enabled: bool,
    pub now_unix_ms: u64,
    pub project_id: Option<&'a str>,
    pub lineage: Option<(&'a str, u32)>,
}
pub async fn create_session_prepared(
    db: &RunDatabase,
    creation: SessionCreation<'_>,
) -> Result<(), StorageError> {
    let SessionCreation {
        session_id,
        agent_id,
        owner_user_id,
        title,
        permission_mode,
        authorized_folders,
        memory_enabled,
        now_unix_ms,
        project_id,
        lineage,
    } = creation;
    if session_id.starts_with(super::run_store::MOOD_STREAM_PREFIX) {
        return Err(StorageError::InvalidRequest {
            detail: "reserved mood stream namespace cannot be used as a session id".into(),
        });
    }
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
    let project = project_id.map(str::to_string);
    let parent = lineage.map(|(parent, _)| parent.to_string());
    let depth = lineage.map_or(0, |(_, depth)| i64::from(depth));
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
                super::session_operations::assert_fresh_identity(conn, &sid)?;
                if let Some(parent) = parent.as_deref() {
                    let actual: Option<i64> = conn
                        .query_row(
                            "SELECT lineage_depth FROM sessions WHERE session_id=?1",
                            [parent],
                            |row| row.get(0),
                        )
                        .optional()
                        .map_err(map_err)?;
                    if actual
                        .and_then(|depth| {
                            lingxi_kernel::session_tree::check_fork_depth(depth as u32)
                        })
                        .map(i64::from)
                        != Some(depth)
                    {
                        return Err(StorageError::Conflict {
                            detail: "new_session_lineage_changed".into(),
                        });
                    }
                }
                conn.execute(
                    "INSERT INTO sessions \
                 (session_id, agent_id, owner_user_id, title, created_at_unix_ms, \
                  lifecycle, lineage_depth, permission_mode, authorized_folders_json, \
                  memory_enabled, last_activity_unix_ms, project_id, parent_session_id) \
                 VALUES (?1,?2,?3,?4,?5,'active',?11,?6,?7,?8,?5,?9,?10)",
                    rusqlite::params![
                        sid,
                        aid,
                        owner,
                        ttl,
                        now,
                        perm,
                        folders_json,
                        mem,
                        project,
                        parent,
                        depth
                    ],
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
    _now_unix_ms: u64,
) -> Result<(), StorageError> {
    let ids: Vec<String> = ordered_session_ids.to_vec();
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                for (idx, sid) in ids.iter().enumerate() {
                    let n = conn
                        .execute(
                            "UPDATE sessions SET pin_order=?2 \
                             WHERE session_id=?1 AND pinned_at_unix_ms IS NOT NULL",
                            rusqlite::params![sid, (idx + 1) as i64],
                        )
                        .map_err(map_err)?;
                    if n == 0 {
                        return Err(StorageError::InvalidRequest {
                            detail: format!("pin_order session {sid} missing or not pinned"),
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
    archive_session_counted(db, session_id, child_mode, skip_ids, now_unix_ms)
        .await
        .map_err(|failure| failure.source)
}

pub async fn archive_session_counted(
    db: &RunDatabase,
    session_id: &str,
    child_mode: Option<ChildMode>,
    skip_ids: &[String],
    now_unix_ms: u64,
) -> Result<ArchiveOutcome, ArchiveFailure> {
    let expected_descendants = if db.legacy_session_root().is_some() {
        Some(
            list_descendant_sessions(db, session_id)
                .await
                .map_err(|source| ArchiveFailure {
                    source,
                    outcome: ArchiveOutcome {
                        archived_children: 0,
                        detached_children: 0,
                        skipped_streaming_children: 0,
                        errors: 1,
                    },
                    rolled_back: false,
                })?,
        )
    } else {
        None
    };
    let mut ids = vec![session_id.to_string()];
    if child_mode == Some(ChildMode::ArchiveChildren) {
        if let Some(descendants) = &expected_descendants {
            ids.extend(
                descendants
                    .iter()
                    .filter(|id| !skip_ids.contains(id))
                    .cloned(),
            );
        }
    }
    let physical = super::legacy_session_files::Batch::prepare(
        db,
        &ids,
        super::legacy_session_files::Transition::Archive,
        now_unix_ms,
    )
    .await
    .map_err(|source| ArchiveFailure {
        source,
        outcome: ArchiveOutcome {
            archived_children: 0,
            detached_children: 0,
            skipped_streaming_children: skip_ids.len(),
            errors: 1,
        },
        rolled_back: false,
    })?;
    let physical_in_txn = physical.clone();
    let sid = session_id.to_string();
    let mode = child_mode;
    let skips: Vec<String> = skip_ids.to_vec();
    let now = now_unix_ms as i64;
    let busy = db.options().busy_timeout_ms;
    let skipped = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let skipped_in_txn = skipped.clone();
    let result = db
        .queue()
        .submit(move |conn| {
            physical_in_txn.transaction(conn, busy, |conn| {
                if let Some(expected)=&expected_descendants {
                    let mut stmt=conn.prepare("WITH RECURSIVE descendants(id) AS (SELECT session_id FROM sessions WHERE parent_session_id=?1 UNION SELECT s.session_id FROM sessions s JOIN descendants d ON s.parent_session_id=d.id) SELECT id FROM descendants ORDER BY id").map_err(map_err)?;
                    let actual=stmt.query_map([&sid],|r|r.get::<_,String>(0)).map_err(map_err)?.collect::<Result<Vec<_>,_>>().map_err(map_err)?;
                    let mut expected=expected.clone();expected.sort();
                    if actual!=expected{return Err(StorageError::Conflict{detail:"legacy archive lineage changed before commit".into()});}
                }
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
                    errors: 0,
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
                        outcome.skipped_streaming_children =
                            descendants.iter().filter(|id| skips.contains(id)).count();
                        skipped_in_txn.store(
                            outcome.skipped_streaming_children,
                            std::sync::atomic::Ordering::SeqCst,
                        );
                        for d in &descendants {
                            if skips.contains(d) {
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
        .await;
    let result = result.map_err(|source| ArchiveFailure {
        source,
        outcome: ArchiveOutcome {
            // 事务未提交，已尝试的翻转不计为成功。
            archived_children: 0,
            detached_children: 0,
            skipped_streaming_children: skipped.load(std::sync::atomic::Ordering::SeqCst),
            errors: 1,
        },
        rolled_back: true,
    })?;
    physical.finish(db).await.map_err(|source| ArchiveFailure {
        source,
        outcome: result.clone(),
        rolled_back: false,
    })?;
    Ok(result)
}

/// 恢复归档会话为 active（现役 restore：仅归档态允许）。
pub async fn restore_session(
    db: &RunDatabase,
    session_id: &str,
    now_unix_ms: u64,
) -> Result<LifecycleOutcome, StorageError> {
    restore_session_checked(db, session_id, now_unix_ms, None).await
}
/// 已认证DB归属作最终CAS；路径只比较，不能登记任意客户端落点。
pub async fn restore_session_checked(
    db: &RunDatabase,
    session_id: &str,
    now_unix_ms: u64,
    expected: Option<super::legacy_session_files::ExpectedSource>,
) -> Result<LifecycleOutcome, StorageError> {
    let physical = super::legacy_session_files::Batch::prepare_checked(
        db,
        &[session_id.to_string()],
        super::legacy_session_files::Transition::Restore,
        now_unix_ms,
        expected.clone(),
    )
    .await?;
    let physical_in_txn = physical.clone();
    let sid = session_id.to_string();
    let now = now_unix_ms as i64;
    let busy = db.options().busy_timeout_ms;
    let result = db
        .queue()
        .submit(move |conn| {
            physical_in_txn.transaction(conn, busy, |conn| {
                if let Some(expected)=&expected {
                    let actual:Option<(String,String)>=conn.query_row("SELECT agent_id,owner_user_id FROM sessions WHERE session_id=?1",[&sid],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(map_err)?;
                    if actual.is_some()&&actual!=Some((expected.agent.clone(),expected.owner.clone())){return Err(StorageError::Conflict{detail:"legacy_session_conflict: authorized restore owner/agent changed before commit".into()});}
                }
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
        .await?;
    physical.finish(db).await?;
    Ok(result)
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
    let physical = super::legacy_session_files::Batch::prepare(
        db,
        &[session_id.to_string()],
        super::legacy_session_files::Transition::Delete,
        0,
    )
    .await?;
    let physical_in_txn = physical.clone();
    let sid = session_id.to_string();
    let busy = db.options().busy_timeout_ms;
    let result=db.queue()
        .submit(move |conn| {
            physical_in_txn.transaction(conn, busy, |conn| {
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
                // 托管字节尚未实际删完时保留完整身份行，供失败后或重启恢复清扫。
                conn.execute("DELETE FROM session_file_refs WHERE session_id=?1",[&sid]).map_err(map_err)?;
                conn.execute("DELETE FROM session_file_aliases WHERE canonical_file_id IN (SELECT file_id FROM session_files WHERE owner_session_id=?1 AND storage_kind!='managed_cache' AND NOT EXISTS (SELECT 1 FROM session_file_refs r WHERE r.file_id=session_files.file_id))",[&sid]).map_err(map_err)?;
                conn.execute("DELETE FROM session_files WHERE owner_session_id=?1 AND storage_kind!='managed_cache' AND NOT EXISTS (SELECT 1 FROM session_file_refs r WHERE r.file_id=session_files.file_id)",[&sid]).map_err(map_err)?;
                // 分支锚属于本会话审计，先于其消息外键清理。
                conn.execute("DELETE FROM run_history_anchors WHERE session_id=?1", [&sid]).map_err(map_err)?;
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
        .await?;
    physical.finish(db).await?;
    Ok(result)
}

/// 列出已归档会话（归档时间倒序；对照现役 listArchivedSessions）。
/// `owner = None` 不过滤（本地主人视野）；`Some(uid)` 只看该用户。
pub async fn list_archived_sessions(
    db: &RunDatabase,
    owner: Option<&str>,
) -> Result<Vec<SessionSummaryRow>, StorageError> {
    read_archived_sessions(db, owner, false).await
}

pub async fn list_archived_sessions_with_preview(
    db: &RunDatabase,
    owner: Option<&str>,
) -> Result<Vec<SessionSummaryRow>, StorageError> {
    read_archived_sessions(db, owner, true).await
}

async fn read_archived_sessions(
    db: &RunDatabase,
    owner: Option<&str>,
    include_preview: bool,
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
                        first_message: String::new(),
                    })
                })
                .map_err(map_err)?;
            let mut out = rows.collect::<Result<Vec<_>, _>>().map_err(map_err)?;
            if !include_preview {
                return Ok(out);
            }
            let mut messages = conn
                .prepare(
                    "SELECT content_json FROM messages WHERE session_id=?1 \
                     AND role='user' AND entry_type='message' ORDER BY seq",
                )
                .map_err(map_err)?;
            for row in &mut out {
                let contents = messages
                    .query_map([&row.session_id], |r| r.get::<_, String>(0))
                    .map_err(map_err)?;
                for content in contents {
                    let raw = content.map_err(map_err)?;
                    let input: StoredUserInput =
                        serde_json::from_str(&raw).map_err(|error| StorageError::Corrupted {
                            detail: format!(
                                "archived session {} has invalid message content: {error}",
                                row.session_id
                            ),
                        })?;
                    let visible = strip_preview_envelopes(&input.text);
                    if !visible.is_empty() {
                        // Match the original JS 100-unit limit without splitting a UTF-8 scalar.
                        let mut units = 0;
                        row.first_message = visible
                            .chars()
                            .take_while(|ch| {
                                units += ch.len_utf16();
                                units <= 100
                            })
                            .collect();
                        break;
                    }
                }
            }
            Ok(out)
        })
        .await
}

fn strip_preview_envelopes(text: &str) -> String {
    let mut visible = Vec::new();
    let mut block_end = None;
    let mut drop_separator = false;
    for line in text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
    {
        if let Some(end) = block_end {
            if line == end {
                block_end = None;
                drop_separator = true;
            }
            continue;
        }
        block_end = match line {
            "[hana_reference]" => Some("[/hana_reference]"),
            "[KnowledgeContext]" => Some("[/KnowledgeContext]"),
            "[KnowledgeResearchContext]" => Some("[/KnowledgeResearchContext]"),
            _ if is_preview_reminder_header(line) => Some("[/hana_reminder]"),
            _ => None,
        };
        if block_end.is_some() || (drop_separator && line.is_empty()) {
            continue;
        }
        drop_separator = false;
        visible.push(line);
    }
    while visible.last() == Some(&"") {
        visible.pop();
    }
    visible.join("\n")
}

fn is_preview_reminder_header(line: &str) -> bool {
    if line == "[hana_reminder]" {
        return true;
    }
    let Some(timestamp) = line
        .strip_prefix("[hana_reminder at ")
        .and_then(|value| value.strip_suffix(']'))
    else {
        return false;
    };
    timestamp.len() == 16
        && timestamp
            .bytes()
            .enumerate()
            .all(|(index, byte)| match index {
                4 | 7 => byte == b'-',
                10 => byte == b' ',
                13 => byte == b':',
                _ => byte.is_ascii_digit(),
            })
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
                let archived: Option<Option<i64>> = conn
                    .query_row(
                        "SELECT archived_at_unix_ms FROM sessions WHERE session_id=?1",
                        [&sid],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(map_err)?;
                match archived {
                    None => return Ok(None),
                    Some(Some(_)) => {
                        return Err(StorageError::Conflict {
                            detail: "session_memory_inactive".into(),
                        })
                    }
                    Some(None) => {}
                }
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

/// R06 项目删除：列出归属某项目的活动会话 id（会话改派用）。
pub async fn session_ids_for_project(
    db: &RunDatabase,
    project_id: &str,
) -> Result<Vec<String>, StorageError> {
    let project = project_id.to_string();
    db.queue()
        .submit(move |conn| {
            let mut statement = conn
                .prepare(
                    "SELECT session_id FROM sessions \
                     WHERE project_id=?1 AND archived_at_unix_ms IS NULL \
                     ORDER BY session_id",
                )
                .map_err(map_err)?;
            let rows = statement
                .query_map([&project], |row| row.get::<_, String>(0))
                .map_err(map_err)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(map_err)?);
            }
            Ok(out)
        })
        .await
}

/// R06 轮换：按创建时间取指定助手最近一个活动会话 id（无则 None）。
pub async fn latest_active_session_for_agent(
    db: &RunDatabase,
    agent_id: &str,
) -> Result<Option<String>, StorageError> {
    let agent = agent_id.to_string();
    db.queue()
        .submit(move |conn| {
            conn.query_row(
                "SELECT session_id FROM sessions \
                 WHERE agent_id=?1 AND archived_at_unix_ms IS NULL \
                 ORDER BY created_at_unix_ms DESC, session_id DESC LIMIT 1",
                [&agent],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(map_err)
        })
        .await
}

/// R06 v12 会话项目归属写入（现役 setSessionProjectAssignment 的最小端口）：
/// trim 后非空字符串为分配，空/None 为清除（NULL）。仅活动会话可改。
/// 返回 None = 会话不存在；Some(新归属) = 已写入。
pub async fn set_session_project(
    db: &RunDatabase,
    session_id: &str,
    project_id: Option<&str>,
) -> Result<Option<Option<String>>, StorageError> {
    let sid = session_id.to_string();
    let normalized = project_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let owned = normalized.clone();
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                let archived: Option<Option<i64>> = conn
                    .query_row(
                        "SELECT archived_at_unix_ms FROM sessions WHERE session_id=?1",
                        [&sid],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(map_err)?;
                match archived {
                    None => return Ok(None),
                    Some(Some(_)) => {
                        return Err(StorageError::Conflict {
                            detail: "session_project_inactive".into(),
                        })
                    }
                    Some(None) => {}
                }
                let n = conn
                    .execute(
                        "UPDATE sessions SET project_id=?2 WHERE session_id=?1",
                        rusqlite::params![sid, owned],
                    )
                    .map_err(map_err)?;
                Ok(if n > 0 { Some(normalized) } else { None })
            })
        })
        .await
}

/// R06 v13 工作台身份投影行（disposal / sweep 的扫描面）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionWorkspaceRow {
    pub session_id: String,
    pub owner_user_id: String,
    /// studio 挂载 mountId（NULL = 历史 cwd 形态或无工作台会话）。
    pub workspace_mount_id: Option<String>,
    /// 会话工作目录（NULL = 未记录）。
    pub cwd: Option<String>,
}

/// R06 v13 工作台身份写入（现役会话创建时携带 workspaceMountId/cwd 的
/// 存储层端口）：trim 后非空字符串为赋值，空串/None 存 NULL（清除）。
/// 仅活动会话可改（对照 set_session_project 的生命周期闸）；返回
/// None = 会话不存在，Some((mount, cwd)) = 写入后的两列值。
pub async fn set_session_workspace(
    db: &RunDatabase,
    session_id: &str,
    workspace_mount_id: Option<&str>,
    cwd: Option<&str>,
) -> Result<Option<(Option<String>, Option<String>)>, StorageError> {
    let sid = session_id.to_string();
    let mount = workspace_mount_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let owned_cwd = cwd
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                let archived: Option<Option<i64>> = conn
                    .query_row(
                        "SELECT archived_at_unix_ms FROM sessions WHERE session_id=?1",
                        [&sid],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(map_err)?;
                match archived {
                    None => return Ok(None),
                    Some(Some(_)) => {
                        return Err(StorageError::Conflict {
                            detail: "session_workspace_inactive".into(),
                        })
                    }
                    Some(None) => {}
                }
                let n = conn
                    .execute(
                        "UPDATE sessions SET workspace_mount_id=?2, cwd=?3 WHERE session_id=?1",
                        rusqlite::params![sid, mount, owned_cwd],
                    )
                    .map_err(map_err)?;
                Ok(if n > 0 {
                    Some((mount, owned_cwd))
                } else {
                    None
                })
            })
        })
        .await
}

/// R06 v13 工作台身份扫描：全部活动会话的 (session_id, owner, mount, cwd)
/// （现役 disposal/sweep 前置的 engine.listSessions 投影；`owner = None`
/// 不过滤——本地主人视野，`Some(uid)` 只看该用户，与归档列表同口径）。
pub async fn list_active_session_workspaces(
    db: &RunDatabase,
    owner: Option<&str>,
) -> Result<Vec<SessionWorkspaceRow>, StorageError> {
    let owner: Option<String> = owner.map(str::to_string);
    db.queue()
        .submit(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT session_id, owner_user_id, workspace_mount_id, cwd \
                     FROM sessions WHERE lifecycle='active' \
                     AND (?1 IS NULL OR owner_user_id=?1) \
                     ORDER BY session_id",
                )
                .map_err(map_err)?;
            let rows = stmt
                .query_map([&owner], |r| {
                    Ok(SessionWorkspaceRow {
                        session_id: r.get(0)?,
                        owner_user_id: r.get(1)?,
                        workspace_mount_id: r.get(2)?,
                        cwd: r.get(3)?,
                    })
                })
                .map_err(map_err)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(map_err)?);
            }
            Ok(out)
        })
        .await
}

/// R06 独立会话的谱系锚点写入（new-detached 的 forkedFrom 线索）：
/// 只写 parent_session_id / lineage_depth，不复制消息（区别于 fork）。
pub async fn set_session_lineage(
    db: &RunDatabase,
    session_id: &str,
    parent_session_id: &str,
    lineage_depth: u32,
) -> Result<bool, StorageError> {
    let sid = session_id.to_string();
    let parent = parent_session_id.to_string();
    let depth = i64::from(lineage_depth);
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                let n = conn
                    .execute(
                        "UPDATE sessions SET parent_session_id=?2, lineage_depth=?3 \
                         WHERE session_id=?1 AND lifecycle='active'",
                        rusqlite::params![sid, parent, depth],
                    )
                    .map_err(map_err)?;
                Ok(n > 0)
            })
        })
        .await
}

/// R06 会话授权目录整体替换（现役 setSessionAuthorizedFolders 的存储层）：
/// 仅活动会话可改；已归档 → 冲突，未知会话 → None。列表原样落盘
/// （去重/存在性校验由调用方先行完成），返回替换后的列表。
pub async fn set_authorized_folders(
    db: &RunDatabase,
    session_id: &str,
    folders: &[String],
) -> Result<Option<Vec<String>>, StorageError> {
    let sid = session_id.to_string();
    let owned = folders.to_vec();
    let json = serde_json::to_string(&owned).map_err(|e| StorageError::Internal {
        detail: e.to_string(),
    })?;
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                let archived: Option<Option<i64>> = conn
                    .query_row(
                        "SELECT archived_at_unix_ms FROM sessions WHERE session_id=?1",
                        [&sid],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(map_err)?;
                match archived {
                    None => return Ok(None),
                    Some(Some(_)) => {
                        return Err(StorageError::Conflict {
                            detail: "session_authorized_folders_inactive".into(),
                        })
                    }
                    Some(None) => {}
                }
                let n = conn
                    .execute(
                        "UPDATE sessions SET authorized_folders_json=?2 WHERE session_id=?1",
                        rusqlite::params![sid, json],
                    )
                    .map_err(map_err)?;
                Ok(if n > 0 { Some(owned) } else { None })
            })
        })
        .await
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

/// 标题片段围绕真正命中位置，长标题和多字节字符不会把命中截掉。
fn title_search_snippet(title: &str, like_pattern: &str) -> String {
    let mut escaped = false;
    let mut query = String::new();
    let wrapped = like_pattern.strip_prefix('%').unwrap_or(like_pattern);
    let literal = wrapped.strip_suffix('%').unwrap_or(wrapped);
    for ch in literal.chars() {
        if escaped {
            query.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else {
            query.push(ch);
        }
    }
    let text: Vec<char> = title.chars().collect();
    let needle: Vec<char> = query.chars().map(|ch| ch.to_ascii_lowercase()).collect();
    let folded: Vec<char> = text.iter().map(|ch| ch.to_ascii_lowercase()).collect();
    let at = if needle.is_empty() {
        0
    } else {
        folded
            .windows(needle.len())
            .position(|part| part == needle)
            .unwrap_or(0)
    };
    let start = at.saturating_sub(35);
    let end = (at + needle.len().max(1) + 65).min(text.len());
    let mut snippet: String = text[start..end].iter().collect();
    if start > 0 {
        snippet.insert(0, '…');
    }
    if end < text.len() {
        snippet.push('…');
    }
    snippet
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
                    let snippet = title_search_snippet(&title, &pat);
                    out.push(SearchHit {
                        session_id: sid,
                        title,
                        snippet,
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

/// /reset 真正清除当前会话消息与正文事件，不以回移分支头冒充删除。
/// 保留会话身份及已发生费用；别的会话、文件内容和模型账单不参与清理。
pub async fn clear_current_history(
    db: &RunDatabase,
    session_id: &str,
    owner: &str,
    now: u64,
) -> Result<(usize, lingxi_protocol::EventEnvelope), StorageError> {
    let permit = db
        .session_admission()
        .try_acquire(&[session_id], usize::MAX)
        .map_err(|_| StorageError::Conflict {
            detail: "session_busy".into(),
        })?;
    let (sid, owner) = (session_id.to_owned(), owner.to_owned());
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move|conn| {
        let _permit=permit;
        with_write_txn(conn,busy,|conn| {
            let row:Option<(String,String)>=conn.query_row("SELECT owner_user_id,lifecycle FROM sessions WHERE session_id=?1",[&sid],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(map_err)?;
            match row {Some((actual,lifecycle)) if actual==owner&&lifecycle=="active"=>{},_=>return Err(StorageError::Conflict{detail:"session ownership/lifecycle changed before clear".into()})}
            let active:i64=conn.query_row("SELECT COUNT(*) FROM runs WHERE session_id=?1 AND status IN ('queued','running','waiting_approval','cancelling')",[&sid],|r|r.get(0)).map_err(map_err)?;
            let pending:i64=conn.query_row("SELECT COUNT(*) FROM session_operations WHERE session_id=?1 AND state='prepared'",[&sid],|r|r.get(0)).map_err(map_err)?;
            if active>0||pending>0{return Err(StorageError::Conflict{detail:"session_busy".into()});}
            conn.execute("DELETE FROM checkpoint_file_versions WHERE checkpoint_id IN (SELECT checkpoint_id FROM checkpoints WHERE session_id=?1)",[&sid]).map_err(map_err)?;
            conn.execute("DELETE FROM file_checkpoints WHERE session_id=?1",[&sid]).map_err(map_err)?;
            conn.execute("DELETE FROM checkpoints WHERE session_id=?1",[&sid]).map_err(map_err)?;
            conn.execute("DELETE FROM run_history_anchors WHERE session_id=?1",[&sid]).map_err(map_err)?;
            let count=conn.execute("DELETE FROM messages WHERE session_id=?1",[&sid]).map_err(map_err)?;
            let old_seq:i64=conn.query_row("SELECT COALESCE(MAX(seq),0) FROM key_events WHERE stream_id=?1",[&sid],|r|r.get(0)).map_err(map_err)?;
            let next_seq=old_seq.checked_add(1).ok_or_else(||StorageError::Corrupted{detail:"history event sequence exhausted".into()})?;
            conn.execute("DELETE FROM key_events WHERE stream_id=?1",[&sid]).map_err(map_err)?;
            let payload=super::session_tree::unknown_payload("session_history_cleared",serde_json::Map::from_iter([("deletedMessages".into(),serde_json::json!(count))]));
            let event_id=format!("history-clear:{sid}:{next_seq}");
            conn.execute("INSERT INTO key_events(event_id,stream_id,seq,session_id,run_id,attempt,event_type,payload_json,committed_at_unix_ms) VALUES(?1,?2,?3,?2,NULL,NULL,'session_history_cleared',?4,?5)",rusqlite::params![event_id,sid,next_seq,lingxi_protocol::canon::canonical_string(&payload),now as i64]).map_err(map_err)?;
            let event=lingxi_protocol::EventEnvelope::new(lingxi_protocol::EventId::new(event_id),lingxi_protocol::StreamId::new(&sid),lingxi_protocol::Seq::new(next_seq as u64),lingxi_protocol::SessionId::new(&sid),None,None,payload);
            conn.execute("DELETE FROM session_operations WHERE session_id=?1",[&sid]).map_err(map_err)?;
            // 空头和递增版本使旧历史游标失效；不复用已清除的旧分支版本。
            conn.execute("INSERT INTO session_branch_heads(session_id,head_message_id,observed_tail_message_id,revision,head_resolution,updated_at_unix_ms) VALUES(?1,NULL,NULL,1,'persisted_head',?2) ON CONFLICT(session_id) DO UPDATE SET head_message_id=NULL,observed_tail_message_id=NULL,revision=session_branch_heads.revision+1,head_resolution='persisted_head',updated_at_unix_ms=excluded.updated_at_unix_ms",rusqlite::params![sid,now as i64]).map_err(map_err)?;
            Ok((count,event))
        })
    }).await
}

/// 只撤销调用方仍持许可、尚无任何运行或消息的新建行；绝不清理既有会话。
pub async fn remove_unstarted_creation(
    db: &RunDatabase,
    session: &str,
    owner: &str,
) -> Result<(), StorageError> {
    let (sid, owner, busy) = (
        session.to_string(),
        owner.to_string(),
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move |connection| with_write_txn(connection, busy, |connection| {
        let used: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM runs WHERE session_id=?1) OR EXISTS(SELECT 1 FROM messages WHERE session_id=?1)", [&sid], |row| row.get(0)).map_err(map_err)?;
        if used { return Err(StorageError::Conflict { detail: "new session already has durable activity; initialization cleanup refused".into() }); }
        connection.execute("DELETE FROM sessions WHERE session_id=?1 AND owner_user_id=?2", rusqlite::params![sid, owner]).map_err(map_err)?;
        Ok(())
    })).await
}
