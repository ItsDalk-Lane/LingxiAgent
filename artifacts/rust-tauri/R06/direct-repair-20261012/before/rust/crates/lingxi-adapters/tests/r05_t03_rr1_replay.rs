//! R05 RR1 WP-T03 permanent counterexamples (F06–F10): the frozen
//! adversarial legs of
//! `artifacts/rust-tauri/R05/RR1/INPUT-adversarial-2026-10-04/audit/protocol`
//! migrated onto the production crates, with their assertion strengths kept
//! (and tightened where the master repair prompt demands a leg the original
//! probe did not carry: buffered reasoning carriers, fail-closed missing
//! replay state, tool/text interleaving, source-bound replay controls).
//!
//! Every test here walks the REAL production chain for its family —
//! parse/accumulate → canonical exchange → renderer (+ compat where the
//! family consumes it) — never a pre-processed payload. The old-red evidence
//! for these shapes on the frozen HEAD and on the pre-fix candidate is
//! `artifacts/rust-tauri/R05/RR1/WP-T03-E01/old-red-probe-on-current-tree.log`
//! (the untouched adversarial probe, exit 101) plus this suite's own pre-fix
//! run (`old-red-r05-t03-rr1-replay.log`).
//!
//! The F32 hardening legs (RR1 R2, 2026-10-05) pin the R1 behaviors the
//! battery lacked (pure test additions, zero production change): the compat
//! off/utility carrier exemptions and strips, the declared-non-reasoning
//! strip, the no-origin refusals of all three origin-gated families, and the
//! responses duplicated-anchor refusal.

use lingxi_adapters::models::{
    anthropic_messages as ant, compat, config, google_generative_ai as goog,
    openai_codex_responses as codex, openai_completions as chat, openai_responses as resp,
    streaming::{SseDecoder, SseEvent},
};
use lingxi_kernel::model_exchange::{
    CredentialAuthKind, CredentialReference, ExchangeItem, ModelOperation, ModelTurnInput,
    ProtocolFamily, RequestedToolCall, ResolvedModelRoute, ToolDeclaration,
    ToolDeclarationSnapshot, TurnOrigin,
};
use lingxi_kernel::ports::{ProviderTurn, ToolOutcome, ToolRequest};
use lingxi_kernel::toolcatalog::SchemaBudget;
use lingxi_protocol::{ContentBlock, ErrorCode, ModelCallId, ToolCallId};
use serde_json::{json, Value};

// ── shared fixtures (the probe's shapes, production types) ──────────────────

fn route(protocol: ProtocolFamily, provider: &str, model: &str) -> ResolvedModelRoute {
    ResolvedModelRoute {
        provider: provider.to_string(),
        model: model.to_string(),
        operation: ModelOperation::Chat,
        protocol,
        endpoint: format!("https://{provider}.example.test/v1"),
        credential: CredentialReference {
            provider: provider.to_string(),
            auth: CredentialAuthKind::ApiKey,
        },
        config_generation: 1,
        group_id: None,
    }
}

fn origin_of(protocol: ProtocolFamily, provider: &str, model: &str) -> TurnOrigin {
    let _ = protocol;
    TurnOrigin {
        provider: provider.to_string(),
        model: model.to_string(),
    }
}

