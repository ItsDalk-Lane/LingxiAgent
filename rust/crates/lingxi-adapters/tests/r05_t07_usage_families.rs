//! R05-T07 acceptance: per-protocol usage normalization (C04/C06/C07)
//! at the adapter layer.
//!
//! Every fixture below is a REAL wire shape of its protocol family; the
//! assertions read the normalized [`lingxi_kernel::usage::ModelCallUsage`]
//! fact plus the family's [`USAGE_MAPPINGS`] formula row — the machine
//! form of `docs/rust-tauri/R05/MODEL_USAGE_SEMANTICS.md`:
//! - C04: cumulative/running/final aggregation per protocol (repeated or
//!   disordered fragments never double-count; a running total never sums);
//! - C06: negative/non-integer/overflowing counts mark the fact invalid —
//!   never truncated, never saturated, never zero;
//! - C07: cache/reasoning components map with documented inclusion (a
//!   component already inside a total is never re-added by aggregation).

use lingxi_adapters::models::streaming::decode_complete_body;
use lingxi_adapters::models::usage::{
    decode_family_usage, mapping_of, UsageDecode, USAGE_MAPPINGS,
};
use lingxi_kernel::model_exchange::ProtocolFamily;
use lingxi_kernel::toolcatalog::SchemaBudget;
use lingxi_kernel::usage::{ModelCallUsage, ReportedUsage, UsageAggregationMode, UsageProvenance};

