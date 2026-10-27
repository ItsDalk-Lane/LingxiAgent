//! 手机投递的持久因果、决策和预算；真实 Run 只从运行表及谱系读取。
pub use super::conversations_collab::*;
use super::{migrations::map_rusqlite, run_store::with_write_txn, RunDatabase};
use crate::conversation_files::ConversationMutation;
pub use crate::conversation_files::{get_operation, operation_mutation};
use lingxi_kernel::ports::StorageError;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PhoneDeliveryInput {
    pub delivery_id: String,
    pub cause_id: String,
    pub actor_id: String,
    pub conversation_id: String,
    pub conversation_type: String,
    pub batch_digest: String,
    pub root_chain_id: String,
    pub budget_limit: u32,
    pub dm_direction: Option<(String, String)>,
    pub source_run_id: Option<String>,
    pub source_message_id: String,
    pub membership_digest: String,
    pub owner_subject: String,
    pub input_json: Value,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhoneDeliveryStatus {
    Claimed,
    Running,
    Completed,
    Failed,
    Cancelled,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhoneDecision {
    Reply,
    Pass,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneDelivery {
    pub input: PhoneDeliveryInput,
    pub status: PhoneDeliveryStatus,
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    pub decision: Option<PhoneDecision>,
    pub decision_body: Option<String>,
    pub decision_at: Option<u64>,
    pub error: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryClaim {
    pub claimed: bool,
    pub delivery: Option<PhoneDelivery>,
    pub reason: Option<String>,
    pub retry_at: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneBindingInput {
    pub actor_id: String,
    pub conversation_id: String,
    pub conversation_type: String,
    pub owner_subject: String,
    pub session_id: String,
    pub mode: String,
    pub expected_epoch: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneBinding {
    pub input: PhoneBindingInput,
    pub epoch: u64,
    pub last_used: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionClaim {
    pub claimed: bool,
    pub delivery: PhoneDelivery,
}
fn invalid(detail: impl Into<String>) -> StorageError {
    StorageError::InvalidRequest {
        detail: detail.into(),
    }
}
fn conflict(detail: impl Into<String>) -> StorageError {
    StorageError::Conflict {
        detail: detail.into(),
    }
}
fn ident(value: &str) -> Result<(), StorageError> {
    if value.trim().is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
        Err(invalid("invalid conversation identity"))
    } else {
        Ok(())
    }
}
fn number(value: u64) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| invalid("conversation integer overflow"))
}
fn wire<T: Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .expect("closed enum")
        .as_str()
        .expect("enum string")
        .into()
}
fn parse<T: serde::de::DeserializeOwned>(value: String, col: usize) -> rusqlite::Result<T> {
    serde_json::from_str(&value).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(col, rusqlite::types::Type::Text, Box::new(e))
    })
}
fn read_u64(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(column)?;
    u64::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}
const DELIVERY_COLUMNS: &str =
    "input_json,status,session_id,run_id,decision,decision_body,decision_at,error,created_at,updated_at";
fn delivery_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PhoneDelivery> {
    let decision: Option<String> = row.get(4)?;
    Ok(PhoneDelivery {
        input: parse(row.get(0)?, 0)?,
        status: parse(
            serde_json::to_string(&row.get::<_, String>(1)?).expect("string"),
            1,
        )?,
        session_id: row.get(2)?,
        run_id: row.get(3)?,
        decision: decision
            .map(|v| parse(serde_json::to_string(&v).expect("string"), 4))
            .transpose()?,
        decision_body: row.get(5)?,
        decision_at: row
            .get::<_, Option<i64>>(6)?
            .map(|value| {
                u64::try_from(value).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        6,
                        rusqlite::types::Type::Integer,
                        Box::new(error),
                    )
                })
            })
            .transpose()?,
        error: row.get(7)?,
        created_at: read_u64(row, 8)?,
        updated_at: read_u64(row, 9)?,
    })
}
fn delivery_conn(
    conn: &rusqlite::Connection,
    id: &str,
) -> Result<Option<PhoneDelivery>, StorageError> {
    let row = conn
        .query_row(
            &format!("SELECT {DELIVERY_COLUMNS} FROM phone_deliveries_v1 WHERE delivery_id=?1"),
            [id],
            delivery_row,
        )
        .optional()
        .map_err(map_rusqlite)?;
    if row.as_ref().is_some_and(|row| row.input.delivery_id != id) {
        return Err(StorageError::Corrupted {
            detail: "phone delivery identity damaged".into(),
        });
    }
    Ok(row)
}
pub async fn get_delivery(
    db: &RunDatabase,
    id: &str,
) -> Result<Option<PhoneDelivery>, StorageError> {
    let id = id.to_owned();
    db.queue()
        .submit(move |conn| delivery_conn(conn, &id))
        .await
}
fn actual_run(
    conn: &rusqlite::Connection,
    delivery: &PhoneDelivery,
) -> Result<Option<(String, String, String)>, StorageError> {
    let mut stmt=conn.prepare("SELECT r.run_id,r.session_id,r.status,l.origin,l.source_message_id,l.parent_run_id,r.owner_subject,s.owner_user_id,s.agent_id FROM run_lineage l JOIN runs r ON r.run_id=l.run_id JOIN sessions s ON s.session_id=r.session_id WHERE l.cause_id=?1 AND l.origin IN ('channel','dm') ORDER BY r.run_id LIMIT 2").map_err(map_rusqlite)?;
    let rows = stmt
        .query_map([&delivery.input.delivery_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
            ))
        })
        .map_err(map_rusqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_rusqlite)?;
    if rows.len() > 1 {
        return Err(conflict("multiple actual Runs for one phone delivery"));
    }
    let Some((run, sid, status, origin, source, parent, run_owner, session_owner, agent)) =
        rows.into_iter().next()
    else {
        return Ok(None);
    };
    if origin != delivery.input.conversation_type
        || source.as_deref() != Some(&delivery.input.source_message_id)
        || parent != delivery.input.source_run_id
        || run_owner != delivery.input.owner_subject
        || session_owner != delivery.input.owner_subject
        || agent != delivery.input.actor_id
        || delivery.run_id.as_ref().is_some_and(|old| old != &run)
        || delivery.session_id.as_ref().is_some_and(|old| old != &sid)
    {
        return Err(conflict("phone actual Run causal authority mismatch"));
    }
    Ok(Some((run, sid, status)))
}
pub async fn get_delivery_run(
    db: &RunDatabase,
    id: &str,
) -> Result<Option<(String, String, String)>, StorageError> {
    let id = id.to_owned();
    db.queue()
        .submit(move |conn| {
            let row = delivery_conn(conn, &id)?.ok_or_else(|| invalid("phone delivery missing"))?;
            actual_run(conn, &row)
        })
        .await
}
fn validate_input(input: &PhoneDeliveryInput) -> Result<String, StorageError> {
    for id in [
        &input.delivery_id,
        &input.cause_id,
        &input.actor_id,
        &input.conversation_id,
        &input.batch_digest,
        &input.root_chain_id,
        &input.source_message_id,
        &input.membership_digest,
        &input.owner_subject,
    ] {
        ident(id)?;
    }
    if input.budget_limit == 0
        || input.budget_limit > 64
        || input.input_json.to_string().len() > 512 * 1024
    {
        return Err(invalid("phone input exceeds bound"));
    }
    if let Some(source) = &input.source_run_id {
        ident(source)?;
    }
    match (input.conversation_type.as_str(), &input.dm_direction) {
        ("channel", None) => Ok(input.conversation_id.clone()),
        ("dm", Some((from, to)))
            if from != to
                && to == &input.actor_id
                && input.conversation_id == format!("dm:{from}")
                && input.budget_limit <= 3 =>
        {
            ident(from)?;
            ident(to)?;
            let mut pair = [from.as_str(), to.as_str()];
            pair.sort();
            Ok(serde_json::to_string(&pair).expect("pair"))
        }
        _ => Err(invalid(
            "phone conversation scope or DM three-round budget invalid",
        )),
    }
}
pub async fn claim_delivery(
    db: &RunDatabase,
    input: &PhoneDeliveryInput,
    now: u64,
) -> Result<DeliveryClaim, StorageError> {
    let scope = validate_input(input)?;
    let (input, now, busy) = (input.clone(), number(now)?, db.options().busy_timeout_ms);
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        if let Some(old)=delivery_conn(conn,&input.delivery_id)?{
            if old.input!=input{return Err(conflict("phone delivery ID already binds different input"));}
            return Ok(DeliveryClaim{claimed:false,delivery:Some(old),reason:Some("already_claimed".into()),retry_at:None});
        }
        let duplicate:Option<String>=conn.query_row("SELECT delivery_id FROM phone_deliveries_v1 WHERE owner_subject=?1 AND cause_id=?2 AND actor_id=?3 AND batch_digest=?4",params![input.owner_subject,input.cause_id,input.actor_id,input.batch_digest],|r|r.get(0)).optional().map_err(map_rusqlite)?;
        if let Some(id)=duplicate{let old=delivery_conn(conn,&id)?.ok_or_else(||invalid("phone duplicate disappeared"))?;let mut compare=input.clone();compare.delivery_id=id;if old.input!=compare{return Err(conflict("phone causal batch already binds different input"));}return Ok(DeliveryClaim{claimed:false,delivery:Some(old),reason:Some("duplicate_batch".into()),retry_at:None});}
        if let Some(source)=&input.source_run_id{
            let valid:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM runs r JOIN sessions s ON s.session_id=r.session_id WHERE r.run_id=?1 AND r.owner_subject=?2 AND s.owner_user_id=?2)",params![source,input.owner_subject],|r|r.get(0)).map_err(map_rusqlite)?;
            if !valid{return Err(conflict("phone source must be an actual owned Run"));}
            // 子代理也不能重新起一条预算链；沿真实父谱系找既有手机来源。
            let mut stmt=conn.prepare("WITH RECURSIVE ancestors(id,depth) AS (SELECT ?1,0 UNION ALL SELECT l.parent_run_id,a.depth+1 FROM run_lineage l JOIN ancestors a ON l.run_id=a.id WHERE l.parent_run_id IS NOT NULL AND a.depth<64) SELECT DISTINCT d.root_chain_id FROM ancestors a JOIN run_lineage l ON l.run_id=a.id JOIN phone_deliveries_v1 d ON d.delivery_id=l.cause_id WHERE l.origin IN ('channel','dm')").map_err(map_rusqlite)?;
            let roots=stmt.query_map([source],|r|r.get::<_,String>(0)).map_err(map_rusqlite)?.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)?;
            if roots.iter().any(|root|root!=&input.root_chain_id){return Err(conflict("phone descendant cannot replace its root chain budget"));}
        }
        type Chain=(String,String,String,u32,u32);
        let old:Option<Chain>=conn.query_row("SELECT owner_subject,conversation_type,conversation_scope,budget_limit,spent FROM phone_chains_v1 WHERE root_chain_id=?1",[&input.root_chain_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(map_rusqlite)?;
        let spent=if let Some((owner,kind,old_scope,limit,spent))=old{if owner!=input.owner_subject || kind!=input.conversation_type || old_scope!=scope || limit!=input.budget_limit{return Err(conflict("phone root chain scope and original budget are immutable"));}spent}else{conn.execute("INSERT INTO phone_chains_v1 VALUES(?1,?2,?3,?4,?5,0,?6)",params![input.root_chain_id,input.owner_subject,input.conversation_type,scope,input.budget_limit,now]).map_err(map_rusqlite)?;0};
        if spent>=input.budget_limit{return Ok(DeliveryClaim{claimed:false,delivery:None,reason:Some("root_chain_budget_exhausted".into()),retry_at:None});}
        if let Some((from,to))=&input.dm_direction{
            let lock:Option<(i64,i64)>=conn.query_row("SELECT processing_until,cooldown_until FROM phone_dm_directions_v1 WHERE owner_subject=?1 AND from_actor=?2 AND to_actor=?3",params![input.owner_subject,from,to],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(map_rusqlite)?;
            if let Some((processing,cooldown))=lock{let until=processing.max(cooldown);if until>now{return Ok(DeliveryClaim{claimed:false,delivery:None,reason:Some(if processing>now{"dm_direction_processing"}else{"dm_direction_cooldown"}.into()),retry_at:Some(until as u64)});}}
        }
        conn.execute("UPDATE phone_chains_v1 SET spent=spent+1 WHERE root_chain_id=?1 AND spent<budget_limit",[&input.root_chain_id]).map_err(map_rusqlite)?;
        conn.execute("INSERT INTO phone_deliveries_v1(delivery_id,input_json,owner_subject,actor_id,conversation_id,conversation_type,cause_id,batch_digest,root_chain_id,status,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'claimed',?10,?10)",params![input.delivery_id,serde_json::to_string(&input).map_err(|e|invalid(e.to_string()))?,input.owner_subject,input.actor_id,input.conversation_id,input.conversation_type,input.cause_id,input.batch_digest,input.root_chain_id,now]).map_err(map_rusqlite)?;
        if let Some((from,to))=&input.dm_direction{conn.execute("INSERT INTO phone_dm_directions_v1 VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(owner_subject,from_actor,to_actor) DO UPDATE SET delivery_id=excluded.delivery_id,processing_until=excluded.processing_until,cooldown_until=excluded.cooldown_until",params![input.owner_subject,from,to,input.delivery_id,now.checked_add(300_000).ok_or_else(||invalid("phone time overflow"))?,now.checked_add(10_000).ok_or_else(||invalid("phone time overflow"))?]).map_err(map_rusqlite)?;}
        Ok(DeliveryClaim{claimed:true,delivery:delivery_conn(conn,&input.delivery_id)?,reason:None,retry_at:None})
    })).await
}
fn bind_conn(
    conn: &rusqlite::Connection,
    id: &str,
    sid: &str,
    run: &str,
    now: i64,
) -> Result<PhoneDelivery, StorageError> {
    let row = delivery_conn(conn, id)?.ok_or_else(|| invalid("phone delivery missing"))?;
    let actual =
        actual_run(conn, &row)?.ok_or_else(|| conflict("phone actual causal Run missing"))?;
    if actual.0 != run || actual.1 != sid {
        return Err(conflict("phone Run binding mismatch"));
    }
    if row.run_id.is_some() {
        return Ok(row);
    }
    if row.status != PhoneDeliveryStatus::Claimed {
        return Err(conflict("settled phone delivery cannot bind a new Run"));
    }
    let active: bool = conn
        .query_row(
            "SELECT lifecycle='active' FROM sessions WHERE session_id=?1",
            [sid],
            |r| r.get(0),
        )
        .map_err(map_rusqlite)?;
    if !active {
        return Err(conflict("phone session inactive"));
    }
    conn.execute("UPDATE phone_deliveries_v1 SET session_id=?2,run_id=?3,status='running',updated_at=?4 WHERE delivery_id=?1",params![id,sid,run,now]).map_err(map_rusqlite)?;
    delivery_conn(conn, id)?.ok_or_else(|| invalid("phone bound delivery missing"))
}
pub async fn bind_delivery_run(
    db: &RunDatabase,
    id: &str,
    sid: &str,
    run: &str,
    now: u64,
) -> Result<PhoneDelivery, StorageError> {
    let (id, sid, run, now, busy) = (
        id.to_owned(),
        sid.to_owned(),
        run.to_owned(),
        number(now)?,
        db.options().busy_timeout_ms,
    );
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| bind_conn(conn, &id, &sid, &run, now))
        })
        .await
}
pub async fn record_decision(
    db: &RunDatabase,
    id: &str,
    run: &str,
    decision: PhoneDecision,
    body: Option<String>,
    now: u64,
) -> Result<DecisionClaim, StorageError> {
    if (decision == PhoneDecision::Reply
        && body
            .as_deref()
            .is_none_or(|s| s.trim().is_empty() || s.len() > 100_000 || s.contains('\0')))
        || (decision == PhoneDecision::Pass && body.as_deref().is_some_and(|s| !s.is_empty()))
    {
        return Err(invalid("invalid phone reply/pass body"));
    }
    let (id, run, now, busy) = (
        id.to_owned(),
        run.to_owned(),
        number(now)?,
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let row=delivery_conn(conn,&id)?.ok_or_else(||invalid("phone delivery missing"))?;
        let actual=actual_run(conn,&row)?.ok_or_else(||conflict("phone decision has no actual Run"))?;
        if actual.0!=run{return Err(conflict("phone decision Run mismatch"));}
        if let Some(old)=row.decision{if old!=decision || row.decision_body!=body{return Err(conflict("one phone Run may decide reply/pass only once"));}return Ok(DecisionClaim{claimed:false,delivery:row});}
        if !matches!(actual.2.as_str(),"running"|"waiting_approval"){return Err(conflict("phone decision requires a live actual Run"));}
        bind_conn(conn,&id,&actual.1,&run,now)?;
        conn.execute("UPDATE phone_deliveries_v1 SET decision=?2,decision_body=?3,decision_at=?4,updated_at=?4 WHERE delivery_id=?1 AND decision IS NULL",params![id,wire(decision),body,now]).map_err(map_rusqlite)?;
        Ok(DecisionClaim{claimed:true,delivery:delivery_conn(conn,&id)?.ok_or_else(||invalid("phone decided delivery missing"))?})
    })).await
}
fn settle_conn(
    conn: &rusqlite::Connection,
    id: &str,
    status: PhoneDeliveryStatus,
    error: Option<String>,
    now: i64,
) -> Result<PhoneDelivery, StorageError> {
    if matches!(
        status,
        PhoneDeliveryStatus::Claimed | PhoneDeliveryStatus::Running
    ) {
        return Err(invalid("phone settlement must be terminal"));
    }
    let row = delivery_conn(conn, id)?.ok_or_else(|| invalid("phone delivery missing"))?;
    if !matches!(
        row.status,
        PhoneDeliveryStatus::Claimed | PhoneDeliveryStatus::Running
    ) {
        return if row.status == status && row.error == error {
            Ok(row)
        } else {
            Err(conflict("phone terminal receipt cannot change"))
        };
    }
    if status != PhoneDeliveryStatus::Unknown {
        if let Some((_, _, actual)) = actual_run(conn, &row)? {
            let expected = match status {
                PhoneDeliveryStatus::Completed => "completed",
                PhoneDeliveryStatus::Failed => "failed",
                PhoneDeliveryStatus::Cancelled => "cancelled",
                _ => unreachable!(),
            };
            if actual != expected {
                return Err(conflict("phone settlement differs from actual Run"));
            }
        } else if status != PhoneDeliveryStatus::Failed {
            return Err(conflict("phone completion requires an actual causal Run"));
        }
    }
    conn.execute(
        "UPDATE phone_deliveries_v1 SET status=?2,error=?3,updated_at=?4 WHERE delivery_id=?1",
        params![id, wire(status), error, now],
    )
    .map_err(map_rusqlite)?;
    if status != PhoneDeliveryStatus::Unknown {
        conn.execute("UPDATE phone_dm_directions_v1 SET processing_until=0,cooldown_until=MAX(cooldown_until,?2) WHERE delivery_id=?1",params![id,now.checked_add(10_000).ok_or_else(||invalid("phone time overflow"))?]).map_err(map_rusqlite)?;
    }
    delivery_conn(conn, id)?.ok_or_else(|| invalid("phone terminal delivery missing"))
}
pub async fn settle_delivery(
    db: &RunDatabase,
    id: &str,
    status: PhoneDeliveryStatus,
    error: Option<String>,
    now: u64,
) -> Result<PhoneDelivery, StorageError> {
    let (id, now, busy) = (id.to_owned(), number(now)?, db.options().busy_timeout_ms);
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                settle_conn(conn, &id, status, error, now)
            })
        })
        .await
}
pub async fn list_pending_deliveries(db: &RunDatabase) -> Result<Vec<PhoneDelivery>, StorageError> {
    db.queue().submit(|conn|{let mut stmt=conn.prepare(&format!("SELECT {DELIVERY_COLUMNS} FROM phone_deliveries_v1 WHERE status IN ('claimed','running') ORDER BY created_at,delivery_id LIMIT 1024")).map_err(map_rusqlite)?;let rows=stmt.query_map([],delivery_row).map_err(map_rusqlite)?.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)?;Ok(rows)}).await
}
pub async fn list_active_deliveries(
    db: &RunDatabase,
    root: Option<&str>,
    actor: Option<&str>,
) -> Result<Vec<PhoneDelivery>, StorageError> {
    let (root, actor) = (root.map(str::to_owned), actor.map(str::to_owned));
    db.queue().submit(move|conn|{let mut stmt=conn.prepare(&format!("WITH RECURSIVE descendants(delivery_id,run_id) AS (SELECT seed.delivery_id,l.run_id FROM phone_deliveries_v1 seed JOIN run_lineage l ON l.cause_id=seed.delivery_id AND l.origin=seed.conversation_type WHERE (?1 IS NULL OR seed.root_chain_id=?1) AND (?2 IS NULL OR seed.actor_id=?2) UNION SELECT tree.delivery_id,l.run_id FROM descendants tree JOIN run_lineage l ON l.parent_run_id=tree.run_id) SELECT {DELIVERY_COLUMNS} FROM phone_deliveries_v1 d WHERE (?1 IS NULL OR root_chain_id=?1) AND (?2 IS NULL OR actor_id=?2) AND (status IN ('claimed','running') OR EXISTS(SELECT 1 FROM descendants tree JOIN runs r ON r.run_id=tree.run_id WHERE tree.delivery_id=d.delivery_id AND r.status NOT IN ('completed','failed','cancelled','interrupted_needs_attention'))) ORDER BY created_at,delivery_id")).map_err(map_rusqlite)?;let rows=stmt.query_map(params![root,actor],delivery_row).map_err(map_rusqlite)?.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)?;Ok(rows)}).await
}
pub async fn active_runs_for_chain(
    db: &RunDatabase,
    root: &str,
) -> Result<Vec<(String, String, String)>, StorageError> {
    let root = root.to_owned();
    db.queue().submit(move|conn|{let mut stmt=conn.prepare("WITH RECURSIVE tree(id) AS (SELECT l.run_id FROM phone_deliveries_v1 d JOIN run_lineage l ON l.cause_id=d.delivery_id AND l.origin IN ('channel','dm') WHERE d.root_chain_id=?1 UNION SELECT l.run_id FROM run_lineage l JOIN tree t ON l.parent_run_id=t.id) SELECT r.run_id,r.session_id,s.agent_id FROM tree t JOIN runs r ON r.run_id=t.id JOIN sessions s ON s.session_id=r.session_id WHERE r.status NOT IN ('completed','failed','cancelled','interrupted_needs_attention') ORDER BY r.run_id").map_err(map_rusqlite)?;let rows=stmt.query_map([root],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(map_rusqlite)?.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)?;Ok(rows)}).await
}
pub async fn recover_deliveries(db: &RunDatabase, now: u64) -> Result<usize, StorageError> {
    let (now, busy) = (number(now)?, db.options().busy_timeout_ms);
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{let mut stmt=conn.prepare(&format!("SELECT {DELIVERY_COLUMNS} FROM phone_deliveries_v1 WHERE status IN ('claimed','running') ORDER BY created_at")).map_err(map_rusqlite)?;let rows=stmt.query_map([],delivery_row).map_err(map_rusqlite)?.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)?;drop(stmt);let mut changed=0;for row in rows{if let Some((run,sid,state))=actual_run(conn,&row)?{conn.execute("UPDATE phone_deliveries_v1 SET run_id=?2,session_id=?3 WHERE delivery_id=?1",params![row.input.delivery_id,run,sid]).map_err(map_rusqlite)?;let status=match state.as_str(){"completed"=>PhoneDeliveryStatus::Completed,"failed"=>PhoneDeliveryStatus::Failed,"cancelled"=>PhoneDeliveryStatus::Cancelled,_=>PhoneDeliveryStatus::Unknown};settle_conn(conn,&row.input.delivery_id,status,Some("recovered actual causal Run; do not recreate".into()),now)?;changed+=1;}else if row.status==PhoneDeliveryStatus::Running{settle_conn(conn,&row.input.delivery_id,PhoneDeliveryStatus::Unknown,Some("phone binding lacks actual causal Run".into()),now)?;changed+=1;}}Ok(changed)})).await
}
fn binding_conn(
    conn: &rusqlite::Connection,
    actor: &str,
    conversation: &str,
    owner: &str,
) -> Result<Option<PhoneBinding>, StorageError> {
    conn.query_row("SELECT conversation_type,session_id,mode,epoch,last_used,expires_at FROM phone_bindings_v1 WHERE actor_id=?1 AND conversation_id=?2 AND owner_subject=?3",params![actor,conversation,owner],|r|Ok(PhoneBinding{input:PhoneBindingInput{actor_id:actor.into(),conversation_id:conversation.into(),conversation_type:r.get(0)?,owner_subject:owner.into(),session_id:r.get(1)?,mode:r.get(2)?,expected_epoch:None},epoch:read_u64(r,3)?,last_used:read_u64(r,4)?,expires_at:read_u64(r,5)?})).optional().map_err(map_rusqlite)
}
pub async fn get_phone_binding(
    db: &RunDatabase,
    actor: &str,
    conversation: &str,
    owner: &str,
) -> Result<Option<PhoneBinding>, StorageError> {
    let (actor, conversation, owner) =
        (actor.to_owned(), conversation.to_owned(), owner.to_owned());
    db.queue()
        .submit(move |conn| binding_conn(conn, &actor, &conversation, &owner))
        .await
}
/// 重置后仍保留上一代，首次并发投递据此共同派生同一个新会话身份。
pub async fn phone_binding_epoch(
    db: &RunDatabase,
    actor: &str,
    conversation: &str,
    owner: &str,
) -> Result<u64, StorageError> {
    let (actor, conversation, owner) =
        (actor.to_owned(), conversation.to_owned(), owner.to_owned());
    db.queue().submit(move|conn|{
        let epoch:Option<i64>=conn.query_row("SELECT MAX(epoch) FROM (SELECT epoch FROM phone_binding_epochs_v1 WHERE actor_id=?1 AND conversation_id=?2 AND owner_subject=?3 UNION ALL SELECT epoch FROM phone_bindings_v1 WHERE actor_id=?1 AND conversation_id=?2 AND owner_subject=?3)",params![actor,conversation,owner],|r|r.get(0)).map_err(map_rusqlite)?;
        u64::try_from(epoch.unwrap_or(0)).map_err(|_|StorageError::Corrupted{detail:"phone binding epoch damaged".into()})
    }).await
}
fn validate_binding_input(input: &PhoneBindingInput) -> Result<(), StorageError> {
    for id in [
        &input.actor_id,
        &input.conversation_id,
        &input.owner_subject,
        &input.session_id,
    ] {
        ident(id)?;
    }
    if !matches!(input.conversation_type.as_str(), "channel" | "dm")
        || !matches!(input.mode.as_str(), "ask" | "read_only" | "operate")
    {
        return Err(invalid("invalid phone session binding"));
    }
    Ok(())
}
fn put_binding_conn(
    conn: &rusqlite::Connection,
    input: &PhoneBindingInput,
    now: i64,
) -> Result<PhoneBinding, StorageError> {
    let valid:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM sessions WHERE session_id=?1 AND agent_id=?2 AND owner_user_id=?3 AND lifecycle='active')",params![input.session_id,input.actor_id,input.owner_subject],|r|r.get(0)).map_err(map_rusqlite)?;
    if !valid {
        return Err(conflict(
            "phone binding requires actual active owned session",
        ));
    }
    let old = binding_conn(
        conn,
        &input.actor_id,
        &input.conversation_id,
        &input.owner_subject,
    )?;
    let epoch = match (old, input.expected_epoch) {
        (None, None) => {
            let previous:Option<i64>=conn.query_row("SELECT epoch FROM phone_binding_epochs_v1 WHERE actor_id=?1 AND conversation_id=?2 AND owner_subject=?3",params![input.actor_id,input.conversation_id,input.owner_subject],|r|r.get(0)).optional().map_err(map_rusqlite)?;
            u64::try_from(previous.unwrap_or(0))
                .map_err(|_| invalid("phone epoch damaged"))?
                .checked_add(1)
                .ok_or_else(|| invalid("phone epoch exhausted"))?
        }
        (Some(old), Some(expected)) if old.epoch == expected => {
            if old.input.conversation_type != input.conversation_type {
                return Err(conflict("phone conversation kind changed"));
            }
            if old.input.session_id == input.session_id && old.input.mode == input.mode {
                old.epoch
            } else {
                old.epoch
                    .checked_add(1)
                    .ok_or_else(|| invalid("phone epoch exhausted"))?
            }
        }
        _ => return Err(conflict("phone binding epoch conflict")),
    };
    conn.execute("INSERT INTO phone_binding_epochs_v1 VALUES(?1,?2,?3,?4) ON CONFLICT(actor_id,conversation_id,owner_subject) DO UPDATE SET epoch=excluded.epoch",params![input.actor_id,input.conversation_id,input.owner_subject,number(epoch)?]).map_err(map_rusqlite)?;
    conn.execute("INSERT INTO phone_bindings_v1 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(actor_id,conversation_id,owner_subject) DO UPDATE SET session_id=excluded.session_id,mode=excluded.mode,epoch=excluded.epoch,last_used=excluded.last_used,expires_at=excluded.expires_at",params![input.actor_id,input.conversation_id,input.conversation_type,input.owner_subject,input.session_id,input.mode,number(epoch)?,now,now.checked_add(1_800_000).ok_or_else(||invalid("phone binding time overflow"))?]).map_err(map_rusqlite)?;
    binding_conn(
        conn,
        &input.actor_id,
        &input.conversation_id,
        &input.owner_subject,
    )?
    .ok_or_else(|| invalid("phone binding missing after write"))
}
pub async fn put_phone_binding(
    db: &RunDatabase,
    input: &PhoneBindingInput,
    now: u64,
) -> Result<PhoneBinding, StorageError> {
    validate_binding_input(input)?;
    let (input, now, busy) = (input.clone(), number(now)?, db.options().busy_timeout_ms);
    db.queue()
        .submit(move |conn| with_write_txn(conn, busy, |conn| put_binding_conn(conn, &input, now)))
        .await
}
/// 投递尚未受理且代次仍相同时才可绑定，和重置/撤权共享同一数据库写入序列。
pub async fn put_phone_binding_for_delivery(
    db: &RunDatabase,
    input: &PhoneBindingInput,
    delivery_id: &str,
    now: u64,
) -> Result<PhoneBinding, StorageError> {
    validate_binding_input(input)?;
    ident(delivery_id)?;
    let (mut input, id, now, busy) = (
        input.clone(),
        delivery_id.to_owned(),
        number(now)?,
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let delivery=delivery_conn(conn,&id)?.ok_or_else(||invalid("phone delivery missing"))?;
        let d=&delivery.input;
        if delivery.status!=PhoneDeliveryStatus::Claimed || actual_run(conn,&delivery)?.is_some() || d.actor_id!=input.actor_id || d.conversation_id!=input.conversation_id || d.conversation_type!=input.conversation_type || d.owner_subject!=input.owner_subject || d.input_json["sessionId"].as_str()!=Some(input.session_id.as_str()) {
            return Err(conflict("phone binding delivery authority changed"));
        }
        let expected_mode=match d.input_json["settings"]["mode"].as_str(){Some("read_only")=>"read_only",Some("write")=>"operate",_=>return Err(conflict("phone binding lacks frozen permission"))};
        if input.mode!=expected_mode{return Err(conflict("phone binding permission differs from delivery"));}
        let base=d.input_json["bindingBaseEpoch"].as_u64().ok_or_else(||conflict("phone binding lacks frozen base epoch"))?;
        let current:Option<i64>=conn.query_row("SELECT epoch FROM phone_binding_epochs_v1 WHERE actor_id=?1 AND conversation_id=?2 AND owner_subject=?3",params![input.actor_id,input.conversation_id,input.owner_subject],|r|r.get(0)).optional().map_err(map_rusqlite)?;
        let current=u64::try_from(current.unwrap_or(0)).map_err(|_|invalid("phone binding epoch damaged"))?;
        let old=binding_conn(conn,&input.actor_id,&input.conversation_id,&input.owner_subject)?;
        if let Some(old)=old {
            if current!=old.epoch{return Err(conflict("phone binding epoch receipt mismatch"));}
            let same_slot=old.input.session_id==input.session_id && old.input.mode==input.mode && old.input.conversation_type==input.conversation_type && (old.epoch==base || base.checked_add(1)==Some(old.epoch));
            if same_slot {
                if input.expected_epoch.is_some_and(|e|e!=old.epoch && e!=base){return Err(conflict("phone binding stale epoch"));}
                input.expected_epoch=Some(old.epoch);
            }else if current!=base {return Err(conflict("phone binding base epoch changed"));}
        }else if current!=base {return Err(conflict("phone binding reset epoch changed"));}
        put_binding_conn(conn,&input,now)
    })).await
}
pub async fn reset_phone_binding(
    db: &RunDatabase,
    actor: &str,
    conversation: &str,
    owner: &str,
    expected_epoch: u64,
) -> Result<bool, StorageError> {
    let (actor, conversation, owner, epoch, busy) = (
        actor.to_owned(),
        conversation.to_owned(),
        owner.to_owned(),
        number(expected_epoch)?,
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{let old=binding_conn(conn,&actor,&conversation,&owner)?;if old.is_none(){return Ok(false);}
if old.as_ref().is_some_and(|row|row.epoch!=epoch as u64){return Err(conflict("phone reset epoch conflict"));}Ok(conn.execute("DELETE FROM phone_bindings_v1 WHERE actor_id=?1 AND conversation_id=?2 AND owner_subject=?3 AND epoch=?4",params![actor,conversation,owner,epoch]).map_err(map_rusqlite)?==1)})).await
}

