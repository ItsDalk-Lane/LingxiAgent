//! `current_status` 与 `session_folders` 一方工具（现役 lib/tools/current-status-tool.ts
//! 与 lib/tools/session-folders-tool.ts 的模型侧入口）。
//!
//! 两者此前只有 HTTP 只读面（/lingxi/v1/current-status、/sessions/{id}/authorized-
//! folders），没有工具登记——模型经按需目录（mcp_describe_tool → mcp_call）调用时
//! 落 "unknown tool"。本模块把它们接入统一工具网关：
//! - `current_status`：只读（Read）；list 返回键目录，get 按键取值。负载与 HTTP 面
//!   同源（agent_config_readback::current_status_payload），会话/agent 取运行会话及其
//!   归属 agent，模型/会话值与运行时一致。
//! - `session_folders`：list 只读；add/remove 为写类（ArgumentAwareSessionFolders：
//!   operate 预批、ask 经审批、read_only 拒——权限由内核目录按 action 解析），变更只
//!   作用于当前运行会话的授权目录，不改 cwd 与提示词；目录校验与路由层同口径
//!   （add 须存在且为目录；remove 只规范化）。
use crate::agents_admin::agent_config_readback::{current_status_payload, CurrentStatusError};
use crate::toolgateway::ToolInvocationGateway;
use crate::{session_admin, ServiceState};
use lingxi_kernel::invocation::ToolRecoveryCapability;
use lingxi_kernel::{
    ports::{ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, ToolSuccess},
    toolcatalog::{
        Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
        ToolManifest, ToolOrigin, ToolRegistry,
    },
    RunContext,
};
use lingxi_protocol::{ErrorCode, ProtocolError, ToolCallId, ToolSchemaDocument};
use serde_json::{json, Value};
use std::sync::Arc;

fn fail(message: impl Into<String>) -> ToolOutcome {
    ToolOutcome::Failed {
        error: ProtocolError::new(ErrorCode::InvalidMessage, message.into(), false),
    }
}

fn ok(message: impl Into<String>, details: Value) -> ToolOutcome {
    let mut result = ToolSuccess::text(message.into());
    result.details = details.as_object().cloned();
    ToolOutcome::Success { result }
}

#[derive(Clone, Copy)]
enum Kind {
    CurrentStatus,
    SessionFolders,
}

struct Executor {
    state: ServiceState,
    kind: Kind,
}

/// 现役 folderScopeText：cwd/工作区/沙盒根由桌面运行时持有，端口服务不跟踪，
/// 显式缺席（null/空）不猜造；授权目录来自会话行。
fn folder_scope_text(session_id: &str, authorized: &[String]) -> String {
    serde_json::to_string_pretty(&json!({
        "session_folders": {
            "sessionPath": crate::session_locator::native(session_id),
            "cwd": Value::Null,
            "workspaceFolders": [],
            "authorizedFolders": authorized,
            "sandboxFolders": [],
        }
    }))
    .unwrap_or_else(|_| "{}".to_string())
}

/// 现役 validateAuthorizedFolder：必填 → 绝对化 → 必须存在且为目录。
fn validate_folder(raw: Option<&str>) -> Result<String, String> {
    let Some(folder) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err("session_folders add/remove requires folder.".into());
    };
    let absolute = std::path::absolute(folder).unwrap_or_else(|_| std::path::PathBuf::from(folder));
    let metadata = std::fs::metadata(&absolute).map_err(|_| "folder does not exist".to_string())?;
    if !metadata.is_dir() {
        return Err("folder must be a directory".into());
    }
    Ok(absolute
        .canonicalize()
        .unwrap_or(absolute)
        .to_string_lossy()
        .into_owned())
}

/// 现役 normalizeAuthorizedFolderPath：remove 只规范化，不要求存在。
fn normalize_folder(raw: Option<&str>) -> Result<String, String> {
    let Some(folder) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err("session_folders add/remove requires folder.".into());
    };
    let absolute = std::path::absolute(folder).unwrap_or_else(|_| std::path::PathBuf::from(folder));
    Ok(absolute
        .canonicalize()
        .unwrap_or(absolute)
        .to_string_lossy()
        .into_owned())
}

