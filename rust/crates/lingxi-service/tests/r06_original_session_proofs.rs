//! 原279条中的会话证明：复用既有真实服务夹具，只补原要求的正反检查。
#![allow(dead_code)]

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

enum ScriptStep {
    Final(String),
    Hold {
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    },
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
        if let Ok(root) = std::env::var("LINGXI_W03_EVIDENCE") {
            use std::io::Write as _;
            let mut evidence = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(PathBuf::from(root).join(format!(
                    "original-t03-provider-{}.jsonl",
                    std::process::id()
                )))
                .unwrap();
            writeln!(evidence,"{}",serde_json::json!({"call_id":call.to_string(),"input_submission":_input.submission,"input_system_prompt":_input.system_prompt,"input_turn":_input.turn,"full_actual_provider_input_debug":format!("{_input:?}")})).unwrap();
        }
        let step = self.script.lock().unwrap().pop_front();
        let ctx_at_issue = ctx.clone();
        let call_id = call.to_string();
        Box::pin(async move {
            match step {
                Some(ScriptStep::Hold { entered, release }) => {
                    entered.notify_one();
                    release.notified().await;
                    ProviderTurnResult::of_ctx(
                        &ctx_at_issue,
                        ProviderTurn::Final {
                            message: NormalizedMessage {
                                role: "assistant".into(),
                                content: vec![ContentBlock::Text {
                                    text: "已释放运行中的子会话".into(),
                                }],
                                model_call_id: Some(ModelCallId::new(call_id)),
                            },
                        },
                    )
                }
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
    let home = synthetic_home(tag);
    start_server_at(tag, home, provider).await
}

async fn start_server_at(tag: &str, home: PathBuf, provider: Arc<ScriptedProvider>) -> TestServer {
    let layout = prepare_layout(&home).expect("prepare layout");
    let home = home.canonicalize().unwrap();
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
            workspace_root: Some(workspace),
            ..ServiceDeps::default()
        },
    )
    .await
    .expect("bootstrap");
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
    if let Ok(root) = std::env::var("LINGXI_W03_EVIDENCE") {
        std::fs::write(PathBuf::from(root).join(format!("original-t03-ready-{}.json",addr.port())),
            serde_json::to_vec_pretty(&serde_json::json!({"case_tag":tag,"actual_home":home,"home_source":"Cli","ready_address":addr.to_string(),"actual_db":state.storage().db_path(),"provider_boundary":"确定性响应替身；原生产 Run 与 SQLite 未替换"})).unwrap()).unwrap();
    }
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

    async fn stop_and_retain(self) -> PathBuf {
        self.stop.send(()).expect("server still listening");
        tokio::time::timeout(Duration::from_secs(10), self.handle)
            .await
            .expect("server shuts down within timeout")
            .expect("server task join")
            .expect("clean serve result");
        if let Ok(root) = std::env::var("LINGXI_W03_EVIDENCE") {
            std::fs::write(PathBuf::from(root).join(format!("original-t03-exit-{}.json",self.addr.port())),
                serde_json::to_vec_pretty(&serde_json::json!({"actual_home":self.home,"actual_db":self.state.storage().db_path(),"actual_serve_task_exited":true,"home_retained_local_only":true})).unwrap()).unwrap();
        }
        self.home
    }

    async fn stop_and_clean(self) {
        let home = self.stop_and_retain().await;
        if std::env::var("LINGXI_W03_EVIDENCE").is_err() {
            let _ = std::fs::remove_dir_all(home);
        }
    }

    async fn post(&self, path: &str, body: &str) -> (u16, String) {
        http(
            &self.addr,
            "POST",
            path,
            &[("Authorization", &self.bearer())],
            Some(body),
        )
        .await
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

async fn read_session(server: &TestServer, sid: &str) -> serde_json::Value {
    let (status, body) = server
        .get(&format!("/lingxi/v1/sessions/find?sessionId={sid}"))
        .await;
    assert_eq!(status, 200, "{body}");
    serde_json::from_str(&body).unwrap()
}

#[tokio::test]
async fn original_session_rename_preserves_title_after_invalid_foreign_and_storage_refusal() {
    let server = start_server("original-rename", ScriptedProvider::new(vec![])).await;
    create_session(&server, "rename-owned", "原标题").await;
    create_session(&server, "rename-other", "另一会话").await;
    let other = read_session(&server, "rename-other").await;
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/rename",
            r#"{"sessionId":"rename-owned","title":"  新标题  "}"#,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let baseline = read_session(&server, "rename-owned").await;
    assert_eq!(baseline["title"], "新标题");
    for (body, expected) in [
        (r#"{"sessionId":"rename-owned","title":"  "}"#, 400),
        (r#"{"sessionId":"missing","title":"不应创建"}"#, 404),
        (r#"{"sessionId":"rename-owned","title":false}"#, 400),
    ] {
        let (status, response) = server.post("/lingxi/v1/sessions/rename", body).await;
        assert_eq!(status, expected, "{response}");
        let error: serde_json::Value = serde_json::from_str(&response).unwrap();
        assert!(
            error["code"]
                .as_str()
                .is_some_and(|value| !value.is_empty()),
            "拒绝必须有明确错误代码：{response}"
        );
        assert!(
            error["message"]
                .as_str()
                .is_some_and(|value| !value.is_empty()),
            "拒绝必须有明确错误说明：{response}"
        );
        assert_eq!(read_session(&server, "rename-owned").await, baseline);
    }
    let foreign = server.mint_device_token("foreign-original-proof").await;
    let bearer = format!("Bearer {foreign}");
    let (status, body) = http(
        &server.addr,
        "POST",
        "/lingxi/v1/sessions/rename",
        &[("Authorization", &bearer)],
        Some(r#"{"sessionId":"rename-owned","title":"越权"}"#),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(read_session(&server, "rename-owned").await, baseline);
    let db = rusqlite::Connection::open(server.state.storage().db_path()).unwrap();
    db.execute_batch("CREATE TRIGGER original_rename_abort BEFORE UPDATE OF title ON sessions WHEN OLD.session_id='rename-owned' BEGIN SELECT RAISE(ABORT,'原验收合成存储失败'); END;").unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/rename",
            r#"{"sessionId":"rename-owned","title":"不得伪装保存"}"#,
        )
        .await;
    assert_eq!(status, 500, "{body}");
    let error: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(
        error["code"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "存储失败必须有明确错误代码：{body}"
    );
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "存储失败必须有明确错误说明：{body}"
    );
    assert_eq!(read_session(&server, "rename-owned").await, baseline);
    db.execute_batch("DROP TRIGGER original_rename_abort")
        .unwrap();
    assert_eq!(read_session(&server, "rename-other").await, other);
    let (status, _) = server
        .post(
            "/lingxi/v1/sessions/rename",
            r#"{"sessionId":"rename-owned","title":"恢复正对照"}"#,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        read_session(&server, "rename-owned").await["title"],
        "恢复正对照"
    );
    println!(
        "{}",
        serde_json::json!({"originalLeaf":"R00-T02-LA-FDB0FE6528EE","invalid":400,"foreign":403,"storageFailure":500,"rejectedWritesUnchanged":true,"otherSessionUnchanged":true,"restoredPositive":true})
    );
    drop(db);
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_checkpoint_delete_preserves_scope_after_invalid_foreign_and_storage_refusal() {
    let server = start_server(
        "original-checkpoint-delete",
        ScriptedProvider::new(vec![ScriptStep::Final("原检查点锚点".into())]),
    )
    .await;
    create_session(&server, "checkpoint-owned", "检查点会话").await;
    let run = server.execute("checkpoint-owned", "真实原检查点").await;
    for name in ["delete-target", "keep-other"] {
        let (status, body) = server
            .post(
                "/lingxi/v1/sessions/checkpoint-owned/checkpoints",
                &serde_json::json!({"name":name,"targetMessageId":format!("{run}-final")})
                    .to_string(),
            )
            .await;
        assert_eq!(status, 200, "{body}");
    }
    let (_, body) = server
        .get("/lingxi/v1/sessions/checkpoint-owned/checkpoints")
        .await;
    let before: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(before["checkpoints"].as_array().unwrap().len(), 2);
    let (status, body) = server.get("/lingxi/v1/sessions/missing/checkpoints").await;
    assert_eq!(status, 404, "{body}");
    for (body, expected) in [(r#"{"name":"missing"}"#, 404), (r#"{"name":false}"#, 400)] {
        let (status, response) = server
            .post(
                "/lingxi/v1/sessions/checkpoint-owned/checkpoints/delete",
                body,
            )
            .await;
        assert_eq!(status, expected, "{response}");
        let (_, current) = server
            .get("/lingxi/v1/sessions/checkpoint-owned/checkpoints")
            .await;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&current).unwrap(),
            before
        );
    }
    let foreign = server
        .mint_device_token("foreign-original-checkpoint")
        .await;
    let bearer = format!("Bearer {foreign}");
    let (status, body) = http(
        &server.addr,
        "POST",
        "/lingxi/v1/sessions/checkpoint-owned/checkpoints/delete",
        &[("Authorization", &bearer)],
        Some(r#"{"name":"delete-target"}"#),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    let (status, body) = http(
        &server.addr,
        "GET",
        "/lingxi/v1/sessions/checkpoint-owned/checkpoints",
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(status, 403, "{body}");
    let db = rusqlite::Connection::open(server.state.storage().db_path()).unwrap();
    // 在独立夹具内暂时隐藏检查点表，验证实际读取失败不变成空列表成功。
    db.execute_batch("ALTER TABLE checkpoints RENAME TO original_hidden_checkpoints")
        .unwrap();
    let (status, body) = server
        .get("/lingxi/v1/sessions/checkpoint-owned/checkpoints")
        .await;
    assert_eq!(status, 500, "{body}");
    db.execute_batch("ALTER TABLE original_hidden_checkpoints RENAME TO checkpoints")
        .unwrap();
    let (_, current) = server
        .get("/lingxi/v1/sessions/checkpoint-owned/checkpoints")
        .await;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&current).unwrap(),
        before
    );
    db.execute_batch("CREATE TRIGGER original_checkpoint_abort BEFORE DELETE ON checkpoints WHEN OLD.session_id='checkpoint-owned' BEGIN SELECT RAISE(ABORT,'原验收合成删除失败'); END;").unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/checkpoint-owned/checkpoints/delete",
            r#"{"name":"delete-target"}"#,
        )
        .await;
    assert_eq!(status, 500, "{body}");
    let (_, current) = server
        .get("/lingxi/v1/sessions/checkpoint-owned/checkpoints")
        .await;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&current).unwrap(),
        before
    );
    db.execute_batch("DROP TRIGGER original_checkpoint_abort")
        .unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/checkpoint-owned/checkpoints/delete",
            r#"{"name":"delete-target"}"#,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["ok"],
        true
    );
    let (_, current) = server
        .get("/lingxi/v1/sessions/checkpoint-owned/checkpoints")
        .await;
    let current: serde_json::Value = serde_json::from_str(&current).unwrap();
    assert_eq!(current["checkpoints"].as_array().unwrap().len(), 1);
    assert_eq!(current["checkpoints"][0]["name"], "keep-other");
    let (status, _) = server
        .post(
            "/lingxi/v1/sessions/checkpoint-owned/checkpoints/delete",
            r#"{"name":"delete-target"}"#,
        )
        .await;
    assert_eq!(status, 404);
    println!(
        "{}",
        serde_json::json!({"originalLeaf":"R00-T02-LA-1AEDB2670EB9","invalid":400,"foreign":403,"storageFailure":500,"failedDeletePreservedBothCheckpoints":true,"successDeletedOnlyTarget":true,"secondDelete":404})
    );
    drop(db);
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_rewind_preview_reports_anchor_and_files_without_writing_any_state() {
    let server = start_server(
        "original-rewind-preview",
        ScriptedProvider::new(vec![ScriptStep::Final("预览锚点".into())]),
    )
    .await;
    let work = server.home.join("workspace").canonicalize().unwrap();
    let file = work.join("preview.txt");
    std::fs::write(&file, "预览原件").unwrap();
    let (status, body) = server.post("/lingxi/v1/sessions", &serde_json::json!({"sessionId":"preview-owned","agentId":"agent","title":"预览","authorizedFolders":[work]}).to_string()).await;
    assert_eq!(status, 201, "{body}");
    let run = server.execute("preview-owned", "预览这一轮").await;
    let (status, body) = server.post("/lingxi/v1/sessions/preview-owned/checkpoints", &serde_json::json!({"name":"preview","targetMessageId":format!("{run}-final"),"filePaths":[file]}).to_string()).await;
    assert_eq!(status, 200, "{body}");
    server.state.set_rollback_file_changes(true);
    let branch = server.branch_ids("preview-owned").await;
    let session = read_session(&server, "preview-owned").await;
    let bytes = std::fs::read(&file).unwrap();
    let metadata = std::fs::metadata(&file).unwrap().modified().unwrap();
    let event_count = server
        .state
        .storage()
        .query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM key_events", vec![])
        .await
        .unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/preview-owned/rewind/preview",
            r#"{"checkpoint":"preview","restoreFiles":true}"#,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let preview: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(preview["turnInputMessageId"], format!("user:{run}"));
    assert_eq!(preview["newHeadMessageId"], serde_json::Value::Null);
    assert_eq!(
        preview["files"],
        serde_json::json!([{"filePath":file,"verdict":"skipped"}])
    );
    for (body, expected) in [
        (r#"{"checkpoint":"missing","restoreFiles":true}"#, 404),
        (r#"{"checkpoint":false}"#, 400),
    ] {
        let (status, response) = server
            .post("/lingxi/v1/sessions/preview-owned/rewind/preview", body)
            .await;
        assert_eq!(status, expected, "{response}");
    }
    let foreign = server.mint_device_token("foreign-original-preview").await;
    let bearer = format!("Bearer {foreign}");
    let (status, body) = http(
        &server.addr,
        "POST",
        "/lingxi/v1/sessions/preview-owned/rewind/preview",
        &[("Authorization", &bearer)],
        Some(r#"{"checkpoint":"preview","restoreFiles":true}"#),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    let db = rusqlite::Connection::open(server.state.storage().db_path()).unwrap();
    db.execute_batch("ALTER TABLE checkpoints RENAME TO original_preview_hidden_checkpoints")
        .unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/preview-owned/rewind/preview",
            r#"{"checkpoint":"preview","restoreFiles":true}"#,
        )
        .await;
    assert_eq!(status, 500, "{body}");
    db.execute_batch("ALTER TABLE original_preview_hidden_checkpoints RENAME TO checkpoints")
        .unwrap();
    assert_eq!(server.branch_ids("preview-owned").await, branch);
    assert_eq!(read_session(&server, "preview-owned").await, session);
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    assert_eq!(
        std::fs::metadata(&file).unwrap().modified().unwrap(),
        metadata
    );
    assert_eq!(
        server
            .state
            .storage()
            .query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM key_events", vec![])
            .await
            .unwrap(),
        event_count
    );
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/preview-owned/rewind/preview",
            r#"{"checkpoint":"preview","restoreFiles":true}"#,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap(),
        preview
    );
    println!(
        "{}",
        serde_json::json!({"originalLeaf":"R00-T02-LA-477DA3D558B8","preview":preview,"invalid":400,"foreign":403,"storageFailure":500,"branchSessionEventsAndFileUnchanged":true,"restoredPositive":true})
    );
    drop(db);
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_input_drafts_concurrent_updates_and_actual_caps_preserve_other_owners() {
    let server = start_server("original-input-drafts-caps", ScriptedProvider::new(vec![])).await;
    for id in ["draft-concurrent-a", "draft-concurrent-b", "draft-cap-new"] {
        create_session(&server, id, id).await;
    }
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let mut handles = Vec::new();
    for id in ["draft-concurrent-a", "draft-concurrent-b"] {
        let barrier = barrier.clone();
        let addr = server.addr;
        let bearer = server.bearer();
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            http(
                &addr,
                "PUT",
                "/lingxi/v1/input-drafts",
                &[("Authorization", &bearer)],
                Some(
                    &serde_json::json!({"surface":"electron","sessionId":id,"text":id}).to_string(),
                ),
            )
            .await
        }));
    }
    barrier.wait().await;
    for handle in handles {
        let (status, body) = handle.await.unwrap();
        assert_eq!(status, 200, "{body}");
    }
    let (status, body) = server.get("/lingxi/v1/input-drafts?surface=electron").await;
    assert_eq!(status, 200, "{body}");
    let concurrent: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        concurrent["sessions"]["draft-concurrent-a"]["text"],
        "draft-concurrent-a"
    );
    assert_eq!(
        concurrent["sessions"]["draft-concurrent-b"]["text"],
        "draft-concurrent-b"
    );
    let big_doc = serde_json::json!({"surface":"pwa","scope":"home","text":"文本必须保留","doc":{"content":"d".repeat(512*1024)}});
    let (status, body) = http(
        &server.addr,
        "PUT",
        "/lingxi/v1/input-drafts",
        &[("Authorization", &server.bearer())],
        Some(&big_doc.to_string()),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let adjusted: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(adjusted["docDropped"], true);
    assert_eq!(adjusted["textTruncated"], false);
    let (status, body) = server.get("/lingxi/v1/input-drafts?surface=pwa").await;
    assert_eq!(status, 200, "{body}");
    let pwa: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(pwa["home"]["text"], "文本必须保留");
    assert!(pwa["home"].get("doc").is_none());
    let big_text = serde_json::json!({"surface":"pwa","scope":"home","text":format!("{}😀suffix","x".repeat(512*1024-1))});
    let (status, body) = http(
        &server.addr,
        "PUT",
        "/lingxi/v1/input-drafts",
        &[("Authorization", &server.bearer())],
        Some(&big_text.to_string()),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["textTruncated"],
        true
    );
    let (status, body) = server.get("/lingxi/v1/input-drafts?surface=pwa").await;
    assert_eq!(status, 200, "{body}");
    let pwa: serde_json::Value = serde_json::from_str(&body).unwrap();
    let truncated = pwa["home"]["text"].as_str().unwrap();
    assert_eq!(truncated, "x".repeat(512 * 1024 - 1));
    assert!(truncated.encode_utf16().count() <= 512 * 1024);
    let path = server.home.join("input-drafts.v1.json");
    let foreign = server.mint_device_token("draft-cap-foreign").await;
    let foreign_bearer = format!("Bearer {foreign}");
    let (status, body) = http(
        &server.addr,
        "POST",
        "/lingxi/v1/sessions",
        &[("Authorization", &foreign_bearer)],
        Some(
            r#"{"sessionId":"draft-foreign-new","agentId":"foreign-agent","title":"外国主体草稿"}"#,
        ),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let mut slots = serde_json::Map::new();
    // Prepare200 real stored sessions without spending the HTTP test's request budget.
    for i in 0..200 {
        let id = format!("draft-cap-{i:03}");
        lingxi_adapters::storage::session_admin::create_session(
            server.state.storage(),
            &id,
            "agent",
            "draft-root-owner",
            &id,
            None,
            &[],
            true,
            1,
        )
        .await
        .unwrap();
        slots.insert(
            id,
            serde_json::json!({"text":format!("已保存{i}"),"updatedAt":i+1}),
        );
    }
    let seeded = serde_json::json!({"version":1,"surfaces":{"electron":{"home":null,"sessions":slots},"pwa":pwa}});
    std::fs::write(&path, serde_json::to_vec(&seeded).unwrap()).unwrap();
    let before = std::fs::read(&path).unwrap();
    let (status, body) = http(
        &server.addr,
        "PUT",
        "/lingxi/v1/input-drafts",
        &[("Authorization", &foreign_bearer)],
        Some(r#"{"surface":"electron","sessionId":"draft-foreign-new","text":"不准淘汰他人"}"#),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let (status, body) = http(
        &server.addr,
        "PUT",
        "/lingxi/v1/input-drafts",
        &[("Authorization", &server.bearer())],
        Some(r#"{"surface":"electron","sessionId":"draft-cap-new","text":"新增并淘汰最旧"}"#),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["evictedSessionIds"],
        serde_json::json!(["draft-cap-000"])
    );
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        saved["surfaces"]["electron"]["sessions"]
            .as_object()
            .unwrap()
            .len(),
        200
    );
    assert!(saved["surfaces"]["electron"]["sessions"]
        .get("draft-cap-000")
        .is_none());
    assert_eq!(
        saved["surfaces"]["electron"]["sessions"]["draft-cap-new"]["text"],
        "新增并淘汰最旧"
    );
    assert_eq!(saved["surfaces"]["pwa"], pwa);
    println!(
        "{}",
        serde_json::json!({"concurrentBarrierWritesPreserved":true,"actualEntryUnitLimit":512*1024,"explicitDocDropAndTextTruncation":true,"noSplitScalar":true,"actual200SlotLimit":true,"oldestOwnedEvicted":true,"foreignEvictionRejectedWithoutWrite":true,"otherSurfacePreserved":true})
    );
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_input_drafts_persist_home_and_sessions_per_surface_and_delete_only_target() {
    let server = start_server("original-input-drafts", ScriptedProvider::new(vec![])).await;
    for sid in ["draft-a", "draft-b"] {
        create_session(&server, sid, sid).await;
    }
    let doc =
        serde_json::json!({"type":"doc","content":[{"type":"paragraph","text":"富文本草稿"}]});
    for body in [
        serde_json::json!({"surface":"electron","scope":"home","text":"桌面主页","doc":doc}),
        serde_json::json!({"surface":"pwa","scope":"home","text":"手机主页"}),
        serde_json::json!({"surface":"electron","sessionId":"draft-a","text":"会话A"}),
        serde_json::json!({"surface":"electron","sessionId":"draft-b","text":"会话B"}),
        serde_json::json!({"surface":"pwa","sessionId":"draft-a","text":"手机会话A"}),
    ] {
        let (status, result) = http(
            &server.addr,
            "PUT",
            "/lingxi/v1/input-drafts",
            &[("Authorization", &server.bearer())],
            Some(&body.to_string()),
        )
        .await;
        assert_eq!(status, 200, "{result}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&result).unwrap()["ok"],
            true
        );
    }
    let (status, body) = server.get("/lingxi/v1/input-drafts?surface=electron").await;
    assert_eq!(status, 200, "{body}");
    let before: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(before["home"]["text"], "桌面主页");
    assert_eq!(before["home"]["doc"], doc);
    assert!(before["home"]["updatedAt"].as_u64().unwrap() > 0);
    assert_eq!(before["sessions"]["draft-a"]["text"], "会话A");
    assert_eq!(before["sessions"]["draft-b"]["text"], "会话B");
    assert_eq!(before["sessions"].as_object().unwrap().len(), 2);
    let (status, body) = server.get("/lingxi/v1/input-drafts?surface=%70wa").await;
    assert_eq!(status, 200, "{body}");
    let pwa: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(pwa["home"]["text"], "手机主页");
    assert_eq!(pwa["sessions"]["draft-a"]["text"], "手机会话A");
    assert_eq!(pwa["sessions"].as_object().unwrap().len(), 1);
    let path = server.home.join("input-drafts.v1.json");
    let disk = std::fs::read(&path).unwrap();
    let disk_json: serde_json::Value = serde_json::from_slice(&disk).unwrap();
    assert_eq!(disk_json["version"], 1);
    assert_eq!(disk_json["surfaces"]["electron"], before);
    assert_eq!(disk_json["surfaces"]["pwa"], pwa);
    let home = server.stop_and_retain().await;
    let server = start_server_at(
        "original-input-drafts-restart",
        home,
        ScriptedProvider::new(vec![]),
    )
    .await;
    let (status, body) = server.get("/lingxi/v1/input-drafts?surface=electron").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap(),
        before
    );
    assert_eq!(std::fs::read(&path).unwrap(), disk, "读取/重启不重写草稿");
    for body in [
        serde_json::json!({"surface":"electron","scope":"home","text":""}),
        serde_json::json!({"surface":"electron","sessionId":"draft-a","text":" \n\t"}),
    ] {
        let (status, response) = http(
            &server.addr,
            "PUT",
            "/lingxi/v1/input-drafts",
            &[("Authorization", &server.bearer())],
            Some(&body.to_string()),
        )
        .await;
        assert_eq!(status, 200, "{response}");
    }
    let (status, body) = server.get("/lingxi/v1/input-drafts?surface=electron").await;
    assert_eq!(status, 200, "{body}");
    let cleared: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(cleared["home"].is_null());
    assert!(cleared["sessions"].get("draft-a").is_none());
    assert_eq!(
        cleared["sessions"]["draft-b"],
        before["sessions"]["draft-b"]
    );
    let (status, body) = server.get("/lingxi/v1/input-drafts?surface=pwa").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap(),
        pwa
    );
    let cleared_disk = std::fs::read(&path).unwrap();
    let home = server.stop_and_retain().await;
    let server = start_server_at(
        "original-input-drafts-cleared-restart",
        home,
        ScriptedProvider::new(vec![]),
    )
    .await;
    let (status, body) = server.get("/lingxi/v1/input-drafts?surface=electron").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap(),
        cleared
    );
    assert_eq!(std::fs::read(path).unwrap(), cleared_disk);
    println!(
        "{}",
        serde_json::json!({"originalLeaves":["R00-T02-LA-081FACA79F2C","R00-T02-LA-1B712E189DDF","R00-T02-LA-F829AEA5931C"],"twoRealRestarts":true,"textDocUpdatedAt":true,"exactSurfaceBuckets":true,"readDoesNotWrite":true,"emptyAndWhitespaceDeleteOnlyTarget":true,"otherSessionSurfaceUnchanged":true})
    );
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_input_drafts_reject_bad_scope_and_authorization_and_preserve_failed_storage() {
    let server = start_server(
        "original-input-drafts-errors",
        ScriptedProvider::new(vec![]),
    )
    .await;
    create_session(&server, "draft-owned", "受保护草稿").await;
    let (status, body) = http(
        &server.addr,
        "PUT",
        "/lingxi/v1/input-drafts",
        &[("Authorization", &server.bearer())],
        Some(r#"{"surface":"electron","sessionId":"draft-owned","text":"必须保留"}"#),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let path = server.home.join("input-drafts.v1.json");
    let before = std::fs::read(&path).unwrap();
    for query in [
        "",
        "?surface=invalid",
        "?surface=Electron",
        "?surface=electron&surface=pwa",
        "?surface=electron&unexpected=true",
    ] {
        let (status, body) = server.get(&format!("/lingxi/v1/input-drafts{query}")).await;
        assert_eq!(status, 400, "{body}");
        assert!(serde_json::from_str::<serde_json::Value>(&body)
            .unwrap()
            .get("home")
            .is_none());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    for body in [
        r#"{"surface":"invalid","scope":"home","text":""}"#,
        r#"{"surface":"electron","scope":"other","text":""}"#,
        r#"{"surface":"electron","text":""}"#,
        r#"{"surface":"electron","scope":"home","sessionId":"draft-owned","text":""}"#,
        r#"{"surface":"electron","sessionId":"draft-owned","text":null}"#,
        r#"{"surface":"electron","sessionId":"draft-owned","text":5}"#,
        r#"{"surface":"electron","sessionId":"draft-owned","text":"" ,"doc":[]}"#,
        r#"{"surface":"electron","sessionPath":"/not/a/session","text":""}"#,
        r#"{"surface":"electron","sessionId":"draft-owned","text":"","unknown":true}"#,
    ] {
        let (status, response) = http(
            &server.addr,
            "PUT",
            "/lingxi/v1/input-drafts",
            &[("Authorization", &server.bearer())],
            Some(body),
        )
        .await;
        assert_eq!(status, 400, "{response}");
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    let foreign = server.mint_device_token("draft-foreign").await;
    let foreign_bearer = format!("Bearer {foreign}");
    for method in ["GET", "PUT"] {
        let route = if method == "GET" {
            "/lingxi/v1/input-drafts?surface=electron"
        } else {
            "/lingxi/v1/input-drafts"
        };
        let body =
            (method == "PUT").then_some(r#"{"surface":"electron","scope":"home","text":""}"#);
        let (status, response) = http(&server.addr, method, route, &[], body).await;
        assert_eq!(status, 401, "{response}");
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    let (status, response) = http(
        &server.addr,
        "PUT",
        "/lingxi/v1/input-drafts",
        &[("Authorization", &foreign_bearer)],
        Some(r#"{"surface":"electron","sessionId":"draft-owned","text":""}"#),
    )
    .await;
    assert_eq!(status, 403, "{response}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let (status, response) = http(
        &server.addr,
        "PUT",
        "/lingxi/v1/input-drafts",
        &[("Authorization", &server.bearer())],
        Some(r#"{"surface":"electron","sessionId":"missing-draft-session","text":""}"#),
    )
    .await;
    assert_eq!(status, 404, "{response}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let (status, response) = http(
        &server.addr,
        "GET",
        "/lingxi/v1/input-drafts?surface=electron",
        &[("Authorization", &foreign_bearer)],
        None,
    )
    .await;
    assert_eq!(status, 200, "{response}");
    let visible: serde_json::Value = serde_json::from_str(&response).unwrap();
    assert_eq!(visible["sessions"], serde_json::json!({}));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    std::fs::write(&path, b"{broken").unwrap();
    for method in ["GET", "PUT"] {
        let route = if method == "GET" {
            "/lingxi/v1/input-drafts?surface=electron"
        } else {
            "/lingxi/v1/input-drafts"
        };
        let body =
            (method == "PUT").then_some(r#"{"surface":"electron","scope":"home","text":""}"#);
        let (status, response) = http(
            &server.addr,
            method,
            route,
            &[("Authorization", &server.bearer())],
            body,
        )
        .await;
        assert_eq!(status, 500, "{response}");
        assert!(serde_json::from_str::<serde_json::Value>(&response)
            .unwrap()
            .get("ok")
            .is_none());
        assert_eq!(std::fs::read(&path).unwrap(), b"{broken");
    }
    std::fs::write(&path, &before).unwrap();
    let mut invalid_version: serde_json::Value = serde_json::from_slice(&before).unwrap();
    invalid_version["version"] = serde_json::json!(2);
    let invalid_bytes = serde_json::to_vec(&invalid_version).unwrap();
    std::fs::write(&path, &invalid_bytes).unwrap();
    let (status, response) = server.get("/lingxi/v1/input-drafts?surface=electron").await;
    assert_eq!(status, 500, "{response}");
    assert_eq!(std::fs::read(&path).unwrap(), invalid_bytes);
    std::fs::write(&path, &before).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let file_mode = std::fs::metadata(&path).unwrap().permissions().mode();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        let unreadable = server.get("/lingxi/v1/input-drafts?surface=electron").await;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(file_mode)).unwrap();
        assert_eq!(unreadable.0, 500, "{}", unreadable.1);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let mode = std::fs::metadata(&server.home)
            .unwrap()
            .permissions()
            .mode();
        std::fs::set_permissions(&server.home, std::fs::Permissions::from_mode(0o555)).unwrap();
        let mut failures = Vec::new();
        for body in [
            r#"{"surface":"electron","sessionId":"draft-owned","text":""}"#,
            r#"{"surface":"electron","scope":"home","text":"写失败不能成功"}"#,
        ] {
            failures.push(
                http(
                    &server.addr,
                    "PUT",
                    "/lingxi/v1/input-drafts",
                    &[("Authorization", &server.bearer())],
                    Some(body),
                )
                .await,
            );
        }
        std::fs::set_permissions(&server.home, std::fs::Permissions::from_mode(mode)).unwrap();
        for result in failures {
            assert_eq!(result.0, 500, "{}", result.1);
            assert!(serde_json::from_str::<serde_json::Value>(&result.1)
                .unwrap()
                .get("ok")
                .is_none());
        }
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    let (status, response) = http(
        &server.addr,
        "PUT",
        "/lingxi/v1/input-drafts",
        &[("Authorization", &server.bearer())],
        Some(r#"{"surface":"electron","sessionId":"draft-owned","text":""}"#),
    )
    .await;
    assert_eq!(status, 200, "{response}");
    let (status, response) = server.get("/lingxi/v1/input-drafts?surface=electron").await;
    assert_eq!(status, 200, "{response}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&response).unwrap()["sessions"],
        serde_json::json!({})
    );
    println!(
        "{}",
        serde_json::json!({"originalLeaves":["R00-T02-LA-081FACA79F2C","R00-T02-LA-1B712E189DDF","R00-T02-LA-F829AEA5931C"],"invalid":400,"missingAuth":401,"foreignSession":403,"missingSession":404,"corruptAndActualUnwritableStorage":500,"refusalsPreserveBytes":true,"repairedRetry":true,"unresolvedSessionPathNotClaimed":true})
    );
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_archived_list_aggregates_agents_and_projects_first_visible_user_text() {
    let server = start_server(
        "original-archived-projection",
        ScriptedProvider::new(
            (0..6)
                .map(|_| ScriptStep::Final("助手回复不是首条用户消息".into()))
                .collect(),
        ),
    )
    .await;
    for (sid, agent) in [
        ("archived-first", "agent-first"),
        ("archived-second", "agent-second"),
        ("archived-hidden", "agent-first"),
        ("archived-empty", "agent-empty"),
        ("archived-scalar-edge", "agent-second"),
        ("still-active", "agent-second"),
    ] {
        let (status, body) = server
            .post(
                "/lingxi/v1/sessions",
                &serde_json::json!({"sessionId":sid,"agentId":agent,"title":sid}).to_string(),
            )
            .await;
        assert_eq!(status, 201, "{body}");
    }
    let long_first = "原首消息内容".repeat(30);
    server.execute("archived-first", &long_first).await;
    server
        .execute("archived-first", "后来的用户消息不能替代首条")
        .await;
    let astral_first = "😀".repeat(70);
    server.execute("archived-second", &astral_first).await;
    server
        .execute(
            "archived-hidden",
            "[hana_reference]\n内部参考不能成为列表文本\n[/hana_reference]\n\n[KnowledgeResearchContext]\n截断的内部研究包裹",
        )
        .await;
    server
        .execute(
            "archived-hidden",
            "[hana_reminder at 2026-10-08 10:30]\n内部提醒\n[/hana_reminder]\n\n[KnowledgeContext]\n内部证据\n[/KnowledgeContext]\n\n首条可见文本",
        )
        .await;
    server
        .execute("archived-scalar-edge", &format!("{}😀后文", "x".repeat(99)))
        .await;
    let raw_first = server
        .state
        .storage()
        .query_one_text(
            "SELECT content_json FROM messages WHERE session_id=?1 AND role='user' ORDER BY seq LIMIT 1",
            vec!["archived-first".into()],
        )
        .await
        .unwrap();
    for sid in [
        "archived-first",
        "archived-second",
        "archived-hidden",
        "archived-empty",
        "archived-scalar-edge",
    ] {
        let (status, body) = server
            .post(
                "/lingxi/v1/sessions/archive",
                &serde_json::json!({"sessionId":sid}).to_string(),
            )
            .await;
        assert_eq!(status, 200, "{body}");
    }
    let (status, body) = server.get("/lingxi/v1/sessions/archived").await;
    assert_eq!(status, 200, "{body}");
    let result: serde_json::Value = serde_json::from_str(&body).unwrap();
    let sessions = result["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 5);
    let first = sessions
        .iter()
        .find(|r| r["sessionId"] == "archived-first")
        .unwrap();
    assert_eq!(first["agentId"], "agent-first");
    assert_eq!(
        first["firstMessage"],
        long_first.chars().take(100).collect::<String>()
    );
    assert_eq!(
        first["firstMessage"]
            .as_str()
            .unwrap()
            .encode_utf16()
            .count(),
        100
    );
    let second = sessions
        .iter()
        .find(|r| r["sessionId"] == "archived-second")
        .unwrap();
    assert_eq!(second["agentId"], "agent-second");
    assert_eq!(second["firstMessage"], "😀".repeat(50));
    assert_eq!(
        second["firstMessage"]
            .as_str()
            .unwrap()
            .encode_utf16()
            .count(),
        100
    );
    let hidden = sessions
        .iter()
        .find(|r| r["sessionId"] == "archived-hidden")
        .unwrap();
    assert_eq!(hidden["firstMessage"], "首条可见文本");
    assert_eq!(
        sessions
            .iter()
            .find(|r| r["sessionId"] == "archived-empty")
            .unwrap()["firstMessage"],
        ""
    );
    assert!(!sessions.iter().any(|r| r["sessionId"] == "still-active"));
    let scalar_edge = sessions
        .iter()
        .find(|r| r["sessionId"] == "archived-scalar-edge")
        .unwrap();
    assert_eq!(scalar_edge["firstMessage"], "x".repeat(99));
    assert!(
        scalar_edge["firstMessage"]
            .as_str()
            .unwrap()
            .encode_utf16()
            .count()
            <= 100
    );
    assert_eq!(
        server
            .state
            .storage()
            .query_one_text(
                "SELECT content_json FROM messages WHERE session_id=?1 AND role='user' ORDER BY seq LIMIT 1",
                vec!["archived-first".into()],
            )
            .await
            .unwrap(),
        raw_first,
        "列表投影不得截断或重写原始模型历史"
    );
    println!(
        "{}",
        serde_json::json!({"originalLeaf":"R00-T02-LA-43FFB75F252C","agentAggregation":3,"archivedRows":5,"firstUserNotLastOrAssistant":true,"utf16Limit":100,"noSplitScalar":true,"internalEnvelopesExcluded":true,"emptyPreview":"","historyUnchanged":true})
    );
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_archived_list_refusals_and_corrupt_history_do_not_return_empty_success() {
    let server = start_server(
        "original-archived-refusals",
        ScriptedProvider::new(vec![ScriptStep::Final("归档响应".into())]),
    )
    .await;
    create_session(&server, "archived-owned", "本地主人的归档").await;
    server.execute("archived-owned", "保留首条用户文本").await;
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/archive",
            r#"{"sessionId":"archived-owned"}"#,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let foreign = server.mint_device_token("archived-foreign-user").await;
    let foreign_bearer = format!("Bearer {foreign}");
    let (status, body) = http(
        &server.addr,
        "POST",
        "/lingxi/v1/sessions",
        &[("Authorization", &foreign_bearer)],
        Some(r#"{"sessionId":"archived-foreign","agentId":"foreign-agent","title":"另一主体的归档"}"#),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let (status, body) = http(
        &server.addr,
        "POST",
        "/lingxi/v1/sessions/archive",
        &[("Authorization", &foreign_bearer)],
        Some(r#"{"sessionId":"archived-foreign"}"#),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = server.get("/lingxi/v1/sessions/archived").await;
    assert_eq!(status, 200, "{body}");
    let before: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(before["sessions"].as_array().unwrap().len(), 2);
    let (status, body) = http(
        &server.addr,
        "GET",
        "/lingxi/v1/sessions/archived",
        &[("Authorization", &foreign_bearer)],
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let scoped: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(scoped["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(scoped["sessions"][0]["sessionId"], "archived-foreign");
    let (status, body) = server
        .post(
            "/lingxi/v1/devices/credentials",
            r#"{"userId":"archived-settings-only","scopes":["settings"]}"#,
        )
        .await;
    assert_eq!(status, 201, "{body}");
    let denied = format!(
        "Bearer {}",
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["secret"]
            .as_str()
            .unwrap()
    );
    for (headers, expected) in [
        (vec![], 401),
        (
            vec![("Authorization", "Bearer invalid-archived-credential")],
            401,
        ),
        (vec![("Authorization", denied.as_str())], 403),
    ] {
        let (status, body) = http(
            &server.addr,
            "GET",
            "/lingxi/v1/sessions/archived",
            &headers,
            None,
        )
        .await;
        assert_eq!(status, expected, "{body}");
        assert!(serde_json::from_str::<serde_json::Value>(&body)
            .unwrap()
            .get("sessions")
            .is_none());
    }
    let db = rusqlite::Connection::open(server.state.storage().db_path()).unwrap();
    let original: String = db.query_row(
        "SELECT content_json FROM messages WHERE session_id='archived-owned' AND role='user' ORDER BY seq LIMIT 1",
        [], |r| r.get(0),
    ).unwrap();
    db.execute_batch("ALTER TABLE messages RENAME TO original_archived_hidden_messages")
        .unwrap();
    assert_eq!(
        lingxi_adapters::storage::session_admin::list_archived_sessions(
            server.state.storage(),
            None
        )
        .await
        .unwrap()
        .len(),
        2,
        "清理所用元数据读取不应新增消息历史依赖"
    );
    let (status, body) = server.get("/lingxi/v1/sessions/archived").await;
    assert_eq!(status, 500, "{body}");
    assert!(serde_json::from_str::<serde_json::Value>(&body)
        .unwrap()
        .get("sessions")
        .is_none());
    db.execute_batch("ALTER TABLE original_archived_hidden_messages RENAME TO messages")
        .unwrap();
    db.execute(
        "UPDATE messages SET content_json='{invalid' WHERE session_id='archived-owned' AND role='user'",
        [],
    ).unwrap();
    let (status, body) = server.get("/lingxi/v1/sessions/archived").await;
    assert_eq!(status, 500, "{body}");
    assert!(serde_json::from_str::<serde_json::Value>(&body)
        .unwrap()
        .get("sessions")
        .is_none());
    db.execute(
        "UPDATE messages SET content_json=?1 WHERE session_id='archived-owned' AND role='user'",
        [&original],
    )
    .unwrap();
    db.execute(
        r#"UPDATE messages SET content_json='{"text":null}' WHERE session_id='archived-owned' AND role='user'"#,
        [],
    ).unwrap();
    let (status, body) = server.get("/lingxi/v1/sessions/archived").await;
    assert_eq!(status, 500, "{body}");
    assert!(serde_json::from_str::<serde_json::Value>(&body)
        .unwrap()
        .get("sessions")
        .is_none());
    db.execute(
        "UPDATE messages SET content_json=?1 WHERE session_id='archived-owned' AND role='user'",
        [&original],
    )
    .unwrap();
    let (status, body) = server.get("/lingxi/v1/sessions/archived").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap(),
        before
    );
    println!(
        "{}",
        serde_json::json!({"originalLeaf":"R00-T02-LA-43FFB75F252C","ownerFiltering":true,"missingAndInvalidCredential":401,"insufficientScope":403,"missingHistoryTable":500,"corruptHistory":500,"notEmptySuccess":true,"restoredReadUnchanged":true})
    );
    drop(db);
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_session_memory_only_active_live_agents_persist_and_refuse_explicitly() {
    let server = start_server("original-memory-active", ScriptedProvider::new(vec![])).await;
    create_session(&server, "memory-active", "活动会话").await;
    create_session(&server, "memory-unrelated", "不受影响").await;
    let path = "/lingxi/v1/sessions/memory-active/memory";
    let (status, body) = server.get(path).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["memoryEnabled"],
        true
    );
    let unrelated = read_session(&server, "memory-unrelated").await;
    for enabled in [false, true, false] {
        let (status, body) = http(
            &server.addr,
            "PATCH",
            path,
            &[("Authorization", &server.bearer())],
            Some(&serde_json::json!({"memoryEnabled":enabled}).to_string()),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap()["memoryEnabled"],
            enabled
        );
        let (status, body) = server.get(path).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap()["memoryEnabled"],
            enabled
        );
        assert_eq!(read_session(&server, "memory-unrelated").await, unrelated);
    }
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/archive",
            r#"{"sessionId":"memory-active"}"#,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let archived = read_session(&server, "memory-active").await;
    let direct = lingxi_adapters::storage::session_admin::set_memory_enabled(
        server.state.storage(),
        "memory-active",
        true,
    )
    .await;
    assert!(
        matches!(direct, Err(lingxi_kernel::ports::StorageError::Conflict { ref detail }) if detail == "session_memory_inactive")
    );
    assert_eq!(read_session(&server, "memory-active").await, archived);
    for method in ["GET", "PATCH"] {
        let (status, body) = http(
            &server.addr,
            method,
            path,
            &[("Authorization", &server.bearer())],
            (method == "PATCH").then_some(r#"{"memoryEnabled":true}"#),
        )
        .await;
        assert_eq!(status, 403, "{body}");
        assert_eq!(read_session(&server, "memory-active").await, archived);
    }
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/restore",
            r#"{"sessionId":"memory-active"}"#,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let restored = read_session(&server, "memory-active").await;
    let dir = server.home.join("agents/agent");
    std::fs::create_dir_all(&dir).unwrap();
    let marker = dir.join(".deleted-agent.json");
    std::fs::write(&marker, "{}").unwrap();
    for method in ["GET", "PATCH"] {
        let (status, body) = http(
            &server.addr,
            method,
            path,
            &[("Authorization", &server.bearer())],
            (method == "PATCH").then_some(r#"{"memoryEnabled":true}"#),
        )
        .await;
        assert_eq!(status, 409, "{body}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap()["reason"],
            "agent_deleted"
        );
        assert_eq!(read_session(&server, "memory-active").await, restored);
        assert_eq!(std::fs::read(&marker).unwrap(), b"{}");
    }
    std::fs::remove_file(&marker).unwrap();
    let foreign = server.mint_device_token("foreign-memory-active").await;
    for method in ["GET", "PATCH"] {
        for (token, expected) in [
            (&foreign, 403),
            (&String::from("invalid-memory-token"), 401),
        ] {
            let (status, body) = http(
                &server.addr,
                method,
                path,
                &[("Authorization", &format!("Bearer {token}"))],
                (method == "PATCH").then_some(r#"{"memoryEnabled":true}"#),
            )
            .await;
            assert_eq!(status, expected, "{body}");
        }
    }
    for invalid in [
        r#"{}"#,
        r#"{"memoryEnabled":"true"}"#,
        r#"{"memoryEnabled":null}"#,
        r#"{"memoryEnabled":false,"unexpected":true}"#,
    ] {
        let (status, body) = http(
            &server.addr,
            "PATCH",
            path,
            &[("Authorization", &server.bearer())],
            Some(invalid),
        )
        .await;
        assert_eq!(status, 400, "{body}");
        assert_eq!(read_session(&server, "memory-active").await, restored);
    }
    for method in ["GET", "PATCH"] {
        let (status, body) = http(
            &server.addr,
            method,
            "/lingxi/v1/sessions/missing-memory/memory",
            &[("Authorization", &server.bearer())],
            (method == "PATCH").then_some(r#"{"memoryEnabled":true}"#),
        )
        .await;
        assert_eq!(status, 404, "{body}");
    }
    let db = rusqlite::Connection::open(server.state.storage().db_path()).unwrap();
    db.execute_batch("CREATE TRIGGER original_memory_abort BEFORE UPDATE OF memory_enabled ON sessions BEGIN SELECT RAISE(ABORT,'memory save failed'); END;").unwrap();
    let (status, body) = http(
        &server.addr,
        "PATCH",
        path,
        &[("Authorization", &server.bearer())],
        Some(r#"{"memoryEnabled":true}"#),
    )
    .await;
    assert_eq!(status, 500, "{body}");
    assert_eq!(read_session(&server, "memory-active").await, restored);
    db.execute_batch("DROP TRIGGER original_memory_abort")
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000)).unwrap();
        let result = server.get(path).await;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(mode)).unwrap();
        assert_eq!(result.0, 500, "{}", result.1);
        assert!(!result.1.contains("memoryEnabled"));
        assert_eq!(read_session(&server, "memory-active").await, restored);
    }
    db.execute_batch("ALTER TABLE sessions RENAME TO original_memory_unavailable")
        .unwrap();
    let result = server.get(path).await;
    db.execute_batch("ALTER TABLE original_memory_unavailable RENAME TO sessions")
        .unwrap();
    assert_eq!(result.0, 500, "{}", result.1);
    assert!(!result.1.contains("memoryEnabled"));
    let (status, body) = server.get(path).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["memoryEnabled"],
        false
    );
    assert_eq!(read_session(&server, "memory-unrelated").await, unrelated);
    drop(db);
    let home = server.stop_and_retain().await;
    let server = start_server_at(
        "original-memory-restarted",
        home,
        ScriptedProvider::new(vec![]),
    )
    .await;
    let (status, body) = server.get(path).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["memoryEnabled"],
        false
    );
    let (status, body) = http(
        &server.addr,
        "PATCH",
        path,
        &[("Authorization", &server.bearer())],
        Some(r#"{"memoryEnabled":true}"#),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        read_session(&server, "memory-active").await["memoryEnabled"],
        true
    );
    println!(
        "{}",
        serde_json::json!({"originalLeaves":["R00-T02-LA-51905CECDD1B","R00-T02-LA-9616596B5720"],"actualBoolSaveRead":true,"archivedRefused":403,"deletedAgentRefused":409,"malformed":400,"foreign":403,"invalidAuth":401,"missing":404,"actualSaveReadFailures":500,"failedWritesUnchanged":true,"restoredRead":true})
    );
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_pin_deleted_agent_refuses_without_writes_and_allows_unpin() {
    let server = start_server("original-pin-deleted-agent", ScriptedProvider::new(vec![])).await;
    create_session(&server, "deleted-agent-pin", "保留已删助手的会话").await;
    create_session(&server, "live-agent-pin", "未删助手").await;
    let agent_dir = server.home.join("agents/agent");
    std::fs::create_dir_all(&agent_dir).unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/pin",
            r#"{"sessionId":"deleted-agent-pin","pinned":true}"#,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let before = read_session(&server, "deleted-agent-pin").await;
    let tombstone = agent_dir.join(".deleted-agent.json");
    let bytes = br#"{"agentId":"agent","deletedAt":"2026-10-13T07:19:21Z"}"#;
    std::fs::write(&tombstone, bytes).unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/pin-order",
            r#"{"sessionIds":["deleted-agent-pin","live-agent-pin"]}"#,
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(read_session(&server, "deleted-agent-pin").await, before);
    assert!(read_session(&server, "live-agent-pin").await["pinnedAtUnixMs"].is_null());
    for sid in ["deleted-agent-pin", "live-agent-pin"] {
        let snapshot = read_session(&server, sid).await;
        let (status, body) = server
            .post(
                "/lingxi/v1/sessions/pin",
                &serde_json::json!({"sessionId":sid,"pinned":true}).to_string(),
            )
            .await;
        assert_eq!(status, 409, "{body}");
        let error: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(error["error"], "agent_deleted");
        assert_eq!(error["reason"], "agent_deleted");
        assert_eq!(read_session(&server, sid).await, snapshot);
        assert_eq!(std::fs::read(&tombstone).unwrap(), bytes);
    }
    assert_eq!(read_session(&server, "deleted-agent-pin").await, before);
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions",
            r#"{"sessionId":"other-agent-pin","agentId":"other-agent","title":"独立助手"}"#,
        )
        .await;
    assert_eq!(status, 201, "{body}");
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/pin",
            r#"{"sessionId":"other-agent-pin","pinned":true}"#,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(read_session(&server, "other-agent-pin").await["pinnedAtUnixMs"].is_u64());
    for input in [
        r#"{"sessionId":"deleted-agent-pin","pinned":"true"}"#,
        r#"{"sessionId":"deleted-agent-pin"}"#,
        r#"{"sessionId":"deleted-agent-pin","pinned":null}"#,
    ] {
        let (status, body) = server.post("/lingxi/v1/sessions/pin", input).await;
        assert_eq!(status, 400, "{body}");
        assert_eq!(read_session(&server, "deleted-agent-pin").await, before);
    }
    let foreign = server.mint_device_token("foreign-deleted-agent-pin").await;
    let bearer = format!("Bearer {foreign}");
    let (status, body) = http(
        &server.addr,
        "POST",
        "/lingxi/v1/sessions/pin",
        &[("Authorization", &bearer)],
        Some(r#"{"sessionId":"deleted-agent-pin","pinned":true}"#),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    assert!(!body.contains("agent_deleted"));
    let (status, body) = http(
        &server.addr,
        "POST",
        "/lingxi/v1/sessions/pin",
        &[],
        Some(r#"{"sessionId":"deleted-agent-pin","pinned":true}"#),
    )
    .await;
    assert_eq!(status, 401, "{body}");
    assert_eq!(read_session(&server, "deleted-agent-pin").await, before);
    std::fs::write(&tombstone, "{").unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/pin",
            r#"{"sessionId":"deleted-agent-pin","pinned":true}"#,
        )
        .await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(std::fs::read(&tombstone).unwrap(), b"{");
    assert_eq!(read_session(&server, "deleted-agent-pin").await, before);
    std::fs::write(&tombstone, bytes).unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/pin",
            r#"{"sessionId":"deleted-agent-pin","pinned":false}"#,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let cleared = read_session(&server, "deleted-agent-pin").await;
    assert!(cleared["pinnedAtUnixMs"].is_null() && cleared["pinOrder"].is_null());
    assert_eq!(std::fs::read(&tombstone).unwrap(), bytes);
    std::fs::remove_file(&tombstone).unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/pin",
            r#"{"sessionId":"live-agent-pin","pinned":true}"#,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(read_session(&server, "live-agent-pin").await["pinnedAtUnixMs"].is_u64());
    let before = read_session(&server, "deleted-agent-pin").await;
    let db = rusqlite::Connection::open(server.state.storage().db_path()).unwrap();
    db.execute_batch("CREATE TRIGGER original_pin_write_abort BEFORE UPDATE OF pinned_at_unix_ms ON sessions WHEN OLD.session_id='deleted-agent-pin' BEGIN SELECT RAISE(ABORT,'pin persistence failed'); END;").unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/pin",
            r#"{"sessionId":"deleted-agent-pin","pinned":true}"#,
        )
        .await;
    assert_eq!(status, 500, "{body}");
    assert_eq!(read_session(&server, "deleted-agent-pin").await, before);
    db.execute_batch("DROP TRIGGER original_pin_write_abort")
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let before = read_session(&server, "deleted-agent-pin").await;
        let mode = std::fs::metadata(&agent_dir).unwrap().permissions().mode();
        std::fs::set_permissions(&agent_dir, std::fs::Permissions::from_mode(0o000)).unwrap();
        let response = server
            .post(
                "/lingxi/v1/sessions/pin",
                r#"{"sessionId":"deleted-agent-pin","pinned":true}"#,
            )
            .await;
        std::fs::set_permissions(&agent_dir, std::fs::Permissions::from_mode(mode)).unwrap();
        assert_eq!(response.0, 500, "{}", response.1);
        assert_eq!(read_session(&server, "deleted-agent-pin").await, before);
    }
    println!(
        "{}",
        serde_json::json!({"originalLeaf":"R00-T02-LA-0D4F0EFC9DAE","deletedAgentPin":409,"unpinStillAllowed":true,"markerAndSessionUnchangedOnRefusal":true,"realUnreadableDeletionState":500,"restoredLiveAgentPin":true})
    );
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_pin_order_checks_each_owner_and_rolls_back_failed_reordering() {
    let server = start_server("original-pin-order", ScriptedProvider::new(vec![])).await;
    for sid in ["pin-a", "pin-b"] {
        create_session(&server, sid, sid).await;
        let (status, body) = server
            .post(
                "/lingxi/v1/sessions/pin",
                &serde_json::json!({"sessionId":sid,"pinned":true}).to_string(),
            )
            .await;
        assert_eq!(status, 200, "{body}");
        let row = read_session(&server, sid).await;
        assert!(row["pinnedAtUnixMs"].is_u64());
        assert!(row["pinOrder"].is_i64());
    }
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/pin-order",
            r#"{"sessionIds":["pin-b","pin-a"]}"#,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let before_a = read_session(&server, "pin-a").await;
    let before_b = read_session(&server, "pin-b").await;
    assert!(before_b["pinOrder"].as_i64().unwrap() < before_a["pinOrder"].as_i64().unwrap());
    for (body, expected) in [
        (r#"{"sessionIds":["pin-a","pin-a"]}"#, 400),
        (r#"{"sessionIds":["pin-a","missing"]}"#, 404),
    ] {
        let (status, response) = server.post("/lingxi/v1/sessions/pin-order", body).await;
        assert_eq!(status, expected, "{response}");
        assert_eq!(read_session(&server, "pin-a").await, before_a);
        assert_eq!(read_session(&server, "pin-b").await, before_b);
    }
    let foreign = server.mint_device_token("foreign-pin-order").await;
    let bearer = format!("Bearer {foreign}");
    let (status, body) = http(
        &server.addr,
        "POST",
        "/lingxi/v1/sessions",
        &[("Authorization", &bearer)],
        Some(r#"{"sessionId":"foreign-pin-own","agentId":"agent","title":"外部主体自己的会话"}"#),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let (status, body) = http(
        &server.addr,
        "POST",
        "/lingxi/v1/sessions/pin",
        &[("Authorization", &bearer)],
        Some(r#"{"sessionId":"foreign-pin-own","pinned":true}"#),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let foreign_before = read_session(&server, "foreign-pin-own").await;
    let (status, body) = http(
        &server.addr,
        "POST",
        "/lingxi/v1/sessions/pin-order",
        &[("Authorization", &bearer)],
        Some(r#"{"sessionIds":["foreign-pin-own","pin-a"]}"#),
    )
    .await;
    assert_eq!(status, 403, "逐项检查不能只验证第一项：{body}");
    assert_eq!(
        read_session(&server, "foreign-pin-own").await,
        foreign_before
    );
    assert_eq!(read_session(&server, "pin-a").await, before_a);
    let db = rusqlite::Connection::open(server.state.storage().db_path()).unwrap();
    db.execute_batch("CREATE TRIGGER original_pin_order_abort BEFORE UPDATE OF pin_order ON sessions WHEN OLD.session_id='pin-b' BEGIN SELECT RAISE(ABORT,'原验收第二项更新失败'); END;").unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/pin-order",
            r#"{"sessionIds":["pin-a","pin-b"]}"#,
        )
        .await;
    assert_eq!(status, 500, "{body}");
    assert_eq!(
        read_session(&server, "pin-a").await,
        before_a,
        "第二项失败时不得只改第一项"
    );
    assert_eq!(read_session(&server, "pin-b").await, before_b);
    db.execute_batch("DROP TRIGGER original_pin_order_abort")
        .unwrap();
    let (status, _) = server
        .post(
            "/lingxi/v1/sessions/pin-order",
            r#"{"sessionIds":["pin-a","pin-b"]}"#,
        )
        .await;
    assert_eq!(status, 200);
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/pin",
            r#"{"sessionId":"pin-a","pinned":false}"#,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let unpinned = read_session(&server, "pin-a").await;
    assert!(unpinned["pinnedAtUnixMs"].is_null() && unpinned["pinOrder"].is_null());
    println!(
        "{}",
        serde_json::json!({"originalLeaf":"R00-T02-LA-2ED065BED168","duplicate":400,"missing":404,"mixedOwners":403,"secondUpdateFailure":500,"failedReordersUnchanged":true,"restoredPositive":true,"unpinClearsTimeAndOrder":true})
    );
    drop(db);
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_session_search_returns_scoped_title_content_and_explicit_read_failures() {
    let server = start_server(
        "original-search",
        ScriptedProvider::new(vec![ScriptStep::Final(
            "BODY-SEARCH-NEEDLE 真实回答片段".into(),
        )]),
    )
    .await;
    create_session(&server, "search-owned", "TITLE-SEARCH-NEEDLE 本地主人").await;
    server.execute("search-owned", "搜索原要求").await;
    for (phase, query) in [
        ("title", "TITLE-SEARCH-NEEDLE"),
        ("content", "BODY-SEARCH-NEEDLE"),
    ] {
        let (status, body) = server
            .get(&format!(
                "/lingxi/v1/sessions/search?q={query}&phase={phase}&limit=1"
            ))
            .await;
        assert_eq!(status, 200, "{body}");
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        let results = value["results"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["sessionId"], "search-owned");
        assert_eq!(results[0]["matchKind"], phase);
        if phase == "title" {
            assert!(results[0]["title"].as_str().unwrap().contains(query));
            assert!(results[0]["snippet"].as_str().unwrap().contains(query));
        } else {
            assert!(results[0]["snippet"].as_str().unwrap().contains(query));
        }
    }
    let (status, body) = server.get("/lingxi/v1/sessions/search?q=").await;
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["results"],
        serde_json::json!([])
    );
    create_session(
        &server,
        "search-long-title",
        &format!(
            "{}LONG-TITLE-MARKER{}",
            "前缀".repeat(100),
            "后缀".repeat(100)
        ),
    )
    .await;
    let (status, body) = server
        .get("/lingxi/v1/sessions/search?q=LONG-TITLE-MARKER&phase=title&limit=1")
        .await;
    assert_eq!(status, 200);
    let long: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(long["results"][0]["sessionId"], "search-long-title");
    let snippet = long["results"][0]["snippet"].as_str().unwrap();
    assert!(snippet.contains("LONG-TITLE-MARKER"));
    assert!(snippet.chars().count() < 160);
    let baseline = read_session(&server, "search-owned").await;
    for query in [
        "q=needle&phase=invalid".to_string(),
        "q=needle&limit=abc".to_string(),
        format!("q={}", "x".repeat(600)),
    ] {
        let (status, body) = server
            .get(&format!("/lingxi/v1/sessions/search?{query}"))
            .await;
        assert_eq!(status, 400, "{body}");
        assert_eq!(read_session(&server, "search-owned").await, baseline);
    }
    let foreign = server.mint_device_token("foreign-search-owner").await;
    let bearer = format!("Bearer {foreign}");
    let (status,body)=http(&server.addr,"POST","/lingxi/v1/sessions",&[("Authorization",&bearer)],Some(r#"{"sessionId":"search-foreign","agentId":"agent","title":"TITLE-SEARCH-NEEDLE 另一主体"}"#)).await;
    assert_eq!(status, 201, "{body}");
    let (status, body) = http(
        &server.addr,
        "GET",
        "/lingxi/v1/sessions/search?q=TITLE-SEARCH-NEEDLE&phase=title",
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let results: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(results["results"].as_array().unwrap().len(), 1);
    assert_eq!(results["results"][0]["sessionId"], "search-foreign");
    let db = rusqlite::Connection::open(server.state.storage().db_path()).unwrap();
    let (_, body) = server
        .get("/lingxi/v1/sessions/search?q=BODY-SEARCH-NEEDLE&phase=content")
        .await;
    let before: serde_json::Value = serde_json::from_str(&body).unwrap();
    db.execute_batch("ALTER TABLE messages RENAME TO original_search_hidden_messages")
        .unwrap();
    let (status, body) = server
        .get("/lingxi/v1/sessions/search?q=BODY-SEARCH-NEEDLE&phase=content")
        .await;
    assert_eq!(status, 500, "读取失败不能当空搜索成功：{body}");
    db.execute_batch("ALTER TABLE original_search_hidden_messages RENAME TO messages")
        .unwrap();
    let (status, body) = server
        .get("/lingxi/v1/sessions/search?q=BODY-SEARCH-NEEDLE&phase=content")
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap(),
        before
    );
    assert_eq!(read_session(&server, "search-owned").await, baseline);
    println!(
        "{}",
        serde_json::json!({"originalLeaf":"R00-T02-LA-A18187567AF9","titleAndContent":true,"limit":1,"emptyQueryEmpty":true,"invalid":400,"foreignSeesOwnOnly":true,"storageFailure":500,"restoredResultsAndSessionUnchanged":true})
    );
    drop(db);
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_workspace_rollback_preference_persists_without_restoring_files_and_controls_real_gate(
) {
    let server = start_server(
        "original-rollback-preference",
        ScriptedProvider::new(vec![ScriptStep::Final("checkpoint response".into())]),
    )
    .await;
    let path = "/lingxi/v1/sessions/workspace-rollback";
    let (status, body) = server.get(path).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap(),
        serde_json::json!({"enabled": false})
    );
    let workspace = server.home.join("workspace");
    let file = workspace.join("preference.txt");
    std::fs::write(&file, "checkpoint contents").unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions",
            &serde_json::json!({
                "sessionId": "rollback-preference", "agentId": "agent", "title": "preference",
                "authorizedFolders": [workspace]
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 201, "{body}");
    let run = server
        .execute("rollback-preference", "create checkpoint")
        .await;
    let (status, body) = server.post("/lingxi/v1/sessions/rollback-preference/checkpoints", &serde_json::json!({
        "name": "before-preference", "targetMessageId": format!("{run}-final"), "filePaths": [file]
    }).to_string()).await;
    assert_eq!(status, 200, "{body}");
    std::fs::write(
        &file,
        "user changes must not be rolled back by a preference",
    )
    .unwrap();
    let bytes = std::fs::read(&file).unwrap();
    let modified = std::fs::metadata(&file).unwrap().modified().unwrap();
    let branch = server.branch_ids("rollback-preference").await;
    let snapshots = server
        .state
        .storage()
        .query_one_text(
            "SELECT CAST(COUNT(*) AS TEXT) FROM file_history_snapshots",
            vec![],
        )
        .await
        .unwrap();
    let preferences = server.home.join("user/preferences.json");
    std::fs::create_dir_all(preferences.parent().unwrap()).unwrap();
    std::fs::write(&preferences, r#"{"locale":"ja","unrelated":{"keep":1}}"#).unwrap();
    for enabled in [true, false, true] {
        let request = serde_json::json!({"enabled": enabled}).to_string();
        let (status, body) = http(
            &server.addr,
            "PUT",
            path,
            &[("Authorization", &server.bearer())],
            Some(&request),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap(),
            serde_json::json!({"ok": true, "enabled": enabled})
        );
        assert_eq!(server.state.rollback_file_changes_enabled(), enabled);
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&preferences).unwrap()).unwrap();
        assert_eq!(
            saved,
            serde_json::json!({"locale":"ja","unrelated":{"keep":1},"rollback_file_changes":enabled})
        );
        assert_eq!(std::fs::read(&file).unwrap(), bytes);
        assert_eq!(
            std::fs::metadata(&file).unwrap().modified().unwrap(),
            modified
        );
        assert_eq!(server.branch_ids("rollback-preference").await, branch);
        assert_eq!(
            server
                .state
                .storage()
                .query_one_text(
                    "SELECT CAST(COUNT(*) AS TEXT) FROM file_history_snapshots",
                    vec![]
                )
                .await
                .unwrap(),
            snapshots
        );
    }
    let home = server.stop_and_retain().await;
    let restarted = start_server_at(
        "original-rollback-preference-restart",
        home,
        ScriptedProvider::new(vec![]),
    )
    .await;
    let (status, body) = restarted.get(path).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["enabled"],
        true
    );
    let scoped_token = restarted
        .mint_device_token("rollback-authorized-chat")
        .await;
    let scoped_auth = format!("Bearer {scoped_token}");
    let (status, body) = http(
        &restarted.addr,
        "PUT",
        path,
        &[("Authorization", &scoped_auth)],
        Some(r#"{"enabled":true}"#),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    for _ in 0..5 {
        let local_auth = restarted.bearer();
        let local_headers = [("Authorization", local_auth.as_str())];
        let scoped_headers = [("Authorization", scoped_auth.as_str())];
        let (left, right) = tokio::join!(
            http(
                &restarted.addr,
                "PUT",
                path,
                &local_headers,
                Some(r#"{"enabled":true}"#)
            ),
            http(
                &restarted.addr,
                "PUT",
                path,
                &scoped_headers,
                Some(r#"{"enabled":false}"#)
            )
        );
        assert_eq!(left.0, 200, "{}", left.1);
        assert_eq!(right.0, 200, "{}", right.1);
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&preferences).unwrap()).unwrap();
        assert_eq!(
            Some(restarted.state.rollback_file_changes_enabled()),
            saved["rollback_file_changes"].as_bool()
        );
        assert_eq!(saved["unrelated"], serde_json::json!({"keep":1}));
    }
    let (status, body) = http(
        &restarted.addr,
        "PUT",
        path,
        &[("Authorization", &scoped_auth)],
        Some(r#"{"enabled":true}"#),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let preview = "/lingxi/v1/sessions/rollback-preference/rewind/preview";
    let request = r#"{"checkpoint":"before-preference","restoreFiles":true}"#;
    let (status, body) = restarted.post(preview, request).await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = http(
        &restarted.addr,
        "PUT",
        path,
        &[("Authorization", &restarted.bearer())],
        Some(r#"{"enabled":false}"#),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = restarted.post(preview, request).await;
    assert_eq!(status, 403, "{body}");
    assert!(body.contains("file_rollback_disabled"), "{body}");
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    assert_eq!(restarted.branch_ids("rollback-preference").await, branch);
    restarted.stop_and_clean().await;
}

#[tokio::test]
async fn original_workspace_rollback_preference_refusals_keep_saved_and_live_state_and_retry() {
    let server = start_server(
        "original-rollback-preference-errors",
        ScriptedProvider::new(vec![]),
    )
    .await;
    let path = "/lingxi/v1/sessions/workspace-rollback";
    let auth = server.bearer();
    let (status, body) = http(
        &server.addr,
        "PUT",
        path,
        &[("Authorization", &auth)],
        Some(r#"{"enabled":true}"#),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let preferences = server.home.join("user/preferences.json");
    let saved = std::fs::read(&preferences).unwrap();
    let file = server.home.join("workspace/refusal.txt");
    std::fs::write(&file, "untouched").unwrap();
    for request in [
        "{}",
        r#"{"enabled":"true"}"#,
        r#"{"enabled":null}"#,
        r#"{"enabled":0}"#,
        "[]",
        "null",
        "{",
    ] {
        let (status, body) = http(
            &server.addr,
            "PUT",
            path,
            &[("Authorization", &auth)],
            Some(request),
        )
        .await;
        assert_eq!(status, 400, "{body}");
        assert!(!body.contains(r#""ok":true"#), "{body}");
        assert_eq!(std::fs::read(&preferences).unwrap(), saved);
        assert!(server.state.rollback_file_changes_enabled());
    }
    let (status, body) = http(&server.addr, "PUT", path, &[], Some(r#"{"enabled":false}"#)).await;
    assert_eq!(status, 401, "{body}");
    let (status, body) = server
        .post(
            "/lingxi/v1/devices/credentials",
            r#"{"userId":"rollback-no-chat","scopes":["files.read"]}"#,
        )
        .await;
    assert_eq!(status, 201, "{body}");
    let secret = serde_json::from_str::<serde_json::Value>(&body).unwrap()["secret"]
        .as_str()
        .unwrap()
        .to_string();
    let denied_auth = format!("Bearer {secret}");
    for method in ["GET", "PUT"] {
        let (status, body) = http(
            &server.addr,
            method,
            path,
            &[("Authorization", &denied_auth)],
            (method == "PUT").then_some(r#"{"enabled":false}"#),
        )
        .await;
        assert_eq!(status, 403, "{body}");
        assert_eq!(std::fs::read(&preferences).unwrap(), saved);
        assert!(server.state.rollback_file_changes_enabled());
    }
    std::fs::remove_file(&preferences).unwrap();
    std::fs::create_dir(&preferences).unwrap();
    let (status, body) = http(
        &server.addr,
        "PUT",
        path,
        &[("Authorization", &auth)],
        Some(r#"{"enabled":false}"#),
    )
    .await;
    assert_eq!(status, 500, "{body}");
    assert!(body.contains("cannot read preferences.json"), "{body}");
    assert!(preferences.is_dir());
    assert!(server.state.rollback_file_changes_enabled());
    std::fs::remove_dir(&preferences).unwrap();
    for invalid in ["not JSON", "[]", r#"{"rollback_file_changes":"true"}"#] {
        std::fs::write(&preferences, invalid).unwrap();
        let (status, body) = http(
            &server.addr,
            "PUT",
            path,
            &[("Authorization", &auth)],
            Some(r#"{"enabled":false}"#),
        )
        .await;
        assert_eq!(status, 500, "{body}");
        assert_eq!(std::fs::read_to_string(&preferences).unwrap(), invalid);
        assert!(server.state.rollback_file_changes_enabled());
    }
    std::fs::write(&preferences, &saved).unwrap();
    let (status, body) = http(
        &server.addr,
        "PUT",
        path,
        &[("Authorization", &auth)],
        Some(r#"{"enabled":false}"#),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(!server.state.rollback_file_changes_enabled());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "untouched");
    std::fs::write(&preferences, "invalid startup preferences").unwrap();
    let home = server.stop_and_retain().await;
    let layout = prepare_layout(&home).unwrap();
    let outcome = ServiceState::bootstrap(
        ServiceConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            data_home: home.clone(),
            home_source: HomeSource::Cli,
            network_mode: NetworkMode::Loopback,
            shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
        },
        &layout,
    )
    .await;
    let error = match outcome {
        Ok(_) => panic!("corrupt saved preferences must not default to a successful bootstrap"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("cannot load workspace rollback preference"),
        "{error}"
    );
    if std::env::var("LINGXI_W03_EVIDENCE").is_err() {
        std::fs::remove_dir_all(home).unwrap();
    }
}

// 原已有会话操作的拒绝链；只向独立合成数据库注入故障，不修改产品实现。
#[tokio::test]
async fn original_retry_refusals_preserve_history_and_recover_after_actual_storage_failure() {
    let server = start_server(
        "original-retry-errors",
        ScriptedProvider::new(vec![ScriptStep::Final("原重试回答".into())]),
    )
    .await;
    create_session(&server, "retry-owned", "原重试").await;
    create_session(&server, "retry-other", "另一会话").await;
    let run = server.execute("retry-owned", "原重试输入").await;
    let branch = server.branch_ids("retry-owned").await;
    let session = read_session(&server, "retry-owned").await;
    let other = read_session(&server, "retry-other").await;
    for (path, body, expected) in [
        (
            "/lingxi/v1/sessions/retry-owned/turns/retry",
            r#"{"targetMessageId":false}"#,
            400,
        ),
        (
            "/lingxi/v1/sessions/retry-owned/turns/retry",
            r#"{"targetMessageId":"missing"}"#,
            400,
        ),
        ("/lingxi/v1/sessions/missing/turns/retry", "{}", 404),
    ] {
        let (status, response) = server.post(path, body).await;
        assert_eq!(status, expected, "{response}");
        assert_eq!(server.branch_ids("retry-owned").await, branch);
        assert_eq!(read_session(&server, "retry-owned").await, session);
    }
    let foreign = server.mint_device_token("foreign-retry-proof").await;
    let bearer = format!("Bearer {foreign}");
    let (status, body) = http(
        &server.addr,
        "POST",
        "/lingxi/v1/sessions/retry-owned/turns/retry",
        &[("Authorization", &bearer)],
        Some("{}"),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(server.branch_ids("retry-owned").await, branch);
    let db = rusqlite::Connection::open(server.state.storage().db_path()).unwrap();
    db.execute_batch("CREATE TRIGGER original_retry_abort BEFORE INSERT ON messages WHEN NEW.session_id='retry-owned' AND NEW.entry_type='hana-session-branch-reset' BEGIN SELECT RAISE(ABORT,'原重试合成存储失败'); END;").unwrap();
    let request = serde_json::json!({"targetMessageId":format!("{run}-final")}).to_string();
    let (status, body) = server
        .post("/lingxi/v1/sessions/retry-owned/turns/retry", &request)
        .await;
    assert_eq!(status, 500, "{body}");
    assert_eq!(server.branch_ids("retry-owned").await, branch);
    assert_eq!(read_session(&server, "retry-owned").await, session);
    assert_eq!(read_session(&server, "retry-other").await, other);
    db.execute_batch("DROP TRIGGER original_retry_abort")
        .unwrap();
    let (status, body) = server
        .post("/lingxi/v1/sessions/retry-owned/turns/retry", &request)
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["turnInputMessageId"],
        format!("user:{run}")
    );
    assert_ne!(server.branch_ids("retry-owned").await, branch);
    assert_eq!(read_session(&server, "retry-other").await, other);
    drop(db);
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_detach_archive_and_permanent_delete_refuse_atomically_and_do_not_restore_deleted_session(
) {
    let server = start_server(
        "original-archive-errors",
        ScriptedProvider::new(vec![ScriptStep::Final("原归档内容".into())]),
    )
    .await;
    create_session(&server, "archive-parent", "原父会话").await;
    let run = server.execute("archive-parent", "原父消息").await;
    let (status,body)=server.post("/lingxi/v1/sessions/archive-parent/fork",&serde_json::json!({"newSessionId":"archive-child","boundaryMessageId":format!("{run}-final")}).to_string()).await;
    assert_eq!(status, 200, "{body}");
    let parent = read_session(&server, "archive-parent").await;
    let child = read_session(&server, "archive-child").await;
    let branch = server.branch_ids("archive-parent").await;
    let child_branch = server.branch_ids("archive-child").await;
    for (body, expected) in [
        (r#"{"sessionId":"archive-parent","childMode":"wrong"}"#, 400),
        (
            r#"{"sessionId":"missing","childMode":"detach_children"}"#,
            404,
        ),
    ] {
        let (status, response) = server.post("/lingxi/v1/sessions/archive", body).await;
        assert_eq!(status, expected, "{response}");
        assert_eq!(read_session(&server, "archive-parent").await, parent);
        assert_eq!(read_session(&server, "archive-child").await, child);
    }
    let foreign = server.mint_device_token("foreign-archive-proof").await;
    let bearer = format!("Bearer {foreign}");
    let request = r#"{"sessionId":"archive-parent","childMode":"detach_children"}"#;
    let (status, body) = http(
        &server.addr,
        "POST",
        "/lingxi/v1/sessions/archive",
        &[("Authorization", &bearer)],
        Some(request),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    let db = rusqlite::Connection::open(server.state.storage().db_path()).unwrap();
    db.execute_batch("CREATE TRIGGER original_archive_abort BEFORE UPDATE OF lifecycle ON sessions WHEN OLD.session_id='archive-parent' BEGIN SELECT RAISE(ABORT,'原归档父项写失败'); END;").unwrap();
    let (status, body) = server.post("/lingxi/v1/sessions/archive", request).await;
    assert_eq!(status, 500, "{body}");
    assert_eq!(read_session(&server, "archive-parent").await, parent);
    assert_eq!(
        read_session(&server, "archive-child").await,
        child,
        "子项先解除谱系也必须随父写失败回滚"
    );
    assert_eq!(server.branch_ids("archive-parent").await, branch);
    db.execute_batch("DROP TRIGGER original_archive_abort")
        .unwrap();
    let (status, body) = server.post("/lingxi/v1/sessions/archive", request).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["detachedChildren"],
        1
    );
    assert_eq!(
        read_session(&server, "archive-parent").await["lifecycle"],
        "archived"
    );
    let detached = read_session(&server, "archive-child").await;
    assert_eq!(detached["lifecycle"], "active");
    assert!(detached["parentSessionId"].is_null());
    assert_eq!(server.branch_ids("archive-child").await, child_branch);
    let (status, body) = server.get("/lingxi/v1/sessions").await;
    assert_eq!(status, 200, "{body}");
    let list: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(list["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["sessionId"] == "archive-child"));
    assert!(!list["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["sessionId"] == "archive-parent"));
    let archived = read_session(&server, "archive-parent").await;
    for (body, expected) in [
        (r#"{"sessionId":false}"#, 400),
        (r#"{"sessionId":"missing"}"#, 404),
        (r#"{"sessionId":"archive-child"}"#, 409),
    ] {
        let (status, response) = server
            .post("/lingxi/v1/sessions/archived/delete", body)
            .await;
        assert_eq!(status, expected, "{response}");
        assert_eq!(read_session(&server, "archive-parent").await, archived);
        assert_eq!(read_session(&server, "archive-child").await, detached);
    }
    let (status, body) = http(
        &server.addr,
        "POST",
        "/lingxi/v1/sessions/archived/delete",
        &[("Authorization", &bearer)],
        Some(r#"{"sessionId":"archive-parent"}"#),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    db.execute_batch("CREATE TRIGGER original_delete_abort BEFORE DELETE ON sessions WHEN OLD.session_id='archive-parent' BEGIN SELECT RAISE(ABORT,'原删除最终写失败'); END;").unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/archived/delete",
            r#"{"sessionId":"archive-parent"}"#,
        )
        .await;
    assert_eq!(status, 500, "{body}");
    assert_eq!(read_session(&server, "archive-parent").await, archived);
    assert_eq!(
        server.branch_ids("archive-parent").await,
        branch,
        "清理后段失败必须保留原历史"
    );
    assert_eq!(read_session(&server, "archive-child").await, detached);
    db.execute_batch("DROP TRIGGER original_delete_abort")
        .unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/archived/delete",
            r#"{"sessionId":"archive-parent"}"#,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/restore",
            r#"{"sessionId":"archive-parent"}"#,
        )
        .await;
    assert_eq!(status, 404, "删除不能恢复：{body}");
    let (status, _) = server
        .get("/lingxi/v1/sessions/find?sessionId=archive-parent")
        .await;
    assert_eq!(status, 404);
    assert_eq!(read_session(&server, "archive-child").await, detached);
    assert_eq!(server.branch_ids("archive-child").await, child_branch);
    drop(db);
    server.stop_and_clean().await;
}

// 真正启动子会话运行，检查归档递归、跳过及失败计数和整批回滚。
#[tokio::test]
async fn original_archive_children_recurses_skips_running_and_preserves_failed_transaction() {
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let server = start_server(
        "original-archive-children",
        ScriptedProvider::new(vec![
            ScriptStep::Final("父会话原始历史".into()),
            ScriptStep::Hold {
                entered: entered.clone(),
                release: release.clone(),
            },
        ]),
    )
    .await;
    create_session(&server, "recursive-parent", "原父会话").await;
    create_session(&server, "recursive-other", "无关会话").await;
    let parent_run = server.execute("recursive-parent", "原归档输入").await;
    for (parent, child) in [
        ("recursive-parent", "recursive-child"),
        ("recursive-child", "recursive-grandchild"),
        ("recursive-parent", "recursive-running"),
    ] {
        let (status, body) = server.post(&format!("/lingxi/v1/sessions/{parent}/fork"),
            &serde_json::json!({"newSessionId":child,"boundaryMessageId":format!("{parent_run}-final")}).to_string()).await;
        assert_eq!(status, 200, "{body}");
    }
    let tracked = [
        "recursive-parent",
        "recursive-child",
        "recursive-grandchild",
        "recursive-running",
        "recursive-other",
    ];
    let mut before = Vec::new();
    let mut before_branches = Vec::new();
    for id in tracked {
        before.push(read_session(&server, id).await);
        before_branches.push(server.branch_ids(id).await);
    }
    let request = r#"{"sessionId":"recursive-parent","childMode":"archive_children"}"#;
    for bad in [
        r#"{"sessionId":"recursive-parent","childMode":false}"#,
        r#"{"sessionId":"recursive-parent","childMode":"invalid"}"#,
    ] {
        assert_eq!(server.post("/lingxi/v1/sessions/archive", bad).await.0, 400);
    }
    assert_eq!(
        server
            .post(
                "/lingxi/v1/sessions/archive",
                r#"{"sessionId":"missing","childMode":"archive_children"}"#
            )
            .await
            .0,
        404
    );
    let foreign = format!(
        "Bearer {}",
        server.mint_device_token("foreign-recursive").await
    );
    assert_eq!(
        http(
            &server.addr,
            "POST",
            "/lingxi/v1/sessions/archive",
            &[("Authorization", &foreign)],
            Some(request)
        )
        .await
        .0,
        403
    );
    let db = rusqlite::Connection::open(server.state.storage().db_path()).unwrap();
    db.execute_batch("CREATE TRIGGER original_recursive_failure BEFORE UPDATE OF lifecycle ON sessions WHEN OLD.session_id='recursive-grandchild' BEGIN SELECT RAISE(ABORT,'归档子孙写入失败'); END;").unwrap();
    let (status, body) = server.post("/lingxi/v1/sessions/archive", request).await;
    assert_eq!(status, 500, "{body}");
    let failed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(failed["details"]["archive"]["errors"], 1);
    assert_eq!(failed["details"]["archive"]["archivedChildren"], 0);
    assert_eq!(failed["details"]["archive"]["detachedChildren"], 0);
    assert_eq!(failed["details"]["rolledBack"], true);
    assert_ne!(failed["ok"], true);
    for (index, id) in tracked.iter().enumerate() {
        assert_eq!(
            read_session(&server, id).await,
            before[index],
            "任一子孙失败应整批回滚：{id}"
        );
    }
    db.execute_batch("DROP TRIGGER original_recursive_failure")
        .unwrap();
    // 公开执行请求等待运行结束才返回；并发发起请求，先等模型真正进入运行。
    let address = server.addr;
    let authorization = server.bearer();
    let running_response = tokio::spawn(async move {
        http(
            &address,
            "POST",
            "/lingxi/v1/sessions/recursive-running/execute",
            &[("Authorization", &authorization)],
            Some(r#"{"input":"保持实际运行"}"#),
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), entered.notified())
        .await
        .unwrap();
    let running_id = server
        .state
        .storage()
        .query_one_text(
            "SELECT run_id FROM runs WHERE session_id=?1 AND status='running'",
            vec!["recursive-running".into()],
        )
        .await
        .unwrap()
        .expect("已有真正运行中的子会话");
    assert_eq!(
        run_row(server.state.storage(), &running_id).await.0,
        "running"
    );
    let (status, body) = server.post("/lingxi/v1/sessions/archive", request).await;
    assert_eq!(status, 200, "{body}");
    let receipt: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(receipt["archivedChildren"], 2);
    assert_eq!(receipt["skippedStreamingChildren"], 1);
    assert_eq!(receipt["detachedChildren"], 0);
    assert_eq!(receipt["errors"], 0);
    for id in [
        "recursive-parent",
        "recursive-child",
        "recursive-grandchild",
    ] {
        assert_eq!(read_session(&server, id).await["lifecycle"], "archived");
        assert_eq!(
            server.branch_ids(id).await,
            before_branches[tracked.iter().position(|value| *value == id).unwrap()]
        );
        assert_eq!(
            server
                .post(
                    "/lingxi/v1/sessions/restore",
                    &serde_json::json!({"sessionId":id}).to_string()
                )
                .await
                .0,
            200
        );
        assert_eq!(read_session(&server, id).await["lifecycle"], "active");
    }
    assert_eq!(
        read_session(&server, "recursive-running").await["lifecycle"],
        "active"
    );
    assert_eq!(read_session(&server, "recursive-other").await, before[4]);
    assert_eq!(
        run_row(server.state.storage(), &running_id).await.0,
        "running"
    );
    release.notify_one();
    let (status, body) = tokio::time::timeout(Duration::from_secs(5), running_response)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["runId"],
        running_id
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if run_row(server.state.storage(), &running_id).await.0 == "completed" {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    println!(
        "原连同子会话归档局部证明：父与两层子孙可恢复；实际运行项跳过；拒绝和写失败无错误副作用；原错误计数仍未实现"
    );
    drop(db);
    server.stop_and_clean().await;
}

#[tokio::test]
async fn original_cleanup_expired_archive_removes_bytes_title_and_lifecycle_with_honest_failures() {
    let server = start_server(
        "original-cleanup",
        ScriptedProvider::new(vec![ScriptStep::Final("真实过期会话内容".into())]),
    )
    .await;
    for id in ["cleanup-expired", "cleanup-recent", "cleanup-active"] {
        create_session(&server, id, id).await;
    }
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/cleanup-expired/attachments/blob",
            r#"{"mime":"image/png","name":"原过期附件.png","dataBase64":"iVBORw0KGgo="}"#,
        )
        .await;
    assert_eq!(status, 201, "{body}");
    let file = serde_json::from_str::<serde_json::Value>(&body).unwrap()["file"].clone();
    let file_id = file["fileId"].as_str().unwrap();
    let path = PathBuf::from(file["realPath"].as_str().unwrap());
    let bytes = std::fs::read(&path).unwrap();
    let run = server
        .execute(
            "cleanup-expired",
            &format!("[SessionFile] {{\"fileId\":\"{file_id}\"}}"),
        )
        .await;
    for id in ["cleanup-expired", "cleanup-recent"] {
        assert_eq!(
            server
                .post(
                    "/lingxi/v1/sessions/archive",
                    &serde_json::json!({"sessionId":id}).to_string()
                )
                .await
                .0,
            200
        );
    }
    let db = rusqlite::Connection::open(server.state.storage().db_path()).unwrap();
    db.execute(
        "UPDATE sessions SET archived_at_unix_ms=1 WHERE session_id='cleanup-expired'",
        [],
    )
    .unwrap();
    let old = read_session(&server, "cleanup-expired").await;
    let recent = read_session(&server, "cleanup-recent").await;
    let active = read_session(&server, "cleanup-active").await;
    let old_branch = server.branch_ids("cleanup-expired").await;
    for bad in [
        r#"{"maxAgeDays":-1}"#,
        r#"{"maxAgeDays":"90"}"#,
        r#"{"maxAgeDays":false}"#,
    ] {
        let (status, body) = server.post("/lingxi/v1/sessions/cleanup", bad).await;
        assert_eq!(status, 400, "{body}");
        assert_eq!(read_session(&server, "cleanup-expired").await, old);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    assert_eq!(
        http(
            &server.addr,
            "POST",
            "/lingxi/v1/sessions/cleanup",
            &[],
            Some("{}")
        )
        .await
        .0,
        401
    );
    let foreign = format!(
        "Bearer {}",
        server.mint_device_token("foreign-cleanup").await
    );
    assert_eq!(http(&server.addr, "POST", "/lingxi/v1/sessions", &[("Authorization", &foreign)], Some(r#"{"sessionId":"cleanup-foreign","agentId":"agent","title":"另一所有人的过期会话"}"#)).await.0, 201);
    assert_eq!(
        http(
            &server.addr,
            "POST",
            "/lingxi/v1/sessions/archive",
            &[("Authorization", &foreign)],
            Some(r#"{"sessionId":"cleanup-foreign"}"#)
        )
        .await
        .0,
        200
    );
    db.execute(
        "UPDATE sessions SET archived_at_unix_ms=1 WHERE session_id='cleanup-foreign'",
        [],
    )
    .unwrap();
    let (status, body) = http(
        &server.addr,
        "POST",
        "/lingxi/v1/sessions/cleanup",
        &[("Authorization", &foreign)],
        Some("{}"),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["deleted"],
        1
    );
    assert_eq!(read_session(&server, "cleanup-expired").await, old);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    db.execute_batch("CREATE TRIGGER original_cleanup_failure BEFORE DELETE ON sessions WHEN OLD.session_id='cleanup-expired' BEGIN SELECT RAISE(ABORT,'清理过期会话末段失败'); END;").unwrap();
    let (status, body) = server.post("/lingxi/v1/sessions/cleanup", "{}").await;
    assert_eq!(status, 500, "{body}");
    assert_eq!(read_session(&server, "cleanup-expired").await, old);
    assert_eq!(server.branch_ids("cleanup-expired").await, old_branch);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(read_session(&server, "cleanup-recent").await, recent);
    assert_eq!(read_session(&server, "cleanup-active").await, active);
    db.execute_batch("DROP TRIGGER original_cleanup_failure")
        .unwrap();
    let (status, body) = server.post("/lingxi/v1/sessions/cleanup", "{}").await;
    assert_eq!(status, 200, "{body}");
    let receipt: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(receipt["deleted"], 1);
    assert_eq!(receipt["maxAgeDays"], 90);
    assert!(!path.exists(), "过期归档实际托管字节必须永久清除");
    assert_eq!(
        server
            .get("/lingxi/v1/sessions/find?sessionId=cleanup-expired")
            .await
            .0,
        404
    );
    assert_eq!(
        server
            .get(&format!(
                "/lingxi/v1/sessions/cleanup-expired/files/{file_id}"
            ))
            .await
            .0,
        404
    );
    assert_eq!(
        server
            .state
            .storage()
            .query_one_text(
                "SELECT title FROM sessions WHERE session_id=?1",
                vec!["cleanup-expired".into()]
            )
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        server
            .state
            .storage()
            .query_one_text("SELECT status FROM runs WHERE run_id=?1", vec![run])
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        server
            .state
            .storage()
            .query_one_text(
                "SELECT CAST(COUNT(*) AS TEXT) FROM messages WHERE session_id=?1",
                vec!["cleanup-expired".into()],
            )
            .await
            .unwrap()
            .as_deref(),
        Some("0"),
        "过期会话原始历史也必须永久清理",
    );
    assert_eq!(read_session(&server, "cleanup-recent").await, recent);
    assert_eq!(read_session(&server, "cleanup-active").await, active);
    assert_eq!(
        server.post("/lingxi/v1/sessions/cleanup", "{}").await.0,
        200
    );
    println!(
        "原过期清理完整检查：默认90天，只删过期归档；实际字节、标题和生命周期已清理；主体隔离、拒绝和实际存储失败副作用一致"
    );
    drop(db);
    server.stop_and_clean().await;
}
