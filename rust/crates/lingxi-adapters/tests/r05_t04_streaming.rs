//! R05-T04 adapter-level evidence: the incremental SSE decode + per-family
//! stream accumulators under arbitrary byte fragmentation, batch-admission
//! honesty, duplicate/conflict handling, stop-reason semantics, unknown-block
//! isolation and the decoder bound/error honesty — C01/C02 and the
//! accumulator halves of C05-C11/C14/C15.
//!
//! Double boundary: no network here — the decoder and the accumulators are
//! pure functions of the byte stream (the production read drives the SAME
//! objects through `dispatch::drive_sse_stream`; the service-level suite
//! proves the wiring over loopback HTTP). The one-shot reference is
//! `streaming::decode_complete_body` + one accumulator pass.
//!
//! Coverage map (appendix-B R05-T04 checkpoints):
//! - C01: one complete response (Chinese, emoji, a signature block, TWO
//!   interleaved tool calls) fragmented byte-by-byte, at EVERY two-way cut
//!   point and at fixed-seed random cuts is semantically identical to the
//!   unfragmented decode; tool fragments never leave the accumulator as
//!   live deltas, so nothing executable exists before the batch closes.
//! - C02: invalid UTF-8 (split multi-byte sequences included) is one
//!   deterministic loud error at every fragmentation; no replacement
//!   characters, no event leak past the corrupt line.
//! - C04: the pre-registered bounds are pinned and loud — the whole-stream
//!   undelivered buffer (8 MiB, tighter than the 16 MiB ceiling), the
//!   per-frame data bound (1 MiB), the per-tool arguments bound (1 MiB,
//!   refused at feed time in both fragmenting families) and pathological
//!   JSON nesting (the close fails loud, never a stack overflow).
//! - C05: half-JSON tool arguments at stream end are a loud InvalidMessage
//!   in both fragmenting families — the batch never parses, nothing
//!   dispatches.
//! - C06: parseable arguments mid-stream admit NOTHING (the accumulator
//!   exposes no tool batch before the protocol terminal; an early finish()
//!   is loud).
//! - C07: wrong-shaped arguments (null/array/number/bool) refuse loudly;
//!   duplicate keys and over-precision numbers follow ONE deterministic
//!   canonical rule — the digest covers exactly the value the executor
//!   sees, so no approval summary can distort.
//! - C08: two tool calls interleaved fragment-by-fragment accumulate
//!   independently under their own indexes (no global arguments buffer).
//! - C09: a truncated batch (one complete tool call, `length`/`max_tokens`
//!   stop) classifies as a retryable budget failure — ZERO dispatch, never
//!   a half-executed batch.
//! - C10: duplicate terminal markers, conflicting ids/names/usage/
//!   signatures and event-field disagreement are loud; identical re-sends
//!   are tolerated; adjacent identical text deltas are NEVER deduplicated.
//! - C11: the stop-reason table over the closed-batch parsers of all three
//!   fragmenting families (stop/tool/length/refusal/EOF).
//! - C14: anthropic opaque blocks (`redacted_thinking`) round-trip
//!   verbatim, a delta for an opaque block is loud, and a thinking
//!   signature never leaks into the emitted text/reasoning deltas.
//! - C15 (decoder half): a mid-frame EOF and a non-JSON data frame are
//!   loud refusals; a sink closure abandons the read as non-retryable
//!   Cancelled (the cancel half is service-level).

use lingxi_adapters::models::dispatch::{FamilyStreamDrive, SseStreamHandler};
use lingxi_adapters::models::streaming::{decode_complete_body, SseDecoder, SseEvent};
use lingxi_adapters::models::{
    anthropic_messages::MessagesStreamAccumulator, google_generative_ai::GenerateStreamAccumulator,
    openai_completions::ChatStreamAccumulator,
};
use lingxi_kernel::model_exchange::{ToolDeclaration, ToolDeclarationSnapshot};
use lingxi_kernel::ports::{ModelTurnDelta, ProviderTurn, TurnDeltaSink, TurnDeltaSinkClosed};
use lingxi_kernel::toolcatalog::SchemaBudget;
use lingxi_protocol::{ContentBlock, ErrorCode, ModelCallId, ToolSchemaDocument, UsageRecord};

// ── fixtures ─────────────────────────────────────────────────────────────────

fn snapshot() -> ToolDeclarationSnapshot {
    let tool = |target: &str, wire: &str, schema: serde_json::Value| ToolDeclaration {
        target: target.to_string(),
        wire_name: wire.to_string(),
        description: format!("{wire} tool"),
        input_schema: ToolSchemaDocument {
            dialect: "json-schema/2020-12".to_string(),
            schema,
        },
    };
    ToolDeclarationSnapshot {
        catalog_generation: 7,
        declarations: vec![
            tool(
                "tool:first-party:read",
                "read",
                serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            ),
            tool(
                "tool:first-party:write",
                "write",
                serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}}),
            ),
        ],
    }
}

fn call() -> ModelCallId {
    ModelCallId::new("run_t04-mc0001")
}

/// The anthropic C01 fixture: thinking + signature, Chinese/emoji text, TWO
/// tool_use blocks, a `ping` heartbeat and the full event-field vocabulary.
fn anthropic_c01_frames() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({"type":"message_start","message":{"id":"msg_c01","model":"m","usage":{"input_tokens":41}}}),
        serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"先想一下"}}),
        serde_json::json!({"type":"ping"}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"🤔 方案"}}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig-C01-签名"}}),
        serde_json::json!({"type":"content_block_stop","index":0}),
        serde_json::json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
        serde_json::json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"回答："}}),
        serde_json::json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"你好，世界 🌍"}}),
        serde_json::json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"！"}}),
        serde_json::json!({"type":"content_block_stop","index":1}),
        serde_json::json!({"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_c01_a","name":"read"}}),
        serde_json::json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}),
        serde_json::json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"\"note-一.txt\"}"}}),
        serde_json::json!({"type":"content_block_stop","index":2}),
        serde_json::json!({"type":"content_block_start","index":3,"content_block":{"type":"tool_use","id":"toolu_c01_b","name":"write"}}),
        serde_json::json!({"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"o.txt\",\"content\":\"x\"}"}}),
        serde_json::json!({"type":"content_block_stop","index":3}),
        serde_json::json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":17}}),
        serde_json::json!({"type":"message_stop"}),
    ]
}

/// The openai C01/C08 fixture: reasoning + text interleaved, TWO tool calls
/// whose fragments interleave at the wire level, a usage frame, [DONE].
fn openai_c01_frames() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({"id":"chatcmpl-c01","choices":[{"index":0,"delta":{"role":"assistant","reasoning_content":"先推理"},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-c01","choices":[{"index":0,"delta":{"content":"正文一 🚀"},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-c01","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"name":"read","arguments":"{\"path\":"}}]},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-c01","choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"id":"call_b","type":"function","function":{"name":"write","arguments":"{\"path\":\"o.txt\","}}]},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-c01","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"note-二.txt\"}"}}]},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-c01","choices":[{"index":0,"delta":{"reasoning_content":"再推理 🧠"},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-c01","choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"function":{"arguments":"\"content\":\"y\"}"}}]},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-c01","choices":[{"index":0,"delta":{"content":"正文二"},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-c01","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
        serde_json::json!({"id":"chatcmpl-c01","choices":[],"usage":{"prompt_tokens":29,"completion_tokens":13}}),
    ]
}

