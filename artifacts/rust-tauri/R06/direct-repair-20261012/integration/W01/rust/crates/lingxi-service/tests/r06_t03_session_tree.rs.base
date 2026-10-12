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
    let home = synthetic_home(tag);
    let layout = prepare_layout(&home).expect("prepare layout");
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
        self.stop.send(()).expect("server still listening");
        tokio::time::timeout(Duration::from_secs(10), self.handle)
            .await
            .expect("server shuts down within timeout")
            .expect("server task join")
            .expect("clean serve result");
        let _ = std::fs::remove_dir_all(&self.home);
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

// ──────────────────────────────────────────── A05：fork 不串写

#[tokio::test]
async fn fork_copies_history_with_stable_ids_and_writes_stay_independent() {
    // 两轮对话（每轮 user + final 两条消息）。
    let provider = ScriptedProvider::new(vec![
        ScriptStep::Final("answer-1".into()),
        ScriptStep::Final("answer-2".into()),
        ScriptStep::Final("answer-a3".into()),
        ScriptStep::Final("answer-b1".into()),
    ]);
    let server = start_server("fork-happy", provider).await;
    create_session(&server, "src_sess", "source").await;
    let run1 = server.execute("src_sess", "q1").await;
    let _run2 = server.execute("src_sess", "q2").await;

    let before = server.branch_ids("src_sess").await;
    assert_eq!(
        before,
        vec![
            format!("user:{run1}"),
            format!("{run1}-final"),
            format!("user:{}", _run2),
            format!("{_run2}-final"),
        ],
        "两轮后分支 = user1/final1/user2/final2"
    );

    // 在 final1 处 fork（fork 点 = 边界消息）。
    let fork_point = format!("{run1}-final");
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/src_sess/fork",
            &serde_json::json!({
                "newSessionId": "fork_sess",
                "boundaryMessageId": fork_point,
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 200, "fork: {body}");
    let fork = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    assert_eq!(fork["parentSessionId"], "src_sess");
    assert_eq!(fork["forkPointMessageId"], fork_point);
    assert_eq!(fork["lineageDepth"], 1, "第一层 fork 深度=1");

    // fork 分支 = root→boundary 保留段，消息 ID 与源相同。
    let forked = server.branch_ids("fork_sess").await;
    assert_eq!(
        forked,
        vec![format!("user:{run1}"), fork_point.clone()],
        "fork 复制 root→boundary 且保留消息 ID（createBranchedSession）"
    );

    // 各自追加：源写 run_a3、fork 写 run_b1，互不可见。
    let run_a3 = server.execute("src_sess", "q3").await;
    let run_b1 = server.execute("fork_sess", "b1").await;
    let src_ids = server.branch_ids("src_sess").await;
    let fork_ids = server.branch_ids("fork_sess").await;
    assert!(
        src_ids.contains(&format!("user:{run_a3}")) && !src_ids.contains(&format!("user:{run_b1}")),
        "源含自己的新消息、不含 fork 的：{src_ids:?}"
    );
    assert!(
        fork_ids.contains(&format!("user:{run_b1}"))
            && fork_ids.contains(&fork_point)
            && !fork_ids.contains(&format!("user:{}", _run2)),
        "fork 含 fork 前历史+自己的新消息，不含源的后续：{fork_ids:?}"
    );
    server.stop_and_clean().await;
}

/// 深度闸：第 3 层 fork → 409 session_fork_depth_limit。
#[tokio::test]
async fn fork_beyond_depth_two_is_refused_409() {
    let provider = ScriptedProvider::new(vec![ScriptStep::Final("a".into())]);
    let server = start_server("fork-depth", provider).await;
    create_session(&server, "root", "r").await;
    let run1 = server.execute("root", "q").await;
    let boundary = format!("{run1}-final");

    for (src, dst, expect) in [
        ("root", "child", 200),
        ("child", "grand", 200),
        ("grand", "great", 409),
    ] {
        let (status, body) = server
            .post(
                &format!("/lingxi/v1/sessions/{src}/fork"),
                &serde_json::json!({
                    "newSessionId": dst,
                    "boundaryMessageId": boundary,
                })
                .to_string(),
            )
            .await;
        assert_eq!(status, expect, "fork {src}→{dst}: {body}");
        if expect == 409 {
            assert!(
                body.contains("session_fork_depth_limit"),
                "深度拒绝原因须为 session_fork_depth_limit: {body}"
            );
        }
    }
    server.stop_and_clean().await;
}

/// 会话 id 撞库 → 响亮 409 session_exists（REPAIR-R1 观察项：fork
/// newSessionId 撞库与创建撞库都不得落 Internal 500；对照现役
/// sessions.ts:587 active_session_conflict）。
#[tokio::test]
async fn duplicate_session_id_is_loud_409_on_fork_and_create() {
    let provider = ScriptedProvider::new(vec![ScriptStep::Final("a1".into())]);
    let server = start_server("dup-sess", provider).await;
    create_session(&server, "src", "s").await;
    let run1 = server.execute("src", "q1").await;

    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/src/fork",
            &serde_json::json!({
                "newSessionId": "dup",
                "boundaryMessageId": format!("{run1}-final"),
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 200, "first fork: {body}");
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/src/fork",
            &serde_json::json!({
                "newSessionId": "dup",
                "boundaryMessageId": format!("{run1}-final"),
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 409, "fork newSessionId 撞库 → 409: {body}");
    assert!(body.contains("session_exists"), "body: {body}");

    // 管理面创建同 id → 同样 409 session_exists。
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions",
            &serde_json::json!({
                "sessionId": "src",
                "agentId": "agent",
                "title": "dup-create",
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 409, "create 撞库 → 409: {body}");
    assert!(body.contains("session_exists"), "body: {body}");
    server.stop_and_clean().await;
}

/// busy 闸：会话持有运行租约时 fork/retry/rewind → 409 session_busy
/// （现役 isSessionStreaming → session_busy，session-turn-actions.ts:382-384）。
/// 租约经 supervisor 公共入口真实取得（与 execute 路径同一把闸）。
#[tokio::test]
async fn busy_session_refuses_fork_retry_rewind_409() {
    let provider = ScriptedProvider::new(vec![ScriptStep::Final("seed".into())]);
    let server = start_server("busy", provider).await;
    create_session(&server, "s", "s").await;
    let run1 = server.execute("s", "q1").await;
    let boundary = format!("{run1}-final");

    // 真实占用：与 execute 同一把闸的会话租约。
    let lease = server
        .state
        .sessions()
        .session_supervisor()
        .try_begin_run("s")
        .expect("lease on idle session");
    assert!(server.state.sessions().session_supervisor().is_busy("s"));

    for (path, body) in [
        (
            "/lingxi/v1/sessions/s/fork",
            serde_json::json!({"newSessionId":"f","boundaryMessageId":boundary}).to_string(),
        ),
        ("/lingxi/v1/sessions/s/turns/retry", "{}".to_string()),
        (
            "/lingxi/v1/sessions/s/rewind",
            serde_json::json!({"checkpoint":"latest"}).to_string(),
        ),
    ] {
        let (status, body) = server.post(path, &body).await;
        assert_eq!(status, 409, "busy {path}: {body}");
        assert!(body.contains("session_busy"), "busy reason: {body}");
    }
    // 释放租约后会话回到非 busy，fork 立即可行（闸门无泄漏）。
    drop(lease);
    assert!(!server.state.sessions().session_supervisor().is_busy("s"));
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s/fork",
            &serde_json::json!({"newSessionId":"f","boundaryMessageId":boundary}).to_string(),
        )
        .await;
    assert_eq!(status, 200, "释放后 fork 可行: {body}");
    server.stop_and_clean().await;
}

// ──────────────────────────────────────────── retry：分支重置 + 旧结果保留

#[tokio::test]
async fn retry_resets_branch_and_new_run_never_overwrites_old() {
    let provider = ScriptedProvider::new(vec![
        ScriptStep::Final("a1".into()),
        ScriptStep::Final("a2".into()),
        ScriptStep::Final("a2-retry".into()),
    ]);
    let server = start_server("retry", provider).await;
    create_session(&server, "s", "s").await;
    let run1 = server.execute("s", "q1").await;
    let run2 = server.execute("s", "q2").await;

    // latest-user 形态（无 targetMessageId）：目标 = 最近 user 回合 q2。
    let (status, body) = server.post("/lingxi/v1/sessions/s/turns/retry", "{}").await;
    assert_eq!(status, 200, "retry: {body}");
    let outcome = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    assert_eq!(
        outcome["turnInputMessageId"],
        format!("user:{run2}"),
        "最新 user 回合 = run2 的输入"
    );
    assert_eq!(
        outcome["newHeadMessageId"],
        format!("{run1}-final"),
        "新头 = 回合输入的 parent（现役 retryBranchParentId）"
    );
    let marker_id = outcome["resetMarkerId"].as_str().unwrap().to_string();
    assert!(
        marker_id.starts_with("reset:s:"),
        "标记 id 服务端铸造: {marker_id}"
    );
    // 回合输入随响应返回（D6 两段式：客户端经 execute 重新提交）。
    assert_eq!(outcome["turnInputContentJson"], r#"{"text":"q2"}"#);

    // 分支 = user1/final1 + reset 标记；旧消息（user2/final2）不在分支但在存储。
    let branch = server.branch_ids("s").await;
    assert_eq!(
        branch,
        vec![format!("user:{run1}"), format!("{run1}-final"), marker_id],
        "重置后分支 = 保留段 + reset 标记"
    );
    let all =
        lingxi_adapters::storage::session_tree::list_all_messages(server.state.storage(), "s")
            .await
            .unwrap();
    let all_ids: Vec<&str> = all.iter().map(|m| m.message_id.as_str()).collect();
    assert!(
        all_ids.contains(&format!("user:{run2}").as_str())
            && all_ids.contains(&format!("{run2}-final").as_str()),
        "旧历史 append-only 不删：{all_ids:?}"
    );

    // 重新提交为全新 run（不覆盖旧结果）。
    let run3 = server.execute("s", "q2").await;
    assert_ne!(run3, run2, "retry 重提交是全新 run");
    let branch_after = server.branch_ids("s").await;
    assert!(
        branch_after.contains(&format!("user:{run3}"))
            && branch_after.contains(&format!("{run3}-final")),
        "新轮次接在重置点之后：{branch_after:?}"
    );
    let (status, _) = run_row(server.state.storage(), &run2).await;
    assert_eq!(status, "completed", "旧 run 结果保留不动");
    server.stop_and_clean().await;
}

/// 目标解析：assistant 目标 → 前方最近 user 回合；user 目标 → 该回合；
/// 不在分支上的目标 → 400。
#[tokio::test]
async fn retry_target_resolution_matches_incumbent() {
    let provider = ScriptedProvider::new(vec![
        ScriptStep::Final("a1".into()),
        ScriptStep::Final("a2".into()),
    ]);
    let server = start_server("retry-target", provider).await;
    create_session(&server, "s", "s").await;
    let run1 = server.execute("s", "q1").await;
    let run2 = server.execute("s", "q2").await;

    // assistant 目标（final2）→ 回合 = q2（user2），新头 = final1。
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s/turns/retry",
            &serde_json::json!({ "targetMessageId": format!("{run2}-final") }).to_string(),
        )
        .await;
    assert_eq!(status, 200, "assistant target retry: {body}");
    let outcome = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    assert_eq!(outcome["turnInputMessageId"], format!("user:{run2}"));
    assert_eq!(outcome["newHeadMessageId"], format!("{run1}-final"));

    // 根回合 user 目标 → 新头 = null（重置到根之前）。
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s/turns/retry",
            &serde_json::json!({ "targetMessageId": format!("user:{run1}") }).to_string(),
        )
        .await;
    assert_eq!(status, 200, "root turn retry: {body}");
    let outcome = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    assert_eq!(outcome["turnInputMessageId"], format!("user:{run1}"));
    assert!(
        outcome["newHeadMessageId"].is_null(),
        "根回合的新头 = null（现役 retryBranchParentId = null）"
    );

    // 不在分支上的目标 → 400。
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s/turns/retry",
            r#"{"targetMessageId":"no-such-message"}"#,
        )
        .await;
    assert_eq!(status, 400, "off-branch target: {body}");
    assert!(body.contains("session_turn_target_invalid"), "body: {body}");
    server.stop_and_clean().await;
}

/// retry 的 fileRollback 契约（REPAIR-R1 FINDING-02；现役 sessions.ts:1584-1607）：
/// 非法值 → 400 invalid_file_rollback（绝不静默降级成 none）；workspace 且偏好
/// 未开 → 403 file_rollback_disabled；workspace 且该回合无检查点 → no_checkpoint
/// 报告且分支照常重置（对照现役 restoreTurn 的 no_checkpoint 不阻塞 commit）；
/// workspace 且有检查点 → 与 rewind 同一套内容级恢复，报告随响应返回。
#[tokio::test]
async fn retry_file_rollback_contract_matches_incumbent() {
    let provider = ScriptedProvider::new(vec![
        ScriptStep::Final("a1".into()),
        ScriptStep::Final("a2".into()),
        ScriptStep::Final("a3".into()),
    ]);
    let server = start_server("retry-rollback", provider).await;
    let work = synthetic_home("retry-rollback-files");
    std::fs::create_dir_all(&work).unwrap();
    let work = std::fs::canonicalize(&work).unwrap();
    let file_w = work.join("w.txt");
    let file_x = work.join("x.txt");
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions",
            &serde_json::json!({
                "sessionId": "s",
                "agentId": "agent",
                "title": "s",
                "authorizedFolders": [work.to_string_lossy()],
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 201, "create s: {body}");
    let _run1 = server.execute("s", "q1").await;
    let run2 = server.execute("s", "q2").await;

    // 非法值 → 400 invalid_file_rollback。
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s/turns/retry",
            r#"{"fileRollback":"everything"}"#,
        )
        .await;
    assert_eq!(status, 400, "非法 fileRollback → 400: {body}");
    assert!(body.contains("invalid_file_rollback"), "body: {body}");

    // workspace 且偏好未开 → 403 file_rollback_disabled；分支不动。
    let branch_before = server.branch_ids("s").await;
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s/turns/retry",
            r#"{"fileRollback":"workspace"}"#,
        )
        .await;
    assert_eq!(status, 403, "偏好未开 → 403: {body}");
    assert!(body.contains("file_rollback_disabled"), "body: {body}");
    assert_eq!(server.branch_ids("s").await, branch_before, "403 不动分支");

    // 偏好开启；该回合无检查点 → no_checkpoint 报告，分支照常重置（不阻塞）。
    server.state.set_rollback_file_changes(true);
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s/turns/retry",
            r#"{"fileRollback":"workspace"}"#,
        )
        .await;
    assert_eq!(status, 200, "无检查点不阻塞 retry: {body}");
    let outcome = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    assert_eq!(outcome["turnInputMessageId"], format!("user:{run2}"));
    assert_eq!(outcome["fileRollbackReport"]["ok"], false);
    assert_eq!(outcome["fileRollbackReport"]["reason"], "no_checkpoint");
    assert!(
        server
            .branch_ids("s")
            .await
            .last()
            .unwrap()
            .starts_with("reset:s:"),
        "无检查点时分支照常重置"
    );

    // 重提交为 run3，然后建两个检查点：cp-old 记录 w=v-before；系统侧把 w
    // 改成 v-after-model 并把 x=x1 一起被 cp-new 见证（同回合输入锚点）。
    let run3 = server.execute("s", "q2").await;
    std::fs::write(&file_w, "v-before").unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s/checkpoints",
            &serde_json::json!({
                "name": "cp-old",
                "targetMessageId": format!("{run3}-final"),
                "filePaths": [file_w.to_string_lossy()],
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 200, "cp-old: {body}");
    std::fs::write(&file_w, "v-after-model").unwrap();
    std::fs::write(&file_x, "x1").unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s/checkpoints",
            &serde_json::json!({
                "name": "cp-new",
                "targetMessageId": format!("{run3}-final"),
                "filePaths": [file_w.to_string_lossy(), file_x.to_string_lossy()],
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 200, "cp-new: {body}");

    // w 回到见证过的旧版本（restored 输入）；x 被外部修改（conflicted）。
    std::fs::write(&file_w, "v-before").unwrap();
    std::fs::write(&file_x, "x-user-edit").unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s/turns/retry",
            r#"{"fileRollback":"workspace"}"#,
        )
        .await;
    assert_eq!(status, 200, "workspace retry: {body}");
    let outcome = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    assert_eq!(outcome["turnInputMessageId"], format!("user:{run3}"));
    // 锚点 = 该回合输入的最新检查点 cp-new：w 当前=v-before（被见证）→
    // restored 真实写回 v-after-model；x 当前=x-user-edit（未见证）→
    // conflicted 保留用户修改。
    let report = &outcome["fileRollbackReport"];
    assert_eq!(report["ok"], false, "有冲突文件时 ok=false");
    assert_eq!(
        report["restored"].as_array().unwrap(),
        &vec![serde_json::Value::String(
            file_w.to_string_lossy().into_owned()
        )],
    );
    assert_eq!(
        report["conflicted"].as_array().unwrap(),
        &vec![serde_json::Value::String(
            file_x.to_string_lossy().into_owned()
        )],
    );
    assert_eq!(
        std::fs::read_to_string(&file_w).unwrap(),
        "v-after-model",
        "retry fileRollback 真实写回存档字节"
    );
    assert_eq!(
        std::fs::read_to_string(&file_x).unwrap(),
        "x-user-edit",
        "冲突文件保留用户修改"
    );
    assert!(
        server
            .branch_ids("s")
            .await
            .last()
            .unwrap()
            .starts_with("reset:s:"),
        "retry 分支重置不受文件冲突影响"
    );

    let _ = std::fs::remove_dir_all(&work);
    server.stop_and_clean().await;
}

