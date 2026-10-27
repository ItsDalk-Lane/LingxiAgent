//! 会话补齐：真实HTTP、SQLite、旧JSONL定位及改前存档。
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
    start_server_at_with_gateway(tag, home, provider, None).await
}
async fn start_server_at_with_gateway(
    tag: &str,
    home: PathBuf,
    provider: Arc<ScriptedProvider>,
    gateway: Option<Arc<lingxi_service::toolgateway::ToolInvocationGateway>>,
) -> TestServer {
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
            tool_gateway: gateway,
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
async fn draft_native_and_legacy_locator_persist_only_id_and_refusals_preserve_other_sessions() {
    use serde_json::{json, Value};
    let server = start_server("draft-locators", ScriptedProvider::new(vec![])).await;
    create_session(&server, "draft-a", "草稿甲").await;
    create_session(&server, "draft-b", "草稿乙").await;
    let legacy = server
        .home
        .join("agents/agent/sessions/actual-legacy.jsonl");
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(&legacy, "{\"type\":\"session\",\"id\":\"draft-a\"}\n").unwrap();
    let draft_file = server.home.join("input-drafts.v1.json");
    for body in [
        json!({"surface":"electron","sessionId":"draft-b","text":"乙独立草稿"}),
        json!({"surface":"electron","sessionPath":legacy,"text":"旧位置恢复"}),
        json!({"surface":"electron","sessionPath":"native:draft-a","text":"当前原生位置","doc":{"type":"doc","content":[]}}),
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
    }
    let (status, body) = server.get("/lingxi/v1/input-drafts?surface=electron").await;
    assert_eq!(status, 200);
    let body: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["sessions"]["draft-a"]["text"], "当前原生位置");
    assert_eq!(body["sessions"]["draft-b"]["text"], "乙独立草稿");
    let baseline = std::fs::read(&draft_file).unwrap();
    let disk: Value = serde_json::from_slice(&baseline).unwrap();
    assert!(!disk.to_string().contains("actual-legacy.jsonl"));
    assert!(!disk.to_string().contains("native:draft-a"));
    for body in [
        json!({"surface":"electron","sessionPath":"/outside/forged.jsonl","text":"禁止"}),
        json!({"surface":"electron","sessionPath":"native:missing","text":"禁止"}),
        json!({"surface":"electron","sessionId":"draft-b","sessionPath":"native:draft-a","text":"禁止"}),
        json!({"surface":"electron","scope":"home","sessionPath":"native:draft-a","text":"禁止"}),
    ] {
        let (status, result) = http(
            &server.addr,
            "PUT",
            "/lingxi/v1/input-drafts",
            &[("Authorization", &server.bearer())],
            Some(&body.to_string()),
        )
        .await;
        assert_eq!(status, 400, "{result}");
        assert_eq!(std::fs::read(&draft_file).unwrap(), baseline);
    }
    let foreign = server.mint_device_token("foreign-user").await;
    let (status, _) = http(
        &server.addr,
        "PUT",
        "/lingxi/v1/input-drafts",
        &[("Authorization", &format!("Bearer {foreign}"))],
        Some(
            &json!({"surface":"electron","sessionPath":"native:draft-a","text":"禁止"}).to_string(),
        ),
    )
    .await;
    assert_eq!(status, 403);
    assert_eq!(std::fs::read(&draft_file).unwrap(), baseline);
    let home = server.stop_and_retain().await;
    let server = start_server_at("draft-restart", home, ScriptedProvider::new(vec![])).await;
    let (status, body) = server.get("/lingxi/v1/input-drafts?surface=electron").await;
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["sessions"]["draft-a"]["text"],
        "当前原生位置"
    );
    server.stop_and_retain().await;
}

