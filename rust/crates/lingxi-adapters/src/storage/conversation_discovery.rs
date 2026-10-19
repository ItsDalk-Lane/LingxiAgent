//! 手机旧文件发现游标；最新消息已有正式写入收据时沿原因果恢复，不能另造根链。
use super::{migrations::map_rusqlite, run_store::with_write_txn, RunDatabase};
use lingxi_kernel::ports::StorageError;
use rusqlite::{params, OptionalExtension};
#[derive(Debug, Clone)]
pub struct DiscoveryCursor {
    pub fingerprint: String,
    pub proactive_slot: u64,
    pub updated_at: u64,
}
pub async fn cursor(
    db: &RunDatabase,
    conversation: &str,
) -> Result<Option<DiscoveryCursor>, StorageError> {
    let id = conversation.to_string();
    db.queue().submit(move|conn|{
 conn.query_row("SELECT message_fingerprint,proactive_slot,updated_at FROM phone_discovery_cursors_v1 WHERE conversation_id=?1",[id],|r|Ok(DiscoveryCursor{fingerprint:r.get(0)?,proactive_slot:r.get::<_,i64>(1)? as u64,updated_at:r.get::<_,i64>(2)? as u64})).optional().map_err(map_rusqlite)
}).await
}
pub async fn save_cursor(
    db: &RunDatabase,
    conversation: &str,
    fingerprint: &str,
    slot: u64,
    now: u64,
) -> Result<(), StorageError> {
    let (id, fp, busy) = (
        conversation.to_string(),
        fingerprint.to_string(),
        db.options().busy_timeout_ms,
    );
    let invalid = || StorageError::InvalidRequest {
        detail: "phone cursor integer overflow".into(),
    };
    let (slot, now) = (
        i64::try_from(slot).map_err(|_| invalid())?,
        i64::try_from(now).map_err(|_| invalid())?,
    );
    db.queue().submit(move|conn|with_write_txn(conn,busy,|conn|{conn.execute("INSERT INTO phone_discovery_cursors_v1 VALUES(?1,?2,?3,?4) ON CONFLICT(conversation_id) DO UPDATE SET message_fingerprint=excluded.message_fingerprint,proactive_slot=MAX(proactive_slot,excluded.proactive_slot),updated_at=excluded.updated_at",params![id,fp,slot,now]).map_err(map_rusqlite)?;Ok(())})).await
}
/// 文件动作的原文和身份都必须精确命中；只按正文相似不能认作重复消息。
pub async fn message_operation(
    db: &RunDatabase,
    conversation: &str,
    sender: &str,
    body: &str,
    timestamp: &str,
) -> Result<Option<String>, StorageError> {
    let (id, sender, body, time) = (
        conversation.to_string(),
        sender.to_string(),
        body.to_string(),
        timestamp.to_string(),
    );
    db.queue().submit(move|conn|conn.query_row("SELECT operation_id FROM conversation_file_operations_v1 WHERE status='applied' AND json_extract(mutation_json,'$.body')=?3 AND json_extract(mutation_json,'$.timestamp')=?4 AND ((json_extract(mutation_json,'$.kind')='append_channel' AND json_extract(mutation_json,'$.id')=?1 AND json_extract(mutation_json,'$.sender')=?2) OR (json_extract(mutation_json,'$.kind')='append_dm' AND json_extract(mutation_json,'$.from')=?2 AND (?1='dm:'||json_extract(mutation_json,'$.from') OR ?1='dm:'||json_extract(mutation_json,'$.to')))) ORDER BY created_at DESC,rowid DESC LIMIT 1",params![id,sender,body,time],|r|r.get(0)).optional().map_err(map_rusqlite)).await
}
