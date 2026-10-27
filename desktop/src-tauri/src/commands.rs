//! R09-T01/T02 Tauri commands：渲染层 boot 桥 + 服务状态（设计 §3.4/§4.5）。
//!
//! 安全边界：
//! - token/连接信息命令只允许 `main` 窗口（T06 收口前的显式最小面）；
//! - 诊断只写入宿主自己的 `{LINGXI_HOME}/diagnostics/tauri-launch/`，绝不把
//!   token 写进诊断（Diagnostics 统一脱敏）；
//! - `app_ready` 的 DOM 投影只在 debug 构建采集（A01 证据的 JS 投影面），
//!   release 只记录 `app-ready` 事件（url/title 由宿主 API 读取，不 eval）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State, WebviewWindow};

use crate::native::window_controls;
use crate::service::{ConnectionInfoView, Diagnostics, ServiceRuntime};

pub struct HostState {
    runtime: Mutex<Option<Arc<ServiceRuntime>>>,
    diagnostics: Mutex<Option<Diagnostics>>,
    ready_seen: AtomicBool,
    started_at: Instant,
}

impl HostState {
    pub fn new(runtime: Arc<ServiceRuntime>, diagnostics: Diagnostics) -> Self {
        Self {
            runtime: Mutex::new(Some(runtime)),
            diagnostics: Mutex::new(Some(diagnostics)),
            ready_seen: AtomicBool::new(false),
            started_at: Instant::now(),
        }
    }

    pub fn empty() -> Self {
        Self {
            runtime: Mutex::new(None),
            diagnostics: Mutex::new(None),
            ready_seen: AtomicBool::new(false),
            started_at: Instant::now(),
        }
    }

    pub fn runtime(&self) -> Option<Arc<ServiceRuntime>> {
        self.runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn diagnostics(&self) -> Option<Diagnostics> {
        self.diagnostics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn install(&self, runtime: Arc<ServiceRuntime>, diagnostics: Diagnostics) {
        *self
            .runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(runtime);
        *self
            .diagnostics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(diagnostics);
    }

    pub fn note_window_created(&self) -> u128 {
        self.started_at.elapsed().as_millis()
    }
}

/// 自动化/测试模式：`LINGXI_R09_AUTOMATION=1` 时禁止任何用户可见 UI（窗口 show /
/// modal / 通知）。R09-T06 D11：测试注入点只在 `r09-test-hooks` 构建生效。
#[cfg(feature = "r09-test-hooks")]
pub fn automation_mode() -> bool {
    std::env::var("LINGXI_R09_AUTOMATION")
        .map(|value| value == "1")
        .unwrap_or(false)
}

#[cfg(not(feature = "r09-test-hooks"))]
pub fn automation_mode() -> bool {
    false
}

/// `app_ready` 的窗口可见性决策（纯函数，便于单测；与 already_ready 无关）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppReadyWindowAction {
    /// automation：绝不 show；窗口若已被其他路径显示则 hide。
    Suppress {
        hide_if_visible: bool,
    },
    Show,
}

pub(crate) fn app_ready_window_action(
    automation: bool,
    _already_ready: bool,
    visible: bool,
) -> AppReadyWindowAction {
    if automation {
        AppReadyWindowAction::Suppress {
            hide_if_visible: visible,
        }
    } else {
        AppReadyWindowAction::Show
    }
}

/// automation 下 window 控制命令（minimize/maximize）的守卫决策（纯函数）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutomationWindowControl {
    Suppress,
    Proceed,
}

pub(crate) fn automation_window_control_action(automation: bool) -> AutomationWindowControl {
    if automation {
        AutomationWindowControl::Suppress
    } else {
        AutomationWindowControl::Proceed
    }
}

/// automation 下抑制会改变主窗可见性的 window 控制命令；返回 true 表示已抑制。
fn suppress_window_control_in_automation(window: &WebviewWindow, command: &str) -> bool {
    if automation_window_control_action(automation_mode()) == AutomationWindowControl::Proceed {
        return false;
    }
    let visible_before = window.is_visible().unwrap_or(false);
    if visible_before {
        let _ = window.hide();
    }
    if let Some(state) = window.app_handle().try_state::<HostState>() {
        log_event(
            &state,
            "window-control-suppressed",
            json!({
                "command": command,
                "automation": true,
                "visibleBefore": visible_before,
                "visibleAfter": window.is_visible().unwrap_or(false),
            }),
        );
    }
    true
}

