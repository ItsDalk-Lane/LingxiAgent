//! REVIEWER-R06-T02-R3 的独立对抗探针（第三轮，全部为新构造，不重放
//! R1 的孤儿/双计/级联毒化与 R2 的 pending 中段/USAGE_MAPPINGS/级联/
//! 触发边界/切点方向探针）。
//!
//! 探针清单：
//!   A — fit 检查退化面：window=0（未声明）/window=1/u64::MAX 溢出安全
//!       （kernel 直测，补 85% 精确边界之外的未声明臂）。
//!   B — 无进展护栏的精确分层：kernel `plan_compaction` 对
//!       「[摘要, 保留区≥keep_recent]」形状返回 Some(cut=1)（kernel 本身
//!       会重压同文摘要），护栏只在 service 层；service 级复证「旧区仅
//!       摘要 → NotTriggered」「硬截断产物再进 → NotTriggered（不互替
//!       循环）」「旧区有内容 → HardTruncated 且 0 摘要调用」。
//!   C — placeholder 恢复 × fileOps enrichment 交互：恢复臂之后最终
//!       摘要仍携带 <read-files>/<modified-files> 段（两段后处理的
//!       组合顺序无任何既有测试覆盖）。
//!   D — 族判定配置注入与槽状态（R3-F-01 行为级复证）：
//!       D1 openai 族注入 outputCapRequired=true → 线上出现 max_tokens
//!          （对照 None → 无键；Some(false) → 不豁免族清单，仍无键）；
//!       D2 summarize 路由窗口未声明 → mid-run 触发后 HardTruncated、
//!          摘要模型 0 次调用、marker 摘要替换旧区（现役同配置下用
//!          session.model 窗口可走正常摘要——分叉实证）；
//!       D3 summarize 槽整体缺失 → Failed、0 调用、原交换逐字不动
//!          （现役压缩根本不经 summarize 槽——分叉实证）。
//!   E — compact_now 手动压缩的 fileOps 跨轮续传：第一轮产物（摘要含
//!       段）+ 新文件操作组成第二交换，第二轮段 = 播种 ∪ 新增。
//!   F — RC-3 防再发鉴别力：重实现交叉锁逻辑，实证「删 §十 中间条目
//!       而代码引用残留」组合不被现有断言捕获（越界检查只查 >max）；
//!       族矩阵独立重算：对 output_cap_required 跑 族×声明×provider/
//!       endpoint 组合并与现役 OUTPUT_CAP_CAPABILITIES 五档逐档对照。

use lingxi_kernel::compaction;
use lingxi_kernel::model_exchange::{
    ExchangeItem, ProtocolFamily, RequestedToolCall, ToolDeclaration, ToolDeclarationSnapshot,
};
use lingxi_kernel::ports::{StoragePort, ToolOutcome, ToolRequest};
use lingxi_kernel::toolcatalog::SchemaBudget;
use lingxi_kernel::usage::{CallOutcome, ModelCallUsage, ModelUsageQuery};
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

// ─── 夹具（自构造） ───

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

/// 配对完整的长历史：`groups` 组 [assistant(带一个 read 调用), 结果]，
/// 每组正文约 `chunk` 字符（≈chunk/4 tokens 每侧）。seq 从 1000 起，
/// 与手工构造的 1..100 段错开（撞 id = 配对证明响亮拒绝）。
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
            ToolDeclaration {
                target: "tool:first-party:edit".to_string(),
                wire_name: "edit".to_string(),
                description: "Edit a file".to_string(),
                input_schema: schema(),
            },
        ],
    }
}

