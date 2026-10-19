//! 用户明确发起的封面后台作业。原始写入版本和付费阶段持久保存，重启不盲目重投。
use super::{migrations::map_rusqlite, run_store::with_write_txn, RunDatabase};
use lingxi_kernel::ports::StorageError;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CoverPhase {
    Queued,
    Starting,
    Prompting,
    PromptReady,
    MediaPreparing,
    MediaPending,
    Applying,
    Completed,
    Failed,
    Cancelled,
    Unknown,
}
impl CoverPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Starting => "starting",
            Self::Prompting => "prompting",
            Self::PromptReady => "prompt_ready",
            Self::MediaPreparing => "media_preparing",
            Self::MediaPending => "media_pending",
            Self::Applying => "applying",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
        }
    }
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Unknown
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverJob {
    pub id: String,
    pub request_id: String,
    pub principal_id: String,
    pub principal_kind: String,
    pub principal_subject: String,
    pub owner_subject: String,
    pub agent_id: String,
    pub session_id: String,
    pub run_id: Option<String>,
    pub input_digest: String,
    pub input: Value,
    /// 仅由认证入口保存，不回给客户端；WebSession 内部号单独冻结以支持撤销复核。
    pub principal: Value,
    pub web_session_id: Option<String>,
    pub frozen_write: Value,
    pub admitted_write: Option<Value>,
    pub phase: CoverPhase,
    pub state: Value,
    pub revision: u64,
    pub created_at: u64,
    pub updated_at: u64,
}
#[derive(Debug)]
pub enum CoverClaim {
    New(CoverJob),
    Existing(CoverJob),
}
fn invalid(s: impl Into<String>) -> StorageError {
    StorageError::InvalidRequest { detail: s.into() }
}
fn conflict(s: &str) -> StorageError {
    StorageError::Conflict { detail: s.into() }
}
fn integer(n: u64) -> Result<i64, StorageError> {
    i64::try_from(n).map_err(|_| invalid("cover integer overflow"))
}
fn read(c: &rusqlite::Connection, id: &str) -> Result<Option<CoverJob>, StorageError> {
    let text = c
        .query_row(
            "SELECT job_json FROM beautify_jobs_v1 WHERE job_id=?1",
            [id],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(map_rusqlite)?;
    text.map(|v| {
        serde_json::from_str(&v).map_err(|_| StorageError::Corrupted {
            detail: "cover receipt is damaged".into(),
        })
    })
    .transpose()
}
pub async fn get(db: &RunDatabase, id: &str) -> Result<Option<CoverJob>, StorageError> {
    let id = id.to_owned();
    db.queue().submit(move |c| read(c, &id)).await
}
pub async fn get_for_run(db: &RunDatabase, run: &str) -> Result<Option<CoverJob>, StorageError> {
    let run = run.to_owned();
    db.queue()
        .submit(move |c| {
            let id = c
                .query_row(
                    "SELECT job_id FROM beautify_jobs_v1 WHERE run_id=?1",
                    [run],
                    |r| r.get::<_, String>(0),
                )
                .optional()
                .map_err(map_rusqlite)?;
            id.map(|id| read(c, &id)?.ok_or_else(|| invalid("cover receipt disappeared")))
                .transpose()
        })
        .await
}
pub async fn claim(db: &RunDatabase, mut job: CoverJob) -> Result<CoverClaim, StorageError> {
    if job.id.is_empty()
        || job.request_id.is_empty()
        || job.request_id.len() > 1024
        || job.input_digest.len() != 64
        || job.phase != CoverPhase::Queued
        || job.run_id.is_some()
        || job.admitted_write.is_some()
        || job.principal_id.is_empty()
        || job.session_id.is_empty()
        || job.owner_subject.is_empty()
        || job.input.to_string().len() > 128 * 1024
        || job.frozen_write.to_string().len() > 32 * 1024
        || !matches!(
            job.principal_kind.as_str(),
            "local_user" | "device" | "web_session"
        )
    {
        return Err(invalid("invalid cover claim"));
    }
    job.revision = 1;
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move|c|with_write_txn(c,busy,|c|{
  if let Some(old)=read(c,&job.id)? { if old.input_digest!=job.input_digest||old.principal_id!=job.principal_id||old.request_id!=job.request_id||old.input!=job.input||old.owner_subject!=job.owner_subject||old.agent_id!=job.agent_id {return Err(conflict("cover requestId is bound to different input"));} return Ok(CoverClaim::Existing(old)); }
  c.execute("INSERT INTO beautify_jobs_v1(job_id,principal_id,request_id,session_id,run_id,phase,revision,job_json,created_at,updated_at) VALUES(?1,?2,?3,?4,NULL,'queued',1,?5,?6,?7)",params![job.id,job.principal_id,job.request_id,job.session_id,serde_json::to_string(&job).map_err(|_|invalid("cover serialization failed"))?,integer(job.created_at)?,integer(job.updated_at)?]).map_err(map_rusqlite)?;
  Ok(CoverClaim::New(job.clone()))
 })).await
}
fn save(c: &rusqlite::Connection, old: &CoverJob, new: &CoverJob) -> Result<(), StorageError> {
    let n=c.execute("UPDATE beautify_jobs_v1 SET run_id=?2,phase=?3,revision=?4,job_json=?5,updated_at=?6 WHERE job_id=?1 AND revision=?7 AND phase=?8",params![old.id,new.run_id,new.phase.as_str(),integer(new.revision)?,serde_json::to_string(new).map_err(|_|invalid("cover serialization failed"))?,integer(new.updated_at)?,integer(old.revision)?,old.phase.as_str()]).map_err(map_rusqlite)?;
    if n != 1 {
        return Err(conflict("cover receipt changed concurrently"));
    }
    Ok(())
}
pub async fn transition(
    db: &RunDatabase,
    old: &CoverJob,
    phase: CoverPhase,
    state: Value,
    now: u64,
) -> Result<CoverJob, StorageError> {
    if old.phase.terminal() || state.to_string().len() > 128 * 1024 {
        return Err(conflict("cover receipt is terminal or exceeds budget"));
    }
    let allowed = phase.terminal()
        || matches!(
            (old.phase, phase),
            (CoverPhase::Queued, CoverPhase::Starting)
                | (CoverPhase::Starting, CoverPhase::Prompting)
                | (CoverPhase::Prompting, CoverPhase::PromptReady)
                | (CoverPhase::PromptReady, CoverPhase::MediaPreparing)
                | (CoverPhase::MediaPreparing, CoverPhase::MediaPending)
                | (CoverPhase::MediaPending, CoverPhase::MediaPending)
                | (CoverPhase::MediaPending, CoverPhase::Applying)
        );
    if !allowed {
        return Err(conflict("invalid cover phase transition"));
    }
    let old = old.clone();
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |c| {
            with_write_txn(c, busy, |c| {
                let mut new = old.clone();
                new.phase = phase;
                new.state = state.clone();
                new.revision = new
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| invalid("cover revision overflow"))?;
                new.updated_at = now;
                save(c, &old, &new)?;
                Ok(new)
            })
        })
        .await
}
pub async fn bind_run(
    db: &RunDatabase,
    old: &CoverJob,
    run: &str,
    admitted_write: Value,
    now: u64,
) -> Result<CoverJob, StorageError> {
    if old.phase != CoverPhase::Starting
        || old.run_id.is_some()
        || admitted_write.to_string().len() > 32 * 1024
    {
        return Err(conflict("cover Run already bound or wrong phase"));
    }
    let old = old.clone();
    let run = run.to_owned();
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move|c|with_write_txn(c,busy,|c|{
 let valid=c.query_row("SELECT r.run_id FROM runs r JOIN sessions s ON s.session_id=r.session_id JOIN run_lineage l ON l.run_id=r.run_id WHERE r.run_id=?1 AND r.session_id=?2 AND r.owner_kind=?3 AND r.owner_subject=?4 AND s.owner_user_id=?5 AND s.agent_id=?6 AND s.lifecycle='active' AND r.status='running' AND l.origin='beautify' AND l.parent_run_id IS NULL AND l.cause_id=?7",params![run,old.session_id,old.principal_kind,old.principal_subject,old.owner_subject,old.agent_id,old.id],|r|r.get::<_,String>(0)).optional().map_err(map_rusqlite)?;
 if valid.is_none(){return Err(conflict("cover execution requires matching actual Run and authenticated source"));}
 let mut new=old.clone();new.run_id=Some(run.clone());new.admitted_write=Some(admitted_write.clone());new.revision+=1;new.updated_at=now;save(c,&old,&new)?;Ok(new)
 })).await
}
/// 宿主批准只在真实调用已准备、且仍为同一 CoverRun 时持久登记。
/// 封面授权登记的完整参数（拆开传参会越过 clippy 的 7 参上限）。
pub struct CoverAuthorization<'a> {
    pub job_id: &'a str,
    pub run: &'a str,
    pub call: &'a str,
    pub target: &'a str,
    pub digest: &'a str,
    pub binding: Value,
    pub now: u64,
}
pub async fn record_authorization(
    db: &RunDatabase,
    authorization: CoverAuthorization<'_>,
) -> Result<(), StorageError> {
    let CoverAuthorization {
        job_id,
        run,
        call,
        target,
        digest,
        binding,
        now,
    } = authorization;
    if binding.to_string().len() > 64 * 1024 {
        return Err(invalid("cover approval binding exceeds budget"));
    }
    let (id, run, call, target, digest) = (
        job_id.to_owned(),
        run.to_owned(),
        call.to_owned(),
        target.to_owned(),
        digest.to_owned(),
    );
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move|c|with_write_txn(c,busy,|c|{
  let j=read(c,&id)?.ok_or_else(||conflict("cover job missing"))?;
  let phase=match target.as_str(){"tool:plugin:media:generate-image"=>CoverPhase::MediaPreparing,"tool:plugin:beautify:create-cover"=>CoverPhase::Applying,_=>return Err(conflict("cover host approval target is not allowed"))};
  if j.phase!=phase||j.run_id.as_deref()!=Some(&run)||j.admitted_write.is_none(){return Err(conflict("cover approval phase or actual Run mismatch"));}
  let live=c.query_row("SELECT r.run_id FROM runs r JOIN sessions s ON s.session_id=r.session_id JOIN invocation_journal i ON i.run_id=r.run_id AND i.journal_id=?2 WHERE r.run_id=?1 AND r.session_id=?3 AND r.owner_kind=?4 AND r.owner_subject=?5 AND r.status='running' AND s.lifecycle='active' AND s.owner_user_id=?6 AND s.agent_id=?7 AND i.phase='prepared' AND i.target=?8 AND i.args_digest=?9 AND i.generation=r.generation AND i.attempt=(SELECT attempt FROM run_attempts WHERE run_id=r.run_id ORDER BY started_at_unix_ms DESC,rowid DESC LIMIT 1)",params![run,call,j.session_id,j.principal_kind,j.principal_subject,j.owner_subject,j.agent_id,target,digest],|r|r.get::<_,String>(0)).optional().map_err(map_rusqlite)?;
  if live.is_none(){return Err(conflict("cover approval has no current prepared invocation"));}
  let old=c.query_row("SELECT binding_json FROM beautify_authorizations_v1 WHERE job_id=?1 AND call_id=?2",params![id,call],|r|r.get::<_,String>(0)).optional().map_err(map_rusqlite)?;
  let text=binding.to_string();if let Some(old)=old{if old!=text{return Err(conflict("cover approval binding changed"));}return Ok(());}
  c.execute("INSERT INTO beautify_authorizations_v1(job_id,call_id,run_id,target,args_digest,binding_json,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![id,call,run,target,digest,text,integer(now)?]).map_err(map_rusqlite)?;Ok(())
 })).await
}
pub async fn list_open(db: &RunDatabase) -> Result<Vec<CoverJob>, StorageError> {
    db.queue().submit(move|c|{let ids={let mut s=c.prepare("SELECT job_id FROM beautify_jobs_v1 WHERE phase NOT IN('completed','failed','cancelled','unknown') ORDER BY created_at,job_id LIMIT 257").map_err(map_rusqlite)?;let rows=s.query_map([],|r|r.get::<_,String>(0)).map_err(map_rusqlite)?;rows.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)?};if ids.len()>256{return Err(invalid("cover recovery budget exceeded"));}ids.iter().map(|id|read(c,id)?.ok_or_else(||invalid("cover job disappeared"))).collect()}).await
}