fn snapshot() -> ToolDeclarationSnapshot {
    ToolDeclarationSnapshot {
        catalog_generation: 3,
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

fn ev(v: Value) -> SseEvent {
    SseEvent {
        event: None,
        data: v.to_string(),
    }
}

fn done() -> SseEvent {
    SseEvent {
        event: None,
        data: "[DONE]".into(),
    }
}

fn call() -> ModelCallId {
    ModelCallId::new("rr1-mc0001")
}

/// The real driver's exchange step: a parsed tool-request turn becomes the
/// assistant turn + the tool results of the round (host ids paired with the
/// provider's correlation ids). `origin` carries the serving identity the
/// driver records for the turn (R05 RR1 F09).
fn exchange_with_origin(turn: ProviderTurn, origin: Option<TurnOrigin>) -> ModelTurnInput {
    let (requests, content) = match turn {
        ProviderTurn::ToolRequests { requests, content } => (requests, content),
        other => panic!("expected tools: {other:?}"),
    };
    let mut input = ModelTurnInput::first_turn("read files", snapshot());
    let calls: Vec<_> = requests
        .iter()
        .enumerate()
        .map(|(i, request)| RequestedToolCall {
            tool_call_id: ToolCallId::new(format!("rr1-tc{i:04}")),
            provider_call_id: request.provider_call_id.clone(),
            target: request.target.clone(),
            arguments: request.arguments.clone(),
            args_digest: request.args_digest.clone(),
            args_summary: None,
        })
        .collect();
    input.prior.push(ExchangeItem::AssistantTurn {
        call: call(),
        content,
        tool_calls: calls.clone(),
        origin,
    });
    for c in calls {
        input.prior.push(ExchangeItem::ToolResult {
            tool_call_id: c.tool_call_id,
            provider_call_id: c.provider_call_id,
            outcome: ToolOutcome::success_text("actual file bytes"),
        });
    }
    input
}

/// The pre-fix exchange shape (no recorded origin) — the frozen probe's
/// exact construction.
fn exchange(turn: ProviderTurn) -> ModelTurnInput {
    exchange_with_origin(turn, None)
}

// ── F06: Anthropic thinking signature placeholder / empty-text state ────────

/// The official Claude streaming shape: `content_block_start` carries
/// `thinking: ""` AND `signature: ""`; the signature arrives later in ONE
/// non-empty `signature_delta`. The opening placeholder is NOT a signature
/// and must never read as a conflict (PF01 leg 1; frozen probe red).
#[test]
fn rr1_f06_stream_placeholder_signature_accepts_the_final_signature_delta() {
    let mut a = ant::MessagesStreamAccumulator::new();
    a.handle_event(&ev(
        json!({"type":"message_start","message":{"content":[]}}),
    ))
    .expect("message_start");
    a.handle_event(&ev(json!({"type":"content_block_start","index":0,
        "content_block":{"type":"thinking","thinking":"","signature":""}})))
        .expect("block start");
    a.handle_event(&ev(json!({"type":"content_block_delta","index":0,
        "delta":{"type":"thinking_delta","thinking":"check the file"}})))
        .expect("thinking delta");
    let result = a.handle_event(&ev(json!({"type":"content_block_delta","index":0,
        "delta":{"type":"signature_delta","signature":"valid-provider-signature"}})));
    assert!(
        result.is_ok(),
        "the official empty-signature placeholder must accept the final signature delta, got {result:?}"
    );
    // The closed batch keeps the block identity: thinking text + the final
    // signature ride the buffered re-parse.
    a.handle_event(&ev(json!({"type":"content_block_stop","index":0})))
        .expect("block stop");
    a.handle_event(&ev(json!({"type":"content_block_start","index":1,
        "content_block":{"type":"text","text":"answer"}})))
        .expect("text start");
    a.handle_event(&ev(json!({"type":"content_block_stop","index":1})))
        .expect("text stop");
    a.handle_event(&ev(
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"}}),
    ))
    .expect("message delta");
    a.handle_event(&ev(json!({"type":"message_stop"})))
        .expect("message stop");
    let parsed = a
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .expect("closed batch parses");
    match parsed.turn {
        ProviderTurn::Final { message } => {
            assert!(
                matches!(&message.content[0], ContentBlock::Reasoning { text }
                    if text == "check the file"),
                "thinking text survives: {:?}",
                message.content
            );
            assert!(
                matches!(&message.content[1], ContentBlock::Opaque { data, .. }
                    if data["type"] == "thinking_signature"
                        && data["signature"] == "valid-provider-signature"),
                "the FINAL signature rides the adjacent opaque: {:?}",
                message.content
            );
        }
        other => panic!("expected Final, got {other:?}"),
    }
}

/// A missing `signature` key on the start frame behaves identically (the
/// placeholder may be absent entirely).
#[test]
fn rr1_f06_stream_absent_placeholder_signature_accepts_the_final_delta() {
    let mut a = ant::MessagesStreamAccumulator::new();
    a.handle_event(&ev(
        json!({"type":"message_start","message":{"content":[]}}),
    ))
    .expect("message_start");
    a.handle_event(&ev(json!({"type":"content_block_start","index":0,
        "content_block":{"type":"thinking","thinking":""}})))
        .expect("block start");
    let result = a.handle_event(&ev(json!({"type":"content_block_delta","index":0,
        "delta":{"type":"signature_delta","signature":"sig-final"}})));
    assert!(
        result.is_ok(),
        "absent placeholder is not a conflict: {result:?}"
    );
}

/// A TRUE conflict — two DIFFERENT non-empty signatures for one block — is
/// still rejected (the fix must not degrade into merge-everything).
#[test]
fn rr1_f06_stream_two_different_final_signatures_are_a_true_conflict() {
    let mut a = ant::MessagesStreamAccumulator::new();
    a.handle_event(&ev(
        json!({"type":"message_start","message":{"content":[]}}),
    ))
    .expect("message_start");
    a.handle_event(&ev(json!({"type":"content_block_start","index":0,
        "content_block":{"type":"thinking","thinking":"x"}})))
        .expect("block start");
    a.handle_event(&ev(json!({"type":"content_block_delta","index":0,
        "delta":{"type":"signature_delta","signature":"sig-one"}})))
        .expect("first final signature");
    let result = a.handle_event(&ev(json!({"type":"content_block_delta","index":0,
        "delta":{"type":"signature_delta","signature":"sig-two"}})));
    assert!(result.is_err(), "two different final signatures conflict");
}

/// An identical `signature_delta` re-send stays a no-op (re-send tolerance).
#[test]
fn rr1_f06_stream_identical_signature_resend_is_tolerated() {
    let mut a = ant::MessagesStreamAccumulator::new();
    a.handle_event(&ev(
        json!({"type":"message_start","message":{"content":[]}}),
    ))
    .expect("message_start");
    a.handle_event(&ev(json!({"type":"content_block_start","index":0,
        "content_block":{"type":"thinking","thinking":"x","signature":""}})))
        .expect("block start");
    a.handle_event(&ev(json!({"type":"content_block_delta","index":0,
        "delta":{"type":"signature_delta","signature":"sig"}})))
        .expect("final signature");
    let result = a.handle_event(&ev(json!({"type":"content_block_delta","index":0,
        "delta":{"type":"signature_delta","signature":"sig"}})));
    assert!(
        result.is_ok(),
        "identical re-send carries no conflict: {result:?}"
    );
}

/// Non-streaming: a thinking block with EMPTY visible text and a VALID
/// signature is required protocol state — the next request must carry it
/// verbatim (PF01 leg 2; frozen probe red).
#[test]
fn rr1_f06_buffered_empty_thinking_preserves_signature_on_next_request() {
    let original = json!({"type":"thinking","thinking":"","signature":"opaque-sig"});
    let body = json!({"content":[original.clone(),
        {"type":"tool_use","id":"t1","name":"read","input":{"path":"a"}}],
        "stop_reason":"tool_use"});
    let parsed =
        ant::parse_messages_response(&call(), &snapshot(), &body, &SchemaBudget::default())
            .expect("parses");
    let input = exchange_with_origin(
        parsed.turn,
        Some(origin_of(
            ProtocolFamily::AnthropicMessages,
            "anthropic-a",
            "claude",
        )),
    );
    let rendered = ant::render_messages_request(
        &input,
        &route(ProtocolFamily::AnthropicMessages, "anthropic-a", "claude"),
        16384,
        false,
    )
    .expect("renders");
    assert_eq!(
        rendered["messages"][1]["content"][0], original,
        "empty visible thinking with a valid signature is live protocol state: {rendered}"
    );
}

/// Byte-split delivery of the same stream (every byte its own chunk through
/// the REAL incremental SSE decoder) must produce the identical closed batch
/// (the A07/T04-C01 half of the F06 contract).
#[test]
fn rr1_f06_byte_split_stream_keeps_the_signed_thinking_block() {
    let frames = [
        json!({"type":"message_start","message":{"content":[]}}),
        json!({"type":"content_block_start","index":0,
            "content_block":{"type":"thinking","thinking":"","signature":""}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"thinking_delta","thinking":"byte split thinking"}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"signature_delta","signature":"sig-split"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,
            "content_block":{"type":"tool_use","id":"t1","name":"read","input":{}}}),
        json!({"type":"content_block_delta","index":1,
            "delta":{"type":"input_json_delta","partial_json":"{\"path\":\"a\"}"}}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use"}}),
        json!({"type":"message_stop"}),
    ];
    let mut body = String::new();
    for frame in &frames {
        body.push_str(&format!(
            "event: {}\ndata: {frame}\n\n",
            frame["type"].as_str().expect("type")
        ));
    }
    // Split EVERY byte through the production incremental decoder.
    let mut decoder = SseDecoder::new();
    let mut events = Vec::new();
    for byte in body.bytes() {
        events.extend(decoder.feed(&[byte]).expect("byte chunk decodes"));
    }
    let finish = decoder.finish().expect("flush decodes");
    assert!(!finish.discarded_partial, "the body ends frame-terminated");
    events.extend(finish.events);
    let mut a = ant::MessagesStreamAccumulator::new();
    for event in &events {
        a.handle_event(event).expect("event applies");
    }
    let parsed = a
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .expect("closed batch parses");
    match parsed.turn {
        ProviderTurn::ToolRequests { requests, content } => {
            assert_eq!(requests.len(), 1);
            assert!(
                matches!(&content[0], ContentBlock::Reasoning { text }
                    if text == "byte split thinking"),
                "{content:?}"
            );
            assert!(
                matches!(&content[1], ContentBlock::Opaque { data, .. }
                    if data["type"] == "thinking_signature" && data["signature"] == "sig-split"),
                "{content:?}"
            );
            // The full round trip: the signed thinking block rides the NEXT
            // request verbatim (parse → exchange → renderer).
            let origin = origin_of(ProtocolFamily::AnthropicMessages, "anthropic-a", "claude");
            let rendered = ant::render_messages_request(
                &exchange_with_origin(
                    ProviderTurn::ToolRequests {
                        requests,
                        content: content.clone(),
                    },
                    Some(origin),
                ),
                &route(ProtocolFamily::AnthropicMessages, "anthropic-a", "claude"),
                16384,
                true,
            )
            .expect("renders");
            assert_eq!(
                rendered["messages"][1]["content"][0],
                json!({"type":"thinking","thinking":"byte split thinking","signature":"sig-split"}),
                "the signed block survives to the next wire request: {rendered}"
            );
        }
        other => panic!("expected ToolRequests, got {other:?}"),
    }
}

// ── F07: Google functionCall Part signatures + parallel result grouping ────

/// A functionCall Part's `thoughtSignature` is required protocol state: both
/// wire modes must keep it ON the function part of the next request, byte
/// for byte, at its original position (PF02 leg 1; frozen probe red).
#[test]
fn rr1_f07_function_call_signature_survives_both_wire_modes() {
    let original = json!({"functionCall":{"id":"g1","name":"read","args":{"path":"a"}},
        "thoughtSignature":"signed-function-part"});
    let body = json!({"candidates":[{"content":{"role":"model","parts":[original.clone()]},
        "finishReason":"STOP"}]});
    let parsed =
        goog::parse_generate_response(&call(), &snapshot(), &body, &SchemaBudget::default())
            .expect("buffered parses");
    let mut stream = goog::GenerateStreamAccumulator::new();
    stream
        .handle_event(&ev(body))
        .expect("stream frame applies");
    let streamed = stream
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .expect("stream finishes");
    let origin = origin_of(ProtocolFamily::GoogleGenerativeAi, "google-a", "gemini");
    let r = route(ProtocolFamily::GoogleGenerativeAi, "google-a", "gemini");
    let b1 =
        goog::render_generate_request(&exchange_with_origin(parsed.turn, Some(origin.clone())), &r)
            .expect("buffered renders");
    let b2 = goog::render_generate_request(&exchange_with_origin(streamed.turn, Some(origin)), &r)
        .expect("streamed renders");
    assert_eq!(b1, b2, "the two wire modes produce the same next request");
    assert_eq!(
        b1["contents"][1]["parts"][0], original,
        "the thoughtSignature stays on the functionCall part: {b1}"
    );
}

/// Parallel tool results of ONE model tool round travel as ONE user Content
/// with two functionResponse parts (the official Gemini 3 grouping; PF02 leg
/// 2; frozen probe red).
#[test]
fn rr1_f07_parallel_results_form_one_user_content() {
    let body = json!({"candidates":[{"content":{"parts":[
        {"functionCall":{"id":"g1","name":"read","args":{"path":"a"}}},
        {"functionCall":{"id":"g2","name":"read","args":{"path":"b"}}}
    ]},"finishReason":"STOP"}]});
    let parsed =
        goog::parse_generate_response(&call(), &snapshot(), &body, &SchemaBudget::default())
            .expect("parses");
    let rendered = goog::render_generate_request(
        &exchange_with_origin(
            parsed.turn,
            Some(origin_of(
                ProtocolFamily::GoogleGenerativeAi,
                "google-a",
                "gemini",
            )),
        ),
        &route(ProtocolFamily::GoogleGenerativeAi, "google-a", "gemini"),
    )
    .expect("renders");
    let contents = rendered["contents"].as_array().expect("contents");
    assert_eq!(
        contents.len(),
        3,
        "user -> model(two calls) -> user(two functionResponse parts): {rendered}"
    );
    assert_eq!(contents[2]["role"], "user");
    let parts = contents[2]["parts"].as_array().expect("parts");
    assert_eq!(parts.len(), 2, "both responses share the ONE user turn");
    assert_eq!(parts[0]["functionResponse"]["id"], "g1");
    assert_eq!(parts[1]["functionResponse"]["id"], "g2");
}

/// Interleaved parts keep their original relative order on the next request:
/// functionCall, text, functionCall stays functionCall → text → functionCall
/// (the F07 ordering contract on the google family).
#[test]
fn rr1_f07_interleaved_parts_keep_original_order() {
    let body = json!({"candidates":[{"content":{"parts":[
        {"functionCall":{"id":"g1","name":"read","args":{"path":"a"}}},
        {"text":"middle visible text"},
        {"functionCall":{"id":"g2","name":"read","args":{"path":"b"}}}
    ]},"finishReason":"STOP"}]});
    let parsed =
        goog::parse_generate_response(&call(), &snapshot(), &body, &SchemaBudget::default())
            .expect("parses");
    let rendered = goog::render_generate_request(
        &exchange_with_origin(
            parsed.turn,
            Some(origin_of(
                ProtocolFamily::GoogleGenerativeAi,
                "google-a",
                "gemini",
            )),
        ),
        &route(ProtocolFamily::GoogleGenerativeAi, "google-a", "gemini"),
    )
    .expect("renders");
    let parts = rendered["contents"][1]["parts"].as_array().expect("parts");
    assert_eq!(parts[0]["functionCall"]["id"], "g1", "{rendered}");
    assert_eq!(parts[1]["text"], "middle visible text", "{rendered}");
    assert_eq!(parts[2]["functionCall"]["id"], "g2", "{rendered}");
}

/// A Part with EMPTY text and a standalone signature is preserved verbatim
/// (the empty-text-signature shape of the F07 self-check matrix; guard leg).
///
/// R05 RR1 F12 interface note: a normally-stopped turn whose only block is
/// protocol state (no answer text) is PROCESS-ONLY — the parser classifies
/// it `Empty` (never `Final`), and the state blocks ride the Empty turn's
/// `content` for replay. The signature-fidelity assertion is unchanged.
#[test]
fn rr1_f07_empty_text_signature_part_round_trips_verbatim() {
    let original = json!({"text":"","thoughtSignature":"sig-empty-text"});
    let body = json!({"candidates":[{"content":{"role":"model","parts":[original.clone()]},
        "finishReason":"STOP"}]});
    let parsed =
        goog::parse_generate_response(&call(), &snapshot(), &body, &SchemaBudget::default())
            .expect("parses");
    // No tool calls and no answer text: a process-only turn whose protocol
    // state still replays as prior content.
    let content = match parsed.turn {
        ProviderTurn::Empty { content, .. } => content,
        other => panic!("expected the process-only Empty turn, got {other:?}"),
    };
    let mut input = ModelTurnInput::first_turn("go on", snapshot());
    input.prior.push(ExchangeItem::AssistantTurn {
        call: call(),
        content,
        tool_calls: Vec::new(),
        origin: Some(origin_of(
            ProtocolFamily::GoogleGenerativeAi,
            "google-a",
            "gemini",
        )),
    });
    let rendered = goog::render_generate_request(
        &input,
        &route(ProtocolFamily::GoogleGenerativeAi, "google-a", "gemini"),
    )
    .expect("renders");
    assert_eq!(
        rendered["contents"][1]["parts"][0], original,
        "the empty-text signed part round-trips verbatim: {rendered}"
    );
}

/// Consecutive tool STEPS (two rounds back-to-back): the results of one
/// round share ONE user Content, and the rounds never merge with each
/// other; results that complete in REVERSE order still group into their
/// own round's turn.
#[test]
fn rr1_f07_consecutive_rounds_and_reverse_results_group_per_round() {
    let budget = SchemaBudget::default();
    let mk_call = |seq: u32, id: &str, path: &str| RequestedToolCall {
        tool_call_id: ToolCallId::new(format!("rr1-tc{seq:04}")),
        provider_call_id: Some(id.to_string()),
        target: "tool:first-party:read".to_string(),
        arguments: ToolRequest::from_effective_arguments(
            "tool:first-party:read",
            json!({"path": path}),
            &budget,
        )
        .expect("effective")
        .arguments
        .clone(),
        args_digest: ToolRequest::from_effective_arguments(
            "tool:first-party:read",
            json!({"path": path}),
            &budget,
        )
        .expect("effective")
        .args_digest
        .clone(),
        args_summary: None,
    };
    let mut input = ModelTurnInput::first_turn("read twice", snapshot());
    let origin = Some(origin_of(
        ProtocolFamily::GoogleGenerativeAi,
        "google-a",
        "gemini",
    ));
    input.prior.push(ExchangeItem::AssistantTurn {
        call: call(),
        content: vec![ContentBlock::Opaque {
            provider: goog::FAMILY.to_string(),
            data: json!({"type":"functionCallPart","id":"g1"}),
        }],
        tool_calls: vec![mk_call(1, "g1", "a")],
        origin: origin.clone(),
    });
    // Round 1's result arrives AFTER round 2's assistant turn? No — the
    // exchange is ordered fact; the REVERSE-completion leg reverses the
    // RESULTS of one round (round 2's two results below are pushed in
    // reverse completion order).
    input.prior.push(ExchangeItem::ToolResult {
        tool_call_id: ToolCallId::new("rr1-tc0001"),
        provider_call_id: Some("g1".to_string()),
        outcome: ToolOutcome::success_text("round one result"),
    });
    input.prior.push(ExchangeItem::AssistantTurn {
        call: ModelCallId::new("rr1-mc0002"),
        content: vec![
            ContentBlock::Text {
                text: "second round".to_string(),
            },
            ContentBlock::Opaque {
                provider: goog::FAMILY.to_string(),
                data: json!({"type":"functionCallPart","id":"g2"}),
            },
            ContentBlock::Opaque {
                provider: goog::FAMILY.to_string(),
                data: json!({"type":"functionCallPart","id":"g3"}),
            },
        ],
        tool_calls: vec![mk_call(2, "g2", "b"), mk_call(3, "g3", "c")],
        origin,
    });
    input.prior.push(ExchangeItem::ToolResult {
        tool_call_id: ToolCallId::new("rr1-tc0003"),
        provider_call_id: Some("g3".to_string()),
        outcome: ToolOutcome::success_text("c completed last but listed first"),
    });
    input.prior.push(ExchangeItem::ToolResult {
        tool_call_id: ToolCallId::new("rr1-tc0002"),
        provider_call_id: Some("g2".to_string()),
        outcome: ToolOutcome::success_text("b result"),
    });
    let rendered = goog::render_generate_request(
        &input,
        &route(ProtocolFamily::GoogleGenerativeAi, "google-a", "gemini"),
    )
    .expect("renders");
    let contents = rendered["contents"].as_array().expect("contents");
    // user + model(g1) + user(g1 result) + model(text, g2, g3) + user(2
    // results) = 5 — the two rounds NEVER merge.
    assert_eq!(contents.len(), 5, "{rendered}");
    assert_eq!(contents[2]["parts"].as_array().expect("p").len(), 1);
    let round_two_results = contents[4]["parts"].as_array().expect("parts");
    assert_eq!(round_two_results.len(), 2, "round 2 groups its two results");
    // Exchange order is the honest order the host recorded (reverse
    // completion listed g3 first — kept, never re-sorted).
    assert_eq!(round_two_results[0]["functionResponse"]["id"], "g3");
    assert_eq!(round_two_results[1]["functionResponse"]["id"], "g2");
    // Round 2's calls sit at their anchored positions after the text.
    let round_two_model = contents[3]["parts"].as_array().expect("parts");
    assert_eq!(round_two_model[0]["text"], "second round");
    assert_eq!(round_two_model[1]["functionCall"]["id"], "g2");
    assert_eq!(round_two_model[2]["functionCall"]["id"], "g3");
}

/// An anchor that names a call this exchange does not carry is a loud
/// render failure on both anchored families (google + responses) — the
/// position-bound call is never guessed, and a duplicated anchor is never
/// merged.
#[test]
fn rr1_f07_f10_unmatched_or_duplicated_anchors_fail_loudly() {
    let budget = SchemaBudget::default();
    let request = ToolRequest::from_effective_arguments(
        "tool:first-party:read",
        json!({"path":"a"}),
        &budget,
    )
    .expect("effective")
    .with_provider_call_id("real-1");
    let mk = |id: &str| RequestedToolCall {
        tool_call_id: ToolCallId::new("rr1-tc0001"),
        provider_call_id: Some(id.to_string()),
        target: request.target.clone(),
        arguments: request.arguments.clone(),
        args_digest: request.args_digest.clone(),
        args_summary: None,
    };
    // Google: an anchor naming an absent id.
    let mut google_input = ModelTurnInput::first_turn("go", snapshot());
    google_input.prior.push(ExchangeItem::AssistantTurn {
        call: call(),
        content: vec![ContentBlock::Opaque {
            provider: goog::FAMILY.to_string(),
            data: json!({"type":"functionCallPart","id":"ghost-9"}),
        }],
        tool_calls: vec![mk("real-1")],
        origin: Some(origin_of(
            ProtocolFamily::GoogleGenerativeAi,
            "google-a",
            "gemini",
        )),
    });
    let err = goog::render_generate_request(
        &google_input,
        &route(ProtocolFamily::GoogleGenerativeAi, "google-a", "gemini"),
    )
    .expect_err("unmatched anchor refuses");
    assert!(err.message.contains("ghost-9"), "{err:?}");
    // Google: the same anchor twice.
    let mut google_dup = ModelTurnInput::first_turn("go", snapshot());
    google_dup.prior.push(ExchangeItem::AssistantTurn {
        call: call(),
        content: vec![
            ContentBlock::Opaque {
                provider: goog::FAMILY.to_string(),
                data: json!({"type":"functionCallPart","id":"real-1"}),
            },
            ContentBlock::Opaque {
                provider: goog::FAMILY.to_string(),
                data: json!({"type":"functionCallPart","id":"real-1"}),
            },
        ],
        tool_calls: vec![mk("real-1")],
        origin: Some(origin_of(
            ProtocolFamily::GoogleGenerativeAi,
            "google-a",
            "gemini",
        )),
    });
    let err = goog::render_generate_request(
        &google_dup,
        &route(ProtocolFamily::GoogleGenerativeAi, "google-a", "gemini"),
    )
    .expect_err("duplicated anchor refuses");
    assert!(err.message.contains("twice"), "{err:?}");
    // Responses: an anchor naming an absent call id.
    let mut responses_input = ModelTurnInput::first_turn("go", snapshot());
    responses_input.prior.push(ExchangeItem::AssistantTurn {
        call: call(),
        content: vec![ContentBlock::Opaque {
            provider: resp::FAMILY.to_string(),
            data: json!({"type":"function_call_item","call_id":"ghost-r"}),
        }],
        tool_calls: vec![mk("real-1")],
        origin: Some(origin_of(
            ProtocolFamily::OpenAiResponses,
            "openai-a",
            "model",
        )),
    });
    let err = resp::render_responses_request(
        &responses_input,
        &route(ProtocolFamily::OpenAiResponses, "openai-a", "model"),
        true,
    )
    .expect_err("unmatched responses anchor refuses");
    assert!(err.message.contains("ghost-r"), "{err:?}");
}

// ── F08: OpenAI-compatible reasoning carrier through renderer + compat ──────

/// The DeepSeek tool round: the REAL stream accumulator captures
/// `reasoning_content`; the canonical keeps it; the production renderer +
/// compat must put the REAL reasoning back on the assistant tool-call
/// message of the next request (PF03; frozen probe red).
#[test]
fn rr1_f08_deepseek_reasoning_replays_through_renderer_and_compat() {
    let mut stream = chat::ChatStreamAccumulator::new();
    stream
        .handle_event(&ev(json!({"choices":[{"index":0,
            "delta":{"reasoning_content":"original provider reasoning"}}]})))
        .expect("reasoning delta");
    stream
        .handle_event(&ev(
            json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,
            "id":"ds1","function":{"name":"read","arguments":"{\"path\":\"a\"}"}}]},
            "finish_reason":"tool_calls"}]}),
        ))
        .expect("tool delta");
    stream.handle_event(&done()).expect("done");
    let parsed = stream
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .expect("finishes");
    let input = exchange(parsed.turn);
    assert!(
        matches!(&input.prior[0], ExchangeItem::AssistantTurn { content, .. }
            if content.iter().any(|b| matches!(b, ContentBlock::Reasoning { text }
                if text == "original provider reasoning"))),
        "the canonical exchange keeps the real reasoning"
    );
    let r = route(
        ProtocolFamily::OpenAiCompletions,
        "deepseek",
        "deepseek-reasoner",
    );
    let rendered = chat::render_chat_request(&input, &r, true).expect("renders");
    let compatible = compat::apply_for_call(rendered, &r, Some(&compat::CompatCall::default()))
        .expect("compat applies");
    assert_eq!(
        compatible["messages"][1]["reasoning_content"],
        json!("original provider reasoning"),
        "the real reasoning rides the assistant tool-call message: {compatible}"
    );
}

