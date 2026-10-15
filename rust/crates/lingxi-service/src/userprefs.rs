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
            Json(json!({"error":error})),
        )
            .into_response(),
    }
}
