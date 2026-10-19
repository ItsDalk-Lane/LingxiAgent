//! 自动化仅保存计划、触发与投递事实；执行及终态仍由现有运行监督器负责。

use super::{migrations::map_rusqlite, run_store::with_write_txn, RunDatabase};
use lingxi_kernel::ports::StorageError;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationJobInput {
    pub id: String,
    pub studio_id: String,
    pub actor_id: String,
    pub owner_kind: String,
    pub owner_subject: String,
    pub schema_version: u32,
    /// 保留完整原文，不将未知兼容字段读成默认值后覆盖。
    pub config_json: String,
    pub next_run_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationJob {
    pub id: String,
    pub studio_id: String,
    pub actor_id: String,
    pub owner_kind: String,
    pub owner_subject: String,
    pub schema_version: u32,
    pub config_revision: u64,
    pub config_json: String,
    pub next_run_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TriggerStatus {
    Claimed,
    Running,
    Succeeded,
    Failed,
    Skipped,
    Cancelled,
    Unknown,
}

impl TriggerStatus {
    fn terminal(self) -> bool {
        !matches!(self, Self::Claimed | Self::Running)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerInput {
    pub trigger_id: String,
    pub job_id: String,
    pub due_at: i64,
    pub config_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationTrigger {
    pub trigger_id: String,
    pub job_id: String,
    pub due_at: i64,
    pub config_revision: u64,
    /// 领取触发时同事务冻结计划；迟到执行不读取后续编辑后的授权文本。
    pub config_json: String,
    pub status: TriggerStatus,
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimTriggerOutcome {
    pub created: bool,
    pub trigger: AutomationTrigger,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettleTriggerOutcome {
    pub trigger: AutomationTrigger,
    pub cursor_advanced: bool,
}

pub use lingxi_protocol::{
    AutomationNotificationClaim as ReserveNotificationOutcome,
    AutomationNotificationReceipt as NotificationReceipt,
    AutomationNotificationStatus as NotificationStatus,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationInput {
    pub delivery_id: String,
    pub trigger_id: Option<String>,
    pub run_id: Option<String>,
    pub channel: String,
    pub recipient: String,
    pub payload_json: String,
}

fn invalid(detail: impl Into<String>) -> StorageError {
    StorageError::InvalidRequest {
        detail: format!("automation: {}", detail.into()),
    }
}

fn conflict(detail: impl Into<String>) -> StorageError {
    StorageError::Conflict {
        detail: format!("automation: {}", detail.into()),
    }
}

fn require_id(value: &str, label: &str) -> Result<(), StorageError> {
    if value.trim().is_empty() {
        return Err(invalid(format!("{label} is empty")));
    }
    Ok(())
}

fn validate_json(raw: &str) -> Result<(), StorageError> {
    serde_json::from_str::<Value>(raw)
        .map(|_| ())
        .map_err(|err| invalid(format!("invalid JSON: {err}")))
}

pub(crate) fn validate_job(input: &AutomationJobInput) -> Result<(), StorageError> {
    for (value, label) in [
        (&input.id, "id"),
        (&input.studio_id, "studioId"),
        (&input.actor_id, "actorId"),
        (&input.owner_kind, "ownerKind"),
        (&input.owner_subject, "ownerSubject"),
    ] {
        require_id(value, label)?;
    }
    if input.schema_version == 0 {
        return Err(invalid("schemaVersion must be positive"));
    }
    validate_json(&input.config_json)
}

fn revision(value: u64) -> Result<i64, StorageError> {
    i64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| invalid("configRevision outside SQLite positive integer range"))
}

fn wire<T: Serialize>(value: T) -> Result<String, StorageError> {
    let value = serde_json::to_value(value).map_err(|err| invalid(err.to_string()))?;
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid("status must serialize as a string"))
}

fn parse<T: serde::de::DeserializeOwned>(raw: &str, index: usize) -> rusqlite::Result<T> {
    serde_json::from_str(raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(err))
    })
}

fn status<T: serde::de::DeserializeOwned>(row: &Row<'_>, index: usize) -> rusqlite::Result<T> {
    let raw: String = row.get(index)?;
    parse(&format!("\"{raw}\""), index)
}

fn optional_json(row: &Row<'_>, index: usize) -> rusqlite::Result<Option<Value>> {
    row.get::<_, Option<String>>(index)?
        .map(|raw| parse(&raw, index))
        .transpose()
}

fn positive_u64(row: &Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(rusqlite::Error::IntegralValueOutOfRange(index, value))
}

fn job_row(row: &Row<'_>) -> rusqlite::Result<AutomationJob> {
    Ok(AutomationJob {
        id: row.get(0)?,
        studio_id: row.get(1)?,
        actor_id: row.get(2)?,
        owner_kind: row.get(3)?,
        owner_subject: row.get(4)?,
        schema_version: row.get(5)?,
        config_revision: positive_u64(row, 6)?,
        config_json: row.get(7)?,
        next_run_at: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        deleted_at: row.get(11)?,
    })
}

fn trigger_row(row: &Row<'_>) -> rusqlite::Result<AutomationTrigger> {
    Ok(AutomationTrigger {
        trigger_id: row.get(0)?,
        job_id: row.get(1)?,
        due_at: row.get(2)?,
        config_revision: positive_u64(row, 3)?,
        config_json: row.get(11)?,
        status: status(row, 4)?,
        session_id: row.get(5)?,
        run_id: row.get(6)?,
        result: optional_json(row, 7)?,
        error: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn notification_row(row: &Row<'_>) -> rusqlite::Result<NotificationReceipt> {
    Ok(NotificationReceipt {
        delivery_id: row.get(0)?,
        trigger_id: row.get(1)?,
        run_id: row.get(2)?,
        channel: row.get(3)?,
        recipient: row.get(4)?,
        payload_json: row.get(5)?,
        status: status(row, 6)?,
        platform_receipt: optional_json(row, 7)?,
        error: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn read_job(conn: &Connection, id: &str) -> Result<Option<AutomationJob>, StorageError> {
    conn.query_row(
        "SELECT * FROM automation_jobs_v1 WHERE id=?1",
        [id],
        job_row,
    )
    .optional()
    .map_err(map_rusqlite)
}

pub(crate) fn get_job_conn(
    conn: &Connection,
    id: &str,
) -> Result<Option<AutomationJob>, StorageError> {
    read_job(conn, id)
}

fn read_trigger(conn: &Connection, id: &str) -> Result<Option<AutomationTrigger>, StorageError> {
    conn.query_row(
        "SELECT * FROM automation_triggers_v1 WHERE trigger_id=?1",
        [id],
        trigger_row,
    )
    .optional()
    .map_err(map_rusqlite)
}

fn read_notification(
    conn: &Connection,
    id: &str,
) -> Result<Option<NotificationReceipt>, StorageError> {
    conn.query_row(
        "SELECT * FROM automation_notifications_v1 WHERE delivery_id=?1",
        [id],
        notification_row,
    )
    .optional()
    .map_err(map_rusqlite)
}

pub async fn create_job(
    db: &RunDatabase,
    input: &AutomationJobInput,
    now: i64,
) -> Result<AutomationJob, StorageError> {
    validate_job(input)?;
    let input = input.clone();
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move |conn| with_write_txn(conn,busy,|conn| {
        if read_job(conn,&input.id)?.is_some() { return Err(conflict("job id already exists")); }
        conn.execute("INSERT INTO automation_jobs_v1(id,studio_id,actor_id,owner_kind,owner_subject,schema_version,config_revision,config_json,next_run_at,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,1,?7,?8,?9,?9)",params![input.id,input.studio_id,input.actor_id,input.owner_kind,input.owner_subject,input.schema_version,input.config_json,input.next_run_at,now]).map_err(map_rusqlite)?;
        read_job(conn,&input.id)?.ok_or_else(|| invalid("inserted job missing"))
    })).await
}

pub async fn update_job(
    db: &RunDatabase,
    id: &str,
    input: &AutomationJobInput,
    expected_revision: u64,
    now: i64,
) -> Result<AutomationJob, StorageError> {
    validate_job(input)?;
    if input.id != id {
        return Err(invalid("job id cannot change"));
    }
    let expected = revision(expected_revision)?;
    let next = expected
        .checked_add(1)
        .ok_or_else(|| invalid("revision exhausted"))?;
    let input = input.clone();
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move |conn| with_write_txn(conn,busy,|conn| {
        let old=read_job(conn,&input.id)?.ok_or_else(|| invalid("job not found"))?;
        if old.studio_id!=input.studio_id || old.owner_kind!=input.owner_kind || old.owner_subject!=input.owner_subject {return Err(conflict("job ownership cannot change"));}
        let changed=conn.execute("UPDATE automation_jobs_v1 SET actor_id=?2,schema_version=?3,config_revision=?4,config_json=?5,next_run_at=?6,updated_at=?7 WHERE id=?1 AND config_revision=?8 AND deleted_at IS NULL",params![input.id,input.actor_id,input.schema_version,next,input.config_json,input.next_run_at,now,expected]).map_err(map_rusqlite)?;
        if changed!=1{return Err(conflict("job revision changed or deleted"));}
        read_job(conn,&input.id)?.ok_or_else(|| invalid("updated job missing"))
    })).await
}

pub async fn delete_job(
    db: &RunDatabase,
    id: &str,
    expected_revision: u64,
    now: i64,
) -> Result<AutomationJob, StorageError> {
    let expected = revision(expected_revision)?;
    let next = expected
        .checked_add(1)
        .ok_or_else(|| invalid("revision exhausted"))?;
    let id = id.to_owned();
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move |conn| with_write_txn(conn,busy,|conn| {
        let changed=conn.execute("UPDATE automation_jobs_v1 SET config_revision=?2,next_run_at=NULL,updated_at=?3,deleted_at=?3 WHERE id=?1 AND config_revision=?4 AND deleted_at IS NULL",params![id,next,now,expected]).map_err(map_rusqlite)?;
        if changed!=1{return Err(conflict("job revision changed or deleted"));}
        read_job(conn,&id)?.ok_or_else(|| invalid("deleted job missing"))
    })).await
}

pub async fn get_job(db: &RunDatabase, id: &str) -> Result<Option<AutomationJob>, StorageError> {
    let id = id.to_owned();
    db.queue()
        .submit(move |conn| Ok(read_job(conn, &id)?.filter(|job| job.deleted_at.is_none())))
        .await
}

pub async fn list_jobs(
    db: &RunDatabase,
    studio_id: Option<&str>,
) -> Result<Vec<AutomationJob>, StorageError> {
    let studio = studio_id.map(str::to_owned);
    db.queue().submit(move |conn| {
        let mut stmt=conn.prepare("SELECT * FROM automation_jobs_v1 WHERE deleted_at IS NULL AND (?1 IS NULL OR studio_id=?1) ORDER BY created_at,id").map_err(map_rusqlite)?;
        let rows=stmt.query_map([studio],job_row).map_err(map_rusqlite)?;
        rows.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)
    }).await
}

pub async fn list_due_jobs(
    db: &RunDatabase,
    now: i64,
    limit: usize,
) -> Result<Vec<AutomationJob>, StorageError> {
    let limit = i64::try_from(limit).map_err(|_| invalid("limit too large"))?;
    db.queue().submit(move |conn| {
        let mut stmt=conn.prepare("SELECT j.* FROM automation_jobs_v1 j WHERE j.deleted_at IS NULL AND j.next_run_at<=?1 AND NOT EXISTS(SELECT 1 FROM automation_triggers_v1 t WHERE t.job_id=j.id AND t.config_revision=j.config_revision AND t.due_at=j.next_run_at) ORDER BY j.next_run_at,j.id LIMIT ?2").map_err(map_rusqlite)?;
        let rows=stmt.query_map(params![now,limit],job_row).map_err(map_rusqlite)?;
        rows.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)
    }).await
}

pub async fn claim_trigger(
    db: &RunDatabase,
    input: &TriggerInput,
    now: i64,
) -> Result<ClaimTriggerOutcome, StorageError> {
    require_id(&input.trigger_id, "triggerId")?;
    require_id(&input.job_id, "jobId")?;
    let config_revision = revision(input.config_revision)?;
    let input = input.clone();
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move |conn| with_write_txn(conn,busy,|conn| {
        if let Some(trigger)=read_trigger(conn,&input.trigger_id)? {
            if trigger.job_id!=input.job_id || trigger.due_at!=input.due_at || trigger.config_revision!=input.config_revision{return Err(conflict("trigger id reused for another submission"));}
            return Ok(ClaimTriggerOutcome{created:false,trigger});
        }
        let prior=conn.query_row("SELECT * FROM automation_triggers_v1 WHERE job_id=?1 AND due_at=?2 AND config_revision=?3",params![input.job_id,input.due_at,config_revision],trigger_row).optional().map_err(map_rusqlite)?;
        if let Some(trigger)=prior{return Ok(ClaimTriggerOutcome{created:false,trigger});}
        let job=read_job(conn,&input.job_id)?.ok_or_else(||invalid("job not found"))?;
        if job.deleted_at.is_some() || job.config_revision!=input.config_revision{return Err(conflict("job revision changed or deleted"));}
        conn.execute("INSERT INTO automation_triggers_v1(trigger_id,job_id,due_at,config_revision,status,created_at,updated_at,config_json) VALUES (?1,?2,?3,?4,'claimed',?5,?5,?6)",params![input.trigger_id,input.job_id,input.due_at,config_revision,now,job.config_json]).map_err(map_rusqlite)?;
        let trigger=read_trigger(conn,&input.trigger_id)?.ok_or_else(||invalid("claimed trigger missing"))?;
        Ok(ClaimTriggerOutcome{created:true,trigger})
    })).await
}

pub async fn bind_run(
    db: &RunDatabase,
    trigger_id: &str,
    session_id: &str,
    run_id: &str,
    now: i64,
) -> Result<AutomationTrigger, StorageError> {
    require_id(session_id, "sessionId")?;
    require_id(run_id, "runId")?;
    let (trigger_id, session_id, run_id) = (
        trigger_id.to_owned(),
        session_id.to_owned(),
        run_id.to_owned(),
    );
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move |conn| with_write_txn(conn,busy,|conn| {
        let old=read_trigger(conn,&trigger_id)?.ok_or_else(||invalid("trigger not found"))?;
        if old.run_id.as_deref()==Some(&run_id) && old.session_id.as_deref()==Some(&session_id){return Ok(old);}
        if old.status!=TriggerStatus::Claimed || old.run_id.is_some(){return Err(conflict("trigger already bound or terminal"));}
        let matching:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM runs r JOIN run_lineage l ON l.run_id=r.run_id WHERE r.run_id=?1 AND r.session_id=?2 AND l.cause_id=?3 AND l.origin IN ('cron','heartbeat'))",params![run_id,session_id,trigger_id],|row|row.get(0)).map_err(map_rusqlite)?;
        if !matching{return Err(conflict("run lineage does not match trigger"));}
        conn.execute("UPDATE automation_triggers_v1 SET session_id=?2,run_id=?3,status='running',updated_at=?4 WHERE trigger_id=?1",params![trigger_id,session_id,run_id,now]).map_err(map_rusqlite)?;
        read_trigger(conn,&trigger_id)?.ok_or_else(||invalid("bound trigger missing"))
    })).await
}

/// 按不可变因果身份恢复落盘 run；不按时间或会话相似程度猜测。
pub async fn get_trigger_run(
    db: &RunDatabase,
    trigger_id: &str,
) -> Result<Option<(String, String, String)>, StorageError> {
    let trigger_id = trigger_id.to_owned();
    db.queue().submit(move |conn| {
        let mut stmt=conn.prepare("SELECT r.run_id,r.session_id,r.status FROM runs r JOIN run_lineage l ON l.run_id=r.run_id WHERE l.cause_id=?1 AND l.origin IN ('cron','heartbeat') ORDER BY r.run_id LIMIT 2").map_err(map_rusqlite)?;
        let mut rows=stmt.query_map([trigger_id],|row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).map_err(map_rusqlite)?;
        let first=rows.next().transpose().map_err(map_rusqlite)?;
        if rows.next().transpose().map_err(map_rusqlite)?.is_some(){return Err(conflict("multiple runs have the same trigger cause"));}
        Ok(first)
    }).await
}

#[allow(clippy::too_many_arguments)]
pub async fn settle_trigger(
    db: &RunDatabase,
    trigger_id: &str,
    status: TriggerStatus,
    result: Option<Value>,
    error: Option<String>,
    next_run_at: Option<i64>,
    now: i64,
) -> Result<SettleTriggerOutcome, StorageError> {
    if !status.terminal() {
        return Err(invalid("settlement requires terminal trigger status"));
    }
    let status_wire = wire(status)?;
    let trigger_id = trigger_id.to_owned();
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move |conn| with_write_txn(conn,busy,|conn| {
        let old=read_trigger(conn,&trigger_id)?.ok_or_else(||invalid("trigger not found"))?;
        // 活动超时先请求统一取消；只在实际取消终态保留该原因，完成竞态不能被改成失败。
        let error = error.or_else(|| (status == TriggerStatus::Cancelled && old.error.as_deref() == Some("timeout")).then(|| "timeout".to_owned()));
        if old.status.terminal(){
            if old.status==status && old.result==result && old.error==error{return Ok(SettleTriggerOutcome{trigger:old,cursor_advanced:false});}
            return Err(conflict("trigger already settled differently"));
        }
        conn.execute("UPDATE automation_triggers_v1 SET status=?2,result_json=?3,error=?4,updated_at=?5 WHERE trigger_id=?1",params![trigger_id,status_wire,result.as_ref().map(Value::to_string),error,now]).map_err(map_rusqlite)?;
        // 配置编辑与删除均增代；运行收口不增代，且只能更新领取时的同一配置。
        let mut frozen_config:Value=serde_json::from_str(&old.config_json).map_err(|err|StorageError::Corrupted{detail:format!("automation trigger config JSON: {err}")})?;
        let changed=if status==TriggerStatus::Skipped {
            0
        } else if frozen_config.get("type").and_then(Value::as_str)==Some("at") {
            frozen_config["enabled"]=Value::Bool(false);
            conn.execute("UPDATE automation_jobs_v1 SET config_json=?2,next_run_at=NULL,updated_at=?3 WHERE id=?1 AND config_revision=?4 AND deleted_at IS NULL",params![old.job_id,frozen_config.to_string(),now,revision(old.config_revision)?]).map_err(map_rusqlite)?
        } else {
            conn.execute("UPDATE automation_jobs_v1 SET next_run_at=?2,updated_at=?3 WHERE id=?1 AND config_revision=?4 AND deleted_at IS NULL",params![old.job_id,next_run_at,now,revision(old.config_revision)?]).map_err(map_rusqlite)?
        };
        let trigger=read_trigger(conn,&trigger_id)?.ok_or_else(||invalid("settled trigger missing"))?;
        Ok(SettleTriggerOutcome{trigger,cursor_advanced:changed==1})
    })).await
}

pub async fn get_trigger(
    db: &RunDatabase,
    trigger_id: &str,
) -> Result<Option<AutomationTrigger>, StorageError> {
    let id = trigger_id.to_owned();
    db.queue().submit(move |conn| read_trigger(conn, &id)).await
}

pub async fn list_pending_triggers(
    db: &RunDatabase,
) -> Result<Vec<AutomationTrigger>, StorageError> {
    db.queue().submit(move |conn| {
        let mut stmt=conn.prepare("SELECT * FROM automation_triggers_v1 WHERE status IN ('claimed','running') ORDER BY created_at,trigger_id").map_err(map_rusqlite)?;
        let rows=stmt.query_map([],trigger_row).map_err(map_rusqlite)?;
        rows.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)
    }).await
}

