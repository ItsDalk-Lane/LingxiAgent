//! 活动升格与内部会话的独立事实；不修改触发终态，不按名称猜测会话用途。
use super::{migrations::map_rusqlite, run_store::with_write_txn, RunDatabase};
use lingxi_kernel::{ports::StorageError, LOCAL_OWNER_SUBJECT};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationActivityPromotion {
    pub trigger_id: String,
    pub session_id: String,
    pub promoted_at: i64,
}

fn conflict(detail: &str) -> StorageError {
    StorageError::Conflict {
        detail: detail.into(),
    }
}
fn invalid(detail: &str) -> StorageError {
    StorageError::InvalidRequest {
        detail: detail.into(),
    }
}
fn time(now: u64) -> Result<i64, StorageError> {
    i64::try_from(now).map_err(|_| invalid("automation activity timestamp overflow"))
}

fn session_actor(conn: &Connection, sid: &str) -> Result<String, StorageError> {
    let row: Option<(String, String, String)> = conn
        .query_row(
            "SELECT agent_id,owner_user_id,lifecycle FROM sessions WHERE session_id=?1",
            [sid],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(map_rusqlite)?;
    let Some((actor, owner, lifecycle)) = row else {
        return Err(invalid("automation_activity_session_missing"));
    };
    if owner != LOCAL_OWNER_SUBJECT || lifecycle != "active" {
        return Err(conflict("automation_activity_session_not_owned_or_active"));
    }
    Ok(actor)
}

fn trigger_actor(
    conn: &Connection,
    trigger: &str,
) -> Result<(String, Option<String>, Option<String>), StorageError> {
    type TriggerAuthority = (
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
    );
    let row: Option<TriggerAuthority> = conn
        .query_row(
            "SELECT j.actor_id,j.owner_kind,j.owner_subject,t.config_json,t.session_id,t.run_id \
             FROM automation_triggers_v1 t JOIN automation_jobs_v1 j ON j.id=t.job_id \
             WHERE t.trigger_id=?1",
            [trigger],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .optional()
        .map_err(map_rusqlite)?;
    let Some((current_actor, kind, owner, raw, session, run)) = row else {
        return Err(invalid("automation_activity_trigger_missing"));
    };
    if kind != "local_user" || owner != LOCAL_OWNER_SUBJECT {
        return Err(conflict("automation_activity_trigger_not_owned"));
    }
    let config: Value = serde_json::from_str(&raw).map_err(|error| StorageError::Corrupted {
        detail: format!("automation activity frozen config: {error}"),
    })?;
    let actor = config
        .get("actorAgentId")
        .and_then(Value::as_str)
        .unwrap_or(&current_actor)
        .to_string();
    Ok((actor, session, run))
}

fn resolve(conn: &Connection, trigger: &str) -> Result<Option<String>, StorageError> {
    let (actor, bound_session, bound_run) = trigger_actor(conn, trigger)?;
    let mut stmt=conn.prepare("SELECT r.run_id,r.session_id,r.owner_kind,r.owner_subject FROM run_lineage l JOIN runs r ON r.run_id=l.run_id WHERE l.cause_id=?1 AND l.origin IN ('cron','heartbeat') ORDER BY r.run_id LIMIT 2").map_err(map_rusqlite)?;
    let rows = stmt
        .query_map([trigger], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(map_rusqlite)?;
    let rows = rows.collect::<Result<Vec<_>, _>>().map_err(map_rusqlite)?;
    if rows.len() > 1 {
        return Err(conflict("automation_activity_multiple_causal_runs"));
    }
    let Some((run, sid, kind, owner)) = rows.into_iter().next() else {
        return Ok(None);
    };
    if kind != "local_user"
        || owner != LOCAL_OWNER_SUBJECT
        || bound_run.as_ref().is_some_and(|expected| expected != &run)
        || bound_session
            .as_ref()
            .is_some_and(|expected| expected != &sid)
    {
        return Err(conflict("automation_activity_lineage_binding_mismatch"));
    }
    if session_actor(conn, &sid)? != actor {
        return Err(conflict("automation_activity_actor_mismatch"));
    }
    let internal: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT kind,trigger_id FROM automation_internal_sessions_v1 WHERE session_id=?1",
            [&sid],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(map_rusqlite)?;
    if internal.is_some_and(|(kind, id)| kind != "activity" || id.as_deref() != Some(trigger)) {
        return Err(conflict("automation_activity_internal_scope_mismatch"));
    }
    Ok(Some(sid))
}

pub async fn activity_session(
    db: &RunDatabase,
    trigger: &str,
) -> Result<Option<String>, StorageError> {
    let trigger = trigger.to_string();
    db.queue().submit(move |conn| resolve(conn, &trigger)).await
}

pub async fn get_promotion(
    db: &RunDatabase,
    trigger: &str,
) -> Result<Option<AutomationActivityPromotion>, StorageError> {
    let trigger = trigger.to_string();
    db.queue().submit(move|conn|conn.query_row("SELECT trigger_id,session_id,promoted_at FROM automation_activity_promotions_v1 WHERE trigger_id=?1",[trigger],|row|Ok(AutomationActivityPromotion{trigger_id:row.get(0)?,session_id:row.get(1)?,promoted_at:row.get(2)?})).optional().map_err(map_rusqlite)).await
}

/// 服务层持有同一会话运行许可；事务再核真实运行、归属、生命周期与持久 busy 状态。
pub async fn promote(
    db: &RunDatabase,
    trigger: &str,
    expected_session: &str,
    now: u64,
) -> Result<AutomationActivityPromotion, StorageError> {
    let (trigger, sid, now) = (
        trigger.to_string(),
        expected_session.to_string(),
        time(now)?,
    );
    let busy = db.options().busy_timeout_ms;
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        if resolve(conn,&trigger)?.as_deref()!=Some(&sid){return Err(conflict("automation_activity_session_changed"));}
        let active:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM runs WHERE session_id=?1 AND status IN ('queued','running','waiting_approval','cancelling'))",[&sid],|row|row.get(0)).map_err(map_rusqlite)?;
        if active{return Err(conflict("session_busy"));}
        let existing:Option<AutomationActivityPromotion>=conn.query_row("SELECT trigger_id,session_id,promoted_at FROM automation_activity_promotions_v1 WHERE trigger_id=?1",[&trigger],|row|Ok(AutomationActivityPromotion{trigger_id:row.get(0)?,session_id:row.get(1)?,promoted_at:row.get(2)?})).optional().map_err(map_rusqlite)?;
        if let Some(existing)=existing{return if existing.session_id==sid{Ok(existing)}else{Err(conflict("automation_activity_promotion_conflict"))};}
        conn.execute("INSERT INTO automation_activity_promotions_v1(trigger_id,session_id,promoted_at) VALUES(?1,?2,?3)",params![trigger,sid,now]).map_err(map_rusqlite)?;
        Ok(AutomationActivityPromotion{trigger_id:trigger,session_id:sid,promoted_at:now})
    })).await
}

