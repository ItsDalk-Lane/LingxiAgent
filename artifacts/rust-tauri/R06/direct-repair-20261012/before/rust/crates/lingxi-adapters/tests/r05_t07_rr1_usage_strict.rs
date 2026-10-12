//! R05 RR1 F22/F23 permanent regressions: strict operation-usage parsing
//! (no lenient numeric coercion, diagnostics never echo provider payloads)
//! and the Gemini thoughts/candidate output accounting.
//!
//! Migrated from the adversarial audit probes P6/P7 (audit/worker_usage,
//! `usage_probes.log`) with the assertion texts preserved: the expectations
//! come from the protocol semantics (Google bills candidate output and
//! thoughts as separate wire fields; a token count that is not a
//! non-negative integer is invalid, never a coerced number), NOT from the
//! current implementation.

use lingxi_adapters::models::operations::embedding::EmbeddingRequest;
use lingxi_adapters::models::operations::rerank::{parse_rerank, RerankRequest};
use lingxi_adapters::models::usage::UsageDecode;
use lingxi_adapters::models::usage::{decode_family_usage, decode_operation_usage, mapping_of};
use lingxi_kernel::model_exchange::ProtocolFamily;

// ── F23: Gemini thoughts are SEPARATE from candidate output ──────────────────

/// P6 verbatim: prompt=100, candidates=10, thoughts=40, total=150 — the
/// total generated output a consumer bills is 50, never the bare
/// candidatesTokenCount of 10. The billed-output formula is the audit's:
/// when the family's mapping says reasoning is included in output, the
/// output total IS the bill; otherwise the consumer adds the component.
/// Under EITHER convention this fixture must settle at 50.
#[test]
fn rr1_f23_gemini_thoughts_are_separate_from_candidate_tokens() {
    let body = serde_json::json!({"usageMetadata":{"promptTokenCount":100,"candidatesTokenCount":10,"thoughtsTokenCount":40,"totalTokenCount":150}});
    let UsageDecode::Usage(usage) = decode_family_usage(ProtocolFamily::GoogleGenerativeAi, &body)
    else {
        panic!("usage decodes");
    };
    let mapping = mapping_of(ProtocolFamily::GoogleGenerativeAi).unwrap();
    let wire = usage.wire_record().expect("both totals project");
    let billed_output = if mapping.reasoning_included_in_output == Some(true) {
        wire.output_tokens
    } else {
        wire.output_tokens + usage.reasoning_tokens.unwrap_or(0)
    };
    assert_eq!(
        billed_output, 50,
        "candidate output 10 plus separate thought output 40 must total 50, not 10"
    );
    // The provider's own total cross-checks the unified numbers: 100 + 50.
    assert_eq!(
        wire.input_tokens + billed_output,
        150,
        "input 100 plus total generated output 50 equals the provider totalTokenCount"
    );
    // The thought component survives as its own fact — never folded away.
    assert_eq!(usage.reasoning_tokens, Some(40));
    assert_eq!(usage.input_tokens, Some(100));
}

