//! R09-T02 服务 supervisor 集成测试：真实 `lingxi-service` 二进制、真实 READY
//! 握手、真实 identity/health、真实信号关停；负向用可控替身。
//!
//! 运行：
//!   rustup run 1.98.1 cargo test --manifest-path desktop/src-tauri/Cargo.toml \
//!     --features r09-host --test service_supervisor -- --nocapture
//!
//! 纪律：不 mock 被验收的 supervisor/私有记录/进程所有权；替身只替换“服务端”
//! 本身（A03 要求的“确定性替身”），宿主代码路径全部真实执行。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use lingxi_resource_desktop::service::env::EnvSnapshot;
use lingxi_resource_desktop::service::supervisor::{
    ServiceMode, ServiceRuntime, SupervisorConfig, Timings,
};
use serde_json::{json, Value};

struct TempHome(PathBuf);
impl TempHome {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "lingxi-r09-supervisor-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        fs::create_dir_all(&dir).unwrap();
        Self(fs::canonicalize(&dir).unwrap())
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TempHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn service_binary() -> PathBuf {
    let candidate = std::env::var_os("LINGXI_R09_TEST_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rust/target/debug/lingxi-service")
        });
    assert!(
        candidate.is_file(),
        "真实 lingxi-service 调试二进制不存在，先跑 `npm run build:rust-service` 或设置 LINGXI_R09_TEST_BINARY: {}",
        candidate.display()
    );
    candidate
}

fn timings() -> Timings {
    Timings {
        read_timeout: Duration::from_secs(60),
        shutdown_grace: Duration::from_secs(17),
        force_kill_wait: Duration::from_secs(5),
        poll: Duration::from_millis(100),
    }
}

fn runtime_config(
    home: &Path,
    extra_env: Vec<(String, String)>,
    timings: Timings,
) -> SupervisorConfig {
    let binary = service_binary();
    let mut pairs = vec![
        ("LINGXI_HOME".to_string(), home.display().to_string()),
        (
            "LINGXI_SERVICE_BIN".to_string(),
            binary.display().to_string(),
        ),
    ];
    pairs.extend(extra_env);
    SupervisorConfig {
        home: home.to_path_buf(),
        env: EnvSnapshot::from_pairs(pairs),
        packaged: false,
        debug_build: true,
        app_version: "0.1.43".to_string(),
        current_exe: binary.clone(),
        dev_default_binary: binary,
        timings,
    }
}

fn read_diagnostics(home: &Path) -> Vec<Value> {
    let path = home.join("diagnostics/tauri-launch/events.jsonl");
    let text = fs::read_to_string(path).unwrap_or_default();
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect()
}

fn events(home: &Path) -> Vec<String> {
    read_diagnostics(home)
        .into_iter()
        .filter_map(|event| event["event"].as_str().map(str::to_string))
        .collect()
}

fn read_failure(runtime: &ServiceRuntime) -> Value {
    let path = runtime.diagnostics().failure_path();
    serde_json::from_str(
        &fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failure record missing at {path:?}: {error}")),
    )
    .unwrap()
}

