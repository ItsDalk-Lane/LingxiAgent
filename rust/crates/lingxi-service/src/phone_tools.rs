//! 手机专属控制面；一次真实投递只能留下一个公开决策。
use crate::conversations::{ConversationApi, ConversationError};
use lingxi_adapters::storage::conversations as store;
use lingxi_kernel::{
    ports::{
        StoragePort, ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, ToolSuccess,
    },
    toolcatalog::{
        Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
        ToolManifest, ToolOrigin,
    },
    RunContext,
};
use lingxi_protocol::{ErrorCode, ProtocolError, ToolCallId, ToolSchemaDocument};
use serde_json::{json, Value};
use std::sync::Arc;
struct PhoneExecutor {
    api: ConversationApi,
    name: &'static str,
}
impl PhoneExecutor {
    async fn invoke(&self, ctx: &RunContext, args: &Value) -> Result<Value, ConversationError> {
        let lineage = self
            .api
            .storage
            .load_run_lineage(&ctx.run_id)
            .await?
            .ok_or_else(|| ConversationError::new(403, "phone lineage missing"))?;
        if !matches!(
            lineage.origin,
            lingxi_kernel::subagent::RunOrigin::Channel | lingxi_kernel::subagent::RunOrigin::Dm
        ) {
            return Err(ConversationError::new(
                403,
                "tool requires the actual phone root Run",
            ));
        }
        let d = store::get_delivery(
            &self.api.storage,
            lineage
                .cause_id
                .as_deref()
                .ok_or_else(|| ConversationError::new(403, "phone cause missing"))?,
        )
        .await?
        .ok_or_else(|| ConversationError::new(403, "phone delivery missing"))?;
        let actual = store::get_delivery_run(&self.api.storage, &d.input.delivery_id)
            .await?
            .ok_or_else(|| ConversationError::new(403, "actual phone Run missing"))?;
        if actual.0 != ctx.run_id.as_str()
            || actual.1 != ctx.session_id.as_str()
            || ctx.principal != lingxi_kernel::Principal::LocalUser
        {
            return Err(ConversationError::new(403, "phone identity mismatch"));
        }
        let live=self.api.storage.query_one_text("SELECT r.run_id FROM runs r JOIN run_attempts a ON a.run_id=r.run_id WHERE r.run_id=?1 AND r.generation=?2 AND r.status IN ('running','waiting_approval') AND a.attempt=?3 AND a.generation=r.generation AND a.attempt=(SELECT attempt FROM run_attempts WHERE run_id=r.run_id ORDER BY started_at_unix_ms DESC,rowid DESC LIMIT 1)",vec![ctx.run_id.to_string(),ctx.generation.to_string(),ctx.attempt.to_string()]).await?;
        if live.is_none() {
            return Err(ConversationError::new(403, "phone Run is not current"));
        }
        let settings = self
            .api
            .settings(&d.input.actor_id, &d.input.conversation_id)?;
        let mut authorization =
            crate::runs::DriveAuthorization::user_submission(None, settings.permission());
        authorization.grant = crate::runs::RunGrant::Phone;
        authorization.lineage = lineage;
        crate::phone_run_authority::validate(
            Some(&self.api.storage),
            &authorization,
            ctx,
            Some(self.api.memory.home()),
            false,
        )
        .await
        .map_err(|e| ConversationError::new(403, e))?;
        if self.name == "channel_read_context" {
            let count =
                match args.get("count") {
                    None => 20,
                    Some(v) => v.as_u64().filter(|c| *c > 0 && *c <= 50).ok_or_else(|| {
                        ConversationError::new(400, "count must be between 1 and 50")
                    })? as usize,
                };
            let doc = if let Some(peer) = d.input.conversation_id.strip_prefix("dm:") {
                self.api.visible_dm(
                    &d.input.actor_id,
                    peer,
                    self.api.files.read_dm(&d.input.actor_id, peer)?,
                )?
            } else {
                self.api
                    .require_member(&d.input.conversation_id, &d.input.actor_id)?
            };
            let start = doc.messages.len().saturating_sub(count);
            return Ok(
                json!({"conversationId":d.input.conversation_id,"messages":doc.messages[start..],"total":doc.messages.len(),"dropped":start}),
            );
        }
        let (decision, body) = if self.name == "channel_reply" {
            let raw = crate::conversations::field(args, "content")?.to_string();
            let (public, stopped) = crate::conversation_runtime::phone_reply_text(&raw);
            if stopped && public.is_empty() {
                (store::PhoneDecision::Pass, None)
            } else {
                (store::PhoneDecision::Reply, Some(raw))
            }
        } else {
            (store::PhoneDecision::Pass, None)
        };
        let decision = store::record_decision(
            &self.api.storage,
            &d.input.delivery_id,
            ctx.run_id.as_str(),
            decision,
            body,
            self.api.clock.now_unix_ms(),
        )
        .await?;
        self.api
            .publish_decision(
                &decision.delivery,
                decision.delivery.decision.expect("决策已保存"),
            )
            .await?;
        self.api.bookmark_delivery(&decision.delivery).await?;
        self.api.wake();
        Ok(
            json!({"ok":true,"decision":decision.delivery.decision,"deliveryId":d.input.delivery_id,"replayed":!decision.claimed}),
        )
    }
}
impl ToolExecutorPort for PhoneExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            let outcome = match self.invoke(ctx, request.arguments.as_value()).await {
                Ok(v) => {
                    let mut result = ToolSuccess::text(v.to_string());
                    result.details = v.as_object().cloned();
                    ToolOutcome::Success { result }
                }
                Err(e) => ToolOutcome::Failed {
                    error: ProtocolError::new(ErrorCode::Forbidden, e.to_string(), false),
                },
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
    for (name, schema, description) in [
        (
            "channel_read_context",
            json!({"type":"object","properties":{"count":{"type":"integer","minimum":1,"maximum":50}},"additionalProperties":false}),
            "读取本次手机投递所授权对话的最近消息。",
        ),
        (
            "channel_reply",
            json!({"type":"object","properties":{"content":{"type":"string","minLength":1,"maxLength":100000},"mood":{"type":"string"}},"required":["content"],"additionalProperties":false}),
            "在本次授权对话发表一次回复；同次投递不能再作第二次决策。",
        ),
        (
            "channel_pass",
            json!({"type":"object","properties":{"reason":{"type":"string"},"mood":{"type":"string"}},"additionalProperties":false}),
            "本次手机投递明确跳过，不向对话公开普通结尾或内省。",
        ),
    ] {
        let receipt = gateway
            .registry()
            .register(
                ToolManifest {
                    origin: ToolOrigin::FirstParty,
                    local_name: name.into(),
                    display_name: name.into(),
                    aliases: vec![],
                    version: "1".into(),
                    description: description.into(),
                    input_schema: ToolSchemaDocument {
                        dialect: "json-schema/2020-12".into(),
                        schema,
                    },
                    output_schema: None,
                    permission: PermissionContract {
                        kind: PermissionKind::Read,
                        capability_base: "phone.decide".into(),
                    },
                    availability: Availability::Available,
                    timeout_ms: Some(30000),
                    max_concurrency: None,
                    declared_permission: DeclaredPermission::ReadOnly,
                    recovery: lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE,
                },
                budget,
            )
            .map_err(|e| format!("{e:?}"))?;
        gateway
            .registry()
            .add_model_resident(&receipt.target_id)
            .map_err(|e| format!("{e:?}"))?;
        gateway.bind_executor(
            receipt.target_id,
            Arc::new(PhoneExecutor {
                api: api.clone(),
                name,
            }),
            "手机因果控制",
        );
    }
    Ok(())
}
