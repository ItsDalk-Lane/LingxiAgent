//! 助手只提出自动任务建议；通知先入持久队列，真实展示回执才表示送达。
use crate::automation::{job_view, prepare_job, JobApi};
use lingxi_adapters::storage::{
    automation as store, automation_suggestions as drafts, session_tree,
};
use lingxi_kernel::invocation::ToolRecoveryCapability;
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

#[derive(Clone, Copy)]
enum Kind {
    Automation,
    Notify,
}
struct Executor {
    api: JobApi,
    kind: Kind,
}
fn field<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("{key} required"))
}
impl Executor {
    async fn invoke(
        &self,
        ctx: &RunContext,
        call: &ToolCallId,
        args: &Value,
    ) -> Result<Value, String> {
        let current = self
            .api
            .storage
            .load_run(&ctx.run_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("run unavailable")?;
        if current.session_id != ctx.session_id || current.status.is_terminal() {
            return Err("active owned Run required".into());
        }
        let active=self.api.storage.query_one_text("SELECT r.run_id FROM runs r JOIN run_attempts a ON a.run_id=r.run_id WHERE r.run_id=?1 AND r.session_id=?2 AND r.owner_kind=?3 AND r.owner_subject=?4 AND r.generation=?5 AND a.attempt=?6 AND a.generation=r.generation AND r.status IN ('running','waiting_approval') AND a.attempt=(SELECT attempt FROM run_attempts WHERE run_id=r.run_id ORDER BY started_at_unix_ms DESC,rowid DESC LIMIT 1)",vec![ctx.run_id.to_string(),ctx.session_id.to_string(),ctx.principal.storage_kind().into(),ctx.principal.storage_subject(),ctx.generation.to_string(),ctx.attempt.to_string()]).await.map_err(|e|e.to_string())?;
        if active.is_none() {
            return Err("run ownership or generation changed".into());
        }
        match self.kind {
            Kind::Automation => self.automation(ctx, call, args).await,
            Kind::Notify => self.notify(ctx, call, args).await,
        }
    }
    async fn automation(
        &self,
        ctx: &RunContext,
        call: &ToolCallId,
        args: &Value,
    ) -> Result<Value, String> {
        if ctx.principal != lingxi_kernel::Principal::LocalUser {
            return Err("only local owner may propose an automation".into());
        }
        if args["action"] == "list" {
            let jobs = store::list_jobs(&self.api.storage, Some(&self.api.studio_id))
                .await
                .map_err(|e| e.to_string())?;
            return Ok(
                json!({"action":"list","jobs":jobs.iter().map(job_view).collect::<Result<Vec<_>,_>>()?}),
            );
        }
        let operation = field(args, "action")?;
        if !["create", "update"].contains(&operation) {
            return Err("unknown automation action".into());
        }
        let prior = if operation == "update" {
            Some(
                store::get_job(&self.api.storage, field(args, "id")?)
                    .await
                    .map_err(|e| e.to_string())?
                    .ok_or("automation not found")?,
            )
        } else {
            None
        };
        let source = session_tree::get_session_row(&self.api.storage, ctx.session_id.as_str())
            .await
            .map_err(|e| e.to_string())?
            .ok_or("source session unavailable")?;
        let mut config = prior
            .as_ref()
            .map(|j| serde_json::from_str::<Value>(&j.config_json))
            .transpose()
            .map_err(|e| e.to_string())?
            .unwrap_or_else(|| json!({}));
        for key in ["scheduleType", "schedule", "prompt", "label", "model"] {
            if let Some(value) = args.get(key) {
                config[key] = value.clone();
            }
        }
        let kind = config["scheduleType"]
            .as_str()
            .or_else(|| config["type"].as_str())
            .ok_or("scheduleType required")?
            .to_string();
        if kind == "every" && args.get("schedule").is_some() {
            // 模型工具沿用分钟单位；管理接口与数据库沿用毫秒。
            let minutes = config["schedule"]
                .as_str()
                .and_then(|v| v.parse::<u64>().ok())
                .or_else(|| config["schedule"].as_u64())
                .ok_or("every schedule must be minutes")?;
            config["schedule"] = json!(minutes
                .checked_mul(60_000)
                .ok_or("every schedule overflow")?);
        }
        let actor = args["agentId"]
            .as_str()
            .or_else(|| prior.as_ref().map(|j| j.actor_id.as_str()))
            .unwrap_or(&source.agent_id)
            .to_string();
        config["actorAgentId"] = json!(actor);
        config["executionContext"] = if actor == source.agent_id {
            json!({"sourceSessionId":ctx.session_id})
        } else {
            json!({})
        };
        config["createdBy"] =
            json!({"kind":"agent","agentId":source.agent_id,"sourceSessionId":ctx.session_id});
        let principal = crate::automation::local_owner_principal();
        let job = prepare_job(&self.api, &principal, config, prior.as_ref()).await?;
        let id = format!(
            "suggestion-{}",
            lingxi_kernel::session_tree::sha256_hex(format!("{}:{}", ctx.run_id, call).as_bytes())
        );
        let draft = drafts::Suggestion {
            id: id.clone(),
            studio_id: self.api.studio_id.clone(),
            session_id: ctx.session_id.to_string(),
            owner_kind: ctx.principal.storage_kind().into(),
            owner_subject: ctx.principal.storage_subject(),
            expected_revision: prior.as_ref().map(|j| j.config_revision),
            job_id: prior.map(|j| j.id),
            job: job.clone(),
            expires_at: (self.api.clock.now_unix_ms() + 30 * 60_000) as i64,
        };
        drafts::create(
            &self.api.storage,
            &draft,
            self.api.clock.now_unix_ms() as i64,
        )
        .await
        .map_err(|e| e.to_string())?;
        let data: Value = serde_json::from_str(&job.config_json).map_err(|e| e.to_string())?;
        Ok(
            json!({"action":if operation=="create"{"pending_add"}else{"pending_update"},"confirmed":false,"operation":operation,"suggestionId":id,"suggestionShortCode":id,"jobData":data,"automationSuggestion":{"id":id,"suggestionId":id,"shortCode":id,"sessionId":ctx.session_id,"operation":operation,"expiresAtUnixMs":draft.expires_at,"jobData":data},"message":"自动任务建议已保存，需由用户应用后生效。"}),
        )
    }
    async fn notify(
        &self,
        ctx: &RunContext,
        call: &ToolCallId,
        args: &Value,
    ) -> Result<Value, String> {
        if ctx.principal != lingxi_kernel::Principal::LocalUser {
            return Err("notification owner is not available for this principal".into());
        }
        let source = session_tree::get_session_row(&self.api.storage, ctx.session_id.as_str())
            .await
            .map_err(|e| e.to_string())?
            .ok_or("notification source session unavailable")?;
        if source.lifecycle != "active"
            || source.owner_user_id != lingxi_kernel::LOCAL_OWNER_SUBJECT
            || self
                .api
                .memory
                .is_agent_deleted_checked(&source.agent_id)
                .map_err(|e| format!("{e:?}"))?
        {
            return Err("notification source authorization unavailable".into());
        }
        let title = field(args, "title")?;
        let body = field(args, "body")?;
        if title.chars().count() > 500 || body.len() > 64 * 1024 {
            return Err("notification exceeds size limit".into());
        }
        let channels = match args.get("channels") {
            None => vec!["desktop"],
            Some(value) => value
                .as_array()
                .ok_or("channels must be array")?
                .iter()
                .map(|v| v.as_str().ok_or("notification channel must be string"))
                .collect::<Result<Vec<_>, _>>()?,
        };
        let lineage = self
            .api
            .storage
            .load_run_lineage(&ctx.run_id)
            .await
            .map_err(|e| e.to_string())?;
        let trigger = if let Some(cause) = lineage.and_then(|l| l.cause_id) {
            store::get_trigger(&self.api.storage, &cause)
                .await
                .map_err(|e| e.to_string())?
                .map(|t| t.trigger_id)
        } else {
            None
        };
        let mut deliveries = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for channel in channels {
            let channel = if channel == "auto" {
                "desktop"
            } else {
                channel
            };
            if !seen.insert(channel) {
                continue;
            }
            if channel == "bridge_owner" {
                let requested =
                    args.get("bridgePlatforms")
                        .and_then(Value::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(Value::as_str)
                                .collect::<std::collections::BTreeSet<_>>()
                        });
                let connectors =
                    lingxi_adapters::storage::bridge::list_connectors(&self.api.storage)
                        .await
                        .map_err(|e| e.to_string())?;
                let mut matched = false;
                for connector in connectors.into_iter().filter(|row| {
                    row.input.actor_id == source.agent_id
                        && row.input.enabled
                        && requested.as_ref().is_none_or(|platforms| {
                            platforms.is_empty() || platforms.contains(row.input.platform.as_str())
                        })
                }) {
                    matched = true;
                    let id = format!(
                        "notify-{}",
                        lingxi_kernel::session_tree::sha256_hex(
                            json!([ctx.run_id, call, connector.input.id])
                                .to_string()
                                .as_bytes()
                        )
                    );
                    let input=lingxi_adapters::storage::bridge::BridgeOutboxInput {
                        delivery_id:id.clone(),connector_id:connector.input.id.clone(),connector_generation:connector.generation,
                        inbox_id:None,manual_action_id:None,run_id:Some(ctx.run_id.to_string()),loop_delivery_id:None,
                        payload_json:json!({"text":format!("{title}\n\n{body}"),"title":title,"body":body,"contextPolicy":args["contextPolicy"].as_str().unwrap_or("record_when_delivered"),"sourceSessionId":ctx.session_id,"toolCallId":call}).to_string(),attachments:vec![],
                    };
                    match lingxi_adapters::storage::bridge::enqueue_outbox(&self.api.storage,&input,self.api.clock.now_unix_ms()).await {
                        Ok(receipt)=>deliveries.push(json!({"channel":"bridge_owner","platform":connector.input.platform,"status":receipt.outbox.status,"deliveryId":id})),
                        Err(error)=>deliveries.push(json!({"channel":"bridge_owner","platform":connector.input.platform,"status":"failed","error":error.to_string()})),
                    }
                }
                if !matched {
                    deliveries.push(json!({"channel":"bridge_owner","status":"failed","error":"no_enabled_owner_bridge_destination"}));
                }
                continue;
            }
            if channel != "desktop" {
                deliveries.push(json!({"channel":channel,"status":"failed","error":"bridge_notification_transport_unavailable"}));
                continue;
            }
            let id = format!(
                "delivery-{}",
                lingxi_kernel::session_tree::sha256_hex(
                    format!("{}:{}:{channel}", ctx.run_id, call).as_bytes()
                )
            );
            let receipt=store::reserve_notification(&self.api.storage,&store::NotificationInput{delivery_id:id,trigger_id:trigger.clone(),run_id:Some(ctx.run_id.to_string()),channel:channel.into(),recipient:ctx.principal.storage_subject(),payload_json:json!({"title":title,"body":body,"sessionId":ctx.session_id,"runId":ctx.run_id,"agentId":source.agent_id,"contextPolicy":args["contextPolicy"]}).to_string()},self.api.clock.now_unix_ms() as i64).await.map_err(|e|e.to_string())?;
            deliveries.push(json!({"channel":channel,"status":receipt.receipt.status,"deliveryId":receipt.receipt.delivery_id}));
        }
        Ok(
            json!({"title":title,"body":body,"sent":false,"deliveries":deliveries,"message":"通知已排队；真实投递回执到达前不表示已送达。"}),
        )
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
                    result.details = value.as_object().cloned();
                    ToolOutcome::Success { result }
                }
                Err(message) => ToolOutcome::Failed {
                    error: ProtocolError::new(ErrorCode::InvalidMessage, message, false),
                },
            };
            ToolExecutionResult::of_ctx(ctx, outcome)
        })
    }
}
pub fn register(
    gateway: &crate::toolgateway::ToolInvocationGateway,
    api: JobApi,
    budget: &SchemaBudget,
) -> Result<(), String> {
    let definitions=[
        ("automation",Kind::Automation,PermissionKind::Read,"automation.propose",json!({"type":"object","properties":{"action":{"type":"string","enum":["list","create","update"]},"id":{"type":"string"},"agentId":{"type":"string"},"scheduleType":{"type":"string","enum":["at","every","cron"]},"schedule":{"type":"string"},"label":{"type":"string"},"prompt":{"type":"string"},"model":{}},"required":["action"],"additionalProperties":false}),"查看定时任务或提出待用户确认的自动任务建议。every 的 schedule 单位为分钟；建议未经用户应用不会执行。"),
        ("notify",Kind::Notify,PermissionKind::Execute,"notify.send",json!({"type":"object","properties":{"title":{"type":"string"},"body":{"type":"string"},"audience":{"type":"string","enum":["owner"]},"channels":{"type":"array","items":{"type":"string","enum":["auto","desktop","bridge_owner"]}},"bridgePlatforms":{"type":"array","items":{"type":"string","enum":["wechat","feishu","dingtalk","telegram","qq"]}},"contextPolicy":{"type":"string","enum":["none","record_when_delivered"]}},"required":["title","body"],"additionalProperties":false}),"在用户要求提醒或监控发现需要关注的变化时通知用户；排队不表示送达，以真实投递回执为准。"),
    ];
    for (name, kind, permission, capability, schema, description) in definitions {
        let receipt = gateway
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
                    declared_permission: if name == "automation" {
                        DeclaredPermission::ReadOnly
                    } else {
                        DeclaredPermission::Execute
                    },
                    recovery: ToolRecoveryCapability::CONSERVATIVE,
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
            Arc::new(Executor {
                api: api.clone(),
                kind,
            }),
            "自动任务建议与持久通知投递",
        );
    }
    Ok(())
}