fn pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[tokio::test(flavor = "multi_thread")]
async fn real_service_roundtrip_ready_identity_and_graceful_shutdown() {
    let home = TempHome::new("roundtrip");
    let runtime = ServiceRuntime::new(runtime_config(home.path(), Vec::new(), timings()));
    let connection = tokio::time::timeout(Duration::from_secs(120), runtime.start_and_wait())
        .await
        .expect("start_and_wait did not finish in 120s")
        .expect("supervisor must start the real service");
    assert_eq!(connection.mode, ServiceMode::LocalOwned);
    assert!(connection.port > 0);
    assert!(
        connection
            .server_node_kind
            .eq_ignore_ascii_case("lingxi-service"),
        "serverNodeKind={}",
        connection.server_node_kind
    );
    assert_eq!(connection.host, None);
    let pid = connection.pid.expect("owned connection records pid");
    assert!(pid_alive(pid), "owned service pid {pid} must be alive");

    // 真实 identity 端点在此连接上可用（带 Bearer）。
    let identity: Value = reqwest::Client::new()
        .get(format!("{}/lingxi/v1/server/identity", connection.base_url))
        .header("Authorization", format!("Bearer {}", connection.token))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(identity["serverNodeKind"], "lingxi-service");

    // 诊断顺序：spawn → ready → instance → identity → service-ready（禁止固定 sleep）。
    let observed = events(home.path());
    let position = |name: &str| observed.iter().position(|event| event == name);
    let spawn = position("spawn").expect("spawn event");
    let ready = position("ready").expect("ready event");
    let instance = position("instance").expect("instance event");
    let identity_event = position("identity").expect("identity event");
    let service_ready = position("service-ready").expect("service-ready event");
    assert!(
        spawn < ready
            && ready < instance
            && instance <= identity_event
            && identity_event < service_ready,
        "events: {observed:?}"
    );

    // READY 之前 instance.json 不应存在：用 ready 事件里的时间戳 vs spawn 时间戳证明
    // 顺序，而 instance 事件在 ready 之后。真实时间戳比较：
    let spawn_at = read_diagnostics(home.path())
        .into_iter()
        .find(|event| event["event"] == "spawn")
        .and_then(|event| event["spawnedAtUnixMs"].as_u64())
        .expect("spawn timestamp");
    let ready_at = read_diagnostics(home.path())
        .into_iter()
        .find(|event| event["event"] == "ready")
        .and_then(|event| event["unixMs"].as_u64())
        .expect("ready timestamp");
    assert!(ready_at >= spawn_at, "READY must not precede spawn");

    let report = runtime.stop().await;
    assert!(report.confirmed, "shutdown report: {}", report.to_json());
    assert!(!report.touched_remote);
    assert!(
        report.residual.is_empty(),
        "residual: {:?}",
        report.residual
    );
    assert!(
        !pid_alive(pid),
        "owned service {pid} must exit after SIGTERM"
    );
    assert!(events(home.path()).iter().any(|event| event == "shutdown"));
    // 服务优雅退出会自删 instance.json（instance.rs 清理契约）。
    assert!(!home.path().join("lingxi-service/instance.json").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn ready_timeout_kills_spawned_child_and_records_failure() {
    let home = TempHome::new("timeout");
    let fake = home.path().join("fake-service.sh");
    fs::write(&fake, "#!/bin/sh\nexec sleep 30\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut config = runtime_config(home.path(), Vec::new(), timings());
    config.env = EnvSnapshot::from_pairs([
        ("LINGXI_HOME", home.path().display().to_string()),
        ("LINGXI_SERVICE_BIN", fake.display().to_string()),
    ]);
    config.timings.read_timeout = Duration::from_millis(600);
    config.timings.poll = Duration::from_millis(50);
    let runtime = ServiceRuntime::new(config);
    let error = runtime
        .start_and_wait()
        .await
        .expect_err("missing READY must fail");
    assert_eq!(error.code, "RUST_SERVICE_READY_TIMEOUT");
    let failure = read_failure(&runtime);
    assert_eq!(failure["record"]["reason"], "RUST_SERVICE_READY_TIMEOUT");
    assert_eq!(failure["record"]["fallbackAttempts"], 0);
    // 超时后本轮已 spawn 的 child 必须被回收（没有 sleep 30 残留）。
    let table = std::process::Command::new("/bin/ps")
        .args(["-axo", "pid=,command="])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&table.stdout);
    assert!(
        !text.contains(&format!(
            "{} --home {}",
            fake.display(),
            home.path().display()
        )),
        "fake service still running after READY timeout:\n{text}"
    );
}

/// 可控替身服务：READY 格式合法，但 instance.json 公告 wireProtocolMax=3。
fn write_incompatible_fake_service(home: &Path) -> PathBuf {
    let script = home.join("fake-incompatible-service.py");
    let source = r#"#!/usr/bin/env python3
import json, os, signal, socket, sys, threading

def main():
    home = None
    args = sys.argv[1:]
    for index, arg in enumerate(args):
        if arg == "--home" and index + 1 < len(args):
            home = os.path.realpath(args[index + 1])
    if home is None:
        print("missing --home", file=sys.stderr)
        return 2
    runtime = os.path.join(home, "lingxi-service")
    os.makedirs(runtime, exist_ok=True)
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", 0))
    listener.listen(16)
    addr = "127.0.0.1:%d" % listener.getsockname()[1]
    instance_id = "0123456789abcdef0123456789abcdef"
    record = {
        "schemaVersion": 1,
        "serverKind": "lingxi-service",
        "serverVersion": "0.0.0",
        "wireProtocolMin": 1,
        "wireProtocolMax": 3,
        "dataEpoch": 2,
        "instanceId": instance_id,
        "startNonce": "fake",
        "pid": os.getpid(),
        "bindAddr": addr,
        "transport": "http",
        "startedAtUnixMs": 1,
        "homePath": home,
    }
    with open(os.path.join(runtime, "instance.json"), "w", encoding="utf-8") as handle:
        json.dump(record, handle)
    token = {
        "schemaVersion": 1,
        "kind": "local_token",
        "token": "fedcba9876543210fedcba9876543210",
        "instanceId": instance_id,
        "createdAtUnixMs": 1,
    }
    token_path = os.path.join(runtime, "local-token.json")
    with open(token_path, "w", encoding="utf-8") as handle:
        json.dump(token, handle)
    os.chmod(token_path, 0o600)
    print("LINGXI_SERVICE_READY addr=%s home=%s source=--home" % (addr, home), flush=True)
    def stop(_signum, _frame):
        listener.close()
        sys.exit(0)
    signal.signal(signal.SIGTERM, stop)
    def serve():
        while True:
            try:
                connection, _ = listener.accept()
                connection.sendall(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                connection.close()
            except OSError:
                return
    threading.Thread(target=serve, daemon=True).start()
    while True:
        signal.pause()
main()
"#;
    fs::write(&script, source).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    }
    script
}

#[tokio::test(flavor = "multi_thread")]
async fn runtime_protocol_mismatch_rejects_and_reaps_fake_service() {
    let home = TempHome::new("runtime-gate");
    let fake = write_incompatible_fake_service(home.path());
    let mut config = runtime_config(home.path(), Vec::new(), timings());
    config.env = EnvSnapshot::from_pairs([
        ("LINGXI_HOME", home.path().display().to_string()),
        ("LINGXI_SERVICE_BIN", fake.display().to_string()),
    ]);
    let runtime = ServiceRuntime::new(config);
    let error = runtime
        .start_and_wait()
        .await
        .expect_err("runtime protocol mismatch must be rejected after READY");
    assert_eq!(error.code, "RUNTIME_IDENTITY_MISMATCH");
    assert!(error.actual.unwrap().contains("wire 1..=3"));
    let failure = read_failure(&runtime);
    assert_eq!(failure["record"]["fallbackAttempts"], 0);
    // 替身进程必须已被本轮 abort 回收。
    let output = std::process::Command::new("/bin/ps")
        .args(["-axo", "pid=,command="])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        !text.contains(&format!(
            "{} --home {}",
            fake.display(),
            home.path().display()
        )),
        "fake service still running after runtime identity mismatch:\n{text}"
    );
}

async fn spawn_standalone_service(home: &Path) -> (std::process::Child, u16, String) {
    let binary = service_binary();
    let mut child = std::process::Command::new(&binary)
        .arg("--home")
        .arg(home)
        .env("LINGXI_HOME", home)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("standalone service spawn");
    use std::io::BufRead;
    let stdout = child.stdout.take().expect("standalone service stdout");
    let mut reader = std::io::BufReader::new(stdout);
    let mut line = String::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        line.clear();
        let read = reader.read_line(&mut line).unwrap();
        if read > 0 && line.starts_with("LINGXI_SERVICE_READY addr=") {
            let addr = line
                .trim_start_matches("LINGXI_SERVICE_READY addr=")
                .split(' ')
                .next()
                .unwrap()
                .to_string();
            let port = addr.rsplit(':').next().unwrap().parse::<u16>().unwrap();
            // token 由服务写入 owner-only 文件。
            let token_path = home.join("lingxi-service/local-token.json");
            let token: Value =
                serde_json::from_str(&fs::read_to_string(token_path).unwrap()).unwrap();
            return (child, port, token["token"].as_str().unwrap().to_string());
        }
        assert!(
            std::time::Instant::now() < deadline,
            "standalone service did not print READY"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn existing_instance_is_reused_and_detached_on_stop() {
    let home = TempHome::new("reused");
    let (mut child, port, token) = spawn_standalone_service(home.path()).await;
    // 等 identity 可用（READY 与 accept 之间有调度片）。
    let client = reqwest::Client::new();
    let mut ready = false;
    for _ in 0..100 {
        match client
            .get(format!("http://127.0.0.1:{port}/lingxi/v1/server/identity"))
            .header("Authorization", format!("Bearer {token}"))
            .timeout(Duration::from_secs(2))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                ready = true;
                break;
            }
            _ => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
    assert!(ready, "standalone service identity never became ready");

    let runtime = ServiceRuntime::new(runtime_config(home.path(), Vec::new(), timings()));
    let connection = tokio::time::timeout(Duration::from_secs(60), runtime.start_and_wait())
        .await
        .unwrap()
        .expect("dev build must reuse an existing verified instance");
    assert_eq!(connection.mode, ServiceMode::LocalReused);
    assert_eq!(connection.pid, Some(child.id()));
    assert!(connection.token.len() == 32);
    let report = runtime.stop().await;
    assert_eq!(report.mode, "local-reused");
    assert!(!report.confirmed);
    assert!(!report.touched_remote);
    assert!(report.owned_pids.is_empty());
    // 复用实例不被宿主杀死（detached）。
    assert!(
        pid_alive(child.id()),
        "reused service must survive host exit"
    );
    assert!(home.path().join("lingxi-service/instance.json").is_file());
    let _ = child.kill();
    let _ = child.wait();
}

#[tokio::test(flavor = "multi_thread")]
async fn remote_mode_never_spawns_or_touches_local_or_remote_process() {
    let remote_home = TempHome::new("remote-fixture");
    let local_home = TempHome::new("remote-local");
    let (mut child, port, token) = spawn_standalone_service(remote_home.path()).await;
    let client = reqwest::Client::new();
    let mut ready = false;
    for _ in 0..100 {
        match client
            .get(format!("http://127.0.0.1:{port}/lingxi/v1/server/identity"))
            .header("Authorization", format!("Bearer {token}"))
            .timeout(Duration::from_secs(2))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                ready = true;
                break;
            }
            _ => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
    assert!(ready);
    let token_file = remote_home.path().join("remote-token.json");
    fs::write(&token_file, format!("{{\"token\":\"{token}\"}}")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&token_file, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let config = runtime_config(
        local_home.path(),
        vec![
            (
                "LINGXI_DESKTOP_SERVER_RUNTIME".to_string(),
                "rust".to_string(),
            ),
            (
                "LINGXI_DESKTOP_RUST_REMOTE_URL".to_string(),
                format!("http://127.0.0.1:{port}"),
            ),
            (
                "LINGXI_DESKTOP_RUST_REMOTE_TOKEN_FILE".to_string(),
                token_file.display().to_string(),
            ),
        ],
        timings(),
    );
    let runtime = ServiceRuntime::new(config);
    let connection = tokio::time::timeout(Duration::from_secs(60), runtime.start_and_wait())
        .await
        .unwrap()
        .expect("remote mode must verify and publish");
    assert_eq!(connection.mode, ServiceMode::Remote);
    assert_eq!(connection.host.as_deref(), Some("127.0.0.1"));
    assert_eq!(connection.port, port);
    // 结构性零接触：本地 home 没有 instance.json，也没有本地 child。
    assert!(!local_home
        .path()
        .join("lingxi-service/instance.json")
        .exists());
    let report = runtime.stop().await;
    assert_eq!(report.mode, "remote");
    assert!(!report.confirmed);
    assert!(!report.touched_remote);
    assert!(report.owned_pids.is_empty());
    // 远端客户端仍存活且 identity 仍可用。
    assert!(pid_alive(child.id()));
    let response = client
        .get(format!("http://127.0.0.1:{port}/lingxi/v1/server/identity"))
        .header("Authorization", format!("Bearer {token}"))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let _ = child.kill();
    let _ = child.wait();
}

/// A03/C17：崩溃最多重启 1 次；第二次崩溃 → CRASH_RESTART_EXHAUSTED + 无残留 child。
/// 纯 headless（不启动 Tauri 窗口），供“禁止 GUI 反复启动”纪律下的回归使用。
#[tokio::test(flavor = "multi_thread")]
async fn crash_restart_exhausted_is_bounded_and_headless() {
    let home = TempHome::new("crash-exhausted");
    let runtime = ServiceRuntime::new(runtime_config(home.path(), Vec::new(), timings()));
    let connection = tokio::time::timeout(Duration::from_secs(120), runtime.start_and_wait())
        .await
        .expect("start_and_wait timeout")
        .expect("supervisor must start the real service");
    let first_pid = connection.pid.expect("owned pid");
    assert!(pid_alive(first_pid));
    // 第一次强杀：监控应自动重启一次。
    unsafe { libc::kill(first_pid as libc::pid_t, libc::SIGKILL) };
    assert!(
        wait_for_test(
            || {
                let conn = runtime.connection();
                conn.map(|c| c.pid != Some(first_pid) && c.pid.map(pid_alive).unwrap_or(false))
                    .unwrap_or(false)
            },
            Duration::from_secs(90)
        ),
        "monitor did not restart the service once"
    );
    let second_pid = runtime
        .connection()
        .and_then(|c| c.pid)
        .expect("restarted pid");
    assert_ne!(second_pid, first_pid);
    // 第二次强杀：不得无限重启，必须落到失败记录。
    unsafe { libc::kill(second_pid as libc::pid_t, libc::SIGKILL) };
    assert!(
        wait_for_test(
            || {
                home.path()
                    .join("diagnostics/tauri-launch/failure.json")
                    .is_file()
                    || runtime.diagnostics().failure_path().is_file()
            },
            Duration::from_secs(90)
        ),
        "crash restart was not bounded (no failure record)"
    );
    let failure = read_failure(&runtime);
    assert_eq!(
        failure["record"]["reason"],
        "RUST_SERVICE_CRASH_RESTART_EXHAUSTED"
    );
    assert_eq!(failure["record"]["fallbackAttempts"], 0);
    // 失败后无 owned child 残留。
    assert!(!pid_alive(first_pid) && !pid_alive(second_pid));
    let report = runtime.stop().await;
    assert!(
        report.residual.is_empty(),
        "residual: {:?}",
        report.residual
    );
}

fn wait_for_test(mut predicate: impl FnMut() -> bool, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

#[test]
fn release_builds_refuse_remote_without_spawning() {
    let home = TempHome::new("remote-release-refusal");
    let mut config = runtime_config(
        home.path(),
        vec![
            (
                "LINGXI_DESKTOP_SERVER_RUNTIME".to_string(),
                "rust".to_string(),
            ),
            (
                "LINGXI_DESKTOP_RUST_REMOTE_URL".to_string(),
                "http://127.0.0.1:43199".to_string(),
            ),
            (
                "LINGXI_DESKTOP_RUST_REMOTE_TOKEN_FILE".to_string(),
                home.path().join("token.json").display().to_string(),
            ),
        ],
        timings(),
    );
    config.debug_build = false;
    let runtime = ServiceRuntime::new(config);
    let runtime_for_call = runtime.clone();
    let runtime_result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async move { runtime_for_call.start_and_wait().await });
    let error = runtime_result.expect_err("release builds must refuse remote");
    assert_eq!(error.code, "REMOTE_REQUIRES_DEV_BUILD");
    let failure = read_failure(&runtime);
    assert_eq!(failure["record"]["fallbackAttempts"], 0);
}

#[test]
fn node_server_info_record_blocks_rust_startup() {
    let home = TempHome::new("node-server-info");
    fs::write(home.path().join("server-info.json"), b"{\"pid\":1}").unwrap();
    let runtime = ServiceRuntime::new(runtime_config(home.path(), Vec::new(), timings()));
    let error = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async move { runtime.start_and_wait().await })
        .expect_err("legacy Node server record must block Rust startup");
    assert_eq!(error.code, "RUST_DESKTOP_NODE_SERVER_INFO_PRESENT");
    let _guard = std::io::stdout().flush();
}

#[test]
fn data_downgrade_env_is_refused() {
    let home = TempHome::new("downgrade");
    let config = runtime_config(
        home.path(),
        vec![("LINGXI_ALLOW_DATA_DOWNGRADE".to_string(), "1".to_string())],
        timings(),
    );
    let runtime = ServiceRuntime::new(config);
    let error = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async move { runtime.start_and_wait().await })
        .expect_err("LINGXI_ALLOW_DATA_DOWNGRADE must be refused");
    assert_eq!(error.code, "RUST_DESKTOP_DATA_DOWNGRADE_REFUSED");
    let _ = json!({});
}
