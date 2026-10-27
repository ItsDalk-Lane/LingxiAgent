//! 子线程与模型历史使用现有单写队列；线程归属、当前任务和关闭状态均为持久事实。
use super::{
    migrations::map_rusqlite,
    run_store::{with_write_txn, RunDatabase},
};
use lingxi_kernel::{ports::StorageError, RunContext};
use lingxi_protocol::canon;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

fn invalid(detail: impl Into<String>) -> StorageError {
    StorageError::InvalidRequest {
        detail: format!("subagent_thread: {}", detail.into()),
    }
}
fn decode(raw: &str) -> Result<Value, StorageError> {
    serde_json::from_str(raw).map_err(|e| StorageError::Corrupted {
        detail: format!("subagent_thread: {e}"),
    })
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, StorageError> {
    v[key]
        .as_str()
        .ok_or_else(|| invalid(format!("missing {key}")))
}

pub async fn initialize(db: &RunDatabase) -> Result<(), StorageError> {
    db.queue().submit(|conn| {
        conn.execute_batch("CREATE TABLE IF NOT EXISTS subagent_threads_v1 (
            thread_id TEXT PRIMARY KEY, parent_session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
            owner_kind TEXT NOT NULL, owner_subject TEXT NOT NULL, agent_id TEXT NOT NULL,
            state_json TEXT NOT NULL
        ); CREATE TABLE IF NOT EXISTS subagent_history_v1 (
            run_id TEXT PRIMARY KEY, thread_id TEXT NOT NULL REFERENCES subagent_threads_v1(thread_id) ON DELETE CASCADE,
            parent_run_id TEXT NOT NULL REFERENCES runs(run_id) ON DELETE CASCADE, input TEXT NOT NULL,
            history_json TEXT NOT NULL, history_digest TEXT NOT NULL,
            replaces_history INTEGER NOT NULL DEFAULT 0, sequence INTEGER NOT NULL
        ); CREATE INDEX IF NOT EXISTS subagent_history_thread_v1 ON subagent_history_v1(thread_id,sequence);")
            .map_err(map_rusqlite)
    }).await
}

pub async fn read_thread(db: &RunDatabase, id: &str) -> Result<Option<Value>, StorageError> {
    let id = id.to_string();
    db.queue()
        .submit(move |conn| {
            let raw: Option<String> = conn
                .query_row(
                    "SELECT state_json FROM subagent_threads_v1 WHERE thread_id=?1",
                    [id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(map_rusqlite)?;
            raw.map(|s| decode(&s)).transpose()
        })
        .await
}

/// 先持久预留线程，再启动子任务；同线程并发续接和关闭在同一事务内拒绝。
pub async fn reserve(
    db: &RunDatabase,
    mut state: Value,
    parent_run: &str,
    input: &str,
    existing: bool,
) -> Result<Option<Value>, StorageError> {
    let parent_run = parent_run.to_string();
    let input = input.to_string();
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move |conn| with_write_txn(conn,busy,|conn| {
        let id = text(&state, "threadId")?.to_string();
        let session = text(&state, "parentSessionId")?.to_string();
        let kind = text(&state, "ownerKind")?.to_string();
        let subject = text(&state, "ownerSubject")?.to_string();
        let agent = text(&state, "agentId")?.to_string();
        let run = text(&state, "childRunId")?.to_string();
        let actual: Option<String>=conn.query_row("SELECT run_id FROM runs WHERE run_id=?1 AND session_id=?2 AND owner_kind=?3 AND owner_subject=?4",params![parent_run,session,kind,subject],|row|row.get(0)).optional().map_err(map_rusqlite)?;
        if actual.is_none(){return Err(invalid("parent ownership mismatch"));}
        let old: Option<String>=conn.query_row("SELECT state_json FROM subagent_threads_v1 WHERE thread_id=?1",[&id],|row|row.get(0)).optional().map_err(map_rusqlite)?;
        let old=old.map(|s|decode(&s)).transpose()?;
        if existing {
            let prior=old.as_ref().ok_or_else(||invalid("thread not found"))?;
            if prior["parentSessionId"]!=session || prior["ownerKind"]!=kind || prior["ownerSubject"]!=subject || prior["agentId"]!=agent {return Err(invalid("thread ownership mismatch"));}
            if prior["streamStatus"]!="open" || prior["busy"]!=false {return Err(invalid("thread not open or busy"));}
            if state["label"].is_null(){state["label"]=prior["label"].clone();}
            conn.execute("UPDATE subagent_threads_v1 SET state_json=?2 WHERE thread_id=?1",params![id,state.to_string()]).map_err(map_rusqlite)?;
        } else {
            conn.execute("INSERT INTO subagent_threads_v1(thread_id,parent_session_id,owner_kind,owner_subject,agent_id,state_json) VALUES (?1,?2,?3,?4,?5,?6)",params![id,session,kind,subject,agent,state.to_string()]).map_err(map_rusqlite)?;
        }
        let history=json!([{"kind":"user","text":input}]);
        conn.execute("INSERT INTO subagent_history_v1(run_id,thread_id,parent_run_id,input,history_json,history_digest,sequence) VALUES (?1,?2,?3,?4,?5,?6,(SELECT COALESCE(MAX(sequence),0)+1 FROM subagent_history_v1 WHERE thread_id=?2))",params![run,id,parent_run,input,history.to_string(),canon::sha256_hex(&canon::canonical_json_bytes(&history))]).map_err(map_rusqlite)?;
        Ok(old)
    })).await
}

pub async fn rollback(
    db: &RunDatabase,
    thread: &str,
    run: &str,
    previous: Option<Value>,
) -> Result<(), StorageError> {
    let thread = thread.to_string();
    let run = run.to_string();
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                conn.execute(
                    "DELETE FROM subagent_history_v1 WHERE run_id=?1 AND thread_id=?2",
                    params![run, thread],
                )
                .map_err(map_rusqlite)?;
                match previous {
                    Some(state) => {
                        conn.execute(
                            "UPDATE subagent_threads_v1 SET state_json=?2 WHERE thread_id=?1",
                            params![thread, state.to_string()],
                        )
                        .map_err(map_rusqlite)?;
                    }
                    None => {
                        conn.execute(
                            "DELETE FROM subagent_threads_v1 WHERE thread_id=?1",
                            [thread],
                        )
                        .map_err(map_rusqlite)?;
                    }
                }
                Ok(())
            })
        })
        .await
}

