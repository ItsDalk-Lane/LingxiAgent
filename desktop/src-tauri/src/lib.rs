pub mod browser_viewer;
pub mod commands;
pub mod files;
#[cfg_attr(not(feature = "r09-host"), allow(dead_code, unused_imports))]
pub mod host;
pub mod native;
pub mod native_commands;
pub mod observability_export;
pub mod resource_bridge;
pub mod service;
pub mod skills;
pub mod update_about;

use browser_viewer::{
    close_browser_viewer, get_browser_viewer_context, open_browser_viewer, rust_browser_request,
    set_browser_connection,
};
use commands::{
    app_ready, get_app_version, get_platform, get_server_connection_info, get_server_port,
    get_server_token, get_service_status, open_external, window_close, window_is_maximized,
    window_maximize, window_minimize, window_theme_changed,
};
use files::{FileInfo, FileVersion, SaveResult, TextSnapshot};
use native_commands::*;
// R10：`#[tauri::command]` 的 `__cmd__*` 宏在 crate root 导出；generate_handler!
// 只接受裸函数名（模块路径会去找 `update_about::__cmd__*` 而编译失败）。
use observability_export::{
    observability_export_abort, observability_export_begin, observability_export_end,
    observability_export_write,
};
use resource_bridge::ResourceConnection;
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tauri::{Emitter, Manager, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use update_about::{
    ack_announcement, get_pending_announcement, get_update_digest_history, release_check_latest,
};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ViewerPayload {
    file_path: String,
    title: String,
    read_only: bool,
}

#[derive(Clone)]
struct Subscription {
    window: String,
    keys: Vec<String>,
}

#[derive(Default)]
struct DesktopState {
    viewers: Mutex<HashMap<String, ViewerPayload>>,
    skill_viewers: Mutex<HashMap<String, skills::SkillViewerPayload>>,
    subscriptions: Mutex<HashMap<String, Subscription>>,
    legacy_watches: Mutex<HashSet<(String, String)>>,
    /// v3：workspace 监听（按调用窗口 label 隔离；轮询实现，无 notify 重依赖）。
    workspace_watches: Mutex<HashSet<(String, String)>>,
    next_viewer: AtomicU64,
}

fn main_only(window: &WebviewWindow) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("只读窗口不允许修改或执行系统操作".into())
    }
}

fn readable(window: &WebviewWindow, state: &DesktopState, path: &str) -> Result<(), String> {
    files::absolute(path).map_err(|e| e.to_string())?;
    if window.label() == "main" {
        return Ok(());
    }
    let viewers = state.viewers.lock().map_err(|_| "窗口状态不可用")?;
    if viewers
        .get(window.label())
        .is_some_and(|v| v.file_path == path)
    {
        Ok(())
    } else {
        Err("此只读窗口只能读取自身文件".into())
    }
}

fn connection() -> Result<ResourceConnection, String> {
    let home = std::env::var_os("LINGXI_RESOURCE_SERVICE_HOME")
        .ok_or("请先启动 Rust 服务并设置 LINGXI_RESOURCE_SERVICE_HOME")?;
    ResourceConnection::from_home(&PathBuf::from(home))
}

/// R09-T03：文档预览使用宿主自身的 LINGXI_HOME（服务实例记录在该目录），
/// 不依赖旧 R06 resource bridge 的 `LINGXI_RESOURCE_SERVICE_HOME`。
pub(crate) fn resource_home() -> Result<PathBuf, String> {
    if let Some(raw) = std::env::var_os("LINGXI_RESOURCE_SERVICE_HOME") {
        return Ok(PathBuf::from(raw));
    }
    if let Some(raw) = std::env::var_os("LINGXI_HOME") {
        return Ok(PathBuf::from(raw));
    }
    crate::service::resolve::home_from_env(&crate::service::env::EnvSnapshot::from_process())
        .map_err(|error| format!("无法解析 LINGXI_HOME：{error}"))
}

