//! R06-T05 Round 5 生产链测试：HTML 预览面（叶 #26-28）。
//!
//! 对照纪律（RC-3，逐条亲读 server/routes/html-preview.ts 385 行）：
//! - 建立：POST（:24-79）——content 必填字符串（:34-35）、2MB 上限 413
//!   （:36-38）、pv_ id（:18 = 16 字节 hex）、32 字节 base64url token
//!   （:19）、10min TTL（:7）、title 截 240 字符（:65）、assetScope 解析
//!   （:42 + resolvePreviewAssetScope :182-200）、素材引用改写
//!   （rewriteLocalAssetReferences :252-268）+ <base> 注入
//!   （injectAssetBase :221-227）、CSP（buildHtmlPreviewCsp :139-167）。
//! - 读取：servePreview（:89-107）——无效 id/token/过期一律 404 空体；
//!   响应头 Content-Type/CSP/Referrer-Policy/nosniff/no-store/CORP
//!   cross-origin；HEAD 无正文（:82,106）。
//! - 素材：servePreviewAsset（:109-131）——token 在路径段（:113）、
//!   extractAssetPath 前缀剥离 + decodeURIComponent（:350-358）、
//!   resolveAssetPath 拒绝 空段/./../绝对/反斜杠/NUL/symlink 逃逸
//!   （:366-384）、50MB 上限（:9,124）、guessMime（file-content.ts
//!   MIME_BY_EXT）、CORP same-origin（:129）。
//! - 设计登记：D5（/api/preview/html → /lingxi/v1/preview/html）。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use lingxi_service::preview::{PreviewService, DEFAULT_TTL_MS};
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

// ── 服务级（不起 HTTP） ──

struct PvFixture {
    dir: PathBuf,
    now: Arc<std::sync::Mutex<u64>>,
    service: PreviewService,
}