/// The buffered wire mode carries the same carrier top-level on the choice
/// message — it must reach the canonical too (the non-stream half of F08).
#[test]
fn rr1_f08_buffered_reasoning_content_reaches_the_canonical() {
    let body = json!({"choices":[{"finish_reason":"tool_calls","message":{
        "role":"assistant",
        "content":null,
        "reasoning_content":"buffered provider reasoning",
        "tool_calls":[{"id":"b1","type":"function","function":{"name":"read","arguments":"{\"path\":\"a\"}"}}]
    }}]});
    let parsed = chat::parse_chat_response(&call(), &snapshot(), &body, &SchemaBudget::default())
        .expect("parses");
    match parsed.turn {
        ProviderTurn::ToolRequests { content, .. } => {
            assert!(
                content
                    .iter()
                    .any(|b| matches!(b, ContentBlock::Reasoning { text }
                        if text == "buffered provider reasoning")),
                "the buffered top-level carrier parses to canonical reasoning: {content:?}"
            );
        }
        other => panic!("expected ToolRequests, got {other:?}"),
    }
}

/// Fail-closed: a DeepSeek thinking-mode tool round whose history LACKS the
/// reasoning carrier must fail LOCALLY at the compat boundary — never a
/// fabricated empty carrier, never a silently degraded wire request.
#[test]
fn rr1_f08_missing_required_reasoning_fails_closed_before_dispatch() {
    let budget = SchemaBudget::default();
    let request = ToolRequest::from_effective_arguments(
        "tool:first-party:read",
        json!({"path":"a"}),
        &budget,
    )
    .expect("effective")
    .with_provider_call_id("ds2");
    let mut input = ModelTurnInput::first_turn("read files", snapshot());
    input.prior.push(ExchangeItem::AssistantTurn {
        call: call(),
        // Text + tool calls but NO reasoning block: the required state is
        // missing for a thinking-mode replay.
        content: vec![ContentBlock::Text {
            text: "reading".to_string(),
        }],
        tool_calls: vec![RequestedToolCall {
            tool_call_id: ToolCallId::new("rr1-tc0001"),
            provider_call_id: request.provider_call_id.clone(),
            target: request.target.clone(),
            arguments: request.arguments.clone(),
            args_digest: request.args_digest.clone(),
            args_summary: None,
        }],
        origin: Some(origin_of(
            ProtocolFamily::OpenAiCompletions,
            "deepseek",
            "deepseek-reasoner",
        )),
    });
    input.prior.push(ExchangeItem::ToolResult {
        tool_call_id: ToolCallId::new("rr1-tc0001"),
        provider_call_id: Some("ds2".to_string()),
        outcome: ToolOutcome::success_text("file body"),
    });
    let r = route(
        ProtocolFamily::OpenAiCompletions,
        "deepseek",
        "deepseek-reasoner",
    );
    let rendered = chat::render_chat_request(&input, &r, true).expect("renders");
    let refused = compat::apply_for_call(rendered, &r, Some(&compat::CompatCall::default()))
        .expect_err("missing required replay state must refuse locally");
    assert!(
        refused
            .message
            .contains("reasoning_content is missing for tool_calls history"),
        "the refusal names the missing carrier: {refused:?}"
    );
}

