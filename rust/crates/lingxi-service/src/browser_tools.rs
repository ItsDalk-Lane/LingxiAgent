//! 浏览器工具沿正式网关审批与真实调用记录执行，不接受模型自报会话或执行节点。
use crate::{
    browser::BrowserService,
    toolgateway::{ReviewInput, ToolInvocationGateway},
};
use lingxi_kernel::{
    ports::{ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, ToolSuccess},
    toolcatalog::{
        Availability, DeclaredPermission, EffectiveArguments, PermissionContract, PermissionKind,
        SchemaBudget, ToolManifest, ToolOrigin,
    },
    RunContext,
};
use lingxi_protocol::{
    BrowserAction, BrowserCommand, ErrorCode, ProtocolError, ToolCallId, ToolSchemaDocument,
};
use serde_json::{json, Value};
use std::sync::Arc;

const TARGET: &str = "tool:first-party:browser";
const ACTIONS: &[&str] = &[
    "start",
    "stop",
    "navigate",
    "snapshot",
    "screenshot",
    "click",
    "type",
    "scroll",
    "select",
    "key",
    "wait",
    "evaluate",
    "show",
];
fn parse(args: &Value) -> Result<BrowserCommand, String> {
    let command: BrowserCommand =
        serde_json::from_value(args.clone()).map_err(|_| "invalid browser command".to_string())?;
    command.validate().map_err(String::from)?;
    if !command.action.is_tool_action() {
        return Err("human browser controls are not model tools".into());
    }
    if command.action == BrowserAction::Navigate {
        normalized_url(command.url.as_deref().ok_or("navigate requires URL")?)?;
    }
    Ok(command)
}
fn normalized_url(value: &str) -> Result<String, String> {
    let url = reqwest::Url::parse(value)
        .map_err(|_| "browser destination must be an absolute HTTP/HTTPS URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none()
        || url.as_str().len() > 8192
    {
        return Err("browser destination must be HTTP/HTTPS without URL credentials".into());
    }
    Ok(url.to_string())
}
fn review(
    browser: &BrowserService,
    input: &ReviewInput<'_>,
    args: &EffectiveArguments,
) -> Result<Value, String> {
    if matches!(input.principal_kind, "bridge" | "bridge_takeover") {
        return Err("external bridge cannot operate browser".into());
    }
    let command = parse(args.as_value())?;
    let action = serde_json::to_value(command.action).expect("浏览器动作");
    let mut scope = json!({"authorizationEpoch":browser.review_epoch(input.session_id),"action":action,"sourceSessionId":input.session_id,"actorId":input.agent_id,"principalKind":input.principal_kind,"principalSubject":input.principal_subject});
    if matches!(command.action, BrowserAction::Start | BrowserAction::Stop) {
        scope["summary"] = json!(format!(
            "对会话 {} 执行浏览器动作 {}",
            input.session_id,
            action.as_str().unwrap_or("unknown")
        ));
        return Ok(scope);
    }
    let live = browser
        .port()
        .workspaces()
        .map_err(|error| format!("browser current workspace unavailable: {}", error.code))?;
    let workspace = live
        .iter()
        .find(|workspace| workspace.session_id == input.session_id);
    if command.action == BrowserAction::Navigate {
        let url = normalized_url(command.url.as_deref().expect("导航参数已校验"))?;
        scope["target"] = json!({"type":"url","id":url});
        scope["sideEffect"] = json!({"destinationUrl":url});
        scope["summary"] = json!(format!("在当前会话的浏览器导航到 {url}"));
        if let Some(tab) = command.tab_id.as_deref() {
            let workspace = workspace.ok_or("browser workspace is not running")?;
            let tab = workspace
                .tabs
                .iter()
                .find(|candidate| candidate.tab_id == tab)
                .ok_or("browser tab does not belong to this session")?;
            scope["tabId"] = json!(tab.tab_id);
            scope["executionNode"] = json!(workspace.execution_node);
            scope["userTakeover"] = json!(workspace.user_takeover);
        }
        return Ok(scope);
    }
    let workspace = workspace.ok_or("browser workspace is not running")?;
    let tab_id = command
        .tab_id
        .as_deref()
        .or(workspace.active_tab_id.as_deref())
        .ok_or("browser has no active tab")?;
    let tab = workspace
        .tabs
        .iter()
        .find(|candidate| candidate.tab_id == tab_id)
        .ok_or("browser tab does not belong to this session")?;
    scope["target"] = json!({"type":"browser_tab","id":tab.tab_id});
    scope["tabId"] = json!(tab.tab_id);
    scope["currentUrl"] = json!(tab.url);
    scope["executionNode"] = json!(workspace.execution_node);
    scope["userTakeover"] = json!(workspace.user_takeover);
    scope["revoked"] = json!(workspace.revoked);
    scope["summary"] = json!(format!(
        "在会话 {} 的标签页 {} ({}) 执行 {}",
        input.session_id,
        tab.tab_id,
        tab.url.as_deref().unwrap_or("about:blank"),
        action.as_str().unwrap_or("unknown")
    ));
    Ok(scope)
}
struct Executor {
    browser: Arc<BrowserService>,
}
impl ToolExecutorPort for Executor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            let outcome = async {
                if request.target != TARGET || !request.arguments.digest_matches(&request.args_digest) {
                    return Err(ProtocolError::new(ErrorCode::Forbidden,
                        "browser requires the exact gateway-bound target and argument digest", false));
                }
                let command = parse(request.arguments.as_value())
                    .map_err(|message| ProtocolError::new(ErrorCode::InvalidMessage, message, false))?;
                let row = self.browser.tool_scope(ctx, call, request).await.map_err(protocol_error)?;
                let operation = format!("browser-tool-{}", lingxi_protocol::canon::sha256_hex(
                    json!([ctx.run_id.as_str(), call.as_str()]).to_string().as_bytes()));
                let mut value = self.browser.command(row, command.clone(), false, operation, Some(ctx))
                    .await.map_err(protocol_error)?;
                let state = value["state"].clone();
                for key in ["running", "url", "activeTabId", "tabs", "executionNode", "revoked"] {
                    value[key] = state[key].clone();
                }
                value["action"] = serde_json::to_value(command.action).expect("浏览器动作");
                let identity = json!({"sessionId":ctx.session_id.as_str(),"running":state["running"],
                    "url":state["url"],"activeTabId":state["activeTabId"],"tabs":state["tabs"]});
                let result = &value["result"];
                let body = result.as_str().map(String::from).unwrap_or_else(||
                    serde_json::to_string_pretty(result).expect("浏览器结果"));
                let mut text = format!("Browser state: {identity}\n\n{body}");
                if value["files"].as_array().is_some_and(|files| !files.is_empty()) {
                    text.push_str(&format!("\n\nRegistered session files: {}\nUse materialize with the owned fileId, then read its returned filePath to inspect image or downloaded content.", value["files"]));
                }
                let mut result = ToolSuccess::text(text);
                result.details = value.as_object().cloned();
                Ok::<_, ProtocolError>(result)
            }.await;
            ToolExecutionResult::of_ctx(
                ctx,
                match outcome {
                    Ok(result) => ToolOutcome::Success { result },
                    Err(error) => ToolOutcome::Failed { error },
                },
            )
        })
    }
}
fn protocol_error(error: lingxi_kernel::browser::BrowserError) -> ProtocolError {
    let kind = if error.code.contains("FORBIDDEN") || error.code.contains("REVOKED") {
        ErrorCode::Forbidden
    } else {
        ErrorCode::UpstreamUnavailable
    };
    ProtocolError::new(kind, error.message, false).with_details(
        json!({"errorCode":error.code,"effectUnknown":error.effect_unknown})
            .as_object()
            .expect("浏览器错误详情")
            .clone(),
    )
}
pub fn register(
    gateway: &ToolInvocationGateway,
    browser: Arc<BrowserService>,
    budget: &SchemaBudget,
) -> Result<(), String> {
    let schema = json!({"type":"object","properties":{
  "action":{"type":"string","enum":ACTIONS},"url":{"type":"string","maxLength":8192},"tabId":{"type":"string","minLength":1,"maxLength":4096},"ref":{"type":"integer","minimum":0,"maximum":4294967295u64},
  "text":{"type":"string","maxLength":100000},"direction":{"type":"string","enum":["up","down"]},"amount":{"type":"number","minimum":0,"maximum":10000},"value":{"type":"string","maxLength":100000},"key":{"type":"string","maxLength":256},"expression":{"type":"string","maxLength":10000},"timeout":{"type":"integer","minimum":1,"maximum":30000},"state":{"type":"string","enum":["domcontentloaded","load","stable","networkidle","idle"]},"pressEnter":{"type":"boolean"}},"required":["action"],"additionalProperties":false});
    let receipt=gateway.registry().register(ToolManifest{origin:ToolOrigin::FirstParty,local_name:"browser".into(),display_name:"Browser".into(),aliases:vec![],version:"1".into(),description:"操作当前会话的受控浏览器。navigate须给HTTP/HTTPS地址；click/type/select/key/evaluate须使用最新快照返回的精确tabId，元素ref在页面变化后失效。snapshot/screenshot可读，用户接管与急停由用户界面控制。".into(),input_schema:ToolSchemaDocument{dialect:"json-schema/2020-12".into(),schema},output_schema:None,permission:PermissionContract{kind:PermissionKind::ArgumentAwareBrowser,capability_base:"browser.action".into()},availability:Availability::Available,timeout_ms:Some(60000),max_concurrency:Some(64),declared_permission:DeclaredPermission::Execute,recovery:lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE},budget).map_err(|error|format!("{error:?}"))?;
    gateway
        .registry()
        .add_model_resident(&receipt.target_id)
        .map_err(|error| format!("{error:?}"))?;
    let approved = browser.clone();
    gateway.bind_executor_with_review(
        receipt.target_id,
        Arc::new(Executor { browser }),
        Arc::new(move |input, args| review(&approved, input, args)),
        "浏览器动作、真实标签页和权限确认范围绑定",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_model_actions_and_exact_safe_targets_are_accepted() {
        for action in ACTIONS {
            let parsed: BrowserAction = serde_json::from_value(json!(action)).unwrap();
            assert!(parsed.is_tool_action());
        }
        for action in ["takeover", "release", "destroy", "new_tab"] {
            assert!(parse(&json!({"action":action})).is_err());
        }
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://user:pass@example.org/",
        ] {
            assert!(parse(&json!({"action":"navigate","url":url})).is_err());
        }
        assert!(parse(&json!({"action":"click","ref":1})).is_err());
        assert!(parse(&json!({"action":"click","tabId":"*","ref":1})).is_err());
        assert!(parse(&json!({"action":"click","tabId":"tab-exact","ref":1})).is_ok());
    }
}
