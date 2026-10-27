//! 三原叶共享的当前候选二进制父测试；只有外部模型HTTP端点使用替身。
//! 脚本在候选源码摘要范围内，父逐项核验实际退出、全部收据与当前编译字节。
use lingxi_test_guard as _;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn sha256(path: &Path) -> String {
    Sha256::digest(fs::read(path).expect("读取当前实际文件"))
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).expect("必须有实际运行收据")).expect("运行收据JSON有效")
}
fn copy_tree(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let kind = entry.file_type().unwrap();
        assert!(!kind.is_symlink(), "产品材料不得通过链接逃逸");
        if kind.is_dir() {
            copy_tree(&entry.path(), &target.join(entry.file_name()));
        } else {
            assert!(kind.is_file());
            fs::copy(entry.path(), target.join(entry.file_name())).unwrap();
        }
    }
}
fn run_script(
    repo: &Path,
    root: &Path,
    product: &Path,
    script: &str,
    group: &str,
    binary: &Path,
) -> PathBuf {
    let output = root.join(group);
    let script_path = repo.join("tests/rust-native").join(script);
    assert!(script_path.is_file());
    let child = Command::new("python3")
        .arg(&script_path)
        .arg("--binary")
        .arg(binary)
        .arg("--product-dir")
        .arg(product)
        .arg("--output")
        .arg(&output)
        .env("LINGXI_HOME", root.join("runtime-home"))
        .env("TMPDIR", root.join("runtime-tmp"))
        .env("LINGXI_PRODUCT_DIR", product)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("执行候选源码内真实Run检查");
    fs::write(root.join(format!("{group}.stdout.log")), &child.stdout).unwrap();
    fs::write(root.join(format!("{group}.stderr.log")), &child.stderr).unwrap();
    assert!(
        child.status.success(),
        "真实检查退出失败：{group}\n{}\n{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    assert_eq!(
        json(&output.join("summary.json"))["binarySha256"],
        sha256(binary),
        "不得拿冻结历史程序代替本轮Cargo实际bin"
    );
    output
}
fn assert_cases(path: &Path, count: usize, status_key: &str) -> Vec<Value> {
    let value = json(path);
    let rows = value.as_array().expect("逐项场景收据必须为数组");
    assert_eq!(rows.len(), count, "不得漏跑场景");
    for row in rows {
        assert_eq!(row[status_key], "PASS", "每条原断言均须实际通过：{row}");
    }
    rows.clone()
}

#[test]
fn macos_native_discovery_current_binary_real_run_and_boundaries() {
    assert_eq!(
        std::env::consts::OS,
        "macos",
        "此具名运行腿使用macOS真实EACCES/FIFO/lsof；其它平台不能计作本腿PASS"
    );
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let root = Path::new("/private/tmp").join(format!(
        "lingxi-native-current-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    assert!(
        !root.starts_with(&repo),
        "任务数据/tmp/产品材料必须位于仓库外"
    );
    for name in ["runtime-home", "runtime-tmp", "product"] {
        fs::create_dir(root.join(name)).unwrap();
    }
    let product = root.join("product");
    for name in ["yuan", "identity-templates", "agents-templates"] {
        copy_tree(&repo.join("lib").join(name), &product.join(name));
    }
    for name in ["identity.example.md", "agents.example.md"] {
        fs::copy(repo.join("lib").join(name), product.join(name)).unwrap();
    }
    let binary = Path::new(env!("CARGO_BIN_EXE_lingxi-service"));
    assert!(binary.is_file());
    let original = run_script(
        &repo,
        &root,
        &product,
        "native-discovery-cases.py",
        "original40",
        binary,
    );
    let original_rows = assert_cases(&original.join("cases.json"), 40, "status");
    let original_summary = json(&original.join("summary.json"));
    assert_eq!(original_summary["status"], "PASS");
    assert_eq!(original_summary["cases"], 40);
    assert_eq!(original_summary["platform"], "Darwin");
    assert_eq!(
        original_summary["providerBoundary"],
        "external HTTP model endpoint only"
    );
    for required in [
        "read only original bytes metadata and no fake file records",
        "reboot next Run consumes persisted genuine tool history",
        "OS sees real in-flight scan directory handle before cancellation",
        "true in-flight disconnect cancellation and concurrent busy no false success",
        "physical worker releases session permit then next genuine Run succeeds",
    ] {
        assert!(
            original_rows.iter().any(|row| row["case"] == required),
            "共享原条款必须真实受检：{required}"
        );
    }
    assert!(!json(&original.join("raw-tool-events.json"))
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!json(&original.join("model-history.json"))
        .as_array()
        .unwrap()
        .is_empty());
    assert!(fs::read_to_string(original.join("cancel-physical-fds.txt"))
        .unwrap()
        .contains("/workspace/slow"));
    let regression = run_script(
        &repo,
        &root,
        &product,
        "native-discovery-regression-probes.py",
        "independent6",
        binary,
    );
    assert_cases(&regression.join("cases.json"), 6, "result");
    let summary = json(&regression.join("summary.json"));
    assert_eq!(summary["pass"], 6);
    assert_eq!(summary["fail"], 0);
    assert_eq!(summary["zeroFileMutation"], true);
    assert_eq!(summary["zeroFileRegistration"], true);
    let depth = run_script(
        &repo,
        &root,
        &product,
        "native-discovery-depth-probes.py",
        "depth400",
        binary,
    );
    assert_cases(&depth.join("cases.json"), 4, "status");
    let summary = json(&depth.join("summary.json"));
    assert_eq!(summary["status"], "PASS");
    assert_eq!(summary["cases"], 4);
    assert_eq!(summary["zeroFileMutation"], true);
    assert_eq!(summary["zeroFileRegistration"], true);
    let sources = ["native-discovery-cases.py", "native-discovery-regression-probes.py", "native-discovery-depth-probes.py"].map(|name| {
        serde_json::json!({"path":name,"sha256":sha256(&repo.join("tests/rust-native").join(name))})
    });
    let receipt = serde_json::json!({"status":"PASS_MAC_ONLY","actualCurrentBinary":binary,"binarySha256":sha256(binary),"original40":40,"independent6":6,"depth400":4,"sourceScripts":sources,"evidence":root,"windowsRuntime":"NOT_RUN","linuxRuntime":"NOT_RUN"});
    fs::write(
        root.join("parent-receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    println!(
        "NATIVE_DISCOVERY_CURRENT_SOURCE_RECEIPT {}",
        root.join("parent-receipt.json").display()
    );
}