/// Renders anthropic frames (each carries its `event:` field, as the real
/// service sends them).
fn anthropic_body(frames: &[serde_json::Value]) -> String {
    let mut body = String::new();
    for frame in frames {
        let ty = frame["type"].as_str().expect("typed frame");
        body.push_str(&format!("event: {ty}\ndata: {frame}\n\n"));
    }
    body
}

/// Renders openai frames (`data:` lines + the [DONE] sentinel).
fn openai_body(frames: &[serde_json::Value]) -> String {
    let mut body = String::new();
    for frame in frames {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    body
}

// ── the fragmentation harness ────────────────────────────────────────────────

/// A deterministic fixed-seed PRNG (xorshift64*) — the C01 random-cut source.
/// The seed is pinned in the test name; a failure reproduces byte-exact.
struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

/// One accumulated stream outcome: the live delta sequence and the closed
/// batch (turn + usage).
struct StreamOutcome {
    deltas: Vec<ModelTurnDelta>,
    turn: ProviderTurn,
    usage: Option<UsageRecord>,
}

/// Decodes `body` under one fragmentation and drives the events through the
/// accumulator, returning the full outcome. Any decode/accumulate error is a
/// test failure with the fragmentation named — C01 equivalence means the
/// fragmentation NEVER changes the outcome.
fn decode_fragmented(body: &[u8], cuts: &[usize], label: &str) -> Vec<SseEvent> {
    let mut decoder = SseDecoder::new();
    let mut events = Vec::new();
    let mut at = 0;
    for cut in cuts.iter().copied().chain(std::iter::once(body.len())) {
        assert!(at <= cut && cut <= body.len(), "{label}: cut order");
        let chunk = &body[at..cut];
        at = cut;
        let produced = decoder
            .feed(chunk)
            .unwrap_or_else(|err| panic!("{label}: decode failed at byte {cut}: {err}"));
        events.extend(produced);
    }
    let finish = decoder
        .finish()
        .unwrap_or_else(|err| panic!("{label}: finish failed: {err}"));
    assert!(
        !finish.discarded_partial,
        "{label}: a complete fixture never discards a partial frame"
    );
    events.extend(finish.events);
    events
}

fn accumulate_anthropic(events: &[SseEvent], label: &str) -> StreamOutcome {
    let mut accumulator = MessagesStreamAccumulator::new();
    let mut deltas = Vec::new();
    for event in events {
        let emitted = accumulator
            .handle_event(event)
            .unwrap_or_else(|err| panic!("{label}: accumulate failed: {err}"));
        deltas.extend(emitted);
    }
    let parsed = accumulator
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .unwrap_or_else(|err| panic!("{label}: close failed: {err}"));
    let usage = parsed.usage();
    StreamOutcome {
        deltas,
        turn: parsed.turn,
        usage,
    }
}

fn accumulate_openai(events: &[SseEvent], label: &str) -> StreamOutcome {
    let mut accumulator = ChatStreamAccumulator::new();
    let mut deltas = Vec::new();
    for event in events {
        let emitted = accumulator
            .handle_event(event)
            .unwrap_or_else(|err| panic!("{label}: accumulate failed: {err}"));
        deltas.extend(emitted);
    }
    let parsed = accumulator
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .unwrap_or_else(|err| panic!("{label}: close failed: {err}"));
    let usage = parsed.usage();
    StreamOutcome {
        deltas,
        turn: parsed.turn,
        usage,
    }
}

/// The C01 cut-point enumeration: byte-by-byte, every two-way cut, and
/// fixed-seed random multi-cuts. `run` decodes + accumulates + closes.
fn for_each_fragmentation(
    body: &[u8],
    run: impl Fn(&[usize], &str) -> StreamOutcome,
) -> StreamOutcome {
    // Reference: the one-shot decode (the unfragmented truth).
    let reference = run(&[body.len()], "one-shot");
    // Byte-by-byte.
    let cuts: Vec<usize> = (1..body.len()).collect();
    let outcome = run(&cuts, "byte-by-byte");
    assert_outcome_eq(&reference, &outcome, "byte-by-byte");
    // Every two-way cut point (the exhaustive critical-cut space).
    for cut in 1..body.len() {
        let outcome = run(&[cut], &format!("cut@{cut}"));
        assert_outcome_eq(&reference, &outcome, &format!("cut@{cut}"));
    }
    // Fixed-seed random multi-cuts (chunk sizes 1..=41 bytes).
    let mut rng = XorShift(0xC01C_01C0_1C01_C01C);
    for round in 0..200 {
        let mut cuts = Vec::new();
        let mut at = 0;
        while at < body.len() {
            at += 1 + (rng.next() % 41) as usize;
            cuts.push(at.min(body.len()));
        }
        let label = format!("random-seed-C01C-round{round}");
        let outcome = run(&cuts, &label);
        assert_outcome_eq(&reference, &outcome, &label);
    }
    reference
}

fn assert_outcome_eq(reference: &StreamOutcome, other: &StreamOutcome, label: &str) {
    assert_eq!(
        reference.deltas, other.deltas,
        "{label}: live deltas diverge"
    );
    assert_eq!(reference.turn, other.turn, "{label}: closed turn diverges");
    assert_eq!(reference.usage, other.usage, "{label}: usage diverges");
}

// ── C01: arbitrary byte fragmentation equivalence ────────────────────────────

#[test]
fn c01_anthropic_full_response_is_fragmentation_invariant() {
    let body = anthropic_body(&anthropic_c01_frames());
    let reference = for_each_fragmentation(body.as_bytes(), |cuts, label| {
        let events = decode_fragmented(body.as_bytes(), cuts, label);
        accumulate_anthropic(&events, label)
    });
    // The semantic pins (the fixture is Chinese + emoji + signature + two
    // interleaved tool calls): the live stream is ONLY text/reasoning —
    // tool fragments are batch state and never live progress (so nothing
    // executable exists before the batch closes; the closed batch is
    // validated once, below).
    assert!(
        reference
            .deltas
            .iter()
            .all(|d| matches!(d, ModelTurnDelta::Text(_) | ModelTurnDelta::Reasoning(_))),
        "live deltas carry text/reasoning only: {:?}",
        reference.deltas
    );
    // The signature never leaks into a live delta (C14).
    assert!(
        reference.deltas.iter().all(|d| match d {
            ModelTurnDelta::Text(t) | ModelTurnDelta::Reasoning(t) => !t.contains("sig-C01"),
        }),
        "no signature bytes in the live stream"
    );
    let (requests, content) = match &reference.turn {
        ProviderTurn::ToolRequests { requests, content } => (requests, content),
        other => panic!("expected ToolRequests, got {other:?}"),
    };
    assert_eq!(
        requests.len(),
        2,
        "both tool calls materialize exactly once"
    );
    assert_eq!(requests[0].provider_call_id.as_deref(), Some("toolu_c01_a"));
    assert_eq!(requests[0].target, "tool:first-party:read");
    assert_eq!(
        requests[0].arguments.as_value(),
        &serde_json::json!({"path": "note-一.txt"})
    );
    assert_eq!(requests[1].provider_call_id.as_deref(), Some("toolu_c01_b"));
    assert_eq!(
        requests[1].arguments.as_value(),
        &serde_json::json!({"path": "o.txt", "content": "x"})
    );
    // Content order: reasoning, its signature (opaque state), the text.
    assert!(
        matches!(&content[0], ContentBlock::Reasoning { text } if text == "先想一下🤔 方案"),
        "reasoning block: {content:?}"
    );
    assert!(
        matches!(&content[1], ContentBlock::Opaque { data, .. } if data["type"] == "thinking_signature" && data["signature"] == "sig-C01-签名"),
        "the signature is opaque state, never text: {content:?}"
    );
    assert!(
        matches!(&content[2], ContentBlock::Text { text } if text == "回答：你好，世界 🌍！"),
        "text block: {content:?}"
    );
    assert_eq!(
        reference.usage,
        Some(UsageRecord {
            input_tokens: 41,
            output_tokens: 17
        })
    );
}

#[test]
fn c01_openai_full_response_is_fragmentation_invariant() {
    let body = openai_body(&openai_c01_frames());
    let reference = for_each_fragmentation(body.as_bytes(), |cuts, label| {
        let events = decode_fragmented(body.as_bytes(), cuts, label);
        accumulate_openai(&events, label)
    });
    assert!(reference
        .deltas
        .iter()
        .all(|d| matches!(d, ModelTurnDelta::Text(_) | ModelTurnDelta::Reasoning(_))));
    let (requests, content) = match &reference.turn {
        ProviderTurn::ToolRequests { requests, content } => (requests, content),
        other => panic!("expected ToolRequests, got {other:?}"),
    };
    // C08: the two interleaved calls accumulated independently — index 0's
    // arguments never bled into index 1's.
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].provider_call_id.as_deref(), Some("call_a"));
    assert_eq!(
        requests[0].arguments.as_value(),
        &serde_json::json!({"path": "note-二.txt"})
    );
    assert_eq!(requests[1].provider_call_id.as_deref(), Some("call_b"));
    assert_eq!(
        requests[1].arguments.as_value(),
        &serde_json::json!({"path": "o.txt", "content": "y"})
    );
    // Arrival order preserved across the interleave: reasoning, text,
    // reasoning, text.
    let kinds: Vec<&str> = content
        .iter()
        .map(|block| match block {
            ContentBlock::Text { .. } => "text",
            ContentBlock::Reasoning { .. } => "reasoning",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, vec!["reasoning", "text", "reasoning", "text"]);
    assert!(matches!(&content[0], ContentBlock::Reasoning { text } if text == "先推理"),);
    assert!(matches!(&content[2], ContentBlock::Reasoning { text } if text == "再推理 🧠"));
    assert_eq!(
        reference.usage,
        Some(UsageRecord {
            input_tokens: 29,
            output_tokens: 13
        })
    );
}

