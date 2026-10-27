//! 浏览器授权与冷恢复只经现有数据库单写队列持久化。
use super::{migrations::map_rusqlite, run_store::with_write_txn, RunDatabase};
use lingxi_kernel::{browser::BrowserScope, ports::StorageError};
use lingxi_protocol::BrowserWorkspace;
use rusqlite::{params, OptionalExtension};

#[derive(Debug, Clone)]
pub struct BrowserAuthority {
    pub session_id: String,
    pub owner_subject: String,
    pub execution_node: String,
    pub epoch: u64,
    pub revoked: bool,
    pub state: Option<BrowserWorkspace>,
}
fn invalid(message: &str) -> StorageError {
    StorageError::InvalidRequest {
        detail: message.into(),
    }
}
fn conflict(message: &str) -> StorageError {
    StorageError::Conflict {
        detail: message.into(),
    }
}
fn number(value: u64) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| invalid("browser integer overflow"))
}
fn read(conn: &rusqlite::Connection, sid: &str) -> Result<Option<BrowserAuthority>, StorageError> {
    let raw=conn.query_row("SELECT session_id,owner_subject,execution_node,epoch,revoked,state_json FROM browser_workspaces_v1 WHERE session_id=?1",[sid],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,i64>(3)?,r.get::<_,bool>(4)?,r.get::<_,Option<String>>(5)?))).optional().map_err(map_rusqlite)?;
    raw.map(
        |(session_id, owner_subject, execution_node, epoch, revoked, state)| {
            Ok(BrowserAuthority {
                session_id,
                owner_subject,
                execution_node,
                epoch: u64::try_from(epoch).map_err(|_| invalid("invalid browser epoch"))?,
                revoked,
                state: state
                    .map(|s| {
                        serde_json::from_str(&s).map_err(|e| StorageError::Corrupted {
                            detail: format!("browser state: {e}"),
                        })
                    })
                    .transpose()?,
            })
        },
    )
    .transpose()
}
pub async fn get(db: &RunDatabase, sid: &str) -> Result<Option<BrowserAuthority>, StorageError> {
    let sid = sid.to_owned();
    db.queue().submit(move |conn| read(conn, &sid)).await
}
pub async fn list(db: &RunDatabase) -> Result<Vec<BrowserAuthority>, StorageError> {
    db.queue()
        .submit(move |conn| {
            let ids = {
                let mut stmt = conn
                    .prepare("SELECT session_id FROM browser_workspaces_v1 ORDER BY session_id")
                    .map_err(map_rusqlite)?;
                let rows = stmt
                    .query_map([], |r| r.get::<_, String>(0))
                    .map_err(map_rusqlite)?;
                rows.collect::<Result<Vec<_>, _>>().map_err(map_rusqlite)?
            };
            ids.iter()
                .map(|id| read(conn, id)?.ok_or_else(|| invalid("browser row vanished")))
                .collect()
        })
        .await
}
pub async fn ensure(
    db: &RunDatabase,
    sid: &str,
    owner: &str,
    node: &str,
    now: u64,
) -> Result<BrowserAuthority, StorageError> {
    let (sid, owner, node, now, busy) = (
        sid.to_owned(),
        owner.to_owned(),
        node.to_owned(),
        number(now)?,
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
  let session=conn.query_row("SELECT owner_user_id,lifecycle FROM sessions WHERE session_id=?1",[&sid],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).optional().map_err(map_rusqlite)?.ok_or_else(||invalid("browser session missing"))?;
  if session.0!=owner||session.1!="active"{return Err(invalid("browser session ownership or lifecycle changed"));}
  if let Some(row)=read(conn,&sid)? {if row.owner_subject!=owner||row.execution_node!=node{return Err(conflict("browser execution boundary changed"));}return Ok(row);}
  conn.execute("INSERT INTO browser_workspaces_v1(session_id,owner_subject,execution_node,epoch,revoked,updated_at) VALUES(?1,?2,?3,1,0,?4)",params![sid,owner,node,now]).map_err(map_rusqlite)?;
  read(conn,&sid)?.ok_or_else(||invalid("browser authority missing"))
 })).await
}
pub async fn save(
    db: &RunDatabase,
    scope: &lingxi_kernel::browser::BrowserScope,
    state: &BrowserWorkspace,
    now: u64,
) -> Result<(), StorageError> {
    let (scope, state, now, busy) = (
        scope.clone(),
        state.clone(),
        number(now)?,
        db.options().busy_timeout_ms,
    );
    if state.session_id != scope.session_id || state.execution_node != scope.execution_node {
        return Err(invalid("browser state scope mismatch"));
    }
    let json =
        serde_json::to_string(&state).map_err(|_| invalid("browser state encoding failed"))?;
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{let n=conn.execute("UPDATE browser_workspaces_v1 SET state_json=?5,updated_at=?6 WHERE session_id=?1 AND owner_subject=?2 AND execution_node=?3 AND epoch=?4 AND revoked=0",params![scope.session_id,scope.owner_subject,scope.execution_node,number(scope.epoch)?,json,now]).map_err(map_rusqlite)?;if n!=1{return Err(conflict("browser authority changed before state receipt"));}Ok(())})).await
}
pub async fn revoke(
    db: &RunDatabase,
    sid: &str,
    owner: &str,
    now: u64,
) -> Result<Option<BrowserAuthority>, StorageError> {
    let (sid, owner, now, busy) = (
        sid.to_owned(),
        owner.to_owned(),
        number(now)?,
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{let Some(row)=read(conn,&sid)? else{return Ok(None)};if row.owner_subject!=owner{return Err(conflict("browser owner changed"));}
 if row.revoked{return Ok(Some(row));}
 let next=number(row.epoch.checked_add(1).ok_or_else(||invalid("browser epoch exhausted"))?)?;
 conn.execute("UPDATE browser_workspaces_v1 SET epoch=?3,revoked=1,revoked_run_watermark=(SELECT COALESCE(MAX(rowid),0) FROM runs),updated_at=?4 WHERE session_id=?1 AND owner_subject=?2",params![sid,owner,next,now]).map_err(map_rusqlite)?;read(conn,&sid)})).await
}
/// 新人工急停具有新的因果代次；即使已撤权，也必须压过更早捕获授权的插话。
pub async fn revoke_human_intent(
    db: &RunDatabase,
    sid: &str,
    owner: &str,
    operation: &str,
    now: u64,
) -> Result<BrowserAuthority, StorageError> {
    let (sid, owner, operation, now, busy) = (
        sid.to_owned(),
        owner.to_owned(),
        operation.to_owned(),
        number(now)?,
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let pending=conn.query_row("SELECT c.operation_id FROM browser_commands_v1 c JOIN sessions s ON s.session_id=c.session_id WHERE c.operation_id=?1 AND c.session_id=?2 AND c.owner_subject=?3 AND s.owner_user_id=?3 AND c.status='started' AND json_extract(c.input_json,'$.action')='emergency_stop'", params![operation,sid,owner], |row|row.get::<_,String>(0)).optional().map_err(map_rusqlite)?;
        if pending.is_none(){return Err(conflict("browser emergency requires its actual pending human receipt"));}
        let current=read(conn,&sid)?.ok_or_else(||invalid("browser authority missing"))?;
        if current.owner_subject!=owner{return Err(conflict("browser owner changed"));}
        let next=number(current.epoch.checked_add(1).ok_or_else(||invalid("browser epoch exhausted"))?)?;
        conn.execute("UPDATE browser_workspaces_v1 SET epoch=?3,revoked=1,revoked_run_watermark=(SELECT COALESCE(MAX(rowid),0) FROM runs),updated_at=?4 WHERE session_id=?1 AND owner_subject=?2", params![sid,owner,next,now]).map_err(map_rusqlite)?;
        read(conn,&sid)?.ok_or_else(||invalid("browser authority missing after emergency"))
    })).await
}
/// 只有已真正受理的新用户任务能解除急停；模型或旧批准不能解除。
pub async fn release_for_user_run(
    db: &RunDatabase,
    sid: &str,
    run: &str,
    now: u64,
) -> Result<bool, StorageError> {
    let (sid, run, now, busy) = (
        sid.to_owned(),
        run.to_owned(),
        number(now)?,
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
 let actual=conn.query_row("SELECT r.rowid FROM runs r JOIN run_lineage l ON l.run_id=r.run_id JOIN sessions s ON s.session_id=r.session_id WHERE r.run_id=?1 AND r.session_id=?2 AND l.origin='user' AND r.owner_subject=s.owner_user_id AND r.owner_kind IN('local_user','device','web_session') AND s.lifecycle='active'",params![run,sid],|r|r.get::<_,i64>(0)).optional().map_err(map_rusqlite)?;
 let Some(sequence)=actual else{return Ok(false)};
 let Some(row)=read(conn,&sid)? else{return Ok(false)};if !row.revoked{return Ok(false);}
 let next=number(row.epoch.checked_add(1).ok_or_else(||invalid("browser epoch exhausted"))?)?;
 Ok(conn.execute("UPDATE browser_workspaces_v1 SET revoked=0,epoch=?3,updated_at=?4 WHERE session_id=?1 AND revoked=1 AND revoked_run_watermark<?2",params![sid,sequence,next,now]).map_err(map_rusqlite)?==1)
 })).await
}

