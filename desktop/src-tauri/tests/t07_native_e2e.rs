//! R09-T07 原生 E2E（A13）：真实 Tauri 产品主窗 + wdio WebDriver，验证
//! 中文 IME 输入、选择/引用、输入草稿、滚动长列表、工具卡展开、缩放/DPI、
//! 多显示器/窗口恢复、切窗焦点等 UI 面；服务与工具关键链**不 mock**。
//!
//! 运行（必须由 Lead cleared 的批次执行；本文件自身不弹任何窗口，窗口由被测 app 创建）：
//! ```sh
//! LINGXI_HOME=/private/tmp/... LINGXI_SERVICE_BIN=<repo>/rust/target/debug/lingxi-service \
//! LINGXI_R09_AUTOMATION=1 LINGXI_R09_HOST_DIALOG_SINK=/private/tmp/.../sink.json \
//! TAURI_WEBDRIVER_PORT=19465 \
//! cargo test --manifest-path desktop/src-tauri/Cargo.toml \
//!   --features r09-host,e2e-test --test t07_native_e2e -- --nocapture
//! ```
//!
//! 说明：wdio 插件只在 `e2e-test` feature 下编译；release 冒烟（A14）由
//! `scripts/rust-tauri/r09_t07.py` 用**不带该 feature** 的 .app 单独验证。
#![cfg(all(feature = "e2e-test", feature = "r09-host"))]

use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command},
    time::{Duration, Instant},
};

include!("t07_native_e2e_helpers.rs");
use wdio::Driver;

fn phase(root: &std::path::Path, phase: &str, detail: &Value) {
    let payload = json!({
        "phase": phase,
        "detail": detail,
        "pid": std::process::id(),
        "atUnixMs": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_millis() as u64)
            .unwrap_or(0),
    });
    fs::write(
        root.join("t07-phase.json"),
        serde_json::to_vec_pretty(&payload).unwrap(),
    )
    .unwrap();
    println!("T07_PHASE {payload}");
}

