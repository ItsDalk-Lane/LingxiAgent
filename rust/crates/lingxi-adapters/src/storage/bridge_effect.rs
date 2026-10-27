//! 等待提示和原生草稿的真实外部效果；与最终投递分开记账，未知效果禁止盲目重放。
use super::{bridge, migrations::map_rusqlite, run_store::with_write_txn, RunDatabase};
use lingxi_kernel::ports::StorageError;
pub use lingxi_protocol::{BridgeEffectOperation, BridgeEffectReceipt, BridgeEffectRequest};
use lingxi_protocol::{BridgeEffectStatus, BridgeReplyContext};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeEffectMode {
    ReceiptOnly,
    NativeStream,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoredEffectStatus {
    Started,
    Confirmed,
    Failed,
    Unknown,
    Skipped,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeEffectStream {
    pub stream_id: String,
    pub inbox_id: String,
    pub run_id: String,
    pub connector_id: String,
    pub connector_generation: u64,
    pub mode: BridgeEffectMode,
    pub rich: bool,
    pub receipt_independent: bool,
    pub conversation_id: String,
    // 回复令牌只给受控宿主构造 worker 命令，不随公开状态序列化。
    #[serde(skip_serializing)]
    pub reply_context: BridgeReplyContext,
    pub unknown: bool,
    pub closed: bool,
    pub last_sequence: u32,
    pub created_at: i64,
    pub updated_at: i64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeEffectInput {
    pub stream_id: String,
    pub request_key: String,
    pub operation: BridgeEffectOperation,
    pub text: String,
    pub delivery_id: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeEffect {
    pub connector_id: String,
    pub connector_generation: u64,
    pub request_key: String,
    pub request: BridgeEffectRequest,
    pub status: StoredEffectStatus,
    pub receipt: Option<BridgeEffectReceipt>,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeEffectClaim {
    pub claimed: bool,
    pub effect: BridgeEffect,
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
fn number(value: u64) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| invalid("Bridge effect integer overflow"))
}
fn id(value: &str) -> Result<(), StorageError> {
    if value.trim().is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
        Err(invalid("invalid Bridge effect identity"))
    } else {
        Ok(())
    }
}
fn json<T: Serialize>(value: &T) -> Result<String, StorageError> {
    serde_json::to_string(value).map_err(|e| invalid(e.to_string()))
}
fn wire<T: Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .expect("closed enum")
        .as_str()
        .expect("enum string")
        .to_string()
}
fn parse<T: serde::de::DeserializeOwned>(text: String, col: usize) -> rusqlite::Result<T> {
    serde_json::from_str(&text).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(col, rusqlite::types::Type::Text, Box::new(error))
    })
}
const STREAM_COLUMNS:&str="stream_id,inbox_id,run_id,connector_id,connector_generation,mode,rich,conversation_id,reply_context_json,unknown,closed,last_sequence,created_at,updated_at,receipt_independent";
fn stream_row(row: &Row<'_>) -> rusqlite::Result<BridgeEffectStream> {
    Ok(BridgeEffectStream {
        stream_id: row.get(0)?,
        inbox_id: row.get(1)?,
        run_id: row.get(2)?,
        connector_id: row.get(3)?,
        connector_generation: row.get::<_, i64>(4)? as u64,
        mode: parse(
            json(&row.get::<_, String>(5)?).map_err(|_| rusqlite::Error::InvalidQuery)?,
            5,
        )?,
        rich: row.get(6)?,
        receipt_independent: row.get(14)?,
        conversation_id: row.get(7)?,
        reply_context: parse(row.get(8)?, 8)?,
        unknown: row.get(9)?,
        closed: row.get(10)?,
        last_sequence: row.get::<_, i64>(11)? as u32,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}
fn stream_conn(conn: &Connection, id: &str) -> Result<Option<BridgeEffectStream>, StorageError> {
    conn.query_row(
        &format!("SELECT {STREAM_COLUMNS} FROM bridge_effect_streams_v1 WHERE stream_id=?1"),
        [id],
        stream_row,
    )
    .optional()
    .map_err(map_rusqlite)
}
const EFFECT_COLUMNS:&str="e.request_key,e.request_json,e.status,e.receipt_json,e.error,e.created_at,e.updated_at,s.connector_id,s.connector_generation";
fn effect_row(row: &Row<'_>) -> rusqlite::Result<BridgeEffect> {
    let receipt: Option<String> = row.get(3)?;
    Ok(BridgeEffect {
        request_key: row.get(0)?,
        request: parse(row.get(1)?, 1)?,
        status: parse(
            serde_json::to_string(&row.get::<_, String>(2)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            2,
        )?,
        receipt: receipt.map(|value| parse(value, 3)).transpose()?,
        error: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        connector_id: row.get(7)?,
        connector_generation: row.get::<_, i64>(8)? as u64,
    })
}
fn effect_conn(conn: &Connection, id: &str) -> Result<Option<BridgeEffect>, StorageError> {
    conn.query_row(&format!("SELECT {EFFECT_COLUMNS} FROM bridge_effects_v1 e JOIN bridge_effect_streams_v1 s ON s.stream_id=e.stream_id WHERE e.effect_id=?1"),[id],effect_row).optional().map_err(map_rusqlite)
}
pub async fn get_stream(
    db: &RunDatabase,
    id: &str,
) -> Result<Option<BridgeEffectStream>, StorageError> {
    let id = id.to_string();
    db.queue().submit(move |conn| stream_conn(conn, &id)).await
}
pub async fn find_stream(
    db: &RunDatabase,
    inbox_id: &str,
    run_id: &str,
) -> Result<Option<BridgeEffectStream>, StorageError> {
    let (inbox_id, run_id) = (inbox_id.to_string(), run_id.to_string());
    db.queue().submit(move |conn| {
        conn.query_row(&format!("SELECT {STREAM_COLUMNS} FROM bridge_effect_streams_v1 WHERE inbox_id=?1 AND run_id=?2"), params![inbox_id,run_id], stream_row).optional().map_err(map_rusqlite)
    }).await
}
pub async fn get_effect(db: &RunDatabase, id: &str) -> Result<Option<BridgeEffect>, StorageError> {
    let id = id.to_string();
    db.queue().submit(move |conn| effect_conn(conn, &id)).await
}
pub async fn list_open_streams(db: &RunDatabase) -> Result<Vec<BridgeEffectStream>, StorageError> {
    db.queue().submit(|conn|{let mut stmt=conn.prepare(&format!("SELECT {STREAM_COLUMNS} FROM bridge_effect_streams_v1 WHERE closed=0 ORDER BY created_at,stream_id")).map_err(map_rusqlite)?;let rows=stmt.query_map([],stream_row).map_err(map_rusqlite)?;rows.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)}).await
}

fn authority(
    conn: &Connection,
    stream: &BridgeEffectStream,
) -> Result<bridge::BridgeInbox, StorageError> {
    let cfg = bridge::current(conn, &stream.connector_id, stream.connector_generation)?;
    let inbox = bridge::inbox_conn(conn, &stream.inbox_id)?
        .ok_or_else(|| invalid("effect inbox missing"))?;
    if inbox.input.connector_id != stream.connector_id
        || inbox.connector_generation != stream.connector_generation
        || inbox.actor_id != cfg.input.actor_id
        || inbox.run_id.as_deref() != Some(&stream.run_id)
    {
        return Err(conflict("effect source binding changed"));
    }
    let (run, sid, _) = bridge::inbox_run_conn(conn, &inbox)?
        .ok_or_else(|| invalid("effect has no real causal Run"))?;
    if run != stream.run_id {
        return Err(conflict("effect causal Run mismatch"));
    }
    bridge::active_session(conn, &sid)?;
    let context = effect_context(&inbox)?;
    if inbox.input.conversation_id != stream.conversation_id || context != stream.reply_context {
        return Err(conflict("effect frozen destination changed"));
    }
    Ok(inbox)
}
fn effect_context(inbox: &bridge::BridgeInbox) -> Result<BridgeReplyContext, StorageError> {
    let mut context: BridgeReplyContext = serde_json::from_str(&bridge::reply_context(inbox)?)
        .map_err(|_| invalid("effect reply context invalid"))?;
    context.is_group = Some(inbox.input.conversation_kind == bridge::ConversationKind::Group);
    Ok(context)
}
pub async fn ensure_stream(
    db: &RunDatabase,
    inbox_id: &str,
    run_id: &str,
    mode: BridgeEffectMode,
    rich: bool,
    now: u64,
) -> Result<BridgeEffectStream, StorageError> {
    ensure_stream_with_receipt(db, inbox_id, run_id, mode, rich, false, now).await
}
/// 独立输入状态只能由宿主按已确认的平台能力冻结，不能根据错误文案猜测。
#[allow(clippy::too_many_arguments)]
pub async fn ensure_stream_with_receipt(
    db: &RunDatabase,
    inbox_id: &str,
    run_id: &str,
    mode: BridgeEffectMode,
    rich: bool,
    receipt_independent: bool,
    now: u64,
) -> Result<BridgeEffectStream, StorageError> {
    id(inbox_id)?;
    id(run_id)?;
    let (inbox_id, run_id, now, busy) = (
        inbox_id.to_string(),
        run_id.to_string(),
        number(now)?,
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let existing:Option<String>=conn.query_row("SELECT stream_id FROM bridge_effect_streams_v1 WHERE inbox_id=?1 AND run_id=?2",params![inbox_id,run_id],|row|row.get(0)).optional().map_err(map_rusqlite)?;
        if let Some(existing)=existing{return stream_conn(conn,&existing)?.ok_or_else(||invalid("effect stream missing"));}
        let inbox=bridge::inbox_conn(conn,&inbox_id)?.ok_or_else(||invalid("effect inbox missing"))?;
        let context=effect_context(&inbox)?;
        let stream_id=format!("bridge-stream:{}",lingxi_protocol::canon::canonical_sha256_hex(&serde_json::json!([inbox_id,run_id])));
        let stream=BridgeEffectStream{stream_id:stream_id.clone(),inbox_id:inbox_id.clone(),run_id:run_id.clone(),connector_id:inbox.input.connector_id.clone(),connector_generation:inbox.connector_generation,mode,rich,receipt_independent,conversation_id:inbox.input.conversation_id.clone(),reply_context:context,unknown:false,closed:false,last_sequence:0,created_at:now,updated_at:now};
        authority(conn,&stream)?;
        conn.execute("INSERT INTO bridge_effect_streams_v1(stream_id,inbox_id,run_id,connector_id,connector_generation,mode,rich,conversation_id,reply_context_json,unknown,closed,last_sequence,created_at,updated_at,receipt_independent) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,0,0,0,?10,?10,?11)",params![stream_id,inbox_id,run_id,stream.connector_id,number(stream.connector_generation)?,wire(mode),rich,stream.conversation_id,json(&stream.reply_context)?,now,receipt_independent]).map_err(map_rusqlite)?;
        Ok(stream)
    })).await
}
fn cleanup(op: BridgeEffectOperation) -> bool {
    matches!(
        op,
        BridgeEffectOperation::ReceiptStop | BridgeEffectOperation::StreamCancel
    )
}
pub async fn claim_effect(
    db: &RunDatabase,
    input: &BridgeEffectInput,
    now: u64,
) -> Result<BridgeEffectClaim, StorageError> {
    id(&input.stream_id)?;
    id(&input.request_key)?;
    if input.text.len() > 100_000 || input.text.contains('\0') {
        return Err(invalid("effect text exceeds bound"));
    }
    if let Some(delivery) = &input.delivery_id {
        id(delivery)?;
    }
    if (input.operation == BridgeEffectOperation::StreamFinish) != input.delivery_id.is_some() {
        return Err(invalid("only stream finish requires final delivery"));
    }
    if cleanup(input.operation) && !input.text.is_empty() {
        return Err(invalid("effect cleanup cannot introduce text"));
    }
    let (input, now, busy) = (input.clone(), number(now)?, db.options().busy_timeout_ms);
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let stream=stream_conn(conn,&input.stream_id)?.ok_or_else(||invalid("effect stream missing"))?;
        let effect_id=format!("bridge-effect:{}",lingxi_protocol::canon::canonical_sha256_hex(&serde_json::json!([input.stream_id,input.request_key])));
        if let Some(old)=effect_conn(conn,&effect_id)?{
            if old.request.operation!=input.operation || old.request.text!=input.text || old.request.delivery_id!=input.delivery_id{return Err(conflict("same effect request key has different payload"));}
            return Ok(BridgeEffectClaim{claimed:false,effect:old});
        }
        if stream.mode==BridgeEffectMode::ReceiptOnly && matches!(input.operation,BridgeEffectOperation::StreamUpdate|BridgeEffectOperation::StreamFinish|BridgeEffectOperation::StreamCancel){return Err(conflict("receipt-only stream cannot mutate native draft"));}
        if !cleanup(input.operation){
            if stream.closed || stream.unknown {return Err(conflict("closed or unknown stream cannot start another external effect"));}
            authority(conn,&stream)?;
        }
        let inflight:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM bridge_effects_v1 WHERE stream_id=?1 AND status='started')",[&input.stream_id],|row|row.get(0)).map_err(map_rusqlite)?;
        if inflight{return Err(conflict("stream already has an in-flight effect"));}
        if let Some(delivery)=&input.delivery_id {
            let outbox=bridge::outbox_conn(conn,delivery)?.ok_or_else(||invalid("final outbox missing"))?;
            let (_,target)=bridge::outbox_target(conn,&outbox.input,Some(&outbox.target_inbox_id))?;
            let payload:serde_json::Value=serde_json::from_str(&outbox.input.payload_json).map_err(|_|invalid("final payload invalid"))?;
            if outbox.status!=bridge::BridgeOutboxStatus::Started || outbox.input.inbox_id.as_deref()!=Some(&stream.inbox_id) || outbox.input.run_id.as_deref()!=Some(&stream.run_id) || outbox.target_inbox_id!=stream.inbox_id || target.input.conversation_id!=stream.conversation_id || outbox.reply_context_json!=bridge::reply_context(&target)? || payload["text"].as_str()!=Some(&input.text) || !outbox.input.attachments.is_empty(){return Err(conflict("stream finish must use the real started final outbox and exact text"));}
            let prior:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM bridge_effects_v1 WHERE stream_id=?1 AND operation='stream_finish')",[&input.stream_id],|row|row.get(0)).map_err(map_rusqlite)?;
            if prior{return Err(conflict("stream final delivery already attempted"));}
        }
        let sequence=stream.last_sequence.checked_add(1).ok_or_else(||invalid("effect sequence exhausted"))?;
        let request=BridgeEffectRequest{effect_id:effect_id.clone(),stream_id:stream.stream_id.clone(),inbox_id:stream.inbox_id.clone(),run_id:stream.run_id.clone(),sequence,operation:input.operation,delivery_id:input.delivery_id.clone(),conversation_id:stream.conversation_id.clone(),reply_context:stream.reply_context.clone(),text:input.text.clone(),rich:stream.rich};
        request.validate().map_err(invalid)?;
        conn.execute("INSERT INTO bridge_effects_v1(effect_id,stream_id,request_key,sequence,operation,request_json,status,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,'started',?7,?7)",params![effect_id,input.stream_id,input.request_key,sequence,wire(input.operation),json(&request)?,now]).map_err(map_rusqlite)?;
        conn.execute("UPDATE bridge_effect_streams_v1 SET last_sequence=?2,updated_at=?3 WHERE stream_id=?1",params![input.stream_id,sequence,now]).map_err(map_rusqlite)?;
        Ok(BridgeEffectClaim{claimed:true,effect:effect_conn(conn,&effect_id)?.ok_or_else(||invalid("effect missing after claim"))?})
    })).await
}
pub async fn settle_effect(
    db: &RunDatabase,
    receipt: &BridgeEffectReceipt,
    now: u64,
) -> Result<BridgeEffect, StorageError> {
    receipt.validate().map_err(invalid)?;
    let (receipt, now, busy) = (receipt.clone(), number(now)?, db.options().busy_timeout_ms);
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let effect=effect_conn(conn,&receipt.effect_id)?.ok_or_else(||invalid("effect receipt has no claim"))?;
        let request=&effect.request;
        if request.stream_id!=receipt.stream_id || request.sequence!=receipt.sequence || request.operation!=receipt.operation || request.delivery_id!=receipt.delivery_id{return Err(conflict("effect receipt identity mismatch"));}
        if effect.status!=StoredEffectStatus::Started{return if effect.receipt.as_ref()==Some(&receipt){Ok(effect)}else{Err(conflict("terminal effect receipt cannot change"))};}
        let stream=stream_conn(conn,&request.stream_id)?.ok_or_else(||invalid("effect stream missing"))?;
        if let Some(delivery)=&request.delivery_id {
            let outbox=bridge::outbox_conn(conn,delivery)?.ok_or_else(||invalid("final outbox missing"))?;
            if outbox.status!=bridge::BridgeOutboxStatus::Started{return Err(conflict("final outbox is no longer started"));}
            let status=match receipt.status{BridgeEffectStatus::Confirmed=>bridge::BridgeOutboxStatus::Confirmed,BridgeEffectStatus::Unknown=>bridge::BridgeOutboxStatus::Unknown,_=>bridge::BridgeOutboxStatus::Failed};
            let platform=bridge::connector_conn(conn,&effect.connector_id)?.ok_or_else(||invalid("effect connector missing"))?.input.platform;
            let platform_receipt=if status==bridge::BridgeOutboxStatus::Confirmed{Some(json(&bridge::BridgePlatformReceipt{platform,message_ids:receipt.platform_message_ids.clone(),raw_json:None})?)}else{None};
            conn.execute("UPDATE bridge_outbox_v1 SET status=?2,platform_receipt_json=?3,error=?4,updated_at=?5 WHERE delivery_id=?1",params![delivery,wire(status),platform_receipt,receipt.error,now]).map_err(map_rusqlite)?;
        }
        let unknown=receipt.status==BridgeEffectStatus::Unknown
            && request.operation!=BridgeEffectOperation::ReceiptStop
            && !(request.operation==BridgeEffectOperation::ReceiptStart && stream.receipt_independent);
        let closes=matches!(request.operation,BridgeEffectOperation::StreamFinish|BridgeEffectOperation::StreamCancel) || (request.operation==BridgeEffectOperation::ReceiptStop && stream.mode==BridgeEffectMode::ReceiptOnly);
        conn.execute("UPDATE bridge_effects_v1 SET status=?2,receipt_json=?3,error=?4,updated_at=?5 WHERE effect_id=?1",params![receipt.effect_id,wire(receipt.status),json(&receipt)?,receipt.error,now]).map_err(map_rusqlite)?;
        conn.execute("UPDATE bridge_effect_streams_v1 SET unknown=MAX(unknown,?2),closed=MAX(closed,?3),updated_at=?4 WHERE stream_id=?1",params![receipt.stream_id,unknown,closes,now]).map_err(map_rusqlite)?;
        effect_conn(conn,&receipt.effect_id)?.ok_or_else(||invalid("effect missing after receipt"))
    })).await
}
pub async fn recover_started_effects(db: &RunDatabase, now: u64) -> Result<usize, StorageError> {
    let (now, busy) = (number(now)?, db.options().busy_timeout_ms);
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        conn.execute("UPDATE bridge_outbox_v1 SET status='unknown',error='native effect result unknown after restart; do not resend',updated_at=?1 WHERE status='started' AND delivery_id IN (SELECT json_extract(request_json,'$.deliveryId') FROM bridge_effects_v1 WHERE status='started' AND operation='stream_finish')",[now]).map_err(map_rusqlite)?;
        // worker 内的草稿/等待句柄随进程丢失，最后一项已确认也不能重建外部效果。
        conn.execute("UPDATE bridge_effect_streams_v1 SET unknown=1,updated_at=?1 WHERE unknown=0 AND ((closed=0 AND last_sequence>0) OR stream_id IN (SELECT stream_id FROM bridge_effects_v1 WHERE status='started'))",[now]).map_err(map_rusqlite)?;
        conn.execute("UPDATE bridge_effects_v1 SET status='unknown',error='effect result unknown after restart; do not replay',updated_at=?1 WHERE status='started'",[now]).map_err(map_rusqlite)
    })).await
}

