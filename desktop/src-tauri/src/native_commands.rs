//! R09-T03/T04 命令面：把 `native::install::INVOKE_COMMANDS` 的纯逻辑接到 Tauri
//! 命令边界（T03_T04_REGISTRATION.md §2.4/§4.5 的签名契约）。
//!
//! 约定：
//! - 每个命令先过 native 的窗口/参数门（`authorize_*` / `plan_*`），再做真实副作用；
//! - 不做静默 no-op：无法真实执行的平台动作返回显式错误（T03/T07 补齐）；
//! - state 一律 `NativeState`（native::install 在 setup 中 manage）+ 本模块
//!   `CommandState`（keep-awake 后端等宿主侧句柄）。
//!
//! 构建门：
//! - 常态命令随 `r09-host` feature 注册（见 lib.rs/build.rs）；
//! - `debug_open_onboarding*` 只在 `r09-test-hooks` feature 构建注册。

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

use crate::commands::HostState;
use crate::native::speech::SpeechPermissionBackend;
use crate::native::{
    app_info, auto_launch, external_open, file_dialogs, global_shortcut, install, keep_awake,
    notification, onboarding, quick_chat, quick_chat_handoff, reload_restart, settings_broadcast,
    settings_window, speech, splash, window_labels, window_lifecycle,
};

/// 宿主侧句柄（避免在 native/* 内新增状态；T06 收口期由本模块持有）。
#[derive(Default)]
pub struct CommandState {
    keep_awake_backend: Mutex<install::SystemKeepAwake>,
}

fn denied(error: impl ToString) -> String {
    error.to_string()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn request_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{:x}{:x}", std::process::id(), nanos)
}

fn role_label(window: &WebviewWindow) -> &str {
    window.label()
}

// ── 生命周期：reload / restart / settings ──────────────────────────────