impl Drop for PvFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn pv_fixture(tag: &str, now_ms: u64) -> PvFixture {
    let uniq = format!(
        "{}-{}-{tag}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let dir = std::env::temp_dir().join(format!("lingxi-r06t05-pv-{uniq}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    // macOS /var → /private/var：assetRoot 经 realpath，夹具先规范化。
    let dir = dir.canonicalize().expect("canonical");
    let now = Arc::new(std::sync::Mutex::new(now_ms));
    let clock = {
        let now = now.clone();
        move || *now.lock().expect("clock")
    };
    PvFixture {
        dir,
        now,
        service: PreviewService::new(clock),
    }
}

#[test]
fn create_preview_identity_ttl_and_errors() {
    let fx = pv_fixture("create", 1_000_000);
    // content 必须是非缺省字符串（html-preview.ts :34-35）。
    let err = fx
        .service
        .create(&serde_json::json!({}), "http://x")
        .unwrap_err();
    assert_eq!(err.code(), "missing_content");
    let err = fx
        .service
        .create(&serde_json::json!({"content": 42}), "http://x")
        .unwrap_err();
    assert_eq!(err.code(), "missing_content");
    // 2MB 上限 → 413 html_preview_too_large（:36-38）。
    let big = "x".repeat(2 * 1024 * 1024 + 1);
    let err = fx
        .service
        .create(&serde_json::json!({"content": big}), "http://x")
        .unwrap_err();
    assert_eq!(err.code(), "html_preview_too_large");
    // 成功：pv_ id（32 hex 尾）、10min TTL、title 截 240。
    let long_title = "t".repeat(300);
    let created = fx
        .service
        .create(
            &serde_json::json!({"content": "<html>hi</html>", "title": long_title}),
            "http://x",
        )
        .expect("create");
    assert!(
        created.id.starts_with("pv_") && created.id.len() == 3 + 32,
        "pv_ + 16 bytes hex, got {}",
        created.id
    );
    assert_eq!(created.expires_at, 1_000_000 + DEFAULT_TTL_MS);
    // TTL 常数金样（html-preview.ts :14 十分钟）。
    const { assert!(DEFAULT_TTL_MS == 10 * 60 * 1000) };
    // token 不出现在响应里，但内嵌于 url_path 查询串（:71-72）。
    assert!(
        created.url_path.starts_with(&format!(
            "/lingxi/v1/preview/html/{}?previewToken=",
            created.id
        )),
        "candidate D5 path, got {}",
        created.url_path
    );
}

#[test]
fn serve_preview_token_gate_expiry_and_csp() {
    let fx = pv_fixture("serve", 5_000);
    let created = fx
        .service
        .create(&serde_json::json!({"content": "<p>body</p>"}), "http://h")
        .expect("create");
    let token = created
        .url_path
        .split("previewToken=")
        .nth(1)
        .expect("token in url")
        .to_string();
    // 错 token / 未知 id → None（:95-97）。
    assert!(fx.service.serve(&created.id, "wrong").is_none());
    assert!(fx.service.serve("pv_nope", &token).is_none());
    let served = fx.service.serve(&created.id, &token).expect("serve");
    assert_eq!(served.content, "<p>body</p>");
    // 无素材根时 CSP 基线（:141-166）：default-src 'none' + base-uri 'self'。
    assert!(served.csp.contains("default-src 'none'"));
    assert!(served.csp.contains("base-uri 'self'"));
    assert!(served.csp.contains("object-src 'none'"));
    // 过期 → None（cleanupExpired :133-137，expiresAt <= now 过期）。
    *fx.now.lock().expect("clock") = 5_000 + DEFAULT_TTL_MS;
    assert!(fx.service.serve(&created.id, &token).is_none());
}

#[test]
fn asset_scope_rewrite_base_inject_and_serve_gates() {
    let fx = pv_fixture("assets", 9_000);
    // 素材树：root/sub/page.html 引用 root/sub/pic.png（绝对路径）与
    // root/other.css（绝对路径），外加一个 root 之外的 outsider.txt。
    let root = fx.dir.join("site");
    let sub = root.join("sub");
    std::fs::create_dir_all(&sub).expect("mkdir sub");
    std::fs::write(root.join("other.css"), "body{}").expect("css");
    std::fs::write(sub.join("pic.png"), b"\x89PNG").expect("png");
    std::fs::write(fx.dir.join("outsider.txt"), "out").expect("outsider");
    let page = sub.join("page.html");
    let html = format!(
        "<html><head><title>t</title></head><body>\
         <img src=\"{}\"><link href=\"{}\">\
         <img src=\"{}\"></body></html>",
        sub.join("pic.png").display(),
        root.join("other.css").display(),
        fx.dir.join("outsider.txt").display(),
    );
    std::fs::write(&page, &html).expect("page");
    let created = fx
        .service
        .create(
            &serde_json::json!({
                "content": html,
                "sourceFilePath": page.to_str().unwrap(),
                "sourceRootPath": root.to_str().unwrap(),
            }),
            "http://h",
        )
        .expect("create with assets");
    let token = created
        .url_path
        .split("previewToken=")
        .nth(1)
        .expect("token")
        .to_string();
    let served = fx.service.serve(&created.id, &token).expect("serve");
    // <base> 注入到 <head> 之后（injectAssetBase :222-226）。
    let base_idx = served.content.find("<base href=\"").expect("base tag");
    let head_idx = served.content.find("<head>").expect("head");
    assert!(base_idx > head_idx, "base after head");
    // base 指向 sourceRelativeDir（sub/）。
    assert!(served.content.contains(&format!(
        "/lingxi/v1/preview/html/{}/assets/{}/sub/",
        created.id, token
    )));
    // 根内绝对路径引用被改写为素材 URL（rewriteLocalAssetUrl :270-276）。
    assert!(served
        .content
        .contains(&format!("/assets/{}/sub/pic.png", token)));
    assert!(served
        .content
        .contains(&format!("/assets/{}/other.css", token)));
    // 根外引用不改写（resolveAssetFileForLocalPath :339 拒绝）。
    assert!(served
        .content
        .contains(fx.dir.join("outsider.txt").to_str().unwrap()));
    // 带素材根时 CSP 的 base-uri/img-src 含素材基址（:148-163）。
    assert!(served.csp.contains("http://h/lingxi/v1/preview/html/"));
    assert!(served.csp.contains("frame-ancestors 'self'"));

    // 素材读取：成功。
    let asset = fx
        .service
        .serve_asset(&created.id, &token, "sub/pic.png")
        .expect("asset");
    assert_eq!(asset.mime, "image/png");
    assert_eq!(asset.size, 4);
    // 错 token / 无素材根的预览 / 未知 id → None（:115-117）。
    assert!(fx
        .service
        .serve_asset(&created.id, "bad", "sub/pic.png")
        .is_none());
    assert!(fx
        .service
        .serve_asset("pv_x", &token, "sub/pic.png")
        .is_none());
    // 路径闸门（resolveAssetPath :366-384）：..、空段、点段、绝对、反斜杠、NUL。
    for bad in [
        "../outsider.txt",
        "sub/../../outsider.txt",
        "sub//pic.png",
        "./sub/pic.png",
        "/etc/passwd",
        "sub\\pic.png",
        "sub/pic.png\0x",
        "",
    ] {
        assert!(
            fx.service.serve_asset(&created.id, &token, bad).is_none(),
            "reject {bad:?}"
        );
    }
    // 不存在的素材 → None（:375-383 realpath 失败）。
    assert!(fx
        .service
        .serve_asset(&created.id, &token, "sub/nope.png")
        .is_none());
    // symlink 逃逸拒绝（:337,377）。
    let link = sub.join("link.png");
    std::os::unix::fs::symlink(fx.dir.join("outsider.txt"), &link).expect("symlink");
    assert!(fx
        .service
        .serve_asset(&created.id, &token, "sub/link.png")
        .is_none());
}

#[test]
fn asset_over_50mb_refused() {
    let fx = pv_fixture("bigasset", 3_000);
    let root = fx.dir.join("big");
    std::fs::create_dir_all(&root).expect("mkdir");
    let page = root.join("p.html");
    std::fs::write(&page, "<html><head></head></html>").expect("page");
    let big = root.join("huge.bin");
    // 稀疏文件：只设长度不占盘。
    let f = std::fs::File::create(&big).expect("create");
    f.set_len(50 * 1024 * 1024 + 1).expect("set_len");
    drop(f);
    let created = fx
        .service
        .create(
            &serde_json::json!({
                "content": "x",
                "sourceFilePath": page.to_str().unwrap(),
            }),
            "http://h",
        )
        .expect("create");
    let token = created
        .url_path
        .split("previewToken=")
        .nth(1)
        .expect("token")
        .to_string();
    assert!(fx
        .service
        .serve_asset(&created.id, &token, "huge.bin")
        .is_none());
    // 恰好 50MB 放行（:124 是 > 比较）。
    let f = std::fs::File::options()
        .write(true)
        .open(&big)
        .expect("open");
    f.set_len(50 * 1024 * 1024).expect("shrink");
    drop(f);
    assert!(fx
        .service
        .serve_asset(&created.id, &token, "huge.bin")
        .is_some());
}

// ── 路由级（真 TCP + 真 HTTP） ──

struct TestServer {
    addr: SocketAddr,
    home: PathBuf,
    workspace: PathBuf,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
}

async fn start_server(tag: &str) -> TestServer {
    let uniq = format!(
        "{}-{}-{tag}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let home = std::env::temp_dir().join(format!("lingxi-r06t05-pv-http-{uniq}"));
    let workspace = std::env::temp_dir().join(format!("lingxi-r06t05-pv-http-ws-{uniq}"));
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&home).expect("mkdir home");
    std::fs::create_dir_all(&workspace).expect("mkdir workspace");
    let home = home.canonicalize().expect("canonical home");
    let workspace = workspace.canonicalize().expect("canonical workspace");
    let layout = prepare_layout(&home).expect("prepare layout");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static loopback addr"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let deps = ServiceDeps {
        workspace_root: Some(workspace.clone()),
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
        .await
        .expect("bootstrap");
    let token = state.auth().local_token();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let handle = tokio::spawn(async move {
        run(
            state,
            async {
                let _ = stop_rx.await;
            },
            |addr| {
                let _ = ready_tx.send(addr);
            },
            None,
        )
        .await
    });
    let addr = match ready_rx.await {
        Ok(addr) => addr,
        Err(err) => panic!("readiness: {err}; service result: {:?}", handle.await),
    };
    TestServer {
        addr,
        home,
        workspace,
        token,
        stop: stop_tx,
        handle,
    }
}

impl TestServer {
    fn bearer(&self) -> String {
        format!("Bearer {}", self.token)
    }