pub async fn close(
    db: &RunDatabase,
    id: &str,
    session: &str,
    kind: &str,
    subject: &str,
    reason: &str,
) -> Result<Value, StorageError> {
    let (id, session, kind, subject, reason) = (
        id.to_string(),
        session.to_string(),
        kind.to_string(),
        subject.to_string(),
        reason.to_string(),
    );
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                let raw: String = conn
                    .query_row(
                        "SELECT state_json FROM subagent_threads_v1 WHERE thread_id=?1",
                        [&id],
                        |row| row.get(0),
                    )
                    .map_err(map_rusqlite)?;
                let mut state = decode(&raw)?;
                if state["parentSessionId"] != session
                    || state["ownerKind"] != kind
                    || state["ownerSubject"] != subject
                {
                    return Err(invalid("thread ownership mismatch"));
                }
                if state["streamStatus"] != "open" || state["busy"] != false {
                    return Err(invalid("thread not open or busy"));
                }
                state["streamStatus"] = json!("closed");
                if !reason.trim().is_empty() {
                    state["lastRunStatus"] = json!(format!("closed: {}", reason.trim()));
                }
                conn.execute(
                    "UPDATE subagent_threads_v1 SET state_json=?2 WHERE thread_id=?1",
                    params![id, state.to_string()],
                )
                .map_err(map_rusqlite)?;
                Ok(state)
            })
        })
        .await
}

