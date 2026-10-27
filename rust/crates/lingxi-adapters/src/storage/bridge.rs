//! Bridge 持久事实由服务写入；消息正文和外围适配器不能自行授予身份或选择回发目标。
use super::{migrations::map_rusqlite, run_store::with_write_txn, RunDatabase};
use lingxi_kernel::{ports::StorageError, LOCAL_OWNER_SUBJECT};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeConnectorInput {
    pub id: String,
    pub platform: String,
    pub studio_id: String,
    pub actor_id: String,
    pub owner_subject: String,
    pub owner_sender_id: Option<String>,
    pub enabled: bool,
    /// 完整原文兼容未知字段；不从中读取消息主体或授权。
    pub config_json: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeConnector {
    #[serde(flatten)]
    pub input: BridgeConnectorInput,
    pub generation: u64,
    pub created_at: i64,
    pub updated_at: i64,
}
pub use lingxi_protocol::{
    BridgeAttachment, BridgeAttachmentKind, BridgeConversationKind as ConversationKind, BridgeRole,
};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeInboxInput {
    pub platform: String,
    pub connector_id: String,
    pub conversation_id: String,
    pub conversation_kind: ConversationKind,
    pub message_id: String,
    pub sender_id: String,
    pub payload_json: String,
    pub attachments: Vec<BridgeAttachment>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BridgeInboxStatus {
    Command,
    Claimed,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Unknown,
}
impl BridgeInboxStatus {
    fn terminal(self) -> bool {
        !matches!(self, Self::Claimed | Self::Running)
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeInboxFile {
    pub attachment_id: String,
    pub file_id: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeInbox {
    pub id: String,
    #[serde(flatten)]
    pub input: BridgeInboxInput,
    pub role: BridgeRole,
    pub actor_id: String,
    pub owner_subject: String,
    pub connector_generation: u64,
    pub status: BridgeInboxStatus,
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    pub request_id: String,
    pub prepared: bool,
    pub prepared_takeover: bool,
    pub conversation_epoch: Option<u64>,
    pub resolved_files: Vec<BridgeInboxFile>,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeInboxClaim {
    pub claimed: bool,
    pub inbox: BridgeInbox,
    /// 当前宿主配置；重复旧消息仍返回原 inbox，不会再次领取。
    pub connector: BridgeConnector,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeOutboxInput {
    pub delivery_id: String,
    pub connector_id: String,
    pub connector_generation: u64,
    /// None 只可通知真实配置的 owner，并从同代次真实私聊记录确定目标。
    pub inbox_id: Option<String>,
    pub manual_action_id: Option<String>,
    pub run_id: Option<String>,
    /// 循环投递台账来源：loop-reply/loop-notice 之外的投递必须为 None。
    pub loop_delivery_id: Option<String>,
    pub payload_json: String,
    pub attachments: Vec<BridgeAttachment>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BridgeOutboxStatus {
    Queued,
    Started,
    Confirmed,
    Unknown,
    Failed,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgePlatformReceipt {
    pub platform: String,
    pub message_ids: Vec<String>,
    pub raw_json: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeOutbox {
    #[serde(flatten)]
    pub input: BridgeOutboxInput,
    pub conversation_id: String,
    pub conversation_kind: ConversationKind,
    pub target_inbox_id: String,
    /// 仅供受控发送端消费；状态序列化剔除私密上下文。
    #[serde(skip_serializing)]
    pub reply_context_json: String,
    pub status: BridgeOutboxStatus,
    pub platform_receipt: Option<BridgePlatformReceipt>,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeOutboxClaim {
    pub claimed: bool,
    pub outbox: BridgeOutbox,
}

fn invalid(detail: impl Into<String>) -> StorageError {
    StorageError::InvalidRequest {
        detail: format!("bridge: {}", detail.into()),
    }
}
fn conflict(detail: impl Into<String>) -> StorageError {
    StorageError::Conflict {
        detail: format!("bridge: {}", detail.into()),
    }
}
fn text_id(value: &str) -> Result<(), StorageError> {
    if value.trim().is_empty() || value.len() > 4096 || value.contains('\0') {
        return Err(invalid("invalid identity"));
    }
    Ok(())
}
fn json(raw: &str) -> Result<(), StorageError> {
    if raw.len() > 2 * 1024 * 1024 {
        return Err(invalid("JSON exceeds budget"));
    }
    if !serde_json::from_str::<Value>(raw)
        .map_err(|e| invalid(e.to_string()))?
        .is_object()
    {
        return Err(invalid("JSON object required"));
    }
    Ok(())
}
fn number(value: u64) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| invalid("integer overflow"))
}
fn generation(value: u64) -> Result<i64, StorageError> {
    if value == 0 {
        return Err(invalid("generation must be positive"));
    }
    number(value)
}
fn wire<T: Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .expect("enum serialization")
        .as_str()
        .expect("enum string")
        .to_string()
}
fn from_json<T: serde::de::DeserializeOwned>(raw: String, index: usize) -> rusqlite::Result<T> {
    serde_json::from_str(&raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(e))
    })
}
fn enum_row<T: serde::de::DeserializeOwned>(row: &Row<'_>, index: usize) -> rusqlite::Result<T> {
    from_json(format!("\"{}\"", row.get::<_, String>(index)?), index)
}
fn u64_row(row: &Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let v: i64 = row.get(index)?;
    u64::try_from(v).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, v))
}
fn optional_json<T: serde::de::DeserializeOwned>(
    row: &Row<'_>,
    index: usize,
) -> rusqlite::Result<Option<T>> {
    row.get::<_, Option<String>>(index)?
        .map(|raw| from_json(raw, index))
        .transpose()
}
fn attachments_valid(items: &[BridgeAttachment]) -> Result<(), StorageError> {
    if items.len() > 8 {
        return Err(invalid("too many attachments"));
    }
    let mut total = 0u64;
    let mut ids = std::collections::HashSet::new();
    for item in items {
        text_id(&item.attachment_id)?;
        if !ids.insert(&item.attachment_id) {
            return Err(invalid("duplicate attachment identity"));
        }
        if let Some(size) = item.size_bytes {
            total += u64::from(size);
            if size > 50 * 1024 * 1024 || total > 64 * 1024 * 1024 {
                return Err(invalid("attachment byte budget exceeded"));
            }
        }
        if let Some(reference) = &item.platform_ref {
            text_id(reference)?;
            let reference = reference.trim();
            if reference.starts_with('/')
                || reference.starts_with('\\')
                || reference.to_ascii_lowercase().starts_with("file:")
                || (reference.len() > 2 && reference.as_bytes()[1] == b':')
            {
                return Err(invalid("local attachment path is not a platform reference"));
            }
        }
        if item
            .filename
            .as_ref()
            .is_some_and(|name| name.contains('/') || name.contains('\\') || name.contains('\0'))
        {
            return Err(invalid("attachment filename must be a basename"));
        }
    }
    Ok(())
}
fn connector_row(row: &Row<'_>) -> rusqlite::Result<BridgeConnector> {
    Ok(BridgeConnector {
        input: BridgeConnectorInput {
            id: row.get(0)?,
            platform: row.get(1)?,
            studio_id: row.get(2)?,
            actor_id: row.get(3)?,
            owner_subject: row.get(4)?,
            owner_sender_id: row.get(5)?,
            enabled: row.get(7)?,
            config_json: row.get(8)?,
        },
        generation: u64_row(row, 6)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}
const CONNECTOR_COLUMNS:&str="id,platform,studio_id,actor_id,owner_subject,owner_sender_id,generation,enabled,config_json,created_at,updated_at";
pub(crate) fn connector_conn(
    conn: &Connection,
    id: &str,
) -> Result<Option<BridgeConnector>, StorageError> {
    conn.query_row(
        &format!("SELECT {CONNECTOR_COLUMNS} FROM bridge_connectors_v1 WHERE id=?1"),
        [id],
        connector_row,
    )
    .optional()
    .map_err(map_rusqlite)
}
pub(crate) fn current(
    conn: &Connection,
    id: &str,
    expected: u64,
) -> Result<BridgeConnector, StorageError> {
    let cfg = connector_conn(conn, id)?.ok_or_else(|| invalid("connector missing"))?;
    if !cfg.input.enabled || cfg.generation != expected {
        return Err(conflict("connector revoked or generation changed"));
    }
    Ok(cfg)
}
pub async fn put_connector(
    db: &RunDatabase,
    input: &BridgeConnectorInput,
    expected_generation: Option<u64>,
    now: u64,
) -> Result<BridgeConnector, StorageError> {
    for id in [
        &input.id,
        &input.platform,
        &input.studio_id,
        &input.actor_id,
    ] {
        text_id(id)?;
    }
    if !matches!(
        input.platform.as_str(),
        "telegram" | "feishu" | "dingtalk" | "qq" | "wechat"
    ) || input.owner_subject != LOCAL_OWNER_SUBJECT
    {
        return Err(invalid("invalid platform or owner"));
    }
    if let Some(sender) = &input.owner_sender_id {
        text_id(sender)?;
    }
    lingxi_protocol::bridge_subject(
        &input.platform,
        &input.id,
        input.owner_sender_id.as_deref().unwrap_or("guest"),
    )
    .map_err(invalid)?;
    json(&input.config_json)?;
    let (input, now, busy) = (input.clone(), number(now)?, db.options().busy_timeout_ms);
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let existing=connector_conn(conn,&input.id)?;
        let (next,created)=match (existing,expected_generation){
            (None,None)=>(1,now),
            (Some(old),Some(expected)) if old.generation==expected=>{
                if old.input.platform!=input.platform || old.input.studio_id!=input.studio_id{return Err(conflict("connector identity cannot change"));}
                (generation(expected)?.checked_add(1).ok_or_else(||invalid("generation overflow"))?,old.created_at)
            },
            _=>return Err(conflict("connector expected generation conflict")),
        };
        conn.execute("INSERT INTO bridge_connectors_v1(id,platform,studio_id,actor_id,owner_subject,owner_sender_id,generation,enabled,config_json,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) ON CONFLICT(id) DO UPDATE SET actor_id=excluded.actor_id,owner_subject=excluded.owner_subject,owner_sender_id=excluded.owner_sender_id,generation=excluded.generation,enabled=excluded.enabled,config_json=excluded.config_json,updated_at=excluded.updated_at",params![input.id,input.platform,input.studio_id,input.actor_id,input.owner_subject,input.owner_sender_id,next,input.enabled,input.config_json,created,now]).map_err(map_rusqlite)?;
        conn.execute("UPDATE bridge_outbox_v1 SET status='failed',error='connector reconfigured before send',updated_at=?2 WHERE connector_id=?1 AND status='queued' AND connector_generation<>?3",params![input.id,now,next]).map_err(map_rusqlite)?;
        conn.execute("UPDATE bridge_inbox_v1 SET status='unknown',error='connector reconfigured before execution; do not replay',updated_at=?2 WHERE connector_id=?1 AND connector_generation<>?3 AND status='claimed' AND run_id IS NULL",params![input.id,now,next]).map_err(map_rusqlite)?;
        connector_conn(conn,&input.id)?.ok_or_else(||invalid("connector missing after write"))
    })).await
}
pub async fn get_connector(
    db: &RunDatabase,
    id: &str,
) -> Result<Option<BridgeConnector>, StorageError> {
    let id = id.to_string();
    db.queue()
        .submit(move |conn| connector_conn(conn, &id))
        .await
}
pub async fn list_connectors(db: &RunDatabase) -> Result<Vec<BridgeConnector>, StorageError> {
    db.queue()
        .submit(move |conn| {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {CONNECTOR_COLUMNS} FROM bridge_connectors_v1 ORDER BY id"
                ))
                .map_err(map_rusqlite)?;
            let rows = stmt.query_map([], connector_row).map_err(map_rusqlite)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(map_rusqlite)
        })
        .await
}

/// 长度前缀避免拼接歧义；不含配置代次，重配不能把重投变成新任务。
pub fn inbox_id(input: &BridgeInboxInput) -> String {
    let mut hash = Sha256::new();
    for part in [
        &input.platform,
        &input.connector_id,
        &input.conversation_id,
        &input.message_id,
    ] {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part.as_bytes());
    }
    format!(
        "bridge-{}",
        hash.finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}
const INBOX_COLUMNS:&str="id,platform,connector_id,conversation_id,conversation_kind,message_id,sender_id,role,connector_generation,payload_json,attachments_json,status,session_id,run_id,request_id,result_json,error,created_at,updated_at,actor_id,owner_subject,prepared,resolved_files_json,conversation_epoch,prepared_takeover";
fn inbox_row(row: &Row<'_>) -> rusqlite::Result<BridgeInbox> {
    Ok(BridgeInbox {
        id: row.get(0)?,
        input: BridgeInboxInput {
            platform: row.get(1)?,
            connector_id: row.get(2)?,
            conversation_id: row.get(3)?,
            conversation_kind: enum_row(row, 4)?,
            message_id: row.get(5)?,
            sender_id: row.get(6)?,
            payload_json: row.get(9)?,
            attachments: from_json(row.get(10)?, 10)?,
        },
        role: enum_row(row, 7)?,
        actor_id: row.get(19)?,
        owner_subject: row.get(20)?,
        connector_generation: u64_row(row, 8)?,
        status: enum_row(row, 11)?,
        session_id: row.get(12)?,
        run_id: row.get(13)?,
        request_id: row.get(14)?,
        prepared: row.get(21)?,
        prepared_takeover: row.get(24)?,
        conversation_epoch: row
            .get::<_, Option<i64>>(23)?
            .map(|value| {
                u64::try_from(value)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(23, value))
            })
            .transpose()?,
        resolved_files: from_json(row.get(22)?, 22)?,
        result: optional_json(row, 15)?,
        error: row.get(16)?,
        created_at: row.get(17)?,
        updated_at: row.get(18)?,
    })
}
pub(crate) fn inbox_conn(conn: &Connection, id: &str) -> Result<Option<BridgeInbox>, StorageError> {
    conn.query_row(
        &format!("SELECT {INBOX_COLUMNS} FROM bridge_inbox_v1 WHERE id=?1"),
        [id],
        inbox_row,
    )
    .optional()
    .map_err(map_rusqlite)
}
pub async fn get_inbox(db: &RunDatabase, id: &str) -> Result<Option<BridgeInbox>, StorageError> {
    let id = id.to_string();
    db.queue().submit(move |conn| inbox_conn(conn, &id)).await
}
pub async fn claim_inbox(
    db: &RunDatabase,
    input: &BridgeInboxInput,
    expected_generation: u64,
    now: u64,
) -> Result<BridgeInboxClaim, StorageError> {
    for id in [
        &input.platform,
        &input.connector_id,
        &input.conversation_id,
        &input.message_id,
        &input.sender_id,
    ] {
        text_id(id)?;
    }
    json(&input.payload_json)?;
    attachments_valid(&input.attachments)?;
    let (input, now, busy) = (input.clone(), number(now)?, db.options().busy_timeout_ms);
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let id=inbox_id(&input);let cfg=connector_conn(conn,&input.connector_id)?.ok_or_else(||invalid("connector missing"))?;
        if let Some(old)=inbox_conn(conn,&id)?{
            if old.input!=input{return Err(conflict("same message identity has different payload"));}
            return Ok(BridgeInboxClaim{claimed:false,inbox:old,connector:cfg});
        }
        if !cfg.input.enabled || cfg.generation!=expected_generation || cfg.input.platform!=input.platform{return Err(conflict("connector disabled, stale generation or platform mismatch"));}
        let role=if input.conversation_kind==ConversationKind::Dm && cfg.input.owner_sender_id.as_deref()==Some(&input.sender_id){BridgeRole::Owner}else{BridgeRole::Guest};
        let attachments=serde_json::to_string(&input.attachments).map_err(|e|invalid(e.to_string()))?;
        let request_id=format!("bridge:{id}");
        conn.execute("INSERT INTO bridge_inbox_v1(id,platform,connector_id,conversation_id,conversation_kind,message_id,sender_id,role,connector_generation,payload_json,attachments_json,status,request_id,created_at,updated_at,actor_id,owner_subject) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,'claimed',?12,?13,?13,?14,?15)",params![id,input.platform,input.connector_id,input.conversation_id,wire(input.conversation_kind),input.message_id,input.sender_id,wire(role),generation(cfg.generation)?,input.payload_json,attachments,request_id,now,cfg.input.actor_id,cfg.input.owner_subject]).map_err(map_rusqlite)?;
        resolve_conversation_conn(conn,&id,expected_generation,now)?;
        Ok(BridgeInboxClaim{claimed:true,inbox:inbox_conn(conn,&id)?.ok_or_else(||invalid("inbox missing after claim"))?,connector:cfg})
    })).await
}
pub async fn validate_inbox_claim(
    db: &RunDatabase,
    id: &str,
    expected_generation: u64,
) -> Result<BridgeInbox, StorageError> {
    let id = id.to_string();
    db.queue()
        .submit(move |conn| {
            let row = inbox_conn(conn, &id)?.ok_or_else(|| invalid("inbox missing"))?;
            if row.connector_generation != expected_generation {
                return Err(conflict("inbox generation mismatch"));
            }
            current(conn, &row.input.connector_id, expected_generation)?;
            if row.status != BridgeInboxStatus::Claimed {
                return Err(conflict("inbox already started or settled"));
            }
            Ok(row)
        })
        .await
}

