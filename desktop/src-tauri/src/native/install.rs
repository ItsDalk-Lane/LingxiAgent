//! R09-T03/T04 注册安装面（供 host-core 的 `lib.rs` 一行调用）。
//!
//! 本文件是 T03/T04 与 host-core 的唯一注册接缝，**只在真实 crate 编译**
//! （`#[cfg(not(r09_standalone_test))]`；独立 rustc 验证时排除）。它把
//! `native/` 的纯逻辑接到 Tauri/插件/窗口/托盘上：
//!
//! ```ignore
//! // lib.rs（host-core 合并）
//! pub mod native;
//! // 1) single-instance 必须第一个注册 → 用 with_plugins 起头
//! let builder = native::install::with_plugins(tauri::Builder::default());
//! let builder = builder.plugin(tauri_plugin_dialog::init()) /* …既有插件… */;
//! builder.manage(…).setup(|app| {
//!     #[cfg(feature = "r09-host")]
//!     { host::setup(app)?; }
//!     native::install::install(app)?;
//!     Ok(())
//! }).invoke_handler(tauri::generate_handler![ /* install::INVOKE_COMMANDS 裸名 */ ])
//! ```
//!
//! 命令函数本体仍由 host-core 写（只调本模块与 `native::*` 纯逻辑）；本文件提供
//! 状态对象、窗口/托盘/快捷键/窗口状态接线与平台适配实现，避免 host-core 重复
//! 摸索插件 API。依赖精确版本见 `T03_T04_REGISTRATION.md`。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::native::{
    global_shortcut, i18n, keep_awake, permissions, quick_chat, quick_chat_handoff,
    remote_boundary, single_instance, speech, theme_sync, tray, window_labels, window_state,
};

/// 需要加入 `invoke_handler` 的**新**命令名（裸名；既有命令不在本表）。
///
/// 与 `T03_T04_REGISTRATION.md` 的 build.rs/capabilities 清单逐字一致；debug 命令
/// 只在 `cfg(debug_assertions)` 构建加入。
pub const INVOKE_COMMANDS: &[&str] = &[
    "reload_main_window",
    "restart_app",
    "open_settings",
    "settings_changed",
    "show_notification",
    "select_skill",
    "open_folder",
    "open_file",
    "show_in_finder",
    "start_drag",
    "get_keep_awake_status",
    "set_keep_awake_enabled",
    "get_auto_launch_status",
    "set_auto_launch_enabled",
    "keybindings_reload_global",
    "quick_chat_reload_shortcut",
    "keybindings_test_register",
    "quick_chat_shortcut_status",
    "quick_chat_show",
    "quick_chat_hide",
    "quick_chat_resize",
    "quick_chat_open_session",
    "quick_chat_open_session_ack",
    "get_splash_info",
    "get_avatar_path",
    "onboarding_complete",
    "speech_permission_status",
    "speech_permission_request",
];

/// 只在 debug 构建注册的调试命令（release 二进制不得含这些入口）。
pub const DEBUG_INVOKE_COMMANDS: &[&str] =
    &["debug_open_onboarding", "debug_open_onboarding_preview"];

/// 窗口创建规格（host-core 在 lib.rs 创建 main/splash/onboarding；settings/quick-chat
/// 由 `install` 提供的 helper 懒创建）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSpec {
    pub label: &'static str,
    pub title: &'static str,
    pub url: &'static str,
    pub width: u32,
    pub height: u32,
    pub min_width: u32,
    pub min_height: u32,
    pub resizable: bool,
    pub transparent: bool,
    pub always_on_top: bool,
    pub decorations: bool,
    pub skip_taskbar: bool,
    pub visible: bool,
    pub center: bool,
}