pub async fn finish(
    db: &RunDatabase,
    thread: &str,
    run: &str,
    status: &str,
    result: &str,
    error: Option<&str>,
) -> Result<(), StorageError> {
    let (thread, run, status, result, error) = (
        thread.to_string(),
        run.to_string(),
        status.to_string(),
        result.to_string(),
        error.map(str::to_string),
    );
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                let raw: String = conn
                    .query_row(
                        "SELECT state_json FROM subagent_threads_v1 WHERE thread_id=?1",
                        [&thread],
                        |row| row.get(0),
                    )
                    .map_err(map_rusqlite)?;
                let mut state = decode(&raw)?;
                if state["childRunId"] != run {
                    return Err(invalid("superseded child completion"));
                }
                state["busy"] = json!(false);
                state["lastRunStatus"] = json!(status);
                state["result"] = json!(result);
                state["error"] = json!(error);
                conn.execute(
                    "UPDATE subagent_threads_v1 SET state_json=?2 WHERE thread_id=?1",
                    params![thread, state.to_string()],
                )
                .map_err(map_rusqlite)?;
                Ok(())
            })
        })
        .await
}

fn context_thread(
    conn: &rusqlite::Connection,
    ctx: &RunContext,
    agent: &str,
) -> Result<String, StorageError> {
    conn.query_row("SELECT t.thread_id FROM subagent_threads_v1 t JOIN subagent_history_v1 h ON h.thread_id=t.thread_id JOIN runs r ON r.run_id=h.run_id JOIN run_attempts a ON a.run_id=r.run_id AND a.attempt=?5 WHERE h.run_id=?1 AND t.parent_session_id=?2 AND t.owner_kind=?3 AND t.owner_subject=?4 AND t.agent_id=?6 AND r.session_id=t.parent_session_id AND r.owner_kind=t.owner_kind AND r.owner_subject=t.owner_subject AND r.generation=?7 AND a.generation=?7 AND r.status IN ('running','waiting_approval') AND a.attempt=(SELECT attempt FROM run_attempts WHERE run_id=r.run_id ORDER BY started_at_unix_ms DESC,rowid DESC LIMIT 1) AND json_extract(t.state_json,'$.childRunId')=r.run_id AND json_extract(t.state_json,'$.busy')=1 AND json_extract(t.state_json,'$.streamStatus')='open'",params![ctx.run_id.as_str(),ctx.session_id.as_str(),ctx.principal.storage_kind(),ctx.principal.storage_subject(),ctx.attempt.as_str(),agent,ctx.generation as i64],|row|row.get(0)).optional().map_err(map_rusqlite)?.ok_or_else(||invalid("history context/actor/agent/run/generation mismatch"))
}

pub async fn load_history(
    db: &RunDatabase,
    ctx: &RunContext,
    agent: &str,
) -> Result<Vec<Value>, StorageError> {
    let ctx = ctx.clone();
    let agent = agent.to_string();
    db.queue().submit(move |conn|{
        let thread=context_thread(conn,&ctx,&agent)?;
        let mut stmt=conn.prepare("SELECT h.history_json,h.history_digest,h.replaces_history,h.run_id,r.owner_kind,r.owner_subject,r.session_id FROM subagent_history_v1 h JOIN runs r ON r.run_id=h.run_id WHERE h.thread_id=?1 AND h.run_id<>?2 ORDER BY h.sequence").map_err(map_rusqlite)?;
        let rows=stmt.query_map(params![thread,ctx.run_id.as_str()],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,bool>(2)?,row.get::<_,String>(3)?,row.get::<_,String>(4)?,row.get::<_,String>(5)?,row.get::<_,String>(6)?))).map_err(map_rusqlite)?;
        let mut out=Vec::new();
        for row in rows {
            let (raw,digest,replace,_run,kind,subject,session)=row.map_err(map_rusqlite)?;
            if kind!=ctx.principal.storage_kind() || subject!=ctx.principal.storage_subject() || session!=ctx.session_id.as_str(){return Err(invalid("historical run ownership mismatch"));}
            let body=decode(&raw)?;
            if canon::sha256_hex(&canon::canonical_json_bytes(&body))!=digest{return Err(StorageError::Corrupted{detail:"subagent_thread history digest mismatch".into()});}
            if replace{out.clear();}
            out.extend(body.as_array().ok_or_else(||invalid("history is not array"))?.iter().cloned());
        }
        Ok(out)
    }).await
}

