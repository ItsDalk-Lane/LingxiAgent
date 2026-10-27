//! 会话协作只保存真实来源、确认队列和受理事实；未知投递不自动重做。
use super::{migrations::map_rusqlite, run_store::with_write_txn, RunDatabase};
use lingxi_kernel::{ports::StorageError, LOCAL_OWNER_SUBJECT};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const DECISION_ENTRY_TYPE: &str = "hana-session-collab-decision";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollabDraftKind {
    Send,
    Create,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollabDraftState {
    Pending,
    Applying,
    Applied,
    Rejected,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollabDeliveryStatus {
    None,
    Queued,
    Started,
    Accepted,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollabDraftInput {
    pub id: String,
    pub studio_id: String,
    pub owner_subject: String,
    pub source_session_id: String,
    pub source_run_id: String,
    pub source_tool_call_id: String,
    pub kind: CollabDraftKind,
    pub draft_json: String,
    pub expires_at: i64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollabDraft {
    pub input: CollabDraftInput,
    pub state: CollabDraftState,
    pub delivery_status: CollabDeliveryStatus,
    pub frozen_apply_json: Option<String>,
    pub request_id: String,
    pub reserved_target_id: Option<String>,
    pub target_session_id: Option<String>,
    pub target_created: bool,
    pub accepted_run_id: Option<String>,
    pub attempt: u64,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub decision_persisted: bool,
}
#[derive(Debug, Clone)]
pub struct CollabApplyClaim {
    pub claimed: bool,
    pub draft: CollabDraft,
}
#[derive(Debug, Clone)]
pub enum CollabDeliveryResult {
    Accepted { run_id: String },
    RetryQueued { reason: String },
    FailedBeforeAcceptance { reason: String },
    Unknown { reason: String },
}

fn invalid(detail: impl Into<String>) -> StorageError {
    StorageError::InvalidRequest {
        detail: format!("session collaboration: {}", detail.into()),
    }
}
fn conflict(detail: impl Into<String>) -> StorageError {
    StorageError::Conflict {
        detail: format!("session collaboration: {}", detail.into()),
    }
}
fn corrupt(detail: impl Into<String>) -> StorageError {
    StorageError::Corrupted {
        detail: format!("session collaboration: {}", detail.into()),
    }
}
fn number(now: u64) -> Result<i64, StorageError> {
    i64::try_from(now).map_err(|_| invalid("clock outside SQLite range"))
}
fn enum_json<T: Serialize>(v: &T) -> Result<String, StorageError> {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .ok_or_else(|| corrupt("invalid state enum"))
}
fn decode<T: serde::de::DeserializeOwned>(s: &str) -> Result<T, StorageError> {
    serde_json::from_value(json!(s)).map_err(|_| corrupt("unknown durable state"))
}

const COLUMNS:&str="id,studio_id,owner_subject,source_session_id,source_run_id,source_tool_call_id,kind,draft_json,expires_at,state,delivery_status,frozen_apply_json,request_id,reserved_target_id,target_session_id,target_created,accepted_run_id,attempt,error,created_at,updated_at,EXISTS(SELECT 1 FROM session_collab_decisions_v1 n WHERE n.suggestion_id=session_collab_drafts_v1.id AND json_extract(n.decision_json,'$.result.deliveryStatus') IS session_collab_drafts_v1.delivery_status AND json_extract(n.decision_json,'$.result.acceptedRunId') IS session_collab_drafts_v1.accepted_run_id AND json_extract(n.decision_json,'$.result.sessionId') IS session_collab_drafts_v1.target_session_id AND json_extract(n.decision_json,'$.result.sessionCreated') IS session_collab_drafts_v1.target_created AND json_extract(n.decision_json,'$.result.error') IS session_collab_drafts_v1.error AND json_extract(n.decision_json,'$.status') IS CASE WHEN session_collab_drafts_v1.state='rejected' THEN 'rejected' ELSE 'approved' END)";
fn load(conn: &Connection, id: &str) -> Result<Option<CollabDraft>, StorageError> {
    type Raw = (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        i64,
        String,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        bool,
        Option<String>,
        i64,
        Option<String>,
        i64,
        i64,
        bool,
    );
    let raw: Option<Raw> = conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM session_collab_drafts_v1 WHERE id=?1"),
            [id],
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
                    r.get(9)?,
                    r.get(10)?,
                    r.get(11)?,
                    r.get(12)?,
                    r.get(13)?,
                    r.get(14)?,
                    r.get(15)?,
                    r.get(16)?,
                    r.get(17)?,
                    r.get(18)?,
                    r.get(19)?,
                    r.get(20)?,
                    r.get(21)?,
                ))
            },
        )
        .optional()
        .map_err(map_rusqlite)?;
    raw.map(|r| {
        Ok(CollabDraft {
            input: CollabDraftInput {
                id: r.0,
                studio_id: r.1,
                owner_subject: r.2,
                source_session_id: r.3,
                source_run_id: r.4,
                source_tool_call_id: r.5,
                kind: decode(&r.6)?,
                draft_json: r.7,
                expires_at: r.8,
            },
            state: decode(&r.9)?,
            delivery_status: decode(&r.10)?,
            frozen_apply_json: r.11,
            request_id: r.12,
            reserved_target_id: r.13,
            target_session_id: r.14,
            target_created: r.15,
            accepted_run_id: r.16,
            attempt: u64::try_from(r.17).map_err(|_| corrupt("negative attempt"))?,
            error: r.18,
            created_at: r.19,
            updated_at: r.20,
            decision_persisted: r.21,
        })
    })
    .transpose()
}
fn required(conn: &Connection, id: &str) -> Result<CollabDraft, StorageError> {
    load(conn, id)?.ok_or_else(|| invalid("draft not found"))
}
fn session(
    conn: &Connection,
    sid: &str,
    owner: &str,
) -> Result<(String, Option<String>), StorageError> {
    let r:Option<(String,String,String,Option<String>)>=conn.query_row("SELECT agent_id,owner_user_id,lifecycle,permission_mode FROM sessions WHERE session_id=?1",[sid],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(map_rusqlite)?;
    match r {
        Some((agent, subject, life, mode)) if subject == owner && life == "active" => {
            Ok((agent, mode))
        }
        _ => Err(invalid("owned active session unavailable")),
    }
}
fn source(conn: &Connection, input: &CollabDraftInput, write: bool) -> Result<(), StorageError> {
    if input.owner_subject != LOCAL_OWNER_SUBJECT {
        return Err(invalid("local studio owner required"));
    }
    let (_, mode) = session(conn, &input.source_session_id, &input.owner_subject)?;
    if write && mode.as_deref() == Some("read_only") {
        return Err(invalid("read-only source cannot create or apply a draft"));
    }
    let valid:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM runs WHERE run_id=?1 AND session_id=?2 AND owner_subject=?3 AND owner_kind IN ('local_user','device'))",params![input.source_run_id,input.source_session_id,input.owner_subject],|r|r.get(0)).map_err(map_rusqlite)?;
    if !valid {
        return Err(invalid("draft source is not a real owned run"));
    }
    Ok(())
}
pub fn normalize_collab_payload(kind: &CollabDraftKind, raw: &str) -> Result<String, StorageError> {
    if raw.len() > 128 * 1024 {
        return Err(invalid("draft payload too large"));
    }
    let value: Value =
        serde_json::from_str(raw).map_err(|_| invalid("draft payload must be JSON"))?;
    let map = value
        .as_object()
        .ok_or_else(|| invalid("draft payload must be an object"))?;
    let allowed: &[&str] = match kind {
        CollabDraftKind::Send => &["targetSessionId", "message"],
        CollabDraftKind::Create => &["agentId", "model", "title", "firstMessage"],
    };
    if map.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err(invalid(
            "draft payload contains unknown or authority fields",
        ));
    }
    let required: &[&str] = match kind {
        CollabDraftKind::Send => &["targetSessionId", "message"],
        CollabDraftKind::Create => &["agentId", "firstMessage"],
    };
    for key in required {
        if !value[*key].as_str().is_some_and(|s| !s.trim().is_empty()) {
            return Err(invalid(format!("draft field {key} required")));
        }
    }
    for key in ["model", "title"] {
        if value
            .get(key)
            .is_some_and(|v| !v.is_null() && !v.is_string())
        {
            return Err(invalid("optional draft field must be text"));
        }
    }
    let mut normalized = value;
    for key in ["targetSessionId", "agentId", "model", "title"] {
        if let Some(s) = normalized[key].as_str() {
            normalized[key] = json!(s.trim());
        }
    }
    serde_json::to_string(&normalized).map_err(|e| corrupt(e.to_string()))
}
fn payload(draft: &CollabDraft) -> Result<Value, StorageError> {
    serde_json::from_str(
        draft
            .frozen_apply_json
            .as_deref()
            .unwrap_or(&draft.input.draft_json),
    )
    .map_err(|_| corrupt("saved payload is malformed"))
}
fn validate_target(conn: &Connection, draft: &CollabDraft, sid: &str) -> Result<(), StorageError> {
    if sid == draft.input.source_session_id {
        return Err(invalid("cannot deliver to the source session"));
    }
    let (agent, _) = session(conn, sid, &draft.input.owner_subject)?;
    let value = payload(draft)?;
    match draft.input.kind {
        CollabDraftKind::Send => {
            if value["targetSessionId"].as_str() != Some(sid) {
                return Err(invalid("send target differs from approved payload"));
            }
        }
        CollabDraftKind::Create => {
            if value["agentId"].as_str() != Some(agent.as_str())
                || draft.reserved_target_id.as_deref() != Some(sid)
            {
                return Err(invalid("created target differs from frozen reservation"));
            }
        }
    }
    Ok(())
}
fn identity(draft: &CollabDraft, studio: &str, owner: &str) -> Result<(), StorageError> {
    if draft.input.studio_id != studio
        || draft.input.owner_subject != owner
        || owner != LOCAL_OWNER_SUBJECT
    {
        return Err(invalid("draft authority does not match caller"));
    }
    Ok(())
}