pub fn windows() -> Vec<WindowSpec> {
    let is_macos = cfg!(target_os = "macos");
    vec![
        WindowSpec {
            label: window_labels::MAIN,
            title: "Lingxi",
            url: "index.html",
            width: 960,
            height: 820,
            min_width: 420,
            min_height: 500,
            resizable: true,
            transparent: false,
            always_on_top: false,
            decorations: true,
            skip_taskbar: false,
            // 现役 createMainWindow: show=false，等 renderer app_ready 再显示
            visible: false,
            center: true,
        },
        WindowSpec {
            label: window_labels::SETTINGS,
            title: "设置",
            url: "settings.html",
            width: 720,
            height: 700,
            min_width: 640,
            min_height: 560,
            resizable: true,
            transparent: false,
            always_on_top: false,
            decorations: true,
            skip_taskbar: false,
            visible: false,
            center: true,
        },
        WindowSpec {
            label: window_labels::QUICK_CHAT,
            title: "Lingxi Quick Chat",
            url: "quick-chat.html",
            width: quick_chat::QUICK_CHAT_WIDTH,
            height: quick_chat::QUICK_CHAT_COMPACT_HEIGHT,
            min_width: quick_chat::QUICK_CHAT_MIN_WIDTH,
            min_height: quick_chat::QUICK_CHAT_MIN_HEIGHT,
            // compact 固定尺寸；chat 模式在命令里 set_resizable(true)
            resizable: false,
            transparent: true,
            always_on_top: true,
            decorations: false,
            skip_taskbar: !is_macos,
            visible: false,
            center: true,
        },
        WindowSpec {
            label: window_labels::SPLASH,
            title: "Lingxi",
            url: "splash.html",
            width: 480,
            height: 320,
            min_width: 480,
            min_height: 320,
            resizable: false,
            transparent: false,
            always_on_top: false,
            decorations: false,
            skip_taskbar: true,
            visible: false,
            center: true,
        },
        WindowSpec {
            label: window_labels::ONBOARDING,
            title: "Lingxi",
            url: "onboarding.html",
            width: 960,
            height: 720,
            min_width: 720,
            min_height: 560,
            resizable: false,
            transparent: false,
            always_on_top: false,
            decorations: true,
            skip_taskbar: false,
            visible: false,
            center: true,
        },
    ]
}

/// 宿主进程内原生状态（host-core 的 command 通过 `State<NativeState>` 访问）。
pub struct NativeState {
    pub keep_awake: Mutex<keep_awake::KeepAwakeMachine>,
    pub speech: Mutex<speech::SpeechPermissionController>,
    pub computer_use: Mutex<permissions::ComputerUseLeaseManager>,
    pub remote_boundary: Mutex<remote_boundary::RemoteDeviceBoundary>,
    pub quick_chat_mode: Mutex<quick_chat::QuickChatMode>,
    pub handoff: Mutex<Option<quick_chat_handoff::HandoffRequest>>,
    pub registered_shortcuts: Mutex<Vec<String>>,
    pub speech_tcc: Mutex<SpeechTccBackend>,
}

impl Default for NativeState {
    fn default() -> Self {
        Self {
            keep_awake: Mutex::new(keep_awake::KeepAwakeMachine::new()),
            speech: Mutex::new(speech::SpeechPermissionController::new()),
            computer_use: Mutex::new(permissions::ComputerUseLeaseManager::new()),
            remote_boundary: Mutex::new(remote_boundary::RemoteDeviceBoundary::new()),
            quick_chat_mode: Mutex::new(quick_chat::QuickChatMode::Compact),
            handoff: Mutex::new(None),
            registered_shortcuts: Mutex::new(Vec::new()),
            speech_tcc: Mutex::new(SpeechTccBackend::new()),
        }
    }
}

impl NativeState {
    pub fn new() -> Self {
        Self::default()
    }
}

