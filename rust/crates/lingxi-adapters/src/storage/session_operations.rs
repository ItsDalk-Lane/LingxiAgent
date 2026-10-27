//! 会话业务操作收据。prepared代表可能发生文件副作用，重启后绝不自动重做。
use super::{migrations::map_rusqlite as err, run_store::with_write_txn, RunDatabase};
use lingxi_kernel::ports::StorageError;
use rusqlite::{Connection, OptionalExtension};
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct Snapshot {
    pub owner: String,
    pub lifecycle: String,
    pub permission: Option<String>,
    pub folders: String,
    pub revision: i64,
}
#[derive(Debug, Clone)]
pub struct Operation {
    pub id: String,
    pub snapshot: Snapshot,
}
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationView {
    pub operation_id: String,
    pub operation_kind: String,
    pub state: String,
    pub file_effects: bool,
    pub receipt: Option<serde_json::Value>,
}
#[derive(Debug)]
pub enum Begin {
    Fresh(Operation),
    Replay(serde_json::Value),
}
pub struct Prepare<'a> {
    pub session_id: &'a str,
    pub owner_kind: &'a str,
    pub owner_subject: &'a str,
    pub operation_id: &'a str,
    pub request_id: Option<&'a str>,
    pub kind: &'a str,
    pub digest: &'a str,
    pub file_effects: bool,
    pub now: u64,
}
/// 删除后的真实操作身份仍由收据拥有；不能把它重新分配给新会话。
pub(crate) fn identity_retired(conn: &Connection, sid: &str) -> Result<bool, StorageError> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM session_operations WHERE session_id=?1)",
        [sid],
        |r| r.get(0),
    )
    .map_err(err)
}
pub(crate) fn assert_fresh_identity(conn: &Connection, sid: &str) -> Result<(), StorageError> {
    if identity_retired(conn, sid)? {
        return Err(StorageError::Conflict {
            detail: format!("session_identity_retired:{sid}"),
        });
    }
    Ok(())
}
pub fn snapshot(conn: &Connection, sid: &str) -> Result<Snapshot, StorageError> {
    conn.query_row("SELECT owner_user_id,lifecycle,permission_mode,authorized_folders_json,COALESCE((SELECT revision FROM session_branch_heads WHERE session_id=sessions.session_id),0) FROM sessions WHERE session_id=?1",[sid],|r|Ok(Snapshot{owner:r.get(0)?,lifecycle:r.get(1)?,permission:r.get(2)?,folders:r.get(3)?,revision:r.get(4)?})).map_err(err)
}
pub fn verify(conn: &Connection, sid: &str, expected: &Snapshot) -> Result<(), StorageError> {
    if &snapshot(conn, sid)? != expected {
        return Err(StorageError::Conflict {
            detail: "session_snapshot_changed".into(),
        });
    }
    Ok(())
}
pub async fn begin(db: &RunDatabase, p: Prepare<'_>) -> Result<Begin, StorageError> {
    let (sid, ok, os, id, rid, kind, digest) = (
        p.session_id.to_string(),
        p.owner_kind.to_string(),
        p.owner_subject.to_string(),
        p.operation_id.to_string(),
        p.request_id.map(str::to_string),
        p.kind.to_string(),
        p.digest.to_string(),
    );
    let effects = p.file_effects;
    let now = p.now as i64;
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move |conn| {
        with_write_txn(conn, busy, |conn| {
            if let Some(rid) = rid.as_ref() {
                let prior: Option<(String, String, String, Option<String>)> = conn.query_row(
                    "SELECT operation_kind,input_digest,state,receipt_json FROM session_operations WHERE session_id=?1 AND owner_kind=?2 AND owner_subject=?3 AND request_id=?4",
                    rusqlite::params![sid, ok, os, rid],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                ).optional().map_err(err)?;
                if let Some((prior_kind, prior_digest, state, receipt)) = prior {
                    if prior_kind != kind || prior_digest != digest {
                        return Err(StorageError::Conflict { detail: "session_request_id_conflict".into() });
                    }
                    if state != "completed" {
                        return Err(StorageError::Conflict { detail: "session_operation_unconfirmed: query operation status; do not replay unknown file effects".into() });
                    }
                    let receipt = receipt.ok_or(StorageError::Corrupted { detail: "completed session operation has no receipt".into() })?;
                    return serde_json::from_str(&receipt).map(Begin::Replay).map_err(|e| StorageError::Corrupted { detail: e.to_string() });
                }
                // 与原execute幂等键共用同一命名空间，兼容旧行的规范化。
                let mut stmt = conn.prepare("SELECT l.cause_id FROM run_lineage l JOIN runs r ON r.run_id=l.run_id WHERE r.session_id=?1 AND r.owner_kind=?2 AND r.owner_subject=?3 AND l.origin='user'").map_err(err)?;
                let rows = stmt.query_map(rusqlite::params![sid, ok, os], |r| r.get::<_, Option<String>>(0)).map_err(err)?;
                for row in rows {
                    if row.map_err(err)?.as_deref().and_then(|c| c.strip_prefix(lingxi_kernel::subagent::REQUEST_CAUSE_ID_PREFIX)).map(lingxi_kernel::subagent::canonical_request_id) == Some(rid.as_str()) {
                        return Err(StorageError::Conflict { detail: "session_request_id_conflict".into() });
                    }
                }
            }
            let snapshot = snapshot(conn, &sid)?;
            if snapshot.lifecycle != "active" {
                return Err(StorageError::Conflict { detail: "session_not_active".into() });
            }
            let pending: i64 = conn.query_row("SELECT COUNT(*) FROM session_operations WHERE session_id=?1 AND state='prepared' AND file_effects=1", [&sid], |r| r.get(0)).map_err(err)?;
            if pending > 0 {
                return Err(StorageError::Conflict { detail: "session_file_operation_unconfirmed".into() });
            }
            let snap = serde_json::to_string(&snapshot).map_err(|e| StorageError::Internal { detail: e.to_string() })?;
            conn.execute("INSERT INTO session_operations(operation_id,session_id,owner_kind,owner_subject,request_id,operation_kind,input_digest,state,file_effects,snapshot_json,created_at_unix_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,'prepared',?8,?9,?10)", rusqlite::params![id,sid,ok,os,rid,kind,digest,effects as i64,snap,now]).map_err(err)?;
            Ok(Begin::Fresh(Operation { id: id.clone(), snapshot }))
        })
    }).await
}

