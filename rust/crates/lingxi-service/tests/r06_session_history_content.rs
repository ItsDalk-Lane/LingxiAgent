//! R06 C2「会话历史与内容读取」8 叶子的真实语义测试（生产 router + 真实
//! SQLite + 真实鉴权；SessionCollabTools 直接调用 pub 工具面）。
//!
//! 叶子对照（scripts/rust-tauri/r06_original_leaf_bindings.json）：
//! - R00-T02-LA-4F1F69CCA760 GET /sessions/{id}/find — 命中位置、信封剥离、
//!   Bridge 脱敏（纯函数面）；
//! - R00-T02-LA-4DD3F71FA2CD POST .../latest-user-message/replay — 版本重放、
//!   可选替换文字、运行中 409；
//! - R00-T02-LA-CDBFE6616EB9 GET .../history-overview — 授权概览、
//!   未就绪 available:false；
//! - R00-T02-LA-ED089133C106 GET .../content/{contentId} — 会话内取大内容、
//!   找不到 404、资源 id 必须出现在本会话投影历史；
//! - R00-T02-LA-F0319FC4F93D GET .../summary — 滚动摘要记录（hasSummary 非
//!   空才算）；
//! - R00-T02-LA-F6A1A50E97E4 GET .../conversation-map/turns — 折叠问答回合
//!   卡片；
//! - R00-T02-LA-D62A81C2AEB2 POST .../fresh-compact — 默认无压缩服务 503
//!   拒绝；经 ServiceDeps.compaction_service 测试缝注入真实 CompactionService
//!   后压缩 + 重建提示词工件；
//! - R00-T02-LA-5DCB0352EB08 SessionCollabTools — list/read 范围受控、send/
//!   create 确认草稿、apply 投递、create 半成功保留新会话 id。
//!
//! 造历史数据一律走真实链路（POST /sessions + POST execute 跑真 run 写
//! canonical message store）；摘要记录按 TS 落盘契约写实文件（读面的真实
//! 数据源）；legacy 无头区用真实表上的定向 DELETE 模拟（存储读取器明确
//! 支持的既有数据形态）。

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lingxi_kernel::model_exchange::ModelTurnInput;
use lingxi_kernel::ports::{
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, StoragePort, ToolOutcome, TurnDeltaSink,
    TurnProviderPort,
};
use lingxi_kernel::toolcatalog::SchemaBudget;
use lingxi_kernel::RunContext;
use lingxi_protocol::{
    ContentBlock, ModelCallId, NormalizedMessage, ResourceId, ResourceKind, ResourceRef,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use lingxi_service::history::history_content::{
    sanitize_bridge_visible_text, DraftApplyOutcome, SessionCollabDelivery, SessionCollabTools,
    SESSION_FIND_QUERY_MAX_LENGTH,
};
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};

const STUB_API_KEY: &str = "stub-key-not-a-secret";

// ═══════════════ 服务脚手架（r06_session_focus_detached_project 的 boot 同款） ═══════════════

struct Server {
    addr: SocketAddr,
    token: String,
    home: PathBuf,
    state: ServiceState,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
}

/// 同进程并行 boot 的时钟粒度可能碰撞（pid 相同），原子序号保证 home 唯一。
static BOOT_SEQ: AtomicU64 = AtomicU64::new(0);

fn fresh_home(tag: &str) -> PathBuf {
    let uniq = format!(
        "{}-{}-{}",
        std::process::id(),
        BOOT_SEQ.fetch_add(1, Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let home = std::env::temp_dir().join(format!("lingxi-r06-c2t-{tag}-{uniq}"));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("mkdir home");
    home
}

async fn boot_with(tag: &str, deps: ServiceDeps) -> Server {
    let home = fresh_home(tag);
    boot_at(home, deps).await
}

async fn boot_at(home: PathBuf, deps: ServiceDeps) -> Server {
    let home = home.canonicalize().expect("canonical home");
    let layout = prepare_layout(&home).expect("prepare layout");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("loopback"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
        .await
        .expect("bootstrap");
    let token = state.auth().local_token();
    let (stop, stop_rx) = tokio::sync::oneshot::channel::<()>();
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
    let addr = ready_rx.await.expect("readiness");
    Server {
        addr,
        token,
        home,
        state,
        stop,
        handle,
    }
}

async fn boot_default(tag: &str) -> Server {
    boot_with(tag, ServiceDeps::default()).await
}

struct RawResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Value,
    body_text: String,
}

impl RawResponse {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

async fn http(
    server: &Server,
    method: &str,
    path: &str,
    body: Option<&Value>,
    token: Option<&str>,
) -> RawResponse {
    let task = async {
        let mut socket = tokio::net::TcpStream::connect(server.addr).await.unwrap();
        let payload = body.map(|value| value.to_string()).unwrap_or_default();
        let auth = token
            .map(|token| format!("Authorization: Bearer {token}\r\n"))
            .unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {addr}\r\n{auth}Content-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n{payload}",
            addr = server.addr,
            len = payload.len(),
        );
        socket.write_all(request.as_bytes()).await.unwrap();
        let mut response = Vec::new();
        socket.read_to_end(&mut response).await.unwrap();
        let boundary = response
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .expect("HTTP 头存在");
        let headers = String::from_utf8_lossy(&response[..boundary]);
        let status: u16 = headers
            .split_whitespace()
            .nth(1)
            .expect("状态码")
            .parse()
            .expect("合法状态码");
        let header_pairs = headers
            .lines()
            .skip(1)
            .filter_map(|line| {
                line.split_once(':')
                    .map(|(n, v)| (n.trim().to_string(), v.trim().to_string()))
            })
            .collect();
        let body_text = String::from_utf8_lossy(&response[boundary + 4..]).into_owned();
        let body = if body_text.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&body_text).unwrap_or(Value::String(body_text.clone()))
        };
        RawResponse {
            status,
            headers: header_pairs,
            body,
            body_text,
        }
    };
    tokio::time::timeout(Duration::from_secs(20), task)
        .await
        .unwrap_or_else(|_| panic!("HTTP 往返在限时内: {method} {path}"))
}

async fn ok(server: &Server, method: &str, path: &str, body: Option<&Value>) -> RawResponse {
    http(server, method, path, body, Some(&server.token)).await
}

async fn create_session(server: &Server, id: &str, agent: &str, title: &str) {
    let resp = ok(
        server,
        "POST",
        "/lingxi/v1/sessions",
        Some(&json!({"sessionId": id, "agentId": agent, "title": title})),
    )
    .await;
    assert_eq!(resp.status, 201, "会话 {id} 创建：{}", resp.body_text);
}

async fn execute_wait(server: &Server, session_id: &str, input: &str) -> String {
    let resp = ok(
        server,
        "POST",
        &format!("/lingxi/v1/sessions/{session_id}/execute"),
        Some(&json!({ "input": input })),
    )
    .await;
    assert_eq!(resp.status, 200, "execute {session_id}: {}", resp.body_text);
    let run_id = resp.body["runId"].as_str().expect("runId").to_string();
    wait_terminal(server, &run_id).await;
    run_id
}

async fn run_status(server: &Server, run_id: &str) -> Option<(String, Option<String>)> {
    let status = server
        .state
        .storage()
        .query_one_text(
            "SELECT status FROM runs WHERE run_id = ?1",
            vec![run_id.to_string()],
        )
        .await
        .expect("query run status")?;
    let reason = server
        .state
        .storage()
        .query_one_text(
            "SELECT terminal_reason FROM runs WHERE run_id = ?1",
            vec![run_id.to_string()],
        )
        .await
        .expect("query run reason");
    Some((status, reason))
}

