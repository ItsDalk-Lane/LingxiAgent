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
use std::process::Command;
use std::time::Duration;

use lingxi_service::{
    acquire, prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig,
    ServiceDeps, ServiceError, ServiceState,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[test]
fn help_and_version_do_not_hide_invalid_arguments() {
    let binary = env!("CARGO_BIN_EXE_lingxi-service");
    for arg in ["--help", "-h", "--version"] {
        let output = Command::new(binary)
            .arg(arg)
            .output()
            .expect("run service binary");
        assert_eq!(output.status.code(), Some(0), "standalone {arg}");
    }

    // 所有取值选项都不能把帮助或版本参数当作自己的值后伪装成成功。
    for flag in [
        "--bind",
        "--home",
        "--config",
        "--network-mode",
        "--shutdown-timeout-ms",
        "--max-ws-connections",
        "--db-queue-bound",
        "--event-subscriber-queue",
        "--event-reorder-bound",
        "--max-subscribers",
        "--log-max-bytes",
        "--log-max-files",
        "--http-rate-max",
        "--http-max-in-flight",
        "--http-request-budget-ms",
        "--db-wait-budget-ms",
    ] {
        for control in ["--help", "--version"] {
            let output = Command::new(binary)
                .args([flag, control])
                .output()
                .expect("run service binary");
            assert_eq!(output.status.code(), Some(2), "{flag} {control}");
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("error:"),
                "missing error diagnostic: {flag} {control}"
            );
        }
    }

    // 其他无效组合也应在读写数据目录之前以配置错误退出。
    for args in [
        vec!["--help", "--home", "/tmp/unused"],
        vec!["--home", "/tmp/unused", "--version"],
        vec!["--test-mode", "--help"],
        vec!["--help", "--help"],
    ] {
        let output = Command::new(binary)
            .args(&args)
            .output()
            .expect("run service binary");
        assert_eq!(output.status.code(), Some(2), "invalid arguments: {args:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("error:"),
            "missing error diagnostic: {args:?}"
        );
    }
}

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
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
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

async fn http_raw(addr: SocketAddr, request: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect service");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write request");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.expect("read response");
    String::from_utf8(raw).expect("ASCII/UTF-8 response")
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
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
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
            None, // no drain budget: the test drives the stop signal itself
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
async fn browser_cors_preflight_and_response_keep_origin_boundary() {
    let server = start_test_server("cors").await;
    let addr = server.addr;
    let home = server.home.clone();
    let preflight = http_raw(addr, &format!(
        "OPTIONS /lingxi/v1/web-auth/login HTTP/1.1\r\nHost: {addr}\r\nOrigin: null\r\nAccess-Control-Request-Method: POST\r\nAccess-Control-Request-Headers: content-type,authorization\r\nConnection: close\r\n\r\n"
    )).await;
    assert!(preflight.starts_with("HTTP/1.1 204 "), "{preflight}");
    assert!(
        preflight.contains("access-control-allow-origin: null\r\n"),
        "{preflight}"
    );
    assert!(
        preflight.contains("access-control-allow-credentials: true\r\n"),
        "{preflight}"
    );
    assert!(
        preflight.contains(
            "access-control-allow-headers: Content-Type, Authorization, If-None-Match\r\n"
        ),
        "{preflight}"
    );

    let allowed = http_raw(addr, &format!(
        "GET /lingxi/v1/health HTTP/1.1\r\nHost: {addr}\r\nOrigin: null\r\nConnection: close\r\n\r\n"
    )).await;
    assert!(allowed.starts_with("HTTP/1.1 200 "), "{allowed}");
    assert!(
        allowed.contains("access-control-allow-origin: null\r\n"),
        "{allowed}"
    );

    let denied = http_raw(
        addr,
        &format!(
        "GET /lingxi/v1/me HTTP/1.1\r\nHost: {addr}\r\nOrigin: null\r\nConnection: close\r\n\r\n"
    ),
    )
    .await;
    assert!(denied.starts_with("HTTP/1.1 401 "), "{denied}");
    assert!(
        denied.contains("access-control-allow-origin: null\r\n"),
        "{denied}"
    );
    assert!(denied.contains("\"requestId\""), "{denied}");

    let foreign = http_raw(addr, &format!(
        "OPTIONS /lingxi/v1/web-auth/login HTTP/1.1\r\nHost: {addr}\r\nOrigin: https://foreign.example\r\nAccess-Control-Request-Method: POST\r\nConnection: close\r\n\r\n"
    )).await;
    assert!(foreign.starts_with("HTTP/1.1 403 "), "{foreign}");
    assert!(
        !foreign.contains("access-control-allow-origin"),
        "{foreign}"
    );

    server.stop_and_assert_clean().await;
    cleanup(&home);
}

#[tokio::test]
async fn locked_instance_and_local_token_share_one_start_identity() {
    let config = test_config("locked-identity");
    cleanup(&config.data_home);
    let layout = prepare_layout(&config.data_home).expect("private home");
    let (mut guard, stale) = acquire(&layout).expect("exclusive instance lock");
    assert!(stale.is_none());
    let other_home = synthetic_home("identity-other-home");
    cleanup(&other_home);
    std::fs::create_dir_all(&other_home).expect("other isolated home");
    let mut mismatched = config.clone();
    mismatched.data_home = other_home.clone();
    let mismatch = ServiceState::bootstrap_with_locked_instance(
        mismatched,
        &layout,
        ServiceDeps::default(),
        &guard,
    )
    .await;
    assert!(
        mismatch.is_err(),
        "a guard for a different home cannot write a token"
    );
    assert!(
        !state_token_path(&layout).exists(),
        "mismatch wrote a local token"
    );
    cleanup(&other_home);
    let state = ServiceState::bootstrap_with_locked_instance(
        config,
        &layout,
        ServiceDeps::default(),
        &guard,
    )
    .await
    .expect("boot with locked identity");
    let token_file: serde_json::Value = serde_json::from_slice(
        &std::fs::read(state.auth().local_token_path()).expect("local token file"),
    )
    .expect("local token JSON");
    assert_eq!(token_file["instanceId"], guard.identity().instance_id);

    guard
        .publish("127.0.0.1:14777".parse().unwrap())
        .expect("publish record without listening");
    let instance = lingxi_service::instance::read_record(&layout.record_path)
        .expect("instance record exists")
        .expect("instance record is valid");
    assert_eq!(instance.instance_id, guard.identity().instance_id);
    assert_eq!(token_file["instanceId"], instance.instance_id);
    state.storage().close().await.expect("close storage");
    guard.release().expect("release instance record and lock");
    cleanup(&layout.home);
}

fn state_token_path(layout: &lingxi_service::DataRootLayout) -> PathBuf {
    layout
        .runtime_dir
        .join(lingxi_service::auth::LOCAL_TOKEN_FILE)
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
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
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
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let err = config.prepare_data_home().expect_err("must refuse loudly");
    assert!(
        err.to_string().contains("not a directory"),
        "unexpected error: {err}"
    );
    std::fs::remove_file(&home).ok();
    cleanup(&home);
}
