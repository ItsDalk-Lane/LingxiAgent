use std::path::PathBuf;

#[test]
fn original_system_directory_dialog_real_path_cancel_and_no_install() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let target = std::env::var_os("LINGXI_SKILLS_NATIVE_TARGET")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").expect("需要当前用户的构建缓存位置"))
                .join(".cache/lingxi-r06-builds/tauri-resources")
        });
    assert!(
        target.is_dir(),
        "需要已保留的原生构建缓存；此证明不创建新target"
    );
    let proof = std::env::var_os("LINGXI_DIRECTORY_PROOF")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "lingxi-directory-proof-{}",
                lingxi_adapters::storage::agent_memory::random_token_suffix()
            ))
        });
    let frontend = std::process::Command::new("node")
        .arg(repo.join("node_modules/vite/bin/vite.js"))
        .args([
            "build",
            "--config",
            "desktop/vite.tauri-resources.config.ts",
        ])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(
        frontend.status.success(),
        "{}",
        String::from_utf8_lossy(&frontend.stderr)
    );
    std::fs::create_dir_all(&proof).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    println!("DIRECTORY_PICKER_PROOF {}", proof.display());
    let output =
        std::process::Command::new(std::env::var_os("CARGO").expect("需要当前Cargo工具链"))
            .args([
                "test",
                "--locked",
                "--manifest-path",
                "desktop/src-tauri/Cargo.toml",
                "--features",
                "e2e-test",
                "--test",
                "directory_picker_host",
                "--",
                "--nocapture",
            ])
            .current_dir(&repo)
            .env("CARGO_TARGET_DIR", target)
            .env("CARGO_INCREMENTAL", "0")
            .env("TAURI_WEBDRIVER_PORT", port.to_string())
            .env("LINGXI_DIRECTORY_PROOF", &proof)
            .output()
            .unwrap();
    std::fs::create_dir_all(&proof).unwrap();
    std::fs::write(proof.join("runner-stdout.log"), &output.stdout).unwrap();
    std::fs::write(proof.join("runner-stderr.log"), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "当前原生目录选择器失败：{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(proof.join("receipt.json").is_file());
    println!("NATIVE_DIRECTORY_CURRENT_PROOF {}", proof.display());
}
