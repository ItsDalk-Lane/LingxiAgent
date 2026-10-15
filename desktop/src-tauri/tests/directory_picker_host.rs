#![cfg(feature = "e2e-test")]
use base64::{engine::general_purpose::STANDARD, Engine};
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
struct Driver {
    client: reqwest::Client,
    base: String,
    session: String,
}
impl Driver {
    async fn request(&self, method: reqwest::Method, path: &str, body: Value) -> Value {
        let response = self
            .client
            .request(method, format!("{}{}", self.base, path))
            .json(&body)
            .send()
            .await
            .unwrap();
        let value: Value = response.json().await.unwrap();
        assert!(
            value["value"]["error"].is_null(),
            "WebDriver 原始失败：{value}"
        );
        value["value"].clone()
    }
    async fn script(&self, script: &str, args: Value, asynchronous: bool) -> Value {
        self.request(
            reqwest::Method::POST,
            &format!(
                "/session/{}/execute/{}",
                self.session,
                if asynchronous { "async" } else { "sync" }
            ),
            json!({"script":script,"args":args}),
        )
        .await
    }
    async fn invoke(&self, command: &str, args: Value) -> Value {
        // 拒绝结果用 invokeError 而非 error：request() 把响应里的 error 字段
        // 视为 WebDriver 协议失败，键名撞车会把预期的应用层拒绝误判成框架故障。
        self.script("const done=arguments[arguments.length-1];window.__TAURI__.core.invoke(arguments[0],arguments[1]).then(value=>done({ok:true,value}),error=>done({ok:false,invokeError:String(error)}));", json!([command,args]), true).await
    }
    async fn handles(&self) -> Vec<String> {
        self.request(
            reqwest::Method::GET,
            &format!("/session/{}/window/handles", self.session),
            Value::Null,
        )
        .await
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().into())
        .collect()
    }
    async fn window(&self, handle: &str) {
        self.request(
            reqwest::Method::POST,
            &format!("/session/{}/window", self.session),
            json!({"handle":handle}),
        )
        .await;
    }
    async fn wait_script(&self, script: &str, args: Value, limit: Duration) -> Value {
        let started = Instant::now();
        loop {
            let result = self.script(script, args.clone(), false).await;
            if result != Value::Null && result != Value::Bool(false) {
                return result;
            }
            assert!(started.elapsed() < limit, "界面未到达要求状态：{script}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

#[tokio::test]
async fn native_directory_picker_returns_real_path_cancel_null_and_never_installs() {
    let root = PathBuf::from(std::env::var_os("LINGXI_DIRECTORY_PROOF").expect("需要合成证据目录"));
    fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let folder = root.join("selected-skill");
    fs::create_dir_all(&folder).unwrap();
    let document = folder.join("SKILL.md");
    fs::write(&document, "# 只选择目录，不安装\n").unwrap();
    let before = fs::read(&document).unwrap();
    let port = std::env::var("TAURI_WEBDRIVER_PORT").expect("需要隔离驱动端口");
    let native_log = fs::File::create(root.join("native-app.log")).unwrap();
    // 系统自动操作按应用包定位窗口；直接启动无包二进制虽然能显示对话框，
    // 但不会出现在可选择的应用列表。硬链接复用本轮真实产物，不复制构建缓存。
    let executable = PathBuf::from(env!("CARGO_BIN_EXE_lingxi-resource-desktop"));
    #[cfg(target_os = "macos")]
    let executable = {
        let contents = root.join("Lingxi Directory Proof.app/Contents");
        fs::create_dir_all(contents.join("MacOS")).unwrap();
        let bundled = contents.join("MacOS/lingxi-resource-desktop");
        fs::hard_link(&executable, &bundled).unwrap();
        fs::write(contents.join("Info.plist"), r#"<?xml version="1.0" encoding="UTF-8"?><!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd"><plist version="1.0"><dict><key>CFBundleExecutable</key><string>lingxi-resource-desktop</string><key>CFBundleIdentifier</key><string>dev.lingxi.directory-proof</string><key>CFBundleName</key><string>Lingxi Directory Proof</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>1</string><key>NSHighResolutionCapable</key><true/></dict></plist>"#).unwrap();
        bundled
    };
    let app = Command::new(executable)
        .stdout(native_log.try_clone().unwrap())
        .stderr(native_log)
        .spawn()
        .unwrap();
    let _app = NativeApp(app);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(40))
        .build()
        .unwrap();
    let base = format!("http://127.0.0.1:{port}");
    let started = Instant::now();
    while client.get(format!("{base}/status")).send().await.is_err() {
        assert!(started.elapsed() < Duration::from_secs(30));
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
    let driver = Driver {
        client,
        base,
        session: session["value"]["sessionId"].as_str().unwrap().into(),
    };
    let main = driver.handles().await.into_iter().next().unwrap();
    driver.window(&main).await;
    driver
        .wait_script(
            "return !!document.querySelector('[data-testid=resource-desktop]');",
            json!([]),
            Duration::from_secs(30),
        )
        .await;

    driver.script("window.__directoryResults=[];window.addEventListener('lingxi-resource-response',event=>{if(event.detail.command==='select_folder')window.__directoryResults.push(event.detail.result);});document.querySelector('details').open=true;document.querySelector('[data-testid=select-folder]').click();return true;",json!([]),false).await;
    fs::write(
        root.join("choose-ready.json"),
        serde_json::to_vec(&json!({"phase":"choose","folder":folder})).unwrap(),
    )
    .unwrap();
    let selected = driver.wait_script("return window.__directoryResults.length===1 && {result:window.__directoryResults[0],input:document.querySelector('[aria-label=技能目录]').value,notice:document.querySelector('[role=status]')?.textContent};",json!([]),Duration::from_secs(240)).await;
    assert_eq!(selected["result"], json!(folder));
    assert_eq!(selected["input"], json!(folder));
    assert!(selected["notice"].as_str().unwrap().contains("尚未安装"));
    assert_eq!(fs::read(&document).unwrap(), before);
    let screenshot = driver
        .request(
            reqwest::Method::GET,
            &format!("/session/{}/screenshot", driver.session),
            Value::Null,
        )
        .await;
    fs::write(
        root.join("selected.png"),
        STANDARD.decode(screenshot.as_str().unwrap()).unwrap(),
    )
    .unwrap();
    driver
        .script(
            "document.querySelector('[data-testid=select-folder]').click();return true;",
            json!([]),
            false,
        )
        .await;
    fs::write(root.join("cancel-ready.json"), b"{\"phase\":\"cancel\"}").unwrap();
    let cancelled=driver.wait_script("return window.__directoryResults.length===2 && {result:window.__directoryResults[1],input:document.querySelector('[aria-label=技能目录]').value,notice:document.querySelector('[role=status]')?.textContent};",json!([]),Duration::from_secs(240)).await;
    assert_eq!(cancelled["result"], Value::Null);
    assert_eq!(cancelled["input"], json!(folder));
    assert!(cancelled["notice"].as_str().unwrap().contains("已取消"));
    assert_eq!(fs::read(&document).unwrap(), before);
    assert_eq!(fs::read_dir(&folder).unwrap().count(), 1);
    let home = PathBuf::from(std::env::var_os("LINGXI_HOME").expect("需要隔离业务目录"));
    assert!(!home.join("skills").exists());
    fs::write(
        root.join("selection-and-cancel.json"),
        serde_json::to_vec_pretty(&json!({"selected":selected,"cancelled":cancelled})).unwrap(),
    )
    .unwrap();
    let opened = driver
        .invoke(
            "open_skill_viewer",
            json!({"data":{"name":"未安装目录","baseDir":folder,"installed":false}}),
        )
        .await;
    assert_eq!(opened["ok"], true);
    let start = Instant::now();
    let other = loop {
        let handles = driver.handles().await;
        if let Some(h) = handles.into_iter().find(|h| h != &main) {
            break h;
        }
        assert!(start.elapsed() < Duration::from_secs(20));
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    driver.window(&other).await;
    driver.wait_script("return !!window.__TAURI__?.core?.invoke && !!document.querySelector('[data-testid=native-skill-viewer]');", json!([]), Duration::from_secs(30)).await;
    let refused = driver.invoke("select_folder", json!({})).await;
    assert_eq!(refused["ok"], false, "只读窗口不能开启系统选择器");
    assert_eq!(fs::read(&document).unwrap(), before);
    assert!(!home.join("skills").exists());
    fs::write(root.join("receipt.json"),serde_json::to_vec_pretty(&json!({"checks":["actual-native-dialog-selected-path","current-ui-retains-selected-path-not-installed","actual-cancel-null-keeps-existing-selection","source-bytes-unchanged-no-install","readonly-window-rejected"],"selected":selected,"cancelled":cancelled,"readonly":refused})).unwrap()).unwrap();
}
