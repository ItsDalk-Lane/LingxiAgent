//! 频道和私信的宿主边界；客户端不能指定消息身份或后台运行来源。
use crate::{auth::Principal, phone_settings::PhoneSettings, ServiceState};
use lingxi_adapters::{
    conversation_files::{
        ConversationDocument, ConversationFiles, ConversationMutation, FileReceiptStatus,
    },
    storage::{conversations as store, RunDatabase},
};
use lingxi_kernel::ports::{StorageError, StoragePort};
use lingxi_protocol::RunId;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Debug)]
pub struct ConversationError {
    pub status: u16,
    pub message: String,
}
impl ConversationError {
    pub fn new(status: u16, message: impl ToString) -> Self {
        Self {
            status,
            message: message.to_string(),
        }
    }
}
impl std::fmt::Display for ConversationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl From<StorageError> for ConversationError {
    fn from(e: StorageError) -> Self {
        let status = match &e {
            StorageError::InvalidRequest { detail }
                if detail.contains("conversation file missing:") =>
            {
                404
            }
            StorageError::InvalidRequest { .. } => 400,
            StorageError::Conflict { .. } => 409,
            StorageError::QueueFull | StorageError::QueueClosed | StorageError::Busy { .. } => 503,
            _ => 500,
        };
        Self::new(status, e.to_string())
    }
}
impl From<String> for ConversationError {
    fn from(e: String) -> Self {
        Self::new(400, e)
    }
}
pub(crate) fn digest(value: Value) -> String {
    lingxi_protocol::canon::sha256_hex(value.to_string().as_bytes())
}
pub(crate) fn timestamp(now: u64) -> Result<String, ConversationError> {
    Ok(chrono::DateTime::from_timestamp_millis(
        i64::try_from(now).map_err(|_| ConversationError::new(400, "clock out of range"))?,
    )
    .ok_or_else(|| ConversationError::new(400, "clock out of range"))?
    .with_timezone(&chrono::Local)
    .format("%Y-%m-%d %H:%M:%S")
    .to_string())
}
pub(crate) fn field<'a>(v: &'a Value, k: &str) -> Result<&'a str, ConversationError> {
    v[k].as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ConversationError::new(400, format!("{k} required")))
}
pub(crate) fn dm_scope_digest(actor: &str, peer: &str, meta: &Value) -> String {
    digest(json!([
        actor,
        peer,
        meta.get("visibleAfterTimestamp"),
        meta.get("visibleAfterMessageCount"),
        meta.get("visibleAfterPrefixSha256")
    ]))
}
pub(crate) fn members(doc: &ConversationDocument) -> Result<Vec<String>, ConversationError> {
    doc.meta["members"]
        .as_array()
        .ok_or_else(|| ConversationError::new(409, "channel members missing"))?
        .iter()
        .map(|v| {
            v.as_str()
                .map(String::from)
                .ok_or_else(|| ConversationError::new(409, "invalid member"))
        })
        .collect()
}