fn anthropic_body(frames: &[serde_json::Value]) -> String {
    let mut body = String::new();
    for frame in frames {
        body.push_str(&format!(
            "event: {}\n",
            frame["type"].as_str().unwrap_or("x")
        ));
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body
}

fn google_body(frames: &[serde_json::Value]) -> String {
    let mut body = String::new();
    for frame in frames {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body
}

fn openai_body(frames: &[serde_json::Value]) -> String {
    let mut body = String::new();
    for frame in frames {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    body
}

// ── C07: the formula table maps every chat family with documented inclusion ──

#[test]
fn c07_every_chat_family_declares_its_formula_and_inclusion_semantics() {
    // The five chat families each name their total fields, their optional
    // component fields and — critically — whether a component is already
    // INSIDE its total (never re-added by aggregation).
    for mapping in USAGE_MAPPINGS {
        assert!(
            mapping.input.starts_with('/') && mapping.output.starts_with('/'),
            "{}: totals are field paths",
            mapping.family.config_name()
        );
        match mapping.family {
            ProtocolFamily::OpenAiCompletions
            | ProtocolFamily::OpenAiResponses
            | ProtocolFamily::OpenAiCodexResponses => {
                assert_eq!(
                    mapping.cache_read_included_in_input,
                    Some(true),
                    "{}: cached tokens are a subset of the prompt total",
                    mapping.family.config_name()
                );
                assert_eq!(
                    mapping.reasoning_included_in_output,
                    Some(true),
                    "{}: reasoning tokens are a subset of the output total",
                    mapping.family.config_name()
                );
                assert_eq!(mapping.cache_write, None, "no cache-write field");
            }
            ProtocolFamily::AnthropicMessages => {
                assert_eq!(
                    mapping.cache_read_included_in_input,
                    Some(false),
                    "anthropic cache reads are a SEPARATE input category"
                );
                assert_eq!(
                    mapping.cache_write_included_in_input,
                    Some(false),
                    "anthropic cache writes are a SEPARATE input category"
                );
                assert_eq!(mapping.reasoning, None, "no reasoning usage field");
            }
            ProtocolFamily::GoogleGenerativeAi => {
                assert_eq!(
                    mapping.cache_read_included_in_input,
                    Some(true),
                    "gemini cachedContentTokenCount is inside promptTokenCount"
                );
                assert_eq!(
                    mapping.reasoning_included_in_output,
                    Some(true),
                    "RR1 F23: the wire splits candidates/thoughts; the decoder normalizes the \
                     unified output to their sum, so the unified total INCLUDES the reasoning \
                     component (never re-added, never dropped)"
                );
            }
            _ => unreachable!("only chat families carry mappings"),
        }
    }
}

#[test]
fn c07_openai_families_normalize_cache_and_reasoning_without_re_adding() {
    // The openai-completions shape (responses uses the same field names
    // under input_tokens/output_tokens).
    let body = serde_json::json!({
        "usage": {
            "prompt_tokens": 100,
            "completion_tokens": 40,
            "prompt_tokens_details": {"cached_tokens": 60},
            "completion_tokens_details": {"reasoning_tokens": 25}
        }
    });
    let UsageDecode::Usage(usage) = decode_family_usage(ProtocolFamily::OpenAiCompletions, &body)
    else {
        panic!("decodes");
    };
    assert_eq!(usage.input_tokens, Some(100));
    assert_eq!(usage.output_tokens, Some(40));
    assert_eq!(usage.cache_read_tokens, Some(60));
    assert_eq!(usage.reasoning_tokens, Some(25));
    assert_eq!(usage.provenance, UsageProvenance::Reported);
    // The aggregate IS the totals — components ride along, they are not
    // added into anything.
    let wire = usage.wire_record().expect("both totals");
    assert_eq!((wire.input_tokens, wire.output_tokens), (100, 40));

    let responses_shape = serde_json::json!({
        "usage": {
            "input_tokens": 100,
            "output_tokens": 40,
            "input_tokens_details": {"cached_tokens": 60},
            "output_tokens_details": {"reasoning_tokens": 25}
        }
    });
    let UsageDecode::Usage(usage) =
        decode_family_usage(ProtocolFamily::OpenAiResponses, &responses_shape)
    else {
        panic!("decodes");
    };
    assert_eq!(usage.cache_read_tokens, Some(60));
    assert_eq!(usage.reasoning_tokens, Some(25));
}

#[test]
fn c07_anthropic_cache_categories_stay_separate_from_the_input_total() {
    let body = serde_json::json!({
        "usage": {
            "input_tokens": 10,
            "output_tokens": 4,
            "cache_creation_input_tokens": 1000,
            "cache_read_input_tokens": 2000
        }
    });
    let UsageDecode::Usage(usage) = decode_family_usage(ProtocolFamily::AnthropicMessages, &body)
    else {
        panic!("decodes");
    };
    assert_eq!(usage.input_tokens, Some(10), "the total stays the total");
    assert_eq!(usage.cache_write_tokens, Some(1000));
    assert_eq!(usage.cache_read_tokens, Some(2000));
    // Documented formula: the EFFECTIVE billed input = input + cache_write
    // + cache_read (the mapping's inclusion flags say exactly this); the
    // unified fields keep them separate so no consumer re-adds blindly.
    let mapping = mapping_of(ProtocolFamily::AnthropicMessages).unwrap();
    assert_eq!(mapping.cache_read_included_in_input, Some(false));
    assert_eq!(mapping.cache_write_included_in_input, Some(false));
}

#[test]
fn c07_gemini_thoughts_and_cache_map_inside_their_totals() {
    let body = serde_json::json!({
        "usageMetadata": {
            "promptTokenCount": 88,
            "candidatesTokenCount": 19,
            "thoughtsTokenCount": 7,
            "cachedContentTokenCount": 50
        }
    });
    let UsageDecode::Usage(usage) = decode_family_usage(ProtocolFamily::GoogleGenerativeAi, &body)
    else {
        panic!("decodes");
    };
    assert_eq!(usage.input_tokens, Some(88));
    // RR1 F23: Google splits candidates (19) and thoughts (7) on the wire;
    // the unified output is the TOTAL generated output — 26, matching the
    // provider's own totalTokenCount arithmetic (88 + 26 = 114).
    assert_eq!(usage.output_tokens, Some(26));
    assert_eq!(usage.reasoning_tokens, Some(7));
    assert_eq!(usage.cache_read_tokens, Some(50));
    let wire = usage.wire_record().expect("both totals");
    assert_eq!((wire.input_tokens, wire.output_tokens), (88, 26));
}

// ── C06: illegal or out-of-range counts are invalid, never distorted ─────────

#[test]
fn c06_negative_float_string_and_overflow_counts_mark_the_fact_invalid() {
    let cases = [
        serde_json::json!({"usage": {"prompt_tokens": -1, "completion_tokens": 2}}),
        serde_json::json!({"usage": {"prompt_tokens": 1.5, "completion_tokens": 2}}),
        serde_json::json!({"usage": {"prompt_tokens": "12", "completion_tokens": 2}}),
        serde_json::json!({"usage": {"prompt_tokens": 1e30, "completion_tokens": 2}}),
        // A negative COMPONENT is the same violation.
        serde_json::json!({"usage": {
            "prompt_tokens": 1, "completion_tokens": 2,
            "prompt_tokens_details": {"cached_tokens": -5}
        }}),
    ];
    for body in cases {
        match decode_family_usage(ProtocolFamily::OpenAiCompletions, &body) {
            UsageDecode::Invalid { detail } => {
                assert!(
                    detail.contains("prompt_tokens") || detail.contains("cached_tokens"),
                    "the violation names its field: {detail}"
                );
            }
            other => panic!("expected Invalid, got {other:?} for {body}"),
        }
    }
}

// ── C06 streaming legs (REVIEW-T07 F-01): a violating usage fragment in
// the RUNNING-TOTAL families is never silently dropped — the buffered
// finish parse sees the raw violating numbers and marks the fact invalid,
// exactly like the buffered wire mode and the openai streaming control. ──

/// One well-formed anthropic text stream parameterized by the usage
/// payloads: message_start's input half + the message_delta frames.
fn anthropic_usage_stream(
    start_usage: serde_json::Value,
    deltas: &[serde_json::Value],
) -> Vec<serde_json::Value> {
    let mut frames = vec![
        serde_json::json!({"type":"message_start","message":{"usage": start_usage}}),
        serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"a"}}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"b"}}),
        // R05 RR1 F12: a completed batch closes its blocks (an unclosed
        // block is a protocol violation independent of usage folding).
        serde_json::json!({"type":"content_block_stop","index":0}),
    ];
    for delta in deltas {
        frames.push(delta.clone());
    }
    frames.push(serde_json::json!({"type":"message_stop"}));
    frames
}