/// 文件已落盘但可能尚未来得及派发手机的一批真实动作，时间和正文沿用首次冻结值。
pub async fn pending_message_operations(
    db: &RunDatabase,
) -> Result<Vec<(String, ConversationMutation)>, StorageError> {
    db.queue().submit(|conn| {
        let mut statement = conn.prepare("SELECT operation_id,mutation_json FROM conversation_file_operations_v1 WHERE status='applied' AND json_extract(mutation_json,'$.kind') IN ('append_channel','append_dm','create_channel') AND NOT EXISTS(SELECT 1 FROM conversation_message_dispatch_v1 dispatch WHERE dispatch.operation_id=conversation_file_operations_v1.operation_id) ORDER BY created_at,rowid").map_err(map_rusqlite)?;
        let rows = statement.query_map([], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?))).map_err(map_rusqlite)?.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)?;
        rows.into_iter().map(|(id,raw)| {
            let mutation = serde_json::from_str(&raw).map_err(|_|StorageError::Corrupted{detail:"frozen message operation damaged".into()})?;
            Ok((id,mutation))
        }).collect()
    }).await
}

/// 先于文件副作用冻结宿主来源；不能让重启后的一次发帖变成新的传播根。
pub async fn bind_message_operation_source(
    db: &RunDatabase,
    operation_id: &str,
    source_run_id: &str,
    source_tool_call_id: &str,
) -> Result<(), StorageError> {
    for value in [operation_id, source_run_id, source_tool_call_id] {
        ident(value)?;
    }
    let (op, run, call, busy) = (
        operation_id.to_owned(),
        source_run_id.to_owned(),
        source_tool_call_id.to_owned(),
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let old:Option<(String,String)>=conn.query_row("SELECT source_run_id,source_tool_call_id FROM conversation_file_sources_v1 WHERE operation_id=?1",[&op],|row|Ok((row.get(0)?,row.get(1)?))).optional().map_err(map_rusqlite)?;
        if old.as_ref().is_some_and(|old|old!=&(run.clone(),call.clone())) { return Err(conflict("message operation source is immutable")); }
        let actual:Option<(String,String)>=conn.query_row("SELECT j.target,j.phase FROM invocation_journal j JOIN runs r ON r.run_id=j.run_id JOIN sessions s ON s.session_id=r.session_id WHERE j.journal_id=?1 AND j.run_id=?2 AND j.session_id=r.session_id AND j.owner_kind=r.owner_kind AND j.owner_subject=r.owner_subject AND j.generation=r.generation AND j.attempt=(SELECT attempt FROM run_attempts latest WHERE latest.run_id=r.run_id ORDER BY started_at_unix_ms DESC,rowid DESC LIMIT 1) AND EXISTS(SELECT 1 FROM run_attempts a WHERE a.run_id=r.run_id AND a.attempt=j.attempt AND a.generation=j.generation) AND s.owner_user_id=r.owner_subject AND r.status IN ('running','waiting_approval') AND s.lifecycle='active'",params![call,run],|row|Ok((row.get(0)?,row.get(1)?))).optional().map_err(map_rusqlite)?;
        let Some((target,phase))=actual else{return Err(conflict("message operation requires an actual live Run and journal call"));};
        if !matches!(target.as_str(),"tool:first-party:channel"|"tool:first-party:dm") || phase!="started" {return Err(conflict("message source call is not a dispatched conversation tool"));}
        if old.is_some() { return Ok(()); }
        conn.execute("INSERT INTO conversation_file_sources_v1 VALUES(?1,?2,?3)",params![op,run,call]).map_err(map_rusqlite)?;
        Ok(())
    })).await
}
pub async fn message_operation_source(
    db: &RunDatabase,
    operation_id: &str,
) -> Result<Option<String>, StorageError> {
    let id = operation_id.to_owned();
    db.queue()
        .submit(move |conn| {
            conn.query_row(
                "SELECT source_run_id FROM conversation_file_sources_v1 WHERE operation_id=?1",
                [id],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_rusqlite)
        })
        .await
}
/// 已经确认的真实回复决策独立于 Run 终态恢复，不重新调用模型、不重写已完成的文件。
pub async fn unpublished_decisions(db: &RunDatabase) -> Result<Vec<PhoneDelivery>, StorageError> {
    db.queue().submit(|conn|{
        let mut statement=conn.prepare(&format!("SELECT {DELIVERY_COLUMNS} FROM phone_deliveries_v1 d WHERE decision='reply' AND NOT EXISTS(SELECT 1 FROM conversation_file_operations_v1 f WHERE f.operation_id='phone-reply-'||d.delivery_id AND f.status='applied') ORDER BY created_at,delivery_id")).map_err(map_rusqlite)?;
        let rows=statement.query_map([],delivery_row).map_err(map_rusqlite)?.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)?;
        Ok(rows)
    }).await
}