/// 等待 run 到达终态（真实 SQLite 轮询，无睡眠超时）；非 completed 时
/// 携带终态原因与尾部事件（失败诊断）。
async fn wait_terminal(server: &Server, run_id: &str) {
    for _ in 0..200 {
        if let Some((status, reason)) = run_status(server, run_id).await {
            if !matches!(
                status.as_str(),
                "queued" | "running" | "waiting_approval" | "cancelling"
            ) {
                assert_eq!(
                    status, "completed",
                    "run {run_id} 终态: {status} ({reason:?})"
                );
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("run {run_id} 未在 5s 内到终态");
}

/// 设备主体令牌（跨主体 403 测试）。
async fn mint_device_token(server: &Server, user_id: &str) -> String {
    let resp = ok(
        server,
        "POST",
        "/lingxi/v1/devices/credentials",
        Some(&json!({ "userId": user_id, "scopes": ["chat"] })),
    )
    .await;
    assert_eq!(
        resp.status, 201,
        "mint device credential: {}",
        resp.body_text
    );
    resp.body["secret"].as_str().expect("secret").to_string()
}

impl Server {
    async fn stop_and_clean(self) {
        self.stop.send(()).expect("server still listening");
        let _ = tokio::time::timeout(Duration::from_secs(10), self.handle)
            .await
            .expect("限时停机");
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

// ═══════════════ 模型/工具替身 ═══════════════

/// 脚本化 provider：按脚本依次产出 Final 文本（真实 run 写真实消息）。
struct ScriptedProvider {
    script: Mutex<VecDeque<String>>,
}

impl ScriptedProvider {
    fn new(script: Vec<String>) -> Arc<Self> {
        Arc::new(Self {
            script: Mutex::new(script.into_iter().collect()),
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
        ctx: &'a RunContext,
        call: &'a ModelCallId,
        _input: &'a ModelTurnInput,
        _deltas: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let text = self
            .script
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| "兜底答复".to_string());
        let ctx_at_issue = ctx.clone();
        let call_id = call.to_string();
        Box::pin(async move {
            ProviderTurnResult::of_ctx(
                &ctx_at_issue,
                ProviderTurn::Final {
                    message: NormalizedMessage {
                        role: "assistant".to_string(),
                        content: vec![ContentBlock::Text { text }],
                        model_call_id: Some(ModelCallId::new(call_id)),
                    },
                },
            )
        })
    }
}

/// 闸门 provider：阻塞在 watch 通道上直到测试打开闸门（状态型等待，无
/// 丢失唤醒窗口），打开后产出 Final。
struct GatedProvider {
    release: Arc<tokio::sync::watch::Sender<bool>>,
}

impl TurnProviderPort for GatedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.provider".to_string(),
            model: "stub.model".to_string(),
            operation: "chat".to_string(),
        }
    }

    fn next_turn<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ModelCallId,
        _input: &'a ModelTurnInput,
        _deltas: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let mut gate = self.release.subscribe();
        let ctx_at_issue = ctx.clone();
        let call_id = call.to_string();
        Box::pin(async move {
            gate.wait_for(|open| *open)
                .await
                .expect("gate channel open");
            ProviderTurnResult::of_ctx(
                &ctx_at_issue,
                ProviderTurn::Final {
                    message: NormalizedMessage {
                        role: "assistant".to_string(),
                        content: vec![ContentBlock::Text {
                            text: "闸门释放后的最终答复".to_string(),
                        }],
                        model_call_id: Some(ModelCallId::new(call_id)),
                    },
                },
            )
        })
    }
}

fn session_file_ref(resource_id: &str) -> ResourceRef {
    ResourceRef {
        resource_id: ResourceId::new(resource_id.to_string()),
        kind: ResourceKind::SessionFile,
        display_name: Some("attachment.txt".to_string()),
        uri: None,
        digest: None,
        size_bytes: None,
    }
}

// ═══════════════ 压缩服务缝（r06_t02_compaction 的构造同款） ═══════════════

/// loopback stub：一连接一请求，脚本化 SSE 应答（摘要模型链的确定性替身）。
struct StubResponse {
    body: String,
}

struct StubServer {
    addr: SocketAddr,
    hits: Arc<AtomicUsize>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl StubServer {
    async fn start(responses: Vec<StubResponse>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("stub bind");
        let addr = listener.local_addr().expect("stub addr");
        let hits = Arc::new(AtomicUsize::new(0));
        let responses = Arc::new(Mutex::new(VecDeque::from(responses)));
        let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let (task_hits, task_responses) = (Arc::clone(&hits), Arc::clone(&responses));
        let task = tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accepted = listener.accept() => accepted,
                };
                let Ok((mut socket, _)) = accepted else { break };
                task_hits.fetch_add(1, Ordering::SeqCst);
                let mut raw = Vec::new();
                let header_end = loop {
                    let mut chunk = [0_u8; 4096];
                    let read =
                        tokio::time::timeout(Duration::from_secs(10), socket.read(&mut chunk))
                            .await;
                    let Ok(Ok(read)) = read else { break None };
                    if read == 0 {
                        break None;
                    }
                    raw.extend_from_slice(&chunk[..read]);
                    if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
                    {
                        let headers = String::from_utf8_lossy(&raw[..pos]).to_string();
                        let len = headers
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if raw.len() >= pos + len {
                            break Some(pos + len);
                        }
                    }
                };
                let Some(_) = header_end else {
                    continue;
                };
                let payload = task_responses
                    .lock()
                    .unwrap()
                    .pop_front()
                    .map(|response| response.body)
                    .unwrap_or_else(|| {
                        "{\"error\":{\"message\":\"stub script exhausted\"}}".to_string()
                    });
                let raw = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                let _ = socket.write_all(raw.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        Self {
            addr,
            hits,
            shutdown: Some(shutdown),
            task,
        }
    }

    fn v1(&self) -> String {
        format!("http://{}/v1", self.addr)
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }

    async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = tokio::time::timeout(Duration::from_secs(5), &mut self.task).await;
    }
}

/// openai-completions SSE：一段文本 + stop + usage。
fn sse_final(text: &str) -> StubResponse {
    let mut body = String::new();
    for frame in [
        json!({"id":"chatcmpl-c2t","model":"stub-model","choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]}),
        json!({"id":"chatcmpl-c2t","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
        json!({"id":"chatcmpl-c2t","choices":[],"usage":{"prompt_tokens":17,"completion_tokens":5}}),
    ] {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    StubResponse { body }
}

/// 合法结构化摘要（9 标题齐全，通过清洗校验）。
fn golden_summary() -> String {
    "## Goal\n完成用户请求\n\n## Constraints & Preferences\n- (none)\n\n\
     ## Progress\n### Done\n- [x] 处理附件\n\n### In Progress\n- [ ] 汇总\n\n\
     ### Blocked\n- 无\n\n## Key Decisions\n- **用真实链路**: 避免猜测\n\n\
     ## Next Steps\n1. 写出结论\n\n## Critical Context\n- (none)\n"
        .to_string()
}

fn plane_json(stub: &StubServer, chat_window: u64) -> String {
    format!(
        r#"{{
            "providers": {{
                "stub_svc": {{
                    "protocol": "openai-completions",
                    "endpoint": "{}",
                    "auth": {{"kind": "apiKey", "apiKey": "{STUB_API_KEY}"}}
                }}
            }},
            "models": {{
                "chat": {{"provider": "stub_svc", "model": "chat-model", "capabilities": {{"tools": true}}, "compat": {{"contextWindow": {chat_window}}}}},
                "summarize": {{"provider": "stub_svc", "model": "summarize-model", "compat": {{"contextWindow": 1000000}}}}
            }}
        }}"#,
        stub.v1()
    )
}

/// 真实 CompactionService（真 ModelGateway/CredentialService/
/// AuxiliaryExecutor → loopback stub），经 ServiceDeps.compaction_service 注入。
fn build_compaction_service(
    plane_json: &str,
    runtime_dir: &Path,
) -> Arc<lingxi_service::compaction::CompactionService> {
    let plane = lingxi_adapters::models::config::ModelPlaneConfig::parse_and_validate(plane_json)
        .expect("plane parses");
    let credentials = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .expect("credential service"),
    );
    let gateway =
        Arc::new(lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane));
    let executor = lingxi_adapters::models::auxiliary::AuxiliaryExecutor::new(
        Arc::clone(&gateway),
        credentials as Arc<dyn lingxi_adapters::models::credentials::ProviderCredentialPort>,
        SchemaBudget::default(),
    )
    .expect("auxiliary executor");
    Arc::new(lingxi_service::compaction::CompactionService::new(
        Arc::new(executor),
        gateway,
        Arc::new(lingxi_service::quotas::QuotaManager::new(
            lingxi_service::quotas::QuotaLimits::default(),
        )),
        Arc::new(lingxi_service::inject::SystemClock),
    ))
}

// ═══════════════ 上下文编译器材料夹具（r06_t01_context_compiler 同款） ═══════════════

fn write_fixture(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create fixture parent");
    }
    std::fs::write(path, content).expect("write fixture");
}

/// 现役 $LINGXI_HOME 布局的最小材料夹具（agent lingxi，zh 主调）。
fn write_context_home_fixture(home: &Path) {
    write_fixture(
        &home.join("agents/lingxi/config.yaml"),
        "locale: zh-CN\nagent:\n  name: 灵犀\nmemory:\n  enabled: true\n",
    );
    write_fixture(
        &home.join("agents/lingxi/memory/memory.md"),
        "C2T-长期记忆锚点内容\n",
    );
}

/// 产品模板夹具（yuan 的 zh 与 en 变体 + 兜底）。
fn write_context_product_fixture(product: &Path) {
    write_fixture(
        &product.join("yuan/lingxi.md"),
        "C2T-YUAN 你是{{userName}}的伙伴，名叫{{agentName}}。",
    );
    write_fixture(
        &product.join("yuan/en/lingxi.md"),
        "C2T-YUAN-EN You are {{userName}}'s partner named {{agentName}}.",
    );
    write_fixture(&product.join("identity.example.md"), "C2T-EXAMPLE");
}

fn context_compiler(
    home: &Path,
    product: &Path,
) -> Arc<lingxi_service::context_compiler::ContextCompilerService> {
    let host = lingxi_service::context_compiler::HostFacts {
        platform: "darwin".to_string(),
        os_type: "Darwin".to_string(),
        os_release: "25.0.0".to_string(),
        shell_label: "zsh".to_string(),
        workspace: "/tmp/r06-c2t-ws".to_string(),
    };
    Arc::new(
        lingxi_service::context_compiler::ContextCompilerService::new(Arc::new(
            lingxi_service::context_compiler::FileContextMaterialSource::from_home(
                home,
                product.to_path_buf(),
                host,
            ),
        )),
    )
}

// ═══════════════ 4F1F69CCA760：会话内查找 ═══════════════

/// 命中位置（displayable 序号）、exact/token 命中、模型侧信封剥离、
/// 空/超长/未知参数、401/403/404。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn find_route_reports_match_positions_and_strips_model_envelopes() {
    let provider = ScriptedProvider::new(vec![
        "第一答复：关于依赖安装的说明".to_string(),
        "答复开头\n[hana_reminder]\n提醒内容 SECRETNEEDLE 不参与检索\n[/hana_reminder]\n可见答复：staging 部署完成".to_string(),
        "收尾答复".to_string(),
    ]);
    let server = boot_with(
        "find",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            ..ServiceDeps::default()
        },
    )
    .await;
    create_session(&server, "find-s", "agent", "查找").await;
    execute_wait(&server, "find-s", "部署流水线的问题").await;
    execute_wait(&server, "find-s", "关于 staging 环境的疑问").await;
    execute_wait(&server, "find-s", "收尾提问").await;

    // displayable 序号：u1=0, f1=1, u2=2, f2=3, u3=4, f3=5。
    // u2 整句命中（exact）+ f2 词元命中（"staging"）：exact 优先取 best。
    let resp = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/find-s/find?q=staging%20%E7%8E%AF%E5%A2%83",
        None,
    )
    .await;
    assert_eq!(resp.status, 200, "find: {}", resp.body_text);
    assert_eq!(
        resp.body["total"],
        json!(2),
        "u2 整句 + f2 词元各一次命中: {}",
        resp.body
    );
    assert_eq!(resp.body["bestIndex"], json!(2), "exact 命中优先");
    assert_eq!(resp.body["matches"].as_array().map(Vec::len), Some(2));
    assert_eq!(resp.body["matches"][0]["index"], json!(2));
    assert_eq!(resp.body["matches"][0]["exact"], json!(true));
    assert_eq!(resp.body["matches"][1]["index"], json!(3));
    assert_eq!(
        resp.body["matches"][1]["exact"],
        json!(false),
        "f2 词元命中"
    );
    assert!(
        resp.body["matches"][0]["snippet"]
            .as_str()
            .unwrap()
            .contains("staging"),
        "片段承载命中上下文"
    );
    assert_eq!(resp.body["tokens"], json!(["staging", "环境"]));
    assert!(
        resp.body["revision"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .is_ok(),
        "revision 是数值字符串"
    );
    assert_eq!(resp.body["truncated"], json!(false));

    // token 命中（非整句）：f1 同时含「依赖」「第一」两词元。
    let resp = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/find-s/find?q=%E7%AC%AC%E4%B8%80%20%E4%BE%9D%E8%B5%96",
        None,
    )
    .await;
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body["total"], json!(1));
    assert_eq!(resp.body["bestIndex"], json!(1), "f1 = 序号 1");
    assert_eq!(resp.body["matches"][0]["exact"], json!(false), "词元命中");

    // 信封剥离：reminder 块内文字不参与命中（f2 不可被 SECRETNEEDLE 查到）。
    let resp = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/find-s/find?q=secretneedle",
        None,
    )
    .await;
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body["total"], json!(0), "信封内内容不得命中");
    assert_eq!(resp.body["bestIndex"], Value::Null);

    // 剥离后可见正文可命中（f2 = 序号 3）。
    let resp = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/find-s/find?q=%E9%83%A8%E7%BD%B2%E5%AE%8C%E6%88%90",
        None,
    )
    .await;
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body["bestIndex"], json!(3));
    assert!(
        resp.body["matches"][0]["snippet"]
            .as_str()
            .unwrap()
            .contains("部署完成"),
        "片段承载命中词"
    );

    // 空查询：空结果（非错误）。
    let resp = ok(&server, "GET", "/lingxi/v1/sessions/find-s/find?q=", None).await;
    assert_eq!(resp.status, 200, "空查询: {}", resp.body_text);
    assert_eq!(resp.body["total"], json!(0));
    assert_eq!(resp.body["matches"], json!([]));
    // 缺 q 参数同空查询。
    let resp = ok(&server, "GET", "/lingxi/v1/sessions/find-s/find", None).await;
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body["total"], json!(0));

    // 未知参数键：400（现役入口拒绝未知 query key）。
    let resp = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/find-s/find?q=x&extra=1",
        None,
    )
    .await;
    assert_eq!(resp.status, 400, "未知参数键: {}", resp.body_text);

    // 超长查询：400 + query_too_long（现役 :950-952）。
    let long_query = "x".repeat(SESSION_FIND_QUERY_MAX_LENGTH + 1);
    let resp = ok(
        &server,
        "GET",
        &format!("/lingxi/v1/sessions/find-s/find?q={long_query}"),
        None,
    )
    .await;
    assert_eq!(resp.status, 400, "超长查询 400");
    assert_eq!(resp.body["error"], json!("query_too_long"));
    assert_eq!(resp.body["maxLength"], json!(SESSION_FIND_QUERY_MAX_LENGTH));

    // 重复 q：400。
    let resp = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/find-s/find?q=a&q=b",
        None,
    )
    .await;
    assert_eq!(resp.status, 400, "重复 q");

    // 越权与存在性。
    let resp = ok(&server, "GET", "/lingxi/v1/sessions/ghost/find?q=x", None).await;
    assert_eq!(resp.status, 404, "未知会话 404");
    let foreign = mint_device_token(&server, "find_foreign").await;
    let resp = http(
        &server,
        "GET",
        "/lingxi/v1/sessions/find-s/find?q=x",
        None,
        Some(&foreign),
    )
    .await;
    assert_eq!(resp.status, 403, "跨主体 403");
    let resp = http(
        &server,
        "GET",
        "/lingxi/v1/sessions/find-s/find?q=x",
        None,
        None,
    )
    .await;
    assert_eq!(resp.status, 401, "无凭证 401");

    server.stop_and_clean().await;
}

