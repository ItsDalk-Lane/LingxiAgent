//! Independent reviewer scenarios for R05-T03 (C03/C04/C05/C06/C07/C10).
//! Every scenario runs against the REAL production wiring; the only double
//! is the loopback HTTP stub at the far end of the wire.

mod harness;

use harness::*;

use lingxi_kernel::model_exchange::{
    ExchangeItem, ModelTurnInput, ToolDeclaration, ToolDeclarationSnapshot,
};
use lingxi_kernel::ports::ToolOutcome;
use lingxi_kernel::toolcatalog::SchemaBudget;
use lingxi_protocol::{
    ContentBlock, ErrorCode, KnownEventPayload, ModelCallId, ProtocolError, ToolCallId,
    ToolSchemaDocument,
};
use lingxi_adapters::models::{
    anthropic_messages, google_generative_ai, openai_completions, openai_responses,
};
use lingxi_adapters::models::streaming::{SseDecoder, SseEvent};

// ── shared builders ──────────────────────────────────────────────────────────

fn snapshot_two_tools() -> ToolDeclarationSnapshot {
    let mk = |target: &str, wire: &str| ToolDeclaration {
        target: target.to_string(),
        wire_name: wire.to_string(),
        description: format!("The {wire} tool"),
        input_schema: ToolSchemaDocument {
            dialect: "json-schema/2020-12".to_string(),
            schema: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}}),
        },
    };
    ToolDeclarationSnapshot {
        catalog_generation: 1,
        declarations: vec![mk("tool:first-party:read", "read"), mk("tool:first-party:write", "write")],
    }
}

fn requested(
    seq: u32,
    provider_id: &str,
    target: &str,
    args: serde_json::Value,
) -> lingxi_kernel::model_exchange::RequestedToolCall {
    let budget = SchemaBudget::default();
    let request = lingxi_kernel::ports::ToolRequest::from_effective_arguments(target, args, &budget)
        .expect("effective")
        .with_provider_call_id(provider_id);
    lingxi_kernel::model_exchange::RequestedToolCall {
        tool_call_id: ToolCallId::new(format!("review-tc{seq:04}")),
        provider_call_id: request.provider_call_id.clone(),
        target: request.target.clone(),
        arguments: request.arguments.clone(),
        args_digest: request.args_digest.clone(),
        args_summary: None,
    }
}

fn route_for(family: lingxi_kernel::model_exchange::ProtocolFamily) -> lingxi_kernel::model_exchange::ResolvedModelRoute {
    lingxi_kernel::model_exchange::ResolvedModelRoute {
        provider: "review".to_string(),
        model: "review-model".to_string(),
        operation: lingxi_kernel::model_exchange::ModelOperation::Chat,
        protocol: family,
        endpoint: "https://review.invalid/v1".to_string(),
        credential: lingxi_kernel::model_exchange::CredentialReference {
            provider: "review".to_string(),
            auth: lingxi_kernel::model_exchange::CredentialAuthKind::ApiKey,
        },
        config_generation: 1,
    }
}

// ── C05: runtime nonce the stub cannot know ──────────────────────────────────

fn anthropic_tool_use(call_id: &str, name: &str, input: serde_json::Value) -> StubResponse {
    StubResponse::json(serde_json::json!({
        "id": "msg_review",
        "model": "the-stub-lies",
        "content": [{"type": "tool_use", "id": call_id, "name": name, "input": input}],
        "stop_reason": "tool_use",
        "usage": {"input_tokens": 5, "output_tokens": 3}
    }))
}

fn anthropic_final(text: &str) -> StubResponse {
    StubResponse::json(serde_json::json!({
        "id": "msg_review",
        "model": "the-stub-lies",
        "content": [{"type": "text", "text": text}],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 7, "output_tokens": 2}
    }))
}