pub async fn list_trigger_history(
    db: &RunDatabase,
    job_id: Option<&str>,
    limit: usize,
) -> Result<Vec<AutomationTrigger>, StorageError> {
    let job = job_id.map(str::to_owned);
    let limit = i64::try_from(limit).map_err(|_| invalid("limit too large"))?;
    db.queue().submit(move |conn| {
        let mut stmt=conn.prepare("SELECT * FROM automation_triggers_v1 WHERE (?1 IS NULL OR job_id=?1) ORDER BY due_at DESC,trigger_id LIMIT ?2").map_err(map_rusqlite)?;
        let rows=stmt.query_map(params![job,limit],trigger_row).map_err(map_rusqlite)?;
        rows.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)
    }).await
}

pub async fn reserve_notification(
    db: &RunDatabase,
    input: &NotificationInput,
    now: i64,
) -> Result<ReserveNotificationOutcome, StorageError> {
    for (value, label) in [
        (&input.delivery_id, "deliveryId"),
        (&input.channel, "channel"),
        (&input.recipient, "recipient"),
    ] {
        require_id(value, label)?;
    }
    if input.trigger_id.is_none() && input.run_id.is_none() {
        return Err(invalid("notification must reference a trigger or run"));
    }
    for value in [&input.trigger_id, &input.run_id].into_iter().flatten() {
        require_id(value, "notification identity")?;
    }
    validate_json(&input.payload_json)?;
    let input = input.clone();
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move |conn|with_write_txn(conn,busy,|conn|{
        if let Some(receipt)=read_notification(conn,&input.delivery_id)?{
            if receipt.trigger_id!=input.trigger_id || receipt.run_id!=input.run_id || receipt.channel!=input.channel || receipt.recipient!=input.recipient || receipt.payload_json!=input.payload_json{return Err(conflict("delivery id reused with different payload or recipient"));}
            return Ok(ReserveNotificationOutcome{reserved:false,receipt});
        }
        if let Some(trigger_id)=&input.trigger_id {
            let trigger=read_trigger(conn,trigger_id)?.ok_or_else(||invalid("notification trigger not found"))?;
            if let Some(run_id)=&input.run_id {
                let matching:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM run_lineage WHERE run_id=?1 AND cause_id=?2 AND origin IN ('cron','heartbeat'))",params![run_id,trigger_id],|row|row.get(0)).map_err(map_rusqlite)?;
                if !matching || trigger.run_id.as_ref().is_some_and(|prior|prior!=run_id){return Err(conflict("notification run does not match trigger"));}
            }
        }
        if let Some(run_id)=&input.run_id {
            let exists:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM runs WHERE run_id=?1)",[run_id],|row|row.get(0)).map_err(map_rusqlite)?;
            if !exists{return Err(invalid("notification run not found"));}
        }
        conn.execute("INSERT INTO automation_notifications_v1(delivery_id,trigger_id,run_id,channel,recipient,payload_json,status,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,'queued',?7,?7)",params![input.delivery_id,input.trigger_id,input.run_id,input.channel,input.recipient,input.payload_json,now]).map_err(map_rusqlite)?;
        let receipt=read_notification(conn,&input.delivery_id)?.ok_or_else(||invalid("reserved notification missing"))?;
        Ok(ReserveNotificationOutcome{reserved:true,receipt})
    })).await
}