#[test]
fn c01_split_bom_and_crlf_are_fragmentation_invariant() {
    // A BOM split across chunks plus CRLF line endings and a comment
    // heartbeat line (the C03 shapes) inside the C01 harness.
    let mut body = vec![0xEF, 0xBB, 0xBF];
    body.extend_from_slice(b": heartbeat\r\n\r\n");
    body.extend_from_slice(
        openai_body(&openai_c01_frames())
            .replace('\n', "\r\n")
            .as_bytes(),
    );
    let reference = for_each_fragmentation(&body, |cuts, label| {
        let events = decode_fragmented(&body, cuts, label);
        accumulate_openai(&events, label)
    });
    assert!(matches!(reference.turn, ProviderTurn::ToolRequests { .. }));
}

// ── C02: invalid UTF-8 is one deterministic loud error ───────────────────────

#[test]
fn c02_invalid_utf8_is_deterministic_at_every_fragmentation() {
    // An invalid byte inside a data line; the multi-byte character before it
    // is split across feeds in some fragmentations.
    let mut body = String::new();
    body.push_str("data: {\"ok\":\"好的 🌍\"}\n\n");
    body.push_str("data: {\"bad\":\"");
    let mut bytes = body.into_bytes();
    bytes.push(0xFF); // never valid UTF-8
    bytes.extend_from_slice(b"\"}\n\ndata: [DONE]\n\n");

    // Reference: the one-shot decode of the corrupt stream fails loudly.
    let mut decoder = SseDecoder::new();
    let err = decoder
        .feed(&bytes)
        .expect_err("the corrupt stream must fail");
    assert_eq!(err.code, ErrorCode::InvalidMessage);
    assert!(err.message.contains("UTF-8"), "{err}");

    // EVERY fragmentation: byte-by-byte, every two-way cut, fixed-seed
    // random — the error is the same class at the same content position.
    let check = |cuts: &[usize], label: &str| {
        let mut decoder = SseDecoder::new();
        let mut events = Vec::new();
        let mut at = 0;
        let mut failed = None;
        for cut in cuts.iter().copied().chain(std::iter::once(bytes.len())) {
            let chunk = &bytes[at..cut];
            at = cut;
            match decoder.feed(chunk) {
                Ok(produced) => events.extend(produced),
                Err(err) => {
                    failed = Some(err);
                    break;
                }
            }
        }
        let err = failed.unwrap_or_else(|| panic!("{label}: the corrupt stream must fail"));
        assert_eq!(err.code, ErrorCode::InvalidMessage, "{label}");
        // The honest prefix rule: events delivered before the error are
        // always a PREFIX of the intact stream's events — never more,
        // never altered, never replacement characters. A feed whose chunk
        // contains BOTH a completable frame and the corrupt line refuses
        // the whole feed (the completed frame of that feed drops WITH the
        // loud error — the stream is refused as a whole, nothing is
        // silently half-consumed); finer fragmentations deliver the intact
        // frame in its own earlier feed. Both shapes are deterministic
        // functions of the byte stream.
        assert!(
            events.len() <= 1,
            "{label}: at most the intact frame can precede the error"
        );
        for event in &events {
            assert!(
                event.data.contains("好的 🌍"),
                "{label}: the intact frame is whole and unaltered"
            );
        }
    };
    check(&(1..bytes.len()).collect::<Vec<_>>(), "byte-by-byte");
    for cut in 1..bytes.len() {
        check(&[cut], &format!("cut@{cut}"));
    }
    let mut rng = XorShift(0xC02C_02C0_2C02_C02C);
    for round in 0..100 {
        let mut cuts = Vec::new();
        let mut at = 0;
        while at < bytes.len() {
            at += 1 + (rng.next() % 29) as usize;
            cuts.push(at.min(bytes.len()));
        }
        check(&cuts, &format!("random-round{round}"));
    }
}

// ── C05: half-JSON arguments never execute ───────────────────────────────────

#[test]
fn c05_openai_half_json_arguments_close_loud_with_zero_dispatch() {
    // The arguments stop mid-string; the provider then terminates the
    // stream "cleanly" with [DONE]. The batch must refuse, not guess.
    let body = openai_body(&[
        serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_h","type":"function","function":{"name":"read","arguments":"{\"path\":\"note"}}]},"finish_reason":null}]}),
        serde_json::json!({"id":"x","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
    ]);
    let events = decode_complete_body(&body).expect("decode");
    let mut accumulator = ChatStreamAccumulator::new();
    let mut deltas = Vec::new();
    for event in &events {
        deltas.extend(accumulator.handle_event(event).expect("accumulate"));
    }
    let err = accumulator
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .expect_err("half-JSON arguments are a loud refusal, never a brace-guess");
    assert_eq!(err.code, ErrorCode::InvalidMessage);
    assert!(err.message.contains("not JSON"), "{err}");
    assert!(
        deltas.is_empty(),
        "no live deltas and no tool batch: zero dispatch ({deltas:?})"
    );
}