type RunAuthority = (String, String, String, String, String);
fn run_session(conn: &Connection, run_id: &str, actor: &str) -> Result<RunAuthority, StorageError> {
    type StoredRun = (String, String, String, String, String, String);
    let row: Option<StoredRun> = conn.query_row(
        "SELECT r.session_id,r.status,r.owner_kind,r.owner_subject,s.agent_id,s.owner_user_id FROM runs r JOIN sessions s ON s.session_id=r.session_id WHERE r.run_id=?1",
        [run_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
    ).optional().map_err(map_rusqlite)?;
    let Some((sid, status, kind, owner, actual_actor, session_owner)) = row else {
        return Err(invalid("real run or session missing"));
    };
    if actual_actor != actor {
        return Err(conflict("run session actor mismatch"));
    }
    Ok((sid, status, kind, owner, session_owner))
}
pub(crate) fn active_session(conn: &Connection, sid: &str) -> Result<(), StorageError> {
    let active: bool = conn
        .query_row(
            "SELECT lifecycle='active' FROM sessions WHERE session_id=?1",
            [sid],
            |row| row.get(0),
        )
        .map_err(map_rusqlite)?;
    if !active {
        return Err(conflict("run session not active"));
    }
    Ok(())
}
pub(crate) fn inbox_run_conn(
    conn: &Connection,
    inbox: &BridgeInbox,
) -> Result<Option<(String, String, String)>, StorageError> {
    let cause = &inbox.id;
    let mut stmt=conn.prepare("SELECT r.run_id,r.session_id,r.status,l.source_message_id FROM runs r JOIN run_lineage l ON l.run_id=r.run_id WHERE l.origin='bridge' AND l.cause_id=?1 ORDER BY r.run_id LIMIT 2").map_err(map_rusqlite)?;
    let rows = stmt
        .query_map([cause], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(map_rusqlite)?;
    let rows = rows.collect::<Result<Vec<_>, _>>().map_err(map_rusqlite)?;
    if rows.len() > 1 {
        return Err(conflict("multiple real runs for one inbox"));
    }
    let Some((run, sid, status, source)) = rows.into_iter().next() else {
        return Ok(None);
    };
    if source.as_deref() != Some(&inbox.input.message_id) {
        return Err(conflict("inbox real message source mismatch"));
    }
    let (actual_sid, _, kind, owner, session_owner) = run_session(conn, &run, &inbox.actor_id)?;
    let subject = lingxi_protocol::bridge_subject(
        &inbox.input.platform,
        &inbox.input.connector_id,
        &inbox.input.sender_id,
    )
    .map_err(invalid)?;
    let (expected_kind, expected_owner, expected_session_owner) = if inbox.prepared_takeover {
        (
            "bridge_takeover",
            lingxi_protocol::bridge_takeover_subject(&subject, &sid).map_err(invalid)?,
            LOCAL_OWNER_SUBJECT.to_string(),
        )
    } else {
        ("bridge", subject.clone(), subject)
    };
    if actual_sid != sid
        || kind != expected_kind
        || owner != expected_owner
        || session_owner != expected_session_owner
        || inbox.run_id.as_ref().is_some_and(|id| id != &run)
        || inbox.session_id.as_ref().is_some_and(|id| id != &sid)
    {
        return Err(conflict("inbox real run authority mismatch"));
    }
    Ok(Some((run, sid, status)))
}
pub async fn get_inbox_run(
    db: &RunDatabase,
    id: &str,
) -> Result<Option<(String, String, String)>, StorageError> {
    let id = id.to_string();
    db.queue()
        .submit(move |conn| {
            let inbox = inbox_conn(conn, &id)?.ok_or_else(|| invalid("inbox missing"))?;
            inbox_run_conn(conn, &inbox)
        })
        .await
}
pub async fn bind_inbox(
    db: &RunDatabase,
    id: &str,
    expected_generation: u64,
    session_id: &str,
    run_id: &str,
    now: u64,
) -> Result<BridgeInbox, StorageError> {
    let (id, sid, run, now, busy) = (
        id.to_string(),
        session_id.to_string(),
        run_id.to_string(),
        number(now)?,
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let row=inbox_conn(conn,&id)?.ok_or_else(||invalid("inbox missing"))?;current(conn,&row.input.connector_id,expected_generation)?;
        if row.connector_generation!=expected_generation{return Err(conflict("inbox generation mismatch"));}
        if !row.prepared || row.session_id.as_deref()!=Some(&sid){return Err(conflict("inbox session and attachments not prepared"));}
        let actual=inbox_run_conn(conn,&row)?.ok_or_else(||invalid("inbox real causal run missing"))?;
        if actual.0!=run || actual.1!=sid{return Err(conflict("inbox run binding mismatch"));}
        active_session(conn,&sid)?;
        if row.run_id.is_some(){return Ok(row);}
        if row.status!=BridgeInboxStatus::Claimed{return Err(conflict("inbox already settled"));}
        conn.execute("UPDATE bridge_inbox_v1 SET session_id=?2,run_id=?3,status='running',updated_at=?4 WHERE id=?1",params![id,sid,run,now]).map_err(map_rusqlite)?;
        inbox_conn(conn,&id)?.ok_or_else(||invalid("inbox missing"))
    })).await
}
pub async fn list_pending_inbox(db: &RunDatabase) -> Result<Vec<BridgeInbox>, StorageError> {
    db.queue().submit(move|conn|{let mut stmt=conn.prepare(&format!("SELECT {INBOX_COLUMNS} FROM bridge_inbox_v1 WHERE status IN ('claimed','running') ORDER BY created_at,id")).map_err(map_rusqlite)?;let rows=stmt.query_map([],inbox_row).map_err(map_rusqlite)?;rows.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)}).await
}
fn run_terminal(status: &str) -> BridgeInboxStatus {
    match status {
        "completed" => BridgeInboxStatus::Succeeded,
        "failed" => BridgeInboxStatus::Failed,
        "cancelled" => BridgeInboxStatus::Cancelled,
        _ => BridgeInboxStatus::Unknown,
    }
}
pub async fn settle_inbox(
    db: &RunDatabase,
    id: &str,
    status: BridgeInboxStatus,
    result: Option<Value>,
    error: Option<String>,
    now: u64,
) -> Result<BridgeInbox, StorageError> {
    if !status.terminal() {
        return Err(invalid("inbox settlement must be terminal"));
    }
    let (id, now, busy) = (id.to_string(), number(now)?, db.options().busy_timeout_ms);
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let row=inbox_conn(conn,&id)?.ok_or_else(||invalid("inbox missing"))?;
        if row.status.terminal(){return if row.status==status && row.result==result && row.error==error{Ok(row)}else{Err(conflict("inbox terminal result cannot change"))};}
        let actual=inbox_run_conn(conn,&row)?;
        if status!=BridgeInboxStatus::Unknown && !actual.as_ref().is_some_and(|(_,_,s)|run_terminal(s)==status){return Err(conflict("inbox result disagrees with real run"));}
        let (run,sid)=actual.map(|(run,sid,_)|(Some(run),Some(sid))).unwrap_or((row.run_id,row.session_id));
        conn.execute("UPDATE bridge_inbox_v1 SET status=?2,result_json=?3,error=?4,session_id=?5,run_id=?6,updated_at=?7 WHERE id=?1",params![id,wire(status),result.as_ref().map(Value::to_string),error,sid,run,now]).map_err(map_rusqlite)?;
        inbox_conn(conn,&id)?.ok_or_else(||invalid("inbox missing"))
    })).await
}
/// 重启修复真实绑定；已有运行留给宿主先补回复入队再收口，绝不重建运行。
pub async fn recover_inbox(db: &RunDatabase, now: u64) -> Result<usize, StorageError> {
    let (now, busy) = (number(now)?, db.options().busy_timeout_ms);
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
    let rows={let mut stmt=conn.prepare(&format!("SELECT {INBOX_COLUMNS} FROM bridge_inbox_v1 WHERE status IN ('claimed','running')")).map_err(map_rusqlite)?;let rows=stmt.query_map([],inbox_row).map_err(map_rusqlite)?;rows.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)?};
    let mut recovered=0;
    for row in &rows {
        let actual=inbox_run_conn(conn,row)?;
        if actual.is_none() && row.prepared && row.status==BridgeInboxStatus::Claimed {
            if let Some(sid)=&row.session_id {
                if validate_prepared_files(conn,row,sid,&row.resolved_files).is_ok(){continue;}
            }
            conn.execute("UPDATE bridge_inbox_v1 SET status='unknown',error='prepared media or session unavailable after restart; do not replay',updated_at=?2 WHERE id=?1",params![row.id,now]).map_err(map_rusqlite)?;
            recovered+=1;
            continue;
        }
        let (run,sid,status)=actual.map(|(run,sid,_)|(Some(run),Some(sid),BridgeInboxStatus::Running)).unwrap_or((row.run_id.clone(),row.session_id.clone(),BridgeInboxStatus::Unknown));
        conn.execute("UPDATE bridge_inbox_v1 SET status=?2,run_id=?3,session_id=?4,error=CASE WHEN ?2='unknown' THEN 'execution unknown after restart; do not replay' ELSE error END,updated_at=?5 WHERE id=?1",params![row.id,wire(status),run,sid,now]).map_err(map_rusqlite)?;
        recovered+=1;
    }

    Ok(recovered)
})).await
}