/// A provider WITHOUT a declared reasoning-replay contract never receives
/// the carrier (no blanket OpenAI injection) — control leg, must stay green.
#[test]
fn rr1_f08_no_contract_provider_never_receives_the_carrier() {
    let mut stream = chat::ChatStreamAccumulator::new();
    stream
        .handle_event(&ev(json!({"choices":[{"index":0,
            "delta":{"reasoning_content":"some reasoning"}}]})))
        .expect("reasoning delta");
    stream
        .handle_event(&ev(
            json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,
            "id":"ac1","function":{"name":"read","arguments":"{}"}}]},
            "finish_reason":"tool_calls"}]}),
        ))
        .expect("tool delta");
    stream.handle_event(&done()).expect("done");
    let parsed = stream
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .expect("finishes");
    let r = route(ProtocolFamily::OpenAiCompletions, "acme", "acme-1");
    let rendered = chat::render_chat_request(&exchange(parsed.turn), &r, true).expect("renders");
    let compatible = compat::apply_for_call(rendered, &r, Some(&compat::CompatCall::default()))
        .expect("compat applies");
    let messages = compatible["messages"].as_array().expect("messages");
    for message in messages {
        assert!(
            message.get("reasoning_content").is_none(),
            "no contract → no carrier on the wire: {message}"
        );
    }
}

