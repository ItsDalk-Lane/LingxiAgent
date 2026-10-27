//! R09-T01/T02 产品宿主生命周期：服务先就绪、再创建产品 React 窗口。
//!
//! 时序（设计 §4.3，显式记录与 Electron 的差异）：
//!   setup → 解析 home → `ServiceRuntime::start_and_wait()`（spawn→READY→identity）
//!   → `run_on_main_thread` 创建隐藏主窗口 → 页面 `app_init` 调 `app_ready`
//!   → 显示窗口。失败时先落盘失败记录，再弹原生 dialog 并以退出码 2 结束
//!   （无任何 Node 回退路径）。
//!
//! 退出：
//!   - 窗口关闭 / Cmd+Q / OS 登出 → `RunEvent::ExitRequested{code:None}` →
//!     `prevent_exit` → 异步清理 owned 服务 → `app.exit(0)`；
//!   - SIGTERM/SIGINT/SIGHUP → libc handler 置标志，轮询线程触发与上面同一条
//!     清理路径（不装 handler 时 SIGTERM 会直接终止、绕过清理——A04 会红）。
//!
//! 该模块只在 `r09-host` feature 下被调用；未启用时保持 R06 文件资源宿主行为。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::{Emitter, Manager, RunEvent};

use crate::commands::HostState;
use crate::service::shutdown::ShutdownReport;
use crate::service::{resolve, Diagnostics, ServiceRuntime, StartError, SupervisorConfig, Timings};

const MAIN_WINDOW_LABEL: &str = "main";
const MAIN_WINDOW_TITLE: &str = "Lingxi · Rust 预览";
const APP_READY_FALLBACK: Duration = Duration::from_secs(30);

fn home_for_diagnostics(env: &crate::service::env::EnvSnapshot) -> Result<PathBuf, StartError> {
    resolve::home_from_env(env)
}

// ── asset protocol 运行时授权（v2 Patch §1；D8/A10）────────────────────
//
// 静态 scope（tauri.conf/tauri.release.conf）覆盖 $HOME 下的三个标准 LINGXI_HOME
// 与 $TEMP；非标准 LINGXI_HOME（隔离测试、用户自定义）与 home 外 workspace 必须在
// setup 阶段用 Rust `asset_protocol_scope().allow_directory` 动态授权。
// 红线：只授权“已验证存在的真实目录”，绝不使用全盘 `/**`。

const WORKSPACE_PREFERENCE_KEYS: [&str; 3] = ["workspacePath", "workspace_path", "workspaceRoot"];

fn install_asset_protocol_scope(app: &tauri::App, home: &Path) {
    let scope = app.asset_protocol_scope();
    // R09-T06 v3.1-G1b 诊断（hooks-only）：asset 协议失败排查需要运行期可观察值。
    #[cfg(feature = "r09-test-hooks")]
    {
        let config = &app.config().app.security.asset_protocol;
        let config_patterns: Vec<String> = config
            .scope
            .allowed_paths()
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        eprintln!(
            "[tauri-host][asset-scope] event=before-allow enable={} configScope={:?} runtimePatterns={} home={} homeRawAllowed={} homeCanonical={:?} homeCanonicalAllowed={}",
            config.enable,
            config_patterns,
            scope.allowed_patterns().len(),
            home.display(),
            scope.is_allowed(home),
            std::fs::canonicalize(home).ok(),
            std::fs::canonicalize(home)
                .ok()
                .map(|path| scope.is_allowed(&path))
                .unwrap_or(false),
        );
    }
    if let Err(error) = scope.allow_directory(home, true) {
        eprintln!("[tauri-host] asset scope allow home failed: {error}");
    }
    #[cfg(feature = "r09-test-hooks")]
    {
        let config = &app.config().app.security.asset_protocol;
        let canonical_home = std::fs::canonicalize(home).ok();
        eprintln!(
            "[tauri-host][asset-scope] event=after-allow enable={} runtimePatterns={} homeRawAllowed={} homeCanonicalAllowed={}",
            config.enable,
            scope.allowed_patterns().len(),
            scope.is_allowed(home),
            canonical_home
                .as_ref()
                .map(|path| scope.is_allowed(path))
                .unwrap_or(false),
        );
    }
    for root in asset_workspace_roots(home) {
        if let Err(error) = scope.allow_directory(&root, true) {
            eprintln!(
                "[tauri-host] asset scope allow workspace failed: {}: {error}",
                root.display()
            );
        }
    }
}

