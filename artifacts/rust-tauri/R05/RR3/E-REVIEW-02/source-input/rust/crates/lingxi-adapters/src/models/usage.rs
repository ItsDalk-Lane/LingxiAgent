//! Per-protocol usage normalization (R05-T07): the machine-readable
//! supplier-field → unified-field formula table plus the strict decoder
//! every chat family and operation dialect routes through.
//!
//! One formula table ([`USAGE_MAPPINGS`]) is the single source the
//! semantics doc (`docs/rust-tauri/R05/MODEL_USAGE_SEMANTICS.md`) and the
//! tests are generated/verified against — cache/reasoning component
//! handling is a DOCUMENTED per-family fact, never inferred from values.
//!
//! Sources of the mapping (per the taskbook's protocol list; official
//! wire docs, cited in the semantics doc):
//! - OpenAI chat-completions / responses: `prompt_tokens_details.cached_tokens`
//!   is included in `prompt_tokens`/`input_tokens`;
//!   `completion_tokens_details.reasoning_tokens` /
//!   `output_tokens_details.reasoning_tokens` is included in
//!   `completion_tokens`/`output_tokens`.
//! - Anthropic messages: `cache_read_input_tokens` and
//!   `cache_creation_input_tokens` are SEPARATE input categories — they
//!   are NOT included in `input_tokens`.
//! - Google generative-ai: `cachedContentTokenCount` is included in
//!   `promptTokenCount`. `thoughtsTokenCount` is a SEPARATE wire field
//!   from `candidatesTokenCount` (R05 RR1 F23: Google bills candidate
//!   output AND thoughts, e.g.
//!   `totalTokenCount = promptTokenCount + candidatesTokenCount +
//!   thoughtsTokenCount`); the decoder normalizes the unified
//!   `output_tokens` to the TOTAL generated output
//!   (`candidatesTokenCount + thoughtsTokenCount`) so one convention —
//!   "the output total already contains the reasoning component" — holds
//!   across every family, and no consumer ever under-bills by the
//!   thoughts half.
//!
//! Numeric contract (taskbook T07-C06): a token count must be a
//! non-negative JSON integer within `u64`. Negative values, floats,
//! strings or out-of-range magnitudes are [`UsageDecode::Invalid`] — the
//! turn still settles, the numbers never do (no truncation, no
//! saturation, no zero).

use lingxi_kernel::model_exchange::ProtocolFamily;
use lingxi_kernel::usage::{ModelCallUsage, UsageProvenance};

/// How the family's usage frames aggregate within one request.
pub use lingxi_kernel::usage::UsageAggregationMode;

/// One family's supplier-field → unified-field formula (T07-C07).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyUsageMapping {
    pub family: ProtocolFamily,
    /// Aggregation of usage frames within one request (stream vs buffered
    /// can differ; the two modes are listed separately).
    pub buffered: UsageAggregationMode,
    pub streaming: UsageAggregationMode,
    /// JSON-pointer-ish field paths (relative to the response root).
    pub input: &'static str,
    pub output: &'static str,
    pub cache_read: Option<&'static str>,
    pub cache_write: Option<&'static str>,
    pub reasoning: Option<&'static str>,
    /// `Some(true)` = the component is ALREADY included in the input
    /// total; `Some(false)` = a separate category (Anthropic cache);
    /// `None` = the family has no such component field.
    ///
    /// R05 RR1 F23: the flags describe the UNIFIED fact
    /// ([`ModelCallUsage`]) — whether the component is part of the
    /// unified total. For OpenAI the wire total natively contains the
    /// component; for Google the wire SPLITS candidates and thoughts and
    /// the decoder normalizes the unified output to their sum, so the
    /// flag is `Some(true)` there too. A consumer NEVER re-adds a
    /// component when the flag says included — one convention, every
    /// family.
    pub cache_read_included_in_input: Option<bool>,
    pub cache_write_included_in_input: Option<bool>,
    pub reasoning_included_in_output: Option<bool>,
}

