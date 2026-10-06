//! R05 RR1 (WP-T04, F11/F12/F33): the permanent regression home of the
//! adversarial batch/terminal counterexamples migrated from the 2026-10-04
//! protocol audit (`artifacts/rust-tauri/R05/RR1/INPUT-adversarial-2026-10-04/
//! audit/protocol/src/lib.rs`, PF05/PF06 legs owned by T04), plus the F33
//! present-but-unmapped terminal-value legs.
//!
//! The assertions are the audit's CONTRACT assertions verbatim in intent:
//! they were red on the frozen candidate (side effects before admission,
//! transport end treated as protocol completion, process-only content
//! promoted to final). Fixture adaptation beyond the audited破坏目标 is
//! limited to legal preconditions (the tool snapshot the audited probes
//! already carried); the counterexample inputs themselves are unchanged.
//!
//! Positive controls sit next to every counterexample: the same families
//! with legal batches / normal terminals must still classify.

use lingxi_adapters::models::streaming::SseEvent;
use lingxi_adapters::models::{
    anthropic_messages as ant, google_generative_ai as goog, openai_codex_responses,
    openai_completions as chat, openai_responses as resp,
};
use lingxi_kernel::model_exchange::{ToolDeclaration, ToolDeclarationSnapshot};
use lingxi_kernel::ports::ProviderTurn;
use lingxi_kernel::toolcatalog::SchemaBudget;
use lingxi_protocol::{ContentBlock, ErrorCode, ModelCallId, ToolSchemaDocument};
use serde_json::{json, Value};