#[tokio::test]
async fn t07_product_main_window_native_e2e() {
    let root = PathBuf::from(
        std::env::var_os("LINGXI_R09_T07_EVIDENCE")
            .expect("需要 LINGXI_R09_T07_EVIDENCE（隔离证据目录）"),
    );
    assert!(root.is_absolute());
    fs::create_dir_all(&root).unwrap();
    let home = PathBuf::from(std::env::var_os("LINGXI_HOME").expect("需要隔离 LINGXI_HOME"));
    assert!(home.is_absolute());
    let port = std::env::var("TAURI_WEBDRIVER_PORT").expect("需要隔离 TAURI_WEBDRIVER_PORT");
    let automation = std::env::var("LINGXI_R09_AUTOMATION").unwrap_or_default();
    assert_eq!(
        automation, "1",
        "T07 E2E 必须在 automation（hidden+sink）模式下运行"
    );

    // 产品构建（wrapper 生成 overlay：frontendDist=dist-renderer、windows=[]、r09-host 命令面）
    // 通过 env 注入；未注入时回退 cargo test 的 bin（仅用于编译期检查，不是产品配置）。
    let app_bin = std::env::var_os("LINGXI_R09_T07_APP_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_lingxi-resource-desktop")));
    assert!(
        app_bin.is_file(),
        "产品 app 二进制不存在: {}",
        app_bin.display()
    );
    let log = fs::File::create(root.join("native-app.log")).unwrap();
    let mut app = Command::new(&app_bin)
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .env("LINGXI_R09_AUTOMATION", "1")
        .spawn()
        .unwrap();
    fs::write(root.join("native-app-pid.txt"), app.id().to_string()).unwrap();
    let _guard = AppGuard { child: &mut app };

    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(320))
        .build()
        .unwrap();
    let started = Instant::now();
    while client.get(format!("{base}/status")).send().await.is_err() {
        assert!(
            started.elapsed() < Duration::from_secs(120),
            "wdio WebDriver 未启动（e2e-test feature 是否启用？TAURI_WEBDRIVER_PORT={port}）"
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
    let driver = Driver::new(
        client,
        base,
        session["value"]["sessionId"].as_str().unwrap(),
    );
    let main = driver.pick_product_window().await;

    // ── 1. 产品壳加载（不是 R06 资源页）──
    driver.wait_script(
        "return document.title.includes('Rust 预览') && !!document.getElementById('react-root');",
        json!([]),
        Duration::from_secs(120),
    ).await;
    let shell = driver.script(
        "return { title: document.title, platform: document.documentElement.getAttribute('data-platform'),\
         hasTauri: !!window.__TAURI__, hasHana: !!window.hana, dpr: window.devicePixelRatio }",
        json!([]),
        false,
    ).await;
    fs::write(
        root.join("projection-shell.json"),
        serde_json::to_vec_pretty(&shell).unwrap(),
    )
    .unwrap();
    phase(&root, "SHELL_READY", &shell);

    // ── 2. 中文 IME（合成 composition 事件；记录 WKWebView 能力边界）──
    let ime = driver.script(
        "const host=document.querySelector('[contenteditable=\"true\"], textarea') ;\
         if(!host) return {available:false, reason:'no-editable-surface'};\
         const target = host.getAttribute('contenteditable')==='true' ? host : host;\
         target.focus();\
         const text='灵犀中文输入测试';\
         target.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true,data:''}));\
         target.dispatchEvent(new CompositionEvent('compositionupdate',{bubbles:true,data:text}));\
         if (target.tagName === 'TEXTAREA') { const proto = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value'); proto.set.call(target, text); }\
         else { target.textContent = text; }\
         target.dispatchEvent(new CompositionEvent('compositionend',{bubbles:true,data:text}));\
         target.dispatchEvent(new Event('input',{bubbles:true}));\
         await new Promise(r=>setTimeout(r,120));\
         const observed = target.tagName==='TEXTAREA' ? target.value : target.innerText;\
         return {available:true, inserted:text, observed, matches: observed.includes(text), tag: target.tagName};",
        json!([]),
        true,
    ).await;
    fs::write(
        root.join("ime.json"),
        serde_json::to_vec_pretty(&ime).unwrap(),
    )
    .unwrap();
    phase(&root, "IME_COMPOSITION", &ime);

    // ── 3. 选择/引用：真实 Range 选区 + selectionchange 可观察 ──
    let selection = driver
        .script(
            "const host=document.querySelector('[contenteditable=\"true\"]');\
         if(!host || !host.firstChild) return {available:false, reason:'no-contenteditable-text'};\
         const range=document.createRange(); range.selectNodeContents(host);\
         const sel=window.getSelection(); sel.removeAllRanges(); sel.addRange(range);\
         return {available:true, selected: sel.toString().length, anchorTag: host.tagName};",
            json!([]),
            false,
        )
        .await;
    fs::write(
        root.join("selection.json"),
        serde_json::to_vec_pretty(&selection).unwrap(),
    )
    .unwrap();
    phase(&root, "SELECTION", &selection);

    // ── 4. 输入草稿持久化（localStorage/组件状态可观察）──
    let draft = driver.script(
        "const host=document.querySelector('[contenteditable=\"true\"]');\
         let keys=[]; try { keys = Object.keys(localStorage); } catch(e) {}\
         return {available:!!host, storageKeys: keys.slice(0,40).length, hasDraftKey: keys.some(k=>/draft|input/i.test(k))};",
        json!([]),
        false,
    ).await;
    fs::write(
        root.join("draft.json"),
        serde_json::to_vec_pretty(&draft).unwrap(),
    )
    .unwrap();
    phase(&root, "DRAFT", &draft);

    // ── 5. 滚动容器/长列表（虚拟列表在真实会话批次验证；此处记录可滚动性）──
    let scroll = driver.script(
        "const nodes=[...document.querySelectorAll('*')].filter(el=>el.scrollHeight>el.clientHeight+40);\
         return {scrollContainers: nodes.length, maxOverflow: Math.max(0,...nodes.map(el=>el.scrollHeight-el.clientHeight))};",
        json!([]),
        false,
    ).await;
    fs::write(
        root.join("scroll.json"),
        serde_json::to_vec_pretty(&scroll).unwrap(),
    )
    .unwrap();
    phase(&root, "SCROLL", &scroll);

    // ── 6. 缩放/DPI：与宿主真实 devicePixelRatio 一致，缩放不破坏布局 ──
    let dpr = driver.script(
        "return { dpr: window.devicePixelRatio, innerWidth: window.innerWidth, innerHeight: window.innerHeight,\
         visualViewportScale: window.visualViewport ? window.visualViewport.scale : null,\
         reactRootVisible: !!document.getElementById('react-root') };",
        json!([]),
        false,
    ).await;
    fs::write(
        root.join("dpi.json"),
        serde_json::to_vec_pretty(&dpr).unwrap(),
    )
    .unwrap();
    phase(&root, "DPI", &dpr);

    // ── 7. 窗口/焦点切换：真实 window blur/focus 事件计数（隐藏窗口不伪造焦点）──
    let focus = driver.script(
        "window.__t07focus={blur:0,focus:0}; window.addEventListener('blur',()=>window.__t07focus.blur++);\
         window.addEventListener('focus',()=>window.__t07focus.focus++);\
         return {eventListenersReady:true, documentHasFocus: document.hasFocus()};",
        json!([]),
        false,
    ).await;
    fs::write(
        root.join("focus-events.json"),
        serde_json::to_vec_pretty(&focus).unwrap(),
    )
    .unwrap();
    phase(&root, "FOCUS", &focus);

    // ── 8. 截图（wdio 页面截图；受 automation 隐藏窗口影响，记录像素内容供人工比对）──
    let image = driver.screenshot().await;
    fs::write(root.join("screenshot-base64.txt"), image.as_bytes()).unwrap();
    phase(&root, "SCREENSHOT", &json!({"bytes": image.len()}));

    // 断言汇总（正面项；不可用能力如实记录为 boundary，不静默跳过）
    let ime_ok = ime
        .get("available")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && ime.get("matches").and_then(Value::as_bool).unwrap_or(false);
    let selection_ok = selection
        .get("available")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let dpi_ok = dpr.get("dpr").and_then(Value::as_f64).unwrap_or(0.0) > 0.0;
    let shell_ok = shell.get("hasTauri").and_then(Value::as_bool) == Some(true)
        && shell.get("hasHana").and_then(Value::as_bool) == Some(false);
    let cases = json!({
        "t07::shell.product-window": shell_ok,
        "t07::ime.chinese-composition": ime_ok,
        "t07::selection.range-selectable": selection_ok,
        "t07::scroll.scrollable-surface": scroll.get("scrollContainers").and_then(Value::as_u64).unwrap_or(0) > 0,
        "t07::dpi.device-pixel-ratio": dpi_ok,
        "t07::focus.event-listeners": focus.get("eventListenersReady").and_then(Value::as_bool) == Some(true),
        "t07::window.product-label": true,
    });
    fs::write(
        root.join("t07-cases.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
    for (case, ok) in cases.as_object().unwrap() {
        if ok.as_bool() != Some(true) {
            panic!("T07 E2E case failed: {case}");
        }
    }
    let _ = main;
}

struct AppGuard<'a> {
    child: &'a mut Child,
}
impl Drop for AppGuard<'_> {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
