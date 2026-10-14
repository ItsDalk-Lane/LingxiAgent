//! 当前候选真实Rust保存后停服，再由Electron实际IPC与启动组件显示本地头像。
use serde_json::Value;
use std::{path::Path, process::Stdio, time::Duration};

async fn checked_process(mut command: tokio::process::Command, label: &str, seconds: u64) {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    command.kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let child = command
        .spawn()
        .unwrap_or_else(|error| panic!("{label}无法启动：{error}"));
    let pid = child.id().unwrap();
    let output =
        match tokio::time::timeout(Duration::from_secs(seconds), child.wait_with_output()).await {
            Ok(result) => result.unwrap(),
            Err(_) => {
                // 超时清理本检查独占的宿主及服务子进程，不触及用户窗口或服务。
                #[cfg(unix)]
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
                panic!("{label}超时，不能算通过");
            }
        };
    assert!(
        output.status.success(),
        "{label}失败：{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn current_rust_saved_offline_splash_real_ipc_and_image_fallback() {
    assert_eq!(
        std::env::consts::OS,
        "macos",
        "当前原生证据需macOS Electron；其他平台未验证，不能跳过算成功"
    );
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let raw =
        std::env::temp_dir().join(format!("r06-splash-native-{}-{nonce}", std::process::id()));
    std::fs::create_dir_all(&raw).unwrap();
    // 每次从本候选构建实际组件和桥；不得借用作者树或旧编译输出签PASS。
    for config in ["vite.config.preload.js", "vite.config.splash.ts"] {
        let mut command = tokio::process::Command::new("node");
        command
            .current_dir(root)
            .arg(root.join("node_modules/vite/bin/vite.js"))
            .args(["build", "--config", config])
            .env_remove("ELECTRON_RUN_AS_NODE");
        checked_process(command, config, 120).await;
    }
    let electron = std::fs::canonicalize(
        root.join("node_modules/electron/dist/Electron.app/Contents/MacOS/Electron"),
    )
    .unwrap();
    let mut command = tokio::process::Command::new(electron);
    command
        .current_dir(root)
        .arg(root.join("tests/rust-native/splash-avatar-native.cjs"))
        .arg(root)
        .arg(&raw)
        .arg(env!("CARGO_BIN_EXE_lingxi-service"))
        .env_remove("ELECTRON_RUN_AS_NODE")
        .env_remove("LINGXI_SPLASH_BASELINE_SOURCE");
    checked_process(command, "真实当前候选离线启动头像", 180).await;
    let receipt: Value =
        serde_json::from_slice(&std::fs::read(raw.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["leaf"], "R00-T02-LA-A02D8C572F5C");
    assert_eq!(receipt["result"], "PASS");
    assert_eq!(receipt["backend"], "rust");
    assert_eq!(receipt["serverRequiredForSplash"], false);
    assert!(receipt["actualElectron"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    let records = receipt["records"].as_array().unwrap();
    assert_eq!(records.len(), 16);
    let find = |name: &str| records.iter().find(|row| row["case"] == name).unwrap();
    assert_eq!(
        find("actual-service-stopped-before-splash")["serviceOffline"],
        true
    );
    let live = find("rust-saved-offline-current-avatar");
    assert_eq!(live["visible"], true);
    assert_eq!(live["splashInfo"]["yuan"], "ming");
    assert!(live["localPath"]
        .as_str()
        .unwrap()
        .ends_with("/agents/zeta/avatars/agent.webp"));
    assert_eq!(live["renderer"]["complete"], true);
    assert_eq!(live["renderer"]["width"], 48);
    assert!(live["renderer"]["opacity"].as_f64().unwrap() >= 0.99);
    for case in [
        "missing-default",
        "wrong-format-default",
        "malformed-webp-default",
        "extension-mismatch-default",
        "nonregular-directory-default",
        "nonregular-fifo-default",
        "symlink-file-default",
        "symlink-ancestor-default",
    ] {
        assert!(find(case)["localPath"].is_null(), "{case}不应返回文件路径");
        assert!(find(case)["renderer"]["src"]
            .as_str()
            .unwrap()
            .contains("/assets/Ming.png"));
    }
    assert_eq!(
        find("actual-permission-rejection-default")["ipcRejected"],
        true
    );
    assert_eq!(
        find("actual-file-load-failure-default")["fileRemovedAfterPath"],
        true
    );
    assert!(find("offline-empty-default")["localPath"].is_null());
    assert!(find("offline-empty-default")["renderer"]["src"]
        .as_str()
        .unwrap()
        .contains("/assets/Lingxi.png"));
    for png in [
        "rust-saved-offline-current-avatar.png",
        "missing-default.png",
        "offline-empty-default.png",
    ] {
        assert!(std::fs::metadata(raw.join(png)).unwrap().len() > 100);
    }
}