const OUTBOX_COLUMNS:&str="delivery_id,connector_id,connector_generation,inbox_id,run_id,conversation_id,payload_json,attachments_json,status,platform_receipt_json,error,created_at,updated_at,conversation_kind,target_inbox_id,reply_context_json,manual_action_id,loop_delivery_id";
fn outbox_row(row: &Row<'_>) -> rusqlite::Result<BridgeOutbox> {
    Ok(BridgeOutbox {
        input: BridgeOutboxInput {
            delivery_id: row.get(0)?,
            connector_id: row.get(1)?,
            connector_generation: u64_row(row, 2)?,
            inbox_id: row.get(3)?,
            manual_action_id: row.get(16)?,
            run_id: row.get(4)?,
            loop_delivery_id: row.get(17)?,
            payload_json: row.get(6)?,
            attachments: from_json(row.get(7)?, 7)?,
        },
        conversation_id: row.get(5)?,
        conversation_kind: enum_row(row, 13)?,
        target_inbox_id: row.get(14)?,
        reply_context_json: row.get(15)?,
        status: enum_row(row, 8)?,
        platform_receipt: optional_json(row, 9)?,
        error: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}
pub(crate) fn outbox_conn(
    conn: &Connection,
    id: &str,
) -> Result<Option<BridgeOutbox>, StorageError> {
    conn.query_row(
        &format!("SELECT {OUTBOX_COLUMNS} FROM bridge_outbox_v1 WHERE delivery_id=?1"),
        [id],
        outbox_row,
    )
    .optional()
    .map_err(map_rusqlite)
}
pub async fn get_outbox(db: &RunDatabase, id: &str) -> Result<Option<BridgeOutbox>, StorageError> {
    let id = id.to_string();
    db.queue().submit(move |conn| outbox_conn(conn, &id)).await
}
pub(crate) fn outbox_target(
    conn: &Connection,
    input: &BridgeOutboxInput,
    frozen_target: Option<&str>,
) -> Result<(BridgeConnector, BridgeInbox), StorageError> {
    let cfg = current(conn, &input.connector_id, input.connector_generation)?;
    // 台账来源列专属循环投递；其他来源携带即伪造，响亮拒绝。
    if !input.delivery_id.starts_with("loop-reply:")
        && !input.delivery_id.starts_with("loop-notice:")
        && input.loop_delivery_id.is_some()
    {
        return Err(conflict(
            "loop delivery source is reserved for loop deliveries",
        ));
    }
    // 桥循环投递两条腿（台账锚定；先于通用腿，防止被主动通知腿放宽接收）。
    if let Some(run_id) = input.delivery_id.strip_prefix("loop-reply:") {
        if input.manual_action_id.is_some()
            || input.inbox_id.is_some()
            || input.run_id.as_deref() != Some(run_id)
            || input.loop_delivery_id.as_deref() != Some(input.delivery_id.as_str())
        {
            return Err(conflict("loop reply impersonates another source"));
        }
        let delivery = loop_delivery_conn(conn, &input.delivery_id)?
            .ok_or_else(|| invalid("loop reply ledger missing"))?;
        if delivery.input.kind != "reply"
            || delivery.input.connector_id != input.connector_id
            || delivery.input.connector_generation != input.connector_generation
            || delivery.input.run_id.as_deref() != Some(run_id)
        {
            return Err(conflict("loop reply ledger mismatch"));
        }
        loop_turn_run_conn(
            conn,
            run_id,
            &cfg.input.actor_id,
            &delivery.input.session_id,
        )?;
        let (conversation, inbox) =
            loop_delivery_target_conn(conn, &cfg, input, &delivery, frozen_target)?;
        loop_reply_session_conn(conn, &conversation, &delivery.input.session_id)?;
        return Ok((cfg, inbox));
    }
    if input.delivery_id.starts_with("loop-notice:") {
        if input.manual_action_id.is_some()
            || input.inbox_id.is_some()
            || input.run_id.is_some()
            || input.loop_delivery_id.as_deref() != Some(input.delivery_id.as_str())
        {
            return Err(conflict("loop notice impersonates another source"));
        }
        let delivery = loop_delivery_conn(conn, &input.delivery_id)?
            .ok_or_else(|| invalid("loop notice ledger missing"))?;
        if delivery.input.kind != "notice"
            || delivery.input.run_id.is_some()
            || delivery.input.connector_id != input.connector_id
            || delivery.input.connector_generation != input.connector_generation
        {
            return Err(conflict("loop notice ledger mismatch"));
        }
        let (_conversation, inbox) =
            loop_delivery_target_conn(conn, &cfg, input, &delivery, frozen_target)?;
        return Ok((cfg, inbox));
    }
    if let Some(id) = &input.manual_action_id {
        if input.run_id.is_some() || input.inbox_id.is_some() {
            return Err(conflict("manual action cannot impersonate inbox or Run"));
        }
        let action =
            manual_conn(conn, id)?.ok_or_else(|| invalid("manual action receipt missing"))?;
        if action.input.connector_id != input.connector_id
            || action.input.connector_generation != input.connector_generation
            || frozen_target.is_some_and(|id| id != action.input.target_inbox_id)
        {
            return Err(conflict("manual action source mismatch"));
        }
        let (filename, mime, size, _) = validate_manual_source(conn, &cfg, &action.input)?;
        if filename != action.filename || mime != action.mime || size != action.size_bytes {
            return Err(conflict("manual file metadata changed after authorization"));
        }
        let inbox = inbox_conn(conn, &action.input.target_inbox_id)?
            .ok_or_else(|| invalid("manual target inbox missing"))?;
        return Ok((cfg, inbox));
    }
    let Some(run_id) = input.run_id.as_deref() else {
        let id = input
            .inbox_id
            .as_deref()
            .ok_or_else(|| invalid("delivery without Run requires a command inbox"))?;
        let inbox = inbox_conn(conn, id)?.ok_or_else(|| invalid("command inbox missing"))?;
        let receipt = command_conn(conn, id)?.ok_or_else(|| invalid("command receipt missing"))?;
        if receipt.status != BridgeCommandStatus::Confirmed
            || inbox.status != BridgeInboxStatus::Command
            || inbox.run_id.is_some()
            || inbox.input.connector_id != input.connector_id
            || inbox.connector_generation != input.connector_generation
            || inbox.role != BridgeRole::Owner
            || inbox.input.conversation_kind != ConversationKind::Dm
            || cfg.input.owner_sender_id.as_deref() != Some(&inbox.input.sender_id)
        {
            return Err(conflict(
                "command delivery lacks confirmed current owner source",
            ));
        }
        return Ok((cfg, inbox));
    };
    let (sid, _, kind, owner, session_owner) = run_session(conn, run_id, &cfg.input.actor_id)?;
    active_session(conn, &sid)?;
    if let Some(id) = &input.inbox_id {
        let inbox = inbox_conn(conn, id)?.ok_or_else(|| invalid("reply inbox missing"))?;
        if inbox.input.connector_id != input.connector_id
            || inbox.connector_generation != input.connector_generation
            || inbox.run_id.as_deref() != Some(run_id)
            || inbox.session_id.as_deref() != Some(&sid)
        {
            return Err(conflict("reply source binding mismatch"));
        }
        let actual = inbox_run_conn(conn, &inbox)?
            .ok_or_else(|| invalid("reply real causal run missing"))?;
        if actual.0 != run_id {
            return Err(conflict("reply run mismatch"));
        }
        return Ok((cfg, inbox));
    }
    // 主动通知只接受本机用户的真实运行；目标取真实 owner 私聊收据，不接收模型目标参数。
    if kind != "local_user" || owner != LOCAL_OWNER_SUBJECT || session_owner != LOCAL_OWNER_SUBJECT
    {
        return Err(conflict("owner notification run not owned"));
    }
    let sender = cfg
        .input
        .owner_sender_id
        .as_deref()
        .ok_or_else(|| invalid("owner sender not configured"))?;
    let target = if let Some(id) = frozen_target {
        Some(id.to_string())
    } else {
        conn.query_row("SELECT id FROM bridge_inbox_v1 WHERE connector_id=?1 AND connector_generation=?2 AND role='owner' AND conversation_kind='dm' AND sender_id=?3 ORDER BY created_at DESC,id DESC LIMIT 1",params![input.connector_id,generation(input.connector_generation)?,sender],|row|row.get(0)).optional().map_err(map_rusqlite)?
    };
    let inbox = inbox_conn(
        conn,
        &target.ok_or_else(|| invalid("verified owner DM destination missing"))?,
    )?
    .ok_or_else(|| invalid("owner target inbox missing"))?;
    if inbox.role != BridgeRole::Owner
        || inbox.input.conversation_kind != ConversationKind::Dm
        || inbox.input.connector_id != input.connector_id
        || inbox.connector_generation != input.connector_generation
        || inbox.input.sender_id != sender
    {
        return Err(conflict("owner target authority changed"));
    }
    Ok((cfg, inbox))
}
pub(crate) fn reply_context(inbox: &BridgeInbox) -> Result<String, StorageError> {
    let payload: Value =
        serde_json::from_str(&inbox.input.payload_json).map_err(|e| invalid(e.to_string()))?;
    match payload.get("replyContext") {
        Some(Value::Object(_)) => Ok(payload["replyContext"].to_string()),
        Some(Value::Null) | None => Ok("{}".into()),
        _ => Err(invalid("reply context must be object")),
    }
}
pub async fn enqueue_outbox(
    db: &RunDatabase,
    input: &BridgeOutboxInput,
    now: u64,
) -> Result<BridgeOutboxClaim, StorageError> {
    for id in [&input.delivery_id, &input.connector_id] {
        text_id(id)?;
    }
    if let Some(run) = &input.run_id {
        text_id(run)?;
    }
    generation(input.connector_generation)?;
    json(&input.payload_json)?;
    attachments_valid(&input.attachments)?;
    let (input, now, busy) = (input.clone(), number(now)?, db.options().busy_timeout_ms);
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| enqueue_outbox_conn(conn, &input, now))
        })
        .await
}
fn equivalent_outbox_input(
    stored: &BridgeOutboxInput,
    incoming: &BridgeOutboxInput,
) -> Result<bool, StorageError> {
    if stored == incoming {
        return Ok(true);
    }
    let old: Value =
        serde_json::from_str(&stored.payload_json).map_err(|e| invalid(e.to_string()))?;
    let mut supplied: Value =
        serde_json::from_str(&incoming.payload_json).map_err(|e| invalid(e.to_string()))?;
    if supplied.get("contextPolicy").and_then(Value::as_str) != Some("record_when_delivered") {
        return Ok(false);
    }
    if let Some(frozen) = old.get("contextSessionId") {
        if supplied
            .get("contextSessionId")
            .is_some_and(|value| value != frozen)
        {
            return Ok(false);
        }
        supplied["contextSessionId"] = frozen.clone();
    }
    if let Some(frozen) = old.get("contextEpoch") {
        if supplied
            .get("contextEpoch")
            .is_some_and(|value| value != frozen)
        {
            return Ok(false);
        }
        supplied["contextEpoch"] = frozen.clone();
    }
    let mut equivalent = incoming.clone();
    if supplied == old {
        equivalent.payload_json = stored.payload_json.clone();
    }
    Ok(&equivalent == stored)
}
fn enqueue_outbox_conn(
    conn: &Connection,
    input: &BridgeOutboxInput,
    now: i64,
) -> Result<BridgeOutboxClaim, StorageError> {
    if let Some(old) = outbox_conn(conn, &input.delivery_id)? {
        return if equivalent_outbox_input(&old.input, input)? {
            Ok(BridgeOutboxClaim {
                claimed: false,
                outbox: old,
            })
        } else {
            Err(conflict("delivery identity payload conflict"))
        };
    }
    let (_, target) = outbox_target(conn, input, None)?;
    let mut authoritative = input.clone();
    let mut payload: Value =
        serde_json::from_str(&input.payload_json).map_err(|e| invalid(e.to_string()))?;
    if payload.get("contextPolicy").and_then(Value::as_str) == Some("record_when_delivered") {
        if input.inbox_id.is_some()
            || input.manual_action_id.is_some()
            || input.run_id.is_none()
            || target.role != BridgeRole::Owner
            || target.input.conversation_kind != ConversationKind::Dm
        {
            return Err(conflict(
                "delivery context requires actual owner DM notification",
            ));
        }
        let conversation = conversation_conn(
            conn,
            &target.input.connector_id,
            &target.input.conversation_id,
            &target.input.sender_id,
            &target.actor_id,
        )?
        .ok_or_else(|| invalid("owner context conversation missing"))?;
        let subject = lingxi_protocol::bridge_subject(
            &target.input.platform,
            &target.input.connector_id,
            &target.input.sender_id,
        )
        .map_err(invalid)?;
        if conversation.subject != subject {
            return Err(conflict("owner delivery context subject mismatch"));
        }
        let sid = conversation
            .current_session_id
            .as_deref()
            .ok_or_else(|| invalid("owner context session unavailable"))?;
        let valid:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM sessions WHERE session_id=?1 AND agent_id=?2 AND owner_user_id=?3 AND lifecycle='active')",params![sid,target.actor_id,subject],|row|row.get(0)).map_err(map_rusqlite)?;
        if !valid {
            return Err(conflict("owner delivery context session unavailable"));
        }
        if payload
            .get("contextSessionId")
            .is_some_and(|provided| provided.as_str() != Some(sid))
        {
            return Err(conflict(
                "caller delivery context session does not match actual target",
            ));
        }
        if payload
            .get("contextEpoch")
            .is_some_and(|value| value.as_u64() != Some(conversation.epoch))
        {
            return Err(conflict("caller context epoch mismatch"));
        }
        payload["contextSessionId"] = Value::String(sid.into());
        payload["contextEpoch"] = Value::from(conversation.epoch);
        authoritative.payload_json = payload.to_string();
    }
    let input = &authoritative;
    let reply_context = reply_context(&target)?;
    let attachments =
        serde_json::to_string(&input.attachments).map_err(|e| invalid(e.to_string()))?;
    conn.execute("INSERT INTO bridge_outbox_v1(delivery_id,connector_id,connector_generation,inbox_id,run_id,conversation_id,payload_json,attachments_json,status,created_at,updated_at,conversation_kind,target_inbox_id,reply_context_json,manual_action_id,loop_delivery_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'queued',?9,?9,?10,?11,?12,?13,?14)",params![input.delivery_id,input.connector_id,generation(input.connector_generation)?,input.inbox_id,input.run_id,target.input.conversation_id,input.payload_json,attachments,now,wire(target.input.conversation_kind),target.id,reply_context,input.manual_action_id,input.loop_delivery_id]).map_err(map_rusqlite)?;
    Ok(BridgeOutboxClaim {
        claimed: true,
        outbox: outbox_conn(conn, &input.delivery_id)?
            .ok_or_else(|| invalid("outbox missing after queue"))?,
    })
}
pub async fn list_pending_outbox(db: &RunDatabase) -> Result<Vec<BridgeOutbox>, StorageError> {
    db.queue().submit(move|conn|{let mut stmt=conn.prepare(&format!("SELECT {OUTBOX_COLUMNS} FROM bridge_outbox_v1 WHERE status='queued' ORDER BY created_at,delivery_id")).map_err(map_rusqlite)?;let rows=stmt.query_map([],outbox_row).map_err(map_rusqlite)?;rows.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)}).await
}
pub async fn claim_outbox(
    db: &RunDatabase,
    id: &str,
    expected_generation: u64,
    now: u64,
) -> Result<BridgeOutboxClaim, StorageError> {
    let (id, now, busy) = (id.to_string(), number(now)?, db.options().busy_timeout_ms);
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let row=outbox_conn(conn,&id)?.ok_or_else(||invalid("outbox missing"))?;
        if row.status!=BridgeOutboxStatus::Queued{return Ok(BridgeOutboxClaim{claimed:false,outbox:row});}
        if row.input.connector_generation!=expected_generation{return Err(conflict("outbox generation mismatch"));}
        let (_,target)=outbox_target(conn,&row.input,Some(&row.target_inbox_id))?;if target.input.conversation_id!=row.conversation_id || target.input.conversation_kind!=row.conversation_kind || reply_context(&target)?!=row.reply_context_json{return Err(conflict("outbox destination changed"));}
        conn.execute("UPDATE bridge_outbox_v1 SET status='started',updated_at=?2 WHERE delivery_id=?1 AND status='queued'",params![id,now]).map_err(map_rusqlite)?;
        Ok(BridgeOutboxClaim{claimed:true,outbox:outbox_conn(conn,&id)?.ok_or_else(||invalid("outbox missing"))?})
    })).await
}
/// 分片准备完成后、真正外发前重新核验权限；不重新领取，也不改变投递事实。
pub async fn validate_started_outbox(
    db: &RunDatabase,
    id: &str,
    expected_generation: u64,
) -> Result<BridgeOutbox, StorageError> {
    let (id, busy) = (id.to_string(), db.options().busy_timeout_ms);
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                let row = outbox_conn(conn, &id)?.ok_or_else(|| invalid("outbox missing"))?;
                if row.status != BridgeOutboxStatus::Started {
                    return Err(conflict("outbox is not started"));
                }
                if row.input.connector_generation != expected_generation {
                    return Err(conflict("outbox generation mismatch"));
                }
                let (_, target) = outbox_target(conn, &row.input, Some(&row.target_inbox_id))?;
                if target.input.conversation_id != row.conversation_id
                    || target.input.conversation_kind != row.conversation_kind
                    || reply_context(&target)? != row.reply_context_json
                {
                    return Err(conflict("outbox destination changed"));
                }
                Ok(row)
            })
        })
        .await
}
pub async fn settle_outbox(
    db: &RunDatabase,
    id: &str,
    status: BridgeOutboxStatus,
    receipt: Option<BridgePlatformReceipt>,
    error: Option<String>,
    now: u64,
) -> Result<BridgeOutbox, StorageError> {
    if matches!(
        status,
        BridgeOutboxStatus::Queued | BridgeOutboxStatus::Started
    ) {
        return Err(invalid("outbox settlement must be terminal"));
    }
    if status == BridgeOutboxStatus::Confirmed {
        let receipt = receipt
            .as_ref()
            .ok_or_else(|| invalid("platform message receipt required"))?;
        if receipt.message_ids.is_empty() {
            return Err(invalid("real platform message IDs required"));
        }
        for id in &receipt.message_ids {
            text_id(id)?;
        }
    } else if receipt.is_some() {
        return Err(invalid(
            "non-confirmed delivery cannot claim success receipt",
        ));
    }
    if let Some(raw) = receipt.as_ref().and_then(|r| r.raw_json.as_ref()) {
        json(raw)?;
    }
    let (id, now, busy) = (id.to_string(), number(now)?, db.options().busy_timeout_ms);
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let row=outbox_conn(conn,&id)?.ok_or_else(||invalid("outbox missing"))?;
        if row.status!=BridgeOutboxStatus::Started{return if row.status==status && row.platform_receipt==receipt && row.error==error{Ok(row)}else{Err(conflict("outbox not started or terminal result changed"))};}
        if let Some(receipt)=&receipt{let cfg=connector_conn(conn,&row.input.connector_id)?.ok_or_else(||invalid("connector missing"))?;if cfg.input.platform!=receipt.platform{return Err(conflict("platform receipt mismatch"));}}
        let raw=receipt.as_ref().map(serde_json::to_string).transpose().map_err(|e|invalid(e.to_string()))?;
        conn.execute("UPDATE bridge_outbox_v1 SET status=?2,platform_receipt_json=?3,error=?4,updated_at=?5 WHERE delivery_id=?1",params![id,wire(status),raw,error,now]).map_err(map_rusqlite)?;
        outbox_conn(conn,&id)?.ok_or_else(||invalid("outbox missing"))
    })).await
}
pub async fn recover_started_outbox(db: &RunDatabase, now: u64) -> Result<usize, StorageError> {
    let (now, busy) = (number(now)?, db.options().busy_timeout_ms);
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|conn.execute("UPDATE bridge_outbox_v1 SET status='unknown',error='send result unknown after restart; do not resend',updated_at=?1 WHERE status='started'",[now]).map_err(map_rusqlite))).await
}

