//! 手机状态来自真实投递和运行；下一次提醒与正式发现游标使用同一时间条件。
use crate::conversations::{ConversationApi, ConversationError};
use lingxi_adapters::storage::{conversation_discovery, conversation_status};
use serde_json::{json, Value};

pub(crate) async fn ticker(
    api: &ConversationApi,
    conversation: &str,
    actor: &str,
) -> Result<Value, ConversationError> {
    let snapshot = conversation_status::snapshot(&api.storage, conversation, actor).await?;
    let next_reminder = if conversation.starts_with("dm:")
        || !api.enabled()?
        || snapshot.queued
        || snapshot.running
    {
        None
    } else {
        let doc = api.files.read_channel(conversation)?;
        let settings = api.settings(actor, conversation)?;
        if !settings.proactive_enabled || doc.messages.is_empty() {
            None
        } else {
            let last = doc.messages.last().expect("非空消息已核实");
            let parsed =
                chrono::NaiveDateTime::parse_from_str(&last.timestamp, "%Y-%m-%d %H:%M:%S")
                    .or_else(|_| {
                        chrono::NaiveDateTime::parse_from_str(&last.timestamp, "%Y-%m-%d %H:%M")
                    })
                    .map_err(|e| ConversationError::new(409, e))?;
            let actual = chrono::TimeZone::from_local_datetime(&chrono::Local, &parsed)
                .latest()
                .ok_or_else(|| {
                    ConversationError::new(
                        409,
                        "conversation timestamp is not a local clock instant",
                    )
                })?
                .timestamp_millis();
            if actual < 0 {
                None
            } else {
                let base =
                    (actual as u64).max(api.last_phone_completion_ms(&doc).await?.unwrap_or(0));
                let interval = u64::from(settings.reminder_interval_minutes) * 60_000;
                let cursor = conversation_discovery::cursor(&api.storage, conversation).await?;
                let next_slot = cursor
                    .map(|c| c.proactive_slot)
                    .unwrap_or(0)
                    .checked_add(1)
                    .and_then(|slot| slot.checked_mul(interval))
                    .ok_or_else(|| ConversationError::new(409, "phone reminder slot overflow"))?;
                let due = base
                    .checked_add(interval)
                    .ok_or_else(|| ConversationError::new(409, "phone reminder time overflow"))?
                    .max(next_slot);
                let due_time =
                    chrono::DateTime::from_timestamp_millis(i64::try_from(due).map_err(|_| {
                        ConversationError::new(409, "phone reminder timestamp overflow")
                    })?)
                    .ok_or_else(|| {
                        ConversationError::new(409, "phone reminder timestamp out of range")
                    })?;
                Some(
                    json!({"channelName":conversation,"dueAt":due_time.to_rfc3339_opts(chrono::SecondsFormat::Millis,true),"dueAtMs":due,"intervalMs":interval}),
                )
            }
        }
    };
    Ok(
        json!({"active":snapshot.active,"queued":snapshot.queued,"running":snapshot.running,"nextReminder":next_reminder}),
    )
}
