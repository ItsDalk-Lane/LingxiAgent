//! R06-T05 Round 5 生产链测试：桥媒体 token 读面（叶 #18）。
//!
//! 对照纪律（RC-3，逐条亲读）：
//! - `lib/bridge/media-publisher.ts`（publish :45-82 / resolve :84-107 /
//!   allowed roots :113-120、5min TTL :6、maxDownloads 5 :7、32B
//!   base64url token :24、唯一 token 重试 :122-128）。
//! - `server/routes/bridge.ts` :676-701（GET /bridge/media/:token；
//!   404 "media not found" / 413 "media too large"（50MB，:42）；
//!   content-disposition inline=image|video / attachment（:1112-1115）+
//!   filename* RFC5987（:1106-1110）；no-store + nosniff）。
//! - `lib/bridge/media-roots.ts`（根集合 canonical、拒绝文件系统根 :62）。
//! - 设计登记：D5（/api/bridge/media → /lingxi/v1/bridge/media）；
//!   publish 是服务内 API（叶 #18 只断言 token 读路径），桥外发调用方
//!   属后续阶段。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use lingxi_service::bridgemedia::{BridgeMediaService, MAX_BRIDGE_MEDIA_SIZE};
use lingxi_service::sessionfiles::SessionFileService;
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

// ── 服务级（不起 HTTP） ──

struct BmFixture {
    home: PathBuf,
    payload_dir: PathBuf,
    now: Arc<std::sync::Mutex<u64>>,
    sf: Arc<SessionFileService>,
}

impl Drop for BmFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.home);
        let _ = std::fs::remove_dir_all(&self.payload_dir);
    }
}

async fn bm_fixture(tag: &str, now_ms: u64) -> BmFixture {
    let uniq = format!(
        "{}-{}-{tag}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let home = std::env::temp_dir().join(format!("lingxi-r06t05-bm-{uniq}"));
    let payload_dir = std::env::temp_dir().join(format!("lingxi-r06t05-bm-payload-{uniq}"));
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&payload_dir);
    std::fs::create_dir_all(&home).expect("mkdir home");
    std::fs::create_dir_all(&payload_dir).expect("mkdir payload");
    let home = home.canonicalize().expect("canonical home");
    let payload_dir = payload_dir.canonicalize().expect("canonical payload");
    let db = Arc::new(
        lingxi_adapters::storage::RunDatabase::open(
            &home.join("runs.db"),
            lingxi_adapters::storage::StoreOptions::default(),
        )
        .await
        .expect("open db"),
    );
    let sf = Arc::new(SessionFileService::new(home.clone(), db, move || now_ms));
    BmFixture {
        home,
        payload_dir,
        now: Arc::new(std::sync::Mutex::new(now_ms)),
        sf,
    }
}

fn bm_service(fx: &BmFixture, base_url: &str, roots: &[PathBuf]) -> BridgeMediaService {
    let now = {
        let now = fx.now.clone();
        move || *now.lock().expect("clock")
    };
    BridgeMediaService::new(base_url, roots, now).expect("service config")
}

#[tokio::test]
async fn publish_resolve_download_budget_and_expiry() {
    let fx = bm_fixture("publish", 10_000).await;
    let payload = fx.payload_dir.join("photo.png");
    std::fs::write(&payload, b"\x89PNG\r\n\x1a\n").expect("payload");
    let view = fx
        .sf
        .register_file("s-bm", &payload, None, Some("照片.png"), "agent_write")
        .await
        .expect("register");
    let svc = bm_service(
        &fx,
        "https://bridge.example.com",
        std::slice::from_ref(&fx.payload_dir),
    );
    let published = svc.publish(&view).expect("publish");
    // token = 32B base64url（43 字符）；publicUrl 走 D5 前缀。
    assert_eq!(published.token.len(), 43);
    assert_eq!(
        published.public_url,
        format!(
            "https://bridge.example.com/lingxi/v1/bridge/media/{}",
            published.token
        )
    );
    assert_eq!(published.expires_at, 10_000 + 5 * 60 * 1000);
    // resolve：字段直传（entry :65-74）。
    let entry = svc.resolve(&published.token).expect("resolve 1");
    assert_eq!(entry.file_id, view.file_id);
    // 现役 entry 取名序：filename || label || basename（:64）——注册表
    // filename 非空时优先于 label。
    assert_eq!(entry.filename, "photo.png");
    assert_eq!(entry.mime, "image/png");
    assert_eq!(entry.size, 8);
    // 下载预算 5：第 2..5 次放行，第 6 次回收（resolve :92-95）。
    for i in 2..=5 {
        assert!(svc.resolve(&published.token).is_some(), "download {i}");
    }
    assert!(svc.resolve(&published.token).is_none(), "budget exhausted");
    // 未知 token → None。
    assert!(svc.resolve("nope").is_none());

    // 过期：TTL 边界（:88-91 expiresAt <= now 过期）。
    let published2 = svc.publish(&view).expect("publish 2");
    *fx.now.lock().expect("clock") = 10_000 + 5 * 60 * 1000;
    assert!(svc.resolve(&published2.token).is_none(), "expired at TTL");

    // 源文件消失 → resolve 拒绝（:96-104）。
    *fx.now.lock().expect("clock") = 20_000;
    let published3 = svc.publish(&view).expect("publish 3");
    std::fs::remove_file(&payload).expect("remove payload");
    assert!(svc.resolve(&published3.token).is_none(), "source gone");
}