// ── 循环投递台账（V25 bridge_loop_deliveries_v1）───────────────────────
// 桥循环的外发意图先落账：唤起轮次时写 reply 行（携带 run_id），循环事件
// 写 notice 行（携带 text）；连接会话 tick 把 queued 行推进为真实 outbox
// （载荷在 run 终态后才能冻结）。reply 的授权锚是 run 血统（origin='loop'、
// cause=绑定会话、无父 run、本机调度主体）；notice 的循环记录核验在服务侧
// （loop-state.json 非存储层可读），存储侧钉死连接器/会话挂接/owner 私聊
// 目标。两侧都是持续重查：挂接或连接器代次变化即拒绝。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeLoopDeliveryInput {
    pub delivery_id: String,
    pub connector_id: String,
    pub connector_generation: u64,
    pub conversation_id: String,
    pub sender_id: String,
    pub actor_id: String,
    pub session_id: String,
    /// reply 必填（循环轮真实 run）；notice 必须为空。
    pub run_id: Option<String>,
    /// reply | notice（与表 CHECK 一致）。
    pub kind: String,
    /// notice 必填；reply 必须为空（回复文本由 run 终态决定）。
    pub text: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeLoopDelivery {
    #[serde(flatten)]
    pub input: BridgeLoopDeliveryInput,
    /// queued | enqueued | failed
    pub status: String,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}
