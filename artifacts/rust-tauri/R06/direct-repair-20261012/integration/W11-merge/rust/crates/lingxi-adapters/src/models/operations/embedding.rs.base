//! Embedding dialects (R05-T06): openai-embeddings / ollama-embed /
//! gemini-embed / voyage-embeddings / minimax-embeddings — the incumbent
//! `core/model-operation-client.ts` embedding surface, ported rule for
//! rule (bounds, URL joins, auth shape, response validation).
//!
//! Fidelity notes (registered in §30):
//! - text bounds are RAW UTF-16 code-unit counts (the incumbent's
//!   `String.length`), NOT trimmed code-point counts — see
//!   [`super::assert_text_list`].
//! - `operationUrl`/`ollamaEmbedUrl` are case-SENSITIVE `endsWith` joins —
//!   deliberately NOT the chat plane's case-insensitive
//!   `append_provider_api_path`.
//! - the gemini batch URL interpolates the model id VERBATIM (the
//!   incumbent does not URL-encode it).
//! - the minimax 200-envelope check is `typeof status_code === "number"
//!   && status_code !== 0` — a STRING status code does NOT fail (ported
//!   exactly).
//! - GAP: the incumbent also validates the response dimensionality against
//!   `execution.model.dimensions` (the catalog entry's declared width);
//!   `ResolvedModelRoute` carries no such field, so only the request-side
//!   `dimensions` check runs here.

use lingxi_kernel::model_exchange::{ProtocolFamily, ResolvedModelRoute};
use lingxi_protocol::ProtocolError;

use super::{
    assert_text_list, encode_uri_component, operation_url, parse, require_group_id,
    OperationRequestPlan,
};
use crate::models::credentials::ApplicableAuth;
use crate::models::dispatch::BearerStyle;

/// The incumbent per-call input cap (`MAX_EMBED_INPUTS`).
pub const MAX_EMBED_INPUTS: usize = 128;

/// The voyage/minimax input-type hint (the incumbent `input_type` /
/// `type` field; default "document").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EmbeddingInputType {
    #[default]
    Document,
    Query,
}

impl EmbeddingInputType {
    fn voyage_wire(self) -> &'static str {
        match self {
            EmbeddingInputType::Document => "document",
            EmbeddingInputType::Query => "query",
        }
    }
    fn minimax_wire(self) -> &'static str {
        match self {
            EmbeddingInputType::Document => "db",
            EmbeddingInputType::Query => "query",
        }
    }
}

/// One embedding call's request (host-validated before any dialect sees
/// it — [`validate_embedding_request`]).
#[derive(Debug, Clone)]
pub struct EmbeddingRequest {
    pub inputs: Vec<String>,
    /// Requested output dimensionality (openai `dimensions`, ollama
    /// `dimensions`, gemini `outputDimensionality`, voyage `dimensions`);
    /// absent = the model default. MiniMax does NOT carry it (embo-01 is
    /// fixed 1536 — the incumbent omits the field).
    pub dimensions: Option<u32>,
    /// The ollama num_ctx DECLARED value (the incumbent contextWindow
    /// parameter); the dialect clamps/derives per the incumbent rule.
    pub context_window: Option<u32>,
    pub input_type: EmbeddingInputType,
}

/// A validated embedding answer: one vector per input, in input order.
#[derive(Debug, Clone)]
pub struct EmbeddingOutcome {
    pub vectors: Vec<Vec<f32>>,
    /// The usage object per the incumbent's `normalizeEmbeddingUsage`
    /// (`body.usage` verbatim when it is an object/array) — except minimax,
    /// whose usage is SYNTHESIZED from the top-level `total_tokens`.
    pub usage: Option<serde_json::Value>,
}

/// The incumbent validation (loud, before any wire work): the text list
/// through [`assert_text_list`] (label `texts`), dimensions a positive
/// integer ≤ 65536, contextWindow a positive integer ≤ 1048576 — with the
/// incumbent's verbatim refusal texts.
pub fn validate_embedding_request(request: &EmbeddingRequest) -> Result<(), ProtocolError> {
    assert_text_list(&request.inputs, "texts", MAX_EMBED_INPUTS)?;
    if let Some(dimensions) = request.dimensions {
        if dimensions == 0 || dimensions > 65_536 {
            return Err(parse::invalid(
                "dimensions must be a positive integer".to_string(),
            ));
        }
    }
    if let Some(window) = request.context_window {
        if window == 0 || window > 1_048_576 {
            return Err(parse::invalid(
                "contextWindow must be a positive integer".to_string(),
            ));
        }
    }
    Ok(())
}