/// home 外 workspace 根（来自 preferences 的显式配置；逐项校验后去重）。
fn asset_workspace_roots(home: &Path) -> Vec<PathBuf> {
    let path = home.join("user/preferences.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let mut roots: Vec<PathBuf> = Vec::new();
    for key in WORKSPACE_PREFERENCE_KEYS {
        let Some(raw) = value.get(key).and_then(Value::as_str) else {
            continue;
        };
        let Some(root) = validate_asset_workspace_root(home, raw) else {
            continue;
        };
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    roots
}

/// workspace 根校验：非空/非控制字符、绝对路径、存在且是目录、canonical 后不在
/// LINGXI_HOME 内、不是文件系统根（`/`）。任何一条不满足 → None（拒绝授权）。
fn validate_asset_workspace_root(home: &Path, raw: &str) -> Option<PathBuf> {
    if raw.is_empty() || raw.len() > 4096 || raw.chars().any(char::is_control) {
        return None;
    }
    let candidate = PathBuf::from(raw);
    if !candidate.is_absolute() {
        return None;
    }
    let canonical = std::fs::canonicalize(&candidate).ok()?;
    if !std::fs::metadata(&canonical).ok()?.is_dir() {
        return None;
    }
    if canonical == Path::new("/") {
        return None;
    }
    let canonical_home = std::fs::canonicalize(home).ok()?;
    if canonical == canonical_home || canonical.starts_with(&canonical_home) {
        return None;
    }
    Some(canonical)
}

/// setup 阶段：只做状态初始化；服务启动放异步任务（不阻塞事件循环）。
pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let env = crate::service::env::EnvSnapshot::from_process();
    let home = home_for_diagnostics(&env)?;
    // v2 Patch §1：getFileUrl/convertFileSrc 的 asset 协议运行时授权。
    install_asset_protocol_scope(app, &home);
    let debug_build = cfg!(debug_assertions);
    let packaged = !debug_build;
    let current_exe = std::env::current_exe()?;
    let app_version = app.package_info().version.to_string();
    let dev_default_binary = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../rust/target/debug")
        .join(if cfg!(windows) {
            "lingxi-service.exe"
        } else {
            "lingxi-service"
        });
    let diagnostics = Diagnostics::for_home(&home);
    diagnostics.log(
        "host-start",
        json!({
            "pid": std::process::id(),
            "currentExe": current_exe.display().to_string(),
            "home": home.display().to_string(),
            "packaged": packaged,
            "debugBuild": debug_build,
            "appVersion": app_version,
            "platform": resolve::platform_name(),
        }),
    );
    // 自动化模式：不出现 Dock 图标/不激活应用（macOS Accessory）；生产默认不变。
    #[cfg(all(target_os = "macos", feature = "r09-test-hooks"))]
    if automation_mode() {
        if let Err(error) = app
            .handle()
            .set_activation_policy(tauri::ActivationPolicy::Accessory)
        {
            diagnostics.log(
                "activation-policy-suppressed",
                json!({"error": error.to_string()}),
            );
        }
    }
    let runtime = ServiceRuntime::new_with_diagnostics(
        SupervisorConfig {
            home: home.clone(),
            env,
            packaged,
            debug_build,
            app_version: app_version.clone(),
            current_exe,
            dev_default_binary,
            timings: Timings::default(),
        },
        diagnostics.clone(),
    );
    let app_handle = app.handle().clone();
    let emitter = app_handle.clone();
    runtime.set_event_sink(Arc::new(move |event: &str, payload: Value| {
        if let Err(error) = emitter.emit(event, payload) {
            eprintln!("[tauri-host] emit {event} failed: {error}");
        }
    }));
    let fatal_handle = app_handle.clone();
    let fatal_diagnostics = diagnostics.clone();
    let fatal_home = home.clone();
    runtime.set_fatal_handler(Arc::new(move |error: &StartError| {
        show_failure_dialog(
            fatal_handle.clone(),
            error.clone(),
            fatal_home.clone(),
            fatal_diagnostics.clone(),
        );
    }));
    if let Some(state) = app.try_state::<HostState>() {
        state.install(runtime.clone(), diagnostics.clone());
    }

    #[cfg(unix)]
    install_signal_handlers(app_handle.clone());

    let start_runtime = runtime.clone();
    tauri::async_runtime::spawn(async move {
        match start_runtime.start_and_wait().await {
            Ok(connection) => {
                start_runtime.diagnostics().log(
                    "host-service-ready",
                    json!({
                        "mode": connection.mode.as_str(),
                        "port": connection.port,
                        "pid": connection.pid,
                    }),
                );
                let window_runtime = start_runtime.clone();
                let window_handle = app_handle.clone();
                if let Err(error) = app_handle.run_on_main_thread(move || {
                    if let Err(error) = create_main_window(&window_handle, &window_runtime) {
                        window_runtime
                            .diagnostics()
                            .log("window-create-failed", json!({"error": error}));
                        let diagnostics = window_runtime.diagnostics().clone();
                        show_failure_dialog(
                            window_handle.clone(),
                            StartError::new("TAURI_WINDOW_CREATE_FAILED", error),
                            window_runtime.home().to_path_buf(),
                            diagnostics,
                        );
                    }
                }) {
                    start_runtime.diagnostics().log(
                        "run-on-main-thread-failed",
                        json!({"error": error.to_string()}),
                    );
                }
            }
            Err(error) => {
                // 失败记录/诊断/弹窗已由 runtime 的 fatal handler 完成；这里只补退出兜底。
                start_runtime.diagnostics().log(
                    "host-startup-failed",
                    json!({"reason": error.code, "message": error.message}),
                );
            }
        }
    });
    Ok(())
}

/// 服务就绪后在主线程创建隐藏主窗口；`app_ready` 命令负责显示。
fn create_main_window(app: &tauri::AppHandle, runtime: &Arc<ServiceRuntime>) -> Result<(), String> {
    if app.get_webview_window(MAIN_WINDOW_LABEL).is_some() {
        return Ok(());
    }
    let diagnostics = runtime.diagnostics().clone();
    let page_diagnostics = diagnostics.clone();
    let builder = tauri::WebviewWindowBuilder::new(
        app,
        MAIN_WINDOW_LABEL,
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title(MAIN_WINDOW_TITLE)
    .inner_size(960.0, 820.0)
    .min_inner_size(420.0, 500.0)
    .visible(false)
    .center()
    .on_page_load(move |window, payload| {
        page_diagnostics.log(
            "page-load",
            json!({
                "label": window.label(),
                "url": payload.url().to_string(),
                "phase": format!("{:?}", payload.event()),
            }),
        );
    });
    // A01 证据：debug 构建在页面脚本之前注入错误/资源探针，DOM 投影一并回传。
    // 这是构建期调试能力（release 不注入），用于把 ErrorBoundary 兜底归因到
    // 具体异常/成员/资源，而不是靠猜。
    #[cfg(feature = "r09-test-hooks")]
    let builder = builder.initialization_script(
        r#"(function(){
  window.__r09Errors = [];
  window.__r09Resources = [];
  try {
    window.addEventListener('error', function(event){
      var target = event && event.target;
      window.__r09Errors.push({
        type: 'error',
        message: String(event && (event.message || (event.error && event.error.message)) || ''),
        source: event && event.filename ? String(event.filename) : null,
        line: event && event.lineno ? event.lineno : null,
        stack: event && event.error && event.error.stack ? String(event.error.stack).slice(0, 1200) : null,
        resource: target && target.tagName ? target.tagName + ':' + String(target.src || target.href || '') : null
      });
    }, true);
    window.addEventListener('unhandledrejection', function(event){
      var reason = event && event.reason;
      window.__r09Errors.push({
        type: 'unhandledrejection',
        message: String(reason && (reason.message || reason) || ''),
        stack: reason && reason.stack ? String(reason.stack).slice(0, 1200) : null
      });
    });
    var originalError = console.error ? console.error.bind(console) : function(){};
    console.error = function(){
      try {
        window.__r09Errors.push({
          type: 'console.error',
          message: Array.prototype.map.call(arguments, function(value){
            return String(value && (value.message || value) || '');
          }).join(' ').slice(0, 800)
        });
      } catch (_) {}
      return originalError.apply(console, arguments);
    };
    window.addEventListener('load', function(){
      try {
        window.__r09Resources = performance.getEntriesByType('resource').map(function(entry){
          return { name: String(entry.name), duration: Math.round(entry.duration), transferSize: entry.transferSize };
        });
      } catch (_) {}
    });
  } catch (_) {}
})();"#,
    );
    let window = builder.build().map_err(|error| error.to_string())?;

    diagnostics.log(
        "window-created",
        json!({
            "label": window.label(),
            "url": window.url().map(|url| url.to_string()).unwrap_or_default(),
            "title": window.title().unwrap_or_default(),
            "visible": window.is_visible().unwrap_or(false),
            "msSinceHostStart": app
                .try_state::<HostState>()
                .map(|state| state.note_window_created())
                .unwrap_or(0),
        }),
    );

    // 与 Electron `main.cjs:3184-3199` 同语义的 30s app-ready 兜底：
    // 页面初始化失败也不能把用户卡在不可见窗口。
    let fallback_window = window.clone();
    let fallback_diagnostics = diagnostics.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(APP_READY_FALLBACK).await;
        let visible = fallback_window.is_visible().unwrap_or(false);
        if !visible {
            fallback_diagnostics.log(
                "app-ready-timeout",
                json!({"label": fallback_window.label(), "timeoutMs": APP_READY_FALLBACK.as_millis() as u64,
                       "automation": automation_mode()}),
            );
            if !automation_mode() {
                let _ = fallback_window.show();
            }
        }
    });
    Ok(())
}

