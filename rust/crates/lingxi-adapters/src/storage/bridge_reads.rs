//! Bridge 读面只投影必要公开字段；凭证和平台回复令牌不进入管理页面。
use super::{migrations::map_rusqlite, RunDatabase};
use lingxi_kernel::ports::StorageError;
use serde::Serialize;
use serde_json::Value;

/// 委派链逐级核对真实运行的身份，避免仅凭父编号沿用桥接授权。
pub async fn has_run_identity(
    db: &RunDatabase,
    run_id: &str,
    session_id: &str,
    owner_kind: &str,
    owner_subject: &str,
) -> Result<bool, StorageError> {
    let values = (
        run_id.to_owned(),
        session_id.to_owned(),
        owner_kind.to_owned(),
        owner_subject.to_owned(),
    );
    db.queue().submit(move |conn| {
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM runs WHERE run_id=?1 AND session_id=?2 AND owner_kind=?3 AND owner_subject=?4)",
            rusqlite::params![values.0, values.1, values.2, values.3],
            |row| row.get(0),
        ).map_err(map_rusqlite)
    }).await
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeKnownSender {
    pub platform: String,
    pub user_id: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeMessageSummary {
    pub id: String,
    pub platform: String,
    pub user_id: String,
    pub session_key: String,
    pub message_id: String,
    pub text: String,
    pub status: String,
    pub run_id: Option<String>,
    pub timestamp: i64,
    pub agent_id: String,
    pub direction: String,
    pub sender: String,
    pub is_group: bool,
    pub ts: i64,
    /// 外发只有平台真实确认后才公开消息编号；排队和未知仍保留原状态。
    pub platform_message_ids: Vec<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeSessionSummary {
    pub platform: String,
    pub session_key: String,
    pub user_id: String,
    pub session_id: String,
    pub session_path: Option<String>,
    pub role: String,
    pub title: String,
    pub last_active_at: i64,
    pub chat_id: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub is_owner: bool,
    pub last_active: i64,
}
pub async fn known_senders(
    db: &RunDatabase,
    actor: &str,
) -> Result<Vec<BridgeKnownSender>, StorageError> {
    let actor = actor.to_owned();
    db.queue().submit(move|conn|{let mut stmt=conn.prepare("SELECT DISTINCT platform,sender_id FROM bridge_inbox_v1 WHERE actor_id=?1 ORDER BY platform,sender_id LIMIT 512").map_err(map_rusqlite)?;let rows=stmt.query_map([actor],|row|Ok(BridgeKnownSender{platform:row.get(0)?,user_id:row.get(1)?})).map_err(map_rusqlite)?;rows.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)}).await
}
pub async fn messages(
    db: &RunDatabase,
    actor: &str,
    limit: usize,
) -> Result<Vec<BridgeMessageSummary>, StorageError> {
    let actor = actor.to_owned();
    let limit = limit.clamp(1, 200) as i64;
    db.queue().submit(move |conn| {
        struct Row {
            id: String, platform: String, sender_id: String, connector_id: String,
            conversation_id: String, actor_id: String, conversation_kind: String,
            message_id: Option<String>, payload: String, status: String,
            run_id: Option<String>, created_at: i64, direction: String,
            receipt: Option<String>,
        }
        // 外发沿冻结的 target_inbox 取真实归属，不能从正文或 replyContext 猜主人身份。
        let mut statement = conn.prepare(
            "SELECT id,platform,sender_id,connector_id,conversation_id,actor_id,conversation_kind,message_id,payload_json,status,run_id,created_at,direction,receipt FROM (
             SELECT id,platform,sender_id,connector_id,conversation_id,actor_id,conversation_kind,message_id,payload_json,status,run_id,created_at,'in' AS direction,NULL AS receipt FROM bridge_inbox_v1 WHERE actor_id=?1
             UNION ALL
             SELECT o.delivery_id,i.platform,i.sender_id,o.connector_id,o.conversation_id,i.actor_id,o.conversation_kind,NULL,o.payload_json,o.status,o.run_id,o.created_at,'out',o.platform_receipt_json FROM bridge_outbox_v1 o JOIN bridge_inbox_v1 i ON i.id=o.target_inbox_id WHERE i.actor_id=?1
             ) ORDER BY created_at DESC,id DESC LIMIT ?2"
        ).map_err(map_rusqlite)?;
        let rows = statement.query_map((&actor, limit), |row| Ok(Row {
            id: row.get(0)?, platform: row.get(1)?, sender_id: row.get(2)?,
            connector_id: row.get(3)?, conversation_id: row.get(4)?, actor_id: row.get(5)?,
            conversation_kind: row.get(6)?, message_id: row.get(7)?, payload: row.get(8)?,
            status: row.get(9)?, run_id: row.get(10)?, created_at: row.get(11)?,
            direction: row.get(12)?, receipt: row.get(13)?,
        })).map_err(map_rusqlite)?;
        let mut result = Vec::new();
        for row in rows {
            let row = row.map_err(map_rusqlite)?;
            let corrupted = || StorageError::Corrupted { detail: "Bridge message projection damaged".into() };
            let payload: Value = serde_json::from_str(&row.payload).map_err(|_| corrupted())?;
            let text = payload["text"].as_str().ok_or_else(corrupted)?.to_owned();
            let is_group = match row.conversation_kind.as_str() { "group" => true, "dm" => false, _ => return Err(corrupted()) };
            // 与 bridge::conversation_key / sessions() 共用完全相同的规范化四元身份。
            let identity = serde_json::json!([&row.connector_id, &row.conversation_id, &row.sender_id, &row.actor_id]);
            let session_key = format!("bridge-conversation-{}", lingxi_protocol::canon::sha256_hex(&lingxi_protocol::canon::canonical_json_bytes(&identity)));
            let mut message_id = row.message_id.unwrap_or_default();
            let mut platform_message_ids = Vec::new();
            if row.direction == "out" && row.status == "confirmed" {
                let receipt: super::bridge::BridgePlatformReceipt = serde_json::from_str(row.receipt.as_deref().ok_or_else(corrupted)?).map_err(|_| corrupted())?;
                if receipt.platform != row.platform || receipt.message_ids.is_empty() || receipt.message_ids.iter().any(|id| id.trim().is_empty()) { return Err(corrupted()); }
                platform_message_ids = receipt.message_ids;
                message_id = platform_message_ids[0].clone();
            }
            result.push(BridgeMessageSummary {
                id: row.id, platform: row.platform, user_id: row.sender_id.clone(),
                session_key, message_id, text, status: row.status, run_id: row.run_id,
                timestamp: row.created_at, agent_id: row.actor_id.clone(),
                direction: row.direction.clone(), sender: if row.direction == "in" { row.sender_id } else { row.actor_id },
                is_group, ts: row.created_at, platform_message_ids,
            });
        }
        Ok(result)
    }).await
}