/// Bridge 时间标签脱敏（纯函数面；bridge 会话宿主未落地，路由层不可达，
/// 单元证明其语义——见 history_content.rs 模块头偏差声明）。
#[test]
fn find_bridge_sanitizer_strips_time_tags_only() {
    assert_eq!(
        sanitize_bridge_visible_text("  <t> 10-3 09:30 </t>你好"),
        "你好",
        "合法时间标签整段移除"
    );
    assert_eq!(
        sanitize_bridge_visible_text("保留 <t> 非时间标签</t> 原文"),
        "保留 <t> 非时间标签</t> 原文",
        "非时间形态的 <t> 不动"
    );
    assert_eq!(
        sanitize_bridge_visible_text("前文<t>12-31 23:59</t>后文"),
        "前文后文",
        "标签两侧文本保留"
    );
    assert_eq!(
        sanitize_bridge_visible_text("无标签正文"),
        "无标签正文",
        "无标签原样通过"
    );
}

// ═══════════════ 4DD3F71FA2CD：最近用户消息重放 ═══════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn latest_user_message_replay_replays_by_version_with_optional_replacement() {
    let provider = ScriptedProvider::new(vec!["第一轮答复".to_string(), "第二轮答复".to_string()]);
    let server = boot_with(
        "replay",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            ..ServiceDeps::default()
        },
    )
    .await;
    create_session(&server, "replay-s", "agent", "重放").await;
    let run1 = execute_wait(&server, "replay-s", "第一轮提问内容").await;
    let run2 = execute_wait(&server, "replay-s", "第二轮提问内容").await;
    let user2 = format!("user:{run2}");
    let user1 = format!("user:{run1}");

    // 非最近用户消息 → 明确 400（不回退重放别的消息）。
    let resp = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/replay-s/latest-user-message/replay",
        Some(&json!({ "sourceMessageId": user1 })),
    )
    .await;
    assert_eq!(resp.status, 400, "非最近消息: {}", resp.body_text);
    assert_eq!(
        resp.body["details"]["causeId"],
        json!("session.replay_not_latest"),
        "{}",
        resp.body_text
    );

    // 版本 + 替换文字重放最近消息（source 匹配 + snapshotVersion 回执）。
    let resp = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/replay-s/latest-user-message/replay",
        Some(&json!({
            "sourceMessageId": user2,
            "snapshotVersion": 7,
            "text": "替换后的重放文本",
        })),
    )
    .await;
    assert_eq!(resp.status, 200, "替换重放: {}", resp.body_text);
    assert_eq!(resp.body["ok"], json!(true));
    assert_eq!(resp.body["sessionId"], json!("replay-s"));
    assert_eq!(resp.body["turnInputMessageId"], json!(user2));
    assert_eq!(resp.body["snapshotVersion"], json!(7), "版本随响应回执");
    let replayed: Value = serde_json::from_str(resp.body["turnInputContentJson"].as_str().unwrap())
        .expect("turn input content json");
    assert_eq!(
        replayed["text"],
        json!("替换后的重放文本"),
        "可选替换文字生效: {replayed}"
    );

    // 重放后分支已回退：最近用户消息变为第一轮 → 纯重放回执原文与缺省版本 1。
    let resp = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/replay-s/latest-user-message/replay",
        Some(&json!({})),
    )
    .await;
    assert_eq!(resp.status, 200, "纯重放: {}", resp.body_text);
    assert_eq!(resp.body["turnInputMessageId"], json!(user1));
    assert_eq!(resp.body["snapshotVersion"], json!(1), "缺省版本 1");
    let replayed: Value =
        serde_json::from_str(resp.body["turnInputContentJson"].as_str().unwrap()).unwrap();
    assert_eq!(replayed["text"], json!("第一轮提问内容"));

    // 分支已无用户消息 → 明确 400（不编造重放点）。
    let resp = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/replay-s/latest-user-message/replay",
        Some(&json!({})),
    )
    .await;
    assert_eq!(resp.status, 400, "无用户回合: {}", resp.body_text);
    assert_eq!(
        resp.body["details"]["causeId"],
        json!("session.turn_target_invalid"),
        "{}",
        resp.body_text
    );

    // 无效输入：版本 0 / 空白替换文字 / 未知字段 / 畸形 JSON。
    for (label, body, raw) in [
        ("版本 0", json!({"snapshotVersion": 0}), false),
        ("空白替换", json!({"text": "   "}), false),
        ("未知字段", json!({"unknown": 1}), false),
        ("畸形 JSON", Value::Null, true),
    ] {
        let path = "/lingxi/v1/sessions/replay-s/latest-user-message/replay";
        let resp = if raw {
            http(
                &server,
                "POST",
                path,
                Some(&Value::String("{not json".to_string())),
                Some(&server.token),
            )
            .await
        } else {
            ok(&server, "POST", path, Some(&body)).await
        };
        assert_eq!(resp.status, 400, "{label}: {}", resp.body_text);
    }

    // 空会话（无 run）→ 400。
    create_session(&server, "replay-empty", "agent", "空").await;
    let resp = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/replay-empty/latest-user-message/replay",
        Some(&json!({})),
    )
    .await;
    assert_eq!(resp.status, 400, "空会话重放: {}", resp.body_text);

    // 归档会话 → 409 session_not_active（写路径生命周期闸）。
    create_session(&server, "replay-arch", "agent", "归档").await;
    execute_wait(&server, "replay-arch", "归档前的提问").await;
    ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/archive",
        Some(&json!({"sessionId": "replay-arch"})),
    )
    .await;
    let resp = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/replay-arch/latest-user-message/replay",
        Some(&json!({})),
    )
    .await;
    assert_eq!(resp.status, 409, "归档会话: {}", resp.body_text);
    assert_eq!(
        resp.body["details"]["reason"],
        json!("session_not_active"),
        "{}",
        resp.body_text
    );

    // 404 / 403 / 401。
    let resp = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/ghost/latest-user-message/replay",
        Some(&json!({})),
    )
    .await;
    assert_eq!(resp.status, 404);
    let foreign = mint_device_token(&server, "replay_foreign").await;
    let resp = http(
        &server,
        "POST",
        "/lingxi/v1/sessions/replay-empty/latest-user-message/replay",
        Some(&json!({})),
        Some(&foreign),
    )
    .await;
    assert_eq!(resp.status, 403);
    let resp = http(
        &server,
        "POST",
        "/lingxi/v1/sessions/replay-empty/latest-user-message/replay",
        Some(&json!({})),
        None,
    )
    .await;
    assert_eq!(resp.status, 401);

    server.stop_and_clean().await;
}