#[test]
fn c06_anthropic_streaming_violating_usage_fragments_mark_the_fact_invalid() {
    use lingxi_adapters::models::anthropic_messages::MessagesStreamAccumulator;
    let delta = |usage: serde_json::Value| serde_json::json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage": usage});
    let bare_delta = |usage: serde_json::Value| serde_json::json!({"type":"message_delta","delta":{},"usage": usage});
    let valid_input_half = serde_json::json!({"input_tokens":25});
    let cases: Vec<(&str, Vec<serde_json::Value>, &str)> = vec![
        (
            "negative output total after a valid input half",
            anthropic_usage_stream(
                valid_input_half.clone(),
                &[delta(serde_json::json!({"output_tokens":-5}))],
            ),
            "output_tokens",
        ),
        (
            "float output total",
            anthropic_usage_stream(
                valid_input_half.clone(),
                &[delta(serde_json::json!({"output_tokens":1.5}))],
            ),
            "output_tokens",
        ),
        (
            "string output total",
            anthropic_usage_stream(
                valid_input_half.clone(),
                &[delta(serde_json::json!({"output_tokens":"9"}))],
            ),
            "output_tokens",
        ),
        (
            "violating input half on message_start",
            anthropic_usage_stream(
                serde_json::json!({"input_tokens":-3}),
                &[delta(serde_json::json!({"output_tokens":9}))],
            ),
            "input_tokens",
        ),
        (
            "worst case: a valid running total THEN a violating delta",
            anthropic_usage_stream(
                valid_input_half,
                &[
                    bare_delta(serde_json::json!({"output_tokens":9})),
                    delta(serde_json::json!({"output_tokens":-4})),
                ],
            ),
            "output_tokens",
        ),
    ];
    for (label, frames, field) in cases {
        let body = anthropic_body(&frames);
        let events = decode_complete_body(&body).expect("decode");
        let mut accumulator = MessagesStreamAccumulator::new();
        for event in &events {
            accumulator.handle_event(event).expect("accumulate");
        }
        let parsed = accumulator
            .finish(
                &lingxi_protocol::ModelCallId::new("run-mc0001"),
                &lingxi_kernel::model_exchange::ToolDeclarationSnapshot::empty(),
                &SchemaBudget::default(),
            )
            .expect("finish");
        match parsed.usage_report {
            ReportedUsage::Invalid { detail } => {
                assert!(
                    detail.contains(field),
                    "{label}: the violation names its field: {detail}"
                );
                let nature_named = detail.contains("negative")
                    || detail.contains("non-integer")
                    || detail.contains("never a string");
                assert!(
                    nature_named,
                    "{label}: the violation names its nature: {detail}"
                );
            }
            other => panic!(
                "{label}: expected Invalid, got {other:?} — a contract-violating fragment \
                 is never silently dropped into a clean/partial/unknown fact"
            ),
        }
    }
}

