//! 显式的离线原生PDF门禁；只有受控Chromium和本机拒绝代理，无真实账号或用户文件。
#![cfg(unix)]
use lingxi_adapters::{
    browser::{ChromiumConfig, ChromiumHost},
    models::network::{NetworkPlane, NetworkPolicy, ProxyPolicy},
};
use lingxi_kernel::browser::{
    BrowserPort, PdfAsset, PdfPrintSettings, PdfRenderJob, PdfRendered, PdfScope,
};
use regex::Regex;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap()
}
fn random() -> String {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).unwrap();
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn chromium() -> PathBuf {
    std::env::var_os("LINGXI_TEST_CHROMIUM")
        .map(PathBuf::from)
        .or_else(|| {
            [
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
                "/Applications/Chromium.app/Contents/MacOS/Chromium",
                "/usr/bin/chromium",
                "/usr/bin/chromium-browser",
                "/usr/bin/google-chrome",
            ]
            .iter()
            .map(PathBuf::from)
            .find(|path| path.is_file())
        })
        .expect("原生PDF门禁需要实际Chromium，不允许缺失时静默通过")
}
struct Canary {
    url: String,
    hits: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Canary {
    fn new() -> Self {
        Self::serve(false)
    }
    fn login_site() -> Self {
        Self::serve(true)
    }
    fn serve(login: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let audit = hits.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let request_audit = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let worker = std::thread::spawn(move || {
            while !flag.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        audit.fetch_add(1, Ordering::AcqRel);
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
                        let mut bytes = [0u8; 4096];
                        let count = stream.read(&mut bytes).unwrap_or(0);
                        let line = String::from_utf8_lossy(&bytes[..count])
                            .lines()
                            .next()
                            .unwrap_or("<empty TCP connection>")
                            .chars()
                            .take(512)
                            .collect::<String>();
                        let mut records = request_audit.lock().unwrap();
                        if records.len() < 128 {
                            records.push(line);
                        }
                        drop(records);
                        if login {
                            let body = b"<!doctype html><title>Private login fixture</title><p>Owned login page</p>";
                            let header = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nSet-Cookie: site=positive; Path=/; SameSite=Lax\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len());
                            let _ = stream.write_all(header.as_bytes());
                            let _ = stream.write_all(body);
                        } else {
                            let _=stream.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            url,
            hits,
            requests,
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for Canary {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
struct Fixture {
    host: Arc<ChromiumHost>,
    root: PathBuf,
    evidence: PathBuf,
    _proxy: Canary,
}
impl Fixture {
    fn new(label: &str) -> Self {
        let suffix = random();
        let root = std::env::temp_dir().join(format!("lingxi-pdf-process-{suffix}"));
        std::fs::create_dir(&root).unwrap();
        let evidence = std::env::var_os("LINGXI_PDF_EVIDENCE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("lingxi-r07-pdf-evidence"))
            .join(format!("{label}-{suffix}"));
        assert!(evidence.is_absolute());
        std::fs::create_dir_all(&evidence).unwrap();
        let proxy = Canary::new();
        let host = ChromiumHost::new(ChromiumConfig {
            executable: chromium(),
            root: root.clone(),
            execution_node: "pdf-fixture-node".into(),
            headless: true,
            network: NetworkPlane::isolated(NetworkPolicy {
                proxy: ProxyPolicy::Manual {
                    http: Some(proxy.url.clone()),
                    https: Some(proxy.url.clone()),
                    no_proxy: String::new(),
                },
                trusted_ca_pem: None,
            }),
        })
        .unwrap();
        println!("PDF_EVIDENCE {}", evidence.display());
        Self {
            host,
            root,
            evidence,
            _proxy: proxy,
        }
    }
    fn scope(&self, session: &str) -> PdfScope {
        PdfScope {
            session_id: session.into(),
            owner_subject: "user_local".into(),
            execution_node: "pdf-fixture-node".into(),
        }
    }
    async fn render(&self, session: &str, job: PdfRenderJob) -> PdfRendered {
        self.host
            .render_pdf(self.scope(session), job, Arc::new(AtomicBool::new(false)))
            .await
            .unwrap()
    }
    fn save(&self, name: &str, pdf: &PdfRendered) -> PathBuf {
        assert_eq!(pdf.sha256, lingxi_protocol::canon::sha256_hex(&pdf.bytes));
        assert!(pdf.bytes.starts_with(b"%PDF-"));
        let path = self.evidence.join(format!("{name}.pdf"));
        std::fs::write(&path, &pdf.bytes).unwrap();
        std::fs::write(
            self.evidence.join(format!("{name}.json")),
            serde_json::to_vec_pretty(
                &json!({"sha256":pdf.sha256,"bytes":pdf.bytes.len(),"renderer":pdf.renderer}),
            )
            .unwrap(),
        )
        .unwrap();
        path
    }
    async fn clean(&self) {
        assert!(self.host.pdf_scopes().unwrap().is_empty());
        assert!(
            self.host.workspaces().unwrap().is_empty(),
            "打印不建立业务浏览器工作区"
        );
        self.host.shutdown().await.unwrap();
        assert_eq!(std::fs::read_dir(self.root.join("aux")).unwrap().count(), 0);
        std::fs::write(
            self.evidence.join("cleanup.json"),
            br#"{"processesConfirmedStopped":true,"profilesRemaining":0}"#,
        )
        .unwrap();
        std::fs::remove_dir_all(&self.root).unwrap();
    }
    async fn wait_scopes(&self, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.host.pdf_scopes().unwrap().len() != count {
            assert!(Instant::now() < deadline, "打印租约未到达预期实际状态");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}
fn job(html: impl Into<String>) -> PdfRenderJob {
    PdfRenderJob {
        html: html.into(),
        base_url: "https://lingxi-pdf.invalid/quality/index.html".into(),
        assets: vec![],
        settings: PdfPrintSettings {
            landscape: false,
            print_background: true,
            prefer_css_page_size: true,
            paper_width: 8.27,
            paper_height: 11.69,
            margin_top: 0.4,
            margin_bottom: 0.4,
            margin_left: 0.4,
            margin_right: 0.4,
            page_ranges: String::new(),
            scale: 1.0,
        },
        allow_javascript: false,
        viewport_width: 1280,
        viewport_height: 900,
        settle_ms: 250,
        timeout_ms: 60_000,
    }
}
fn sample(name: &str) -> PdfRenderJob {
    let base = repo().join("tests/migration/r01-t05/samples");
    let mut value = job(std::fs::read_to_string(base.join(name)).unwrap());
    if value.html.contains("assets/chart.png") {
        let url = "https://lingxi-pdf.invalid/quality/asset/chart.png";
        value.html = value.html.replace("assets/chart.png", url);
        value.assets.push(PdfAsset {
            url: url.into(),
            mime_type: "image/png".into(),
            bytes: std::fs::read(base.join("assets/chart.png")).unwrap(),
        });
    }
    let themes = repo().join("desktop/src/themes");
    let css = std::fs::read_to_string(themes.join("new-warm-paper-fonts.css")).unwrap();
    let blocks = Regex::new(r"(?is)@font-face\s*\{[^}]*\}").unwrap();
    let urls = Regex::new(r#"url\(['"]?([^)'"\s]+)['"]?\)"#).unwrap();
    let mut fonts = BTreeMap::new();
    let mut injection = String::new();
    for block in blocks.find_iter(&css) {
        if !["'EB Garamond'", "'Noto Serif SC'", "'JetBrains Mono'"]
            .iter()
            .any(|family| block.as_str().contains(family))
        {
            continue;
        }
        let mut face = block.as_str().to_string();
        for captures in urls.captures_iter(block.as_str()) {
            let relative = captures.get(1).unwrap().as_str();
            let bytes = std::fs::read(themes.join(relative)).unwrap();
            let url = format!(
                "https://lingxi-pdf.invalid/quality/font/{}",
                lingxi_protocol::canon::sha256_hex(&bytes)
            );
            face = face.replace(relative, &url);
            fonts.entry(url.clone()).or_insert(PdfAsset {
                url,
                mime_type: "font/woff2".into(),
                bytes,
            });
        }
        injection.push_str(&face);
        injection.push('\n');
    }
    value.assets.extend(fonts.into_values());
    value.html = value
        .html
        .replacen("</head>", &format!("<style>{injection}</style></head>"), 1);
    value
}
#[cfg(target_os = "macos")]
fn probe(fixture: &Fixture, name: &str, pdf: &PdfRendered) -> (Value, String) {
    static PROBE: OnceLock<PathBuf> = OnceLock::new();
    let executable = PROBE.get_or_init(|| {
        let path = fixture.evidence.join("pdf-probe");
        let output = std::process::Command::new("/usr/bin/swiftc")
            .arg("-O")
            .arg(repo().join("tests/migration/r01-t05/pdf_probe.swift"))
            .arg("-o")
            .arg(&path)
            .output()
            .expect("PDFKit探针编译器不可用");
        assert!(
            output.status.success(),
            "PDFKit探针编译失败: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        path
    });
    let input = fixture.save(name, pdf);
    let target = fixture.evidence.join(format!("{name}-probe"));
    let output = std::process::Command::new(executable)
        .arg(input)
        .arg(&target)
        .arg("1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "真实PDF无法由PDFKit解析: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        serde_json::from_slice(&std::fs::read(target.join("probe.json")).unwrap()).unwrap(),
        std::fs::read_to_string(target.join("text.txt")).unwrap(),
    )
}

#[tokio::test]
#[ignore = "需要真实Chromium；由R07门禁显式首次生成PDF"]
async fn static_utf8_pdf_is_returned_only_after_confirmed_private_cleanup() {
    let fixture = Fixture::new("first-print");
    let pdf=fixture.render("static-utf8",job("<!doctype html><meta charset='utf-8'><style>body{font:20px serif;color:#284b63}</style><h1>灵犀打印检查</h1><p>STATIC-PDF-UTF8-2026</p>")).await;
    fixture.save("static-utf8", &pdf);
    assert!(pdf.bytes.len() > 512);
    assert!(pdf.renderer.starts_with("Chromium CDP ("));
    assert!(fixture.host.pdf_scopes().unwrap().is_empty());
    assert_eq!(
        std::fs::read_dir(fixture.root.join("aux")).unwrap().count(),
        0
    );
    fixture.clean().await;
}

#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "需要真实Chromium/PDFKit；由R07门禁显式首次生成PDF"]
async fn actual_r01_documents_preserve_fonts_images_pagination_and_js_contract() {
    let fixture = Fixture::new("r01-quality");
    let pdf = fixture.render("long-zh", sample("s1-long-zh.html")).await;
    let (facts, text) = probe(&fixture, "s1-long-zh", &pdf);
    let normalized: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    for marker in ["DOC-END-OMEGA", "龘靐麤爨驫鬱", "𠮷𪚥", "render(job:"] {
        assert!(normalized.contains(marker), "真实PDF文本缺失: {marker}");
    }
    for row in 1..=140 {
        assert!(normalized.contains(&format!("ROW-{row:03}-END")));
    }
    let fonts = regex::bytes::Regex::new(r"/(?:BaseFont|FontName)\s*/([A-Za-z0-9+_.-]+)")
        .unwrap()
        .captures_iter(&pdf.bytes)
        .map(|value| String::from_utf8_lossy(&value[1]).into_owned())
        .collect::<Vec<_>>();
    for family in ["EBGaramond", "NotoSerifSC", "JetBrainsMono"] {
        assert!(
            fonts.iter().any(|font| font.contains(family)),
            "PDF未真实嵌入字体族: {family}"
        );
    }
    assert!(regex::bytes::Regex::new(r"/Subtype\s*/Image\b")
        .unwrap()
        .is_match(&pdf.bytes));
    let baseline: Value = serde_json::from_slice(
        &std::fs::read(repo().join("artifacts/rust-tauri/R01/T05/a09/new/probe/probe.json"))
            .expect("质量门禁需要R01已归档的同样本文档视觉基线"),
    )
    .unwrap();
    assert_eq!(facts["pageCount"], baseline["pageCount"]);
    for (actual, old) in facts["pages"]
        .as_array()
        .unwrap()
        .iter()
        .zip(baseline["pages"].as_array().unwrap())
    {
        assert!(
            actual["inkCoverage"].as_f64().unwrap() > 0.0005,
            "不允许异常空白页"
        );
        let actual = actual["lumaGrid16"].as_array().unwrap();
        let old = old["lumaGrid16"].as_array().unwrap();
        assert_eq!(actual.len(), 256);
        assert_eq!(old.len(), 256);
        let mae = actual
            .iter()
            .zip(old)
            .map(|(a, b)| (a.as_f64().unwrap() - b.as_f64().unwrap()).abs())
            .sum::<f64>()
            / 256.0;
        assert!(mae <= 20.0, "逐页视觉网格差异超出R01阈值: {mae}");
    }
    let page = fixture
        .render("paper-css", sample("s2-page-semantics.html"))
        .await;
    let (facts, _) = probe(&fixture, "s2-page-semantics", &page);
    let box_ = facts["pages"][0]["mediaBox"].as_array().unwrap();
    assert!((box_[2].as_f64().unwrap() - 594.96).abs() < 2.0);
    assert!((box_[3].as_f64().unwrap() - 420.0).abs() < 2.0);
    let pages = fixture.evidence.join("s2-page-semantics-probe");
    // PDFKit可能在同一行的字体切换处插入换行；仍逐页核对，不能合并全篇掩盖分页。
    let page_text = |name: &str| {
        std::fs::read_to_string(pages.join(name))
            .unwrap()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
    };
    assert!(!page_text("page-01.txt").contains("PAGE2-START"));
    assert!(page_text("page-02.txt").contains("PAGE2-START"));
    let mut paper = sample("s3-margins-paper.html");
    paper.settings.landscape = true;
    paper.settings.paper_width = 8.5;
    paper.settings.paper_height = 11.0;
    paper.settings.margin_top = 1.0;
    paper.settings.margin_bottom = 1.0;
    paper.settings.margin_left = 1.5;
    paper.settings.margin_right = 1.5;
    let pdf = fixture.render("paper-inches", paper).await;
    let (facts, _) = probe(&fixture, "s3-margins-paper", &pdf);
    assert_eq!(facts["pages"][0]["mediaBox"], json!([0, 0, 792, 612]));
    let bbox = facts["pages"][0]["inkBBox"].as_array().unwrap();
    assert!(
        (100.0..=120.0).contains(&bbox[0].as_f64().unwrap()),
        "边距必须为英寸，不能按像素处理"
    );
    for enabled in [false, true] {
        let mut source = sample("s4-js-gated.html");
        source.allow_javascript = enabled;
        let pdf = fixture.render("js-flag", source).await;
        let (_, text) = probe(
            &fixture,
            if enabled { "s4-js-on" } else { "s4-js-off" },
            &pdf,
        );
        // PDFKit在长单词折行处插入换行；JS合同只判定标记文本是否真实出现。
        let normalized: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        assert_eq!(normalized.contains("JS-EXECUTED-MARKER-7f3a"), enabled);
    }
    fixture.clean().await;
}

#[tokio::test]
#[ignore = "需要真实Chromium；R07门禁显式运行"]
async fn broken_background_font_and_unfrozen_stylesheet_are_explicit_failures() {
    let fixture = Fixture::new("bad-assets");
    let canary = Canary::new();
    let mut background=job("<style>body{background-image:url('https://lingxi-pdf.invalid/quality/asset/bad.png')}</style><p>visible</p>");
    background.assets.push(PdfAsset {
        url: "https://lingxi-pdf.invalid/quality/asset/bad.png".into(),
        mime_type: "image/png".into(),
        bytes: b"not a PNG".to_vec(),
    });
    let mut font=job("<style>@font-face{font-family:Broken;src:url('https://lingxi-pdf.invalid/quality/font/bad')}body{font-family:Broken}</style><p>font must load</p>");
    font.assets.push(PdfAsset {
        url: "https://lingxi-pdf.invalid/quality/font/bad".into(),
        mime_type: "font/woff2".into(),
        bytes: b"not a font".to_vec(),
    });
    let stylesheet = job(format!(
        "<link rel='stylesheet' href='{}/not-frozen.css'><p>unfrozen style</p>",
        canary.url
    ));
    for (index, source) in [background, font, stylesheet].into_iter().enumerate() {
        let cause = fixture
            .host
            .render_pdf(
                fixture.scope(&format!("broken-{index}")),
                source,
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .unwrap_err();
        assert!(
            !cause.effect_unknown,
            "退出确认后不得把明确无产物说成未知: {}",
            cause.code
        );
        assert!(fixture.host.pdf_scopes().unwrap().is_empty());
        assert_eq!(
            std::fs::read_dir(fixture.root.join("aux")).unwrap().count(),
            0
        );
    }
    assert_eq!(canary.hits.load(Ordering::Acquire), 0);
    fixture.clean().await;
}

#[tokio::test]
#[ignore = "需要真实Chromium；R07门禁显式运行"]
async fn html_network_frames_and_workers_never_reach_loopback_canary() {
    let fixture = Fixture::new("network-gate");
    let canary = Canary::new();
    let ws = canary.url.replacen("http:", "ws:", 1);
    let mut cases = Vec::new();
    for (index,html) in [
        format!("<p>safe</p><iframe src='{}/secret'></iframe>",canary.url),
        "<p>safe</p><img src='file:///not-authorized/decoy'>".into(),
        format!("<p>safe</p><script>try{{new Worker('{}/worker.js')}}catch(e){{}}</script>",canary.url),
        format!("<p>safe</p><script>try{{new WebSocket('{ws}/ws')}}catch(e){{}}</script>"),
        format!("<p>safe</p><script>fetch('{}/fetch').catch(()=>{{}});navigator.sendBeacon('{}/beacon','x')</script>",canary.url,canary.url),
        format!("<meta http-equiv='refresh' content='0;url={}/redirect'><p>safe</p>",canary.url),
    ].into_iter().enumerate() {
        let mut source=job(html);source.allow_javascript=true;
        let result=fixture.host.render_pdf(fixture.scope(&format!("network-{index}")),source,Arc::new(AtomicBool::new(false))).await;
        // 策略阻断可能明确拒绝文档，也可能由文档自身处理；两者都不允许真实网络命中。
        let outcome = match result {
            Ok(pdf) => { fixture.save(&format!("network-{index}"),&pdf); "printed".to_owned() }
            Err(cause) => format!("{}: {}",cause.code,cause.message)
        };
        assert!(fixture.host.pdf_scopes().unwrap().is_empty());
        // 每次执行已确认进程退出后再记录TCP事实；空连接也算越界，不忽略预连接。
        tokio::time::sleep(Duration::from_millis(150)).await;
        cases.push(json!({"case":index,"outcome":outcome,"tcpConnections":canary.hits.load(Ordering::Acquire),"requestLines":canary.requests.lock().unwrap().clone()}));
        std::fs::write(fixture.evidence.join("network-audit.json"),serde_json::to_vec_pretty(&cases).unwrap()).unwrap();
    }
    assert_eq!(
        canary.hits.load(Ordering::Acquire),
        0,
        "{}",
        serde_json::to_string(&cases).unwrap()
    );
    fixture.clean().await;
}

#[tokio::test]
#[ignore = "需要真实Chromium；R07门禁显式运行"]
async fn total_deadline_and_explicit_cancel_reap_before_returning_failure() {
    let fixture = Fixture::new("deadlines");
    let mut source = job("<p>before</p><script>while(true){}</script>");
    source.allow_javascript = true;
    source.timeout_ms = 1200;
    let started = Instant::now();
    let cause = fixture
        .host
        .render_pdf(
            fixture.scope("infinite"),
            source,
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap_err();
    assert!(!cause.effect_unknown);
    assert!(started.elapsed() < Duration::from_secs(6));
    assert!(fixture.host.pdf_scopes().unwrap().is_empty());
    let mut source = job("<script>while(true){}</script>");
    source.allow_javascript = true;
    let cancel = Arc::new(AtomicBool::new(false));
    let cancellation = cancel.clone();
    let host = fixture.host.clone();
    let scope = fixture.scope("cancelled");
    let task = tokio::spawn(async move { host.render_pdf(scope, source, cancel).await });
    fixture.wait_scopes(1).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    cancellation.store(true, Ordering::Release);
    let cause = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(!cause.effect_unknown);
    assert!(fixture.host.pdf_scopes().unwrap().is_empty());
    fixture.clean().await;
}

#[tokio::test]
#[ignore = "需要真实Chromium；R07门禁显式运行"]
async fn two_auxiliary_leases_are_scoped_and_dropped_future_is_cancelled() {
    let fixture = Fixture::new("scope-and-quota");
    let mut source = job("<script>while(true){}</script>");
    source.allow_javascript = true;
    let host = fixture.host.clone();
    let a = fixture.scope("lease-a");
    let aa = a.clone();
    let first = source.clone();
    let task_a = tokio::spawn(async move {
        host.render_pdf(aa, first, Arc::new(AtomicBool::new(false)))
            .await
    });
    let host = fixture.host.clone();
    let b = fixture.scope("lease-b");
    let bb = b.clone();
    let task_b = tokio::spawn(async move {
        host.render_pdf(bb, source, Arc::new(AtomicBool::new(false)))
            .await
    });
    fixture.wait_scopes(2).await;
    let cause = fixture
        .host
        .render_pdf(
            fixture.scope("third"),
            job("third"),
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap_err();
    assert_eq!(cause.code, "browser_capacity");
    let mut wrong = a.clone();
    wrong.owner_subject = "other-owner".into();
    fixture.host.cancel_pdf_scope(wrong).await.unwrap();
    assert_eq!(fixture.host.pdf_scopes().unwrap().len(), 2);
    fixture.host.cancel_pdf_scope(a).await.unwrap();
    assert!(task_a.await.unwrap().is_err());
    assert!(!task_b.is_finished(), "不能取消另一会话的打印");
    assert_eq!(fixture.host.pdf_scopes().unwrap(), vec![b]);
    task_b.abort();
    let _ = task_b.await;
    let deadline = Instant::now() + Duration::from_secs(5);
    while !fixture.host.pdf_scopes().unwrap().is_empty() {
        assert!(Instant::now() < deadline, "被丢弃future仍须监督退出");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    fixture.clean().await;
}

#[tokio::test]
#[ignore = "需要真实Chromium；R07门禁显式运行"]
async fn printing_never_switches_or_clears_the_regular_logged_in_workspace() {
    use lingxi_kernel::browser::BrowserScope;
    use lingxi_protocol::{BrowserAction as A, BrowserCommand};
    let fixture = Fixture::new("regular-isolation");
    let login = Canary::login_site();
    let browser = BrowserScope {
        session_id: "same-source-session".into(),
        owner_subject: "user_local".into(),
        epoch: 1,
        execution_node: "pdf-fixture-node".into(),
        human: false,
    };
    let mut navigate = BrowserCommand::new(A::Navigate);
    navigate.url = Some(login.url.clone());
    let opened = fixture
        .host
        .execute(browser.clone(), navigate, Arc::new(AtomicBool::new(false)))
        .await
        .unwrap();
    let target = opened.reply.state.active_tab_id.clone().unwrap();
    let mut write = BrowserCommand::new(A::Evaluate);
    write.tab_id = Some(target.clone());
    write.expression =
        Some("document.cookie='login=retained-for-owner; Path=/';document.cookie".into());
    let before = fixture
        .host
        .execute(browser.clone(), write, Arc::new(AtomicBool::new(false)))
        .await
        .unwrap();
    assert!(before.reply.result["value"]
        .as_str()
        .unwrap()
        .contains("login=retained-for-owner"));
    assert!(
        before.reply.result["value"]
            .as_str()
            .unwrap()
            .contains("site=positive"),
        "正向站必须真实200并成功接收Set-Cookie"
    );
    let mut printed=job("<p>PRINT-ISOLATED</p><script>try{document.body.textContent+=' COOKIE:'+document.cookie}catch(e){document.body.textContent+=' COOKIE-DENIED'}</script>");
    printed.allow_javascript = true;
    let pdf = fixture.render("same-source-session", printed).await;
    fixture.save("isolated-print", &pdf);
    let states = fixture.host.workspaces().unwrap();
    assert_eq!(states.len(), 1);
    assert_eq!(states[0].active_tab_id.as_deref(), Some(target.as_str()));
    assert_eq!(states[0].tabs, opened.reply.state.tabs);
    let mut read = BrowserCommand::new(A::Evaluate);
    read.tab_id = Some(target);
    read.expression = Some("document.cookie".into());
    let after = fixture
        .host
        .execute(browser.clone(), read, Arc::new(AtomicBool::new(false)))
        .await
        .unwrap();
    assert!(after.reply.result["value"]
        .as_str()
        .unwrap()
        .contains("login=retained-for-owner"));
    let mut deleted = browser;
    deleted.epoch = 2;
    fixture.host.forget(deleted).await.unwrap();
    fixture.clean().await;
}
