//! R06-T02 服务侧测试：CompactionService 经真 ModelGateway/CredentialService/
//! AuxiliaryExecutor 对 loopback stub 的摘要调用链（触发、缓存保留形状、
//! 修复、能力门、台账），以及 drive_run 生产链上的 mid-run 压缩接线
//! （R06-A03/A04 的 service 腿）。
//!
//! 夹具纪律：所有材料落独立临时目录（绝不读写真实用户目录）；外部协议
//! 用确定性 loopback 替身（StubServer，脚本化 SSE 应答）；不使用真实付费
//! 凭证（stub 的 apiKey 是字面量占位）。
//!
//! 测试替身边界：ScriptedUsageProvider 只扮演「外部模型的应答」，逐字记录
//! 每次调用的 prior 形状；它从不决定权限、不写运行状态。StubServer 只
//! 回放脚本化应答。

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use lingxi_kernel::model_exchange::{
    ExchangeItem, ModelTurnInput, RequestedToolCall, ToolDeclarationSnapshot,
};
use lingxi_kernel::ports::{
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, StoragePort, ToolOutcome, ToolRequest,
    TurnDeltaSink, TurnProviderPort,
};
use lingxi_kernel::toolcatalog::SchemaBudget;
use lingxi_kernel::usage::{CallOutcome, ModelCallUsage, ModelUsageQuery};
use lingxi_kernel::RunContext;
use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage, ToolCallId, UsageRecord};
use lingxi_service::compaction::{CompactionOutcome, CompactionService, MidRunCompactionInput};
use lingxi_service::quotas::{QuotaLimits, QuotaManager};
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use serde_json::json;

const NOW_MS: u64 = 1_790_409_600_000;
const STUB_API_KEY: &str = "stub-key-not-a-secret";

// ─── 临时目录 ───

static DIR_SEQ: AtomicUsize = AtomicUsize::new(0);

fn fresh_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r06t02-{tag}-{}-{}",
        std::process::id(),
        DIR_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create fixture dir");
    dir
}

// ─── loopback stub（r05_t06 形状：一连接一请求，脚本化 SSE 应答） ───

struct StubResponse {
    body: String,
}

struct RecordedRequest {
    body: String,
}