/// 自动化/测试模式：`LINGXI_R09_AUTOMATION=1` 时禁止任何用户可见 UI。
///
/// R09-T06 D11：这是**测试注入点**，只在 `r09-test-hooks` feature 构建生效；
/// 交付 release 不编译该分支，二进制不含 env 名（A12）。
#[cfg(feature = "r09-test-hooks")]
fn automation_mode() -> bool {
    std::env::var("LINGXI_R09_AUTOMATION")
        .map(|value| value == "1")
        .unwrap_or(false)
}

#[cfg(not(feature = "r09-test-hooks"))]
fn automation_mode() -> bool {
    false
}

/// 启动失败弹窗。自动化模式下改为把结构化 JSON 写入
/// `LINGXI_R09_HOST_DIALOG_SINK`（或仅写诊断事件），绝不弹 modal。
fn show_failure_dialog(
    app: tauri::AppHandle,
    error: StartError,
    home: std::path::PathBuf,
    diagnostics: Diagnostics,
) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    #[cfg(not(feature = "r09-test-hooks"))]
    let _ = &home;
    // 失败记录路径可能落在 home（已 stamp）或 TMPDIR fallback（未 stamp 的新 home）。
    let detail_path = diagnostics.failure_path();
    // 自动化/测试 sink 只在 r09-test-hooks 构建存在（D11）；交付 release 无该分支。
    #[cfg(feature = "r09-test-hooks")]
    if automation_mode() {
        let payload = json!({
            "schema": "lingxi.r09.host-failure-dialog.v1",
            "code": error.code,
            "message": error.message,
            "expected": error.expected,
            "actual": error.actual,
            "detailPath": detail_path.display().to_string(),
            "home": home.display().to_string(),
            "modalShown": false,
            "atUnixMs": unix_ms(),
        });
        diagnostics.log("failure-dialog-suppressed", payload.clone());
        match std::env::var_os("LINGXI_R09_HOST_DIALOG_SINK") {
            Some(sink) => {
                let sink = std::path::PathBuf::from(sink);
                if let Some(parent) = sink.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match serde_json::to_string_pretty(&payload) {
                    Ok(text) => {
                        if let Err(error) = std::fs::write(&sink, format!("{text}\n")) {
                            eprintln!("[tauri-host] automation dialog sink write failed: {error}");
                        } else {
                            eprintln!(
                                "[tauri-host] automation dialog suppressed; sink: {}",
                                sink.display()
                            );
                        }
                    }
                    Err(error) => {
                        eprintln!("[tauri-host] automation dialog serialize failed: {error}")
                    }
                }
            }
            None => {
                eprintln!("[tauri-host] automation mode without sink: failure dialog suppressed")
            }
        }
        // 自动化路径不等待任何 UI，直接以非零码退出（失败记录已先落盘）。
        app.exit(2);
        return;
    }
    // 生产路径：真实 home 路径必须插值（旧模板曾把 "{LINGXI_HOME}" 当字面量）。
    let message = format!(
        "{}\n\n{}\n\n诊断记录：{}",
        error.code,
        error.message,
        detail_path.display()
    );
    let dialog_handle = app.clone();
    std::thread::spawn(move || {
        dialog_handle
            .dialog()
            .message(message)
            .title("Lingxi 启动失败")
            .kind(MessageDialogKind::Error)
            .show(move |_dismissed| {
                dialog_handle.exit(2);
            });
    });
    let watchdog = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(15));
        watchdog.exit(2);
    });
}