fn office_preview_path_allowed(path: &str) -> bool {
    const MAX_OFFICE_BYTES: u64 = 20 * 1024 * 1024;
    let candidate = std::path::Path::new(path);
    if !candidate.is_absolute() {
        return false;
    }
    let Ok(metadata) = std::fs::symlink_metadata(candidate) else {
        return false;
    };
    metadata.file_type().is_file() && !metadata.file_type().is_symlink() && metadata.len() <= MAX_OFFICE_BYTES
}

async fn office_preview_html(kind: &str, path: &str) -> Result<Option<String>, String> {
    // Electron 语义：相对路径/不存在/超 20MB/symlink 返回 null，不抛错、不建窗口。
    if !office_preview_path_allowed(path) {
        return Ok(None);
    }
    let connection = ResourceConnection::from_home(&resource_home()?)?;
    connection.office_html(kind, path).await
}

#[tauri::command]
async fn read_docx_html(window: WebviewWindow, path: String) -> Result<Option<String>, String> {
    main_only(&window)?;
    office_preview_html("docx", &path).await
}

#[tauri::command]
async fn read_xlsx_html(window: WebviewWindow, path: String) -> Result<Option<String>, String> {
    main_only(&window)?;
    office_preview_html("xlsx", &path).await
}

#[tauri::command]
async fn select_files(
    window: WebviewWindow,
    multiple: Option<bool>,
) -> Result<Vec<String>, String> {
    main_only(&window)?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let dialog = window.dialog().file().set_parent(&window);
    if multiple.unwrap_or(true) {
        dialog.pick_files(move |paths| {
            let _ = tx.send(
                paths
                    .unwrap_or_default()
                    .into_iter()
                    .map(|p| p.to_string())
                    .collect(),
            );
        });
    } else {
        dialog.pick_file(move |path| {
            let _ = tx.send(path.into_iter().map(|p| p.to_string()).collect());
        });
    }
    rx.await.map_err(|_| "原生文件选择器未返回".into())
}

#[tauri::command]
async fn select_folder(window: WebviewWindow) -> Result<Option<String>, String> {
    main_only(&window)?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    window
        .dialog()
        .file()
        .set_parent(&window)
        .pick_folder(move |path| {
            let _ = tx.send(path.map(|path| path.to_string()));
        });
    rx.await.map_err(|_| "原生目录选择器未返回".into())
}