// ── F09: opaque state is bound to the provider/model that minted it ────────

/// Same protocol FAMILY is not source authorization: an Anthropic signature
/// minted by provider A must never enter a request for provider B of the
/// same family (PF04; frozen probe red).
#[test]
fn rr1_f09_same_family_other_provider_never_receives_the_signature() {
    let body = json!({"content":[
        {"type":"thinking","thinking":"private-to-provider-a","signature":"provider-a-sig"},
        {"type":"tool_use","id":"a1","name":"read","input":{"path":"a"}}],
        "stop_reason":"tool_use"});
    let parsed =
        ant::parse_messages_response(&call(), &snapshot(), &body, &SchemaBudget::default())
            .expect("parses");
    let from_a = exchange_with_origin(
        parsed.turn,
        Some(origin_of(
            ProtocolFamily::AnthropicMessages,
            "provider-a",
            "claude-a",
        )),
    );
    let to_b = ant::render_messages_request(
        &from_a,
        &route(
            ProtocolFamily::AnthropicMessages,
            "provider-b",
            "other-model",
        ),
        16384,
        true,
    );
    match &to_b {
        Err(error) => assert!(
            error.message.contains("provider-a") && error.message.contains("provider-b"),
            "the refusal names the origin and the target: {error:?}"
        ),
        Ok(body) => {
            panic!("a family tag alone must not authorize cross-provider signature replay: {body}")
        }
    }
}

/// Positive control: the SAME source replays fine (same provider + model).
#[test]
fn rr1_f09_same_origin_replays_the_signature_verbatim() {
    let body = json!({"content":[
        {"type":"thinking","thinking":"private-to-provider-a","signature":"provider-a-sig"},
        {"type":"tool_use","id":"a1","name":"read","input":{"path":"a"}}],
        "stop_reason":"tool_use"});
    let parsed =
        ant::parse_messages_response(&call(), &snapshot(), &body, &SchemaBudget::default())
            .expect("parses");
    let origin = origin_of(ProtocolFamily::AnthropicMessages, "provider-a", "claude-a");
    let from_a = exchange_with_origin(parsed.turn, Some(origin));
    let rendered = ant::render_messages_request(
        &from_a,
        &route(ProtocolFamily::AnthropicMessages, "provider-a", "claude-a"),
        16384,
        true,
    )
    .expect("same-origin replay renders");
    let blocks = rendered["messages"][1]["content"]
        .as_array()
        .expect("blocks");
    assert_eq!(
        blocks[0],
        json!({"type":"thinking","thinking":"private-to-provider-a","signature":"provider-a-sig"}),
        "same origin replays the signed thinking verbatim: {rendered}"
    );
}

/// Same provider, DIFFERENT model: still a source change — the signature
/// cannot be proven valid for another model; refuse loudly.
#[test]
fn rr1_f09_same_provider_other_model_refuses() {
    let body = json!({"content":[
        {"type":"thinking","thinking":"text","signature":"model-a-sig"},
        {"type":"tool_use","id":"a1","name":"read","input":{"path":"a"}}],
        "stop_reason":"tool_use"});
    let parsed =
        ant::parse_messages_response(&call(), &snapshot(), &body, &SchemaBudget::default())
            .expect("parses");
    let from_a = exchange_with_origin(
        parsed.turn,
        Some(origin_of(
            ProtocolFamily::AnthropicMessages,
            "provider-a",
            "model-a",
        )),
    );
    let result = ant::render_messages_request(
        &from_a,
        &route(ProtocolFamily::AnthropicMessages, "provider-a", "model-b"),
        16384,
        true,
    );
    assert!(
        result.is_err(),
        "same provider + other model is a source change: {result:?}"
    );
}

/// Same model ID under a DIFFERENT provider is a different source (the
/// same-modelId-different-provider leg of the self-check matrix).
#[test]
fn rr1_f09_same_model_id_different_provider_refuses() {
    let body = json!({"content":[
        {"type":"thinking","thinking":"text","signature":"shared-model-sig"},
        {"type":"tool_use","id":"a1","name":"read","input":{"path":"a"}}],
        "stop_reason":"tool_use"});
    let parsed =
        ant::parse_messages_response(&call(), &snapshot(), &body, &SchemaBudget::default())
            .expect("parses");
    let from_a = exchange_with_origin(
        parsed.turn,
        Some(origin_of(
            ProtocolFamily::AnthropicMessages,
            "provider-a",
            "shared-model",
        )),
    );
    let result = ant::render_messages_request(
        &from_a,
        &route(
            ProtocolFamily::AnthropicMessages,
            "provider-b",
            "shared-model",
        ),
        16384,
        true,
    );
    assert!(
        result.is_err(),
        "same modelId under another provider is a source change"
    );
}

