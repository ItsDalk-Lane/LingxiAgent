// R01-T06 隔离 spike：Tauri v2 桌面宿主能力与安全边界原型。
// 不是生产入口。前端无任何 shell/任意执行权限；sidecar 由宿主 Rust 侧管理。

pub mod probes;
pub mod report;
pub mod sidecar;

use report::Reporter;
use sidecar::SharedSidecar;
use std::sync::Arc;

#[derive(serde::Serialize)]
pub struct PublicInfo {
    app: &'static str,
    spike: &'static str,
    platform: &'static str,
}

/// 良性命令：main 窗口授权；untrusted/remote 期望被 ACL 拒绝。
#[tauri::command]
async fn shell_public_info() -> Result<PublicInfo, String> {
    Ok(PublicInfo {
        app: "lingxi-t06-shell-spike",
        spike: "R01-T06",
        platform: std::env::consts::OS,
    })
}

/// 敏感命令替身：在宿主 scratch 目录写一个标记文件（真实副作用，供验收断言）。
/// 只有 main-capability 明确授权；untrusted/remote 调用必须被 ACL 拒绝且无副作用。
#[tauri::command]
async fn shell_sensitive_probe() -> Result<String, String> {
    let dir = std::env::var("T06_SCRATCH").map_err(|_| "T06_SCRATCH not set".to_string())?;
    let marker = std::path::Path::new(&dir).join("sensitive-probe-executed.marker");
    let payload = format!(
        "sensitive probe executed at {:?}\n",
        std::time::SystemTime::now()
    );
    std::fs::write(&marker, &payload).map_err(|e| format!("marker write failed: {e}"))?;
    Ok(format!("SENSITIVE_PROBE_OK marker={}", marker.display()))
}

/// 只经 main-secondary-capability 授权：验证多 capability 叠加的并集语义（W02）。
#[tauri::command]
async fn shell_secondary_probe() -> Result<String, String> {
    Ok("SECONDARY_PROBE_OK (granted only via main-secondary-capability)".to_string())
}

#[tauri::command]
async fn sidecar_status(
    state: tauri::State<'_, Arc<SharedSidecar>>,
) -> Result<serde_json::Value, String> {
    Ok(sidecar::status(&state))
}

#[tauri::command]
async fn sidecar_ping(
    state: tauri::State<'_, Arc<SharedSidecar>>,
) -> Result<serde_json::Value, String> {
    sidecar::ping(state.inner()).await
}

#[tauri::command]
async fn sidecar_restart(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SharedSidecar>>,
) -> Result<serde_json::Value, String> {
    sidecar::restart(&app, state.inner()).await
}

/// 仅 main 授权：E2E 收尾——sidecar 有序 shutdown、确认退出、app.exit(0)。
#[tauri::command]
fn finish_e2e(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SharedSidecar>>,
) -> Result<(), String> {
    let reporter = Reporter::global();
    reporter.event("e2e.finish_requested", serde_json::json!({}));
    let outcome = sidecar::shutdown_blocking(state.inner());
    reporter.event("e2e.sidecar_shutdown", outcome);
    app.exit(0);
    Ok(())
}

pub fn run() {
    Reporter::init_from_env();

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|_app, _shortcut, event| {
                    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        Reporter::global().event(
                            "shortcut.fired",
                            serde_json::json!({"source": "global-shortcut handler"}),
                        );
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(Arc::new(SharedSidecar::new()))
        .invoke_handler(tauri::generate_handler![
            shell_public_info,
            shell_sensitive_probe,
            shell_secondary_probe,
            sidecar_status,
            sidecar_ping,
            sidecar_restart,
            finish_e2e,
        ])
        .setup(|app| probes::on_setup(app));

    // R01-A12：嵌入式 WebDriver 测试入口只进 e2e-test feature 构建。
    #[cfg(feature = "e2e-test")]
    let builder = builder.plugin(tauri_plugin_wdio_webdriver::init());

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