/// 单个 worker 退出只收口自己的代次，不能把其他仍在运行的连接置为未知。
pub async fn abandon_connector_streams(
    db: &RunDatabase,
    connector_id: &str,
    generation: u64,
    now: u64,
) -> Result<usize, StorageError> {
    let (connector_id, generation, now, busy) = (
        connector_id.to_string(),
        number(generation)?,
        number(now)?,
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move |conn| with_write_txn(conn, busy, |conn| {
        conn.execute("UPDATE bridge_outbox_v1 SET status='unknown',error='native effect result unknown after worker exit; do not resend',updated_at=?3 WHERE connector_id=?1 AND connector_generation=?2 AND status='started' AND delivery_id IN (SELECT json_extract(e.request_json,'$.deliveryId') FROM bridge_effects_v1 e JOIN bridge_effect_streams_v1 s ON s.stream_id=e.stream_id WHERE s.connector_id=?1 AND s.connector_generation=?2 AND e.status='started' AND e.operation='stream_finish')",params![connector_id,generation,now]).map_err(map_rusqlite)?;
        conn.execute("UPDATE bridge_effect_streams_v1 SET unknown=1,updated_at=?3 WHERE connector_id=?1 AND connector_generation=?2 AND unknown=0 AND ((closed=0 AND last_sequence>0) OR stream_id IN (SELECT stream_id FROM bridge_effects_v1 WHERE status='started'))",params![connector_id,generation,now]).map_err(map_rusqlite)?;
        conn.execute("UPDATE bridge_effects_v1 SET status='unknown',error='effect result unknown after worker exit; do not replay',updated_at=?3 WHERE status='started' AND stream_id IN (SELECT stream_id FROM bridge_effect_streams_v1 WHERE connector_id=?1 AND connector_generation=?2)",params![connector_id,generation,now]).map_err(map_rusqlite)
    })).await
}
