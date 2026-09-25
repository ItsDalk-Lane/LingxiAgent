//! 宿主侧系统能力探针 + E2E 窗口编排。全部真实调用；每个探针把真实 Ok/Err 写进
//! 宿主 ndjson 报告，不做静默降级。窗口全部 visible(false)，避免打扰用户桌面。

use crate::report::Reporter;
use crate::sidecar::{self, SharedSidecar};
use serde_json::json;
use std::sync::Arc;
use tauri::Manager;

pub fn on_setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let reporter = Reporter::global();
    let mode = std::env::var("T06_MODE").unwrap_or_else(|_| "e2e".to_string());
    reporter.event(
        "host.started",
        json!({
            "mode": mode,
            "pid": std::process::id(),
            "version": env!("CARGO_PKG_VERSION"),
            "cfg_e2e_test": cfg!(feature = "e2e-test"),
            "debug_assertions": cfg!(debug_assertions),
        }),
    );

    if mode == "relaunch-probe" {
        relaunch_probe(app);
        return Ok(());
    }

    // 1) sidecar 随宿主启动 + 就绪握手（异步；结果进宿主报告）
    {
        let handle = app.handle().clone();
        let shared: Arc<SharedSidecar> = app.state::<Arc<SharedSidecar>>().inner().clone();
        tauri::async_runtime::spawn(async move {
            match sidecar::spawn_and_handshake(&handle, shared).await {
                Ok(hello) => Reporter::global().event("sidecar.handshake_ok", json!({"hello": hello})),
                Err(e) => Reporter::global().event("sidecar.handshake_failed", json!({"error": e})),
            }
        });
    }

    if mode == "e2e" {
        create_aux_windows(app, reporter);
        probe_window_ops(app, reporter);
        probe_tray(app, reporter);
        probe_global_shortcut(app, reporter);
        probe_clipboard(app, reporter);
        probe_notification(app, reporter);
        probe_dialog(app, reporter);
        probe_autostart(app, reporter);
        probe_updater(app, reporter);
        probe_tcc(app, reporter);
    }

    Ok(())
}

/// 创建未授权窗口：untrusted（打包页，不在任何 capability 的 windows 列表）与
/// remote（loopback HTTP 源 = "远端 URL"，任何 capability 未授权 remote.urls）。
fn create_aux_windows(app: &mut tauri::App, reporter: &Reporter) {
    let remote_url = std::env::var("T06_REMOTE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:19274/remote.html".to_string());
    let untrusted = tauri::WebviewWindowBuilder::new(
        app,
        "untrusted",
        tauri::WebviewUrl::App("untrusted.html".into()),
    )
    .title("lingxi-t06 untrusted (bundled, no capability)")
    .visible(false)
    .build();
    reporter.event(
        "window.untrusted_created",
        match &untrusted {
            Ok(_) => json!({"ok": true}),
            Err(e) => json!({"ok": false, "error": e.to_string()}),
        },
    );
    let remote = tauri::WebviewWindowBuilder::new(
        app,
        "remote",
        tauri::WebviewUrl::External(remote_url.parse().expect("remote url")),
    )
    .title("lingxi-t06 remote (loopback http origin)")
    .visible(false)
    .build();
    reporter.event(
        "window.remote_created",
        match &remote {
            Ok(_) => json!({"ok": true, "url": remote_url}),
            Err(e) => json!({"ok": false, "error": e.to_string()}),
        },
    );
}

fn probe_window_ops(app: &mut tauri::App, reporter: &Reporter) {
    let Some(w) = app.get_webview_window("main") else {
        reporter.event("window.ops", json!({"ok": false, "error": "main window missing"}));
        return;
    };
    let r = |res: Result<(), tauri::Error>| format!("{res:?}");
    let mut out = serde_json::Map::new();
    out.insert("is_maximized_before".into(), json!(format!("{:?}", w.is_maximized())));
    out.insert("maximize".into(), json!(r(w.maximize())));
    out.insert("is_maximized_after".into(), json!(format!("{:?}", w.is_maximized())));
    out.insert("unmaximize".into(), json!(r(w.unmaximize())));
    out.insert("set_always_on_top".into(), json!(r(
        w.set_always_on_top(true)
            .and_then(|_| w.set_always_on_top(false))
    )));
    out.insert("inner_size".into(), json!(format!("{:?}", w.inner_size().map(|s| (s.width, s.height)))));
    out.insert("scale_factor".into(), json!(format!("{:?}", w.scale_factor())));
    let monitor = w.current_monitor().map(|m| {
        m.map(|mm| {
            json!({
                "name": mm.name().cloned(),
                "size": [mm.size().width, mm.size().height],
                "scale_factor": mm.scale_factor(),
            })
        })
    });
    out.insert("current_monitor".into(), json!(format!("{:?}", monitor)));
    reporter.event("window.ops", json!({"ok": true, "results": out}));
}