pub async fn settle_notification(
    db: &RunDatabase,
    delivery_id: &str,
    status: NotificationStatus,
    platform_receipt: Option<Value>,
    error: Option<String>,
    now: i64,
) -> Result<NotificationReceipt, StorageError> {
    if matches!(
        status,
        NotificationStatus::Queued | NotificationStatus::Started
    ) {
        return Err(invalid("notification settlement requires terminal status"));
    }
    let id = delivery_id.to_owned();
    let status_wire = wire(status)?;
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move |conn|with_write_txn(conn,busy,|conn|{
        let old=read_notification(conn,&id)?.ok_or_else(||invalid("notification not found"))?;
        if old.status==NotificationStatus::Queued{return Err(conflict("notification has not been claimed for delivery"));}
        if old.status!=NotificationStatus::Started{
            if old.status==status && old.platform_receipt==platform_receipt && old.error==error{return Ok(old);}
            return Err(conflict("notification already settled differently"));
        }
        conn.execute("UPDATE automation_notifications_v1 SET status=?2,platform_receipt_json=?3,error=?4,updated_at=?5 WHERE delivery_id=?1",params![id,status_wire,platform_receipt.as_ref().map(Value::to_string),error,now]).map_err(map_rusqlite)?;
        read_notification(conn,&id)?.ok_or_else(||invalid("settled notification missing"))
    })).await
}

