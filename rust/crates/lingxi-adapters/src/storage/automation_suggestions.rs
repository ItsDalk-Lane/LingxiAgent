//! 自动任务建议只在所属会话明确应用后生效；计划与应用回执同事务提交。

use super::{
    automation::{get_job_conn, validate_job, AutomationJob, AutomationJobInput},
    migrations::map_rusqlite,
    run_store::with_write_txn,
    RunDatabase,
};
use lingxi_kernel::{ports::StorageError, LOCAL_OWNER_SUBJECT};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub id: String,
    pub studio_id: String,
    pub session_id: String,
    pub owner_kind: String,
    pub owner_subject: String,
    pub expected_revision: Option<u64>,
    pub job_id: Option<String>,
    pub job: AutomationJobInput,
    pub expires_at: i64,
}

struct StoredSuggestion {
    value: Suggestion,
    operation: String,
    state: String,
    applied_job_id: Option<String>,
}

struct RawSuggestion {
    studio_id: String,
    session_id: String,
    owner_subject: String,
    operation: String,
    job_id: Option<String>,
    expected_revision: Option<i64>,
    job_json: String,
    expires_at: i64,
    state: String,
    applied_job_id: Option<String>,
}

fn invalid(detail: &str) -> StorageError {
    StorageError::InvalidRequest {
        detail: detail.into(),
    }
}

fn conflict(detail: &str) -> StorageError {
    StorageError::Conflict {
        detail: detail.into(),
    }
}

fn corrupted(detail: impl Into<String>) -> StorageError {
    StorageError::Corrupted {
        detail: detail.into(),
    }
}

fn expected_revision(value: Option<u64>) -> Result<Option<i64>, StorageError> {
    value
        .map(|revision| {
            i64::try_from(revision)
                .ok()
                .filter(|revision| *revision > 0 && *revision < i64::MAX)
                .ok_or_else(|| invalid("suggestion revision outside positive incrementable range"))
        })
        .transpose()
}

fn validate_identity(value: &Suggestion) -> Result<(), StorageError> {
    if [&value.id, &value.studio_id, &value.session_id]
        .iter()
        .any(|id| id.trim().is_empty())
        || value.owner_kind != "local_user"
        || value.owner_subject != LOCAL_OWNER_SUBJECT
        || value.studio_id != value.job.studio_id
        || value.owner_kind != value.job.owner_kind
        || value.owner_subject != value.job.owner_subject
    {
        return Err(invalid("invalid automation suggestion identity"));
    }
    validate_job(&value.job)?;
    if !serde_json::from_str::<Value>(&value.job.config_json)
        .map_err(|_| invalid("invalid suggestion config JSON"))?
        .is_object()
    {
        return Err(invalid("suggestion config must be an object"));
    }
    expected_revision(value.expected_revision)?;
    match (&value.job_id, value.expected_revision) {
        (None, None) => Ok(()),
        (Some(id), Some(_)) if id == &value.job.id => Ok(()),
        _ => Err(invalid(
            "suggestion operation, target and revision disagree",
        )),
    }
}

fn read_stored(conn: &Connection, id: &str) -> Result<Option<StoredSuggestion>, StorageError> {
    let raw=conn.query_row("SELECT studio_id,session_id,owner_subject,operation,job_id,expected_revision,job_json,expires_at,state,applied_job_id FROM automation_suggestions_v1 WHERE id=?1",[id],|row|Ok(RawSuggestion{
        studio_id:row.get(0)?,session_id:row.get(1)?,owner_subject:row.get(2)?,operation:row.get(3)?,job_id:row.get(4)?,expected_revision:row.get(5)?,job_json:row.get(6)?,expires_at:row.get(7)?,state:row.get(8)?,applied_job_id:row.get(9)?
    })).optional().map_err(map_rusqlite)?;
    let Some(raw) = raw else { return Ok(None) };
    let value = Suggestion {
        id: id.into(),
        studio_id: raw.studio_id,
        session_id: raw.session_id,
        owner_kind: "local_user".into(),
        owner_subject: raw.owner_subject,
        expected_revision: raw
            .expected_revision
            .map(|revision| {
                u64::try_from(revision)
                    .map_err(|_| corrupted("negative stored suggestion revision"))
            })
            .transpose()?,
        job_id: raw.job_id,
        job: serde_json::from_str(&raw.job_json)
            .map_err(|err| corrupted(format!("invalid stored suggestion job: {err}")))?,
        expires_at: raw.expires_at,
    };
    validate_identity(&value)
        .map_err(|err| corrupted(format!("stored suggestion identity is inconsistent: {err}")))?;
    let operation = if value.job_id.is_some() {
        "update"
    } else {
        "create"
    };
    if raw.operation != operation
        || !matches!(raw.state.as_str(), "pending" | "applied")
        || (raw.state == "applied" && raw.applied_job_id.as_deref() != Some(&value.job.id))
        || (raw.state == "pending" && raw.applied_job_id.is_some())
    {
        return Err(corrupted(
            "stored suggestion state or operation is inconsistent",
        ));
    }
    Ok(Some(StoredSuggestion {
        value,
        operation: raw.operation,
        state: raw.state,
        applied_job_id: raw.applied_job_id,
    }))
}