pub async fn record_history(
    db: &RunDatabase,
    ctx: &RunContext,
    agent: &str,
    items: Vec<Value>,
    replace: bool,
) -> Result<(), StorageError> {
    let ctx = ctx.clone();
    let agent = agent.to_string();
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move |conn|with_write_txn(conn,busy,|conn|{
        context_thread(conn,&ctx,&agent)?;
        let raw:String=conn.query_row("SELECT history_json FROM subagent_history_v1 WHERE run_id=?1",[ctx.run_id.as_str()],|row|row.get(0)).map_err(map_rusqlite)?;
        let mut history=if replace{Vec::new()}else{decode(&raw)?.as_array().cloned().ok_or_else(||invalid("history is not array"))?};
        history.extend(items);let history=json!(history);
        conn.execute("UPDATE subagent_history_v1 SET history_json=?2,history_digest=?3,replaces_history=MAX(replaces_history,?4) WHERE run_id=?1",params![ctx.run_id.as_str(),history.to_string(),canon::sha256_hex(&canon::canonical_json_bytes(&history)),replace as i64]).map_err(map_rusqlite)?;
        Ok(())
    })).await
}

/// 进度取自实际Run状态和已提交调用事件，不能以固定百分比冒充。
pub async fn progress(
    db: &RunDatabase,
    session: &str,
    kind: &str,
    subject: &str,
) -> Result<Vec<Value>, StorageError> {
    let (session, kind, subject) = (session.to_string(), kind.to_string(), subject.to_string());
    db.queue().submit(move |conn|{
        let mut stmt=conn.prepare("SELECT t.state_json,r.status,r.terminal_reason,(SELECT COUNT(*) FROM key_events e WHERE e.run_id=h.run_id AND e.event_type='model_call_completed'),(SELECT COUNT(*) FROM key_events e WHERE e.run_id=h.run_id AND e.event_type='tool_call_completed') FROM subagent_threads_v1 t JOIN subagent_history_v1 h ON h.run_id=json_extract(t.state_json,'$.childRunId') LEFT JOIN runs r ON r.run_id=h.run_id WHERE t.parent_session_id=?1 AND t.owner_kind=?2 AND t.owner_subject=?3 ORDER BY t.thread_id").map_err(map_rusqlite)?;
        let rows=stmt.query_map(params![session,kind,subject],|row|Ok((row.get::<_,String>(0)?,row.get::<_,Option<String>>(1)?,row.get::<_,Option<String>>(2)?,row.get::<_,i64>(3)?,row.get::<_,i64>(4)?))).map_err(map_rusqlite)?;
        let mut out=Vec::new();
        for row in rows {let(raw,status,reason,models,tools)=row.map_err(map_rusqlite)?;let state=decode(&raw)?;out.push(json!({"taskId":state["childRunId"],"threadId":state["threadId"],"status":status.unwrap_or_else(||"queued".into()),"streamStatus":state["streamStatus"],"terminalReason":reason,"modelCallsCompleted":models,"toolCallsCompleted":tools,"result":state["result"],"error":state["error"]}));}
        Ok(out)
    }).await
}