fn log_event(state: &HostState, event: &str, fields: Value) {
    if let Some(diagnostics) = state.diagnostics() {
        diagnostics.log(event, fields);
    }
}

fn main_window_only(window: &WebviewWindow) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("该宿主命令只允许主窗口调用".to_string())
    }
}

#[tauri::command]
pub fn get_server_connection_info(
    window: WebviewWindow,
    state: State<'_, HostState>,
) -> Result<ConnectionInfoView, String> {
    main_window_only(&window)?;
    Ok(match state.runtime() {
        Some(runtime) => runtime.view(),
        None => ConnectionInfoView::empty(),
    })
}

#[tauri::command]
pub fn get_server_port(
    window: WebviewWindow,
    state: State<'_, HostState>,
) -> Result<Option<u16>, String> {
    main_window_only(&window)?;
    Ok(state
        .runtime()
        .and_then(|runtime| runtime.connection())
        .map(|connection| connection.port))
}

#[tauri::command]
pub fn get_server_token(
    window: WebviewWindow,
    state: State<'_, HostState>,
) -> Result<Option<String>, String> {
    main_window_only(&window)?;
    Ok(state
        .runtime()
        .and_then(|runtime| runtime.connection())
        .map(|connection| connection.token))
}

#[tauri::command]
pub fn get_service_status(state: State<'_, HostState>) -> Value {
    match state.runtime() {
        Some(runtime) => runtime.status(),
        None => json!({
            "schema": "lingxi.r09.service-status.v1",
            "mode": null,
            "lifecycle": "unavailable",
            "reason": "R09 host service supervisor is not compiled into this build (feature r09-host)",
        }),
    }
}

#[tauri::command]
pub fn get_platform() -> String {
    crate::service::resolve::platform_name().to_string()
}

#[tauri::command]
pub fn get_app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

/// 主窗口 ready：显示窗口并把页面来源写进诊断（A02 的 page-load 断言面）。
#[tauri::command]
pub fn app_ready(window: WebviewWindow, state: State<'_, HostState>) -> Result<(), String> {
    let url = window.url().map(|url| url.to_string()).unwrap_or_default();
    let title = window.title().unwrap_or_default();
    let already_ready = state.ready_seen.swap(true, Ordering::SeqCst);
    let automation = automation_mode();
    let visible_before = window.is_visible().unwrap_or(false);
    match app_ready_window_action(automation, already_ready, visible_before) {
        AppReadyWindowAction::Suppress { hide_if_visible } => {
            // 自动化：每次 app_ready 都保证不可见（防止其它路径先 show 后留下可见窗）。
            if hide_if_visible {
                let _ = window.hide();
            }
            log_event(
                &state,
                "app-ready-window-suppressed",
                json!({
                    "label": window.label(),
                    "automation": true,
                    "alreadyReady": already_ready,
                    "visibleBefore": visible_before,
                    "visibleAfter": window.is_visible().unwrap_or(false),
                }),
            );
        }
        AppReadyWindowAction::Show => {
            if let Err(error) = window.show() {
                log_event(
                    &state,
                    "app-ready-show-failed",
                    json!({"label": window.label(), "error": error.to_string()}),
                );
                return Err(format!("主窗口显示失败: {error}"));
            }
        }
    }
    log_event(
        &state,
        "app-ready",
        json!({
            "label": window.label(),
            "senderUrl": url,
            "title": title,
            "alreadyReady": already_ready,
            "automation": automation,
            "windowVisible": window.is_visible().unwrap_or(false),
        }),
    );
    // A01 的 DOM 投影（仅 r09-test-hooks 构建；交付 release 不 eval 页面）。
    #[cfg(feature = "r09-test-hooks")]
    {
        if window.label() == "main" && !already_ready {
            if let Some(diagnostics) = state.diagnostics() {
                let projection_window = window.clone();
                tauri::async_runtime::spawn(async move {
                    // 等 React 完成 app-ready 后的最后一次提交，投影才是稳定 DOM。
                    tokio::time::sleep(std::time::Duration::from_millis(3000)).await;
                    let script = r#"(function(){
  var root = document.getElementById('react-root');
  // R09-T04：系统权限卡片按宿主真实 TCC 状态渲染，不属于业务 DOM；
  // A01 文本投影移除该宿主状态节点，避免把状态差误判为界面回归。
  document.querySelectorAll('[data-testid="system-permissions-card"]').forEach(function(el){
    el.parentNode && el.parentNode.removeChild(el);
  });
  var text = (document.body && document.body.innerText) || '';
  text = text.replace(/\s+/g, ' ').trim();
  var hash = 2166136261 >>> 0;
  for (var i = 0; i < text.length; i++) { hash ^= text.charCodeAt(i); hash = Math.imul(hash, 16777619) >>> 0; }
  var path = [];
  if (root) {
    for (var c = 0; c < root.children.length && c < 12; c++) {
      var el = root.children[c];
      path.push(el.tagName + '#' + (el.id || '') + '.' + (el.className && typeof el.className === 'string' ? el.className.split(/\s+/).slice(0,3).join('.') : ''));
    }
  }
  return {
    title: document.title,
    platform: document.documentElement.getAttribute('data-platform'),
    reactRootMounted: !!root,
    rootChildCount: root ? root.children.length : -1,
    rootPath: path,
    bodyTextHash: hash,
    bodyTextLength: text.length,
    hasHana: !!window.hana,
    hasTauri: !!window.__TAURI__,
    tauriInvoke: !!(window.__TAURI__ && window.__TAURI__.core && typeof window.__TAURI__.core.invoke === 'function'),
    location: String(window.location.href),
    bodyText: text.slice(0, 700),
    errors: (window.__r09Errors || []).slice(-12),
    hostErrors: (window.__r09HostErrors || []).slice(-12),
    resources: (window.__r09Resources || []).slice(0, 80)
  };
})()"#;
                    let callback_diagnostics = diagnostics.clone();
                    let callback_window_label = projection_window.label().to_string();
                    let _ = projection_window.eval_with_callback(script, move |result| {
                        match serde_json::from_str::<Value>(&result) {
                            Ok(projection) => callback_diagnostics.log(
                                "dom-projection",
                                json!({"label": callback_window_label, "projection": projection}),
                            ),
                            Err(error) => callback_diagnostics.log(
                                "dom-projection-failed",
                                json!({"label": callback_window_label, "raw": result, "error": error.to_string()}),
                            ),
                        }
                    });
                });
            }
        }
    }
    Ok(())
}