/// The single formula table (T07-C07). Asserted against per-family
/// fixtures by the T07 tests and mirrored in
/// `docs/rust-tauri/R05/MODEL_USAGE_SEMANTICS.md`.
pub const USAGE_MAPPINGS: &[FamilyUsageMapping] = &[
    FamilyUsageMapping {
        family: ProtocolFamily::OpenAiCompletions,
        buffered: UsageAggregationMode::FinalSnapshot,
        streaming: UsageAggregationMode::FinalSnapshot,
        input: "/usage/prompt_tokens",
        output: "/usage/completion_tokens",
        cache_read: Some("/usage/prompt_tokens_details/cached_tokens"),
        cache_write: None,
        reasoning: Some("/usage/completion_tokens_details/reasoning_tokens"),
        cache_read_included_in_input: Some(true),
        cache_write_included_in_input: None,
        reasoning_included_in_output: Some(true),
    },
    FamilyUsageMapping {
        family: ProtocolFamily::AnthropicMessages,
        buffered: UsageAggregationMode::FinalSnapshot,
        // message_start carries the input half (input_tokens + cache
        // categories), message_delta carries the RUNNING output total.
        streaming: UsageAggregationMode::RunningTotal,
        input: "/usage/input_tokens",
        output: "/usage/output_tokens",
        cache_read: Some("/usage/cache_read_input_tokens"),
        cache_write: Some("/usage/cache_creation_input_tokens"),
        // The messages API reports no separate reasoning-token usage
        // (thinking output bills inside output_tokens) — no field, no
        // fabricated component.
        reasoning: None,
        cache_read_included_in_input: Some(false),
        cache_write_included_in_input: Some(false),
        reasoning_included_in_output: None,
    },
    FamilyUsageMapping {
        family: ProtocolFamily::GoogleGenerativeAi,
        buffered: UsageAggregationMode::FinalSnapshot,
        // Streaming chunks carry a running cumulative usageMetadata.
        streaming: UsageAggregationMode::RunningTotal,
        input: "/usageMetadata/promptTokenCount",
        // The CANDIDATE field; the unified output the decoder produces is
        // `candidatesTokenCount + thoughtsTokenCount` (R05 RR1 F23 — see
        // the module docs and `decode_family_usage`).
        output: "/usageMetadata/candidatesTokenCount",
        cache_read: Some("/usageMetadata/cachedContentTokenCount"),
        cache_write: None,
        reasoning: Some("/usageMetadata/thoughtsTokenCount"),
        cache_read_included_in_input: Some(true),
        cache_write_included_in_input: None,
        reasoning_included_in_output: Some(true),
    },
    FamilyUsageMapping {
        family: ProtocolFamily::OpenAiResponses,
        buffered: UsageAggregationMode::FinalSnapshot,
        // The SSE aggregate ends with the terminal response object's
        // usage — one final snapshot.
        streaming: UsageAggregationMode::FinalSnapshot,
        input: "/usage/input_tokens",
        output: "/usage/output_tokens",
        cache_read: Some("/usage/input_tokens_details/cached_tokens"),
        cache_write: None,
        reasoning: Some("/usage/output_tokens_details/reasoning_tokens"),
        cache_read_included_in_input: Some(true),
        cache_write_included_in_input: None,
        reasoning_included_in_output: Some(true),
    },
    FamilyUsageMapping {
        family: ProtocolFamily::OpenAiCodexResponses,
        buffered: UsageAggregationMode::FinalSnapshot,
        streaming: UsageAggregationMode::FinalSnapshot,
        input: "/usage/input_tokens",
        output: "/usage/output_tokens",
        cache_read: Some("/usage/input_tokens_details/cached_tokens"),
        cache_write: None,
        reasoning: Some("/usage/output_tokens_details/reasoning_tokens"),
        cache_read_included_in_input: Some(true),
        cache_write_included_in_input: None,
        reasoning_included_in_output: Some(true),
    },
];

/// Looks up one family's mapping (None = the family reports no token
/// usage on its wire — media dialects bill per item, not per token).
pub fn mapping_of(family: ProtocolFamily) -> Option<&'static FamilyUsageMapping> {
    USAGE_MAPPINGS.iter().find(|m| m.family == family)
}

/// The outcome of decoding one usage-bearing response (or fragment).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageDecode {
    /// No usage object on the wire (missing — never zero).
    Absent,
    /// A usable usage fact (complete or partial; provenance set).
    Usage(ModelCallUsage),
    /// The usage object's numbers violate the contract (T07-C06): the
    /// detail names the violation; no number is trusted.
    Invalid { detail: String },
}

/// Reads one JSON-pointer-ish path (`/a/b`) from a value.
fn pointer<'a>(body: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    body.pointer(path)
}