fn test_ctx() -> RunContext {
    RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("sess-r3"),
        run_id: lingxi_protocol::RunId::new("run-r3"),
        attempt: lingxi_protocol::AttemptId::new("attempt-r3"),
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
        json!({"id":"chatcmpl-r3","model":"stub-model","choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]}),
        json!({"id":"chatcmpl-r3","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
        json!({"id":"chatcmpl-r3","choices":[],"usage":{"prompt_tokens":17,"completion_tokens":5}}),
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
        json!({"id":"chatcmpl-r3","model":"stub-model","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":tool_calls},"finish_reason":null}]}),
        json!({"id":"chatcmpl-r3","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
        json!({"id":"chatcmpl-r3","choices":[],"usage":{"prompt_tokens":21,"completion_tokens":7}}),
    ] {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    StubResponse { body }
}

// ─── plane / service 构造 ───

/// summarize_extra: 追加进 summarize 路由绑定的 JSON 片段（compat 等）。
fn plane_custom(
    stub: &StubServer,
    chat_window: Option<u64>,
    summarize_window: Option<u64>,
    summarize_tools: bool,
    summarize_extra: &str,
    include_summarize: bool,
) -> String {
    let chat_compat = match chat_window {
        Some(window) => format!(r#", "compat": {{"contextWindow": {window}}}"#),
        None => String::new(),
    };
    let summarize_binding = if include_summarize {
        let tools = if summarize_tools {
            r#", "capabilities": {"tools": true}"#
        } else {
            ""
        };
        let compat = match summarize_window {
            Some(window) => format!(r#", "compat": {{"contextWindow": {window}{summarize_extra}}}"#),
            None if !summarize_extra.is_empty() => {
                format!(r#", "compat": {{{summarize_extra}}}"#)
            }
            None => String::new(),
        };
        format!(
            r#", "summarize": {{"provider": "stub_svc", "model": "summarize-model"{tools}{compat}}}"#
        )
    } else {
        String::new()
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
                "chat": {{"provider": "stub_svc", "model": "chat-model", "capabilities": {{"tools": true}}{chat_compat}}}{summarize_binding}
            }}
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
        "lingxi-r06t02-r3-{tag}-{}-{}",
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

async fn ledger_count(state: &lingxi_service::ServiceState, purpose: &str) -> usize {
    state
        .storage()
        .query_model_call_usage(ModelUsageQuery {
            purpose: Some(purpose.to_string()),
            ..ModelUsageQuery::default()
        })
        .await
        .expect("usage query")
        .len()
}

// ─── 探针 A：fit 检查退化面（kernel） ───

fn probe_a_fit_degenerate() {
    println!("── A. fit 检查退化面");
    // window=0（summarize 路由未声明窗口 → service unwrap_or(0)）→ 超窗。
    assert!(!compaction::cache_preserving_request_fits(0, 0), "window=0");
    assert!(!compaction::cache_preserving_request_fits(1, 0), "window=0 any estimate");
    // window=1 → floor(1×0.85)=0 → 只有 0 估算才 fits。
    assert!(compaction::cache_preserving_request_fits(0, 1), "window=1 zero estimate");
    assert!(!compaction::cache_preserving_request_fits(1, 1), "window=1 nonzero estimate");
    // u64::MAX：saturating_mul 不得 panic；注意 saturating 语义——
    // window > u64::MAX/8500（≈2.17e15）时阈值恒为 u64::MAX/10000
    // （非真 0.85×window）。无生产意义（真实窗口 ≤ 数百万），记录为
    // 理论边界行为而非缺陷。
    let max_threshold = u64::MAX / 10_000;
    assert!(compaction::cache_preserving_request_fits(max_threshold, u64::MAX));
    assert!(!compaction::cache_preserving_request_fits(max_threshold + 1, u64::MAX));
    // 现实上界（1e12 窗口）：阈值 = 8.5e11，正常比例。
    assert!(compaction::cache_preserving_request_fits(850_000_000_000, 1_000_000_000_000));
    assert!(!compaction::cache_preserving_request_fits(850_000_000_001, 1_000_000_000_000));
    // 精确边界：estimated == floor(window×0.85) → fits（≤），+1 → 不 fits。
    assert!(compaction::cache_preserving_request_fits(85_000, 100_000));
    assert!(!compaction::cache_preserving_request_fits(85_001, 100_000));
    // summary_output_cap 公式：reserve=16384 → max(512, floor(0.8×16384))=13107。
    assert_eq!(compaction::summary_output_cap(16_384), 13_107);
    assert_eq!(compaction::summary_output_cap(0), 512, "reserve=0 的地板是 512");
    println!("   A OK：window=0/1/MAX 退化臂与 85% 精确边界、cap 公式地板全部符合");
}

// ─── 探针 B：无进展护栏的分层（kernel + service） ───

fn probe_b_kernel_resummarizes_a_summary() {
    println!("── B1. kernel 对 [摘要, 保留区≥keep_recent] 仍会给出切点");
    // 形状：既有摘要领头 + 5 组配对（每组约 2000+2000 tokens），保留区
    // 从尾累积 20000 恰好落在第一组 → 唯一合法切点 = 1（摘要项本身）。
    let mut exchange = vec![ExchangeItem::CompactionSummary {
        summary: compaction::HARD_TRUNCATE_MARKER_TEXT.to_string(),
        covered_items: 9,
        mid_run: true,
    }];
    exchange.extend(long_exchange(5, 8_000));
    let total = compaction::estimate_exchange_tokens(&exchange);
    assert!(total > compaction::KEEP_RECENT_TOKENS, "夹具要超 keep_recent");
    let plan = compaction::plan_compaction(&exchange, compaction::KEEP_RECENT_TOKENS)
        .expect("provable")
        .expect("a cut exists");
    assert_eq!(plan.cut_index, 1, "唯一合法切点落在既有摘要项");
    assert!(
        plan.summarized_tokens > 0,
        "summarized = 既有摘要的估算（kernel 会重压同文摘要）"
    );
    println!(
        "   B1 OK：plan=Some(cut=1, summarized={})——kernel 层无护栏，护栏纯属 service 职责",
        plan.summarized_tokens
    );
}

async fn probe_b_service_guard(service: &CompactionService, tag: &str) {
    println!("── B2. service 护栏三态（summarize 窗口=1000 强制 fit 失败）");
    let (state, home) = boot_storage(tag).await;
    let snapshot = read_snapshot();
    let usage = ModelCallUsage::reported(880, 20);

    // (i) 旧区仅摘要 → 无进展护栏 → NotTriggered（不互替循环的第一半）。
    let mut only_summary_exchange = vec![ExchangeItem::CompactionSummary {
        summary: compaction::HARD_TRUNCATE_MARKER_TEXT.to_string(),
        covered_items: 9,
        mid_run: true,
    }];
    only_summary_exchange.extend(long_exchange(5, 8_000));
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-r3");
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &MidRunCompactionInput {
                exchange: &only_summary_exchange,
                system_prompt: None,
                submission: "继续",
                tools: &snapshot,
                last_usage: Some(&usage),
                tail_from: only_summary_exchange.len(),
            },
            &cancel,
            2,
        )
        .await
        .expect("no storage failure");
    assert!(
        matches!(outcome, CompactionOutcome::NotTriggered),
        "旧区仅摘要 + fit 失败 → 护栏 NotTriggered，实际 {outcome:?}"
    );
    println!("   B2(i) OK：旧区仅摘要 → NotTriggered（同文标记不互替）");

    // (ii) 旧区有真实内容 → HardTruncated：0 摘要调用、marker 摘要替换
    // 旧区、保留区逐字、mid_run=true（自动路径）。
    let real_exchange = long_exchange(6, 8_000);
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &MidRunCompactionInput {
                exchange: &real_exchange,
                system_prompt: None,
                submission: "继续",
                tools: &snapshot,
                last_usage: Some(&usage),
                tail_from: real_exchange.len(),
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
        panic!("旧区有内容 + fit 失败 → HardTruncated，实际 {outcome:?}");
    };
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
        &real_exchange[plan.cut_index..],
        "保留区逐字"
    );
    // (iii) 硬截断产物再进一次（同窗口）→ 护栏 → NotTriggered。
    let outcome = service
        .maybe_compact_mid_run(
            &test_ctx(),
            state.storage().as_ref(),
            "agent:local",
            "user",
            None,
            None,
            &MidRunCompactionInput {
                exchange: &truncated,
                system_prompt: None,
                submission: "继续",
                tools: &snapshot,
                last_usage: Some(&usage),
                tail_from: truncated.len(),
            },
            &cancel,
            2,
        )
        .await
        .expect("no storage failure");
    assert!(
        matches!(outcome, CompactionOutcome::NotTriggered),
        "硬截断产物再进 → 护栏 NotTriggered，实际 {outcome:?}"
    );
    println!("   B2(ii)(iii) OK：HardTruncated → 再进 NotTriggered（闭环不循环）");
    teardown(&state, &home).await;
}

// ─── 探针 C：placeholder 恢复 × fileOps enrichment ───

async fn probe_c_placeholder_then_enrichment() {
    println!("── C. placeholder 恢复后 enrichment 仍追加");
    let stub = StubServer::start(vec![
        sse_tool_calls(&[("call_r3_recover", "read", "{\"path\":\"/f/late\"}")]),
        sse_final(&golden_summary()),
    ])
    .await;
    let runtime = fresh_dir("runtime-c");
    // summarize 声明 tools:true + 大窗口（fit 通过）。
    let plane = plane_custom(&stub, Some(1_000), Some(1_000_000), true, "", true);
    let service = build_service(&plane, &runtime);
    let (state, home) = boot_storage("c").await;

    // 旧区含 read /f/a、write /f/b、read /f/b（read−modified 语义）
    // 各配结果，再加若干配对组撑过 keep_recent。
    let mut exchange = Vec::new();
    exchange.push(assistant_turn(
        1,
        &"x".repeat(8_000),
        vec![
            requested_call(1, "tool:first-party:read", "/f/a"),
            requested_call(2, "tool:first-party:write", "/f/b"),
        ],
    ));
    exchange.push(tool_result(1, &"r".repeat(8_000)));
    exchange.push(tool_result(2, &"r".repeat(8_000)));
    exchange.push(assistant_turn(
        3,
        &"y".repeat(8_000),
        vec![requested_call(3, "tool:first-party:read", "/f/b")],
    ));
    exchange.push(tool_result(3, &"r".repeat(8_000)));
    exchange.extend(long_exchange(4, 8_000));

    let snapshot = read_snapshot();
    let usage = ModelCallUsage::reported(880, 20);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-r3");
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
        exchange: compacted, ..
    } = outcome
    else {
        panic!("placeholder 恢复后应 Compacted，实际 {outcome:?}");
    };
    assert_eq!(stub.hits(), 2, "意图 turn + 恰好一次恢复调用");

    // 最终摘要（恢复臂应答经清洗校验后）仍携带 fileOps 段——enrichment
    // 追加发生在恢复之后，两段后处理不互斥。
    let ExchangeItem::CompactionSummary { summary, .. } = &compacted[0] else {
        panic!("首项是摘要");
    };
    assert!(
        summary.contains("<read-files>"),
        "恢复后的摘要仍带 read-files 段: {summary}"
    );
    assert!(
        summary.contains("<modified-files>"),
        "恢复后的摘要仍带 modified-files 段: {summary}"
    );
    assert!(
        summary.contains("/f/b"),
        "被写过的路径在段内: {summary}"
    );
    // modified={/f/b}（write），readOnly 含 /f/a 与 /f/probe；/f/b 被写过
    // 不得留在 read-only。恢复臂里模型「意图读」的 /f/late 是 placeholder
    // 应答、非真实文件操作——不得混入清单（旧区不含它）。
    let modified_pos = summary.find("<modified-files>").expect("modified section");
    let modified_section = &summary[modified_pos..];
    assert!(
        !modified_section.contains("/f/a"),
        "/f/a 只读，不得进 modified: {modified_section}"
    );
    assert!(
        !summary.contains("/f/late"),
        "placeholder 意图的路径不是真实文件操作: {summary}"
    );

    // 恢复请求的线体：placeholder 结果逐字、配对的 tool 消息紧随意图
    // turn；第一发请求不含 placeholder。
    let bodies = stub.bodies();
    assert!(!bodies[0].contains(compaction::PLACEHOLDER_TOOL_RESULT_TEXT));
    assert!(bodies[1].contains(compaction::PLACEHOLDER_TOOL_RESULT_TEXT));

    // 台账两行 Succeeded。
    let rows = state
        .storage()
        .query_model_call_usage(ModelUsageQuery {
            purpose: Some("auxiliary.summarize".to_string()),
            ..ModelUsageQuery::default()
        })
        .await
        .expect("usage query");
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.outcome == CallOutcome::Succeeded));
    println!("   C OK：恢复×enrichment 组合成立，placeholder 意图不污染清单，台账 2 行");
    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

// ─── 探针 D：族判定配置注入与槽状态 ───

async fn probe_d1_output_cap_injection() {
    println!("── D1. outputCapRequired 配置注入三态");
    // (i) 注入 true：openai-completions（optional 族）→ 线上必须出现
    //     max_tokens = min(maxTokens, contextWindow) 或回退公式。
    let stub = StubServer::start(vec![sse_final(&golden_summary())]).await;
    let runtime = fresh_dir("runtime-d1t");
    let plane = plane_custom(
        &stub,
        Some(1_000),
        Some(1_000_000),
        true,
        r#", "outputCapRequired": true, "maxTokens": 4096"#,
        true,
    );
    let service = build_service(&plane, &runtime);
    let (state, home) = boot_storage("d1t").await;
    let exchange = long_exchange(6, 8_000);
    let snapshot = read_snapshot();
    let usage = ModelCallUsage::reported(880, 20);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-r3");
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
        "注入 true 后照常压缩: {outcome:?}"
    );
    let body: serde_json::Value =
        serde_json::from_str(&stub.bodies()[0]).expect("request json");
    assert_eq!(
        body.get("max_tokens").and_then(|v| v.as_u64()),
        Some(4096),
        "required 注入 → max_tokens=min(4096, 1000000)=4096 上线: {body}"
    );
    println!("   D1(i) OK：outputCapRequired=true → max_tokens=4096 上线");
    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);

    // (ii) 注入 false：不豁免族清单——openai 族仍 optional → 线上无键。
    let stub = StubServer::start(vec![sse_final(&golden_summary())]).await;
    let runtime = fresh_dir("runtime-d1f");
    let plane = plane_custom(
        &stub,
        Some(1_000),
        Some(1_000_000),
        true,
        r#", "outputCapRequired": false"#,
        true,
    );
    let service = build_service(&plane, &runtime);
    let (state, home) = boot_storage("d1f").await;
    let exchange = long_exchange(6, 8_000);
    let snapshot = read_snapshot();
    let usage = ModelCallUsage::reported(880, 20);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-r3");
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
    assert!(matches!(outcome, CompactionOutcome::Compacted { .. }));
    let body: serde_json::Value =
        serde_json::from_str(&stub.bodies()[0]).expect("request json");
    assert!(
        body.get("max_tokens").is_none()
            && body.get("max_output_tokens").is_none()
            && body.get("max_completion_tokens").is_none(),
        "Some(false) 不豁免族清单：openai 仍 optional、线上无 cap 键: {body}"
    );
    println!("   D1(ii) OK：outputCapRequired=false → 族清单兜底，线上仍无 cap 键");
    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

async fn probe_d2_undeclared_summarize_window() {
    println!("── D2. summarize 路由窗口未声明 → 永远硬截断（R3-F-01 复证一）");
    let stub = StubServer::start(vec![]).await; // 不应有任何调用
    let runtime = fresh_dir("runtime-d2");
    // chat 窗口 1000（触发门过线）；summarize 路由存在但无 compat。
    let plane = plane_custom(&stub, Some(1_000), None, true, "", true);
    let service = build_service(&plane, &runtime);
    let (state, home) = boot_storage("d2").await;
    let exchange = long_exchange(6, 8_000);
    let snapshot = read_snapshot();
    let usage = ModelCallUsage::reported(880, 20);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-r3");
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
    let CompactionOutcome::HardTruncated { .. } = outcome else {
        panic!("summarize 窗口未声明 → HardTruncated，实际 {outcome:?}");
    };
    assert_eq!(stub.hits(), 0, "硬截断不调摘要模型");
    assert_eq!(
        ledger_count(&state, "auxiliary.summarize").await,
        0,
        "硬截断不落摘要台账行"
    );
    println!("   D2 OK：窗口未声明 → HardTruncated、0 调用、0 台账（现役同配置走正常摘要——分叉）");
    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

async fn probe_d3_missing_summarize_slot() {
    println!("── D3. summarize 槽缺失 → Failed（R3-F-01 复证二）");
    let stub = StubServer::start(vec![]).await;
    let runtime = fresh_dir("runtime-d3");
    let plane = plane_custom(&stub, Some(1_000), None, false, "", false);
    let service = build_service(&plane, &runtime);
    let (state, home) = boot_storage("d3").await;
    let exchange = long_exchange(6, 8_000);
    let original = exchange.clone();
    let snapshot = read_snapshot();
    let usage = ModelCallUsage::reported(880, 20);
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-r3");
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
        panic!("summarize 槽缺失 → Failed，实际 {outcome:?}");
    };
    assert!(
        detail.contains("summarize"),
        "失败详情点名 summarize 槽: {detail}"
    );
    assert_eq!(stub.hits(), 0, "路由失败 0 物理调用");
    assert_eq!(exchange, original, "原交换逐字不动（eq 语义）");
    assert_eq!(ledger_count(&state, "auxiliary.summarize").await, 0);
    println!("   D3 OK：槽缺失 → Failed({detail:.60}…)、0 调用、原交换不动");
    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

// ─── 探针 E：compact_now 手动压缩的 fileOps 跨轮续传 ───

async fn probe_e_manual_compaction_chains_file_ops() {
    println!("── E. compact_now 两轮 fileOps 续传");
    let stub = StubServer::start(vec![
        sse_final(&golden_summary()),
        sse_final(&golden_summary()),
    ])
    .await;
    let runtime = fresh_dir("runtime-e");
    let plane = plane_custom(&stub, None, Some(1_000_000), true, "", true);
    let service = build_service(&plane, &runtime);
    let (state, home) = boot_storage("e").await;
    let snapshot = read_snapshot();
    let cancel = lingxi_service::cancel::CancelScope::run_root("run-r3");

    // 第一轮：read /e/one + write /e/two + 配对组。
    let mut round1 = Vec::new();
    round1.push(assistant_turn(
        1,
        &"p".repeat(8_000),
        vec![
            requested_call(1, "tool:first-party:read", "/e/one"),
            requested_call(2, "tool:first-party:write", "/e/two"),
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
            &MidRunCompactionInput {
                exchange: &round1,
                system_prompt: None,
                submission: "手动压缩",
                tools: &snapshot,
                last_usage: None,
                tail_from: round1.len(),
            },
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
    assert!(summary1.contains("/e/one") && summary1.contains("/e/two"));

    // 第二轮：第一轮产物 + 新 read /e/three（配对组撑长度），手动再压。
    let mut round2 = compacted1.clone();
    let base = 100_u32;
    round2.push(assistant_turn(
        base + 1,
        &"q".repeat(8_000),
        vec![requested_call(base + 1, "tool:first-party:read", "/e/three")],
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
            &MidRunCompactionInput {
                exchange: &round2,
                system_prompt: None,
                submission: "再压",
                tools: &snapshot,
                last_usage: None,
                tail_from: round2.len(),
            },
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
    // 续传：第一轮播种的 /e/one、/e/two 必须在第二轮段内（旧区最后一个
    // 摘要的段被解析回集合）；新操作 /e/three 也在。
    assert!(
        summary2.contains("/e/one") && summary2.contains("/e/two"),
        "第一轮清单播种续传: {summary2}"
    );
    assert!(summary2.contains("/e/three"), "本轮新操作并入: {summary2}");
    let modified_pos = summary2.find("<modified-files>").expect("modified section");
    assert!(
        summary2[modified_pos..].contains("/e/two"),
        "write 过的路径留在 modified: {summary2}"
    );
    println!("   E OK：两轮手动压缩 fileOps 续传成立（播种 ∪ 新增）");
    stub.stop().await;
    teardown(&state, &home).await;
    let _ = std::fs::remove_dir_all(&runtime);
}

// ─── 探针 F：RC-3 防再发鉴别力 ───

fn probe_f_cross_lock_discrimination() {
    println!("── F1. 交叉锁鉴别力：删中间条目+引用残留 是否被捕获");
    // 重实现交叉锁逻辑（与 service 测试 report_deviation_register_matches_test_references
    // 逐行同构），然后对「被篡改的报告」跑同一逻辑。
    let detect = |entries: &[u32], referenced: &[u32]| -> (Vec<u32>, Vec<u32>) {
        let missing: Vec<u32> = entries
            .iter()
            .copied()
            .filter(|n| !referenced.contains(n))
            .collect();
        let max = entries.iter().max().copied().unwrap_or(0);
        let stray: Vec<u32> = referenced.iter().copied().filter(|n| *n > max).collect();
        (missing, stray)
    };
    // 现状：1..=16 条目，引用全覆盖 → 通过。
    let entries: Vec<u32> = (1..=16).collect();
    let referenced: Vec<u32> = (1..=16).collect();
    let (missing, stray) = detect(&entries, &referenced);
    assert!(missing.is_empty() && stray.is_empty(), "现状通过");
    // 篡改一：删条目 5、引用同步删 → missing 抓不到（entries 无 5）、
    // stray 抓不到（5 ≤ 16）→ 通过！编号前移后 16 消失才会被 missing 抓。
    let tampered_entries: Vec<u32> = (1..=15).collect(); // 删 16 号
    let tampered_refs: Vec<u32> = (1..=15).collect();
    let (missing, stray) = detect(&tampered_entries, &tampered_refs);
    assert!(
        missing.is_empty() && stray.is_empty(),
        "删尾条目+同步删引用：检测不到（条目集合与引用集合同步收缩）"
    );
    // 篡改二：删中间条目 5 但代码引用 5 残留 → missing 无 5（entries 不含），
    // stray 无 5（5≤max=16）→ 通过！这就是鉴别力缺口。
    let tampered_entries: Vec<u32> = (1..=16).filter(|n| *n != 5).collect();
    let tampered_refs: Vec<u32> = (1..=16).collect(); // 引用 5 残留
    let (missing, stray) = detect(&tampered_entries, &tampered_refs);
    assert!(
        missing.is_empty() && stray.is_empty(),
        "删中间条目+引用残留：现有断言捕获不到（stray 只查 >max）"
    );
    println!(
        "   F1 CONFIRMED GAP：『删中间条目、代码引用残留』逃逸检测——交叉锁只有 \
         『条目→至少一次引用』与『引用 ≤ max』两方向，无『引用 ∈ 条目集合』方向"
    );
}

fn probe_f_family_matrix_independent() {
    println!("── F2. 族矩阵独立重算 × 现役 OUTPUT_CAP_CAPABILITIES 五档对照");
    use ProtocolFamily as PF;
    let openai = ["openai-completions", "openai-responses", "codex"];
    // 档 1：declared true 压过一切（含 deepseek endpoint）。
    assert!(compaction::output_cap_required(
        PF::OpenAiCompletions,
        "deepseek",
        "https://api.deepseek.com/v1",
        Some(true)
    ));
    // 档 2：deepseek provider/endpoint → optional（即使 anthropic-messages 族？——
    // 注意顺序：deepseek 判定在族判定之前！anthropic 族 + deepseek endpoint
    // → optional。现役同序：deepseek 行在 anthropic 行之前。）
    assert!(!compaction::output_cap_required(
        PF::AnthropicMessages,
        "deepseek",
        "https://api.deepseek.com/v1",
        None
    ));
    // 档 3：anthropic provider/endpoint → required。
    assert!(compaction::output_cap_required(
        PF::OpenAiCompletions,
        "anthropic",
        "https://api.anthropic.com",
        None
    ));
    // 档 4：bedrock → required（两种 provider 名）。
    assert!(compaction::output_cap_required(
        PF::OpenAiCompletions,
        "amazon-bedrock",
        "https://bedrock.us-east-1.amazonaws.com",
        None
    ));
    assert!(compaction::output_cap_required(
        PF::OpenAiCompletions,
        "bedrock",
        "https://example.com",
        None
    ));
    // 档 5：anthropic-messages 族 → required（通用 endpoint）。
    assert!(compaction::output_cap_required(
        PF::AnthropicMessages,
        "third-party",
        "https://proxy.example.com",
        None
    ));
    // 档 6：默认 optional（openai×3/google 族、任意 endpoint）。
    for family in [
        PF::OpenAiCompletions,
        PF::OpenAiResponses,
        PF::GoogleGenerativeAi,
    ] {
        assert!(
            !compaction::output_cap_required(family, "openai", "https://api.openai.com", None),
            "{family:?} 默认 optional"
        );
    }
    for _provider in openai {
        // openai provider 名不触发 required。
        assert!(!compaction::output_cap_required(
            PF::OpenAiCompletions,
            "openai",
            "https://api.openai.com",
            Some(false)
        ));
    }
    // 大小写敏感对照：现役 output-budget.ts 用 lower() 归一（不区分大小写），
    // Rust 是精确匹配——「Anthropic」provider 名在 Rust 落默认 optional，
    // 在现役落 required。方向：假 optional（线上无 cap）→ 对真实
    // required 端点会被服务端拒绝吗？——Anthropic 官方端点是
    // api.anthropic.com，endpoint 判定救回；provider 字段大写变体只在
    // 自建网关冒名时出现。记录为 informational 笔记。
    assert!(
        !compaction::output_cap_required(
            PF::OpenAiCompletions,
            "Anthropic",
            "https://selfhosted.example.com",
            None
        ),
        "大小写变体在 Rust 落 optional（现役 lower() 落 required）——informational"
    );
    println!("   F2 OK：六档逐档一致；大小写敏感度差异记录为 informational");
}

#[tokio::main]
async fn main() {
    println!("REVIEWER-R06-T02-R3 对抗探针");
    probe_a_fit_degenerate();
    probe_b_kernel_resummarizes_a_summary();

    // B2 用 summarize 窗口=1000 的 service（fit 必失败）。
    {
        let stub = StubServer::start(vec![]).await;
        let runtime = fresh_dir("runtime-b");
        let plane = plane_custom(&stub, Some(1_000), Some(1_000), true, "", true);
        let service = build_service(&plane, &runtime);
        probe_b_service_guard(&service, "b").await;
        assert_eq!(stub.hits(), 0, "护栏/硬截断全程 0 摘要调用");
        stub.stop().await;
        let _ = std::fs::remove_dir_all(&runtime);
    }

    probe_c_placeholder_then_enrichment().await;
    probe_d1_output_cap_injection().await;
    probe_d2_undeclared_summarize_window().await;
    probe_d3_missing_summarize_slot().await;
    probe_e_manual_compaction_chains_file_ops().await;
    probe_f_cross_lock_discrimination();
    probe_f_family_matrix_independent();
    println!("ALL PROBES DONE");
}