pub async fn get_notification(
    db: &RunDatabase,
    delivery_id: &str,
) -> Result<Option<NotificationReceipt>, StorageError> {
    let id = delivery_id.to_owned();
    db.queue()
        .submit(move |conn| read_notification(conn, &id))
        .await
}

pub async fn list_notifications(
    db: &RunDatabase,
    trigger_id: &str,
) -> Result<Vec<NotificationReceipt>, StorageError> {
    let trigger = trigger_id.to_owned();
    db.queue().submit(move |conn| {
        let mut stmt=conn.prepare("SELECT * FROM automation_notifications_v1 WHERE trigger_id=?1 ORDER BY created_at,delivery_id").map_err(map_rusqlite)?;
        let rows=stmt.query_map([trigger],notification_row).map_err(map_rusqlite)?;
        rows.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)
    }).await
}

pub async fn claim_notification(
    db: &RunDatabase,
    delivery_id: &str,
    now: i64,
) -> Result<ReserveNotificationOutcome, StorageError> {
    let id = delivery_id.to_owned();
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move |conn|with_write_txn(conn,busy,|conn|{
        let old=read_notification(conn,&id)?.ok_or_else(||invalid("notification not found"))?;
        if old.status!=NotificationStatus::Queued{return Ok(ReserveNotificationOutcome{reserved:false,receipt:old});}
        conn.execute("UPDATE automation_notifications_v1 SET status='started',updated_at=?2 WHERE delivery_id=?1",params![id,now]).map_err(map_rusqlite)?;
        let receipt=read_notification(conn,&id)?.ok_or_else(||invalid("claimed notification missing"))?;
        Ok(ReserveNotificationOutcome{reserved:true,receipt})
    })).await
}