#[tauri::command]
fn read_file(
    window: WebviewWindow,
    state: State<'_, Arc<DesktopState>>,
    path: String,
) -> Option<String> {
    readable(&window, &state, &path).ok()?;
    files::snapshot(&path).ok().map(|s| s.content)
}
#[tauri::command]
fn read_file_snapshot(
    window: WebviewWindow,
    state: State<'_, Arc<DesktopState>>,
    path: String,
) -> Option<TextSnapshot> {
    readable(&window, &state, &path).ok()?;
    files::snapshot(&path).ok()
}
#[tauri::command]
fn read_file_base64(
    window: WebviewWindow,
    state: State<'_, Arc<DesktopState>>,
    path: String,
) -> Option<String> {
    readable(&window, &state, &path).ok()?;
    files::base64(&path).ok()
}
#[tauri::command]
fn file_info(
    window: WebviewWindow,
    state: State<'_, Arc<DesktopState>>,
    path: String,
) -> Option<FileInfo> {
    readable(&window, &state, &path).ok()?;
    files::info(&path).ok()
}
#[tauri::command]
fn write_file(window: WebviewWindow, path: String, content: String) -> Result<bool, String> {
    main_only(&window)?;
    Ok(files::write(&path, &content))
}
#[tauri::command]
fn write_file_binary(window: WebviewWindow, path: String, data: String) -> Result<bool, String> {
    main_only(&window)?;
    let home = window
        .app_handle()
        .path()
        .home_dir()
        .map_err(|e| e.to_string())?;
    Ok(files::write_binary(&path, &data, &home))
}
#[tauri::command]
fn copy_file(window: WebviewWindow, source: String, destination: String) -> Result<bool, String> {
    main_only(&window)?;
    Ok(files::copy(&source, &destination))
}
#[tauri::command]
fn write_file_if_unchanged(
    window: WebviewWindow,
    path: String,
    content: String,
    expected_version: Option<FileVersion>,
) -> Result<SaveResult, String> {
    main_only(&window)?;
    Ok(files::save(&path, &content, expected_version.as_ref()))
}
#[tauri::command]
async fn open_file(window: WebviewWindow, path: String) -> Result<bool, String> {
    // 外部打开命令按 T03 窗口门（main/settings；R06 语义保持）。
    native::external_open::plan_open_file(window.label(), &path)
        .map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || files::open_file(&path).map(|_| true))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn trash_item(window: WebviewWindow, path: String) -> Result<bool, String> {
    main_only(&window)?;
    tauri::async_runtime::spawn_blocking(move || files::trash_item(&path))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn run_edit_command(window: WebviewWindow, command: String) -> Result<bool, String> {
    let script = files::edit_script(&command)?;
    main_only(&window)?;
    // 指向调用窗口的真实 WebView，不接受渲染侧传入脚本或系统命令。
    #[cfg(target_os = "macos")]
    {
        use objc2::{msg_send, runtime::AnyObject, sel};
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = script;
        window.with_webview(move |webview| {
            let target = webview.inner().cast::<AnyObject>();
            let dispatched = unsafe {
                let native_window: *mut AnyObject = msg_send![target, window];
                let responder: *mut AnyObject = msg_send![native_window, firstResponder];
                let selector = match command.as_str() { "cut" => sel!(cut:), "copy" => sel!(copy:), "paste" => sel!(paste:), "selectAll" => sel!(selectAll:), _ => unreachable!("白名单已检查") };
                if responder.is_null() { false } else { msg_send![responder, tryToPerform: selector, with: Option::<&AnyObject>::None] }
            };
            let _ = tx.send(dispatched);
        }).map_err(|e| e.to_string())?;
        rx.await.map_err(|_| "编辑命令未派发".into())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        window
            .eval_with_callback(script, move |result| {
                let _ = tx.try_send(result);
            })
            .map_err(|e| e.to_string())?;
        let result = tokio::time::timeout(Duration::from_secs(3), rx.recv())
            .await
            .map_err(|_| "编辑命令超时")?
            .ok_or("编辑命令未返回")?;
        Ok(result == "true")
    }
}

#[tauri::command]
fn watch_file(
    window: WebviewWindow,
    state: State<'_, Arc<DesktopState>>,
    path: String,
) -> Result<bool, String> {
    main_only(&window)?;
    if files::absolute(&path).is_err() {
        return Ok(false);
    }
    let key = (window.label().to_owned(), path.clone());
    if !state
        .legacy_watches
        .lock()
        .map_err(|_| "监听状态不可用")?
        .insert(key.clone())
    {
        return Ok(true);
    }
    let state = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        let signature = |path: &str| {
            std::fs::metadata(path).ok().map(|m| {
                (
                    m.len(),
                    m.modified().ok(),
                    files::snapshot(path).ok().map(|s| s.version.sha256),
                )
            })
        };
        let mut previous = signature(&path);
        loop {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if !state.legacy_watches.lock().unwrap().contains(&key) {
                break;
            }
            let next = signature(&path);
            if next != previous {
                previous = next;
                let _ = window.emit("file-changed", json!({"filePath":path}));
            }
        }
    });
    Ok(true)
}
#[tauri::command]
fn unwatch_file(
    window: WebviewWindow,
    state: State<'_, Arc<DesktopState>>,
    path: String,
) -> Result<bool, String> {
    main_only(&window)?;
    state
        .legacy_watches
        .lock()
        .map_err(|_| "监听状态不可用")?
        .remove(&(window.label().into(), path));
    Ok(true)
}