pub async fn create_collab_draft(
    db: &RunDatabase,
    input: &CollabDraftInput,
    now: u64,
) -> Result<CollabDraft, StorageError> {
    let mut input = input.clone();
    let now = number(now)?;
    if [
        &input.id,
        &input.studio_id,
        &input.source_session_id,
        &input.source_run_id,
        &input.source_tool_call_id,
    ]
    .iter()
    .any(|s| s.trim().is_empty() || s.len() > 2048)
        || input.expires_at <= now
    {
        return Err(invalid("invalid draft identity or expiry"));
    }
    input.draft_json = normalize_collab_payload(&input.kind, &input.draft_json)?;
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        if let Some(old)=load(conn,&input.id)?{let mut replay=input.clone();replay.expires_at=old.input.expires_at;if old.input==replay{return Ok(old);}return Err(conflict("draft ID reused with different payload"));}
        let old:Option<String>=conn.query_row("SELECT id FROM session_collab_drafts_v1 WHERE source_run_id=?1 AND source_tool_call_id=?2",params![input.source_run_id,input.source_tool_call_id],|r|r.get(0)).optional().map_err(map_rusqlite)?;
        if old.is_some(){return Err(conflict("source tool call already has a draft"));}
        source(conn,&input,true)?;
        if input.kind==CollabDraftKind::Send{let value:Value=serde_json::from_str(&input.draft_json).map_err(|_|corrupt("validated payload"))?;let target=value["targetSessionId"].as_str().ok_or_else(||invalid("send target required"))?;if target==input.source_session_id{return Err(invalid("cannot send to source session"));}session(conn,target,&input.owner_subject)?;}
        conn.execute("INSERT INTO session_collab_drafts_v1(id,studio_id,owner_subject,source_session_id,source_run_id,source_tool_call_id,kind,draft_json,request_id,state,delivery_status,created_at,expires_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'pending','none',?10,?11,?10)",params![input.id,input.studio_id,input.owner_subject,input.source_session_id,input.source_run_id,input.source_tool_call_id,enum_json(&input.kind)?,input.draft_json,format!("session-collab:{}",input.id),now,input.expires_at]).map_err(map_rusqlite)?;
        required(conn,&input.id)
    })).await
}
pub async fn get_collab_draft(
    db: &RunDatabase,
    id: &str,
) -> Result<Option<CollabDraft>, StorageError> {
    let id = id.to_owned();
    db.queue().submit(move |conn| load(conn, &id)).await
}
pub async fn list_collab_drafts_for_session(
    db: &RunDatabase,
    sid: &str,
) -> Result<Vec<CollabDraft>, StorageError> {
    let sid = sid.to_owned();
    db.queue().submit(move|conn|{let ids={let mut stmt=conn.prepare("SELECT id FROM session_collab_drafts_v1 WHERE source_session_id=?1 ORDER BY created_at,id").map_err(map_rusqlite)?;let rows=stmt.query_map([sid],|r|r.get::<_,String>(0)).map_err(map_rusqlite)?;rows.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)?};ids.iter().map(|id|required(conn,id)).collect()}).await
}
#[allow(clippy::too_many_arguments)]
pub async fn claim_collab_apply(
    db: &RunDatabase,
    id: &str,
    studio: &str,
    owner: &str,
    edited_json: &str,
    now: u64,
) -> Result<CollabApplyClaim, StorageError> {
    let (id, studio, owner, edited, now, busy) = (
        id.to_owned(),
        studio.to_owned(),
        owner.to_owned(),
        edited_json.to_owned(),
        number(now)?,
        db.options().busy_timeout_ms,
    );
    let reserved = format!(
        "session-collab-{}",
        super::agent_memory::random_token_suffix()
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let old=required(conn,&id)?;identity(&old,&studio,&owner)?;source(conn,&old.input,false)?;
        let normalized=normalize_collab_payload(&old.input.kind,&edited)?;
        if old.frozen_apply_json.as_ref().is_some_and(|s|s!=&normalized){return Err(conflict("approved draft payload is already frozen"));}
        if old.state!=CollabDraftState::Pending{return Ok(CollabApplyClaim{claimed:false,draft:old});}
        source(conn,&old.input,true)?;
        if old.frozen_apply_json.is_none()&&old.input.expires_at<=now{return Err(invalid("draft expired"));}
        let value:Value=serde_json::from_str(&normalized).map_err(|_|corrupt("normalized payload"))?;
        let reserved=old.reserved_target_id.clone().unwrap_or_else(||match old.input.kind{CollabDraftKind::Send=>value["targetSessionId"].as_str().unwrap_or_default().into(),CollabDraftKind::Create=>reserved.clone()});
        if old.input.kind==CollabDraftKind::Send{session(conn,&reserved,&owner)?;if reserved==old.input.source_session_id{return Err(invalid("cannot send to source session"));}}
        conn.execute("UPDATE session_collab_drafts_v1 SET state='applying',delivery_status='queued',frozen_apply_json=?2,reserved_target_id=?3,error=NULL,updated_at=?4 WHERE id=?1",params![id,normalized,reserved,now]).map_err(map_rusqlite)?;
        Ok(CollabApplyClaim{claimed:true,draft:required(conn,&id)?})
    })).await
}
pub async fn claim_collab_delivery(
    db: &RunDatabase,
    id: &str,
    now: u64,
) -> Result<CollabApplyClaim, StorageError> {
    let (id, now, busy) = (id.to_owned(), number(now)?, db.options().busy_timeout_ms);
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let old=required(conn,&id)?;if old.state!=CollabDraftState::Applying||old.delivery_status!=CollabDeliveryStatus::Queued{return Ok(CollabApplyClaim{claimed:false,draft:old});}source(conn,&old.input,true)?;
        if old.attempt>=i64::MAX as u64{return Err(conflict("delivery attempt exhausted"));}
        conn.execute("UPDATE session_collab_drafts_v1 SET delivery_status='started',attempt=attempt+1,error=NULL,updated_at=?2 WHERE id=?1",params![id,now]).map_err(map_rusqlite)?;
        Ok(CollabApplyClaim{claimed:true,draft:required(conn,&id)?})
    })).await
}
pub async fn bind_collab_target(
    db: &RunDatabase,
    id: &str,
    attempt: u64,
    sid: &str,
    created: bool,
    now: u64,
) -> Result<CollabDraft, StorageError> {
    let (id, sid, now, busy) = (
        id.to_owned(),
        sid.to_owned(),
        number(now)?,
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let d=required(conn,&id)?;if !matches!(d.delivery_status,CollabDeliveryStatus::Started|CollabDeliveryStatus::Unknown)||d.attempt!=attempt{return Err(conflict("target binding lease changed"));}source(conn,&d.input,false)?;validate_target(conn,&d,&sid)?;
        if d.reserved_target_id.as_deref()!=Some(sid.as_str())||d.target_session_id.as_ref().is_some_and(|old|old!=&sid)||created!=(d.input.kind==CollabDraftKind::Create){return Err(invalid("target reservation changed"));}
        conn.execute("UPDATE session_collab_drafts_v1 SET target_session_id=?2,target_created=?3,updated_at=?4 WHERE id=?1",params![id,sid,created,now]).map_err(map_rusqlite)?;required(conn,&id)
    })).await
}
fn actual_run(
    conn: &Connection,
    draft: &CollabDraft,
) -> Result<Option<(String, String, String)>, StorageError> {
    let mut stmt=conn.prepare("SELECT r.run_id,r.session_id,r.status,r.owner_subject,l.parent_run_id,r.owner_kind,l.source_message_id FROM run_lineage l JOIN runs r ON r.run_id=l.run_id WHERE l.origin='session_collab' AND l.cause_id=?1").map_err(map_rusqlite)?;
    let rows = stmt
        .query_map([&draft.input.id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, Option<String>>(6)?,
            ))
        })
        .map_err(map_rusqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_rusqlite)?;
    if rows.len() > 1 {
        return Err(conflict(
            "more than one real run for one collaboration draft",
        ));
    }
    if let Some((id, sid, status, owner, parent, owner_kind, source_message)) =
        rows.into_iter().next()
    {
        if owner != draft.input.owner_subject
            || parent.as_deref() != Some(draft.input.source_run_id.as_str())
            || draft.reserved_target_id.as_deref() != Some(sid.as_str())
            || !matches!(owner_kind.as_str(), "local_user" | "device")
            || source_message.is_some()
        {
            return Err(conflict(
                "real collaboration lineage differs from frozen draft",
            ));
        }
        return Ok(Some((id, sid, status)));
    }
    Ok(None)
}
pub async fn get_collab_run(
    db: &RunDatabase,
    id: &str,
) -> Result<Option<(String, String, String)>, StorageError> {
    let id = id.to_owned();
    db.queue()
        .submit(move |conn| actual_run(conn, &required(conn, &id)?))
        .await
}
pub async fn settle_collab_delivery(
    db: &RunDatabase,
    id: &str,
    attempt: u64,
    result: CollabDeliveryResult,
    now: u64,
) -> Result<CollabDraft, StorageError> {
    let (id, now, busy) = (id.to_owned(), number(now)?, db.options().busy_timeout_ms);
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let d=required(conn,&id)?;
        if let CollabDeliveryResult::Accepted{run_id}=&result{if d.state==CollabDraftState::Applied&&d.accepted_run_id.as_ref()==Some(run_id){return Ok(d);}let run=actual_run(conn,&d)?.ok_or_else(||invalid("accepted delivery has no real run"))?;if &run.0!=run_id||d.target_session_id.as_deref()!=Some(run.1.as_str()){return Err(invalid("accepted delivery run does not match target"));}}
        if d.attempt!=attempt||!matches!(d.delivery_status,CollabDeliveryStatus::Started|CollabDeliveryStatus::Unknown){return Err(conflict("delivery lease changed"));}
        if !matches!(&result,CollabDeliveryResult::Accepted{..})&&actual_run(conn,&d)?.is_some(){return Err(conflict("real accepted run cannot be requeued or marked unaccepted"));}
        let(state,status,run,error)=match result{CollabDeliveryResult::Accepted{run_id}=>("applied","accepted",Some(run_id),None),CollabDeliveryResult::RetryQueued{reason}=>("applying","queued",None,Some(reason)),CollabDeliveryResult::FailedBeforeAcceptance{reason}=>("pending","failed",None,Some(reason)),CollabDeliveryResult::Unknown{reason}=>("unknown","unknown",None,Some(reason))};
        if d.delivery_status==CollabDeliveryStatus::Unknown&&status!="accepted"{return Err(conflict("unknown delivery cannot be retried automatically"));}
        conn.execute("UPDATE session_collab_drafts_v1 SET state=?2,delivery_status=?3,accepted_run_id=?4,error=?5,updated_at=?6 WHERE id=?1",params![id,state,status,run,error,now]).map_err(map_rusqlite)?;required(conn,&id)
    })).await
}
pub async fn fail_queued_collab_draft(
    db: &RunDatabase,
    id: &str,
    reason: &str,
    now: u64,
) -> Result<CollabDraft, StorageError> {
    let (id, reason, now, busy) = (
        id.to_owned(),
        reason.to_owned(),
        number(now)?,
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{let d=required(conn,&id)?;if d.delivery_status==CollabDeliveryStatus::Queued{if actual_run(conn,&d)?.is_some(){return Err(conflict("real accepted run cannot be marked unaccepted"));}conn.execute("UPDATE session_collab_drafts_v1 SET state='pending',delivery_status='failed',error=?2,updated_at=?3 WHERE id=?1",params![id,reason,now]).map_err(map_rusqlite)?;}required(conn,&id)})).await
}
pub async fn reject_collab_draft(
    db: &RunDatabase,
    id: &str,
    studio: &str,
    owner: &str,
    now: u64,
) -> Result<CollabDraft, StorageError> {
    let (id, studio, owner, now, busy) = (
        id.to_owned(),
        studio.to_owned(),
        owner.to_owned(),
        number(now)?,
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{let d=required(conn,&id)?;identity(&d,&studio,&owner)?;source(conn,&d.input,false)?;if d.state==CollabDraftState::Rejected{return Ok(d);}
if d.state!=CollabDraftState::Pending{return Err(conflict("draft is already applying or settled"));}conn.execute("UPDATE session_collab_drafts_v1 SET state='rejected',updated_at=?2 WHERE id=?1",params![id,now]).map_err(map_rusqlite)?;required(conn,&id)})).await
}
async fn list_delivery(db: &RunDatabase, status: &str) -> Result<Vec<CollabDraft>, StorageError> {
    let status = status.to_owned();
    db.queue().submit(move|conn|{let ids={let mut stmt=conn.prepare("SELECT id FROM session_collab_drafts_v1 WHERE delivery_status=?1 ORDER BY created_at,id LIMIT 128").map_err(map_rusqlite)?;let rows=stmt.query_map([status],|r|r.get::<_,String>(0)).map_err(map_rusqlite)?;rows.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)?};ids.iter().map(|id|required(conn,id)).collect()}).await
}
pub async fn list_queued_collab_drafts(db: &RunDatabase) -> Result<Vec<CollabDraft>, StorageError> {
    list_delivery(db, "queued").await
}
pub async fn list_started_collab_drafts(
    db: &RunDatabase,
) -> Result<Vec<CollabDraft>, StorageError> {
    list_delivery(db, "started").await
}
pub async fn list_unknown_collab_drafts(
    db: &RunDatabase,
) -> Result<Vec<CollabDraft>, StorageError> {
    list_delivery(db, "unknown").await
}