#[tokio::test]
async fn user_edit_absolute_checkpoint_reasons_binary_bytes_and_failure_have_no_partial_archive() {
    use serde_json::{json, Value};
    let server = start_server("user-edit", ScriptedProvider::new(vec![])).await;
    let file = server.home.join("workspace/binary.dat");
    let before = vec![0, 255, 1, 42, 128];
    std::fs::write(&file, &before).unwrap();
    let archive = server.home.join("checkpoints");
    for reason in ["edit-start", "autosave-interval"] {
        let (status, body) = server
            .post(
                "/lingxi/v1/checkpoints/user-edit",
                &json!({"filePath":file,"reason":reason}).to_string(),
            )
            .await;
        assert_eq!(status, 200, "{body}");
        let body: Value = serde_json::from_str(&body).unwrap();
        let id = body["checkpoint"]["id"].as_str().unwrap();
        assert_eq!(body["checkpoint"]["reason"], reason);
        assert_eq!(body["checkpoint"]["path"], json!(file));
        let saved: Value =
            serde_json::from_slice(&std::fs::read(archive.join(format!("{id}.json"))).unwrap())
                .unwrap();
        assert_eq!(saved["tool"], "user-edit");
        assert_eq!(saved["source"], "user-edit");
        assert!(saved["sessionPath"].is_null());
        std::fs::write(&file, "变更").unwrap();
        let (status, body) = server
            .post(&format!("/lingxi/v1/checkpoints/{id}/restore"), "{}")
            .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(std::fs::read(&file).unwrap(), before);
    }
    let count = std::fs::read_dir(&archive).unwrap().count();
    for (path, reason, expected) in [
        ("relative.dat".into(), "edit-start", 400),
        (file.to_string_lossy().into_owned(), "model-change", 400),
        (
            server.home.join("missing").to_string_lossy().into_owned(),
            "edit-start",
            500,
        ),
        (
            server.home.join("workspace").to_string_lossy().into_owned(),
            "edit-start",
            500,
        ),
    ] {
        let (status, body) = server
            .post(
                "/lingxi/v1/checkpoints/user-edit",
                &json!({"filePath":path,"reason":reason}).to_string(),
            )
            .await;
        assert_eq!(status, expected, "{body}");
        assert_eq!(std::fs::read_dir(&archive).unwrap().count(), count);
        assert_eq!(std::fs::read(&file).unwrap(), before);
    }
    let foreign = server.mint_device_token("owner").await;
    let (status, _) = http(
        &server.addr,
        "POST",
        "/lingxi/v1/checkpoints/user-edit",
        &[("Authorization", &format!("Bearer {foreign}"))],
        Some(&json!({"filePath":file,"reason":"edit-start"}).to_string()),
    )
    .await;
    assert_eq!(status, 403);
    assert_eq!(std::fs::read_dir(&archive).unwrap().count(), count);
    server.stop_and_retain().await;
}

#[tokio::test]
async fn prompt_sidecar_reads_actual_sent_input_survives_restart_and_read_error_is_not_empty() {
    use serde_json::Value;
    let server = start_server(
        "frozen-prompt",
        ScriptedProvider::new(vec![ScriptStep::Final("实际回执".into())]),
    )
    .await;
    create_session(&server, "snapshot-a", "甲").await;
    create_session(&server, "snapshot-b", "乙").await;
    let run = server.execute("snapshot-a", "实际用户输入").await;
    let (status, body) = server
        .get("/lingxi/v1/sessions/snapshot-a/prompt-snapshot")
        .await;
    assert_eq!(status, 200, "{body}");
    let frozen: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(frozen["runId"], run);
    assert_eq!(frozen["sessionId"], "snapshot-a");
    assert!(frozen["promptSnapshot"].is_null());
    assert_eq!(frozen["toolNames"], serde_json::json!([]));
    let (_, empty) = server
        .get("/lingxi/v1/sessions/snapshot-b/prompt-snapshot")
        .await;
    assert_eq!(
        serde_json::from_str::<Value>(&empty).unwrap()["available"],
        false
    );
    let dir = server
        .state
        .storage()
        .db_path()
        .parent()
        .unwrap()
        .join("session-prompt-snapshots");
    let path = dir.join(format!(
        "{}.json",
        lingxi_kernel::session_tree::sha256_hex(b"snapshot-a")
    ));
    let original = std::fs::read(&path).unwrap();
    std::fs::write(&path, b"{bad").unwrap();
    let (status, _) = server
        .get("/lingxi/v1/sessions/snapshot-a/prompt-snapshot")
        .await;
    assert_eq!(status, 500);
    std::fs::write(&path, &original).unwrap();
    let foreign = server.mint_device_token("foreign").await;
    let (status, _) = http(
        &server.addr,
        "GET",
        "/lingxi/v1/sessions/snapshot-a/prompt-snapshot",
        &[("Authorization", &format!("Bearer {foreign}"))],
        None,
    )
    .await;
    assert_eq!(status, 403);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let home = server.stop_and_retain().await;
    let server = start_server_at("snapshot-restart", home, ScriptedProvider::new(vec![])).await;
    let (status, body) = server
        .get("/lingxi/v1/sessions/snapshot-a/prompt-snapshot")
        .await;
    assert_eq!(status, 200);
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap(), frozen);
    server.stop_and_retain().await;
}