fn snapshot() -> ToolDeclarationSnapshot {
    ToolDeclarationSnapshot {
        catalog_generation: 3,
        declarations: vec![ToolDeclaration {
            target: "tool:first-party:read".into(),
            wire_name: "read".into(),
            description: "Read a file".into(),
            input_schema: ToolSchemaDocument {
                dialect: "json-schema/2020-12".into(),
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
    ModelCallId::new("audit-mc0001")
}

// ── F11: whole-batch admission — duplicate provider ids ─────────────────────

/// PF05 leg 1 (assertion unchanged): two SAME-id calls with DIFFERENT
/// arguments in one model turn must never be admitted — by ANY of the four
/// parser families (Codex shares the Responses parser).
#[test]
fn duplicate_provider_ids_with_conflicting_arguments_reject_whole_batch() {
    let bodies = [
        json!({"choices":[{"message":{"tool_calls":[{"id":"same","function":{"name":"read","arguments":"{\"path\":\"a\"}"}},{"id":"same","function":{"name":"read","arguments":"{\"path\":\"b\"}"}}]},"finish_reason":"tool_calls"}]}),
        json!({"content":[{"type":"tool_use","id":"same","name":"read","input":{"path":"a"}},{"type":"tool_use","id":"same","name":"read","input":{"path":"b"}}],"stop_reason":"tool_use"}),
        json!({"status":"completed","output":[{"type":"function_call","call_id":"same","name":"read","arguments":"{\"path\":\"a\"}"},{"type":"function_call","call_id":"same","name":"read","arguments":"{\"path\":\"b\"}"}]}),
        json!({"candidates":[{"content":{"parts":[{"functionCall":{"id":"same","name":"read","args":{"path":"a"}}},{"functionCall":{"id":"same","name":"read","args":{"path":"b"}}}]},"finishReason":"STOP"}]}),
    ];
    let budget = SchemaBudget::default();
    let results = [
        chat::parse_chat_response(&call(), &snapshot(), &bodies[0], &budget),
        ant::parse_messages_response(&call(), &snapshot(), &bodies[1], &budget),
        resp::parse_responses_response(&call(), &snapshot(), &bodies[2], &budget),
        goog::parse_generate_response(&call(), &snapshot(), &bodies[3], &budget),
    ];
    let summary: Vec<_> = results
        .iter()
        .map(|r| match r {
            Ok(p) => format!("{:?}", p.turn),
            Err(e) => format!("rejected: {e:?}"),
        })
        .collect();
    println!("duplicate_id_results={summary:#?}");
    assert!(
        results.iter().all(|r| r.is_err()),
        "conflicting same-ID calls must never be admitted"
    );
}

/// The identical re-send leg (T04-C10): a SAME-id, SAME-arguments completed
/// re-send carries no new information — it collapses to ONE executable
/// request (never a double execution), while two DIFFERENT-id calls with
/// identical arguments stay two independent requests.
#[test]
fn identical_resends_collapse_but_distinct_ids_stay_independent() {
    let budget = SchemaBudget::default();
    // Identical duplicate (same id, same args) → ONE request.
    let body = json!({"choices":[{"message":{"tool_calls":[
        {"id":"same","function":{"name":"read","arguments":"{\"path\":\"a\"}"}},
        {"id":"same","function":{"name":"read","arguments":"{\"path\":\"a\"}"}}]},
        "finish_reason":"tool_calls"}]});
    let parsed = chat::parse_chat_response(&call(), &snapshot(), &body, &budget)
        .expect("identical re-send is not a conflict");
    match parsed.turn {
        ProviderTurn::ToolRequests { requests, .. } => {
            assert_eq!(requests.len(), 1, "identical re-send collapses to one call");
            assert_eq!(requests[0].provider_call_id.as_deref(), Some("same"));
        }
        other => panic!("unexpected turn: {other:?}"),
    }
    // Same arguments under DIFFERENT ids → TWO requests (no content dedup).
    let body = json!({"choices":[{"message":{"tool_calls":[
        {"id":"one","function":{"name":"read","arguments":"{\"path\":\"a\"}"}},
        {"id":"two","function":{"name":"read","arguments":"{\"path\":\"a\"}"}}]},
        "finish_reason":"tool_calls"}]});
    let parsed = chat::parse_chat_response(&call(), &snapshot(), &body, &budget)
        .expect("distinct ids with equal arguments are two legal calls");
    match parsed.turn {
        ProviderTurn::ToolRequests { requests, .. } => {
            assert_eq!(requests.len(), 2, "distinct ids never merge");
        }
        other => panic!("unexpected turn: {other:?}"),
    }
}

// ── F12: transport end ≠ protocol completion ────────────────────────────────

/// PF05 leg 2 (assertion unchanged): a tool block that never received its
/// `content_block_stop` cannot close as a completed batch just because
/// `message_stop` arrived.
#[test]
fn anthropic_open_tool_block_cannot_close_as_a_completed_batch() {
    let mut a = ant::MessagesStreamAccumulator::new();
    for frame in [
        json!({"type":"message_start","message":{"content":[]}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"t1","name":"read","input":{}}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"a\"}"}}),
        // Deliberately no content_block_stop / message_delta stop_reason.
        json!({"type":"message_stop"}),
    ] {
        a.handle_event(&ev(frame)).unwrap();
    }
    let r = a.finish(&call(), &snapshot(), &SchemaBudget::default());
    assert!(
        r.is_err(),
        "unclosed tool block is not a trustworthy terminal: actual={r:?}"
    );
}

/// PF05 leg 3 (assertion unchanged): text + [DONE] with NO normal choice
/// finish reason is a transport end, not a protocol completion — it must not
/// make a Final.
#[test]
fn openai_done_without_finish_reason_does_not_make_final() {
    let mut a = chat::ChatStreamAccumulator::new();
    a.handle_event(&ev(
        json!({"choices":[{"index":0,"delta":{"content":"only partial"}}]}),
    ))
    .unwrap();
    a.handle_event(&done()).unwrap();
    let r = a.finish(&call(), &snapshot(), &SchemaBudget::default());
    assert!(
        !matches!(r, Ok(ref p) if matches!(p.turn, ProviderTurn::Final { .. })),
        "no normal choice finish reason: actual={r:?}"
    );
}

/// PF06 leg 1 (assertion unchanged): a normally-completed request carrying
/// ONLY non-answer state (reasoning / opaque) is not a final answer — for
/// all five families.
#[test]
fn thinking_only_and_opaque_only_do_not_form_final_answers() {
    let budget = SchemaBudget::default();
    let mut chat_stream = chat::ChatStreamAccumulator::new();
    chat_stream
        .handle_event(&ev(json!({"choices":[{"index":0,"delta":{"reasoning_content":"process only"},"finish_reason":"stop"}]})))
        .unwrap();
    chat_stream.handle_event(&done()).unwrap();
    let results = [
        chat_stream.finish(&call(), &snapshot(), &budget).unwrap(),
        ant::parse_messages_response(&call(), &snapshot(), &json!({"content":[{"type":"thinking","thinking":"process only","signature":"sig"}],"stop_reason":"end_turn"}), &budget).unwrap(),
        goog::parse_generate_response(&call(), &snapshot(), &json!({"candidates":[{"content":{"parts":[{"text":"process only","thought":true}]},"finishReason":"STOP"}]}), &budget).unwrap(),
        resp::parse_responses_response(&call(), &snapshot(), &json!({"status":"completed","output":[{"type":"reasoning","id":"r1","summary":[{"type":"summary_text","text":"process only"}],"encrypted_content":"opaque"}]}), &budget).unwrap(),
        openai_codex_responses::parse_codex_response(&call(), &snapshot(), &json!({"status":"completed","output":[{"type":"reasoning","id":"r1","summary":[],"encrypted_content":"opaque"}]}), &budget).unwrap(),
    ];
    for (i, r) in results.iter().enumerate() {
        println!("process_only_family_{i}={:?}", r.turn);
    }
    assert!(
        results
            .iter()
            .all(|r| !matches!(r.turn, ProviderTurn::Final { .. })),
        "a completed request carrying only non-answer state is not a final answer"
    );
}

/// The missing-terminal legs for the remaining families (F12 self-check
/// "缺 finish reason"): a BUFFERED body without the family's normal stop
/// reason never classifies as Final / ToolRequests — the turn is incomplete,
/// never guessed.
#[test]
fn buffered_bodies_without_a_normal_stop_reason_are_loud() {
    let budget = SchemaBudget::default();
    let no_reason = chat::parse_chat_response(
        &call(),
        &snapshot(),
        &json!({"choices":[{"message":{"content":"t"}}]}),
        &budget,
    );
    assert!(
        no_reason.is_err(),
        "openai buffered body without finish_reason must be loud: {no_reason:?}"
    );
    let no_stop = ant::parse_messages_response(
        &call(),
        &snapshot(),
        &json!({"content":[{"type":"text","text":"t"}]}),
        &budget,
    );
    assert!(
        no_stop.is_err(),
        "anthropic buffered body without stop_reason must be loud: {no_stop:?}"
    );
    let no_finish = goog::parse_generate_response(
        &call(),
        &snapshot(),
        &json!({"candidates":[{"content":{"parts":[{"text":"t"}]}}]}),
        &budget,
    );
    assert!(
        no_finish.is_err(),
        "google buffered body without finishReason must be loud: {no_finish:?}"
    );
    let no_status = resp::parse_responses_response(
        &call(),
        &snapshot(),
        &json!({"output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"t"}]}]}),
        &budget,
    );
    assert!(
        no_status.is_err(),
        "responses buffered body without status must be loud: {no_status:?}"
    );
}

// ── F33: present-but-unmapped terminal values ───────────────────────────────

/// F33: a terminal slot that is PRESENT on the wire but carries a value this
/// adapter does not map (`finish_reason:"weird"`, `stop_reason:"pause_turn"`,
/// `finishReason:"OTHER"`, `status:"odd"`, and an `incomplete` response whose
/// `incomplete_details.reason` is unmapped) is never guessed into a completed
/// turn: every family rejects the WHOLE body with a loud InvalidMessage that
/// NAMES the unknown value. Unlike the ABSENT legs above, these bodies do
/// carry a terminal — so a future widening of any family's mapping (silently
/// treating an unknown value as a normal stop) turns exactly these legs red.
/// Inline positive controls pin the attribution: the SAME bodies with the
/// family's mapped terminal classify normally. Codex shares the Responses
/// parser, per this battery's convention.
#[test]
fn present_but_unmapped_terminal_values_are_loud_and_name_the_value() {
    let budget = SchemaBudget::default();

    // openai-completions — unmapped finish_reason.
    let err = chat::parse_chat_response(
        &call(),
        &snapshot(),
        &json!({"choices":[{"message":{"content":"t"},"finish_reason":"weird"}]}),
        &budget,
    )
    .expect_err("an unmapped finish_reason must be loud");
    assert_eq!(err.code, ErrorCode::InvalidMessage, "openai: {err}");
    assert!(!err.retryable, "openai rejection is not retryable: {err}");
    assert!(
        err.message.contains("UNKNOWN finish_reason") && err.message.contains("\"weird\""),
        "openai error must name the unknown value: {err}"
    );
    let ok = chat::parse_chat_response(
        &call(),
        &snapshot(),
        &json!({"choices":[{"message":{"content":"t"},"finish_reason":"stop"}]}),
        &budget,
    )
    .expect("positive control: a mapped stop still classifies");
    assert!(
        matches!(ok.turn, ProviderTurn::Final { .. }),
        "openai positive control: {ok:?}"
    );

    // anthropic — unmapped stop_reason ("pause_turn" is a real Anthropic
    // stop value this adapter does not map).
    let err = ant::parse_messages_response(
        &call(),
        &snapshot(),
        &json!({"content":[{"type":"text","text":"t"}],"stop_reason":"pause_turn"}),
        &budget,
    )
    .expect_err("an unmapped stop_reason must be loud");
    assert_eq!(err.code, ErrorCode::InvalidMessage, "anthropic: {err}");
    assert!(
        !err.retryable,
        "anthropic rejection is not retryable: {err}"
    );
    assert!(
        err.message.contains("UNKNOWN stop_reason") && err.message.contains("\"pause_turn\""),
        "anthropic error must name the unknown value: {err}"
    );
    let ok = ant::parse_messages_response(
        &call(),
        &snapshot(),
        &json!({"content":[{"type":"text","text":"t"}],"stop_reason":"end_turn"}),
        &budget,
    )
    .expect("positive control: a mapped end_turn still classifies");
    assert!(
        matches!(ok.turn, ProviderTurn::Final { .. }),
        "anthropic positive control: {ok:?}"
    );

    // google — unmapped finishReason ("OTHER" is a real Gemini finishReason
    // this adapter does not map).
    let err = goog::parse_generate_response(
        &call(),
        &snapshot(),
        &json!({"candidates":[{"content":{"parts":[{"text":"t"}]},"finishReason":"OTHER"}]}),
        &budget,
    )
    .expect_err("an unmapped finishReason must be loud");
    assert_eq!(err.code, ErrorCode::InvalidMessage, "google: {err}");
    assert!(!err.retryable, "google rejection is not retryable: {err}");
    assert!(
        err.message.contains("UNKNOWN finishReason") && err.message.contains("\"OTHER\""),
        "google error must name the unknown value: {err}"
    );
    let ok = goog::parse_generate_response(
        &call(),
        &snapshot(),
        &json!({"candidates":[{"content":{"parts":[{"text":"t"}]},"finishReason":"STOP"}]}),
        &budget,
    )
    .expect("positive control: a mapped STOP still classifies");
    assert!(
        matches!(ok.turn, ProviderTurn::Final { .. }),
        "google positive control: {ok:?}"
    );

    // responses — unmapped top-level status.
    let terminal = |status: &str| json!({"status":status,"output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"t"}]}]});
    let err = resp::parse_responses_response(&call(), &snapshot(), &terminal("odd"), &budget)
        .expect_err("an unmapped status must be loud");
    assert_eq!(err.code, ErrorCode::InvalidMessage, "responses: {err}");
    assert!(
        !err.retryable,
        "responses rejection is not retryable: {err}"
    );
    assert!(
        err.message.contains("UNKNOWN status") && err.message.contains("\"odd\""),
        "responses error must name the unknown value: {err}"
    );
    let ok = resp::parse_responses_response(&call(), &snapshot(), &terminal("completed"), &budget)
        .expect("positive control: a mapped completed status still classifies");
    assert!(
        matches!(ok.turn, ProviderTurn::Final { .. }),
        "responses positive control: {ok:?}"
    );

    // responses — `incomplete` whose incomplete_details.reason is present
    // but unmapped: an incomplete turn is a truncation FACT, never a
    // content-classified (or guessed) final.
    let incomplete = |reason: &str| json!({"status":"incomplete","incomplete_details":{"reason":reason},"output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"t"}]}]});
    let err = resp::parse_responses_response(
        &call(),
        &snapshot(),
        &incomplete("weird_incomplete_reason"),
        &budget,
    )
    .expect_err("an unmapped incomplete reason must be loud");
    assert_eq!(
        err.code,
        ErrorCode::InvalidMessage,
        "responses incomplete: {err}"
    );
    assert!(
        !err.retryable,
        "responses incomplete rejection is not retryable: {err}"
    );
    assert!(
        err.message.contains("UNMAPPED reason") && err.message.contains("weird_incomplete_reason"),
        "responses incomplete error must name the unmapped reason: {err}"
    );
    let ok = resp::parse_responses_response(
        &call(),
        &snapshot(),
        &incomplete("max_output_tokens"),
        &budget,
    )
    .expect("positive control: a mapped truncation reason classifies");
    match ok.turn {
        ProviderTurn::Failed { error, retryable } => {
            assert_eq!(error.code, ErrorCode::BudgetExceeded, "{error}");
            assert!(retryable, "a budget truncation is retryable: {error}");
        }
        other => panic!("responses incomplete positive control: {other:?}"),
    }
}

/// F33, stream delivery mode: both wire modes share the SAME classifier —
/// the stream accumulators rebuild the buffered body (splicing the terminal
/// value they captured verbatim) and parse it with the buffered parser, so
/// an unknown terminal stays loud on the stream path too, never guessed into
/// a Final just because the transport ended cleanly. (Google/Responses
/// streams reuse their buffered parsers the same way —
/// `GenerateStreamAccumulator::finish` → `parse_generate_response`,
/// `ResponsesStreamAccumulator::finish` → `parse_responses_response_as`.)
#[test]
fn unmapped_terminal_values_through_the_stream_accumulators_stay_loud() {
    // openai-completions: a clean stream whose only blemish is an unmapped
    // finish_reason value.
    let mut a = chat::ChatStreamAccumulator::new();
    a.handle_event(&ev(
        json!({"choices":[{"index":0,"delta":{"content":"answer"},"finish_reason":"weird"}]}),
    ))
    .unwrap();
    a.handle_event(&done()).unwrap();
    let err = a
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .expect_err("stream finish must surface the unmapped finish_reason");
    assert_eq!(err.code, ErrorCode::InvalidMessage, "openai stream: {err}");
    assert!(
        err.message.contains("UNKNOWN finish_reason") && err.message.contains("\"weird\""),
        "openai stream error must name the unknown value: {err}"
    );

    // anthropic: a fully closed text block + message_delta carrying an
    // unmapped stop_reason.
    let mut a = ant::MessagesStreamAccumulator::new();
    for frame in [
        json!({"type":"message_start","message":{"content":[]}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"answer"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"pause_turn"}}),
        json!({"type":"message_stop"}),
    ] {
        a.handle_event(&ev(frame)).unwrap();
    }
    let err = a
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .expect_err("stream finish must surface the unmapped stop_reason");
    assert_eq!(
        err.code,
        ErrorCode::InvalidMessage,
        "anthropic stream: {err}"
    );
    assert!(
        err.message.contains("UNKNOWN stop_reason") && err.message.contains("\"pause_turn\""),
        "anthropic stream error must name the unknown value: {err}"
    );
}

/// Positive controls (the audit's `positive_controls_…` shape): normal
/// terminals with visible text still form Final; a legal two-call batch
/// still forms ToolRequests with both calls in order.
#[test]
fn positive_controls_normal_terminals_and_legal_batches_still_work() {
    let budget = SchemaBudget::default();
    let mut a = ant::MessagesStreamAccumulator::new();
    for frame in [
        json!({"type":"message_start","message":{"content":[]}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"answer"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"}}),
        json!({"type":"message_stop"}),
    ] {
        a.handle_event(&ev(frame)).unwrap();
    }
    assert!(matches!(
        a.finish(&call(), &snapshot(), &budget).unwrap().turn,
        ProviderTurn::Final { .. }
    ));
    let mut a = chat::ChatStreamAccumulator::new();
    a.handle_event(&ev(
        json!({"choices":[{"index":0,"delta":{"content":"real answer"},"finish_reason":"stop"}]}),
    ))
    .unwrap();
    a.handle_event(&done()).unwrap();
    assert!(matches!(
        a.finish(&call(), &snapshot(), &budget).unwrap().turn,
        ProviderTurn::Final { .. }
    ));
    // Two legal independent calls with distinct ids both survive, in order.
    let body = json!({"choices":[{"message":{"tool_calls":[
        {"id":"first","function":{"name":"read","arguments":"{\"path\":\"a\"}"}},
        {"id":"second","function":{"name":"read","arguments":"{\"path\":\"b\"}"}}]},
        "finish_reason":"tool_calls"}]});
    match chat::parse_chat_response(&call(), &snapshot(), &body, &budget)
        .unwrap()
        .turn
    {
        ProviderTurn::ToolRequests { requests, content } => {
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0].provider_call_id.as_deref(), Some("first"));
            assert_eq!(requests[1].provider_call_id.as_deref(), Some("second"));
            assert!(
                content.is_empty(),
                "a tools-only turn carries no content blocks"
            );
        }
        other => panic!("unexpected turn: {other:?}"),
    }
    // Mixed visible text + reasoning with a normal terminal is still a Final
    // that keeps BOTH blocks (the reasoning is process state, the text is the
    // answer).
    match ant::parse_messages_response(
        &call(),
        &snapshot(),
        &json!({"content":[{"type":"thinking","thinking":"why","signature":"sig"},{"type":"text","text":"answer"}],"stop_reason":"end_turn"}),
        &budget,
    )
    .unwrap()
    .turn
    {
        ProviderTurn::Final { message } => {
            assert!(message
                .content
                .iter()
                .any(|b| matches!(b, ContentBlock::Text { text } if text == "answer")));
            assert!(message
                .content
                .iter()
                .any(|b| matches!(b, ContentBlock::Reasoning { text } if text == "why")));
        }
        other => panic!("unexpected turn: {other:?}"),
    }
}