#[tokio::test]
async fn publish_refusals_are_loud() {
    let fx = bm_fixture("refuse", 1_000).await;
    let payload = fx.payload_dir.join("a.png");
    std::fs::write(&payload, b"\x89PNG").expect("payload");
    let view = fx
        .sf
        .register_file("s-bm", &payload, None, None, "agent_write")
        .await
        .expect("register");
    // 未配 baseUrl → publish 拒绝（:46-48）。
    let svc = bm_service(&fx, "", std::slice::from_ref(&fx.payload_dir));
    assert!(svc.publish(&view).is_err(), "base url required");
    // 白名单外 → 拒绝（_assertAllowed :113-120）。
    let svc = bm_service(&fx, "https://b.example.com", std::slice::from_ref(&fx.home));
    let err = svc.publish(&view).unwrap_err();
    assert!(
        err.to_string().contains("outside allowed roots"),
        "outside roots, got {err}"
    );
    // 空白名单 → 拒绝（:114-116）。
    let svc = bm_service(&fx, "https://b.example.com", &[]);
    assert!(svc.publish(&view).is_err(), "empty roots refuse");
    // 目录不是文件 → 拒绝（:59-60）。
    let svc = bm_service(
        &fx,
        "https://b.example.com",
        std::slice::from_ref(&fx.payload_dir),
    );
    let dir_view = {
        // 目录直接构造视图：register_file 只收文件，publish 的面只关心
        // real_path 指向目录这一事实。
        let mut v = view.clone();
        v.real_path = fx.payload_dir.to_string_lossy().into_owned();
        v
    };
    assert!(svc.publish(&dir_view).is_err(), "dir refused");
    // baseUrl 非 http(s) → 构造拒绝（normalizeBaseUrl :131-139）。
    assert!(
        BridgeMediaService::new("ftp://x", std::slice::from_ref(&fx.payload_dir), || 0).is_err()
    );
    // 文件系统根不允许进白名单（media-roots.ts :62）。
    assert!(BridgeMediaService::new("https://b.example.com", &[PathBuf::from("/")], || 0).is_err());
}

// ── 路由级（真 TCP + 真 HTTP） ──

struct TestServer {
    addr: SocketAddr,
    home: PathBuf,
    workspace: PathBuf,
    session_files: Arc<SessionFileService>,
    bridge_media: Arc<BridgeMediaService>,
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
    let home = std::env::temp_dir().join(format!("lingxi-r06t05-bm-http-{uniq}"));
    let workspace = std::env::temp_dir().join(format!("lingxi-r06t05-bm-http-ws-{uniq}"));
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&home).expect("mkdir home");
    std::fs::create_dir_all(&workspace).expect("mkdir workspace");
    let home = home.canonicalize().expect("canonical home");
    let workspace = workspace.canonicalize().expect("canonical workspace");
    // publish 需要 baseUrl（media-publisher.ts :46-48）；候选人配置源 =
    // 现役环境变量回退 LINGXI_BRIDGE_PUBLIC_BASE_URL。进程内唯一 bootstrap
    // 该面的测试，串行读写无竞态。
    std::env::set_var(
        "LINGXI_BRIDGE_PUBLIC_BASE_URL",
        "https://bridge.example.com",
    );
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
    let session_files = state.session_files().clone();
    let bridge_media = state.bridge_media().clone();
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
        session_files,
        bridge_media,
        stop: stop_tx,
        handle,
    }
}