/// The incumbent `ollamaEmbedUrl`: trailing slashes and a trailing `/v1`
/// stripped; a base already ending in `/api` gains `/embed`, anything else
/// gains `/api/embed`.
fn ollama_embed_url(endpoint: &str) -> String {
    let base = route_endpoint_trimmed(endpoint);
    let base = base.strip_suffix("/v1").unwrap_or(base);
    if base.ends_with("/api") {
        format!("{base}/embed")
    } else {
        format!("{base}/api/embed")
    }
}

fn route_endpoint_trimmed(endpoint: &str) -> &str {
    endpoint.trim_end_matches('/')
}

/// Builds the embedding request plan for the resolved route's family.
pub fn build_embedding(
    route: &ResolvedModelRoute,
    auth: &ApplicableAuth,
    request: &EmbeddingRequest,
) -> Result<OperationRequestPlan, ProtocolError> {
    validate_embedding_request(request)?;
    match route.protocol {
        ProtocolFamily::OpenAiEmbeddings => {
            let mut body = serde_json::json!({
                "model": route.model,
                "input": request.inputs,
                "encoding_format": "float",
            });
            if let Some(dimensions) = request.dimensions {
                body["dimensions"] = serde_json::json!(dimensions);
            }
            Ok(
                OperationRequestPlan::post_json(operation_url(&route.endpoint, "embeddings"), body)
                    .with_auth(auth, BearerStyle::AuthorizationBearer),
            )
        }
        ProtocolFamily::OllamaEmbed => {
            // The incumbent strips a trailing /v1 and strips the
            // Authorization header entirely (ollama is the keyless local
            // contract). num_ctx = clamp(max(declared, derived), 2048,
            // 32768), derived = ceil((maxTextChars*1.6 + 512)/1024)*1024
            // with maxTextChars the longest RAW UTF-16 length.
            let max_text_chars = request
                .inputs
                .iter()
                .map(|s| s.chars().map(|c| c.len_utf16()).sum::<usize>())
                .max()
                .unwrap_or(0) as f64;
            let derived = (((max_text_chars * 1.6 + 512.0) / 1024.0).ceil() * 1024.0) as u32;
            let num_ctx = request
                .context_window
                .unwrap_or(0)
                .max(derived)
                .clamp(2_048, 32_768);
            let mut body = serde_json::json!({
                "model": route.model,
                "input": request.inputs,
            });
            if let Some(dimensions) = request.dimensions {
                body["dimensions"] = serde_json::json!(dimensions);
            }
            body["options"] = serde_json::json!({ "num_ctx": num_ctx });
            Ok(OperationRequestPlan::post_json(
                ollama_embed_url(&route.endpoint),
                body,
            ))
        }
        ProtocolFamily::GeminiEmbed => {
            let mut requests = Vec::with_capacity(request.inputs.len());
            for text in &request.inputs {
                let mut entry = serde_json::json!({
                    "model": format!("models/{}", route.model),
                    "content": {"parts": [{"text": text}]},
                });
                if let Some(dimensions) = request.dimensions {
                    entry["outputDimensionality"] = serde_json::json!(dimensions);
                }
                requests.push(entry);
            }
            // The incumbent interpolates the model id VERBATIM (no URL
            // encoding): `${trimBaseUrl(base)}/models/${modelId}:batchEmbedContents`.
            let url = format!(
                "{}/models/{}:batchEmbedContents",
                route_endpoint_trimmed(&route.endpoint),
                route.model
            );
            Ok(
                OperationRequestPlan::post_json(url, serde_json::json!({"requests": requests}))
                    .with_auth(auth, BearerStyle::NamedHeader("x-goog-api-key")),
            )
        }
        ProtocolFamily::VoyageEmbeddings => {
            let mut body = serde_json::json!({
                "model": route.model,
                "input": request.inputs,
                "input_type": request.input_type.voyage_wire(),
            });
            if let Some(dimensions) = request.dimensions {
                body["dimensions"] = serde_json::json!(dimensions);
            }
            Ok(OperationRequestPlan::post_json(
                format!("{}/v1/embeddings", route_endpoint_trimmed(&route.endpoint)),
                body,
            )
            .with_auth(auth, BearerStyle::AuthorizationBearer))
        }
        ProtocolFamily::MinimaxEmbeddings => {
            // Origin-only URL (any configured path is discarded) + the
            // MANDATORY GroupId query (encodeURIComponent'd) — absent or
            // empty is the incumbent's verbatim loud build refusal.
            let origin = origin_of(&route.endpoint)?;
            let group_id = require_group_id(
                route,
                "MiniMax embeddings require a GroupId configured on the model entry (settings > \
                 providers > model > GroupId)",
            )?;
            let body = serde_json::json!({
                "model": route.model,
                "texts": request.inputs,
                "type": request.input_type.minimax_wire(),
            });
            Ok(OperationRequestPlan::post_json(
                format!(
                    "{origin}/v1/embeddings?GroupId={}",
                    encode_uri_component(group_id)
                ),
                body,
            )
            .with_auth(auth, BearerStyle::AuthorizationBearer))
        }
        other => Err(parse::invalid(format!(
            "protocol family {} does not serve embedding (the gateway's serves-matrix \
             refusal precedes dispatch; reaching here is a wiring bug)",
            other.config_name()
        ))),
    }
}