#[tauri::command]
pub fn reload_main_window(window: WebviewWindow) -> Result<(), String> {
    reload_restart::authorize_reload(role_label(&window)).map_err(denied)?;
    window.reload().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn restart_app(window: WebviewWindow, app: AppHandle) -> Result<Value, String> {
    let plan = reload_restart::authorize_restart(role_label(&window)).map_err(denied)?;
    let decision =
        reload_restart::begin_restart(reload_restart::RestartGuard::default()).map_err(denied)?;
    if plan.accepts_arguments {
        return Err("重启命令不接受渲染层参数".to_string());
    }
    if matches!(decision, reload_restart::RestartDecision::Proceed) {
        // 复用 T02 的退出清理：ExitRequested → service supervisor shutdown，
        // 随后 process::restart 以当前可执行/环境重启（无 renderer argv）。
        app.restart();
    }
    Err("重启决策未进入 Proceed（重入保护）".to_string())
}

#[tauri::command]
pub fn open_settings(
    window: WebviewWindow,
    app: AppHandle,
    tab: Option<String>,
) -> Result<Value, String> {
    // 先按调用窗口角色决策（renderer 不能自报目标窗口），再走 install helper 执行。
    let main_alive = app.get_webview_window(window_labels::MAIN).is_some();
    let settings_alive = app.get_webview_window(window_labels::SETTINGS).is_some();
    let route = settings_window::plan_open_settings(
        role_label(&window),
        tab.as_deref(),
        main_alive,
        false,
        settings_alive,
    )
    .map_err(denied)?;
    install::open_settings_window(&app, tab.as_deref())?;
    Ok(json!({
        "ok": true,
        "target": format!("{:?}", route.target),
        "event": route.event,
    }))
}

#[tauri::command]
pub fn settings_changed(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, HostState>,
    event_type: String,
    data: Value,
) -> Result<Value, String> {
    settings_broadcast::authorize_sender(role_label(&window)).map_err(denied)?;
    let field = |key: &str| data.get(key).and_then(Value::as_str);
    let keep_awake = data
        .get("enabled")
        .and_then(Value::as_bool)
        .or_else(|| data.get("keepAwake").and_then(Value::as_bool));
    let input = settings_broadcast::SettingsEventInput {
        event: &event_type,
        theme: field("theme"),
        keep_awake,
        locale: field("locale"),
        payload_is_object: data.is_object(),
        experiment_id: field("id"),
    };
    let plan = settings_broadcast::plan_broadcast(&input).map_err(denied)?;
    let mut effects: Vec<Value> = Vec::new();
    let mut unavailable: Vec<Value> = Vec::new();
    for effect in &plan.side_effects {
        match effect {
            settings_broadcast::SideEffect::UpdateBrowserViewerTheme { theme } => {
                let _ = app.emit_to(
                    window_labels::BROWSER_VIEWER,
                    "settings-changed",
                    json!({ "type": event_type, "data": data, "theme": theme }),
                );
                effects.push(json!({ "effect": "update-browser-viewer-theme", "theme": theme }));
            }
            settings_broadcast::SideEffect::ApplyKeepAwake { enabled } => {
                // 设置广播携带的 keep-awake 状态必须真实应用到系统，不能只转发。
                if let Some(command_state) = app.try_state::<CommandState>() {
                    if let Some(native) = app.try_state::<install::NativeState>() {
                        let platform = app_info::platform_literal();
                        if let Some(platform) = keep_awake::KeepAwakePlatform::from_str(platform) {
                            let mut backend = command_state
                                .keep_awake_backend
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            let mut machine = native
                                .keep_awake
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            match machine.set_enabled(&mut *backend, *enabled, platform) {
                                Ok(status) => effects.push(json!({
                                    "effect": "apply-keep-awake",
                                    "enabled": status.enabled,
                                    "active": status.active,
                                })),
                                Err(error) => unavailable.push(json!({
                                    "effect": "apply-keep-awake",
                                    "error": error.to_string(),
                                })),
                            }
                        }
                    }
                }
            }
            settings_broadcast::SideEffect::ReregisterQuickChatShortcut => {
                let report = install::register_quick_chat_shortcuts(&app);
                effects.push(json!({
                    "effect": "reregister-quick-chat-shortcut",
                    "ok": report.ok,
                    "shortcut": report.shortcut,
                    "failures": report.failures,
                }));
            }
            settings_broadcast::SideEffect::ApplyNetworkProxy => {
                // Tauri 无 session.setProxy 等价物；显式记录为未接线，不静默假成功。
                if let Some(diagnostics) = state.diagnostics() {
                    diagnostics.log(
                        "settings-effect-unavailable",
                        json!({"effect": "apply-network-proxy", "reason": "tauri-no-session-proxy-api"}),
                    );
                }
                unavailable.push(json!({
                    "effect": "apply-network-proxy",
                    "reason": "tauri-no-session-proxy-api",
                }));
            }
            settings_broadcast::SideEffect::RebuildTrayForLocale { locale } => {
                // install::create_tray 为私有实现；locale 主题重建由 T03 后续暴露 helper。
                if let Some(diagnostics) = state.diagnostics() {
                    diagnostics.log(
                        "settings-effect-unavailable",
                        json!({"effect": "rebuild-tray-locale", "locale": locale}),
                    );
                }
                unavailable.push(json!({
                    "effect": "rebuild-tray-locale",
                    "locale": locale,
                    "reason": "tray-rebuild-helper-not-exposed",
                }));
            }
        }
    }
    let mut fanout = Vec::new();
    for role in &plan.fanout_roles {
        let label = role.as_str();
        if label == window.label() {
            continue;
        }
        if app.get_webview_window(label).is_some() {
            let _ = app.emit_to(
                label,
                "settings-changed",
                json!({ "type": event_type, "data": data }),
            );
            fanout.push(label);
        }
    }
    Ok(json!({
        "ok": unavailable.is_empty(),
        "event": plan.event,
        "sideEffects": effects,
        "unavailable": unavailable,
        "fanout": fanout,
    }))
}

// ── 通知 ────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn show_notification(
    window: WebviewWindow,
    app: AppHandle,
    title: String,
    body: String,
    agent_id: Option<String>,
    options: Option<Value>,
) -> Result<Value, String> {
    let focus_policy = options
        .as_ref()
        .and_then(|value| {
            value
                .get("focusPolicy")
                .or_else(|| value.get("focus_policy"))
        })
        .and_then(Value::as_str);
    let request = notification::validate_notification_request(
        role_label(&window),
        &title,
        &body,
        agent_id.as_deref(),
        focus_policy,
    )
    .map_err(denied)?;
    let focused = app
        .get_webview_window(window_labels::MAIN)
        .and_then(|main| main.is_focused().ok())
        .unwrap_or(false);
    if notification::should_suppress(request.focus_policy, focused) {
        let receipt = notification::suppressed_receipt();
        return Ok(json!({
            "shown": receipt.shown,
            "reason": receipt.reason.map(|reason| reason.as_str()),
            "bodyTruncated": request.body_truncated,
        }));
    }
    match install::show_notification(&app, &request.title, &request.body) {
        Ok(()) => {
            let receipt = notification::shown_receipt();
            Ok(json!({
                "shown": receipt.shown,
                "reason": receipt.reason.map(|reason| reason.as_str()),
                "bodyTruncated": request.body_truncated,
            }))
        }
        Err(error) => {
            let receipt = notification::failed_receipt(notification::ReceiptReason::Failed);
            Ok(json!({
                "shown": receipt.shown,
                "reason": receipt.reason.map(|reason| reason.as_str()),
                "error": error,
                "bodyTruncated": request.body_truncated,
            }))
        }
    }
}

