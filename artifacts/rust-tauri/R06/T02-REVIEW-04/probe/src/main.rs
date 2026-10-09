//! REVIEWER-R06-T02-R4 的独立对抗探针（第四轮，全部为新构造组合，不重放
//! R1 孤儿/双计/级联、R2 pending 中段/USAGE_MAPPINGS/触发边界/切点方向、
//! R3 fit 退化面/护栏分层/placeholder×enrichment/族注入/槽缺失/手动续传/
//! 交叉锁鉴别力/族矩阵）。
//!
//! 本轮探针围绕 R3-F-01（生效摘要路由：summarize 槽显式覆盖 + chat 回退 +
//! 双缺响亮失败）的**未覆盖组合**：
//!   P1 — 回退成功但 chat 窗口本身太小：自动路径 × 无 summarize 槽 ×
//!        chat 窗口 1000 → 回退后 fit 仍不足 → HardTruncated（诚实降级臂
//!        在回退路由上仍成立；R3 修复测试只覆盖「回退后窗口足够」与
//!        「双缺」，未覆盖「回退后窗口仍不足」）。
//!   P2 — placeholder 单次恢复 × chat 回退链：意图 turn + 恢复调用两发都
//!        走 chat 路由（线体 model、台账身份、placeholder 逐字、
//!        emitted_tool_calls 纪律）。R3 探针 C 走 summarize 槽路径。
//!   P3 — 自动路径 × chat 与 summarize 双缺 → 触发门 NotTriggered（实证
//!        双缺 Failed 臂在自动路径结构上不可达；手动路径才可达）。
//!   P4 — 手动 compact_now × 无 summarize 槽 × chat 窗口未声明 →
//!        fit 窗口 0 → HardTruncated 且 mid_run=false（手动 × 回退 ×
//!        窗口未声明三重组合无既有测试）。
//!   P5 — 手动两轮 × 回退路由：fileOps 跨纪元续传（播种 ∪ 新增）在
//!        chat 回退链上成立。R3 探针 E 走 summarize 槽路径。
//!   P6 — 修复臂（settle 第三调用点）× 回退路由：无效摘要 → 一次修复，
//!        两发都走 chat 路由，repair 载荷内嵌当轮原始文本。
//!   P7 — 交叉锁「正文稀释」演示（纯逻辑）：条目编号集合不变、正文掏空
//!        → 锁仍绿。锁的声明契约只管锚定存在性（编号双向匹配），不管
//!        正文保真——记录为 informational，非缺陷。

use lingxi_kernel::compaction;
use lingxi_kernel::model_exchange::{
    ExchangeItem, RequestedToolCall, ToolDeclaration, ToolDeclarationSnapshot,
};
use lingxi_kernel::ports::{StoragePort, ToolOutcome, ToolRequest};
use lingxi_kernel::toolcatalog::SchemaBudget;
use lingxi_kernel::usage::{CallOutcome, ModelCallUsage, ModelCallUsageRecord, ModelUsageQuery};
use lingxi_kernel::RunContext;
use lingxi_protocol::{ContentBlock, ModelCallId, ToolCallId};
use lingxi_service::compaction::{CompactionOutcome, CompactionService, MidRunCompactionInput};
use serde_json::json;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

const STUB_API_KEY: &str = "stub-key-not-a-secret";

// ─── 夹具 ───

fn requested_call(seq: u32, target: &str, path: &str) -> RequestedToolCall {
    let request = ToolRequest::from_effective_arguments(
        target,
        json!({ "path": path }),
        &SchemaBudget::default(),
    )
    .expect("effective arguments")
    .with_provider_call_id(format!("pc{seq:04}"));
    RequestedToolCall {
        tool_call_id: ToolCallId::new(format!("tc{seq:04}")),
        provider_call_id: request.provider_call_id.clone(),
        target: request.target.clone(),
        arguments: request.arguments.clone(),
        args_digest: request.args_digest,
        args_summary: None,
    }
}