// ──────────────────────────────────────────── A06：rewind 内容级回滚

/// A06（REPAIR-R1 FINDING-05 裁决语义）：rewind = 内容级回滚，逐文件三档判定。
/// 外部修改（系统从未见证的版本）→ 该文件 conflicted、用户修改原样保留、
/// 分支照常回移；文件被删除同样判冲突（绝不悄悄重建）；系统见证过的版本
/// （模型改且被检查点记录）→ 真实写回存档字节（restored）；未改动 → skipped。
/// 收据逐文件如实标注，绝不整批 409、绝不伪称全部撤销。
#[tokio::test]
async fn rewind_restores_content_with_per_file_verdicts_and_branch_rewinds() {
    let provider = ScriptedProvider::new(vec![
        ScriptStep::Final("a1".into()),
        ScriptStep::Final("a2".into()),
        ScriptStep::Final("a3".into()),
    ]);
    let server = start_server("rewind-a06", provider).await;

    // ── 场景 1：外部修改 → conflicted，用户文件保留，分支照常回移。
    let work1 = synthetic_home("rewind-s1");
    std::fs::create_dir_all(&work1).unwrap();
    let work1 = std::fs::canonicalize(&work1).unwrap();
    let file_a = work1.join("a.txt");
    std::fs::write(&file_a, "checkpoint-content").unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions",
            &serde_json::json!({
                "sessionId": "s1",
                "agentId": "agent",
                "title": "s1",
                "authorizedFolders": [work1.to_string_lossy()],
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 201, "create s1: {body}");
    let run1 = server.execute("s1", "q1").await;
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s1/checkpoints",
            &serde_json::json!({
                "name": "cp1",
                "targetMessageId": format!("{run1}-final"),
                "filePaths": [file_a.to_string_lossy()],
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 200, "s1 checkpoint: {body}");

    // 偏好默认关：restoreFiles → 403（现役 getRollbackFileChanges 默认关）。
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s1/rewind",
            r#"{"checkpoint":"cp1","restoreFiles":true}"#,
        )
        .await;
    assert_eq!(status, 403, "pref off → 403: {body}");
    assert!(body.contains("file_rollback_disabled"), "body: {body}");

    // 打开偏好（测试缝），外部修改文件。
    server.state.set_rollback_file_changes(true);
    std::fs::write(&file_a, "user-edit-after-checkpoint").unwrap();
    let branch_before = server.branch_ids("s1").await;

    // 预览（只读）：conflicted 判定如实列出，且不写任何状态。
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s1/rewind/preview",
            r#"{"checkpoint":"cp1","restoreFiles":true}"#,
        )
        .await;
    assert_eq!(status, 200, "s1 preview: {body}");
    let preview = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    assert_eq!(preview["files"][0]["verdict"], "conflicted");
    assert_eq!(server.branch_ids("s1").await, branch_before, "预览只读");

    // 执行：200——冲突如实入收据而非整批 409；分支照常回移；用户修改保留。
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s1/rewind",
            r#"{"checkpoint":"cp1","restoreFiles":true}"#,
        )
        .await;
    assert_eq!(status, 200, "外部修改 → conflicted 而非整批 409: {body}");
    let receipt = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    assert_eq!(receipt["externalEffects"], "not_rolled_back");
    assert_eq!(
        receipt["conflicted"].as_array().unwrap(),
        &vec![serde_json::Value::String(
            file_a.to_string_lossy().into_owned()
        )],
        "外部修改文件入 conflicted"
    );
    assert!(receipt["restored"].as_array().unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(&file_a).unwrap(),
        "user-edit-after-checkpoint",
        "用户修改原样保留，绝不覆盖"
    );
    let branch_after = server.branch_ids("s1").await;
    assert!(
        branch_after.last().unwrap().starts_with("reset:s1:"),
        "冲突不阻塞分支回移: {branch_after:?}"
    );

    // ── 场景 2：文件在检查点后被删除 = 外部修改 → conflicted，绝不悄悄重建。
    let work2 = synthetic_home("rewind-s2");
    std::fs::create_dir_all(&work2).unwrap();
    let work2 = std::fs::canonicalize(&work2).unwrap();
    let file_d = work2.join("d.txt");
    std::fs::write(&file_d, "to-be-deleted").unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions",
            &serde_json::json!({
                "sessionId": "s2",
                "agentId": "agent",
                "title": "s2",
                "authorizedFolders": [work2.to_string_lossy()],
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 201, "create s2: {body}");
    let run2 = server.execute("s2", "q2").await;
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s2/checkpoints",
            &serde_json::json!({
                "name": "cp1",
                "targetMessageId": format!("{run2}-final"),
                "filePaths": [file_d.to_string_lossy()],
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 200, "s2 checkpoint: {body}");
    std::fs::remove_file(&file_d).unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s2/rewind",
            r#"{"checkpoint":"cp1","restoreFiles":true}"#,
        )
        .await;
    assert_eq!(status, 200, "文件被删 → conflicted 而非整批 409: {body}");
    let receipt = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    assert_eq!(
        receipt["conflicted"].as_array().unwrap(),
        &vec![serde_json::Value::String(
            file_d.to_string_lossy().into_owned()
        )],
    );
    assert!(!file_d.exists(), "被删文件不悄悄重建");

    // ── 场景 3：见证版本 → restored 真实写回存档字节；未改动 → skipped。
    let work3 = synthetic_home("rewind-s3");
    std::fs::create_dir_all(&work3).unwrap();
    let work3 = std::fs::canonicalize(&work3).unwrap();
    let file_b = work3.join("b.txt");
    let file_c = work3.join("c.txt");
    std::fs::write(&file_b, "v1").unwrap();
    std::fs::write(&file_c, "same").unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions",
            &serde_json::json!({
                "sessionId": "s3",
                "agentId": "agent",
                "title": "s3",
                "authorizedFolders": [work3.to_string_lossy()],
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 201, "create s3: {body}");
    let run3 = server.execute("s3", "q3").await;
    // cp-v1 记录 b=v1 / c=same；随后系统侧把 b 改为 v2 并被 cp-v2 见证。
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s3/checkpoints",
            &serde_json::json!({
                "name": "cp-v1",
                "targetMessageId": format!("{run3}-final"),
                "filePaths": [file_b.to_string_lossy(), file_c.to_string_lossy()],
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 200, "s3 cp-v1: {body}");
    std::fs::write(&file_b, "v2").unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s3/checkpoints",
            &serde_json::json!({
                "name": "cp-v2",
                "targetMessageId": format!("{run3}-final"),
                "filePaths": [file_b.to_string_lossy()],
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 200, "s3 cp-v2: {body}");

    // 预览：b=restored（当前 v2 被系统见证），c=skipped（已在目标态）。
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s3/rewind/preview",
            r#"{"checkpoint":"cp-v1","restoreFiles":true}"#,
        )
        .await;
    assert_eq!(status, 200, "s3 preview: {body}");
    let preview = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    let verdict_of = |path: &std::path::Path| {
        preview["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["filePath"].as_str() == Some(path.to_string_lossy().as_ref()))
            .unwrap_or_else(|| panic!("preview 缺少 {}", path.display()))["verdict"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(verdict_of(&file_b), "restored");
    assert_eq!(verdict_of(&file_c), "skipped");

    // 执行：b 真实写回 v1 存档字节，c 不动。
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s3/rewind",
            r#"{"checkpoint":"cp-v1","restoreFiles":true}"#,
        )
        .await;
    assert_eq!(status, 200, "s3 rewind: {body}");
    let receipt = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    assert_eq!(
        receipt["restored"].as_array().unwrap(),
        &vec![serde_json::Value::String(
            file_b.to_string_lossy().into_owned()
        )],
    );
    assert_eq!(
        receipt["skipped"].as_array().unwrap(),
        &vec![serde_json::Value::String(
            file_c.to_string_lossy().into_owned()
        )],
    );
    assert!(receipt["conflicted"].as_array().unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(&file_b).unwrap(),
        "v1",
        "真实写回存档字节"
    );
    assert_eq!(std::fs::read_to_string(&file_c).unwrap(), "same");

    let _ = std::fs::remove_dir_all(&work1);
    let _ = std::fs::remove_dir_all(&work2);
    let _ = std::fs::remove_dir_all(&work3);
    server.stop_and_clean().await;
}

/// 授权目录闸：checkpoint 的 filePaths 越出授权目录 → 403。
#[tokio::test]
async fn checkpoint_file_paths_outside_authorized_folders_are_403() {
    let provider = ScriptedProvider::new(vec![ScriptStep::Final("a1".into())]);
    let server = start_server("ckpt-gate", provider).await;
    let allowed = synthetic_home("ckpt-allowed");
    let outside = synthetic_home("ckpt-outside");
    std::fs::create_dir_all(&allowed).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let outside_file = std::fs::canonicalize(&outside).unwrap().join("secret.txt");
    std::fs::write(&outside_file, "secret").unwrap();

    let (status, body) = server
        .post(
            "/lingxi/v1/sessions",
            &serde_json::json!({
                "sessionId": "s",
                "agentId": "agent",
                "title": "s",
                "authorizedFolders": [std::fs::canonicalize(&allowed).unwrap().to_string_lossy()],
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 201, "create: {body}");
    let run1 = server.execute("s", "q1").await;
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s/checkpoints",
            &serde_json::json!({
                "name": "cp",
                "targetMessageId": format!("{run1}-final"),
                "filePaths": [outside_file.to_string_lossy()],
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 403, "越出授权目录 → 403: {body}");
    let _ = std::fs::remove_dir_all(&allowed);
    let _ = std::fs::remove_dir_all(&outside);
    server.stop_and_clean().await;
}

/// 检查点 CRUD：latest 覆盖 / 非 latest 重名 409 / 列表 / 删除 / 404。
#[tokio::test]
async fn checkpoint_crud_matches_incumbent_shape() {
    let provider = ScriptedProvider::new(vec![ScriptStep::Final("a1".into())]);
    let server = start_server("ckpt-crud", provider).await;
    create_session(&server, "s", "s").await;
    let run1 = server.execute("s", "q1").await;
    let target = format!("{run1}-final");

    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s/checkpoints",
            &serde_json::json!({"name":"latest","targetMessageId":target}).to_string(),
        )
        .await;
    assert_eq!(status, 200, "latest create: {body}");
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s/checkpoints",
            &serde_json::json!({"name":"latest","targetMessageId":format!("user:{run1}")})
                .to_string(),
        )
        .await;
    assert_eq!(status, 200, "latest overwrite: {body}");

    let (status, _) = server
        .post(
            "/lingxi/v1/sessions/s/checkpoints",
            &serde_json::json!({"name":"v1","targetMessageId":target}).to_string(),
        )
        .await;
    assert_eq!(status, 200, "v1 create");
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s/checkpoints",
            &serde_json::json!({"name":"v1","targetMessageId":target}).to_string(),
        )
        .await;
    assert_eq!(status, 409, "非 latest 重名 → 409: {body}");
    assert!(body.contains("checkpoint_conflict"), "body: {body}");

    let (status, body) = server.get("/lingxi/v1/sessions/s/checkpoints").await;
    assert_eq!(status, 200, "list: {body}");
    let listed = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    let names: Vec<&str> = listed["checkpoints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["latest", "v1"], "创建时间升序");

    let (status, _) = server
        .post(
            "/lingxi/v1/sessions/s/checkpoints/delete",
            r#"{"name":"v1"}"#,
        )
        .await;
    assert_eq!(status, 200, "delete v1");
    let (status, _) = server
        .post(
            "/lingxi/v1/sessions/s/checkpoints/delete",
            r#"{"name":"v1"}"#,
        )
        .await;
    assert_eq!(status, 404, "再删 → 404");
    server.stop_and_clean().await;
}

/// latest 覆盖 × filePaths（REPAIR-R1 FINDING-01 的 HTTP 级回归）：
/// 同名 latest 重复记录同一文件不再 500 UNIQUE；换一批文件后旧文件版本与
/// 内容存档随检查点原子替换（无残留）；rewind 只按最新文件集判定。
#[tokio::test]
async fn checkpoint_latest_overwrite_with_file_paths_replaces_atomically() {
    let provider = ScriptedProvider::new(vec![ScriptStep::Final("a1".into())]);
    let server = start_server("ckpt-latest-files", provider).await;
    let work = synthetic_home("ckpt-latest-files-dir");
    std::fs::create_dir_all(&work).unwrap();
    let work = std::fs::canonicalize(&work).unwrap();
    let file1 = work.join("f1.txt");
    let file2 = work.join("f2.txt");
    std::fs::write(&file1, "v1").unwrap();
    std::fs::write(&file2, "x").unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions",
            &serde_json::json!({
                "sessionId": "s",
                "agentId": "agent",
                "title": "s",
                "authorizedFolders": [work.to_string_lossy()],
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 201, "create s: {body}");
    let run1 = server.execute("s", "q1").await;
    let target = format!("{run1}-final");

    // 第一次记录 f1。
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s/checkpoints",
            &serde_json::json!({
                "name": "latest",
                "targetMessageId": target,
                "filePaths": [file1.to_string_lossy()],
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 200, "latest #1: {body}");

    // 同文件再记录（修复前 = 500 UNIQUE）：现在 200。
    std::fs::write(&file1, "v2").unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s/checkpoints",
            &serde_json::json!({
                "name": "latest",
                "targetMessageId": target,
                "filePaths": [file1.to_string_lossy()],
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 200, "latest 同文件覆盖不再 500: {body}");

    // 换一批文件：旧文件版本行随检查点原子替换，无残留。
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s/checkpoints",
            &serde_json::json!({
                "name": "latest",
                "targetMessageId": target,
                "filePaths": [file2.to_string_lossy()],
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 200, "latest 换文件集: {body}");
    let versions = lingxi_adapters::storage::session_tree::list_file_versions(
        server.state.storage(),
        "s:latest",
    )
    .await
    .unwrap();
    assert_eq!(
        versions
            .iter()
            .map(|v| v.file_path.as_str())
            .collect::<Vec<_>>(),
        vec![file2.to_string_lossy().as_ref()],
        "旧文件版本行无残留"
    );
    // 内容存档同样替换（rewind 恢复语义的承载）。
    let contents = lingxi_adapters::storage::session_tree::list_file_contents(
        server.state.storage(),
        "s:latest",
    )
    .await
    .unwrap();
    assert_eq!(contents.len(), 1, "内容存档随行替换");
    assert_eq!(contents[0].content, b"x");

    // rewind 只按最新文件集判定：f2 未改动 → skipped；f1 不出现在收据任何栏。
    server.state.set_rollback_file_changes(true);
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s/rewind",
            r#"{"checkpoint":"latest","restoreFiles":true}"#,
        )
        .await;
    assert_eq!(status, 200, "rewind latest: {body}");
    let receipt = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    assert_eq!(
        receipt["skipped"].as_array().unwrap(),
        &vec![serde_json::Value::String(
            file2.to_string_lossy().into_owned()
        )],
    );
    let f1_path = file1.to_string_lossy().to_string();
    for key in ["restored", "conflicted", "skipped", "failed"] {
        assert!(
            !receipt[key]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_str() == Some(f1_path.as_str())),
            "f1 不应出现在收据 {key} 栏: {body}"
        );
    }
    let _ = std::fs::remove_dir_all(&work);
    server.stop_and_clean().await;
}

// ──────────────────────────────────────────── 管理面

#[tokio::test]
async fn management_surface_rename_pin_memory_archive_restore_delete_cleanup() {
    let provider = ScriptedProvider::new(vec![ScriptStep::Final("a1".into())]);
    let server = start_server("mgmt", provider).await;
    create_session(&server, "parent", "parent-title").await;
    let run1 = server.execute("parent", "q1").await;

    // rename
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/rename",
            r#"{"sessionId":"parent","title":"renamed"}"#,
        )
        .await;
    assert_eq!(status, 200, "rename: {body}");
    let (status, body) = server
        .get("/lingxi/v1/sessions/find?sessionId=parent")
        .await;
    assert_eq!(status, 200, "find: {body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["title"],
        "renamed"
    );

    // pin / pin-order
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/pin",
            r#"{"sessionId":"parent","pinned":true}"#,
        )
        .await;
    assert_eq!(status, 200, "pin: {body}");
    assert!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["pinOrder"].is_i64(),
        "置顶后排到置顶区尾"
    );
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/pin-order",
            r#"{"sessionIds":["parent"]}"#,
        )
        .await;
    assert_eq!(status, 200, "pin-order: {body}");
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/pin-order",
            r#"{"sessionIds":["parent","parent"]}"#,
        )
        .await;
    assert_eq!(status, 400, "重复 id → 400: {body}");

    // memory GET/PATCH
    let (status, body) = server.get("/lingxi/v1/sessions/parent/memory").await;
    assert_eq!(status, 200, "memory get: {body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["memoryEnabled"],
        true
    );
    let (status, body) = http(
        &server.addr,
        "PATCH",
        "/lingxi/v1/sessions/parent/memory",
        &[("Authorization", &server.bearer())],
        Some(r#"{"memoryEnabled":false}"#),
    )
    .await;
    assert_eq!(status, 200, "memory patch: {body}");
    let (_, body) = server.get("/lingxi/v1/sessions/parent/memory").await;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["memoryEnabled"],
        false
    );

    // fork 一个子对话，然后归档父对话不带 childMode → 409 child_sessions_present。
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/parent/fork",
            &serde_json::json!({
                "newSessionId": "child",
                "boundaryMessageId": format!("{run1}-final"),
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 200, "fork child: {body}");
    let (status, body) = server
        .post("/lingxi/v1/sessions/archive", r#"{"sessionId":"parent"}"#)
        .await;
    assert_eq!(status, 409, "有子对话无 childMode → 409: {body}");
    assert!(body.contains("child_sessions_present"), "body: {body}");

    // detach_children 归档：子对话谱系指针清空、父归档。
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/archive",
            r#"{"sessionId":"parent","childMode":"detach_children"}"#,
        )
        .await;
    assert_eq!(status, 200, "archive detach: {body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["detachedChildren"],
        1
    );
    let (_, body) = server.get("/lingxi/v1/sessions/find?sessionId=child").await;
    assert!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["parentSessionId"].is_null(),
        "detach 后子对话释放到顶层"
    );

    // REPAIR-R1 FINDING-03：主列表（GET /sessions）只列 active——归档的
    // parent 不可见，仍 active 的 child 可见。
    let (status, body) = server.get("/lingxi/v1/sessions").await;
    assert_eq!(status, 200, "main list: {body}");
    let parsed = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    let ids: Vec<&str> = parsed["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["sessionId"].as_str().unwrap())
        .collect();
    assert!(!ids.contains(&"parent"), "归档会话不进主列表: {ids:?}");
    assert!(ids.contains(&"child"), "active 会话在主列表: {ids:?}");

    // 归档会话拒绝 execute（生命周期闸）。
    let (status, body) = server
        .post("/lingxi/v1/sessions/parent/execute", r#"{"input":"x"}"#)
        .await;
    assert_eq!(status, 409, "归档会话 execute → 409: {body}");
    assert!(body.contains("session_not_active"), "body: {body}");

    // archived 列表可见；restore 回 active。
    let (status, body) = server.get("/lingxi/v1/sessions/archived").await;
    assert_eq!(status, 200, "archived list: {body}");
    assert!(body.contains("parent"));
    let (status, body) = server
        .post("/lingxi/v1/sessions/restore", r#"{"sessionId":"parent"}"#)
        .await;
    assert_eq!(status, 200, "restore: {body}");
    // restore 后回到主列表（FINDING-03 的对偶断言）。
    let (status, body) = server.get("/lingxi/v1/sessions").await;
    assert_eq!(status, 200, "main list after restore: {body}");
    let parsed = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    let ids: Vec<&str> = parsed["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["sessionId"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"parent"), "restore 后回到主列表: {ids:?}");

    // 再归档后永久删除；删除后 find → 404；未归档会话不可永久删除。
    let (status, _) = server
        .post("/lingxi/v1/sessions/archive", r#"{"sessionId":"parent"}"#)
        .await;
    assert_eq!(status, 200, "re-archive");
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/archived/delete",
            r#"{"sessionId":"child"}"#,
        )
        .await;
    assert_eq!(status, 409, "未归档会话永久删除 → 409: {body}");
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/archived/delete",
            r#"{"sessionId":"parent"}"#,
        )
        .await;
    assert_eq!(status, 200, "archived delete: {body}");
    let (status, _) = server
        .get("/lingxi/v1/sessions/find?sessionId=parent")
        .await;
    assert_eq!(status, 404, "删除后 find → 404");

    // cleanup：归档 child 后 maxAgeDays=0 清理 → 全部过期归档删除。
    let (status, _) = server
        .post("/lingxi/v1/sessions/archive", r#"{"sessionId":"child"}"#)
        .await;
    assert_eq!(status, 200, "archive child");
    // 现役语义是严格小于截止线（mtime < cutoff）；让真实时钟越过归档毫秒。
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (status, body) = server
        .post("/lingxi/v1/sessions/cleanup", r#"{"maxAgeDays":0}"#)
        .await;
    assert_eq!(status, 200, "cleanup: {body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["deleted"],
        1,
        "maxAgeDays=0 → 刚归档的也过期"
    );

    // search：title 阶段命中、content 阶段命中消息内容、超长查询 400。
    create_session(&server, "searchable", "unique-title-xyz").await;
    let (status, body) = server
        .get("/lingxi/v1/sessions/search?q=unique-title-xyz&phase=title")
        .await;
    assert_eq!(status, 200, "search title: {body}");
    assert!(body.contains("searchable"));
    let long_query = "x".repeat(600);
    let (status, body) = server
        .get(&format!("/lingxi/v1/sessions/search?q={long_query}"))
        .await;
    assert_eq!(status, 400, "超长查询 → 400: {body}");
    server.stop_and_clean().await;
}

// ──────────────────────────────────────────── 归属：跨主体 403

#[tokio::test]
async fn cross_principal_tree_access_is_403() {
    let provider = ScriptedProvider::new(vec![]);
    let server = start_server("cross", provider).await;
    let remote = server.mint_device_token("user_remote").await;
    let remote_bearer = format!("Bearer {remote}");

    for (method, path, body) in [
        ("GET", "/lingxi/v1/sessions/sess_local_alpha/branch", None),
        (
            "GET",
            "/lingxi/v1/sessions/sess_local_alpha/checkpoints",
            None,
        ),
        (
            "POST",
            "/lingxi/v1/sessions/sess_local_alpha/fork",
            Some(r#"{"newSessionId":"x","boundaryMessageId":"y"}"#),
        ),
        (
            "POST",
            "/lingxi/v1/sessions/sess_local_alpha/turns/retry",
            Some("{}"),
        ),
        (
            "POST",
            "/lingxi/v1/sessions/sess_local_alpha/rewind",
            Some(r#"{"checkpoint":"latest"}"#),
        ),
        ("GET", "/lingxi/v1/sessions/sess_local_alpha/memory", None),
    ] {
        let (status, body) = http(
            &server.addr,
            method,
            path,
            &[("Authorization", &remote_bearer)],
            body,
        )
        .await;
        assert_eq!(
            status, 403,
            "设备主体访问他人会话 {method} {path} → 403: {body}"
        );
    }
    server.stop_and_clean().await;
}
