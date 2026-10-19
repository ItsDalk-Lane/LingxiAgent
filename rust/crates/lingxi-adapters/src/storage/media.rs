//! 媒体任务的真实回执。提交之前先落库；服务重启后不重复提交外部请求。
use super::{migrations::map_rusqlite, run_store::with_write_txn, RunDatabase};
use lingxi_kernel::ports::StorageError;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MediaPhase {
    Submitting,
    Submitted,
    Generating,
    Downloading,
    Completed,
    Failed,
    Cancelled,
    Unknown,
}
impl MediaPhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Submitting => "submitting",
            Self::Submitted => "submitted",
            Self::Generating => "generating",
            Self::Downloading => "downloading",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
        }
    }
    pub fn terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Unknown
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaJob {
    pub operation_id: String,
    pub session_id: String,
    pub owner_subject: String,
    pub source_run_id: String,
    pub execution_run_id: Option<String>,
    pub tool_call_id: String,
    pub kind: String,
    pub provider: String,
    pub request_digest: String,
    pub spec: serde_json::Value,
    pub state: serde_json::Value,
    pub phase: MediaPhase,
    pub revision: u64,
    pub created_at: u64,
    pub updated_at: u64,
}
#[derive(Debug)]
pub enum MediaClaim {
    New(MediaJob),
    Existing(MediaJob),
}
fn invalid(detail: impl Into<String>) -> StorageError {
    StorageError::InvalidRequest {
        detail: detail.into(),
    }
}
fn conflict(detail: &str) -> StorageError {
    StorageError::Conflict {
        detail: detail.into(),
    }
}
fn integer(value: u64) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| invalid("media integer overflow"))
}
fn read(conn: &rusqlite::Connection, id: &str) -> Result<Option<MediaJob>, StorageError> {
    let text = conn.query_row("SELECT json_object('operation_id',operation_id,'session_id',session_id,'owner_subject',owner_subject,'source_run_id',source_run_id,'execution_run_id',execution_run_id,'tool_call_id',tool_call_id,'kind',kind,'provider',provider,'request_digest',request_digest,'spec',json(spec_json),'state',json(state_json),'phase',phase,'revision',revision,'created_at',created_at,'updated_at',updated_at) FROM media_jobs_v1 WHERE operation_id=?1", [id], |r| r.get::<_,String>(0)).optional().map_err(map_rusqlite)?;
    text.map(|text| {
        serde_json::from_str(&text).map_err(|_| StorageError::Corrupted {
            detail: "media job record is damaged".into(),
        })
    })
    .transpose()
}
pub async fn get(db: &RunDatabase, id: &str) -> Result<Option<MediaJob>, StorageError> {
    let id = id.to_owned();
    db.queue().submit(move |conn| read(conn, &id)).await
}
pub async fn claim(db: &RunDatabase, mut job: MediaJob) -> Result<MediaClaim, StorageError> {
    if job.operation_id.is_empty()
        || job.request_digest.len() != 64
        || !job.request_digest.bytes().all(|b| b.is_ascii_hexdigit())
        || !matches!(job.kind.as_str(), "image" | "video" | "speech")
        || job.phase != MediaPhase::Submitting
        || job.execution_run_id.is_some()
        || job.spec.to_string().len() > 256 * 1024
        || job.state.to_string().len() > 64 * 1024
    {
        return Err(invalid("invalid media claim"));
    }
    job.revision = 1;
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move |conn| with_write_txn(conn,busy,|conn| {
        if let Some(old)=read(conn,&job.operation_id)? {
            if old.session_id!=job.session_id || old.owner_subject!=job.owner_subject || old.source_run_id!=job.source_run_id || old.tool_call_id!=job.tool_call_id || old.request_digest!=job.request_digest || old.provider!=job.provider || old.kind!=job.kind || old.spec!=job.spec { return Err(conflict("media operation is bound to different input")); }
            return Ok(MediaClaim::Existing(old));
        }
        let valid=conn.query_row("SELECT r.run_id FROM runs r JOIN sessions s ON s.session_id=r.session_id WHERE r.run_id=?1 AND r.session_id=?2 AND s.owner_user_id=?3 AND s.lifecycle='active' AND r.status='running'",params![job.source_run_id,job.session_id,job.owner_subject],|r|r.get::<_,String>(0)).optional().map_err(map_rusqlite)?;
        if valid.is_none() { return Err(conflict("media requires an active owned session and running source Run")); }
        conn.execute("INSERT INTO media_jobs_v1(operation_id,session_id,owner_subject,source_run_id,tool_call_id,kind,provider,request_digest,spec_json,state_json,phase,revision,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'submitting',1,?11,?12)",params![job.operation_id,job.session_id,job.owner_subject,job.source_run_id,job.tool_call_id,job.kind,job.provider,job.request_digest,job.spec.to_string(),job.state.to_string(),integer(job.created_at)?,integer(job.updated_at)?]).map_err(map_rusqlite)?;
        super::media_gallery::sync_from_job_txn(conn,&job)?;
        Ok(MediaClaim::New(job.clone()))
    })).await
}
pub async fn transition(
    db: &RunDatabase,
    previous: &MediaJob,
    phase: MediaPhase,
    state: serde_json::Value,
    now: u64,
) -> Result<MediaJob, StorageError> {
    if state.to_string().len() > 64 * 1024 {
        return Err(invalid("media receipt exceeds budget"));
    }
    if previous.phase.terminal() {
        return Err(conflict("media receipt is already terminal"));
    }
    let allowed = matches!(
        (&previous.phase, &phase),
        (
            MediaPhase::Submitting,
            MediaPhase::Submitting
                | MediaPhase::Submitted
                | MediaPhase::Downloading
                | MediaPhase::Failed
                | MediaPhase::Cancelled
                | MediaPhase::Unknown
        ) | (
            MediaPhase::Submitted,
            MediaPhase::Submitted
                | MediaPhase::Generating
                | MediaPhase::Downloading
                | MediaPhase::Failed
                | MediaPhase::Cancelled
                | MediaPhase::Unknown
        ) | (
            MediaPhase::Generating,
            MediaPhase::Generating
                | MediaPhase::Downloading
                | MediaPhase::Failed
                | MediaPhase::Cancelled
                | MediaPhase::Unknown
        ) | (
            MediaPhase::Downloading,
            MediaPhase::Completed
                | MediaPhase::Failed
                | MediaPhase::Cancelled
                | MediaPhase::Unknown
        )
    );
    if !allowed {
        return Err(conflict("invalid media phase transition"));
    }
    let previous = previous.clone();
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move |conn| with_write_txn(conn,busy,|conn| {
        let changed=conn.execute("UPDATE media_jobs_v1 SET phase=?2,state_json=?3,revision=revision+1,updated_at=?4 WHERE operation_id=?1 AND revision=?5 AND phase=?6",params![previous.operation_id,phase.as_str(),state.to_string(),integer(now)?,integer(previous.revision)?,previous.phase.as_str()]).map_err(map_rusqlite)?;
        if changed!=1 { return Err(conflict("media receipt changed concurrently")); }
        let next=read(conn,&previous.operation_id)?.ok_or_else(|| invalid("media receipt disappeared"))?;
        super::media_gallery::sync_from_job_txn(conn,&next)?;
        Ok(next)
    })).await
}
/// 执行权只绑定正式媒体来源的新 Run；旧执行者与并发恢复不能覆盖新回执。
pub async fn bind_execution(
    db: &RunDatabase,
    job: &MediaJob,
    execution_run_id: &str,
    now: u64,
) -> Result<MediaJob, StorageError> {
    if job.phase.terminal() {
        return Err(conflict("terminal media job cannot bind an execution"));
    }
    let job = job.clone();
    let execution_run_id = execution_run_id.to_owned();
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let valid=conn.query_row("SELECT r.run_id FROM runs r JOIN run_lineage l ON l.run_id=r.run_id JOIN sessions s ON s.session_id=r.session_id JOIN runs p ON p.run_id=?4 WHERE r.run_id=?1 AND r.session_id=?2 AND s.owner_user_id=?3 AND r.owner_subject=p.owner_subject AND r.owner_kind=p.owner_kind AND s.lifecycle='active' AND r.status='running' AND l.origin='media' AND l.parent_run_id=?4 AND l.cause_id=?5 AND l.source_message_id=?6",params![execution_run_id,job.session_id,job.owner_subject,job.source_run_id,job.operation_id,job.tool_call_id],|r|r.get::<_,String>(0)).optional().map_err(map_rusqlite)?;
        if valid.is_none(){return Err(conflict("media execution has no matching actual authorized Run"));}
        let n=conn.execute("UPDATE media_jobs_v1 SET execution_run_id=?2,revision=revision+1,updated_at=?3 WHERE operation_id=?1 AND revision=?4 AND execution_run_id IS ?5 AND phase=?6",params![job.operation_id,execution_run_id,integer(now)?,integer(job.revision)?,job.execution_run_id,job.phase.as_str()]).map_err(map_rusqlite)?;
        if n!=1{return Err(conflict("media execution changed concurrently"));}
        read(conn,&job.operation_id)?.ok_or_else(||invalid("media execution receipt disappeared"))
    })).await
}
pub async fn list_open(db: &RunDatabase) -> Result<Vec<MediaJob>, StorageError> {
    db.queue().submit(move |conn| {
        let ids={ let mut stmt=conn.prepare("SELECT operation_id FROM media_jobs_v1 WHERE phase IN('submitting','submitted','generating','downloading') ORDER BY created_at,operation_id LIMIT 1025").map_err(map_rusqlite)?; let rows=stmt.query_map([],|r|r.get::<_,String>(0)).map_err(map_rusqlite)?; rows.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)? };
        if ids.len()>1024{return Err(invalid("open media jobs exceed the explicit recovery budget"));}
        ids.iter().map(|id| read(conn,id)?.ok_or_else(|| invalid("media receipt disappeared"))).collect()
    }).await
}
pub async fn list_session(
    db: &RunDatabase,
    sid: &str,
    owner: &str,
) -> Result<Vec<MediaJob>, StorageError> {
    let sid = sid.to_owned();
    let owner = owner.to_owned();
    db.queue().submit(move|conn|{
        let ids={let mut stmt=conn.prepare("SELECT operation_id FROM media_jobs_v1 WHERE session_id=?1 AND owner_subject=?2 ORDER BY created_at DESC,operation_id LIMIT 1001").map_err(map_rusqlite)?;let rows=stmt.query_map(params![sid,owner],|r|r.get::<_,String>(0)).map_err(map_rusqlite)?;rows.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)?};
        if ids.len()>1000{return Err(invalid("media session history exceeds the explicit response budget"));}
        ids.iter().map(|id|read(conn,id)?.ok_or_else(||invalid("media job disappeared"))).collect()
    }).await
}

