//! 当前Rust桌面连接下的真实生产编辑IPC，不把方法替身当成原生执行证明。
use lingxi_test_guard as _;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn current_rust_desktop_edit_ipc_allows_only_four_native_sender_commands() {
    assert_eq!(std::env::consts::OS, "macos", "本实际窗口运行腿仅代表macOS");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let root = std::env::temp_dir().join(format!(
        "r06-edit-native-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    assert!(!root.starts_with(&repo));
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
    // 生成本轮实际preload，不能由遗留bundle冒称当前源码。
    let build = Command::new(std::env::var_os("R06_NODE_BIN").unwrap_or_else(|| "node".into()))
        .arg(repo.join("node_modules/vite/bin/vite.js"))
        .args(["build", "--config", "vite.config.preload.js"])
        .current_dir(&repo)
        .output()
        .unwrap();
    fs::write(root.join("preload-build.stdout.log"), &build.stdout).unwrap();
    fs::write(root.join("preload-build.stderr.log"), &build.stderr).unwrap();
    assert!(build.status.success(), "当前preload必须真实构建成功");
    let binary = Path::new(env!("CARGO_BIN_EXE_lingxi-service"));
    let binary_sha = Sha256::digest(fs::read(binary).unwrap())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let output =
        Command::new(repo.join("node_modules/electron/dist/Electron.app/Contents/MacOS/Electron"))
            .arg(repo.join("tests/rust-native/edit-command-native.cjs"))
            .arg(&repo)
            .arg(&root)
            .arg(binary)
            .env("LINGXI_HOME", root.join("runtime-home"))
            .env("LINGXI_PRODUCT_DIR", root.join("product"))
            .output()
            .unwrap();
    fs::write(root.join("stdout.log"), &output.stdout).unwrap();
    fs::write(root.join("stderr.log"), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "真实编辑IPC失败：{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value =
        serde_json::from_slice(&fs::read(root.join("result.json")).unwrap()).unwrap();
    assert_eq!(result["status"], "PASS_MAC_ONLY");
    assert_eq!(result["binarySha256"], binary_sha);
    assert_eq!(
        result["allowed"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["command"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["cut", "copy", "paste", "selectAll"]
    );
    assert_eq!(result["refusedCount"], 10);
    assert_eq!(result["otherWindowEvents"], 0);
    assert_eq!(result["fixtureReadsClipboardContents"], false);
}
