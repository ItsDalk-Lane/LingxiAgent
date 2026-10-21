//! R06 用户偏好读写对（server/routes/preferences.ts 的端口）：
//! 自动学习（`autolearn`，默认开）与快捷聊天窗口（`quick_chat`）。
//! 存储与 workspace-rollback 同源的 user/preferences.json，合并写入保留
//! 其他键；读取不回退文件语义（显式值或默认，不写盘）。
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use crate::memory::MemoryService;
use crate::{EndpointError, ServiceState};

const DEFAULT_SHORTCUT: &str = "Alt+Space";
const DEFAULT_REUSE_TIMEOUT_MINUTES: i64 = 10;
const MAX_REUSE_TIMEOUT_MINUTES: i64 = 120;

impl MemoryService {
    #[allow(clippy::result_large_err)]
    fn read_root(&self) -> Result<Value, Response> {
        // 复用 agents_admin 的严格读取口径：缺失为空对象，损坏显式 500。
        self.read_preferences_root().map_err(|message| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": message })),
            )
                .into_response()
        })
    }

    #[allow(clippy::result_large_err)]
    fn merge_write(&self, key: &str, patch: Value) -> Result<Value, Response> {
        let _lock = crate::agents_admin::preferences_locked();
        let mut root = self.read_root()?;
        if !patch.is_object() {
            return Err(
                EndpointError::invalid_message(format!("{key} must be an object")).into_response(),
            );
        }
        let current = root.get(key).cloned().unwrap_or_else(|| json!({}));
        if !current.is_object() {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("saved {key} must be an object") })),
            )
                .into_response());
        }
        let mut merged = current;
        if let (Some(target), Some(source)) = (merged.as_object_mut(), patch.as_object()) {
            for (field, value) in source {
                target.insert(field.clone(), value.clone());
            }
        }
        root[key] = merged.clone();
        self.write_preferences_root(&root).map_err(|message| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": message })),
            )
                .into_response()
        })?;
        Ok(merged)
    }

    /// 自动学习开关读取：显式布尔优先，默认 enabled=true；不写盘。
    #[allow(clippy::result_large_err)]
    pub fn autolearn_preferences(&self) -> Result<Value, Response> {
        let root = self.read_root()?;
        let enabled = root["autolearn"]["enabled"].as_bool().unwrap_or(true);
        Ok(json!({ "enabled": enabled }))
    }

    /// 合并写入自动学习开关并返回新状态。
    #[allow(clippy::result_large_err)]
    pub fn set_autolearn_preferences(&self, patch: Value) -> Result<Value, Response> {
        if let Some(enabled) = patch.get("enabled") {
            if !enabled.is_boolean() {
                return Err(
                    EndpointError::invalid_message("enabled must be a boolean").into_response()
                );
            }
        }
        self.merge_write("autolearn", patch)
    }

    /// 新会话默认权限写面（R07 收口）：校验闭集词汇后写
    /// session_permission_mode_default，返回规范化后的实际值。
    #[allow(clippy::result_large_err)]
    pub fn set_session_permission_default(&self, patch: Value) -> Result<Value, Response> {
        let mode = patch
            .get("permissionMode")
            .or_else(|| patch.get("sessionPermissionDefault"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                EndpointError::invalid_message("permissionMode is required").into_response()
            })?;
        if !matches!(mode, "auto" | "operate" | "ask" | "read_only") {
            return Err(EndpointError::invalid_message(
                "permissionMode must be auto|operate|ask|read_only",
            )
            .into_response());
        }
        let _lock = crate::agents_admin::preferences_locked();
        let mut root = self.read_root()?;
        root["session_permission_mode_default"] = json!(mode);
        self.write_preferences_root(&root).map_err(|message| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": message })),
            )
                .into_response()
        })?;
        Ok(json!({ "permissionMode": mode }))
    }

    /// 目标预算为全局偏好；只读时返回默认值，不生成偏好文件。
    #[allow(clippy::result_large_err)]
    pub fn goal_preferences(&self) -> Result<Value, Response> {
        let root = self.read_root()?;
        match root.get("goal") {
            None => Ok(
                json!({"enabled":true,"default_token_budget":null,"default_time_budget_minutes":null}),
            ),
            Some(value) if value.is_object() => Ok(value.clone()),
            Some(_) => Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":"saved goal must be an object"})),
            )
                .into_response()),
        }
    }

    #[allow(clippy::result_large_err)]
    pub fn set_goal_preferences(&self, patch: Value) -> Result<Value, Response> {
        if patch
            .get("enabled")
            .is_some_and(|value| !value.is_boolean())
        {
            return Err(
                EndpointError::invalid_message("goal enabled must be a boolean").into_response(),
            );
        }
        self.merge_write("goal", patch)
    }

    fn normalize_quick_chat(source: &Value) -> Value {
        let shortcut = source
            .get("shortcut")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_SHORTCUT);
        let raw_timeout = source
            .get("reuseTimeoutMinutes")
            .or_else(|| source.get("reuse_timeout_minutes"))
            .and_then(Value::as_i64);
        let reuse = raw_timeout
            .map(|value| value.clamp(0, MAX_REUSE_TIMEOUT_MINUTES))
            .unwrap_or(DEFAULT_REUSE_TIMEOUT_MINUTES);
        json!({ "shortcut": shortcut, "reuseTimeoutMinutes": reuse })
    }

    #[allow(clippy::result_large_err)]
    pub fn quick_chat_preferences(&self) -> Result<Value, Response> {
        let root = self.read_root()?;
        Ok(Self::normalize_quick_chat(&root["quick_chat"]))
    }

    #[allow(clippy::result_large_err)]
    pub fn set_quick_chat_preferences(&self, patch: Value) -> Result<Value, Response> {
        let merged = self.merge_write("quick_chat", patch)?;
        Ok(Self::normalize_quick_chat(&merged))
    }
}

