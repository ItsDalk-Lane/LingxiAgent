#![cfg(feature = "e2e-test")]
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command},
    time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
struct ClipboardRestore(Vec<u8>);
#[cfg(target_os = "macos")]
impl Drop for ClipboardRestore {
    fn drop(&mut self) {
        use std::io::Write;
        if let Ok(mut child) = Command::new("/usr/bin/pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(&self.0);
            }
            let _ = child.wait();
        }
    }
}

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

fn phase(root: &std::path::Path, phase: &str, file: &std::path::Path) {
    let value = json!({"phase":phase,"file":file,"pid":std::process::id(),"at":format!("{:?}", std::time::SystemTime::now())});
    fs::write(
        root.join("e2e-phase.json"),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
    println!("NATIVE_UI_PHASE {value}");
}

#[tokio::test]
async fn native_desktop_resource_roundtrip_real_gui() {
    let root = PathBuf::from(
        std::env::var_os("LINGXI_DESKTOP_PROOF_DIR")
            .expect("需要合成的 LINGXI_DESKTOP_PROOF_DIR，不跳过真实桌面检查"),
    );
    assert!(root.is_absolute());
    fs::create_dir_all(&root).unwrap();
    assert!(
        !root.join("os-open-observation.json").exists()
            && !root.join("system-trash-observation.json").exists(),
        "原生观察必须来自本轮新目录，拒绝旧回执"
    );
    let selected = root.join("selected.txt");
    let note = root.join("live.md");
    let binary = root.join("nested/data.bin");
    let copied = root.join("copied.md");
    fs::write(&selected, "原生选择不修改文件").unwrap();
    fs::write(&note, "# initial\n").unwrap();
    let port = std::env::var("TAURI_WEBDRIVER_PORT").expect("需要指定隔离驱动端口");
    let native_log = fs::File::create(root.join("native-app.log")).unwrap();
    let app = Command::new(env!("CARGO_BIN_EXE_lingxi-resource-desktop"))
        .stdout(native_log.try_clone().unwrap())
        .stderr(native_log)
        .spawn()
        .unwrap();
    fs::write(root.join("native-app-pid.txt"), app.id().to_string()).unwrap();
    let _app = NativeApp(app);
    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(320))
        .build()
        .unwrap();
    let started = Instant::now();
    while client.get(format!("{base}/status")).send().await.is_err() {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "真实宿主未启动"
        );
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
    driver
        .request(
            reqwest::Method::POST,
            &format!("/session/{}/timeouts", driver.session),
            json!({"script":300000}),
        )
        .await;
    let main = driver.handles().await.into_iter().next().unwrap();
    driver.window(&main).await;
    driver
        .wait_script(
            "return !!document.querySelector('[data-testid=resource-desktop]');",
            json!([]),
            Duration::from_secs(30),
        )
        .await;

    // 观察当前 renderer 适配器派发的真实回包，不替换宿主、对话框或文件读写。
    driver.script("window.__nativeSelections=[];window.__nativeEvents=[];window.addEventListener('lingxi-resource-response',e=>{if(e.detail.command==='select_files')window.__nativeSelections.push(e.detail.result);if(e.detail.command==='resource_events')window.__nativeEvents.push(e.detail.result);});", json!([]), false).await;
    phase(&root, "PICK_FILE", &selected);
    driver
        .script(
            "document.querySelector('[data-testid=select-files]').click();return true;",
            json!([]),
            false,
        )
        .await;
    driver
        .wait_script(
            "return window.__nativeSelections.length===1;",
            json!([]),
            Duration::from_secs(300),
        )
        .await;
    let pick = driver
        .script("return window.__nativeSelections[0];", json!([]), false)
        .await;
    assert_eq!(pick, json!([selected]));
    assert_eq!(fs::read_to_string(&selected).unwrap(), "原生选择不修改文件");
    assert_eq!(
        driver
            .script(
                "return !!document.querySelector('[data-testid=file-editor]');",
                json!([]),
                false
            )
            .await,
        false
    );
    phase(&root, "CANCEL_PICKER", &selected);
    driver
        .script(
            "document.querySelector('[data-testid=select-files]').click();return true;",
            json!([]),
            false,
        )
        .await;
    driver
        .wait_script(
            "return window.__nativeSelections.length===2;",
            json!([]),
            Duration::from_secs(300),
        )
        .await;
    assert_eq!(
        driver
            .script("return window.__nativeSelections[1];", json!([]), false)
            .await,
        json!([])
    );
    assert_eq!(
        driver
            .script(
                "return document.querySelectorAll('aside [data-path]').length;",
                json!([]),
                false
            )
            .await,
        1
    );

    let open_ui = "const input=document.querySelector('[aria-label=\"本地文件路径\"]');Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(input,arguments[0]);input.dispatchEvent(new Event('input',{bubbles:true}));return true;";
    driver.script(open_ui, json!([note]), false).await;
    driver
        .script(
            "document.querySelector('[data-testid=open-path]').click();return true;",
            json!([]),
            false,
        )
        .await;
    driver
        .wait_script(
            "return document.querySelector('[data-testid=file-editor]')?.value==='# initial\\n';",
            json!([]),
            Duration::from_secs(15),
        )
        .await;
    fs::write(&note, "# resource-changed\n").unwrap();
    driver.wait_script("return document.querySelector('[data-testid=file-editor]')?.value==='# resource-changed\\n';", json!([]), Duration::from_secs(15)).await;
    let resource_evidence = driver.script("return window.__nativeEvents.filter(v=>(v.events||[]).some(e=>e.type==='resource.changed'));", json!([]), false).await;
    assert!(
        !resource_evidence.as_array().unwrap().is_empty(),
        "须消费现行 Rust ResourceIO 的真实 resource.changed"
    );
    fs::write(
        root.join("resource-events.json"),
        serde_json::to_vec_pretty(&resource_evidence).unwrap(),
    )
    .unwrap();
    driver.script("const done=arguments[arguments.length-1];window.__legacyChanges=[];window.__TAURI__.event.listen('file-changed',e=>window.__legacyChanges.push(e.payload)).then(()=>done(true),e=>done({error:String(e)}));", json!([]), true).await;
    assert_eq!(
        driver
            .invoke("watch_file", json!({"path":"relative"}))
            .await["value"],
        false
    );
    assert_eq!(
        driver.invoke("watch_file", json!({"path":note})).await["value"],
        true
    );
    fs::write(&note, "# legacy and resource\n").unwrap();
    driver
        .wait_script(
            "return window.__legacyChanges.length>0;",
            json!([]),
            Duration::from_secs(15),
        )
        .await;
    assert_eq!(
        driver
            .script(
                "return window.__legacyChanges[0].filePath;",
                json!([]),
                false
            )
            .await,
        json!(note)
    );
    assert_eq!(
        driver.invoke("unwatch_file", json!({"path":note})).await["value"],
        true
    );

    let snap = driver
        .invoke("read_file_snapshot", json!({"path":note}))
        .await;
    assert_eq!(snap["ok"], true);
    let version = snap["value"]["version"].clone();
    let saved = driver
        .invoke(
            "write_file_if_unchanged",
            json!({"path":note,"content":"# version saved\n","expectedVersion":version}),
        )
        .await;
    assert_eq!(saved["value"]["ok"], true);
    assert_eq!(fs::read_to_string(&note).unwrap(), "# version saved\n");
    let after = driver
        .invoke("read_file_snapshot", json!({"path":note}))
        .await;
    assert_eq!(saved["value"]["version"], after["value"]["version"]);
    fs::write(&note, "# external remains\n").unwrap();
    let rejected = driver
        .invoke(
            "write_file_if_unchanged",
            json!({"path":note,"content":"bad","expectedVersion":saved["value"]["version"]}),
        )
        .await;
    assert_eq!(rejected["value"]["ok"], false);
    assert_eq!(rejected["value"]["conflict"], true);
    assert_eq!(fs::read_to_string(&note).unwrap(), "# external remains\n");
    driver.wait_script("return document.querySelector('[data-testid=file-editor]')?.value==='# external remains\\n';", json!([]), Duration::from_secs(15)).await;
    #[cfg(target_os = "macos")]
    {
        let _clipboard =
            ClipboardRestore(Command::new("/usr/bin/pbpaste").output().unwrap().stdout);
        driver
            .script(
                "document.querySelector('[data-testid=file-editor]').focus();return true;",
                json!([]),
                false,
            )
            .await;
        assert_eq!(
            driver
                .invoke("run_edit_command", json!({"command":"selectAll"}))
                .await["value"],
            true
        );
        assert_eq!(driver.script("const e=document.querySelector('[data-testid=file-editor]');return e.selectionStart===0&&e.selectionEnd===e.value.length;", json!([]), false).await, true);
        assert_eq!(
            driver
                .invoke("run_edit_command", json!({"command":"copy"}))
                .await["value"],
            true
        );
        assert_eq!(
            Command::new("/usr/bin/pbpaste").output().unwrap().stdout,
            b"# external remains\n"
        );
        assert_eq!(
            driver
                .invoke("run_edit_command", json!({"command":"cut"}))
                .await["value"],
            true
        );
        driver
            .wait_script(
                "return document.querySelector('[data-testid=file-editor]')?.value==='';",
                json!([]),
                Duration::from_secs(5),
            )
            .await;
        assert_eq!(
            driver
                .invoke("run_edit_command", json!({"command":"paste"}))
                .await["value"],
            true
        );
        driver.wait_script("return document.querySelector('[data-testid=file-editor]')?.value==='# external remains\\n';", json!([]), Duration::from_secs(5)).await;
    }
    driver.script("const editor=document.querySelector('[data-testid=file-editor]');Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value').set.call(editor,'UNSAVED MAIN');editor.dispatchEvent(new Event('input',{bubbles:true}));return true;", json!([]), false).await;
    driver
        .script(
            "document.querySelector('[data-testid=spawn-viewer]').click();return true;",
            json!([]),
            false,
        )
        .await;
    let start = Instant::now();
    let viewer = loop {
        let handles = driver.handles().await;
        if let Some(v) = handles.into_iter().find(|h| h != &main) {
            break v;
        }
        assert!(start.elapsed() < Duration::from_secs(15));
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    driver.window(&viewer).await;
    driver
        .wait_script(
            "return document.querySelector('[data-testid=file-editor]')?.readOnly===true;",
            json!([]),
            Duration::from_secs(15),
        )
        .await;
    assert_eq!(
        driver
            .script(
                "return document.querySelector('[data-testid=file-editor]').value;",
                json!([]),
                false
            )
            .await,
        "# external remains\n"
    );
    assert_eq!(
        driver
            .invoke("write_file", json!({"path":note,"content":"BAD VIEWER"}))
            .await["ok"],
        false
    );
    assert_eq!(fs::read_to_string(&note).unwrap(), "# external remains\n");
    assert_eq!(
        driver.invoke("read_file", json!({"path":selected})).await["value"],
        Value::Null
    );
    driver.window(&main).await;
    assert_eq!(
        driver
            .script(
                "return document.querySelector('[data-testid=file-editor]').value;",
                json!([]),
                false
            )
            .await,
        "UNSAVED MAIN"
    );
    fs::write(&note, "# later external\n").unwrap();
    driver
        .script(
            "document.querySelector('[data-testid=version-save]').click();return true;",
            json!([]),
            false,
        )
        .await;
    driver.wait_script("return document.querySelector('[data-testid=operation-error]')?.textContent.includes('保存失败');", json!([]), Duration::from_secs(15)).await;
    assert_eq!(fs::read_to_string(&note).unwrap(), "# later external\n");
    assert_eq!(
        driver
            .invoke(
                "write_file_binary",
                json!({"path":binary,"data":STANDARD.encode([0,1,255])})
            )
            .await["value"],
        true
    );
    assert_eq!(fs::read(&binary).unwrap(), [0, 1, 255]);
    assert_eq!(
        driver
            .invoke("read_file_base64", json!({"path":binary}))
            .await["value"],
        STANDARD.encode([0, 1, 255])
    );
    assert_eq!(
        driver
            .invoke("copy_file", json!({"source":note,"destination":copied}))
            .await["value"],
        true
    );
    assert_eq!(fs::read(&copied).unwrap(), fs::read(&note).unwrap());
    assert!(note.is_file());
    assert_eq!(
        driver
            .invoke("run_edit_command", json!({"command":"exec"}))
            .await["ok"],
        false
    );

    // 默认程序的实际窗口不能由 open 的退出码代替：此阶段需 CUA 核实并留下原始观察。
    phase(&root, "OS_OPEN_WINDOW", &selected);
    assert_eq!(
        driver.invoke("open_file", json!({"path":selected})).await["value"],
        true
    );
    let observation = root.join("os-open-observation.json");
    let start = Instant::now();
    while !observation.is_file() {
        assert!(
            start.elapsed() < Duration::from_secs(300),
            "缺少真实 OS 默认程序窗口观察"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let evidence: Value = serde_json::from_slice(&fs::read(observation).unwrap()).unwrap();
    assert_eq!(evidence["path"], json!(selected));
    assert_eq!(evidence["observedWindow"], true);
    assert!(!evidence["program"].as_str().unwrap_or("").is_empty());
    assert!(!evidence["windowTitle"].as_str().unwrap_or("").is_empty());
    assert!(PathBuf::from(evidence["screenshot"].as_str().expect("实际窗口截图")).is_file());
    let trash = root.join(format!("lingxi-native-trash-{}.txt", std::process::id()));
    fs::write(&trash, "真实系统废纸篓载荷").unwrap();
    driver.script(open_ui, json!([trash]), false).await;
    driver
        .script(
            "document.querySelector('[data-testid=open-path]').click();return true;",
            json!([]),
            false,
        )
        .await;
    driver
        .wait_script(
            "return document.querySelector('[data-testid=file-path]')?.textContent===arguments[0];",
            json!([trash]),
            Duration::from_secs(15),
        )
        .await;
    driver
        .script(
            "document.querySelector('[data-testid=trash-item]').click();return true;",
            json!([]),
            false,
        )
        .await;
    driver.wait_script("return !Array.from(document.querySelectorAll('aside [data-path]')).some(e=>e.dataset.path===arguments[0]);", json!([trash]), Duration::from_secs(15)).await;
    assert!(!trash.exists(), "实际移到废纸篓后桌面列表移除原文件");
    phase(&root, "VERIFY_SYSTEM_TRASH", &trash);
    let receipt_path = root.join("system-trash-observation.json");
    let start = Instant::now();
    while !receipt_path.is_file() {
        assert!(
            start.elapsed() < Duration::from_secs(300),
            "须确认真实系统 Trash 载荷，不能仅源文件消失"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let receipt: Value = serde_json::from_slice(&fs::read(receipt_path).unwrap()).unwrap();
    assert_eq!(receipt["source"], json!(trash));
    let trashed = PathBuf::from(receipt["trashPath"].as_str().unwrap());
    assert_eq!(fs::read_to_string(trashed).unwrap(), "真实系统废纸篓载荷");
    driver
        .request(
            reqwest::Method::DELETE,
            &format!("/session/{}", driver.session),
            Value::Null,
        )
        .await;
    phase(&root, "COMPLETE", &selected);
}