#[test]
fn production_cli_checkpoint_tools_human_review_deferred_rewind_and_frozen_prompt() {
    let evidence = synthetic_home("production-checkpoint-model");
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output = std::process::Command::new(
        std::env::var("R06_PYTHON_BIN").unwrap_or_else(|_| "python3".into()),
    )
    .arg(repository.join("tests/rust-native/session-checkpoint-live.py"))
    .env("R06_SESSION_EVIDENCE", &evidence)
    .env(
        "R06_SESSION_SERVICE_BIN",
        env!("CARGO_BIN_EXE_lingxi-service"),
    )
    .env(
        "R06_SESSION_UI",
        if cfg!(target_os = "macos") { "1" } else { "0" },
    )
    .output()
    .expect("run actual CLI model proof");
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        println!("SESSION-CLI: {line}");
    }
    for line in String::from_utf8_lossy(&output.stderr).lines() {
        println!("SESSION-CLI-ERR: {line}");
    }
    assert!(
        output.status.success(),
        "real CLI proof failed; evidence: {}",
        evidence.display()
    );
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(evidence.join("result.json")).unwrap()).unwrap();
    assert_eq!(value["status"], "PASS");
    assert_eq!(
        value["results"].as_array().unwrap().len(),
        if cfg!(target_os = "macos") { 10 } else { 9 }
    );
    assert_eq!(value["deferred"]["status"], "completed");
    assert_eq!(value["deferred"]["restoreFiles"], true);
    assert_eq!(value["chatCount"], value["persistedModelCalls"]);
    assert_eq!(value["embeddingCount"], 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn registered_checkpoint_tool_refuses_missing_session_and_user_anchor_before_snapshot() {
    use lingxi_kernel::toolcatalog::{SchemaBudget, ToolRegistry, ToolTargetRef};
    use lingxi_service::toolgateway::{
        CallerSurface, InvocationPermissionContext, InvocationRequest, ToolInvocationGateway,
    };
    let clock: Arc<dyn lingxi_service::inject::ServiceClock> =
        Arc::new(lingxi_service::inject::SystemClock);
    let registry = Arc::new(ToolRegistry::new());
    let approval = Arc::new(lingxi_service::approval_service::ApprovalService::new(
        clock.clone(),
    ));
    let gateway = Arc::new(ToolInvocationGateway::new(
        registry,
        approval,
        clock,
        SchemaBudget::default(),
        300_000,
        100,
    ));
    let home = synthetic_home("checkpoint-missing-anchor");
    let server = start_server_at_with_gateway(
        "checkpoint-missing-anchor",
        home,
        ScriptedProvider::new(vec![]),
        Some(gateway.clone()),
    )
    .await;
    create_session(&server, "without-user", "空用户分支").await;
    let workspace = server.home.join("workspace");
    std::fs::write(workspace.join("must-not-snapshot.txt"), "原内容").unwrap();
    let principal = lingxi_kernel::Principal::LocalUser;
    let authorization = lingxi_service::runs::DriveAuthorization::user_submission(
        None,
        lingxi_kernel::subagent::SessionPermissionMode::Operate,
    );
    let permit = Arc::new(
        server
            .state
            .storage()
            .session_admission()
            .try_acquire(&["without-user"], 100)
            .unwrap(),
    );
    let registration = server
        .state
        .runs()
        .task_supervisor()
        .retain_authorized_run_permit(
            "trusted-background-without-user",
            "without-user",
            "agent",
            &principal,
            &authorization,
            permit,
        )
        .unwrap();
    for (sid, run, action, expected) in [
        (
            "missing-session",
            "missing-run",
            "create",
            "no active session",
        ),
        (
            "missing-session",
            "missing-list-run",
            "list",
            "no active session",
        ),
        (
            "without-user",
            "trusted-background-without-user",
            "create",
            "no user turn",
        ),
    ] {
        let ctx = lingxi_kernel::RunContext {
            principal: principal.clone(),
            session_id: lingxi_protocol::SessionId::new(sid.to_string()),
            run_id: lingxi_protocol::RunId::new(run.to_string()),
            attempt: lingxi_protocol::AttemptId::new(format!("{run}#a1")),
            generation: 1,
        };
        let call = lingxi_protocol::ToolCallId::new(format!("{run}-checkpoint"));
        let prepared = gateway
            .prepare(InvocationRequest::from_trusted_entry(
                &ctx,
                CallerSurface::UserRun,
                "agent",
                InvocationPermissionContext::UserSession {
                    mode: lingxi_kernel::subagent::SessionPermissionMode::Operate,
                },
                ToolTargetRef::ByName {
                    name: "checkpoint".into(),
                },
                None,
                serde_json::json!({"action":action,"name":"refused","snapshot":true}),
                call.clone(),
            ))
            .unwrap();
        let result = gateway
            .execute_prepared(&ctx, &call, &prepared.handle)
            .await
            .unwrap();
        assert!(
            matches!(&result.outcome, lingxi_kernel::ports::ToolOutcome::Failed { error } if error.message.contains(expected)),
            "{result:?}"
        );
        assert!(server
            .state
            .session_tree()
            .list_checkpoints("without-user")
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            std::fs::read_to_string(workspace.join("must-not-snapshot.txt")).unwrap(),
            "原内容"
        );
        let count = server
            .state
            .storage()
            .query_one_text(
                "SELECT CAST(COUNT(*) AS TEXT) FROM checkpoint_file_versions",
                vec![],
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(count, "0", "refused tool cannot write a workspace snapshot");
    }
    drop(registration);
    server.stop_and_clean().await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn explicitly_authorized_root_alias_captures_real_files_but_inner_symlink_degrades() {
    use lingxi_kernel::toolcatalog::{SchemaBudget, ToolRegistry, ToolTargetRef};
    use lingxi_service::toolgateway::{
        CallerSurface, InvocationPermissionContext, InvocationRequest, ToolInvocationGateway,
    };
    let clock: Arc<dyn lingxi_service::inject::ServiceClock> =
        Arc::new(lingxi_service::inject::SystemClock);
    let gateway = Arc::new(ToolInvocationGateway::new(
        Arc::new(ToolRegistry::new()),
        Arc::new(lingxi_service::approval_service::ApprovalService::new(
            clock.clone(),
        )),
        clock,
        SchemaBudget::default(),
        300_000,
        100,
    ));
    let server = start_server_at_with_gateway(
        "root-alias",
        synthetic_home("root-alias"),
        ScriptedProvider::new(vec![ScriptStep::Final("实际用户锚点".into())]),
        Some(gateway.clone()),
    )
    .await;
    let workspace = server.home.join("workspace");
    let first = workspace.join("first.txt");
    let second = workspace.join("second.bin");
    std::fs::write(&first, "明确授权根的真实资料").unwrap();
    std::fs::write(&second, [0u8, 255, 7]).unwrap();
    let alias = server.home.join("explicit-authorized-alias");
    std::os::unix::fs::symlink(&workspace, &alias).unwrap();
    let (status, text) = server.post("/lingxi/v1/sessions", &serde_json::json!({"sessionId":"alias-session","agentId":"agent","title":"根别名","authorizedFolders":[alias]}).to_string()).await;
    assert_eq!(status, 201, "{text}");
    let row = lingxi_adapters::storage::session_tree::get_session_row(
        server.state.storage(),
        "alias-session",
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        row.authorized_folders,
        vec![workspace.to_string_lossy().to_string()],
        "明确新授权先绑定真实根，之后不得重新扩大范围"
    );
    server.execute("alias-session", "创建真实用户锚点").await;
    let principal = lingxi_kernel::Principal::LocalUser;
    let authorization = lingxi_service::runs::DriveAuthorization::user_submission(
        None,
        lingxi_kernel::subagent::SessionPermissionMode::Operate,
    );
    let registration = server
        .state
        .runs()
        .task_supervisor()
        .retain_authorized_run_permit(
            "alias-checkpoint",
            "alias-session",
            "agent",
            &principal,
            &authorization,
            Arc::new(
                server
                    .state
                    .storage()
                    .session_admission()
                    .try_acquire(&["alias-session"], 100)
                    .unwrap(),
            ),
        )
        .unwrap();
    let ctx = lingxi_kernel::RunContext {
        principal,
        session_id: lingxi_protocol::SessionId::new("alias-session".to_string()),
        run_id: lingxi_protocol::RunId::new("alias-checkpoint".to_string()),
        attempt: lingxi_protocol::AttemptId::new("alias-checkpoint#a1".to_string()),
        generation: 1,
    };
    for (name, degraded, file_count) in [("root-alias", false, 2), ("inner-link", true, 0)] {
        if degraded {
            let outside = server.home.join("outside.txt");
            std::fs::write(&outside, "根内链接不得读取此文件").unwrap();
            std::os::unix::fs::symlink(&outside, workspace.join("new-inner-link")).unwrap();
        }
        let call = lingxi_protocol::ToolCallId::new(format!("alias-{name}"));
        let prepared = gateway
            .prepare(InvocationRequest::from_trusted_entry(
                &ctx,
                CallerSurface::UserRun,
                "agent",
                InvocationPermissionContext::UserSession {
                    mode: lingxi_kernel::subagent::SessionPermissionMode::Operate,
                },
                ToolTargetRef::ByName {
                    name: "checkpoint".into(),
                },
                None,
                serde_json::json!({"action":"create","name":name,"snapshot":true}),
                call.clone(),
            ))
            .unwrap();
        let outcome = gateway
            .execute_prepared(&ctx, &call, &prepared.handle)
            .await
            .unwrap();
        let lingxi_kernel::ports::ToolOutcome::Success { result } = outcome.outcome else {
            panic!("{outcome:?}");
        };
        let details = result.details.unwrap();
        assert_eq!(details["snapshotDegraded"], degraded);
        assert_eq!(details["fileCount"], file_count);
        if degraded {
            assert!(details["snapshotCommit"].is_null());
            assert!(details["snapshotError"]
                .as_str()
                .unwrap()
                .contains("symbolic link"));
        } else {
            assert_eq!(details["snapshotCommit"], "alias-session:root-alias");
        }
    }
    let count = server
        .state
        .storage()
        .query_one_text(
            "SELECT CAST(COUNT(*) AS TEXT) FROM checkpoint_file_versions",
            vec![],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        count, "2",
        "根内新增链接只留下明确降级的会话锚点，不能存入额外文件版本"
    );
    assert_eq!(
        std::fs::read_to_string(&first).unwrap(),
        "明确授权根的真实资料"
    );
    assert_eq!(std::fs::read(&second).unwrap(), [0u8, 255, 7]);
    drop(registration);
    server.stop_and_clean().await;
}