pub async fn sessions(
    db: &RunDatabase,
    actor: &str,
    platform: Option<&str>,
) -> Result<Vec<BridgeSessionSummary>, StorageError> {
    let mut result = Vec::new();
    for connector in super::bridge::list_connectors(db)
        .await?
        .into_iter()
        .filter(|row| {
            row.input.actor_id == actor
                && platform.is_none_or(|platform| row.input.platform == platform)
        })
    {
        for conversation in super::bridge::list_conversations(db, &connector.input.id, 512).await? {
            let session = match conversation.current_session_id.as_deref() {
                Some(sid) => db.get_session(sid).await?,
                None => None,
            };
            let title = session
                .as_ref()
                .map(|row| row.title.clone())
                .or_else(|| {
                    conversation.metadata["displayName"]
                        .as_str()
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| "Bridge".into());
            let session_id = conversation
                .current_session_id
                .clone()
                .filter(|_| session.is_some())
                .unwrap_or_default();
            result.push(BridgeSessionSummary {
                platform: connector.input.platform.clone(),
                session_key: super::bridge::conversation_key(&conversation),
                user_id: conversation.sender_id.clone(),
                session_path: (!session_id.is_empty()).then(|| format!("native:{session_id}")),
                session_id,
                role: if conversation.conversation_kind == super::bridge::ConversationKind::Dm
                    && connector.input.owner_sender_id.as_deref()
                        == Some(conversation.sender_id.as_str())
                {
                    "owner"
                } else {
                    "guest"
                }
                .into(),
                display_name: conversation.metadata["displayName"]
                    .as_str()
                    .unwrap_or(&title)
                    .to_owned(),
                avatar_url: conversation.metadata["avatarUrl"]
                    .as_str()
                    .map(str::to_owned),
                is_owner: conversation.conversation_kind == super::bridge::ConversationKind::Dm
                    && connector.input.owner_sender_id.as_deref()
                        == Some(conversation.sender_id.as_str()),
                chat_id: conversation.conversation_id.clone(),
                last_active: conversation.updated_at,
                title,
                last_active_at: conversation.updated_at,
            });
        }
    }
    result.sort_by(|a, b| {
        b.last_active_at
            .cmp(&a.last_active_at)
            .then_with(|| a.session_key.cmp(&b.session_key))
    });
    result.truncate(512);
    Ok(result)
}
pub async fn final_messages(
    db: &RunDatabase,
    session: &str,
    run: &str,
) -> Result<Vec<String>, StorageError> {
    let (session, run) = (session.to_owned(), run.to_owned());
    db.queue().submit(move|conn|{let mut stmt=conn.prepare("SELECT content_json FROM messages WHERE session_id=?1 AND run_id=?2 AND role='assistant' AND entry_type='message' ORDER BY seq").map_err(map_rusqlite)?;let rows=stmt.query_map([session,run],|row|row.get::<_,String>(0)).map_err(map_rusqlite)?;rows.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)}).await
}

