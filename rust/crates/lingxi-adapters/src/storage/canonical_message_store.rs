//! CanonicalMessageStore — R06-T04 交付物①：统一消息历史的权威读取。
//!
//! 职责（任务书 R06-T04 步骤③）：
//! - 分支链从索引/游标定位，逐页回溯**不每页完整扫描全部历史**：
//!   每步是一次主键/索引查找（parent_message_id 链接；legacy 区域回退
//!   `idx_messages_session` 上的 seq 前驱），单页成本 O(页大小)，与历史
//!   总长无关。`PageStats` 是这一纪律的 I/O 探针（不上线）。
//! - LegacyReader（步骤①④）：v8 迁移前的行 parent 全 NULL（pre-T03 线性
//!   写序），回退按 **committed seq** 线性化——存储事实，不按时间猜。
//!   根重置标记（parent NULL 且 entry_type 为标记类）是分支清空语义：
//!   walk 到标记即停，绝不用 seq 回退穿透重置点。
//! - 游标诚实：before 必须引用本会话真实存在的消息且 seq 吻合，
//!   否则 `InvalidRequest`（service 映射 400），绝不静默当作首页。
//!
//! 本模块只读既有表（messages / key_events / run_lineage /
//! session_branch_heads），不改任何写路径（T03 语义零回退）。

use lingxi_kernel::ports::StorageError;
use lingxi_protocol::history::HistoryCursor;
use lingxi_protocol::EventPayload;
use rusqlite::OptionalExtension;

use super::migrations;
use super::run_store::RunDatabase;
use super::session_tree::MessageRow;

fn map_err(err: rusqlite::Error) -> StorageError {
    migrations::map_rusqlite(err)
}

/// 单页 I/O 探针（A08 证据面；仅供测试与台账，不上线）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PageStats {
    /// 实际检查的行数（每次查找命中计 1）。
    pub rows_examined: u64,
    /// 发起的查询次数。
    pub queries: u64,
}

/// 一页分支链消息（链序：旧→新）。
#[derive(Debug, Clone)]
pub struct BranchPage {
    pub messages: Vec<MessageRow>,
    /// 分支头 revision（ETag 材料；无头行的 legacy 会话为 0）。
    pub head_revision: u64,
    pub has_more: bool,
    /// 下一页的 before 游标（本页最老消息；has_more=false 时 None）。
    pub next_before: Option<HistoryCursor>,
    pub stats: PageStats,
}

/// 一条持久事件（投影输入；payload 已解码为类型化 EventPayload，
/// 未知类型经 EventPayload::Unknown 原样保留）。
#[derive(Debug, Clone)]
pub struct StoredEventRow {
    pub seq: u64,
    pub run_id: Option<String>,
    pub event_id: String,
    pub payload: EventPayload,
}

const MESSAGE_COLS: &str = "message_id, parent_message_id, run_id, role, content_json, \
     entry_type, seq, model_call_id, committed_at_unix_ms";

fn read_message(
    conn: &rusqlite::Connection,
    session_id: &str,
    message_id: &str,
) -> Result<Option<MessageRow>, StorageError> {
    conn.query_row(
        &format!("SELECT {MESSAGE_COLS} FROM messages WHERE session_id=?1 AND message_id=?2"),
        rusqlite::params![session_id, message_id],
        |r| {
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
        },
    )
    .optional()
    .map_err(map_err)
}

