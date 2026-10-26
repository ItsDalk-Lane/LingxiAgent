//! R08-C③（叶 4F1F69CCA760 A0「Bridge 内容脱敏」）：会话内查找对 bridge 挂接
//! 会话做可见文本脱敏（`<t> M-D HH:MM </t>` 时间标签整段移除），非 bridge
//! 会话原样保留——路由级真链验收（真 TCP/HTTP、真 SQLite、真桥会话表挂接）。
//!
//! bridge 身份走产品真实路径：put_connector → claim_inbox（同事务 resolve
//! conversation 并铸造确定性 session id）→ 以该 id 建会话 → 追加含时间标签
//! 的用户/助手消息 → GET /lingxi/v1/sessions/{id}/find?q=。对照组为同内容
//! 的普通会话。修复前（未接线）两组命中片段均含标签 → 本测试红；修复后绿。
use lingxi_adapters::storage::bridge::{
    claim_inbox, put_connector, BridgeConnectorInput, BridgeInboxInput, ConversationKind,
};
use lingxi_adapters::storage::session_tree::{append_message, AppendMessage};
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use serde_json::Value;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

const TAGGED_USER: &str = "<t> 10-3 09:30 </t>用户问 RedactProbe 第一次";
const TAGGED_ASSISTANT: &str = "前文 <t> 10-3 09:31 </t> 助手答 RedactProbe 后文";

struct TestServer {
    addr: SocketAddr,
    home: PathBuf,
    token: String,
    state: ServiceState,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
}