/// Responses-family reasoning items carry the same source binding.
#[test]
fn rr1_f09_responses_reasoning_item_never_crosses_providers() {
    let body = json!({"status":"completed","output":[
        {"type":"reasoning","id":"r1","summary":[{"type":"summary_text","text":"t"}],
         "encrypted_content":"enc-a"},
        {"type":"function_call","call_id":"t1","name":"read","arguments":"{\"path\":\"a\"}"}]});
    let parsed =
        resp::parse_responses_response(&call(), &snapshot(), &body, &SchemaBudget::default())
            .expect("parses");
    let from_a = exchange_with_origin(
        parsed.turn,
        Some(origin_of(
            ProtocolFamily::OpenAiResponses,
            "openai-a",
            "model-a",
        )),
    );
    let result = resp::render_responses_request(
        &from_a,
        &route(ProtocolFamily::OpenAiResponses, "openai-b", "model-a"),
        true,
    );
    assert!(
        !matches!(&result, Ok(body) if body.to_string().contains("enc-a")),
        "provider A's encrypted reasoning never enters provider B's request: {result:?}"
    );
}

// ── F10: Responses/Codex replay keeps text/reasoning/tool relative order ────

/// Original order text → reasoning → tool must survive the next request
/// (PF07; frozen probe red).
#[test]
fn rr1_f10_responses_keep_text_reasoning_tool_order() {
    let body = json!({"status":"completed","output":[
        {"type":"message","role":"assistant","content":[{"type":"output_text","text":"first text"}]},
        {"type":"reasoning","id":"r1","summary":[{"type":"summary_text","text":"then thought"}],
         "encrypted_content":"sig"},
        {"type":"function_call","call_id":"t1","name":"read","arguments":"{\"path\":\"a\"}"}]});
    let parsed =
        resp::parse_responses_response(&call(), &snapshot(), &body, &SchemaBudget::default())
            .expect("parses");
    let origin = origin_of(ProtocolFamily::OpenAiResponses, "openai-a", "model");
    let rendered = resp::render_responses_request(
        &exchange_with_origin(parsed.turn, Some(origin)),
        &route(ProtocolFamily::OpenAiResponses, "openai-a", "model"),
        true,
    )
    .expect("renders");
    let items = rendered["input"].as_array().expect("input items");
    assert_eq!(
        items[1]["type"],
        json!("message"),
        "text preceded reasoning originally: {rendered}"
    );
    assert_eq!(
        items[1]["content"][0]["text"],
        json!("first text"),
        "the original text rides its position: {rendered}"
    );
    assert_eq!(items[2]["type"], json!("reasoning"), "{rendered}");
    assert_eq!(items[3]["type"], json!("function_call"), "{rendered}");
}

/// reasoning → text → tool keeps its order too (the standard reasoning-first
/// shape is a positive control, not a forced normalization).
#[test]
fn rr1_f10_responses_keep_reasoning_text_tool_order() {
    let body = json!({"status":"completed","output":[
        {"type":"reasoning","id":"r1","summary":[{"type":"summary_text","text":"thought first"}],
         "encrypted_content":"sig"},
        {"type":"message","role":"assistant","content":[{"type":"output_text","text":"then text"}]},
        {"type":"function_call","call_id":"t1","name":"read","arguments":"{\"path\":\"a\"}"}]});
    let parsed =
        resp::parse_responses_response(&call(), &snapshot(), &body, &SchemaBudget::default())
            .expect("parses");
    let origin = origin_of(ProtocolFamily::OpenAiResponses, "openai-a", "model");
    let rendered = resp::render_responses_request(
        &exchange_with_origin(parsed.turn, Some(origin)),
        &route(ProtocolFamily::OpenAiResponses, "openai-a", "model"),
        true,
    )
    .expect("renders");
    let items = rendered["input"].as_array().expect("input items");
    assert_eq!(items[1]["type"], json!("reasoning"), "{rendered}");
    assert_eq!(items[2]["type"], json!("message"), "{rendered}");
    assert_eq!(
        items[2]["content"][0]["text"],
        json!("then text"),
        "{rendered}"
    );
    assert_eq!(items[3]["type"], json!("function_call"), "{rendered}");
}

/// Multi-segment interleave: message → reasoning → message keeps BOTH text
/// segments and the reasoning item at their original positions.
#[test]
fn rr1_f10_responses_multi_segment_interleave_keeps_positions() {
    let body = json!({"status":"completed","output":[
        {"type":"message","role":"assistant","content":[{"type":"output_text","text":"seg one"}]},
        {"type":"reasoning","id":"r1","summary":[{"type":"summary_text","text":"mid thought"}],
         "encrypted_content":"sig-mid"},
        {"type":"message","role":"assistant","content":[{"type":"output_text","text":"seg two"}]}]});
    let parsed =
        resp::parse_responses_response(&call(), &snapshot(), &body, &SchemaBudget::default())
            .expect("parses");
    let content = match parsed.turn {
        ProviderTurn::Final { message } => message.content,
        other => panic!("expected Final, got {other:?}"),
    };
    let mut input = ModelTurnInput::first_turn("go on", snapshot());
    input.prior.push(ExchangeItem::AssistantTurn {
        call: call(),
        content,
        tool_calls: Vec::new(),
        origin: Some(origin_of(
            ProtocolFamily::OpenAiResponses,
            "openai-a",
            "model",
        )),
    });
    let rendered = resp::render_responses_request(
        &input,
        &route(ProtocolFamily::OpenAiResponses, "openai-a", "model"),
        false,
    )
    .expect("renders");
    let items = rendered["input"].as_array().expect("input items");
    assert_eq!(items[1]["type"], json!("message"), "{rendered}");
    assert_eq!(
        items[1]["content"][0]["text"],
        json!("seg one"),
        "{rendered}"
    );
    assert_eq!(items[2]["type"], json!("reasoning"), "{rendered}");
    assert_eq!(
        items[2]["encrypted_content"],
        json!("sig-mid"),
        "{rendered}"
    );
    assert_eq!(items[3]["type"], json!("message"), "{rendered}");
    assert_eq!(
        items[3]["content"][0]["text"],
        json!("seg two"),
        "{rendered}"
    );
}

/// Tools interspersed with content keep their positions: function_call,
/// text, function_call replays in that exact order.
#[test]
fn rr1_f10_responses_tools_interspersed_keep_positions() {
    let body = json!({"status":"completed","output":[
        {"type":"function_call","call_id":"c1","name":"read","arguments":"{\"path\":\"a\"}"},
        {"type":"message","role":"assistant","content":[{"type":"output_text","text":"between calls"}]},
        {"type":"function_call","call_id":"c2","name":"read","arguments":"{\"path\":\"b\"}"}]});
    let parsed =
        resp::parse_responses_response(&call(), &snapshot(), &body, &SchemaBudget::default())
            .expect("parses");
    let origin = origin_of(ProtocolFamily::OpenAiResponses, "openai-a", "model");
    let rendered = resp::render_responses_request(
        &exchange_with_origin(parsed.turn, Some(origin)),
        &route(ProtocolFamily::OpenAiResponses, "openai-a", "model"),
        true,
    )
    .expect("renders");
    let items = rendered["input"].as_array().expect("input items");
    assert_eq!(items[1]["type"], json!("function_call"), "{rendered}");
    assert_eq!(items[1]["call_id"], json!("c1"), "{rendered}");
    assert_eq!(items[2]["type"], json!("message"), "{rendered}");
    assert_eq!(
        items[2]["content"][0]["text"],
        json!("between calls"),
        "{rendered}"
    );
    assert_eq!(items[3]["type"], json!("function_call"), "{rendered}");
    assert_eq!(items[3]["call_id"], json!("c2"), "{rendered}");
}