#[test]
fn c05_anthropic_half_json_arguments_close_loud_with_zero_dispatch() {
    let body = anthropic_body(&[
        serde_json::json!({"type":"message_start","message":{"usage":{"input_tokens":5}}}),
        serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_h","name":"read"}}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"note"}}),
        serde_json::json!({"type":"content_block_stop","index":0}),
        serde_json::json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":3}}),
        serde_json::json!({"type":"message_stop"}),
    ]);
    let events = decode_complete_body(&body).expect("decode");
    let mut accumulator = MessagesStreamAccumulator::new();
    for event in &events {
        accumulator.handle_event(event).expect("accumulate");
    }
    let err = accumulator
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .expect_err("half-JSON input is a loud refusal, never an empty-object placeholder");
    assert_eq!(err.code, ErrorCode::InvalidMessage);
    assert!(err.message.contains("not complete JSON"), "{err}");
}

// ── C06: parseable JSON is not call completion ───────────────────────────────

#[test]
fn c06_parseable_arguments_admit_nothing_before_the_protocol_terminal() {
    // Call A's arguments are complete and parseable from the third frame on;
    // the stream then continues (call B, more text) before the terminal.
    // Nothing executable may exist at the parseable-but-open point.
    // (R05 RR1 F12: the terminal is the pair finish_reason + [DONE] — the
    // fixture now carries the legal `tool_calls` finish frame.)
    let frames = vec![
        serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"name":"read","arguments":"{\"path\":\"a.txt\"}"}}]},"finish_reason":null}]}),
        serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"content":"中间的文本"},"finish_reason":null}]}),
        serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"id":"call_b","type":"function","function":{"name":"write","arguments":"{\"path\":\"b.txt\",\"content\":\"v\"}"}}]},"finish_reason":null}]}),
        serde_json::json!({"id":"x","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
    ];
    let body = openai_body(&frames);
    let events = decode_complete_body(&body).expect("decode");
    let mut accumulator = ChatStreamAccumulator::new();
    let mut deltas = Vec::new();
    for event in &events {
        deltas.extend(accumulator.handle_event(event).expect("accumulate"));
    }
    // The parseable window: call A's arguments already parse — but the ONLY
    // admission surface is the closed batch, and the batch is not closed.
    assert!(
        deltas.iter().all(|d| matches!(d, ModelTurnDelta::Text(_))),
        "only the text delta was live; tool state never leaves the accumulator"
    );
    // An early close is loud (the terminal marker is missing) — the
    // parseable prefix is NOT a dispatchable call.
    let early = ChatStreamAccumulator::new();
    let err = early
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .expect_err("a stream without [DONE] is truncated");
    assert_eq!(err.code, ErrorCode::InvalidMessage);
    assert!(err.message.contains("[DONE]"), "{err}");
    // The closed batch admits BOTH calls exactly once.
    let parsed = accumulator
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .expect("closed batch");
    let (requests, _) = match parsed.turn {
        ProviderTurn::ToolRequests { requests, content } => (requests, content),
        other => panic!("expected ToolRequests, got {other:?}"),
    };
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].provider_call_id.as_deref(), Some("call_a"));
    assert_eq!(requests[1].provider_call_id.as_deref(), Some("call_b"));
}

// ── C07: argument types and canonical honesty ────────────────────────────────

#[test]
fn c07_wrong_shaped_arguments_refuse_loudly() {
    // The arguments string parses to a non-object JSON value: the canonical
    // invariant check refuses (never a guessed default, never an empty
    // object placeholder).
    for raw in ["null", "[1,2]", "42", "true"] {
        let body = openai_body(&[
            serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_w","type":"function","function":{"name":"read","arguments":raw}}]},"finish_reason":null}]}),
            serde_json::json!({"id":"x","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
        ]);
        let events = decode_complete_body(&body).expect("decode");
        let mut accumulator = ChatStreamAccumulator::new();
        for event in &events {
            accumulator.handle_event(event).expect("accumulate");
        }
        let err = accumulator
            .finish(&call(), &snapshot(), &SchemaBudget::default())
            .expect_err("a non-object argument payload is a loud refusal");
        assert_eq!(err.code, ErrorCode::InvalidMessage, "payload {raw}");
        assert!(err.message.contains("invariants"), "{err}");
    }
    // A wire-level non-string arguments field (an array VALUE in the chunk)
    // is equally loud at the close — never silently re-typed.
    let body = openai_body(&[
        serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_w","type":"function","function":{"name":"read","arguments":[1,2]}}]},"finish_reason":null}]}),
        serde_json::json!({"id":"x","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
    ]);
    let events = decode_complete_body(&body).expect("decode");
    let mut accumulator = ChatStreamAccumulator::new();
    for event in &events {
        accumulator.handle_event(event).expect("accumulate");
    }
    let err = accumulator
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .expect_err("a non-string wire arguments field is a loud refusal");
    assert_eq!(err.code, ErrorCode::InvalidMessage);
}

#[test]
fn c07_duplicate_keys_and_overprecision_numbers_follow_one_canonical_rule() {
    // Duplicate keys: serde_json's deterministic last-wins; the digest
    // covers EXACTLY the value the executor receives — the approval chain
    // can never summarize a different value than the one bound by the
    // digest (no silent type coercion either: the canonical bytes below are
    // the digest's input).
    let body = openai_body(&[
        serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_d","type":"function","function":{"name":"read","arguments":"{\"path\":\"first.txt\",\"path\":\"second.txt\",\"n\":9007199254740991}"}}]},"finish_reason":null}]}),
        serde_json::json!({"id":"x","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
    ]);
    let events = decode_complete_body(&body).expect("decode");
    let mut accumulator = ChatStreamAccumulator::new();
    for event in &events {
        accumulator.handle_event(event).expect("accumulate");
    }
    let parsed = accumulator
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .expect("canonical-rule arguments admit");
    let (requests, _) = match parsed.turn {
        ProviderTurn::ToolRequests { requests, content } => (requests, content),
        other => panic!("expected ToolRequests, got {other:?}"),
    };
    let request = &requests[0];
    // The effective value IS the last-wins value…
    let expected = serde_json::json!({"path": "second.txt", "n": 9007199254740991_i64});
    assert_eq!(request.arguments.as_value(), &expected);
    // …and the digest covers its canonical bytes — the same bytes any
    // approval summary is derived from (no distortion between what is
    // approved and what executes).
    let canonical = lingxi_protocol::canon::canonical_json_bytes(&expected);
    assert_eq!(request.arguments.canonical_bytes(), canonical.as_slice());
    assert!(request.digest_matches_arguments());

    // Over-precision / non-integral numbers: the frozen canonical rule is
    // LOUD REFUSAL (the safe-integer boundary — the TS canonical consumer
    // throws on floats, parity is mandatory). Never a silent f64 rounding
    // that would distort the digest.
    for raw in [
        "{\"path\":\"a.txt\",\"n\":1.0000000000000000000001}",
        "{\"path\":\"a.txt\",\"n\":1.5}",
        "{\"path\":\"a.txt\",\"n\":9007199254740993}",
    ] {
        let body = openai_body(&[
            serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_f","type":"function","function":{"name":"read","arguments":raw}}]},"finish_reason":null}]}),
            serde_json::json!({"id":"x","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
        ]);
        let events = decode_complete_body(&body).expect("decode");
        let mut accumulator = ChatStreamAccumulator::new();
        for event in &events {
            accumulator.handle_event(event).expect("accumulate");
        }
        let err = accumulator
            .finish(&call(), &snapshot(), &SchemaBudget::default())
            .expect_err("a float / out-of-safe-range integer refuses loudly");
        assert_eq!(err.code, ErrorCode::InvalidMessage, "payload {raw}");
    }
}

// ── C08: interleaved tool fragments never cross-contaminate ──────────────────

#[test]
fn c08_anthropic_interleaved_tool_blocks_accumulate_independently() {
    // Two tool_use blocks whose input fragments strictly alternate.
    let body = anthropic_body(&[
        serde_json::json!({"type":"message_start","message":{"usage":{"input_tokens":9}}}),
        serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_i0","name":"read"}}),
        serde_json::json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_i1","name":"write"}}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"pa"}}),
        serde_json::json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"x.txt\",\"content\":"}}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"th\":\"a.txt\"}"}}),
        serde_json::json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"v\"}"}}),
        serde_json::json!({"type":"content_block_stop","index":0}),
        serde_json::json!({"type":"content_block_stop","index":1}),
        serde_json::json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":4}}),
        serde_json::json!({"type":"message_stop"}),
    ]);
    let events = decode_complete_body(&body).expect("decode");
    let mut accumulator = MessagesStreamAccumulator::new();
    for event in &events {
        assert!(
            accumulator
                .handle_event(event)
                .expect("accumulate")
                .is_empty(),
            "tool fragments are never live deltas"
        );
    }
    let parsed = accumulator
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .expect("closed batch");
    let (requests, _) = match parsed.turn {
        ProviderTurn::ToolRequests { requests, content } => (requests, content),
        other => panic!("expected ToolRequests, got {other:?}"),
    };
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].provider_call_id.as_deref(), Some("toolu_i0"));
    assert_eq!(
        requests[0].arguments.as_value(),
        &serde_json::json!({"path": "a.txt"}),
        "block 0's fragments assembled under block 0 only"
    );
    assert_eq!(requests[1].provider_call_id.as_deref(), Some("toolu_i1"));
    assert_eq!(
        requests[1].arguments.as_value(),
        &serde_json::json!({"path": "x.txt", "content": "v"})
    );
}

