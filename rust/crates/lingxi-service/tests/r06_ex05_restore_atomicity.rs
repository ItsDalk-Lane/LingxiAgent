//! 原 F-D07 逐断言补充：真实 HTTP、生产存储和所属 Agent 回读；不补未实现产品功能。
#![cfg(target_os = "macos")]
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
async fn denied_write_keeps_files(server: &TestServer, method: &str, path: &str, body: Value) {
    let before_a = file_snapshot(&memory_dir(server, "agentA"));
    let before_b = file_snapshot(&memory_dir(server, "agentB"));
    let (status, response) = server
        .request_token(method, path, Some(body), &server.readonly_token)
        .await;
    assert_eq!(
        status, 403,
        "只有 settings.read 的真实凭证不能写：{response}"
    );
    assert_eq!(file_snapshot(&memory_dir(server, "agentA")), before_a);
    assert_eq!(file_snapshot(&memory_dir(server, "agentB")), before_b);
}

#[tokio::test]
async fn review_http_second_write_failure_keeps_bytes_and_success_undo_keeps_ledger() {
    let server = start_server("ex05-http-second").await;
    let mem = lingxi_service::memory::MemoryService::new(
        server.home.clone(),
        server.home.join("runtime"),
    );
    let p = memory_dir(&server, "agentA");
    let compiled = mem.compiled_store("agentA").unwrap();
    compiled.write_facts("OLD facts").unwrap();
    compiled.write_today("OLD today").unwrap();
    compiled.write_longterm("OLD longterm").unwrap();
    let target = mem
        .dream_store("agentA")
        .unwrap()
        .save("http-original-target", "manual", "dream", None)
        .unwrap();
    let id = target["revisionId"].as_str().unwrap();
    compiled.write_facts("CURRENT facts").unwrap();
    compiled.write_today("CURRENT today").unwrap();
    compiled.write_longterm("CURRENT longterm").unwrap();
    mem.compiled_store("agentB")
        .unwrap()
        .write_facts("B protected")
        .unwrap();
    let b = file_snapshot(&memory_dir(&server, "agentB"));
    let names = ["facts.md", "today.md", "longterm.md", "memory.md"];
    let before: Vec<_> = names
        .iter()
        .map(|n| std::fs::read(p.join(n)).unwrap())
        .collect();
    let db = rusqlite::Connection::open_with_flags(
        server.home.join("lingxi-service/data/runs.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let count = || {
        db.query_row("SELECT COUNT(*) FROM model_call_usage", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap()
    };
    let paid_before = count();
    denied_write_keeps_files(
        &server,
        "POST",
        &format!("/lingxi/v1/memories/dream/revisions/{id}/restore?agentId=agentA"),
        json!({}),
    )
    .await;
    assert!(std::process::Command::new("chflags")
        .arg("uchg")
        .arg(p.join("today.md"))
        .status()
        .unwrap()
        .success());
    let failed = server
        .request(
            "POST",
            &format!("/lingxi/v1/memories/dream/revisions/{id}/restore?agentId=agentA"),
            Some(json!({})),
        )
        .await;
    assert!(std::process::Command::new("chflags")
        .arg("nouchg")
        .arg(p.join("today.md"))
        .status()
        .unwrap()
        .success());
    assert_eq!(failed.0, 500, "{failed:?}");
    assert_eq!(failed.1["code"], "dream_restore_failed");
    assert_eq!(
        names
            .iter()
            .map(|n| std::fs::read(p.join(n)).unwrap())
            .collect::<Vec<_>>(),
        before
    );
    let detail = server
        .request(
            "GET",
            &format!("/lingxi/v1/memories/dream/revisions/{id}?agentId=agentA"),
            None,
        )
        .await;
    assert_eq!(detail.0, 200);
    assert_eq!(detail.1["lastRestore"]["status"], "failed_rolled_back");
    let done = server
        .request(
            "POST",
            &format!("/lingxi/v1/memories/dream/revisions/{id}/restore?agentId=agentA"),
            Some(json!({"expectedCurrentHash":detail.1["currentHash"]})),
        )
        .await;
    assert_eq!(done.0, 200, "{done:?}");
    let undo = done.1["preRestoreRevisionId"].as_str().unwrap();
    assert_eq!(compiled.read_sections().unwrap().facts, "OLD facts");
    let undone = server
        .request(
            "POST",
            &format!("/lingxi/v1/memories/dream/revisions/{undo}/restore?agentId=agentA"),
            Some(json!({})),
        )
        .await;
    assert_eq!(undone.0, 200, "{undone:?}");
    assert_eq!(
        names
            .iter()
            .map(|n| std::fs::read(p.join(n)).unwrap())
            .collect::<Vec<_>>(),
        before
    );
    assert_eq!(file_snapshot(&memory_dir(&server, "agentB")), b);
    assert_eq!(count(), paid_before);
    println!(
        "EX05_HTTP {}",
        json!({"secondWriteStatus":failed.0,"failure":failed.1,"readReceipt":detail.1["lastRestore"],"success":done.1,"undo":undone.1,"paidBefore":paid_before,"paidAfter":count(),"allCurrentBytesPreserved":true,"agentBUnchanged":true})
    );
    drop(db);
    server.stop_and_clean().await;
}

#[tokio::test]
async fn review_next_context_rejects_external_conflict_and_reads_recovered_current_only() {
    use lingxi_adapters::storage::agent_memory as am;
    use lingxi_service::context_compiler::{
        ContextMaterialSource, FileContextMaterialSource, HostFacts, MaterialQuery,
    };
    let server = start_server("ex05-context-recovery").await;
    let mem = lingxi_service::memory::MemoryService::new(
        server.home.clone(),
        server.home.join("runtime"),
    );
    let p = memory_dir(&server, "agentA");
    let c = mem.compiled_store("agentA").unwrap();
    c.write_facts("OLD facts").unwrap();
    c.write_today("OLD today").unwrap();
    let r = mem.dream_store("agentA").unwrap();
    let t = r.save("context-target", "manual", "dream", None).unwrap();
    c.write_facts("CURRENT facts").unwrap();
    c.write_today("CURRENT today").unwrap();
    let hp = p.clone();
    let hook = am::memory_restore_test_hook(
        &p,
        std::sync::Arc::new(move |name| {
            if name == "facts.md" {
                std::fs::write(hp.join("today.md"), "EXTERNAL edit\n").unwrap();
            }
        }),
    );
    assert!(r.restore(t["revisionId"].as_str().unwrap()).is_err());
    drop(hook);
    let source = FileContextMaterialSource::from_home(
        &server.home,
        PathBuf::from(std::env::var("LINGXI_PRODUCT_DIR").unwrap()),
        HostFacts {
            platform: "darwin".into(),
            os_type: "Darwin".into(),
            os_release: "fixture".into(),
            shell_label: "zsh".into(),
            workspace: server.home.to_string_lossy().into_owned(),
        },
    );
    let query = MaterialQuery {
        agent_id: "agentA".into(),
        for_subagent: false,
        session_started_unix_ms: 1,
    };
    let failed = source.gather(&query);
    assert!(failed.is_err());
    assert_eq!(
        std::fs::read_to_string(p.join("today.md")).unwrap(),
        "EXTERNAL edit\n"
    );
    std::fs::write(p.join("today.md"), "CURRENT today\n").unwrap();
    let recovered = source.gather(&query).unwrap();
    assert!(recovered.memory_md.contains("CURRENT facts"));
    assert!(!recovered.memory_md.contains("OLD facts"));
    assert!(!recovered.memory_md.contains("EXTERNAL edit"));
    assert_eq!(
        r.restore_status().unwrap().unwrap()["status"],
        "interrupted_rolled_back"
    );
    server.stop_and_clean().await;
}