/// Parses and VALIDATES one embedding response per the incumbent checks:
/// one row per input, legal unique indexes, non-empty all-finite vectors,
/// consistent dimensionality, the requested dimensionality honored.
pub fn parse_embedding(
    family: ProtocolFamily,
    request: &EmbeddingRequest,
    body: &serde_json::Value,
) -> Result<EmbeddingOutcome, ProtocolError> {
    let expected = request.inputs.len();
    let rows: Vec<(usize, Vec<f32>)> = match family {
        ProtocolFamily::MinimaxEmbeddings => {
            // The 200-embedded error envelope: `typeof status === "number"
            // && status !== 0` — a STRING status code does NOT fail.
            if let Some(status) = body
                .pointer("/base_resp/status_code")
                .and_then(|v| v.as_f64())
            {
                if status != 0.0 {
                    let detail = body
                        .pointer("/base_resp/status_msg")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("{status}"));
                    return Err(parse::invalid(format!(
                        "MiniMax embeddings failed: {detail}"
                    )));
                }
            }
            let vectors = body["vectors"]
                .as_array()
                .ok_or_else(|| parse::invalid("minimax embeddings: no vectors[]".to_string()))?;
            vectors
                .iter()
                .enumerate()
                .map(|(i, v)| Ok((i, parse_vector(v, "minimax vectors[]")?)))
                .collect::<Result<Vec<_>, _>>()?
        }
        ProtocolFamily::GeminiEmbed => {
            let embeddings = body["embeddings"]
                .as_array()
                .ok_or_else(|| parse::invalid("gemini embed: no embeddings[]".to_string()))?;
            embeddings
                .iter()
                .enumerate()
                .map(|(i, e)| Ok((i, parse_vector(&e["values"], "gemini embeddings[].values")?)))
                .collect::<Result<Vec<_>, _>>()?
        }
        _ => {
            // openai / voyage / ollama: data[] (ollama: embeddings[]) with
            // per-row index; voyage/non-integer indexes fall back to the
            // row's POSITION (the incumbent's
            // `Number.isSafeInteger(row?.index) ? row.index : index`).
            if family == ProtocolFamily::OllamaEmbed {
                let embeddings = body["embeddings"]
                    .as_array()
                    .ok_or_else(|| parse::invalid("ollama embed: no embeddings[]".to_string()))?;
                embeddings
                    .iter()
                    .enumerate()
                    .map(|(i, v)| Ok((i, parse_vector(v, "ollama embeddings[]")?)))
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                let data = body["data"]
                    .as_array()
                    .ok_or_else(|| parse::invalid("embedding response: no data[]".to_string()))?;
                let mut rows = Vec::with_capacity(data.len());
                for (position, row) in data.iter().enumerate() {
                    let index = match row.get("index").and_then(|v| v.as_u64()) {
                        Some(index) => index as usize,
                        // Voyage omits index on some tiers: positional.
                        None if family == ProtocolFamily::VoyageEmbeddings => position,
                        None => {
                            return Err(parse::invalid(format!(
                                "embedding data[{position}] carries no index"
                            )))
                        }
                    };
                    rows.push((index, parse_vector(&row["embedding"], "data[].embedding")?));
                }
                rows
            }
        }
    };
    if rows.len() != expected {
        return Err(parse::invalid(format!(
            "embedding response carries {} rows for {expected} inputs",
            rows.len()
        )));
    }
    let mut placed: Vec<Option<Vec<f32>>> = vec![None; expected];
    for (index, vector) in rows {
        if index >= expected {
            return Err(parse::invalid(format!(
                "embedding row index {index} out of range of {expected} inputs"
            )));
        }
        if placed[index].replace(vector).is_some() {
            return Err(parse::invalid(format!(
                "embedding row index {index} appears twice"
            )));
        }
    }
    let mut vectors = Vec::with_capacity(expected);
    let mut width: Option<usize> = None;
    for slot in placed {
        let vector = slot.expect("row count checked; every slot filled exactly once");
        match width {
            None => width = Some(vector.len()),
            Some(w) if w != vector.len() => {
                return Err(parse::invalid(format!(
                    "embedding vectors disagree on dimensionality ({w} vs {})",
                    vector.len()
                )));
            }
            _ => {}
        }
        vectors.push(vector);
    }
    if let (Some(requested), Some(actual)) = (request.dimensions, width) {
        if requested as usize != actual {
            return Err(parse::invalid(format!(
                "embedding dimensionality {actual} does not honor the requested {requested}"
            )));
        }
    }
    let usage = if family == ProtocolFamily::MinimaxEmbeddings {
        // The incumbent's `Number(body?.total_tokens)` finite →
        // `{ total_tokens }`, made strict (R05 RR1 F22): the RAW value
        // passes through and the strict decoder judges its type — no f64
        // round-trip (integer precision survives; a string/float is
        // invalid, never a coerced number; a JSON null is absent).
        body.get("total_tokens")
            .map(|total| serde_json::json!({"total_tokens": total}))
    } else {
        // `body?.usage && typeof body.usage === "object"` (arrays count as
        // objects in JS; null is falsy and drops out).
        body.get("usage")
            .filter(|v| v.is_object() || v.is_array())
            .cloned()
    };
    Ok(EmbeddingOutcome { vectors, usage })
}