#[cfg(feature = "r09-test-hooks")]
fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(unix)]
fn install_signal_handlers(app: tauri::AppHandle) {
    use std::sync::atomic::AtomicBool;
    static SIGNAL_RECEIVED: AtomicBool = AtomicBool::new(false);

    extern "C" fn on_signal(_signal: libc::c_int) {
        SIGNAL_RECEIVED.store(true, Ordering::SeqCst);
    }
    unsafe {
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGHUP, on_signal as *const () as libc::sighandler_t);
    }
    std::thread::spawn(move || loop {
        if SIGNAL_RECEIVED.swap(false, Ordering::SeqCst) {
            if let Some(state) = app.try_state::<HostState>() {
                if let Some(diagnostics) = state.diagnostics() {
                    diagnostics.log("signal", json!({"signal": "SIGTERM|SIGINT|SIGHUP"}));
                }
            }
            shutdown_and_exit(app);
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    });
}

/// 统一退出路径：只有 owned 服务会被清理；reused/remote 结构性零接触。
pub fn shutdown_and_exit(app: tauri::AppHandle) {
    static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);
    if SHUTTING_DOWN.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let (report, diagnostics) = match app.try_state::<HostState>() {
            Some(state) => {
                let diagnostics = state.diagnostics();
                let report = match state.runtime() {
                    Some(runtime) => runtime.stop().await,
                    None => ShutdownReport::detached("none"),
                };
                (report, diagnostics)
            }
            None => (ShutdownReport::detached("none"), None),
        };
        if let Some(diagnostics) = diagnostics {
            diagnostics.log("shutdown-complete", report.to_json());
        }
        app.exit(0);
    });
}