/// The codex sibling shares the renderer — the same ordering contract holds.
#[test]
fn rr1_f10_codex_replay_keeps_text_reasoning_tool_order() {
    let body = json!({"status":"completed","output":[
        {"type":"message","role":"assistant","content":[{"type":"output_text","text":"codex text"}]},
        {"type":"reasoning","id":"r1","summary":[{"type":"summary_text","text":"codex thought"}],
         "encrypted_content":"codex-sig"},
        {"type":"function_call","call_id":"t1","name":"read","arguments":"{\"path\":\"a\"}"}]});
    let parsed = codex::parse_codex_response(&call(), &snapshot(), &body, &SchemaBudget::default())
        .expect("parses");
    let origin = origin_of(
        ProtocolFamily::OpenAiCodexResponses,
        "codex-a",
        "codex-model",
    );
    let rendered = codex::render_codex_request(
        &exchange_with_origin(parsed.turn, Some(origin)),
        &route(
            ProtocolFamily::OpenAiCodexResponses,
            "codex-a",
            "codex-model",
        ),
    )
    .expect("renders");
    let items = rendered["input"].as_array().expect("input items");
    assert_eq!(
        items[1]["type"],
        json!("message"),
        "text first, as produced: {rendered}"
    );
    assert_eq!(items[2]["type"], json!("reasoning"), "{rendered}");
    assert_eq!(
        items[2]["encrypted_content"],
        json!("codex-sig"),
        "{rendered}"
    );
    assert_eq!(items[3]["type"], json!("function_call"), "{rendered}");
}

// ── F32: the hardening legs the battery lacked (pure test additions; the
// behaviors shipped with F06–F10 and were verified by the R1 reviewer's
// isolated probe — these legs pin them permanently) ──────────────────────────

fn deepseek_route() -> ResolvedModelRoute {
    route(
        ProtocolFamily::OpenAiCompletions,
        "deepseek",
        "deepseek-reasoner",
    )
}

/// The DeepSeek thinking-model tool round whose history LACKS the reasoning
/// carrier — the exact shape the F08 leg
/// `rr1_f08_missing_required_reasoning_fails_closed_before_dispatch` REFUSES
/// under default (thinking-mode) options. The F32 legs below run the very
/// same exchange under off/utility options and must be exempt.
fn deepseek_tool_round_without_carrier() -> ModelTurnInput {
    let budget = SchemaBudget::default();
    let request = ToolRequest::from_effective_arguments(
        "tool:first-party:read",
        json!({"path":"a"}),
        &budget,
    )
    .expect("effective")
    .with_provider_call_id("ds2");
    let mut input = ModelTurnInput::first_turn("read files", snapshot());
    input.prior.push(ExchangeItem::AssistantTurn {
        call: call(),
        // Text + tool calls but NO reasoning block: the required state is
        // missing for a thinking-mode replay.
        content: vec![ContentBlock::Text {
            text: "reading".to_string(),
        }],
        tool_calls: vec![RequestedToolCall {
            tool_call_id: ToolCallId::new("rr1-tc0001"),
            provider_call_id: request.provider_call_id.clone(),
            target: request.target.clone(),
            arguments: request.arguments.clone(),
            args_digest: request.args_digest.clone(),
            args_summary: None,
        }],
        origin: Some(origin_of(
            ProtocolFamily::OpenAiCompletions,
            "deepseek",
            "deepseek-reasoner",
        )),
    });
    input.prior.push(ExchangeItem::ToolResult {
        tool_call_id: ToolCallId::new("rr1-tc0001"),
        provider_call_id: Some("ds2".to_string()),
        outcome: ToolOutcome::success_text("file body"),
    });
    input
}

/// The REAL stream chain of a DeepSeek thinking-mode tool round WITH the
/// reasoning carrier on board (the F08 replay shape).
fn deepseek_tool_round_with_carrier() -> ModelTurnInput {
    let mut stream = chat::ChatStreamAccumulator::new();
    stream
        .handle_event(&ev(json!({"choices":[{"index":0,
            "delta":{"reasoning_content":"real reasoning carrier"}}]})))
        .expect("reasoning delta");
    stream
        .handle_event(&ev(
            json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,
            "id":"ds9","function":{"name":"read","arguments":"{\"path\":\"a\"}"}}]},
            "finish_reason":"tool_calls"}]}),
        ))
        .expect("tool delta");
    stream.handle_event(&done()).expect("done");
    let parsed = stream
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .expect("finishes");
    exchange(parsed.turn)
}

/// No message of the payload carries a `reasoning_content` key.
fn assert_no_reasoning_carrier(payload: &Value, context: &str) {
    let messages = payload["messages"].as_array().expect("messages");
    for message in messages {
        assert!(
            message.get("reasoning_content").is_none(),
            "{context}: no reasoning_content may ride the wire: {message}"
        );
    }
}

/// (a1) `reasoning_level: "off"`: the SAME missing-carrier DeepSeek tool
/// round that default options refuse is EXEMPT — no local refusal, no
/// fabricated empty carrier, thinking marked disabled.
#[test]
fn rr1_f32_off_level_deepseek_round_without_carrier_is_exempt() {
    let r = deepseek_route();
    let rendered = chat::render_chat_request(&deepseek_tool_round_without_carrier(), &r, true)
        .expect("renders");
    let off = compat::CompatCall {
        options: compat::CompatOptions {
            reasoning_level: Some("off".to_string()),
            ..Default::default()
        },
        ..Default::default()
    };
    let compatible = compat::apply_for_call(rendered, &r, Some(&off))
        .expect("off level exempts the require-carrier refusal");
    assert_no_reasoning_carrier(&compatible, "off level never fabricates a carrier");
    assert_eq!(
        compatible["thinking"],
        json!({"type": "disabled"}),
        "the module marks thinking disabled: {compatible}"
    );
}

/// (a2) `CompatMode::Utility`: the same exemption — an auxiliary-slot call
/// never runs thinking mode, so the missing carrier is no protocol breach.
#[test]
fn rr1_f32_utility_mode_deepseek_round_without_carrier_is_exempt() {
    let r = deepseek_route();
    let rendered = chat::render_chat_request(&deepseek_tool_round_without_carrier(), &r, true)
        .expect("renders");
    let utility = compat::CompatCall {
        options: compat::CompatOptions {
            mode: compat::CompatMode::Utility,
            ..Default::default()
        },
        ..Default::default()
    };
    let compatible = compat::apply_for_call(rendered, &r, Some(&utility))
        .expect("utility mode exempts the require-carrier refusal");
    assert_no_reasoning_carrier(&compatible, "utility mode never fabricates a carrier");
    assert_eq!(
        compatible["thinking"],
        json!({"type": "disabled"}),
        "the module marks thinking disabled: {compatible}"
    );
}

/// (a3) `reasoning_level: "off"` with a REAL carrier on board: still no
/// refusal, and the carrier is STRIPPED from the wire. The inline control
/// (default options on the same rendered payload) keeps the carrier — the
/// strip is the off-level behavior, not a blanket removal.
#[test]
fn rr1_f32_off_level_strips_a_present_deepseek_carrier() {
    let r = deepseek_route();
    let rendered =
        chat::render_chat_request(&deepseek_tool_round_with_carrier(), &r, true).expect("renders");
    let kept = compat::apply_for_call(rendered.clone(), &r, Some(&compat::CompatCall::default()))
        .expect("control: thinking-mode options replay the carrier");
    assert_eq!(
        kept["messages"][1]["reasoning_content"],
        json!("real reasoning carrier"),
        "control: the REAL carrier rides the thinking-mode wire: {kept}"
    );
    let off = compat::CompatCall {
        options: compat::CompatOptions {
            reasoning_level: Some("off".to_string()),
            ..Default::default()
        },
        ..Default::default()
    };
    let stripped = compat::apply_for_call(rendered, &r, Some(&off))
        .expect("off level never refuses a present carrier either");
    assert_no_reasoning_carrier(&stripped, "off level strips the carrier");
    assert_eq!(
        stripped["thinking"],
        json!({"type": "disabled"}),
        "the module marks thinking disabled: {stripped}"
    );
}

