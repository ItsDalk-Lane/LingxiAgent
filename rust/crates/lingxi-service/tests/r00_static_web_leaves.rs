//! R00 手机和桌面静态入口：独立测试进程隔离环境变量，真实 TCP 取证。

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceError,
    ServiceState,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

struct Server {
    addr: SocketAddr,
    home: PathBuf,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
}

struct HttpResponse {
    status: u16,
    headers: String,
    body: String,
}

async fn request(addr: SocketAddr, path: &str) -> HttpResponse {
    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect to actual service listener");
    let raw_request = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(raw_request.as_bytes())
        .await
        .expect("write request");
    let mut raw = Vec::new();
    loop {
        let mut chunk = [0_u8; 4096];
        match stream.read(&mut chunk).await {
            Ok(0) => break,
            Ok(size) => raw.extend_from_slice(&chunk[..size]),
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
                ) =>
            {
                break
            }
            Err(err) => panic!("read response: {err}"),
        }
    }
    let text = String::from_utf8_lossy(&raw);
    let (headers, body) = text.split_once("\r\n\r\n").expect("HTTP response split");
    let status = headers
        .split_whitespace()
        .nth(1)
        .expect("HTTP status")
        .parse()
        .expect("numeric HTTP status");
    HttpResponse {
        status,
        headers: headers.into(),
        body: body.into(),
    }
}

