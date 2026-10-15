//! 当前 CLI、实际供应商网络、真实地图和桌面浏览器宿主的联合证据。
fn run_proof(name: &str, script: &str) -> (std::path::PathBuf, serde_json::Value) {
    let root = std::env::temp_dir().join(format!(
        "lingxi-r06-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let repository = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output = std::process::Command::new(
        std::env::var("R06_PYTHON_BIN").unwrap_or_else(|_| "python3".into()),
    )
    .arg(repository.join("tests/rust-native").join(script))
    .env("R06_SESSION_EVIDENCE", &root)
    .env(
        "R06_SESSION_SERVICE_BIN",
        env!("CARGO_BIN_EXE_lingxi-service"),
    )
    .env(
        "R06_SESSION_UI",
        if cfg!(target_os = "macos") { "1" } else { "0" },
    )
    .output()
    .expect("启动当前生产联合检查");
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        println!("SESSION-{name}: {line}");
    }
    for line in String::from_utf8_lossy(&output.stderr).lines() {
        println!("SESSION-{name}-ERR: {line}");
    }
    assert!(
        output.status.success(),
        "当前生产检查失败：{}",
        root.display()
    );
    let file = if name == "browser" {
        "browser-result.json"
    } else {
        "result.json"
    };
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join(file)).unwrap()).unwrap();
    assert_eq!(value["status"], "PASS");
    (root, value)
}

#[test]
fn current_map_seven_groups_real_storage_models_and_receipts() {
    let (_, value) = run_proof("map", "session-map-live.py");
    let expected = if cfg!(target_os = "macos") { 36 } else { 21 };
    assert_eq!(value["requests"].as_array().unwrap().len(), expected);
    assert_eq!(value["persistedModelCalls"], expected);
    assert_eq!(value["usageProofs"].as_array().unwrap().len(), expected / 3);
    if cfg!(target_os = "macos") {
        let checks = value["ui"]["checks"].as_array().unwrap();
        assert_eq!(checks.len(), 19);
        for group in 0..7 {
            assert!(
                checks.iter().any(|check| check["group"] == group),
                "缺少地图原断言组{group}"
            );
        }
        assert_eq!(value["ui"]["status"], "PASS");
        assert!(value["ui"]["observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["kind"] == "real-watchdog-unknown"
                && item["phase"] == "delivery_unknown"
                && item["draft"] == "MAP-DELIVERY-UNKNOWN"));
        assert_eq!(
            value["ui"]["frames"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|frame| frame["type"] == "prompt")
                .count(),
            4
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn current_browser_host_restores_target_preserves_old_view_and_reports_disconnect() {
    let (_, value) = run_proof("browser", "session-browser-live.py");
    assert_eq!(value["oldViewSurvived"], true);
    let trace = value["trace"].as_array().unwrap();
    assert!(trace
        .iter()
        .any(|item| item["data"]["browserResume"]["status"] == "resumed"
            && item["data"]["sessionId"] == "browser-b"));
    assert!(trace.iter().any(
        |item| item["data"]["browserResume"]["status"] == "attempted"
            && item["data"]["sessionId"] == "browser-c"
    ));
    assert!(trace
        .iter()
        .any(|item| item["kind"] == "foreign-host-refusal" && item["status"] == 403));
    assert!(trace
        .iter()
        .any(|item| item["kind"] == "second-host-refusal" && item["status"] == 409));
    assert!(trace
        .iter()
        .any(|item| item["path"] == "/lingxi/v1/sessions/switch" && item["status"] == 502));
    assert_eq!(value["actualSelectedPath"], "native:browser-b");
}
