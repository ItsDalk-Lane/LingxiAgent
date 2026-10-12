#![allow(dead_code)]
//! R06-T03 服务层生产链测试：真 TCP loopback + 真 HTTP + 真 SQLite
//! （bootstrap_with_deps 注入确定性 provider 替身），覆盖 fork / retry /
//! rewind / checkpoint / 管理面的端到端语义。
//!
//! 对照纪律（RC-3）：断言值可追溯到现役锚点——
//! - fork 深度上限 2 → 409：`server/routes/sessions.ts:1738,1795-1801`。
//! - fork 保留消息 ID：`pi-coding-agent/dist/core/session-manager.js:1113`。
//! - busy 409：`core/session-turn-actions.ts:382-384`。
//! - retry 目标解析（user/assistant/latest-user）：
//!   `core/session-turn-actions.ts:78-165`。
//! - rewind 文件冲突整体拒绝（A06 加固，差异 D1）：现役盲写回
//!   `core/workspace-snapshots.ts:584-620`。
//! - 回退偏好默认关 → 403：`core/preferences-manager.ts:322-324`。
//! - 管理面：rename:2493 / pin:1014 / pin-order:1052 / archive:2569
//!   （child_sessions_present 409）/ restore:2813 / archived-delete:2882 /
//!   cleanup:2517（maxAgeDays 默认 90）/ search:857 / memory:1103,1135。
//! - 跨主体 403：`sessions.rs` can_access（本地主人全见，设备主体仅本人）。

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use lingxi_kernel::model_exchange::ModelTurnInput;
use lingxi_kernel::ports::{
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, TurnDeltaSink, TurnProviderPort,
};
use lingxi_protocol::{ContentBlock, ErrorCode, ModelCallId, NormalizedMessage, ProtocolError};
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

// ── 确定性 provider 替身（只产出外部响应，不写任何状态） ──

enum ScriptStep {
    /// 立即返回 Final。
    Final(String),
}

struct ScriptedProvider {
    script: std::sync::Mutex<VecDeque<ScriptStep>>,
}

impl ScriptedProvider {
    fn new(script: Vec<ScriptStep>) -> Arc<Self> {
        Arc::new(Self {
            script: std::sync::Mutex::new(script.into_iter().collect()),
        })
    }
}

impl TurnProviderPort for ScriptedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.provider".to_string(),
            model: "stub.model".to_string(),
            operation: "chat".to_string(),
        }
    }

    fn next_turn<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        call: &'a ModelCallId,
        _input: &'a ModelTurnInput,
        _deltas: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let step = self.script.lock().unwrap().pop_front();
        let ctx_at_issue = ctx.clone();
        let call_id = call.to_string();
        Box::pin(async move {
            match step {
                Some(ScriptStep::Final(text)) => ProviderTurnResult::of_ctx(
                    &ctx_at_issue,
                    ProviderTurn::Final {
                        message: NormalizedMessage {
                            role: "assistant".to_string(),
                            content: vec![ContentBlock::Text { text }],
                            model_call_id: Some(ModelCallId::new(call_id)),
                        },
                    },
                ),
                None => ProviderTurnResult::of_ctx(
                    &ctx_at_issue,
                    ProviderTurn::Failed {
                        error: ProtocolError::new(
                            ErrorCode::UpstreamUnavailable,
                            "script exhausted",
                            false,
                        ),
                        retryable: false,
                    },
                ),
            }
        })
    }
}

// ── harness ──

struct TestServer {
    addr: SocketAddr,
    home: PathBuf,
    token: String,
    state: ServiceState,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
}

fn synthetic_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r06t03-svc-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