/// 与 host-core 的既有插件一并注册；**必须作为 Builder 的第一个 plugin 调用**
/// （tauri-plugin-single-instance 要求最先注册）。
pub fn with_plugins(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    let builder = if isolation_mode() {
        // 隔离 LINGXI_HOME（/private/tmp/lingxi-r09-*、自定义 home）下不注册单实例
        // 插件：插件按 app identifier 收敛，会把隔离目录进程误判为第二实例，
        // 破坏「不同 home 可并行」语义（R09 START_RECEIPT §7）。生产默认 home
        // 才启用单实例。该差异写入 T03_T04_PROGRESS §6（T03-SINGLE-01）。
        eprintln!(
            "[native] LINGXI_HOME 非标准目录：跳过 single-instance 插件（隔离 home 允许并行）"
        );
        builder
    } else {
        builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // 第二实例：只聚焦既有主窗/引导窗，不启动第二 server。
            focus_primary(app);
        }))
    };

    let builder = builder.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(|app, _shortcut, event| {
                if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                    // quick-chat.toggle：显示/聚焦快聊窗（语义见 quick_chat::show_plan）
                    let _ = show_quick_chat(app);
                }
            })
            .build(),
    );
    let builder = builder.plugin(tauri_plugin_notification::init());
    builder.plugin(
        tauri_plugin_autostart::Builder::new()
            .macos_launcher(tauri_plugin_autostart::MacosLauncher::LaunchAgent)
            .build(),
    )
}

/// setup 阶段安装：manage 状态 + 托盘 + 快捷键 + 通知点击 delegate + 权限探测。
pub fn install(app: &mut tauri::App) -> Result<(), String> {
    app.manage(NativeState::new());
    create_tray(app)?;
    let _ = register_quick_chat_shortcuts(app.handle());
    if cfg!(target_os = "macos") {
        #[cfg(target_os = "macos")]
        {
            // OQ-03：宿主自管 UNUserNotificationCenter（不用插件展示），点击聚焦主窗。
            super::notification_center_macos::install_click_handler(app.handle())?;
        }
        probe_notification_permission(app.handle());
    }
    Ok(())
}

/// 通知展示（mac 用宿主 delegate 路径以支持点击聚焦；其它平台用插件）。
pub fn show_notification(app: &AppHandle, title: &str, body: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let _ = app;
        super::notification_center_macos::show(title, body)
    }
    #[cfg(not(target_os = "macos"))]
    {
        use tauri_plugin_notification::NotificationExt;
        app.notification()
            .builder()
            .title(title)
            .body(body)
            .show()
            .map_err(|error| error.to_string())
    }
}

// ── 路径 / 偏好 ────────────────────────────────────────────────────────

pub fn lingxi_home() -> PathBuf {
    if let Some(value) = std::env::var_os("LINGXI_HOME") {
        return PathBuf::from(value);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));
    home.join(".lingxi")
}

/// 隔离模式：LINGXI_HOME 不在用户标准目录（~/.lingxi / ~/.lingxi-dev / ~/.hanako-dev）
/// 时禁用单实例插件，保证「不同 home 允许并行」（R09 START_RECEIPT §7）。
pub fn isolation_mode() -> bool {
    let home = lingxi_home();
    let Some(user_home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return false;
    };
    let standard = [
        user_home.join(".lingxi"),
        user_home.join(".lingxi-dev"),
        user_home.join(".hanako-dev"),
    ];
    !standard.iter().any(|candidate| candidate == &home)
}