fn probe_tray(app: &mut tauri::App, reporter: &Reporter) {
    let result = (|| -> Result<String, String> {
        let item = tauri::menu::MenuItemBuilder::with_id("t06.quit", "T06 spike quit")
            .build(app)
            .map_err(|e| e.to_string())?;
        let menu = tauri::menu::MenuBuilder::new(app)
            .item(&item)
            .build()
            .map_err(|e| e.to_string())?;
        let icon = app
            .default_window_icon()
            .cloned()
            .ok_or("no default window icon")?;
        let tray = tauri::tray::TrayIconBuilder::with_id("t06-tray")
            .icon(icon)
            .menu(&menu)
            .tooltip("lingxi-t06-spike")
            .on_menu_event(|_app, event| {
                if event.id().as_ref() == "t06.quit" {
                    Reporter::global().event("tray.menu_event", json!({"id": "t06.quit"}));
                }
            })
            .build(app)
            .map_err(|e| e.to_string())?;
        std::mem::forget(tray); // 活到进程退出
        Ok("tray icon + menu created".to_string())
    })();
    reporter.event(
        "tray.created",
        match result {
            Ok(m) => json!({"ok": true, "detail": m}),
            Err(e) => json!({"ok": false, "error": e}),
        },
    );
}

fn probe_global_shortcut(app: &mut tauri::App, reporter: &Reporter) {
    use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};
    let result = (|| -> Result<serde_json::Value, String> {
        let shortcut = Shortcut::new(
            Some(Modifiers::SUPER | Modifiers::ALT | Modifiers::SHIFT),
            Code::Digit9,
        );
        app.global_shortcut()
            .register(shortcut.clone())
            .map_err(|e| e.to_string())?;
        let registered = app.global_shortcut().is_registered(shortcut);
        Ok(json!({"registered": registered, "shortcut": "CmdOrControl+Alt+Shift+9"}))
    })();
    reporter.event(
        "shortcut.register",
        match result {
            Ok(v) => json!({"ok": true, "detail": v}),
            Err(e) => json!({"ok": false, "error": e}),
        },
    );
}

fn probe_clipboard(app: &mut tauri::App, reporter: &Reporter) {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    let result = (|| -> Result<serde_json::Value, String> {
        let cb = app.clipboard();
        let previous = cb.read_text().ok();
        let marker = "t06-clipboard-probe-中文剪贴板";
        cb.write_text(marker.to_string()).map_err(|e| e.to_string())?;
        let read_back = cb.read_text().map_err(|e| e.to_string())?;
        let roundtrip_ok = read_back == marker;
        if let Some(prev) = previous.clone() {
            let _ = cb.write_text(prev); // 恢复用户剪贴板
        }
        Ok(json!({
            "roundtrip_ok": roundtrip_ok,
            "had_previous": previous.is_some(),
            "restored": previous.is_some(),
        }))
    })();
    reporter.event(
        "clipboard.roundtrip",
        match result {
            Ok(v) => json!({"ok": true, "detail": v}),
            Err(e) => json!({"ok": false, "error": e}),
        },
    );
}

fn probe_notification(app: &mut tauri::App, reporter: &Reporter) {
    use tauri_plugin_notification::NotificationExt;
    let result = (|| -> Result<serde_json::Value, String> {
        let n = app.notification();
        let perm = n.request_permission().map_err(|e| e.to_string())?;
        // 最小真实调用：一次通知。
        n.builder()
            .title("Lingxi R01-T06 spike")
            .body("notification probe (single minimal real call)")
            .show()
            .map_err(|e| e.to_string())?;
        Ok(json!({"permission": format!("{perm:?}"), "shown": true}))
    })();
    reporter.event(
        "notification.probe",
        match result {
            Ok(v) => json!({"ok": true, "detail": v}),
            Err(e) => json!({"ok": false, "error": e}),
        },
    );
}