#[allow(clippy::result_large_err)]
fn body_object(body: axum::body::Bytes, wrapper: &str) -> Result<Value, Response> {
    if body.is_empty() {
        return Err(EndpointError::invalid_message("request body is required").into_response());
    }
    let payload: Value = serde_json::from_slice(&body).map_err(|err| {
        EndpointError::invalid_message(format!("invalid JSON body: {err}")).into_response()
    })?;
    if let Some(inner) = payload.get(wrapper).filter(|value| value.is_object()) {
        return Ok(inner.clone());
    }
    if payload.is_object() {
        return Ok(payload);
    }
    Err(EndpointError::invalid_message(format!("{wrapper} must be an object")).into_response())
}

macro_rules! pref_pair {
    ($get:ident, $put:ident, $get_wrapper:expr, $put_wrapper:expr, $read:ident, $write:ident) => {
        pub async fn $get(State(state): State<ServiceState>) -> Response {
            match state.memory().$read() {
                Ok(value) => Json(json!({ $get_wrapper: value })).into_response(),
                Err(response) => response,
            }
        }

        pub async fn $put(State(state): State<ServiceState>, body: axum::body::Bytes) -> Response {
            let patch = match body_object(body, $put_wrapper) {
                Ok(value) => value,
                Err(response) => return response,
            };
            match state.memory().$write(patch) {
                Ok(value) => Json(json!({ "ok": true, $put_wrapper: value })).into_response(),
                Err(response) => response,
            }
        }
    };
}

pref_pair!(
    autolearn_get_route,
    autolearn_put_route,
    "autolearn",
    "autolearn",
    autolearn_preferences,
    set_autolearn_preferences
);
pref_pair!(
    quick_chat_get_route,
    quick_chat_put_route,
    "quickChat",
    "quickChat",
    quick_chat_preferences,
    set_quick_chat_preferences
);

pref_pair!(
    goal_get_route,
    goal_put_route,
    "goal",
    "goal",
    goal_preferences,
    set_goal_preferences
);

/// 新会话默认权限读面：复用现役规范化默认 auto，坏文件显式失败，不写入偏好。
pub async fn session_permission_default_get_route(State(state): State<ServiceState>) -> Response {
    match state.memory().read_preferences_root() {
        Ok(root) => {
            let raw = root["session_permission_mode_default"]
                .as_str()
                .unwrap_or("auto");
            let mode = if matches!(raw, "auto" | "operate" | "ask" | "read_only") {
                raw
            } else {
                "auto"
            };
            Json(json!({"permissionMode":mode})).into_response()
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error":error})),
        )
            .into_response(),
    }
}