/// 渲染层不允许指定窗口 id：出现即按越权尝试显式拒绝（Electron 语义是忽略
/// 参数、只作用于调用者窗口；Tauri 自定义命令以显式错误表达同一安全边界）。
fn reject_injected_window_id(window_id: Option<&str>) -> Result<(), String> {
    window_controls::reject_window_id_argument(window_id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn window_minimize(window: WebviewWindow, window_id: Option<String>) -> Result<(), String> {
    reject_injected_window_id(window_id.as_deref())?;
    if suppress_window_control_in_automation(&window, "window_minimize") {
        return Ok(());
    }
    window.minimize().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn window_maximize(window: WebviewWindow, window_id: Option<String>) -> Result<(), String> {
    reject_injected_window_id(window_id.as_deref())?;
    if suppress_window_control_in_automation(&window, "window_maximize") {
        return Ok(());
    }
    if window.is_maximized().unwrap_or(false) {
        window.unmaximize().map_err(|error| error.to_string())
    } else {
        window.maximize().map_err(|error| error.to_string())
    }
}

#[tauri::command]
pub fn window_close(window: WebviewWindow, window_id: Option<String>) -> Result<(), String> {
    reject_injected_window_id(window_id.as_deref())?;
    window.close().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn window_is_maximized(
    window: WebviewWindow,
    window_id: Option<String>,
) -> Result<bool, String> {
    reject_injected_window_id(window_id.as_deref())?;
    window.is_maximized().map_err(|error| error.to_string())
}

/// Electron `window-theme-changed` 的主题白名单：dark/light 映射原生外观，
/// auto/system 显式跟随系统；空/控制字符/未知主题一律显式错误且不触碰窗口外观。
fn native_theme_for(theme: &str) -> Result<Option<tauri::Theme>, String> {
    if theme.is_empty() || theme.len() > 64 || theme.chars().any(char::is_control) {
        return Err("主题形态非法（空/控制字符/超长）".to_string());
    }
    match theme {
        "dark" => Ok(Some(tauri::Theme::Dark)),
        "light" => Ok(Some(tauri::Theme::Light)),
        "auto" | "system" => Ok(None),
        other => Err(format!("未知主题: {other}")),
    }
}

/// Electron `window-theme-changed` 的等价：主题名反射到原生窗口 appearance。
#[tauri::command]
pub fn window_theme_changed(
    window: WebviewWindow,
    theme: String,
    state: State<'_, HostState>,
) -> Result<(), String> {
    let native_theme = native_theme_for(&theme)?;
    // 非法主题在上一步已返回错误，不会执行 set_theme（窗口外观保持原状）。
    window
        .set_theme(native_theme)
        .map_err(|error| error.to_string())?;
    log_event(
        &state,
        "window-theme",
        json!({"label": window.label(), "theme": theme}),
    );
    Ok(())
}

/// 只允许 http(s)，与 Electron `open-external` 的协议门一致；失败回执而不抛。
#[tauri::command]
pub fn open_external(url: String, state: State<'_, HostState>) -> Value {
    let parsed = match reqwest::Url::parse(&url) {
        Ok(parsed) => parsed,
        Err(error) => return json!({"ok": false, "error": format!("无效地址: {error}")}),
    };
    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return json!({"ok": false, "error": "只支持 HTTP 或 HTTPS 浏览器地址"});
    }
    match open::that_detached(parsed.as_str()) {
        Ok(()) => {
            log_event(&state, "open-external", json!({"scheme": parsed.scheme()}));
            json!({"ok": true})
        }
        Err(error) => json!({"ok": false, "error": error.to_string()}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_ready_suppresses_every_call_independent_of_already_ready() {
        for already_ready in [false, true] {
            for visible in [false, true] {
                assert_eq!(
                    app_ready_window_action(true, already_ready, visible),
                    AppReadyWindowAction::Suppress { hide_if_visible: visible },
                    "automation 下 app_ready 每次都必须抑制 show（already_ready={already_ready}, visible={visible}）"
                );
            }
        }
        // 连续两次调用（第一次 ready=false，第二次 ready=true）都抑制。
        assert_eq!(
            app_ready_window_action(true, false, false),
            AppReadyWindowAction::Suppress {
                hide_if_visible: false
            }
        );
        assert_eq!(
            app_ready_window_action(true, true, true),
            AppReadyWindowAction::Suppress {
                hide_if_visible: true
            }
        );
    }

    #[test]
    fn app_ready_shows_only_when_not_automation() {
        for already_ready in [false, true] {
            for visible in [false, true] {
                assert_eq!(
                    app_ready_window_action(false, already_ready, visible),
                    AppReadyWindowAction::Show
                );
            }
        }
    }

    #[test]
    fn automation_suppresses_window_controls() {
        assert_eq!(
            automation_window_control_action(true),
            AutomationWindowControl::Suppress
        );
        assert_eq!(
            automation_window_control_action(false),
            AutomationWindowControl::Proceed
        );
    }

    #[test]
    fn window_theme_whitelist_accepts_four_values_only() {
        assert!(matches!(
            native_theme_for("dark"),
            Ok(Some(tauri::Theme::Dark))
        ));
        assert!(matches!(
            native_theme_for("light"),
            Ok(Some(tauri::Theme::Light))
        ));
        // auto/system → 显式跟随系统（set_theme(None)），不是静默 no-op。
        assert!(matches!(native_theme_for("auto"), Ok(None)));
        assert!(matches!(native_theme_for("system"), Ok(None)));
        // 空/控制字符/未知主题（含旧主题注册表名）必须显式错误，调用方不得 set_theme。
        for bad in [
            "",
            "\u{0}",
            "dark\u{0}",
            "midnight",
            "coral",
            "system ",
            "DARK",
        ] {
            assert!(native_theme_for(bad).is_err(), "{bad:?} 应被拒绝");
        }
    }

    #[test]
    fn renderer_window_id_is_rejected_for_window_controls() {
        assert!(reject_injected_window_id(None).is_ok());
        for injected in ["main", "viewer-999", ""] {
            let error = reject_injected_window_id(Some(injected)).unwrap_err();
            assert!(
                error.contains("窗口控制命令不接受"),
                "{injected:?} 应拒绝，实际 {error}"
            );
        }
    }
}