/// 分支链一页（A08：索引定位 + 游标回溯，绝不全扫）。
///
/// - `before=None`：从当前分支头回溯（头=session_branch_heads；无头行回退
///   legacy_tail：seq 最大的 entry_type='message' 行，与 T03
///   branch_id_chain 同一规则）。
/// - `before=Some(cursor)`：游标消息必须属于本会话且 seq 吻合
///   （否则 InvalidRequest），页内容为**严格更老**的消息窗口。
pub async fn read_branch_page(
    db: &RunDatabase,
    session_id: &str,
    before: Option<HistoryCursor>,
    limit: u32,
) -> Result<BranchPage, StorageError> {
    let sid = session_id.to_string();
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    db.queue()
        .submit(move |conn| {
            let mut stats = PageStats::default();
            // 1) 分支头 revision（ETag 材料）与 walk 起点。
            let head_row: Option<(Option<String>, i64)> = conn
                .query_row(
                    "SELECT head_message_id, revision FROM session_branch_heads \
                     WHERE session_id=?1",
                    [&sid],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(map_err)?;
            stats.queries += 1;
            stats.rows_examined += u64::from(head_row.is_some());
            let (head_id, head_revision) = match head_row {
                Some((head, revision)) => (head, revision as u64),
                None => (None, 0),
            };

            // walk 起点（含/不含语义：头部页含 head 本身；before 页从游标
            // 的前驱开始，游标消息属于上一页）。
            let start: Option<MessageRow> = match &before {
                None => {
                    let head_id: Option<String> = match head_id {
                        Some(h) => Some(h),
                        None => {
                            // legacy_tail：seq 最大的真实 message（跳过标记）。
                            let t: Option<String> = conn
                                .query_row(
                                    "SELECT message_id FROM messages WHERE session_id=?1 \
                                     AND entry_type='message' ORDER BY seq DESC LIMIT 1",
                                    [&sid],
                                    |r| r.get(0),
                                )
                                .optional()
                                .map_err(map_err)?;
                            stats.queries += 1;
                            stats.rows_examined += u64::from(t.is_some());
                            t
                        }
                    };
                    match head_id {
                        Some(id) => {
                            let row = read_message(conn, &sid, &id)?;
                            stats.queries += 1;
                            stats.rows_examined += u64::from(row.is_some());
                            row
                        }
                        None => None,
                    }
                }
                Some(cursor) => {
                    let row = read_message(conn, &sid, &cursor.message_id)?;
                    stats.queries += 1;
                    stats.rows_examined += u64::from(row.is_some());
                    let row = row.ok_or_else(|| StorageError::InvalidRequest {
                        detail: format!(
                            "history cursor references unknown message '{}' of session {sid}",
                            cursor.message_id
                        ),
                    })?;
                    if row.seq < 0 || row.seq as u64 != cursor.seq {
                        return Err(StorageError::InvalidRequest {
                            detail: format!(
                                "history cursor seq {} does not match stored seq {} of '{}'",
                                cursor.seq, row.seq, cursor.message_id
                            ),
                        });
                    }
                    // before 语义：严格更老 → 从前驱起步。
                    predecessor(conn, &sid, &row, &mut stats)?
                }
            };

            // 2) 回溯 walk：取 limit+1 探测 has_more。
            let mut walked: Vec<MessageRow> = Vec::new();
            let mut current = start;
            while let Some(row) = current {
                let is_marker = row.entry_type != "message";
                walked.push(row);
                if walked.len() > limit {
                    break;
                }
                let last = walked.last().expect("just pushed");
                if is_marker && last.parent_message_id.is_none() {
                    // 根重置标记：分支清空边界，walk 停止（绝不 seq 穿透）。
                    break;
                }
                current = predecessor(conn, &sid, last, &mut stats)?;
            }
            let has_more = walked.len() > limit;
            walked.truncate(limit);
            // 链序（旧→新）。
            walked.reverse();
            let next_before = if has_more {
                walked.first().map(|m| HistoryCursor {
                    message_id: m.message_id.clone(),
                    seq: m.seq as u64,
                })
            } else {
                None
            };
            Ok(BranchPage {
                messages: walked,
                head_revision,
                has_more,
                next_before,
                stats,
            })
        })
        .await
}

/// 前驱解析：parent 链接优先；parent NULL 且为真实 message → legacy
/// 线性回退（seq 前驱，idx_messages_session 索引查找）；标记类 NULL
/// parent 由调用方先行拦截（根重置边界）。
fn predecessor(
    conn: &rusqlite::Connection,
    session_id: &str,
    row: &MessageRow,
    stats: &mut PageStats,
) -> Result<Option<MessageRow>, StorageError> {
    if let Some(parent) = &row.parent_message_id {
        let found = read_message(conn, session_id, parent)?;
        stats.queries += 1;
        stats.rows_examined += u64::from(found.is_some());
        return match found {
            Some(p) => Ok(Some(p)),
            None => Err(StorageError::Corrupted {
                detail: format!(
                    "message '{}' of session {session_id} references missing parent '{parent}'",
                    row.message_id
                ),
            }),
        };
    }
    if row.entry_type == "message" {
        // LegacyReader 回退：pre-T03 线性写序以 committed seq 为准。
        let pred_id: Option<String> = conn
            .query_row(
                "SELECT message_id FROM messages WHERE session_id=?1 \
                 AND entry_type='message' AND seq<?2 ORDER BY seq DESC LIMIT 1",
                rusqlite::params![session_id, row.seq],
                |r| r.get(0),
            )
            .optional()
            .map_err(map_err)?;
        stats.queries += 1;
        stats.rows_examined += u64::from(pred_id.is_some());
        match pred_id {
            Some(id) => {
                let found = read_message(conn, session_id, &id)?;
                stats.queries += 1;
                stats.rows_examined += u64::from(found.is_some());
                Ok(found)
            }
            None => Ok(None),
        }
    } else {
        Ok(None)
    }
}

/// 一组 run 在本会话流上的全部事件（seq 升序）。
/// `idx_key_events_run`/`idx_key_events_session` 索引查找，与页大小同阶。
pub async fn read_branch_events(
    db: &RunDatabase,
    session_id: &str,
    run_ids: &[String],
) -> Result<Vec<StoredEventRow>, StorageError> {
    let sid = session_id.to_string();
    let run_ids = run_ids.to_vec();
    db.queue()
        .submit(move |conn| {
            let mut out = Vec::new();
            for run_id in &run_ids {
                let mut stmt = conn
                    .prepare(
                        "SELECT seq, run_id, event_id, payload_json FROM key_events \
                         WHERE session_id=?1 AND run_id=?2 ORDER BY seq",
                    )
                    .map_err(map_err)?;
                let rows = stmt
                    .query_map(rusqlite::params![sid, run_id], |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, Option<String>>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                        ))
                    })
                    .map_err(map_err)?;
                for row in rows {
                    let (seq, run_id, event_id, payload_json) = row.map_err(map_err)?;
                    let payload: EventPayload =
                        serde_json::from_str(&payload_json).map_err(|err| {
                            StorageError::Corrupted {
                                detail: format!(
                                    "key_events payload for {event_id} is not valid JSON: {err}"
                                ),
                            }
                        })?;
                    out.push(StoredEventRow {
                        seq: seq as u64,
                        run_id,
                        event_id,
                        payload,
                    });
                }
            }
            out.sort_by_key(|e| e.seq);
            Ok(out)
        })
        .await
}