async fn start_server(dist: Option<&Path>, working_dir: Option<&Path>) -> Server {
    let original_dist = std::env::var_os("LINGXI_RENDERER_DIST");
    let original_dir = std::env::current_dir().expect("current directory");
    match dist {
        Some(path) => std::env::set_var("LINGXI_RENDERER_DIST", path),
        None => std::env::remove_var("LINGXI_RENDERER_DIST"),
    }
    if let Some(path) = working_dir {
        std::env::set_current_dir(path).expect("isolated fixture working directory");
    }
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let home = std::env::temp_dir().join(format!(
        "lingxi-r02-r00-static-{}-{unique}",
        std::process::id()
    ));
    assert!(!home.exists(), "static test home must be fresh");
    let layout = prepare_layout(&home).expect("prepare synthetic home");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static address"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let state = ServiceState::bootstrap(config, &layout)
        .await
        .expect("bootstrap static service");
    std::env::set_current_dir(original_dir).expect("restore working directory");
    match original_dist {
        Some(value) => std::env::set_var("LINGXI_RENDERER_DIST", value),
        None => std::env::remove_var("LINGXI_RENDERER_DIST"),
    }
    let (stop, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let handle = tokio::spawn(async move {
        run(
            state,
            async {
                let _ = stop_rx.await;
            },
            |addr| {
                let _ = ready.send(addr);
            },
            None,
        )
        .await
    });
    let addr = ready_rx.await.expect("actual static service READY");
    Server {
        addr,
        home,
        stop,
        handle,
    }
}

impl Server {
    async fn stop(self) {
        self.stop.send(()).expect("signal static service stop");
        tokio::time::timeout(Duration::from_secs(10), self.handle)
            .await
            .expect("bounded static shutdown")
            .expect("static service task")
            .expect("clean static service shutdown");
        std::fs::remove_dir_all(self.home).expect("remove only synthetic static home");
    }
}

fn record(cases: &mut Vec<Value>, name: &str, observed: Value) {
    cases.push(json!({"case": name, "expect": 1, "actual": 1, "ok": true, "observed": observed}));
    // 失败前完成的逐项观察也保留，缺项仍会使独立生产者判 FAIL。
    save_cases(cases);
}

fn save_cases(cases: &[Value]) {
    let Ok(path) = std::env::var("R02_STATIC_CASES_PATH") else {
        return;
    };
    let path = Path::new(&path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("static evidence directory");
    }
    std::fs::write(
        path,
        serde_json::to_vec_pretty(
            &json!({"schema": "lingxi.leaf-case-results.v1", "cases": cases}),
        )
        .expect("serialize static cases"),
    )
    .expect("write static evidence");
}

#[tokio::test]
async fn static_mobile_and_desktop_routes_on_real_service() {
    // 此测试文件只有一个测试，Cargo 为它创建独立进程；环境变量不会污染其它测试进程。
    let fixture = std::env::temp_dir().join(format!(
        "lingxi-r02-r00-static-fixture-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let dist = fixture.join("dist");
    std::fs::create_dir_all(dist.join("assets")).expect("create synthetic dist");
    std::fs::write(dist.join("mobile.html"), b"<html>R02 static fixture</html>")
        .expect("write exact fixture page");
    std::fs::write(
        dist.join("assets/app.js"),
        b"console.log('R02 static fixture')",
    )
    .expect("write exact fixture asset");
    let oversized = std::fs::File::create(dist.join("assets/oversized.js"))
        .expect("create oversized synthetic asset");
    oversized
        .set_len(16 * 1024 * 1024 + 1)
        .expect("set sparse oversized asset length");
    let outside = fixture.join("private.txt");
    std::fs::write(&outside, b"must not be served").expect("write outside fixture");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, dist.join("assets/escape.js"))
        .expect("create synthetic escaping symlink");

    let mut cases = Vec::new();
    let dist_server = start_server(Some(&dist), None).await;
    for (name, page, asset) in [
        (
            "static-mobile-dist-page-assets",
            "/mobile/",
            "/mobile/assets/app.js",
        ),
        (
            "static-desktop-dist-page-assets",
            "/desktop/",
            "/desktop/assets/app.js",
        ),
    ] {
        let html = request(dist_server.addr, page).await;
        let javascript = request(dist_server.addr, asset).await;
        assert_eq!(html.status, 200);
        assert!(html.headers.to_ascii_lowercase().contains("text/html"));
        assert_eq!(html.body, "<html>R02 static fixture</html>");
        assert_eq!(javascript.status, 200);
        assert!(javascript
            .headers
            .to_ascii_lowercase()
            .contains("text/javascript"));
        assert_eq!(javascript.body, "console.log('R02 static fixture')");
        record(
            &mut cases,
            name,
            json!({"pageStatus": html.status, "assetStatus": javascript.status, "pageBody": html.body, "assetBody": javascript.body, "contentTypesExact": true}),
        );
    }
    let mut negative_routes = serde_json::Map::new();
    let mut oversized_routes = serde_json::Map::new();
    for route in ["mobile", "desktop"] {
        let missing = request(dist_server.addr, &format!("/{route}/assets/missing.js")).await;
        let outside_root = request(dist_server.addr, &format!("/{route}/private.txt")).await;
        let encoded_traversal = request(
            dist_server.addr,
            &format!("/{route}/assets/%2e%2e/private.txt"),
        )
        .await;
        assert_eq!(missing.status, 404);
        assert_eq!(outside_root.status, 404);
        assert_ne!(encoded_traversal.status, 200);
        assert!(!encoded_traversal.body.contains("must not be served"));
        #[cfg(unix)]
        {
            let symlink_escape =
                request(dist_server.addr, &format!("/{route}/assets/escape.js")).await;
            assert_eq!(symlink_escape.status, 404);
            assert!(!symlink_escape.body.contains("must not be served"));
        }
        negative_routes.insert(
            route.to_owned(),
            json!({"missing": missing.status, "outsideRoot": outside_root.status,
                "encodedTraversal": encoded_traversal.status, "outsideBytesAbsent": true}),
        );
        let oversized = request(dist_server.addr, &format!("/{route}/assets/oversized.js")).await;
        assert_eq!(oversized.status, 413);
        assert!(!oversized.body.contains("R02 static fixture"));
        oversized_routes.insert(
            route.to_owned(),
            json!({"status": oversized.status, "successBodyAbsent": true}),
        );
    }
    record(
        &mut cases,
        "static-web-traversal-secret-refused",
        Value::Object(negative_routes),
    );
    record(
        &mut cases,
        "static-web-oversized-asset-explicit-error",
        Value::Object(oversized_routes),
    );
    dist_server.stop().await;

    let guide_dir = fixture.join("guide-working-dir");
    std::fs::create_dir_all(&guide_dir).expect("create empty guide cwd");
    let guide_server = start_server(None, Some(&guide_dir)).await;
    for (name, path) in [
        ("static-mobile-guide-without-dist", "/mobile/"),
        ("static-desktop-guide-without-dist", "/desktop/"),
    ] {
        let guide = request(guide_server.addr, path).await;
        assert_eq!(guide.status, 200);
        assert!(guide.body.contains("网页界面尚未安装"));
        assert!(guide.headers.to_ascii_lowercase().contains("text/html"));
        record(
            &mut cases,
            name,
            json!({"status": guide.status, "guideVisible": true, "body": guide.body}),
        );
    }
    let guide_asset = request(guide_server.addr, "/mobile/assets/app.js").await;
    assert_eq!(guide_asset.status, 404);
    record(
        &mut cases,
        "static-web-guide-missing-asset-404",
        json!({"status": guide_asset.status}),
    );
    guide_server.stop().await;

    let invalid_dist = fixture.join("missing-explicit-dist");
    assert!(!invalid_dist.exists());
    let error_server = start_server(Some(&invalid_dist), None).await;
    for (name, path) in [
        ("static-mobile-invalid-explicit-dist-503", "/mobile/"),
        ("static-desktop-invalid-explicit-dist-503", "/desktop/"),
    ] {
        let error = request(error_server.addr, path).await;
        assert_eq!(error.status, 503);
        assert!(error.body.contains("网页界面目录缺失或损坏"));
        assert!(!error.body.contains(invalid_dist.to_string_lossy().as_ref()));
        record(
            &mut cases,
            name,
            json!({"status": error.status, "explicitError": true, "privatePathAbsent": true, "body": error.body}),
        );
    }
    error_server.stop().await;

    save_cases(&cases);
    std::fs::remove_dir_all(fixture).expect("remove only synthetic static fixture");
}