#[derive(Clone)]
pub struct ConversationApi {
    pub(crate) storage: Arc<RunDatabase>,
    pub(crate) memory: Arc<crate::memory::MemoryService>,
    pub(crate) events: Arc<crate::appevents::AppEventLog>,
    pub(crate) clock: Arc<dyn crate::ServiceClock>,
    pub(crate) runs: std::sync::Weak<crate::runs::RunSupervisor>,
    pub(crate) host: std::sync::Weak<crate::conversation_runtime::ConversationHost>,
    pub(crate) ids: Arc<dyn crate::RequestIdGen>,
    pub(crate) files: ConversationFiles,
    pub(crate) models: Option<Arc<lingxi_adapters::models::gateway::ConfigModelGateway>>,
}
impl ConversationApi {
    pub fn from_state(s: &ServiceState) -> Self {
        Self {
            storage: s.storage().clone(),
            memory: s.memory().clone(),
            events: s.app_events().clone(),
            clock: s.clock().clone(),
            runs: Arc::downgrade(s.runs()),
            host: Arc::downgrade(s.conversations()),
            ids: s.request_ids().clone(),
            files: s.conversation_files().clone(),
            models: s.wired_model_gateway().cloned(),
        }
    }
    pub(crate) fn wake(&self) {
        if let Some(h) = self.host.upgrade() {
            h.wake();
        }
    }
    pub(crate) fn enabled(&self) -> Result<bool, ConversationError> {
        let p = self.memory.read_preferences_root()?;
        match p.get("channels_enabled") {
            None => Ok(false),
            Some(Value::Bool(v)) => Ok(*v),
            _ => Err(ConversationError::new(
                409,
                "invalid channels_enabled preference",
            )),
        }
    }
    pub(crate) fn actor(&self, actor: Option<&str>) -> Result<String, ConversationError> {
        let id = match actor {
            Some(id) => id.to_string(),
            None => self
                .memory
                .resolve_primary_agent_checked()
                .map_err(|e| ConversationError::new(409, e.to_string()))?
                .ok_or_else(|| ConversationError::new(409, "no primary agent configured"))?,
        };
        self.memory
            .agent_dir_checked(&id)
            .map_err(|e| ConversationError::new(404, e.message))?;
        Ok(id)
    }
    pub(crate) fn phone_document(
        &self,
        actor: &str,
        id: &str,
    ) -> Result<ConversationDocument, ConversationError> {
        match self.files.read_phone(actor, id) {
            Ok(d) => Ok(d),
            Err(StorageError::InvalidRequest { detail })
                if detail.contains("conversation file missing:") =>
            {
                Ok(ConversationDocument {
                    meta: json!({"agentId":actor,"conversationId":id}),
                    messages: vec![],
                    activities: vec![],
                    content_sha256: digest(json!([])),
                })
            }
            Err(e) => Err(e.into()),
        }
    }
    pub(crate) fn settings(
        &self,
        actor: &str,
        id: &str,
    ) -> Result<PhoneSettings, ConversationError> {
        if id.starts_with("dm:") {
            let d = self.phone_document(actor, id)?;
            Ok(PhoneSettings::read_dm(&d.meta)?)
        } else {
            let d = self.files.read_channel(id)?;
            Ok(PhoneSettings::read(&d.meta, members(&d)?.len())?)
        }
    }
    pub(crate) fn require_member(
        &self,
        id: &str,
        actor: &str,
    ) -> Result<ConversationDocument, ConversationError> {
        self.actor(Some(actor))?;
        let d = self.files.read_channel(id)?;
        if !members(&d)?.iter().any(|m| m == actor) {
            return Err(ConversationError::new(403, "actor is not a channel member"));
        }
        Ok(d)
    }
    pub(crate) async fn frozen_timestamp(
        &self,
        id: &str,
        fallback: &str,
    ) -> Result<String, ConversationError> {
        let prior = store::operation_mutation(&self.storage, id).await?;
        Ok(prior
            .and_then(|m| serde_json::to_value(m).ok())
            .and_then(|v| v["timestamp"].as_str().map(String::from))
            .unwrap_or_else(|| fallback.into()))
    }
    pub(crate) async fn apply(
        &self,
        id: &str,
        mutation: &ConversationMutation,
    ) -> Result<(), ConversationError> {
        let frozen = store::operation_mutation(&self.storage, id).await?;
        let mutation = if let Some(old) = frozen.as_ref() {
            let mut a = serde_json::to_value(old).expect("文件动作");
            let mut b = serde_json::to_value(mutation).expect("文件动作");
            a.as_object_mut().expect("文件动作对象").remove("timestamp");
            b.as_object_mut().expect("文件动作对象").remove("timestamp");
            if a != b {
                return Err(ConversationError::new(
                    409,
                    "requestId is bound to different conversation input",
                ));
            }
            old
        } else {
            mutation
        };
        let r = self
            .files
            .apply(&self.storage, id, mutation, self.clock.now_unix_ms())
            .await?;
        if r.status != FileReceiptStatus::Applied {
            return Err(ConversationError::new(
                409,
                format!(
                    "conversation write {:?}; inspect operation {}",
                    r.status, r.operation_id
                ),
            ));
        }
        self.wake();
        Ok(())
    }

