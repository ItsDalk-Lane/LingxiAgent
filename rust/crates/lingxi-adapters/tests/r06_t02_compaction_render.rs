//! R06-T02 适配器渲染测试：压缩摘要/通知/指令在四个协议族（codex 经共享的
//! responses 渲染函数继承）的线上形状。
//!
//! 锚定的现役语义：
//! - 摘要 = 普通 user 角色历史（pi `messages.js` convertToLlm 的
//!   前缀/后缀包装，逐字）；mid-run 时再补一条 MIDRUN_COMPACTION_NOTICE
//!   user 消息；摘要绝不进入任何族的 system 槽。
//! - 指令 = 请求作用域 user 消息，渲染在 prior 末尾（现役缓存保留形状
//!   [system, submission, ...exchange, instruction] 的最后一项）。
//! - R06-A03 适配器侧：压缩后的交换渲染出的线体里，每个工具结果都能与
//!   前方的工具调用配对（无孤立 tool 消息，两个方向）。

use lingxi_adapters::models::{
    anthropic_messages, google_generative_ai, openai_codex_responses, openai_completions,
    openai_responses,
};
use lingxi_kernel::compaction::{
    render_summary_message_text, COMPACTION_SUMMARY_PREFIX, COMPACTION_SUMMARY_SUFFIX,
    MIDRUN_COMPACTION_NOTICE,
};
use lingxi_kernel::model_exchange::{
    CredentialAuthKind, CredentialReference, ExchangeItem, ModelOperation, ModelTurnInput,
    ProtocolFamily, RequestedToolCall, ResolvedModelRoute, ToolDeclaration,
    ToolDeclarationSnapshot,
};
use lingxi_kernel::ports::{ToolOutcome, ToolRequest};
use lingxi_kernel::toolcatalog::SchemaBudget;
use lingxi_protocol::{ContentBlock, ModelCallId, ToolCallId};
use serde_json::json;

// ─── 夹具 ───

fn route(protocol: ProtocolFamily) -> ResolvedModelRoute {
    ResolvedModelRoute {
        provider: "stub".to_string(),
        model: "stub-model".to_string(),
        operation: ModelOperation::Chat,
        protocol,
        endpoint: "https://stub.example.test/v1".to_string(),
        credential: CredentialReference {
            provider: "stub".to_string(),
            auth: CredentialAuthKind::ApiKey,
        },
        config_generation: 1,
        group_id: None,
    }
}