const LOOP_DELIVERY_COLUMNS:&str="delivery_id,connector_id,connector_generation,conversation_id,sender_id,actor_id,session_id,run_id,kind,text,status,error,created_at,updated_at";
fn loop_delivery_row(row: &Row<'_>) -> rusqlite::Result<BridgeLoopDelivery> {
    Ok(BridgeLoopDelivery {
        input: BridgeLoopDeliveryInput {
            delivery_id: row.get(0)?,
            connector_id: row.get(1)?,
            connector_generation: u64_row(row, 2)?,
            conversation_id: row.get(3)?,
            sender_id: row.get(4)?,
            actor_id: row.get(5)?,
            session_id: row.get(6)?,
            run_id: row.get(7)?,
            kind: row.get(8)?,
            text: row.get(9)?,
        },
        status: row.get(10)?,
        error: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}
fn loop_delivery_conn(
    conn: &Connection,
    id: &str,
) -> Result<Option<BridgeLoopDelivery>, StorageError> {
    conn.query_row(
        &format!(
            "SELECT {LOOP_DELIVERY_COLUMNS} FROM bridge_loop_deliveries_v1 WHERE delivery_id=?1"
        ),
        [id],
        loop_delivery_row,
    )
    .optional()
    .map_err(map_rusqlite)
}
/// 循环轮 run 的血统必须是绑定会话的循环根轮（无父 run、本机调度主体）。
fn loop_turn_run_conn(
    conn: &Connection,
    run_id: &str,
    actor: &str,
    session_id: &str,
) -> Result<(), StorageError> {
    let (sid, _, kind, owner, _) = run_session(conn, run_id, actor)?;
    let lineage: Option<(String, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT origin,cause_id,parent_run_id FROM run_lineage WHERE run_id=?1",
            [run_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(map_rusqlite)?;
    let (origin, cause, parent) =
        lineage.ok_or_else(|| invalid("loop reply run lineage missing"))?;
    if sid != session_id
        || kind != "local_user"
        || owner != LOCAL_OWNER_SUBJECT
        || origin != "loop"
        || cause.as_deref() != Some(session_id)
        || parent.is_some()
    {
        return Err(conflict("loop reply run is not a local root loop turn"));
    }
    Ok(())
}
/// 台账行钉住的会话挂接与 owner 私聊目标；返回会话行与目标 inbox。
/// 会话存续/属主校验不在此列：通知（如会话重置终止通告）必须能在会话
/// 换代后继续送达，由回复腿单独补 session 校验。
fn loop_delivery_target_conn(
    conn: &Connection,
    cfg: &BridgeConnector,
    input: &BridgeOutboxInput,
    delivery: &BridgeLoopDelivery,
    frozen_target: Option<&str>,
) -> Result<(BridgeConversation, BridgeInbox), StorageError> {
    let conversation = conversation_conn(
        conn,
        &input.connector_id,
        &delivery.input.conversation_id,
        &delivery.input.sender_id,
        &delivery.input.actor_id,
    )?
    .ok_or_else(|| invalid("loop conversation missing"))?;
    if conversation.conversation_kind != ConversationKind::Dm
        || cfg.input.owner_sender_id.as_deref() != Some(delivery.input.sender_id.as_str())
        || cfg.input.actor_id != delivery.input.actor_id
    {
        return Err(conflict("loop delivery conversation authority changed"));
    }
    let target = match frozen_target {
        Some(id) => id.to_string(),
        None => conn
            .query_row(
                "SELECT id FROM bridge_inbox_v1 WHERE connector_id=?1 AND connector_generation=?2 AND conversation_id=?3 AND role='owner' AND conversation_kind='dm' AND sender_id=?4 ORDER BY created_at DESC,id DESC LIMIT 1",
                params![
                    input.connector_id,
                    generation(input.connector_generation)?,
                    delivery.input.conversation_id,
                    delivery.input.sender_id
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_rusqlite)?
            .ok_or_else(|| invalid("loop delivery destination missing"))?,
    };
    let inbox = inbox_conn(conn, &target)?.ok_or_else(|| invalid("loop target inbox missing"))?;
    if inbox.role != BridgeRole::Owner
        || inbox.input.conversation_kind != ConversationKind::Dm
        || inbox.input.connector_id != input.connector_id
        || inbox.connector_generation != input.connector_generation
        || inbox.input.conversation_id != delivery.input.conversation_id
        || inbox.input.sender_id != delivery.input.sender_id
    {
        return Err(conflict("loop delivery target authority changed"));
    }
    Ok((conversation, inbox))
}
/// 回复腿的会话存续校验：回执必须仍属于挂接的当前活跃会话。
fn loop_reply_session_conn(
    conn: &Connection,
    conversation: &BridgeConversation,
    session_id: &str,
) -> Result<(), StorageError> {
    let session_ok: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE session_id=?1 AND owner_user_id=?2 AND lifecycle='active')",
            params![session_id, conversation.subject],
            |row| row.get(0),
        )
        .map_err(map_rusqlite)?;
    if conversation.current_session_id.as_deref() != Some(session_id) || !session_ok {
        return Err(conflict("loop reply session authority changed"));
    }
    Ok(())
}
pub async fn enqueue_loop_delivery(
    db: &RunDatabase,
    input: &BridgeLoopDeliveryInput,
    now: u64,
) -> Result<BridgeLoopDelivery, StorageError> {
    for id in [
        &input.delivery_id,
        &input.connector_id,
        &input.conversation_id,
        &input.sender_id,
        &input.actor_id,
        &input.session_id,
    ] {
        text_id(id)?;
    }
    if let Some(run) = &input.run_id {
        text_id(run)?;
    }
    generation(input.connector_generation)?;
    match input.kind.as_str() {
        "reply" if input.run_id.is_some() && input.text.is_none() => {}
        "notice"
            if input.run_id.is_none()
                && input
                    .text
                    .as_deref()
                    .is_some_and(|text| !text.trim().is_empty()) => {}
        _ => return Err(invalid("loop delivery shape violates its kind")),
    }
    let (input, now, busy) = (input.clone(), number(now)?, db.options().busy_timeout_ms);
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                if let Some(old) = loop_delivery_conn(conn, &input.delivery_id)? {
                    if old.input != input {
                        return Err(conflict("loop delivery identity payload conflict"));
                    }
                    return Ok(old);
                }
                let cfg = current(conn, &input.connector_id, input.connector_generation)?;
                let conversation = conversation_conn(
                    conn,
                    &input.connector_id,
                    &input.conversation_id,
                    &input.sender_id,
                    &input.actor_id,
                )?
                .ok_or_else(|| invalid("loop conversation missing"))?;
                if conversation.conversation_kind != ConversationKind::Dm
                    || cfg.input.owner_sender_id.as_deref() != Some(input.sender_id.as_str())
                    || cfg.input.actor_id != input.actor_id
                {
                    return Err(conflict("loop delivery requires the owner DM conversation"));
                }
                // 回复锚定当前挂接会话；通知（如重置终止通告）允许在会话
                // 换代后继续送达，会话校验只属回复。
                if input.kind == "reply" {
                    if conversation.current_session_id.as_deref()
                        != Some(input.session_id.as_str())
                    {
                        return Err(conflict("loop reply requires the current bound session"));
                    }
                    active_session(conn, &input.session_id)?;
                    loop_turn_run_conn(
                        conn,
                        input.run_id.as_deref().expect("reply shape checked"),
                        &input.actor_id,
                        &input.session_id,
                    )?;
                }
                conn.execute("INSERT INTO bridge_loop_deliveries_v1(delivery_id,connector_id,connector_generation,conversation_id,sender_id,actor_id,session_id,run_id,kind,text,status,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'queued',?11,?11)",params![input.delivery_id,input.connector_id,generation(input.connector_generation)?,input.conversation_id,input.sender_id,input.actor_id,input.session_id,input.run_id,input.kind,input.text,now]).map_err(map_rusqlite)?;
                loop_delivery_conn(conn, &input.delivery_id)?
                    .ok_or_else(|| invalid("loop delivery missing after write"))
            })
        })
        .await
}
/// 服务侧反查：桥循环目标会话 → 当前挂接的会话行。一行以上即挂接损坏，
/// 响亮拒绝而不是猜一条。
pub async fn loop_conversation_by_session(
    db: &RunDatabase,
    session_id: &str,
) -> Result<Option<BridgeConversation>, StorageError> {
    let session_id = session_id.to_string();
    db.queue()
        .submit(move |conn| {
            let mut stmt = conn
                .prepare(&format!("SELECT {CONVERSATION_COLUMNS} FROM bridge_conversations_v1 WHERE current_session_id=?1 ORDER BY created_at,conversation_id LIMIT 2"))
                .map_err(map_rusqlite)?;
            let rows = stmt
                .query_map([session_id.as_str()], conversation_row)
                .map_err(map_rusqlite)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_rusqlite)?;
            if rows.len() > 1 {
                return Err(conflict("loop session bound by multiple conversations"));
            }
            Ok(rows.into_iter().next())
        })
        .await
}
pub async fn pending_loop_deliveries(
    db: &RunDatabase,
    connector_id: &str,
) -> Result<Vec<BridgeLoopDelivery>, StorageError> {
    let connector_id = connector_id.to_string();
    db.queue()
        .submit(move |conn| {
            let mut stmt = conn
                .prepare(&format!("SELECT {LOOP_DELIVERY_COLUMNS} FROM bridge_loop_deliveries_v1 WHERE status='queued' AND connector_id=?1 ORDER BY created_at,delivery_id"))
                .map_err(map_rusqlite)?;
            let rows = stmt
                .query_map([connector_id.as_str()], loop_delivery_row)
                .map_err(map_rusqlite)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(map_rusqlite)
        })
        .await
}
pub async fn settle_loop_delivery(
    db: &RunDatabase,
    id: &str,
    status: &str,
    error: Option<String>,
    now: u64,
) -> Result<BridgeLoopDelivery, StorageError> {
    if !matches!(status, "enqueued" | "failed") {
        return Err(invalid(
            "loop delivery settlement must be enqueued or failed",
        ));
    }
    if status == "enqueued" && error.is_some() {
        return Err(invalid("enqueued loop delivery cannot carry an error"));
    }
    let (id, status, now, busy) = (
        id.to_string(),
        status.to_string(),
        number(now)?,
        db.options().busy_timeout_ms,
    );
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                let row = loop_delivery_conn(conn, &id)?
                    .ok_or_else(|| invalid("loop delivery missing"))?;
                if row.status != "queued" {
                    return if row.status == status && row.error == error {
                        Ok(row)
                    } else {
                        Err(conflict("loop delivery not queued or result changed"))
                    };
                }
                conn.execute(
                    "UPDATE bridge_loop_deliveries_v1 SET status=?2,error=?3,updated_at=?4 WHERE delivery_id=?1 AND status='queued'",
                    params![id, status, error, now],
                )
                .map_err(map_rusqlite)?;
                loop_delivery_conn(conn, &id)?
                    .ok_or_else(|| invalid("loop delivery missing"))
            })
        })
        .await
}

