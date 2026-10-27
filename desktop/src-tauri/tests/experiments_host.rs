#![cfg(feature = "e2e-test")]
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command},
    time::{Duration, Instant},
};
struct NativeApp(Child);
impl Drop for NativeApp {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[tokio::test]
async fn original_experiments_current_tauri_window() {
    let root = PathBuf::from(std::env::var_os("R06_EXPERIMENT_PROOF").unwrap());
    fs::create_dir_all(&root).unwrap();
    let file = fs::File::create(root.join("native-app.log")).unwrap();
    let _app = NativeApp(
        Command::new(env!("CARGO_BIN_EXE_lingxi-resource-desktop"))
            .stdout(file.try_clone().unwrap())
            .stderr(file)
            .spawn()
            .unwrap(),
    );
    let base = format!(
        "http://127.0.0.1:{}",
        std::env::var("TAURI_WEBDRIVER_PORT").unwrap()
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    let start = Instant::now();
    while client.get(format!("{base}/status")).send().await.is_err() {
        assert!(start.elapsed() < Duration::from_secs(30));
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let session: Value = client
        .post(format!("{base}/session"))
        .json(&json!({"capabilities":{"alwaysMatch":{}}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let sid = session["value"]["sessionId"].as_str().unwrap();
    let handles: Value = client
        .get(format!("{base}/session/{sid}/window/handles"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let handle = handles["value"][0].as_str().unwrap();
    let selected: Value = client
        .post(format!("{base}/session/{sid}/window"))
        .json(&json!({"handle":handle}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(selected["value"]["error"].is_null(), "{selected}");
    fs::write(
        root.join("native-driver.json"),
        json!({"base":base,"session":sid}).to_string(),
    )
    .unwrap();
    let start = Instant::now();
    while !root.join("native-finished.json").is_file() {
        assert!(
            start.elapsed() < Duration::from_secs(100),
            "未收到实际界面完成收据"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let result: Value =
        serde_json::from_slice(&fs::read(root.join("native-finished.json")).unwrap()).unwrap();
    assert_eq!(result["ok"], true, "{result}");
    assert!(root.join("experiments.png").is_file());
    assert!(root.join("permission-state.png").is_file());
}