    async fn stop_and_clean(self) {
        self.stop.send(()).expect("server still listening");
        tokio::time::timeout(Duration::from_secs(10), self.handle)
            .await
            .expect("server shuts down within timeout")
            .expect("server task join")
            .expect("clean serve result");
        let _ = std::fs::remove_dir_all(&self.home);
        let _ = std::fs::remove_dir_all(&self.workspace);
    }
}

struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpResponse {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

async fn http(
    addr: &SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> HttpResponse {
    let mut stream = TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("connect {addr}: {e}"));
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if let Some(body) = body {
        head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await.expect("write head");
    if let Some(body) = body {
        stream.write_all(body.as_bytes()).await.expect("write body");
    }
    let mut raw = Vec::new();
    loop {
        let mut chunk = [0u8; 4096];
        match stream.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&chunk[..n]),
            Err(err)
                if err.kind() == std::io::ErrorKind::ConnectionReset
                    || err.kind() == std::io::ErrorKind::BrokenPipe =>
            {
                break;
            }
            Err(err) => panic!("read response: {err}"),
        }
    }
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| (p, p + 4));
    let (head_bytes, body_bytes) = match split {
        Some((h, b)) => (&raw[..h], &raw[b..]),
        None => (&raw[..], &[][..]),
    };
    let head = String::from_utf8_lossy(head_bytes).into_owned();
    let status: u16 = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let headers = head
        .lines()
        .skip(1)
        .filter_map(|line| {
            line.split_once(':')
                .map(|(n, v)| (n.trim().to_string(), v.trim().to_string()))
        })
        .collect();
    HttpResponse {
        status,
        headers,
        body: body_bytes.to_vec(),
    }
}