/// One full C05 leg: stub script fixed FIRST (it never sees the nonce),
/// then the nonce is generated and written, then the run executes.
/// Returns the nonce and the tool_result content the second request carried.
async fn c05_leg(tag: &str) -> (String, String, String) {
    // 1. The stub script is FIXED before any nonce exists.
    let stub = StubServer::start(vec![
        anthropic_tool_use("toolu_review_c05", "read", serde_json::json!({"path": "nonce.txt"})),
        anthropic_final("review final answer"),
    ])
    .await;
    let plane_json = format!(
        r#""providers": {{
            "anthropic_review": {{
                "protocol": "anthropic-messages",
                "endpoint": "{}",
                "auth": {{"kind": "apiKey", "apiKey": "sk-review-synthetic"}}
            }}
        }},
        "models": {{"chat": {{"provider": "anthropic_review", "model": "claude-review"}}}}"#,
        stub.origin()
    );
    let boot = boot_with_config(tag, &plane_json).await;
    // 2. The nonce is generated at RUNTIME, after the stub script exists.
    let nonce = runtime_nonce(tag);
    std::fs::write(boot.workspace.join("nonce.txt"), &nonce).expect("seed nonce file");

    let run_id = execute(&boot.state, "sess_local_alpha", "read nonce.txt").await;
    let (status, _) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");

    let requests = stub.requests();
    assert_eq!(requests.len(), 2, "tool turn + final turn");
    let messages = requests[1].body["messages"].as_array().expect("messages");
    let blocks = messages[2]["content"].as_array().expect("tool_result blocks");
    assert_eq!(blocks[0]["type"], "tool_result");
    assert_eq!(blocks[0]["tool_use_id"], "toolu_review_c05");
    assert_eq!(blocks[0]["is_error"], false);
    let content = blocks[0]["content"].as_str().expect("text content").to_string();
    stub.stop().await;
    teardown_boot(&boot).await;
    (nonce, content, run_id)
}

#[tokio::test]
async fn c05_runtime_nonce_rides_the_next_request_and_follows_changes() {
    let (nonce_a, wire_a, _) = c05_leg("c05a").await;
    let (nonce_b, wire_b, _) = c05_leg("c05b").await;
    assert_ne!(nonce_a, nonce_b, "the two legs must use different runtime nonces");
    // The wire carries the REAL file bytes of THAT leg — a preset answer or
    // a fixed done-string fails here.
    assert_eq!(wire_a, nonce_a, "leg A: the wire carries leg A's runtime nonce");
    assert_eq!(wire_b, nonce_b, "leg B: the wire carries leg B's runtime nonce");
    // A canned "done" marker must NOT be what rides the wire.
    assert!(!wire_a.contains("done"), "no preset-done substitution");
}

// ── C03: reversed completion order pairs by call id, never by position ───────