/// 运行中 409：run 持有会话 admission → replay 的 begin_command
/// try_acquire 拒绝 → session_busy；释放后重放成功。
/// execute POST 同步驱动 run 到终态，故并发发起（run 挂起在闸门 provider
/// 上时该 POST 保持未决，admission 全程被持有）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn latest_user_message_replay_returns_409_while_run_in_flight() {
    let (release, _gate) = tokio::sync::watch::channel(false);
    let release = Arc::new(release);
    let provider = GatedProvider {
        release: Arc::clone(&release),
    };
    let server = boot_with(
        "replay-busy",
        ServiceDeps {
            turn_provider: Some(Arc::new(provider) as Arc<dyn TurnProviderPort>),
            ..ServiceDeps::default()
        },
    )
    .await;
    create_session(&server, "busy-s", "agent", "忙").await;

    // 唯一一轮：provider 阻塞在闸门上，execute POST 并发发起、保持未决
    //（run 持有 admission 直到终态；user 输入锚在 run 开始时已提交）。
    let exec_task = {
        let addr = server.addr;
        let token = server.token.clone();
        tokio::spawn(async move {
            let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
            let payload = json!({ "input": "阻塞中的一轮提问" }).to_string();
            let request = format!(
                "POST /lingxi/v1/sessions/busy-s/execute HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len(),
            );
            socket.write_all(request.as_bytes()).await.unwrap();
            let mut response = Vec::new();
            socket.read_to_end(&mut response).await.unwrap();
            response
        })
    };

    // 等 run 落库且 admission 已被 run 持有（is_busy 是 admission 的公开
    // 观测面），再发重放。
    let run2 = {
        let mut latest = None;
        for _ in 0..400 {
            if !server.state.storage().session_admission().is_busy("busy-s") {
                tokio::time::sleep(Duration::from_millis(25)).await;
                continue;
            }
            latest = server
                .state
                .storage()
                .query_one_text(
                    "SELECT run_id FROM runs WHERE session_id = 'busy-s' ORDER BY created_at_unix_ms DESC, run_id DESC LIMIT 1",
                    vec![],
                )
                .await
                .ok()
                .flatten();
            break;
        }
        latest.expect("run 未在 10s 内持有会话 admission")
    };

    let resp = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/busy-s/latest-user-message/replay",
        Some(&json!({})),
    )
    .await;
    assert_eq!(resp.status, 409, "运行中重放: {}", resp.body_text);
    assert_eq!(
        resp.body["details"]["reason"],
        json!("session_busy"),
        "{}",
        resp.body_text
    );
    assert_eq!(resp.body["retryable"], json!(true));

    // 释放闸门 → run 完成（execute POST 返回）→ 重放成功（最近用户消息
    // = 阻塞轮的输入锚）。
    release.send_replace(true);
    let _ = tokio::time::timeout(Duration::from_secs(10), exec_task)
        .await
        .expect("execute 在释放后返回");
    wait_terminal(&server, &run2).await;
    let resp = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/busy-s/latest-user-message/replay",
        Some(&json!({})),
    )
    .await;
    assert_eq!(resp.status, 200, "空闲重放: {}", resp.body_text);
    assert_eq!(
        resp.body["turnInputMessageId"],
        json!(format!("user:{run2}")),
        "最近用户消息 = 阻塞轮的输入锚"
    );

    server.stop_and_clean().await;
}

// ═══════════════ CDBFE6616EB9：会话历史概览 ═══════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn history_overview_reports_counts_and_unready_availability() {
    let provider = ScriptedProvider::new(vec!["答复一".to_string(), "答复二".to_string()]);
    let server = boot_with(
        "overview",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            ..ServiceDeps::default()
        },
    )
    .await;
    create_session(&server, "ov-s", "agent", "概览").await;
    execute_wait(&server, "ov-s", "问题一").await;
    execute_wait(&server, "ov-s", "问题二").await;

    let resp = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/ov-s/history-overview",
        None,
    )
    .await;
    assert_eq!(resp.status, 200, "overview: {}", resp.body_text);
    assert_eq!(resp.body["schemaVersion"], json!(1));
    assert_eq!(resp.body["available"], json!(true));
    assert_eq!(resp.body["sessionId"], json!("ov-s"));
    assert_eq!(
        resp.body["counts"]["displayRecords"],
        json!(4),
        "2 user + 2 assistant displayable: {}",
        resp.body
    );
    assert!(
        resp.body["counts"]["sourceRecords"].as_u64().unwrap() >= 4,
        "源记录不少于 displayable"
    );
    assert_eq!(resp.body["counts"]["runsWithAssistant"], json!(2));
    assert_eq!(resp.body["counts"]["referencedTasks"], json!(0));
    assert_eq!(resp.body["taskDistribution"]["subagent"], json!(0));
    assert_eq!(resp.body["runSizeDistribution"]["oneTo50"], json!(2));
    assert_eq!(resp.body["runSizeDistribution"]["from51To200"], json!(0));
    assert_eq!(resp.body["runSizeDistribution"]["over200"], json!(0));
    assert_eq!(resp.body["pagination"]["legacyDefaultLimit"], json!(50));
    assert_eq!(resp.body["pagination"]["recommendedLimit"], json!(100));
    assert_eq!(resp.body["pagination"]["maxLimit"], json!(200));
    assert_eq!(
        resp.body["pagination"]["estimatedPagesAtRecommendedLimit"],
        json!(1)
    );
    assert_eq!(resp.header("lingxi-history-protocol"), Some("1"));
    assert_eq!(resp.header("cache-control"), Some("private, no-store"));
    let revision = resp.body["revision"].as_str().unwrap().to_string();
    assert!(revision.parse::<u64>().unwrap() > 0, "revision 非零");

    // 空会话：空是已知事实 → available:true + 零计数。
    create_session(&server, "ov-empty", "agent", "空概览").await;
    let resp = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/ov-empty/history-overview",
        None,
    )
    .await;
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body["available"], json!(true), "{}", resp.body_text);
    assert_eq!(resp.body["revision"], json!("0"));
    assert_eq!(resp.body["counts"]["displayRecords"], json!(0));
    assert_eq!(resp.body["counts"]["sourceRecords"], json!(0));

    // legacy 区：消息存在但无持久分支头行 → revision 事实不可靠 →
    // available:false + reason（200，非错误）。定向删除真实表上的头行模拟
    // 该既有数据形态（读取器明确支持的 legacy 区）。
    let db_path = server
        .home
        .join("lingxi-service")
        .join("data")
        .join("runs.db");
    {
        let conn = rusqlite::Connection::open(&db_path).expect("open runs.db");
        conn.execute(
            "DELETE FROM session_branch_heads WHERE session_id = ?1",
            ["ov-s"],
        )
        .expect("delete head row");
    }
    let resp = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/ov-s/history-overview",
        None,
    )
    .await;
    assert_eq!(resp.status, 200, "未就绪仍 200");
    assert_eq!(resp.body["available"], json!(false), "{}", resp.body_text);
    assert_eq!(resp.body["reason"], json!("revision_unknown"));

    // 404 / 403 / 401。
    let resp = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/ghost/history-overview",
        None,
    )
    .await;
    assert_eq!(resp.status, 404);
    let foreign = mint_device_token(&server, "ov_foreign").await;
    let resp = http(
        &server,
        "GET",
        "/lingxi/v1/sessions/ov-s/history-overview",
        None,
        Some(&foreign),
    )
    .await;
    assert_eq!(resp.status, 403);
    let resp = http(
        &server,
        "GET",
        "/lingxi/v1/sessions/ov-s/history-overview",
        None,
        None,
    )
    .await;
    assert_eq!(resp.status, 401);

    server.stop_and_clean().await;
}