fn ensure_session(conn: &Connection, session: &str, owner: &str) -> Result<(), StorageError> {
    if owner != LOCAL_OWNER_SUBJECT {
        return Err(invalid("local owner required for automation suggestion"));
    }
    let current_owner: Option<String> = conn
        .query_row(
            "SELECT owner_user_id FROM sessions WHERE session_id=?1 AND lifecycle='active'",
            [session],
            |row| row.get(0),
        )
        .optional()
        .map_err(map_rusqlite)?;
    if current_owner.as_deref() != Some(owner) {
        return Err(conflict(
            "suggestion session ownership changed or session is inactive",
        ));
    }
    Ok(())
}

fn ensure_job_scope(job: &AutomationJob, suggestion: &Suggestion) -> Result<(), StorageError> {
    if job.id != suggestion.job.id
        || job.studio_id != suggestion.studio_id
        || job.owner_kind != "local_user"
        || job.owner_subject != LOCAL_OWNER_SUBJECT
        || job.deleted_at.is_some()
    {
        return Err(conflict(
            "suggestion job ownership changed or target was deleted",
        ));
    }
    Ok(())
}

/// 只允许编辑界面声明的计划字段；来源、执行主体及未知配置字段均冻结。
fn validate_prepared(
    suggestion: &Suggestion,
    prepared: &AutomationJobInput,
) -> Result<(), StorageError> {
    validate_job(prepared)?;
    let frozen = &suggestion.job;
    if prepared.id != frozen.id
        || prepared.studio_id != frozen.studio_id
        || prepared.actor_id != frozen.actor_id
        || prepared.owner_kind != frozen.owner_kind
        || prepared.owner_subject != frozen.owner_subject
        || prepared.schema_version != frozen.schema_version
    {
        return Err(invalid("prepared suggestion changed frozen job identity"));
    }
    let mut original: Value = serde_json::from_str(&frozen.config_json)
        .map_err(|_| corrupted("invalid stored suggestion config"))?;
    let mut candidate: Value = serde_json::from_str(&prepared.config_json)
        .map_err(|_| invalid("invalid prepared suggestion config"))?;
    let (Some(original), Some(candidate)) = (original.as_object_mut(), candidate.as_object_mut())
    else {
        return Err(invalid("suggestion config must be an object"));
    };
    for key in [
        "type",
        "scheduleType",
        "schedule",
        "prompt",
        "label",
        "model",
        "enabled",
    ] {
        original.remove(key);
        candidate.remove(key);
    }
    if original != candidate {
        return Err(invalid(
            "prepared suggestion changed frozen source or configuration",
        ));
    }
    Ok(())
}

pub async fn create(db: &RunDatabase, value: &Suggestion, now: i64) -> Result<(), StorageError> {
    validate_identity(value)?;
    if value.expires_at <= now {
        return Err(invalid("automation_suggestion_expired"));
    }
    let value = value.clone();
    let revision = expected_revision(value.expected_revision)?;
    let encoded =
        serde_json::to_string(&value.job).map_err(|_| invalid("invalid suggestion job"))?;
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        ensure_session(conn,&value.session_id,&value.owner_subject)?;
        if let Some(existing)=read_stored(conn,&value.id)?{return if existing.value==value{Ok(())}else{Err(conflict("suggestion id reused with different frozen content"))};}
        if let Some(job_id)=&value.job_id {
            let job=get_job_conn(conn,job_id)?.ok_or_else(||conflict("suggestion target unavailable"))?;
            ensure_job_scope(&job,&value)?;
            if Some(job.config_revision)!=value.expected_revision{return Err(conflict("cron_job_revision_conflict"));}
        }else if get_job_conn(conn,&value.job.id)?.is_some(){return Err(conflict("suggested job id already exists"));}
        conn.execute("INSERT INTO automation_suggestions_v1(id,studio_id,session_id,owner_subject,operation,job_id,expected_revision,job_json,expires_at,state,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'pending',?10)",params![value.id,value.studio_id,value.session_id,value.owner_subject,if value.job_id.is_some(){"update"}else{"create"},value.job_id,revision,encoded,value.expires_at,now]).map_err(map_rusqlite)?;
        Ok(())
    })).await
}

pub async fn get(
    db: &RunDatabase,
    id: &str,
    studio: &str,
    session: &str,
    owner: &str,
    now: i64,
) -> Result<Suggestion, StorageError> {
    let (id, studio, session, owner) = (
        id.to_owned(),
        studio.to_owned(),
        session.to_owned(),
        owner.to_owned(),
    );
    db.queue()
        .submit(move |conn| {
            let stored = read_stored(conn, &id)?
                .ok_or_else(|| invalid("automation_suggestion_unavailable"))?;
            let value = stored.value;
            if value.studio_id != studio
                || value.session_id != session
                || value.owner_subject != owner
            {
                return Err(invalid("automation_suggestion_unavailable"));
            }
            ensure_session(conn, &session, &owner)?;
            if stored.state == "pending" && value.expires_at <= now {
                return Err(invalid("automation_suggestion_expired"));
            }
            Ok(value)
        })
        .await
}