#[test]
fn c06_gemini_streaming_violating_usage_metadata_marks_the_fact_invalid() {
    use lingxi_adapters::models::google_generative_ai::GenerateStreamAccumulator;
    let content_frame = |text: &str| serde_json::json!({"candidates":[{"content":{"role":"model","parts":[{"text":text}]}}]});
    let terminal_frame = || serde_json::json!({"candidates":[{"content":{"role":"model","parts":[{"text":"b"}]},"finishReason":"STOP"}]});
    let cases: Vec<(&str, Vec<serde_json::Value>, &str)> = vec![
        (
            "negative candidatesTokenCount",
            vec![
                content_frame("a"),
                serde_json::json!({"usageMetadata":{"promptTokenCount":88,"candidatesTokenCount":-7}}),
                terminal_frame(),
            ],
            "candidatesTokenCount",
        ),
        (
            "float promptTokenCount",
            vec![
                content_frame("a"),
                serde_json::json!({"usageMetadata":{"promptTokenCount":2.5,"candidatesTokenCount":19}}),
                terminal_frame(),
            ],
            "promptTokenCount",
        ),
        (
            "string candidatesTokenCount",
            vec![
                content_frame("a"),
                serde_json::json!({"usageMetadata":{"candidatesTokenCount":"19"}}),
                terminal_frame(),
            ],
            "candidatesTokenCount",
        ),
        (
            "worst case: a valid running snapshot THEN a violating one",
            vec![
                content_frame("a"),
                serde_json::json!({"usageMetadata":{"promptTokenCount":88,"candidatesTokenCount":19}}),
                serde_json::json!({"usageMetadata":{"promptTokenCount":88,"candidatesTokenCount":-4}}),
                terminal_frame(),
            ],
            "candidatesTokenCount",
        ),
    ];
    for (label, frames, field) in cases {
        let body = google_body(&frames);
        let events = decode_complete_body(&body).expect("decode");
        let mut accumulator = GenerateStreamAccumulator::new();
        for event in &events {
            accumulator.handle_event(event).expect("accumulate");
        }
        let parsed = accumulator
            .finish(
                &lingxi_protocol::ModelCallId::new("run-mc0001"),
                &lingxi_kernel::model_exchange::ToolDeclarationSnapshot::empty(),
                &SchemaBudget::default(),
            )
            .expect("finish");
        match parsed.usage_report {
            ReportedUsage::Invalid { detail } => {
                assert!(
                    detail.contains(field),
                    "{label}: the violation names its field: {detail}"
                );
            }
            other => panic!(
                "{label}: expected Invalid, got {other:?} — a contract-violating \
                 usageMetadata frame is never silently dropped"
            ),
        }
    }
}

// ── C04: aggregation modes — running totals replace, finals are unique ───────

#[test]
fn c04_anthropic_running_output_replaces_never_sums() {
    use lingxi_adapters::models::anthropic_messages::MessagesStreamAccumulator;
    let body = anthropic_body(&[
        serde_json::json!({"type":"message_start","message":{"usage":{
            "input_tokens": 25, "cache_read_input_tokens": 400}}}),
        serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"a"}}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"b"}}),
        // R05 RR1 F12: a completed batch closes its blocks.
        serde_json::json!({"type":"content_block_stop","index":0}),
        // Two message_delta frames: the running output total GROWS (each
        // frame reports the cumulative count — per the family's streaming
        // contract they are snapshots, not increments).
        serde_json::json!({"type":"message_delta","delta":{"stop_reason":null},"usage":{"output_tokens":3}}),
        serde_json::json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":9}}),
        serde_json::json!({"type":"message_stop"}),
    ]);
    let events = decode_complete_body(&body).expect("decode");
    let mut accumulator = MessagesStreamAccumulator::new();
    for event in &events {
        accumulator.handle_event(event).expect("accumulate");
    }
    let parsed = accumulator
        .finish(
            &lingxi_protocol::ModelCallId::new("run-mc0001"),
            &lingxi_kernel::model_exchange::ToolDeclarationSnapshot::empty(),
            &SchemaBudget::default(),
        )
        .expect("finish");
    match parsed.usage_report {
        ReportedUsage::Known(ModelCallUsage {
            input_tokens,
            output_tokens,
            cache_read_tokens,
            provenance,
            ..
        }) => {
            assert_eq!(input_tokens, Some(25));
            // 3 → 9 REPLACES (never 12): a cumulative snapshot is never
            // summed.
            assert_eq!(output_tokens, Some(9));
            assert_eq!(cache_read_tokens, Some(400));
            assert_eq!(provenance, UsageProvenance::Reported);
        }
        other => panic!("expected a known usage, got {other:?}"),
    }
}