fn assistant_turn(seq: u32, text: &str, calls: Vec<RequestedToolCall>) -> ExchangeItem {
    ExchangeItem::AssistantTurn {
        call: ModelCallId::new(format!("mc{seq:04}")),
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        tool_calls: calls,
        origin: None,
    }
}

fn tool_result(seq: u32, text: &str) -> ExchangeItem {
    ExchangeItem::ToolResult {
        tool_call_id: ToolCallId::new(format!("tc{seq:04}")),
        provider_call_id: Some(format!("pc{seq:04}")),
        outcome: ToolOutcome::success_text(text),
    }
}

/// 配对完整的长历史：`groups` 组 [assistant(带一个 read 调用), 结果]。
fn long_exchange(groups: u32, chunk: usize) -> Vec<ExchangeItem> {
    let mut exchange = Vec::new();
    for g in 0..groups {
        let seq = 1000 + g * 2 + 1;
        exchange.push(assistant_turn(
            seq,
            &format!("assistant chunk {g} {}", "a".repeat(chunk)),
            vec![requested_call(seq, "tool:first-party:read", "/f/probe")],
        ));
        exchange.push(tool_result(
            seq,
            &format!("result {g} {}", "r".repeat(chunk)),
        ));
    }
    exchange
}

fn read_snapshot() -> ToolDeclarationSnapshot {
    let schema = || lingxi_protocol::ToolSchemaDocument {
        dialect: "json-schema/2020-12".to_string(),
        schema: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
    };
    ToolDeclarationSnapshot {
        catalog_generation: 1,
        declarations: vec![
            ToolDeclaration {
                target: "tool:first-party:read".to_string(),
                wire_name: "read".to_string(),
                description: "Read a file".to_string(),
                input_schema: schema(),
            },
            ToolDeclaration {
                target: "tool:first-party:write".to_string(),
                wire_name: "write".to_string(),
                description: "Write a file".to_string(),
                input_schema: schema(),
            },
        ],
    }
}

fn test_ctx() -> RunContext {
    RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("sess-r4"),
        run_id: lingxi_protocol::RunId::new("run-r4"),
        attempt: lingxi_protocol::AttemptId::new("attempt-r4"),
        generation: 1,
    }
}

fn golden_summary() -> String {
    "## Goal\n完成用户请求\n\n## Constraints & Preferences\n- (none)\n\n\
     ## Progress\n### Done\n- [x] 读取文件\n\n### In Progress\n- [ ] 汇总\n\n\
     ### Blocked\n- 无\n\n## Key Decisions\n- **用真实工具**: 避免猜测\n\n\
     ## Next Steps\n1. 写出结论\n\n## Critical Context\n- (none)\n"
        .to_string()
}

// ─── loopback stub（一连接一请求，脚本化 SSE 应答） ───

struct StubResponse {
    body: String,
}

struct StubServer {
    addr: SocketAddr,
    hits: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<String>>>,
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
        let requests: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let responses = Arc::new(Mutex::new(std::collections::VecDeque::from(responses)));
        let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let (task_hits, task_requests, task_responses) = (
            Arc::clone(&hits),
            Arc::clone(&requests),
            Arc::clone(&responses),
        );
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
                let Some(header_end) = header_end else { continue };
                let text = String::from_utf8_lossy(&raw[..header_end]).to_string();
                let (_head, body) = text
                    .split_once("\r\n\r\n")
                    .map(|(h, b)| (h.to_string(), b.to_string()))
                    .unwrap_or((text, String::new()));
                task_requests.lock().expect("requests").push(body);
                let next = task_responses.lock().expect("responses").pop_front();
                let payload = next
                    .map(|response| response.body)
                    .unwrap_or_else(|| "{\"error\":{\"message\":\"stub script exhausted\"}}".into());
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
            requests,
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

    fn bodies(&self) -> Vec<String> {
        self.requests.lock().expect("requests").clone()
    }

    async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = tokio::time::timeout(Duration::from_secs(5), &mut self.task).await;
    }
}