// ── 文件对话框 / 外链 / 拖出 ────────────────────────────────────────────

#[tauri::command]
pub async fn select_skill(window: WebviewWindow) -> Result<Option<String>, String> {
    file_dialogs::authorize_dialog(role_label(&window), file_dialogs::DialogKind::Skill)
        .map_err(denied)?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    window
        .dialog()
        .file()
        .set_parent(&window)
        .add_filter("Skill", &["skill", "zip"])
        .pick_file(move |path| {
            let _ = tx.send(path.map(|path| path.to_string()));
        });
    let picked = rx.await.map_err(|_| "原生技能选择器未返回".to_string())?;
    match picked {
        Some(path) => {
            let selection =
                file_dialogs::normalize_skill_selection(&path, false).map_err(denied)?;
            Ok(Some(selection.path))
        }
        None => Ok(None),
    }
}

#[tauri::command]
pub async fn open_folder(window: WebviewWindow, path: String) -> Result<bool, String> {
    let effect = external_open::plan_open_folder(role_label(&window), &path).map_err(denied)?;
    let target = match effect {
        external_open::OpenEffect::OpenFolder(target) => target,
        _ => return Err("open_folder 计划类型不一致".to_string()),
    };
    tauri::async_runtime::spawn_blocking(move || open::that_detached(&target).map(|()| true))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn show_in_finder(window: WebviewWindow, path: String) -> Result<bool, String> {
    external_open::plan_reveal_in_finder(role_label(&window), &path).map_err(denied)?;
    let target = PathBuf::from(path);
    tauri::async_runtime::spawn_blocking(move || install::reveal_in_finder(&target).map(|()| true))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn start_drag(window: WebviewWindow, paths: Vec<String>) -> Result<Value, String> {
    let payload =
        crate::native::drag_out::plan_start_drag(role_label(&window), &paths).map_err(denied)?;
    // 真实文件拖出（NSDragging/DoDragDrop）尚无 Tauri 等价后端；显式失败，不冒充成功。
    let _ = payload;
    Err("start_drag 的 Tauri 文件拖出后端尚未实现（T03/T07 待补）".to_string())
}

// ── keep-awake / auto-launch ────────────────────────────────────────────

fn keep_awake_json(status: keep_awake::KeepAwakeStatus) -> Value {
    json!({
        "enabled": status.enabled,
        "active": status.active,
        "blockerId": status.blocker_id,
        "kind": status.kind,
    })
}

#[tauri::command]
pub fn get_keep_awake_status(
    window: WebviewWindow,
    native: State<'_, install::NativeState>,
    command: State<'_, CommandState>,
) -> Result<Value, String> {
    keep_awake::authorize_caller(role_label(&window)).map_err(denied)?;
    let backend = command
        .keep_awake_backend
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let machine = native
        .keep_awake
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Ok(keep_awake_json(machine.status(&*backend)))
}

#[tauri::command]
pub fn set_keep_awake_enabled(
    window: WebviewWindow,
    native: State<'_, install::NativeState>,
    command: State<'_, CommandState>,
    enabled: bool,
) -> Result<Value, String> {
    keep_awake::authorize_caller(role_label(&window)).map_err(denied)?;
    let platform_literal = app_info::platform_literal();
    let platform = keep_awake::KeepAwakePlatform::from_str(platform_literal)
        .ok_or_else(|| "当前平台不支持 keep-awake".to_string())?;
    let mut backend = command
        .keep_awake_backend
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut machine = native
        .keep_awake
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let status = machine
        .set_enabled(&mut *backend, enabled, platform)
        .map_err(denied)?;
    Ok(keep_awake_json(status))
}

fn auto_launch_json(status: auto_launch::AutoLaunchStatus) -> Value {
    json!({
        "supported": status.supported,
        "openAtLogin": status.open_at_login,
        "openedAtLogin": status.opened_at_login,
        "status": status.status,
        "executableWillLaunchAtLogin": status.executable_will_launch_at_login,
    })
}

fn auto_launch_status(app: &AppHandle) -> auto_launch::AutoLaunchStatus {
    use tauri_plugin_autostart::ManagerExt;
    let platform = auto_launch::platform_name();
    if !auto_launch::is_supported(platform) {
        return auto_launch::AutoLaunchStatus::unsupported();
    }
    let raw = app.autolaunch().is_enabled().ok();
    let argv: Vec<String> = std::env::args().collect();
    auto_launch::normalize_status(
        platform,
        &argv,
        raw,
        raw.map(|enabled| if enabled { "enabled" } else { "disabled" }),
        None,
        false,
        false,
    )
}

#[tauri::command]
pub fn get_auto_launch_status(window: WebviewWindow, app: AppHandle) -> Result<Value, String> {
    auto_launch::authorize_status_read(role_label(&window)).map_err(denied)?;
    Ok(auto_launch_json(auto_launch_status(&app)))
}

#[tauri::command]
pub fn set_auto_launch_enabled(
    window: WebviewWindow,
    app: AppHandle,
    enabled: bool,
) -> Result<Value, String> {
    auto_launch::authorize_set_enabled(role_label(&window), enabled).map_err(denied)?;
    use tauri_plugin_autostart::ManagerExt;
    let platform = auto_launch::platform_name();
    let plan = auto_launch::plan_set_enabled(platform, enabled);
    let Some(action) = plan.action else {
        return Ok(auto_launch_json(
            auto_launch::AutoLaunchStatus::unsupported(),
        ));
    };
    let manager = app.autolaunch();
    if action {
        manager.enable().map_err(|error| error.to_string())?;
    } else {
        manager.disable().map_err(|error| error.to_string())?;
    }
    // set 后必须重读真实状态（失败不得假报）。
    Ok(auto_launch_json(auto_launch_status(&app)))
}

// ── 全局快捷键 ──────────────────────────────────────────────────────────

fn registration_json(report: global_shortcut::RegistrationResult) -> Value {
    json!({
        "ok": report.ok,
        "shortcut": report.shortcut,
        "shortcuts": report.shortcuts,
        "failures": report.failures,
        "error": report.error,
    })
}

#[tauri::command]
pub fn keybindings_reload_global(window: WebviewWindow, app: AppHandle) -> Result<Value, String> {
    window_labels::require_role(
        role_label(&window),
        &[
            window_labels::WindowRole::Main,
            window_labels::WindowRole::Settings,
        ],
    )
    .map_err(denied)?;
    Ok(registration_json(install::register_quick_chat_shortcuts(
        &app,
    )))
}

#[tauri::command]
pub fn quick_chat_reload_shortcut(window: WebviewWindow, app: AppHandle) -> Result<Value, String> {
    keybindings_reload_global(window, app)
}

#[tauri::command]
pub fn keybindings_test_register(
    window: WebviewWindow,
    app: AppHandle,
    shortcut: String,
) -> Result<Value, String> {
    window_labels::require_role(
        role_label(&window),
        &[
            window_labels::WindowRole::Main,
            window_labels::WindowRole::Settings,
        ],
    )
    .map_err(denied)?;
    let plan = global_shortcut::plan_test_register(&shortcut).map_err(denied)?;
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let registered = app
        .global_shortcut()
        .register(plan.shortcut.as_str())
        .is_ok();
    let cleaned = if registered && plan.must_unregister_after {
        app.global_shortcut()
            .unregister(plan.shortcut.as_str())
            .is_ok()
    } else {
        !registered
    };
    Ok(json!({
        "ok": registered && cleaned,
        "shortcut": plan.shortcut,
        "registered": registered,
        "selfRegistered": plan.already_registered,
    }))
}

#[tauri::command]
pub fn quick_chat_shortcut_status(
    window: WebviewWindow,
    native: State<'_, install::NativeState>,
) -> Result<Value, String> {
    window_labels::require_role(
        role_label(&window),
        &[
            window_labels::WindowRole::Main,
            window_labels::WindowRole::Settings,
        ],
    )
    .map_err(denied)?;
    let registered = native
        .registered_shortcuts
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let status = global_shortcut::shortcut_status(&registered, &registered);
    Ok(json!({
        "ok": true,
        "shortcut": status.shortcut,
        "registered": status.registered,
        "shortcuts": status.shortcuts,
    }))
}

// ── quick-chat 窗口 / 会话交接 ─────────────────────────────────────────

#[tauri::command]
pub fn quick_chat_show(window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    quick_chat::authorize_show(role_label(&window)).map_err(denied)?;
    install::show_quick_chat(&app)
}

#[tauri::command]
pub fn quick_chat_hide(window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    quick_chat::authorize_hide(role_label(&window)).map_err(denied)?;
    install::hide_quick_chat(&app)
}

#[tauri::command]
pub fn quick_chat_resize(
    window: WebviewWindow,
    native: State<'_, install::NativeState>,
    mode: Option<String>,
    height: Option<f64>,
) -> Result<Value, String> {
    quick_chat::authorize_resize(role_label(&window)).map_err(denied)?;
    let request = quick_chat::normalize_resize_request(mode.as_deref(), height);
    let work_area_height = window
        .current_monitor()
        .ok()
        .flatten()
        .map(|monitor| {
            let size = monitor.size();
            let scale = monitor.scale_factor();
            f64::from(size.height) / scale
        })
        .unwrap_or(0.0);
    let target_height = quick_chat::height_for_mode(request.mode, request.height, work_area_height);
    let logical = tauri::LogicalSize::new(
        f64::from(quick_chat::QUICK_CHAT_WIDTH),
        f64::from(target_height),
    );
    window
        .set_size(logical)
        .map_err(|error| error.to_string())?;
    let mut current = native
        .quick_chat_mode
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *current = request.mode;
    let mode_name = match request.mode {
        quick_chat::QuickChatMode::Compact => "compact",
        quick_chat::QuickChatMode::Chat => "chat",
    };
    Ok(json!({
        "ok": true,
        "mode": mode_name,
        "height": target_height,
    }))
}

#[tauri::command]
pub fn quick_chat_open_session(
    window: WebviewWindow,
    native: State<'_, install::NativeState>,
    app: AppHandle,
    target: Value,
) -> Value {
    let handoff_target = quick_chat_handoff::HandoffTarget {
        session_path: target
            .get("sessionPath")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        session_id: target
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_owned),
        owner_key: target
            .get("ownerKey")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    };
    let main_alive = app.get_webview_window(window_labels::MAIN).is_some();
    match quick_chat_handoff::plan_open(
        role_label(&window) == window_labels::QUICK_CHAT,
        main_alive,
        &handoff_target,
        request_id(),
        now_ms(),
    ) {
        Ok(request) => {
            *native
                .handoff
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(request.clone());
            json!({
                "ok": true,
                "requestId": request.request_id,
                "expiresAt": request.expires_at_ms,
            })
        }
        Err(failure) => json!({
            "ok": false,
            "code": failure.code,
            "message": failure.message,
        }),
    }
}

#[tauri::command]
pub fn quick_chat_open_session_ack(
    window: WebviewWindow,
    native: State<'_, install::NativeState>,
    app: AppHandle,
    result: Value,
) -> Value {
    window_labels::require_role(role_label(&window), &[window_labels::WindowRole::Main]).ok();
    let ack = quick_chat_handoff::HandoffAck {
        request_id: result
            .get("requestId")
            .and_then(Value::as_str)
            .map(str::to_owned),
        ok: result.get("ok").and_then(Value::as_bool).unwrap_or(false),
        session_path: result
            .get("sessionPath")
            .and_then(Value::as_str)
            .map(str::to_owned),
        session_id: result
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_owned),
        owner_key: result
            .get("ownerKey")
            .and_then(Value::as_str)
            .map(str::to_owned),
        code: result
            .get("code")
            .and_then(Value::as_str)
            .map(str::to_owned),
        message: result
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_owned),
    };
    let mut guard = native
        .handoff
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(request) = guard.clone() else {
        return json!({"ok": false, "code": "QUICK_CHAT_TARGET_MISMATCH", "message": "没有在途会话交接"});
    };
    let main_alive = app.get_webview_window(window_labels::MAIN).is_some();
    match quick_chat_handoff::decide_ack(&request, true, main_alive, &ack) {
        quick_chat_handoff::AckDecision::Complete { session_id, .. } => {
            install::focus_main(&app);
            install::hide_quick_chat(&app).ok();
            *guard = None;
            json!({"ok": true, "sessionId": session_id})
        }
        quick_chat_handoff::AckDecision::Rejected(failure) => {
            *guard = None;
            json!({"ok": false, "code": failure.code, "message": failure.message})
        }
        quick_chat_handoff::AckDecision::Ignored => json!({
            "ok": false,
            "code": quick_chat_handoff::CODE_TARGET_MISMATCH,
            "message": "会话交接确认未匹配",
        }),
    }
}

