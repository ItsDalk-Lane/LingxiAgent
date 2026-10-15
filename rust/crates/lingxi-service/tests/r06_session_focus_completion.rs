//! 当前 CLI、真实模型网关与生产输入框的焦点创建/草稿联合证据。
#[test]
fn focused_creation_consumes_pinned_model_workspace_and_native_input_drafts() {
    let root = std::env::temp_dir().join(format!(
        "lingxi-r06-session-focus-{}-{}",
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
    .arg(repository.join("tests/rust-native/session-focus-live.py"))
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
    .expect("start actual production proof");
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        println!("SESSION-FOCUS: {line}");
    }
    for line in String::from_utf8_lossy(&output.stderr).lines() {
        println!("SESSION-FOCUS-ERR: {line}");
    }
    assert!(
        output.status.success(),
        "actual proof failed: {}",
        root.display()
    );
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("result.json")).unwrap()).unwrap();
    assert_eq!(value["status"], "PASS");
    assert_eq!(value["requests"].as_array().unwrap().len(), 9);
    assert_eq!(value["persistedModelCalls"], 9);
    if cfg!(target_os = "macos") {
        assert_eq!(value["ui"]["checks"].as_array().unwrap().len(), 6);
        assert_eq!(value["ui"]["status"], "PASS");
    }
}
