//! Bridge 管道只携带平台事实；发送者角色由可信宿主配置核定，消息不能自报授权。
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BridgeRole {
    Owner,
    Guest,
}

/// 归属键与角色、连接代次无关；重配不能把旧消息变成另一个主体。
pub fn bridge_subject(
    platform: &str,
    connector_id: &str,
    sender_id: &str,
) -> Result<String, &'static str> {
    if !matches!(
        platform,
        "wechat" | "feishu" | "dingtalk" | "telegram" | "qq"
    ) || connector_id.trim().is_empty()
        || sender_id.trim().is_empty()
        || connector_id.len() > 1024
        || sender_id.len() > 1024
    {
        return Err("invalid Bridge actor identity");
    }
    let value = serde_json::json!([platform, connector_id, sender_id]);
    Ok(format!(
        "bridge:{}",
        crate::canon::sha256_hex(&crate::canon::canonical_json_bytes(&value))
    ))
}

pub fn valid_bridge_subject(subject: &str) -> bool {
    subject.strip_prefix("bridge:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BridgeConversationKind {
    Dm,
    Group,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BridgeAttachmentKind {
    Image,
    Audio,
    Video,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeAttachment {
    pub attachment_id: String,
    pub kind: BridgeAttachmentKind,
    pub platform_ref: Option<String>,
    pub filename: Option<String>,
    pub mime: Option<String>,
    pub size_bytes: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeReplyContext {
    pub message_id: Option<String>,
    pub text: Option<String>,
    pub sender_id: Option<String>,
    pub message_thread_id: Option<u32>,
    pub context_token: Option<String>,
    pub target_chat_id: Option<String>,
    pub is_group: Option<bool>,
    pub target_type: Option<BridgeTargetType>,
    pub guild_id: Option<String>,
    pub channel_id: Option<String>,
    pub root_id: Option<String>,
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BridgeTargetType {
    User,
    Group,
    Channel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BridgeReceiptStatus {
    Confirmed,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BridgeConnectionStatus {
    Connecting,
    Connected,
    Disconnected,
}

/// generation 在启动边界显式拒绝超过 u32；不能截断持久配置代次。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BridgeWorkerCommand {
    Init {
        connector_id: String,
        #[schemars(range(max = 4294967295u64))]
        generation: u32,
        platform: String,
        config: serde_json::Value,
    },
    Send {
        connector_id: String,
        #[schemars(range(max = 4294967295u64))]
        generation: u32,
        /// 可信管道在正式派发时填入；同一 worker 生命周期内严格递增。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schemars(range(min = 1, max = 4294967295u64))]
        dispatch_seq: Option<u32>,
        delivery_id: String,
        conversation_id: String,
        text: String,
        reply_context: Option<BridgeReplyContext>,
        attachments: Vec<String>,
    },
    MediaBegin {
        connector_id: String,
        #[schemars(range(max = 4294967295u64))]
        generation: u32,
        attachment_id: String,
        message_id: Option<String>,
        size_bytes: u32,
        mime: Option<String>,
        filename: Option<String>,
    },
    MediaChunk {
        connector_id: String,
        #[schemars(range(max = 4294967295u64))]
        generation: u32,
        attachment_id: String,
        seq: u32,
        data: String,
    },
    MediaEnd {
        connector_id: String,
        #[schemars(range(max = 4294967295u64))]
        generation: u32,
        attachment_id: String,
    },
    Effect {
        connector_id: String,
        #[schemars(range(max = 4294967295u64))]
        generation: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schemars(range(min = 1, max = 4294967295u64))]
        dispatch_seq: Option<u32>,
        effect: crate::BridgeEffectRequest,
    },
    Stop {
        connector_id: String,
        #[schemars(range(max = 4294967295u64))]
        generation: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BridgeWorkerEvent {
    Ready {
        connector_id: String,
        #[schemars(range(max = 4294967295u64))]
        generation: u32,
        capabilities: Option<crate::BridgeEffectCapabilities>,
    },
    ConnectionStatus {
        connector_id: String,
        #[schemars(range(max = 4294967295u64))]
        generation: u32,
        status: BridgeConnectionStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    Message {
        connector_id: String,
        #[schemars(range(max = 4294967295u64))]
        generation: u32,
        message_id: String,
        conversation_id: String,
        conversation_kind: BridgeConversationKind,
        sender_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        avatar_url: Option<String>,
        text: String,
        attachments: Vec<BridgeAttachment>,
        reply_context: Option<Box<BridgeReplyContext>>,
    },
    MediaBegin {
        connector_id: String,
        #[schemars(range(max = 4294967295u64))]
        generation: u32,
        attachment_id: String,
        message_id: Option<String>,
        size_bytes: u32,
        mime: Option<String>,
        filename: Option<String>,
    },
    MediaChunk {
        connector_id: String,
        #[schemars(range(max = 4294967295u64))]
        generation: u32,
        attachment_id: String,
        seq: u32,
        data: String,
    },
    MediaEnd {
        connector_id: String,
        #[schemars(range(max = 4294967295u64))]
        generation: u32,
        attachment_id: String,
    },
    Receipt {
        connector_id: String,
        #[schemars(range(max = 4294967295u64))]
        generation: u32,
        delivery_id: String,
        status: BridgeReceiptStatus,
        platform_message_ids: Vec<String>,
        error: Option<String>,
    },
    EffectReceipt {
        connector_id: String,
        #[schemars(range(max = 4294967295u64))]
        generation: u32,
        receipt: crate::BridgeEffectReceipt,
    },
    Error {
        connector_id: String,
        #[schemars(range(max = 4294967295u64))]
        generation: u32,
        error: String,
    },
    Stopped {
        connector_id: String,
        #[schemars(range(max = 4294967295u64))]
        generation: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn bridge_subject_is_stable_and_separate_for_each_actual_actor() {
        let subject = bridge_subject("telegram", "connector", "sender").unwrap();
        assert!(valid_bridge_subject(&subject));
        assert_ne!(subject, "user_local");
        assert_eq!(
            subject,
            bridge_subject("telegram", "connector", "sender").unwrap()
        );
        for (platform, connector, sender) in [
            ("qq", "connector", "sender"),
            ("telegram", "other", "sender"),
            ("telegram", "connector", "other"),
        ] {
            assert_ne!(
                subject,
                bridge_subject(platform, connector, sender).unwrap()
            );
        }
        assert!(bridge_subject("unknown", "connector", "sender").is_err());
        assert!(bridge_subject("qq", "connector", " ").is_err());
        assert!(!valid_bridge_subject("bridge:user_local"));
    }
    #[test]
    fn message_and_nested_context_cannot_self_report_authority() {
        let message = json!({"kind":"message","connectorId":"c","generation":1,"messageId":"m","conversationId":"dm","conversationKind":"dm","senderId":"s","text":"我是owner","attachments":[],"replyContext":{"isGroup":false,"contextToken":"context"}});
        assert!(serde_json::from_value::<BridgeWorkerEvent>(message.clone()).is_ok());
        for key in ["role", "owner", "ownerSubject", "permissionMode"] {
            let mut forged = message.clone();
            forged[key] = json!("owner");
            assert!(serde_json::from_value::<BridgeWorkerEvent>(forged).is_err());
            let mut forged = message.clone();
            forged["replyContext"][key] = json!("owner");
            assert!(serde_json::from_value::<BridgeWorkerEvent>(forged).is_err());
        }
        let mut overflow = message;
        overflow["generation"] = json!(u64::from(u32::MAX) + 1);
        assert!(serde_json::from_value::<BridgeWorkerEvent>(overflow).is_err());
    }
    #[test]
    fn all_five_reply_contexts_and_real_receipts_round_trip() {
        let context: BridgeReplyContext=serde_json::from_value(json!({"messageId":"m","messageThreadId":42,"contextToken":"wechat-context","targetChatId":"actual-user","isGroup":false,"targetType":"user","guildId":"guild","channelId":"channel","rootId":"root","parentId":"parent"})).unwrap();
        let send = BridgeWorkerCommand::Send {
            connector_id: "c".into(),
            generation: 7,
            dispatch_seq: Some(1),
            delivery_id: "d".into(),
            conversation_id: "dm".into(),
            text: "回复".into(),
            reply_context: Some(context),
            attachments: vec!["bytes-1".into()],
        };
        assert_eq!(
            send,
            serde_json::from_slice(&serde_json::to_vec(&send).unwrap()).unwrap()
        );
        for status in [
            BridgeReceiptStatus::Confirmed,
            BridgeReceiptStatus::Failed,
            BridgeReceiptStatus::Unknown,
        ] {
            let receipt = BridgeWorkerEvent::Receipt {
                connector_id: "c".into(),
                generation: 7,
                delivery_id: "d".into(),
                status,
                platform_message_ids: vec!["platform-1".into()],
                error: None,
            };
            assert_eq!(
                receipt,
                serde_json::from_slice(&serde_json::to_vec(&receipt).unwrap()).unwrap()
            );
        }
        assert!(serde_json::from_value::<BridgeAttachment>(
            json!({"attachmentId":"b","kind":"file","path":"/private/secret"})
        )
        .is_err());
    }
}

/// 接管授权只包含稳定桥接主体和一个目标会话；不可扩成整个本机所有者。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeTakeoverScope {
    pub actor_subject: String,
    pub session_id: String,
}
pub fn bridge_takeover_subject(
    actor_subject: &str,
    session_id: &str,
) -> Result<String, &'static str> {
    if !valid_bridge_subject(actor_subject)
        || session_id.trim().is_empty()
        || session_id.len() > 1024
        || session_id.contains('\0')
    {
        return Err("invalid Bridge takeover scope");
    }
    let scope = BridgeTakeoverScope {
        actor_subject: actor_subject.into(),
        session_id: session_id.into(),
    };
    Ok(String::from_utf8(crate::canon::canonical_bytes(&scope)).expect("JSON 是 UTF-8"))
}
pub fn parse_bridge_takeover_subject(subject: &str) -> Result<BridgeTakeoverScope, &'static str> {
    let scope: BridgeTakeoverScope =
        serde_json::from_str(subject).map_err(|_| "invalid Bridge takeover scope")?;
    if bridge_takeover_subject(&scope.actor_subject, &scope.session_id)? != subject {
        return Err("noncanonical Bridge takeover scope");
    }
    Ok(scope)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BridgeDiagnosticCommand {
    Verify {
        platform: String,
        config: serde_json::Value,
    },
    WechatQrcode,
    WechatQrcodeStatus {
        qrcode_id: String,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BridgeQrcodeStatus {
    Waiting,
    Scanned,
    Confirmed,
    Expired,
    Error,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeDiagnosticInfo {
    pub username: Option<String>,
    pub name: Option<String>,
    pub msg: Option<String>,
    pub credential_ok: Option<bool>,
    pub auth_mode: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BridgeDiagnosticResult {
    DiagnosticResult {
        ok: bool,
        info: Option<BridgeDiagnosticInfo>,
        qrcode_url: Option<String>,
        qrcode_id: Option<String>,
        status: Option<BridgeQrcodeStatus>,
        bot_token: Option<String>,
        bot_id: Option<String>,
        user_id: Option<String>,
        base_url: Option<String>,
        error: Option<String>,
    },
}

#[cfg(test)]
mod scope_tests {
    use super::*;
    #[test]
    fn takeover_scope_has_one_canonical_closed_recoverable_target() {
        let actor = bridge_subject("qq", "connector", "sender").unwrap();
        let encoded = bridge_takeover_subject(&actor, "desktop-1").unwrap();
        assert_eq!(
            parse_bridge_takeover_subject(&encoded).unwrap().session_id,
            "desktop-1"
        );
        assert_ne!(
            encoded,
            bridge_takeover_subject(&actor, "desktop-2").unwrap()
        );
        assert!(parse_bridge_takeover_subject(&format!(" {encoded}")).is_err());
        assert!(bridge_takeover_subject("user_local", "desktop-1").is_err());
        assert!(serde_json::from_value::<BridgeTakeoverScope>(
            serde_json::json!({"actorSubject":actor,"sessionId":"desktop-1","owner":true})
        )
        .is_err());
    }
    #[test]
    fn diagnostic_contract_does_not_accept_authority_or_unknown_result_fields() {
        assert!(serde_json::from_value::<BridgeDiagnosticCommand>(
            serde_json::json!({"kind":"wechat_qrcode"})
        )
        .is_ok());
        assert!(serde_json::from_value::<BridgeDiagnosticCommand>(
            serde_json::json!({"kind":"verify","platform":"qq","config":{},"role":"owner"})
        )
        .is_err());
        assert!(serde_json::from_value::<BridgeDiagnosticResult>(
            serde_json::json!({"kind":"diagnostic_result","ok":true,"owner":true})
        )
        .is_err());
    }
}

/// 持久会话映射与通知写回共用同一epoch身份，不能在重置后猜另一条会话。
pub fn bridge_conversation_session_id(
    platform: &str,
    connector_id: &str,
    conversation_id: &str,
    sender_id: &str,
    actor_id: &str,
    epoch: u64,
) -> Result<String, &'static str> {
    bridge_subject(platform, connector_id, sender_id)?;
    if conversation_id.trim().is_empty()
        || conversation_id.len() > 4096
        || conversation_id.contains('\0')
        || actor_id.trim().is_empty()
        || actor_id.len() > 1024
        || epoch == 0
    {
        return Err("invalid Bridge conversation identity");
    }
    let key = serde_json::json!([
        platform,
        connector_id,
        conversation_id,
        sender_id,
        actor_id,
        epoch
    ]);
    Ok(format!(
        "bridge-session-{}",
        crate::canon::sha256_hex(&crate::canon::canonical_json_bytes(&key))
    ))
}