// ═══════════════ ED089133C106：历史延迟内容 ═══════════════

/// provider 替身：Final 消息 = 固定文本块 + 共享槽中的 ResourceRef（槽为
/// 空则纯文本；附件须先经 HTTP 注册才有 sf_ 身份，故经槽延迟注入）。
/// 正文经 normalize_final_message 原样保留非文本块，最终消息提交进历史。
struct RefFinalProvider {
    refs: Arc<Mutex<Vec<ResourceRef>>>,
}

impl TurnProviderPort for RefFinalProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.provider".to_string(),
            model: "stub.model".to_string(),
            operation: "chat".to_string(),
        }
    }

    fn next_turn<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ModelCallId,
        _input: &'a ModelTurnInput,
        _deltas: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let mut content = vec![ContentBlock::Text {
            text: "引用附件的最终答复".to_string(),
        }];
        content.extend(self.refs.lock().unwrap().iter().map(|resource| {
            ContentBlock::ResourceRef {
                resource: resource.clone(),
            }
        }));
        let ctx_at_issue = ctx.clone();
        let call_id = call.to_string();
        Box::pin(async move {
            ProviderTurnResult::of_ctx(
                &ctx_at_issue,
                ProviderTurn::Final {
                    message: NormalizedMessage {
                        role: "assistant".to_string(),
                        content,
                        model_call_id: Some(ModelCallId::new(call_id)),
                    },
                },
            )
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_content_serves_only_history_referenced_resources() {
    // 附件 payload 必须在 lingxiHome 之外（安全闸封锁 home 内部路径）。
    let payload_dir = fresh_home("content-payload");
    let payload_a = payload_dir.join("attachment-a.txt");
    std::fs::write(&payload_a, "延迟大内容正文 ABC-123（附件 A）").expect("write payload a");
    let payload_b = payload_dir.join("attachment-b.txt");
    std::fs::write(&payload_b, "附件 B 的正文（不被引用）").expect("write payload b");

    let ref_slot: Arc<Mutex<Vec<ResourceRef>>> = Arc::new(Mutex::new(Vec::new()));
    let provider = RefFinalProvider {
        refs: Arc::clone(&ref_slot),
    };
    let server = boot_with(
        "content",
        ServiceDeps {
            turn_provider: Some(Arc::new(provider) as Arc<dyn TurnProviderPort>),
            ..ServiceDeps::default()
        },
    )
    .await;
    create_session(&server, "ct-s", "agent", "内容").await;
    create_session(&server, "ct-other", "agent", "旁观").await;

    async fn attach(server: &Server, session: &str, path: &Path) -> String {
        let resp = ok(
            server,
            "POST",
            &format!("/lingxi/v1/sessions/{session}/attachments/local"),
            Some(&json!({ "path": path.to_str().unwrap() })),
        )
        .await;
        assert_eq!(resp.status, 201, "attach: {}", resp.body_text);
        resp.body["file"]["fileId"].as_str().unwrap().to_string()
    }
    let sf_a = attach(&server, "ct-s", &payload_a).await;
    let sf_b = attach(&server, "ct-s", &payload_b).await;
    let res_a = format!("res_{sf_a}");
    let res_b = format!("res_{sf_b}");

    // 真实 run：最终消息携带 ResourceRef → 投影历史含资源引用块。
    *ref_slot.lock().unwrap() = vec![session_file_ref(&res_a)];
    execute_wait(&server, "ct-s", "读取附件").await;
    let history = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/ct-s/history?all=1",
        None,
    )
    .await;
    assert!(
        history.body_text.contains(&res_a),
        "投影历史的最终消息携带资源引用"
    );

    // 正路径：本会话投影历史内按 contentId 取大内容。
    let resp = ok(
        &server,
        "GET",
        &format!("/lingxi/v1/sessions/ct-s/content/{res_a}"),
        None,
    )
    .await;
    assert_eq!(resp.status, 200, "content: {}", resp.body_text);
    assert_eq!(resp.body["id"], json!(res_a));
    assert_eq!(resp.body["kind"], json!("resource"));
    assert_eq!(
        resp.body["content"],
        json!("延迟大内容正文 ABC-123（附件 A）"),
        "正文 = 真实文件字节"
    );

    // 资源存在但未出现在本会话投影历史 → 404（locator 不是授权凭证）。
    let resp = ok(
        &server,
        "GET",
        &format!("/lingxi/v1/sessions/ct-s/content/{res_b}"),
        None,
    )
    .await;
    assert_eq!(
        resp.status, 404,
        "已注册但未被本会话引用: {}",
        resp.body_text
    );
    // 引用只存在于 ct-s 的历史：经其他会话取 → 404。
    let resp = ok(
        &server,
        "GET",
        &format!("/lingxi/v1/sessions/ct-other/content/{res_a}"),
        None,
    )
    .await;
    assert_eq!(resp.status, 404, "跨会话按 id 取他人历史内容 404");
    // 未知资源 id → 404。
    let resp = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/ct-s/content/res_sf_ghost0000000000",
        None,
    )
    .await;
    assert_eq!(resp.status, 404);

    // 404 / 403 / 401。
    let resp = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/ghost/content/res_sf_x",
        None,
    )
    .await;
    assert_eq!(resp.status, 404);
    let foreign = mint_device_token(&server, "ct_foreign").await;
    let resp = http(
        &server,
        "GET",
        &format!("/lingxi/v1/sessions/ct-s/content/{res_a}"),
        None,
        Some(&foreign),
    )
    .await;
    assert_eq!(resp.status, 403);
    let resp = http(
        &server,
        "GET",
        &format!("/lingxi/v1/sessions/ct-s/content/{res_a}"),
        None,
        None,
    )
    .await;
    assert_eq!(resp.status, 401);

    let _ = std::fs::remove_dir_all(&payload_dir);
    server.stop_and_clean().await;
}

// ═══════════════ F0319FC4F93D：单会话滚动摘要 ═══════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_summary_returns_rolling_record_and_blank_is_not_a_summary() {
    let server = boot_default("summary").await;
    create_session(&server, "sum-s", "agentA", "摘要").await;

    // 缺记录：hasSummary:false，字段为 null（如实，不编造）。
    let resp = ok(&server, "GET", "/lingxi/v1/sessions/sum-s/summary", None).await;
    assert_eq!(resp.status, 200, "summary: {}", resp.body_text);
    assert_eq!(resp.body["hasSummary"], json!(false));
    assert_eq!(resp.body["summary"], Value::Null);
    assert_eq!(resp.body["createdAt"], Value::Null);
    assert_eq!(resp.body["updatedAt"], Value::Null);

    // TS 落盘契约：agents/{agentId}/memory/summaries/{sessionId}.json。
    let record_path = server
        .home
        .join("agents/agentA/memory/summaries/sum-s.json");
    std::fs::create_dir_all(record_path.parent().unwrap()).expect("mkdir summaries");
    std::fs::write(
        &record_path,
        serde_json::to_string(&json!({
            "summary": "  会话滚动摘要：完成了三件事。  ",
            "created_at": "2026-10-01T08:00:00.000Z",
            "updated_at": "2026-10-02T09:30:00.000Z",
        }))
        .unwrap(),
    )
    .expect("write summary record");

    let resp = ok(&server, "GET", "/lingxi/v1/sessions/sum-s/summary", None).await;
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body["hasSummary"], json!(true), "{}", resp.body_text);
    assert_eq!(
        resp.body["summary"],
        json!("  会话滚动摘要：完成了三件事。  "),
        "记录原文（trim 只参与非空判定）"
    );
    assert_eq!(resp.body["createdAt"], json!("2026-10-01T08:00:00.000Z"));
    assert_eq!(resp.body["updatedAt"], json!("2026-10-02T09:30:00.000Z"));

    // 空白 summary：非空才算 → hasSummary:false，summary 不回传。
    std::fs::write(
        &record_path,
        serde_json::to_string(&json!({
            "summary": "   ",
            "created_at": "2026-10-01T08:00:00.000Z",
            "updated_at": "2026-10-02T09:30:00.000Z",
        }))
        .unwrap(),
    )
    .expect("overwrite summary record");
    let resp = ok(&server, "GET", "/lingxi/v1/sessions/sum-s/summary", None).await;
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body["hasSummary"], json!(false), "空白不算");
    assert_eq!(resp.body["summary"], Value::Null);

    // 损坏 JSON：读面按契约回 null 记录（不抛 500）。
    std::fs::write(&record_path, "{broken").expect("break record");
    let resp = ok(&server, "GET", "/lingxi/v1/sessions/sum-s/summary", None).await;
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body["hasSummary"], json!(false), "损坏记录按缺失处理");

    // 404 / 403 / 401。
    let resp = ok(&server, "GET", "/lingxi/v1/sessions/ghost/summary", None).await;
    assert_eq!(resp.status, 404);
    let foreign = mint_device_token(&server, "sum_foreign").await;
    let resp = http(
        &server,
        "GET",
        "/lingxi/v1/sessions/sum-s/summary",
        None,
        Some(&foreign),
    )
    .await;
    assert_eq!(resp.status, 403);
    let resp = http(
        &server,
        "GET",
        "/lingxi/v1/sessions/sum-s/summary",
        None,
        None,
    )
    .await;
    assert_eq!(resp.status, 401);

    server.stop_and_clean().await;
}