pub async fn list_pending_notifications(
    db: &RunDatabase,
) -> Result<Vec<NotificationReceipt>, StorageError> {
    db.queue().submit(move |conn|{
        let mut stmt=conn.prepare("SELECT * FROM automation_notifications_v1 WHERE status='queued' ORDER BY created_at,delivery_id").map_err(map_rusqlite)?;
        let rows=stmt.query_map([],notification_row).map_err(map_rusqlite)?;
        rows.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)
    }).await
}

/// 仅由服务重启恢复调用：已经领取但未确认的外发按未知结果封存，不再排队。
pub async fn recover_started_notifications(
    db: &RunDatabase,
    now: i64,
) -> Result<usize, StorageError> {
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move |conn|with_write_txn(conn,busy,|conn|{
        conn.execute("UPDATE automation_notifications_v1 SET status='unknown',error='service restarted before delivery acknowledgement',updated_at=?1 WHERE status='started'",[now]).map_err(map_rusqlite)
    })).await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DailyStatus {
    Started,
    Succeeded,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReceipt {
    pub session_id: String,
    pub logical_date: String,
    pub attempt: u64,
    pub status: DailyStatus,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub retry_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyClaim {
    pub claimed: bool,
    pub receipt: DailyReceipt,
    pub retry_after: Option<i64>,
}

fn daily_row(row: &Row<'_>) -> rusqlite::Result<DailyReceipt> {
    Ok(DailyReceipt {
        session_id: row.get(0)?,
        logical_date: row.get(1)?,
        attempt: positive_u64(row, 2)?,
        status: status(row, 3)?,
        result: optional_json(row, 4)?,
        error: row.get(5)?,
        started_at: row.get(6)?,
        completed_at: row.get(7)?,
        retry_at: row.get(8)?,
    })
}

fn read_daily(
    conn: &Connection,
    session: &str,
    date: &str,
) -> Result<Option<DailyReceipt>, StorageError> {
    conn.query_row(
        "SELECT * FROM automation_daily_receipts_v1 WHERE session_id=?1 AND logical_date=?2",
        params![session, date],
        daily_row,
    )
    .optional()
    .map_err(map_rusqlite)
}

pub async fn get_daily(
    db: &RunDatabase,
    session_id: &str,
    logical_date: &str,
) -> Result<Option<DailyReceipt>, StorageError> {
    let (session, date) = (session_id.to_owned(), logical_date.to_owned());
    db.queue()
        .submit(move |conn| read_daily(conn, &session, &date))
        .await
}

pub async fn claim_daily(
    db: &RunDatabase,
    session_id: &str,
    logical_date: &str,
    now: i64,
) -> Result<DailyClaim, StorageError> {
    require_id(session_id, "sessionId")?;
    if logical_date.len() != 10
        || logical_date.as_bytes().iter().enumerate().any(|(i, b)| {
            if i == 4 || i == 7 {
                *b != b'-'
            } else {
                !b.is_ascii_digit()
            }
        })
    {
        return Err(invalid("logicalDate must be YYYY-MM-DD"));
    }
    let year: u32 = logical_date[..4]
        .parse()
        .map_err(|_| invalid("invalid logicalDate year"))?;
    let month: usize = logical_date[5..7]
        .parse()
        .map_err(|_| invalid("invalid logicalDate month"))?;
    let day: u32 = logical_date[8..]
        .parse()
        .map_err(|_| invalid("invalid logicalDate day"))?;
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if year == 0 || month == 0 || month > 12 || day == 0 || day > days[month - 1] {
        return Err(invalid("logicalDate is not a calendar date"));
    }
    let (session, date) = (session_id.to_owned(), logical_date.to_owned());
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move |conn|with_write_txn(conn,busy,|conn|{
        if let Some(old)=read_daily(conn,&session,&date)?{
            if old.status!=DailyStatus::Failed || old.retry_at.is_none_or(|retry|now<retry){
                let retry_after=if old.status==DailyStatus::Failed{old.retry_at}else{None};
                return Ok(DailyClaim{claimed:false,receipt:old,retry_after});
            }
            let attempt=revision(old.attempt)?.checked_add(1).ok_or_else(||invalid("daily attempt exhausted"))?;
            conn.execute("UPDATE automation_daily_receipts_v1 SET attempt=?3,status='started',result_json=NULL,error=NULL,started_at=?4,completed_at=NULL,retry_at=NULL WHERE session_id=?1 AND logical_date=?2",params![session,date,attempt,now]).map_err(map_rusqlite)?;
        }else{
            conn.execute("INSERT INTO automation_daily_receipts_v1(session_id,logical_date,attempt,status,started_at) VALUES (?1,?2,1,'started',?3)",params![session,date,now]).map_err(map_rusqlite)?;
        }
        let receipt=read_daily(conn,&session,&date)?.ok_or_else(||invalid("daily receipt missing"))?;
        Ok(DailyClaim{claimed:true,receipt,retry_after:None})
    })).await
}

#[allow(clippy::too_many_arguments)]
pub async fn settle_daily(
    db: &RunDatabase,
    session_id: &str,
    logical_date: &str,
    attempt: u64,
    status: DailyStatus,
    result: Option<Value>,
    error: Option<String>,
    now: i64,
) -> Result<DailyReceipt, StorageError> {
    if status == DailyStatus::Started {
        return Err(invalid("daily settlement requires terminal status"));
    }
    let (session, date) = (session_id.to_owned(), logical_date.to_owned());
    let status_wire = wire(status)?;
    let retry_at = if status == DailyStatus::Failed {
        Some(
            now.checked_add(3_600_000)
                .ok_or_else(|| invalid("daily retry timestamp overflow"))?,
        )
    } else {
        None
    };
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move |conn|with_write_txn(conn,busy,|conn|{
        let old=read_daily(conn,&session,&date)?.ok_or_else(||invalid("daily receipt not found"))?;
        if old.attempt!=attempt{return Err(conflict("daily attempt changed"));}
        if old.status!=DailyStatus::Started{
            if old.status==status && old.result==result && old.error==error{return Ok(old);}
            return Err(conflict("daily receipt already settled differently"));
        }
        conn.execute("UPDATE automation_daily_receipts_v1 SET status=?3,result_json=?4,error=?5,completed_at=?6,retry_at=?7 WHERE session_id=?1 AND logical_date=?2",params![session,date,status_wire,result.as_ref().map(Value::to_string),error,now,retry_at]).map_err(map_rusqlite)?;
        read_daily(conn,&session,&date)?.ok_or_else(||invalid("settled daily receipt missing"))
    })).await
}
