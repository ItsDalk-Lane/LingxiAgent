//! 真实送达之后追加独立宿主事实；收据与分支条目同事务，清历史后不重放。
use super::{migrations::map_rusqlite, run_store::with_write_txn, RunDatabase};
use lingxi_kernel::ports::StorageError;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub const ENTRY_TYPE: &str = "lingxi-delivered-notification-v1";
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeNoticeFact {
    pub delivery_id: String,
    pub session_id: String,
    pub actor_subject: String,
    pub agent_id: String,
    pub platform: String,
    pub platform_message_ids: Vec<String>,
    pub text: String,
    pub delivered_at: i64,
}
fn invalid(detail: impl Into<String>) -> StorageError {
    StorageError::InvalidRequest {
        detail: format!("bridge notice: {}", detail.into()),
    }
}
fn corrupt(detail: impl Into<String>) -> StorageError {
    StorageError::Corrupted {
        detail: format!("bridge notice: {}", detail.into()),
    }
}
/// 只接收投递ID，正文、身份、接收会话与平台回执均重新读取持久事实。
pub async fn record_delivered_bridge_notice(
    db: &RunDatabase,
    delivery_id: &str,
    now: u64,
) -> Result<Option<BridgeNoticeFact>, StorageError> {
    let (id, now, busy) = (
        delivery_id.to_owned(),
        i64::try_from(now).map_err(|_| invalid("clock overflow"))?,
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn| {
        type Delivery=(String,String,Option<String>,String,String,i64);
        let row:Delivery=conn.query_row("SELECT payload_json,status,platform_receipt_json,target_inbox_id,connector_id,updated_at FROM bridge_outbox_v1 WHERE delivery_id=?1",[&id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional().map_err(map_rusqlite)?.ok_or_else(||invalid("delivery missing"))?;
        if row.1!="confirmed" {return Err(invalid("delivery is not confirmed"));}
        let payload:Value=serde_json::from_str(&row.0).map_err(|_|corrupt("frozen payload malformed"))?;
        if payload["contextPolicy"]!="record_when_delivered" {return Ok(None);}
        let receipt:super::bridge::BridgePlatformReceipt=serde_json::from_str(row.2.as_deref().ok_or_else(||corrupt("confirmed platform receipt missing"))?).map_err(|_|corrupt("platform receipt malformed"))?;
        if receipt.message_ids.is_empty()||receipt.message_ids.iter().any(|id|id.trim().is_empty()) {return Err(corrupt("confirmed platform message IDs missing"));}
        let old:Option<String>=conn.query_row("SELECT fact_json FROM bridge_notice_receipts_v1 WHERE delivery_id=?1",[&id],|r|r.get(0)).optional().map_err(map_rusqlite)?;
        if let Some(old)=old {let fact:BridgeNoticeFact=serde_json::from_str(&old).map_err(|_|corrupt("saved notice receipt malformed"))?;if fact.delivery_id!=id{return Err(corrupt("notice receipt identity mismatch"));}return Ok(Some(fact));}
        type Recipient=(String,String,String,String,String,String,String,String);
        let target:Recipient=conn.query_row("SELECT platform,connector_id,conversation_id,sender_id,actor_id,role,conversation_kind,owner_subject FROM bridge_inbox_v1 WHERE id=?1",[&row.3],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))).optional().map_err(map_rusqlite)?.ok_or_else(||corrupt("frozen owner target missing"))?;
        if target.1!=row.4 || target.5!="owner"||target.6!="dm"||target.7!=lingxi_kernel::LOCAL_OWNER_SUBJECT||receipt.platform!=target.0 {return Err(corrupt("delivery does not name an actual owner DM"));}
        let session=payload["contextSessionId"].as_str().filter(|s|!s.is_empty()).ok_or_else(||invalid("frozen context session missing"))?;
        let epoch=payload["contextEpoch"].as_u64().filter(|n|*n>0).ok_or_else(||invalid("frozen context epoch missing"))?;
        let expected=lingxi_protocol::bridge_conversation_session_id(&target.0,&target.1,&target.2,&target.3,&target.4,epoch).map_err(invalid)?;
        if expected!=session {return Err(corrupt("context session is not the frozen owner conversation"));}
        let subject=lingxi_protocol::bridge_subject(&target.0,&target.1,&target.3).map_err(invalid)?;
        let session_valid:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM sessions WHERE session_id=?1 AND owner_user_id=?2 AND agent_id=?3 AND lifecycle='active')",params![session,subject,target.4],|r|r.get(0)).map_err(map_rusqlite)?;
        if !session_valid{return Err(invalid("owner context session changed or archived"));}
        let text=payload["text"].as_str().filter(|s|!s.trim().is_empty()&&s.len()<=128*1024).ok_or_else(||invalid("frozen delivery text missing or too large"))?.to_owned();
        let fact=BridgeNoticeFact{delivery_id:id.clone(),session_id:session.into(),actor_subject:subject,agent_id:target.4,platform:target.0,platform_message_ids:receipt.message_ids,text,delivered_at:row.5};
        let body=serde_json::to_string(&fact).map_err(|e|corrupt(e.to_string()))?;
        let message_id=format!("bridge-notice:{}",lingxi_protocol::canon::sha256_hex(id.as_bytes()));
        let parent:Option<Option<String>>=conn.query_row("SELECT head_message_id FROM session_branch_heads WHERE session_id=?1",[session],|r|r.get(0)).optional().map_err(map_rusqlite)?;
        let parent=match parent {Some(head)=>head,None=>conn.query_row("SELECT message_id FROM messages WHERE session_id=?1 AND entry_type='message' ORDER BY seq DESC LIMIT 1",[session],|r|r.get(0)).optional().map_err(map_rusqlite)?};
        let seq:i64=conn.query_row("SELECT COALESCE(MAX(seq),0)+1 FROM messages WHERE session_id=?1",[session],|r|r.get(0)).map_err(map_rusqlite)?;
        conn.execute("INSERT INTO messages(message_id,session_id,run_id,role,content_json,model_call_id,committed_at_unix_ms,seq,parent_message_id,entry_type) VALUES(?1,?2,NULL,'host_context',?3,NULL,?4,?5,?6,?7)",params![message_id,session,body,now,seq,parent,ENTRY_TYPE]).map_err(map_rusqlite)?;
        conn.execute("INSERT INTO session_branch_heads(session_id,head_message_id,observed_tail_message_id,revision,head_resolution,updated_at_unix_ms) VALUES(?1,?2,?2,1,'persisted_head',?3) ON CONFLICT(session_id) DO UPDATE SET head_message_id=excluded.head_message_id,revision=session_branch_heads.revision+1,head_resolution='persisted_head',updated_at_unix_ms=excluded.updated_at_unix_ms",params![session,message_id,now]).map_err(map_rusqlite)?;
        conn.execute("INSERT INTO bridge_notice_receipts_v1(delivery_id,session_id,message_id,fact_json,created_at) VALUES(?1,?2,?3,?4,?5)",params![id,session,message_id,body,now]).map_err(map_rusqlite)?;
        Ok(Some(fact))
    })).await
}
/// 历史只能恢复已有的原子写入收据，不能拿普通文本冒充宿主事实。
pub async fn load_notice_fact(
    db: &RunDatabase,
    delivery_id: &str,
) -> Result<Option<BridgeNoticeFact>, StorageError> {
    let raw=db.query_one_text("SELECT json_object('fact',json(n.fact_json),'payload',json(o.payload_json),'receipt',json(o.platform_receipt_json),'platform',i.platform,'connector',i.connector_id,'conversation',i.conversation_id,'sender',i.sender_id,'agent',i.actor_id,'role',i.role,'conversationKind',i.conversation_kind,'owner',i.owner_subject) FROM bridge_notice_receipts_v1 n JOIN bridge_outbox_v1 o ON o.delivery_id=n.delivery_id JOIN bridge_inbox_v1 i ON i.id=o.target_inbox_id WHERE n.delivery_id=?1 AND o.status='confirmed' AND o.platform_receipt_json IS NOT NULL",vec![delivery_id.into()]).await?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    let source: Value =
        serde_json::from_str(&raw).map_err(|_| corrupt("saved notice evidence malformed"))?;
    let fact: BridgeNoticeFact = serde_json::from_value(source["fact"].clone())
        .map_err(|_| corrupt("saved notice fact malformed"))?;
    let receipt: super::bridge::BridgePlatformReceipt =
        serde_json::from_value(source["receipt"].clone())
            .map_err(|_| corrupt("saved platform receipt malformed"))?;
    let text = |name: &str| {
        source[name]
            .as_str()
            .ok_or_else(|| corrupt(format!("notice evidence {name} missing")))
    };
    let actor =
        lingxi_protocol::bridge_subject(text("platform")?, text("connector")?, text("sender")?)
            .map_err(corrupt)?;
    let epoch = source["payload"]["contextEpoch"]
        .as_u64()
        .ok_or_else(|| corrupt("notice context epoch missing"))?;
    let session = lingxi_protocol::bridge_conversation_session_id(
        text("platform")?,
        text("connector")?,
        text("conversation")?,
        text("sender")?,
        text("agent")?,
        epoch,
    )
    .map_err(corrupt)?;
    if fact.delivery_id != delivery_id
        || fact.platform != text("platform")?
        || fact.platform != receipt.platform
        || fact.platform_message_ids != receipt.message_ids
        || fact.platform_message_ids.is_empty()
        || fact.actor_subject != actor
        || fact.agent_id != text("agent")?
        || fact.session_id != session
        || source["payload"]["contextSessionId"] != fact.session_id
        || source["payload"]["text"] != fact.text
        || source["payload"]["contextPolicy"] != "record_when_delivered"
        || text("role")? != "owner"
        || text("conversationKind")? != "dm"
        || text("owner")? != lingxi_kernel::LOCAL_OWNER_SUBJECT
    {
        return Err(corrupt(
            "saved notice differs from the actual confirmed delivery",
        ));
    }
    Ok(Some(fact))
}
