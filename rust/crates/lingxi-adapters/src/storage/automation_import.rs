//! 旧自动任务单次导入：保留旧版本与时间；任何已有身份（包括墓碑）都不能被覆盖。
use super::{
    automation::{get_job_conn, validate_job, AutomationJob, AutomationJobInput},
    migrations::map_rusqlite,
    run_store::with_write_txn,
    RunDatabase,
};
use lingxi_kernel::ports::StorageError;
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportJobOutcome {
    pub imported: bool,
    pub job: AutomationJob,
}

pub async fn import_job(
    db: &RunDatabase,
    input: &AutomationJobInput,
    config_revision: u64,
    created_at: i64,
    updated_at: i64,
) -> Result<ImportJobOutcome, StorageError> {
    validate_job(input)?;
    if input.owner_kind != "local_user" || input.owner_subject != lingxi_kernel::LOCAL_OWNER_SUBJECT
    {
        return Err(StorageError::InvalidRequest {
            detail: "legacy automation import requires the local owner".into(),
        });
    }
    let revision = i64::try_from(config_revision)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| StorageError::InvalidRequest {
            detail: "invalid imported automation revision".into(),
        })?;
    let input = input.clone();
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        if let Some(job)=get_job_conn(conn,&input.id)?{return Ok(ImportJobOutcome{imported:false,job});}
        conn.execute("INSERT INTO automation_jobs_v1(id,studio_id,actor_id,owner_kind,owner_subject,schema_version,config_revision,config_json,next_run_at,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![input.id,input.studio_id,input.actor_id,input.owner_kind,input.owner_subject,input.schema_version,revision,input.config_json,input.next_run_at,created_at,updated_at]).map_err(map_rusqlite)?;
        let job=get_job_conn(conn,&input.id)?.ok_or_else(||StorageError::Corrupted{detail:"imported automation row missing".into()})?;
        Ok(ImportJobOutcome{imported:true,job})
    })).await
}