/// lineage 反查：给定 run 集合的（父→子）边，**内部迭代到传递闭包**
/// （visited 集合防环：环不出新成员即停）。调用方一次调用拿到闭包。
pub async fn read_lineage_children(
    db: &RunDatabase,
    run_ids: &[String],
) -> Result<Vec<(String, String)>, StorageError> {
    let run_ids = run_ids.to_vec();
    db.queue()
        .submit(move |conn| {
            let mut edges: Vec<(String, String)> = Vec::new();
            let mut visited: std::collections::HashSet<String> = run_ids.iter().cloned().collect();
            let mut frontier = run_ids;
            while !frontier.is_empty() {
                let mut next = Vec::new();
                for run_id in &frontier {
                    let mut stmt = conn
                        .prepare(
                            "SELECT parent_run_id, run_id FROM run_lineage WHERE parent_run_id=?1",
                        )
                        .map_err(map_err)?;
                    let rows = stmt
                        .query_map([run_id], |r| {
                            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                        })
                        .map_err(map_err)?;
                    for row in rows {
                        let edge = row.map_err(map_err)?;
                        if visited.insert(edge.1.clone()) {
                            next.push(edge.1.clone());
                        }
                        edges.push(edge);
                    }
                }
                frontier = next;
            }
            Ok(edges)
        })
        .await
}

