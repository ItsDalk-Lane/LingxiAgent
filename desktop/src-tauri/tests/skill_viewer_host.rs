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
async fn original_native_skill_viewer_real_window_file_tree_and_readonly_boundaries() {
    let root = PathBuf::from(
        std::env::var_os("LINGXI_SKILLS_NATIVE_PROOF").expect("需要本轮合成窗口证据目录"),
    );
    fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let skill = root.join("sample-skill");
    fs::create_dir_all(skill.join("examples")).unwrap();
    let document = skill.join("SKILL.md");
    let selected = skill.join("examples/中文.txt");
    fs::write(
        &document,
        "---\nname: viewer-proof\n---\n# 默认技能正文 <script>此处只是文本</script>\n",
    )
    .unwrap();
    fs::write(&selected, "所选配套文件真实正文").unwrap();
    let outside = root.join("outside.txt");
    fs::write(&outside, "不属于此技能的资料").unwrap();
    let large = skill.join("oversized.txt");
    fs::File::create(&large)
        .unwrap()
        .set_len(2 * 1024 * 1024 + 1)
        .unwrap();
    let before = fs::read(&document).unwrap();
    let port = std::env::var("TAURI_WEBDRIVER_PORT").expect("需要隔离驱动端口");
    let native_log = fs::File::create(root.join("native-app.log")).unwrap();
    let app = Command::new(env!("CARGO_BIN_EXE_lingxi-resource-desktop"))
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
    assert_eq!(
        driver
            .invoke("list_skill_files", json!({"baseDir":"relative"}))
            .await["value"],
        json!([])
    );
    assert_eq!(
        driver
            .invoke("list_skill_files", json!({"baseDir":outside}))
            .await["value"],
        json!([])
    );
    assert_eq!(
        driver
            .invoke("read_skill_file", json!({"path":"relative"}))
            .await["value"],
        Value::Null
    );
    assert_eq!(
        driver
            .invoke("read_skill_file", json!({"path":large}))
            .await["value"],
        Value::Null
    );
    driver.script("const input=document.querySelector('input[aria-label=技能目录]');input.closest('details').open=true;Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(input,arguments[0]);input.dispatchEvent(new Event('input',{bubbles:true}));return true;",json!([skill]),false).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    driver
        .script(
            "document.querySelector('[data-testid=open-skill-viewer]').click();return true;",
            json!([]),
            false,
        )
        .await;
    let started = Instant::now();
    let viewer = loop {
        let handles = driver.handles().await;
        if let Some(handle) = handles.iter().find(|handle| **handle != main) {
            break handle.clone();
        }
        assert!(started.elapsed() < Duration::from_secs(20));
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    driver.window(&viewer).await;
    driver.wait_script("return document.querySelector('[data-testid=skill-content]')?.textContent.includes('默认技能正文');",json!([]),Duration::from_secs(30)).await;
    assert_eq!(
        driver.invoke("skill_viewer_load", json!({})).await["value"]["installed"],
        true
    );
    assert_eq!(
        driver
            .invoke("read_skill_file", json!({"path":outside}))
            .await["value"],
        Value::Null
    );
    assert_eq!(
        driver
            .invoke("list_skill_files", json!({"baseDir":root}))
            .await["value"],
        json!([])
    );
    assert_eq!(
        driver
            .invoke("write_file", json!({"path":document,"content":"禁止写入"}))
            .await["ok"],
        false
    );
    driver.script("const folder=Array.from(document.querySelectorAll('summary')).find(node=>node.textContent==='examples');folder.click();Array.from(document.querySelectorAll('button')).find(node=>node.textContent==='中文.txt').click();return true;",json!([]),false).await;
    driver.wait_script("return document.querySelector('[data-testid=skill-content]')?.textContent==='所选配套文件真实正文';",json!([]),Duration::from_secs(10)).await;
    assert_eq!(driver.script("return document.querySelectorAll('iframe').length===0 && !document.body.innerText.includes('复制到技能');",json!([]),false).await,true);
    let screenshot = driver
        .request(
            reqwest::Method::GET,
            &format!("/session/{}/screenshot", driver.session),
            Value::Null,
        )
        .await;
    fs::write(
        root.join("skill-viewer.png"),
        STANDARD.decode(screenshot.as_str().unwrap()).unwrap(),
    )
    .unwrap();
    fs::write(root.join("receipt.json"),serde_json::to_vec_pretty(&json!({"checks":["actual_tauri_main_entry","invalid_relative_and_non_directory_empty","oversize_null","actual_separate_skill_window","default_document_content","actual_file_tree_selected_text","installed_true_no_copy","foreign_path_refused","write_command_refused","source_bytes_unchanged","read_never_installs"],"main":main,"viewer":viewer,"source":document,"screenshot":root.join("skill-viewer.png")})).unwrap()).unwrap();
    driver.script("Array.from(document.querySelectorAll('button')).find(node=>node.textContent==='oversized.txt').click();return true;",json!([]),false).await;
    driver
        .wait_script(
            "return !!document.querySelector('[data-testid=skill-empty]');",
            json!([]),
            Duration::from_secs(10),
        )
        .await;
    assert_eq!(fs::read(&document).unwrap(), before);
    assert!(!root.join("skills").exists());
    driver.invoke("skill_viewer_close", json!({})).await;
    println!("R06_SKILL_NATIVE_PROOF {}", root.display());
}
