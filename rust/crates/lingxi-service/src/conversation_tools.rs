//! 助手的频道与私信工具：发送身份只读真实 Run，会话成员与确认范围在写入前重检。
use crate::conversations::{digest, field, members, timestamp, ConversationApi};
use lingxi_adapters::{
    conversation_files::{
        operation_mutation, ConversationDocument, ConversationMutation, FileReceiptStatus,
    },
    storage::{conversations as store, session_tree},
};
use lingxi_kernel::{
    invocation::ToolRecoveryCapability,
    ports::{
        StoragePort, ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, ToolSuccess,
    },
    subagent::{RunOrigin, SessionPermissionMode},
    toolcatalog::{
        Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
        ToolManifest, ToolOrigin,
    },
    RunContext,
};
use lingxi_protocol::{ErrorCode, ProtocolError, ToolCallId, ToolSchemaDocument};
use serde_json::{json, Value};
use std::sync::Arc;

const MAX_MESSAGE_BYTES: usize = 64 * 1024;
#[derive(Clone, Copy)]
enum Kind {
    Channel,
    Dm,
}
struct Executor {
    api: ConversationApi,
    kind: Kind,
}
enum Failure {
    Refused(String),
    Unknown(String),
}
impl From<String> for Failure {
    fn from(error: String) -> Self {
        Self::Refused(error)
    }
}
impl From<crate::conversations::ConversationError> for Failure {
    fn from(error: crate::conversations::ConversationError) -> Self {
        Self::Refused(error.message)
    }
}
impl From<lingxi_kernel::ports::StorageError> for Failure {
    fn from(error: lingxi_kernel::ports::StorageError) -> Self {
        Self::Refused(error.to_string())
    }
}
fn text<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    let value = field(args, key).map_err(|error| error.message)?;
    if value.len() > MAX_MESSAGE_BYTES {
        return Err(format!("{key} exceeds message budget"));
    }
    Ok(value)
}
fn resolve_channel(
    api: &ConversationApi,
    actor: &str,
    requested: &str,
) -> Result<ConversationDocument, String> {
    let all = api
        .files
        .list_channels()
        .map_err(|error| error.to_string())?;
    if let Some(document) = all.iter().find(|document| document.meta["id"] == requested) {
        let id = field(&document.meta, "id").map_err(|error| error.message)?;
        return api.require_member(id, actor).map_err(|error| error.message);
    }
    let matches = all
        .iter()
        .filter(|document| document.meta["name"] == requested)
        .filter(|document| {
            members(document).is_ok_and(|roster| roster.iter().any(|id| id == actor))
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [document] => api
            .require_member(
                field(&document.meta, "id").map_err(|error| error.message)?,
                actor,
            )
            .map_err(|error| error.message),
        [] => Err("channel not found or actor is not a member".into()),
        _ => Err(format!(
            "ambiguous channel name; use one of these IDs: {}",
            matches
                .iter()
                .map(|document| document.meta["id"].as_str().unwrap_or("invalid"))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}
fn resolve_agent(api: &ConversationApi, requested: &str) -> Result<String, String> {
    let roster = api
        .memory
        .list_agent_summaries()
        .map_err(|error| error.message)?;
    if roster.iter().any(|agent| agent["id"] == requested) {
        return api.actor(Some(requested)).map_err(|error| error.message);
    }
    let matches = roster
        .iter()
        .filter(|agent| agent["name"] == requested)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [agent] => api
            .actor(Some(field(agent, "id").map_err(|error| error.message)?))
            .map_err(|error| error.message),
        _ => Err(format!(
            "target agent is missing or ambiguous; use an ID: {}",
            roster
                .iter()
                .filter_map(|agent| agent["id"].as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}
fn create_roster(api: &ConversationApi, actor: &str, args: &Value) -> Result<Vec<String>, String> {
    let mut roster = args["members"]
        .as_array()
        .ok_or("members required")?
        .iter()
        .map(|value| {
            value
                .as_str()
                .ok_or("member must be an agent ID")
                .and_then(|id| {
                    api.actor(Some(id))
                        .map_err(|_| "member agent is unavailable")
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    roster.push(actor.to_owned());
    roster.sort();
    roster.dedup();
    if roster.len() < 2 {
        return Err("a channel requires at least two agents".into());
    }
    Ok(roster)
}
fn review(
    api: &ConversationApi,
    kind: Kind,
    actor: &str,
    session: &str,
    args: &Value,
) -> Result<Value, String> {
    if !api.enabled().map_err(|error| error.message)? {
        return Err("Agent phone is disabled".into());
    }
    api.actor(Some(actor)).map_err(|error| error.message)?;
    let base = json!({"actorId":actor,"sourceSessionId":session});
    let mut scope = base;
    match kind {
        Kind::Channel => match text(args, "action")? {
            "list" => {
                scope["action"] = json!("list");
                scope["summary"] = json!("查看当前助手加入的频道目录");
            }
            "read" | "post" => {
                let action = text(args, "action")?;
                let document = resolve_channel(api, actor, text(args, "channel")?)?;
                let id = field(&document.meta, "id").map_err(|error| error.message)?;
                let name = document.meta["name"].as_str().unwrap_or(id);
                let mut roster = members(&document).map_err(|error| error.message)?;
                roster.sort();
                roster.dedup();
                scope["action"] = json!(action);
                scope["channelId"] = json!(id);
                scope["members"] = json!(roster);
                scope["summary"] = json!(if action == "post" {
                    format!(
                        "以助手 {actor} 向频道「{name}」({id}) 发送：\n{}",
                        text(args, "content")?
                    )
                } else {
                    format!("读取频道「{name}」({id})")
                });
            }
            "create" => {
                let name = text(args, "name")?;
                if name != name.trim() || name.len() > 256 {
                    return Err("invalid channel name".into());
                }
                let roster = create_roster(api, actor, args)?;
                scope["action"] = json!("create");
                scope["members"] = json!(roster);
                scope["draftId"] = json!(digest(json!([name, roster, args.get("intro")])));
                scope["summary"] = json!(format!(
                    "创建频道「{name}」，成员：{}{}",
                    roster.join(", "),
                    args["intro"]
                        .as_str()
                        .map(|intro| format!("\n介绍：{intro}"))
                        .unwrap_or_default()
                ));
            }
            _ => return Err("unknown channel action".into()),
        },
        Kind::Dm => {
            let target = resolve_agent(api, text(args, "to")?)?;
            if target == actor {
                return Err("self DM is not allowed".into());
            }
            scope["action"] = json!("send");
            scope["targetAgentId"] = json!(target);
            scope["summary"] = json!(format!(
                "以助手 {actor} 向助手 {} ({target}) 发送私信：\n{}",
                api.agent_name(&target).map_err(|error| error.message)?,
                text(args, "message")?
            ));
        }
    }
    Ok(scope)
}
impl Executor {
    async fn actor(&self, ctx: &RunContext) -> Result<String, Failure> {
        let active = self.api.storage.query_one_text(
            "SELECT r.run_id FROM runs r JOIN run_attempts a ON a.run_id=r.run_id JOIN sessions s ON s.session_id=r.session_id WHERE r.run_id=?1 AND r.session_id=?2 AND r.owner_kind=?3 AND r.owner_subject=?4 AND r.generation=?5 AND a.attempt=?6 AND a.generation=r.generation AND r.status IN ('running','waiting_approval') AND s.lifecycle='active' AND a.attempt=(SELECT attempt FROM run_attempts WHERE run_id=r.run_id ORDER BY started_at_unix_ms DESC,rowid DESC LIMIT 1)",
            vec![ctx.run_id.to_string(),ctx.session_id.to_string(),ctx.principal.storage_kind().into(),ctx.principal.storage_subject(),ctx.generation.to_string(),ctx.attempt.to_string()],
        ).await?;
        if active.is_none() {
            return Err(
                "active owned run with current attempt and generation required"
                    .to_string()
                    .into(),
            );
        }
        let session = session_tree::get_session_row(&self.api.storage, ctx.session_id.as_str())
            .await?
            .ok_or_else(|| Failure::Refused("source session unavailable".into()))?;
        if !ctx
            .principal
            .can_access_session(&session.owner_user_id, ctx.session_id.as_str())
        {
            return Err("source session ownership changed".to_string().into());
        }
        if ctx.principal.bridge_actor_subject().is_some() {
            let lineage = self
                .api
                .storage
                .load_run_lineage(&ctx.run_id)
                .await?
                .ok_or_else(|| Failure::Refused("Bridge source lineage missing".into()))?;
            if lineage.origin != RunOrigin::Bridge || lineage.parent_run_id.is_some() {
                return Err(
                    "only a current owner Bridge root can use conversation tools"
                        .to_string()
                        .into(),
                );
            }
            let mut authorization = crate::runs::DriveAuthorization::user_submission(
                None,
                crate::bridge_permission::read_current(Some(self.api.memory.home()))?
                    .session_mode(),
            );
            authorization.lineage = lineage;
            authorization.grant = crate::runs::RunGrant::Bridge {
                role: lingxi_protocol::BridgeRole::Owner,
            };
            crate::bridge_run_authority::validate(
                Some(&self.api.storage),
                &authorization,
                ctx,
                &session.agent_id,
                Some(self.api.memory.home()),
            )
            .await?;
        } else {
            if ctx.principal.storage_subject() != lingxi_kernel::LOCAL_OWNER_SUBJECT
                || session.owner_user_id != lingxi_kernel::LOCAL_OWNER_SUBJECT
            {
                return Err("local studio owner session required".to_string().into());
            }
            if self
                .api
                .source_delivery(Some(ctx.run_id.as_str()))
                .await?
                .is_some()
            {
                return Err("phone runs cannot restart channel or DM propagation"
                    .to_string()
                    .into());
            }
        }
        self.api
            .actor(Some(&session.agent_id))
            .map_err(Failure::from)
    }
    async fn stamp(&self, operation: &str) -> Result<String, Failure> {
        let frozen = operation_mutation(&self.api.storage, operation).await?;
        match frozen {
            Some(
                ConversationMutation::CreateChannel { timestamp, .. }
                | ConversationMutation::AppendChannel { timestamp, .. }
                | ConversationMutation::AppendDm { timestamp, .. },
            ) => Ok(timestamp),
            Some(_) => Err("tool operation is bound to a different mutation"
                .to_string()
                .into()),
            None => timestamp(self.api.clock.now_unix_ms()).map_err(Failure::from),
        }
    }
    async fn apply(&self, operation: &str, mutation: &ConversationMutation) -> Result<(), Failure> {
        let receipt = self
            .api
            .files
            .apply(
                &self.api.storage,
                operation,
                mutation,
                self.api.clock.now_unix_ms(),
            )
            .await?;
        match receipt.status {
            FileReceiptStatus::Applied => Ok(()),
            FileReceiptStatus::Failed => {
                Err(format!("conversation write failed; operation {operation}").into())
            }
            FileReceiptStatus::Prepared
            | FileReceiptStatus::Partial
            | FileReceiptStatus::Unknown => Err(Failure::Unknown(format!(
                "conversation write {:?}; do not resend; operation {operation}",
                receipt.status
            ))),
        }
    }
    async fn invoke(
        &self,
        ctx: &RunContext,
        call: &ToolCallId,
        args: &Value,
    ) -> Result<Value, Failure> {
        let actor = self.actor(ctx).await?;
        if !self.api.enabled()? {
            return Err("Agent phone is disabled".to_string().into());
        }
        let scope = review(&self.api, self.kind, &actor, ctx.session_id.as_str(), args)?;
        let action = match self.kind {
            Kind::Channel => text(args, "action")?,
            Kind::Dm => "send",
        };
        if matches!(self.kind, Kind::Channel) && action == "list" {
            let bookmarks = self.api.files.read_bookmarks(&actor)?;
            let mut channels = Vec::new();
            for document in self.api.files.list_channels()? {
                let roster = members(&document)?;
                if !roster.iter().any(|id| id == &actor) {
                    continue;
                }
                let id = field(&document.meta, "id")?;
                let actual = self.api.require_member(id, &actor)?;
                if actual.content_sha256 != document.content_sha256 {
                    return Err("channel identity differs from file".to_string().into());
                }
                channels.push(json!({"id":id,"name":document.meta["name"],"description":document.meta.get("description"),
                    "members":roster,"lastRead":bookmarks.get(id)}));
            }
            channels.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
            return Ok(json!({"action":"list","channels":channels}));
        }
        if matches!(self.kind, Kind::Channel) && action == "read" {
            let document = self
                .api
                .require_member(field(&scope, "channelId")?, &actor)?;
            let count = args
                .get("count")
                .map(|n| {
                    n.as_u64()
                        .filter(|n| (1..=200).contains(n))
                        .ok_or_else(|| Failure::Refused("count must be between 1 and 200".into()))
                })
                .transpose()?
                .unwrap_or(20) as usize;
            let total = document.messages.len();
            let mut messages = Vec::new();
            let mut remaining = MAX_MESSAGE_BYTES;
            let mut shortened = false;
            for message in document.messages.iter().rev().take(count) {
                if remaining == 0 {
                    break;
                }
                let mut end = message.body.len().min(remaining);
                while !message.body.is_char_boundary(end) {
                    end -= 1;
                }
                messages.push(json!({"sender":message.sender,"timestamp":message.timestamp,"body":&message.body[..end]}));
                remaining -= end;
                if end < message.body.len() {
                    shortened = true;
                    break;
                }
            }
            messages.reverse();
            let truncated = messages.len() < total || shortened;
            return Ok(
                json!({"action":"read","channel":scope["channelId"],"messages":messages,
                "messageCount":messages.len(),"totalMessageCount":total,"truncated":truncated}),
            );
        }
        if crate::toolgateway::review_scope().as_ref() != Some(&scope) {
            return Err(
                "fresh confirmation for this exact conversation scope required"
                    .to_string()
                    .into(),
            );
        }
        let session = session_tree::get_session_row(&self.api.storage, ctx.session_id.as_str())
            .await?
            .ok_or_else(|| Failure::Refused("source session unavailable".into()))?;
        if session.permission_mode.as_deref() == Some(SessionPermissionMode::ReadOnly.wire_name()) {
            return Err("source session is read only".to_string().into());
        }
        let operation = format!("conversation-tool-{}", digest(json!([ctx.run_id, call])));
        let stamp = self.stamp(&operation).await?;
        let (conversation, mutation, mut result) = match self.kind {
            Kind::Channel if action == "post" => {
                let id = field(&scope, "channelId")?.to_owned();
                let body = text(args, "content")?.to_owned();
                let mutation = ConversationMutation::AppendChannel {
                    id: id.clone(),
                    actor: Some(actor.clone()),
                    sender: actor.clone(),
                    body: body.clone(),
                    timestamp: stamp.clone(),
                };
                (
                    id.clone(),
                    mutation,
                    json!({"action":"post","channel":id,"timestamp":stamp,"message":{"sender":actor,"timestamp":stamp,"body":body}}),
                )
            }
            Kind::Channel if action == "create" => {
                let name = text(args, "name")?;
                let roster = create_roster(&self.api, &actor, args)?;
                let id = format!("ch_{}", &digest(json!(&operation))[..24]);
                let mutation = ConversationMutation::CreateChannel {
                    id: id.clone(),
                    meta: json!({"id":id,"name":name,"members":roster}),
                    intro: args["intro"].as_str().map(String::from),
                    timestamp: stamp.clone(),
                };
                (
                    id.clone(),
                    mutation,
                    json!({"action":"create","channel":id,"members":roster,"timestamp":stamp}),
                )
            }
            Kind::Dm => {
                let target = field(&scope, "targetAgentId")?.to_owned();
                let mutation = ConversationMutation::AppendDm {
                    from: actor.clone(),
                    to: target.clone(),
                    body: text(args, "message")?.into(),
                    timestamp: stamp.clone(),
                };
                (
                    format!("dm:{target}"),
                    mutation,
                    json!({"action":"dm","from":actor,"to":target,"timestamp":stamp}),
                )
            }
            _ => return Err("unknown conversation write action".to_string().into()),
        };
        // 审阅和等待之后，再核验真实身份；文件适配器在实际替换前还会核频道成员。
        if self.actor(ctx).await? != actor {
            return Err("source actor changed".to_string().into());
        }
        store::bind_message_operation_source(
            &self.api.storage,
            &operation,
            ctx.run_id.as_str(),
            call.as_str(),
        )
        .await?;
        self.apply(&operation, &mutation).await?;
        match &mutation {
            ConversationMutation::AppendChannel {
                id,
                sender,
                body,
                timestamp,
                ..
            } => {
                self.api.events.publish("channel_new_message", json!({"channelName":id,"message":{"sender":sender,"timestamp":timestamp,"body":body}}));
            }
            ConversationMutation::AppendDm { from, to, .. } => {
                self.api
                    .events
                    .publish("dm_new_message", json!({"from":from,"to":to}));
            }
            ConversationMutation::CreateChannel { .. } => {
                self.api.events.publish("channel_created", json!({}));
            }
            _ => unreachable!("只构造正式会话写入"),
        }
        self.api
            .enqueue_message(&conversation, &actor, &operation, Some(ctx.run_id.as_str()))
            .await
            .map_err(|error| {
                Failure::Unknown(format!(
                    "message committed; delivery queue unconfirmed; operation {operation}: {}",
                    error.message
                ))
            })?;
        result["operationId"] = json!(operation);
        result["deliveryQueued"] = json!(true);
        Ok(result)
    }
}
impl ToolExecutorPort for Executor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            let outcome = match self.invoke(ctx, call, request.arguments.as_value()).await {
                Ok(value) => {
                    let mut result = ToolSuccess::text(value.to_string());
                    result.truncated = value["truncated"] == true;
                    result.details = value.as_object().cloned();
                    ToolOutcome::Success { result }
                }
                Err(Failure::Refused(message)) => ToolOutcome::Failed {
                    error: ProtocolError::new(ErrorCode::InvalidMessage, message, false),
                },
                Err(Failure::Unknown(reason)) => ToolOutcome::Unknown { reason },
            };
            ToolExecutionResult::of_ctx(ctx, outcome)
        })
    }
}

pub fn register(
    gateway: &crate::toolgateway::ToolInvocationGateway,
    api: ConversationApi,
    budget: &SchemaBudget,
) -> Result<(), String> {
    let definitions = [
        ("channel",Kind::Channel,PermissionKind::ArgumentAwareChannel,"channel.action",
            json!({"type":"object","properties":{"action":{"type":"string","enum":["list","read","post","create"]},"channel":{"type":"string"},"content":{"type":"string","maxLength":65536},"name":{"type":"string","maxLength":256},"members":{"type":"array","items":{"type":"string"},"maxItems":128},"intro":{"type":"string","maxLength":65536},"count":{"type":"integer","minimum":1,"maximum":200}},"required":["action"],"additionalProperties":false}),
            "读取已加入的频道，或经具体范围确认后创建频道、发送消息。发送者来自当前真实会话，不能用此工具查看助手私信。"),
        ("dm",Kind::Dm,PermissionKind::Review,"dm.send",
            json!({"type":"object","properties":{"to":{"type":"string"},"message":{"type":"string","maxLength":65536}},"required":["to","message"],"additionalProperties":false}),
            "经确认后向另一助手发送单条私信；不是委派任务工具。双方历史均有持久写入回执，排队不表示对方已经回复。"),
    ];
    for (name, kind, permission, capability, schema, description) in definitions {
        let registered = gateway
            .registry()
            .register(
                ToolManifest {
                    origin: ToolOrigin::FirstParty,
                    local_name: name.into(),
                    display_name: name.into(),
                    aliases: vec![],
                    version: "1.0.0".into(),
                    description: description.into(),
                    input_schema: ToolSchemaDocument {
                        dialect: "json-schema/2020-12".into(),
                        schema,
                    },
                    output_schema: None,
                    permission: PermissionContract {
                        kind: permission,
                        capability_base: capability.into(),
                    },
                    availability: Availability::Available,
                    timeout_ms: Some(60_000),
                    max_concurrency: None,
                    declared_permission: DeclaredPermission::Execute,
                    recovery: ToolRecoveryCapability::CONSERVATIVE,
                },
                budget,
            )
            .map_err(|error| format!("{error:?}"))?;
        gateway
            .registry()
            .add_model_resident(&registered.target_id)
            .map_err(|error| format!("{error:?}"))?;
        let review_api = api.clone();
        gateway.bind_executor_with_review(
            registered.target_id,
            Arc::new(Executor {
                api: api.clone(),
                kind,
            }),
            Arc::new(move |input, args| {
                review(
                    &review_api,
                    kind,
                    input.agent_id,
                    input.session_id,
                    args.as_value(),
                )
            }),
            "真实 Run 身份、频道成员和单次确认范围约束的会话通信",
        );
    }
    Ok(())
}