fn sse_final(text: &str) -> StubResponse {
    let mut body = String::new();
    for frame in [
        json!({"id":"chatcmpl-r4","model":"stub-model","choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]}),
        json!({"id":"chatcmpl-r4","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
        json!({"id":"chatcmpl-r4","choices":[],"usage":{"prompt_tokens":17,"completion_tokens":5}}),
    ] {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    StubResponse { body }
}

fn sse_tool_calls(calls: &[(&str, &str, &str)]) -> StubResponse {
    let tool_calls: Vec<serde_json::Value> = calls
        .iter()
        .enumerate()
        .map(|(index, (id, name, arguments))| {
            json!({
                "index": index,
                "id": id,
                "type": "function",
                "function": {"name": name, "arguments": arguments}
            })
        })
        .collect();
    let mut body = String::new();
    for frame in [
        json!({"id":"chatcmpl-r4","model":"stub-model","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":tool_calls},"finish_reason":null}]}),
        json!({"id":"chatcmpl-r4","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
        json!({"id":"chatcmpl-r4","choices":[],"usage":{"prompt_tokens":21,"completion_tokens":7}}),
    ] {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    StubResponse { body }
}

// ─── plane / service 构造 ───

/// chat-only 平面（summarize 槽整体缺失）——R3-F-01 回退链的臂 A 配置。
fn plane_chat_only(stub: &StubServer, chat_window: Option<u64>) -> String {
    let chat_compat = match chat_window {
        Some(window) => format!(r#", "compat": {{"contextWindow": {window}}}"#),
        None => String::new(),
    };
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
                "chat": {{"provider": "stub_svc", "model": "chat-model", "capabilities": {{"tools": true}}{chat_compat}}}
            }}
        }}"#,
        stub.v1()
    )
}

/// 空平面：chat 与 summarize 皆无（双缺配置）。
fn plane_no_models(stub: &StubServer) -> String {
    format!(
        r#"{{
            "providers": {{
                "stub_svc": {{
                    "protocol": "openai-completions",
                    "endpoint": "{}",
                    "auth": {{"kind": "apiKey", "apiKey": "{STUB_API_KEY}"}}
                }}
            }},
            "models": {{}}
        }}"#,
        stub.v1()
    )
}

fn build_service(plane: &str, runtime_dir: &std::path::Path) -> Arc<CompactionService> {
    let plane = lingxi_adapters::models::config::ModelPlaneConfig::parse_and_validate(plane)
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
    Arc::new(CompactionService::new(
        Arc::new(executor),
        gateway,
        Arc::new(lingxi_service::quotas::QuotaManager::new(
            lingxi_service::quotas::QuotaLimits::default(),
        )),
        Arc::new(lingxi_service::inject::SystemClock),
    ))
}

static DIR_SEQ: AtomicUsize = AtomicUsize::new(0);

fn fresh_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r06t02-r4-{tag}-{}-{}",
        std::process::id(),
        DIR_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create fixture dir");
    dir
}

async fn boot_storage(tag: &str) -> (lingxi_service::ServiceState, PathBuf) {
    let home = fresh_dir(tag);
    let config = lingxi_service::ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static addr"),
        data_home: home.clone(),
        home_source: lingxi_service::HomeSource::Cli,
        network_mode: lingxi_service::NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let layout = lingxi_service::prepare_layout(&home).expect("layout");
    let state = lingxi_service::ServiceState::bootstrap_with_deps(
        config,
        &layout,
        lingxi_service::ServiceDeps::default(),
    )
    .await
    .expect("bootstrap");
    (state, home)
}

async fn teardown(state: &lingxi_service::ServiceState, home: &PathBuf) {
    state.storage().close().await.expect("close storage");
    let _ = std::fs::remove_dir_all(home);
}

async fn ledger_rows(
    state: &lingxi_service::ServiceState,
    purpose: &str,
) -> Vec<ModelCallUsageRecord> {
    state
        .storage()
        .query_model_call_usage(ModelUsageQuery {
            purpose: Some(purpose.to_string()),
            ..ModelUsageQuery::default()
        })
        .await
        .expect("usage query")
}