/// 外发已经确认但上下文尚未写入的窗口，可在重启后安全补齐。
pub async fn pending_notices(db: &RunDatabase) -> Result<Vec<String>, StorageError> {
    db.queue().submit(move|conn|{let mut stmt=conn.prepare("SELECT o.delivery_id FROM bridge_outbox_v1 o WHERE o.status='confirmed' AND json_extract(o.payload_json,'$.contextPolicy')='record_when_delivered' AND NOT EXISTS(SELECT 1 FROM bridge_notice_receipts_v1 n WHERE n.delivery_id=o.delivery_id) ORDER BY o.created_at,o.delivery_id LIMIT 1024").map_err(map_rusqlite)?;let rows=stmt.query_map([],|row|row.get(0)).map_err(map_rusqlite)?;rows.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)}).await
}

/// 回收真实根运行和仍在继续的委派后代，不依赖收件中尚未绑定的 run_id。
pub async fn active_causal_runs(
    db: &RunDatabase,
    connector: &str,
    generation: u64,
) -> Result<Vec<(String, String, String)>, StorageError> {
    let connector = connector.to_owned();
    let generation = i64::try_from(generation).map_err(|_| StorageError::InvalidRequest {
        detail: "Bridge generation overflow".into(),
    })?;
    db.queue().submit(move|conn|{
        let mut stmt=conn.prepare("WITH RECURSIVE causal(inbox_id,run_id,root_run_id,session_id,owner_kind,owner_subject) AS (
            SELECT i.id,r.run_id,r.run_id,r.session_id,r.owner_kind,r.owner_subject
            FROM bridge_inbox_v1 i JOIN run_lineage l ON l.cause_id=i.id AND l.origin='bridge'
            JOIN runs r ON r.run_id=l.run_id WHERE i.connector_id=?1 AND i.connector_generation=?2
            UNION
            SELECT c.inbox_id,r.run_id,c.root_run_id,c.session_id,c.owner_kind,c.owner_subject
            FROM causal c JOIN run_lineage l ON l.parent_run_id=c.run_id AND l.origin='subagent'
            JOIN runs r ON r.run_id=l.run_id AND r.session_id=c.session_id AND r.owner_kind=c.owner_kind AND r.owner_subject=c.owner_subject
        ) SELECT c.inbox_id,c.run_id,c.root_run_id FROM causal c JOIN runs r ON r.run_id=c.run_id
        WHERE r.status NOT IN ('completed','cancelled','failed') ORDER BY c.inbox_id,c.run_id").map_err(map_rusqlite)?;
        let rows=stmt.query_map((connector,generation),|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).map_err(map_rusqlite)?;
        rows.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)
    }).await
}
