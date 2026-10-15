//! 原独立双服务误写反例：保留原窗口断言，使用本次Cargo二进制与当前生产设置页。
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn current_settings_owner_switch_cannot_edit_other_real_service_before_identity_ready() {
    assert_eq!(
        std::env::consts::OS,
        "macos",
        "本原生窗口运行腿仅代表实际macOS环境"
    );
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let root = std::env::temp_dir().join(format!(
        "r06-settings-owner-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    assert!(!root.starts_with(&repo));
    for name in [
        "settings-owner-main.cjs",
        "settings-owner-preload.cjs",
        "settings-owner-orchestrator.mjs",
    ] {
        fs::copy(repo.join("tests/rust-native").join(name), root.join(name)).unwrap();
    }
    for name in ["yuan", "identity-templates", "agents-templates"] {
        let destination = root.join("product").join(name);
        fs::create_dir_all(&destination).unwrap();
        for entry in fs::read_dir(repo.join("lib").join(name)).unwrap() {
            let entry = entry.unwrap();
            assert!(!entry.file_type().unwrap().is_symlink());
            if entry.file_type().unwrap().is_file() {
                fs::copy(entry.path(), destination.join(entry.file_name())).unwrap();
            }
        }
    }
    let binary = Path::new(env!("CARGO_BIN_EXE_lingxi-service"));
    let binary_sha = Sha256::digest(fs::read(binary).unwrap())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let output = Command::new(std::env::var_os("R06_NODE_BIN").unwrap_or_else(|| "node".into()))
        .arg(root.join("settings-owner-orchestrator.mjs"))
        .env("R06_OWNER_OUTPUT", &root)
        .env("R06_OWNER_REPO", &repo)
        .env("R06_OWNER_BINARY", binary)
        .output()
        .unwrap();
    fs::write(root.join("stdout.log"), &output.stdout).unwrap();
    fs::write(root.join("stderr.log"), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "原独立双服务隔离反例未通过：{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value =
        serde_json::from_slice(&fs::read(root.join("result.json")).unwrap()).unwrap();
    assert!(
        result["stale"]["name"].is_null(),
        "新连接尚未验证时不能展示旧可编辑名称"
    );
    assert_eq!(result["noUtility"]["puts"], 0);
    assert_eq!(result["noUtility"]["disabled"], true);
    assert!(result["forwarded"]
        .as_array()
        .unwrap()
        .iter()
        .all(|request| request["method"] != "PUT"));
    assert!(result["aConfig"].as_str().unwrap().contains("A独占原名称"));
    assert!(result["bConfig"].as_str().unwrap().contains("B独占原名称"));
    assert!(!result["bConfig"].as_str().unwrap().contains("A旧页误写到B"));
    assert_eq!(
        Sha256::digest(fs::read(binary).unwrap())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        binary_sha
    );
    fs::write(root.join("parent-receipt.json"), serde_json::to_vec_pretty(&serde_json::json!({"status":"PASS_MAC_ONLY", "currentBinarySha256":binary_sha,"evidence":root,"foreignPutCount":0,"originalIndependentAssertionsPreserved":true})).unwrap()).unwrap();
}