fn validate_prepared_files(
    conn: &Connection,
    inbox: &BridgeInbox,
    sid: &str,
    files: &[BridgeInboxFile],
) -> Result<(), StorageError> {
    let subject = lingxi_protocol::bridge_subject(
        &inbox.input.platform,
        &inbox.input.connector_id,
        &inbox.input.sender_id,
    )
    .map_err(invalid)?;
    let expected_owner = if inbox.prepared_takeover {
        LOCAL_OWNER_SUBJECT
    } else {
        &subject
    };
    let valid:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM sessions WHERE session_id=?1 AND agent_id=?2 AND owner_user_id=?3 AND lifecycle='active')",params![sid,inbox.actor_id,expected_owner],|row|row.get(0)).map_err(map_rusqlite)?;
    if !valid {
        return Err(conflict("prepared inbox session authority mismatch"));
    }
    let expected: std::collections::HashSet<_> = inbox
        .input
        .attachments
        .iter()
        .map(|item| item.attachment_id.as_str())
        .collect();
    let provided: std::collections::HashSet<_> = files
        .iter()
        .map(|item| item.attachment_id.as_str())
        .collect();
    if files.len() != expected.len() || provided != expected {
        return Err(conflict(
            "inbox attachment resolution incomplete or duplicate",
        ));
    }
    let mut total = 0u64;
    for file in files {
        text_id(&file.file_id)?;
        let size:Option<i64>=conn.query_row("SELECT size_bytes FROM session_files WHERE file_id=?1 AND owner_session_id=?2 AND storage_kind='managed_cache' AND status='available' AND is_directory=0",params![file.file_id,sid],|row|row.get(0)).optional().map_err(map_rusqlite)?;
        let size = size
            .and_then(|size| u64::try_from(size).ok())
            .ok_or_else(|| conflict("inbox attachment file is not owned available managed data"))?;
        total = total
            .checked_add(size)
            .ok_or_else(|| invalid("resolved bytes overflow"))?;
        if size > 50 * 1024 * 1024 || total > 64 * 1024 * 1024 {
            return Err(invalid("resolved attachment byte budget exceeded"));
        }
    }
    Ok(())
}
pub async fn prepare_inbox_session(
    db: &RunDatabase,
    id: &str,
    expected_generation: u64,
    sid: &str,
    files: &[BridgeInboxFile],
    now: u64,
) -> Result<BridgeInbox, StorageError> {
    let (id, sid, files, now, busy) = (
        id.to_string(),
        sid.to_string(),
        files.to_vec(),
        number(now)?,
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let row=inbox_conn(conn,&id)?.ok_or_else(||invalid("inbox missing"))?;current(conn,&row.input.connector_id,expected_generation)?;
        if row.connector_generation!=expected_generation{return Err(conflict("inbox generation mismatch"));}
        if row.session_id.as_deref()!=Some(&sid){return Err(conflict("inbox resolved conversation session mismatch"));}
        validate_prepared_files(conn,&row,&sid,&files)?;
        if row.prepared{return if row.session_id.as_deref()==Some(&sid) && row.resolved_files==files{Ok(row)}else{Err(conflict("prepared inbox identity cannot change"))};}
        if row.status!=BridgeInboxStatus::Claimed || row.run_id.is_some(){return Err(conflict("inbox already started or settled"));}
        let raw=serde_json::to_string(&files).map_err(|e|invalid(e.to_string()))?;
        conn.execute("UPDATE bridge_inbox_v1 SET session_id=?2,prepared=1,resolved_files_json=?3,updated_at=?4 WHERE id=?1",params![id,sid,raw,now]).map_err(map_rusqlite)?;
        inbox_conn(conn,&id)?.ok_or_else(||invalid("prepared inbox missing"))
    })).await
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeConversation {
    pub connector_id: String,
    pub conversation_id: String,
    pub sender_id: String,
    pub actor_id: String,
    pub subject: String,
    pub conversation_kind: ConversationKind,
    pub epoch: u64,
    pub current_session_id: Option<String>,
    pub metadata: Value,
    pub created_at: i64,
    pub updated_at: i64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeConversationResolution {
    pub session_id: String,
    pub epoch: u64,
}
const CONVERSATION_COLUMNS:&str="connector_id,conversation_id,sender_id,actor_id,subject,conversation_kind,epoch,current_session_id,metadata_json,created_at,updated_at";
fn conversation_row(row: &Row<'_>) -> rusqlite::Result<BridgeConversation> {
    Ok(BridgeConversation {
        connector_id: row.get(0)?,
        conversation_id: row.get(1)?,
        sender_id: row.get(2)?,
        actor_id: row.get(3)?,
        subject: row.get(4)?,
        conversation_kind: enum_row(row, 5)?,
        epoch: u64_row(row, 6)?,
        current_session_id: row.get(7)?,
        metadata: from_json(row.get(8)?, 8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}
fn conversation_conn(
    conn: &Connection,
    connector: &str,
    conversation: &str,
    sender: &str,
    actor: &str,
) -> Result<Option<BridgeConversation>, StorageError> {
    conn.query_row(&format!("SELECT {CONVERSATION_COLUMNS} FROM bridge_conversations_v1 WHERE connector_id=?1 AND conversation_id=?2 AND sender_id=?3 AND actor_id=?4"),params![connector,conversation,sender,actor],conversation_row).optional().map_err(map_rusqlite)
}
fn resolve_conversation_conn(
    conn: &Connection,
    id: &str,
    expected_generation: u64,
    now: i64,
) -> Result<BridgeConversationResolution, StorageError> {
    let inbox = inbox_conn(conn, id)?.ok_or_else(|| invalid("inbox missing"))?;
    current(conn, &inbox.input.connector_id, expected_generation)?;
    if inbox.connector_generation != expected_generation {
        return Err(conflict("inbox conversation generation mismatch"));
    }
    if let (Some(sid), Some(epoch)) = (&inbox.session_id, inbox.conversation_epoch) {
        return Ok(BridgeConversationResolution {
            session_id: sid.clone(),
            epoch,
        });
    }
    let subject = lingxi_protocol::bridge_subject(
        &inbox.input.platform,
        &inbox.input.connector_id,
        &inbox.input.sender_id,
    )
    .map_err(invalid)?;
    let old = conversation_conn(
        conn,
        &inbox.input.connector_id,
        &inbox.input.conversation_id,
        &inbox.input.sender_id,
        &inbox.actor_id,
    )?;
    if old.as_ref().is_some_and(|old| {
        old.subject != subject || old.conversation_kind != inbox.input.conversation_kind
    }) {
        return Err(conflict("conversation identity conflict"));
    }
    // 角色由真实配置冻结；权限边界改变时不能继续读取上一角色的模型历史。
    let isolate = if let Some(old) = &old {
        let mut stmt = conn.prepare("SELECT DISTINCT role FROM bridge_inbox_v1 WHERE connector_id=?1 AND conversation_id=?2 AND sender_id=?3 AND actor_id=?4 AND conversation_epoch=?5 AND id<>?6 LIMIT 2").map_err(map_rusqlite)?;
        let roles = stmt
            .query_map(
                params![
                    old.connector_id,
                    old.conversation_id,
                    old.sender_id,
                    old.actor_id,
                    generation(old.epoch)?,
                    inbox.id
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(map_rusqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_rusqlite)?;
        let previous = match roles.as_slice() {
            [role] => Some(role.as_str()),
            [] if old.metadata["authorityVersion"] == 1 => old.metadata["authorityRole"].as_str(),
            _ => None,
        };
        previous != Some(wire(inbox.role).as_str())
    } else {
        false
    };
    let epoch = match &old {
        Some(old) if isolate => old
            .epoch
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
            .ok_or_else(|| invalid("conversation epoch overflow"))?,
        Some(old) => old.epoch,
        None => 1,
    };
    let session_id = match old
        .as_ref()
        .filter(|_| !isolate)
        .and_then(|old| old.current_session_id.clone())
    {
        Some(sid) => sid,
        None => lingxi_protocol::bridge_conversation_session_id(
            &inbox.input.platform,
            &inbox.input.connector_id,
            &inbox.input.conversation_id,
            &inbox.input.sender_id,
            &inbox.actor_id,
            epoch,
        )
        .map_err(invalid)?,
    };
    let mut metadata = old
        .as_ref()
        .map_or_else(|| serde_json::json!({}), |old| old.metadata.clone());
    if !metadata.is_object() {
        return Err(invalid("conversation metadata must be an object"));
    }
    metadata["authorityVersion"] = serde_json::json!(1);
    metadata["authorityRole"] = serde_json::json!(inbox.role);
    let payload: Value =
        serde_json::from_str(&inbox.input.payload_json).map_err(|e| invalid(e.to_string()))?;
    for key in ["displayName", "senderName", "avatarUrl"] {
        if let Some(Value::String(value)) = payload.get(key) {
            if value.len() <= 4096 {
                metadata[key] = Value::String(value.clone());
            }
        }
    }
    conn.execute("INSERT INTO bridge_conversations_v1(connector_id,conversation_id,sender_id,actor_id,subject,conversation_kind,epoch,current_session_id,metadata_json,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) ON CONFLICT(connector_id,conversation_id,sender_id,actor_id) DO UPDATE SET epoch=excluded.epoch,current_session_id=excluded.current_session_id,metadata_json=excluded.metadata_json,updated_at=excluded.updated_at",params![inbox.input.connector_id,inbox.input.conversation_id,inbox.input.sender_id,inbox.actor_id,subject,wire(inbox.input.conversation_kind),generation(epoch)?,session_id,metadata.to_string(),old.as_ref().map_or(now,|old|old.created_at),now]).map_err(map_rusqlite)?;
    conn.execute(
        "UPDATE bridge_inbox_v1 SET session_id=?2,conversation_epoch=?3 WHERE id=?1",
        params![inbox.id, session_id, generation(epoch)?],
    )
    .map_err(map_rusqlite)?;
    Ok(BridgeConversationResolution { session_id, epoch })
}
/// 只分配稳定身份，不创建会话、消息或运行；首次 claim 已在同事务冻结此结果。
pub async fn resolve_conversation(
    db: &RunDatabase,
    inbox_id: &str,
    expected_generation: u64,
    now: u64,
) -> Result<BridgeConversationResolution, StorageError> {
    let (id, now, busy) = (
        inbox_id.to_string(),
        number(now)?,
        db.options().busy_timeout_ms,
    );
    db.queue()
        .submit(move |conn| {
            with_write_txn(conn, busy, |conn| {
                resolve_conversation_conn(conn, &id, expected_generation, now)
            })
        })
        .await
}
pub async fn list_conversations(
    db: &RunDatabase,
    connector_id: &str,
    limit: usize,
) -> Result<Vec<BridgeConversation>, StorageError> {
    let (connector, limit) = (connector_id.to_string(), limit.min(1000) as i64);
    db.queue().submit(move|conn|{let mut stmt=conn.prepare(&format!("SELECT {CONVERSATION_COLUMNS} FROM bridge_conversations_v1 WHERE connector_id=?1 ORDER BY updated_at DESC,conversation_id,sender_id LIMIT ?2")).map_err(map_rusqlite)?;let rows=stmt.query_map(params![connector,limit],conversation_row).map_err(map_rusqlite)?;rows.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)}).await
}
/// 服务层持有真实会话运行许可；只清引用并推进后续上下文，不删文件身份或旧运行事实。
pub async fn reset_conversation(
    db: &RunDatabase,
    connector_id: &str,
    conversation_id: &str,
    sender_id: &str,
    expected_generation: u64,
    now: u64,
) -> Result<BridgeConversation, StorageError> {
    let (connector, conversation, sender, now, busy) = (
        connector_id.to_string(),
        conversation_id.to_string(),
        sender_id.to_string(),
        number(now)?,
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
    let cfg=current(conn,&connector,expected_generation)?;
    let old=conversation_conn(conn,&connector,&conversation,&sender,&cfg.input.actor_id)?.ok_or_else(||invalid("conversation missing"))?;
    let active:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM bridge_inbox_v1 i JOIN run_lineage l ON l.origin='bridge' AND l.cause_id=i.id JOIN runs r ON r.run_id=l.run_id WHERE i.connector_id=?1 AND i.conversation_id=?2 AND i.sender_id=?3 AND i.actor_id=?4 AND r.status IN ('queued','running','waiting_approval','cancelling'))",params![connector,conversation,sender,cfg.input.actor_id],|row|row.get(0)).map_err(map_rusqlite)?;
    if active{return Err(conflict("session_busy"));}
    if let Some(sid)=&old.current_session_id{
        let busy:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM runs WHERE session_id=?1 AND status IN ('queued','running','waiting_approval','cancelling'))",[sid],|row|row.get(0)).map_err(map_rusqlite)?;
        if busy{return Err(conflict("session_busy"));}
        conn.execute("DELETE FROM session_file_refs WHERE session_id=?1",[sid]).map_err(map_rusqlite)?;
    }
    let next=generation(old.epoch)?.checked_add(1).ok_or_else(||invalid("conversation epoch overflow"))?;
    conn.execute("UPDATE bridge_conversations_v1 SET current_session_id=NULL,epoch=?5,updated_at=?6 WHERE connector_id=?1 AND conversation_id=?2 AND sender_id=?3 AND actor_id=?4",params![connector,conversation,sender,cfg.input.actor_id,next,now]).map_err(map_rusqlite)?;
    conn.execute("UPDATE bridge_inbox_v1 SET status='unknown',error='conversation reset before dispatch; do not replay',updated_at=?5 WHERE connector_id=?1 AND conversation_id=?2 AND sender_id=?3 AND actor_id=?4 AND status='claimed' AND run_id IS NULL AND NOT EXISTS(SELECT 1 FROM run_lineage l WHERE l.origin='bridge' AND l.cause_id=bridge_inbox_v1.id)",params![connector,conversation,sender,cfg.input.actor_id,now]).map_err(map_rusqlite)?;
    conversation_conn(conn,&connector,&conversation,&sender,&cfg.input.actor_id)?.ok_or_else(||invalid("conversation missing after reset"))
})).await
}

/// 仅可信宿主的明确接管选择可调用；普通 Bridge 记录不因本入口获得桌面会话权限。
pub async fn prepare_inbox_takeover(
    db: &RunDatabase,
    id: &str,
    expected_generation: u64,
    desktop_sid: &str,
    files: &[BridgeInboxFile],
    now: u64,
) -> Result<BridgeInbox, StorageError> {
    let (id, sid, files, now, busy) = (
        id.to_string(),
        desktop_sid.to_string(),
        files.to_vec(),
        number(now)?,
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let row=inbox_conn(conn,&id)?.ok_or_else(||invalid("takeover inbox missing"))?;let cfg=current(conn,&row.input.connector_id,expected_generation)?;
        if row.connector_generation!=expected_generation || row.role!=BridgeRole::Owner || row.input.conversation_kind!=ConversationKind::Dm || cfg.input.owner_sender_id.as_deref()!=Some(&row.input.sender_id){return Err(conflict("takeover requires current explicitly bound owner DM"));}
        let bridge:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM bridge_conversations_v1 WHERE current_session_id=?1) OR EXISTS(SELECT 1 FROM bridge_inbox_v1 WHERE session_id=?1 AND prepared_takeover=0)",[&sid],|r|r.get(0)).map_err(map_rusqlite)?;
        let internal:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM automation_internal_sessions_v1 i WHERE i.session_id=?1 AND NOT EXISTS(SELECT 1 FROM automation_activity_promotions_v1 p WHERE p.session_id=i.session_id))",[&sid],|r|r.get(0)).map_err(map_rusqlite)?;
        if bridge||internal{return Err(conflict("takeover target must be an ordinary desktop session"));}
        let mut scoped=row.clone();scoped.prepared_takeover=true;
        validate_prepared_files(conn,&scoped,&sid,&files)?;
        if row.prepared{return if row.prepared_takeover && row.session_id.as_deref()==Some(&sid) && row.resolved_files==files{Ok(row)}else{Err(conflict("prepared inbox target cannot change"))};}
        if row.status!=BridgeInboxStatus::Claimed || row.run_id.is_some(){return Err(conflict("takeover inbox already started or settled"));}
        let raw=serde_json::to_string(&files).map_err(|e|invalid(e.to_string()))?;
        conn.execute("UPDATE bridge_inbox_v1 SET session_id=?2,prepared=1,prepared_takeover=1,resolved_files_json=?3,updated_at=?4 WHERE id=?1",params![id,sid,raw,now]).map_err(map_rusqlite)?;
        inbox_conn(conn,&id)?.ok_or_else(||invalid("takeover inbox missing after write"))
    })).await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BridgeCommandStatus {
    Started,
    Confirmed,
    Unknown,
    Failed,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeCommandReceipt {
    pub inbox_id: String,
    pub command: String,
    pub status: BridgeCommandStatus,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeCommandClaim {
    pub claimed: bool,
    pub receipt: BridgeCommandReceipt,
}
const COMMAND_COLUMNS: &str = "inbox_id,command,status,result_json,error,created_at,updated_at";
fn command_row(row: &Row<'_>) -> rusqlite::Result<BridgeCommandReceipt> {
    Ok(BridgeCommandReceipt {
        inbox_id: row.get(0)?,
        command: row.get(1)?,
        status: enum_row(row, 2)?,
        result: optional_json(row, 3)?,
        error: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}
fn command_conn(conn: &Connection, id: &str) -> Result<Option<BridgeCommandReceipt>, StorageError> {
    conn.query_row(
        &format!("SELECT {COMMAND_COLUMNS} FROM bridge_command_receipts_v1 WHERE inbox_id=?1"),
        [id],
        command_row,
    )
    .optional()
    .map_err(map_rusqlite)
}
pub async fn get_command(
    db: &RunDatabase,
    id: &str,
) -> Result<Option<BridgeCommandReceipt>, StorageError> {
    let id = id.to_string();
    db.queue().submit(move |conn| command_conn(conn, &id)).await
}
/// 先领取命令执行收据再调用正式派发器；重投和重启不再次执行有副作用的命令。
pub async fn claim_command(
    db: &RunDatabase,
    inbox_id: &str,
    expected_generation: u64,
    command: &str,
    now: u64,
) -> Result<BridgeCommandClaim, StorageError> {
    text_id(command)?;
    let (id, command, now, busy) = (
        inbox_id.to_string(),
        command.to_string(),
        number(now)?,
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        if let Some(old)=command_conn(conn,&id)?{return if old.command==command{Ok(BridgeCommandClaim{claimed:false,receipt:old})}else{Err(conflict("command identity changed"))};}
        let inbox=inbox_conn(conn,&id)?.ok_or_else(||invalid("command inbox missing"))?;let cfg=current(conn,&inbox.input.connector_id,expected_generation)?;
        if inbox.connector_generation!=expected_generation || inbox.status!=BridgeInboxStatus::Claimed || inbox.run_id.is_some() || inbox.role!=BridgeRole::Owner || inbox.input.conversation_kind!=ConversationKind::Dm || cfg.input.owner_sender_id.as_deref()!=Some(&inbox.input.sender_id){return Err(conflict("command requires a fresh current owner DM"));}
        let payload:Value=serde_json::from_str(&inbox.input.payload_json).map_err(|e|invalid(e.to_string()))?;let text=payload.get("text").and_then(Value::as_str).unwrap_or("").trim();
        let slash=text.split_whitespace().next().unwrap_or("");
        if !((slash.starts_with('/') && slash.trim_start_matches('/').eq_ignore_ascii_case(command.trim_start_matches('/'))) || (matches!(command.trim_start_matches('/'),"rc_select"|"rc") && !text.is_empty() && text.bytes().all(|b|b.is_ascii_digit()))){return Err(conflict("command does not match frozen message"));}
        conn.execute("INSERT INTO bridge_command_receipts_v1(inbox_id,command,status,created_at,updated_at) VALUES(?1,?2,'started',?3,?3)",params![id,command,now]).map_err(map_rusqlite)?;
        conn.execute("UPDATE bridge_inbox_v1 SET status='command',updated_at=?2 WHERE id=?1",params![id,now]).map_err(map_rusqlite)?;
        Ok(BridgeCommandClaim{claimed:true,receipt:command_conn(conn,&id)?.ok_or_else(||invalid("command missing after claim"))?})
    })).await
}
pub async fn settle_command(
    db: &RunDatabase,
    inbox_id: &str,
    status: BridgeCommandStatus,
    result: Option<Value>,
    error: Option<String>,
    now: u64,
) -> Result<BridgeCommandReceipt, StorageError> {
    if status == BridgeCommandStatus::Started
        || (status == BridgeCommandStatus::Confirmed && result.is_none())
    {
        return Err(invalid(
            "command terminal status and actual result required",
        ));
    }
    let (id, now, busy) = (
        inbox_id.to_string(),
        number(now)?,
        db.options().busy_timeout_ms,
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let old=command_conn(conn,&id)?.ok_or_else(||invalid("command receipt missing"))?;
        if old.status!=BridgeCommandStatus::Started{return if old.status==status&&old.result==result&&old.error==error{Ok(old)}else{Err(conflict("command terminal receipt cannot change"))};}
        conn.execute("UPDATE bridge_command_receipts_v1 SET status=?2,result_json=?3,error=?4,updated_at=?5 WHERE inbox_id=?1",params![id,wire(status),result.as_ref().map(Value::to_string),error,now]).map_err(map_rusqlite)?;
        command_conn(conn,&id)?.ok_or_else(||invalid("command receipt missing"))
    })).await
}
pub async fn recover_started_commands(db: &RunDatabase, now: u64) -> Result<usize, StorageError> {
    let (now, busy) = (number(now)?, db.options().busy_timeout_ms);
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|conn.execute("UPDATE bridge_command_receipts_v1 SET status='unknown',error='command result unknown after restart; do not execute again',updated_at=?1 WHERE status='started'",[now]).map_err(map_rusqlite))).await
}
pub async fn list_pending_command_replies(
    db: &RunDatabase,
) -> Result<Vec<BridgeCommandReceipt>, StorageError> {
    db.queue().submit(move|conn|{let mut stmt=conn.prepare(&format!("SELECT {COMMAND_COLUMNS} FROM bridge_command_receipts_v1 WHERE status='confirmed' AND NOT EXISTS(SELECT 1 FROM bridge_outbox_v1 o WHERE o.inbox_id=bridge_command_receipts_v1.inbox_id AND o.run_id IS NULL) ORDER BY created_at,inbox_id")).map_err(map_rusqlite)?;let rows=stmt.query_map([],command_row).map_err(map_rusqlite)?;rows.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)}).await
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeManualActionInput {
    pub id: String,
    pub connector_id: String,
    pub connector_generation: u64,
    pub target_inbox_id: String,
    pub source_session_id: String,
    pub file_id: String,
    /// 宿主读取真实、受限字节后计算，不能从外部请求直接复制。
    pub content_sha256: String,
    pub label: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeManualAction {
    #[serde(flatten)]
    pub input: BridgeManualActionInput,
    pub filename: String,
    pub mime: String,
    pub size_bytes: u64,
    pub created_at: i64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeManualActionClaim {
    pub created: bool,
    pub action: BridgeManualAction,
    pub outbox: BridgeOutbox,
}
const MANUAL_COLUMNS:&str="id,connector_id,connector_generation,target_inbox_id,source_session_id,file_id,content_sha256,label,filename,mime,size_bytes,created_at";
fn manual_row(row: &Row<'_>) -> rusqlite::Result<BridgeManualAction> {
    Ok(BridgeManualAction {
        input: BridgeManualActionInput {
            id: row.get(0)?,
            connector_id: row.get(1)?,
            connector_generation: u64_row(row, 2)?,
            target_inbox_id: row.get(3)?,
            source_session_id: row.get(4)?,
            file_id: row.get(5)?,
            content_sha256: row.get(6)?,
            label: row.get(7)?,
        },
        filename: row.get(8)?,
        mime: row.get(9)?,
        size_bytes: u64_row(row, 10)?,
        created_at: row.get(11)?,
    })
}
fn manual_conn(conn: &Connection, id: &str) -> Result<Option<BridgeManualAction>, StorageError> {
    conn.query_row(
        &format!("SELECT {MANUAL_COLUMNS} FROM bridge_manual_actions_v1 WHERE id=?1"),
        [id],
        manual_row,
    )
    .optional()
    .map_err(map_rusqlite)
}
pub async fn get_manual_action(
    db: &RunDatabase,
    id: &str,
) -> Result<Option<BridgeManualAction>, StorageError> {
    let id = id.to_string();
    db.queue().submit(move |conn| manual_conn(conn, &id)).await
}
fn validate_manual_source(
    conn: &Connection,
    cfg: &BridgeConnector,
    input: &BridgeManualActionInput,
) -> Result<(String, String, u64, String), StorageError> {
    let owned:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM sessions WHERE session_id=?1 AND agent_id=?2 AND owner_user_id=?3 AND lifecycle='active')",params![input.source_session_id,cfg.input.actor_id,LOCAL_OWNER_SUBJECT],|row|row.get(0)).map_err(map_rusqlite)?;
    if !owned {
        return Err(conflict(
            "manual source session is not currently owned by local user and actor",
        ));
    }
    let target = inbox_conn(conn, &input.target_inbox_id)?
        .ok_or_else(|| invalid("manual target inbox missing"))?;
    if target.input.connector_id != input.connector_id
        || target.connector_generation != input.connector_generation
        || target.actor_id != cfg.input.actor_id
    {
        return Err(conflict(
            "manual target is not a current known conversation",
        ));
    }
    type FileMeta = (String, String, i64, String);
    let file:Option<FileMeta>=conn.query_row("SELECT filename,mime,size_bytes,file_kind FROM session_files WHERE file_id=?1 AND owner_session_id=?2 AND status='available' AND is_directory=0",params![input.file_id,input.source_session_id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).optional().map_err(map_rusqlite)?;
    let (filename, mime, size, kind) =
        file.ok_or_else(|| conflict("manual file identity missing, unavailable or not owned"))?;
    let size = u64::try_from(size).map_err(|_| invalid("negative manual file size"))?;
    if size > 50 * 1024 * 1024 {
        return Err(invalid("manual file exceeds 50 MiB"));
    }
    if filename.contains('/') || filename.contains('\\') || filename.contains('\0') {
        return Err(invalid("manual filename must be basename"));
    }
    if cfg.input.platform == "qq"
        && target.input.conversation_kind == ConversationKind::Group
        && !matches!(kind.as_str(), "image" | "video" | "audio")
    {
        return Err(invalid("QQ group document upload unsupported"));
    }
    Ok((filename, mime, size, kind))
}
/// 用户明确操作与排队同事务；无模型运行时也保留真实来源，不制造入站消息。
pub async fn enqueue_manual_action(
    db: &RunDatabase,
    input: &BridgeManualActionInput,
    now: u64,
) -> Result<BridgeManualActionClaim, StorageError> {
    for id in [
        &input.id,
        &input.connector_id,
        &input.target_inbox_id,
        &input.source_session_id,
        &input.file_id,
    ] {
        text_id(id)?;
    }
    if input.content_sha256.len() != 64
        || !input
            .content_sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid("manual content digest invalid"));
    }
    let (input, now, busy) = (input.clone(), number(now)?, db.options().busy_timeout_ms);
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{
        let delivery=format!("manual:{}",input.id);
        if let Some(old)=manual_conn(conn,&input.id)?{return if old.input==input{Ok(BridgeManualActionClaim{created:false,action:old,outbox:outbox_conn(conn,&delivery)?.ok_or_else(||conflict("manual action missing its atomic outbox"))?})}else{Err(conflict("manual request identity payload conflict"))};}
        let cfg=current(conn,&input.connector_id,input.connector_generation)?;let(filename,mime,size,kind)=validate_manual_source(conn,&cfg,&input)?;
        conn.execute("INSERT INTO bridge_manual_actions_v1(id,connector_id,connector_generation,target_inbox_id,source_session_id,file_id,content_sha256,label,filename,mime,size_bytes,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",params![input.id,input.connector_id,generation(input.connector_generation)?,input.target_inbox_id,input.source_session_id,input.file_id,input.content_sha256,input.label,filename,mime,number(size)?,now]).map_err(map_rusqlite)?;
        let kind=match kind.as_str(){"image"=>BridgeAttachmentKind::Image,"video"=>BridgeAttachmentKind::Video,"audio"=>BridgeAttachmentKind::Audio,_=>BridgeAttachmentKind::File};
        let queued=enqueue_outbox_conn(conn,&BridgeOutboxInput{delivery_id:delivery,connector_id:input.connector_id.clone(),connector_generation:input.connector_generation,inbox_id:None,manual_action_id:Some(input.id.clone()),run_id:None,loop_delivery_id:None,payload_json:serde_json::json!({"text":""}).to_string(),attachments:vec![BridgeAttachment{attachment_id:format!("manual:{}",input.id),kind,platform_ref:None,filename:Some(filename),mime:Some(mime),size_bytes:Some(u32::try_from(size).map_err(|_|invalid("manual file size exceeds protocol"))?)}]},now)?;
        Ok(BridgeManualActionClaim{created:true,action:manual_conn(conn,&input.id)?.ok_or_else(||invalid("manual action missing after write"))?,outbox:queued.outbox})
    })).await
}