fn preferences_json() -> Value {
    let path = lingxi_home().join("user/preferences.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

/// 当前 locale（与 i18n 模块同规则；偏好缺失 → zh）。
pub fn current_locale() -> &'static str {
    let raw = preferences_json();
    let locale = raw.get("locale").and_then(Value::as_str);
    i18n::resolve_locale_preference(locale)
}

// ── 托盘 ───────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
const TRAY_ICON_PNG: &[u8] = include_bytes!("../../../src/assets/tray-template.png");
#[cfg(not(target_os = "macos"))]
const TRAY_ICON_PNG: &[u8] = include_bytes!("../../../src/assets/tray-template.png");

fn create_tray(app: &mut tauri::App) -> Result<(), String> {
    let strings = i18n::tray_strings(current_locale());
    let model = tray::build_menu_model(&strings);
    let mut menu = tauri::menu::MenuBuilder::new(app);
    for item in &model {
        match item {
            tray::TrayMenuItem::Action { id, label, .. } => {
                menu = menu.text(*id, label);
            }
            tray::TrayMenuItem::Separator => {
                menu = menu.separator();
            }
        }
    }
    let menu = menu.build().map_err(|error| error.to_string())?;
    let icon = tauri::image::Image::from_bytes(TRAY_ICON_PNG)
        .map_err(|error| format!("托盘图标解码失败: {error}"))?;
    let platform =
        tray::TrayPlatform::from_str(app_info_platform()).map_err(|error| error.to_string())?;
    tauri::tray::TrayIconBuilder::with_id(tray::TRAY_ID)
        .icon(icon)
        .icon_as_template(tray::icon_as_template(platform))
        .tooltip(tray::tooltip(cfg!(debug_assertions)))
        .menu(&menu)
        .on_menu_event(|app, event| {
            let Some(action) = tray::action_for_id(event.id().as_ref()) else {
                return;
            };
            let _ = run_tray_action(app, action);
        })
        .on_tray_icon_event(|tray_icon, event| {
            if let tauri::tray::TrayIconEvent::DoubleClick { .. } = event {
                focus_primary(tray_icon.app_handle());
            }
        })
        .build(app)
        .map_err(|error| format!("托盘创建失败: {error}"))?;
    Ok(())
}

pub fn app_info_platform() -> &'static str {
    crate::native::app_info::platform_literal()
}

fn run_tray_action(app: &AppHandle, action: tray::TrayAction) -> Result<(), String> {
    for effect in tray::plan_action(action).effects {
        match effect {
            tray::TrayEffect::ShowPrimaryWindow => focus_primary(app),
            tray::TrayEffect::OpenSettingsWindow => {
                let _ = open_settings_window(app, None);
            }
            tray::TrayEffect::BeginQuit { .. } => {
                // 退出清理走 T02 supervisor（host-core 的 RunEvent::ExitRequested 路径）
                app.exit(0);
            }
        }
    }
    Ok(())
}

// ── 主窗 / 设置窗 / 快聊窗 ─────────────────────────────────────────────

