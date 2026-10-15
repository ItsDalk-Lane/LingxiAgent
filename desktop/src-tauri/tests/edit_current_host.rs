#![cfg(all(feature = "e2e-test", target_os = "macos"))]
use serde_json::{json, Value};
use std::{
    fs,
    net::TcpListener,
    process::{Child, Command},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
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
async fn current_tauri_native_edit_four_commands_and_refusals() {
    let root = std::env::temp_dir().join(format!(
        "r06-tauri-edit-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let note = root.join("synthetic.txt");
    fs::write(&note, "合成编辑样例").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let log = fs::File::create(root.join("native-app.log")).unwrap();
    let app = Command::new(env!("CARGO_BIN_EXE_lingxi-resource-desktop"))
        .env("TAURI_WEBDRIVER_PORT", port.to_string())
        .env("LINGXI_HOME", root.join("home"))
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let _app = NativeApp(app);
    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    let start = Instant::now();
    while client.get(format!("{base}/status")).send().await.is_err() {
        assert!(start.elapsed() < Duration::from_secs(30), "真实Tauri未启动");
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
            Duration::from_secs(15),
        )
        .await;
    driver.script(r#"const e=document.querySelector('[aria-label="本地文件路径"]');Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(e,arguments[0]);e.dispatchEvent(new Event('input',{bubbles:true}));return true;"#,json!([note]),false).await;
    driver
        .script(
            "document.querySelector('[data-testid=open-path]').click();return true;",
            json!([]),
            false,
        )
        .await;
    driver.wait_script(r#"return document.querySelector('[data-testid=file-editor]')?.value==="合成编辑样例";"#,json!([]),Duration::from_secs(10)).await;
    // 取消原生事件的默认剪贴板动作，仅记录事件名，不读取用户剪贴板内容。
    driver.script("window.editEvents=[];for(const name of ['cut','copy','paste'])document.addEventListener(name,e=>{e.preventDefault();window.editEvents.push(e.type)});document.querySelector('[data-testid=file-editor]').focus();return true;",json!([]),false).await;
    assert_eq!(
        driver
            .invoke("run_edit_command", json!({"command":"selectAll"}))
            .await["value"],
        true
    );
    assert_eq!(driver.script("const e=document.querySelector('[data-testid=file-editor]');return {start:e.selectionStart,end:e.selectionEnd};",json!([]),false).await,json!({"start":0,"end":6}));
    for (index, command) in ["cut", "copy", "paste"].iter().enumerate() {
        assert_eq!(
            driver
                .invoke("run_edit_command", json!({"command":command}))
                .await["value"],
            true
        );
        driver
            .wait_script(
                "return window.editEvents.length===arguments[0];",
                json!([index + 1]),
                Duration::from_secs(5),
            )
            .await;
        assert_eq!(
            driver
                .script(
                    "return window.editEvents[arguments[0]];",
                    json!([index]),
                    false
                )
                .await,
            *command
        );
        assert_eq!(
            driver
                .script(
                    "return document.querySelector('[data-testid=file-editor]').value;",
                    json!([]),
                    false
                )
                .await,
            "合成编辑样例"
        );
    }
    let bad = [
        "",
        "exec",
        "shell",
        "undo",
        "delete",
        "executeJavaScript",
        "copy; rm -rf /",
        "Copy",
        "__proto__",
    ];
    for command in bad {
        let response = driver
            .invoke("run_edit_command", json!({"command":command}))
            .await;
        assert_eq!(response["ok"], false);
        assert!(
            response["invokeError"]
                .as_str()
                .unwrap()
                .contains("Unknown edit command"),
            "{response}"
        );
    }
    assert_eq!(
        driver
            .script("return window.editEvents;", json!([]), false)
            .await,
        json!(["cut", "copy", "paste"])
    );
    let view = driver.invoke("spawn_viewer", json!({"path":note})).await;
    assert_eq!(view["ok"], true, "{view}");
    let start = Instant::now();
    let viewer = loop {
        if let Some(handle) = driver.handles().await.into_iter().find(|h| h != &main) {
            break handle;
        }
        assert!(start.elapsed() < Duration::from_secs(10));
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    driver.window(&viewer).await;
    driver
        .wait_script(
            "return document.querySelector('[data-testid=file-editor]')?.readOnly===true;",
            json!([]),
            Duration::from_secs(10),
        )
        .await;
    for command in ["cut", "copy", "paste", "selectAll"] {
        assert_eq!(
            driver
                .invoke("run_edit_command", json!({"command":command}))
                .await["ok"],
            false,
            "只读窗口不能修改或向主窗派发"
        );
    }
    driver.window(&main).await;
    assert_eq!(
        driver
            .script("return window.editEvents;", json!([]), false)
            .await,
        json!(["cut", "copy", "paste"])
    );
    assert_eq!(fs::read_to_string(&note).unwrap(), "合成编辑样例");
    let receipt = json!({"status":"PASS_MAC_TAURI_ACTUAL","allowed":4,"unknownRefused":9,"readonlyRefused":4,"actualNativeEvents":["cut","copy","paste"],"actualSelectAll":{"start":0,"end":6},"originalFileUnchanged":true,"fixtureReadsClipboardContents":false,"root":root});
    fs::write(
        root.join("result.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    println!("TAURI_EDIT_RECEIPT {receipt}");
    driver
        .request(
            reqwest::Method::DELETE,
            &format!("/session/{}", driver.session),
            Value::Null,
        )
        .await;
}
