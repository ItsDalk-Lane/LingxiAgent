//! 当前服务父测试同时启动本轮服务与真实Tauri窗口；外部观察只读本次合成废纸篓载荷。
use lingxi_test_guard as _;
use std::{
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
#[test]
fn current_native_resource_three_leaves_real_windows_and_os_trash() {
    assert_eq!(
        std::env::consts::OS,
        "macos",
        "本腿必须实际运行macOS窗口，其他平台不能冒充通过"
    );
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let output = std::env::temp_dir().join(format!(
        "r06-resource-parent-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&output).unwrap();
    let frontend = Command::new(repo.join("node_modules/.bin/vite"))
        .args([
            "build",
            "--config",
            "desktop/vite.tauri-resources.config.ts",
        ])
        .current_dir(&repo)
        .output()
        .expect("构建本轮实际原生页面");
    std::fs::write(output.join("frontend.stdout.log"), &frontend.stdout).unwrap();
    std::fs::write(output.join("frontend.stderr.log"), &frontend.stderr).unwrap();
    assert!(
        frontend.status.success(),
        "{}",
        String::from_utf8_lossy(&frontend.stderr)
    );
    let cargo = std::env::var_os("CARGO").expect("需要当前Cargo工具链");
    let target = std::env::var_os("LINGXI_TAURI_PROOF_TARGET")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            let root = std::env::var_os("CARGO_TARGET_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| output.join("service-target"));
            root.parent()
                .expect("构建目录须有父目录")
                .join("tauri-resources")
        });
    assert!(target.is_absolute());
    assert!(!target.starts_with(&repo), "禁止在仓库中堆积构建副本");
    let child = Command::new(cargo)
        .args([
            "test",
            "--manifest-path",
            "desktop/src-tauri/Cargo.toml",
            "--locked",
            "--features",
            "e2e-test",
            "--test",
            "resource_current_host",
            "current_tauri_resources_real_preview_watch_and_system_trash",
            "--",
            "--exact",
            "--show-output",
        ])
        .current_dir(&repo)
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_INCREMENTAL", "0")
        .env(
            "LINGXI_NATIVE_SERVICE_BINARY",
            env!("CARGO_BIN_EXE_lingxi-service"),
        )
        .output()
        .expect("真实执行当前Tauri窗口测试");
    std::fs::write(output.join("native.stdout.log"), &child.stdout).unwrap();
    std::fs::write(output.join("native.stderr.log"), &child.stderr).unwrap();
    assert!(
        child.status.success(),
        "当前原生检查失败，证据{}\n{}\n{}",
        output.display(),
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    let stdout = String::from_utf8(child.stdout).unwrap();
    for marker in [
        "legacy_absolute_registration_and_current_resource_changed",
        "actual_text_markdown_office_binary_large_error_readonly_isolation",
        "real_system_trash_payload_and_desktop_list",
    ] {
        assert!(
            stdout.contains(&format!("NATIVE_RESOURCE_CASE {marker} PASS")),
            "遗漏真实原叶场景：{marker}"
        );
    }
    let line = stdout
        .lines()
        .find_map(|line| line.strip_prefix("TAURI_RESOURCE_RECEIPT "))
        .expect("必须有本次完整原生运行收据");
    let receipt: serde_json::Value = serde_json::from_str(line).unwrap();
    assert_eq!(receipt["status"], "PASS_MAC_TAURI_ACTUAL");
    for field in [
        "legacyAbsoluteRegistration",
        "currentViewerUsesResourceChanged",
        "rendererLegacyToggleAbsent",
        "textMarkdownOfficeBinaryLargeError",
        "readonlyIndependentRefresh",
        "unsavedMainPreserved",
        "trashMissingAndRelativeFalse",
    ] {
        assert_eq!(receipt[field], true, "{field}");
    }
    assert_eq!(
        receipt["trashRecoveredBytes"],
        "当前原生OS废纸篓唯一合成内容"
    );
    assert!(!Path::new(receipt["trashSource"].as_str().unwrap()).exists());
    assert!(
        receipt["actualSystemTrashPath"]
            .as_str()
            .unwrap()
            .contains("/.Trash/")
            || receipt["actualSystemTrashPath"]
                .as_str()
                .unwrap()
                .contains("/.Trashes/")
    );
    std::fs::write(
        output.join("result.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    println!("CURRENT_NATIVE_RESOURCE_PARENT_PASS {}", output.display());
}