#[test]
fn c03_reversed_completion_order_pairs_each_result_with_its_own_call_id() {
    // The exchange: assistant requested A=read(toolu_a) then B=write(toolu_b);
    // the outcomes arrived REVERSED: B failed first, then A succeeded.
    let mut input = ModelTurnInput::first_turn("do both", snapshot_two_tools());
    input.prior.push(ExchangeItem::AssistantTurn {
        call: ModelCallId::new("review-mc0001"),
        content: vec![],
        tool_calls: vec![
            requested(1, "toolu_a", "tool:first-party:read", serde_json::json!({"path": "/tmp/a"})),
            requested(2, "toolu_b", "tool:first-party:write", serde_json::json!({"path": "/tmp/b", "content": "B"})),
        ],
    });
    // REVERSED completion order in the exchange history.
    input.prior.push(ExchangeItem::ToolResult {
        tool_call_id: ToolCallId::new("review-tc0002"),
        provider_call_id: Some("toolu_b".to_string()),
        outcome: ToolOutcome::Failed {
            error: ProtocolError::new(ErrorCode::Forbidden, "B denied marker", false),
        },
    });
    input.prior.push(ExchangeItem::ToolResult {
        tool_call_id: ToolCallId::new("review-tc0001"),
        provider_call_id: Some("toolu_a".to_string()),
        outcome: ToolOutcome::success_text("A body marker"),
    });

    // anthropic-messages
    let body = anthropic_messages::render_messages_request(
        &input,
        &route_for(lingxi_kernel::model_exchange::ProtocolFamily::AnthropicMessages),
        anthropic_messages::DEFAULT_MAX_OUTPUT_TOKENS,
    )
    .expect("anthropic render");
    let results = body["messages"][2]["content"].as_array().expect("merged results");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["tool_use_id"], "toolu_b", "B's result rides B's id");
    assert_eq!(results[0]["is_error"], true, "B's FAILURE is not paired onto A");
    assert!(results[0]["content"].as_str().unwrap().contains("B denied marker"));
    assert_eq!(results[1]["tool_use_id"], "toolu_a");
    assert_eq!(results[1]["is_error"], false);
    assert_eq!(results[1]["content"], "A body marker");

    // openai-completions
    let body = openai_completions::render_chat_request(
        &input,
        &route_for(lingxi_kernel::model_exchange::ProtocolFamily::OpenAiCompletions),
        false,
    )
    .expect("openai render");
    let messages = body["messages"].as_array().expect("messages");
    let tool_msgs: Vec<&serde_json::Value> =
        messages.iter().filter(|m| m["role"] == "tool").collect();
    assert_eq!(tool_msgs.len(), 2);
    assert_eq!(tool_msgs[0]["tool_call_id"], "toolu_b");
    assert!(tool_msgs[0]["content"].as_str().unwrap().contains("B denied marker"));
    assert_eq!(tool_msgs[1]["tool_call_id"], "toolu_a");
    assert_eq!(tool_msgs[1]["content"], "A body marker");

    // openai-responses
    let body = openai_responses::render_responses_request(
        &input,
        &route_for(lingxi_kernel::model_exchange::ProtocolFamily::OpenAiResponses),
        false,
    )
    .expect("responses render");
    let items = body["input"].as_array().expect("items");
    let outputs: Vec<&serde_json::Value> =
        items.iter().filter(|i| i["type"] == "function_call_output").collect();
    assert_eq!(outputs.len(), 2);
    assert_eq!(outputs[0]["call_id"], "toolu_b");
    assert!(outputs[0]["output"].as_str().unwrap().contains("B denied marker"));
    assert_eq!(outputs[1]["call_id"], "toolu_a");
    assert_eq!(outputs[1]["output"], "A body marker");

    // google-generative-ai: pairing goes through the exchange's own
    // tool_call_id → name mapping (B must render as the WRITE functionResponse
    // with failed status, even though it completed first).
    let body = google_generative_ai::render_generate_request(
        &input,
        &route_for(lingxi_kernel::model_exchange::ProtocolFamily::GoogleGenerativeAi),
    )
    .expect("google render");
    let contents = body["contents"].as_array().expect("contents");
    let fr_b = &contents[2]["parts"][0]["functionResponse"];
    assert_eq!(fr_b["name"], "write", "B is the write call — never crossed to read");
    assert_eq!(fr_b["id"], "toolu_b");
    assert_eq!(fr_b["response"]["status"], "failed");
    assert!(fr_b["response"]["content"].as_str().unwrap().contains("B denied marker"));
    let fr_a = &contents[3]["parts"][0]["functionResponse"];
    assert_eq!(fr_a["name"], "read");
    assert_eq!(fr_a["id"], "toolu_a");
    assert_eq!(fr_a["response"]["status"], "succeeded");
    assert_eq!(fr_a["response"]["content"], "A body marker");
}

// ── C04: same provider call id across two sessions, no crossing ──────────────

