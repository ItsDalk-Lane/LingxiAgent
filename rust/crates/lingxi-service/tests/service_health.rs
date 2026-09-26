//! R02-T01 controlled test harness for lingxi-service (integration layer:
//! real service loop, real loopback TCP, synthetic isolated home under the
//! system temp dir — never a real user directory; no Tauri/Electron in the
//! process tree, matching acceptance R02-A01's environment).
//!
//! Test layer (taskbook 01 §5): 契约/服务集成 — the axum service itself runs
//! for real; the only stubbed part is the shutdown trigger, driven by the
//! test instead of OS signals (the real signal path is exercised by the
//! binary smoke script, `scripts/rust-tauri/r02_t01_service_smoke.sh`).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServiceConfig, ServiceError, ServiceState,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Unique synthetic home under the system temp dir for this test run.
fn synthetic_home(tag: &str) -> PathBuf {
    let pid = std::process::id();
    std::env::temp_dir().join(format!("lingxi-service-r02t01-{pid}-{tag}"))
}

fn test_config(tag: &str) -> ServiceConfig {
    ServiceConfig {
        bind_addr: "127.0.0.1:0"
            .parse()
            .unwrap_or_else(|_| panic!("static loopback address must parse (test bug, tag {tag})")),
        data_home: synthetic_home(tag),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
    }
}

/// Minimal real HTTP/1.1 GET over a tokio TCP stream (deliberately no HTTP
/// client dependency: `Connection: close` keeps the response bounded by EOF).
async fn http_get(addr: SocketAddr, path: &str) -> (String, String) {
    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("connect {addr}: {e}"));
    let request = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .unwrap_or_else(|e| panic!("write request: {e}"));
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .await
        .unwrap_or_else(|e| panic!("read response: {e}"));
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("malformed response (no header/body split): {text:?}"));
    (head.to_string(), body.to_string())
}

fn cleanup(path: &std::path::Path) {
    // Best-effort removal of this test's own synthetic temp dir only.
    if path.exists() {
        let _ = std::fs::remove_dir_all(path);
    }
}

/// A running in-process service instance: readiness arrives over a oneshot
/// channel (no polling, no sleeps), shutdown over another.
struct TestServer {
    addr: SocketAddr,
    home: PathBuf,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<(), ServiceError>>,
}

async fn start_test_server(tag: &str) -> TestServer {
    let config = test_config(tag);
    cleanup(&config.data_home);

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let home = config.data_home.clone();
    let home_for_task = config.data_home.clone();
    let handle = tokio::spawn(async move {
        let layout = prepare_layout(&home_for_task).expect("prepare layout (synthetic home)");
        let state = ServiceState::bootstrap(config, &layout)
            .await
            .expect("bootstrap (synthetic home, real run database)");
        run(
            state,
            async {
                let _ = stop_rx.await;
            },
            |addr| {
                let _ = ready_tx.send(addr);
            },
        )
        .await
    });
    let addr = ready_rx.await.expect("service reports readiness");
    TestServer {
        addr,
        home,
        stop: stop_tx,
        handle,
    }
}

impl TestServer {
    /// Stops the service and asserts a clean, bounded shutdown.
    async fn stop_and_assert_clean(self) {
        self.stop.send(()).expect("server task still listening");
        tokio::time::timeout(Duration::from_secs(10), self.handle)
            .await
            .expect("server shuts down within timeout")
            .expect("server task join")
            .expect("clean serve result");
    }
}

#[tokio::test]
async fn health_check_over_real_loopback() {
    let server = start_test_server("health").await;
    let addr = server.addr;

    let (head, body) = http_get(addr, "/lingxi/v1/health").await;
    assert!(
        head.starts_with("HTTP/1.1 200 "),
        "expected 200, got head: {head}"
    );
    let json: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|e| panic!("health body is not JSON ({e}): {body:?}"));
    let expected = serde_json::json!({
        "status": "ok",
        "serverKind": "lingxi-service",
        "serverVersion": env!("CARGO_PKG_VERSION"),
        "wireProtocolMin": 1,
        "wireProtocolMax": 1,
        "dataEpoch": 1
    });
    assert_eq!(json, expected, "health payload must be exactly minimal");

    // Clean shutdown: run() returns Ok and the port stops accepting.
    server.stop_and_assert_clean().await;
    let refused = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match tokio::net::TcpStream::connect(addr).await {
                Ok(_) => tokio::time::sleep(Duration::from_millis(10)).await,
                Err(_) => return,
            }
        }
    })
    .await;
    assert!(refused.is_ok(), "port must stop accepting after shutdown");

    cleanup(&synthetic_home("health"));
}

#[tokio::test]
async fn unknown_route_is_not_found_for_owner_but_closed_for_strangers() {
    let server = start_test_server("not-found").await;

    // R02-T03: unknown routes fail CLOSED — an unauthenticated request to
    // an unknown /lingxi/v1 path is denied (401) before the 404 would
    // reveal route existence. (The T01 contract was a bare 404; the
    // fail-closed default supersedes it, see the task report.)
    let (head, _body) = http_get(server.addr, "/lingxi/v1/nope").await;
    assert!(
        head.starts_with("HTTP/1.1 401 "),
        "unknown route must fail closed for strangers, got: {head}"
    );

    server.stop_and_assert_clean().await;
    let _ = server.home;
    cleanup(&synthetic_home("not-found"));
}

#[test]
fn prepare_data_home_creates_and_is_idempotent() {
    let home = synthetic_home("prepare-ok");
    cleanup(&home);
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0"
            .parse()
            .unwrap_or_else(|_| panic!("static loopback address must parse")),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
    };
    config.prepare_data_home().expect("creates missing home");
    assert!(home.is_dir());
    config.prepare_data_home().expect("second call is a no-op");
    cleanup(&home);
}

#[test]
fn prepare_data_home_rejects_file_as_home() {
    let home = synthetic_home("prepare-file");
    cleanup(&home);
    std::fs::create_dir_all(home.parent().unwrap_or(&home)).ok();
    std::fs::write(&home, b"not a directory").expect("write blocker file");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0"
            .parse()
            .unwrap_or_else(|_| panic!("static loopback address must parse")),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
    };
    let err = config.prepare_data_home().expect_err("must refuse loudly");
    assert!(
        err.to_string().contains("not a directory"),
        "unexpected error: {err}"
    );
    std::fs::remove_file(&home).ok();
    cleanup(&home);
}