/// 发布许可随真实写闭包存活；权限写链须先取得同一门的独占租约。
pub struct MediaPublicationGuard {
    pub job: MediaJob,
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub execution_cancelled: std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
    pub authority_lease: tokio::sync::OwnedRwLockReadGuard<()>,
    pub artifact_index: u32,
}
pub(crate) fn verify_publication(
    conn: &rusqlite::Connection,
    guard: &MediaPublicationGuard,
) -> Result<MediaJob, StorageError> {
    if guard.cancel.load(std::sync::atomic::Ordering::Acquire)
        || (guard.execution_cancelled)()
        || guard.artifact_index >= 10
    {
        return Err(conflict(
            "media publication cancelled or artifact budget exceeded",
        ));
    }
    let job = read(conn, &guard.job.operation_id)?
        .ok_or_else(|| conflict("media publication job missing"))?;
    if job.revision != guard.job.revision
        || job.phase != MediaPhase::Downloading
        || job.execution_run_id != guard.job.execution_run_id
        || job.owner_subject != guard.job.owner_subject
        || job.session_id != guard.job.session_id
    {
        return Err(conflict("media publication authority changed"));
    }
    let execution = job
        .execution_run_id
        .as_deref()
        .ok_or_else(|| conflict("media publication execution missing"))?;
    let identity:Option<(String,String)>=conn.query_row("SELECT p.owner_kind,p.owner_subject FROM media_jobs_v1 j JOIN runs r ON r.run_id=j.execution_run_id JOIN runs p ON p.run_id=j.source_run_id JOIN run_lineage l ON l.run_id=r.run_id JOIN sessions s ON s.session_id=j.session_id WHERE j.operation_id=?1 AND r.run_id=?2 AND r.status='running' AND r.session_id=j.session_id AND r.owner_kind=p.owner_kind AND r.owner_subject=p.owner_subject AND s.owner_user_id=j.owner_subject AND s.lifecycle='active' AND COALESCE(s.permission_mode,'operate')!='read_only' AND l.origin='media' AND l.parent_run_id=j.source_run_id AND l.cause_id=j.operation_id AND l.source_message_id=j.tool_call_id",params![job.operation_id,execution],|row|Ok((row.get(0)?,row.get(1)?))).optional().map_err(map_rusqlite)?;
    let (kind, subject) = identity.ok_or_else(|| {
        conflict("media publication requires actual running Media Run and active session")
    })?;
    let principal = lingxi_kernel::recovery::principal_from_storage_facts(&kind, &subject)?;
    if !principal.can_access_session(&job.owner_subject, &job.session_id) {
        return Err(conflict("media publication source principal changed"));
    }
    if principal.bridge_actor_subject().is_some() {
        verify_bridge_publication(conn, &job, &principal)?;
    }
    Ok(job)
}
/// 桥来源血缘行走的行形状（origin/parent/cause/message）。
type LineageRow = (String, Option<String>, Option<String>, Option<String>);
fn verify_bridge_publication(
    conn: &rusqlite::Connection,
    job: &MediaJob,
    principal: &lingxi_kernel::Principal,
) -> Result<(), StorageError> {
    use lingxi_protocol::{BridgeConversationKind, BridgeRole};
    let mut current = job.source_run_id.clone();
    let mut seen = std::collections::BTreeSet::new();
    loop {
        if seen.len() >= 64 || !seen.insert(current.clone()) {
            return Err(conflict("media bridge source cycle or budget exceeded"));
        }
        let row: Option<LineageRow> = conn.query_row("SELECT l.origin,l.parent_run_id,l.cause_id,l.source_message_id FROM runs r JOIN run_lineage l ON l.run_id=r.run_id WHERE r.run_id=?1 AND r.session_id=?2 AND r.owner_kind=?3 AND r.owner_subject=?4 AND r.status NOT IN('cancelled','failed','interrupted_needs_attention')",params![current,job.session_id,principal.storage_kind(),principal.storage_subject()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).optional().map_err(map_rusqlite)?;
        let (origin, parent, cause, message) =
            row.ok_or_else(|| conflict("media bridge source identity changed"))?;
        if origin == "subagent" {
            current = parent.ok_or_else(|| conflict("media bridge source parent missing"))?;
            continue;
        }
        if origin != "bridge" || parent.is_some() {
            return Err(conflict("media bridge source lineage invalid"));
        }
        let inbox = super::bridge::inbox_conn(
            conn,
            &cause.ok_or_else(|| conflict("media bridge source cause missing"))?,
        )?
        .ok_or_else(|| conflict("media bridge inbox missing"))?;
        let connector = super::bridge::connector_conn(conn, &inbox.input.connector_id)?
            .ok_or_else(|| conflict("media bridge connector missing"))?;
        let actor = lingxi_protocol::bridge_subject(
            &inbox.input.platform,
            &inbox.input.connector_id,
            &inbox.input.sender_id,
        )
        .map_err(invalid)?;
        let expected = if inbox.prepared_takeover {
            lingxi_kernel::Principal::BridgeTakeover {
                subject: actor,
                session_id: job.session_id.clone(),
            }
        } else {
            lingxi_kernel::Principal::Bridge { subject: actor }
        };
        let role = if inbox.input.conversation_kind == BridgeConversationKind::Dm
            && connector.input.owner_sender_id.as_deref() == Some(inbox.input.sender_id.as_str())
        {
            BridgeRole::Owner
        } else {
            BridgeRole::Guest
        };
        if !inbox.prepared
            || inbox.role != BridgeRole::Owner
            || role != inbox.role
            || expected != *principal
            || inbox.session_id.as_deref() != Some(job.session_id.as_str())
            || message.as_deref() != Some(inbox.input.message_id.as_str())
            || !matches!(
                inbox.status,
                super::bridge::BridgeInboxStatus::Claimed
                    | super::bridge::BridgeInboxStatus::Running
                    | super::bridge::BridgeInboxStatus::Succeeded
            )
            || !connector.input.enabled
            || connector.generation != inbox.connector_generation
            || connector.input.actor_id != inbox.actor_id
            || connector.input.platform != inbox.input.platform
            || connector.input.owner_subject != inbox.owner_subject
        {
            return Err(conflict("media bridge publication revoked"));
        }
        return Ok(());
    }
}
pub(crate) fn commit_publication(
    conn: &rusqlite::Connection,
    guard: &MediaPublicationGuard,
    file: &super::session_files::SessionFileRow,
) -> Result<(), StorageError> {
    let job = verify_publication(conn, guard)?;
    if file.owner_session_id != job.session_id {
        return Err(conflict("media file owner mismatch"));
    }
    let mut state = job.state.clone();
    if state.get("files").is_none() {
        state["files"] = serde_json::json!([]);
    }
    let files = state["files"]
        .as_array_mut()
        .ok_or_else(|| invalid("media files receipt damaged"))?;
    if files.iter().any(|entry| entry["fileId"] == file.file_id) {
        return Ok(());
    }
    if files.len() >= 10 {
        return Err(conflict("media publication file budget exceeded"));
    }
    files.push(serde_json::json!({"fileId":file.file_id,"filename":file.filename,"mime":file.mime,"sizeBytes":file.size_bytes}));
    state["remoteOutcome"] = serde_json::json!("completed");
    let changed=conn.execute("UPDATE media_jobs_v1 SET state_json=?2,revision=revision+1,updated_at=?3 WHERE operation_id=?1 AND revision=?4 AND execution_run_id IS ?5 AND phase='downloading'",params![job.operation_id,state.to_string(),integer(file.updated_at_ms)?,integer(job.revision)?,job.execution_run_id]).map_err(map_rusqlite)?;
    if changed != 1 {
        return Err(conflict("media publication receipt changed"));
    }
    Ok(())
}

/// 清理不受已结束历史的展示分页限制，只扫描本会话尚有执行事实的作业。
pub async fn list_session_unsettled(
    db: &RunDatabase,
    sid: &str,
    owner: &str,
) -> Result<Vec<MediaJob>, StorageError> {
    let (sid, owner) = (sid.to_owned(), owner.to_owned());
    db.queue().submit(move|conn|{
        let ids={let mut statement=conn.prepare("SELECT j.operation_id FROM media_jobs_v1 j LEFT JOIN runs r ON r.run_id=j.execution_run_id WHERE j.session_id=?1 AND j.owner_subject=?2 AND (j.phase IN('submitting','submitted','generating','downloading') OR r.status NOT IN('completed','failed','cancelled','interrupted_needs_attention')) ORDER BY j.created_at,j.operation_id LIMIT 1025").map_err(map_rusqlite)?;let rows=statement.query_map(params![sid,owner],|row|row.get::<_,String>(0)).map_err(map_rusqlite)?;rows.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)?};
        if ids.len()>1024{return Err(invalid("media session cleanup exceeds explicit unsettled budget"));}
        ids.iter().map(|id|read(conn,id)?.ok_or_else(||invalid("media cleanup job disappeared"))).collect()
    }).await
}