/// 后台任务按不可复用的真实子 Run 定位，旧任务不能误指向线程的新一轮。
pub async fn background_tasks(
    db: &RunDatabase,
    session: Option<&str>,
    task: Option<&str>,
) -> Result<Vec<Value>, StorageError> {
    let session = session.map(str::to_string);
    let task = task.map(str::to_string);
    db.queue().submit(move |conn| {
        let mut statement = conn.prepare(
            "SELECT h.run_id,t.thread_id,t.parent_session_id,t.state_json,r.status,r.terminal_reason,r.created_at_unix_ms,h.history_json \
             FROM subagent_history_v1 h JOIN subagent_threads_v1 t ON t.thread_id=h.thread_id \
             JOIN runs r ON r.run_id=h.run_id \
             WHERE (?1 IS NULL OR t.parent_session_id=?1) AND (?2 IS NULL OR h.run_id=?2) \
             ORDER BY r.created_at_unix_ms,h.run_id LIMIT 1001"
        ).map_err(map_rusqlite)?;
        let found = statement.query_map(params![session,task], |row| Ok((
            row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,
            row.get::<_,String>(3)?,row.get::<_,String>(4)?,row.get::<_,Option<String>>(5)?,row.get::<_,i64>(6)?,row.get::<_,String>(7)?
        ))).map_err(map_rusqlite)?;
        let mut out = Vec::new();
        for found in found {
            let (run,thread,session,raw,status,reason,created,history) = found.map_err(map_rusqlite)?;
            let state = decode(&raw)?;
            let current = state["childRunId"].as_str()==Some(run.as_str());
            let deferred = match status.as_str() {
                "completed" => "resolved",
                "failed"|"cancelled"|"interrupted" => "failed",
                _ => "pending",
            };
            // 线程续接后，旧任务读自己的正式历史，不能冒用新一轮结果。
            let result = if current {state["result"].clone()} else if status=="completed" {
                let history=decode(&history)?;
                history.as_array().ok_or_else(||invalid("task history is not an array"))?.iter().rev()
                    .find(|item|item["kind"]=="assistant").map(|item|item["content"].clone())
                    .unwrap_or_else(||json!({"terminalReason":reason}))
            } else {json!({"terminalReason":reason})};
            out.push(json!({"taskId":run,"threadId":thread,"parentSessionId":session,
                "type":"subagent","status":deferred,"runStatus":status,"deferredAtUnixMs":created,
                "reason":reason,"result":result,
                "meta":{"type":"subagent","threadId":thread}}));
        }
        if out.len()>1000 {return Err(invalid("too many background tasks; archive older sessions before listing"));}
        Ok(out)
    }).await
}

/// 旧可信宿主直接驱动的无子线程Run不读取父历史；已有线程缺档案则响亮拒绝。
pub async fn has_history(db: &RunDatabase, run: &str) -> Result<bool, StorageError> {
    let run = run.to_string();
    db.queue().submit(move|conn|{
        let exists:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM subagent_history_v1 WHERE run_id=?1)",[&run],|row|row.get(0)).map_err(map_rusqlite)?;
        let expected:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM subagent_threads_v1 WHERE json_extract(state_json,'$.childRunId')=?1)",[run],|row|row.get(0)).map_err(map_rusqlite)?;
        if expected && !exists{return Err(StorageError::Corrupted{detail:"subagent current child missing archive".into()});}
        Ok(exists)
    }).await
}

pub async fn record_failure(
    db: &RunDatabase,
    ctx: &RunContext,
    agent: &str,
    detail: &str,
) -> Result<(), StorageError> {
    let ctx = ctx.clone();
    let agent = agent.to_string();
    let detail = detail.to_string();
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                let thread = context_thread(conn, &ctx, &agent)?;
                let raw: String = conn
                    .query_row(
                        "SELECT state_json FROM subagent_threads_v1 WHERE thread_id=?1",
                        [&thread],
                        |r| r.get(0),
                    )
                    .map_err(map_rusqlite)?;
                let mut state = decode(&raw)?;
                state["error"] = json!(detail);
                conn.execute(
                    "UPDATE subagent_threads_v1 SET state_json=?2 WHERE thread_id=?1",
                    params![thread, state.to_string()],
                )
                .map_err(map_rusqlite)?;
                Ok(())
            })
        })
        .await
}