// ── C09: a truncated batch is zero-side-effect ───────────────────────────────

#[test]
fn c09_openai_length_stop_with_one_complete_tool_dispatches_nothing() {
    // Tool call A is COMPLETE; the stream then ends with finish_reason
    // "length" (as if a second call was truncated). The whole turn
    // classifies as a retryable budget failure — the complete call never
    // dispatches (batch admission is all-or-nothing per closed turn).
    let body = openai_body(&[
        serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_done","type":"function","function":{"name":"read","arguments":"{\"path\":\"a.txt\"}"}}]},"finish_reason":null}]}),
        serde_json::json!({"id":"x","choices":[{"index":0,"delta":{},"finish_reason":"length"}]}),
        serde_json::json!({"id":"x","choices":[],"usage":{"prompt_tokens":11,"completion_tokens":33}}),
    ]);
    let events = decode_complete_body(&body).expect("decode");
    let mut accumulator = ChatStreamAccumulator::new();
    for event in &events {
        accumulator.handle_event(event).expect("accumulate");
    }
    let parsed = accumulator
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .expect("a length stop is a classified failure, not a parse error");
    let usage = parsed.usage();
    match parsed.turn {
        ProviderTurn::Failed { error, retryable } => {
            assert_eq!(error.code, ErrorCode::BudgetExceeded);
            assert!(retryable, "a length truncation retries on a new attempt");
            assert!(error.message.contains("length"), "{error}");
        }
        other => panic!("expected Failed(length), got {other:?}"),
    }
    // The usage the provider DID report rides the failure (honest cost).
    assert_eq!(
        usage,
        Some(UsageRecord {
            input_tokens: 11,
            output_tokens: 33
        })
    );
}

#[test]
fn c09_anthropic_max_tokens_stop_with_one_complete_tool_dispatches_nothing() {
    let body = anthropic_body(&[
        serde_json::json!({"type":"message_start","message":{"usage":{"input_tokens":19}}}),
        serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_t","name":"read"}}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"a.txt\"}"}}),
        serde_json::json!({"type":"content_block_stop","index":0}),
        serde_json::json!({"type":"message_delta","delta":{"stop_reason":"max_tokens"},"usage":{"output_tokens":51}}),
        serde_json::json!({"type":"message_stop"}),
    ]);
    let events = decode_complete_body(&body).expect("decode");
    let mut accumulator = MessagesStreamAccumulator::new();
    for event in &events {
        accumulator.handle_event(event).expect("accumulate");
    }
    let parsed = accumulator
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .expect("a max_tokens stop is a classified failure");
    let usage = parsed.usage();
    match parsed.turn {
        ProviderTurn::Failed { error, retryable } => {
            assert_eq!(error.code, ErrorCode::BudgetExceeded);
            assert!(retryable);
            assert!(error.message.contains("max_tokens"), "{error}");
        }
        other => panic!("expected Failed(max_tokens), got {other:?}"),
    }
    assert_eq!(
        usage,
        Some(UsageRecord {
            input_tokens: 19,
            output_tokens: 51
        })
    );
}

// ── C10: duplicate and conflict events are distinguishable ───────────────────