    fn operation(
        &self,
        principal: &Principal,
        action: &str,
        payload: &Value,
    ) -> Result<String, ConversationError> {
        let key = match payload.get("requestId") {
            Some(Value::String(v)) if !v.trim().is_empty() && v.len() <= 256 => v.clone(),
            None => self.ids.next_request_id(),
            _ => return Err(ConversationError::new(400, "invalid requestId")),
        };
        Ok(format!(
            "conversation-{}",
            digest(json!([principal.user_id, action, key]))
        ))
    }
    pub(crate) async fn cancel_scope(
        &self,
        conversation: Option<&str>,
        actor: Option<&str>,
    ) -> Result<(), ConversationError> {
        let all = store::list_active_deliveries(&self.storage, None, actor).await?;
        let mut chains = std::collections::BTreeSet::new();
        for d in all {
            if conversation.is_none_or(|id| d.input.conversation_id == id) {
                if store::get_delivery_run(&self.storage, &d.input.delivery_id)
                    .await?
                    .is_none()
                {
                    store::settle_delivery(
                        &self.storage,
                        &d.input.delivery_id,
                        store::PhoneDeliveryStatus::Failed,
                        Some("authority changed before admission".into()),
                        self.clock.now_unix_ms(),
                    )
                    .await?;
                }
                chains.insert(d.input.root_chain_id);
            }
        }
        let mut pending = Vec::new();
        for chain in chains {
            for (run, _, _) in store::active_runs_for_chain(&self.storage, &chain).await? {
                self.runs
                    .upgrade()
                    .ok_or_else(|| ConversationError::new(503, "run supervisor unavailable"))?
                    .cancel_run(&run, "conversation authority changed");
                pending.push(run);
            }
        }
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let mut next = Vec::new();
            for run in pending {
                if self
                    .storage
                    .load_run(&RunId::new(&run))
                    .await?
                    .is_some_and(|r| !r.status.is_terminal())
                {
                    next.push(run);
                }
            }
            if next.is_empty() {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(ConversationError::new(
                    409,
                    "conversation cancellation cleanup unconfirmed",
                ));
            }
            pending = next;
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn request(
        &self,
        principal: &Principal,
        action: &str,
        id: Option<&str>,
        peer: Option<&str>,
        agent: Option<&str>,
        payload: Value,
    ) -> Result<Value, ConversationError> {
        if !principal.is_local_owner()
            && !(principal.kind == crate::auth::PrincipalKind::Device
                && principal.user_id.as_deref() == Some(lingxi_kernel::LOCAL_OWNER_SUBJECT))
        {
            return Err(ConversationError::new(
                403,
                "owner conversation access required",
            ));
        }
        if action == "channel.toggle" {
            let enabled = payload["enabled"]
                .as_bool()
                .ok_or_else(|| ConversationError::new(400, "enabled must be boolean"))?;
            let mut prefs = self.memory.read_preferences_root()?;
            prefs["channels_enabled"] = json!(enabled);
            self.memory.write_preferences_root(&prefs)?;
            if !enabled {
                self.cancel_scope(None, None).await?;
            }
            self.wake();
            return Ok(json!({"ok":true,"enabled":enabled}));
        }
        if action != "conversation.export" && !self.enabled()? {
            return Err(ConversationError::new(503, "Agent phone is disabled"));
        }
        let actor = self.actor(agent)?;
        let now = self.clock.now_unix_ms();
        let stamp = timestamp(now)?;
        if action == "channel.list" {
            let bookmarks = self.files.read_user_bookmarks()?;
            let mut channels = Vec::new();
            for d in self.files.list_channels()? {
                let cid = field(&d.meta, "id")?;
                let last = d.messages.last();
                let read = bookmarks.get(cid);
                let unread = d
                    .messages
                    .iter()
                    .filter(|m| read.is_none_or(|t| m.timestamp > *t))
                    .count();
                channels.push(json!({"id":cid,"name":d.meta.get("name").cloned().unwrap_or_else(||json!(field(&d.meta,"id").unwrap_or(""))),"description":d.meta.get("description").cloned().unwrap_or(json!("")),"members":members(&d)?,"messageCount":d.messages.len(),"newMessageCount":unread,"lastMessage":last.map(|m|&m.body),"lastSender":last.map(|m|&m.sender),"lastTimestamp":last.map(|m|&m.timestamp)}));
            }
            return Ok(json!({"channels":channels,"bookmarks":bookmarks}));
        }
        if action == "channel.create" {
            let name = field(&payload, "name")?;
            if name.len() > 256 {
                return Err(ConversationError::new(400, "channel name too long"));
            }
            let mut roster = payload["members"]
                .as_array()
                .ok_or_else(|| ConversationError::new(400, "members required"))?
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(String::from)
                        .ok_or_else(|| ConversationError::new(400, "invalid member"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            roster.sort();
            roster.dedup();
            if roster.len() < 2 {
                return Err(ConversationError::new(
                    400,
                    "a channel requires at least two members",
                ));
            }
            for m in &roster {
                self.actor(Some(m))?;
            }
            let op = self.operation(principal, action, &payload)?;
            let cid = format!("ch_{}", &digest(json!(op))[..24]);
            self.apply(&op,&ConversationMutation::CreateChannel{id:cid.clone(),meta:json!({"id":cid,"name":name,"description":payload["description"].as_str().unwrap_or(""),"members":roster}),intro:payload["intro"].as_str().map(String::from),timestamp:stamp}).await?;
            self.events.publish("channel_created", json!({}));
            return Ok(json!({"ok":true,"id":cid,"name":name,"members":roster}));
        }
        if action == "dm.list" {
            let mut dms = Vec::new();
            for a in self
                .memory
                .list_agent_summaries()
                .map_err(|e| ConversationError::new(409, e.message))?
            {
                let aid = field(&a, "id")?;
                if aid == actor {
                    continue;
                }
                let messages = match self.files.read_dm(&actor, aid) {
                    Ok(d) => self.visible_dm(&actor, aid, d)?.messages,
                    Err(StorageError::InvalidRequest { detail }) if detail.contains("missing") => {
                        Vec::new()
                    }
                    Err(e) => return Err(e.into()),
                };
                let last = messages.last();
                dms.push(json!({"ownerAgentId":actor,"peerId":aid,"peerName":a["name"],"lastMessage":last.map(|m|&m.body).unwrap_or(&String::new()),"lastSender":last.map(|m|&m.sender).unwrap_or(&String::new()),"lastTimestamp":last.map(|m|&m.timestamp).unwrap_or(&String::new()),"messageCount":messages.len()}));
            }
            dms.sort_by(|a, b| {
                b["lastTimestamp"]
                    .as_str()
                    .cmp(&a["lastTimestamp"].as_str())
                    .then_with(|| a["peerName"].as_str().cmp(&b["peerName"].as_str()))
            });
            return Ok(json!({"ownerAgentId":actor,"dms":dms}));
        }
        let conversation = match (id, peer) {
            (Some(id), _) => id.to_string(),
            (_, Some(peer)) => format!("dm:{peer}"),
            _ => return Err(ConversationError::new(400, "conversation required")),
        };
        if let Some(peer) = conversation.strip_prefix("dm:") {
            self.actor(Some(peer))?;
            if peer == actor {
                return Err(ConversationError::new(400, "self DM is not allowed"));
            }
            if action == "dm.read" {
                let d = self.visible_dm(&actor, peer, self.files.read_dm(&actor, peer)?)?;
                return Ok(
                    json!({"ownerAgentId":actor,"peerId":peer,"peerName":self.agent_name(peer)?,"messages":d.messages}),
                );
            }
            if action == "dm.reset" {
                let host = self
                    .host
                    .upgrade()
                    .ok_or_else(|| ConversationError::new(503, "conversation host unavailable"))?;
                let _reset = host.reset_lock.clone().lock_owned().await;
                let op = self.operation(principal, action, &payload)?;
                if let Some(frozen) = store::operation_mutation(&self.storage, &op).await? {
                    let ConversationMutation::PhoneReset {
                        actor: original_actor,
                        conversation: original_id,
                        visible_after_timestamp,
                        ..
                    } = &frozen
                    else {
                        return Err(ConversationError::new(
                            409,
                            "requestId belongs to another operation",
                        ));
                    };
                    if original_actor != &actor || original_id != &conversation {
                        return Err(ConversationError::new(
                            409,
                            "requestId belongs to another DM",
                        ));
                    }
                    self.apply(&op, &frozen).await?;
                    return Ok(
                        json!({"ok":true,"ownerAgentId":actor,"peerId":peer,"visibleAfterTimestamp":visible_after_timestamp}),
                    );
                }
                let messages = match self.files.read_dm(&actor, peer) {
                    Ok(d) => d.messages,
                    Err(StorageError::InvalidRequest { detail })
                        if detail.contains("conversation file missing:") =>
                    {
                        Vec::new()
                    }
                    Err(e) => return Err(e.into()),
                };
                self.cancel_scope(Some(&conversation), Some(&actor)).await?;
                self.apply(
                    &op,
                    &ConversationMutation::PhoneReset {
                        actor: actor.clone(),
                        conversation: conversation.clone(),
                        conversation_type: "dm".into(),
                        visible_after_timestamp: stamp.clone(),
                        visible_after_message_count: Some(messages.len() as u64),
                        visible_after_prefix_sha256: Some(digest(json!(messages))),
                        reset_by: "user".into(),
                        timestamp: stamp.clone(),
                    },
                )
                .await?;
                return Ok(
                    json!({"ok":true,"ownerAgentId":actor,"peerId":peer,"visibleAfterTimestamp":stamp}),
                );
            }
        }

        if action == "conversation.export" {
            let d = if let Some(peer) = conversation.strip_prefix("dm:") {
                self.files.read_dm(&actor, peer)?
            } else {
                self.files.read_channel(&conversation)?
            };
            let mut content = format!("# {}\n\n", d.meta["name"].as_str().unwrap_or(&conversation));
            for m in d.messages {
                content.push_str(&format!(
                    "### {} | {}\n\n{}\n\n",
                    m.sender, m.timestamp, m.body
                ));
            }
            return Ok(json!({"content":content,"filename":format!("{conversation}.md")}));
        }
        if action.starts_with("phone.") {
            let settings = self.settings(&actor, &conversation)?;
            match action {
                "phone.settings.get" => {
                    return Ok(serde_json::to_value(settings).expect("手机配置"))
                }
                "phone.mode.get" => return Ok(json!({"mode":settings.mode})),
                "phone.settings.set" | "phone.mode.set" => {
                    let next = settings.update(&payload)?;
                    if next.model_override_enabled {
                        let m = next.model_override_model.as_ref().expect("模型已验证");
                        self.validate_model(m)?;
                    }
                    let op = self.operation(principal, action, &json!({}))?;
                    let mutation = if conversation.starts_with("dm:") {
                        ConversationMutation::PhonePatch {
                            actor: actor.clone(),
                            conversation: conversation.clone(),
                            conversation_type: "dm".into(),
                            patch: next.patch_dm(),
                            timestamp: stamp,
                        }
                    } else {
                        ConversationMutation::PatchChannel {
                            id: conversation.clone(),
                            patch: next.patch(),
                        }
                    };
                    self.apply(&op, &mutation).await?;
                    self.cancel_scope(Some(&conversation), None).await?;
                    let mut result = serde_json::to_value(next).expect("手机配置");
                    result["ok"] = json!(true);
                    return Ok(result);
                }
                "phone.activities" => {
                    let scopes = if let Some(peer) = conversation.strip_prefix("dm:") {
                        vec![
                            (actor.clone(), conversation.clone()),
                            (peer.into(), format!("dm:{actor}")),
                        ]
                    } else {
                        members(&self.files.read_channel(&conversation)?)?
                            .into_iter()
                            .map(|a| (a, conversation.clone()))
                            .collect::<Vec<_>>()
                    };
                    let kind = if conversation.starts_with("dm:") {
                        "dm"
                    } else {
                        "channel"
                    };
                    let mut activities = Vec::new();
                    for (agent_id, scope) in scopes {
                        let d = self.phone_document(&agent_id, &scope)?;
                        for a in d.activities {
                            activities.push(json!({"agentId":agent_id,"conversationId":conversation,"conversationType":kind,"timestamp":a.timestamp,"state":a.state,"summary":a.summary,"details":a.details}));
                        }
                    }
                    activities
                        .sort_by(|a, b| a["timestamp"].as_str().cmp(&b["timestamp"].as_str()));
                    return Ok(
                        json!({"activities":activities,"ticker":crate::conversation_status::ticker(self,&conversation,&actor).await?}),
                    );
                }
                _ => return Err(ConversationError::new(404, "unknown phone action")),
            }
        }
        let d = self.files.read_channel(&conversation)?;
        let mut roster = members(&d)?;
        match action {
            "channel.read" => Ok(
                json!({"id":conversation,"name":d.meta.get("name").cloned().unwrap_or_else(||json!(field(&d.meta,"id").unwrap_or(""))),"description":d.meta.get("description").cloned().unwrap_or(json!("")),"messages":d.messages,"members":roster}),
            ),
            "channel.add_member" | "channel.remove_member" => {
                let member = field(&payload, "memberId")?;
                if action == "channel.add_member" {
                    self.actor(Some(member))?;
                    if !roster.iter().any(|m| m == member) {
                        roster.push(member.into());
                    }
                } else {
                    roster.retain(|m| m != member);
                    if roster.len() < 2 {
                        return Err(ConversationError::new(
                            400,
                            "at least two members must remain",
                        ));
                    }
                }
                let op = self.operation(principal, action, &json!({}))?;
                self.apply(
                    &op,
                    &ConversationMutation::PatchChannel {
                        id: conversation.clone(),
                        patch: json!({"members":roster}),
                    },
                )
                .await?;
                self.cancel_scope(Some(&conversation), None).await?;
                Ok(json!({"ok":true,"members":roster}))
            }
            "channel.post" => {
                let body = field(&payload, "body")?;
                let op = self.operation(principal, action, &payload)?;
                let stamp = self.frozen_timestamp(&op, &stamp).await?;
                let sender = match store::operation_mutation(&self.storage, &op).await? {
                    Some(ConversationMutation::AppendChannel { sender, .. }) => sender,
                    _ => {
                        let preferences = self.memory.read_preferences_root()?;
                        let scan = crate::context_compiler::scan_preferences(Some(&preferences));
                        if let Some(name) = scan.user_name {
                            name
                        } else {
                            let primary = self.actor(None)?;
                            let dir = self
                                .memory
                                .agent_dir_checked(&primary)
                                .map_err(|e| ConversationError::new(409, e.message))?;
                            let config =
                                lingxi_adapters::storage::agent_memory::config::read_agent_config(
                                    &dir.join("config.yaml"),
                                )
                                .and_then(|c| c.scan())
                                .map_err(|e| ConversationError::new(409, e))?;
                            let locale =
                                config.locale.or(scan.locale).unwrap_or_else(|| "en".into());
                            if locale.to_lowercase().starts_with("zh") {
                                "用户".into()
                            } else {
                                "User".into()
                            }
                        }
                    }
                };
                self.apply(
                    &op,
                    &ConversationMutation::AppendChannel {
                        id: conversation.clone(),
                        actor: None,
                        sender: sender.clone(),
                        body: body.into(),
                        timestamp: stamp.clone(),
                    },
                )
                .await?;
                self.events.publish("channel_new_message",json!({"channelName":conversation,"message":{"sender":sender,"timestamp":stamp,"body":body}}));
                Ok(json!({"ok":true,"timestamp":stamp}))
            }
            "channel.read_mark" => {
                let at = field(&payload, "timestamp")?;
                let op = self.operation(principal, action, &json!({}))?;
                self.apply(
                    &op,
                    &ConversationMutation::BookmarkUser {
                        channel: conversation,
                        timestamp: Some(at.into()),
                    },
                )
                .await?;
                Ok(json!({"ok":true}))
            }
            "channel.delete" => {
                let op = self.operation(principal, action, &json!({}))?;
                self.apply(
                    &op,
                    &ConversationMutation::DeleteChannel {
                        id: conversation.clone(),
                    },
                )
                .await?;
                self.cancel_scope(Some(&conversation), None).await?;
                self.events.publish("channel_created", json!({}));
                Ok(json!({"ok":true}))
            }
            _ => Err(ConversationError::new(404, "unknown conversation action")),
        }
    }
    pub(crate) fn agent_name(&self, id: &str) -> Result<String, ConversationError> {
        let roster = self
            .memory
            .list_agent_summaries()
            .map_err(|e| ConversationError::new(409, e.message))?;
        Ok(roster
            .iter()
            .find(|a| a["id"] == id)
            .and_then(|a| a["name"].as_str())
            .unwrap_or(id)
            .into())
    }
    pub(crate) fn visible_dm(
        &self,
        actor: &str,
        peer: &str,
        mut d: ConversationDocument,
    ) -> Result<ConversationDocument, ConversationError> {
        let p = self.phone_document(actor, &format!("dm:{peer}"))?;
        let count = match p.meta.get("visibleAfterMessageCount") {
            None | Some(Value::Null) => None,
            Some(value) => Some(
                value
                    .as_u64()
                    .or_else(|| value.as_str().and_then(|text| text.parse::<u64>().ok()))
                    .ok_or_else(|| ConversationError::new(409, "DM reset count is damaged"))?,
            ),
        };
        match (count, p.meta["visibleAfterPrefixSha256"].as_str()) {
            (Some(count), Some(sha)) => {
                let count = usize::try_from(count)
                    .map_err(|_| ConversationError::new(409, "DM reset boundary is too large"))?;
                if count > d.messages.len() || digest(json!(d.messages[..count])) != sha {
                    return Err(ConversationError::new(
                        409,
                        "DM history prefix differs from the confirmed reset boundary",
                    ));
                }
                d.messages.drain(..count);
            }
            (None, None) => {
                if let Some(bound) = p.meta["visibleAfterTimestamp"].as_str() {
                    d.messages.retain(|m| m.timestamp.as_str() > bound);
                }
            }
            _ => return Err(ConversationError::new(409, "DM reset boundary is damaged")),
        }
        Ok(d)
    }
    pub(crate) async fn last_phone_completion_ms(
        &self,
        channel: &ConversationDocument,
    ) -> Result<Option<u64>, ConversationError> {
        let id = field(&channel.meta, "id")?;
        let actors = members(channel)?;
        let actual = lingxi_adapters::storage::conversation_status::last_completion(
            &self.storage,
            id,
            actors.clone(),
        )
        .await?;
        if actual.is_some() {
            return Ok(actual);
        }
        // 没有正式任务记录的旧资料才使用活动时间；延迟展示不能改写真实完成时刻。
        let mut latest = None;
        for actor in actors {
            let phone = self.phone_document(&actor, id)?;
            for activity in phone.activities {
                if !matches!(activity.state.as_str(), "replied" | "passed" | "error") {
                    continue;
                }
                let parsed = chrono::DateTime::parse_from_rfc3339(&activity.timestamp)
                    .map_err(|e| ConversationError::new(409, e))?
                    .timestamp_millis();
                let value = u64::try_from(parsed).map_err(|_| {
                    ConversationError::new(
                        409,
                        "phone completion timestamp precedes the supported clock",
                    )
                })?;
                latest = Some(latest.unwrap_or(0).max(value));
            }
        }
        Ok(latest)
    }
    pub(crate) fn validate_model(&self, model: &Value) -> Result<(), ConversationError> {
        let provider = field(model, "provider")?;
        let id = field(model, "id")?;
        let gateway = self
            .models
            .as_ref()
            .ok_or_else(|| ConversationError::new(503, "model plane unavailable"))?;
        let dispatch = gateway
            .resolve_dispatch(&lingxi_kernel::model_exchange::ModelRouteRequest {
                operation: lingxi_kernel::model_exchange::ModelOperation::Chat,
                provider: Some(provider.into()),
                model: Some(id.into()),
            })
            .map_err(|e| ConversationError::new(400, e))?;
        if dispatch.capabilities.tools != Some(true) {
            return Err(ConversationError::new(
                400,
                "model has no declared chat tool support",
            ));
        }
        Ok(())
    }
}