/// Strict token-count parse: a non-negative JSON integer within `u64`
/// (T07-C06). Everything else is invalid, named by field.
fn strict_token(body: &serde_json::Value, path: &str) -> Result<Option<u64>, String> {
    match pointer(body, path) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(value) => {
            if value.is_u64() {
                Ok(value.as_u64())
            } else if value.is_i64() {
                Err(format!(
                    "usage field {path} is negative ({value}): a token count is never \
                     negative; the fact is marked invalid, never saturated"
                ))
            } else if value.is_f64() {
                Err(format!(
                    "usage field {path} is a non-integer number ({value}): a token count \
                     is an integer; the fact is marked invalid, never rounded"
                ))
            } else {
                Err(format!(
                    "usage field {path} is not a JSON number ({}): a token count is \
                     never a string or container",
                    type_of(value)
                ))
            }
        }
    }
}

fn type_of(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// Decodes one family's usage from a response (or one usage-bearing
/// fragment — streaming callers fold fragments through
/// [`lingxi_kernel::usage::UsageFolder`] per the mapping's mode).
///
/// Provenance rules (T07-C05): both totals present → `Reported`; exactly
/// one present → `Partial` naming the missing half; the usage object
/// present with neither total → `Partial` naming both; absent →
/// [`UsageDecode::Absent`].
pub fn decode_family_usage(family: ProtocolFamily, body: &serde_json::Value) -> UsageDecode {
    let Some(mapping) = mapping_of(family) else {
        return UsageDecode::Absent;
    };
    // The usage container itself decides presence.
    let container_present = pointer(body, mapping.input).is_some()
        || pointer(body, mapping.output).is_some()
        || mapping
            .cache_read
            .is_some_and(|path| pointer(body, path).is_some())
        || mapping
            .cache_write
            .is_some_and(|path| pointer(body, path).is_some())
        || mapping
            .reasoning
            .is_some_and(|path| pointer(body, path).is_some());
    if !container_present {
        return UsageDecode::Absent;
    }
    let fields = [
        (mapping.input, true),
        (mapping.output, true),
        (mapping.cache_read.unwrap_or("/__absent"), false),
        (mapping.cache_write.unwrap_or("/__absent"), false),
        (mapping.reasoning.unwrap_or("/__absent"), false),
    ];
    let mut invalid: Option<String> = None;
    let mut values: [Option<u64>; 5] = [None; 5];
    for (index, (path, _total)) in fields.iter().enumerate() {
        if *path == "/__absent" {
            continue;
        }
        match strict_token(body, path) {
            Ok(value) => values[index] = value,
            Err(detail) => {
                // First violation is enough; the whole fact is invalid.
                invalid.get_or_insert(detail);
            }
        }
    }
    if let Some(detail) = invalid {
        return UsageDecode::Invalid { detail };
    }
    // R05 RR1 F23: Google reports candidate output and thoughts as
    // SEPARATE wire fields (`totalTokenCount = prompt + candidates +
    // thoughts`); the unified `output_tokens` is normalized to the TOTAL
    // generated output (candidates + thoughts, saturating) so the
    // "output total contains the reasoning component" convention matches
    // the OpenAI families and the mapping's inclusion flag. A missing
    // thoughts field keeps the candidates as-is (missing ≠ 0).
    if matches!(family, ProtocolFamily::GoogleGenerativeAi) {
        values[1] = match (values[1], values[4]) {
            (Some(candidates), Some(thoughts)) => Some(candidates.saturating_add(thoughts)),
            (candidates, thoughts) => candidates.or(thoughts),
        };
    }
    let mut missing: Vec<&'static str> = Vec::new();
    if values[0].is_none() {
        missing.push("input_tokens");
    }
    if values[1].is_none() {
        missing.push("output_tokens");
    }
    let provenance = if missing.is_empty() {
        UsageProvenance::Reported
    } else {
        UsageProvenance::Partial { missing }
    };
    UsageDecode::Usage(ModelCallUsage {
        input_tokens: values[0],
        output_tokens: values[1],
        cache_read_tokens: values[2],
        cache_write_tokens: values[3],
        reasoning_tokens: values[4],
        provenance,
    })
}

/// R05 RR1 F38 (the F21 "unexpected tool response has usage" same-path
/// fix): salvages the usage fact of a stream turn whose buffered PARSE
/// failed — e.g. a tool-call turn a tools-less call cannot map, or a
/// malformed batch. The turn still fails LOUDLY; the usage the wire DID
/// deliver with that very turn must not vanish with the parse error (a
/// possibly-billable request keeps its accounting). `raw_usage` is the
/// usage container exactly as the family's accumulator observed it,
/// wrapped into the family's buffered body shape for the SAME strict
/// decoder (no leniency is added — an invalid salvaged fact stays
/// `Invalid`, a half-known one stays `Partial`, absent stays `Unknown`).
pub fn salvage_usage_report(
    family: lingxi_kernel::model_exchange::ProtocolFamily,
    raw_usage: Option<&serde_json::Value>,
) -> lingxi_kernel::usage::ReportedUsage {
    use lingxi_kernel::usage::ReportedUsage;
    let Some(raw) = raw_usage else {
        return ReportedUsage::Unknown;
    };
    let body = match family {
        lingxi_kernel::model_exchange::ProtocolFamily::GoogleGenerativeAi => {
            serde_json::json!({ "usageMetadata": raw })
        }
        _ => serde_json::json!({ "usage": raw }),
    };
    match decode_family_usage(family, &body) {
        UsageDecode::Absent => ReportedUsage::Unknown,
        UsageDecode::Usage(usage) => ReportedUsage::Known(usage),
        UsageDecode::Invalid { detail } => ReportedUsage::Invalid { detail },
    }
}

/// Decodes one OPERATION-plane usage object (the incumbent-normalized
/// shapes the operation dialects return: embedding `{prompt_tokens?,
/// total_tokens?}`, rerank `{input_tokens?, output_tokens?, total_tokens?}`).
/// Only token-shaped integer fields are read; `total_tokens` alone is a
/// `Partial` fact naming both halves (a total is never split by guessing).
pub fn decode_operation_usage(usage: &serde_json::Value) -> UsageDecode {
    if !usage.is_object() {
        // R05 RR1 F22: the diagnostic names the received TYPE and SIZE
        // only — the payload itself (which may carry provider-echoed
        // credential material) is NEVER copied into a diagnostic, and the
        // text is bounded by construction.
        return UsageDecode::Invalid {
            detail: format!(
                "operation usage must be an object; the payload arrived as a {} of {} \
                 serialized bytes (never echoed into diagnostics) — the fact is marked \
                 invalid, never coerced",
                type_of(usage),
                serde_json::to_string(usage)
                    .map(|text| text.len())
                    .unwrap_or(0)
            ),
        };
    }
    let read = |field: &str| -> Result<Option<u64>, String> {
        strict_token(
            &serde_json::json!({field: usage.get(field)}),
            &format!("/{field}"),
        )
    };
    // `input_tokens` first; when absent, the embedding dialects' name
    // (`prompt_tokens`) is the same fact (the incumbent normalize rule).
    // An ABSENT field falls through; an INVALID one is the fact's verdict.
    let input = match read("input_tokens") {
        Ok(Some(value)) => Some(value),
        Ok(None) => match read("prompt_tokens") {
            Ok(value) => value,
            Err(detail) => return UsageDecode::Invalid { detail },
        },
        Err(detail) => return UsageDecode::Invalid { detail },
    };
    let output = match read("output_tokens") {
        Ok(value) => value,
        Err(detail) => return UsageDecode::Invalid { detail },
    };
    let (input, output) = match (input, output) {
        (Some(input), Some(output)) => (Some(input), Some(output)),
        (None, None) => {
            // total_tokens alone: a total is never split by guessing.
            match read("total_tokens") {
                Ok(Some(_)) => (None, None),
                Ok(None) => return UsageDecode::Absent,
                Err(detail) => return UsageDecode::Invalid { detail },
            }
        }
        (input, output) => (input, output),
    };
    let mut missing: Vec<&'static str> = Vec::new();
    if input.is_none() {
        missing.push("input_tokens");
    }
    if output.is_none() {
        missing.push("output_tokens");
    }
    if input.is_none() && output.is_none() {
        // Only a total: the fact exists but neither half is known.
        return UsageDecode::Usage(ModelCallUsage {
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            provenance: UsageProvenance::Partial { missing },
        });
    }
    UsageDecode::Usage(ModelCallUsage {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: None,
        cache_write_tokens: None,
        reasoning_tokens: None,
        provenance: if missing.is_empty() {
            UsageProvenance::Reported
        } else {
            UsageProvenance::Partial { missing }
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mapping_names_all_five_chat_families() {
        let families: Vec<ProtocolFamily> = USAGE_MAPPINGS.iter().map(|m| m.family).collect();
        for family in [
            ProtocolFamily::OpenAiCompletions,
            ProtocolFamily::AnthropicMessages,
            ProtocolFamily::GoogleGenerativeAi,
            ProtocolFamily::OpenAiResponses,
            ProtocolFamily::OpenAiCodexResponses,
        ] {
            assert!(families.contains(&family), "{family:?} has a mapping");
        }
    }

    #[test]
    fn openai_totals_include_their_components_but_never_re_add() {
        let body = serde_json::json!({
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 40,
                "prompt_tokens_details": {"cached_tokens": 60},
                "completion_tokens_details": {"reasoning_tokens": 25}
            }
        });
        let UsageDecode::Usage(usage) =
            decode_family_usage(ProtocolFamily::OpenAiCompletions, &body)
        else {
            panic!("decodes");
        };
        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.output_tokens, Some(40));
        assert_eq!(usage.cache_read_tokens, Some(60));
        assert_eq!(usage.reasoning_tokens, Some(25));
        // The formula: cached/reasoning are INCLUDED in the totals — the
        // mapping says so and the aggregate is the totals themselves.
        let mapping = mapping_of(ProtocolFamily::OpenAiCompletions).unwrap();
        assert_eq!(mapping.cache_read_included_in_input, Some(true));
        assert_eq!(mapping.reasoning_included_in_output, Some(true));
    }

    #[test]
    fn anthropic_cache_categories_are_separate_from_input() {
        let body = serde_json::json!({
            "usage": {
                "input_tokens": 10,
                "output_tokens": 4,
                "cache_creation_input_tokens": 1000,
                "cache_read_input_tokens": 2000
            }
        });
        let UsageDecode::Usage(usage) =
            decode_family_usage(ProtocolFamily::AnthropicMessages, &body)
        else {
            panic!("decodes");
        };
        assert_eq!(usage.input_tokens, Some(10));
        assert_eq!(usage.cache_write_tokens, Some(1000));
        assert_eq!(usage.cache_read_tokens, Some(2000));
        let mapping = mapping_of(ProtocolFamily::AnthropicMessages).unwrap();
        assert_eq!(mapping.cache_read_included_in_input, Some(false));
        assert_eq!(mapping.cache_write_included_in_input, Some(false));
    }

    #[test]
    fn negative_float_and_string_counts_are_invalid_never_satruated() {
        for bad in [
            serde_json::json!({"usage": {"prompt_tokens": -1, "completion_tokens": 2}}),
            serde_json::json!({"usage": {"prompt_tokens": 1.5, "completion_tokens": 2}}),
            serde_json::json!({"usage": {"prompt_tokens": "12", "completion_tokens": 2}}),
        ] {
            match decode_family_usage(ProtocolFamily::OpenAiCompletions, &bad) {
                UsageDecode::Invalid { detail } => {
                    assert!(
                        detail.contains("prompt_tokens"),
                        "the violation names the field: {detail}"
                    );
                }
                other => panic!("expected Invalid, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_half_reported_usage_is_partial_naming_the_missing_half() {
        let body = serde_json::json!({"usage": {"input_tokens": 7}});
        let UsageDecode::Usage(usage) =
            decode_family_usage(ProtocolFamily::AnthropicMessages, &body)
        else {
            panic!("decodes");
        };
        assert_eq!(usage.input_tokens, Some(7));
        assert_eq!(
            usage.provenance,
            UsageProvenance::Partial {
                missing: vec!["output_tokens"]
            }
        );
    }

    #[test]
    fn absent_usage_is_absent_never_zero() {
        let body = serde_json::json!({"content": []});
        assert_eq!(
            decode_family_usage(ProtocolFamily::AnthropicMessages, &body),
            UsageDecode::Absent
        );
    }

    #[test]
    fn operation_totals_are_never_split_by_guessing() {
        let total_only = serde_json::json!({"total_tokens": 33});
        let UsageDecode::Usage(usage) = decode_operation_usage(&total_only) else {
            panic!("decodes");
        };
        assert_eq!(usage.input_tokens, None);
        assert_eq!(usage.output_tokens, None);
        assert_eq!(
            usage.provenance,
            UsageProvenance::Partial {
                missing: vec!["input_tokens", "output_tokens"]
            }
        );
        let both = serde_json::json!({"input_tokens": 3, "output_tokens": 4});
        let UsageDecode::Usage(usage) = decode_operation_usage(&both) else {
            panic!("decodes");
        };
        assert_eq!(
            usage.provenance,
            UsageProvenance::Reported,
            "both halves known is reported"
        );
        let prompt_only = serde_json::json!({"prompt_tokens": 9});
        let UsageDecode::Usage(usage) = decode_operation_usage(&prompt_only) else {
            panic!("decodes");
        };
        assert_eq!(usage.input_tokens, Some(9));
        assert_eq!(
            usage.provenance,
            UsageProvenance::Partial {
                missing: vec!["output_tokens"]
            }
        );
    }
}
