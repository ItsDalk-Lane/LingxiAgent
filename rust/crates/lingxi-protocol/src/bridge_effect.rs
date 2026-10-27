//! 原生等待与预览效果独立于最终消息投递；所有命令只由可信宿主授予真实来源。
use crate::{BridgeConversationKind, BridgeReplyContext};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BridgeEffectOperation {
    ReceiptStart,
    ReceiptStop,
    StreamUpdate,
    StreamFinish,
    StreamCancel,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BridgeEffectStatus {
    Confirmed,
    Failed,
    Unknown,
    Skipped,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BridgeWaitingReceiptMode {
    NativeTyping,
    Text,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BridgeStreamMode {
    Batch,
    Draft,
    RichDraft,
    EditMessage,
    CardkitStream,
    Block,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeWaitingReceiptCapability {
    pub mode: BridgeWaitingReceiptMode,
    pub scopes: Vec<BridgeConversationKind>,
    pub refresh_interval_ms: u32,
    pub cancellable: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeStreamCapability {
    pub mode: BridgeStreamMode,
    pub scopes: Vec<BridgeConversationKind>,
    pub min_interval_ms: u32,
    pub max_chars: u32,
    pub rich: bool,
    pub receipt_folded: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeEffectCapabilities {
    pub receipt: BridgeWaitingReceiptCapability,
    pub streams: Vec<BridgeStreamCapability>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeEffectRequest {
    pub effect_id: String,
    pub stream_id: String,
    pub inbox_id: String,
    pub run_id: String,
    #[schemars(range(min = 1, max = 4294967295u64))]
    pub sequence: u32,
    pub operation: BridgeEffectOperation,
    pub delivery_id: Option<String>,
    pub conversation_id: String,
    pub reply_context: BridgeReplyContext,
    pub text: String,
    pub rich: bool,
}
impl BridgeEffectRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        for id in [
            &self.effect_id,
            &self.stream_id,
            &self.inbox_id,
            &self.run_id,
            &self.conversation_id,
        ] {
            if id.trim().is_empty() || id.len() > 1024 || id.contains('\0') {
                return Err("invalid Bridge effect source identity");
            }
        }
        if self.sequence == 0
            || self.reply_context.is_group.is_none()
            || self.text.len() > 100_000
            || self.text.contains('\0')
        {
            return Err("invalid Bridge effect sequence, scope or text");
        }
        if self
            .delivery_id
            .as_deref()
            .is_some_and(|id| id.trim().is_empty() || id.len() > 1024 || id.contains('\0'))
        {
            return Err("invalid Bridge effect delivery identity");
        }
        if (self.operation == BridgeEffectOperation::StreamFinish) != self.delivery_id.is_some()
            || (self.operation == BridgeEffectOperation::StreamFinish
                && self.text.trim().is_empty())
        {
            return Err("final Bridge effect requires an actual outbox identity and text");
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeEffectReceipt {
    pub effect_id: String,
    pub stream_id: String,
    #[schemars(range(min = 1, max = 4294967295u64))]
    pub sequence: u32,
    pub operation: BridgeEffectOperation,
    pub delivery_id: Option<String>,
    pub status: BridgeEffectStatus,
    pub platform_ack: bool,
    pub platform_message_ids: Vec<String>,
    pub error: Option<String>,
}
impl BridgeEffectReceipt {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.effect_id.trim().is_empty()
            || self.stream_id.trim().is_empty()
            || self.effect_id.len() > 1024
            || self.stream_id.len() > 1024
            || self.sequence == 0
        {
            return Err("invalid Bridge effect identity");
        }
        if self.platform_message_ids.len() > 16
            || self
                .platform_message_ids
                .iter()
                .any(|id| id.trim().is_empty() || id.len() > 4096)
        {
            return Err("invalid Bridge effect platform IDs");
        }
        if (self.operation == BridgeEffectOperation::StreamFinish) != self.delivery_id.is_some()
            || self
                .delivery_id
                .as_deref()
                .is_some_and(|id| id.trim().is_empty() || id.len() > 1024 || id.contains('\0'))
        {
            return Err("Bridge final effect receipt must bind its actual outbox identity");
        }
        if self.status == BridgeEffectStatus::Confirmed
            && (!self.platform_ack
                || (self.operation == BridgeEffectOperation::StreamFinish
                    && (self.platform_message_ids.is_empty()
                        || self.delivery_id.as_deref().is_none_or(str::is_empty))))
        {
            return Err("Bridge effect confirmation lacks actual platform evidence");
        }
        if self.status == BridgeEffectStatus::Skipped
            && (self.platform_ack
                || !self.platform_message_ids.is_empty()
                || self.error.as_deref().is_none_or(str::is_empty))
        {
            return Err("skipped Bridge effect cannot claim an external operation");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn receipt() -> BridgeEffectReceipt {
        BridgeEffectReceipt {
            effect_id: "effect-1".into(),
            stream_id: "stream-1".into(),
            sequence: 1,
            operation: BridgeEffectOperation::StreamUpdate,
            delivery_id: None,
            status: BridgeEffectStatus::Confirmed,
            platform_ack: true,
            platform_message_ids: vec![],
            error: None,
        }
    }

    #[test]
    fn preview_ack_does_not_confirm_a_final_delivery() {
        let mut value = receipt();
        assert!(value.validate().is_ok());
        value.operation = BridgeEffectOperation::StreamFinish;
        assert!(value.validate().is_err());
        value.delivery_id = Some("delivery-1".into());
        assert!(value.validate().is_err());
        value.platform_message_ids = vec!["actual-platform-id".into()];
        assert!(value.validate().is_ok());
        value.platform_ack = false;
        assert!(value.validate().is_err());
        value.status = BridgeEffectStatus::Unknown;
        assert!(value.validate().is_ok());
    }

    #[test]
    fn skipped_effect_cannot_claim_an_external_ack_or_message() {
        let mut value = receipt();
        value.status = BridgeEffectStatus::Skipped;
        value.error = Some("native_streaming_scope_unsupported".into());
        assert!(value.validate().is_err());
        value.platform_ack = false;
        assert!(value.validate().is_ok());
        value.platform_message_ids = vec!["fabricated-id".into()];
        assert!(value.validate().is_err());
        value.platform_message_ids.clear();
        value.error = None;
        assert!(value.validate().is_err());
    }

    #[test]
    fn effect_and_capability_fields_are_closed_and_keep_old_ready_compatible() {
        let value = json!({"effectId":"fx","streamId":"st","inboxId":"in","runId":"r","sequence":1,"operation":"stream_update","deliveryId":null,"conversationId":"dm","replyContext":{"isGroup":false},"text":"实际预览","rich":false});
        let decoded: BridgeEffectRequest = serde_json::from_value(value.clone()).unwrap();
        assert!(decoded.validate().is_ok());
        let mut invalid = decoded.clone();
        invalid.sequence = 0;
        assert!(invalid.validate().is_err());
        invalid = decoded.clone();
        invalid.reply_context.is_group = None;
        assert!(invalid.validate().is_err());
        invalid = decoded;
        invalid.operation = BridgeEffectOperation::StreamFinish;
        assert!(invalid.validate().is_err());
        for key in ["owner", "role", "permissionMode", "principal"] {
            let mut forged = value.clone();
            forged[key] = json!("owner");
            assert!(serde_json::from_value::<BridgeEffectRequest>(forged).is_err());
            let mut forged = value.clone();
            forged["replyContext"][key] = json!("owner");
            assert!(serde_json::from_value::<BridgeEffectRequest>(forged).is_err());
        }
        let old_ready = json!({"kind":"ready","connectorId":"c","generation":1});
        assert!(serde_json::from_value::<crate::BridgeWorkerEvent>(old_ready).is_ok());
        let mut invalid = serde_json::to_value(receipt()).unwrap();
        invalid["ownerSubject"] = json!("user_local");
        assert!(serde_json::from_value::<BridgeEffectReceipt>(invalid).is_err());
    }
}