/// (a4) `CompatMode::Utility` with a REAL carrier on board: same strip.
#[test]
fn rr1_f32_utility_mode_strips_a_present_deepseek_carrier() {
    let r = deepseek_route();
    let rendered =
        chat::render_chat_request(&deepseek_tool_round_with_carrier(), &r, true).expect("renders");
    let utility = compat::CompatCall {
        options: compat::CompatOptions {
            mode: compat::CompatMode::Utility,
            ..Default::default()
        },
        ..Default::default()
    };
    let stripped = compat::apply_for_call(rendered, &r, Some(&utility))
        .expect("utility mode never refuses a present carrier either");
    assert_no_reasoning_carrier(&stripped, "utility mode strips the carrier");
}

/// (b) A model DECLARING `hints.reasoning = false` has no replay contract:
/// the carrier is stripped before dispatch (never rides a non-reasoning
/// wire) and the missing/present carrier never triggers a refusal. The
/// inline control (no declaration) keeps the carrier.
#[test]
fn rr1_f32_declared_non_reasoning_model_strips_the_carrier() {
    let r = deepseek_route();
    let rendered =
        chat::render_chat_request(&deepseek_tool_round_with_carrier(), &r, true).expect("renders");
    let kept = compat::apply_for_call(rendered.clone(), &r, Some(&compat::CompatCall::default()))
        .expect("control: an undeclared view replays the carrier");
    assert_eq!(
        kept["messages"][1]["reasoning_content"],
        json!("real reasoning carrier"),
        "control: the carrier rides without the declaration: {kept}"
    );
    let declared_non_reasoning = compat::CompatCall {
        hints: Some(config::RouteCompatHints {
            reasoning: Some(false),
            ..Default::default()
        }),
        options: compat::CompatOptions::default(),
    };
    let stripped = compat::apply_for_call(rendered, &r, Some(&declared_non_reasoning))
        .expect("a declared non-reasoning model is never refused over the carrier");
    assert_no_reasoning_carrier(
        &stripped,
        "a declared non-reasoning model strips the carrier",
    );
}

/// (c) All three origin-gated families: a turn carrying the family's OWN
/// opaque state with NO recorded serving origin is refused with
/// `InvalidMessage`("… no recorded serving origin …"), non-retryable — and
/// the SAME turn with its recorded origin renders (the refusal is the
/// missing origin, nothing else about the payload).
#[test]
fn rr1_f32_no_origin_family_state_refused_on_all_three_gated_families() {
    // anthropic: a signed thinking block is family opaque state.
    let body = json!({"content":[
        {"type":"thinking","thinking":"private thought","signature":"sig-no-origin"},
        {"type":"tool_use","id":"a1","name":"read","input":{"path":"a"}}],
        "stop_reason":"tool_use"});
    let parsed =
        ant::parse_messages_response(&call(), &snapshot(), &body, &SchemaBudget::default())
            .expect("parses");
    let r = route(ProtocolFamily::AnthropicMessages, "anthropic-a", "claude");
    let err = ant::render_messages_request(&exchange(parsed.turn.clone()), &r, 16384, true)
        .expect_err("anthropic family state without a recorded origin refuses");
    assert_eq!(err.code, ErrorCode::InvalidMessage, "{err:?}");
    assert!(!err.retryable, "{err:?}");
    assert!(
        err.message.contains("no recorded serving origin"),
        "the refusal names the missing origin: {err:?}"
    );
    ant::render_messages_request(
        &exchange_with_origin(
            parsed.turn,
            Some(origin_of(
                ProtocolFamily::AnthropicMessages,
                "anthropic-a",
                "claude",
            )),
        ),
        &r,
        16384,
        true,
    )
    .expect("positive control: the recorded origin authorizes the same state");

    // google: a signed functionCall Part is family opaque state.
    let body = json!({"candidates":[{"content":{"role":"model","parts":[
        {"functionCall":{"id":"g1","name":"read","args":{"path":"a"}},
         "thoughtSignature":"sig-no-origin"}]},
        "finishReason":"STOP"}]});
    let parsed =
        goog::parse_generate_response(&call(), &snapshot(), &body, &SchemaBudget::default())
            .expect("parses");
    let r = route(ProtocolFamily::GoogleGenerativeAi, "google-a", "gemini");
    let err = goog::render_generate_request(&exchange(parsed.turn.clone()), &r)
        .expect_err("google family state without a recorded origin refuses");
    assert_eq!(err.code, ErrorCode::InvalidMessage, "{err:?}");
    assert!(!err.retryable, "{err:?}");
    assert!(
        err.message.contains("no recorded serving origin"),
        "the refusal names the missing origin: {err:?}"
    );
    goog::render_generate_request(
        &exchange_with_origin(
            parsed.turn,
            Some(origin_of(
                ProtocolFamily::GoogleGenerativeAi,
                "google-a",
                "gemini",
            )),
        ),
        &r,
    )
    .expect("positive control: the recorded origin authorizes the same state");

    // responses: an encrypted reasoning item is family opaque state.
    let body = json!({"status":"completed","output":[
        {"type":"reasoning","id":"r1","summary":[{"type":"summary_text","text":"t"}],
         "encrypted_content":"enc-no-origin"},
        {"type":"function_call","call_id":"t1","name":"read","arguments":"{\"path\":\"a\"}"}]});
    let parsed =
        resp::parse_responses_response(&call(), &snapshot(), &body, &SchemaBudget::default())
            .expect("parses");
    let r = route(ProtocolFamily::OpenAiResponses, "openai-a", "model");
    let err = resp::render_responses_request(&exchange(parsed.turn.clone()), &r, true)
        .expect_err("responses family state without a recorded origin refuses");
    assert_eq!(err.code, ErrorCode::InvalidMessage, "{err:?}");
    assert!(!err.retryable, "{err:?}");
    assert!(
        err.message.contains("no recorded serving origin"),
        "the refusal names the missing origin: {err:?}"
    );
    resp::render_responses_request(
        &exchange_with_origin(
            parsed.turn,
            Some(origin_of(
                ProtocolFamily::OpenAiResponses,
                "openai-a",
                "model",
            )),
        ),
        &r,
        true,
    )
    .expect("positive control: the recorded origin authorizes the same state");
}

/// (d) The responses-side duplicated `function_call_item` anchor: naming the
/// same call twice is a conflict, refused loudly (the google sibling of this
/// leg exists; the responses duplicate was missing).
#[test]
fn rr1_f32_responses_duplicated_function_call_anchor_is_refused() {
    let budget = SchemaBudget::default();
    let request = ToolRequest::from_effective_arguments(
        "tool:first-party:read",
        json!({"path":"a"}),
        &budget,
    )
    .expect("effective")
    .with_provider_call_id("real-1");
    let mk = |id: &str| RequestedToolCall {
        tool_call_id: ToolCallId::new("rr1-tc0001"),
        provider_call_id: Some(id.to_string()),
        target: request.target.clone(),
        arguments: request.arguments.clone(),
        args_digest: request.args_digest.clone(),
        args_summary: None,
    };
    let mut input = ModelTurnInput::first_turn("go", snapshot());
    input.prior.push(ExchangeItem::AssistantTurn {
        call: call(),
        // The SAME anchor twice: the second hit must refuse (never merge).
        content: vec![
            ContentBlock::Opaque {
                provider: resp::FAMILY.to_string(),
                data: json!({"type":"function_call_item","call_id":"real-1"}),
            },
            ContentBlock::Opaque {
                provider: resp::FAMILY.to_string(),
                data: json!({"type":"function_call_item","call_id":"real-1"}),
            },
        ],
        tool_calls: vec![mk("real-1")],
        origin: Some(origin_of(
            ProtocolFamily::OpenAiResponses,
            "openai-a",
            "model",
        )),
    });
    let err = resp::render_responses_request(
        &input,
        &route(ProtocolFamily::OpenAiResponses, "openai-a", "model"),
        true,
    )
    .expect_err("a duplicated responses anchor refuses");
    assert_eq!(err.code, ErrorCode::InvalidMessage, "{err:?}");
    assert!(!err.retryable, "{err:?}");
    assert!(
        err.message.contains("real-1") && err.message.contains("twice"),
        "the refusal names the duplicated anchor: {err:?}"
    );
}