/// 已应用回执先于重新计算计划读取，过期日期不能破坏成功应用的重放。
pub async fn applied_job(
    db: &RunDatabase,
    id: &str,
    studio: &str,
    session: &str,
    owner: &str,
) -> Result<Option<AutomationJob>, StorageError> {
    let (id, studio, session, owner) = (
        id.to_owned(),
        studio.to_owned(),
        session.to_owned(),
        owner.to_owned(),
    );
    db.queue()
        .submit(move |conn| {
            let stored = read_stored(conn, &id)?
                .ok_or_else(|| invalid("automation_suggestion_unavailable"))?;
            let value = stored.value;
            if value.studio_id != studio
                || value.session_id != session
                || value.owner_subject != owner
            {
                return Err(invalid("automation_suggestion_unavailable"));
            }
            ensure_session(conn, &session, &owner)?;
            if stored.state == "pending" {
                return Ok(None);
            }
            let job = get_job_conn(
                conn,
                stored
                    .applied_job_id
                    .as_deref()
                    .ok_or_else(|| corrupted("applied job receipt missing"))?,
            )?
            .ok_or_else(|| conflict("applied job unavailable"))?;
            ensure_job_scope(&job, &value)?;
            Ok(Some(job))
        })
        .await
}

/// 事务重读冻结记录，不以传入建议对象中的目标或版本作为写入依据。
pub async fn apply(
    db: &RunDatabase,
    suggestion: &Suggestion,
    prepared: &AutomationJobInput,
    now: i64,
) -> Result<AutomationJob, StorageError> {
    let (supplied, prepared) = (suggestion.clone(), prepared.clone());
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let stored=read_stored(conn,&supplied.id)?.ok_or_else(||invalid("automation_suggestion_unavailable"))?;
        let value=stored.value;
        if supplied!=value{return Err(invalid("suggestion differs from the durable frozen record"));}
        ensure_session(conn,&value.session_id,&value.owner_subject)?;
        validate_prepared(&value,&prepared)?;
        if stored.state=="applied"{
            let job=get_job_conn(conn,stored.applied_job_id.as_deref().ok_or_else(||corrupted("applied job receipt missing"))?)?.ok_or_else(||conflict("applied job unavailable"))?;
            ensure_job_scope(&job,&value)?;
            return Ok(job);
        }
        if value.expires_at<=now{return Err(invalid("automation_suggestion_expired"));}
        if stored.operation=="update" {
            let old_id=value.job_id.as_deref().ok_or_else(||corrupted("stored update target missing"))?;
            let current=get_job_conn(conn,old_id)?.ok_or_else(||conflict("suggestion target unavailable"))?;
            ensure_job_scope(&current,&value)?;
            let expected=expected_revision(value.expected_revision)?.ok_or_else(||corrupted("stored update revision missing"))?;
            if Some(current.config_revision)!=value.expected_revision{return Err(conflict("cron_job_revision_conflict"));}
            let next=expected.checked_add(1).ok_or_else(||invalid("suggestion revision exhausted"))?;
            let changed=conn.execute("UPDATE automation_jobs_v1 SET actor_id=?2,schema_version=?3,config_json=?4,next_run_at=?5,config_revision=?6,updated_at=?7 WHERE id=?1 AND config_revision=?8 AND deleted_at IS NULL",params![prepared.id,prepared.actor_id,prepared.schema_version,prepared.config_json,prepared.next_run_at,next,now,expected]).map_err(map_rusqlite)?;
            if changed!=1{return Err(conflict("cron_job_revision_conflict"));}
        }else {
            if get_job_conn(conn,&prepared.id)?.is_some(){return Err(conflict("suggested job id already exists"));}
            conn.execute("INSERT INTO automation_jobs_v1(id,studio_id,actor_id,owner_kind,owner_subject,schema_version,config_revision,config_json,next_run_at,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,1,?7,?8,?9,?9)",params![prepared.id,prepared.studio_id,prepared.actor_id,prepared.owner_kind,prepared.owner_subject,prepared.schema_version,prepared.config_json,prepared.next_run_at,now]).map_err(map_rusqlite)?;
        }
        let changed=conn.execute("UPDATE automation_suggestions_v1 SET state='applied',applied_job_id=?2 WHERE id=?1 AND state='pending'",params![value.id,prepared.id]).map_err(map_rusqlite)?;
        if changed!=1{return Err(conflict("suggestion application receipt changed"));}
        get_job_conn(conn,&prepared.id)?.ok_or_else(||corrupted("applied job missing"))
    })).await
}