/// RunEvent 退出钩子：用户关闭最后窗口/Cmd+Q/OS 登出统一走清理路径。
pub fn on_run_event(app: &tauri::AppHandle, event: &RunEvent) {
    if let RunEvent::ExitRequested { api, code, .. } = event {
        // code = Some(..) 是程序化 exit（清理路径已经跑完），直接放行；
        // code = None 是用户/系统发起的退出，必须先清理 owned 服务。
        if code.is_none() {
            api.prevent_exit();
            shutdown_and_exit(app.clone());
        }
    }
}

/// 事件 payload 脱敏的守卫：`server-restarted` 只允许主窗口订阅（T06 收口）。
pub fn redact_event_for_evidence(payload: &Value) -> Value {
    let mut redacted = payload.clone();
    if let Some(object) = redacted.as_object_mut() {
        if object.contains_key("token") {
            object.insert("token".to_string(), json!("[redacted]"));
        }
    }
    redacted
}

#[cfg(test)]
mod asset_scope_tests {
    use super::validate_asset_workspace_root;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "lingxi-r09-asset-scope-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp root");
        root
    }

    #[test]
    fn asset_scope_rejects_invalid_workspace_roots() {
        let base = temp_root("invalid");
        let home = base.join("home");
        let inside = home.join("workspace");
        std::fs::create_dir_all(&inside).expect("inside dir");
        let outside = base.join("outside");
        std::fs::create_dir_all(&outside).expect("outside dir");
        let file = base.join("file.txt");
        std::fs::write(&file, b"x").expect("file");

        // 相对路径 / 空串 / 控制字符 / 不存在 / 普通文件 / 文件系统根
        assert_eq!(validate_asset_workspace_root(&home, "relative/dir"), None);
        assert_eq!(validate_asset_workspace_root(&home, ""), None);
        assert_eq!(validate_asset_workspace_root(&home, "/tmp/a\u{0}b"), None);
        assert_eq!(
            validate_asset_workspace_root(&home, "/no/such/r09/dir"),
            None
        );
        assert_eq!(
            validate_asset_workspace_root(&home, file.to_str().unwrap()),
            None
        );
        assert_eq!(validate_asset_workspace_root(&home, "/"), None);
        // LINGXI_HOME 自身 / home 内子目录 → 拒绝（home 由 install_asset_protocol_scope 单独授权）
        assert_eq!(
            validate_asset_workspace_root(&home, home.to_str().unwrap()),
            None
        );
        assert_eq!(
            validate_asset_workspace_root(&home, inside.to_str().unwrap()),
            None
        );
        // home 外真实目录 → 允许（canonical 化）
        assert_eq!(
            validate_asset_workspace_root(&home, outside.to_str().unwrap()),
            Some(std::fs::canonicalize(&outside).unwrap())
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