pub fn collab_result(d: &CollabDraft) -> Value {
    json!({"sessionId":d.target_session_id,"targetSessionId":d.target_session_id,"sessionCreated":d.target_created,"firstMessageAccepted":d.delivery_status==CollabDeliveryStatus::Accepted,"deliveryStatus":d.delivery_status,"acceptedRunId":d.accepted_run_id,"requestId":d.request_id,"retryable":d.delivery_status==CollabDeliveryStatus::Failed,"error":d.error})
}
/// 决策和源会话宿主条目同事务；状态变化追加新条目，不改写既有历史。
pub async fn collab_decisions_for_session(
    db: &RunDatabase,
    session_id: &str,
) -> Result<Vec<Value>, StorageError> {
    let session_id = session_id.to_owned();
    db.queue().submit(move |conn| {
        let mut statement=conn.prepare("SELECT decision_json FROM session_collab_decisions_v1 WHERE session_id=?1 ORDER BY created_at,suggestion_id").map_err(map_rusqlite)?;
        let encoded=statement.query_map([session_id],|row|row.get::<_,String>(0)).map_err(map_rusqlite)?.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)?;
        encoded.into_iter().map(|value|serde_json::from_str(&value).map_err(|_|corrupt("collaboration decision receipt"))).collect()
    }).await
}
pub async fn list_unrecorded_collab_decisions(
    db: &RunDatabase,
) -> Result<Vec<CollabDraft>, StorageError> {
    db.queue().submit(move |conn| {
        let mut statement=conn.prepare("SELECT d.id FROM session_collab_drafts_v1 d LEFT JOIN session_collab_decisions_v1 receipt ON receipt.suggestion_id=d.id WHERE (d.frozen_apply_json IS NOT NULL OR d.state='rejected') AND (receipt.suggestion_id IS NULL OR json_extract(receipt.decision_json,'$.result.deliveryStatus') IS NOT d.delivery_status OR json_extract(receipt.decision_json,'$.result.acceptedRunId') IS NOT d.accepted_run_id OR json_extract(receipt.decision_json,'$.result.sessionId') IS NOT d.target_session_id OR json_extract(receipt.decision_json,'$.result.sessionCreated') IS NOT d.target_created OR json_extract(receipt.decision_json,'$.result.error') IS NOT d.error OR json_extract(receipt.decision_json,'$.status') IS NOT CASE WHEN d.state='rejected' THEN 'rejected' ELSE 'approved' END) ORDER BY d.updated_at,d.id LIMIT 128").map_err(map_rusqlite)?;
        let ids=statement.query_map([],|row|row.get::<_,String>(0)).map_err(map_rusqlite)?.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)?;
        ids.into_iter().map(|id|required(conn,&id)).collect()
    }).await
}
pub async fn record_collab_decision(
    db: &RunDatabase,
    id: &str,
    now: u64,
) -> Result<Option<Value>, StorageError> {
    let (id, now, busy) = (id.to_owned(), number(now)?, db.options().busy_timeout_ms);
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let d=required(conn,&id)?;
        if d.state==CollabDraftState::Pending&&d.frozen_apply_json.is_none(){return Ok(None);}
        source(conn,&d.input,false)?;
        let frozen_apply=d.frozen_apply_json.as_deref().map(serde_json::from_str::<Value>).transpose().map_err(|_|corrupt("frozen collaboration payload"))?;
        let mut decision=json!({"suggestionId":id,"status":if d.state==CollabDraftState::Rejected{"rejected"}else{"approved"},"result":collab_result(&d),"frozenApply":frozen_apply});
        if d.target_created{decision["resultSessionId"]=json!(d.target_session_id);}
        let old:Option<String>=conn.query_row("SELECT decision_json FROM session_collab_decisions_v1 WHERE suggestion_id=?1",[&id],|r|r.get(0)).optional().map_err(map_rusqlite)?;
        if let Some(old)=old{let mut parsed:Value=serde_json::from_str(&old).map_err(|_|corrupt("decision JSON malformed"))?;let original=parsed.clone();parsed.as_object_mut().ok_or_else(||corrupt("decision must be object"))?.remove("timestamp");if parsed==decision{return Ok(Some(original));}}
        decision["timestamp"]=json!(now);
        let body=serde_json::to_string(&decision).map_err(|e|corrupt(e.to_string()))?;
        let parent:Option<Option<String>>=conn.query_row("SELECT head_message_id FROM session_branch_heads WHERE session_id=?1",[&d.input.source_session_id],|r|r.get(0)).optional().map_err(map_rusqlite)?;
        let parent=match parent{Some(head)=>head,None=>conn.query_row("SELECT message_id FROM messages WHERE session_id=?1 ORDER BY seq DESC LIMIT 1",[&d.input.source_session_id],|r|r.get(0)).optional().map_err(map_rusqlite)?};
        let seq:i64=conn.query_row("SELECT COALESCE(MAX(seq),0)+1 FROM messages WHERE session_id=?1",[&d.input.source_session_id],|r|r.get(0)).map_err(map_rusqlite)?;
        let message=format!("collab-decision:{id}:{seq}");
        conn.execute("INSERT INTO messages(message_id,session_id,run_id,role,content_json,model_call_id,committed_at_unix_ms,seq,parent_message_id,entry_type) VALUES(?1,?2,NULL,'host_context',?3,NULL,?4,?5,?6,?7)",params![message,d.input.source_session_id,body,now,seq,parent,DECISION_ENTRY_TYPE]).map_err(map_rusqlite)?;
        conn.execute("INSERT INTO session_branch_heads(session_id,head_message_id,observed_tail_message_id,revision,head_resolution,updated_at_unix_ms) VALUES(?1,?2,?2,1,'persisted_head',?3) ON CONFLICT(session_id) DO UPDATE SET head_message_id=excluded.head_message_id,revision=session_branch_heads.revision+1,head_resolution='persisted_head',updated_at_unix_ms=excluded.updated_at_unix_ms",params![d.input.source_session_id,message,now]).map_err(map_rusqlite)?;
        conn.execute("INSERT INTO session_collab_decisions_v1(suggestion_id,session_id,message_id,decision_json,created_at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(suggestion_id) DO UPDATE SET message_id=excluded.message_id,decision_json=excluded.decision_json,created_at=excluded.created_at",params![id,d.input.source_session_id,message,body,now]).map_err(map_rusqlite)?;
        Ok(Some(decision))
    })).await
}