fn synthetic_home(tag: &str) -> PathBuf {
    let base = std::env::var("LINGXI_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let dir = base.join(format!(
        "lingxi-r08c3-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

async fn start_server(tag: &str) -> TestServer {
    let home = synthetic_home(tag);
    let layout = prepare_layout(&home).expect("prepare layout");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("loopback"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, ServiceDeps::default())
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
    let addr = ready_rx.await.expect("service readiness");
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
    async fn stop_and_clean(self) {
        self.stop.send(()).expect("server still listening");
        tokio::time::timeout(Duration::from_secs(10), self.handle)
            .await
            .expect("shutdown within timeout")
            .expect("join")
            .expect("clean serve result");
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

async fn http_get(addr: &SocketAddr, path: &str, token: &str) -> (u16, Value) {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    let head = format!(
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nAuthorization: Bearer {token}\r\n\r\n"
    );
    stream.write_all(head.as_bytes()).await.expect("write");
    let mut raw = Vec::new();
    loop {
        let mut chunk = [0u8; 4096];
        match stream.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&chunk[..n]),
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
                ) =>
            {
                break
            }
            Err(err) => panic!("read: {err}"),
        }
    }
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status: u16 = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let json = serde_json::from_str(body).unwrap_or(Value::Null);
    (status, json)
}

async fn seed_tagged_history(state: &ServiceState, sid: &str) {
    let user_json = serde_json::json!({
        "role": "user",
        "content": [{"type": "text", "text": TAGGED_USER}],
    })
    .to_string();
    let assistant_json = serde_json::json!({
        "role": "assistant",
        "content": [{"type": "text", "text": TAGGED_ASSISTANT}],
    })
    .to_string();
    append_message(
        state.storage(),
        AppendMessage {
            session_id: sid,
            message_id: "u1",
            parent_message_id: None,
            run_id: None,
            role: "user",
            content_json: &user_json,
            entry_type: "message",
            model_call_id: None,
            now_unix_ms: 1,
        },
    )
    .await
    .expect("append user");
    append_message(
        state.storage(),
        AppendMessage {
            session_id: sid,
            message_id: "a1",
            parent_message_id: Some("u1"),
            run_id: None,
            role: "assistant",
            content_json: &assistant_json,
            entry_type: "message",
            model_call_id: None,
            now_unix_ms: 2,
        },
    )
    .await
    .expect("append assistant");
}

fn snippets(body: &Value) -> Vec<String> {
    body["matches"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|m| m["snippet"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn c3_find_redacts_bridge_time_tags_only_for_bridge_bound_sessions() {
    let server = start_server("find-redaction").await;
    let now = server.state.clock().now_unix_ms();
    let (agent_id, _agent_name) = server
        .state
        .memory()
        .create_agent("R08C3", None, None)
        .unwrap_or_else(|err| panic!("create agent: {err}"));
    let owner = lingxi_service::LOCAL_OWNER_USER_ID;

    // ── bridge 挂接会话：产品真实路径铸造 session id ──
    let connector = put_connector(
        server.state.storage(),
        &BridgeConnectorInput {
            id: "r08c3-connector".into(),
            platform: "telegram".into(),
            studio_id: "local".into(),
            actor_id: agent_id.clone(),
            owner_subject: lingxi_kernel::LOCAL_OWNER_SUBJECT.into(),
            owner_sender_id: Some("sender-1".into()),
            enabled: true,
            config_json: "{}".into(),
        },
        None,
        now,
    )
    .await
    .expect("put connector");
    let claim = claim_inbox(
        server.state.storage(),
        &BridgeInboxInput {
            platform: "telegram".into(),
            connector_id: "r08c3-connector".into(),
            conversation_id: "conv-1".into(),
            conversation_kind: ConversationKind::Dm,
            message_id: "m-1".into(),
            sender_id: "sender-1".into(),
            payload_json: "{\"text\":\"hi\"}".into(),
            attachments: Vec::new(),
        },
        connector.generation,
        now,
    )
    .await
    .expect("claim inbox");
    let bridge_sid = claim
        .inbox
        .session_id
        .clone()
        .expect("resolved conversation mints a session id");
    server
        .state
        .session_admin()
        .create_session(&bridge_sid, &agent_id, owner, "bridge 会话", None, &[], true, now)
        .await
        .expect("create bridge session row");
    seed_tagged_history(&server.state, &bridge_sid).await;

    // ── 对照：普通会话，同一内容 ──
    let plain_sid = "r08c3-plain";
    server
        .state
        .session_admin()
        .create_session(plain_sid, &agent_id, owner, "普通会话", None, &[], true, now)
        .await
        .expect("create plain session row");
    seed_tagged_history(&server.state, plain_sid).await;

    let (status, bridge_body) = http_get(
        &server.addr,
        &format!("/lingxi/v1/sessions/{bridge_sid}/find?q=RedactProbe"),
        &server.token,
    )
    .await;
    assert_eq!(status, 200, "bridge find: {bridge_body}");
    let bridge_snips = snippets(&bridge_body);
    assert!(!bridge_snips.is_empty(), "至少一条命中: {bridge_body}");
    for snip in &bridge_snips {
        assert!(
            !snip.contains("<t>") && !snip.contains("</t>"),
            "bridge 会话命中片段必须脱去时间标签: {snip:?}"
        );
    }
    assert!(
        bridge_snips.iter().any(|s| s.contains("前文") && s.contains("后文")),
        "助手文本保留且标签整段移除: {bridge_snips:?}"
    );

    let (status, plain_body) = http_get(
        &server.addr,
        &format!("/lingxi/v1/sessions/{plain_sid}/find?q=RedactProbe"),
        &server.token,
    )
    .await;
    assert_eq!(status, 200, "plain find: {plain_body}");
    let plain_snips = snippets(&plain_body);
    assert!(!plain_snips.is_empty(), "{plain_body}");
    // 普通会话：助手正文中段的时间标签不是 bridge 语义，原样保留（现役同口径；
    // 行首时间标签两种会话都被 FIND_TURN_TAG_PREFIX 前缀剥离，不构成区分面）。
    assert!(
        plain_snips
            .iter()
            .any(|s| s.contains("<t> 10-3 09:31 </t>")),
        "非 bridge 会话不脱敏: {plain_snips:?}"
    );

    // 负路径：无效会话显式拒（A1）。
    let (status, bad) = http_get(
        &server.addr,
        "/lingxi/v1/sessions/sess-does-not-exist/find?q=x",
        &server.token,
    )
    .await;
    assert!(status == 403 || status == 404, "invalid session must be rejected: {status} {bad}");

    server.stop_and_clean().await;
}