// ═══════════════ F6A1A50E97E4：会话地图回合折叠 ═══════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conversation_map_turns_folds_question_answer_cards() {
    let long_question = format!("超长问题{}", "长".repeat(9_000));
    let provider = ScriptedProvider::new(vec!["第一轮答复".to_string(), "第二轮答复".to_string()]);
    let server = boot_with(
        "turns",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            ..ServiceDeps::default()
        },
    )
    .await;
    create_session(&server, "map-s", "agent", "回合").await;
    execute_wait(&server, "map-s", "第一个问题").await;
    execute_wait(&server, "map-s", &long_question).await;

    let resp = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/map-s/conversation-map/turns",
        None,
    )
    .await;
    assert_eq!(resp.status, 200, "turns: {}", resp.body_text);
    assert_eq!(resp.body["sessionId"], json!("map-s"));
    let turns = resp.body["turns"].as_array().expect("turns 数组").clone();
    assert_eq!(turns.len(), 2, "两问两答折叠为两张卡: {turns:?}");
    assert_eq!(turns[0]["turnIndex"], json!(0));
    assert_eq!(turns[0]["question"], json!("第一个问题"));
    assert_eq!(turns[0]["answer"], json!("第一轮答复"));
    assert_eq!(turns[0]["processCount"], json!(0));
    assert_eq!(turns[0]["truncated"], json!(false));
    assert!(
        turns[0]["questionEntryId"]
            .as_str()
            .unwrap()
            .starts_with("user:"),
        "问题锚 = 回合输入消息 id"
    );
    assert!(
        turns[0]["answerEntryId"]
            .as_str()
            .unwrap()
            .ends_with("-final"),
        "答案锚 = final 消息 id"
    );
    assert_eq!(
        turns[0]["entryIds"].as_array().map(Vec::len),
        Some(2),
        "entryIds 收集问题与答案身份: {turns:?}"
    );
    // 超限文本截断 + truncated 标注。
    assert_eq!(
        turns[1]["question"].as_str().unwrap().chars().count(),
        8_000,
        "超过 8000 字符截断"
    );
    assert_eq!(turns[1]["truncated"], json!(true));
    assert_eq!(turns[1]["answer"], json!("第二轮答复"));

    // 空会话：空卡片数组（如实，非错误）。
    create_session(&server, "map-empty", "agent", "空回合").await;
    let resp = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/map-empty/conversation-map/turns",
        None,
    )
    .await;
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body["turns"], json!([]));

    // 404 / 403 / 401（无越权读取）。
    let resp = ok(
        &server,
        "GET",
        "/lingxi/v1/sessions/ghost/conversation-map/turns",
        None,
    )
    .await;
    assert_eq!(resp.status, 404);
    let foreign = mint_device_token(&server, "map_foreign").await;
    let resp = http(
        &server,
        "GET",
        "/lingxi/v1/sessions/map-s/conversation-map/turns",
        None,
        Some(&foreign),
    )
    .await;
    assert_eq!(resp.status, 403);
    let resp = http(
        &server,
        "GET",
        "/lingxi/v1/sessions/map-s/conversation-map/turns",
        None,
        None,
    )
    .await;
    assert_eq!(resp.status, 401);

    server.stop_and_clean().await;
}

// ═══════════════ D62A81C2AEB2：fresh-compact ═══════════════

/// 默认 deps 无压缩服务 → 显式 503 拒绝（不静默降级为仅重建）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fresh_compact_refuses_503_when_compaction_service_not_wired() {
    let server = boot_default("fresh-503").await;
    create_session(&server, "fc-s", "agent", "压缩").await;
    let resp = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/fc-s/fresh-compact",
        Some(&json!({})),
    )
    .await;
    assert_eq!(resp.status, 503, "未接线 503: {}", resp.body_text);
    assert_eq!(
        resp.body["details"]["reason"],
        json!("compaction_service_unavailable"),
        "{}",
        resp.body_text
    );
    assert!(
        resp.body_text.contains("no real model chain"),
        "拒绝语义明示原因"
    );
    server.stop_and_clean().await;
}

/// 注入真实 CompactionService（真 ModelGateway/凭证/AuxiliaryExecutor →
/// loopback stub 摘要链）+ ContextCompilerService 后：空历史 noop、长历史
/// 真实压缩（stub 命中一次）+ 提示词工件重建；404/403/401/归档 409。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fresh_compact_compacts_and_rebuilds_prompt_through_real_service() {
    let stub = StubServer::start(vec![sse_final(&golden_summary())]).await;
    let runtime_dir = fresh_home("fresh-runtime");
    let product_dir = fresh_home("fresh-product");
    let home = fresh_home("fresh-pos");
    write_context_home_fixture(&home);
    write_context_product_fixture(&product_dir);
    // chat 路由声明 200_000 窗口：多轮驱动的恢复预算检查（runs.rs
    // restored_request_fits）在窗口过小时会拒绝后续 run；fresh-compact 的
    // 手动压缩按固定 keep_recent(20k) 切点，窗口只影响响应的 contextWindow。
    let compaction = build_compaction_service(&plane_json(&stub, 200_000), &runtime_dir);
    let provider = ScriptedProvider::new(vec![
        "z".repeat(40_000),
        "z".repeat(40_000),
        "z".repeat(40_000),
        "z".repeat(40_000),
        "z".repeat(40_000),
    ]);
    let deps = ServiceDeps {
        turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
        compaction_service: Some(compaction),
        context_compiler: Some(context_compiler(&home, &product_dir)),
        ..ServiceDeps::default()
    };
    let server = boot_at(home, deps).await;
    create_session(&server, "fc-empty", "lingxi", "空压缩").await;
    create_session(&server, "fc-big", "lingxi", "长压缩").await;
    for i in 0..5 {
        execute_wait(&server, "fc-big", &format!("第 {i} 轮长任务")).await;
    }

    // 空历史：Nothing to compact → noopReason，不碰摘要链。
    let resp = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/fc-empty/fresh-compact",
        Some(&json!({})),
    )
    .await;
    assert_eq!(resp.status, 200, "空压缩: {}", resp.body_text);
    assert_eq!(resp.body["ok"], json!(true));
    assert_eq!(resp.body["fresh"], json!(true));
    assert_eq!(resp.body["noopReason"], json!("nothing_to_compact"));
    assert_eq!(
        resp.body["tokensBefore"].as_u64(),
        resp.body["tokensAfter"].as_u64(),
        "noop 不改变估算"
    );
    assert_eq!(resp.body["contextWindow"], Value::Null);
    assert!(
        resp.body["rebuiltPrompt"]["materialSha256"]
            .as_str()
            .is_some_and(|sha| !sha.is_empty()),
        "提示词工件按当前材料重建: {}",
        resp.body
    );
    assert_eq!(stub.hits(), 0, "noop 不调摘要模型");

    // 长历史：真实压缩（旧上下文换摘要）+ 重建工件。
    let resp = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/fc-big/fresh-compact",
        Some(&json!({})),
    )
    .await;
    assert_eq!(resp.status, 200, "长压缩: {}", resp.body_text);
    assert_eq!(resp.body["ok"], json!(true));
    assert_eq!(resp.body["fresh"], json!(true));
    assert_eq!(resp.body["noopReason"], Value::Null);
    let before = resp.body["tokensBefore"].as_u64().expect("tokensBefore");
    let after = resp.body["tokensAfter"].as_u64().expect("tokensAfter");
    assert!(before > 20_000, "旧上下文远超保留线: {before}");
    assert!(
        after < before,
        "压缩后估算必须下降: before={before} after={after}"
    );
    assert_eq!(resp.body["contextWindow"], json!(200_000), "chat 路由窗口");
    assert!(
        resp.body["rebuiltPrompt"]["materialSha256"]
            .as_str()
            .is_some_and(|sha| !sha.is_empty()),
        "压缩后仍重建提示词工件"
    );
    assert_eq!(stub.hits(), 1, "恰好一次摘要调用（经真实网关链）");
    // 台账：摘要调用落账（真实 CompactionService 经 StoragePort 记 usage）。
    let summary_rows: usize = server
        .state
        .storage()
        .query_model_call_usage(lingxi_kernel::usage::ModelUsageQuery {
            purpose: Some("auxiliary.summarize".to_string()),
            ..lingxi_kernel::usage::ModelUsageQuery::default()
        })
        .await
        .expect("usage query")
        .len();
    assert_eq!(summary_rows, 1, "摘要调用入台账");

    // 负路径（注入服务后闸可达）：未知 404 / 跨主体 403 / 无凭证 401 /
    // 归档 409。
    let resp = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/ghost/fresh-compact",
        Some(&json!({})),
    )
    .await;
    assert_eq!(resp.status, 404);
    let foreign = mint_device_token(&server, "fc_foreign").await;
    let resp = http(
        &server,
        "POST",
        "/lingxi/v1/sessions/fc-big/fresh-compact",
        Some(&json!({})),
        Some(&foreign),
    )
    .await;
    assert_eq!(resp.status, 403);
    let resp = http(
        &server,
        "POST",
        "/lingxi/v1/sessions/fc-big/fresh-compact",
        Some(&json!({})),
        None,
    )
    .await;
    assert_eq!(resp.status, 401);
    create_session(&server, "fc-arch", "lingxi", "归档压缩").await;
    ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/archive",
        Some(&json!({"sessionId": "fc-arch"})),
    )
    .await;
    let resp = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/fc-arch/fresh-compact",
        Some(&json!({})),
    )
    .await;
    assert_eq!(resp.status, 409, "归档会话: {}", resp.body_text);
    assert_eq!(resp.body["details"]["reason"], json!("session_not_active"));

    stub.stop().await;
    let _ = std::fs::remove_dir_all(&runtime_dir);
    let _ = std::fs::remove_dir_all(&product_dir);
    server.stop_and_clean().await;
}