/// The F23 component matrix: no thoughts, thoughts larger than candidates,
/// missing components, a REAL zero, and the OpenAI no-re-add control.
#[test]
fn rr1_f23_gemini_component_matrix_and_family_controls() {
    // No thoughts reported (a non-thinking model/turn): output IS the
    // candidates count; reasoning stays None (missing, never 0).
    let body = serde_json::json!({"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":4,"totalTokenCount":14}});
    let UsageDecode::Usage(usage) = decode_family_usage(ProtocolFamily::GoogleGenerativeAi, &body)
    else {
        panic!("usage decodes");
    };
    assert_eq!(
        usage.output_tokens,
        Some(4),
        "no thoughts: candidates stand alone"
    );
    assert_eq!(
        usage.reasoning_tokens, None,
        "missing thoughts stay None, not 0"
    );

    // Thoughts larger than candidates: the total is still the honest sum.
    let body = serde_json::json!({"usageMetadata":{"promptTokenCount":5,"candidatesTokenCount":3,"thoughtsTokenCount":97,"totalTokenCount":105}});
    let UsageDecode::Usage(usage) = decode_family_usage(ProtocolFamily::GoogleGenerativeAi, &body)
    else {
        panic!("usage decodes");
    };
    let mapping = mapping_of(ProtocolFamily::GoogleGenerativeAi).unwrap();
    let wire = usage.wire_record().expect("totals");
    let billed = if mapping.reasoning_included_in_output == Some(true) {
        wire.output_tokens
    } else {
        wire.output_tokens + usage.reasoning_tokens.unwrap_or(0)
    };
    assert_eq!(billed, 100, "3 candidates + 97 thoughts");

    // A REAL provider zero is zero (never "unknown").
    let body = serde_json::json!({"usageMetadata":{"promptTokenCount":7,"candidatesTokenCount":0,"thoughtsTokenCount":0,"totalTokenCount":7}});
    let UsageDecode::Usage(usage) = decode_family_usage(ProtocolFamily::GoogleGenerativeAi, &body)
    else {
        panic!("usage decodes");
    };
    assert_eq!(usage.output_tokens, Some(0));
    assert_eq!(usage.reasoning_tokens, Some(0));

    // OpenAI control: completion_tokens ALREADY includes reasoning — the
    // mapping says so and the aggregate never re-adds it.
    let body = serde_json::json!({
        "usage": {"prompt_tokens": 100, "completion_tokens": 40,
                  "completion_tokens_details": {"reasoning_tokens": 25}}
    });
    let UsageDecode::Usage(usage) = decode_family_usage(ProtocolFamily::OpenAiCompletions, &body)
    else {
        panic!("usage decodes");
    };
    let mapping = mapping_of(ProtocolFamily::OpenAiCompletions).unwrap();
    assert_eq!(mapping.reasoning_included_in_output, Some(true));
    let wire = usage.wire_record().expect("totals");
    assert_eq!(
        wire.output_tokens, 40,
        "the openai total is the bill — no re-add"
    );

    // Anthropic control: cache categories stay SEPARATE input categories
    // (the anthropic relationship is not pasted onto other families).
    let body = serde_json::json!({
        "usage": {"input_tokens": 10, "output_tokens": 4,
                  "cache_read_input_tokens": 2000}
    });
    let UsageDecode::Usage(usage) = decode_family_usage(ProtocolFamily::AnthropicMessages, &body)
    else {
        panic!("usage decodes");
    };
    assert_eq!(usage.input_tokens, Some(10));
    assert_eq!(usage.cache_read_tokens, Some(2000));
}

// ── F22: strict operation-usage parsing ──────────────────────────────────────

/// P7 verbatim: `input_tokens:null` / `output_tokens:"7"` must NOT become a
/// Reported 0/7 — null stays absent, a string token count is invalid.
#[test]
fn rr1_f22_rerank_non_numeric_usage_must_not_be_coerced_to_reported_zero() {
    let raw = serde_json::json!({
        "results":[{"index":0,"relevance_score":0.5}],
        "meta":{"tokens":{"input_tokens":null,"output_tokens":"7"}}
    });
    let outcome = parse_rerank(ProtocolFamily::CohereRerank, 1, 1, &raw).expect("parses");
    let decoded = decode_operation_usage(outcome.usage.as_ref().expect("usage present"));
    assert!(
        matches!(decoded, UsageDecode::Invalid { .. }),
        "null must remain absent and a string token count invalid; conversion to reported 0/7 \
         falsifies supplier facts (got {decoded:?})"
    );
}

/// The F22 table: every illegal shape is invalid (or absent where the
/// supplier sent nothing) — never a coerced number. A legal 0 stays 0.
#[test]
fn rr1_f22_operation_usage_type_table() {
    let invalid_cases = [
        // string numbers
        serde_json::json!({"input_tokens":"12","output_tokens":3}),
        serde_json::json!({"input_tokens":1,"output_tokens":"3"}),
        // booleans
        serde_json::json!({"input_tokens":false,"output_tokens":3}),
        // negatives
        serde_json::json!({"input_tokens":-1,"output_tokens":3}),
        // floats
        serde_json::json!({"input_tokens":1.5,"output_tokens":3}),
        // containers
        serde_json::json!({"input_tokens":[1],"output_tokens":3}),
        serde_json::json!({"input_tokens":{"n":1},"output_tokens":3}),
        // the embedding dialect name coerces the same way
        serde_json::json!({"prompt_tokens":"9"}),
        // a total-only string
        serde_json::json!({"total_tokens":"33"}),
    ];
    for usage in invalid_cases {
        assert!(
            matches!(decode_operation_usage(&usage), UsageDecode::Invalid { .. }),
            "{usage}: an illegal token shape is invalid, never a coerced number"
        );
    }
    // Null is ABSENT (the supplier sent nothing usable for that half).
    match decode_operation_usage(&serde_json::json!({"input_tokens":null,"output_tokens":3})) {
        UsageDecode::Usage(usage) => {
            assert_eq!(usage.input_tokens, None, "null stays absent");
            assert_eq!(usage.output_tokens, Some(3));
        }
        other => panic!("null input + valid output decodes, got {other:?}"),
    }
    // A legal 0 is a reported 0.
    match decode_operation_usage(&serde_json::json!({"input_tokens":0,"output_tokens":0})) {
        UsageDecode::Usage(usage) => {
            assert_eq!(usage.input_tokens, Some(0));
            assert_eq!(usage.output_tokens, Some(0));
        }
        other => panic!("real zeros decode, got {other:?}"),
    }
    // A huge-but-integer u64 survives without f64 loss.
    match decode_operation_usage(
        &serde_json::json!({"input_tokens":9007199254740993u64,"output_tokens":1}),
    ) {
        UsageDecode::Usage(usage) => {
            assert_eq!(usage.input_tokens, Some(9_007_199_254_740_993));
        }
        other => panic!("big integers keep exact precision, got {other:?}"),
    }
}

