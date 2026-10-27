//! 快捷窗真实双Electron窗口 + 当前Rust CLI；权限与会话均为合成资料。
use lingxi_test_guard as _;
#[test]
fn original_quick_chat_visible_resize_focus_existing_missing_destroyed_and_late_ack() {
    let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let proof = std::env::temp_dir().join(format!(
        "lingxi-quickchat-completion-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    ));
    let output = std::process::Command::new("python3")
        .arg(repo.join("tests/rust-native/quickchat-live.py"))
        .current_dir(&repo)
        .env("R06_QUICKCHAT_EVIDENCE", &proof)
        .env(
            "R06_QUICKCHAT_SERVICE_BIN",
            env!("CARGO_BIN_EXE_lingxi-service"),
        )
        .output()
        .unwrap();
    std::fs::create_dir_all(&proof).unwrap();
    std::fs::write(proof.join("parent-stdout.log"), &output.stdout).unwrap();
    std::fs::write(proof.join("parent-stderr.log"), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "真实快捷窗失败 {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(proof.join("result.json")).unwrap()).unwrap();
    assert_eq!(value["status"], "PASS");
    assert_eq!(value["checks"].as_array().unwrap().len(), 9);
    assert!(value["modelCalls"].as_u64().unwrap() > 0);
    println!("QUICK_CHAT_REAL_PROOF {}", proof.display());
}