#[test]
fn c10_openai_duplicates_conflicts_and_adjacent_identical_text() {
    // Adjacent identical text deltas: both are delivered and merged — the
    // content length is the ONLY dedup witness (no content-based dropping).
    let body = openai_body(&[
        serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"role":"assistant","content":"same "},"finish_reason":null}]}),
        serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"content":"same "},"finish_reason":null}]}),
        serde_json::json!({"id":"x","choices":[],"usage":{"prompt_tokens":1,"completion_tokens":2}}),
        // An IDENTICAL usage re-send: tolerated (no new information).
        serde_json::json!({"id":"x","choices":[],"usage":{"prompt_tokens":1,"completion_tokens":2}}),
        serde_json::json!({"id":"x","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
    ]);
    let events = decode_complete_body(&body).expect("decode");
    let mut accumulator = ChatStreamAccumulator::new();
    let mut deltas = Vec::new();
    for event in &events {
        deltas.extend(accumulator.handle_event(event).expect("accumulate"));
    }
    assert_eq!(
        deltas,
        vec![
            ModelTurnDelta::Text("same ".to_string()),
            ModelTurnDelta::Text("same ".to_string())
        ],
        "identical adjacent deltas are never deduplicated"
    );
    let parsed = accumulator
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .expect("closed");
    match parsed.turn {
        ProviderTurn::Final { message } => {
            assert_eq!(
                message.content,
                vec![ContentBlock::Text {
                    text: "same same ".to_string()
                }]
            );
        }
        other => panic!("expected Final, got {other:?}"),
    }

    // An event AFTER [DONE] is loud.
    let mut accumulator = ChatStreamAccumulator::new();
    let done = SseEvent {
        event: None,
        data: "[DONE]".to_string(),
    };
    accumulator.handle_event(&done).expect("[DONE]");
    let late = SseEvent {
        event: None,
        data: serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"content":"late"},"finish_reason":null}]}).to_string(),
    };
    let err = accumulator
        .handle_event(&late)
        .expect_err("post-[DONE] event");
    assert_eq!(err.code, ErrorCode::InvalidMessage);

    // Two DIFFERENT usage objects: a conflict, never merged.
    let mut accumulator = ChatStreamAccumulator::new();
    let first = SseEvent {
        event: None,
        data: serde_json::json!({"id":"x","choices":[],"usage":{"prompt_tokens":1,"completion_tokens":2}}).to_string(),
    };
    let second = SseEvent {
        event: None,
        data: serde_json::json!({"id":"x","choices":[],"usage":{"prompt_tokens":9,"completion_tokens":9}}).to_string(),
    };
    accumulator.handle_event(&first).expect("first usage");
    let err = accumulator
        .handle_event(&second)
        .expect_err("conflicting usage is loud");
    assert_eq!(err.code, ErrorCode::InvalidMessage);

    // Two DIFFERENT ids for one tool index: a conflict, never merged.
    let body = openai_body(&[
        serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_one","type":"function","function":{"name":"read","arguments":""}}]},"finish_reason":null}]}),
        serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_TWO","function":{"arguments":"{}"}}]},"finish_reason":null}]}),
    ]);
    let events = decode_complete_body(&body).expect("decode");
    let mut accumulator = ChatStreamAccumulator::new();
    accumulator.handle_event(&events[0]).expect("first id");
    let err = accumulator
        .handle_event(&events[1])
        .expect_err("conflicting ids are loud");
    assert_eq!(err.code, ErrorCode::InvalidMessage);
    assert!(err.message.contains("DIFFERENT ids"), "{err}");
}

#[test]
fn c10_anthropic_duplicates_conflicts_and_event_field_agreement() {
    // The SSE event field and the frame type must agree.
    let mut accumulator = MessagesStreamAccumulator::new();
    let mismatched = SseEvent {
        event: Some("message_start".to_string()),
        data: serde_json::json!({"type":"content_block_stop","index":0}).to_string(),
    };
    let err = accumulator
        .handle_event(&mismatched)
        .expect_err("event/type disagreement is loud");
    assert_eq!(err.code, ErrorCode::InvalidMessage);
    assert!(err.message.contains("disagrees"), "{err}");

    // A duplicate content_block_start on one index is loud…
    let body = anthropic_body(&[
        serde_json::json!({"type":"message_start","message":{"usage":{"input_tokens":1}}}),
        serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"a"}}),
        serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"b"}}),
    ]);
    let events = decode_complete_body(&body).expect("decode");
    let mut accumulator = MessagesStreamAccumulator::new();
    accumulator.handle_event(&events[0]).expect("message_start");
    accumulator.handle_event(&events[1]).expect("first start");
    let err = accumulator
        .handle_event(&events[2])
        .expect_err("duplicate block start is loud");
    assert_eq!(err.code, ErrorCode::InvalidMessage);
    assert!(err.message.contains("reopens"), "{err}");

    // …but an IDENTICAL signature re-send is tolerated (no new information),
    // while a CONFLICTING signature is loud.
    let body = anthropic_body(&[
        serde_json::json!({"type":"message_start","message":{"usage":{"input_tokens":1}}}),
        serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"t"}}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig-x"}}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig-x"}}),
        serde_json::json!({"type":"content_block_stop","index":0}),
        serde_json::json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}),
        serde_json::json!({"type":"message_stop"}),
    ]);
    let events = decode_complete_body(&body).expect("decode");
    let mut accumulator = MessagesStreamAccumulator::new();
    for event in &events {
        accumulator
            .handle_event(event)
            .expect("identical re-sends are tolerated");
    }
    accumulator
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .expect("closed");

    let body = anthropic_body(&[
        serde_json::json!({"type":"message_start","message":{"usage":{"input_tokens":1}}}),
        serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"t"}}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig-x"}}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig-DIFFERENT"}}),
    ]);
    let events = decode_complete_body(&body).expect("decode");
    let mut accumulator = MessagesStreamAccumulator::new();
    for event in &events[..3] {
        accumulator.handle_event(event).expect("prefix");
    }
    let err = accumulator
        .handle_event(&events[3])
        .expect_err("conflicting signatures are loud");
    assert_eq!(err.code, ErrorCode::InvalidMessage);

    // An event after message_stop is loud.
    let mut accumulator = MessagesStreamAccumulator::new();
    let stop = SseEvent {
        event: Some("message_stop".to_string()),
        data: serde_json::json!({"type":"message_stop"}).to_string(),
    };
    accumulator.handle_event(&stop).expect("message_stop");
    let err = accumulator
        .handle_event(&stop)
        .expect_err("post-message_stop event");
    assert_eq!(err.code, ErrorCode::InvalidMessage);
}

// ── C11: the stop-reason table over the closed-batch parsers ─────────────────