/// 新会话默认权限写面（R07 收口）：合法闭集写入并返回规范化值。
pub async fn session_permission_default_put_route(
    State(state): State<ServiceState>,
    body: axum::body::Bytes,
) -> Response {
    let patch = match body_object(body, "permissionMode") {
        Ok(value) => value,
        Err(response) => return response,
    };
    match state.memory().set_session_permission_default(patch) {
        Ok(value) => {
            Json(json!({ "ok": true, "permissionMode": value["permissionMode"] })).into_response()
        }
        Err(response) => response,
    }
}

// 与 shared/keybindings-preferences.cjs 的注册表和规范一致；只存显式覆盖，PUT全量替换。
const KEYBINDING_DEFAULTS: &[(&str, &str)] = &[
    ("quick-chat.toggle", "Alt+Space"),
    ("app.restart", "CommandOrControl+Alt+R"),
    ("app.open-settings", "CommandOrControl+,"),
    ("app.new-session", "CommandOrControl+N"),
    ("app.toggle-sidebar", "CommandOrControl+Shift+S"),
    ("voice.record-toggle", "CommandOrControl+Shift+M"),
];
fn preference_error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error":message}))).into_response()
}
fn js_trim(text: &str) -> &str {
    text.trim_matches(|c| matches!(c, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'))
}
fn normalize_binding(value: &Value) -> String {
    let Some(text) = value.as_str() else {
        return String::new();
    };
    let raw = text.replace('\u{00a0}', " ");
    if js_trim(&raw).is_empty() {
        return String::new();
    }
    raw.split('+')
        .filter_map(|part| {
            if part == "Spacebar" || (!part.is_empty() && js_trim(part).is_empty()) {
                return Some("Space".to_owned());
            }
            let part = js_trim(part);
            if part.is_empty() {
                return None;
            }
            let normalized = match part.to_ascii_lowercase().as_str() {
                "shift" => "Shift",
                "control" | "ctrl" => "Control",
                "alt" | "option" => "Alt",
                "meta" => "Meta",
                "cmd" | "command" => "Command",
                "commandorcontrol" | "cmdorctrl" | "ctrlorcommand" => "CommandOrControl",
                "space" => "Space",
                _ if part == "Esc" => "Escape",
                _ => "",
            };
            Some(if !normalized.is_empty() {
                normalized.to_owned()
            } else if part.encode_utf16().count() == 1 {
                part.to_uppercase()
            } else {
                part.to_owned()
            })
        })
        .collect::<Vec<_>>()
        .join("+")
}
fn normalize_keybindings(source: &Value) -> Value {
    let mut result = serde_json::Map::new();
    for (id, default) in KEYBINDING_DEFAULTS {
        let Some(raw) = source.get(*id).filter(|v| !v.is_null()) else {
            continue;
        };
        let values = raw.as_array().cloned().unwrap_or_else(|| vec![raw.clone()]);
        let mut keys = Vec::new();
        for value in values {
            let key = normalize_binding(&value);
            if !key.is_empty() && !keys.contains(&key) {
                keys.push(key);
            }
            if keys.len() == 3 {
                break;
            }
        }
        if keys.len() != 1 || keys[0] != *default {
            result.insert((*id).to_owned(), json!(keys));
        }
    }
    Value::Object(result)
}
fn sidebar_ids(source: &Value) -> Value {
    let mut ids = Vec::<String>::new();
    if let Some(values) = source.as_array() {
        for value in values {
            let Some(id) = value
                .as_str()
                .map(js_trim)
                .filter(|id| !id.is_empty() && id.encode_utf16().count() <= 240)
            else {
                continue;
            };
            if !ids.iter().any(|existing| existing == id) {
                ids.push(id.to_owned());
            }
            if ids.len() == 256 {
                break;
            }
        }
    }
    json!(ids)
}
fn normalize_sidebar(source: &Value) -> Value {
    json!({"projectView":{
        "collapsedProjectIds":sidebar_ids(&source["projectView"]["collapsedProjectIds"]),
        "collapsedFolderIds":sidebar_ids(&source["projectView"]["collapsedFolderIds"]),
        "showAllProjectIds":sidebar_ids(&source["projectView"]["showAllProjectIds"])
    },"sessionList":{"rowMode":if source["sessionList"]["rowMode"] == "single-line" {"single-line"}else{"two-line"}}})
}
fn merge_sidebar(current: &Value, patch: &Value) -> Value {
    let mut value = normalize_sidebar(current);
    for field in [
        "collapsedProjectIds",
        "collapsedFolderIds",
        "showAllProjectIds",
    ] {
        if let Some(raw) = patch.get("projectView").and_then(|v| v.get(field)) {
            value["projectView"][field] = sidebar_ids(raw);
        }
    }
    if let Some(mode) = patch
        .get("sessionList")
        .and_then(|v| v.get("rowMode"))
        .and_then(Value::as_str)
    {
        if matches!(mode, "single-line" | "two-line") {
            value["sessionList"]["rowMode"] = json!(mode);
        }
    }
    value
}
#[derive(Clone, Copy)]
enum InterfacePreference {
    Keybindings,
    Sidebar,
}
impl InterfacePreference {
    fn key(self) -> &'static str {
        match self {
            Self::Keybindings => "keybindings",
            Self::Sidebar => "sidebar_ui",
        }
    }
    fn scope(self) -> &'static str {
        match self {
            Self::Keybindings => "settings.write",
            Self::Sidebar => "chat",
        }
    }
    fn wrapper(self) -> &'static str {
        match self {
            Self::Keybindings => "keybindings",
            Self::Sidebar => "sidebarUi",
        }
    }
    fn normalize(self, value: &Value) -> Value {
        match self {
            Self::Keybindings => normalize_keybindings(value),
            Self::Sidebar => normalize_sidebar(value),
        }
    }
}
fn current_preference_authority(
    state: &ServiceState,
    principal: &crate::auth::Principal,
    scope: &str,
) -> bool {
    principal.has_scope(scope)
        && state.auth().validate_principal(principal).is_ok()
        && (principal.credential_kind != crate::auth::CredentialKind::WebSession
            || state
                .management
                .web_session_current(principal, state.clock.now_unix_ms(), true)
                .unwrap_or(false))
}
impl MemoryService {
    // 读写同一MemoryService/preferences锁和原子写口，绝不另建偏好文件或持久化缓存。
    #[allow(clippy::result_large_err)]
    fn interface_preferences(&self, kind: InterfacePreference) -> Result<Value, Response> {
        let root = self.read_root()?;
        let raw = root
            .get(kind.key())
            .filter(|v| !v.is_null())
            .cloned()
            .unwrap_or_else(|| json!({}));
        if !raw.is_object() {
            return Err(preference_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "saved interface preferences must be an object",
            ));
        }
        Ok(kind.normalize(&raw))
    }
    #[allow(clippy::result_large_err)]
    fn set_interface_preferences(
        &self,
        kind: InterfacePreference,
        patch: Value,
        authorized: impl Fn() -> bool,
    ) -> Result<Value, Response> {
        let _lock = crate::agents_admin::preferences_locked();
        let revoked = || preference_error(StatusCode::FORBIDDEN, "preference_authority_revoked");
        if !authorized() {
            return Err(revoked());
        }
        let mut root = self.read_root()?;
        let current = root
            .get(kind.key())
            .filter(|v| !v.is_null())
            .cloned()
            .unwrap_or_else(|| json!({}));
        if !current.is_object() {
            return Err(preference_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "saved interface preferences must be an object",
            ));
        }
        let value = match kind {
            InterfacePreference::Keybindings => normalize_keybindings(&patch),
            InterfacePreference::Sidebar => merge_sidebar(&current, &patch),
        };
        root[kind.key()] = value.clone();
        if !authorized() {
            return Err(revoked());
        }
        self.write_preferences_root_authorized(&root, &authorized)
            .map_err(|_| {
                if !authorized() {
                    revoked()
                } else {
                    preference_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "cannot save interface preferences",
                    )
                }
            })?;
        Ok(value)
    }
}
#[allow(clippy::result_large_err)]
fn interface_body(body: axum::body::Bytes, kind: InterfacePreference) -> Result<Value, Response> {
    let value: Value = serde_json::from_slice(&body)
        .map_err(|_| preference_error(StatusCode::BAD_REQUEST, "invalid JSON body"))?;
    let value = value.get(kind.wrapper()).unwrap_or(&value);
    if !value.is_object() {
        return Err(preference_error(
            StatusCode::BAD_REQUEST,
            "interface preferences must be an object",
        ));
    }
    if matches!(kind, InterfacePreference::Sidebar) {
        for key in ["projectView", "sessionList"] {
            if value.get(key).is_some_and(|v| !v.is_object()) {
                return Err(preference_error(
                    StatusCode::BAD_REQUEST,
                    "sidebar preference sections must be objects",
                ));
            }
        }
    }
    Ok(value.clone())
}
fn interface_response(kind: InterfacePreference, value: Value, saved: bool) -> Response {
    // 统一协议类型限定返回字段；额外的偏好/秘密绝不顺着整个root返回。
    match kind {
        InterfacePreference::Keybindings => match serde_json::from_value(value) {
            Ok(keybindings) => {
                if saved {
                    Json(lingxi_protocol::KeybindingsUpdate {
                        ok: true,
                        keybindings,
                    })
                    .into_response()
                } else {
                    Json(lingxi_protocol::KeybindingsPreferences { keybindings }).into_response()
                }
            }
            Err(_) => preference_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid normalized keybindings",
            ),
        },
        InterfacePreference::Sidebar => match serde_json::from_value(value) {
            Ok(sidebar_ui) => {
                if saved {
                    Json(lingxi_protocol::SidebarUiUpdate {
                        ok: true,
                        sidebar_ui,
                    })
                    .into_response()
                } else {
                    Json(lingxi_protocol::SidebarUiResponse { sidebar_ui }).into_response()
                }
            }
            Err(_) => preference_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid normalized sidebar preferences",
            ),
        },
    }
}
async fn interface_get(
    state: ServiceState,
    principal: crate::auth::Principal,
    kind: InterfacePreference,
) -> Response {
    let scope = if matches!(kind, InterfacePreference::Keybindings) {
        "settings.read"
    } else {
        "chat"
    };
    if !current_preference_authority(&state, &principal, scope) {
        return preference_error(StatusCode::FORBIDDEN, "preference_authority_revoked");
    }
    match state.memory().interface_preferences(kind) {
        Ok(value) if current_preference_authority(&state, &principal, scope) => {
            interface_response(kind, value, false)
        }
        Ok(_) => preference_error(StatusCode::FORBIDDEN, "preference_authority_revoked"),
        Err(error) => error,
    }
}
async fn interface_put(
    state: ServiceState,
    principal: crate::auth::Principal,
    kind: InterfacePreference,
    body: axum::body::Bytes,
) -> Response {
    let patch = match interface_body(body, kind) {
        Ok(patch) => patch,
        Err(error) => return error,
    };
    // 阻塞文件锁不占HTTP执行线程；排队后与写入前都核对原主体和原scope。
    match tokio::task::spawn_blocking(move || {
        state
            .memory()
            .set_interface_preferences(kind, patch, || {
                current_preference_authority(&state, &principal, kind.scope())
            })
            .map_err(Box::new)
    })
    .await
    {
        Ok(Ok(value)) => interface_response(kind, value, true),
        Ok(Err(error)) => *error,
        Err(_) => preference_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "interface preference task failed",
        ),
    }
}
pub async fn keybindings_get_route(
    State(state): State<ServiceState>,
    axum::Extension(principal): axum::Extension<crate::auth::Principal>,
) -> Response {
    interface_get(state, principal, InterfacePreference::Keybindings).await
}
pub async fn keybindings_put_route(
    State(state): State<ServiceState>,
    axum::Extension(principal): axum::Extension<crate::auth::Principal>,
    body: axum::body::Bytes,
) -> Response {
    interface_put(state, principal, InterfacePreference::Keybindings, body).await
}
pub async fn sidebar_ui_get_route(
    State(state): State<ServiceState>,
    axum::Extension(principal): axum::Extension<crate::auth::Principal>,
) -> Response {
    interface_get(state, principal, InterfacePreference::Sidebar).await
}
pub async fn sidebar_ui_put_route(
    State(state): State<ServiceState>,
    axum::Extension(principal): axum::Extension<crate::auth::Principal>,
    body: axum::body::Bytes,
) -> Response {
    interface_put(state, principal, InterfacePreference::Sidebar, body).await
}