struct StubServer {
    addr: SocketAddr,
    hits: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
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
        let requests: Arc<Mutex<Vec<RecordedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let responses = Arc::new(Mutex::new(VecDeque::from(responses)));
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
                let Some(header_end) = header_end else {
                    continue;
                };
                let text = String::from_utf8_lossy(&raw[..header_end]).to_string();
                let (_head, body) = text
                    .split_once("\r\n\r\n")
                    .map(|(h, b)| (h.to_string(), b.to_string()))
                    .unwrap_or((text, String::new()));
                task_requests
                    .lock()
                    .expect("requests")
                    .push(RecordedRequest { body });
                let next = task_responses.lock().expect("responses").pop_front();
                let payload = next.map(|response| response.body).unwrap_or_else(|| {
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
        self.requests
            .lock()
            .expect("requests")
            .iter()
            .map(|request| request.body.clone())
            .collect()
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
        json!({"id":"chatcmpl-t02","model":"stub-model","choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]}),
        json!({"id":"chatcmpl-t02","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
        json!({"id":"chatcmpl-t02","choices":[],"usage":{"prompt_tokens":17,"completion_tokens":5}}),
    ] {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    StubResponse { body }
}

/// openai-completions SSE：工具意图 turn（delta.tool_calls + finish_reason
/// "tool_calls" + usage）——F-03 现役 placeholder 恢复语义的夹具。
/// `calls` 每项 = (provider_call_id, wire_name, arguments_json)。
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
        json!({"id":"chatcmpl-t02","model":"stub-model","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":tool_calls},"finish_reason":null}]}),
        json!({"id":"chatcmpl-t02","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
        json!({"id":"chatcmpl-t02","choices":[],"usage":{"prompt_tokens":21,"completion_tokens":7}}),
    ] {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    StubResponse { body }
}

/// openai-completions SSE：provider 侧的确定性失败（content_filter 是非
/// 重试的协议级拒绝——A04 的"摘要 provider 返回错误"替身）。
fn sse_content_filter_refusal() -> StubResponse {
    let mut body = String::new();
    for frame in [
        json!({"id":"chatcmpl-t02","model":"stub-model","choices":[{"index":0,"delta":{"role":"assistant","content":"..."},"finish_reason":null}]}),
        json!({"id":"chatcmpl-t02","choices":[{"index":0,"delta":{},"finish_reason":"content_filter"}]}),
    ] {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    StubResponse { body }
}

// ─── 合法摘要（kernel 测试的 golden 形态） ───

fn golden_summary() -> String {
    "## Goal\n完成用户请求\n\n## Constraints & Preferences\n- (none)\n\n\
     ## Progress\n### Done\n- [x] 读取文件\n\n### In Progress\n- [ ] 汇总\n\n\
     ### Blocked\n- 无\n\n## Key Decisions\n- **用真实工具**: 避免猜测\n\n\
     ## Next Steps\n1. 写出结论\n\n## Critical Context\n- (none)\n"
        .to_string()
}

// ─── plane / service 构造 ───

fn plane_json(
    stub: &StubServer,
    chat_window: Option<u64>,
    summarize_declares_tools: bool,
) -> String {
    // FIX-07（N-1）：现役 fit 检查以**摘要模型**的 contextWindow 为准，
    // 窗口未声明（≤0）即硬截断、不调模型。本夹具的用例全部走摘要模型
    // 路径，故 summarize 路由声明一个足够大的窗口让 fit 检查通过；
    // 硬截断臂由 plane_json_with_summarize_window 的小窗口夹具覆盖。
    plane_json_with_summarize_window(stub, chat_window, summarize_declares_tools, 1_000_000)
}

fn plane_json_with_summarize_window(
    stub: &StubServer,
    chat_window: Option<u64>,
    summarize_declares_tools: bool,
    summarize_window: u64,
) -> String {
    let chat_compat = match chat_window {
        Some(window) => format!(r#", "compat": {{"contextWindow": {window}}}"#),
        None => String::new(),
    };
    let summarize_tools = if summarize_declares_tools {
        r#", "capabilities": {"tools": true}"#
    } else {
        ""
    };
    let summarize_compat = format!(r#", "compat": {{"contextWindow": {summarize_window}}}"#);
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
                "chat": {{"provider": "stub_svc", "model": "chat-model", "capabilities": {{"tools": true}}{chat_compat}}},
                "summarize": {{"provider": "stub_svc", "model": "summarize-model"{summarize_tools}{summarize_compat}}}
            }}
        }}"#,
        stub.v1()
    )
}

fn build_compaction_service(
    plane_json: &str,
    runtime_dir: &std::path::Path,
) -> Arc<CompactionService> {
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
    Arc::new(CompactionService::new(
        Arc::new(executor),
        gateway,
        Arc::new(QuotaManager::new(QuotaLimits::default())),
        Arc::new(lingxi_service::inject::SystemClock),
    ))
}

// ─── 直测夹具（交换 / 快照 / 上下文） ───

fn read_snapshot() -> ToolDeclarationSnapshot {
    ToolDeclarationSnapshot {
        catalog_generation: 1,
        declarations: vec![lingxi_kernel::model_exchange::ToolDeclaration {
            target: "tool:first-party:read".to_string(),
            wire_name: "read".to_string(),
            description: "Read a file".to_string(),
            input_schema: lingxi_protocol::ToolSchemaDocument {
                dialect: "json-schema/2020-12".to_string(),
                schema: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            },
        }],
    }
}

fn assistant_turn(seq: u32, text: &str, with_call: bool) -> ExchangeItem {
    ExchangeItem::AssistantTurn {
        call: ModelCallId::new(format!("mc{seq:04}")),
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        tool_calls: if with_call {
            let request = ToolRequest::from_effective_arguments(
                "tool:first-party:read",
                json!({"path": format!("/f/{seq}")}),
                &SchemaBudget::default(),
            )
            .expect("arguments")
            .with_provider_call_id(format!("pc{seq:04}"));
            vec![RequestedToolCall {
                tool_call_id: ToolCallId::new(format!("tc{seq:04}")),
                provider_call_id: request.provider_call_id,
                target: request.target,
                arguments: request.arguments,
                args_digest: request.args_digest,
                args_summary: None,
            }]
        } else {
            Vec::new()
        },
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

/// 长会话夹具（service 侧）：`groups` 组 [assistant(带一个工具调用), 结果]，
/// 每组正文约 `chunk` 字符。
fn long_exchange(groups: u32, chunk: usize) -> Vec<ExchangeItem> {
    let mut exchange = Vec::new();
    for g in 0..groups {
        let seq = g * 2 + 1;
        exchange.push(assistant_turn(
            seq,
            &format!("assistant chunk {g} {}", "a".repeat(chunk)),
            true,
        ));
        exchange.push(tool_result(
            seq,
            &format!("result {g} {}", "r".repeat(chunk)),
        ));
    }
    exchange
}

fn test_ctx() -> RunContext {
    RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("sess-t02"),
        run_id: lingxi_protocol::RunId::new("run-t02"),
        attempt: lingxi_protocol::AttemptId::new("attempt-t02"),
        generation: 1,
    }
}

/// 直测的 boot：只要真 storage（台账）——turn_provider 为 None。
async fn boot_storage_only(tag: &str) -> (ServiceState, PathBuf) {
    let home = fresh_dir(tag);
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static addr"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let layout = prepare_layout(&home).expect("layout");
    let state = ServiceState::bootstrap_with_deps(config, &layout, ServiceDeps::default())
        .await
        .expect("bootstrap");
    (state, home)
}

async fn teardown(state: &ServiceState, home: &PathBuf) {
    state.storage().close().await.expect("close storage");
    let _ = std::fs::remove_dir_all(home);
}

async fn ledger_rows(
    state: &ServiceState,
    purpose: &str,
) -> Vec<lingxi_kernel::usage::ModelCallUsageRecord> {
    state
        .storage()
        .query_model_call_usage(ModelUsageQuery {
            purpose: Some(purpose.to_string()),
            ..ModelUsageQuery::default()
        })
        .await
        .expect("usage query")
}

// ─── 直测：触发 + 缓存保留形状 + 台账 ───

#[tokio::test]
async fn trigger_fires_and_compacts_through_the_real_chain() {
    // ASK 事件线（0.5 阈值 + 重问增量/重置降幅）未迁移（无交互面）——§十#8。
    let stub = StubServer::start(vec![sse_final(&golden_summary())]).await;
    let runtime = fresh_dir("runtime-fire");
    let service = build_compaction_service(&plane_json(&stub, Some(1_000), true), &runtime);
    let (state, home) = boot_storage_only("fire").await;

    // window=1000：reserve = max(16384, 200) = 16384 > window → reserve 线
    // 恒过；usage=900 同时过 FORCE 线（90% ≥ 80%）。exchange 超 keep_recent
    // 才可切：6 组 × 8000 字符 ≈ 6×4000 tokens > 20000。
    let exchange = long_exchange(6, 8_000);
    let snapshot = read_snapshot();
    let usage = ModelCallUsage::reported(880, 20);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-t02");
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &MidRunCompactionInput {
                exchange: &exchange,
                system_prompt: Some("SYS-PROMPT-ANCHOR"),
                submission: "把读到的文件汇总",
                tools: &snapshot,
                last_usage: Some(&usage),
                tail_from: exchange.len(),
            },
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    let CompactionOutcome::Compacted {
        exchange: compacted,
        plan,
        report,
    } = outcome
    else {
        panic!("compaction must fire: {outcome:?}");
    };
    // 新交换 = [摘要项] + 保留区；保留区逐字等于原交换的尾部切片。
    assert_eq!(compacted.len(), exchange.len() - plan.cut_index + 1);
    let ExchangeItem::CompactionSummary {
        summary,
        covered_items,
        mid_run,
    } = &compacted[0]
    else {
        panic!("the first item is the compaction summary");
    };
    assert!(summary.contains("## Goal"), "sanitized structured summary");
    assert_eq!(*covered_items as usize, plan.cut_index);
    assert!(*mid_run, "mid-run compaction marks the notice");
    assert_eq!(
        &compacted[1..],
        &exchange[plan.cut_index..],
        "the retained suffix is verbatim"
    );
    assert_eq!(report.context_window, 1_000);
    assert!(report.usage_context_tokens.is_some());

    // 缓存保留形状（线上请求逐字节核对）：[system, submission, ...history,
    // instruction]，工具声明随请求携带（历史渲染的 wire 名解析必需）。
    let bodies = stub.bodies();
    assert_eq!(bodies.len(), 1, "exactly one summary call");
    let body: serde_json::Value = serde_json::from_str(&bodies[0]).expect("request json");
    let messages = body["messages"].as_array().expect("messages");
    assert_eq!(messages[0]["role"], "system");
    assert_eq!(messages[0]["content"], "SYS-PROMPT-ANCHOR");
    assert_eq!(messages[1]["role"], "user");
    assert_eq!(messages[1]["content"], "把读到的文件汇总");
    // 历史渲染：assistant(tool_calls) + role=tool 配对完整（A03 线体腿）。
    let mut known_call_ids: Vec<String> = Vec::new();
    for message in &messages[2..] {
        match message["role"].as_str() {
            Some("assistant") => {
                for call in message["tool_calls"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
                {
                    known_call_ids.push(call["id"].as_str().expect("id").to_string());
                }
            }
            Some("tool") => {
                let id = message["tool_call_id"].as_str().expect("pairing id");
                assert!(
                    known_call_ids.iter().any(|known| known == id),
                    "tool message {id} pairs with an earlier assistant call"
                );
            }
            _ => {}
        }
    }
    assert_eq!(
        known_call_ids.len(),
        6,
        "every group's call rode the wire: {known_call_ids:?}"
    );
    // 末尾 = 指令 user 消息（现役模板标记逐字）。
    let last = messages.last().expect("instruction");
    assert_eq!(last["role"], "user");
    let instruction = last["content"].as_str().expect("instruction text");
    assert!(instruction.contains("Internal compaction-only run."));
    assert!(instruction.contains("Do not call tools."));
    assert!(instruction.contains("## Critical Context"));
    // R2-F-01 修复后的现役线体语义（§十#5）：openai-completions 属
    // optional-cap 族——摘要请求线上**不发**任何输出上限字段（现役
    // `normalizeCompactionProviderPayload` 对 optional 族删除全部
    // OUTPUT_CAP_FIELDS）。0.8×reserve 公式只服务预算估算与 required
    // 族兜底，不是线体默认。
    assert!(
        body.get("max_tokens").is_none()
            && body.get("max_output_tokens").is_none()
            && body.get("max_completion_tokens").is_none(),
        "the optional-cap family sends NO output cap field on the wire: {body}"
    );
    let tools = body["tools"].as_array().expect("tools ride the request");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["function"]["name"], "read");

    // 台账：一行 auxiliary.summarize，Succeeded，usage 来自 stub 的应答
    // （17+5），身份是 RESOLVED 路由。
    let rows = ledger_rows(&state, "auxiliary.summarize").await;
    assert_eq!(rows.len(), 1, "one ledger row");
    let row = &rows[0];
    assert_eq!(row.outcome, CallOutcome::Succeeded);
    assert_eq!(row.model, "summarize-model");
    assert_eq!(row.provider, "stub_svc");
    assert_eq!(
        row.usage.as_ref().and_then(|usage| usage.input_tokens),
        Some(17)
    );
    assert_eq!(row.transport_attempts, Some(1));

    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

// ─── 直测：A04 —— 摘要 provider 返回错误 ───

#[tokio::test]
async fn provider_failure_keeps_the_original_exchange_and_accounts_it() {
    let stub = StubServer::start(vec![sse_content_filter_refusal()]).await;
    let runtime = fresh_dir("runtime-a04");
    let service = build_compaction_service(&plane_json(&stub, Some(1_000), true), &runtime);
    let (state, home) = boot_storage_only("a04").await;

    let exchange = long_exchange(6, 8_000);
    let snapshot = read_snapshot();
    let usage = ModelCallUsage::reported(880, 20);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-t02");
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &MidRunCompactionInput {
                exchange: &exchange,
                system_prompt: Some("SYS"),
                submission: "汇总",
                tools: &snapshot,
                last_usage: Some(&usage),
                tail_from: exchange.len(),
            },
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    // A04：错误明确（detail 非空、可诊断），原上下文保留（无新交换）。
    let CompactionOutcome::Failed { detail } = outcome else {
        panic!("provider failure must surface as Failed: {outcome:?}");
    };
    assert!(
        detail.contains("content filter") || detail.contains("refused"),
        "diagnosable detail: {detail}"
    );
    // 原始交换不受影响（调用方持有的切片从未被 service 修改）。
    assert_eq!(exchange.len(), 12);
    assert!(
        !exchange
            .iter()
            .any(|item| matches!(item, ExchangeItem::CompactionSummary { .. })),
        "no summary ever lands in the original exchange"
    );
    // 台账对照：Failed 行落账（失败账不丢），run 的 chat 行不受污染
    // （本测试未发 chat 调用 → chat 行数为 0）。
    let rows = ledger_rows(&state, "auxiliary.summarize").await;
    assert_eq!(rows.len(), 1, "the failed call accounts too");
    assert_eq!(rows[0].outcome, CallOutcome::Failed);
    assert_eq!(rows[0].transport_attempts, Some(1));
    let chat_rows = ledger_rows(&state, "chat").await;
    assert_eq!(chat_rows.len(), 0);

    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

// ─── 直测：清洗 + 一次修复 ───

#[tokio::test]
async fn repair_flow_accepts_a_fixed_summary() {
    // customInstructions 注入面未接（service 恒 None，归配置面任务）——§十#13。
    // 第一次应答：缺标题 + 带 mood 旁白（sanitize 剥闭合块、validate 判
    // 标题）；第二次：合法摘要。
    let bad = "<mood>busy</mood>\n## Goal\n做完\n".to_string();
    let stub = StubServer::start(vec![sse_final(&bad), sse_final(&golden_summary())]).await;
    let runtime = fresh_dir("runtime-repair");
    let service = build_compaction_service(&plane_json(&stub, Some(1_000), true), &runtime);
    let (state, home) = boot_storage_only("repair").await;

    let exchange = long_exchange(6, 8_000);
    let snapshot = read_snapshot();
    let usage = ModelCallUsage::reported(880, 20);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-t02");
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &MidRunCompactionInput {
                exchange: &exchange,
                system_prompt: None,
                submission: "汇总",
                tools: &snapshot,
                last_usage: Some(&usage),
                tail_from: exchange.len(),
            },
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    let CompactionOutcome::Compacted {
        exchange: compacted,
        ..
    } = outcome
    else {
        panic!("the repaired summary must land: {outcome:?}");
    };
    let ExchangeItem::CompactionSummary { summary, .. } = &compacted[0] else {
        panic!("summary item");
    };
    assert!(summary.contains("## Goal"));
    assert!(!summary.contains("<mood>"), "narration stripped");

    // 修复请求 = 同一活前缀 + 原指令 + 草稿(assistant) + 修复指令（含
    // 问题清单与 <draft-summary>）。
    let bodies = stub.bodies();
    assert_eq!(bodies.len(), 2, "first + one repair call");
    let repair_body: serde_json::Value = serde_json::from_str(&bodies[1]).expect("repair json");
    let messages = repair_body["messages"].as_array().expect("messages");
    let repair_text = messages
        .last()
        .expect("repair instruction")
        .pointer("/content")
        .and_then(|c| c.as_str())
        .expect("repair text");
    assert!(repair_text.contains("Internal compaction summary repair."));
    assert!(repair_text.contains("Validation failures:"));
    assert!(repair_text.contains("<draft-summary>"));
    // R2-F-04：`<draft-summary>` 内嵌**当轮原始文本**（现役 agent-run.ts
    // :601 rawText）——含已被 sanitize 剥离的 `<mood>busy</mood>`。
    let draft_pos = repair_text.find("<draft-summary>\n").expect("draft block");
    let embedded = &repair_text[draft_pos..];
    assert!(
        embedded.contains("<mood>busy</mood>"),
        "the repair payload embeds the RAW draft (strippable content intact): {embedded}"
    );
    // 草稿以 assistant 身份在修复指令之前（模型看到自己的产出）——同为
    // 原始文本（现役 repair 会话中的草稿消息是原始 assistant message；
    // 渲染器对消息正文的尾部空白修整是渲染层行为，与载荷语义无关，
    // 承载事实 = 可剥除的 `<mood>busy</mood>` 仍在）。
    let repair_pos = messages.len() - 1;
    assert_eq!(messages[repair_pos - 1]["role"], "assistant");
    assert!(
        messages[repair_pos - 1]["content"]
            .as_str()
            .is_some_and(|text| text.contains("<mood>busy</mood>")),
        "the draft assistant message is the RAW text (strippable content intact)"
    );

    let rows = ledger_rows(&state, "auxiliary.summarize").await;
    assert_eq!(rows.len(), 2, "both calls account");

    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

#[tokio::test]
async fn repair_exhaustion_is_a_loud_failure() {
    let bad = "no headings at all".to_string();
    let stub = StubServer::start(vec![sse_final(&bad), sse_final(&bad)]).await;
    let runtime = fresh_dir("runtime-repair2");
    let service = build_compaction_service(&plane_json(&stub, Some(1_000), true), &runtime);
    let (state, home) = boot_storage_only("repair2").await;

    let exchange = long_exchange(6, 8_000);
    let snapshot = read_snapshot();
    let usage = ModelCallUsage::reported(880, 20);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-t02");
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &MidRunCompactionInput {
                exchange: &exchange,
                system_prompt: None,
                submission: "汇总",
                tools: &snapshot,
                last_usage: Some(&usage),
                tail_from: exchange.len(),
            },
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    let CompactionOutcome::Failed { detail } = outcome else {
        panic!("repair exhaustion must fail loudly: {outcome:?}");
    };
    assert!(
        detail.contains("still fails validation"),
        "the failure names the repair exhaustion: {detail}"
    );
    assert_eq!(stub.bodies().len(), 2, "exactly one repair attempt");
    // 两次模型调用本身都成功应答（不合规是 service 层判定）——台账忠实
    // 记 Succeeded；压缩失败只体现在 outcome 与 detail。
    let rows = ledger_rows(&state, "auxiliary.summarize").await;
    assert_eq!(rows.len(), 2);

    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

// ─── 直测：placeholder 工具单次恢复（现役 tool_recovery 语义，F-03） ───

/// 首次应答的单个工具意图 → placeholder 结果应答 → 恰好一次恢复调用 →
/// 摘要成功。工具从不真正执行；恢复请求在同一会话里延伸（现役
/// context.messages 累积语义）。
#[tokio::test]
async fn placeholder_recovery_answers_a_single_tool_intent_and_compacts() {
    let stub = StubServer::start(vec![
        sse_tool_calls(&[("call_recover_1", "read", "{\"path\":\"/f/9\"}")]),
        sse_final(&golden_summary()),
    ])
    .await;
    let runtime = fresh_dir("runtime-toolrec");
    let service = build_compaction_service(&plane_json(&stub, Some(1_000), true), &runtime);
    let (state, home) = boot_storage_only("toolrec").await;

    let exchange = long_exchange(6, 8_000);
    let snapshot = read_snapshot();
    let usage = ModelCallUsage::reported(880, 20);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-t02");
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &MidRunCompactionInput {
                exchange: &exchange,
                system_prompt: None,
                submission: "汇总",
                tools: &snapshot,
                last_usage: Some(&usage),
                tail_from: exchange.len(),
            },
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    let CompactionOutcome::Compacted { .. } = outcome else {
        panic!("a single first-turn tool intent must RECOVER, not fail: {outcome:?}");
    };
    assert_eq!(stub.hits(), 2, "intent turn + exactly one recovery call");

    // 恢复请求（第二发）的线体：同一会话延伸——意图 turn 渲染为带
    // tool_calls 的 assistant 消息，紧随其后的 role:"tool" 消息携带宿主
    // 铸造的 placeholder 文本（现役逐字应答），配对 id 一致。
    let bodies = stub.bodies();
    let recovery: serde_json::Value = serde_json::from_str(&bodies[1]).expect("request json");
    let messages = recovery["messages"].as_array().expect("messages");
    let intent_pos = messages
        .iter()
        .position(|message| {
            message["role"] == "assistant"
                && message["tool_calls"]
                    .as_array()
                    .is_some_and(|calls| calls.iter().any(|call| call["id"] == "call_recover_1"))
        })
        .expect("the intent turn rides the recovery request");
    let tool_message = &messages[intent_pos + 1];
    assert_eq!(tool_message["role"], "tool");
    assert_eq!(tool_message["tool_call_id"], "call_recover_1");
    assert_eq!(
        tool_message["content"],
        lingxi_kernel::compaction::PLACEHOLDER_TOOL_RESULT_TEXT,
        "the placeholder answer is the incumbent verbatim text"
    );
    // 第一发请求（意图 turn 的那次）不含 placeholder：配对段是恢复时才
    // 追加的。
    let first: serde_json::Value = serde_json::from_str(&bodies[0]).expect("request json");
    assert!(
        !bodies[0].contains(lingxi_kernel::compaction::PLACEHOLDER_TOOL_RESULT_TEXT),
        "the first request predates the placeholder: {}",
        first["messages"]
    );

    // 台账：两次物理调用各一行、都 Succeeded（意图 turn 是已结算应答，
    // F21）；意图行的 emitted_tool_calls 带宿主铸造的占位调用身份。
    let rows = ledger_rows(&state, "auxiliary.summarize").await;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.outcome == CallOutcome::Succeeded));
    let intent_rows: Vec<_> = rows
        .iter()
        .filter(|row| !row.emitted_tool_calls.is_empty())
        .collect();
    assert_eq!(intent_rows.len(), 1, "exactly one tool-intent row");
    assert_eq!(intent_rows[0].emitted_tool_calls.len(), 1);
    assert!(
        intent_rows[0].emitted_tool_calls[0].ends_with("-tc0001"),
        "host-minted placeholder call identity: {:?}",
        intent_rows[0].emitted_tool_calls
    );

    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

/// 恢复调用上再出现工具意图 = 现役 toolViolation「Tool intent appeared
/// after the first placeholder recovery turn」→ 响亮失败，原文续跑。
#[tokio::test]
async fn second_tool_intent_after_recovery_fails_loudly() {
    let stub = StubServer::start(vec![
        sse_tool_calls(&[("call_recover_1", "read", "{\"path\":\"/f/9\"}")]),
        sse_tool_calls(&[("call_recover_2", "read", "{\"path\":\"/f/10\"}")]),
    ])
    .await;
    let runtime = fresh_dir("runtime-toolrec2");
    let service = build_compaction_service(&plane_json(&stub, Some(1_000), true), &runtime);
    let (state, home) = boot_storage_only("toolrec2").await;

    let exchange = long_exchange(6, 8_000);
    let snapshot = read_snapshot();
    let usage = ModelCallUsage::reported(880, 20);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-t02");
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &MidRunCompactionInput {
                exchange: &exchange,
                system_prompt: None,
                submission: "汇总",
                tools: &snapshot,
                last_usage: Some(&usage),
                tail_from: exchange.len(),
            },
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    let CompactionOutcome::Failed { detail } = outcome else {
        panic!("a repeated tool intent must fail loudly: {outcome:?}");
    };
    assert!(
        detail.contains("after the first placeholder recovery turn"),
        "the incumbent toolViolation wording: {detail}"
    );
    assert_eq!(stub.hits(), 2, "intent + recovery both left the process");
    // 两次应答都已结算：台账忠实记 Succeeded；压缩失败是 service 层判定。
    let rows = ledger_rows(&state, "auxiliary.summarize").await;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.outcome == CallOutcome::Succeeded));

    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

/// 一次应答多个工具调用 = 现役 toolViolation「tool intent ceiling
/// exceeded」→ 响亮失败（placeholder 只应答恰好一次恰好一个调用）。
#[tokio::test]
async fn multiple_tool_calls_in_one_turn_fail_loudly() {
    let stub = StubServer::start(vec![sse_tool_calls(&[
        ("call_multi_1", "read", "{\"path\":\"/f/a\"}"),
        ("call_multi_2", "read", "{\"path\":\"/f/b\"}"),
    ])])
    .await;
    let runtime = fresh_dir("runtime-toolmulti");
    let service = build_compaction_service(&plane_json(&stub, Some(1_000), true), &runtime);
    let (state, home) = boot_storage_only("toolmulti").await;

    let exchange = long_exchange(6, 8_000);
    let snapshot = read_snapshot();
    let usage = ModelCallUsage::reported(880, 20);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-t02");
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &MidRunCompactionInput {
                exchange: &exchange,
                system_prompt: None,
                submission: "汇总",
                tools: &snapshot,
                last_usage: Some(&usage),
                tail_from: exchange.len(),
            },
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    let CompactionOutcome::Failed { detail } = outcome else {
        panic!("multiple calls in one turn must fail loudly: {outcome:?}");
    };
    assert!(
        detail.contains("exactly one call exactly once"),
        "the placeholder ceiling wording: {detail}"
    );
    assert!(
        detail.contains('2'),
        "the detail counts the calls: {detail}"
    );
    assert_eq!(
        stub.hits(),
        1,
        "no recovery call is sent for a ceiling breach"
    );
    let rows = ledger_rows(&state, "auxiliary.summarize").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].outcome, CallOutcome::Succeeded);

    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

// ─── 直测：不触发各臂 ───

#[tokio::test]
async fn below_threshold_never_calls_the_model() {
    let stub = StubServer::start(vec![]).await;
    let runtime = fresh_dir("runtime-below");
    let service = build_compaction_service(&plane_json(&stub, Some(100_000), true), &runtime);
    let (state, home) = boot_storage_only("below").await;

    let exchange = long_exchange(2, 200);
    let snapshot = read_snapshot();
    // usage=100，window=100000：ratio 0.1% < 80%，reserve=20000，
    // window-reserve=80000 > 100 → 不触发。
    let usage = ModelCallUsage::reported(90, 10);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-t02");
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &MidRunCompactionInput {
                exchange: &exchange,
                system_prompt: None,
                submission: "汇总",
                tools: &snapshot,
                last_usage: Some(&usage),
                tail_from: exchange.len(),
            },
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    assert!(matches!(outcome, CompactionOutcome::NotTriggered));
    assert_eq!(stub.hits(), 0, "no model call below the threshold");
    assert!(ledger_rows(&state, "auxiliary.summarize").await.is_empty());

    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

#[tokio::test]
async fn undeclared_window_never_triggers() {
    // settings.enabled 用户开关与 keepRecent/reserve 覆盖面未接（归配置面
    // 任务；Rust 侧结构性门=窗口声明）——§十#9。
    let stub = StubServer::start(vec![]).await;
    let runtime = fresh_dir("runtime-nowin");
    let service = build_compaction_service(&plane_json(&stub, None, true), &runtime);
    let (state, home) = boot_storage_only("nowin").await;

    let exchange = long_exchange(6, 8_000);
    let snapshot = read_snapshot();
    let usage = ModelCallUsage::reported(999_999, 1);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-t02");
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &MidRunCompactionInput {
                exchange: &exchange,
                system_prompt: None,
                submission: "汇总",
                tools: &snapshot,
                last_usage: Some(&usage),
                tail_from: exchange.len(),
            },
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    assert!(matches!(outcome, CompactionOutcome::NotTriggered));
    assert_eq!(stub.hits(), 0);

    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

/// 手动压缩入口（任务书步骤①的"手动压缩"）：跳过阈值判定——窗口未
/// 声明、无 usage 事实（自动路径的两道门都不满足）也照压；reserve 按
/// 现役公式取缺省 16384 → 输出上限 max(512, floor(0.8×16384)) = 13107。
#[tokio::test]
async fn manual_compaction_skips_the_threshold_and_compacts() {
    // 手动压缩的 slash 命令面未接（归壳层/CLI 与 R06-T04）——§十#3。
    let stub = StubServer::start(vec![sse_final(&golden_summary())]).await;
    let runtime = fresh_dir("runtime-manual");
    // chat 槽无 compat（窗口未声明）；对照：同形状的自动路径是
    // undeclared_window_never_triggers 的 NotTriggered。
    let service = build_compaction_service(&plane_json(&stub, None, true), &runtime);
    let (state, home) = boot_storage_only("manual").await;

    let exchange = long_exchange(6, 8_000);
    let snapshot = read_snapshot();
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-t02");
    let outcome = service
        .compact_now(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &MidRunCompactionInput {
                exchange: &exchange,
                system_prompt: Some("SYS-MANUAL"),
                submission: "手动压缩这段历史",
                tools: &snapshot,
                last_usage: None,
                tail_from: exchange.len(),
            },
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    let CompactionOutcome::Compacted {
        exchange: compacted,
        plan,
        report,
    } = outcome
    else {
        panic!("manual compaction must not consult the threshold: {outcome:?}");
    };
    assert_eq!(compacted.len(), exchange.len() - plan.cut_index + 1);
    // F-04：手动 /compact 产物不带 mid-run notice（现役 compactSession
    // 不追加；notice 只属于 run 内自动触发路径）。
    let ExchangeItem::CompactionSummary { mid_run, .. } = &compacted[0] else {
        panic!("the first item is the compaction summary");
    };
    assert!(!mid_run, "manual compaction carries NO mid-run notice");
    // 手动路径的账目：reserve 缺省 16384，窗口 0（未声明），usage 未知。
    assert_eq!(report.output_reserve_tokens, 16_384);
    assert_eq!(report.context_window, 0);
    assert_eq!(report.usage_context_tokens, None);

    // 线体：一次摘要调用；openai-completions 属 optional-cap 族——
    // 线上无输出上限字段（§十#5；0.8×reserve 公式不是线体默认）。
    let bodies = stub.bodies();
    assert_eq!(bodies.len(), 1);
    let body: serde_json::Value = serde_json::from_str(&bodies[0]).expect("request json");
    assert!(
        body.get("max_tokens").is_none()
            && body.get("max_output_tokens").is_none()
            && body.get("max_completion_tokens").is_none(),
        "the optional-cap family sends NO output cap field on the wire: {body}"
    );
    let messages = body["messages"].as_array().expect("messages");
    assert_eq!(messages[0]["content"], "SYS-MANUAL");
    assert_eq!(messages[1]["content"], "手动压缩这段历史");

    let rows = ledger_rows(&state, "auxiliary.summarize").await;
    assert_eq!(rows.len(), 1, "manual calls account the same way");
    assert_eq!(rows[0].outcome, CallOutcome::Succeeded);

    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

#[tokio::test]
async fn unknown_usage_never_triggers_the_service() {
    let stub = StubServer::start(vec![]).await;
    let runtime = fresh_dir("runtime-nousage");
    let service = build_compaction_service(&plane_json(&stub, Some(1_000), true), &runtime);
    let (state, home) = boot_storage_only("nousage").await;

    let exchange = long_exchange(6, 8_000);
    let snapshot = read_snapshot();
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-t02");
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &MidRunCompactionInput {
                exchange: &exchange,
                system_prompt: None,
                submission: "汇总",
                tools: &snapshot,
                last_usage: None,
                tail_from: exchange.len(),
            },
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    assert!(matches!(outcome, CompactionOutcome::NotTriggered));
    assert_eq!(stub.hits(), 0);

    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

// ─── 直测：能力门 / 配对不可证明 / 取消 ───

#[tokio::test]
async fn capability_gate_refuses_a_toolsless_summarize_route() {
    // 压缩请求携带 live 工具快照（历史渲染必需）→ summarize 路由未声明
    // tools 能力 = 本地响亮拒绝（0 次物理请求，F01 前检），台账 Failed
    // 行 attempts=Some(0)（not-sent 绝不记成 1，F38/F21）。
    let stub = StubServer::start(vec![]).await;
    let runtime = fresh_dir("runtime-capgate");
    let service = build_compaction_service(&plane_json(&stub, Some(1_000), false), &runtime);
    let (state, home) = boot_storage_only("capgate").await;

    let exchange = long_exchange(6, 8_000);
    let snapshot = read_snapshot();
    let usage = ModelCallUsage::reported(880, 20);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-t02");
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &MidRunCompactionInput {
                exchange: &exchange,
                system_prompt: None,
                submission: "汇总",
                tools: &snapshot,
                last_usage: Some(&usage),
                tail_from: exchange.len(),
            },
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    let CompactionOutcome::Failed { detail } = outcome else {
        panic!("the capability gate must fail loudly: {outcome:?}");
    };
    assert!(
        detail.contains("tools"),
        "the refusal names the capability: {detail}"
    );
    assert_eq!(stub.hits(), 0, "refused before any transport");
    let rows = ledger_rows(&state, "auxiliary.summarize").await;
    assert_eq!(rows.len(), 1, "a refused call still accounts");
    assert_eq!(rows[0].outcome, CallOutcome::Failed);
    assert_eq!(rows[0].transport_attempts, Some(0), "not-sent is never 1");

    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

#[tokio::test]
async fn unprovable_pairs_fail_without_a_model_call() {
    // 读时孤儿修复未迁移；Rust 等价保护 = 规划期证明（不可证明即拒绝）——§十#2。
    let stub = StubServer::start(vec![]).await;
    let runtime = fresh_dir("runtime-unprovable");
    let service = build_compaction_service(&plane_json(&stub, Some(1_000), true), &runtime);
    let (state, home) = boot_storage_only("unprovable").await;

    // 孤立 tool result（无归属 assistant turn）——配对不可证明。
    let mut exchange = long_exchange(6, 8_000);
    exchange.insert(0, tool_result(99, "orphan"));
    let snapshot = read_snapshot();
    let usage = ModelCallUsage::reported(880, 20);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-t02");
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &MidRunCompactionInput {
                exchange: &exchange,
                system_prompt: None,
                submission: "汇总",
                tools: &snapshot,
                last_usage: Some(&usage),
                tail_from: exchange.len(),
            },
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    let CompactionOutcome::Failed { detail } = outcome else {
        panic!("unprovable pairs must fail loudly: {outcome:?}");
    };
    assert!(detail.contains("unprovable"), "{detail}");
    assert_eq!(stub.hits(), 0, "no model call when planning refuses");
    assert!(ledger_rows(&state, "auxiliary.summarize").await.is_empty());

    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

#[tokio::test]
async fn cancellation_during_the_summary_call_accounts_cancelled() {
    let stub = StubServer::start(vec![sse_final(&golden_summary())]).await;
    let runtime = fresh_dir("runtime-cancel");
    let service = build_compaction_service(&plane_json(&stub, Some(1_000), true), &runtime);
    let (state, home) = boot_storage_only("cancel").await;

    let exchange = long_exchange(6, 8_000);
    let snapshot = read_snapshot();
    let usage = ModelCallUsage::reported(880, 20);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-t02");
    // 先取消再进调用——select! 的 biased 取消臂必胜。
    cancel.cancel("test-cancel");
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &MidRunCompactionInput {
                exchange: &exchange,
                system_prompt: None,
                submission: "汇总",
                tools: &snapshot,
                last_usage: Some(&usage),
                tail_from: exchange.len(),
            },
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    assert!(matches!(outcome, CompactionOutcome::Cancelled));
    let rows = ledger_rows(&state, "auxiliary.summarize").await;
    assert_eq!(rows.len(), 1, "the cancelled call accounts");
    assert_eq!(rows[0].outcome, CallOutcome::Cancelled);
    assert_eq!(
        rows[0].transport_attempts, None,
        "a dropped-before-settlement call never fabricates an attempt count"
    );

    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

// ─── drive_run 生产链接线（A03/A04 的 service 腿） ───

/// 记录每次调用的 prior 形状与 usage 脚本的 provider double。
struct ScriptedUsageProvider {
    script: Mutex<VecDeque<(ProviderTurn, Option<UsageRecord>)>>,
    priors: Mutex<Vec<Vec<&'static str>>>,
}

impl ScriptedUsageProvider {
    fn new(script: Vec<(ProviderTurn, Option<UsageRecord>)>) -> Arc<Self> {
        Arc::new(Self {
            script: Mutex::new(script.into_iter().collect()),
            priors: Mutex::new(Vec::new()),
        })
    }

    fn priors(&self) -> Vec<Vec<&'static str>> {
        self.priors.lock().unwrap().clone()
    }
}

fn continue_turn(text: String) -> ProviderTurn {
    ProviderTurn::Continue { process_note: text }
}

fn final_turn(text: &str) -> ProviderTurn {
    ProviderTurn::Final {
        message: NormalizedMessage {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            model_call_id: None,
        },
    }
}

fn big_usage() -> UsageRecord {
    UsageRecord {
        input_tokens: 880,
        output_tokens: 20,
    }
}

impl TurnProviderPort for ScriptedUsageProvider {
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
        _call: &'a ModelCallId,
        input: &'a ModelTurnInput,
        _deltas: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        self.priors.lock().unwrap().push(
            input
                .prior
                .iter()
                .map(|item| match item {
                    ExchangeItem::AssistantTurn { .. } => "assistant",
                    ExchangeItem::ToolResult { .. } => "tool_result",
                    ExchangeItem::CompactionSummary { .. } => "compaction_summary",
                    ExchangeItem::CompactionInstruction { .. } => "compaction_instruction",
                })
                .collect(),
        );
        let (turn, usage) = self
            .script
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| (final_turn("done"), None));
        let ctx_at_issue = ctx.clone();
        Box::pin(async move {
            match usage {
                Some(usage) => ProviderTurnResult::of_ctx_with_usage(&ctx_at_issue, turn, usage),
                None => ProviderTurnResult::of_ctx(&ctx_at_issue, turn),
            }
        })
    }
}

async fn boot_driven(
    tag: &str,
    script: Vec<(ProviderTurn, Option<UsageRecord>)>,
    compaction: Option<Arc<CompactionService>>,
) -> (ServiceState, Arc<ScriptedUsageProvider>, PathBuf) {
    let home = fresh_dir(tag);
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static addr"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let layout = prepare_layout(&home).expect("layout");
    let provider = ScriptedUsageProvider::new(script);
    let deps = ServiceDeps {
        turn_provider: Some(provider.clone() as Arc<dyn TurnProviderPort>),
        compaction_service: compaction,
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
        .await
        .expect("bootstrap");
    (state, provider, home)
}

fn owner_principal() -> lingxi_service::Principal {
    lingxi_service::Principal {
        schema_version: 1,
        principal_id: "principal_local".to_string(),
        kind: lingxi_service::PrincipalKind::LocalUser,
        user_id: Some(lingxi_service::LOCAL_OWNER_USER_ID.to_string()),
        studio_id: None,
        server_node_id: None,
        device_id: None,
        credential_id: None,
        web_session_id: None,
        connection_kind: lingxi_service::auth::ConnectionKindSerde::Local,
        credential_kind: lingxi_service::CredentialKind::LoopbackToken,
        trust_state: lingxi_service::TrustState::Local,
        scopes: vec!["chat".to_string()],
    }
}

async fn drive(state: &ServiceState, input: &str) {
    state
        .sessions()
        .execute_for(
            state.storage().as_ref(),
            state.events(),
            state.runs(),
            &owner_principal(),
            "sess_local_alpha",
            input,
            NOW_MS,
        )
        .await
        .expect("run drives");
}

/// A03/A04 正例主链：长历史越过 FORCE 线 → loop 顶触发自动压缩 → 摘要经
/// 真 ModelGateway 落到 loopback stub → 压缩后的 live 交换携带摘要项
/// （普通 user 历史身份）→ run 正常完成。
#[tokio::test]
async fn driven_run_compacts_mid_run_and_continues_with_the_summary() {
    // 压缩作用域 = 当前 run 的 exchange（现役 = 整会话，归 R06-T04）——§十#15。
    let stub = StubServer::start(vec![sse_final(&golden_summary())]).await;
    let runtime = fresh_dir("runtime-drive");
    let service = build_compaction_service(&plane_json(&stub, Some(1_000), true), &runtime);
    // 4 个 process-only turn（各 12000 字符 ≈ 3000 tokens；4 项 ≈ 12000 <
    // keep_recent 20000 … 需要超 20000：用 30000 字符 ≈ 7500 tokens/项，
    // 4 项 = 30000 > 20000 → 可切）。usage=900 过 FORCE 线（window=1000）。
    let script = vec![
        (continue_turn("c".repeat(30_000)), Some(big_usage())),
        (continue_turn("c".repeat(30_000)), Some(big_usage())),
        (continue_turn("c".repeat(30_000)), Some(big_usage())),
        (continue_turn("c".repeat(30_000)), Some(big_usage())),
        (final_turn("汇总完成"), None),
    ];
    let (state, provider, home) = boot_driven("drive-fire", script, Some(service)).await;

    drive(&state, "处理这个长任务").await;

    let priors = provider.priors();
    assert_eq!(priors.len(), 5, "4 process turns + 1 final");
    // 前四次调用：交换只含 assistant（process-only）turns。
    assert_eq!(priors[0].len(), 0);
    assert_eq!(priors[1], vec!["assistant"]);
    assert_eq!(priors[2], vec!["assistant", "assistant"]);
    assert_eq!(priors[3], vec!["assistant", "assistant", "assistant"]);
    // 第五次调用的 prior = [摘要项, ...保留区]——压缩在 turn5 的 loop 顶
    // 触发（turn4 settle 后 usage=900 过线，4 项估算 30000 > 20000）。
    assert_eq!(
        priors[4][0], "compaction_summary",
        "the compacted exchange opens with the summary item: {:?}",
        priors[4]
    );
    assert!(
        priors[4][1..].iter().all(|tag| *tag == "assistant"),
        "the retained suffix is verbatim assistant turns: {:?}",
        priors[4]
    );
    assert!(
        priors[4].len() < 5,
        "the old region was replaced: {:?}",
        priors[4]
    );

    // 摘要调用上了 stub（真 ModelGateway/凭证/网络链）。
    assert_eq!(stub.hits(), 1, "one summary call");
    let body: serde_json::Value =
        serde_json::from_str(&stub.bodies()[0]).expect("summary request json");
    let text = serde_json::to_string(&body).expect("serialize");
    assert!(text.contains("Internal compaction-only run."));
    // 台账：5 行 chat + 1 行 auxiliary.summarize（Succeeded）。
    let summary_rows = ledger_rows(&state, "auxiliary.summarize").await;
    assert_eq!(summary_rows.len(), 1);
    assert_eq!(summary_rows[0].outcome, CallOutcome::Succeeded);
    let chat_rows = ledger_rows(&state, "chat").await;
    assert_eq!(chat_rows.len(), 5, "every chat call accounts");

    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

/// A04 drive 腿：摘要 provider 返回错误 → 压缩失败 → run 以原交换续跑
/// （原始记录保留、错误明确、台账 Failed 行），run 正常完成。
#[tokio::test]
async fn driven_run_survives_a_failed_compaction() {
    let stub = StubServer::start(vec![sse_content_filter_refusal()]).await;
    let runtime = fresh_dir("runtime-drive-fail");
    let service = build_compaction_service(&plane_json(&stub, Some(1_000), true), &runtime);
    let script = vec![
        (continue_turn("c".repeat(30_000)), Some(big_usage())),
        (continue_turn("c".repeat(30_000)), Some(big_usage())),
        (continue_turn("c".repeat(30_000)), Some(big_usage())),
        (continue_turn("c".repeat(30_000)), Some(big_usage())),
        (final_turn("汇总完成"), None),
    ];
    let (state, provider, home) = boot_driven("drive-fail", script, Some(service)).await;

    drive(&state, "处理这个长任务").await;

    let priors = provider.priors();
    assert_eq!(priors.len(), 5);
    // 压缩失败：第五次调用的 prior 是原交换（4 个 assistant，无摘要项）。
    assert_eq!(
        priors[4],
        vec!["assistant", "assistant", "assistant", "assistant"],
        "the original exchange continues untouched: {:?}",
        priors[4]
    );
    // 台账对照：4 行 chat Succeeded（前四个 process turn）+ 1 行 chat
    // Succeeded（final）+ 1 行 auxiliary.summarize Failed——原始记录完整，
    // 失败明确落账。
    let chat_rows = ledger_rows(&state, "chat").await;
    assert_eq!(chat_rows.len(), 5, "the original chat rows stay whole");
    assert!(chat_rows
        .iter()
        .all(|row| row.outcome == CallOutcome::Succeeded));
    let summary_rows = ledger_rows(&state, "auxiliary.summarize").await;
    assert_eq!(summary_rows.len(), 1, "the failed summary call accounts");
    assert_eq!(summary_rows[0].outcome, CallOutcome::Failed);
    assert_eq!(stub.hits(), 1);

    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

/// 回归腿：不接线 compaction service 时，drive 形状与 pre-R06-T02 完全
/// 一致（零 stub 命中、prior 无摘要项）。
#[tokio::test]
async fn without_compaction_service_the_drive_shape_is_unchanged() {
    // （本测试 Continue 轮全部带 tool_calls 或大 content；reasoning-only
    // turn 的 openai 族渲染形态差异见 §十#4。）
    let script = vec![
        (continue_turn("c".repeat(1_000)), Some(big_usage())),
        (final_turn("done"), None),
    ];
    let (state, provider, home) = boot_driven("drive-none", script, None).await;
    drive(&state, "普通任务").await;
    let priors = provider.priors();
    assert_eq!(priors.len(), 2);
    assert_eq!(priors[1], vec!["assistant"]);
    assert!(ledger_rows(&state, "auxiliary.summarize").await.is_empty());
    teardown(&state, &home).await;
}

// ─── FIX-01（R2-F-01）：required-cap 族的线体回填（现役
// safeRequiredOutputCap：min(model.maxTokens‖maxOutput, contextWindow)） ───

/// anthropic-messages SSE：一段文本 + end_turn + usage（r05_t03 形状）。
fn anthropic_sse_final(text: &str) -> StubResponse {
    let mut body = String::new();
    for frame in [
        json!({"type":"message_start","message":{"id":"msg-t02","model":"stub","usage":{"input_tokens":31}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":7}}),
        json!({"type":"message_stop"}),
    ] {
        let ty = frame["type"].as_str().expect("typed frame");
        body.push_str(&format!("event: {ty}\ndata: {frame}\n\n"));
    }
    StubResponse { body }
}

/// chat 槽留在 openai 族，summarize 槽挂 anthropic-messages provider
/// （required-cap 族），compat 声明 contextWindow 与 maxTokens。
fn plane_json_anthropic_summarize(
    stub: &StubServer,
    summarize_window: u64,
    summarize_max_tokens: u64,
) -> String {
    format!(
        r#"{{
            "providers": {{
                "stub_svc": {{
                    "protocol": "openai-completions",
                    "endpoint": "{}",
                    "auth": {{"kind": "apiKey", "apiKey": "{STUB_API_KEY}"}}
                }},
                "stub_anth": {{
                    "protocol": "anthropic-messages",
                    "endpoint": "{}",
                    "auth": {{"kind": "apiKey", "apiKey": "{STUB_API_KEY}"}}
                }}
            }},
            "models": {{
                "chat": {{"provider": "stub_svc", "model": "chat-model", "capabilities": {{"tools": true}}, "compat": {{"contextWindow": 1000}}}},
                "summarize": {{"provider": "stub_anth", "model": "summarize-anth", "capabilities": {{"tools": true}}, "compat": {{"contextWindow": {summarize_window}, "maxTokens": {summarize_max_tokens}}}}}
            }}
        }}"#,
        stub.v1(),
        stub.v1()
    )
}

#[tokio::test]
async fn required_cap_family_backfills_min_of_declared_caps_on_the_wire() {
    // 两个 min 方向各一例（§十#5 / FIX-01；现役回填
    // min(model.maxTokens, contextWindow)）：
    // - maxTokens=8000 < contextWindow=100000 → 线上 8000；
    // - maxTokens=100000 > contextWindow=50000 → 线上 50000。
    // （50000 窗口下估算总量 ≈ 3.9 万 < floor(50000×0.85)，fit 通过。）
    for (window, declared_max, expected) in [
        (100_000_u64, 8_000_u64, 8_000_u64),
        (50_000, 100_000, 50_000),
    ] {
        let stub = StubServer::start(vec![anthropic_sse_final(&golden_summary())]).await;
        let runtime = fresh_dir("runtime-anth-cap");
        let service = build_compaction_service(
            &plane_json_anthropic_summarize(&stub, window, declared_max),
            &runtime,
        );
        let (state, home) = boot_storage_only("anth-cap").await;
        let exchange = long_exchange(6, 8_000);
        let snapshot = read_snapshot();
        let usage = ModelCallUsage::reported(880, 20);
        let cancel = lingxi_service::cancel::CancelScope::run_root("run-t02");
        let outcome = service
            .maybe_compact_mid_run(
                &test_ctx(),
                state.storage().as_ref(),
                "agent:local",
                "user",
                None,
                None,
                &MidRunCompactionInput {
                    exchange: &exchange,
                    system_prompt: None,
                    submission: "汇总",
                    tools: &snapshot,
                    last_usage: Some(&usage),
                    tail_from: exchange.len(),
                },
                &cancel,
                1,
            )
            .await
            .expect("no storage failure");
        assert!(
            matches!(outcome, CompactionOutcome::Compacted { .. }),
            "the anthropic-arm compaction must land: {outcome:?}"
        );
        let bodies = stub.bodies();
        assert_eq!(bodies.len(), 1, "one summary call");
        let body: serde_json::Value = serde_json::from_str(&bodies[0]).expect("request json");
        assert_eq!(
            body["max_tokens"],
            json!(expected),
            "required-cap family wire cap = min(maxTokens={declared_max}, contextWindow={window})"
        );
        stub.stop().await;
        teardown(&state, &home).await;
        let _ = std::fs::remove_dir_all(&runtime);
    }
}

// ─── FIX-07（N-1）：摘要请求自身超窗 → 诚实硬截断（不调模型） ───

#[tokio::test]
async fn over_window_summary_request_hard_truncates_without_a_model_call() {
    // marker 用共享管线变体（guard-ext 措辞变体不迁移）——§十#7。
    // summarize 窗口 1000：fit 估算（历史 ≈2.4 万 + 输出上限公式 13107 +
    // 缓冲 1024 + …）远超 floor(1000×0.85)=850 → 现役硬截断臂。
    let stub = StubServer::start(vec![]).await;
    let runtime = fresh_dir("runtime-trunc");
    let service = build_compaction_service(
        &plane_json_with_summarize_window(&stub, Some(1_000), true, 1_000),
        &runtime,
    );
    let (state, home) = boot_storage_only("trunc").await;

    let exchange = long_exchange(6, 8_000);
    let snapshot = read_snapshot();
    let usage = ModelCallUsage::reported(880, 20);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-t02");
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &MidRunCompactionInput {
                exchange: &exchange,
                system_prompt: Some("SYS"),
                submission: "汇总",
                tools: &snapshot,
                last_usage: Some(&usage),
                tail_from: exchange.len(),
            },
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
        panic!("the over-window request must hard-truncate honestly: {outcome:?}");
    };
    // 产物带降级标注（逐字现役标记文本）+ 保留区逐字 + 不追加 fileOps 段
    // （现役硬截断的 details 只有 reason/keepRecent）。
    let ExchangeItem::CompactionSummary {
        summary, mid_run, ..
    } = &truncated[0]
    else {
        panic!("the first item is the marker summary");
    };
    assert_eq!(
        summary,
        lingxi_kernel::compaction::HARD_TRUNCATE_MARKER_TEXT,
        "the marker text is the incumbent's verbatim"
    );
    assert!(mid_run, "mid-run trigger marks the notice");
    assert_eq!(
        &truncated[1..],
        &exchange[plan.cut_index..],
        "the retained suffix is verbatim"
    );
    // 不调摘要模型（天然无每-turn 重试）、不落摘要台账行。
    assert_eq!(stub.hits(), 0, "no summary request left the process");
    let rows = ledger_rows(&state, "auxiliary.summarize").await;
    assert_eq!(rows.len(), 0, "no model call, no ledger row");

    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

#[tokio::test]
async fn hard_truncate_no_progress_guard_skips_when_only_summaries_remain() {
    // 无进展护栏（现役 computeHardTruncation `effectiveCutIndex<=0 → null`
    // 的对应物）：旧区只剩摘要项时硬截断不产生新信息——跳过而不是
    // 每 turn 用同文标记替换同文标记。
    let stub = StubServer::start(vec![]).await;
    let runtime = fresh_dir("runtime-trunc-guard");
    let service = build_compaction_service(
        &plane_json_with_summarize_window(&stub, Some(1_000), true, 1_000),
        &runtime,
    );
    let (state, home) = boot_storage_only("trunc-guard").await;

    // [上一摘要] + 一个巨型组（单组 ≥keep_recent → 切点恒为 1，旧区
    // 恒为 [摘要]）。
    let exchange = vec![
        ExchangeItem::CompactionSummary {
            summary: "上一轮摘要".to_string(),
            covered_items: 9,
            mid_run: true,
        },
        assistant_turn(1, &"x".repeat(90_000), true),
        tool_result(1, &"r".repeat(1_000)),
    ];
    let snapshot = read_snapshot();
    let usage = ModelCallUsage::reported(880, 20);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-t02");
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &MidRunCompactionInput {
                exchange: &exchange,
                system_prompt: None,
                submission: "汇总",
                tools: &snapshot,
                last_usage: Some(&usage),
                tail_from: exchange.len(),
            },
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    assert!(
        matches!(outcome, CompactionOutcome::NotTriggered),
        "no-progress guard skips: {outcome:?}"
    );
    assert_eq!(stub.hits(), 0);

    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

// ─── R3-F-01：摘要模型路由 = summarize 槽显式覆盖、未配置回退 chat 路由
// （现役 `core/auxiliary-slots.ts:83-88` summarize 槽 `fallback:"chat"`；现役
// 会话压缩摘要用会话模型本人——`core/session-compactor.ts:1987`
// `model = session?.model`，fit 检查同模型 :2064-2074）——§十#17 ───

/// 无 summarize 绑定的 plane（现役同配置：只配 chat 时压缩走会话模型本人）。
fn plane_json_without_summarize(stub: &StubServer, chat_window: Option<u64>) -> String {
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

/// summarize 绑定存在但**未声明 compat**（无 contextWindow）的 plane。
fn plane_json_summarize_without_window(stub: &StubServer, chat_window: u64) -> String {
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
                "summarize": {{"provider": "stub_svc", "model": "summarize-model", "capabilities": {{"tools": true}}}}
            }}
        }}"#,
        stub.v1()
    )
}

/// summarize 槽与 chat 绑定都缺失的 plane（回退链两端皆空臂）。
fn plane_json_without_chat_and_summarize(stub: &StubServer) -> String {
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

/// 臂 A：summarize 槽未配置 → 回退 chat 路由（现役 fallback:"chat" 对齐）。
/// 修复前（R3 探针 D3 实证）：每次触发 Failed、0 调用、上下文无限增长。
#[tokio::test]
async fn missing_summarize_slot_falls_back_to_the_chat_route() {
    let stub = StubServer::start(vec![sse_final(&golden_summary())]).await;
    let runtime = fresh_dir("runtime-slot-fallback");
    // chat 窗口 100_000：usage 90_000 过 FORCE 线（80_000）；fit 窗口 =
    // 生效路由（chat）窗口，估算 ≈4.3 万 < floor(100000×0.85)。
    let service = build_compaction_service(
        &plane_json_without_summarize(&stub, Some(100_000)),
        &runtime,
    );
    let (state, home) = boot_storage_only("slot-fallback").await;

    let exchange = long_exchange(6, 8_000);
    let snapshot = read_snapshot();
    let usage = ModelCallUsage::reported(88_000, 2_000);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-t02");
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &MidRunCompactionInput {
                exchange: &exchange,
                system_prompt: None,
                submission: "汇总",
                tools: &snapshot,
                last_usage: Some(&usage),
                tail_from: exchange.len(),
            },
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    let CompactionOutcome::Compacted { .. } = outcome else {
        panic!("the missing summarize slot must fall back to the chat route: {outcome:?}");
    };
    assert_eq!(stub.hits(), 1, "the summary call left via the chat route");
    let body: serde_json::Value = serde_json::from_str(&stub.bodies()[0]).expect("request json");
    assert_eq!(
        body["model"], "chat-model",
        "the effective route is the chat route: {body}"
    );
    // 台账：purpose 仍是语义槽 auxiliary.summarize，身份是 RESOLVED 的
    // chat 路由（回退不伪造槽身份）。
    let rows = ledger_rows(&state, "auxiliary.summarize").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].outcome, CallOutcome::Succeeded);
    assert_eq!(rows[0].model, "chat-model");

    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

/// 臂 B：summarize 槽已配置但窗口未声明 → fit 检查取回退路由（chat）窗口
/// 而非硬截断。修复前（R3 探针 D2 实证）：每次触发 HardTruncated（marker
/// 替换真实历史、0 调用）。
#[tokio::test]
async fn undeclared_summarize_window_falls_back_to_the_chat_window_not_hard_truncation() {
    let stub = StubServer::start(vec![sse_final(&golden_summary())]).await;
    let runtime = fresh_dir("runtime-window-fallback");
    let service = build_compaction_service(
        &plane_json_summarize_without_window(&stub, 100_000),
        &runtime,
    );
    let (state, home) = boot_storage_only("window-fallback").await;

    let exchange = long_exchange(6, 8_000);
    let snapshot = read_snapshot();
    let usage = ModelCallUsage::reported(88_000, 2_000);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-t02");
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &MidRunCompactionInput {
                exchange: &exchange,
                system_prompt: None,
                submission: "汇总",
                tools: &snapshot,
                last_usage: Some(&usage),
                tail_from: exchange.len(),
            },
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    let CompactionOutcome::Compacted { .. } = outcome else {
        panic!(
            "an undeclared summarize window must fall back to the chat window, \
             never silently hard-truncate: {outcome:?}"
        );
    };
    assert_eq!(
        stub.hits(),
        1,
        "the summary call left via the summarize route"
    );
    let body: serde_json::Value = serde_json::from_str(&stub.bodies()[0]).expect("request json");
    assert_eq!(
        body["model"], "summarize-model",
        "the explicit slot override still serves: {body}"
    );
    let rows = ledger_rows(&state, "auxiliary.summarize").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].model, "summarize-model");

    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

/// 回退链两端皆空：summarize 槽与 chat 路由都不可解析 → 响亮失败、原交换
/// 不动（自动路径不可达此臂——chat 不可解析时触发门已 NotTriggered；
/// 手动路径覆盖）。修复前的「槽缺失即 Failed」语义只在这一臂保留。
#[tokio::test]
async fn unresolvable_summarize_slot_and_chat_route_fail_loudly() {
    let stub = StubServer::start(vec![]).await;
    let runtime = fresh_dir("runtime-both-missing");
    let service = build_compaction_service(&plane_json_without_chat_and_summarize(&stub), &runtime);
    let (state, home) = boot_storage_only("both-missing").await;

    let exchange = long_exchange(6, 8_000);
    let original = exchange.clone();
    let snapshot = read_snapshot();
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-t02");
    let outcome = service
        .compact_now(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &MidRunCompactionInput {
                exchange: &exchange,
                system_prompt: None,
                submission: "手动压缩",
                tools: &snapshot,
                last_usage: None,
                tail_from: exchange.len(),
            },
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    let CompactionOutcome::Failed { detail } = outcome else {
        panic!("both routes unresolvable must fail loudly: {outcome:?}");
    };
    assert!(
        detail.contains("summarize") && detail.contains("chat"),
        "the detail names BOTH legs of the fallback chain: {detail}"
    );
    assert_eq!(stub.hits(), 0, "route failure = zero physical calls");
    assert_eq!(exchange, original, "the original exchange is untouched");
    assert!(ledger_rows(&state, "auxiliary.summarize").await.is_empty());

    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

// ─── FIX-02（R2-F-02）：fileOps 段随摘要落交换（生产链腿） ───

#[tokio::test]
async fn file_ops_sections_ride_the_compacted_summary() {
    let stub = StubServer::start(vec![sse_final(&golden_summary())]).await;
    let runtime = fresh_dir("runtime-fileops");
    let service = build_compaction_service(&plane_json(&stub, Some(1_000), true), &runtime);
    let (state, home) = boot_storage_only("fileops").await;

    let exchange = long_exchange(6, 8_000);
    let snapshot = read_snapshot();
    let usage = ModelCallUsage::reported(880, 20);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-t02");
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &MidRunCompactionInput {
                exchange: &exchange,
                system_prompt: None,
                submission: "汇总",
                tools: &snapshot,
                last_usage: Some(&usage),
                tail_from: exchange.len(),
            },
            &cancel,
            1,
        )
        .await
        .expect("no storage failure");
    let CompactionOutcome::Compacted {
        exchange: compacted,
        plan,
        ..
    } = outcome
    else {
        panic!("compaction must land: {outcome:?}");
    };
    let ExchangeItem::CompactionSummary { summary, .. } = &compacted[0] else {
        panic!("summary item");
    };
    // 旧区全部调用都是 read（long_exchange 的夹具形状）→ <read-files>
    // 段带旧区路径，无 <modified-files> 段。
    assert!(
        summary.contains("<read-files>"),
        "the read section rides: {summary}"
    );
    assert!(
        !summary.contains("<modified-files>"),
        "no writes happened: {summary}"
    );
    for item in &exchange[..plan.cut_index] {
        if let ExchangeItem::AssistantTurn { tool_calls, .. } = item {
            for call in tool_calls {
                let path = call.arguments.as_value()["path"].as_str().expect("path");
                assert!(summary.contains(path), "old-region read path {path} listed");
            }
        }
    }

    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

// ─── FIX-08（N-2）：失败 turn 的 usage 不触发压缩（现役
// stopReason∉{error,aborted} 门） ───

#[tokio::test]
async fn failed_turn_usage_never_triggers_compaction() {
    // stopReason∈{error,aborted} 不触发压缩的 Rust 落位（失败 turn 的
    // usage 不捕获）——§十#16。
    use lingxi_protocol::{ErrorCode, ProtocolError};
    let stub = StubServer::start(vec![sse_final(&golden_summary())]).await;
    let runtime = fresh_dir("runtime-failgate");
    let service = build_compaction_service(&plane_json(&stub, Some(100_000), true), &runtime);
    // 前三个 turn 小 usage（远低于 FORCE/reserve 线）攒出可切历史；
    // 第四个 turn 的 attempt1 失败但携带超大 usage（90000/100000 过
    // FORCE 线）——FIX-08：失败 turn 的 usage 不捕获，重试前的 loop 顶
    // 不得触发压缩；attempt2 正常结束。
    let small = || UsageRecord {
        input_tokens: 1_000,
        output_tokens: 20,
    };
    let script = vec![
        (continue_turn("c".repeat(30_000)), Some(small())),
        (continue_turn("c".repeat(30_000)), Some(small())),
        (continue_turn("c".repeat(30_000)), Some(small())),
        // 第 4 组让交换可切（尾部 3 项 ≈22500 ≥ keep_recent，crossing=1，
        // cut=1 合法）；前三个 turn 的小 usage 保证此前不触发。
        (continue_turn("c".repeat(30_000)), Some(small())),
        (
            ProviderTurn::Failed {
                error: ProtocolError::new(ErrorCode::UpstreamUnavailable, "boom", true),
                retryable: true,
            },
            Some(UsageRecord {
                input_tokens: 90_000,
                output_tokens: 20,
            }),
        ),
        (final_turn("恢复完成"), None),
    ];
    let (state, provider, home) = boot_driven("failgate", script, Some(service)).await;

    drive(&state, "处理这个长任务").await;

    // 6 次调用（4 个 process turn + attempt1 失败 + attempt2 成功），
    // prior 从不含 compaction_summary——失败 turn 的 usage 没有触发压缩。
    let priors = provider.priors();
    assert_eq!(priors.len(), 6, "4 process turns + failed attempt + retry");
    for (index, prior) in priors.iter().enumerate() {
        assert!(
            !prior.contains(&"compaction_summary"),
            "prior #{index} carries no summary item: {prior:?}"
        );
    }
    assert_eq!(stub.hits(), 0, "no summary call ever left the process");
    let rows = ledger_rows(&state, "auxiliary.summarize").await;
    assert_eq!(rows.len(), 0, "no compaction ledger row");

    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

// ─── 防再发（RC-3 §五-4）：报告-测试交叉锁 ───
// 报告 §十 的每条「记录在案差异」必须在 rust/ 代码面（源码或测试）至少
// 被 `§十#N` 引用一次，且代码面引用不得指向不存在的条目——双向精确匹配
// （引用目标集合 == 条目集合）。「删中间条目 + 代码引用残留」组合必须变红
// （R3-F-03：旧实现用「引用 > max」近似「引用 ∉ 条目集合」，该组合逃逸
// 检测——R3 探针 F1 实证）。这是台账失信（RC-2）的机器门禁：锚定不再能
// 静默存活。

/// §十 条目号解析：段首数字编号行（"N. **..."，§十 区段内）。
fn parse_register_entries(report_text: &str) -> std::collections::BTreeSet<u32> {
    let section = report_text
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
    assert!(!entries.is_empty(), "§十 has numbered entries");
    entries
}

/// 代码面 `§十#N` 引用扫描（给定根目录下的 .rs 文件；跳过 target/ 与本
/// 测试自身的说明文字行）。
fn collect_code_references(root: &std::path::Path) -> std::collections::BTreeSet<u32> {
    let mut referenced = std::collections::BTreeSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                if path.file_name().map(|n| n == "target") == Some(true) {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if path.extension().map(|e| e == "rs") != Some(true) {
                continue;
            }
            let body = std::fs::read_to_string(&path).expect("read rs");
            for line in body.lines() {
                if line.contains("报告 §十 的每条") || line.contains("§十#N` 引用") {
                    continue; // 本测试自身的说明文字
                }
                let mut rest = line;
                while let Some(pos) = rest.find("§十#") {
                    let after = &rest[pos + "§十#".len()..];
                    let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
                    if !digits.is_empty() {
                        referenced.insert(digits.parse().expect("ref number"));
                    }
                    rest = &after[digits.len()..];
                }
            }
        }
    }
    referenced
}

/// 双向精确匹配：每条目至少一次代码面引用 ∧ 每个引用目标 ∈ 条目集合
/// （R3-F-03 修复：stray 判定从「引用 > max」改为「引用 ∉ 条目集合」——
/// 「删中间条目 + 引用残留 ≤ max」组合必须变红）。
fn check_register_lock(
    entries: &std::collections::BTreeSet<u32>,
    referenced: &std::collections::BTreeSet<u32>,
) -> Vec<String> {
    let mut problems = Vec::new();
    let missing: Vec<u32> = entries
        .iter()
        .copied()
        .filter(|n| !referenced.contains(n))
        .collect();
    if !missing.is_empty() {
        problems.push(format!(
            "§十 entries without any code-side `§十#N` reference: {missing:?}"
        ));
    }
    let stray: Vec<u32> = referenced
        .iter()
        .copied()
        .filter(|n| !entries.contains(n))
        .collect();
    if !stray.is_empty() {
        problems.push(format!("code references unknown §十 entries: {stray:?}"));
    }
    problems
}

#[test]
fn report_deviation_register_matches_test_references() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let report = root.join("docs/rust-tauri/R06/R06-T02_REPORT.md");
    let text = std::fs::read_to_string(&report)
        .unwrap_or_else(|e| panic!("read {}: {e}", report.display()));
    let entries = parse_register_entries(&text);
    let referenced = collect_code_references(&root.join("rust"));
    let problems = check_register_lock(&entries, &referenced);
    assert!(
        problems.is_empty(),
        "register lock violated:\n{}",
        problems.join("\n")
    );

    // ── 鉴别力自证（R3-F-03）：篡改样例必须变红 ──
    let tag = |n: u32| format!("§十#{n}");
    // (a) 删中间条目 + 代码引用残留（R3 探针 F1 的逃逸组合）：条目 {1,3}，
    // 引用 {1,2,3}——残留引用 2 必须被捕获。
    let tampered_entries = std::collections::BTreeSet::from([1, 3]);
    let tampered_refs = std::collections::BTreeSet::from([1, 2, 3]);
    assert!(
        !check_register_lock(&tampered_entries, &tampered_refs).is_empty(),
        "delete-middle-entry + dangling {} must go red",
        tag(2)
    );
    // (b) 加了条目没有任何代码面引用：条目 {1,2,3}，引用 {1,3}。
    let full_entries = std::collections::BTreeSet::from([1, 2, 3]);
    let partial_refs = std::collections::BTreeSet::from([1, 3]);
    assert!(
        !check_register_lock(&full_entries, &partial_refs).is_empty(),
        "entry without any {} reference must go red",
        tag(2)
    );
    // (c) 集合精确相等 → 绿。
    assert!(
        check_register_lock(&full_entries, &tampered_refs).is_empty(),
        "exact set equality stays green"
    );
    // (d) 端到端（临时目录）：篡改报告删中间条目 + .rs 残留引用，解析→
    // 扫描→判定全链必须变红。
    let dir = fresh_dir("register-lock");
    let tampered_report =
        "## 十、记录在案差异\n\n1. **甲**\n3. **丙**\n\n## 十一、次节\n".to_string();
    std::fs::write(
        dir.join("probe.rs"),
        format!("// 残留引用 {} 与 {}\n", tag(1), tag(2)),
    )
    .expect("write tampered rs");
    let parsed = parse_register_entries(&tampered_report);
    let scanned = collect_code_references(&dir);
    assert_eq!(parsed, std::collections::BTreeSet::from([1, 3]));
    assert_eq!(scanned, std::collections::BTreeSet::from([1, 2]));
    assert!(
        !check_register_lock(&parsed, &scanned).is_empty(),
        "the end-to-end tamper sample must go red"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