#[derive(Debug, Clone)]
pub enum CommandClaim {
    Started,
    Confirmed(serde_json::Value),
    Unknown(String),
    Failed(String),
}
pub async fn claim_command(
    db: &RunDatabase,
    scope: &lingxi_kernel::browser::BrowserScope,
    operation: &str,
    input: &serde_json::Value,
    allow_revoked: bool,
    now: u64,
) -> Result<CommandClaim, StorageError> {
    let (scope, operation, input, now, busy) = (
        scope.clone(),
        operation.to_owned(),
        input.to_string(),
        number(now)?,
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
 let old=conn.query_row("SELECT session_id,owner_subject,epoch,input_json,status,result_json,error FROM browser_commands_v1 WHERE operation_id=?1",[&operation],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,Option<String>>(5)?,r.get::<_,Option<String>>(6)?))).optional().map_err(map_rusqlite)?;
 if let Some((sid,owner,_epoch,args,status,result,error))=old {
  if sid!=scope.session_id||owner!=scope.owner_subject||args!=input{return Err(conflict("browser requestId is bound to different input"));}
  return Ok(match status.as_str(){"confirmed"=>CommandClaim::Confirmed(serde_json::from_str(&result.ok_or_else(||invalid("confirmed browser receipt missing"))?).map_err(|_|invalid("browser receipt damaged"))?),"failed"=>CommandClaim::Failed(error.unwrap_or_else(||"browser operation failed".into())),_=>CommandClaim::Unknown(error.unwrap_or_else(||"browser operation may already have started; do not repeat".into()))});
 }
 let current=read(conn,&scope.session_id)?.ok_or_else(||invalid("browser authority missing"))?;
 if (current.revoked && !allow_revoked)||current.epoch!=scope.epoch||current.owner_subject!=scope.owner_subject||current.execution_node!=scope.execution_node{return Err(conflict("browser authority changed before dispatch"));}
 conn.execute("INSERT INTO browser_commands_v1(operation_id,session_id,owner_subject,epoch,input_json,status,updated_at) VALUES(?1,?2,?3,?4,?5,'started',?6)",params![operation,scope.session_id,scope.owner_subject,number(scope.epoch)?,input,now]).map_err(map_rusqlite)?;Ok(CommandClaim::Started)
 })).await
}
pub async fn finish_command(
    db: &RunDatabase,
    operation: &str,
    result: Option<serde_json::Value>,
    error: Option<String>,
    unknown: bool,
    now: u64,
) -> Result<(), StorageError> {
    let (operation, result, now) = (
        operation.to_owned(),
        result.map(|v| v.to_string()),
        number(now)?,
    );
    let status = if result.is_some() {
        "confirmed"
    } else if unknown {
        "unknown"
    } else {
        "failed"
    };
    db.queue().submit(move|conn|{let n=conn.execute("UPDATE browser_commands_v1 SET status=?2,result_json=?3,error=?4,updated_at=?5 WHERE operation_id=?1 AND status='started'",params![operation,status,result,error,now]).map_err(map_rusqlite)?;if n!=1{return Err(conflict("browser receipt already settled"));}Ok(())}).await
}
pub async fn recover_commands(db: &RunDatabase, now: u64) -> Result<(), StorageError> {
    let now = number(now)?;
    db.queue().submit(move|conn|{conn.execute("UPDATE browser_commands_v1 SET status='unknown',error='service stopped before browser receipt; do not repeat',updated_at=?1 WHERE status='started'",[now]).map_err(map_rusqlite)?;Ok(())}).await
}
pub async fn record_download(
    db: &RunDatabase,
    scope: &BrowserScope,
    guid: &str,
    run: Option<&str>,
    file: Option<&str>,
    error: Option<&str>,
    now: u64,
) -> Result<(), StorageError> {
    let (sid, owner, guid, run, file, error, now) = (
        scope.session_id.clone(),
        scope.owner_subject.clone(),
        guid.to_owned(),
        run.map(String::from),
        file.map(String::from),
        error.map(String::from),
        number(now)?,
    );
    db.queue().submit(move|conn|{conn.execute("INSERT INTO browser_downloads_v1(session_id,download_id,owner_subject,source_run_id,status,file_id,error,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(session_id,download_id) DO UPDATE SET status=excluded.status,file_id=excluded.file_id,error=excluded.error,updated_at=excluded.updated_at WHERE browser_downloads_v1.status='unknown' AND excluded.status='completed' AND browser_downloads_v1.owner_subject=excluded.owner_subject AND browser_downloads_v1.source_run_id IS excluded.source_run_id",params![sid,guid,owner,run,if file.is_some(){"completed"}else{"unknown"},file,error,now]).map_err(map_rusqlite)?;Ok(())}).await
}