#[test]
fn c04_anthropic_running_total_going_backwards_is_loud() {
    use lingxi_adapters::models::anthropic_messages::MessagesStreamAccumulator;
    let body = anthropic_body(&[
        serde_json::json!({"type":"message_start","message":{"usage":{"input_tokens":25}}}),
        serde_json::json!({"type":"message_delta","delta":{},"usage":{"output_tokens":9}}),
        serde_json::json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":4}}),
        serde_json::json!({"type":"message_stop"}),
    ]);
    let events = decode_complete_body(&body).expect("decode");
    let mut accumulator = MessagesStreamAccumulator::new();
    accumulator.handle_event(&events[0]).expect("start");
    accumulator.handle_event(&events[1]).expect("first delta");
    let error = accumulator
        .handle_event(&events[2])
        .expect_err("a running total never goes backwards");
    assert!(
        error.message.contains("backwards") || error.message.contains("conflict"),
        "{error}"
    );
}

#[test]
fn c04_anthropic_interrupted_before_output_is_partial_never_zero() {
    // The stream dies after message_start: the input half is REAL, the
    // output half never arrived. (message_stop missing → the accumulator
    // finish is the loud truncation error; the FOLD itself is asserted
    // through the folder — this pins the contract at the folder level.)
    let mut folder = lingxi_kernel::usage::UsageFolder::new(UsageAggregationMode::RunningTotal);
    folder
        .fold(ModelCallUsage {
            input_tokens: Some(25),
            output_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            provenance: UsageProvenance::Reported,
        })
        .expect("input half folds");
    let folded = folder.finish().expect("partial fact survives");
    assert_eq!(folded.input_tokens, Some(25));
    assert_eq!(folded.output_tokens, None, "missing is missing, not 0");
    assert_eq!(
        folded.provenance,
        UsageProvenance::Partial {
            missing: vec!["output_tokens"]
        }
    );
    assert!(
        folded.wire_record().is_none(),
        "a half-known usage never projects a wire number"
    );
}

#[test]
fn c04_gemini_streaming_running_metadata_replaces_not_sums() {
    use lingxi_adapters::models::google_generative_ai::GenerateStreamAccumulator;
    let body = google_body(&[
        serde_json::json!({"candidates":[{"content":{"role":"model","parts":[{"text":"a"}]}}]}),
        serde_json::json!({"candidates":[{"content":{"role":"model","parts":[{"text":"b"}]}}],
            "usageMetadata":{"promptTokenCount":88,"candidatesTokenCount":4}}),
        serde_json::json!({"candidates":[{"content":{"role":"model","parts":[{"text":"c"}]},"finishReason":"STOP"}],
            "usageMetadata":{"promptTokenCount":88,"candidatesTokenCount":19,"thoughtsTokenCount":7}}),
    ]);
    let events = decode_complete_body(&body).expect("decode");
    let mut accumulator = GenerateStreamAccumulator::new();
    for event in &events {
        accumulator.handle_event(event).expect("accumulate");
    }
    let parsed = accumulator
        .finish(
            &lingxi_protocol::ModelCallId::new("run-mc0001"),
            &lingxi_kernel::model_exchange::ToolDeclarationSnapshot::empty(),
            &SchemaBudget::default(),
        )
        .expect("finish");
    match parsed.usage_report {
        ReportedUsage::Known(usage) => {
            // RR1 F23: each frame's unified output is candidates + thoughts
            // of THAT snapshot — frame 2 (candidates 4, no thoughts) → 4;
            // frame 3 (candidates 19 + thoughts 7) → 26. 4 → 26 REPLACES
            // (never sums): a cumulative snapshot is never added up.
            assert_eq!(usage.output_tokens, Some(26));
            assert_eq!(usage.input_tokens, Some(88));
            assert_eq!(usage.reasoning_tokens, Some(7));
        }
        other => panic!("expected a known usage, got {other:?}"),
    }
}