// ── splash / onboarding ────────────────────────────────────────────────

fn splash_page_allowed(window: &WebviewWindow) -> Result<(), String> {
    splash::authorize_splash_caller(role_label(window)).map_err(denied)?;
    let url = window.url().map_err(|error| error.to_string())?;
    splash::validate_splash_page_url(url.as_str()).map_err(denied)
}

fn current_agent_id(home: &Path) -> Option<String> {
    let preferences = std::fs::read_to_string(home.join("user/preferences.json")).ok()?;
    let value: Value = serde_json::from_str(&preferences).ok()?;
    for key in ["current_agent_id", "primary_agent_id", "primaryAgent"] {
        if let Some(id) = value.get(key).and_then(Value::as_str) {
            if splash::valid_agent_id(id) {
                return Some(id.to_string());
            }
        }
    }
    None
}

fn read_splash_info(home: &Path) -> splash::SplashInfo {
    let Some(agent_id) = current_agent_id(home) else {
        return splash::SplashInfo::default();
    };
    let config_path = home.join("agents").join(&agent_id).join("config.yaml");
    let Ok(text) = std::fs::read_to_string(config_path) else {
        return splash::SplashInfo::default();
    };
    // config.yaml 的宿主侧读取只做最小字段提取（完整 YAML 解析由服务/后续 crate 处理）；
    // 行级提取失败时回落默认值，与现役 fallback 语义一致。
    let mut name: Option<String> = None;
    let mut locale: Option<String> = None;
    let mut yuan: Option<String> = None;
    let mut in_agent = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("agent:") {
            in_agent = true;
            continue;
        }
        if in_agent && !line.starts_with(' ') && !line.starts_with('\t') {
            in_agent = false;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let value = value.trim().trim_matches('"').trim_matches('\'');
        if value.is_empty() {
            continue;
        }
        match key.trim() {
            "name" if in_agent => name.get_or_insert_with(|| value.to_string()),
            "yuan" if in_agent => yuan.get_or_insert_with(|| value.to_string()),
            "locale" if !in_agent => locale.get_or_insert_with(|| value.to_string()),
            _ => continue,
        };
    }
    splash::normalize_splash_info(name.as_deref(), locale.as_deref(), yuan.as_deref())
}

