#![cfg(all(feature = "e2e-test", target_os = "macos"))]
use base64::{engine::general_purpose::STANDARD, Engine as _};
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
            if started.elapsed() >= limit {
                let diagnostic=self.script("return {body:document.body.innerText,editor:document.querySelector('[data-testid=file-editor]')?.value,html:document.querySelector('iframe')?.getAttribute('srcdoc')};",json!([]),false).await;
                panic!("界面未到达要求状态：{script} expected={args} actual={diagnostic}");
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

// 此父测试由现行服务 Cargo 测试传入本轮实际服务二进制，不接受历史收据。
#[tokio::test]
async fn current_tauri_resources_real_preview_watch_and_system_trash() {
    let root = std::env::temp_dir().join(format!(
        "r06-native-resource-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let home = root.join("home");
    let workspace = root.join("workspace");
    fs::create_dir(&workspace).unwrap();
    let config = root.join("config.json");
    fs::write(
        &config,
        serde_json::to_vec(&json!({"home":home,"workspace":workspace})).unwrap(),
    )
    .unwrap();
    let service_log = fs::File::create(root.join("service.log")).unwrap();
    let _service = NativeApp(
        Command::new(
            std::env::var_os("LINGXI_NATIVE_SERVICE_BINARY").expect("必须使用当前服务Cargo二进制"),
        )
        .args(["--config", config.to_str().unwrap()])
        .env("LINGXI_HOME", &home)
        .stdout(service_log.try_clone().unwrap())
        .stderr(service_log)
        .spawn()
        .unwrap(),
    );
    let started = Instant::now();
    while !home.join("lingxi-service/instance.json").exists() {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "真实Rust服务未就绪"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let note = workspace.join("synthetic-live.md");
    fs::write(&note, "# 初始正文\n").unwrap();
    let text = workspace.join("plain.txt");
    fs::write(&text, "原始纯文本").unwrap();
    let binary = workspace.join("small.bin");
    fs::write(&binary, [0, 1, 2, 3]).unwrap();
    let large = workspace.join("large.txt");
    fs::File::create(&large)
        .unwrap()
        .set_len(21 * 1024 * 1024)
        .unwrap();
    let broken = workspace.join("broken.docx");
    fs::write(&broken, b"broken actual ZIP").unwrap();
    // 使用原有产品依赖生成真实Word/Excel归档，不替换转换器或原生读文件。
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let node=Command::new("node").current_dir(&repo).arg("--input-type=module").arg("-e").arg(r#"
      import fs from 'node:fs'; import JSZip from 'jszip'; import ExcelJS from 'exceljs';
      const root=process.argv[1],zip=new JSZip();
      zip.file('[Content_Types].xml','<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>');
      zip.file('_rels/.rels','<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="r1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>');
      zip.file('word/document.xml','<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>当前原生Word正文</w:t></w:r></w:p></w:body></w:document>');
      fs.writeFileSync(root+'/actual.docx',await zip.generateAsync({type:'nodebuffer'}));
      const book=new ExcelJS.Workbook();book.addWorksheet('First').addRow(['第一张表的真实内容','<script>拒绝执行</script>']);book.addWorksheet('Second').addRow(['不应显示第二张表']);fs.writeFileSync(root+'/actual.xlsx',Buffer.from(await book.xlsx.writeBuffer()));
    "#).arg(&workspace).output().unwrap();
    assert!(
        node.status.success(),
        "{}",
        String::from_utf8_lossy(&node.stderr)
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let log = fs::File::create(root.join("native-app.log")).unwrap();
    let _app = NativeApp(
        Command::new(env!("CARGO_BIN_EXE_lingxi-resource-desktop"))
            .env("TAURI_WEBDRIVER_PORT", port.to_string())
            .env("LINGXI_HOME", &home)
            .env("LINGXI_RESOURCE_SERVICE_HOME", &home)
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap(),
    );
    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
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
    let main = driver.handles().await[0].clone();
    driver.window(&main).await;
    driver
        .wait_script(
            "return !!document.querySelector('[data-testid=resource-desktop]');",
            json!([]),
            Duration::from_secs(15),
        )
        .await;
    // 观察真实事件回包，既不替换invoke也不通过旧监听驱动当前页面。
    driver.script("window.actualResourceEvents=[];window.legacyEvents=[];window.addEventListener('lingxi-resource-response',e=>{if(e.detail.command==='resource_events')window.actualResourceEvents.push(e.detail.result)});return true;",json!([]),false).await;
    driver.script("const done=arguments[arguments.length-1];window.__TAURI__.event.listen('file-changed',e=>window.legacyEvents.push(e.payload)).then(()=>done(true));",json!([]),true).await;
    assert_eq!(
        driver
            .invoke("watch_file", json!({"path":"relative.md"}))
            .await["value"],
        false
    );
    assert_eq!(
        driver.invoke("watch_file", json!({"path":note})).await["value"],
        true
    );
    tokio::time::sleep(Duration::from_millis(350)).await;
    open_path(&driver, &note).await;
    driver
        .wait_script(
            "return document.querySelector('[data-testid=file-editor]')?.value===arguments[0];",
            json!(["# 初始正文\n"]),
            Duration::from_secs(10),
        )
        .await;
    assert_eq!(
        driver
            .script(
                "return document.querySelector('[data-testid=legacy-watch-toggle]')===null;",
                json!([]),
                false
            )
            .await,
        true
    );
    fs::write(&note, "# 外部修改后正文\n").unwrap();
    driver
        .wait_script(
            "return window.legacyEvents.some(e=>e.filePath===arguments[0]);",
            json!([note]),
            Duration::from_secs(10),
        )
        .await;
    driver
        .wait_script(
            "return document.querySelector('[data-testid=file-editor]')?.value===arguments[0];",
            json!(["# 外部修改后正文\n"]),
            Duration::from_secs(10),
        )
        .await;
    driver.wait_script("return window.actualResourceEvents.some(r=>(r.events||[]).some(e=>e.type==='resource.changed'&&e.resourceKey===arguments[0]));",json!([format!("local_fs:{}",note.display())]),Duration::from_secs(10)).await;
    assert_eq!(
        driver.invoke("unwatch_file", json!({"path":note})).await["value"],
        true
    );
    println!("NATIVE_RESOURCE_CASE legacy_absolute_registration_and_current_resource_changed PASS");
    open_path(&driver, &text).await;
    driver
        .wait_script(
            "return document.querySelector('[data-testid=file-editor]')?.value==='原始纯文本';",
            json!([]),
            Duration::from_secs(10),
        )
        .await;
    for (file, selector, needle) in [
        (
            note.clone(),
            "markdown-preview",
            "外部修改后正文".to_string(),
        ),
        (
            workspace.join("actual.docx"),
            "office-preview",
            "当前原生Word正文".to_string(),
        ),
        (
            workspace.join("actual.xlsx"),
            "office-preview",
            "第一张表的真实内容".to_string(),
        ),
    ] {
        open_path(&driver, &file).await;
        let value=driver.wait_script("const f=document.querySelector('[data-testid='+arguments[0]+']');return f?.getAttribute('srcdoc')?.includes(arguments[1])?f.getAttribute('srcdoc'):false;",json!([selector,needle]),Duration::from_secs(15)).await;
        assert!(!value.as_str().unwrap().contains("不应显示第二张表"));
        save_window(
            &driver,
            &root.join(format!(
                "preview-{}.png",
                file.file_name().unwrap().to_string_lossy()
            )),
        )
        .await;
        assert_eq!(driver.script("return document.querySelector('[data-testid='+arguments[0]+']').getAttribute('sandbox');",json!([selector]),false).await,"");
    }
    for file in [&binary, &large] {
        open_path(&driver, file).await;
        driver.wait_script("return document.querySelector('[data-testid=file-info]')?.textContent.includes('字节');",json!([]),Duration::from_secs(10)).await;
    }
    open_path(&driver, &broken).await;
    driver
        .wait_script(
            "return !!document.querySelector('[data-testid=operation-error]')?.textContent;",
            json!([]),
            Duration::from_secs(10),
        )
        .await;
    open_path(&driver, &note).await;
    driver
        .wait_script(
            "return !!document.querySelector('[data-testid=file-editor]');",
            json!([]),
            Duration::from_secs(10),
        )
        .await;
    driver.script("const e=document.querySelector('[data-testid=file-editor]');Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value').set.call(e,'主窗口未保存的编辑');e.dispatchEvent(new Event('input',{bubbles:true}));return true;",json!([]),false).await;
    driver
        .script(
            "document.querySelector('[data-testid=spawn-viewer]').click();return true;",
            json!([]),
            false,
        )
        .await;
    let started = Instant::now();
    let viewer = loop {
        if let Some(h) = driver.handles().await.into_iter().find(|h| h != &main) {
            break h;
        }
        assert!(started.elapsed() < Duration::from_secs(10));
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
    assert_eq!(
        driver.invoke("read_file", json!({"path":text})).await["value"],
        Value::Null
    );
    assert_eq!(
        driver
            .invoke("write_file", json!({"path":note,"content":"不得写入"}))
            .await["ok"],
        false
    );
    fs::write(&note, "# 只读窗口自动更新\n").unwrap();
    driver
        .wait_script(
            "return document.querySelector('[data-testid=file-editor]')?.value===arguments[0];",
            json!(["# 只读窗口自动更新\n"]),
            Duration::from_secs(10),
        )
        .await;
    driver.window(&main).await;
    assert_eq!(
        driver
            .script(
                "return document.querySelector('[data-testid=file-editor]').value;",
                json!([]),
                false
            )
            .await,
        "主窗口未保存的编辑"
    );
    save_window(&driver, &root.join("main-unsaved-preserved.png")).await;
    println!("NATIVE_RESOURCE_CASE actual_text_markdown_office_binary_large_error_readonly_isolation PASS");
    let trash_name = format!(
        "r06-only-synthetic-trash-{}.txt",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let trash = workspace.join(&trash_name);
    let payload = "当前原生OS废纸篓唯一合成内容";
    fs::write(&trash, payload).unwrap();
    for bad in [
        "relative.txt".to_string(),
        workspace.join("missing.txt").to_string_lossy().into_owned(),
    ] {
        assert_eq!(
            driver.invoke("trash_item", json!({"path":bad})).await["value"],
            false
        );
    }
    open_path(&driver, &trash).await;
    driver.wait_script("return Array.from(document.querySelectorAll('aside [data-path]')).some(e=>e.dataset.path===arguments[0]);",json!([trash]),Duration::from_secs(10)).await;
    driver
        .script(
            "document.querySelector('[data-testid=trash-item]').click();return true;",
            json!([]),
            false,
        )
        .await;
    driver.wait_script("return !Array.from(document.querySelectorAll('aside [data-path]')).some(e=>e.dataset.path===arguments[0])&&document.body.textContent.includes('已移到系统废纸篓');",json!([trash]),Duration::from_secs(15)).await;
    assert!(!trash.exists());
    // Finder读取本次唯一合成文件的真实废纸篓身份，再复制这一个载荷用于字节核验。
    // 不列举或接触用户其他废纸篓内容，也不永久删除任何文件。
    let recovered = root.join("trash-observation");
    fs::create_dir(&recovered).unwrap();
    let phase = json!({"phase":"VERIFY_OWN_SYSTEM_TRASH","source":trash,"filename":trash_name,"expectedPayload":payload,"observationDirectory":recovered});
    fs::write(
        root.join("system-trash-phase.json"),
        serde_json::to_vec_pretty(&phase).unwrap(),
    )
    .unwrap();
    println!("SYSTEM_TRASH_OBSERVATION_REQUIRED {}", root.display());
    let receipt_path = root.join("system-trash-observation.json");
    let started = Instant::now();
    while !receipt_path.is_file() {
        assert!(
            started.elapsed() < Duration::from_secs(240),
            "须通过本轮真实Finder确认唯一合成文件，不能以源消失代替系统废纸篓证明"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let observation: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    assert_eq!(observation["source"], json!(trash));
    let trash_path = observation["trashPath"].as_str().unwrap().to_owned();
    assert!(
        trash_path.contains("/.Trash/") || trash_path.contains("/.Trashes/"),
        "{trash_path}"
    );
    assert_eq!(
        fs::read_to_string(recovered.join(&trash_name)).unwrap(),
        payload
    );
    println!("NATIVE_RESOURCE_CASE real_system_trash_payload_and_desktop_list PASS");
    let receipt = json!({"status":"PASS_MAC_TAURI_ACTUAL","legacyAbsoluteRegistration":true,"currentViewerUsesResourceChanged":true,"rendererLegacyToggleAbsent":true,"textMarkdownOfficeBinaryLargeError":true,"readonlyIndependentRefresh":true,"unsavedMainPreserved":true,"trashSource":trash,"actualSystemTrashPath":trash_path,"trashRecoveredBytes":payload,"trashMissingAndRelativeFalse":true,"root":root});
    fs::write(
        root.join("result.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    println!("TAURI_RESOURCE_RECEIPT {receipt}");
    driver
        .request(
            reqwest::Method::DELETE,
            &format!("/session/{}", driver.session),
            Value::Null,
        )
        .await;
}
async fn open_path(driver: &Driver, path: &std::path::Path) {
    driver.script(r#"const e=document.querySelector('[aria-label="本地文件路径"]');Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(e,arguments[0]);e.dispatchEvent(new Event('input',{bubbles:true}));return true;"#,json!([path]),false).await;
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
            json!([path]),
            Duration::from_secs(10),
        )
        .await;
}

async fn save_window(driver: &Driver, path: &std::path::Path) {
    let value = driver
        .request(
            reqwest::Method::GET,
            &format!("/session/{}/screenshot", driver.session),
            Value::Null,
        )
        .await;
    let bytes = STANDARD
        .decode(value.as_str().expect("必须为真实WebView截图"))
        .unwrap();
    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    fs::write(path, bytes).unwrap();
}