#[tokio::test]
async fn c04_same_provider_call_id_in_two_sessions_stays_isolated() {
    let stub = StubServer::start(vec![
        // session 1: same external id, file one.txt
        anthropic_tool_use("toolu_SHARED", "read", serde_json::json!({"path": "one.txt"})),
        anthropic_final("session one final"),
        // session 2: the SAME external id, file two.txt
        anthropic_tool_use("toolu_SHARED", "read", serde_json::json!({"path": "two.txt"})),
        anthropic_final("session two final"),
    ])
    .await;
    let plane_json = format!(
        r#""providers": {{
            "anthropic_review": {{
                "protocol": "anthropic-messages",
                "endpoint": "{}",
                "auth": {{"kind": "apiKey", "apiKey": "sk-review-synthetic"}}
            }}
        }},
        "models": {{"chat": {{"provider": "anthropic_review", "model": "claude-review"}}}}"#,
        stub.origin()
    );
    let boot = boot_with_config("c04", &plane_json).await;
    std::fs::write(boot.workspace.join("one.txt"), "session-one-body-Ⅰ").expect("seed one");
    std::fs::write(boot.workspace.join("two.txt"), "session-two-body-Ⅱ").expect("seed two");

    let run_1 = execute(&boot.state, "sess_local_alpha", "read one.txt").await;
    let run_2 = execute(&boot.state, "sess_local_beta", "read two.txt").await;
    assert_ne!(run_1, run_2);
    assert_eq!(run_row(&boot.state, &run_1).await.0, "completed");
    assert_eq!(run_row(&boot.state, &run_2).await.0, "completed");

    let requests = stub.requests();
    assert_eq!(requests.len(), 4, "two turns per session");
    // Session 1's continuation carries session 1's REAL body under the shared id.
    let s1_blocks = requests[1].body["messages"][2]["content"].as_array().expect("s1 blocks");
    assert_eq!(s1_blocks[0]["tool_use_id"], "toolu_SHARED");
    assert_eq!(s1_blocks[0]["content"], "session-one-body-Ⅰ");
    // Session 2's continuation carries session 2's REAL body under the SAME id.
    let s2_blocks = requests[3].body["messages"][2]["content"].as_array().expect("s2 blocks");
    assert_eq!(s2_blocks[0]["tool_use_id"], "toolu_SHARED");
    assert_eq!(s2_blocks[0]["content"], "session-two-body-Ⅱ");
    // No dedup across sessions: each session's journal holds its own completed call.
    for (session, run, marker) in [
        ("sess_local_alpha", &run_1, "session-one-body-Ⅰ"),
        ("sess_local_beta", &run_2, "session-two-body-Ⅱ"),
    ] {
        let payloads = known_payloads(&boot.state, session, run).await;
        let completed: Vec<_> = payloads
            .iter()
            .filter_map(|p| match p {
                KnownEventPayload::ToolCallCompleted(p) => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(completed.len(), 1, "{session}: exactly one tool completion");
        let text = serde_json::to_string(&completed[0].result).expect("wire json");
        assert!(text.contains(marker), "{session}: its own body, never the other session's");
    }
    stub.stop().await;
    teardown_boot(&boot).await;
}

// ── C06: mixed content, buffered AND streamed ────────────────────────────────

#[test]
fn c06_mixed_content_buffered_keeps_every_block_in_order_with_tools() {
    let call = ModelCallId::new("review-mc-c06");
    let budget = SchemaBudget::default();
    let snapshot = snapshot_two_tools();
    // Buffered: text + reasoning + unknown + tool call in ONE response.
    let body = serde_json::json!({
        "id": "chatcmpl-review",
        "choices": [{
            "index": 0,
            "finish_reason": "tool_calls",
            "message": {"role": "assistant", "content": [
                {"type": "text", "text": "part one"},
                {"type": "reasoning", "text": "thinking between"},
                {"type": "future-block", "z": 9},
                {"type": "text", "text": "part two"}
            ], "tool_calls": [{
                "id": "call_mix", "type": "function",
                "function": {"name": "read", "arguments": "{\"path\":\"/tmp/m\"}"}
            }]}
        }],
        "usage": {"prompt_tokens": 3, "completion_tokens": 2}
    });
    let parsed = openai_completions::parse_chat_response(&call, &snapshot, &body, &budget)
        .expect("parses");
    let (requests, content) = match parsed.turn {
        lingxi_kernel::ports::ProviderTurn::ToolRequests { requests, content } => (requests, content),
        other => panic!("expected ToolRequests, got {other:?}"),
    };
    assert_eq!(requests.len(), 1, "the tool call survives the mixed content");
    assert_eq!(content.len(), 4, "every block survives: {content:?}");
    assert!(matches!(&content[0], ContentBlock::Text { text } if text == "part one"));
    assert!(matches!(&content[1], ContentBlock::Reasoning { text } if text == "thinking between"));
    assert!(matches!(&content[2], ContentBlock::Opaque { provider, data }
        if provider == "openai-completions" && data["type"] == "future-block"));
    assert!(matches!(&content[3], ContentBlock::Text { text } if text == "part two"));
}

#[test]
fn c06_streamed_interleaved_tool_fragments_and_text_pair_independently() {
    let call = ModelCallId::new("review-mc-c06s");
    let budget = SchemaBudget::default();
    let snapshot = snapshot_two_tools();
    // Two tool calls whose argument fragments INTERLEAVE, plus text deltas.
    let frames = [
        r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_s1","type":"function","function":{"name":"read","arguments":"{\"pa"}}]}}]}"#,
        r#"{"choices":[{"index":0,"delta":{"content":"visible text"}}]}"#,
        r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"id":"call_s2","type":"function","function":{"name":"write","arguments":"{\"path\":\"/tmp/w\""}}]}}]}"#,
        r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"/tmp/r\"}"}}]}}]}"#,
        r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"function":{"arguments":",\"content\":\"W\"}"}}]}}]}"#,
        r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
        r#"[DONE]"#,
    ];
    let wire = frames.iter().map(|f| format!("data: {f}\n\n")).collect::<String>();
    let mut decoder = SseDecoder::new();
    let mut events: Vec<SseEvent> = decoder.feed(wire.as_bytes()).expect("feed");
    events.extend(decoder.finish().expect("finish").events);
    let parsed = openai_completions::parse_chat_stream(&call, &snapshot, &events, &budget)
        .expect("parses");
    let (requests, content) = match parsed.turn {
        lingxi_kernel::ports::ProviderTurn::ToolRequests { requests, content } => (requests, content),
        other => panic!("expected ToolRequests, got {other:?}"),
    };
    assert_eq!(requests.len(), 2, "both interleaved calls assemble independently");
    assert_eq!(requests[0].provider_call_id.as_deref(), Some("call_s1"));
    assert_eq!(requests[0].target, "tool:first-party:read");
    assert_eq!(requests[0].arguments.as_value(), &serde_json::json!({"path": "/tmp/r"}));
    assert_eq!(requests[1].provider_call_id.as_deref(), Some("call_s2"));
    assert_eq!(requests[1].target, "tool:first-party:write");
    assert_eq!(requests[1].arguments.as_value(), &serde_json::json!({"path": "/tmp/w", "content": "W"}));
    assert!(
        matches!(&content[0], ContentBlock::Text { text } if text == "visible text"),
        "the text delta survives alongside the tool calls: {content:?}"
    );
}

