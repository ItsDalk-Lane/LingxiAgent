//! 记忆清理补全：真实 HTTP 清除总结来源，检查隔离与拒绝后的实际文件。
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

struct TestServer {
    addr: SocketAddr,
    home: PathBuf,
    token: String,
    readonly_token: String,
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
    let home = std::env::temp_dir().join(format!("lingxi-r06-original-mem-{uniq}"));
    let workspace = std::env::temp_dir().join(format!("lingxi-r06-original-mem-ws-{uniq}"));
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&home).expect("mkdir home");
    std::fs::create_dir_all(&workspace).expect("mkdir workspace");
    // 两个 agent（启动后落盘——R02 数据纪元闸只锁 runs.db 种子面；记忆材料
    // 是服务面文件，与 T05 的 canonical home 落盘纪律一致）。
    for agent_id in ["agentA", "agentB"] {
        let agent_dir = home.join("agents").join(agent_id);
        std::fs::create_dir_all(agent_dir.join("memory")).expect("mkdir agent");
        std::fs::write(
            agent_dir.join("config.yaml"),
            "locale: zh\nagent:\n  name: 助手\nmemory:\n  enabled: true\n",
        )
        .expect("write config");
    }
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
    let readonly_token = state
        .auth()
        .issue_device_credential(
            lingxi_service::LOCAL_OWNER_USER_ID,
            &["settings.read"],
            None,
        )
        .expect("只读凭证")
        .secret;
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
        token,
        readonly_token,
        stop: stop_tx,
        handle,
    }
}

impl TestServer {
    async fn request(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        self.request_token(method, path, body, &self.token).await
    }

