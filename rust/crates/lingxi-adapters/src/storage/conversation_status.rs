//! 状态只统计持久接收者与真实因果Run；领取队列不是执行成功。
use super::{migrations::map_rusqlite, RunDatabase};
use lingxi_kernel::{ports::StorageError, LOCAL_OWNER_SUBJECT};
use rusqlite::params;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneTickerActive {
    pub agent_id: String,
    pub delivered: u64,
    pub agent_count: u64,
    pub checks: u64,
    pub max_checks: u64,
    pub mode: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneTickerSnapshot {
    pub active: Option<PhoneTickerActive>,
    pub queued: bool,
    pub running: bool,
}
const ROOT_PROOF: &str = r#"
 SELECT d.delivery_id,d.root_chain_id,d.cause_id,d.actor_id,r.run_id,r.status,r.updated_at_unix_ms AS completed_at
 FROM phone_deliveries_v1 d JOIN run_lineage l ON l.cause_id=d.delivery_id AND l.origin=d.conversation_type
 JOIN runs r ON r.run_id=l.run_id JOIN sessions s ON s.session_id=r.session_id
 WHERE d.owner_subject=?3 AND r.owner_subject=d.owner_subject AND r.owner_kind='local_user'
 AND s.owner_user_id=d.owner_subject AND s.agent_id=d.actor_id
 AND l.source_message_id=json_extract(d.input_json,'$.sourceMessageId')
 AND l.parent_run_id IS json_extract(d.input_json,'$.sourceRunId')
 AND r.session_id=json_extract(d.input_json,'$.inputJson.sessionId')
 AND (d.run_id IS NULL OR d.run_id=r.run_id) AND (d.session_id IS NULL OR d.session_id=r.session_id)
 AND (SELECT COUNT(*) FROM run_lineage other WHERE other.cause_id=d.delivery_id AND other.origin IN ('channel','dm'))=1
"#;
const SNAPSHOT: &str = r#"
WITH RECURSIVE
scoped AS (
 SELECT * FROM phone_deliveries_v1 d WHERE d.owner_subject=?3 AND
 ((?1 NOT LIKE 'dm:%' AND d.conversation_id=?1 AND d.conversation_type='channel') OR
  (?1 LIKE 'dm:%' AND d.conversation_type='dm' AND
   ((d.actor_id=?2 AND d.conversation_id=?1) OR
    (d.actor_id=substr(?1,4) AND d.conversation_id='dm:'||?2))))
),
roots AS (
/*ROOT_PROOF*/
),
causal(delivery_id,run_id) AS (
 SELECT d.delivery_id,r.run_id FROM scoped d JOIN roots r ON r.delivery_id=d.delivery_id
 UNION SELECT tree.delivery_id,l.run_id FROM causal tree JOIN run_lineage l ON l.parent_run_id=tree.run_id
),
active AS (
 SELECT d.delivery_id,d.root_chain_id,d.cause_id,s.agent_id,r.created_at_unix_ms,r.run_id
 FROM causal t JOIN scoped d ON d.delivery_id=t.delivery_id
 JOIN runs r ON r.run_id=t.run_id JOIN sessions s ON s.session_id=r.session_id
 WHERE r.owner_subject=?3 AND s.owner_user_id=?3 AND s.lifecycle='active'
 AND r.status IN ('queued','running','waiting_approval','cancelling')
 ORDER BY r.created_at_unix_ms,r.run_id LIMIT 1
)
SELECT EXISTS(SELECT 1 FROM scoped d WHERE d.status='claimed' AND NOT EXISTS(SELECT 1 FROM run_lineage l WHERE l.cause_id=d.delivery_id AND l.origin IN ('channel','dm'))),
 a.agent_id,
 (SELECT COUNT(DISTINCT r.delivery_id) FROM roots r WHERE r.root_chain_id=a.root_chain_id AND r.cause_id=a.cause_id),
 (SELECT COUNT(DISTINCT d.actor_id) FROM phone_deliveries_v1 d WHERE d.owner_subject=?3 AND d.root_chain_id=a.root_chain_id AND d.cause_id=a.cause_id),
 (SELECT COUNT(DISTINCT r.delivery_id) FROM roots r WHERE r.root_chain_id=a.root_chain_id),
 (SELECT c.budget_limit FROM phone_chains_v1 c WHERE c.root_chain_id=a.root_chain_id AND c.owner_subject=?3),
 CASE WHEN EXISTS(SELECT 1 FROM phone_deliveries_v1 d WHERE d.root_chain_id=a.root_chain_id AND d.owner_subject=?3 AND d.cause_id LIKE 'phone-proactive-%') THEN 'reminder' ELSE 'delivery' END
FROM (SELECT 1) LEFT JOIN active a ON 1=1
"#;
pub async fn snapshot(
    db: &RunDatabase,
    conversation: &str,
    actor: &str,
) -> Result<PhoneTickerSnapshot, StorageError> {
    let (conversation, actor) = (conversation.to_owned(), actor.to_owned());
    db.queue()
        .submit(move |conn| {
            conn.query_row(
                &SNAPSHOT.replace("/*ROOT_PROOF*/", ROOT_PROOF),
                params![conversation, actor, LOCAL_OWNER_SUBJECT],
                |row| {
                    let agent: Option<String> = row.get(1)?;
                    let number = |index| -> rusqlite::Result<u64> {
                        let value: i64 = row.get(index)?;
                        u64::try_from(value).map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                index,
                                rusqlite::types::Type::Integer,
                                Box::new(e),
                            )
                        })
                    };
                    let active = if let Some(agent_id) = agent {
                        Some(PhoneTickerActive {
                            agent_id,
                            delivered: number(2)?,
                            agent_count: number(3)?,
                            checks: number(4)?,
                            max_checks: number(5)?,
                            mode: row.get(6)?,
                        })
                    } else {
                        None
                    };
                    Ok(PhoneTickerSnapshot {
                        running: active.is_some(),
                        active,
                        queued: row.get(0)?,
                    })
                },
            )
            .map_err(map_rusqlite)
        })
        .await
}

/// 只认当前成员已存在的真实终态Run；恢复收据的写入时间不代表完成时刻。
pub async fn last_completion(
    db: &RunDatabase,
    conversation: &str,
    members: Vec<String>,
) -> Result<Option<u64>, StorageError> {
    let conversation = conversation.to_owned();
    let members = serde_json::to_string(&members).map_err(|e| StorageError::InvalidRequest {
        detail: e.to_string(),
    })?;
    db.queue().submit(move|conn|{
        let sql=format!("WITH roots AS ({ROOT_PROOF}) SELECT MAX(r.completed_at) FROM roots r JOIN phone_deliveries_v1 d ON d.delivery_id=r.delivery_id WHERE d.conversation_id=?1 AND d.conversation_type='channel' AND r.actor_id IN (SELECT value FROM json_each(?2)) AND r.status IN ('completed','failed','interrupted_needs_attention')");
        let value:Option<i64>=conn.query_row(&sql,params![conversation,members,LOCAL_OWNER_SUBJECT],|row|row.get(0)).map_err(map_rusqlite)?;
        value.map(|value|u64::try_from(value).map_err(|_|StorageError::Corrupted{detail:"negative phone Run completion timestamp".into()})).transpose()
    }).await
}