impl Executor {
    async fn run(&self, ctx: &RunContext, args: &Value) -> ToolOutcome {
        let session_id = ctx.session_id.as_str();
        match self.kind {
            Kind::CurrentStatus => {
                let action = args["action"].as_str().unwrap_or("list");
                let key = args["key"].as_str();
                let agent_id = match self.state.session_admin().get_session_row(session_id).await {
                    Ok(Some(row)) => Some(row.agent_id),
                    Ok(None) => None,
                    Err(error) => {
                        return fail(format!("current_status session lookup failed: {error:?}"))
                    }
                };
                match current_status_payload(
                    &self.state,
                    action,
                    key,
                    Some(session_id),
                    agent_id.as_deref(),
                )
                .await
                {
                    Ok(payload) => ok(
                        serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string()),
                        json!({ "action": action, "key": key }),
                    ),
                    Err(CurrentStatusError::BadRequest(message)) => fail(message),
                    Err(CurrentStatusError::Internal(message)) => fail(message),
                }
            }
            Kind::SessionFolders => {
                let action = args["action"].as_str().unwrap_or("list");
                let admin = self.state.session_admin();
                if action == "list" {
                    return match admin.get_session_row(session_id).await {
                        Ok(Some(row)) => ok(
                            folder_scope_text(session_id, &row.authorized_folders),
                            json!({ "action": "list", "sessionPath": crate::session_locator::native(session_id) }),
                        ),
                        Ok(None) => fail("session_folders requires a current session."),
                        Err(error) => fail(format!("session folder scope unavailable: {error:?}")),
                    };
                }
                let mutation = match action {
                    "add" => match validate_folder(args["folder"].as_str()) {
                        Ok(folder) => session_admin::FolderAction::Add(folder),
                        Err(message) => return fail(message),
                    },
                    "remove" => match normalize_folder(args["folder"].as_str()) {
                        Ok(folder) => session_admin::FolderAction::Remove(folder),
                        Err(message) => return fail(message),
                    },
                    other => return fail(format!("Invalid action {other} (list|add|remove)")),
                };
                let folder = match &mutation {
                    session_admin::FolderAction::Add(folder)
                    | session_admin::FolderAction::Remove(folder) => folder.clone(),
                    session_admin::FolderAction::Set(_) => unreachable!("set 不是工具动作"),
                };
                match admin.mutate_authorized_folders(session_id, mutation).await {
                    Ok(folders) => ok(
                        folder_scope_text(session_id, &folders),
                        json!({
                            "action": action,
                            "confirmed": true,
                            "sessionPath": crate::session_locator::native(session_id),
                            "folder": folder,
                        }),
                    ),
                    Err(error) => fail(format!("session folder change failed: {error:?}")),
                }
            }
        }
    }
}

impl ToolExecutorPort for Executor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            ToolExecutionResult::of_ctx(ctx, self.run(ctx, request.arguments.as_value()).await)
        })
    }
}

pub fn register(
    registry: &Arc<ToolRegistry>,
    gateway: &ToolInvocationGateway,
    state: ServiceState,
    budget: &SchemaBudget,
) {
    let entries: [(&str, Kind, PermissionKind, &str, DeclaredPermission, Value, &str); 2] = [
        (
            "current_status",
            Kind::CurrentStatus,
            PermissionKind::Read,
            "current_status.read",
            DeclaredPermission::ReadOnly,
            json!({"type":"object","properties":{"action":{"type":"string","enum":["list","get"]},"key":{"type":"string"}},"required":["action"],"additionalProperties":false}),
            "Lightweight current-environment status (time, agent identity, model, session, session files/folders, Bridge context, subagents). The system prompt's \"Session started at\" line is a frozen snapshot of when the session began; call with key=\"time\" for the precise current time. Use action=list to discover available keys.",
        ),
        (
            "session_folders",
            Kind::SessionFolders,
            PermissionKind::ArgumentAwareSessionFolders,
            "session_folders",
            DeclaredPermission::Execute,
            json!({"type":"object","properties":{"action":{"type":"string","enum":["list","add","remove"]},"folder":{"type":"string"}},"required":["action"],"additionalProperties":false}),
            "List or request changes to the current session's extra authorized sandbox folders. Use action=list to inspect cwd, prompt-visible workspace folders, user-authorized folders, and effective sandbox roots. Use add/remove only after the user wants this session to gain or drop folder access; auto mode routes the request to the approval reviewer, ask mode asks the user, and the change never modifies CWD or prompt text.",
        ),
    ];
    for (name, kind, permission, capability, declared, schema, description) in entries {
        let receipt = registry
            .register(
                ToolManifest {
                    origin: ToolOrigin::FirstParty,
                    local_name: name.into(),
                    display_name: name.into(),
                    aliases: Vec::new(),
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
                    timeout_ms: Some(30_000),
                    max_concurrency: None,
                    declared_permission: declared,
                    recovery: ToolRecoveryCapability::CONSERVATIVE,
                },
                budget,
            )
            .expect("status tool registers");
        let executor = Arc::new(Executor {
            state: state.clone(),
            kind,
        });
        gateway.bind_executor(receipt.target_id, executor, "当前会话状态与目录范围工具");
    }
}
