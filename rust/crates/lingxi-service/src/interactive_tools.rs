//! 批量提问、完整清单与交互卡正式工具；权限仍由统一工具网关判定。
use crate::{
    interaction_store::InteractionStore, toolgateway::ToolInvocationGateway, Principal,
    ServiceState, SessionAccess,
};
use axum::{
    extract::{Extension, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
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
    Ask,
    Todo,
    Card,
}
struct Executor {
    store: Arc<InteractionStore>,
    kind: Kind,
}
pub fn normalize_todos(args: &Value) -> Result<crate::todos::TodoStateEntry, String> {
    let raw = args["todos"]
        .as_array()
        .ok_or("todos must be the complete array")?;
    if raw.len() > 500 {
        return Err("todo list exceeds 500 items".into());
    }
    let mut seen = std::collections::HashSet::new();
    let mut todos = Vec::new();
    for (i, item) in raw.iter().enumerate() {
        let content = item["content"].as_str().unwrap_or("").trim();
        if content.is_empty() || content.chars().count() > 4000 || !seen.insert(content.to_string())
        {
            return Err("todo content is empty, oversized or duplicated".into());
        }
        let status = item["status"]
            .as_str()
            .filter(|s| crate::todos::TODO_STATUSES.contains(s))
            .ok_or("invalid todo status")?;
        let active = item["activeForm"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(content);
        let blocked = item["blockedReason"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if active.chars().count() > 4000
            || blocked.is_some_and(|s| s.chars().count() > 4000)
            || (status == "blocked" && blocked.is_none())
        {
            return Err("blocked todo requires a bounded reason".into());
        }
        todos.push(crate::todos::TodoItem {
            id: format!("t{}", i + 1),
            content: content.into(),
            status: status.into(),
            active_form: Some(active.into()),
            blocked_reason: blocked.map(str::to_string),
        });
    }
    Ok(crate::todos::TodoStateEntry {
        version: crate::todos::compute_todo_list_version(&todos),
        removed: todos.is_empty(),
        todos,
        action: "replace".into(),
    })
}
impl Executor {
    async fn run(&self, ctx: &RunContext, args: &Value) -> ToolOutcome {
        if let Err(e) = self.store.validate(ctx).await {
            return fail(e);
        }
        match self.kind {
            Kind::Ask => match self.store.ask(ctx, args).await {
                Ok(value) => {
                    if let Err(e) = self
                        .store
                        .commit(ctx, "interactive-answer-v1", &value, "question_answered")
                        .await
                    {
                        return fail(e);
                    }
                    ok("用户提问已结束", value)
                }
                Err(e) => fail(e),
            },
            Kind::Todo => {
                let entry = match normalize_todos(args) {
                    Ok(x) => x,
                    Err(e) => return fail(e),
                };
                let value = serde_json::to_value(&entry).expect("todo serializes");
                match self
                    .store
                    .commit(
                        ctx,
                        crate::todos::TODO_STATE_ENTRY_TYPE,
                        &value,
                        "todo_update",
                    )
                    .await
                {
                    Ok(()) => ok(
                        "已替换完整任务清单",
                        json!({"todos":entry.todos,"todoVersion":2,"version":entry.version}),
                    ),
                    Err(e) => fail(e),
                }
            }
            Kind::Card => {
                let (Some(title), Some(code)) = (args["title"].as_str(), args["code"].as_str())
                else {
                    return fail("title and code must be strings");
                };
                if title.len() > 500 || code.len() > 1024 * 1024 {
                    return fail("card title or code exceeds size limit");
                }
                let card = match crate::auth::hex_random_public(16) {
                    Ok(id) => {
                        json!({"cardId":format!("c_{id}"),"title":title,"code":code,"status":"rendered"})
                    }
                    Err(e) => return fail(e.to_string()),
                };
                match self
                    .store
                    .commit(ctx, "interactive-card-v1", &card, "interactive_card")
                    .await
                {
                    Ok(()) => ok(format!("Card {title} rendered."), card),
                    Err(e) => fail(e),
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
    store: Arc<InteractionStore>,
    budget: &SchemaBudget,
) {
    let questions = json!({"type":"array","minItems":1,"maxItems":8,"items":{"type":"object","properties":{"key":{"type":"string"},"question":{"type":"string"},"type":{"type":"string","enum":["single","multi","text"]},"options":{"type":"array","items":{"type":"object","properties":{"value":{"type":"string"},"label":{"type":"string"},"description":{"type":"string"}},"required":["label"],"additionalProperties":false}},"recommended":{"type":["string","array"],"items":{"type":"string"}},"required":{"type":"boolean"}},"required":["question"],"additionalProperties":false}});
    let schemas=[
        ("ask_user",Kind::Ask,PermissionKind::Read,"ask_user.ask",json!({"type":"object","properties":{"questions":questions,"timeout_seconds":{"type":"number"}},"required":["questions"],"additionalProperties":false}),"Ask the user 1..8 related questions in one card, and wait for explicit answers. Mark recommended option values for timeout; dismissal and abort never produce answers."),
        ("todo_write",Kind::Todo,PermissionKind::Execute,"todo_write.replace",json!({"type":"object","properties":{"todos":{"type":"array","items":{"type":"object","properties":{"content":{"type":"string"},"activeForm":{"type":"string"},"status":{"type":"string","enum":crate::todos::TODO_STATUSES},"blockedReason":{"type":"string"}},"required":["content","status"],"additionalProperties":false}}},"required":["todos"],"additionalProperties":false}),"Replace the entire session task list. Use pending, in_progress, blocked, cancelled, completed; blocked requires blockedReason. Preserve unfinished work honestly."),
        ("show_card",Kind::Card,PermissionKind::Execute,"show_card.render",json!({"type":"object","properties":{"title":{"type":"string"},"code":{"type":"string"}},"required":["title","code"],"additionalProperties":false}),"Show an HTML/SVG interactive card in the conversation. Call hana_card_guide first. Code is kept in the session result and rendered only in an isolated iframe; no source file is written.")
    ];
    for (name, kind, permission, capability, schema, description) in schemas {
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
                    timeout_ms: Some(if name == "ask_user" {
                        1_810_000
                    } else {
                        60_000
                    }),
                    max_concurrency: None,
                    declared_permission: if name == "ask_user" {
                        DeclaredPermission::ReadOnly
                    } else {
                        DeclaredPermission::Execute
                    },
                    recovery: ToolRecoveryCapability::CONSERVATIVE,
                },
                budget,
            )
            .expect("interactive tool registers");
        let executor = Arc::new(Executor {
            store: store.clone(),
            kind,
        });
        if name == "show_card" {
            gateway.bind_executor_with_review(receipt.target_id,executor,Arc::new(|input,args|Ok(json!({"summary":format!("渲染会话内容卡：{}",args.as_value()["title"].as_str().unwrap_or("")),"sessionId":input.session_id,"title":args.as_value()["title"],"codeSHA256":lingxi_kernel::session_tree::sha256_hex(args.as_value()["code"].as_str().unwrap_or("").as_bytes())}))),"会话内容卡审查后渲染");
        } else {
            gateway.bind_executor(receipt.target_id, executor, "正式批量问答与整清单工具");
        }
    }
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionQuery {
    session_id: String,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnswerBody {
    session_id: String,
    id: String,
    action: String,
    #[serde(default)]
    values: Value,
}
async fn authorized(state: &ServiceState, p: &Principal, sid: &str) -> Option<Response> {
    match state.sessions().get_for(p, sid).await {
        Ok(SessionAccess::Ok(_)) => None,
        Ok(SessionAccess::NotFound) => Some(StatusCode::NOT_FOUND.into_response()),
        Ok(SessionAccess::Forbidden) => Some(StatusCode::FORBIDDEN.into_response()),
        Err(e) => Some(crate::EndpointError::storage(&e).into_response()),
    }
}
pub async fn state_route(
    Extension(p): Extension<Principal>,
    State(state): State<ServiceState>,
    query: Result<Query<SessionQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Ok(Query(q)) = query else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if let Some(r) = authorized(&state, &p, &q.session_id).await {
        return r;
    }
    let cards = match state.interactions().cards(&q.session_id).await {
        Ok(x) => x,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":e}))).into_response()
        }
    };
    let snapshot = match crate::todos::latest_todo_snapshot(&state, &q.session_id).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    Json(json!({"sessionId":q.session_id,"questions":state.interactions().list_pending(&q.session_id),"cards":cards,"todoPanel":crate::todos::panel_from_snapshot(snapshot.as_ref())})).into_response()
}
pub async fn answer_route(
    Extension(p): Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<AnswerBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Ok(Json(b)) = body else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if let Some(r) = authorized(&state, &p, &b.session_id).await {
        return r;
    }
    match state
        .interactions()
        .answer(&b.session_id, &b.id, &b.action, &b.values)
    {
        Ok(()) => Json(json!({"ok":true})).into_response(),
        Err(e) => (StatusCode::CONFLICT, Json(json!({"error":e}))).into_response(),
    }
}