/// 文件选择对话框：真实原生弹窗（callback API，生产等价路径）；runner 侧用合成
/// Cmd+period（postToPid）取消。回调是否兑现如实记录（取消 = None）。
/// 实测：blocking_pick_file 与「Escape 关闭面板」两条路径下回调都不兑现（Escape 走
/// performClose: 而非 cancel:）；只有 cancel: 等价物（Cmd+.）能确定性兑现。
/// 该限制写入 T06 报告 §7-R1。
fn probe_dialog(app: &mut tauri::App, _reporter: &Reporter) {
    use tauri_plugin_dialog::DialogExt;
    let handle = app.handle().clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(2500));
        Reporter::global().event("dialog.open_presenting", json!({"api": "pick_file(callback)"}));
        handle.dialog().file().pick_file(move |picked| {
            Reporter::global().event(
                "dialog.open",
                json!({
                    "ok": true,
                    "picked": picked.map(|p| p.to_string()),
                    "note": "null = 用户/runner 取消（预期：合成 Cmd+period 取消）",
                }),
            );
        });
    });
}

fn probe_autostart(app: &mut tauri::App, reporter: &Reporter) {
    use tauri_plugin_autostart::ManagerExt;
    let result = (|| -> Result<serde_json::Value, String> {
        let al = app.autolaunch();
        al.enable().map_err(|e| format!("enable: {e}"))?;
        let enabled = al.is_enabled().map_err(|e| format!("is_enabled: {e}"))?;
        al.disable().map_err(|e| format!("disable: {e}"))?;
        let after = al.is_enabled().map_err(|e| format!("is_enabled(after): {e}"))?;
        Ok(json!({"enabled_after_enable": enabled, "enabled_after_disable": after}))
    })();
    reporter.event(
        "autostart.probe",
        match result {
            Ok(v) => json!({"ok": true, "detail": v}),
            Err(e) => json!({"ok": false, "error": e}),
        },
    );
}

fn probe_updater(app: &mut tauri::App, _reporter: &Reporter) {
    let handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        use tauri_plugin_updater::UpdaterExt;
        let result: Result<serde_json::Value, String> = async {
            let updater = handle.updater().map_err(|e| format!("updater(): {e}"))?;
            match updater.check().await {
                Ok(Some(update)) => {
                    let version = update.version.clone();
                    // 只下载（验签）不安装：禁止触发真实系统更新。
                    let bytes = update
                        .download(|_chunk, _total| {}, || {})
                        .await
                        .map_err(|e| format!("download: {e}"))?;
                    Ok(json!({
                        "update_found": true,
                        "version": version,
                        "downloaded_bytes": bytes.len(),
                        "installed": false,
                        "note": "download 验签通过后未调用 install",
                    }))
                }
                Ok(None) => Ok(json!({"update_found": false})),
                Err(e) => Err(format!("check: {e}")),
            }
        }
        .await;
        Reporter::global().event(
            "updater.probe",
            match result {
                Ok(v) => json!({"ok": true, "detail": v}),
                Err(e) => json!({"ok": false, "error": e}),
            },
        );
    });
}

/// TCC 权限状态只读查询（不触发授权弹窗）：外部 swiftc 探针二进制，固定路径经
/// T06_TCC_PROBE 传入；缺失则如实记录 skipped。
fn probe_tcc(_app: &mut tauri::App, reporter: &Reporter) {    match std::env::var("T06_TCC_PROBE") {
        Ok(path) => match std::process::Command::new(&path).output() {
            Ok(out) => reporter.event(
                "tcc.status",
                json!({
                    "ok": out.status.success(),
                    "exit": out.status.code(),
                    "stdout": String::from_utf8_lossy(&out.stdout),
                    "stderr": String::from_utf8_lossy(&out.stderr),
                }),
            ),
            Err(e) => reporter.event("tcc.status", json!({"ok": false, "error": e.to_string()})),
        },
        Err(_) => reporter.event("tcc.status", json!({"ok": false, "error": "T06_TCC_PROBE not set"})),
    }
}

/// 应用重启探针（对应现役 app:restart）：首发写标记并 relaunch；第二实例见到标记
/// 上报后退出。全程证据进宿主报告。
fn relaunch_probe(app: &mut tauri::App) {
    let reporter = Reporter::global();
    let scratch = std::env::var("T06_SCRATCH").unwrap_or_else(|_| "/tmp/lingxi-t06".into());
    let marker = std::path::Path::new(&scratch).join("relaunch-second.marker");
    if marker.exists() {
        reporter.event("relaunch.second_instance_observed", json!({"ok": true}));
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(800));
            std::process::exit(0);
        });
        return;
    }
    if let Err(e) = std::fs::create_dir_all(&scratch)
        .and_then(|_| std::fs::write(&marker, b"relaunched\n"))
    {
        reporter.event("relaunch.marker_failed", json!({"error": e.to_string()}));
        return;
    }
    reporter.event("relaunch.first_instance", json!({"ok": true}));
    let env = app.env();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(800));
        tauri::process::restart(&env);
    });
}