#[test]
fn c11_stop_reason_table_is_complete_across_the_fragmenting_families() {
    let budget = SchemaBudget::default();
    // openai: stop → Final; tool_calls → ToolRequests; length → retryable
    // budget failure; content_filter → non-retryable refusal; EOF without
    // [DONE] → loud InvalidMessage (a socket close is never a success).
    let openai = |finish_reason: Option<&str>| {
        let frames = match finish_reason {
            Some("tool_calls") => vec![
                serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"c","type":"function","function":{"name":"read","arguments":"{\"path\":\"a\"}"}}]},"finish_reason":null}]}),
                serde_json::json!({"id":"x","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
            ],
            Some(reason) => vec![
                serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"role":"assistant","content":"t"},"finish_reason":null}]}),
                serde_json::json!({"id":"x","choices":[{"index":0,"delta":{},"finish_reason":reason}]}),
            ],
            None => vec![
                serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"role":"assistant","content":"t"},"finish_reason":null}]}),
            ],
        };
        let mut body = String::new();
        for frame in &frames {
            body.push_str(&format!("data: {frame}\n\n"));
        }
        if finish_reason.is_some() {
            body.push_str("data: [DONE]\n\n");
        }
        let events = decode_complete_body(&body).expect("decode");
        let mut accumulator = ChatStreamAccumulator::new();
        for event in &events {
            accumulator.handle_event(event).expect("accumulate");
        }
        accumulator.finish(&call(), &snapshot(), &budget)
    };
    assert!(matches!(
        openai(Some("stop")).expect("stop").turn,
        ProviderTurn::Final { .. }
    ));
    assert!(matches!(
        openai(Some("tool_calls")).expect("tools").turn,
        ProviderTurn::ToolRequests { .. }
    ));
    match openai(Some("length")).expect("length classified").turn {
        ProviderTurn::Failed { error, retryable } => {
            assert_eq!(error.code, ErrorCode::BudgetExceeded);
            assert!(retryable);
        }
        other => panic!("{other:?}"),
    }
    match openai(Some("content_filter"))
        .expect("filter classified")
        .turn
    {
        ProviderTurn::Failed { error, retryable } => {
            assert_eq!(error.code, ErrorCode::Forbidden);
            assert!(!retryable);
        }
        other => panic!("{other:?}"),
    }
    let err = openai(None).expect_err("EOF without [DONE] is loud");
    assert_eq!(err.code, ErrorCode::InvalidMessage);

    // anthropic: end_turn → Final; tool_use → ToolRequests; max_tokens →
    // retryable budget; refusal → non-retryable refusal; EOF without
    // message_stop → loud.
    let anthropic = |stop: Option<&str>| {
        let mut frames = vec![
            serde_json::json!({"type":"message_start","message":{"usage":{"input_tokens":3}}}),
            serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"t"}}),
            serde_json::json!({"type":"content_block_stop","index":0}),
        ];
        if let Some(reason) = stop {
            frames.push(serde_json::json!({"type":"message_delta","delta":{"stop_reason":reason},"usage":{"output_tokens":2}}));
            frames.push(serde_json::json!({"type":"message_stop"}));
        }
        let events = decode_complete_body(&anthropic_body(&frames)).expect("decode");
        let mut accumulator = MessagesStreamAccumulator::new();
        for event in &events {
            accumulator.handle_event(event).expect("accumulate");
        }
        accumulator.finish(&call(), &snapshot(), &budget)
    };
    assert!(matches!(
        anthropic(Some("end_turn")).expect("end_turn").turn,
        ProviderTurn::Final { .. }
    ));
    match anthropic(Some("max_tokens"))
        .expect("max_tokens classified")
        .turn
    {
        ProviderTurn::Failed { error, retryable } => {
            assert_eq!(error.code, ErrorCode::BudgetExceeded);
            assert!(retryable);
        }
        other => panic!("{other:?}"),
    }
    match anthropic(Some("refusal")).expect("refusal classified").turn {
        ProviderTurn::Failed { error, retryable } => {
            assert_eq!(error.code, ErrorCode::Forbidden);
            assert!(!retryable);
        }
        other => panic!("{other:?}"),
    }
    let err = anthropic(None).expect_err("EOF without message_stop is loud");
    assert_eq!(err.code, ErrorCode::InvalidMessage);

    // google: STOP → Final; MAX_TOKENS → retryable budget; SAFETY →
    // non-retryable refusal; promptFeedback.blockReason → non-retryable
    // refusal; EOF without a terminal frame → loud.
    let google = |terminal: Option<serde_json::Value>| {
        let mut body = String::new();
        body.push_str("data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"t\"}]}}]}\n\n");
        if let Some(frame) = terminal {
            body.push_str(&format!("data: {frame}\n\n"));
        }
        let events = decode_complete_body(&body).expect("decode");
        let mut accumulator = GenerateStreamAccumulator::new();
        for event in &events {
            accumulator.handle_event(event).expect("accumulate");
        }
        accumulator.finish(&call(), &snapshot(), &budget)
    };
    assert!(matches!(
        google(Some(serde_json::json!({"candidates":[{"content":{"role":"model","parts":[]},"finishReason":"STOP"}]})))
            .expect("STOP")
            .turn,
        ProviderTurn::Final { .. }
    ));
    match google(Some(serde_json::json!({"candidates":[{"content":{"role":"model","parts":[]},"finishReason":"MAX_TOKENS"}]})))
        .expect("MAX_TOKENS classified")
        .turn
    {
        ProviderTurn::Failed { error, retryable } => {
            assert_eq!(error.code, ErrorCode::BudgetExceeded);
            assert!(retryable);
        }
        other => panic!("{other:?}"),
    }
    match google(Some(serde_json::json!({"candidates":[{"content":{"role":"model","parts":[]},"finishReason":"SAFETY"}]})))
        .expect("SAFETY classified")
        .turn
    {
        ProviderTurn::Failed { error, retryable } => {
            assert_eq!(error.code, ErrorCode::Forbidden);
            assert!(!retryable);
        }
        other => panic!("{other:?}"),
    }
    match google(Some(
        serde_json::json!({"promptFeedback":{"blockReason":"SAFETY"}}),
    ))
    .expect("blockReason classified")
    .turn
    {
        ProviderTurn::Failed { error, retryable } => {
            assert_eq!(error.code, ErrorCode::Forbidden);
            assert!(!retryable);
        }
        other => panic!("{other:?}"),
    }
    let err = google(None).expect_err("EOF without a terminal frame is loud");
    assert_eq!(err.code, ErrorCode::InvalidMessage);
}

// ── C14: unknown blocks and signature isolation ──────────────────────────────

#[test]
fn c14_opaque_blocks_roundtrip_and_signatures_never_leak_into_deltas() {
    let body = anthropic_body(&[
        serde_json::json!({"type":"message_start","message":{"usage":{"input_tokens":8}}}),
        serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"推"}}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig-保密"}}),
        serde_json::json!({"type":"content_block_stop","index":0}),
        serde_json::json!({"type":"content_block_start","index":1,"content_block":{"type":"redacted_thinking","data":"encrypted-加密"}}),
        serde_json::json!({"type":"content_block_stop","index":1}),
        serde_json::json!({"type":"content_block_start","index":2,"content_block":{"type":"text","text":"正文"}}),
        serde_json::json!({"type":"content_block_stop","index":2}),
        serde_json::json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":4}}),
        serde_json::json!({"type":"message_stop"}),
    ]);
    let events = decode_complete_body(&body).expect("decode");
    let mut accumulator = MessagesStreamAccumulator::new();
    let mut deltas = Vec::new();
    for event in &events {
        deltas.extend(accumulator.handle_event(event).expect("accumulate"));
    }
    // The signature and the redacted block produced NO live deltas…
    assert_eq!(
        deltas,
        vec![
            ModelTurnDelta::Reasoning("推".to_string()),
            ModelTurnDelta::Text("正文".to_string())
        ],
        "signature/redacted bytes never leak into the live stream"
    );
    // …and the closed batch preserves the opaque state verbatim.
    let parsed = accumulator
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .expect("closed");
    let message = match parsed.turn {
        ProviderTurn::Final { message } => message,
        other => panic!("expected Final, got {other:?}"),
    };
    assert!(matches!(&message.content[0], ContentBlock::Reasoning { text } if text == "推"));
    assert!(
        matches!(&message.content[1], ContentBlock::Opaque { data, .. } if data["type"] == "thinking_signature" && data["signature"] == "sig-保密"),
        "the signature is opaque protocol state: {:?}",
        message.content
    );
    assert!(
        matches!(&message.content[2], ContentBlock::Opaque { data, .. } if data["type"] == "redacted_thinking" && data["data"] == "encrypted-加密"),
        "the redacted block is preserved verbatim: {:?}",
        message.content
    );

    // A DELTA for an opaque (complete-at-start) block is loud — the adapter
    // cannot know its semantics and never drops content silently.
    let body = anthropic_body(&[
        serde_json::json!({"type":"message_start","message":{"usage":{"input_tokens":1}}}),
        serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"redacted_thinking","data":"enc"}}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"?"}}),
    ]);
    let events = decode_complete_body(&body).expect("decode");
    let mut accumulator = MessagesStreamAccumulator::new();
    accumulator.handle_event(&events[0]).expect("message_start");
    accumulator.handle_event(&events[1]).expect("opaque start");
    let err = accumulator
        .handle_event(&events[2])
        .expect_err("a delta for an opaque block is loud");
    assert_eq!(err.code, ErrorCode::InvalidMessage);
    assert!(err.message.contains("opaque"), "{err}");

    // An unknown STREAM event type is loud (it may carry content).
    let mut accumulator = MessagesStreamAccumulator::new();
    let unknown = SseEvent {
        event: Some("content_block_juggle".to_string()),
        data: serde_json::json!({"type":"content_block_juggle"}).to_string(),
    };
    let err = accumulator
        .handle_event(&unknown)
        .expect_err("unknown stream event types are loud");
    assert_eq!(err.code, ErrorCode::InvalidMessage);
}