impl TestServer {
    fn state_session_files(&self) -> &Arc<SessionFileService> {
        &self.session_files
    }

    fn bridge_media(&self) -> &Arc<BridgeMediaService> {
        &self.bridge_media
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
async fn bridge_media_route_public_token_read() {
    let server = start_server("route").await;
    // 经服务内 API 发布 workspace 里的一个图片与一个文本附件。
    let png = server.workspace.join("路由 图.png");
    std::fs::write(&png, b"\x89PNG-route").expect("png");
    let view = server
        .state_session_files()
        .register_file("s-bm-route", &png, None, Some("路由 图.png"), "agent_write")
        .await
        .expect("register png");
    let published = server
        .bridge_media()
        .publish(&view)
        .expect("publish via service api");
    let path = format!("/lingxi/v1/bridge/media/{}", published.token);

    // Public：无 Authorization 凭 token 读（token 即凭证；bridge.ts :676-701）。
    let resp = http(&server.addr, "GET", &path, &[], None).await;
    assert_eq!(resp.status, 200, "public token read");
    assert_eq!(resp.header("content-type"), Some("image/png"));
    assert_eq!(
        resp.header("content-length"),
        Some(format!("{}", b"\x89PNG-route".len()).as_str())
    );
    // image/* → inline；filename* RFC5987 百分号编码（:692-697）。
    let disposition = resp.header("content-disposition").expect("disposition");
    assert!(
        disposition.starts_with("inline; filename*=UTF-8''"),
        "{disposition}"
    );
    assert!(
        disposition.contains("%E8%B7%AF%E7%94%B1"),
        "utf-8 pct, {disposition}"
    );
    assert_eq!(resp.header("cache-control"), Some("no-store"));
    assert_eq!(resp.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(resp.body, b"\x89PNG-route");

    // 未知 token → 404 文本 "media not found"（:679）。
    let resp = http(
        &server.addr,
        "GET",
        "/lingxi/v1/bridge/media/nope",
        &[],
        None,
    )
    .await;
    assert_eq!(resp.status, 404);
    assert_eq!(String::from_utf8_lossy(&resp.body), "media not found");

    // 非 inline mime → attachment。
    let txt = server.workspace.join("note.txt");
    std::fs::write(&txt, b"plain").expect("txt");
    let view = server
        .state_session_files()
        .register_file("s-bm-route", &txt, None, None, "agent_write")
        .await
        .expect("register txt");
    let published = server.bridge_media().publish(&view).expect("publish txt");
    let resp = http(
        &server.addr,
        "GET",
        &format!("/lingxi/v1/bridge/media/{}", published.token),
        &[],
        None,
    )
    .await;
    assert_eq!(resp.status, 200);
    assert!(resp
        .header("content-disposition")
        .expect("disposition")
        .starts_with("attachment;"));

    // 50MB 上限 → 413 "media too large"（:688-690）。稀疏文件。
    let huge = server.workspace.join("huge.bin");
    let f = std::fs::File::create(&huge).expect("create");
    f.set_len(MAX_BRIDGE_MEDIA_SIZE + 1).expect("set_len");
    drop(f);
    let view = server
        .state_session_files()
        .register_file("s-bm-route", &huge, None, None, "agent_write")
        .await
        .expect("register huge");
    let published = server.bridge_media().publish(&view).expect("publish huge");
    let resp = http(
        &server.addr,
        "GET",
        &format!("/lingxi/v1/bridge/media/{}", published.token),
        &[],
        None,
    )
    .await;
    assert_eq!(resp.status, 413);
    assert_eq!(String::from_utf8_lossy(&resp.body), "media too large");

    // 源文件消失 → 404。
    let gone = server.workspace.join("gone.png");
    std::fs::write(&gone, b"\x89PNG").expect("gone");
    let view = server
        .state_session_files()
        .register_file("s-bm-route", &gone, None, None, "agent_write")
        .await
        .expect("register gone");
    let published = server.bridge_media().publish(&view).expect("publish gone");
    std::fs::remove_file(&gone).expect("remove");
    let resp = http(
        &server.addr,
        "GET",
        &format!("/lingxi/v1/bridge/media/{}", published.token),
        &[],
        None,
    )
    .await;
    assert_eq!(resp.status, 404);

    server.stop_and_clean().await;
}