#[tokio::test]
async fn preview_route_faces_and_public_token_gate() {
    let server = start_server("faces").await;
    let page = server.workspace.join("route.html");
    std::fs::write(&page, "<html><head></head><body>route</body></html>").expect("page");

    // 建立面：无凭证 → 401（Scope("chat")）。
    let resp = http(
        &server.addr,
        "POST",
        "/lingxi/v1/preview/html",
        &[],
        Some(r#"{"content":"x"}"#),
    )
    .await;
    assert_eq!(resp.status, 401, "chat scope required");
    // 坏 JSON → 400。
    let resp = http(
        &server.addr,
        "POST",
        "/lingxi/v1/preview/html",
        &[("Authorization", &server.bearer())],
        Some("{not json"),
    )
    .await;
    assert_eq!(resp.status, 400, "invalid json");
    // 成功建立：{id, previewUrl, expiresAt}。
    let resp = http(
        &server.addr,
        "POST",
        "/lingxi/v1/preview/html",
        &[("Authorization", &server.bearer())],
        Some(
            &serde_json::json!({
                "content": "<html><head></head><body>route</body></html>",
                "sourceFilePath": page.to_str().unwrap(),
                "sourceRootPath": server.workspace.to_str().unwrap(),
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(
        resp.status,
        200,
        "create: {:?}",
        resp.header("content-type")
    );
    let created: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&resp.body)).expect("json");
    let id = created["id"].as_str().expect("id").to_string();
    let preview_url = created["previewUrl"].as_str().expect("previewUrl");
    assert!(created["expiresAt"].as_u64().expect("expiresAt") > 0);
    let token = preview_url
        .split("previewToken=")
        .nth(1)
        .expect("token")
        .to_string();

    // 读取面 Public：无 Authorization 也能凭 token 读（token 即凭证）。
    let path = format!("/lingxi/v1/preview/html/{id}?previewToken={token}");
    let resp = http(&server.addr, "GET", &path, &[], None).await;
    assert_eq!(resp.status, 200, "public token read");
    assert_eq!(
        resp.header("content-type"),
        Some("text/html; charset=utf-8")
    );
    assert!(resp
        .header("content-security-policy")
        .expect("csp")
        .contains("default-src 'none'"));
    assert_eq!(resp.header("referrer-policy"), Some("no-referrer"));
    assert_eq!(resp.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(resp.header("cache-control"), Some("no-store"));
    assert_eq!(
        resp.header("cross-origin-resource-policy"),
        Some("cross-origin")
    );
    assert!(String::from_utf8_lossy(&resp.body).contains("route"));
    // HEAD：同头无正文。
    let resp = http(&server.addr, "HEAD", &path, &[], None).await;
    assert_eq!(resp.status, 200);
    assert!(resp.body.is_empty(), "HEAD carries no body");
    assert_eq!(
        resp.header("content-type"),
        Some("text/html; charset=utf-8")
    );
    // 错 token / 未知 id → 404（绝不回 401/403 泄露存在性）。
    let resp = http(
        &server.addr,
        "GET",
        &format!("/lingxi/v1/preview/html/{id}?previewToken=bad"),
        &[],
        None,
    )
    .await;
    assert_eq!(resp.status, 404);
    let resp = http(
        &server.addr,
        "GET",
        "/lingxi/v1/preview/html/pv_nope?previewToken=bad",
        &[],
        None,
    )
    .await;
    assert_eq!(resp.status, 404);

    // 素材面（Public + 路径内 token）：page.html 同根下无素材引用，直接探测。
    let asset_path = format!("/lingxi/v1/preview/html/{id}/assets/{token}/route.html");
    let resp = http(&server.addr, "GET", &asset_path, &[], None).await;
    assert_eq!(resp.status, 200, "asset read: {:?}", resp.status);
    assert_eq!(
        resp.header("content-type"),
        Some("text/html; charset=utf-8")
    );
    assert_eq!(resp.header("cache-control"), Some("no-store"));
    assert_eq!(
        resp.header("cross-origin-resource-policy"),
        Some("same-origin")
    );
    let resp = http(&server.addr, "HEAD", &asset_path, &[], None).await;
    assert_eq!(resp.status, 200);
    assert!(resp.body.is_empty());
    // 错 token / 越界路径 → 404。
    let resp = http(
        &server.addr,
        "GET",
        &format!("/lingxi/v1/preview/html/{id}/assets/bad/route.html"),
        &[],
        None,
    )
    .await;
    assert_eq!(resp.status, 404);
    let resp = http(
        &server.addr,
        "GET",
        &format!("/lingxi/v1/preview/html/{id}/assets/{token}/../Cargo.toml"),
        &[],
        None,
    )
    .await;
    assert_eq!(resp.status, 404);

    server.stop_and_clean().await;
}