/// 人工插话已被真实运行接收后才提交授权；旧请求回放不能解除后来发生的急停。
pub async fn accept_human_steer(
    db: &RunDatabase,
    scope: &lingxi_kernel::browser::BrowserScope,
    run: &str,
    operation: &str,
    now: u64,
) -> Result<BrowserAuthority, StorageError> {
    let (scope, run, operation, now, busy) = (
        scope.clone(),
        run.to_owned(),
        operation.to_owned(),
        number(now)?,
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let pending=conn.query_row("SELECT operation_id FROM browser_commands_v1 WHERE operation_id=?1 AND session_id=?2 AND owner_subject=?3 AND epoch=?4 AND status='started' AND json_extract(input_json,'$.action')='human_steer' AND json_extract(input_json,'$.runId')=?5",params![operation,scope.session_id,scope.owner_subject,number(scope.epoch)?,run],|r|r.get::<_,String>(0)).optional().map_err(map_rusqlite)?;
        if pending.is_none(){return Err(conflict("browser steering requires its exact pending receipt"));}
        let actual=conn.query_row("SELECT r.run_id FROM runs r JOIN run_lineage l ON l.run_id=r.run_id JOIN sessions s ON s.session_id=r.session_id WHERE r.run_id=?1 AND r.session_id=?2 AND r.status IN ('running','waiting_approval') AND l.origin='user' AND l.parent_run_id IS NULL AND r.owner_subject=s.owner_user_id AND r.owner_kind IN('local_user','device','web_session') AND s.owner_user_id=?3 AND s.lifecycle='active'",params![run,scope.session_id,scope.owner_subject],|r|r.get::<_,String>(0)).optional().map_err(map_rusqlite)?;
        if actual.is_none(){return Err(conflict("accepted browser steering no longer names an active user root Run"));}
        let current=read(conn,&scope.session_id)?.ok_or_else(||invalid("browser authority missing"))?;
        if current.owner_subject!=scope.owner_subject||current.execution_node!=scope.execution_node{return Err(conflict("browser steering ownership changed"));}
        let grant=current.epoch==scope.epoch;
        if grant&&current.revoked{
            let next=number(current.epoch.checked_add(1).ok_or_else(||invalid("browser epoch exhausted"))?)?;
            conn.execute("UPDATE browser_workspaces_v1 SET revoked=0,epoch=?2,updated_at=?3 WHERE session_id=?1",params![scope.session_id,next,now]).map_err(map_rusqlite)?;
        }
        let row=read(conn,&scope.session_id)?.ok_or_else(||invalid("browser authority missing"))?;
        let result=serde_json::json!({"accepted":true,"runId":run,"epoch":row.epoch,"authorityGranted":grant});
        let n=conn.execute("UPDATE browser_commands_v1 SET status='confirmed',result_json=?2,updated_at=?3 WHERE operation_id=?1 AND status='started'",params![operation,result.to_string(),now]).map_err(map_rusqlite)?;
        if n!=1{return Err(conflict("browser steering receipt already settled"));}
        Ok(row)
    })).await
}

/// 撤权后只接受相同代次的回收事实；不能借正常save覆盖rev1，或覆盖更新的人工授权。
pub async fn save_revoked_state(
    db: &RunDatabase,
    scope: &lingxi_kernel::browser::BrowserScope,
    state: &BrowserWorkspace,
    now: u64,
) -> Result<(), StorageError> {
    if state.session_id != scope.session_id
        || state.execution_node != scope.execution_node
        || !state.revoked
    {
        return Err(invalid("revoked browser state scope mismatch"));
    }
    let (scope, json, now, busy) = (
        scope.clone(),
        serde_json::to_string(state).map_err(|_| invalid("browser state encoding failed"))?,
        number(now)?,
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let n=conn.execute("UPDATE browser_workspaces_v1 SET state_json=?5,updated_at=?6 WHERE session_id=?1 AND owner_subject=?2 AND execution_node=?3 AND epoch=?4 AND revoked=1",params![scope.session_id,scope.owner_subject,scope.execution_node,number(scope.epoch)?,json,now]).map_err(map_rusqlite)?;
        if n!=1{return Err(conflict("browser revoke receipt belongs to a different authority epoch"));}Ok(())
    })).await
}