#[tauri::command]
pub fn get_splash_info(window: WebviewWindow) -> Result<Value, String> {
    splash_page_allowed(&window)?;
    let info = read_splash_info(&install::lingxi_home());
    Ok(json!({
        "agentName": info.agent_name,
        "locale": info.locale,
        "yuan": info.yuan,
    }))
}

#[tauri::command]
pub fn get_avatar_path(
    window: WebviewWindow,
    role: Option<String>,
) -> Result<Option<String>, String> {
    splash_page_allowed(&window)?;
    let role = role.unwrap_or_else(|| "agent".to_string());
    let home = install::lingxi_home();
    let agent_id = if role == "agent" {
        current_agent_id(&home)
    } else {
        None
    };
    let candidates =
        splash::avatar_candidate_paths(&home, &role, agent_id.as_deref()).map_err(denied)?;
    for candidate in candidates {
        let Some(bytes) = read_avatar_file(&home, &candidate) else {
            continue;
        };
        let ext = candidate
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default();
        if !splash::avatar_bytes_match_format(&bytes, ext) {
            continue;
        }
        // webp 需要真实解码；Tauri 侧没有同步解码器，显式跳过（不把伪造文件头交出去）。
        if ext == "webp" {
            continue;
        }
        return Ok(Some(candidate.to_string_lossy().into_owned()));
    }
    Ok(None)
}

