//! 电脑操作偏好与当前服务进程的实际系统权限；权限就绪不代表控制引擎可用。
use crate::ServiceState;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
fn failure(status: StatusCode, message: impl std::fmt::Display) -> Response {
    (status, Json(json!({"error":message.to_string()}))).into_response()
}
fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(windows) {
        "win32"
    } else {
        "linux"
    }
}
fn default_provider() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("macos:cua")
    } else if cfg!(windows) {
        Some("windows:uia")
    } else {
        None
    }
}
fn settings(root: &Value) -> Result<Value, String> {
    let mut v = root
        .get("computer_use")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let o = v
        .as_object_mut()
        .ok_or("computer_use settings must be an object")?;
    o.entry("enabled").or_insert(json!(false));
    if !o["enabled"].is_boolean() {
        return Err("enabled must be boolean".into());
    }
    o.entry("provider_by_platform")
        .or_insert(json!({"darwin":"macos:cua","win32":"windows:uia","linux":"mock"}));
    o.entry("allow_windows_input_injection")
        .or_insert(json!(false));
    o.entry("app_approvals").or_insert(json!([]));
    if !o["app_approvals"].is_array() {
        return Err("app_approvals must be array".into());
    }
    if default_provider().is_none() {
        o.insert("enabled".into(), json!(false));
    }
    Ok(v)
}
fn selected(s: &Value) -> Option<String> {
    default_provider().map(|fallback| {
        s["provider_by_platform"][platform()]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(fallback)
            .to_owned()
    })
}
pub fn system_permissions() -> Vec<Value> {
    #[cfg(target_os = "macos")]
    {
        vec![
            json!({"name":"accessibility","granted":native::accessibility(false)}),
            json!({"name":"screenRecording","granted":native::screen(false)}),
        ]
    }
    #[cfg(not(target_os = "macos"))]
    {
        vec![]
    }
}
pub(crate) fn projection(state: &ServiceState) -> Result<Value, String> {
    let s = settings(&state.memory().read_preferences_root()?)?;
    let id = selected(&s);
    let providers=id.as_ref().map(|id|vec![json!({"providerId":id,"status":{"available":false,"reason":"computer_control_engine_unavailable","permissions":system_permissions(),"permissionSubject":"service_process"}})]).unwrap_or_default();
    Ok(
        json!({"settings":s,"selectedProviderId":id,"status":{"enabled":s["enabled"],"platform":platform(),"supported":default_provider().is_some(),"selectedProviderId":id,"providers":providers,"activeLease":null}}),
    )
}
pub async fn get_route(State(s): State<ServiceState>) -> Response {
    match projection(&s) {
        Ok(v) => Json(v).into_response(),
        Err(e) => failure(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

fn save_enabled(
    memory: &crate::memory::MemoryService,
    events: &crate::appevents::AppEventLog,
    enabled: bool,
) -> Result<Value, String> {
    if default_provider().is_none() {
        return Err("Computer Use is not supported on Linux Preview.".into());
    }
    let next = {
        let _guard = crate::agents_admin::preferences_locked();
        let mut root = memory.read_preferences_root()?;
        let mut next = settings(&root)?;
        next["enabled"] = json!(enabled);
        root["computer_use"] = next.clone();
        memory.write_preferences_root(&root)?;
        next
    };
    events.publish(
        "computer-use-settings-changed",
        json!({"selectedProviderId":selected(&next)}),
    );
    Ok(next)
}

pub(crate) fn tool_enabled(memory: &crate::memory::MemoryService) -> Result<bool, String> {
    Ok(settings(&memory.read_preferences_root()?)?["enabled"] == true)
}

/// 只能消费真实网关调用身份；不批准应用，不请求或提升操作系统授权。
pub(crate) fn tool_set_enabled(
    memory: &crate::memory::MemoryService,
    events: &crate::appevents::AppEventLog,
    auth: &crate::auth::AuthService,
    ctx: &lingxi_kernel::RunContext,
    call: &lingxi_protocol::ToolCallId,
    enabled: bool,
) -> Result<Value, String> {
    let principal = auth.mint_local_settings_tool_principal(ctx, call)?;
    if !principal.has_scope("settings.write") {
        return Err("settings.write is required".into());
    }
    save_enabled(memory, events, enabled)
}
pub async fn put_route(
    State(s): State<ServiceState>,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if default_provider().is_none() {
        return failure(
            StatusCode::BAD_REQUEST,
            "Computer Use is not supported on Linux Preview.",
        );
    }
    let Json(body) = match body {
        Ok(v) => v,
        Err(e) => return failure(StatusCode::BAD_REQUEST, e),
    };
    let Some(patch) = body.get("settings").unwrap_or(&body).as_object() else {
        return failure(StatusCode::BAD_REQUEST, "settings must be object");
    };
    // 此页面只保存启用偏好，不增删应用批准，也不提升输入权限。
    if patch.keys().any(|k| k != "enabled") || patch.get("enabled").is_none_or(|v| !v.is_boolean())
    {
        return failure(
            StatusCode::BAD_REQUEST,
            "only boolean enabled may be changed here",
        );
    }
    let next = match save_enabled(s.memory(), s.app_events(), patch["enabled"] == true) {
        Ok(value) => value,
        Err(error) => return failure(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    Json(json!({"ok":true,"settings":next})).into_response()
}
pub async fn request_route(
    State(s): State<ServiceState>,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(v) => v,
        Err(e) => return failure(StatusCode::BAD_REQUEST, e),
    };
    if !body.is_object() {
        return failure(StatusCode::BAD_REQUEST, "body must be object");
    }
    let snapshot = match projection(&s) {
        Ok(v) => v,
        Err(e) => return failure(StatusCode::INTERNAL_SERVER_ERROR, e),
    };
    let provider = snapshot["selectedProviderId"].as_str();
    if body
        .get("providerId")
        .is_some_and(|v| !v.is_null() && v.as_str() != provider)
    {
        return failure(StatusCode::BAD_REQUEST, "selected provider mismatch");
    }
    if provider != Some("macos:cua") || !cfg!(target_os = "macos") {
        return failure(
            StatusCode::NOT_IMPLEMENTED,
            "system_permission_request_unavailable_for_provider",
        );
    }
    let permissions = match tokio::task::spawn_blocking(|| {
        #[cfg(target_os = "macos")]
        {
            vec![
                json!({"name":"accessibility","granted":native::accessibility(true)}),
                json!({"name":"screenRecording","granted":native::screen(true)}),
            ]
        }
        #[cfg(not(target_os = "macos"))]
        {
            Vec::<Value>::new()
        }
    })
    .await
    {
        Ok(v) => v,
        Err(e) => return failure(StatusCode::INTERNAL_SERVER_ERROR, e),
    };
    s.app_events().publish(
        "computer-use-permissions-requested",
        json!({"providerId":provider}),
    );
    let pending = permissions.iter().any(|p| p["granted"] != true);
    Json(json!({"ok":true,"result":{"providerId":provider,"permissions":permissions,"requestPending":pending,"permissionSubject":"service_process"}})).into_response()
}
#[cfg(target_os = "macos")]
mod native {
    use std::ffi::c_void;
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
    }
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrustedWithOptions(options: *const c_void) -> u8;
        static kAXTrustedCheckOptionPrompt: *const c_void;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFDictionaryCreate(
            allocator: *const c_void,
            keys: *const *const c_void,
            values: *const *const c_void,
            count: isize,
            key_callbacks: *const c_void,
            value_callbacks: *const c_void,
        ) -> *const c_void;
        fn CFRelease(value: *const c_void);
        static kCFBooleanTrue: *const c_void;
    }
    pub fn accessibility(prompt: bool) -> bool {
        // 全局常量整个调用期间有效；字典只借用它们，返回后立刻释放。
        unsafe {
            if !prompt {
                return AXIsProcessTrustedWithOptions(std::ptr::null()) != 0;
            }
            let key = kAXTrustedCheckOptionPrompt;
            let value = kCFBooleanTrue;
            let options = CFDictionaryCreate(
                std::ptr::null(),
                &key,
                &value,
                1,
                std::ptr::null(),
                std::ptr::null(),
            );
            assert!(!options.is_null(), "cannot allocate permission options");
            let trusted = AXIsProcessTrustedWithOptions(options) != 0;
            CFRelease(options);
            trusted
        }
    }
    pub fn screen(prompt: bool) -> bool {
        // 弹出申请不等于已经授权，系统返回值保持原样。
        unsafe {
            if prompt {
                CGRequestScreenCaptureAccess()
            } else {
                CGPreflightScreenCaptureAccess()
            }
        }
    }
}

/// 应用批准记录沿用服务器设置面；实验页面只展示，不提供这两个写操作。
pub async fn approve_route(
    State(s): State<ServiceState>,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    approval_change(s, body, true)
}
pub async fn revoke_route(
    State(s): State<ServiceState>,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    approval_change(s, body, false)
}
fn approval_change(
    s: ServiceState,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
    approve: bool,
) -> Response {
    if default_provider().is_none() {
        return failure(
            StatusCode::BAD_REQUEST,
            "Computer Use is not supported on Linux Preview.",
        );
    }
    let Json(body) = match body {
        Ok(v) => v,
        Err(e) => return failure(StatusCode::BAD_REQUEST, e),
    };
    let provider = body["providerId"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let app = body["appId"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let (Some(provider), Some(app)) = (provider, app) else {
        return failure(
            StatusCode::BAD_REQUEST,
            "computer use approval requires providerId and appId",
        );
    };
    let next = {
        let _guard = crate::agents_admin::preferences_locked();
        let mut root = match s.memory().read_preferences_root() {
            Ok(v) => v,
            Err(e) => return failure(StatusCode::INTERNAL_SERVER_ERROR, e),
        };
        let mut next = match settings(&root) {
            Ok(v) => v,
            Err(e) => return failure(StatusCode::INTERNAL_SERVER_ERROR, e),
        };
        let approvals = next["app_approvals"].as_array_mut().expect("已验证列表");
        approvals.retain(|v| v["providerId"] != provider || v["appId"] != app);
        if approve {
            let at = body["approvedAt"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    chrono::DateTime::from_timestamp_millis(s.clock.now_unix_ms() as i64)
                        .unwrap_or_default()
                        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
                });
            approvals.push(json!({"providerId":provider,"appId":app,"appName":body["appName"].as_str().unwrap_or("").trim(),"scope":if body["scope"]=="window"{"window"}else{"app"},"approvedAt":at}));
        }
        root["computer_use"] = next.clone();
        if let Err(e) = s.memory().write_preferences_root(&root) {
            return failure(StatusCode::INTERNAL_SERVER_ERROR, e);
        }
        next
    };
    s.app_events().publish(
        "computer-use-settings-changed",
        json!({"providerId":provider,"appId":app}),
    );
    Json(json!({"ok":true,"settings":next})).into_response()
}