fn snapshot() -> ToolDeclarationSnapshot {
    ToolDeclarationSnapshot {
        catalog_generation: 7,
        declarations: vec![ToolDeclaration {
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

const GOLDEN_SUMMARY: &str = "## Goal\n完成用户请求\n\n## Constraints & Preferences\n- (none)\n\n\
     ## Progress\n### Done\n- [x] 读取文件\n\n### In Progress\n- [ ] 汇总\n\n\
     ### Blocked\n- 无\n\n## Key Decisions\n- **用真实工具**: 避免猜测\n\n\
     ## Next Steps\n1. 写出结论\n\n## Critical Context\n- (none)\n";

const INSTRUCTION_TEXT: &str = "Internal compaction-only run.\nDo not call tools.";

fn requested_call(seq: u32) -> RequestedToolCall {
    let request = ToolRequest::from_effective_arguments(
        "tool:first-party:read",
        json!({"path": format!("/f/{seq}")}),
        &SchemaBudget::default(),
    )
    .expect("arguments")
    .with_provider_call_id(format!("pc{seq:04}"));
    RequestedToolCall {
        tool_call_id: ToolCallId::new(format!("tc{seq:04}")),
        provider_call_id: request.provider_call_id,
        target: request.target,
        arguments: request.arguments,
        args_digest: request.args_digest,
        args_summary: None,
    }
}

fn assistant_turn(seq: u32, text: &str) -> ExchangeItem {
    ExchangeItem::AssistantTurn {
        call: ModelCallId::new(format!("mc{seq:04}")),
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        tool_calls: vec![requested_call(seq)],
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

/// 压缩请求的交换形状：保留区（一组完整调用/结果）+ 摘要 + 末尾指令。
fn compaction_turn_input(mid_run: bool) -> ModelTurnInput {
    let mut input = ModelTurnInput::first_turn("把读到的内容汇总成摘要", snapshot());
    input.system_prompt = Some("SYS-PROMPT-ANCHOR".to_string());
    input.prior = vec![
        assistant_turn(11, "我先读第一个文件"),
        tool_result(11, "file bytes one"),
        ExchangeItem::CompactionSummary {
            summary: GOLDEN_SUMMARY.to_string(),
            covered_items: 20,
            mid_run,
        },
        ExchangeItem::CompactionInstruction {
            text: INSTRUCTION_TEXT.to_string(),
        },
    ];
    input
}

/// 压缩后的 live 交换形状（A03 适配器侧）：[摘要, 保留组(调用+结果)]。
fn compacted_live_input() -> ModelTurnInput {
    let mut input = ModelTurnInput::first_turn("继续任务", snapshot());
    input.system_prompt = Some("SYS-PROMPT-ANCHOR".to_string());
    input.prior = vec![
        ExchangeItem::CompactionSummary {
            summary: GOLDEN_SUMMARY.to_string(),
            covered_items: 20,
            mid_run: true,
        },
        assistant_turn(21, "继续读第二个文件"),
        tool_result(21, "file bytes two"),
    ];
    input
}

// ─── openai-completions ───

#[test]
fn openai_summary_notice_and_instruction_are_user_messages() {
    let input = compaction_turn_input(true);
    let body = openai_completions::render_chat_request(
        &input,
        &route(ProtocolFamily::OpenAiCompletions),
        false,
    )
    .expect("renders");
    let messages = body["messages"].as_array().expect("messages");
    // system 槽仍是会话系统提示——摘要绝不抢占系统指令优先级。
    assert_eq!(messages[0]["role"], "system");
    assert_eq!(messages[0]["content"], "SYS-PROMPT-ANCHOR");
    // submission 在 prior 之前（冻结顺序）。
    assert_eq!(messages[1]["role"], "user");
    assert_eq!(messages[1]["content"], "把读到的内容汇总成摘要");
    // 摘要 = user 消息，内容 = 现役前缀+正文+后缀逐字包装。
    let summary_pos = messages
        .iter()
        .position(|m| {
            m["role"] == "user"
                && m["content"]
                    .as_str()
                    .is_some_and(|c| c.contains("was compacted into the following summary"))
        })
        .expect("summary user message");
    assert_eq!(
        messages[summary_pos]["content"],
        render_summary_message_text(GOLDEN_SUMMARY)
    );
    assert_eq!(
        messages[summary_pos]["content"].as_str().expect("text"),
        format!("{COMPACTION_SUMMARY_PREFIX}{GOLDEN_SUMMARY}{COMPACTION_SUMMARY_SUFFIX}")
    );
    // mid-run：紧随一条 notice user 消息（现役文本逐字）。
    assert_eq!(messages[summary_pos + 1]["role"], "user");
    assert_eq!(
        messages[summary_pos + 1]["content"],
        MIDRUN_COMPACTION_NOTICE
    );
    // 指令 = 末尾 user 消息（缓存保留形状的最后一项）。
    let last = messages.last().expect("last");
    assert_eq!(last["role"], "user");
    assert_eq!(last["content"], INSTRUCTION_TEXT);
}

#[test]
fn openai_no_notice_when_not_mid_run() {
    let input = compaction_turn_input(false);
    let body = openai_completions::render_chat_request(
        &input,
        &route(ProtocolFamily::OpenAiCompletions),
        false,
    )
    .expect("renders");
    let messages = body["messages"].as_array().expect("messages");
    assert!(
        messages
            .iter()
            .all(|m| m["content"].as_str() != Some(MIDRUN_COMPACTION_NOTICE)),
        "no notice without mid_run"
    );
}

#[test]
fn openai_compacted_exchange_has_no_orphan_tool_messages() {
    // R06-A03 适配器侧：压缩后交换渲染出的线体，每个 role=tool 消息都能与
    // 前方 assistant 消息的 tool_calls 配对（无孤立 tool result 上线）。
    let input = compacted_live_input();
    let body = openai_completions::render_chat_request(
        &input,
        &route(ProtocolFamily::OpenAiCompletions),
        false,
    )
    .expect("renders");
    let messages = body["messages"].as_array().expect("messages");
    let mut known_call_ids: Vec<String> = Vec::new();
    for message in messages {
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
                    "tool message {id} pairs with an earlier assistant tool call"
                );
            }
            _ => {}
        }
    }
    assert!(
        known_call_ids.contains(&"pc0021".to_string()),
        "the retained group's call rode the wire"
    );
}

// ─── anthropic ───

#[test]
fn anthropic_summary_notice_and_instruction_are_user_turns() {
    let input = compaction_turn_input(true);
    let body = anthropic_messages::render_messages_request(
        &input,
        &route(ProtocolFamily::AnthropicMessages),
        1_024,
        false,
    )
    .expect("renders");
    // system 槽仍是会话系统提示。
    assert_eq!(body["system"], "SYS-PROMPT-ANCHOR");
    let messages = body["messages"].as_array().expect("messages");
    let summary_pos = messages
        .iter()
        .position(|m| {
            m["role"] == "user"
                && m["content"]
                    .as_str()
                    .is_some_and(|c| c.contains("was compacted into the following summary"))
        })
        .expect("summary user turn");
    assert_eq!(
        messages[summary_pos]["content"],
        render_summary_message_text(GOLDEN_SUMMARY)
    );
    assert_eq!(
        messages[summary_pos + 1]["content"],
        MIDRUN_COMPACTION_NOTICE
    );
    assert_eq!(messages[summary_pos + 1]["role"], "user");
    let last = messages.last().expect("last");
    assert_eq!(last["role"], "user");
    assert_eq!(last["content"], INSTRUCTION_TEXT);
}

#[test]
fn anthropic_compacted_exchange_pairs_every_tool_result() {
    // A03：每个 tool_result 块都能与前方 assistant 的 tool_use 配对。
    let input = compacted_live_input();
    let body = anthropic_messages::render_messages_request(
        &input,
        &route(ProtocolFamily::AnthropicMessages),
        1_024,
        false,
    )
    .expect("renders");
    let messages = body["messages"].as_array().expect("messages");
    let mut known_use_ids: Vec<String> = Vec::new();
    for message in messages {
        if message["role"] == "assistant" {
            for block in message["content"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(&[])
            {
                if block["type"] == "tool_use" {
                    known_use_ids.push(block["id"].as_str().expect("id").to_string());
                }
            }
        }
        if message["role"] == "user" && message["content"].is_array() {
            for block in message["content"].as_array().expect("blocks") {
                if block["type"] == "tool_result" {
                    let id = block["tool_use_id"].as_str().expect("pairing id");
                    assert!(
                        known_use_ids.iter().any(|known| known == id),
                        "tool_result {id} pairs with an earlier tool_use"
                    );
                }
            }
        }
    }
    assert!(known_use_ids.contains(&"pc0021".to_string()));
}

// ─── google ───

#[test]
fn google_summary_notice_and_instruction_are_user_parts() {
    let input = compaction_turn_input(true);
    let body = google_generative_ai::render_generate_request(
        &input,
        &route(ProtocolFamily::GoogleGenerativeAi),
    )
    .expect("renders");
    assert_eq!(
        body["systemInstruction"]["parts"][0]["text"],
        "SYS-PROMPT-ANCHOR"
    );
    let contents = body["contents"].as_array().expect("contents");
    let summary_pos = contents
        .iter()
        .position(|c| {
            c["role"] == "user"
                && c["parts"][0]["text"]
                    .as_str()
                    .is_some_and(|t| t.contains("was compacted into the following summary"))
        })
        .expect("summary user content");
    assert_eq!(
        contents[summary_pos]["parts"][0]["text"],
        render_summary_message_text(GOLDEN_SUMMARY)
    );
    assert_eq!(
        contents[summary_pos + 1]["parts"][0]["text"],
        MIDRUN_COMPACTION_NOTICE
    );
    let last = contents.last().expect("last");
    assert_eq!(last["role"], "user");
    assert_eq!(last["parts"][0]["text"], INSTRUCTION_TEXT);
}

#[test]
fn google_compacted_exchange_pairs_every_function_response() {
    // A03：每个 functionResponse 都能与前方 model 消息的 functionCall 同名配对。
    let input = compacted_live_input();
    let body = google_generative_ai::render_generate_request(
        &input,
        &route(ProtocolFamily::GoogleGenerativeAi),
    )
    .expect("renders");
    let contents = body["contents"].as_array().expect("contents");
    let mut known_call_names: Vec<String> = Vec::new();
    for content in contents {
        for part in content["parts"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            if let Some(call) = part.get("functionCall") {
                known_call_names.push(call["name"].as_str().expect("name").to_string());
            }
            if let Some(response) = part.get("functionResponse") {
                let name = response["name"].as_str().expect("pairing name");
                assert!(
                    known_call_names.iter().any(|known| known == name),
                    "functionResponse {name} pairs with an earlier functionCall"
                );
            }
        }
    }
    assert!(known_call_names.contains(&"read".to_string()));
}

// ─── openai-responses（codex 经同一渲染函数继承） ───

#[test]
fn responses_summary_notice_and_instruction_are_input_messages() {
    let input = compaction_turn_input(true);
    let body = openai_responses::render_responses_request(
        &input,
        &route(ProtocolFamily::OpenAiResponses),
        false,
    )
    .expect("renders");
    let items = body["input"].as_array().expect("input items");
    let summary_pos = items
        .iter()
        .position(|item| {
            item["role"] == "user"
                && item["content"][0]["text"]
                    .as_str()
                    .is_some_and(|t| t.contains("was compacted into the following summary"))
        })
        .expect("summary input message");
    assert_eq!(
        items[summary_pos]["content"][0]["text"],
        render_summary_message_text(GOLDEN_SUMMARY)
    );
    assert_eq!(
        items[summary_pos + 1]["content"][0]["text"],
        MIDRUN_COMPACTION_NOTICE
    );
    let last = items.last().expect("last");
    assert_eq!(last["role"], "user");
    assert_eq!(last["content"][0]["text"], INSTRUCTION_TEXT);
}

#[test]
fn responses_compacted_exchange_pairs_every_call_output() {
    // A03：每个 function_call_output 都能与前方 function_call 的 call_id 配对。
    let input = compacted_live_input();
    let body = openai_responses::render_responses_request(
        &input,
        &route(ProtocolFamily::OpenAiResponses),
        false,
    )
    .expect("renders");
    let items = body["input"].as_array().expect("input items");
    let mut known_call_ids: Vec<String> = Vec::new();
    for item in items {
        if item["type"] == "function_call" {
            known_call_ids.push(item["call_id"].as_str().expect("id").to_string());
        }
        if item["type"] == "function_call_output" {
            let id = item["call_id"].as_str().expect("pairing id");
            assert!(
                known_call_ids.iter().any(|known| known == id),
                "function_call_output {id} pairs with an earlier function_call"
            );
        }
    }
    assert!(known_call_ids.contains(&"pc0021".to_string()));
}

#[test]
fn codex_inherits_the_summary_rendering() {
    // codex 经共享的 responses 渲染函数（FAMILY=codex）继承摘要/通知/指令
    // 三臂——其协议差异（store:false / instructions 槽）不影响历史渲染。
    let input = compaction_turn_input(true);
    let body = openai_codex_responses::render_codex_request(
        &input,
        &route(ProtocolFamily::OpenAiCodexResponses),
    )
    .expect("renders");
    // instructions 槽仍是会话系统提示——摘要绝不抢占它。
    assert_eq!(body["instructions"], "SYS-PROMPT-ANCHOR");
    let items = body["input"].as_array().expect("input items");
    assert!(items.iter().any(|item| {
        item["role"] == "user"
            && item["content"][0]["text"]
                .as_str()
                .is_some_and(|t| t.contains("was compacted into the following summary"))
    }));
    assert!(items
        .iter()
        .any(|item| { item["content"][0]["text"].as_str() == Some(MIDRUN_COMPACTION_NOTICE) }));
    let last = items.last().expect("last");
    assert_eq!(last["content"][0]["text"], INSTRUCTION_TEXT);
}

/// 五族共同纪律：摘要文本绝不出现在任何 system 槽（摘要不是可信的新系统
/// 指令——任务书绝对约束）。
#[test]
fn summary_never_enters_any_system_slot() {
    for family in [
        ProtocolFamily::OpenAiCompletions,
        ProtocolFamily::OpenAiResponses,
        ProtocolFamily::OpenAiCodexResponses,
        ProtocolFamily::AnthropicMessages,
        ProtocolFamily::GoogleGenerativeAi,
    ] {
        let input = compaction_turn_input(true);
        let system_text = match family {
            ProtocolFamily::OpenAiCompletions => {
                let body = openai_completions::render_chat_request(&input, &route(family), false)
                    .expect("renders");
                body["messages"][0]["content"]
                    .as_str()
                    .expect("system")
                    .to_string()
            }
            ProtocolFamily::OpenAiResponses => {
                // 该族无独立 system 槽：system_prompt 仅进入 instructions？
                // 非 codex responses 族不渲染 instructions——system_prompt
                // 由渲染器首条消息承担与否，逐族核实；此处直接渲染并检查
                // 整条 input 里没有把摘要放进 role=system。
                let body =
                    openai_responses::render_responses_request(&input, &route(family), false)
                        .expect("renders");
                let leaked = body["input"].as_array().expect("items").iter().any(|item| {
                    item["role"] == "system"
                        && item
                            .to_string()
                            .contains("was compacted into the following summary")
                });
                assert!(!leaked, "summary must never ride a system role: {family:?}");
                continue;
            }
            ProtocolFamily::OpenAiCodexResponses => {
                let body = openai_codex_responses::render_codex_request(&input, &route(family))
                    .expect("renders");
                body["instructions"]
                    .as_str()
                    .expect("instructions")
                    .to_string()
            }
            ProtocolFamily::AnthropicMessages => {
                let body = anthropic_messages::render_messages_request(
                    &input,
                    &route(family),
                    1_024,
                    false,
                )
                .expect("renders");
                body["system"].as_str().expect("system").to_string()
            }
            ProtocolFamily::GoogleGenerativeAi => {
                let body = google_generative_ai::render_generate_request(&input, &route(family))
                    .expect("renders");
                body["systemInstruction"]["parts"][0]["text"]
                    .as_str()
                    .expect("systemInstruction")
                    .to_string()
            }
            other => panic!("not a chat family under test: {other:?}"),
        };
        assert_eq!(system_text, "SYS-PROMPT-ANCHOR", "{family:?}");
        assert!(
            !system_text.contains("was compacted"),
            "summary must never ride the system slot: {family:?}"
        );
    }
}

// ─── 防再发（RC-3 §五-1/5）：摘要请求输出上限键集合的五族现役黄金形状 ───
// 现役 normalizeCompactionProviderPayload（PROVIDER_DEFAULT 生产链，
// session-compactor.ts:335-352）：optional-cap 族（openai×3/google/
// deepseek）删除全部输出上限字段——线体无键；required-cap 族
// （anthropic 系）协议必需 max_tokens——键恒在，值由 service 侧按
// min(model.maxTokens, contextWindow) 回填（报告 §十#5 / FIX-01）。
// 本测试锁死「键集合」维：None → optional 族无键、anthropic 键恒在
// （渲染层族缺省）；Some(v) → 各族自己的键携带 v。

#[test]
fn output_cap_key_set_matches_the_incumbent_family_split() {
    // optional 族 × None：线体无任何输出上限字段。
    let input = {
        let mut input = compaction_turn_input(false);
        input.max_output_tokens = None;
        input
    };
    let body = openai_completions::render_chat_request(
        &input,
        &route(ProtocolFamily::OpenAiCompletions),
        false,
    )
    .expect("renders");
    assert!(
        body.get("max_tokens").is_none()
            && body.get("max_output_tokens").is_none()
            && body.get("max_completion_tokens").is_none(),
        "openai-completions: no cap key when None: {body}"
    );
    let body = openai_responses::render_responses_request(
        &input,
        &route(ProtocolFamily::OpenAiResponses),
        false,
    )
    .expect("renders");
    assert!(
        body.get("max_output_tokens").is_none(),
        "openai-responses: no cap key when None: {body}"
    );
    let body = openai_codex_responses::render_codex_request(
        &input,
        &route(ProtocolFamily::OpenAiCodexResponses),
    )
    .expect("renders");
    assert!(
        body.get("max_output_tokens").is_none(),
        "codex: no cap key when None: {body}"
    );
    let body = google_generative_ai::render_generate_request(
        &input,
        &route(ProtocolFamily::GoogleGenerativeAi),
    )
    .expect("renders");
    assert!(
        body.pointer("/generationConfig/maxOutputTokens").is_none(),
        "google: no cap key when None: {body}"
    );

    // optional 族 × Some(4321)：各族自己的键携带该值（现役 BOUNDED 政策
    // 或调用方显式决策的形状——渲染器忠实传递）。
    let input = {
        let mut input = compaction_turn_input(false);
        input.max_output_tokens = Some(4_321);
        input
    };
    let body = openai_completions::render_chat_request(
        &input,
        &route(ProtocolFamily::OpenAiCompletions),
        false,
    )
    .expect("renders");
    assert_eq!(body["max_tokens"], 4_321);
    let body = openai_responses::render_responses_request(
        &input,
        &route(ProtocolFamily::OpenAiResponses),
        false,
    )
    .expect("renders");
    assert_eq!(body["max_output_tokens"], 4_321);
    let body = openai_codex_responses::render_codex_request(
        &input,
        &route(ProtocolFamily::OpenAiCodexResponses),
    )
    .expect("renders");
    assert_eq!(body["max_output_tokens"], 4_321);
    let body = google_generative_ai::render_generate_request(
        &input,
        &route(ProtocolFamily::GoogleGenerativeAi),
    )
    .expect("renders");
    assert_eq!(body["generationConfig"]["maxOutputTokens"], 4_321);

    // required 族（anthropic）：协议必需——键恒在且值 = 调用方给定的
    // 回填值（render_messages_request 的上限是显式参数，族缺省
    // DEFAULT_MAX_OUTPUT_TOKENS 只在 dispatch 层 None 时兜底；压缩链
    // 恒发 Some，见 service compaction.rs 的族分流）。
    let body = anthropic_messages::render_messages_request(
        &input,
        &route(ProtocolFamily::AnthropicMessages),
        4_321,
        false,
    )
    .expect("renders");
    assert_eq!(body["max_tokens"], 4_321);
}