/// 仅在所有收件人已领取或明确终止后记下终态，不把冷却中的待办吞掉。
pub async fn finish_message_operation(
    db: &RunDatabase,
    operation_id: &str,
    skipped_reason: Option<&str>,
    now: u64,
) -> Result<(), StorageError> {
    ident(operation_id)?;
    if let Some(reason) = skipped_reason {
        ident(reason)?;
    }
    let (op, reason, now, busy) = (
        operation_id.to_owned(),
        skipped_reason.map(str::to_owned),
        number(now)?,
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let status=if reason.is_some(){"skipped"}else{"completed"};
        let old:Option<(String,Option<String>)>=conn.query_row("SELECT status,reason FROM conversation_message_dispatch_v1 WHERE operation_id=?1",[&op],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(map_rusqlite)?;
        if let Some(old)=old{return if old==(status.into(),reason){Ok(())}else{Err(conflict("message dispatch terminal receipt is immutable"))};}
        let valid:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM conversation_file_operations_v1 WHERE operation_id=?1 AND status='applied' AND json_extract(mutation_json,'$.kind') IN ('append_channel','append_dm','create_channel'))",[&op],|r|r.get(0)).map_err(map_rusqlite)?;
        if !valid{return Err(conflict("message dispatch requires an applied real message operation"));}
        conn.execute("INSERT INTO conversation_message_dispatch_v1 VALUES(?1,?2,?3,?4)",params![op,status,reason,now]).map_err(map_rusqlite)?;
        Ok(())
    })).await
}