// ── C15 (decoder half): mid-frame EOF, non-JSON frames, sink closure ─────────

#[test]
fn c15_decoder_level_stream_failures_are_loud() {
    // A mid-frame EOF: the trailing unterminated frame is discarded and
    // reported (never half-parsed into a turn).
    let err = decode_complete_body("data: {\"complete\":true}\n\ndata: {\"truncat")
        .expect_err("unterminated trailing frame");
    assert_eq!(err.code, ErrorCode::InvalidMessage);
    assert!(err.message.contains("unterminated"), "{err}");

    // A non-JSON data frame in an openai stream is loud at the family layer.
    let events = decode_complete_body("data: not-json-at-all\n\ndata: [DONE]\n\n").expect("decode");
    let mut accumulator = ChatStreamAccumulator::new();
    let err = accumulator
        .handle_event(&events[0])
        .expect_err("non-JSON data frame is loud");
    assert_eq!(err.code, ErrorCode::InvalidMessage);

    // A closed sink abandons the read as non-retryable Cancelled (the
    // adapter never buffers "for later").
    struct ClosedSink;
    impl TurnDeltaSink for ClosedSink {
        fn emit<'a>(
            &'a self,
            _delta: ModelTurnDelta,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<(), TurnDeltaSinkClosed>> + Send + 'a>,
        > {
            Box::pin(async { Err(TurnDeltaSinkClosed) })
        }
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let sink = ClosedSink;
        let mut drive = FamilyStreamDrive {
            accumulator: ChatStreamAccumulator::new(),
            sink: &sink,
        };
        let event = SseEvent {
            event: None,
            data: serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"role":"assistant","content":"t"},"finish_reason":null}]}).to_string(),
        };
        let (error, retryable) = drive
            .handle(event)
            .await
            .expect_err("a closed sink stops the read");
        assert_eq!(error.code, ErrorCode::Cancelled);
        assert!(!retryable);
    });
}

// ── C04: buffer and parse cost stay bounded at the pre-registered limits ─────

#[test]
fn c04_bounds_are_loud_at_the_preregistered_limits() {
    use lingxi_adapters::models::streaming::{
        SSE_BUFFER_LIMIT, SSE_FRAME_MAX_BYTES, TOOL_ARGUMENTS_MAX_BYTES,
    };
    // Pin the R05_BASELINE pre-registered values so a silent loosening is
    // a test failure, not a drift (§8: the whole-stream total is
    // deliberately TIGHTER than the pre-registered 16 MiB ceiling).
    assert_eq!(SSE_FRAME_MAX_BYTES, 1024 * 1024);
    assert_eq!(TOOL_ARGUMENTS_MAX_BYTES, 1024 * 1024);
    assert_eq!(SSE_BUFFER_LIMIT, 8 * 1024 * 1024);

    // Leg 1 — the whole-stream undelivered buffer bound: one newline-less
    // line past the limit is refused loudly (the refusal travels to
    // drive_sse_stream, which abandons the read).
    let mut decoder = SseDecoder::new();
    let huge_line = vec![b'x'; SSE_BUFFER_LIMIT + 1];
    let error = decoder
        .feed(&huge_line)
        .expect_err("an over-limit unterminated line trips the buffer bound");
    assert_eq!(error.code, ErrorCode::InvalidMessage);
    assert!(error.message.contains("buffer bound"), "{}", error.message);
    assert!(!error.retryable);

    // Leg 2 — the per-frame bound: one frame whose joined data payload
    // exceeds the limit is refused loudly, never truncated mid-frame.
    let mut decoder = SseDecoder::new();
    let big_frame = format!("data: {}\n\n", "y".repeat(SSE_FRAME_MAX_BYTES + 1));
    let error = decoder
        .feed(big_frame.as_bytes())
        .expect_err("an over-bound frame trips the per-frame bound");
    assert_eq!(error.code, ErrorCode::InvalidMessage);

    // Leg 3 — the openai tool-arguments bound: fragments accumulating past
    // the limit are refused AT FEED TIME — zero dispatch, no silent
    // truncation of the parameters.
    let mut accumulator = ChatStreamAccumulator::new();
    let half = "z".repeat(TOOL_ARGUMENTS_MAX_BYTES / 2 + 1);
    let args_head = format!("{{\"path\":\"o.txt\",\"content\":\"{half}");
    let args_tail = format!("{half}\"}}");
    let first = SseEvent {
        event: None,
        data: serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_big","type":"function","function":{"name":"write","arguments":args_head}}]},"finish_reason":null}]}).to_string(),
    };
    accumulator
        .handle_event(&first)
        .expect("the first fragment is inside the bound");
    let second = SseEvent {
        event: None,
        data: serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":args_tail}}]},"finish_reason":null}]}).to_string(),
    };
    let error = accumulator
        .handle_event(&second)
        .expect_err("the second fragment crosses the arguments bound");
    assert_eq!(error.code, ErrorCode::InvalidMessage);
    assert!(error.message.contains("bound"), "{}", error.message);

    // Leg 4 — the anthropic input_json_delta bound, same rule.
    let mut accumulator = MessagesStreamAccumulator::new();
    let start = SseEvent {
        event: Some("content_block_start".to_string()),
        data: serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_big","name":"write"}}).to_string(),
    };
    accumulator.handle_event(&start).expect("block start");
    let first = SseEvent {
        event: Some("content_block_delta".to_string()),
        data: serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":args_head}}).to_string(),
    };
    accumulator
        .handle_event(&first)
        .expect("the first fragment is inside the bound");
    let second = SseEvent {
        event: Some("content_block_delta".to_string()),
        data: serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":args_tail}}).to_string(),
    };
    let error = accumulator
        .handle_event(&second)
        .expect_err("the second fragment crosses the arguments bound");
    assert_eq!(error.code, ErrorCode::InvalidMessage);
    assert!(error.message.contains("bound"), "{}", error.message);

    // Leg 5 — pathological nesting: complete but absurdly deep arguments
    // fail the CLOSE loudly (serde_json's recursion guard — never a stack
    // overflow, never a guessed parameter).
    let nested = format!("{}{}", "[".repeat(300), "]".repeat(300));
    let mut accumulator = ChatStreamAccumulator::new();
    for frame in [
        serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_deep","type":"function","function":{"name":"read","arguments":nested}}]},"finish_reason":null}]}),
        serde_json::json!({"id":"x","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
    ] {
        accumulator
            .handle_event(&SseEvent {
                event: None,
                data: frame.to_string(),
            })
            .expect("fragments feed");
    }
    let error = accumulator
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .expect_err("pathologically nested arguments close loud");
    assert_eq!(error.code, ErrorCode::InvalidMessage);
}