#[tauri::command]
async fn resource_subscribe(
    window: WebviewWindow,
    state: State<'_, Arc<DesktopState>>,
    path: String,
) -> Result<Value, String> {
    readable(&window, &state, &path)?;
    let result = connection()?.subscribe(&path).await?;
    let id = result["subscriptionId"]
        .as_str()
        .ok_or("资源服务没有返回订阅身份")?
        .to_owned();
    let keys = result["resourceKeys"]
        .as_array()
        .ok_or("资源服务没有返回资源身份")?
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    // 登记与窗口销毁的释放共用这把锁，避免请求返回前关窗留下孤立订阅。
    let retained = {
        let mut subscriptions = state.subscriptions.lock().map_err(|_| "订阅状态不可用")?;
        if window
            .app_handle()
            .get_webview_window(window.label())
            .is_none()
        {
            false
        } else {
            subscriptions.insert(
                id.clone(),
                Subscription {
                    window: window.label().into(),
                    keys,
                },
            );
            true
        }
    };
    if !retained {
        connection()?.unsubscribe(&id).await?;
        return Err("预览窗口已关闭".into());
    }
    Ok(result)
}
#[tauri::command]
async fn resource_events(
    window: WebviewWindow,
    state: State<'_, Arc<DesktopState>>,
    since: u64,
) -> Result<Value, String> {
    let keys: HashSet<String> = state
        .subscriptions
        .lock()
        .map_err(|_| "订阅状态不可用")?
        .values()
        .filter(|s| s.window == window.label())
        .flat_map(|s| s.keys.iter().cloned())
        .collect();
    let mut result = connection()?.events(since).await?;
    if let Some(events) = result["events"].as_array_mut() {
        events.retain(|e| {
            ["resourceKey", "oldResourceKey", "newResourceKey"]
                .iter()
                .any(|k| e[*k].as_str().is_some_and(|k| keys.contains(k)))
        });
    }
    Ok(result)
}
#[tauri::command]
async fn resource_unsubscribe(
    window: WebviewWindow,
    state: State<'_, Arc<DesktopState>>,
    subscription_id: String,
) -> Result<Value, String> {
    let owned = state
        .subscriptions
        .lock()
        .map_err(|_| "订阅状态不可用")?
        .get(&subscription_id)
        .is_some_and(|s| s.window == window.label());
    if !owned {
        return Err("订阅不属于此窗口".into());
    }
    let result = connection()?.unsubscribe(&subscription_id).await?;
    state
        .subscriptions
        .lock()
        .map_err(|_| "订阅状态不可用")?
        .remove(&subscription_id);
    Ok(result)
}

/// v3：workspace 监听（Electron `main.cjs:6444-6460` 最小等价）。
///
/// Electron 用 chokidar `depth:0` 监听根目录直接子项，80ms 去抖后向订阅窗口发
/// `workspace-changed` payload `{rootPath, changedPath, affectedDir, eventType}`。
/// Tauri 侧不引入 notify 重依赖：250ms 轮询直接子项签名，按 add/unlink/change
/// 与 addDir/unlinkDir 分类；订阅按窗口 label 隔离，窗口销毁自动清理。
type WorkspaceSignature = std::collections::BTreeMap<String, (bool, u64, i64)>;

fn workspace_root(path: &str) -> Result<PathBuf, String> {
    if path.is_empty() || path.len() > 4096 || path.chars().any(|c| c.is_control()) {
        return Err("workspace 路径形态非法".to_string());
    }
    let root = PathBuf::from(path);
    if !root.is_absolute() {
        return Err("workspace 路径必须是绝对路径".to_string());
    }
    let metadata = std::fs::symlink_metadata(&root).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("workspace 根不能是符号链接".to_string());
    }
    if !metadata.is_dir() {
        return Err("workspace 根必须是目录".to_string());
    }
    Ok(root)
}

fn workspace_ignored(name: &str) -> bool {
    if name.starts_with('.') {
        return true;
    }
    matches!(
        name,
        "node_modules" | "dist" | "build" | "coverage" | ".next" | ".turbo"
    )
}

fn workspace_signature(root: &PathBuf) -> WorkspaceSignature {
    let mut signature = WorkspaceSignature::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return signature;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if workspace_ignored(&name) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let modified_ms = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(0);
        signature.insert(name, (metadata.is_dir(), metadata.len(), modified_ms));
    }
    signature
}