/// 运行中拒绝：run 持有会话 admission → fresh-compact 的 try_acquire
/// 拒绝 → 409；释放后可执行（压缩服务已注入，闸真实可达）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fresh_compact_returns_409_while_session_streaming() {
    let stub = StubServer::start(Vec::new()).await;
    let runtime_dir = fresh_home("fresh-busy-runtime");
    let compaction = build_compaction_service(&plane_json(&stub, 1_000), &runtime_dir);
    let (release, _gate) = tokio::sync::watch::channel(false);
    let release = Arc::new(release);
    let provider = GatedProvider {
        release: Arc::clone(&release),
    };
    let deps = ServiceDeps {
        turn_provider: Some(Arc::new(provider) as Arc<dyn TurnProviderPort>),
        compaction_service: Some(compaction),
        ..ServiceDeps::default()
    };
    let server = boot_with("fresh-busy", deps).await;
    create_session(&server, "fb-s", "agent", "流式压缩").await;

    // execute POST 同步驱动 run 到终态，并发发起：provider 阻塞在闸门上，
    // run 持有 admission，POST 保持未决。
    let exec_task = {
        let addr = server.addr;
        let token = server.token.clone();
        tokio::spawn(async move {
            let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
            let payload = json!({ "input": "阻塞中的任务" }).to_string();
            let request = format!(
                "POST /lingxi/v1/sessions/fb-s/execute HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len(),
            );
            socket.write_all(request.as_bytes()).await.unwrap();
            let mut response = Vec::new();
            socket.read_to_end(&mut response).await.unwrap();
            response
        })
    };
    let run_id = {
        let mut latest = None;
        for _ in 0..400 {
            if !server.state.storage().session_admission().is_busy("fb-s") {
                tokio::time::sleep(Duration::from_millis(25)).await;
                continue;
            }
            latest = server
                .state
                .storage()
                .query_one_text(
                    "SELECT run_id FROM runs WHERE session_id = 'fb-s' ORDER BY created_at_unix_ms DESC, run_id DESC LIMIT 1",
                    vec![],
                )
                .await
                .ok()
                .flatten();
            break;
        }
        latest.expect("run 未在 10s 内持有会话 admission")
    };

    let resp = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/fb-s/fresh-compact",
        Some(&json!({})),
    )
    .await;
    assert_eq!(resp.status, 409, "运行中压缩: {}", resp.body_text);
    assert_eq!(
        resp.body["details"]["reason"],
        json!("session_busy"),
        "{}",
        resp.body_text
    );

    release.send_replace(true);
    let _ = tokio::time::timeout(Duration::from_secs(10), exec_task)
        .await
        .expect("execute 在释放后返回");
    wait_terminal(&server, &run_id).await;
    // 空闲后：压缩服务已接线但无 context_compiler → 显式 503（不静默），
    // 证明闸序（busy 在 compiler 之前）与诚实降级。
    let resp = ok(
        &server,
        "POST",
        "/lingxi/v1/sessions/fb-s/fresh-compact",
        Some(&json!({})),
    )
    .await;
    assert_eq!(resp.status, 503, "空闲后: {}", resp.body_text);
    assert_eq!(
        resp.body["details"]["reason"],
        json!("context_compiler_unavailable"),
        "{}",
        resp.body_text
    );
    assert_eq!(stub.hits(), 0, "全程未调摘要模型");

    stub.stop().await;
    let _ = std::fs::remove_dir_all(&runtime_dir);
    server.stop_and_clean().await;
}

// ═══════════════ 5DCB0352EB08：SessionCollabTools ═══════════════

/// 保留旧构造接口的旁路哨兵；正式链路必须不调用这里的投递或建会话。
/// create 调用记录：(agent_id, model, title)。
type CreateCall = (String, Option<String>, Option<String>);

struct RecordingDelivery {
    deliveries: Mutex<Vec<(String, String)>>,
    creates: Mutex<Vec<CreateCall>>,
    fail_first_deliver_to: Mutex<Option<String>>,
    next_seq: AtomicUsize,
}

impl RecordingDelivery {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            deliveries: Mutex::new(Vec::new()),
            creates: Mutex::new(Vec::new()),
            fail_first_deliver_to: Mutex::new(None),
            next_seq: AtomicUsize::new(0),
        })
    }

    fn deliveries(&self) -> Vec<(String, String)> {
        self.deliveries.lock().unwrap().clone()
    }

    fn creates(&self) -> Vec<CreateCall> {
        self.creates.lock().unwrap().clone()
    }
}

impl SessionCollabDelivery for RecordingDelivery {
    fn deliver<'a>(
        &'a self,
        target_session_id: &'a str,
        message: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            let mut fail = self.fail_first_deliver_to.lock().unwrap();
            if fail.as_deref() == Some(target_session_id) {
                *fail = None;
                return Err(format!(
                    "deliver to {target_session_id} failed (first attempt)"
                ));
            }
            self.deliveries
                .lock()
                .unwrap()
                .push((target_session_id.to_string(), message.to_string()));
            Ok(())
        })
    }

    fn create_session<'a>(
        &'a self,
        agent_id: &'a str,
        model: Option<&'a str>,
        title: Option<&'a str>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send + 'a>> {
        Box::pin(async move {
            let seq = self.next_seq.fetch_add(1, Ordering::SeqCst);
            let id = format!("new-collab-session-{seq}");
            self.creates.lock().unwrap().push((
                agent_id.to_string(),
                model.map(str::to_string),
                title.map(str::to_string),
            ));
            Ok(id)
        })
    }
}

fn collab_ctx(session_id: &str, principal: lingxi_kernel::Principal) -> RunContext {
    RunContext {
        principal,
        session_id: lingxi_protocol::SessionId::new(session_id.to_string()),
        run_id: lingxi_protocol::RunId::new("collab-run"),
        attempt: lingxi_protocol::AttemptId::new("collab-attempt"),
        generation: 1,
    }
}

fn outcome_text(outcome: &ToolOutcome) -> String {
    match outcome {
        ToolOutcome::Success { result } => match result.content.first() {
            Some(ContentBlock::Text { text }) => text.clone(),
            other => panic!("expected text content, got {other:?}"),
        },
        ToolOutcome::Failed { error } => panic!("unexpected failure: {}", error.message),
        other => panic!("unexpected outcome: {other:?}"),
    }
}

fn outcome_failure_message(outcome: &ToolOutcome) -> String {
    match outcome {
        ToolOutcome::Failed { error } => error.message.clone(),
        other => panic!("expected failure, got {other:?}"),
    }
}

fn draft_id_of(text: &str) -> String {
    let start = text.find("Draft created (").expect("draft id 前缀") + "Draft created (".len();
    let end = text[start..].find(')').expect("draft id 后缀") + start;
    text[start..end].to_string()
}

