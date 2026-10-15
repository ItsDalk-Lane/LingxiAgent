pub mod files;
pub mod resource_bridge;
pub mod skills;

use files::{FileInfo, FileVersion, SaveResult, TextSnapshot};
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
    main_only(&window)?;
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
        tauri::WebviewUrl::App("index.html?viewer=1".into()),
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
    state: State<'_, Arc<DesktopState>>,
    data: skills::SkillViewerPayload,
) -> Result<String, String> {
    main_only(&window)?;
    let base = skills::root(&data.base_dir).ok_or("技能目录无效")?;
    let payload = skills::SkillViewerPayload {
        base_dir: base.to_string_lossy().into_owned(),
        ..data
    };
    let label = format!(
        "skill-viewer-{}",
        state.next_viewer.fetch_add(1, Ordering::Relaxed)
    );
    state
        .skill_viewers
        .lock()
        .map_err(|_| "窗口状态不可用")?
        .insert(label.clone(), payload.clone());
    match tauri::WebviewWindowBuilder::new(
        window.app_handle(),
        &label,
        tauri::WebviewUrl::App("index.html?skillViewer=1".into()),
    )
    .title(format!("{} · 技能", payload.name))
    .inner_size(1000.0, 720.0)
    .build()
    {
        Ok(_) => Ok(label),
        Err(error) => {
            state.skill_viewers.lock().unwrap().remove(&label);
            Err(error.to_string())
        }
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

pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::new(DesktopState::default()))
        .invoke_handler(tauri::generate_handler![
            open_skill_viewer,
            skill_viewer_load,
            list_skill_files,
            read_skill_file,
            skill_viewer_close,
            select_files,
            read_file,
            read_file_snapshot,
            read_file_base64,
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
            resource_subscribe,
            resource_events,
            resource_unsubscribe,
            spawn_viewer,
            viewer_request_load,
            viewer_close
        ])
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                let state = window.state::<Arc<DesktopState>>();
                state.viewers.lock().unwrap().remove(window.label());
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
        });
    #[cfg(feature = "e2e-test")]
    let builder = builder.plugin(tauri_plugin_wdio_webdriver::init());
    builder
        .run(tauri::generate_context!())
        .expect("资源桌面启动失败");
}