fn register(
    conn: &Connection,
    sid: &str,
    actor: &str,
    kind: &str,
    trigger: Option<&str>,
    now: i64,
) -> Result<(), StorageError> {
    if session_actor(conn, sid)? != actor {
        return Err(conflict("automation_internal_actor_mismatch"));
    }
    if let Some(trigger) = trigger {
        let (expected, bound, _) = trigger_actor(conn, trigger)?;
        if expected != actor || bound.as_deref().is_some_and(|bound| bound != sid) {
            return Err(conflict("automation_internal_trigger_mismatch"));
        }
    }
    let existing:Option<(String,Option<String>,String)>=conn.query_row("SELECT kind,trigger_id,agent_id FROM automation_internal_sessions_v1 WHERE session_id=?1",[sid],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional().map_err(map_rusqlite)?;
    if let Some((old_kind, old_trigger, old_actor)) = existing {
        return if old_kind == kind && old_trigger.as_deref() == trigger && old_actor == actor {
            Ok(())
        } else {
            Err(conflict("automation_internal_identity_conflict"))
        };
    }
    conn.execute("INSERT INTO automation_internal_sessions_v1(session_id,kind,trigger_id,agent_id,created_at) VALUES(?1,?2,?3,?4,?5)",params![sid,kind,trigger,actor,now]).map_err(map_rusqlite)?;
    Ok(())
}

pub async fn register_activity_session(
    db: &RunDatabase,
    trigger: &str,
    sid: &str,
    actor: &str,
    now: u64,
) -> Result<(), StorageError> {
    let (trigger, sid, actor, now) = (
        trigger.to_string(),
        sid.to_string(),
        actor.to_string(),
        time(now)?,
    );
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                register(conn, &sid, &actor, "activity", Some(&trigger), now)
            })
        })
        .await
}