#[tauri::command]
fn watch_workspace(
    window: WebviewWindow,
    state: State<'_, Arc<DesktopState>>,
    root_path: String,
) -> Result<bool, String> {
    main_only(&window)?;
    let root = workspace_root(&root_path)?;
    let key = (
        window.label().to_string(),
        root.to_string_lossy().into_owned(),
    );
    if !state
        .workspace_watches
        .lock()
        .map_err(|_| "workspace 监听状态不可用")?
        .insert(key.clone())
    {
        return Ok(true);
    }
    let state_handle = state.inner().clone();
    let window_handle = window.clone();
    let root_handle = root.clone();
    tauri::async_runtime::spawn(async move {
        let mut previous = workspace_signature(&root_handle);
        loop {
            tokio::time::sleep(Duration::from_millis(250)).await;
            let active = state_handle
                .workspace_watches
                .lock()
                .map(|watches| watches.contains(&key))
                .unwrap_or(false);
            if !active {
                break;
            }
            let next = workspace_signature(&root_handle);
            for (name, next_entry) in &next {
                let changed_path = root_handle.join(name);
                match previous.get(name) {
                    None => {
                        let event_type = if next_entry.0 { "addDir" } else { "add" };
                        let _ = window_handle.emit(
                            "workspace-changed",
                            json!({
                                "rootPath": root_handle.to_string_lossy(),
                                "changedPath": changed_path.to_string_lossy(),
                                "affectedDir": root_handle.to_string_lossy(),
                                "eventType": event_type,
                            }),
                        );
                    }
                    Some(previous_entry) if previous_entry != next_entry && !next_entry.0 => {
                        let _ = window_handle.emit(
                            "workspace-changed",
                            json!({
                                "rootPath": root_handle.to_string_lossy(),
                                "changedPath": changed_path.to_string_lossy(),
                                "affectedDir": root_handle.to_string_lossy(),
                                "eventType": "change",
                            }),
                        );
                    }
                    _ => {}
                }
            }
            for (name, previous_entry) in &previous {
                if next.contains_key(name) {
                    continue;
                }
                let event_type = if previous_entry.0 {
                    "unlinkDir"
                } else {
                    "unlink"
                };
                let changed_path = root_handle.join(name);
                let _ = window_handle.emit(
                    "workspace-changed",
                    json!({
                        "rootPath": root_handle.to_string_lossy(),
                        "changedPath": changed_path.to_string_lossy(),
                        "affectedDir": root_handle.to_string_lossy(),
                        "eventType": event_type,
                    }),
                );
            }
            previous = next;
        }
    });
    Ok(true)
}

#[tauri::command]
fn unwatch_workspace(
    window: WebviewWindow,
    state: State<'_, Arc<DesktopState>>,
    root_path: String,
) -> Result<bool, String> {
    main_only(&window)?;
    let root = workspace_root(&root_path)?;
    let key = (
        window.label().to_string(),
        root.to_string_lossy().into_owned(),
    );
    state
        .workspace_watches
        .lock()
        .map_err(|_| "workspace 监听状态不可用")?
        .remove(&key);
    Ok(true)
}

#[tauri::command]
fn spawn_viewer(
    window: WebviewWindow,
    state: State<'_, Arc<DesktopState>>,
    path: String,
) -> Result<String, String> {
    main_only(&window)?;
    files::info(&path).map_err(|e| e.to_string())?;
    let label = format!(
        "viewer-{}",
        state.next_viewer.fetch_add(1, Ordering::Relaxed)
    );
    let title = PathBuf::from(&path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "文件".into());
    let payload = ViewerPayload {
        file_path: path,
        title: title.clone(),
        read_only: true,
    };
    state
        .viewers
        .lock()
        .map_err(|_| "窗口状态不可用")?
        .insert(label.clone(), payload);
    match tauri::WebviewWindowBuilder::new(
        window.app_handle(),
        &label,
        tauri::WebviewUrl::App("viewer-window.html".into()),
    )
    .title(format!("{title} · 只读"))
    .inner_size(900.0, 680.0)
    .build()
    {
        Ok(_) => Ok(label),
        Err(e) => {
            state.viewers.lock().unwrap().remove(&label);
            Err(e.to_string())
        }
    }
}
#[tauri::command]
fn viewer_request_load(
    window: WebviewWindow,
    state: State<'_, Arc<DesktopState>>,
) -> Option<ViewerPayload> {
    state.viewers.lock().ok()?.get(window.label()).cloned()
}
#[tauri::command]
fn viewer_close(window: WebviewWindow) -> Result<(), String> {
    if !window.label().starts_with("viewer-") {
        return Err("只允许关闭当前独立预览窗口".into());
    }
    window.close().map_err(|e| e.to_string())
}