fn parse_vector(value: &serde_json::Value, what: &str) -> Result<Vec<f32>, ProtocolError> {
    let array = value
        .as_array()
        .ok_or_else(|| parse::invalid(format!("{what} is not an array")))?;
    if array.is_empty() {
        return Err(parse::invalid(format!("{what} is an EMPTY vector")));
    }
    array
        .iter()
        .map(|v| Ok(parse::finite_f64(v, what)? as f32))
        .collect()
}

/// The scheme://host[:port] prefix (the incumbent's `new URL(base).origin`
/// for the minimax/dashscope origin-only rules): trailing slashes stripped,
/// userinfo discarded, the default port elided, refusal texts verbatim.
/// (JS also lowercases scheme+host and normalizes exotic forms; an endpoint
/// is a validated config value, so the plain split suffices — registered.)
pub(crate) fn origin_of(endpoint: &str) -> Result<String, ProtocolError> {
    let base = endpoint.trim_end_matches('/');
    if base.is_empty() {
        return Err(parse::invalid(
            "Provider base URL is unavailable".to_string(),
        ));
    }
    let invalid = || parse::invalid("Provider base URL is invalid".to_string());
    let (scheme, rest) = base.split_once("://").ok_or_else(invalid)?;
    if scheme.is_empty() {
        return Err(invalid());
    }
    let authority = rest.split('/').next().expect("split yields a first part");
    // URL.origin discards userinfo.
    let host_port = authority.rsplit('@').next().expect("rsplit yields a part");
    if host_port.is_empty() {
        return Err(invalid());
    }
    // URL.origin elides the default port.
    let host_port = match scheme.to_ascii_lowercase().as_str() {
        "https" => host_port.strip_suffix(":443").unwrap_or(host_port),
        "http" => host_port.strip_suffix(":80").unwrap_or(host_port),
        _ => host_port,
    };
    Ok(format!("{scheme}://{host_port}"))
}