pub async fn register_heartbeat_scope(
    db: &RunDatabase,
    sid: &str,
    actor: &str,
    now: u64,
) -> Result<(), StorageError> {
    let (sid, actor, now) = (sid.to_string(), actor.to_string(), time(now)?);
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                register(conn, &sid, &actor, "heartbeat_scope", None, now)
            })
        })
        .await
}

pub async fn is_heartbeat_scope(
    db: &RunDatabase,
    sid: &str,
    actor: &str,
) -> Result<bool, StorageError> {
    let (sid, actor) = (sid.to_string(), actor.to_string());
    db.queue().submit(move|conn|{
        conn.query_row("SELECT EXISTS(SELECT 1 FROM automation_internal_sessions_v1 i JOIN sessions s ON s.session_id=i.session_id WHERE i.session_id=?1 AND i.kind='heartbeat_scope' AND i.agent_id=?2 AND s.agent_id=?2 AND s.owner_user_id=?3 AND s.lifecycle='active')",params![sid,actor,LOCAL_OWNER_SUBJECT],|row|row.get(0)).map_err(map_rusqlite)
    }).await
}

/// 删除会话时只清其自身派生数据；保留计划和触发历史，不伪造外发结果。
pub(crate) fn cleanup_deleted_session(conn: &Connection, sid: &str) -> Result<(), StorageError> {
    for sql in ["DELETE FROM automation_activity_promotions_v1 WHERE session_id=?1","DELETE FROM automation_internal_sessions_v1 WHERE session_id=?1","DELETE FROM automation_daily_receipts_v1 WHERE session_id=?1","DELETE FROM automation_suggestions_v1 WHERE session_id=?1","DELETE FROM automation_notifications_v1 WHERE run_id IN (SELECT run_id FROM runs WHERE session_id=?1)"]{conn.execute(sql,[sid]).map_err(map_rusqlite)?;}
    Ok(())
}

/// 记录超时取消原因，不修改真实运行终态、游标或通知回执。重复检查只领取一次。
pub async fn claim_timeout(
    db: &RunDatabase,
    trigger: &str,
    now: u64,
    timeout_ms: u64,
) -> Result<bool, StorageError> {
    let (trigger, now, timeout) = (trigger.to_owned(), time(now)?, time(timeout_ms)?);
    let busy = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                let changed = conn
                    .execute(
                        "UPDATE automation_triggers_v1 SET error='timeout' WHERE trigger_id=?1 \
             AND status IN ('claimed','running') AND error IS NULL \
             AND EXISTS(SELECT 1 FROM run_lineage l JOIN runs r ON r.run_id=l.run_id \
                WHERE l.cause_id=?1 AND l.origin IN ('cron','heartbeat') \
                AND r.status IN ('queued','running','waiting_approval','cancelling') \
                AND r.created_at_unix_ms<=?2-?3)",
                        params![trigger, now, timeout],
                    )
                    .map_err(map_rusqlite)?;
                Ok(changed == 1)
            })
        })
        .await
}