#[test]
fn c04_openai_final_usage_repeats_are_idempotent_and_conflicts_are_loud() {
    use lingxi_adapters::models::openai_completions::ChatStreamAccumulator;
    let usage_frame = serde_json::json!({
        "id":"x","choices":[],"usage":{"prompt_tokens":11,"completion_tokens":33}
    });
    let body = openai_body(&[
        serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"role":"assistant","content":"hi"},"finish_reason":null}]}),
        usage_frame.clone(),
        // An IDENTICAL re-transmission: no new information, no double count.
        usage_frame.clone(),
        serde_json::json!({"id":"x","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
    ]);
    let events = decode_complete_body(&body).expect("decode");
    let mut accumulator = ChatStreamAccumulator::new();
    for event in &events {
        accumulator.handle_event(event).expect("accumulate");
    }
    let parsed = accumulator
        .finish(
            &lingxi_protocol::ModelCallId::new("run-mc0001"),
            &lingxi_kernel::model_exchange::ToolDeclarationSnapshot::empty(),
            &SchemaBudget::default(),
        )
        .expect("finish");
    match parsed.usage_report {
        ReportedUsage::Known(usage) => {
            assert_eq!(usage.input_tokens, Some(11));
            assert_eq!(usage.output_tokens, Some(33));
        }
        other => panic!("expected a known usage, got {other:?}"),
    }

    // A DIFFERENT second final snapshot is the loud conflict it is.
    let body = openai_body(&[
        serde_json::json!({"id":"x","choices":[{"index":0,"delta":{"role":"assistant","content":"hi"},"finish_reason":null}]}),
        usage_frame.clone(),
        serde_json::json!({"id":"x","choices":[],"usage":{"prompt_tokens":99,"completion_tokens":99}}),
        serde_json::json!({"id":"x","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
    ]);
    let events = decode_complete_body(&body).expect("decode");
    let mut accumulator = ChatStreamAccumulator::new();
    accumulator.handle_event(&events[0]).expect("first");
    accumulator.handle_event(&events[1]).expect("usage");
    let error = accumulator
        .handle_event(&events[2])
        .expect_err("different final snapshot is loud");
    assert!(error.message.contains("usage"), "{error}");
}

#[test]
fn c04_responds_families_carry_one_final_snapshot_on_the_terminal_object() {
    // The responses/codex families aggregate the SSE into the terminal
    // response object whose usage IS the request's final usage — one
    // snapshot, no folding (the mapping says FinalSnapshot).
    for family in [
        ProtocolFamily::OpenAiResponses,
        ProtocolFamily::OpenAiCodexResponses,
    ] {
        let mapping = mapping_of(family).unwrap();
        assert_eq!(mapping.streaming, UsageAggregationMode::FinalSnapshot);
        let body = serde_json::json!({
            "usage": {"input_tokens": 8, "output_tokens": 3}
        });
        let UsageDecode::Usage(usage) = decode_family_usage(family, &body) else {
            panic!("decodes");
        };
        assert_eq!(usage.input_tokens, Some(8));
        assert_eq!(usage.output_tokens, Some(3));
    }
}

#[test]
fn c05_absent_usage_is_unknown_and_a_half_reported_usage_is_partial() {
    // Absent (buffered, any family).
    let body = serde_json::json!({"content": []});
    assert_eq!(
        decode_family_usage(ProtocolFamily::AnthropicMessages, &body),
        UsageDecode::Absent
    );
    // Half-reported: the input half only.
    let body = serde_json::json!({"usage": {"input_tokens": 7}});
    let UsageDecode::Usage(usage) = decode_family_usage(ProtocolFamily::AnthropicMessages, &body)
    else {
        panic!("decodes");
    };
    assert_eq!(usage.input_tokens, Some(7));
    assert_eq!(usage.output_tokens, None);
    assert_eq!(
        usage.provenance,
        UsageProvenance::Partial {
            missing: vec!["output_tokens"]
        }
    );
}