// ── C07: a REAL failing tool rides the wire AND the journal honestly ─────────

#[tokio::test]
async fn c07_real_missing_file_failure_matches_wire_and_journal() {
    let stub = StubServer::start(vec![
        StubResponse::json(serde_json::json!({
            "id": "chatcmpl-review-c07",
            "choices": [{
                "index": 0,
                "finish_reason": "tool_calls",
                "message": {"role": "assistant", "content": null, "tool_calls": [{
                    "id": "call_c07", "type": "function",
                    "function": {"name": "read", "arguments": "{\"path\":\"missing.txt\"}"}
                }]}
            }],
            "usage": {"prompt_tokens": 4, "completion_tokens": 2}
        })),
        StubResponse::json(serde_json::json!({
            "id": "chatcmpl-review-c07b",
            "choices": [{
                "index": 0,
                "finish_reason": "stop",
                "message": {"role": "assistant", "content": "acknowledged the failure"}
            }],
            "usage": {"prompt_tokens": 6, "completion_tokens": 2}
        })),
    ])
    .await;
    let plane_json = format!(
        r#""providers": {{
            "main": {{
                "protocol": "openai-completions",
                "endpoint": "{}",
                "auth": {{"kind": "apiKey", "apiKey": "sk-review-synthetic"}}
            }}
        }},
        "models": {{"chat": {{"provider": "main", "model": "gpt-review"}}}}"#,
        stub.v1()
    );
    let boot = boot_with_config("c07", &plane_json).await;

    let run_id = execute(&boot.state, "sess_local_alpha", "read missing.txt").await;
    let (status, _) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");

    // The wire half: the failure rides the next request as an honest error.
    let requests = stub.requests();
    assert_eq!(requests.len(), 2);
    let messages = requests[1].body["messages"].as_array().expect("messages");
    let tool_msg = messages.iter().find(|m| m["role"] == "tool").expect("tool message");
    assert_eq!(tool_msg["tool_call_id"], "call_c07");
    let wire_text = tool_msg["content"].as_str().expect("tool text");
    assert!(
        wire_text.starts_with("tool error ["),
        "the failure is honestly marked, never a fake success: {wire_text}"
    );
    assert!(!wire_text.is_empty());

    // The journal half: the durable payload is a structured FAILURE, and its
    // rendered text is exactly what rode the wire (no flattening either way).
    let payloads = known_payloads(&boot.state, "sess_local_alpha", &run_id).await;
    let completed: Vec<_> = payloads
        .iter()
        .filter_map(|p| match p {
            KnownEventPayload::ToolCallCompleted(p) => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(completed.len(), 1);
    assert_eq!(
        completed[0].result.status,
        lingxi_protocol::ToolResultStatus::Failed,
        "the journal keeps the FAILED state (never success)"
    );
    let error = completed[0].result.error.as_ref().expect("structured error");
    let journal_render =
        format!("tool error [{}]: {}", error.code.wire_name(), error.message);
    assert_eq!(wire_text, journal_render, "model payload == journal render, no flattening");

    stub.stop().await;
    teardown_boot(&boot).await;
}

// ── C10: opaque position fidelity + cross-family isolation ──────────────────

#[test]
fn c10_opaque_roundtrip_positions_and_cross_family_isolation() {
    let budget = SchemaBudget::default();
    let call = ModelCallId::new("review-mc-c10");
    let snapshot = snapshot_two_tools();

    // 1. anthropic: text BEFORE a thinking pair keeps strict order.
    let body = serde_json::json!({
        "id": "msg_review_c10",
        "content": [
            {"type": "text", "text": "first"},
            {"type": "thinking", "thinking": "mid thought", "signature": "sig-mid"},
            {"type": "text", "text": "second"},
            {"type": "redacted_thinking", "data": "enc-mid"}
        ],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 3, "output_tokens": 2}
    });
    let parsed = anthropic_messages::parse_messages_response(&call, &snapshot, &body, &budget)
        .expect("parses");
    let message = match parsed.turn {
        lingxi_kernel::ports::ProviderTurn::Final { message } => message,
        other => panic!("expected Final, got {other:?}"),
    };
    let mut input = ModelTurnInput::first_turn("go on", snapshot.clone());
    input.prior.push(ExchangeItem::AssistantTurn {
        call: call.clone(),
        content: message.content.clone(),
        tool_calls: vec![],
    });
    let rendered = anthropic_messages::render_messages_request(
        &input,
        &route_for(lingxi_kernel::model_exchange::ProtocolFamily::AnthropicMessages),
        anthropic_messages::DEFAULT_MAX_OUTPUT_TOKENS,
    )
    .expect("render");
    let blocks = rendered["messages"][1]["content"].as_array().expect("blocks");
    assert_eq!(
        blocks.as_slice(),
        &[
            serde_json::json!({"type":"text","text":"first"}),
            serde_json::json!({"type":"thinking","thinking":"mid thought","signature":"sig-mid"}),
            serde_json::json!({"type":"text","text":"second"}),
            serde_json::json!({"type":"redacted_thinking","data":"enc-mid"}),
        ],
        "anthropic: strict in-place round trip incl. non-canonical positions"
    );

    // 2. openai-responses: probe the [text, reasoning-item] order — the
    //    canonical provider order is reasoning-first; record what the adapter
    //    actually does with text-first content.
    let reasoning_item = serde_json::json!({
        "type": "reasoning",
        "id": "rs_review",
        "summary": [{"type": "summary_text", "text": "afterthought"}],
        "encrypted_content": "enc-review"
    });
    let mut input2 = ModelTurnInput::first_turn("go on", snapshot.clone());
    input2.prior.push(ExchangeItem::AssistantTurn {
        call: call.clone(),
        content: vec![
            ContentBlock::Text { text: "answer first".to_string() },
            ContentBlock::Reasoning { text: "afterthought".to_string() },
            ContentBlock::Opaque {
                provider: "openai-responses".to_string(),
                data: reasoning_item.clone(),
            },
        ],
        tool_calls: vec![],
    });
    let rendered2 = openai_responses::render_responses_request(
        &input2,
        &route_for(lingxi_kernel::model_exchange::ProtocolFamily::OpenAiResponses),
        false,
    )
    .expect("render");
    let items = rendered2["input"].as_array().expect("items");
    let order: Vec<String> = items
        .iter()
        .skip(1) // the user message
        .map(|i| {
            let t = i["type"].as_str().unwrap_or("?");
            if t == "message" {
                format!("message:{}", i["content"][0]["text"].as_str().unwrap_or(""))
            } else {
                t.to_string()
            }
        })
        .collect();
    println!("C10 openai-responses text-first re-render order: {order:?}");
    let verbatim = items.iter().any(|i| i == &reasoning_item);
    assert!(verbatim, "the opaque item itself round-trips byte-verbatim");
    let positions: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, i)| i["type"] == "reasoning")
        .map(|(ix, _)| ix)
        .collect();
    assert_eq!(positions.len(), 1, "exactly one reasoning item, never duplicated");
    // NOTE(recorded, not asserted): the original order [text, reasoning] is
    // normalized to [reasoning, text] by this renderer — see the review report.

    // 3. cross-family isolation: a google thoughtSignature opaque must never
    //    appear on the anthropic wire, and vice versa.
    let mut input3 = ModelTurnInput::first_turn("go on", snapshot.clone());
    input3.prior.push(ExchangeItem::AssistantTurn {
        call: call.clone(),
        content: vec![
            ContentBlock::Text { text: "visible".to_string() },
            ContentBlock::Opaque {
                provider: "google-generative-ai".to_string(),
                data: serde_json::json!({"type": "thoughtSignature", "signature": "goog-sig-x"}),
            },
            ContentBlock::Opaque {
                provider: "openai-responses".to_string(),
                data: reasoning_item.clone(),
            },
        ],
        tool_calls: vec![],
    });
    let anthropic_wire = anthropic_messages::render_messages_request(
        &input3,
        &route_for(lingxi_kernel::model_exchange::ProtocolFamily::AnthropicMessages),
        anthropic_messages::DEFAULT_MAX_OUTPUT_TOKENS,
    )
    .expect("render");
    let wire_text = anthropic_wire.to_string();
    assert!(!wire_text.contains("goog-sig-x"), "google opaque never echoes onto anthropic");
    assert!(!wire_text.contains("rs_review"), "responses opaque never echoes onto anthropic");
    assert!(wire_text.contains("visible"));

    let google_wire = google_generative_ai::render_generate_request(
        &input3,
        &route_for(lingxi_kernel::model_exchange::ProtocolFamily::GoogleGenerativeAi),
    )
    .expect("render");
    let google_text = google_wire.to_string();
    assert!(!google_text.contains("rs_review"), "responses opaque never echoes onto google");
    assert!(google_text.contains("visible"));
}