/// 本会话实存的消息 id 子集（批量主键探测；投影的跨页锚定判定用——
/// 每 run 一次主键查找，与页大小同阶，绝不全扫 messages）。
pub async fn read_existing_message_ids(
    db: &RunDatabase,
    session_id: &str,
    message_ids: &[String],
) -> Result<Vec<String>, StorageError> {
    let sid = session_id.to_string();
    let message_ids = message_ids.to_vec();
    db.queue()
        .submit(move |conn| {
            let mut out = Vec::new();
            for message_id in &message_ids {
                let exists: Option<String> = conn
                    .query_row(
                        "SELECT message_id FROM messages WHERE session_id=?1 AND message_id=?2",
                        rusqlite::params![sid, message_id],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(map_err)?;
                if let Some(id) = exists {
                    out.push(id);
                }
            }
            Ok(out)
        })
        .await
}

/// 分支头 revision 的常数读（ETag 条件请求用；无头行的 legacy 会话为 0）。
pub async fn read_branch_head_revision(
    db: &RunDatabase,
    session_id: &str,
) -> Result<u64, StorageError> {
    let sid = session_id.to_string();
    db.queue()
        .submit(move |conn| {
            let revision: Option<i64> = conn
                .query_row(
                    "SELECT revision FROM session_branch_heads WHERE session_id=?1",
                    [&sid],
                    |r| r.get(0),
                )
                .optional()
                .map_err(map_err)?;
            Ok(revision.unwrap_or(0) as u64)
        })
        .await
}

/// 本会话实存的 run 子集（run 关联 known/unknown 判定的确定性来源之一：
/// id 约定命中的 run 必须实存于本会话才算 known）。
pub async fn read_existing_run_ids(
    db: &RunDatabase,
    session_id: &str,
    run_ids: &[String],
) -> Result<Vec<String>, StorageError> {
    let sid = session_id.to_string();
    let run_ids = run_ids.to_vec();
    db.queue()
        .submit(move |conn| {
            let mut out = Vec::new();
            for run_id in &run_ids {
                let exists: Option<String> = conn
                    .query_row(
                        "SELECT run_id FROM runs WHERE session_id=?1 AND run_id=?2",
                        rusqlite::params![sid, run_id],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(map_err)?;
                if let Some(id) = exists {
                    out.push(id);
                }
            }
            Ok(out)
        })
        .await
}

/// 全分支读取 = 循环 read_branch_page 到底（导出与分页遍历共用同一分页
/// 路径，证明分页遍历不丢不重）。返回的消息按**全局链序（旧→新）**：
/// 分页器最新页先出，这里逐页前插拼成完整转录序——等价于客户端
/// 「向上翻页时前插」的装配方式，与页大小无关。
pub async fn read_full_branch(
    db: &RunDatabase,
    session_id: &str,
    page_limit: u32,
) -> Result<(Vec<MessageRow>, u64), StorageError> {
    let mut all: Vec<MessageRow> = Vec::new();
    let mut before: Option<HistoryCursor> = None;
    loop {
        let page = read_branch_page(db, session_id, before.clone(), page_limit).await?;
        let head_revision = page.head_revision;
        // 前插：本页（更老）排在已收集（更新）之前。
        let mut messages = page.messages;
        messages.append(&mut all);
        all = messages;
        if !page.has_more {
            return Ok((all, head_revision));
        }
        before = page.next_before;
    }
}
