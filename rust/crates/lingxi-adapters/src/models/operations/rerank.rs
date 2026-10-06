//! Rerank dialects (R05-T06): cohere-rerank / siliconflow-rerank /
//! voyage-rerank / dashscope-rerank — the incumbent
//! `core/model-operation-client.ts` rerank surface (bounds, URL rewrites,
//! response validation and deterministic ordering), ported rule for rule.
//!
//! Fidelity notes (registered in §30):
//! - query AND documents go through the incumbent `assertTextList`
//!   (RAW UTF-16 bounds, verbatim refusal texts); `topN` outside
//!   1..=documents.len() REFUSES ("topN must be within the document
//!   count") — it is never clamped.
//! - the cohere/siliconflow URL dedups a trailing `/rerank` and rewrites
//!   `/compatible-mode/v1` → `/compatible-api/v1` + the PLURAL `/reranks`;
//!   the dashscope COMPATIBLE endpoint always uses the plural `/reranks`
//!   (with the same rewrite when the base carries compatible-mode).
//! - the score ladder `relevance_score ?? relevanceScore ?? score` is
//!   nullish: a JSON null passes THROUGH to the next candidate.
//! - usage is SYNTHESIZED per the incumbent `normalizeRerankUsage`
//!   (meta.tokens{input,output,total} else usage.total_tokens), never a
//!   verbatim passthrough.

use lingxi_kernel::model_exchange::{ProtocolFamily, ResolvedModelRoute};
use lingxi_protocol::ProtocolError;

use super::embedding::origin_of;
use super::{assert_text_list, operation_url, parse, OperationRequestPlan};
use crate::models::credentials::ApplicableAuth;
use crate::models::dispatch::BearerStyle;

/// The incumbent bound (shared/model-operations.ts
/// MODEL_OPERATION_RERANK_MAX_DOCS).
pub const MAX_RERANK_DOCUMENTS: usize = 50;