// ── C12 (reviewer variant): the retry carries the REAL confirmed outcomes ────

#[tokio::test]
async fn c12_retry_carries_real_confirmed_outcomes_cross_checked_with_journal() {
    let nonce = runtime_nonce("c12");
    let stub = StubServer::start(vec![
        // turn 1: read
        StubResponse::json(serde_json::json!({
            "id": "chatcmpl-r1",
            "choices": [{
                "index": 0, "finish_reason": "tool_calls",
                "message": {"role": "assistant", "content": null, "tool_calls": [{
                    "id": "call_r", "type": "function",
                    "function": {"name": "read", "arguments": "{\"path\":\"note.txt\"}"}
                }]}
            }],
            "usage": {"prompt_tokens": 4, "completion_tokens": 2}
        })),
        // turn 2: write
        StubResponse::json(serde_json::json!({
            "id": "chatcmpl-r2",
            "choices": [{
                "index": 0, "finish_reason": "tool_calls",
                "message": {"role": "assistant", "content": null, "tool_calls": [{
                    "id": "call_w", "type": "function",
                    "function": {"name": "write", "arguments": "{\"path\":\"out.txt\",\"content\":\"v1\"}"}
                }]}
            }],
            "usage": {"prompt_tokens": 5, "completion_tokens": 2}
        })),
        // turn 3: retryable failure
        StubResponse { status: 500, content_type: "application/json",
            body: r#"{"error":{"message":"flaky review 抖动"}}"#.to_string() },
        // attempt 2 turn 1: final
        StubResponse::json(serde_json::json!({
            "id": "chatcmpl-r3",
            "choices": [{
                "index": 0, "finish_reason": "stop",
                "message": {"role": "assistant", "content": "recovered final"}
            }],
            "usage": {"prompt_tokens": 9, "completion_tokens": 3}
        })),
    ])
    .await;
    let plane_json = format!(
        r#""providers": {{
            "main": {{
                "protocol": "openai-completions",
                "endpoint": "{}",
                "auth": {{"kind": "apiKey", "apiKey": "sk-review-synthetic"}}
            }}
        }},
        "models": {{"chat": {{"provider": "main", "model": "gpt-review"}}}}"#,
        stub.v1()
    );
    let boot = boot_with_config("c12r", &plane_json).await;
    std::fs::write(boot.workspace.join("note.txt"), &nonce).expect("seed note");

    let run_id = execute(&boot.state, "sess_local_alpha", "read note.txt then write out.txt").await;
    let (status, reason) = run_row(&boot.state, &run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), Some("completed.with_final"));

    let requests = stub.requests();
    assert_eq!(requests.len(), 4, "read, write, 500, retry-final");
    // The retry's first request carries the FULL confirmed exchange with the
    // REAL outcomes: the read result is the runtime nonce (byte-exact), the
    // write result is the real write outcome (cross-checked below).
    let retry_msgs = requests[3].body["messages"].as_array().expect("retry messages");
    assert_eq!(retry_msgs.len(), 5, "user + asst(read) + tool(read) + asst(write) + tool(write)");
    assert_eq!(retry_msgs[2]["tool_call_id"], "call_r");
    assert_eq!(retry_msgs[2]["content"], nonce, "the retry carries the REAL read bytes");
    assert_eq!(retry_msgs[4]["tool_call_id"], "call_w");
    let write_wire = retry_msgs[4]["content"].as_str().expect("write result text").to_string();

    // Journal cross-check: both tool completions persisted exactly once, and
    // the write's wire text equals the journal outcome's own rendering.
    let payloads = known_payloads(&boot.state, "sess_local_alpha", &run_id).await;
    let completed: Vec<_> = payloads
        .iter()
        .filter_map(|p| match p {
            KnownEventPayload::ToolCallCompleted(p) => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(completed.len(), 2, "read + write exactly once across both attempts");
    use lingxi_adapters::models::tool_render::render_tool_outcome_text;
    // Rebuild the ToolOutcome from the durable wire payload (content AND
    // resource refs) and require the model-facing text to be identical to
    // what the retry carried.
    let write_payload = completed[1];
    assert_eq!(write_payload.result.status, lingxi_protocol::ToolResultStatus::Success);
    let mut rebuilt =
        lingxi_kernel::ports::ToolSuccess::from_content(write_payload.result.content.clone());
    rebuilt.resource_refs = write_payload.result.resource_refs.clone();
    rebuilt.truncated = write_payload.result.truncated;
    let journal_text = render_tool_outcome_text(&ToolOutcome::Success { result: rebuilt });
    assert_eq!(write_wire, journal_text, "model payload == journal render for the write");
    // The failed attempt's 500 left NO assistant turn behind and the run's
    // model calls are exactly 4 (3 + 1 retry), each durable.
    let started = payloads
        .iter()
        .filter(|p| matches!(p, KnownEventPayload::ModelCallStarted(_)))
        .count();
    assert_eq!(started, 4);
    assert_eq!(
        std::fs::read_to_string(boot.workspace.join("out.txt")).expect("out.txt"),
        "v1",
        "the confirmed write landed exactly once"
    );

    stub.stop().await;
    teardown_boot(&boot).await;
}