#[tauri::command]
fn open_skill_viewer(
    window: WebviewWindow,
    data: skills::SkillViewerPayload,
) -> Result<String, String> {
    // Electron 语义（main.cjs:3403/6141+）：打开技能不是新窗口，而是向主窗发
    // `show-skill-viewer` 事件由 React overlay 展示（payload: name/baseDir/installed[/filePath]）。
    main_only(&window)?;
    let base_dir = skills::root(&data.base_dir).ok_or("技能目录无效")?;
    let mut payload = serde_json::json!({
        "name": data.name,
        "baseDir": base_dir.to_string_lossy(),
        "installed": data.installed,
    });
    if let Some(file_path) = data.file_path.as_deref() {
        payload["filePath"] = serde_json::Value::String(file_path.to_string());
    }
    if let Some(main) = window.app_handle().get_webview_window("main") {
        main.emit("show-skill-viewer", payload)
            .map_err(|error| error.to_string())?;
        Ok("main".to_string())
    } else {
        Err("主窗口不存在，无法显示技能 overlay".to_string())
    }
}
#[tauri::command]
fn skill_viewer_load(
    window: WebviewWindow,
    state: State<'_, Arc<DesktopState>>,
) -> Option<skills::SkillViewerPayload> {
    state
        .skill_viewers
        .lock()
        .ok()?
        .get(window.label())
        .cloned()
}
#[tauri::command]
fn list_skill_files(
    window: WebviewWindow,
    state: State<'_, Arc<DesktopState>>,
    base_dir: String,
) -> Result<Vec<skills::TreeItem>, String> {
    if window.label() != "main"
        && !state.skill_viewers.lock().ok().is_some_and(|viewers| {
            viewers
                .get(window.label())
                .is_some_and(|payload| payload.base_dir == base_dir)
        })
    {
        return Ok(vec![]);
    }
    skills::list(&base_dir)
}
#[tauri::command]
fn read_skill_file(
    window: WebviewWindow,
    state: State<'_, Arc<DesktopState>>,
    path: String,
) -> Option<String> {
    if window.label() == "main" {
        return skills::read(&path, None);
    }
    let payload = state
        .skill_viewers
        .lock()
        .ok()?
        .get(window.label())
        .cloned()?;
    let root = skills::root(&payload.base_dir)?;
    skills::read(&path, Some(&root))
}
#[tauri::command]
fn skill_viewer_close(window: WebviewWindow) -> Result<(), String> {
    if !window.label().starts_with("skill-viewer-") {
        return Err("只允许关闭自己的技能窗口".into());
    }
    window.close().map_err(|error| error.to_string())
}

static WINDOW_STATE_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// T03：主窗 move/resize 500ms 防抖持久化；多次事件只落最后一次。
fn schedule_window_state_save(window: &WebviewWindow) {
    let generation = WINDOW_STATE_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let window = window.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if WINDOW_STATE_GENERATION.load(Ordering::SeqCst) != generation {
            return;
        }
        if let Err(error) = native::install::save_window_state(&window) {
            eprintln!("[native] window-state save failed: {error}");
        }
    });
}

/// R10/R11：reqwest 使用 `rustls-no-provider`，但服务健康检查在宿主启动早期就会
/// 构建 Client；provider 必须在任何 reqwest Client 之前安装，否则 release 包会在
/// tokio worker 里 panic（HTTP 请求并不需要 TLS，但 Client 构建仍会检查 provider）。
fn install_rustls_crypto_provider() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