#[derive(Debug, Clone)]
pub struct RerankRequest {
    pub query: String,
    pub documents: Vec<String>,
    /// The incumbent `topN ?? documents.length`; a present value outside
    /// 1..=documents.len() refuses loudly.
    pub top_n: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RerankHit {
    pub index: usize,
    pub score: f64,
}

#[derive(Debug, Clone)]
pub struct RerankOutcome {
    /// Sorted: score descending, index ascending (the incumbent order).
    pub results: Vec<RerankHit>,
    pub usage: Option<serde_json::Value>,
}

/// The incumbent validation (loud, before any wire work). Returns the
/// EFFECTIVE top_n (`topN ?? documents.length`, bounds-checked).
pub fn validate_rerank_request(request: &RerankRequest) -> Result<usize, ProtocolError> {
    assert_text_list(std::slice::from_ref(&request.query), "query", 1)?;
    assert_text_list(&request.documents, "documents", MAX_RERANK_DOCUMENTS)?;
    let top_n = request.top_n.unwrap_or(request.documents.len());
    if top_n == 0 || top_n > request.documents.len() {
        return Err(parse::invalid(
            "topN must be within the document count".to_string(),
        ));
    }
    Ok(top_n)
}

/// The incumbent `cohereRerankUrl`: a base containing `/compatible-mode/v1`
/// is rewritten to `/compatible-api/v1` and takes the PLURAL `/reranks`;
/// otherwise the singular `/rerank` — both through `operationUrl` dedup.
fn cohere_rerank_url(endpoint: &str) -> String {
    if !endpoint.contains("/compatible-mode/v1") {
        return operation_url(endpoint, "rerank");
    }
    operation_url(
        &endpoint.replace("/compatible-mode/v1", "/compatible-api/v1"),
        "reranks",
    )
}

/// The incumbent `dashscopeCompatibleRerankUrl`: the compatible-mode
/// rewrite when present, then ALWAYS the plural `/reranks`.
fn dashscope_compatible_rerank_url(endpoint: &str) -> String {
    let rewritten = if endpoint.contains("/compatible-mode/v1") {
        endpoint.replace("/compatible-mode/v1", "/compatible-api/v1")
    } else {
        endpoint.to_string()
    };
    operation_url(&rewritten, "reranks")
}

pub fn build_rerank(
    route: &ResolvedModelRoute,
    auth: &ApplicableAuth,
    request: &RerankRequest,
) -> Result<OperationRequestPlan, ProtocolError> {
    let top_n = validate_rerank_request(request)?;
    match route.protocol {
        ProtocolFamily::CohereRerank | ProtocolFamily::SiliconflowRerank => {
            let body = serde_json::json!({
                "model": route.model,
                "query": request.query,
                "documents": request.documents,
                "top_n": top_n,
                "return_documents": false,
            });
            Ok(
                OperationRequestPlan::post_json(cohere_rerank_url(&route.endpoint), body)
                    .with_auth(auth, BearerStyle::AuthorizationBearer),
            )
        }
        ProtocolFamily::VoyageRerank => {
            let body = serde_json::json!({
                "model": route.model,
                "query": request.query,
                "documents": request.documents,
                "top_k": top_n,
            });
            Ok(OperationRequestPlan::post_json(
                format!("{}/v1/rerank", route.endpoint.trim_end_matches('/')),
                body,
            )
            .with_auth(auth, BearerStyle::AuthorizationBearer))
        }
        ProtocolFamily::DashscopeRerank => {
            // The incumbent's model-prefix split: gte-rerank* and
            // qwen3-vl-rerank* take the NATIVE nested endpoint; everything
            // else takes the compatible (flat cohere-shaped) one.
            let native =
                route.model.starts_with("gte-rerank") || route.model.starts_with("qwen3-vl-rerank");
            if native {
                let origin = origin_of(&route.endpoint)?;
                let body = serde_json::json!({
                    "model": route.model,
                    "input": {"query": request.query, "documents": request.documents},
                    "parameters": {"top_n": top_n, "return_documents": false},
                });
                Ok(OperationRequestPlan::post_json(
                    format!("{origin}/api/v1/services/rerank/text-rerank/text-rerank"),
                    body,
                )
                .with_auth(auth, BearerStyle::AuthorizationBearer))
            } else {
                let body = serde_json::json!({
                    "model": route.model,
                    "query": request.query,
                    "documents": request.documents,
                    "top_n": top_n,
                });
                Ok(OperationRequestPlan::post_json(
                    dashscope_compatible_rerank_url(&route.endpoint),
                    body,
                )
                .with_auth(auth, BearerStyle::AuthorizationBearer))
            }
        }
        other => Err(parse::invalid(format!(
            "protocol family {} does not serve rerank (wiring bug: the gateway matrix \
             refuses earlier)",
            other.config_name()
        ))),
    }
}

/// The incumbent `normalizeRerankUsage` SHAPE, made strict (R05 RR1 F22):
/// `meta.tokens` (object) → `{input_tokens?, output_tokens?,
/// total_tokens}`; otherwise `usage.total_tokens` → `{total_tokens}`;
/// otherwise absent.
///
/// The normalizer is a shape SELECTOR, never a numeric coercion: the
/// supplier's raw values pass through untouched — a JSON null stays null
/// (the strict decoder reads it as ABSENT), a string/float/container
/// reaches [`super::super::usage::decode_operation_usage`] as itself and
/// marks the whole fact invalid. The only synthesized value is
/// `total_tokens`, and only when BOTH halves are genuine non-negative
/// JSON integers (a total is never synthesized from a missing half — that
/// would silently bill the missing half as 0; and no f64 round-trip ever
/// touches an integer).
fn normalize_rerank_usage(body: &serde_json::Value) -> Option<serde_json::Value> {
    if let Some(tokens) = body
        .pointer("/meta/tokens")
        .filter(|v| v.is_object() || v.is_array())
    {
        let input = tokens.get("input_tokens");
        let output = tokens.get("output_tokens");
        let genuine = |value: Option<&serde_json::Value>| -> Option<u64> {
            value.filter(|v| v.is_u64()).and_then(|v| v.as_u64())
        };
        let mut usage = serde_json::Map::new();
        if let Some(input) = input {
            usage.insert("input_tokens".to_string(), input.clone());
        }
        if let Some(output) = output {
            usage.insert("output_tokens".to_string(), output.clone());
        }
        if let (Some(input), Some(output)) = (genuine(input), genuine(output)) {
            usage.insert(
                "total_tokens".to_string(),
                serde_json::json!(input.saturating_add(output)),
            );
        }
        return Some(serde_json::Value::Object(usage));
    }
    // The `usage.total_tokens` branch: RAW passthrough — the strict
    // decoder judges the value's type (never a JS `Number()` coercion of
    // strings/floats).
    body.pointer("/usage/total_tokens")
        .map(|total| serde_json::json!({"total_tokens": total}))
}

/// Parses and validates a rerank response: exactly top_n rows, legal
/// unique indexes, finite scores, sorted score-desc index-asc.
pub fn parse_rerank(
    family: ProtocolFamily,
    top_n: usize,
    documents_len: usize,
    body: &serde_json::Value,
) -> Result<RerankOutcome, ProtocolError> {
    let rows = match family {
        // The dashscope NATIVE answer nests under output.results (the
        // incumbent's normalizeDashscopeNativeRerank folds it up).
        ProtocolFamily::DashscopeRerank if body.get("output").is_some() => body["output"]
            ["results"]
            .as_array()
            .ok_or_else(|| parse::invalid("dashscope rerank: no output.results[]".to_string()))?,
        _ => body["results"]
            .as_array()
            .ok_or_else(|| parse::invalid("rerank response: no results[]".to_string()))?,
    };
    if rows.len() != top_n {
        return Err(parse::invalid(format!(
            "rerank response carries {} rows for top_n {top_n}",
            rows.len()
        )));
    }
    let mut seen = vec![false; documents_len];
    let mut hits = Vec::with_capacity(rows.len());
    for row in rows {
        let index = row["index"]
            .as_u64()
            .ok_or_else(|| parse::invalid(format!("rerank row has no numeric index: {row}")))?
            as usize;
        if index >= documents_len {
            return Err(parse::invalid(format!(
                "rerank row index {index} out of range of {documents_len} documents"
            )));
        }
        if seen[index] {
            return Err(parse::invalid(format!(
                "rerank row index {index} appears twice"
            )));
        }
        seen[index] = true;
        // The incumbent's score field ladder `relevance_score ??
        // relevanceScore ?? score` is NULLISH: a JSON null passes through
        // to the next candidate.
        let score_value = [
            row.get("relevance_score"),
            row.get("relevanceScore"),
            row.get("score"),
        ]
        .into_iter()
        .flatten()
        .find(|v| !v.is_null())
        .ok_or_else(|| parse::invalid(format!("rerank row carries no score: {row}")))?;
        hits.push(RerankHit {
            index,
            score: parse::finite_f64(score_value, "rerank score")?,
        });
    }
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.index.cmp(&b.index))
    });
    Ok(RerankOutcome {
        results: hits,
        usage: normalize_rerank_usage(body),
    })
}