pub fn conversation_key(conversation: &BridgeConversation) -> String {
    let key = serde_json::json!([
        conversation.connector_id,
        conversation.conversation_id,
        conversation.sender_id,
        conversation.actor_id
    ]);
    format!(
        "bridge-conversation-{}",
        lingxi_protocol::canon::sha256_hex(&lingxi_protocol::canon::canonical_json_bytes(&key))
    )
}
pub async fn latest_conversation_inbox(
    db: &RunDatabase,
    conversation: &BridgeConversation,
    generation: u64,
) -> Result<Option<BridgeInbox>, StorageError> {
    let conversation = conversation.clone();
    db.queue().submit(move|conn|{let mut stmt=conn.prepare(&format!("SELECT {INBOX_COLUMNS} FROM bridge_inbox_v1 WHERE connector_id=?1 AND conversation_id=?2 AND sender_id=?3 AND actor_id=?4 AND connector_generation=?5 ORDER BY created_at DESC,id DESC LIMIT 1")).map_err(map_rusqlite)?;stmt.query_row(params![conversation.connector_id,conversation.conversation_id,conversation.sender_id,conversation.actor_id,crate::storage::bridge::generation(generation)?],inbox_row).optional().map_err(map_rusqlite)}).await
}

/// 兼容旧会话键但拒绝歧义；规范键包含真实连接、会话、发送者与角色身份。
pub async fn find_conversation(
    db: &RunDatabase,
    actor: &str,
    key: &str,
    platform: Option<&str>,
    sender: Option<&str>,
) -> Result<Option<BridgeConversation>, StorageError> {
    let (actor, key, platform, sender) = (
        actor.to_string(),
        key.to_string(),
        platform.map(str::to_string),
        sender.map(str::to_string),
    );
    db.queue().submit(move|conn|{
    let mut stmt=conn.prepare(&format!("SELECT {CONVERSATION_COLUMNS} FROM bridge_conversations_v1 WHERE actor_id=?1 ORDER BY updated_at DESC")).map_err(map_rusqlite)?;let rows=stmt.query_map([actor],conversation_row).map_err(map_rusqlite)?;let mut found=None;
    for row in rows {let row=row.map_err(map_rusqlite)?;let cfg=connector_conn(conn,&row.connector_id)?.ok_or_else(||invalid("conversation connector missing"))?;
        if platform.as_ref().is_some_and(|p|p!=&cfg.input.platform)||sender.as_ref().is_some_and(|s|s!=&row.sender_id){continue;}
        if conversation_key(&row)!=key && row.conversation_id!=key && row.current_session_id.as_deref()!=Some(&key){continue;}
        if found.is_some(){return Err(conflict("ambiguous legacy conversation key; use canonical sessionKey"));}found=Some(row);
    }
    Ok(found)
}).await
}