pub fn complete(
    conn: &Connection,
    op: &Operation,
    receipt: &serde_json::Value,
    now: i64,
) -> Result<(), StorageError> {
    let updated=conn.execute("UPDATE session_operations SET state='completed',receipt_json=?2,completed_at_unix_ms=?3 WHERE operation_id=?1 AND state='prepared'",rusqlite::params![op.id,receipt.to_string(),now]).map_err(err)?;
    if updated != 1 {
        return Err(StorageError::Conflict {
            detail: "session_operation_receipt_conflict".into(),
        });
    }
    Ok(())
}
pub async fn list(
    db: &RunDatabase,
    sid: &str,
    owner_kind: &str,
    owner_subject: &str,
) -> Result<Vec<OperationView>, StorageError> {
    let (sid, ok, os) = (
        sid.to_string(),
        owner_kind.to_string(),
        owner_subject.to_string(),
    );
    db.queue().submit(move|conn|{let mut stmt=conn.prepare("SELECT operation_id,operation_kind,state,file_effects,receipt_json FROM session_operations WHERE session_id=?1 AND owner_kind=?2 AND owner_subject=?3 ORDER BY created_at_unix_ms,operation_id").map_err(err)?;let rows=stmt.query_map(rusqlite::params![sid,ok,os],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,bool>(3)?,r.get::<_,Option<String>>(4)?))).map_err(err)?;let mut out=Vec::new();for row in rows {let(id,kind,state,effects,receipt)=row.map_err(err)?;out.push(OperationView{operation_id:id,operation_kind:kind,state,file_effects:effects,receipt:receipt.map(|s|serde_json::from_str(&s)).transpose().map_err(|e|StorageError::Corrupted{detail:e.to_string()})?});}Ok(out)}).await
}
pub async fn assert_executable(db: &RunDatabase, sid: &str) -> Result<(), StorageError> {
    let sid = sid.to_string();
    db.queue().submit(move|conn|{let s=snapshot(conn,&sid)?;if s.lifecycle!="active"{return Err(StorageError::Conflict{detail:"session_not_active".into()});}let pending:i64=conn.query_row("SELECT COUNT(*) FROM session_operations WHERE session_id=?1 AND state='prepared' AND file_effects=1",[sid],|r|r.get(0)).map_err(err)?;if pending>0{return Err(StorageError::Conflict{detail:"session_file_operation_unconfirmed".into()});}Ok(())}).await
}
/// 写文件之前持久记录不确定边界，不能仅在操作失败后补记。
pub async fn arm_files(db: &RunDatabase, sid: &str, op: &Operation) -> Result<(), StorageError> {
    let (sid, op) = (sid.to_string(), op.clone());
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{verify(conn,&sid,&op.snapshot)?;let n=conn.execute("UPDATE session_operations SET file_effects=1 WHERE operation_id=?1 AND state='prepared'",[&op.id]).map_err(err)?;if n!=1{return Err(StorageError::Conflict{detail:"session_operation_not_prepared".into()});}Ok(())})).await
}

/// 已取得会话操作权后读取持久权限，未知词汇响亮拒绝。
pub async fn permission(
    db: &RunDatabase,
    sid: &str,
) -> Result<Option<lingxi_kernel::subagent::SessionPermissionMode>, StorageError> {
    let sid = sid.to_string();
    db.queue()
        .submit(move |conn| {
            let Some(raw) = snapshot(conn, &sid)?.permission else {
                return Ok(None);
            };
            lingxi_kernel::subagent::SessionPermissionMode::parse(&raw)
                .map(Some)
                .ok_or(StorageError::Corrupted {
                    detail: format!("unknown session permission mode: {raw}"),
                })
        })
        .await
}
pub async fn set_permission(
    db: &RunDatabase,
    sid: &str,
    mode: lingxi_kernel::subagent::SessionPermissionMode,
) -> Result<(), StorageError> {
    let sid = sid.to_string();
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                if snapshot(conn, &sid)?.lifecycle != "active" {
                    return Err(StorageError::Conflict {
                        detail: "session_not_active".into(),
                    });
                }
                conn.execute(
                    "UPDATE sessions SET permission_mode=?2 WHERE session_id=?1",
                    rusqlite::params![sid, mode.wire_name()],
                )
                .map_err(err)?;
                Ok(())
            })
        })
        .await
}