pub fn run() {
    install_rustls_crypto_provider();
    // R09-T03/T04 native 插件组（single-instance/global-shortcut/notification/autostart）
    // 必须最先注册：single-instance 要求自己第一个 plugin。
    let builder = native::install::with_plugins(tauri::Builder::default())
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::new(DesktopState::default()))
        // R09 宿主命令的 state 始终存在；未启用 `r09-host` feature 时为空壳
        // （R06 文件资源宿主行为不变），启用后由 setup 注入 ServiceRuntime。
        .manage(commands::HostState::empty())
        // R09-T05 浏览器控制面板宿主桥（token 只留宿主 state，viewer 只拿不透明 key）。
        .manage(browser_viewer::BrowserViewerState::default())
        // R09-T03/T04 命令面的宿主侧句柄（keep-awake 后端等）。
        .manage(native_commands::CommandState::default())
        // R10：About 更新链缓存（release 60s / digest 10min）。
        .manage(update_about::UpdateAboutState::default())
        // R10：可观测导出的 capability-token 会话。
        .manage(observability_export::ObservabilityExportState::default())
        .setup(|app| {
            #[cfg(feature = "r09-host")]
            {
                host::setup(app)?;
            }
            #[cfg(not(feature = "r09-host"))]
            {
                let _ = app;
            }
            // R09-T03/T04：manage NativeState + 托盘 + 快捷聊天快捷键 + mac 通知点击。
            native::install::install(app).map_err(|error| {
                eprintln!("[native] install failed: {error}");
                Box::<dyn std::error::Error>::from(error)
            })?;
            Ok(())
        });
    // R09-T06 D11：debug 命令只在 r09-test-hooks feature 构建注册；交付 release 不含。
    #[cfg(not(feature = "r09-test-hooks"))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        open_skill_viewer,
        skill_viewer_load,
        list_skill_files,
        read_skill_file,
        skill_viewer_close,
        select_files,
        select_folder,
        read_file,
        read_file_snapshot,
        read_file_base64,
        read_docx_html,
        read_xlsx_html,
        file_info,
        write_file,
        write_file_binary,
        copy_file,
        write_file_if_unchanged,
        open_file,
        trash_item,
        run_edit_command,
        watch_file,
        unwatch_file,
        watch_workspace,
        unwatch_workspace,
        resource_subscribe,
        resource_events,
        resource_unsubscribe,
        spawn_viewer,
        viewer_request_load,
        viewer_close,
        get_server_connection_info,
        get_server_port,
        get_server_token,
        get_service_status,
        get_platform,
        get_app_version,
        app_ready,
        window_minimize,
        window_maximize,
        window_close,
        window_is_maximized,
        window_theme_changed,
        open_external,
        set_browser_connection,
        get_browser_viewer_context,
        open_browser_viewer,
        rust_browser_request,
        close_browser_viewer,
        reload_main_window,
        restart_app,
        open_settings,
        settings_changed,
        show_notification,
        select_skill,
        open_folder,
        show_in_finder,
        start_drag,
        get_keep_awake_status,
        set_keep_awake_enabled,
        get_auto_launch_status,
        set_auto_launch_enabled,
        keybindings_reload_global,
        quick_chat_reload_shortcut,
        keybindings_test_register,
        quick_chat_shortcut_status,
        quick_chat_show,
        quick_chat_hide,
        quick_chat_resize,
        quick_chat_open_session,
        quick_chat_open_session_ack,
        get_splash_info,
        get_avatar_path,
        onboarding_complete,
        speech_permission_status,
        speech_permission_request,
        release_check_latest,
        get_update_digest_history,
        get_pending_announcement,
        ack_announcement,
        observability_export_begin,
        observability_export_write,
        observability_export_end,
        observability_export_abort,
    ]);
    #[cfg(feature = "r09-test-hooks")]
    let builder = builder.invoke_handler(tauri::generate_handler![
        open_skill_viewer,
        skill_viewer_load,
        list_skill_files,
        read_skill_file,
        skill_viewer_close,
        select_files,
        select_folder,
        read_file,
        read_file_snapshot,
        read_file_base64,
        read_docx_html,
        read_xlsx_html,
        file_info,
        write_file,
        write_file_binary,
        copy_file,
        write_file_if_unchanged,
        open_file,
        trash_item,
        run_edit_command,
        watch_file,
        unwatch_file,
        watch_workspace,
        unwatch_workspace,
        resource_subscribe,
        resource_events,
        resource_unsubscribe,
        spawn_viewer,
        viewer_request_load,
        viewer_close,
        get_server_connection_info,
        get_server_port,
        get_server_token,
        get_service_status,
        get_platform,
        get_app_version,
        app_ready,
        window_minimize,
        window_maximize,
        window_close,
        window_is_maximized,
        window_theme_changed,
        open_external,
        set_browser_connection,
        get_browser_viewer_context,
        open_browser_viewer,
        rust_browser_request,
        close_browser_viewer,
        reload_main_window,
        restart_app,
        open_settings,
        settings_changed,
        show_notification,
        select_skill,
        open_folder,
        show_in_finder,
        start_drag,
        get_keep_awake_status,
        set_keep_awake_enabled,
        get_auto_launch_status,
        set_auto_launch_enabled,
        keybindings_reload_global,
        quick_chat_reload_shortcut,
        keybindings_test_register,
        quick_chat_shortcut_status,
        quick_chat_show,
        quick_chat_hide,
        quick_chat_resize,
        quick_chat_open_session,
        quick_chat_open_session_ack,
        get_splash_info,
        get_avatar_path,
        onboarding_complete,
        speech_permission_status,
        speech_permission_request,
        release_check_latest,
        get_update_digest_history,
        get_pending_announcement,
        ack_announcement,
        observability_export_begin,
        observability_export_write,
        observability_export_end,
        observability_export_abort,
        debug_open_onboarding,
        debug_open_onboarding_preview,
        debug_asset_probe
    ]);
    let builder = builder.on_window_event(|window, event| {
        match event {
            tauri::WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
                // T03 主窗关闭策略：托盘常驻时隐藏而非退出；从属窗一起隐藏。
                if native_commands::main_close_should_hide() {
                    api.prevent_close();
                    let _ = window.hide();
                    for label in native_commands::dependent_window_labels() {
                        if let Some(dependent) = window.app_handle().get_webview_window(&label) {
                            let _ = dependent.hide();
                        }
                    }
                }
            }
            tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_)
                if window.label() == "main" =>
            {
                // T03 窗口状态：move/resize 500ms 防抖后持久化（sanitize 在 install 内）。
                if let Some(webview) = window.app_handle().get_webview_window(window.label()) {
                    schedule_window_state_save(&webview);
                }
            }
            tauri::WindowEvent::Destroyed => {
                // R10：窗口销毁时关闭其未完成的导出会话并删除半成品。
                observability_export::cleanup_window(window.app_handle(), window.label());
                let state = window.state::<Arc<DesktopState>>();
                // v3：派生 viewer 关闭 → 向主窗发 Electron 同形 `viewer-closed`（数字 windowId）。
                if window.label().starts_with("viewer-") {
                    if let Some(suffix) = window.label().strip_prefix("viewer-") {
                        if let Ok(window_id) = suffix.parse::<u64>() {
                            if let Some(main) = window.app_handle().get_webview_window("main") {
                                let _ = main.emit("viewer-closed", window_id);
                            }
                        }
                    }
                }
                state.viewers.lock().unwrap().remove(window.label());
                state
                    .workspace_watches
                    .lock()
                    .unwrap()
                    .retain(|(label, _)| label != window.label());
                state.skill_viewers.lock().unwrap().remove(window.label());
                state
                    .legacy_watches
                    .lock()
                    .unwrap()
                    .retain(|(label, _)| label != window.label());
                let ids: Vec<String> = state
                    .subscriptions
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(_, s)| s.window == window.label())
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in &ids {
                    state.subscriptions.lock().unwrap().remove(id);
                }
                tauri::async_runtime::spawn(async move {
                    for id in ids {
                        if let Ok(c) = connection() {
                            if let Err(e) = c.unsubscribe(&id).await {
                                eprintln!("资源订阅释放失败：{e}");
                            }
                        }
                    }
                });
            }
            _ => {}
        }
    });
    #[cfg(feature = "e2e-test")]
    let builder = builder.plugin(tauri_plugin_wdio_webdriver::init());
    let app = builder
        .build(tauri::generate_context!())
        .expect("桌面宿主启动失败");
    app.run(|app_handle, event| {
        #[cfg(feature = "r09-host")]
        host::on_run_event(app_handle, &event);
        #[cfg(not(feature = "r09-host"))]
        {
            let _ = (app_handle, event);
        }
    });
}