fn auto_input<'a>(
    exchange: &'a [ExchangeItem],
    tools: &'a ToolDeclarationSnapshot,
    usage: Option<&'a ModelCallUsage>,
) -> MidRunCompactionInput<'a> {
    MidRunCompactionInput {
        exchange,
        system_prompt: None,
        submission: "汇总",
        tools,
        last_usage: usage,
        tail_from: exchange.len(),
    }
}

fn manual_input<'a>(
    exchange: &'a [ExchangeItem],
    tools: &'a ToolDeclarationSnapshot,
) -> MidRunCompactionInput<'a> {
    MidRunCompactionInput {
        exchange,
        system_prompt: None,
        submission: "手动压缩",
        tools,
        last_usage: None,
        tail_from: exchange.len(),
    }
}

// ─── P1：回退成功但 chat 窗口仍不足 → HardTruncated（0 调用） ───

async fn probe_p1_fallback_window_still_too_small() {
    println!("── P1. 无 summarize 槽 × chat 窗口 1000：回退后 fit 仍不足 → HardTruncated");
    let stub = StubServer::start(vec![]).await; // 不应有任何调用
    let runtime = fresh_dir("runtime-p1");
    let service = build_service(&plane_chat_only(&stub, Some(1_000)), &runtime);
    let (state, home) = boot_storage("p1").await;

    let exchange = long_exchange(6, 8_000);
    let original = exchange.clone();
    let snapshot = read_snapshot();
    // usage 880 ≥ FORCE 线 800（1000×80%）→ 触发成立。
    let usage = ModelCallUsage::reported(880, 20);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-r4");
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &auto_input(&exchange, &snapshot, Some(&usage)),
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    let CompactionOutcome::HardTruncated {
        exchange: truncated,
        plan,
        ..
    } = outcome
    else {
        panic!("回退后窗口仍不足 → HardTruncated，实际 {outcome:?}");
    };
    // 产物形状：marker 逐字 + mid_run=true + 保留区逐字；原输入不动。
    let ExchangeItem::CompactionSummary {
        summary, mid_run, ..
    } = &truncated[0]
    else {
        panic!("硬截断产物首项是摘要");
    };
    assert_eq!(summary, compaction::HARD_TRUNCATE_MARKER_TEXT, "marker 逐字");
    assert!(mid_run, "自动路径产物带 mid_run 标记");
    assert_eq!(
        &truncated[1..],
        &original[plan.cut_index..],
        "保留区逐字（硬截断不动保留区）"
    );
    assert_eq!(exchange, original, "输入交换不被原地改动");
    // 0 物理调用、0 台账：诚实降级臂在回退路由上不伪造成功、不落假账。
    assert_eq!(stub.hits(), 0, "HardTruncated 不调摘要模型");
    assert!(
        ledger_rows(&state, "auxiliary.summarize").await.is_empty(),
        "HardTruncated 不落摘要台账行"
    );
    println!("   P1 OK：回退链接手后窗口仍不足 → 诚实硬截断，0 调用 0 台账");
    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

// ─── P2：placeholder 单次恢复 × chat 回退链 ───

async fn probe_p2_placeholder_recovery_over_fallback() {
    println!("── P2. placeholder 恢复 × 回退链（两发都走 chat 路由）");
    let stub = StubServer::start(vec![
        sse_tool_calls(&[("call_r4_recover", "read", "{\"path\":\"/f/late\"}")]),
        sse_final(&golden_summary()),
    ])
    .await;
    let runtime = fresh_dir("runtime-p2");
    // 无 summarize 槽；chat 窗口 100_000（FORCE 80_000；fit 上界 85_000）。
    let service = build_service(&plane_chat_only(&stub, Some(100_000)), &runtime);
    let (state, home) = boot_storage("p2").await;

    let exchange = long_exchange(6, 8_000);
    let snapshot = read_snapshot();
    let usage = ModelCallUsage::reported(88_000, 2_000);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-r4");
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &auto_input(&exchange, &snapshot, Some(&usage)),
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    let CompactionOutcome::Compacted {
        exchange: compacted, ..
    } = outcome
    else {
        panic!("placeholder 恢复后应 Compacted，实际 {outcome:?}");
    };
    assert_eq!(stub.hits(), 2, "意图 turn + 恰好一次恢复调用");

    // 两发线体都走回退后的 chat 路由身份。
    let bodies = stub.bodies();
    for (index, body) in bodies.iter().enumerate() {
        let parsed: serde_json::Value = serde_json::from_str(body).expect("request json");
        assert_eq!(
            parsed["model"], "chat-model",
            "第 {} 发线体 model = chat 路由身份: {parsed}",
            index + 1
        );
    }
    // 恢复请求：placeholder 逐字 + 配对的 tool 消息（role:tool）。
    assert!(
        !bodies[0].contains(compaction::PLACEHOLDER_TOOL_RESULT_TEXT),
        "第一发不含 placeholder"
    );
    assert!(
        bodies[1].contains(compaction::PLACEHOLDER_TOOL_RESULT_TEXT),
        "恢复请求含 placeholder 逐字"
    );
    assert!(
        bodies[1].contains("\"role\":\"tool\""),
        "恢复请求携带配对的 tool 消息"
    );
    // 最终摘要仍经 enrichment（/f/probe 来自旧区的 read 调用）。
    let ExchangeItem::CompactionSummary { summary, .. } = &compacted[0] else {
        panic!("首项是摘要");
    };
    assert!(summary.contains("<read-files>"), "恢复后仍追加 fileOps 段");
    assert!(
        !summary.contains("/f/late"),
        "placeholder 意图路径不是真实文件操作"
    );
    // 台账：两行，purpose 恒 auxiliary.summarize（语义槽），身份 = RESOLVED
    // 的 chat 路由（model=chat-model）——回退不伪造槽身份；意图 turn 的
    // emitted_tool_calls 落行（与 chat 行同一纪律）。
    let rows = ledger_rows(&state, "auxiliary.summarize").await;
    assert_eq!(rows.len(), 2, "意图 + 恢复两行都落账");
    assert!(rows.iter().all(|row| row.outcome == CallOutcome::Succeeded));
    assert!(
        rows.iter().all(|row| row.model == "chat-model"),
        "两行身份都是 chat 路由: {rows:?}"
    );
    let intent_row = rows
        .iter()
        .find(|row| !row.emitted_tool_calls.is_empty())
        .expect("意图行携带 emitted_tool_calls");
    assert_eq!(intent_row.emitted_tool_calls.len(), 1);
    println!("   P2 OK：恢复×回退成立——两发 chat-model、placeholder 逐字、台账 2 行随行");
    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

// ─── P3：自动路径 × 双缺 → 触发门 NotTriggered（Failed 臂不可达） ───

async fn probe_p3_auto_path_both_missing_not_triggered() {
    println!("── P3. 自动路径 × chat/summarize 双缺 → NotTriggered（入口门）");
    let stub = StubServer::start(vec![]).await;
    let runtime = fresh_dir("runtime-p3");
    let service = build_service(&plane_no_models(&stub), &runtime);
    let (state, home) = boot_storage("p3").await;

    let exchange = long_exchange(6, 8_000);
    let original = exchange.clone();
    let snapshot = read_snapshot();
    // usage 给足——若入口门缺席，双缺 Failed 臂才有机会露头。
    let usage = ModelCallUsage::reported(9_000_000, 1_000);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-r4");
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &auto_input(&exchange, &snapshot, Some(&usage)),
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    assert!(
        matches!(outcome, CompactionOutcome::NotTriggered),
        "chat 不可解析 = 无窗口可言 = 触发门 NotTriggered（双缺 Failed 臂结构上不可达），实际 {outcome:?}"
    );
    assert_eq!(stub.hits(), 0, "0 物理调用");
    assert_eq!(exchange, original, "原交换不动");
    assert!(ledger_rows(&state, "auxiliary.summarize").await.is_empty());
    println!("   P3 OK：自动路径双缺在触发门即 NotTriggered——双缺 Failed 臂仅手动路径可达");
    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

// ─── P4：手动 × 回退 × chat 窗口未声明 → HardTruncated（mid_run=false） ───

async fn probe_p4_manual_fallback_undeclared_window() {
    println!("── P4. 手动 compact_now × 无槽 × chat 窗口未声明 → HardTruncated");
    let stub = StubServer::start(vec![]).await;
    let runtime = fresh_dir("runtime-p4");
    let service = build_service(&plane_chat_only(&stub, None), &runtime);
    let (state, home) = boot_storage("p4").await;

    let exchange = long_exchange(6, 8_000);
    let original = exchange.clone();
    let snapshot = read_snapshot();
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-r4");
    let outcome = service
        .compact_now(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &manual_input(&exchange, &snapshot),
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    let CompactionOutcome::HardTruncated {
        exchange: truncated,
        plan,
        ..
    } = outcome
    else {
        panic!("生效路由窗口未声明（回退后仍 0）→ HardTruncated，实际 {outcome:?}");
    };
    let ExchangeItem::CompactionSummary {
        summary, mid_run, ..
    } = &truncated[0]
    else {
        panic!("首项是摘要");
    };
    assert_eq!(summary, compaction::HARD_TRUNCATE_MARKER_TEXT);
    assert!(!mid_run, "手动路径产物不带 mid-run 标记");
    assert_eq!(&truncated[1..], &original[plan.cut_index..], "保留区逐字");
    assert_eq!(stub.hits(), 0, "窗口未声明硬截断不调模型");
    assert!(ledger_rows(&state, "auxiliary.summarize").await.is_empty());
    println!("   P4 OK：手动 × 回退 × 窗口未声明 → HardTruncated、mid_run=false、0 调用");
    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

// ─── P5：手动两轮 × 回退路由 → fileOps 跨纪元续传 ───

async fn probe_p5_manual_fallback_file_ops_continuation() {
    println!("── P5. 手动两轮 × 回退路由：fileOps 续传（播种 ∪ 新增）");
    let stub = StubServer::start(vec![
        sse_final(&golden_summary()),
        sse_final(&golden_summary()),
    ])
    .await;
    let runtime = fresh_dir("runtime-p5");
    let service = build_service(&plane_chat_only(&stub, Some(1_000_000)), &runtime);
    let (state, home) = boot_storage("p5").await;
    let snapshot = read_snapshot();
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-r4");

    // 第一轮：read /r4/one + write /r4/two + 配对组。
    let mut round1 = Vec::new();
    round1.push(assistant_turn(
        1,
        &"p".repeat(8_000),
        vec![
            requested_call(1, "tool:first-party:read", "/r4/one"),
            requested_call(2, "tool:first-party:write", "/r4/two"),
        ],
    ));
    round1.push(tool_result(1, &"r".repeat(8_000)));
    round1.push(tool_result(2, &"r".repeat(8_000)));
    round1.extend(long_exchange(5, 8_000));
    let outcome = service
        .compact_now(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &manual_input(&round1, &snapshot),
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    let CompactionOutcome::Compacted {
        exchange: compacted1, ..
    } = outcome
    else {
        panic!("第一轮手动压缩: {outcome:?}");
    };
    let ExchangeItem::CompactionSummary {
        summary: summary1,
        mid_run: mid1,
        ..
    } = &compacted1[0]
    else {
        panic!("首项是摘要");
    };
    assert!(!mid1, "手动压缩无 mid-run notice");
    assert!(summary1.contains("/r4/one") && summary1.contains("/r4/two"));

    // 第二轮：第一轮产物 + 新 read /r4/three（配对组撑长度）。
    let mut round2 = compacted1.clone();
    let base = 100_u32;
    round2.push(assistant_turn(
        base + 1,
        &"q".repeat(8_000),
        vec![requested_call(base + 1, "tool:first-party:read", "/r4/three")],
    ));
    round2.push(tool_result(base + 1, &"r".repeat(8_000)));
    for g in 0..5u32 {
        let seq = base + 10 + g * 2;
        round2.push(assistant_turn(
            seq,
            &"z".repeat(8_000),
            vec![requested_call(seq, "tool:first-party:read", "/f/probe")],
        ));
        round2.push(tool_result(seq, &"r".repeat(8_000)));
    }
    let outcome = service
        .compact_now(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &manual_input(&round2, &snapshot),
            &cancel,
            2,
        )
        .await
        .expect("no storage failure");
    let CompactionOutcome::Compacted {
        exchange: compacted2, ..
    } = outcome
    else {
        panic!("第二轮手动压缩: {outcome:?}");
    };
    let ExchangeItem::CompactionSummary {
        summary: summary2, ..
    } = &compacted2[0]
    else {
        panic!("首项是摘要");
    };
    // 续传：第一轮播种的 /r4/one、/r4/two 必须在第二轮段内；新操作
    // /r4/three 也在。
    assert!(
        summary2.contains("/r4/one") && summary2.contains("/r4/two"),
        "第一轮清单播种续传（回退路由）: {summary2}"
    );
    assert!(summary2.contains("/r4/three"), "本轮新操作并入: {summary2}");
    let modified_pos = summary2.find("<modified-files>").expect("modified section");
    assert!(
        summary2[modified_pos..].contains("/r4/two"),
        "write 过的路径留在 modified: {summary2}"
    );
    // 两发线体都走 chat 路由。
    let bodies = stub.bodies();
    assert_eq!(bodies.len(), 2);
    for (index, body) in bodies.iter().enumerate() {
        let parsed: serde_json::Value = serde_json::from_str(body).expect("request json");
        assert_eq!(
            parsed["model"], "chat-model",
            "第 {} 发线体 model = chat 路由: {parsed}",
            index + 1
        );
    }
    let rows = ledger_rows(&state, "auxiliary.summarize").await;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.model == "chat-model"));
    println!("   P5 OK：回退路由上 fileOps 跨纪元续传成立（播种 ∪ 新增），两发 chat-model");
    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

// ─── P6：修复臂（settle 第三调用点）× 回退路由 ───

async fn probe_p6_repair_over_fallback() {
    println!("── P6. 修复臂 × 回退路由（repair 调用点也透传生效路由）");
    let bad = "<mood>busy</mood>\n## Goal\n做完\n".to_string();
    let stub = StubServer::start(vec![sse_final(&bad), sse_final(&golden_summary())]).await;
    let runtime = fresh_dir("runtime-p6");
    let service = build_service(&plane_chat_only(&stub, Some(100_000)), &runtime);
    let (state, home) = boot_storage("p6").await;

    let exchange = long_exchange(6, 8_000);
    let snapshot = read_snapshot();
    let usage = ModelCallUsage::reported(88_000, 2_000);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-r4");
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &auto_input(&exchange, &snapshot, Some(&usage)),
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    let CompactionOutcome::Compacted {
        exchange: compacted, ..
    } = outcome
    else {
        panic!("修复后的摘要应落地: {outcome:?}");
    };
    let ExchangeItem::CompactionSummary { summary, .. } = &compacted[0] else {
        panic!("首项是摘要");
    };
    assert!(summary.contains("## Goal"));
    assert!(!summary.contains("<mood>"), "旁白被剥除");

    let bodies = stub.bodies();
    assert_eq!(bodies.len(), 2, "首发 + 恰好一次修复调用");
    for (index, body) in bodies.iter().enumerate() {
        let parsed: serde_json::Value = serde_json::from_str(body).expect("request json");
        assert_eq!(
            parsed["model"], "chat-model",
            "第 {} 发（含修复）线体 model = chat 路由: {parsed}",
            index + 1
        );
    }
    // 修复请求：修复指令含 <draft-summary> 且内嵌当轮原始文本（mood 仍在）。
    let repair_body: serde_json::Value =
        serde_json::from_str(&bodies[1]).expect("repair json");
    let messages = repair_body["messages"].as_array().expect("messages");
    let repair_text = messages
        .last()
        .expect("repair instruction")
        .pointer("/content")
        .and_then(|c| c.as_str())
        .expect("repair text");
    assert!(repair_text.contains("Internal compaction summary repair."));
    assert!(repair_text.contains("<draft-summary>"));
    assert!(
        repair_text.contains("<mood>busy</mood>"),
        "repair 载荷内嵌当轮原始文本（可剥除内容仍在）: {repair_text}"
    );
    let rows = ledger_rows(&state, "auxiliary.summarize").await;
    assert_eq!(rows.len(), 2, "首发与修复都落账");
    assert!(rows.iter().all(|row| row.model == "chat-model"));
    println!("   P6 OK：修复臂 × 回退链——两发 chat-model、repair 载荷 = 当轮原始文本");
    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

// ─── P7：交叉锁「正文稀释」演示（纯逻辑，锁的契约边界） ───

fn probe_p7_cross_lock_content_dilution() {
    println!("── P7. 交叉锁鉴别力边界：正文稀释不在锁的契约内");
    // 重实现修复后的锁逻辑（与 service 测试 parse_register_entries /
    // check_register_lock 同构，R3-F-03 后 = 双向精确集合匹配）。
    let parse = |text: &str| -> std::collections::BTreeSet<u32> {
        let section = text
            .split("## 十、记录在案差异")
            .nth(1)
            .and_then(|rest| rest.split("\n## ").next())
            .expect("§十 section exists");
        let mut entries = std::collections::BTreeSet::new();
        for line in section.lines() {
            let t = line.trim_start();
            let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() && t[digits.len()..].starts_with(". **") {
                entries.insert(digits.parse().expect("entry number"));
            }
        }
        entries
    };
    let lock = |entries: &std::collections::BTreeSet<u32>,
                referenced: &std::collections::BTreeSet<u32>|
     -> Vec<String> {
        let mut problems = Vec::new();
        let missing: Vec<u32> = entries
            .iter()
            .copied()
            .filter(|n| !referenced.contains(n))
            .collect();
        if !missing.is_empty() {
            problems.push(format!("entries without reference: {missing:?}"));
        }
        let stray: Vec<u32> = referenced
            .iter()
            .copied()
            .filter(|n| !entries.contains(n))
            .collect();
        if !stray.is_empty() {
            problems.push(format!("references unknown entries: {stray:?}"));
        }
        problems
    };
    // 现状：17 条全引用 → 绿。
    let full: std::collections::BTreeSet<u32> = (1..=17).collect();
    assert!(lock(&full, &full).is_empty(), "现状绿");
    // 正文稀释：条目 1..=17 编号全部保留、正文掏空为占位符——解析出的
    // 条目集合不变 → 锁仍绿。
    let mut diluted = String::from("## 十、记录在案差异\n\n");
    for n in 1..=17u32 {
        diluted.push_str(&format!("{n}. **（正文略）**\n"));
    }
    diluted.push_str("\n## 十一、次节\n");
    let diluted_entries = parse(&diluted);
    assert_eq!(diluted_entries, full, "编号集合不受正文稀释影响");
    assert!(
        lock(&diluted_entries, &full).is_empty(),
        "CONFIRMED：正文稀释后交叉锁仍绿——锁只锚定编号存在性，不保正文保真"
    );
    println!(
        "   P7 CONFIRMED（informational）：交叉锁契约 = 锚定存在性；正文保真依赖 \
         抽查与本类人工审查，非机器门禁缺口"
    );
}

#[tokio::main]
async fn main() {
    println!("REVIEWER-R06-T02-R4 对抗探针");
    probe_p1_fallback_window_still_too_small().await;
    probe_p2_placeholder_recovery_over_fallback().await;
    probe_p3_auto_path_both_missing_not_triggered().await;
    probe_p4_manual_fallback_undeclared_window().await;
    probe_p5_manual_fallback_file_ops_continuation().await;
    probe_p6_repair_over_fallback().await;
    probe_p7_cross_lock_content_dilution();
    println!("ALL PROBES DONE");
}
