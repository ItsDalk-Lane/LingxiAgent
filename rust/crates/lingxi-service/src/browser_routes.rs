//! 桌面只把明确的会话与用户操作交给服务，不把远端请求回落成本机浏览器。
use crate::{Principal, ServiceState};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use lingxi_kernel::browser::BrowserError;
use lingxi_protocol::{BrowserAction, BrowserCommand};
use serde::Deserialize;
use serde_json::{json, Value};
fn error(error: BrowserError) -> Response {
    let status = if error.code.contains("FORBIDDEN") || error.code.contains("REVOKED") {
        StatusCode::FORBIDDEN
    } else if error.code.contains("NOT_FOUND") {
        StatusCode::NOT_FOUND
    } else if error.code.contains("UNAVAILABLE") {
        StatusCode::SERVICE_UNAVAILABLE
    } else if error.effect_unknown {
        StatusCode::CONFLICT
    } else {
        StatusCode::BAD_REQUEST
    };
    (status,Json(json!({"ok":false,"error":error.message,"code":error.code,"effectUnknown":error.effect_unknown}))).into_response()
}
fn invalid(message: impl ToString) -> Response {
    error(input_error(message))
}
fn input_error(message: impl ToString) -> BrowserError {
    BrowserError {
        code: "BROWSER_INPUT".into(),
        message: message.to_string(),
        effect_unknown: false,
    }
}
async fn locate(
    state: &ServiceState,
    p: &Principal,
    path: &str,
) -> Result<lingxi_adapters::storage::browser::BrowserAuthority, BrowserError> {
    let sid = crate::session_locator::resolve(state.storage(), None, Some(path))
        .await
        .map_err(input_error)?;
    state.browsers().authorize(p, &sid).await
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionWrite {
    session_path: String,
    request_id: Option<String>,
    revoke: Option<bool>,
}
fn operation(
    state: &ServiceState,
    p: &Principal,
    request: Option<&str>,
) -> Result<String, BrowserError> {
    let id = request
        .map(String::from)
        .unwrap_or_else(|| state.request_ids().next_request_id());
    if id.is_empty() || id.len() > 1024 || id.chars().any(char::is_control) {
        return Err(input_error("invalid browser requestId"));
    }
    Ok(format!(
        "human-browser-{}",
        lingxi_protocol::canon::sha256_hex(json!([p.user_id, id]).to_string().as_bytes())
    ))
}
async fn workspaces(
    State(state): State<ServiceState>,
    Extension(p): Extension<Principal>,
) -> Response {
    match state.browsers().workspaces(&p).await {
        Ok(workspaces) => Json(json!({"workspaces":workspaces})).into_response(),
        Err(e) => error(e),
    }
}
async fn sessions(
    State(state): State<ServiceState>,
    Extension(p): Extension<Principal>,
) -> Response {
    match state.browsers().workspaces(&p).await {
        Ok(workspaces) => {
            let mut map = serde_json::Map::new();
            for w in workspaces {
                if let Some(url) = w.url.filter(|url| !url.is_empty()) {
                    map.insert(w.session_path, json!(url));
                }
            }
            Json(map).into_response()
        }
        Err(e) => error(e),
    }
}
async fn states(State(state): State<ServiceState>, Extension(p): Extension<Principal>) -> Response {
    match state.browsers().workspaces(&p).await {
        Ok(workspaces) => {
            let mut map = serde_json::Map::new();
            for w in workspaces {
                map.insert(w.session_path,json!({"url":w.url,"running":w.running,"resumable":w.resumable,"unavailableReason":w.unavailable_reason,"executionNode":w.execution_node,"revoked":w.revoked}));
            }
            Json(map).into_response()
        }
        Err(e) => error(e),
    }
}
async fn open(
    State(state): State<ServiceState>,
    Extension(p): Extension<Principal>,
    Json(body): Json<SessionWrite>,
) -> Response {
    let row = match locate(&state, &p, &body.session_path).await {
        Ok(row) => row,
        Err(e) => return error(e),
    };
    let live = match state.browsers().port().workspaces() {
        Ok(value) => value,
        Err(e) => return error(e),
    };
    let existing = live
        .iter()
        .any(|w| w.session_id == row.session_id && w.running);
    if !existing && row.state.as_ref().is_none_or(|s| s.tabs.is_empty()) {
        return Json(json!({"ok":true,"resume":{"status":"skipped","reason":"no_browser_state","canResume":false,"executionNode":row.execution_node}})).into_response();
    }
    let op = match operation(&state, &p, body.request_id.as_deref()) {
        Ok(op) => op,
        Err(e) => return error(e),
    };
    match state
        .browsers()
        .command(
            row,
            BrowserCommand::new(BrowserAction::Resume),
            true,
            op,
            None,
        )
        .await
    {
        Ok(reply) => {
            let running = reply["state"]["running"].as_bool().unwrap_or(false);
            Json(json!({"ok":true,"resume":{"status":if running {"resumed"}else{"attempted"},"found":running,"running":running,"url":reply["state"]["url"],"executionNode":reply["state"]["executionNode"]},"state":reply["state"]})).into_response()
        }
        Err(e) => error(e),
    }
}
async fn close(
    State(state): State<ServiceState>,
    Extension(p): Extension<Principal>,
    Json(body): Json<SessionWrite>,
) -> Response {
    let row = match locate(&state, &p, &body.session_path).await {
        Ok(row) => row,
        Err(e) => return error(e),
    };
    let op = match operation(&state, &p, body.request_id.as_deref()) {
        Ok(op) => op,
        Err(e) => return error(e),
    };
    if let Err(e) = state
        .browsers()
        .close(row, body.revoke.unwrap_or(false), op)
        .await
    {
        return error(e);
    }
    match state.browsers().workspaces(&p).await {
        Ok(workspaces) => {
            let mut map = serde_json::Map::new();
            for w in workspaces {
                map.insert(w.session_path,json!({"url":w.url,"running":w.running,"resumable":w.resumable,"unavailableReason":w.unavailable_reason,"executionNode":w.execution_node,"revoked":w.revoked}));
            }
            Json(json!({"ok":true,"sessions":map})).into_response()
        }
        Err(e) => error(e),
    }
}
async fn command(
    State(state): State<ServiceState>,
    Extension(p): Extension<Principal>,
    Json(mut body): Json<Value>,
) -> Response {
    let Some(object) = body.as_object_mut() else {
        return invalid("browser command must be an object");
    };
    let Some(path) = object
        .remove("sessionPath")
        .and_then(|v| v.as_str().map(String::from))
    else {
        return invalid("explicit sessionPath required");
    };
    let request = match object.remove("requestId") {
        None => None,
        Some(Value::String(id)) => Some(id),
        _ => return invalid("requestId must be string"),
    };
    let command: BrowserCommand = match serde_json::from_value(body) {
        Ok(c) => c,
        Err(e) => return invalid(e),
    };
    let row = match locate(&state, &p, &path).await {
        Ok(row) => row,
        Err(e) => return error(e),
    };
    let op = match operation(&state, &p, request.as_deref()) {
        Ok(op) => op,
        Err(e) => return error(e),
    };
    match state.browsers().command(row, command, true, op, None).await {
        Ok(reply) => Json(reply).into_response(),
        Err(e) => error(e),
    }
}
async fn preferences(State(state): State<ServiceState>) -> Response {
    match state.browsers().settings() {
        Ok(settings) => Json(json!({"browser":settings})).into_response(),
        Err(e) => invalid(e),
    }
}
async fn configure(State(state): State<ServiceState>, Json(patch): Json<Value>) -> Response {
    match state.browsers().configure(&patch).await {
        Ok(settings) => Json(json!({"ok":true,"browser":settings})).into_response(),
        Err(e) => error(e),
    }
}
async fn clear(State(state): State<ServiceState>) -> Response {
    match state.browsers().clear_site_data().await {
        Ok(()) => Json(json!({"ok":true})).into_response(),
        Err(e) => error(e),
    }
}
pub fn router() -> Router<ServiceState> {
    Router::new()
        .route("/lingxi/v1/browser/workspaces", get(workspaces))
        .route("/lingxi/v1/browser/sessions", get(sessions))
        .route("/lingxi/v1/browser/session-states", get(states))
        .route("/lingxi/v1/browser/open-session", post(open))
        .route("/lingxi/v1/browser/close-session", post(close))
        .route("/lingxi/v1/browser/command", post(command))
        .route(
            "/lingxi/v1/preferences/browser",
            get(preferences).put(configure),
        )
        .route("/lingxi/v1/preferences/browser/clear-cookies", post(clear))
}