/// 逐层拒绝符号链接、限制 15MB、读取后校验大小/路径未变（Electron readChecked 等价）。
fn read_avatar_file(home: &Path, candidate: &Path) -> Option<Vec<u8>> {
    let canonical_home = std::fs::canonicalize(home).ok()?;
    let mut current = canonical_home.clone();
    let relative = candidate.strip_prefix(home).ok()?;
    for segment in relative.components() {
        current.push(segment);
        let metadata = std::fs::symlink_metadata(&current).ok()?;
        if metadata.file_type().is_symlink() || !metadata.is_file() && !metadata.is_dir() {
            return None;
        }
    }
    let metadata = std::fs::metadata(&current).ok()?;
    if !metadata.is_file() || !splash::avatar_size_acceptable(metadata.len()) {
        return None;
    }
    let canonical = std::fs::canonicalize(&current).ok()?;
    if !canonical.starts_with(&canonical_home) {
        return None;
    }
    std::fs::read(&canonical).ok()
}

#[tauri::command]
pub async fn onboarding_complete(window: WebviewWindow) -> Result<Value, String> {
    onboarding::authorize_onboarding_complete(role_label(&window)).map_err(denied)?;
    let home = crate::resource_home()?;
    let connection = crate::resource_bridge::ResourceConnection::from_home(&home)?;
    let response = connection.setup_complete().await?;
    let ok = response["ok"].as_bool() == Some(true);
    let setup_complete = response["setupComplete"].as_bool() == Some(true);
    onboarding::validate_setup_complete_response(ok, setup_complete).map_err(denied)?;
    let app = window.app_handle();
    for label in [crate::native::window_labels::ONBOARDING, crate::native::window_labels::SPLASH] {
        if let Some(active) = app.get_webview_window(label) {
            let _ = active.close();
        }
    }
    if let Some(main) = app.get_webview_window(crate::native::window_labels::MAIN) {
        main.show().map_err(|e| e.to_string())?;
        main.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(serde_json::json!({ "ok": true, "setupComplete": true }))
}

// ── speech 权限（T04；TCC 弹窗只在真实 GUI 用户手势路径）───────────────

#[tauri::command]
pub fn speech_permission_status(
    window: WebviewWindow,
    native: State<'_, install::NativeState>,
) -> Result<Value, String> {
    speech::authorize_speech_caller(role_label(&window)).map_err(denied)?;
    let controller = native
        .speech
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let backend = native
        .speech_tcc
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let state = controller.status(app_info::platform_literal(), &*backend);
    Ok(json!({ "state": state.as_str() }))
}

#[tauri::command]
pub fn speech_permission_request(
    window: WebviewWindow,
    native: State<'_, install::NativeState>,
) -> Result<Value, String> {
    speech::authorize_speech_caller(role_label(&window)).map_err(denied)?;
    let platform = app_info::platform_literal();
    // host_can_prompt：打包壳 + usage description 才允许真实 TCC；调试/hooks 构建不弹。
    let host_can_prompt = cfg!(all(target_os = "macos", not(debug_assertions)));
    let decision = {
        let mut controller = native
            .speech
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let backend = native
            .speech_tcc
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        controller.begin_request(platform, host_can_prompt, &*backend)
    };
    let state = match decision {
        speech::RequestDecision::Terminal(state) | speech::RequestDecision::Unavailable(state) => {
            state
        }
        speech::RequestDecision::HostCannotPrompt | speech::RequestDecision::JoinPending => {
            speech::SpeechPermissionState::NotDetermined
        }
        speech::RequestDecision::Prompt => {
            // 真实 TCC 请求（阻塞等待用户手势结果；自动化不会进入该分支）。
            let raw = {
                let mut backend = native
                    .speech_tcc
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                backend.request_authorization()
            };
            let mut controller = native
                .speech
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            controller.finish_request(raw)
        }
    };
    Ok(json!({ "state": state.as_str() }))
}

// ── debug onboarding（仅 r09-test-hooks 构建注册）───────────────────────

#[cfg(feature = "r09-test-hooks")]
fn open_onboarding_window(app: &AppHandle, preview: bool) -> Result<(), String> {
    let gate = onboarding::debug_onboarding_gate(cfg!(debug_assertions));
    if matches!(gate, onboarding::DebugOnboardingGate::Absent) {
        return Err(onboarding::debug_command_release_error().to_string());
    }
    if let Some(window) = app.get_webview_window(window_labels::ONBOARDING) {
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }
    let spec = install::windows()
        .into_iter()
        .find(|spec| spec.label == window_labels::ONBOARDING)
        .ok_or_else(|| "onboarding 窗口规格缺失".to_string())?;
    let url = onboarding::onboarding_window_url(preview);
    let window =
        tauri::WebviewWindowBuilder::new(app, spec.label, tauri::WebviewUrl::App(url.into()))
            .title(spec.title)
            .inner_size(f64::from(spec.width), f64::from(spec.height))
            .min_inner_size(f64::from(spec.min_width), f64::from(spec.min_height))
            .visible(false)
            .build()
            .map_err(|error| error.to_string())?;
    window.show().map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(feature = "r09-test-hooks")]
#[tauri::command]
pub fn debug_open_onboarding(window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    window_labels::require_role(role_label(&window), &[window_labels::WindowRole::Main])
        .map_err(denied)?;
    open_onboarding_window(&app, false)
}

#[cfg(feature = "r09-test-hooks")]
#[tauri::command]
pub fn debug_open_onboarding_preview(window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    window_labels::require_role(role_label(&window), &[window_labels::WindowRole::Main])
        .map_err(denied)?;
    open_onboarding_window(&app, true)
}

// ── 窗口生命周期（调用点：lib.rs on_window_event）──────────────────────

/// R09-T06 v3.1-G1b 诊断命令（hooks-only）：asset 协议授权/URL/文件事实一次返回。
///
/// 用途：host-core H1 `file-url-asset` 诊断在真实宿主上下文里调用，区分
/// “scope 未授权 / 路径 canonicalize 不匹配 / URL 编码形态 / 文件问题”与
/// “页面级 meta CSP 拦截”（后者需 renderer `connection-csp.js` 补 asset:）。
#[cfg(feature = "r09-test-hooks")]
#[tauri::command]
pub fn debug_asset_probe(
    window: WebviewWindow,
    app: AppHandle,
    path: String,
) -> Result<Value, String> {
    window_labels::require_role(role_label(&window), &[window_labels::WindowRole::Main])
        .map_err(denied)?;
    let config = &app.config().app.security.asset_protocol;
    let scope = app.asset_protocol_scope();
    let canonical = std::fs::canonicalize(&path).ok();
    let metadata = canonical
        .as_ref()
        .and_then(|resolved| std::fs::metadata(resolved).ok());
    let config_patterns: Vec<String> = config
        .scope
        .allowed_paths()
        .iter()
        .map(|item| item.to_string_lossy().into_owned())
        .collect();
    Ok(json!({
        "path": path,
        "canonicalPath": canonical.as_ref().map(|value| value.to_string_lossy().into_owned()),
        "exists": metadata.is_some(),
        "size": metadata.as_ref().map(|value| value.len()),
        "assetProtocolEnable": config.enable,
        "configScope": config_patterns,
        "runtimePatternCount": scope.allowed_patterns().len(),
        "allowedRaw": scope.is_allowed(&path),
        "allowedCanonical": canonical
            .as_ref()
            .map(|value| scope.is_allowed(value))
            .unwrap_or(false),
        "forbiddenRaw": scope.is_forbidden(&path),
        "assetUrl": asset_url(std::path::Path::new(&path)),
        "assetUrlCanonical": canonical.as_ref().map(|value| asset_url(value.as_path())),
    }))
}

#[cfg(feature = "r09-test-hooks")]
fn asset_url(path: &std::path::Path) -> String {
    // 与 convertFileSrc 的 macOS/Linux 形态一致：asset://localhost/<percent-encoded absolute path>。
    let text = path.to_string_lossy();
    let mut encoded = String::with_capacity(text.len() * 3);
    for byte in text.as_bytes() {
        let unreserved = byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'.' | b'_' | b'~');
        if unreserved {
            encoded.push(*byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    #[cfg(target_os = "windows")]
    {
        format!("http://asset.localhost/{encoded}")
    }
    #[cfg(not(target_os = "windows"))]
    {
        format!("asset://localhost/{encoded}")
    }
}

/// 主窗 CloseRequested：托盘可用时隐藏而不是退出（T03 `window_lifecycle`）。
pub fn main_close_should_hide() -> bool {
    let flags = window_lifecycle::LifecycleFlags {
        quitting: false,
        updating: false,
        force_quit: false,
    };
    matches!(
        window_lifecycle::main_close_disposition(flags, true),
        window_lifecycle::CloseDisposition::Hide
    )
}

/// 主窗隐藏时一并隐藏的从属窗口 label（settings/quick-chat 等）。
pub fn dependent_window_labels() -> Vec<String> {
    let labels: Vec<String> = [
        "main",
        "settings",
        "quick-chat",
        "browser-viewer",
        "splash",
        "onboarding",
    ]
    .iter()
    .map(|label| (*label).to_string())
    .collect();
    window_lifecycle::windows_hidden_on_main_close(&labels)
        .into_iter()
        .filter(|label| label != window_labels::MAIN)
        .collect()
}