async fn wait_collab_accepted(
    server: &Server,
    id: &str,
) -> lingxi_adapters::storage::conversations_collab::CollabDraft {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let draft = lingxi_adapters::storage::conversations_collab::get_collab_draft(server.state.storage(), id).await.unwrap().unwrap();
            if draft.delivery_status == lingxi_adapters::storage::conversations_collab::CollabDeliveryStatus::Accepted { return draft; }
            assert!(matches!(draft.delivery_status, lingxi_adapters::storage::conversations_collab::CollabDeliveryStatus::Queued | lingxi_adapters::storage::conversations_collab::CollabDeliveryStatus::Started), "真实协作受理失败: {draft:?}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("协作确认后真实受理")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_collab_tools_scope_drafts_and_delivery_semantics() {
    let provider =
        ScriptedProvider::new(vec!["本会话答复一".to_string(), "本会话答复二".to_string()]);
    let server = boot_with(
        "collab",
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            ..ServiceDeps::default()
        },
    )
    .await;
    create_session(&server, "src-s", "agent", "协作源").await;
    execute_wait(&server, "src-s", "源会话问题一").await;
    let source_run_id = execute_wait(&server, "src-s", "源会话问题二").await;
    create_session(&server, "peer-s", "agent", "协作目标").await;
    // 他人会话（跨主体范围受控测试）。
    lingxi_adapters::storage::session_admin::create_session(
        server.state.storage(),
        "foreign-s",
        "agent",
        "someone_else",
        "他人会话",
        None,
        &[],
        true,
        1,
    )
    .await
    .expect("seed foreign session");
    // 建助手（create action 的 roster 解析）。
    let resp = ok(
        &server,
        "POST",
        "/lingxi/v1/agents",
        Some(&json!({"name": "协作助手", "id": "collabA"})),
    )
    .await;
    assert_eq!(resp.status, 200, "create agent: {}", resp.body_text);
    // 摘要记录（read summary 模式的真实数据源）。
    let summary_path = server
        .home
        .join("agents/agent/memory/summaries/peer-s.json");
    std::fs::create_dir_all(summary_path.parent().unwrap()).unwrap();
    std::fs::write(
        &summary_path,
        serde_json::to_string(&json!({
            "summary": "目标会话的既有摘要",
            "updated_at": "2026-10-03T10:00:00.000Z"
        }))
        .unwrap(),
    )
    .unwrap();

    let delivery = RecordingDelivery::new();
    let tools = SessionCollabTools::new(
        server.state.clone(),
        delivery.clone() as Arc<dyn SessionCollabDelivery>,
        vec![
            "stub.provider/model-a".to_string(),
            "stub.provider/model-b".to_string(),
        ],
    );
    let mut local = collab_ctx("src-s", lingxi_kernel::Principal::LocalUser);
    local.run_id = lingxi_protocol::RunId::new(source_run_id);
    let device_other = collab_ctx(
        "device-s",
        lingxi_kernel::Principal::Device {
            device_id: "dev-1".to_string(),
            user_id: "someone_else".to_string(),
        },
    );

    // ── 手册 ──
    let text = outcome_text(&tools.run_action(&local, &json!({"action": "?"})).await);
    assert!(text.contains("list") && text.contains("send") && text.contains("create"));

    // ── list：范围受控（跨会话范围 = 主体归属；本地主人是整机主人，
    // 设备/网络主体只按 owner_user_id 归属过滤——与 HTTP can_access 同口径） ──
    let text = outcome_text(&tools.run_action(&local, &json!({"action": "list"})).await);
    assert!(
        text.contains("src-s") && text.contains("peer-s") && text.contains("foreign-s"),
        "本地主人可见全部会话: {text}"
    );
    let text = outcome_text(
        &tools
            .run_action(&local, &json!({"action": "list", "query": "协作目标"}))
            .await,
    );
    assert!(
        text.contains("peer-s") && !text.contains("src-s"),
        "query 过滤"
    );
    let text = outcome_text(
        &tools
            .run_action(&local, &json!({"action": "list", "query": "不存在"}))
            .await,
    );
    assert!(text.contains("No sessions matched"), "{text}");
    // 设备主体：只见自己归属的会话（范围受控的授权边界）。
    let text = outcome_text(
        &tools
            .run_action(&device_other, &json!({"action": "list"}))
            .await,
    );
    assert!(
        text.contains("foreign-s") && !text.contains("src-s") && !text.contains("peer-s"),
        "设备主体只归属过滤: {text}"
    );

    // ── read：summary / transcript / 越权与不存在 ──
    let text = outcome_text(
        &tools
            .run_action(&local, &json!({"action": "read", "sessionId": "peer-s"}))
            .await,
    );
    assert!(
        text.contains("目标会话的既有摘要") && text.contains("updated 2026-10-03"),
        "summary 模式回真实记录: {text}"
    );
    let text = outcome_text(
        &tools
            .run_action(&local, &json!({"action": "read", "sessionId": "src-s"}))
            .await,
    );
    assert!(
        text.contains("no summary exists") && !text.contains("本会话答复"),
        "无记录如实说明，不按需生成: {text}"
    );
    let text = outcome_text(
        &tools
            .run_action(
                &local,
                &json!({"action": "read", "sessionId": "src-s", "mode": "transcript"}),
            )
            .await,
    );
    assert!(
        text.contains("2 turns")
            && text.contains("Q: 源会话问题一")
            && text.contains("A: 本会话答复一"),
        "transcript 模式折叠问答: {text}"
    );
    for (label, principal, session) in [
        (
            "越权读",
            lingxi_kernel::Principal::Device {
                device_id: "dev-1".to_string(),
                user_id: "someone_else".to_string(),
            },
            "src-s",
        ),
        ("读不存在", lingxi_kernel::Principal::LocalUser, "ghost-s"),
    ] {
        let ctx = collab_ctx("src-s", principal.clone());
        let outcome = tools
            .run_action(&ctx, &json!({"action": "read", "sessionId": session}))
            .await;
        let message = outcome_failure_message(&outcome);
        assert!(
            message.contains(&format!("Session not found: {session}")),
            "{label} 与不存在同措辞（不泄露存在性）: {message}"
        );
    }
    let outcome = tools.run_action(&local, &json!({"action": "read"})).await;
    assert!(outcome_failure_message(&outcome).contains("sessionId is required"));

    // ── send：确认草稿 + apply 投递到指定目标 ──
    let outcome = tools
        .run_action(
            &local,
            &json!({"action": "send", "sessionId": "peer-s", "message": "给目标的第一条消息"}),
        )
        .await;
    let text = outcome_text(&outcome);
    assert!(text.contains("Draft created"), "send 产确认草稿: {text}");
    let send_draft = draft_id_of(&text);
    assert!(
        matches!(
            tools.apply_draft(&send_draft).await,
            DraftApplyOutcome::Queued { .. }
        ),
        "确认先持久排队"
    );
    lingxi_service::session_collab::tick(&server.state)
        .await
        .unwrap();
    let sent = wait_collab_accepted(&server, &send_draft).await;
    assert_eq!(sent.target_session_id.as_deref(), Some("peer-s"));
    wait_terminal(&server, sent.accepted_run_id.as_deref().unwrap()).await;
    assert!(delivery.deliveries().is_empty(), "生产链不调用内存投递器");
    assert_eq!(
        tools.apply_draft(&send_draft).await,
        DraftApplyOutcome::Sent {
            target_session_id: "peer-s".into()
        },
        "重复确认返回同一真实回执"
    );
    // 拒绝向当前会话自身发送。
    let outcome = tools
        .run_action(
            &local,
            &json!({"action": "send", "sessionId": "src-s", "message": "自指"}),
        )
        .await;
    assert!(outcome_failure_message(&outcome).contains("Refusing to send"));
    // 越权 / 不存在 / 缺参数。
    for (label, principal, params) in [
        (
            "send 越权",
            lingxi_kernel::Principal::Device {
                device_id: "dev-1".to_string(),
                user_id: "someone_else".to_string(),
            },
            json!({"action": "send", "sessionId": "peer-s", "message": "x"}),
        ),
        (
            "send 不存在",
            lingxi_kernel::Principal::LocalUser,
            json!({"action": "send", "sessionId": "ghost-s", "message": "x"}),
        ),
        (
            "send 缺参数",
            lingxi_kernel::Principal::LocalUser,
            json!({"action": "send", "sessionId": "peer-s"}),
        ),
    ] {
        let ctx = collab_ctx("src-s", principal.clone());
        let outcome = tools.run_action(&ctx, &params).await;
        assert!(
            outcome_failure_message(&outcome).contains("usage error"),
            "{label}: {:?}",
            outcome
        );
    }

    // ── create：roster / 模型校验 / 草稿 / apply 全程与半成功 ──
    let outcome = tools
        .run_action(
            &local,
            &json!({"action": "create", "agent": "ghost-agent", "message": "首条"}),
        )
        .await;
    let message = outcome_failure_message(&outcome);
    assert!(
        message.contains("Unknown agent") && message.contains("collabA"),
        "{message}"
    );
    let outcome = tools
        .run_action(
            &local,
            &json!({"action": "create", "agent": "collabA", "model": "stub.provider/missing", "message": "首条"}),
        )
        .await;
    assert!(
        outcome_failure_message(&outcome).contains("session_create_invalid_model"),
        "模型无效分开报告"
    );
    let outcome = tools
        .run_action(
            &local,
            &json!({"action": "create", "agent": "协作助手", "title": "新协作会话", "message": "首条消息"}),
        )
        .await;
    let text = outcome_text(&outcome);
    assert!(
        text.contains("Draft created") && text.contains("collabA"),
        "{text}"
    );
    let create_draft = draft_id_of(&text);
    assert!(matches!(
        tools.apply_draft(&create_draft).await,
        DraftApplyOutcome::Queued { .. }
    ));
    lingxi_service::session_collab::tick(&server.state)
        .await
        .unwrap();
    let created = wait_collab_accepted(&server, &create_draft).await;
    let created_sid = created.target_session_id.unwrap();
    assert!(created.target_created);
    wait_terminal(&server, created.accepted_run_id.as_deref().unwrap()).await;
    let row = lingxi_adapters::storage::session_tree::get_session_row(
        server.state.storage(),
        &created_sid,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(row.agent_id, "collabA");
    assert_eq!(row.title, "新协作会话");
    assert!(delivery.creates().is_empty(), "创建走真实SessionAdmin");
    assert_eq!(
        tools.apply_draft(&create_draft).await,
        DraftApplyOutcome::Created {
            session_id: created_sid
        },
        "重复确认不另建会话"
    );

    // 未知 action。
    let outcome = tools.run_action(&local, &json!({"action": "bogus"})).await;
    assert!(outcome_failure_message(&outcome).contains("Unknown action"));

    server.stop_and_clean().await;
}
