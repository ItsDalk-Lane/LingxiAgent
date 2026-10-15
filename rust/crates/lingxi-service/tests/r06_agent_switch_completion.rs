//! 助手切换使用真CLI/HTTP、真实模型网关、工具、数据库及磁盘验证。
#[test]
fn current_agent_switch_creates_owned_focus_session_and_next_run_uses_its_workspace() {
    let root = std::env::temp_dir().join(format!(
        "r06-agent-switch-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let repository = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output = std::process::Command::new("python3")
        .arg(repository.join("tests/rust-native/agent-switch-live.py"))
        .env("EX12_EVIDENCE", &root)
        .env("EX12_SERVICE_BIN", env!("CARGO_BIN_EXE_lingxi-service"))
        .output()
        .expect("启动真实切换证明");
    println!("AGENT_SWITCH_PROOF {}", root.display());
    println!("{}", String::from_utf8_lossy(&output.stdout));
    println!("{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success(), "真实切换检查失败");
    let result: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("result.json")).unwrap()).unwrap();
    assert_eq!(result["status"], "PASS");
    assert_eq!(result["modelRequests"], 3);
    assert_eq!(result["checks"].as_array().unwrap().len(), 14);
}

#[test]
fn no_model_switch_refuses_without_session_focus_or_config_side_effects() {
    let root = std::env::temp_dir().join(format!(
        "r06-agent-switch-no-model-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let repository = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output = std::process::Command::new("python3")
        .arg(repository.join("tests/rust-native/agent-switch-live.py"))
        .env("EX12_EVIDENCE", &root)
        .env("EX12_EXPECT_NO_MODEL", "1")
        .env("EX12_SERVICE_BIN", env!("CARGO_BIN_EXE_lingxi-service"))
        .output()
        .expect("启动无模型真实拒绝检查");
    println!("AGENT_SWITCH_NO_MODEL_PROOF {}", root.display());
    println!("{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success());
    let result: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("result.json")).unwrap()).unwrap();
    assert_eq!(result["status"], "PASS");
    assert_eq!(result["modelRequests"], 0);
}

#[test]
fn deleting_current_agent_commits_replacement_focus_before_original_deletion_and_recovers_cleanup()
{
    let root = std::env::temp_dir().join(format!(
        "r06-delete-current-focus-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let repository = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output = std::process::Command::new("python3")
        .arg(repository.join("tests/rust-native/agent-switch-live.py"))
        .env("EX12_EVIDENCE", &root)
        .env("EX12_DELETE_CURRENT", "1")
        .env("EX12_SERVICE_BIN", env!("CARGO_BIN_EXE_lingxi-service"))
        .output()
        .expect("启动当前助手删除的真实焦点检查");
    println!("DELETE_CURRENT_FOCUS_PROOF {}", root.display());
    println!("{}", String::from_utf8_lossy(&output.stdout));
    println!("{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success());
    let result: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("result.json")).unwrap()).unwrap();
    assert_eq!(result["status"], "PASS");
    assert_eq!(result["modelRequests"], 3);
    assert_eq!(result["checks"].as_array().unwrap().len(), 11);
}

#[test]
fn target_agent_model_runs_when_global_chat_route_is_absent() {
    let root = std::env::temp_dir().join(format!(
        "r06-target-only-model-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let repository = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output = std::process::Command::new("python3")
        .arg(repository.join("tests/rust-native/agent-switch-live.py"))
        .env("EX12_EVIDENCE", &root)
        .env("EX12_TARGET_ONLY", "1")
        .env("EX12_SERVICE_BIN", env!("CARGO_BIN_EXE_lingxi-service"))
        .output()
        .expect("启动只有目标助手模型的真实检查");
    println!("TARGET_ONLY_MODEL_PROOF {}", root.display());
    println!("{}", String::from_utf8_lossy(&output.stdout));
    println!("{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success());
    let result: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("result.json")).unwrap()).unwrap();
    assert_eq!(result["status"], "PASS");
    assert_eq!(result["modelRequests"], 3);
}