pub fn focus_primary(app: &AppHandle) {
    let target = app
        .get_webview_window(window_labels::MAIN)
        .or_else(|| app.get_webview_window(window_labels::ONBOARDING));
    if let Some(window) = target {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub fn focus_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(window_labels::MAIN) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// 打开设置：主窗优先 modal（`open-settings-modal`），否则独立设置窗。
pub fn open_settings_window(app: &AppHandle, tab: Option<&str>) -> Result<(), String> {
    let main_alive = app.get_webview_window(window_labels::MAIN).is_some();
    let settings_alive = app.get_webview_window(window_labels::SETTINGS).is_some();
    let route =
        super::settings_window::plan_open_settings_internal(tab, main_alive, false, settings_alive)
            .map_err(|error| error.to_string())?;
    match route.target {
        super::settings_window::SettingsTarget::MainModal { tab } => {
            focus_main(app);
            let _ = app.emit_to(
                window_labels::MAIN,
                super::settings_window::EVENT_OPEN_SETTINGS_MODAL,
                serde_json::json!({ "tab": tab }),
            );
        }
        super::settings_window::SettingsTarget::SettingsWindow {
            tab,
            create,
            focus_existing,
        } => {
            if create {
                let spec = windows()
                    .into_iter()
                    .find(|spec| spec.label == window_labels::SETTINGS)
                    .expect("settings spec");
                let window = WebviewWindowBuilder::new(
                    app,
                    window_labels::SETTINGS,
                    WebviewUrl::App(spec.url.into()),
                )
                .title(spec.title)
                .inner_size(f64::from(spec.width), f64::from(spec.height))
                .min_inner_size(f64::from(spec.min_width), f64::from(spec.min_height))
                .visible(false)
                .build()
                .map_err(|error| error.to_string())?;
                window.show().map_err(|error| error.to_string())?;
                if let Some(tab) = tab {
                    let _ = app.emit_to(
                        window_labels::SETTINGS,
                        super::settings_window::EVENT_SETTINGS_SWITCH_TAB,
                        serde_json::json!({ "tab": tab }),
                    );
                }
            } else if focus_existing {
                if let Some(window) = app.get_webview_window(window_labels::SETTINGS) {
                    let _ = window.show();
                    let _ = window.set_focus();
                    if let Some(tab) = tab {
                        let _ = app.emit_to(
                            window_labels::SETTINGS,
                            super::settings_window::EVENT_SETTINGS_SWITCH_TAB,
                            serde_json::json!({ "tab": tab }),
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

fn ensure_quick_chat_window(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(window_labels::QUICK_CHAT) {
        return Ok(window);
    }
    quick_chat::transparent_window_supported(app_info_platform())
        .map_err(|error| error.to_string())?;
    let spec = windows()
        .into_iter()
        .find(|spec| spec.label == window_labels::QUICK_CHAT)
        .expect("quick-chat spec");
    WebviewWindowBuilder::new(
        app,
        window_labels::QUICK_CHAT,
        WebviewUrl::App(spec.url.into()),
    )
    .title(spec.title)
    .inner_size(f64::from(spec.width), f64::from(spec.height))
    .min_inner_size(f64::from(spec.min_width), f64::from(spec.min_height))
    .resizable(spec.resizable)
    .decorations(spec.decorations)
    .transparent(spec.transparent)
    .always_on_top(spec.always_on_top)
    .skip_taskbar(spec.skip_taskbar)
    .visible(false)
    .build()
    .map_err(|error| error.to_string())
}

pub fn show_quick_chat(app: &AppHandle) -> Result<(), String> {
    let window = ensure_quick_chat_window(app)?;
    let plan = quick_chat::show_plan(app_info_platform());
    let _ = window.show();
    if plan.always_on_top {
        let _ = window.set_always_on_top(true);
    }
    if plan.visible_on_all_workspaces {
        let _ = window.set_visible_on_all_workspaces(true);
    }
    let _ = window.set_focus();
    if plan.emit_shown {
        let _ = app.emit_to(
            window_labels::QUICK_CHAT,
            quick_chat::EVENT_QUICK_CHAT_SHOWN,
            serde_json::json!({}),
        );
    }
    Ok(())
}

pub fn hide_quick_chat(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(window_labels::QUICK_CHAT) {
        let _ = window.hide();
    }
    Ok(())
}

// ── 全局快捷键 ─────────────────────────────────────────────────────────

/// 读取偏好并（重新）注册 quick-chat 快捷键；返回与现役同形回执。
pub fn register_quick_chat_shortcuts(app: &AppHandle) -> global_shortcut::RegistrationResult {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let prefs = preferences_json();
    let override_values: Option<Vec<String>> = prefs
        .pointer("/keybindings")
        .and_then(|value| value.get(global_shortcut::QUICK_CHAT_TOGGLE_COMMAND))
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        });
    let legacy = prefs
        .pointer("/quick_chat/shortcut")
        .and_then(Value::as_str);
    let desired =
        match global_shortcut::select_quick_chat_shortcuts(override_values.as_deref(), legacy) {
            Ok(list) => list,
            Err(error) => {
                return global_shortcut::RegistrationResult::unavailable(vec![error
                    .message()
                    .to_owned()])
            }
        };
    let current = app
        .try_state::<NativeState>()
        .map(|state| state.registered_shortcuts.lock().unwrap().clone())
        .unwrap_or_default();
    let plan = global_shortcut::plan_registration(&current, &desired);
    for shortcut in &plan.unregister {
        let _ = app.global_shortcut().unregister(shortcut.as_str());
    }
    let mut results: Vec<(String, bool)> = Vec::new();
    for shortcut in &plan.register {
        let ok = app.global_shortcut().register(shortcut.as_str()).is_ok();
        results.push((shortcut.clone(), ok));
    }
    for shortcut in &plan.unchanged {
        results.push((shortcut.clone(), true));
    }
    let report = global_shortcut::summarize_registration(&desired, &results);
    if let Some(state) = app.try_state::<NativeState>() {
        let mut registered = state.registered_shortcuts.lock().unwrap();
        *registered = desired
            .iter()
            .filter(|shortcut| results.iter().any(|(item, ok)| item == *shortcut && *ok))
            .cloned()
            .collect();
    }
    report
}

// ── 通知（OQ-03：mac 点击聚焦实现见 notification_center_macos；其它平台 unsupported）──

pub fn probe_notification_permission(app: &AppHandle) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().permission_state();
}

// ── 窗口状态持久化 ─────────────────────────────────────────────────────

/// 读取 `LINGXI_HOME/user/window-state.json` 并传入当前显示器工作区做 sanitize。
pub fn load_window_state(window: &WebviewWindow) -> Option<window_state::DesiredWindowState> {
    let path = window_state::persistence_path(&lingxi_home().to_string_lossy()).ok()?;
    let raw = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    let state = window_state::PersistedWindowState {
        x: value.get("x").and_then(Value::as_f64),
        y: value.get("y").and_then(Value::as_f64),
        width: value.get("width").and_then(Value::as_f64),
        height: value.get("height").and_then(Value::as_f64),
        is_maximized: value.get("isMaximized").and_then(Value::as_bool),
    };
    let mut areas: Vec<window_state::WorkArea> = window
        .available_monitors()
        .unwrap_or_default()
        .into_iter()
        .map(|monitor| {
            let position = monitor.position();
            let size = monitor.size();
            let scale = monitor.scale_factor();
            window_state::WorkArea::new(
                f64::from(position.x),
                f64::from(position.y),
                f64::from(size.width) / scale,
                f64::from(size.height) / scale,
            )
        })
        .collect();
    areas.sort_by(|left, right| {
        left.x
            .partial_cmp(&right.x)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    window_state::sanitize_window_state(
        Some(state),
        &areas,
        &window_state::WindowStateOptions::main(),
    )
}

/// 保存主窗状态（host-core 在 move/resize 上做 500ms 防抖后调用）。
pub fn save_window_state(window: &WebviewWindow) -> Result<(), String> {
    let path = window_state::persistence_path(&lingxi_home().to_string_lossy())
        .map_err(|error| error.to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let is_maximized = window.is_maximized().unwrap_or(false);
    let size = if is_maximized {
        window.outer_size().map_err(|error| error.to_string())?
    } else {
        window.outer_size().map_err(|error| error.to_string())?
    };
    let position = window.outer_position().map_err(|error| error.to_string())?;
    let scale = window.scale_factor().unwrap_or(1.0);
    let state = window_state::state_to_persist(
        f64::from(position.x) / scale,
        f64::from(position.y) / scale,
        f64::from(size.width) / scale,
        f64::from(size.height) / scale,
        is_maximized,
    );
    let payload = serde_json::json!({
        "x": state.x, "y": state.y, "width": state.width,
        "height": state.height, "isMaximized": state.is_maximized,
    });
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&payload).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

// ── 主题应用（sync_window_theme / window_theme_changed）──────────────────

pub fn apply_window_theme(
    window: &WebviewWindow,
    plan: &theme_sync::ThemeSyncPlan,
) -> Result<(), String> {
    let theme = match plan.native_appearance {
        theme_sync::NativeAppearance::Dark => tauri::Theme::Dark,
        theme_sync::NativeAppearance::Light => tauri::Theme::Light,
    };
    window
        .set_theme(Some(theme))
        .map_err(|error| error.to_string())
}

// ── 平台适配：keep-awake / speech TCC / Finder reveal ───────────────────

/// 真实系统 keep-awake 后端（mac IOPMAssertion / win SetThreadExecutionState /
/// Linux 显式 unsupported）。
#[derive(Debug, Default)]
pub struct SystemKeepAwake {
    #[cfg(target_os = "macos")]
    assertion: Option<u32>,
}

impl keep_awake::KeepAwakeBackend for SystemKeepAwake {
    fn start(&mut self) -> Result<u64, String> {
        #[cfg(target_os = "macos")]
        {
            let id = mac_keep_awake::create_assertion()?;
            self.assertion = Some(id);
            Ok(u64::from(id))
        }
        #[cfg(target_os = "windows")]
        {
            windows_keep_awake::enable()?;
            Ok(1)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Err("Linux 保持唤醒尚无已验证实现".to_string())
        }
    }

    fn stop(&mut self, _blocker_id: u64) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            if let Some(id) = self.assertion.take() {
                mac_keep_awake::release_assertion(id);
            }
        }
        Ok(())
    }

    fn is_started(&self, _blocker_id: u64) -> Option<bool> {
        #[cfg(target_os = "macos")]
        {
            Some(self.assertion.is_some())
        }
        #[cfg(not(target_os = "macos"))]
        {
            Some(true)
        }
    }
}

#[cfg(target_os = "macos")]
mod mac_keep_awake {
    use std::ffi::c_void;

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    const K_IOPM_ASSERTION_LEVEL_ON: u32 = 255;

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOPMAssertionCreateWithName(
            assertion_type: *const c_void,
            assertion_level: u32,
            assertion_name: *const c_void,
            assertion_id: *mut u32,
        ) -> i32;
        fn IOPMAssertionRelease(assertion_id: u32) -> i32;
    }
    // SDK 里 kIOPMAssertionTypePreventUserIdleSystemSleep 是 CFSTR 宏（编译期
    // 常量字符串），不是可链接的外部静态符号；必须自行建 CFString。
    const ASSERTION_TYPE: &str = "PreventUserIdleSystemSleep";
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithCString(
            allocator: *const c_void,
            c_str: *const std::os::raw::c_char,
            encoding: u32,
        ) -> *const c_void;
        fn CFRelease(value: *const c_void);
    }

    pub fn create_assertion() -> Result<u32, String> {
        let name = c"lingxi keep-awake";
        unsafe {
            let kind_ref = CFStringCreateWithCString(
                std::ptr::null(),
                ASSERTION_TYPE.as_ptr().cast(),
                K_CF_STRING_ENCODING_UTF8,
            );
            let name_ref = CFStringCreateWithCString(
                std::ptr::null(),
                name.as_ptr(),
                K_CF_STRING_ENCODING_UTF8,
            );
            if kind_ref.is_null() || name_ref.is_null() {
                if !kind_ref.is_null() {
                    CFRelease(kind_ref);
                }
                if !name_ref.is_null() {
                    CFRelease(name_ref);
                }
                return Err("无法创建 keep-awake 断言类型/名称".to_string());
            }
            let mut assertion_id: u32 = 0;
            let result = IOPMAssertionCreateWithName(
                kind_ref,
                K_IOPM_ASSERTION_LEVEL_ON,
                name_ref,
                &mut assertion_id,
            );
            CFRelease(kind_ref);
            CFRelease(name_ref);
            if result != 0 {
                return Err(format!("IOPMAssertionCreateWithName 失败: {result}"));
            }
            Ok(assertion_id)
        }
    }

    pub fn release_assertion(assertion_id: u32) {
        unsafe {
            let _ = IOPMAssertionRelease(assertion_id);
        }
    }
}

#[cfg(target_os = "windows")]
mod windows_keep_awake {
    pub fn enable() -> Result<(), String> {
        #[link(name = "kernel32")]
        extern "system" {
            fn SetThreadExecutionState(flags: u32) -> u32;
        }
        const ES_CONTINUOUS: u32 = 0x8000_0000;
        const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
        let result = unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
        if result == 0 {
            return Err("SetThreadExecutionState 失败".to_string());
        }
        Ok(())
    }
}

// ── Speech TCC（mac objc2；非 mac 显式 unavailable）─────────────────────

pub struct SpeechTccBackend {
    requested: bool,
}

impl SpeechTccBackend {
    pub fn new() -> Self {
        Self { requested: false }
    }
}

impl speech::SpeechPermissionBackend for SpeechTccBackend {
    fn authorization_status(&self) -> Option<i64> {
        #[cfg(target_os = "macos")]
        {
            mac_speech::authorization_status()
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    }

    fn request_authorization(&mut self) -> Option<i64> {
        self.requested = true;
        #[cfg(target_os = "macos")]
        {
            mac_speech::request_authorization()
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    }
}

#[cfg(target_os = "macos")]
mod mac_speech {
    use std::sync::mpsc;
    use std::time::Duration;

    #[link(name = "Speech", kind = "framework")]
    extern "C" {}

    fn speech_class() -> Option<&'static objc2::runtime::AnyClass> {
        objc2::runtime::AnyClass::get(c"SFSpeechRecognizer")
    }

    pub fn authorization_status() -> Option<i64> {
        let class = speech_class()?;
        let status: isize = unsafe { objc2::msg_send![class, authorizationStatus] };
        Some(status as i64)
    }

    pub fn request_authorization() -> Option<i64> {
        let class = speech_class()?;
        let (sender, receiver) = mpsc::channel::<isize>();
        let block = block2::RcBlock::new(move |status: isize| {
            let _ = sender.send(status);
        });
        unsafe {
            let _: () = objc2::msg_send![class, requestAuthorization: &*block];
        }
        receiver
            .recv_timeout(Duration::from_secs(120))
            .ok()
            .map(|value| value as i64)
    }
}

// ── Finder reveal（mac objc2 NSWorkspace；其余平台显式 unsupported）─────

pub fn reveal_in_finder(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        mac_reveal::reveal(path)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        Err("showInFinder 在 Windows/Linux 尚无 Tauri 等价实现（平台阻塞）".to_string())
    }
}

#[cfg(target_os = "macos")]
mod mac_reveal {
    use objc2_foundation::{NSString, NSURL};
    use std::path::Path;

    #[link(name = "AppKit", kind = "framework")]
    extern "C" {}

    pub fn reveal(path: &Path) -> Result<(), String> {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let workspace: objc2::rc::Retained<objc2::runtime::AnyObject> =
            unsafe { objc2::msg_send![objc2::class!(NSWorkspace), sharedWorkspace] };
        unsafe {
            let _: () = objc2::msg_send![
                &*workspace,
                selectFile: &*url,
                inFileViewerRootedAtPath: std::ptr::null::<objc2::runtime::AnyObject>()
            ];
        }
        Ok(())
    }
}

// ── 单实例 / 隔离语义辅助（供 host-core 诊断）──────────────────────────

pub fn single_instance_namespace() -> Option<String> {
    let home = lingxi_home();
    single_instance::lock_namespace("com.lingxi.app", &home.to_string_lossy()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invoke_command_names_are_unique_and_snake_case() {
        let mut names = INVOKE_COMMANDS.to_vec();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "命令名重复");
        for name in INVOKE_COMMANDS {
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "命令名必须是 snake_case: {name}"
            );
        }
        for name in DEBUG_INVOKE_COMMANDS {
            assert!(!INVOKE_COMMANDS.contains(name));
        }
    }

    #[test]
    fn window_specs_cover_all_labels_unique() {
        let specs = windows();
        let mut labels: Vec<&str> = specs.iter().map(|spec| spec.label).collect();
        assert_eq!(labels.len(), 5);
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), 5);
        for spec in &specs {
            assert!(!spec.url.is_empty());
            assert!(spec.width > 0 && spec.height > 0);
            assert!(spec.width >= spec.min_width && spec.height >= spec.min_height);
        }
        let main = specs.iter().find(|spec| spec.label == "main").unwrap();
        assert!(!main.visible, "主窗创建时隐藏，等 app_ready");
        assert_eq!((main.width, main.height), (960, 820));
        let quick = specs
            .iter()
            .find(|spec| spec.label == "quick-chat")
            .unwrap();
        assert!(quick.transparent && quick.always_on_top && !quick.decorations);
    }
}