async fn start_server(tag: &str, provider: Arc<ScriptedProvider>) -> TestServer {
    let home = std::env::var_os("LINGXI_W02_CRASH_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| synthetic_home(tag));
    start_server_at(tag, provider, home).await
}

// 重启夹具复用原资料目录，服务仍按真实启动和HTTP入口运行。
async fn start_server_at(tag: &str, provider: Arc<ScriptedProvider>, home: PathBuf) -> TestServer {
    let layout = prepare_layout(&home).expect("prepare layout");
    let home = if std::env::var_os("LINGXI_W02_KEEP_HOME_ALIAS").is_some() {
        home
    } else {
        home.canonicalize().unwrap()
    };
    let workspace = home.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static loopback addr"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let state = ServiceState::bootstrap_with_deps(
        config,
        &layout,
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            workspace_root: Some(if tag == "protected-state" {
                home.clone()
            } else {
                workspace
            }),
            ..ServiceDeps::default()
        },
    )
    .await
    .expect("bootstrap");
    println!(
        "isolation tag={tag} configured_home={} storage={} env_home={} explicit_home_wins=true",
        home.display(),
        state.storage().db_path().display(),
        std::env::var("LINGXI_HOME").unwrap_or_default()
    );
    assert!(state
        .storage()
        .db_path()
        .starts_with(home.canonicalize().unwrap()));
    let token = state.auth().local_token();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let serve_state = state.clone();
    let handle = tokio::spawn(async move {
        run(
            serve_state,
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
        state,
        stop: stop_tx,
        handle,
    }
}

impl TestServer {
    fn bearer(&self) -> String {
        format!("Bearer {}", self.token)
    }

    async fn stop_and_clean(self) {
        let home = self.stop_and_keep().await;
        let _ = std::fs::remove_dir_all(home);
    }

    async fn stop_and_keep(self) -> PathBuf {
        self.stop.send(()).expect("server still listening");
        tokio::time::timeout(Duration::from_secs(10), self.handle)
            .await
            .expect("server shuts down within timeout")
            .expect("server task join")
            .expect("clean serve result");
        self.home
    }

    async fn post(&self, path: &str, body: &str) -> (u16, String) {
        let evidence = path.ends_with("/checkpoints")
            || path.ends_with("/rewind")
            || path.starts_with("/lingxi/v1/resource-io/write")
            || path == "/lingxi/v1/file-history/restore";
        if evidence {
            println!("synthetic_request method=POST path={path} body={body}");
        }
        let response = http(
            &self.addr,
            "POST",
            path,
            &[("Authorization", &self.bearer())],
            Some(body),
        )
        .await;
        if evidence {
            println!(
                "synthetic_response path={path} status={} body={}",
                response.0, response.1
            );
        }
        response
    }

    async fn get(&self, path: &str) -> (u16, String) {
        http(
            &self.addr,
            "GET",
            path,
            &[("Authorization", &self.bearer())],
            None,
        )
        .await
    }

    /// 执行一轮并等待 run 终态（scripted Final 即时完成）。
    async fn execute(&self, session_id: &str, input: &str) -> String {
        let (status, body) = self
            .post(
                &format!("/lingxi/v1/sessions/{session_id}/execute"),
                &serde_json::json!({ "input": input }).to_string(),
            )
            .await;
        assert_eq!(status, 200, "execute {session_id}: {body}");
        let run_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["runId"]
            .as_str()
            .unwrap()
            .to_string();
        // 等终态（最多 5s）。
        for _ in 0..100 {
            let (status, reason) = run_row(self.state.storage(), &run_id).await;
            if status == "completed" || status == "failed" {
                assert_eq!(
                    status, "completed",
                    "scripted Final must complete (reason: {reason:?})"
                );
                return run_id;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("run {run_id} did not finalize within 5s");
    }

    /// 当前分支的消息 id 链（GET branch）。
    async fn branch_ids(&self, session_id: &str) -> Vec<String> {
        let (status, body) = self
            .get(&format!("/lingxi/v1/sessions/{session_id}/branch"))
            .await;
        assert_eq!(status, 200, "branch {session_id}: {body}");
        let parsed = serde_json::from_str::<serde_json::Value>(&body).unwrap();
        parsed["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["messageId"].as_str().unwrap().to_string())
            .collect()
    }

    /// 设备主体令牌（跨主体 403 测试）。
    async fn mint_device_token(&self, user_id: &str) -> String {
        let (status, body) = self
            .post(
                "/lingxi/v1/devices/credentials",
                &serde_json::json!({ "userId": user_id, "scopes": ["chat"] }).to_string(),
            )
            .await;
        assert_eq!(status, 201, "mint device credential: {body}");
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["secret"]
            .as_str()
            .unwrap()
            .to_string()
    }
}

async fn run_row(
    storage: &Arc<lingxi_adapters::storage::RunDatabase>,
    run_id: &str,
) -> (String, Option<String>) {
    let status = storage
        .query_one_text(
            "SELECT status FROM runs WHERE run_id = ?1",
            vec![run_id.to_string()],
        )
        .await
        .expect("query status")
        .expect("run row");
    let reason = storage
        .query_one_text(
            "SELECT terminal_reason FROM runs WHERE run_id = ?1",
            vec![run_id.to_string()],
        )
        .await
        .expect("query reason");
    (status, reason)
}

/// Plain HTTP/1.1 over a one-shot TCP connection（与 auth_matrix 同款）。
async fn http(
    addr: &SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> (u16, String) {
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
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("malformed response: {text:?}"));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("no status in {head:?}"));
    (status, body.to_string())
}

/// 创建顶层会话（管理面创建路由）。
async fn create_session(server: &TestServer, session_id: &str, title: &str) {
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions",
            &serde_json::json!({
                "sessionId": session_id,
                "agentId": "agent",
                "title": title,
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 201, "create session {session_id}: {body}");
}

#[cfg(unix)]
#[tokio::test]
async fn ar_f02_c02_checkpoint_path_symlink_must_not_change_external_sentinel() {
    let server = start_server(
        "ar-f02-old-red",
        ScriptedProvider::new(vec![ScriptStep::Final("answer".into())]),
    )
    .await;
    let work = server.home.join("workspace");
    std::fs::create_dir_all(&work).unwrap();
    let work = work.canonicalize().unwrap();
    let file = work.join("doc.txt");
    let sentinel = server.home.join("sentinel.txt");
    std::fs::write(&file, "A").unwrap();
    let (status, body) = server.post("/lingxi/v1/sessions", &serde_json::json!({"sessionId":"s","agentId":"agent","title":"s","authorizedFolders":[work]}).to_string()).await;
    assert_eq!(status, 201, "{body}");
    let run = server.execute("s", "q").await;
    for (name, bytes) in [("A", "A"), ("B", "B")] {
        std::fs::write(&file, bytes).unwrap();
        let (status, body) = server.post("/lingxi/v1/sessions/s/checkpoints", &serde_json::json!({"name":name,"targetMessageId":format!("{run}-final"),"filePaths":[file]}).to_string()).await;
        assert_eq!(status, 200, "{body}");
    }
    std::fs::write(&sentinel, "B").unwrap();
    std::fs::remove_file(&file).unwrap();
    std::os::unix::fs::symlink(&sentinel, &file).unwrap();
    server.state.set_rollback_file_changes(true);
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s/rewind",
            r#"{"checkpoint":"A","restoreFiles":true}"#,
        )
        .await;
    println!("restore_http={status} receipt={body}");
    let actual = std::fs::read_to_string(&sentinel).unwrap();
    println!("external_sentinel_expected=B actual={actual}");
    assert_eq!(actual, "B", "恢复不得沿检查点路径链接覆盖外部文件");
    std::fs::remove_file(&sentinel).unwrap();
    std::fs::remove_dir_all(&work).unwrap();
    server.stop_and_clean().await;
}

async fn prepared(tag: &str) -> (TestServer, PathBuf, String) {
    let server = start_server(
        tag,
        ScriptedProvider::new(vec![ScriptStep::Final("answer".into())]),
    )
    .await;
    let work = server.home.join("workspace").canonicalize().unwrap();
    let file = work.join("doc.txt");
    std::fs::write(&file, "A").unwrap();
    let(status,body)=server.post("/lingxi/v1/sessions",&serde_json::json!({"sessionId":"s","agentId":"agent","title":"s","authorizedFolders":[work]}).to_string()).await;
    assert_eq!(status, 201, "{body}");
    let run = server.execute("s", "q").await;
    let(status,body)=server.post("/lingxi/v1/sessions/s/checkpoints",&serde_json::json!({"name":"A","targetMessageId":format!("{run}-final"),"filePaths":[file]}).to_string()).await;
    assert_eq!(status, 200, "{body}");
    app_write(&server, &file, "B").await;
    server.state.set_rollback_file_changes(true);
    (server, file, run)
}
async fn app_write(server: &TestServer, file: &std::path::Path, bytes: &str) {
    let(status,body)=server.post("/lingxi/v1/resource-io/write",&serde_json::json!({"sessionId":"s","resource":{"kind":"local-file","path":file},"content":bytes,"encoding":"utf-8"}).to_string()).await;
    assert_eq!(status, 200, "app write: {body}");
}
async fn rewind(server: &TestServer) -> serde_json::Value {
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s/rewind",
            r#"{"checkpoint":"A","restoreFiles":true}"#,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    println!("receipt={body}");
    serde_json::from_str(&body).unwrap()
}

#[tokio::test]
async fn rewind_recovery_records_survive_request_replay_and_service_restart() {
    let (server, file, _) = prepared("receipt-replay").await;
    let journal = server.home.join("file-recovery-records");
    let journal_before_rewind = std::fs::read_dir(&journal).unwrap().count();
    let request = serde_json::json!({
        "checkpoint": "A",
        "restoreFiles": true,
        "requestId": "rewind-recovery-receipt"
    })
    .to_string();
    let path = "/lingxi/v1/sessions/s/rewind";
    let (status, body) = server.post(path, &request).await;
    assert_eq!(status, 200, "{body}");
    let first: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(first["restored"][0], file.to_string_lossy().as_ref());
    assert_eq!(std::fs::read(&file).unwrap(), b"A");
    let records = first["recoveryRecords"].as_array().unwrap();
    assert_eq!(records.len(), 1, "首次真实回退必须生成恢复记录");
    let retained = PathBuf::from(records[0]["retainedPath"].as_str().unwrap());
    assert_eq!(std::fs::read(&retained).unwrap(), b"B");
    let journal_before: Vec<_> = std::fs::read_dir(&journal)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let bytes = std::fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect();
    assert_eq!(journal_before.len(), journal_before_rewind + records.len());
    let resets_before = server
        .state
        .storage()
        .query_one_text(
            "SELECT CAST(COUNT(*) AS TEXT) FROM key_events WHERE session_id=?1 AND event_type='session_branch_reset'",
            vec!["s".to_string()],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resets_before, "1");

    // 先制造回退之后的外部修改，重复请求只能返回旧收据，不能再次写回。
    std::fs::write(&file, "external-after-rewind").unwrap();
    let (status, body) = server.post(path, &request).await;
    assert_eq!(status, 200, "{body}");
    let replay: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(replay, first, "同请求重放必须保留完整首次收据");
    assert_eq!(std::fs::read(&file).unwrap(), b"external-after-rewind");
    assert_eq!(std::fs::read(&retained).unwrap(), b"B");
    assert_eq!(std::fs::read_dir(&journal).unwrap().count(), journal_before.len());
    for (path, bytes) in &journal_before {
        assert_eq!(&std::fs::read(path).unwrap(), bytes);
    }
    let resets_after_replay = server
        .state
        .storage()
        .query_one_text(
            "SELECT CAST(COUNT(*) AS TEXT) FROM key_events WHERE session_id=?1 AND event_type='session_branch_reset'",
            vec!["s".to_string()],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resets_after_replay, resets_before);

    // 重新启动真实服务，验证收据来自持久记录且不重复生成文件恢复或事件。
    let home = server.stop_and_keep().await;
    let restarted = start_server_at("receipt-replay", ScriptedProvider::new(vec![]), home).await;
    restarted.state.set_rollback_file_changes(true);
    let (status, body) = restarted.post(path, &request).await;
    assert_eq!(status, 200, "{body}");
    let after_restart: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(after_restart, first, "重启重放必须保留完整首次收据");
    assert_eq!(std::fs::read(&file).unwrap(), b"external-after-rewind");
    assert_eq!(std::fs::read(&retained).unwrap(), b"B");
    assert_eq!(std::fs::read_dir(&journal).unwrap().count(), journal_before.len());
    for (path, bytes) in &journal_before {
        assert_eq!(&std::fs::read(path).unwrap(), bytes);
    }
    let resets_after_restart = restarted
        .state
        .storage()
        .query_one_text(
            "SELECT CAST(COUNT(*) AS TEXT) FROM key_events WHERE session_id=?1 AND event_type='session_branch_reset'",
            vec!["s".to_string()],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resets_after_restart, resets_before);
    println!(
        "recovery_receipt_replay_evidence={}",
        serde_json::json!({
            "first": first,
            "replay": replay,
            "afterRestart": after_restart,
            "targetAfterReplay": "external-after-rewind",
            "retainedBytes": "B",
            "recoveryJournalCount": journal_before.len(),
            "resetEvents": resets_after_restart
        })
    );
    restarted.stop_and_clean().await;
}

#[tokio::test]
async fn ar_f02_c01_normal_restore_and_external_same_old_bytes_protected() {
    let (server, file, _) = prepared("normal").await;
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s/rewind/preview",
            r#"{"checkpoint":"A","restoreFiles":true}"#,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("restored"));
    let receipt = rewind(&server).await;
    assert_eq!(receipt["restored"][0], file.to_string_lossy().as_ref());
    assert_eq!(std::fs::read(&file).unwrap(), b"A");
    let retained = receipt["recoveryRecords"][0]["retainedPath"]
        .as_str()
        .unwrap();
    assert_eq!(std::fs::read(retained).unwrap(), b"B");
    println!("normal_target=A retained_original=B");
    server.stop_and_clean().await;
    let (server, file, run) = prepared("external-old").await;
    let(status,body)=server.post("/lingxi/v1/sessions/s/checkpoints",&serde_json::json!({"name":"B","targetMessageId":format!("{run}-final"),"filePaths":[file]}).to_string()).await;
    assert_eq!(status, 200, "{body}");
    app_write(&server, &file, "D").await;
    // 外部进程写回旧检查点B的字节，不能因此被认为是应用的这次修改。
    std::process::Command::new("python3")
        .args([
            "-c",
            "import pathlib,sys;pathlib.Path(sys.argv[1]).write_bytes(b'B')",
            file.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    let receipt = rewind(&server).await;
    assert_eq!(receipt["conflicted"][0], file.to_string_lossy().as_ref());
    assert_eq!(std::fs::read(&file).unwrap(), b"B");
    server.stop_and_clean().await;
}
#[tokio::test]
async fn ar_f02_c02_current_folder_revocation() {
    let (server, file, _) = prepared("revoke").await;
    let before = std::fs::read(&file).unwrap();
    rusqlite::Connection::open(server.state.storage().db_path())
        .unwrap()
        .execute(
            "UPDATE sessions SET authorized_folders_json='[]' WHERE session_id='s'",
            [],
        )
        .unwrap();
    let receipt = rewind(&server).await;
    assert_eq!(receipt["conflicted"][0], file.to_string_lossy().as_ref());
    assert_eq!(std::fs::read(&file).unwrap(), before);
    server.stop_and_clean().await;
}
#[tokio::test]
async fn ar_f02_c03_deterministic_external_content_and_identity_change() {
    for replace in [false, true] {
        for boundary in ["before_replace", "commit_boundary"] {
            let (server, file, _) = prepared(if replace { "identity" } else { "content" }).await;
            server.state.resource_io().file_safety().set_test_hook(Some(Arc::new(move |path,step|{
            if step==boundary {
                let script = if replace { "import pathlib,sys,os;p=pathlib.Path(sys.argv[1]);q=p.with_suffix('.external');q.write_bytes(b'C');os.replace(q,p)" }
                    else { "import pathlib,sys;pathlib.Path(sys.argv[1]).write_bytes(b'C')" };
                assert!(std::process::Command::new("python3").args(["-c",script,path.to_str().unwrap()]).status().unwrap().success());
            }Ok(())
        })));
            let receipt = rewind(&server).await;
            assert_eq!(receipt["conflicted"][0], file.to_string_lossy().as_ref());
            assert_eq!(std::fs::read(&file).unwrap(), b"C");
            if boundary == "commit_boundary" {
                let record = &receipt["recoveryRecords"][0];
                assert_eq!(record["status"], "conflict_restored");
                assert_eq!(
                    std::fs::read(record["retainedPath"].as_str().unwrap()).unwrap(),
                    b"A"
                );
                println!("boundary={boundary} replace_identity={replace} final=C preserved_replacement=A");
            }
            server.stop_and_clean().await;
        }
    }
}
#[tokio::test]
async fn ar_f02_c04_failure_stage_receipts_and_original_bytes() {
    for stage in [
        "temp_write",
        "permission_copy",
        "temp_sync",
        "before_replace",
        "after_replace",
    ] {
        let (server, file, _) = prepared(stage).await;
        server
            .state
            .resource_io()
            .file_safety()
            .set_test_hook(Some(Arc::new(move |path, step| {
                #[cfg(unix)]
                if step == "temp_write" {
                    use std::os::unix::fs::PermissionsExt;
                    for entry in std::fs::read_dir(path.parent().unwrap()).unwrap() {
                        let entry = entry.unwrap();
                        if entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with(".lingxi-write-")
                        {
                            assert_eq!(
                                entry.metadata().unwrap().permissions().mode() & 0o777,
                                0o600
                            );
                            if stage == "temp_write" {
                                // 真实临时载荷已部分落盘，再注入磁盘满错误；目标必须仍为 B。
                                std::fs::write(entry.path(), b"PARTIAL").unwrap();
                            }
                        }
                    }
                }
                if step == stage {
                    if stage == "temp_write" {
                        Err(std::io::Error::from_raw_os_error(libc::ENOSPC).to_string())
                    } else {
                        Err("synthetic io failure".into())
                    }
                } else {
                    Ok(())
                }
            })));
        let receipt = rewind(&server).await;
        assert_eq!(receipt["failed"][0], file.to_string_lossy().as_ref());
        assert!(receipt["restored"].as_array().unwrap().is_empty());
        assert_eq!(
            std::fs::read(&file).unwrap(),
            if stage == "after_replace" { b"A" } else { b"B" }
        );
        if stage == "after_replace" {
            assert_eq!(receipt["fileRestoreReason"], "write_may_have_occurred");
        }
        let names: Vec<_> = std::fs::read_dir(file.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert!(!names
            .iter()
            .any(|n| n.to_string_lossy().starts_with(".lingxi-write")));
        server.stop_and_clean().await;
    }
}

#[tokio::test]
async fn ar_f02_c03_second_competitor_preserves_both_external_objects() {
    let (server, file, _) = prepared("second-race").await;
    server
        .state
        .resource_io()
        .file_safety()
        .set_test_hook(Some(Arc::new(move |path, step| {
            let text = match step {
                "commit_boundary" => Some("C"),
                "before_conflict_restore" => Some("D"),
                _ => None,
            };
            if let Some(text) = text {
                let replacement = path.with_extension("outside");
                std::fs::write(&replacement, text).unwrap();
                std::fs::rename(replacement, path).unwrap();
            }
            Ok(())
        })));
    let receipt = rewind(&server).await;
    assert_eq!(receipt["fileRestoreReason"], "write_may_have_occurred");
    assert_eq!(std::fs::read(&file).unwrap(), b"C");
    let record = &receipt["recoveryRecords"][0];
    assert_eq!(record["status"], "unknown_original_retained");
    assert_eq!(
        std::fs::read(record["retainedPath"].as_str().unwrap()).unwrap(),
        b"D"
    );
    println!("second_competitor final=C retained=D receipt=unknown");
    server.stop_and_clean().await;
}
#[tokio::test]
async fn ar_f02_crash_child() {
    let Ok(stage) = std::env::var("LINGXI_W02_CRASH_STAGE") else {
        return;
    };
    let (server, file, run_id) = prepared("crash").await;
    let multi = stage == "multi_after_first";
    if multi {
        let second = file.parent().unwrap().join("z-other.txt");
        app_write(&server, &file, "A").await;
        std::fs::write(&second, "A").unwrap();
        let (status, body) = server.post("/lingxi/v1/sessions/s/checkpoints", &serde_json::json!({"name":"Multi","targetMessageId":format!("{run_id}-final"),"filePaths":[file,second]}).to_string()).await;
        assert_eq!(status, 200, "{body}");
        app_write(&server, &file, "B").await;
        app_write(&server, &second, "B").await;
    }
    server
        .state
        .resource_io()
        .file_safety()
        .set_test_hook(Some(Arc::new(move |_, step| {
            if step == stage || (multi && step == "after_replace") {
                std::process::exit(73);
            }
            Ok(())
        })));
    if multi {
        server
            .post(
                "/lingxi/v1/sessions/s/rewind",
                r#"{"checkpoint":"Multi","restoreFiles":true}"#,
            )
            .await;
    } else {
        rewind(&server).await;
    }
    panic!("crash seam did not execute");
}
#[tokio::test]
async fn ar_f02_c04_process_stop_and_restart_does_not_replay_unknown_write() {
    for stage in ["commit_boundary", "after_replace", "multi_after_first"] {
        let home = synthetic_home(stage);
        std::fs::create_dir_all(&home).unwrap();
        let home = home.canonicalize().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "ar_f02_crash_child", "--nocapture"])
            .env("LINGXI_HOME", &home)
            .env("LINGXI_W02_CRASH_HOME", &home)
            .env("LINGXI_W02_CRASH_STAGE", stage)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(73), "{:?}", output);
        let file = home.join("workspace/doc.txt");
        let actual = std::fs::read(&file).unwrap();
        if stage == "multi_after_first" {
            assert_eq!(
                std::fs::read(home.join("workspace/z-other.txt")).unwrap(),
                b"B"
            );
        }
        assert_eq!(
            actual,
            if stage == "commit_boundary" {
                b"B"
            } else {
                b"A"
            }
        );
        let records: Vec<serde_json::Value> = std::fs::read_dir(home.join("file-recovery-records"))
            .unwrap()
            .map(|e| serde_json::from_slice(&std::fs::read(e.unwrap().path()).unwrap()).unwrap())
            .collect();
        let pending = records.iter().find(|r| r["status"] == "prepared").unwrap();
        let retained = std::fs::read(pending["retainedPath"].as_str().unwrap()).unwrap();
        assert_eq!(
            retained,
            if stage == "commit_boundary" {
                b"A"
            } else {
                b"B"
            }
        );
        let layout = prepare_layout(&home).unwrap();
        let config = ServiceConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            data_home: home.clone(),
            home_source: HomeSource::Cli,
            network_mode: NetworkMode::Loopback,
            shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
        };
        let state = ServiceState::bootstrap_with_deps(
            config,
            &layout,
            ServiceDeps {
                workspace_root: Some(home.join("workspace")),
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), actual);
        if stage == "multi_after_first" {
            assert_eq!(
                std::fs::read(home.join("workspace/z-other.txt")).unwrap(),
                b"B"
            );
        }
        assert_eq!(
            std::fs::read(pending["retainedPath"].as_str().unwrap()).unwrap(),
            retained
        );
        assert_eq!(
            std::fs::read_dir(home.join("file-recovery-records"))
                .unwrap()
                .count(),
            records.len()
        );
        println!(
            "stop_stage={stage} exit=73 target={:?} retained={:?} restart_no_replay=true",
            String::from_utf8(actual),
            String::from_utf8(retained)
        );
        state.storage().close().await.unwrap();
        drop(state);
        std::fs::remove_dir_all(home).unwrap();
    }
}
#[tokio::test]
async fn ar_f02_family_expected_version_and_filehistory_real_routes() {
    let (server, file, _) = prepared("family").await;
    let (status, body) = server
        .post(
            "/lingxi/v1/resource-io/read",
            &serde_json::json!({"resource":{"kind":"local-file","path":file}}).to_string(),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let read: serde_json::Value = serde_json::from_str(&body).unwrap();
    server
        .state
        .resource_io()
        .file_safety()
        .set_test_hook(Some(Arc::new(|path, step| {
            if step == "commit_boundary" {
                std::fs::write(path, "C").unwrap();
            }
            Ok(())
        })));
    let(status,body)=server.post("/lingxi/v1/resource-io/write-expected-version",&serde_json::json!({"resource":{"kind":"local-file","path":file},"expectedVersion":read["version"],"content":"D"}).to_string()).await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(std::fs::read(&file).unwrap(), b"C");
    server.state.resource_io().file_safety().set_test_hook(None);
    let (status, body) = server
        .get("/lingxi/v1/file-history/versions?agentId=agent&relPath=doc.txt")
        .await;
    assert_eq!(status, 200, "{body}");
    let versions: serde_json::Value = serde_json::from_str(&body).unwrap();
    let id = versions["versions"][0]["id"].as_i64().unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/file-history/restore",
            &serde_json::json!({"agentId":"agent","snapshotId":id}).to_string(),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(std::fs::read(&file).unwrap(), b"B");
    println!("family expected_version=409 current=C; filehistory_restore=200 current=B");
    server.stop_and_clean().await;
}

#[cfg(unix)]
#[tokio::test]
async fn ar_f02_c02_ancestor_link_and_false_checkpoint_source() {
    let (server, _, run) = prepared("ancestor").await;
    let root = server.home.join("workspace").canonicalize().unwrap();
    let folder = root.join("sub");
    std::fs::create_dir(&folder).unwrap();
    let file = folder.join("doc.txt");
    std::fs::write(&file, "A").unwrap();
    let(status,body)=server.post("/lingxi/v1/sessions/s/checkpoints",&serde_json::json!({"name":"parent","targetMessageId":format!("{run}-final"),"filePaths":[file]}).to_string()).await;
    assert_eq!(status, 200, "{body}");
    app_write(&server, &file, "B").await;
    let sentinel = server.home.join("sentinel-dir");
    std::fs::create_dir(&sentinel).unwrap();
    std::fs::write(sentinel.join("doc.txt"), "B").unwrap();
    std::fs::rename(&folder, root.join("saved-sub")).unwrap();
    std::os::unix::fs::symlink(&sentinel, &folder).unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s/rewind",
            r#"{"checkpoint":"parent","restoreFiles":true}"#,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let receipt: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(receipt["restored"].as_array().unwrap().is_empty());
    assert_eq!(std::fs::read(sentinel.join("doc.txt")).unwrap(), b"B");
    println!("ancestor_sentinel=B receipt={body}");
    server.stop_and_clean().await;
}

#[tokio::test]
async fn ar_f02_source_state_and_recovery_objects_cannot_be_forged_via_file_api() {
    let (server, file, _) = prepared("protected-state").await;
    let home = server.home.canonicalize().unwrap();
    let broad = Arc::new(
        lingxi_service::resourceaccess::ResourceAccess::new(std::slice::from_ref(&home)).unwrap(),
    );
    broad
        .protect_service_root(&home.join("file-write-witnesses"))
        .unwrap();
    broad
        .protect_service_root(&home.join("file-recovery-records"))
        .unwrap();
    for path in [
        home.join("file-write-witnesses/forged.json"),
        home.join("file-recovery-records/forged.json"),
        home.join("workspace/.lingxi-recovery-forged.original"),
        home.clone(),
    ] {
        assert!(broad
            .authorize(
                "local_user",
                "user_local",
                "s",
                &path,
                &home,
                lingxi_service::resourceaccess::ResourceOp::Write
            )
            .is_err());
        if path != home {
            assert!(broad
                .authorize(
                    "local_user",
                    "user_local",
                    "s",
                    &path,
                    &home,
                    lingxi_service::resourceaccess::ResourceOp::Read
                )
                .is_err());
        }
    }
    let receipt = rewind(&server).await;
    let retained = receipt["recoveryRecords"][0]["retainedPath"]
        .as_str()
        .unwrap();
    let(status,body)=server.post("/lingxi/v1/resource-io/write",&serde_json::json!({"resource":{"kind":"local-file","path":retained},"content":"forged"}).to_string()).await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(std::fs::read(retained).unwrap(), b"B");
    assert_eq!(std::fs::read(&file).unwrap(), b"A");
    assert!(server.state.fs_read().resolve(retained).is_none());
    let (status, body) = server
        .get(&format!("/lingxi/v1/fs/read?path={}", retained))
        .await;
    assert_eq!(status, 403, "{body}");
    assert!(server
        .state
        .session_files()
        .register_file("s", std::path::Path::new(retained), None, None, "local")
        .await
        .is_err());
    for root in ["file-write-witnesses", "file-recovery-records"] {
        let state_file = std::fs::read_dir(home.join(root))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert!(server
            .state
            .fs_read()
            .resolve(state_file.to_str().unwrap())
            .is_none());
        let(status,body)=server.post("/lingxi/v1/resource-io/write",&serde_json::json!({"resource":{"kind":"local-file","path":state_file},"content":"forged"}).to_string()).await;
        assert_eq!(status, 403, "{body}");
        let (status, body) = server
            .get(&format!(
                "/lingxi/v1/fs/read?path={}",
                state_file.to_str().unwrap()
            ))
            .await;
        assert_eq!(status, 403, "{body}");
        assert!(server
            .state
            .session_files()
            .register_file("s", &state_file, None, None, "local")
            .await
            .is_err());
    }
    println!("protected_state broad_root_denied=true retained_original_write=403 original=B");
    let sidecar = home.join("protected-legacy.json");
    std::fs::write(&sidecar,serde_json::json!({"version":1,"files":{"sf_protected":{"fileId":"sf_protected","filePath":retained,"realPath":retained,"storageKind":"external","status":"available","filename":"doc.txt","mime":"text/plain","size":1,"mtimeMs":1,"isDirectory":false,"fileKind":"text","origin":"user_upload","registeredAt":1,"updatedAt":1}},"refs":[]}).to_string()).unwrap();
    let imported = server
        .state
        .session_files()
        .import_legacy_sidecar("s", &sidecar)
        .await
        .unwrap();
    assert_eq!(imported.imported, 1);
    let (status, body) = server
        .get("/lingxi/v1/resources/res_sf_protected/content")
        .await;
    assert_eq!(status, 403, "{body}");
    server.stop_and_clean().await;
}

#[cfg(unix)]
#[tokio::test]
async fn ar_f02_source_directory_link_cannot_supply_a_forged_witness() {
    let (server, file, _) = prepared("source-directory-link").await;
    let home = server.home.canonicalize().unwrap();
    let source = home.join("file-write-witnesses");
    let saved = home.join("saved-witnesses");
    std::fs::rename(&source, &saved).unwrap();
    std::os::unix::fs::symlink(&saved, &source).unwrap();
    let original_state = std::fs::read_dir(&saved)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let original_state_bytes = std::fs::read(&original_state).unwrap();
    let receipt = rewind(&server).await;
    assert_eq!(receipt["conflicted"][0], file.to_string_lossy().as_ref());
    assert_eq!(std::fs::read(&file).unwrap(), b"B");
    assert_eq!(
        std::fs::read(&original_state).unwrap(),
        original_state_bytes
    );
    println!("source_directory_link rejected=true target=B external_state_unchanged=true");
    server.stop_and_clean().await;
}
#[cfg(unix)]
#[tokio::test]
async fn ar_f02_hardlink_delete_recreate_and_large_external_file_are_preserved() {
    let (server, file, _) = prepared("hardlink").await;
    let link = file.with_extension("hardlink");
    std::fs::hard_link(&file, &link).unwrap();
    std::fs::write(&link, "C").unwrap();
    let receipt = rewind(&server).await;
    assert!(receipt["restored"].as_array().unwrap().is_empty());
    assert_eq!(std::fs::read(&file).unwrap(), b"C");
    server.stop_and_clean().await;
    let (server, file, _) = prepared("recreated").await;
    std::fs::remove_file(&file).unwrap();
    std::fs::write(&file, "B").unwrap();
    let receipt = rewind(&server).await;
    assert!(receipt["restored"].as_array().unwrap().is_empty());
    assert_eq!(std::fs::read(&file).unwrap(), b"B");
    server.stop_and_clean().await;
    let (server, file, _) = prepared("large").await;
    let external = std::fs::OpenOptions::new().write(true).open(&file).unwrap();
    external
        .set_len(lingxi_service::file_safety::MAX_RECOVERABLE_FILE_BYTES + 1)
        .unwrap();
    let before = external.metadata().unwrap().len();
    let receipt = rewind(&server).await;
    assert!(receipt["restored"].as_array().unwrap().is_empty());
    assert_eq!(external.metadata().unwrap().len(), before);
    println!("large_external_bytes={before} refused_without_truncation=true");
    server.stop_and_clean().await;
}
#[tokio::test]
async fn ar_f02_checkpoint_content_corruption_is_rejected() {
    let (server, file, _) = prepared("corrupt-archive").await;
    rusqlite::Connection::open(server.state.storage().db_path())
        .unwrap()
        .execute(
            "UPDATE file_checkpoints SET content=x'58' WHERE file_path=?1",
            [file.to_str().unwrap()],
        )
        .unwrap();
    let receipt = rewind(&server).await;
    assert_eq!(receipt["failed"][0], file.to_string_lossy().as_ref());
    assert_eq!(std::fs::read(&file).unwrap(), b"B");
    server.stop_and_clean().await;
}

#[tokio::test]
async fn ar_f02_native_filetools_write_then_restore_and_session_witness_binding() {
    use lingxi_kernel::ports::{ToolOutcome, ToolRequest};
    use lingxi_kernel::toolcatalog::{SchemaBudget, ToolRegistry};
    use lingxi_service::toolgateway::{
        CallerSurface, InvocationPermissionContext, ToolInvocationGateway,
    };
    for (principal, session, expected_restored) in [
        (lingxi_kernel::Principal::LocalUser, "s", true),
        (lingxi_kernel::Principal::LocalUser, "other-session", false),
        (
            lingxi_kernel::Principal::Device {
                device_id: "synthetic-device".into(),
                user_id: "synthetic-other-user".into(),
            },
            "s",
            false,
        ),
    ] {
        let (server, file, _) = prepared("native-filetools").await;
        let root = file.parent().unwrap().to_path_buf();
        let access = Arc::new(
            lingxi_service::resourceaccess::ResourceAccess::new(std::slice::from_ref(&root))
                .unwrap(),
        );
        let registry = Arc::new(ToolRegistry::new());
        let clock = Arc::new(lingxi_service::inject::SystemClock);
        let approvals = Arc::new(lingxi_service::approval_service::ApprovalService::new(
            clock.clone(),
        ));
        let gateway = ToolInvocationGateway::new(
            registry.clone(),
            approvals,
            clock.clone(),
            SchemaBudget::default(),
            60_000,
            100,
        );
        let core = lingxi_service::filetools::register_core_file_tools(
            &registry,
            &gateway,
            access,
            root,
            clock,
            Arc::new(lingxi_service::filetools::NoopChangeLog),
            &SchemaBudget::default(),
        );
        core.tools
            .set_file_safety(server.state.resource_io().file_safety());
        let ctx = lingxi_kernel::RunContext {
            principal,
            session_id: lingxi_protocol::SessionId::new(session),
            run_id: lingxi_protocol::RunId::new("native"),
            attempt: lingxi_protocol::AttemptId::new("native#a1"),
            generation: 1,
        };
        let request = ToolRequest::from_effective_arguments(
            core.write_target.as_str(),
            serde_json::json!({"path":file,"content":"D"}),
            &SchemaBudget::default(),
        )
        .unwrap();
        let call = lingxi_protocol::ToolCallId::new(format!("native-{session}"));
        let prepared = gateway
            .prepare_from_request(
                &ctx,
                CallerSurface::UserRun,
                "agent",
                InvocationPermissionContext::UserSession {
                    mode: lingxi_kernel::subagent::SessionPermissionMode::Operate,
                },
                &call,
                &request,
            )
            .unwrap();
        let result = gateway
            .execute_prepared(&ctx, &call, &prepared.handle)
            .await
            .unwrap();
        assert!(
            matches!(result.outcome, ToolOutcome::Success { .. }),
            "{:?}",
            result.outcome
        );
        assert_eq!(std::fs::read(&file).unwrap(), b"D");
        let receipt = rewind(&server).await;
        if expected_restored {
            assert_eq!(receipt["restored"][0], file.to_string_lossy().as_ref());
            assert_eq!(std::fs::read(&file).unwrap(), b"A");
        } else {
            assert_eq!(receipt["conflicted"][0], file.to_string_lossy().as_ref());
            assert_eq!(std::fs::read(&file).unwrap(), b"D");
        }
        println!("native_gateway write/checkpoint/restore=real source_principal_session_binding=verified");
        server.stop_and_clean().await;
    }
}