/// F22's leak half: a malformed usage value (here an array smuggling the
/// provider's own credential) must NEVER be echoed into the diagnostic —
/// the detail names the type/shape only.
#[test]
fn rr1_f22_invalid_detail_never_echoes_the_payload() {
    let synthetic_secret = "RR1_F22_AUDIT_ONLY_EMBEDDING_CREDENTIAL";
    let usage = serde_json::json!([{"authorization": format!("Bearer {synthetic_secret}")}]);
    match decode_operation_usage(&usage) {
        UsageDecode::Invalid { detail } => {
            assert!(
                !detail.contains(synthetic_secret),
                "the diagnostic must not echo the payload: {detail}"
            );
            assert!(
                !detail.contains("Bearer"),
                "no payload fragment rides along: {detail}"
            );
            assert!(
                detail.contains("array") && detail.contains("object"),
                "the detail names the received type: {detail}"
            );
            assert!(
                detail.chars().count() <= 512,
                "the diagnostic is bounded: {} chars",
                detail.chars().count()
            );
        }
        other => panic!("a non-object usage is invalid, got {other:?}"),
    }
}

/// F22 embedding leg at the dialect level: the MiniMax `total_tokens`
/// passthrough keeps the RAW value (a string/null/float reaches the strict
/// decoder as itself, never pre-coerced through f64).
#[test]
fn rr1_f22_minimax_total_tokens_keeps_its_raw_type() {
    use lingxi_adapters::models::operations::embedding::parse_embedding;
    let request = EmbeddingRequest {
        inputs: vec!["audit".to_string()],
        dimensions: Some(2),
        context_window: None,
        input_type: Default::default(),
    };
    for (raw_total, expect_invalid) in [
        (serde_json::json!("41"), true),
        (serde_json::json!(null), false),
        (serde_json::json!(41), false),
        (serde_json::json!(1.5), true),
    ] {
        let body = serde_json::json!({
            "vectors":[[0.25,0.75]],
            "total_tokens": raw_total
        });
        let outcome = parse_embedding(ProtocolFamily::MinimaxEmbeddings, &request, &body)
            .unwrap_or_else(|e| panic!("{raw_total}: parses ({e})"));
        let usage = outcome.usage.as_ref().expect("usage present");
        let decoded = decode_operation_usage(usage);
        assert_eq!(
            matches!(decoded, UsageDecode::Invalid { .. }),
            expect_invalid,
            "raw {raw_total}: invalid={expect_invalid}, got {decoded:?}"
        );
        if let UsageDecode::Invalid { detail } = decoded {
            assert!(!detail.contains('"'), "no raw payload echo: {detail}");
        }
    }
}

/// F22 rerank builder validation sanity (the strictness work rides on a
/// still-valid rerank surface): a legal rerank response keeps working.
#[test]
fn rr1_f22_legal_rerank_usage_still_reports() {
    let raw = serde_json::json!({
        "results":[{"index":0,"relevance_score":0.5}],
        "meta":{"tokens":{"input_tokens":3,"output_tokens":4}}
    });
    let outcome = parse_rerank(ProtocolFamily::CohereRerank, 1, 1, &raw).expect("parses");
    match decode_operation_usage(outcome.usage.as_ref().expect("usage")) {
        UsageDecode::Usage(usage) => {
            assert_eq!(usage.input_tokens, Some(3));
            assert_eq!(usage.output_tokens, Some(4));
        }
        other => panic!("legal numbers report, got {other:?}"),
    }
    let _ = RerankRequest {
        query: "q".to_string(),
        documents: vec!["d".to_string()],
        top_n: Some(1),
    };
}