    async fn request_token(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
        token: &str,
    ) -> (u16, Value) {
        let payload = body.map(|b| b.to_string()).unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            token,
            payload.len()
        );
        let mut stream = TcpStream::connect(self.addr).await.expect("connect");
        stream.write_all(request.as_bytes()).await.expect("write");
        let mut buf = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut buf)).await;
        let raw = String::from_utf8_lossy(&buf).to_string();
        let status = raw
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0);
        let body = raw
            .split_once("\r\n\r\n")
            .map(|(_, b)| b.to_string())
            .unwrap_or_default();
        let value = serde_json::from_str(&body).unwrap_or(Value::Null);
        (status, value)
    }

    async fn stop_and_clean(self) {
        self.stop.send(()).expect("server still listening");
        tokio::time::timeout(Duration::from_secs(10), self.handle)
            .await
            .expect("server shuts down within timeout")
            .expect("server task join")
            .expect("clean serve result");
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

fn memory_dir(server: &TestServer, agent: &str) -> PathBuf {
    server.home.join("agents").join(agent).join("memory")
}
fn facts(server: &TestServer, agent: &str) -> Vec<lingxi_adapters::storage::agent_memory::FactRow> {
    lingxi_service::memory::MemoryService::new(server.home.clone(), server.home.join("runtime"))
        .fact_store(agent)
        .unwrap()
        .get_all()
        .unwrap()
}
fn file_snapshot(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    fn walk(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<(String, Vec<u8>)>) {
        if !dir.exists() {
            return;
        }
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, root, out);
            } else if !path.to_string_lossy().contains("facts.db") {
                out.push((
                    path.strip_prefix(root).unwrap().to_string_lossy().into(),
                    std::fs::read(path).unwrap(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn seed_sources(server: &TestServer, agent: &str) -> PathBuf {
    let directory = memory_dir(server, agent).join("summaries");
    std::fs::create_dir_all(directory.join("nested")).unwrap();
    std::fs::write(
        directory.join("old-summary.json"),
        r#"{"facts":["旧总结来源"]}"#,
    )
    .unwrap();
    std::fs::write(directory.join("keep.txt"), "不是总结来源").unwrap();
    std::fs::write(directory.join("nested/keep.json"), "不递归删除").unwrap();
    for file in lingxi_adapters::storage::agent_memory::COMPILED_FILES {
        std::fs::write(memory_dir(server, agent).join(file), "旧整理内容").unwrap();
        std::fs::write(
            memory_dir(server, agent).join(format!("{file}.fingerprint")),
            "旧指纹",
        )
        .unwrap();
    }
    directory
}

#[tokio::test]
async fn clear_summary_sources_is_agent_scoped_preserves_raw_until_all_clear_and_next_read_is_empty(
) {
    let server = start_server("clear-sources").await;
    for agent in ["agentA", "agentB"] {
        let (status, _) = server
            .request(
                "POST",
                &format!("/lingxi/v1/memories/import?agentId={agent}"),
                Some(json!({"facts":[{"fact":format!("{agent}原始事实")}]})),
            )
            .await;
        assert_eq!(status, 200);
        seed_sources(&server, agent);
    }
    let other = file_snapshot(&memory_dir(&server, "agentB"));
    let summary = memory_dir(&server, "agentA").join("summaries");
    for (route, remaining) in [
        ("/lingxi/v1/memories/compiled?agentId=agentA", 1),
        ("/lingxi/v1/memories?agentId=agentA", 0),
    ] {
        std::fs::write(summary.join("old-summary.json"), "旧总结").unwrap();
        let (status, body) = server.request("DELETE", route, None).await;
        assert_eq!(status, 200);
        assert_eq!(body["ok"], true);
        assert!(!summary.join("old-summary.json").exists());
        assert_eq!(
            std::fs::read_to_string(summary.join("keep.txt")).unwrap(),
            "不是总结来源"
        );
        assert_eq!(
            std::fs::read_to_string(summary.join("nested/keep.json")).unwrap(),
            "不递归删除"
        );
        assert_eq!(facts(&server, "agentA").len(), remaining);
        for file in lingxi_adapters::storage::agent_memory::COMPILED_FILES {
            assert_eq!(
                std::fs::read_to_string(memory_dir(&server, "agentA").join(file)).unwrap(),
                ""
            );
            assert!(!memory_dir(&server, "agentA")
                .join(format!("{file}.fingerprint"))
                .exists());
        }
        let marker: Value = serde_json::from_slice(
            &std::fs::read(memory_dir(&server, "agentA").join("reset.json")).unwrap(),
        )
        .unwrap();
        assert!(marker["compiledResetAt"].is_string());
        assert_eq!(file_snapshot(&memory_dir(&server, "agentB")), other);
        let (status, readback) = server
            .request("GET", "/lingxi/v1/memories/compiled?agentId=agentA", None)
            .await;
        assert_eq!(status, 200);
        assert_eq!(readback["sections"]["facts"], "");
        assert_eq!(readback["sections"]["today"], "");
    }
    server.stop_and_clean().await;
}

#[tokio::test]
async fn clear_sources_denied_invalid_io_and_database_failure_keep_actual_sources_facts_and_marker()
{
    let server = start_server("clear-refusals").await;
    let (status, _) = server
        .request(
            "POST",
            "/lingxi/v1/memories/import?agentId=agentA",
            Some(json!({"facts":[{"fact":"拒绝时保留原始事实"}]})),
        )
        .await;
    assert_eq!(status, 200);
    seed_sources(&server, "agentA");
    let directory = memory_dir(&server, "agentA");
    std::fs::write(
        directory.join("reset.json"),
        r#"{"compiledResetAt":"2020-01-01T00:00:00Z"}"#,
    )
    .unwrap();
    let before = file_snapshot(&directory);
    let (status, _) = server
        .request_token(
            "DELETE",
            "/lingxi/v1/memories/compiled?agentId=agentA",
            None,
            &server.readonly_token,
        )
        .await;
    assert_eq!(status, 403);
    assert_eq!(file_snapshot(&directory), before);
    let (status, _) = server
        .request("DELETE", "/lingxi/v1/memories?agentId=ghost", None)
        .await;
    assert_eq!(status, 404);
    assert_eq!(file_snapshot(&directory), before);
    // 先在真正数据库注入可复现的拒绝，避免用假的清理成功代替错误处理。
    let database = rusqlite::Connection::open(directory.join("facts.db")).unwrap();
    database.execute_batch("CREATE TRIGGER refuse_clear BEFORE DELETE ON facts BEGIN SELECT RAISE(ABORT, '清理拒绝'); END;").unwrap();
    let (status, error) = server
        .request("DELETE", "/lingxi/v1/memories?agentId=agentA", None)
        .await;
    assert_eq!(status, 500);
    assert!(error["error"].is_string());
    assert_eq!(facts(&server, "agentA").len(), 1);
    let after = file_snapshot(&directory);
    for (name, bytes) in &before {
        if !name.starts_with("facts.db") {
            assert_eq!(
                after
                    .iter()
                    .find(|(candidate, _)| candidate == name)
                    .unwrap()
                    .1,
                *bytes
            );
        }
    }
    database
        .execute_batch("DROP TRIGGER refuse_clear;")
        .unwrap();
    drop(database);
    // 非普通产物路径必须在任何清除或重置标记写入前拒绝。
    std::fs::remove_file(directory.join("week.md")).unwrap();
    std::fs::create_dir(directory.join("week.md")).unwrap();
    let before_invalid = file_snapshot(&directory);
    let (status, error) = server
        .request(
            "DELETE",
            "/lingxi/v1/memories/compiled?agentId=agentA",
            None,
        )
        .await;
    assert_eq!(status, 400);
    assert!(error["error"].is_string());
    assert_eq!(file_snapshot(&directory), before_invalid);
    assert!(directory.join("summaries/old-summary.json").exists());
    assert_eq!(facts(&server, "agentA").len(), 1);
    server.stop_and_clean().await;
}

#[cfg(unix)]
#[tokio::test]
async fn clear_sources_refuses_linked_directory_and_cannot_clear_another_agents_sources() {
    let server = start_server("clear-symlink").await;
    seed_sources(&server, "agentA");
    let other_sources = seed_sources(&server, "agentB");
    let summaries = memory_dir(&server, "agentA").join("summaries");
    std::fs::remove_dir_all(&summaries).unwrap();
    std::os::unix::fs::symlink(other_sources, &summaries).unwrap();
    let before = file_snapshot(&memory_dir(&server, "agentB"));
    let compiled_before = std::fs::read(memory_dir(&server, "agentA").join("memory.md")).unwrap();
    let (status, error) = server
        .request(
            "DELETE",
            "/lingxi/v1/memories/compiled?agentId=agentA",
            None,
        )
        .await;
    assert_eq!(status, 400);
    assert!(error["error"].is_string());
    assert_eq!(file_snapshot(&memory_dir(&server, "agentB")), before);
    assert_eq!(
        std::fs::read(memory_dir(&server, "agentA").join("memory.md")).unwrap(),
        compiled_before
    );
    assert!(!memory_dir(&server, "agentA").join("reset.json").exists());
    std::fs::remove_file(summaries).unwrap();
    server.stop_and_clean().await;
}

#[cfg(unix)]
#[tokio::test]
async fn late_summary_delete_io_failure_rolls_back_compiled_files_marker_and_fact_transaction() {
    use std::os::unix::fs::PermissionsExt;
    let server = start_server("clear-late-io").await;
    let (status, _) = server
        .request(
            "POST",
            "/lingxi/v1/memories/import?agentId=agentA",
            Some(json!({"facts":[{"fact":"晚期失败保留事实"}]})),
        )
        .await;
    assert_eq!(status, 200);
    let summaries = seed_sources(&server, "agentA");
    let directory = memory_dir(&server, "agentA");
    std::fs::write(
        directory.join("reset.json"),
        r#"{"compiledResetAt":"2020-01-01T00:00:00Z"}"#,
    )
    .unwrap();
    let before = file_snapshot(&directory);
    std::fs::set_permissions(&summaries, std::fs::Permissions::from_mode(0o500)).unwrap();
    for route in [
        "/lingxi/v1/memories/compiled?agentId=agentA",
        "/lingxi/v1/memories?agentId=agentA",
    ] {
        let (status, body) = server.request("DELETE", route, None).await;
        assert_eq!(status, 500);
        assert!(body["error"].is_string());
        assert_eq!(facts(&server, "agentA").len(), 1);
        let after = file_snapshot(&directory);
        for (name, bytes) in &before {
            if !name.starts_with("facts.db") {
                assert_eq!(
                    after
                        .iter()
                        .find(|(candidate, _)| candidate == name)
                        .unwrap()
                        .1,
                    *bytes
                );
            }
        }
    }
    std::fs::set_permissions(summaries, std::fs::Permissions::from_mode(0o700)).unwrap();
    server.stop_and_clean().await;
}
