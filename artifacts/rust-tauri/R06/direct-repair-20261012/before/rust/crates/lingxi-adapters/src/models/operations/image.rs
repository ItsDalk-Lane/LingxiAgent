//! Image dialects (R05-T06): openai-images / openai-codex-responses-image /
//! volcengine-images / minimax-images / dashscope-images /
//! gemini-generate-content-image / agnes-images — the incumbent
//! `core/media-adapters/*.ts` image surfaces, ported rule for rule (size
//! resolution incl. [`super::tiers`], auth shape, body construction,
//! refusal texts, response collection).
//!
//! Layering decisions (registered in §30):
//! - reference inputs arrive as [`ImageReference`]; the SERVICE layer has
//!   already read local files into `Bytes`/`DataUrl` (the incumbent's
//!   `fs.readFileSync` happens host-side, never in the dialect). A
//!   `RemoteUrl` reference to the gemini family is a loud wiring refusal:
//!   the incumbent downloads-and-inlines it, the port requires the service
//!   to pre-fetch it through the egress guard into bytes first (the wire
//!   body is identical; the egress is never unguarded here).
//! - base64 decoding is STRICT (RFC 4648, canonical padding). The
//!   incumbent's `Buffer.from(b64, "base64")` silently tolerates garbage;
//!   the port refuses a malformed payload loudly instead of materializing
//!   corrupt bytes.
//! - a submit response carrying neither products nor a provider task id is
//!   a loud failure (the incumbent fabricates a local tracking id there —
//!   polling a fabricated id would be an unverifiable operation).
//! - non-2xx answers classify through the shared dispatch table (the
//!   incumbent adapters hand-roll `API error {status}` texts; the shared
//!   classification is the registered discipline of this crate).

use base64::Engine as _;
use lingxi_kernel::model_exchange::{ProtocolFamily, ResolvedModelRoute};
use lingxi_protocol::{ErrorCode, ProtocolError};

use super::tiers::{
    self, FlexibleConstraints, OpenAiSizeInput, OpenAiSizeOptions, CODEX_IMAGE_RESOLUTION_TIERS,
    OPENAI_FLEXIBLE_IMAGE_RATIOS, OPENAI_FLEXIBLE_RESOLUTION_TIERS, OPENAI_STANDARD_IMAGE_RATIOS,
    OPENAI_STANDARD_RESOLUTION_TIERS,
};
use super::{
    encode_multipart, encode_uri_component, parse, ImageReference, ImageSubmitOutcome,
    MediaProductRef, MultipartPart, OperationRequestPlan, TaskPollOutcome,
};
use crate::models::credentials::ApplicableAuth;
use crate::models::dispatch::BearerStyle;
use crate::models::openai_codex_responses::extract_account_id_from_token;

const FORMAT_TO_MIME_OPENAI: [(&str, &str); 3] = [
    ("png", "image/png"),
    ("jpeg", "image/jpeg"),
    ("webp", "image/webp"),
];
const FORMAT_TO_MIME_VOLCENGINE: [(&str, &str); 2] = [("png", "image/png"), ("jpeg", "image/jpeg")];

/// One image-generation request. The service layer collapses the
/// incumbent's parameter aliases (`aspect_ratio`/`aspectRatio`/`ratio` →
/// `ratio`, `negative_prompt`/`negativePrompt` → `negative_prompt`,
/// `prompt_extend`/`promptExtend` → `prompt_extend`,
/// `guidance_scale`/`guidanceScale` → `guidance_scale`) BEFORE the dialect
/// sees them; the provider-defaults overlay rides separately (the dialects
/// read the same keys the incumbent reads, in the same order).
#[derive(Debug, Clone, Default)]
pub struct ImageRequest {
    pub prompt: String,
    pub size: Option<String>,
    pub resolution: Option<String>,
    pub ratio: Option<String>,
    pub n: Option<u32>,
    pub format: Option<String>,
    pub quality: Option<String>,
    pub style: Option<String>,
    pub background: Option<String>,
    pub output_compression: Option<u32>,
    pub moderation: Option<String>,
    pub watermark: Option<bool>,
    pub guidance_scale: Option<f64>,
    pub seed: Option<i64>,
    pub negative_prompt: Option<String>,
    pub prompt_extend: Option<bool>,
    pub prompt_optimizer: Option<bool>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// The codex family's responses-model override (`params.responsesModel`).
    pub responses_model: Option<String>,
    pub references: Vec<ImageReference>,
}

impl ImageRequest {
    /// `params.n || 1` (JS: a zero `n` is falsy and maps to 1).
    fn n_or_one(&self) -> u32 {
        self.n.filter(|n| *n > 0).unwrap_or(1)
    }
}

/// Builds the image-submit plan for the resolved route's family.
pub fn build_image_submit(
    route: &ResolvedModelRoute,
    auth: &ApplicableAuth,
    request: &ImageRequest,
    provider_defaults: &serde_json::Value,
) -> Result<OperationRequestPlan, ProtocolError> {
    match route.protocol {
        ProtocolFamily::OpenAiImages => build_openai_image(route, auth, request, provider_defaults),
        ProtocolFamily::OpenAiCodexResponsesImage => {
            build_openai_codex_image(route, auth, request, provider_defaults)
        }
        ProtocolFamily::VolcengineImages => {
            build_volcengine_image(route, auth, request, provider_defaults)
        }
        ProtocolFamily::MinimaxImages => build_minimax_image(route, auth, request),
        ProtocolFamily::DashscopeImages => build_dashscope_image(route, auth, request),
        ProtocolFamily::GeminiGenerateContentImage => {
            build_gemini_image(route, auth, request, provider_defaults)
        }
        ProtocolFamily::AgnesImages => build_agnes_image(route, auth, request, provider_defaults),
        other => Err(parse::invalid(format!(
            "protocol family {} does not serve image generation (the gateway's serves-matrix \
             refusal precedes dispatch; reaching here is a wiring bug)",
            other.config_name()
        ))),
    }
}

/// Parses the image-submit answer per family. Codex answers are an SSE
/// aggregate, not JSON — they go through
/// [`parse_openai_codex_image_sse`] instead.
pub fn parse_image_submit(
    route: &ResolvedModelRoute,
    request: &ImageRequest,
    provider_defaults: &serde_json::Value,
    body: &serde_json::Value,
) -> Result<ImageSubmitOutcome, ProtocolError> {
    match route.protocol {
        ProtocolFamily::OpenAiImages => parse_openai_image(route, request, provider_defaults, body),
        ProtocolFamily::VolcengineImages => {
            parse_volcengine_image(route, request, provider_defaults, body)
        }
        ProtocolFamily::MinimaxImages => parse_minimax_image(body),
        ProtocolFamily::DashscopeImages => parse_dashscope_image_submit(body),
        ProtocolFamily::GeminiGenerateContentImage => parse_gemini_image(body),
        ProtocolFamily::AgnesImages => parse_agnes_image(body),
        other => Err(parse::invalid(format!(
            "protocol family {} has no JSON image-submit parse (wiring bug)",
            other.config_name()
        ))),
    }
}

// ── openai-images (core/media-adapters/openai.ts) ────────────────────────

const DALL_E_3_SIZES: [&str; 3] = ["1024x1024", "1792x1024", "1024x1792"];
const DALL_E_3_SIZE_BY_RATIO: [(&str, &str); 3] = [
    ("1:1", "1024x1024"),
    ("16:9", "1792x1024"),
    ("9:16", "1024x1792"),
];

fn is_dall_e_3(model: &str) -> bool {
    model.to_lowercase() == "dall-e-3"
}

/// `normalizeDallE3SizeInput`: the three exact sizes pass; `1k`/`auto`
/// (lowercased) map through the ratio table; anything else refuses.
fn normalize_dall_e_3_size_input(
    value: Option<&str>,
    ratio: Option<&str>,
) -> Result<Option<String>, ProtocolError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    let raw = value.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    if DALL_E_3_SIZES.contains(&raw) {
        return Ok(Some(raw.to_string()));
    }
    let normalized = raw.to_lowercase();
    if normalized == "1k" || normalized == "auto" {
        let ratio = ratio.unwrap_or("1:1");
        let size = DALL_E_3_SIZE_BY_RATIO
            .iter()
            .find(|(label, _)| *label == ratio)
            .map(|(_, size)| *size)
            .unwrap_or("1024x1024");
        return Ok(Some(size.to_string()));
    }
    Err(parse::invalid(format!(
        "OpenAI DALL-E 3 size \"{raw}\" is unsupported"
    )))
}

/// `resolveDallE3Size`: ratio validated against the ratio→size table (raw
/// string lookup — no normalization), then size/resolution input, then the
/// ratio default.
fn resolve_dall_e_3_size(
    request: &ImageRequest,
    provider_defaults: &serde_json::Value,
) -> Result<Option<String>, ProtocolError> {
    let ratio = request
        .ratio
        .as_deref()
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .or_else(|| tiers::truthy_json_string(provider_defaults.get("aspect_ratio")))
        .or_else(|| tiers::truthy_json_string(provider_defaults.get("aspectRatio")))
        .or_else(|| tiers::truthy_json_string(provider_defaults.get("ratio")));
    if let Some(ratio) = ratio.as_deref() {
        if !DALL_E_3_SIZE_BY_RATIO
            .iter()
            .any(|(label, _)| *label == ratio)
        {
            return Err(parse::invalid(format!(
                "OpenAI DALL-E 3 ratio \"{ratio}\" is unsupported"
            )));
        }
    }
    let explicit = request
        .size
        .as_deref()
        .filter(|v| !v.is_empty())
        .or_else(|| request.resolution.as_deref().filter(|v| !v.is_empty()))
        .map(str::to_string)
        .or_else(|| tiers::truthy_json_string(provider_defaults.get("size")))
        .or_else(|| tiers::truthy_json_string(provider_defaults.get("resolution")));
    if let Some(size) = normalize_dall_e_3_size_input(explicit.as_deref(), ratio.as_deref())? {
        return Ok(Some(size));
    }
    Ok(ratio.and_then(|ratio| {
        DALL_E_3_SIZE_BY_RATIO
            .iter()
            .find(|(label, _)| *label == ratio)
            .map(|(_, size)| size.to_string())
    }))
}

/// The incumbent `imageJsonRef`: an http(s) URL (case-insensitive scheme)
/// → `{image_url}`, a `file-…` id → `{file_id}`; anything else is not
/// JSON-expressible. The incumbent's file-id test is a PREFIX regex
/// (`/^file-[A-Za-z0-9_-]+/`, not anchored at the end) — ported exactly.
fn openai_json_ref(reference: &ImageReference) -> Option<serde_json::Value> {
    match reference {
        ImageReference::RemoteUrl(url) => Some(serde_json::json!({ "image_url": url })),
        ImageReference::ProviderRef(id) => {
            let rest = id.strip_prefix("file-")?;
            let first = rest.bytes().next()?;
            if first.is_ascii_alphanumeric() || first == b'_' || first == b'-' {
                Some(serde_json::json!({ "file_id": id }))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn build_openai_image(
    route: &ResolvedModelRoute,
    auth: &ApplicableAuth,
    request: &ImageRequest,
    provider_defaults: &serde_json::Value,
) -> Result<OperationRequestPlan, ProtocolError> {
    let model = route.model.as_str();
    let dall_e_3 = is_dall_e_3(model);
    let mut body = if dall_e_3 {
        serde_json::json!({
            "model": model,
            "prompt": request.prompt,
            "n": request.n_or_one(),
            "response_format": "b64_json",
        })
    } else {
        let output_format = request
            .format
            .as_deref()
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .or_else(|| tiers::truthy_json_string(provider_defaults.get("format")))
            .unwrap_or_else(|| "jpeg".to_string());
        serde_json::json!({
            "model": model,
            "prompt": request.prompt,
            "n": request.n_or_one(),
            "output_format": output_format,
        })
    };

    let size = if dall_e_3 {
        resolve_dall_e_3_size(request, provider_defaults)?
    } else {
        let flexible = model.starts_with("gpt-image-2");
        let flexible_ratios: &[&str] = &OPENAI_FLEXIBLE_IMAGE_RATIOS;
        let standard_ratios: &[&str] = &OPENAI_STANDARD_IMAGE_RATIOS;
        let flexible_tiers: &[&str] = &OPENAI_FLEXIBLE_RESOLUTION_TIERS;
        let standard_tiers: &[&str] = &OPENAI_STANDARD_RESOLUTION_TIERS;
        tiers::resolve_openai_image_size(&OpenAiSizeInput {
            size: request.size.as_deref(),
            resolution: request.resolution.as_deref(),
            ratio: request.ratio.as_deref(),
            provider_defaults,
            options: OpenAiSizeOptions {
                source_name: "OpenAI image",
                flexible,
                supported_ratios: Some(if flexible {
                    flexible_ratios
                } else {
                    standard_ratios
                }),
                supported_resolutions: Some(if flexible {
                    flexible_tiers
                } else {
                    standard_tiers
                }),
                default_ratio: Some("3:2"),
                default_resolution: Some(if flexible { "2K" } else { "1K" }),
                constraints: FlexibleConstraints::default(),
            },
        })?
    };
    if let Some(size) = size {
        body["size"] = serde_json::Value::from(size);
    }

    let quality = request
        .quality
        .as_deref()
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .or_else(|| tiers::truthy_json_string(provider_defaults.get("quality")));
    if let Some(quality) = quality {
        body["quality"] = serde_json::Value::from(quality);
    }

    if dall_e_3 {
        let style = request
            .style
            .as_deref()
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .or_else(|| tiers::truthy_json_string(provider_defaults.get("style")));
        if let Some(style) = style {
            body["style"] = serde_json::Value::from(style);
        }
    } else {
        let background = request
            .background
            .as_deref()
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .or_else(|| tiers::truthy_json_string(provider_defaults.get("background")));
        if let Some(background) = background {
            body["background"] = serde_json::Value::from(background);
        }
        let output_compression = request
            .output_compression
            .map(serde_json::Value::from)
            .or_else(|| {
                provider_defaults
                    .get("output_compression")
                    .filter(|v| !v.is_null())
                    .cloned()
            });
        if let Some(output_compression) = output_compression {
            body["output_compression"] = output_compression;
        }
        let moderation = request
            .moderation
            .as_deref()
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .or_else(|| tiers::truthy_json_string(provider_defaults.get("moderation")));
        if let Some(moderation) = moderation {
            body["moderation"] = serde_json::Value::from(moderation);
        }
    }

    if dall_e_3 && !request.references.is_empty() {
        return Err(parse::invalid(
            "OpenAI DALL-E 3 does not support reference images".to_string(),
        ));
    }

    let base = route.endpoint.trim_end_matches('/');
    let url = if request.references.is_empty() {
        format!("{base}/images/generations")
    } else {
        format!("{base}/images/edits")
    };
    if request.references.is_empty() {
        return Ok(OperationRequestPlan::post_json(url, body)
            .with_auth(auth, BearerStyle::AuthorizationBearer));
    }
    // Edit call: every reference a URL/file_id → the JSON body with an
    // `images` array; every reference host-read bytes → multipart
    // (`image[]` parts); anything mixed refuses exactly like the incumbent.
    let json_refs: Option<Vec<serde_json::Value>> =
        request.references.iter().map(openai_json_ref).collect();
    if let Some(json_refs) = json_refs {
        body["images"] = serde_json::Value::Array(json_refs);
        return Ok(OperationRequestPlan::post_json(url, body)
            .with_auth(auth, BearerStyle::AuthorizationBearer));
    }
    let all_bytes = request
        .references
        .iter()
        .all(|r| matches!(r, ImageReference::Bytes { .. }));
    if !all_bytes {
        return Err(parse::invalid(
            "OpenAI image edit reference must be an HTTP(S) URL, file_id, or local image file \
             path"
                .to_string(),
        ));
    }
    let mut parts = Vec::new();
    if let Some(object) = body.as_object() {
        for (key, value) in object {
            if value.is_null() {
                continue;
            }
            let rendered = match value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            parts.push(MultipartPart::field(key, &rendered));
        }
    }
    for reference in &request.references {
        let ImageReference::Bytes {
            bytes,
            mime,
            filename,
        } = reference
        else {
            continue;
        };
        parts.push(MultipartPart::file(
            "image[]",
            filename,
            mime,
            bytes.clone(),
        ));
    }
    let (multipart_body, content_type) = encode_multipart(&parts)?;
    Ok(OperationRequestPlan {
        method: "POST",
        url,
        headers: vec![("content-type".to_string(), content_type)],
        body: multipart_body,
    }
    .with_auth(auth, BearerStyle::AuthorizationBearer))
}

fn parse_openai_image(
    route: &ResolvedModelRoute,
    request: &ImageRequest,
    provider_defaults: &serde_json::Value,
    body: &serde_json::Value,
) -> Result<ImageSubmitOutcome, ProtocolError> {
    // `data.data || []`: a missing data array is the EMPTY answer, refused
    // with the incumbent's verbatim text.
    let data = body.get("data").and_then(|v| v.as_array());
    let Some(data) = data.filter(|data| !data.is_empty()) else {
        return Err(parse::invalid("API returned no images".to_string()));
    };
    let mime = if is_dall_e_3(&route.model) {
        "image/png"
    } else {
        let output_format = request
            .format
            .as_deref()
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .or_else(|| tiers::truthy_json_string(provider_defaults.get("format")))
            .unwrap_or_else(|| "jpeg".to_string());
        FORMAT_TO_MIME_OPENAI
            .iter()
            .find(|(format, _)| *format == output_format)
            .map(|(_, mime)| *mime)
            .unwrap_or("image/png")
    };
    let mut products = Vec::with_capacity(data.len());
    for (index, item) in data.iter().enumerate() {
        let b64 = item["b64_json"].as_str().ok_or_else(|| {
            parse::invalid(format!("openai image data[{index}] carries no b64_json"))
        })?;
        products.push(MediaProductRef::Bytes {
            bytes: decode_base64_strict(b64, "openai image b64_json")?,
            mime: mime.to_string(),
        });
    }
    Ok(ImageSubmitOutcome::Done { products })
}

// ── openai-codex-responses-image (core/media-adapters/openai-codex.ts) ───

/// The incumbent adapter-injected instruction (verbatim).
pub const CODEX_IMAGE_INSTRUCTIONS: &str =
    "Generate or edit the requested image and return the image result.";

fn build_openai_codex_image(
    route: &ResolvedModelRoute,
    auth: &ApplicableAuth,
    request: &ImageRequest,
    provider_defaults: &serde_json::Value,
) -> Result<OperationRequestPlan, ProtocolError> {
    // The account id is a hard requirement of this family (same posture as
    // the codex chat family): only an OAuth bearer JWT carries it.
    let ApplicableAuth::Bearer(token) = auth else {
        return Err(ProtocolError::new(
            ErrorCode::Unauthorized,
            "openai-codex-responses-image requires an OAuth bearer credential (the codex \
             account id rides its JWT); this route's credential shape cannot serve it"
                .to_string(),
            false,
        ));
    };
    let Some(account_id) = extract_account_id_from_token(token) else {
        return Err(ProtocolError::new(
            ErrorCode::Unauthorized,
            "Provider \"openai-codex-oauth\" missing ChatGPT account id. Please log in again."
                .to_string(),
            false,
        ));
    };

    let output_format = request
        .format
        .as_deref()
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .or_else(|| tiers::truthy_json_string(provider_defaults.get("format")))
        .unwrap_or_else(|| "png".to_string());
    let mut tool = serde_json::json!({
        "type": "image_generation",
        "output_format": output_format,
    });
    let tool_size = tiers::resolve_openai_image_size(&OpenAiSizeInput {
        size: request.size.as_deref(),
        resolution: request.resolution.as_deref(),
        ratio: request.ratio.as_deref(),
        provider_defaults,
        options: OpenAiSizeOptions {
            source_name: "Codex image",
            flexible: true,
            supported_ratios: Some(&OPENAI_FLEXIBLE_IMAGE_RATIOS),
            supported_resolutions: Some(&CODEX_IMAGE_RESOLUTION_TIERS),
            default_ratio: Some("3:2"),
            default_resolution: Some("2K"),
            constraints: FlexibleConstraints {
                max_edge: Some(2048),
                max_pixels: Some(2048 * 2048),
                ..FlexibleConstraints::default()
            },
        },
    })?;
    if let Some(tool_size) = tool_size {
        tool["size"] = serde_json::Value::from(tool_size);
    }
    let quality = request
        .quality
        .as_deref()
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .or_else(|| tiers::truthy_json_string(provider_defaults.get("quality")));
    if let Some(quality) = quality {
        tool["quality"] = serde_json::Value::from(quality);
    }
    let background = request
        .background
        .as_deref()
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .or_else(|| tiers::truthy_json_string(provider_defaults.get("background")));
    if let Some(background) = background {
        tool["background"] = serde_json::Value::from(background);
    }
    let output_compression = request
        .output_compression
        .map(serde_json::Value::from)
        .or_else(|| {
            provider_defaults
                .get("output_compression")
                .filter(|v| !v.is_null())
                .cloned()
        });
    if let Some(output_compression) = output_compression {
        tool["output_compression"] = output_compression;
    }

    let mut content = vec![serde_json::json!({ "type": "input_text", "text": request.prompt })];
    for reference in &request.references {
        content.push(serde_json::json!({
            "type": "input_image",
            "image_url": reference.wire_string(),
        }));
    }
    let responses_model = request
        .responses_model
        .as_deref()
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .or_else(|| tiers::truthy_json_string(provider_defaults.get("responsesModel")))
        .or_else(|| tiers::truthy_json_string(provider_defaults.get("mainlineModel")))
        .unwrap_or_else(|| "gpt-5.5".to_string());
    let body = serde_json::json!({
        "model": responses_model,
        "store": false,
        "stream": true,
        "instructions": CODEX_IMAGE_INSTRUCTIONS,
        "input": [{ "role": "user", "content": content }],
        "tools": [tool],
        "tool_choice": "auto",
        "parallel_tool_calls": false,
    });
    let url = super::super::openai_codex_responses::resolve_codex_responses_url(&route.endpoint);
    Ok(OperationRequestPlan::post_json(url, body)
        .with_auth(auth, BearerStyle::AuthorizationBearer)
        .with_header("chatgpt-account-id", &account_id)
        .with_header("OpenAI-Beta", "responses=experimental")
        .with_header("originator", "pi"))
}

/// The incumbent `readStreamingPayload` block splitter: blocks separated by
/// `\r?\n\r?\n`; per block the `data:` lines (prefix stripped,
/// start-trimmed) join with `\n`; empty/`[DONE]` blocks and unparseable
/// JSON events are skipped (incumbent behavior — a garbage event is not a
/// failure, only a missing image is).
fn parse_codex_sse_events(text: &str) -> Vec<serde_json::Value> {
    let mut events = Vec::new();
    let bytes = text.as_bytes();
    let mut rest = text;
    loop {
        let rest_bytes = rest.as_bytes();
        let mut separator: Option<(usize, usize)> = None;
        let mut index = 0usize;
        while index < rest_bytes.len() {
            if matches!(rest_bytes[index], b'\r' | b'\n') {
                let mut end = index;
                if end < rest_bytes.len() && rest_bytes[end] == b'\r' {
                    end += 1;
                }
                if end < rest_bytes.len() && rest_bytes[end] == b'\n' {
                    end += 1;
                } else {
                    index += 1;
                    continue;
                }
                if end < rest_bytes.len() && rest_bytes[end] == b'\r' {
                    end += 1;
                }
                if end < rest_bytes.len() && rest_bytes[end] == b'\n' {
                    end += 1;
                    separator = Some((index, end));
                    break;
                }
            }
            index += 1;
        }
        let (block, remainder) = match separator {
            Some((start, end)) => (&rest[..start], &rest[end..]),
            None => (rest, ""),
        };
        let data = block
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line))
            .filter(|line| line.starts_with("data:"))
            .map(|line| line[5..].trim_start())
            .collect::<Vec<_>>()
            .join("\n");
        let data = data.trim();
        if !data.is_empty() && data != "[DONE]" {
            if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                events.push(event);
            }
        }
        if remainder.is_empty() {
            break;
        }
        rest = remainder;
    }
    let _ = bytes;
    events
}

/// The incumbent `collectImageResults`: a recursive, cycle-safe (here:
/// serde_json values are parse-depth-bounded already), order-preserving
/// dedup walk collecting `image_generation_call.result` and `b64_json`
/// strings.
fn collect_image_results(data: &serde_json::Value) -> Vec<String> {
    fn visit(
        value: &serde_json::Value,
        seen: &mut std::collections::HashSet<String>,
        out: &mut Vec<String>,
    ) {
        match value {
            serde_json::Value::Array(items) => {
                for item in items {
                    visit(item, seen, out);
                }
            }
            serde_json::Value::Object(map) => {
                if map.get("type").and_then(|t| t.as_str()) == Some("image_generation_call") {
                    if let Some(result) = map.get("result").and_then(|r| r.as_str()) {
                        if seen.insert(result.to_string()) {
                            out.push(result.to_string());
                        }
                        return;
                    }
                }
                if let Some(b64) = map.get("b64_json").and_then(|b| b.as_str()) {
                    if seen.insert(b64.to_string()) {
                        out.push(b64.to_string());
                    }
                    return;
                }
                for child in map.values() {
                    visit(child, seen, out);
                }
            }
            _ => {}
        }
    }
    let root = data
        .get("output")
        .filter(|v| json_truthy(v))
        .or_else(|| data.pointer("/response/output").filter(|v| json_truthy(v)))
        .unwrap_or(data);
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    visit(root, &mut seen, &mut out);
    out
}

/// Parses the codex image SSE aggregate (the bounded body text) into
/// products; an aggregate without any image is the incumbent's
/// `API returned no images`.
pub fn parse_openai_codex_image_sse(
    request: &ImageRequest,
    provider_defaults: &serde_json::Value,
    text: &str,
) -> Result<Vec<MediaProductRef>, ProtocolError> {
    let events = parse_codex_sse_events(text);
    let aggregate = serde_json::json!({ "output": events });
    let images = collect_image_results(&aggregate);
    if images.is_empty() {
        return Err(parse::invalid("API returned no images".to_string()));
    }
    let output_format = request
        .format
        .as_deref()
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .or_else(|| tiers::truthy_json_string(provider_defaults.get("format")))
        .unwrap_or_else(|| "png".to_string());
    let mime = FORMAT_TO_MIME_OPENAI
        .iter()
        .find(|(format, _)| *format == output_format)
        .map(|(_, mime)| *mime)
        .unwrap_or("image/png");
    images
        .iter()
        .map(|b64| {
            Ok(MediaProductRef::Bytes {
                bytes: decode_base64_strict(b64, "codex image result")?,
                mime: mime.to_string(),
            })
        })
        .collect()
}

// ── volcengine-images (core/media-adapters/volcengine.ts) ────────────────

const SEEDREAM_RATIOS: [&str; 8] = ["1:1", "4:3", "3:4", "16:9", "9:16", "3:2", "2:3", "21:9"];

const SEEDREAM_SIZE_TABLE: [(&str, &str, &str); 24] = [
    ("1K", "1:1", "1024x1024"),
    ("1K", "4:3", "1152x864"),
    ("1K", "3:4", "864x1152"),
    ("1K", "16:9", "1280x720"),
    ("1K", "9:16", "720x1280"),
    ("1K", "3:2", "1248x832"),
    ("1K", "2:3", "832x1248"),
    ("1K", "21:9", "1536x656"),
    ("2K", "1:1", "2048x2048"),
    ("2K", "4:3", "2304x1728"),
    ("2K", "3:4", "1728x2304"),
    ("2K", "16:9", "2736x1536"),
    ("2K", "9:16", "1536x2736"),
    ("2K", "3:2", "2496x1664"),
    ("2K", "2:3", "1664x2496"),
    ("2K", "21:9", "3136x1344"),
    ("4K", "1:1", "4096x4096"),
    ("4K", "4:3", "3456x2592"),
    ("4K", "3:4", "2592x3456"),
    ("4K", "16:9", "3840x2160"),
    ("4K", "9:16", "2160x3840"),
    ("4K", "3:2", "3840x2560"),
    ("4K", "2:3", "2560x3840"),
    ("4K", "21:9", "4096x1760"),
];

/// The incumbent `getModelCapabilities` (substring rules on the lowercased
/// id, verbatim).
struct SeedreamCapabilities {
    supports_output_format: bool,
    supports_guidance_scale: bool,
    supports_seed: bool,
    supports_reference_images: bool,
    supported_resolutions: &'static [&'static str],
    default_resolution: &'static str,
}

fn seedream_capabilities(model: &str) -> SeedreamCapabilities {
    let id = model.to_lowercase();
    let is_seedream_5 = id.contains("seedream-5-0") || id.contains("seedream5.0");
    let is_seedream_3 = id.contains("seedream-3-0") || id.contains("seedream3.0");
    SeedreamCapabilities {
        supports_output_format: is_seedream_5,
        supports_guidance_scale: is_seedream_3,
        supports_seed: is_seedream_3,
        supports_reference_images: !is_seedream_3,
        supported_resolutions: if is_seedream_3 {
            &["1K"]
        } else {
            &["1K", "2K", "4K"]
        },
        default_resolution: if is_seedream_3 { "1K" } else { "4K" },
    }
}

/// `normalizeSeedreamSizeTier`: `^([124])\s*k$` (lowercased) → `{d}K`;
/// anything else passes through as the trimmed raw string.
fn normalize_seedream_size_tier(value: &str) -> String {
    let raw = value.trim();
    let lower = raw.to_lowercase();
    let bytes = lower.as_bytes();
    if bytes.len() >= 2 && matches!(bytes[0], b'1' | b'2' | b'4') {
        let mut index = 1;
        while index < bytes.len() && (bytes[index] as char).is_whitespace() {
            index += 1;
        }
        if index == bytes.len() - 1 && bytes[index] == b'k' {
            return format!("{}K", bytes[0] as char);
        }
    }
    raw.to_string()
}

/// `isPixelSize`: `^\d{3,5}x\d{3,5}$` (x case-insensitive).
fn is_seedream_pixel_size(value: &str) -> bool {
    let bytes = value.trim().as_bytes();
    let mut parts = Vec::new();
    let mut current = Vec::new();
    for &b in bytes {
        if b == b'x' || b == b'X' {
            parts.push(std::mem::take(&mut current));
        } else if b.is_ascii_digit() {
            current.push(b);
        } else {
            return false;
        }
    }
    parts.push(current);
    parts.len() == 2 && parts.iter().all(|p| (3..=5).contains(&p.len()))
}

/// `resolveSize` (volcengine): the tier table lookup with the incumbent
/// refusal texts.
fn resolve_seedream_size(
    size: Option<&str>,
    aspect_ratio: Option<&str>,
    provider_defaults: &serde_json::Value,
    model: &str,
    capabilities: &SeedreamCapabilities,
) -> Result<String, ProtocolError> {
    let supported = capabilities.supported_resolutions;
    let effective_ratio = aspect_ratio
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .or_else(|| tiers::truthy_json_string(provider_defaults.get("aspect_ratio")))
        .or_else(|| tiers::truthy_json_string(provider_defaults.get("ratio")))
        .unwrap_or_else(|| "3:2".to_string());
    let explicit = size
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .or_else(|| tiers::truthy_json_string(provider_defaults.get("size")))
        .or_else(|| tiers::truthy_json_string(provider_defaults.get("resolution")))
        .unwrap_or_else(|| capabilities.default_resolution.to_string());
    let effective_size = normalize_seedream_size_tier(&explicit);

    if is_seedream_pixel_size(&effective_size) {
        let allowed = SEEDREAM_SIZE_TABLE
            .iter()
            .filter(|(tier, _, _)| supported.contains(tier))
            .map(|(_, _, size)| *size)
            .collect::<Vec<_>>();
        if !allowed.contains(&effective_size.as_str()) {
            return Err(parse::invalid(format!(
                "Seedream size \"{effective_size}\" is unsupported for model \"{model}\""
            )));
        }
        return Ok(effective_size);
    }
    if !supported.contains(&effective_size.as_str()) {
        return Err(parse::invalid(format!(
            "Seedream resolution \"{explicit}\" is unsupported for model \"{model}\"; supported \
             resolutions: {}",
            supported.join(", ")
        )));
    }
    if !SEEDREAM_RATIOS.contains(&effective_ratio.as_str()) {
        return Err(parse::invalid(format!(
            "Seedream ratio \"{effective_ratio}\" is unsupported"
        )));
    }
    Ok(SEEDREAM_SIZE_TABLE
        .iter()
        .find(|(tier, ratio, _)| *tier == effective_size && *ratio == effective_ratio)
        .map(|(_, _, size)| size.to_string())
        .expect("tier and ratio are both validated against the table"))
}

/// `resolveOutputFormat`: jpeg/png only (jpg→jpeg normalized); the refusal
/// is the incumbent's en locale string with the RAW format.
fn resolve_seedream_output_format(format: &str) -> Result<String, ProtocolError> {
    let normalized = format.trim().to_lowercase();
    let value = if normalized == "jpg" {
        "jpeg"
    } else {
        normalized.as_str()
    };
    if value != "jpeg" && value != "png" {
        return Err(parse::invalid(format!(
            "Volcengine Seedream only supports png/jpeg output format, not \"{format}\""
        )));
    }
    Ok(value.to_string())
}

fn build_volcengine_image(
    route: &ResolvedModelRoute,
    auth: &ApplicableAuth,
    request: &ImageRequest,
    provider_defaults: &serde_json::Value,
) -> Result<OperationRequestPlan, ProtocolError> {
    let model = route.model.as_str();
    let capabilities = seedream_capabilities(model);
    let size = resolve_seedream_size(
        request
            .size
            .as_deref()
            .filter(|v| !v.is_empty())
            .or_else(|| request.resolution.as_deref().filter(|v| !v.is_empty())),
        request.ratio.as_deref(),
        provider_defaults,
        model,
        &capabilities,
    )?;
    let mut body = serde_json::json!({
        "model": model,
        "prompt": request.prompt,
        "response_format": "b64_json",
        "size": size,
    });
    if capabilities.supports_output_format {
        let format = request
            .format
            .as_deref()
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .or_else(|| tiers::truthy_json_string(provider_defaults.get("format")))
            .unwrap_or_else(|| "jpeg".to_string());
        body["output_format"] = serde_json::Value::from(resolve_seedream_output_format(&format)?);
    }
    if !request.references.is_empty() {
        if !capabilities.supports_reference_images {
            return Err(parse::invalid(format!(
                "Volcengine model \"{model}\" does not support reference images"
            )));
        }
        body["image"] = serde_json::Value::Array(
            request
                .references
                .iter()
                .map(|r| serde_json::Value::from(r.wire_string()))
                .collect(),
        );
    }
    body["watermark"] = request
        .watermark
        .map(serde_json::Value::from)
        .or_else(|| {
            provider_defaults
                .get("watermark")
                .filter(|v| !v.is_null())
                .cloned()
        })
        .unwrap_or(serde_json::Value::Bool(false));
    if capabilities.supports_guidance_scale {
        let guidance = request
            .guidance_scale
            .and_then(|g| serde_json::Number::from_f64(g).map(serde_json::Value::Number))
            .or_else(|| {
                provider_defaults
                    .get("guidance_scale")
                    .filter(|v| !v.is_null())
                    .cloned()
            })
            .or_else(|| {
                provider_defaults
                    .get("guidanceScale")
                    .filter(|v| !v.is_null())
                    .cloned()
            });
        if let Some(guidance) = guidance {
            body["guidance_scale"] = guidance;
        }
    }
    if capabilities.supports_seed {
        let seed = request.seed.map(serde_json::Value::from).or_else(|| {
            provider_defaults
                .get("seed")
                .filter(|v| !v.is_null())
                .cloned()
        });
        if let Some(seed) = seed {
            body["seed"] = seed;
        }
    }
    let url = format!(
        "{}/images/generations",
        route.endpoint.trim_end_matches('/')
    );
    Ok(
        OperationRequestPlan::post_json(url, body)
            .with_auth(auth, BearerStyle::AuthorizationBearer),
    )
}

fn parse_volcengine_image(
    route: &ResolvedModelRoute,
    request: &ImageRequest,
    provider_defaults: &serde_json::Value,
    body: &serde_json::Value,
) -> Result<ImageSubmitOutcome, ProtocolError> {
    let capabilities = seedream_capabilities(&route.model);
    let mime = if capabilities.supports_output_format {
        let format = request
            .format
            .as_deref()
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .or_else(|| tiers::truthy_json_string(provider_defaults.get("format")))
            .unwrap_or_else(|| "jpeg".to_string());
        // The build already validated the format; an unreachable invalid
        // value here falls back like the incumbent's `|| mimeType`.
        let normalized = if format.trim().eq_ignore_ascii_case("jpg") {
            "jpeg".to_string()
        } else {
            format.trim().to_lowercase()
        };
        FORMAT_TO_MIME_VOLCENGINE
            .iter()
            .find(|(f, _)| *f == normalized)
            .map(|(_, mime)| *mime)
            .unwrap_or("image/jpeg")
    } else {
        "image/jpeg"
    };
    // `data.data || []`: a missing data array is the EMPTY answer, refused
    // with the incumbent's verbatim text.
    let data = body.get("data").and_then(|v| v.as_array());
    let Some(data) = data.filter(|data| !data.is_empty()) else {
        return Err(parse::invalid("API returned no images".to_string()));
    };
    let mut products = Vec::with_capacity(data.len());
    for (index, item) in data.iter().enumerate() {
        let b64 = item["b64_json"].as_str().ok_or_else(|| {
            parse::invalid(format!(
                "volcengine image data[{index}] carries no b64_json"
            ))
        })?;
        products.push(MediaProductRef::Bytes {
            bytes: decode_base64_strict(b64, "volcengine image b64_json")?,
            mime: mime.to_string(),
        });
    }
    Ok(ImageSubmitOutcome::Done { products })
}

// ── minimax-images (core/media-adapters/minimax.ts) ──────────────────────

const MINIMAX_IMAGE_RATIOS: [&str; 8] = ["1:1", "16:9", "9:16", "4:3", "3:4", "3:2", "2:3", "21:9"];

/// `resolveMiniMaxBaseUrl`: a base ending `/anthropic` swaps to `/v1`, a
/// base ending `/v1` is kept, anything else gains `/v1` (case-sensitive
/// suffix rules, like the incumbent's `endsWith`).
fn resolve_minimax_base_url(endpoint: &str) -> String {
    let base = endpoint.trim_end_matches('/');
    if let Some(stripped) = base.strip_suffix("/anthropic") {
        return format!("{stripped}/v1");
    }
    if base.ends_with("/v1") {
        return base.to_string();
    }
    format!("{base}/v1")
}

fn build_minimax_image(
    route: &ResolvedModelRoute,
    auth: &ApplicableAuth,
    request: &ImageRequest,
) -> Result<OperationRequestPlan, ProtocolError> {
    // `params.size || params.resolution` — a JS-truthy check: an EMPTY
    // string is skipped, not refused.
    if request.size.as_deref().is_some_and(|v| !v.is_empty())
        || request.resolution.as_deref().is_some_and(|v| !v.is_empty())
    {
        return Err(parse::invalid(
            "MiniMax image size/resolution is unsupported".to_string(),
        ));
    }
    let aspect_ratio = request
        .ratio
        .as_deref()
        .filter(|v| !v.is_empty())
        .unwrap_or("3:2");
    if !MINIMAX_IMAGE_RATIOS.contains(&aspect_ratio) {
        return Err(parse::invalid(format!(
            "MiniMax image ratio \"{aspect_ratio}\" is unsupported"
        )));
    }
    if request.width.is_some() != request.height.is_some() {
        return Err(parse::invalid(
            "MiniMax image width and height must be provided together".to_string(),
        ));
    }
    if let (Some(width), Some(height)) = (request.width, request.height) {
        if !(512..=2048).contains(&width)
            || !(512..=2048).contains(&height)
            || width % 8 != 0
            || height % 8 != 0
        {
            return Err(parse::invalid(
                "MiniMax image width and height must be integers between 512 and 2048, \
                 divisible by 8"
                    .to_string(),
            ));
        }
    }
    let mut body = serde_json::json!({
        "model": route.model,
        "prompt": request.prompt,
        "response_format": "base64",
        "aspect_ratio": aspect_ratio,
    });
    // `params.n ? { n } : {}` — a zero n is omitted, not sent.
    if let Some(n) = request.n.filter(|n| *n > 0) {
        body["n"] = serde_json::Value::from(n);
    }
    if let Some(prompt_optimizer) = request.prompt_optimizer {
        body["prompt_optimizer"] = serde_json::Value::from(prompt_optimizer);
    }
    if let Some(seed) = request.seed {
        body["seed"] = serde_json::Value::from(seed);
    }
    if let Some(width) = request.width {
        body["width"] = serde_json::Value::from(width);
    }
    if let Some(height) = request.height {
        body["height"] = serde_json::Value::from(height);
    }
    if !request.references.is_empty() {
        body["subject_reference"] = serde_json::Value::Array(
            request
                .references
                .iter()
                .map(|r| serde_json::json!({ "type": "character", "image_file": r.wire_string() }))
                .collect(),
        );
    }
    let url = format!(
        "{}/image_generation",
        resolve_minimax_base_url(&route.endpoint)
    );
    Ok(
        OperationRequestPlan::post_json(url, body)
            .with_auth(auth, BearerStyle::AuthorizationBearer),
    )
}

/// The incumbent JS-truthiness pick (`a || b || c`): empty string, 0,
/// false, null/absent are skipped; an empty ARRAY is truthy in JS and is
/// therefore picked (a behavioral corner ported deliberately).
fn first_truthy<'a>(values: &[Option<&'a serde_json::Value>]) -> Option<&'a serde_json::Value> {
    values.iter().flatten().copied().find(|v| json_truthy(v))
}

fn json_truthy(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        serde_json::Value::String(s) => !s.is_empty(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => true,
    }
}

/// `collectBase64Images`: `data.data.image_base64 || data.data.images ||
/// data.image_base64`; array items are strings verbatim or objects' truthy
/// `base64`/`b64_json`; a bare string source is a single image.
fn collect_minimax_base64_images(data: &serde_json::Value) -> Vec<String> {
    let raw = first_truthy(&[
        data.pointer("/data/image_base64"),
        data.pointer("/data/images"),
        data.get("image_base64"),
    ]);
    match raw {
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(|item| {
                if let Some(s) = item.as_str() {
                    Some(s.to_string())
                } else {
                    first_truthy(&[item.get("base64"), item.get("b64_json")])
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                }
            })
            .collect(),
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

/// `collectImageUrls`: `data.data.image_urls || data.data.images ||
/// data.image_urls`; objects contribute truthy `url`/`image_url`.
fn collect_minimax_image_urls(data: &serde_json::Value) -> Vec<String> {
    let raw = first_truthy(&[
        data.pointer("/data/image_urls"),
        data.pointer("/data/images"),
        data.get("image_urls"),
    ]);
    match raw {
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(|item| {
                if let Some(s) = item.as_str() {
                    Some(s.to_string())
                } else {
                    first_truthy(&[item.get("url"), item.get("image_url")])
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                }
            })
            .collect(),
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn parse_minimax_image(body: &serde_json::Value) -> Result<ImageSubmitOutcome, ProtocolError> {
    // The 200-embedded failure envelope: `statusCode !== undefined &&
    // Number(statusCode) !== 0` — the JS Number() coercion means a STRING
    // "1000" fails, null/"" coerce to 0 and pass, and garbage (NaN) fails
    // (NaN !== 0 is true). Ported exactly via parse::js_number.
    if let Some(status_code) = body.pointer("/base_resp/status_code") {
        if parse::js_number(status_code) != 0.0 {
            let rendered = status_code
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| status_code.to_string());
            let message = body
                .pointer("/base_resp/status_msg")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("unknown error");
            return Err(parse::invalid(format!(
                "MiniMax API error {rendered}: {message}"
            )));
        }
    }
    let base64_images = collect_minimax_base64_images(body);
    if !base64_images.is_empty() {
        let mut products = Vec::with_capacity(base64_images.len());
        for b64 in &base64_images {
            products.push(MediaProductRef::Bytes {
                bytes: decode_base64_strict(b64, "minimax image_base64")?,
                mime: "image/jpeg".to_string(),
            });
        }
        return Ok(ImageSubmitOutcome::Done { products });
    }
    let urls = collect_minimax_image_urls(body);
    if !urls.is_empty() {
        return Ok(ImageSubmitOutcome::Done {
            products: urls
                .into_iter()
                .map(|url| MediaProductRef::Url {
                    url,
                    mime_hint: None,
                })
                .collect(),
        });
    }
    Err(parse::invalid("MiniMax API returned no images".to_string()))
}

// ── dashscope-images (core/media-adapters/dashscope.ts) ──────────────────

const WAN_IMAGE_RATIOS: [&str; 8] = ["1:1", "16:9", "9:16", "4:3", "3:4", "3:2", "2:3", "21:9"];
const WAN_DEFAULT_RATIO: &str = "3:2";
const QWEN_IMAGE_RATIOS: [&str; 5] = ["16:9", "4:3", "1:1", "3:4", "9:16"];
const QWEN_DEFAULT_RATIO: &str = "4:3";
const QWEN_20_SIZE_BY_RATIO: [(&str, &str); 5] = [
    ("16:9", "2688*1536"),
    ("9:16", "1536*2688"),
    ("1:1", "2048*2048"),
    ("4:3", "2368*1728"),
    ("3:4", "1728*2368"),
];
const QWEN_TEXT_SIZE_BY_RATIO: [(&str, &str); 5] = [
    ("16:9", "1664*928"),
    ("4:3", "1472*1104"),
    ("1:1", "1328*1328"),
    ("3:4", "1104*1472"),
    ("9:16", "928*1664"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DashscopeImageFamily {
    Wan,
    QwenMultimodal,
    QwenText2Image,
}

impl DashscopeImageFamily {
    /// `modelFamily` (verbatim prefix rules).
    fn of_model(model: &str) -> Self {
        if model.starts_with("qwen-image-2") {
            Self::QwenMultimodal
        } else if model.starts_with("qwen-image") {
            Self::QwenText2Image
        } else {
            Self::Wan
        }
    }
}

/// `resolveDashScopeBaseUrl`: `/compatible-mode/v1` swaps to `/api/v1`; an
/// `/api/v1` base is kept; anything else gains `/api/v1`.
fn resolve_dashscope_base_url(endpoint: &str) -> String {
    let base = endpoint.trim_end_matches('/');
    if let Some(stripped) = base.strip_suffix("/compatible-mode/v1") {
        return format!("{stripped}/api/v1");
    }
    if base.ends_with("/api/v1") {
        return base.to_string();
    }
    format!("{base}/api/v1")
}

/// `normalizeDashScopeSize`: `^([124])\s*k$` (lowercased) → `{d}K`; else
/// the trimmed raw string.
fn normalize_dashscope_size(value: &str) -> String {
    let raw = value.trim();
    let lower = raw.to_lowercase();
    let bytes = lower.as_bytes();
    if bytes.len() >= 2 && matches!(bytes[0], b'1' | b'2' | b'4') {
        let mut index = 1;
        while index < bytes.len() && (bytes[index] as char).is_whitespace() {
            index += 1;
        }
        if index == bytes.len() - 1 && bytes[index] == b'k' {
            return format!("{}K", bytes[0] as char);
        }
    }
    raw.to_string()
}

/// `assertResolution`: `None` for an absent/empty value; an unsupported
/// tier refuses with the RAW value in the message.
fn assert_dashscope_resolution(
    value: Option<&str>,
    supported: &[&str],
    label: &str,
) -> Result<Option<String>, ProtocolError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    let resolution = normalize_dashscope_size(value);
    if resolution.is_empty() {
        return Ok(None);
    }
    if !supported.contains(&resolution.as_str()) {
        return Err(parse::invalid(format!(
            "{label} resolution \"{value}\" is unsupported; supported resolutions: {}",
            supported.join(", ")
        )));
    }
    Ok(Some(resolution))
}

/// `assertSupportedRatio`: the trimmed ratio is checked; the RAW value
/// rides the refusal text.
fn assert_dashscope_ratio(
    value: Option<&str>,
    supported: &[&str],
    label: &str,
) -> Result<Option<String>, ProtocolError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    let ratio = value.trim();
    if !supported.contains(&ratio) {
        return Err(parse::invalid(format!(
            "{label} ratio \"{value}\" is unsupported"
        )));
    }
    Ok(Some(ratio.to_string()))
}

/// `wanSupportedResolutions`: wan2.7-image-pro without references serves
/// 4K; everything else caps at 2K.
fn wan_supported_resolutions(model: &str, reference_count: usize) -> &'static [&'static str] {
    if model.to_lowercase() == "wan2.7-image-pro" && reference_count == 0 {
        &["1K", "2K", "4K"]
    } else {
        &["1K", "2K"]
    }
}

/// `resolveQwenSize`: an explicit size must be one of the ratio table's
/// values verbatim; otherwise the (single) supported resolution assert runs
/// and the ratio maps through the table.
fn resolve_qwen_size(
    request: &ImageRequest,
    size_by_ratio: &[(&str, &str); 5],
    supported_resolution: &str,
    label: &str,
) -> Result<String, ProtocolError> {
    if let Some(size) = request.size.as_deref().filter(|v| !v.is_empty()) {
        let raw = size.trim();
        let allowed: Vec<&str> = size_by_ratio.iter().map(|(_, s)| *s).collect();
        if !allowed.contains(&raw) {
            return Err(parse::invalid(format!(
                "{label} size \"{size}\" is unsupported; supported sizes: {}",
                allowed.join(", ")
            )));
        }
        return Ok(raw.to_string());
    }
    let resolution = request
        .resolution
        .as_deref()
        .filter(|v| !v.is_empty())
        .unwrap_or(supported_resolution);
    assert_dashscope_resolution(Some(resolution), &[supported_resolution], label)?;
    let ratio = assert_dashscope_ratio(
        Some(
            request
                .ratio
                .as_deref()
                .filter(|v| !v.is_empty())
                .unwrap_or(QWEN_DEFAULT_RATIO),
        ),
        &QWEN_IMAGE_RATIOS,
        label,
    )?
    .expect("a default ratio is always present");
    Ok(size_by_ratio
        .iter()
        .find(|(label, _)| *label == ratio)
        .map(|(_, size)| size.to_string())
        .expect("the ratio is validated against the table"))
}

fn build_dashscope_image(
    route: &ResolvedModelRoute,
    auth: &ApplicableAuth,
    request: &ImageRequest,
) -> Result<OperationRequestPlan, ProtocolError> {
    let model = route.model.as_str();
    let family = DashscopeImageFamily::of_model(model);
    if family == DashscopeImageFamily::QwenText2Image && !request.references.is_empty() {
        return Err(parse::invalid(format!(
            "DashScope model \"{model}\" does not support reference images"
        )));
    }
    // generationParameters: size/ratio per family, then the shared
    // passthrough fields (n, negative_prompt, prompt_extend, watermark,
    // seed; aspect_ratio only for wan).
    let size: Option<String>;
    let mut aspect_ratio: Option<String> = None;
    match family {
        DashscopeImageFamily::Wan => {
            let supported = wan_supported_resolutions(model, request.references.len());
            let resolved_size =
                if let Some(explicit) = request.size.as_deref().filter(|v| !v.is_empty()) {
                    assert_dashscope_resolution(Some(explicit), supported, "DashScope Wan")?
                } else {
                    let fallback = request
                        .resolution
                        .as_deref()
                        .filter(|v| !v.is_empty())
                        .unwrap_or_else(|| supported.last().copied().unwrap_or("2K"));
                    assert_dashscope_resolution(Some(fallback), supported, "DashScope Wan")?
                };
            size = resolved_size;
            aspect_ratio = assert_dashscope_ratio(
                Some(
                    request
                        .ratio
                        .as_deref()
                        .filter(|v| !v.is_empty())
                        .unwrap_or(WAN_DEFAULT_RATIO),
                ),
                &WAN_IMAGE_RATIOS,
                "DashScope Wan",
            )?;
        }
        DashscopeImageFamily::QwenMultimodal => {
            size = Some(resolve_qwen_size(
                request,
                &QWEN_20_SIZE_BY_RATIO,
                "2K",
                "DashScope Qwen 2",
            )?);
        }
        DashscopeImageFamily::QwenText2Image => {
            size = Some(resolve_qwen_size(
                request,
                &QWEN_TEXT_SIZE_BY_RATIO,
                "1K",
                "DashScope Qwen Image",
            )?);
        }
    }
    let mut parameters = serde_json::json!({ "n": request.n_or_one() });
    if let Some(size) = size.filter(|s| !s.is_empty()) {
        parameters["size"] = serde_json::Value::from(size);
    }
    if let Some(negative_prompt) = request.negative_prompt.as_deref().filter(|v| !v.is_empty()) {
        parameters["negative_prompt"] = serde_json::Value::from(negative_prompt);
    }
    if let Some(prompt_extend) = request.prompt_extend {
        parameters["prompt_extend"] = serde_json::Value::from(prompt_extend);
    }
    if let Some(watermark) = request.watermark {
        parameters["watermark"] = serde_json::Value::from(watermark);
    }
    if let Some(seed) = request.seed {
        parameters["seed"] = serde_json::Value::from(seed);
    }
    if family == DashscopeImageFamily::Wan {
        if let Some(aspect_ratio) = aspect_ratio {
            parameters["aspect_ratio"] = serde_json::Value::from(aspect_ratio);
        }
    }

    let input = if family == DashscopeImageFamily::QwenText2Image {
        serde_json::json!({ "prompt": request.prompt })
    } else {
        let mut content = vec![serde_json::json!({ "text": request.prompt })];
        for reference in &request.references {
            content.push(serde_json::json!({ "image": reference.wire_string() }));
        }
        serde_json::json!({ "messages": [{ "role": "user", "content": content }] })
    };
    let body = serde_json::json!({
        "model": model,
        "input": input,
        "parameters": parameters,
    });
    let endpoint = match family {
        DashscopeImageFamily::QwenText2Image => "/services/aigc/text2image/image-synthesis",
        DashscopeImageFamily::QwenMultimodal => "/services/aigc/multimodal-generation/generation",
        DashscopeImageFamily::Wan => "/services/aigc/image-generation/generation",
    };
    let url = format!("{}{endpoint}", resolve_dashscope_base_url(&route.endpoint));
    let plan = OperationRequestPlan::post_json(url, body)
        .with_auth(auth, BearerStyle::AuthorizationBearer);
    // The async families carry the async header; qwen-multimodal is
    // synchronous and does not.
    Ok(if family == DashscopeImageFamily::QwenMultimodal {
        plan
    } else {
        plan.with_header("X-DashScope-Async", "enable")
    })
}

/// One collected dashscope image value (the incumbent
/// `parseDashScopeImageValue`): a trimmed http(s) URL, a base64 payload
/// extracted from a `data:image/...;base64,` URL, or any other non-empty
/// string treated as a raw base64 payload.
enum DashscopeImageValue {
    Url(String),
    Base64(String),
}

fn parse_dashscope_image_value(value: Option<&serde_json::Value>) -> Option<DashscopeImageValue> {
    let trimmed = value?.as_str()?.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Some(DashscopeImageValue::Url(trimmed.to_string()));
    }
    // `^data:image/[a-z0-9.+-]+;base64,(.+)$` (case-insensitive).
    if let Some(rest) = lower
        .strip_prefix("data:image/")
        .and_then(|_| trimmed.get("data:image/".len()..))
    {
        let mut subtype_len = 0usize;
        for b in rest.bytes() {
            if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'-') {
                subtype_len += 1;
            } else {
                break;
            }
        }
        if subtype_len > 0 {
            let after_subtype = &rest[subtype_len..];
            if let Some(payload) = after_subtype
                .get(..";base64,".len())
                .filter(|marker| marker.eq_ignore_ascii_case(";base64,"))
                .and_then(|_| after_subtype.get(";base64,".len()..))
            {
                if !payload.is_empty() {
                    return Some(DashscopeImageValue::Base64(payload.to_string()));
                }
            }
        }
    }
    Some(DashscopeImageValue::Base64(trimmed.to_string()))
}

/// `forDashScopeImageValues`: visits output.results[], then output.choices
/// [].message.content[], then output.images[] — in that order, with each
/// entry's keys in the incumbent's order.
fn for_dashscope_image_values(
    data: &serde_json::Value,
    visit: &mut impl FnMut(Option<&serde_json::Value>),
) {
    if let Some(results) = data.pointer("/output/results").and_then(|v| v.as_array()) {
        for item in results {
            for key in ["url", "image", "b64_json", "base64", "image_base64"] {
                visit(item.get(key));
            }
        }
    }
    if let Some(choices) = data.pointer("/output/choices").and_then(|v| v.as_array()) {
        for choice in choices {
            match choice.pointer("/message/content") {
                Some(serde_json::Value::Array(parts)) => {
                    for part in parts {
                        for key in ["image", "image_url", "b64_json", "base64", "image_base64"] {
                            visit(part.get(key));
                        }
                    }
                }
                // `content || []` with a STRING content iterates its
                // characters in JS (for..of over a string) — a malformed
                // chat-shaped answer; ported deliberately.
                Some(serde_json::Value::String(text)) => {
                    let chars: Vec<String> = text.chars().map(|c| c.to_string()).collect();
                    for ch in &chars {
                        visit(Some(&serde_json::Value::String(ch.clone())));
                    }
                }
                _ => {}
            }
        }
    }
    if let Some(images) = data.pointer("/output/images").and_then(|v| v.as_array()) {
        for item in images {
            if item.is_string() {
                visit(Some(item));
            } else {
                for key in ["url", "b64_json", "base64", "image_base64"] {
                    visit(item.get(key));
                }
            }
        }
    }
}

/// `collectDashScopeUrls` (deduped, order-preserving).
fn collect_dashscope_urls(data: &serde_json::Value) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut urls = Vec::new();
    for_dashscope_image_values(data, &mut |value| {
        if let Some(DashscopeImageValue::Url(url)) = parse_dashscope_image_value(value) {
            if seen.insert(url.clone()) {
                urls.push(url);
            }
        }
    });
    urls
}

/// `collectDashScopeBase64Images` (NOT deduped — incumbent behavior).
fn collect_dashscope_base64_images(data: &serde_json::Value) -> Vec<String> {
    let mut images = Vec::new();
    for_dashscope_image_values(data, &mut |value| {
        if let Some(DashscopeImageValue::Base64(payload)) = parse_dashscope_image_value(value) {
            images.push(payload);
        }
    });
    images
}

fn dashscope_task_id(body: &serde_json::Value) -> Option<String> {
    first_truthy(&[
        body.pointer("/output/task_id"),
        body.pointer("/output/taskId"),
        body.get("request_id"),
    ])
    .and_then(|v| v.as_str())
    .map(str::to_string)
}

/// The shared submit-response envelope check (`data.code && !== "Success"`).
fn check_dashscope_envelope(body: &serde_json::Value) -> Result<(), ProtocolError> {
    if let Some(code) = body.get("code").and_then(|v| v.as_str()) {
        if !code.is_empty() && code != "Success" {
            let message = body
                .get("message")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("unknown error");
            return Err(parse::invalid(format!(
                "DashScope API error {code}: {message}"
            )));
        }
    }
    Ok(())
}

fn parse_dashscope_image_submit(
    body: &serde_json::Value,
) -> Result<ImageSubmitOutcome, ProtocolError> {
    check_dashscope_envelope(body)?;
    let task_id = dashscope_task_id(body);
    let urls = collect_dashscope_urls(body);
    if !urls.is_empty() {
        return Ok(ImageSubmitOutcome::Done {
            products: urls
                .into_iter()
                .map(|url| MediaProductRef::Url {
                    url,
                    mime_hint: None,
                })
                .collect(),
        });
    }
    let base64_images = collect_dashscope_base64_images(body);
    if !base64_images.is_empty() {
        let mut products = Vec::with_capacity(base64_images.len());
        for payload in &base64_images {
            products.push(MediaProductRef::Bytes {
                bytes: decode_base64_strict(payload, "dashscope image payload")?,
                mime: "image/png".to_string(),
            });
        }
        return Ok(ImageSubmitOutcome::Done { products });
    }
    // The async families (wan / qwen-text2image) return only a task id;
    // the caller polls through the query plan. A response with NEITHER is
    // a loud failure (the incumbent fabricates a local tracking id here;
    // the port refuses — polling a fabricated id is unverifiable).
    match task_id {
        Some(task_id) => Ok(ImageSubmitOutcome::Pending { task_id }),
        None => Err(parse::invalid(
            "dashscope image submit returned neither images nor a task id; refusing instead of \
             tracking a fabricated local id"
                .to_string(),
        )),
    }
}

/// The dashscope image query plan (`GET {base}/tasks/{taskId}`, Bearer).
pub fn build_dashscope_image_query(
    route: &ResolvedModelRoute,
    auth: &ApplicableAuth,
    task_id: &str,
) -> OperationRequestPlan {
    let url = format!(
        "{}/tasks/{}",
        resolve_dashscope_base_url(&route.endpoint),
        encode_uri_component(task_id)
    );
    OperationRequestPlan::get(url).with_auth(auth, BearerStyle::AuthorizationBearer)
}

/// Parses the dashscope image query answer (the incumbent `query`):
/// non-terminal statuses are `pending`, FAILED/CANCELED fail with the
/// provider message, images settle the task (`done`), a terminal status
/// without images stays honestly `pending`.
pub fn parse_dashscope_image_query(
    body: &serde_json::Value,
) -> Result<TaskPollOutcome, ProtocolError> {
    let status = first_truthy(&[
        body.pointer("/output/task_status"),
        body.pointer("/output/taskStatus"),
    ])
    .and_then(|v| v.as_str())
    .map(str::to_string);
    if let Some(status) = status.as_deref() {
        if !["SUCCEEDED", "FAILED", "CANCELED"].contains(&status) {
            return Ok(TaskPollOutcome::Pending);
        }
        if status == "FAILED" || status == "CANCELED" {
            let message = body
                .get("message")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(status);
            return Ok(TaskPollOutcome::Failed {
                reason: message.to_string(),
            });
        }
    }
    let urls = collect_dashscope_urls(body);
    if !urls.is_empty() {
        return Ok(TaskPollOutcome::Done {
            products: urls
                .into_iter()
                .map(|url| MediaProductRef::Url {
                    url,
                    mime_hint: None,
                })
                .collect(),
        });
    }
    let base64_images = collect_dashscope_base64_images(body);
    if base64_images.is_empty() {
        return Ok(TaskPollOutcome::Pending);
    }
    let mut products = Vec::with_capacity(base64_images.len());
    for payload in &base64_images {
        products.push(MediaProductRef::Bytes {
            bytes: decode_base64_strict(payload, "dashscope image query payload")?,
            mime: "image/png".to_string(),
        });
    }
    Ok(TaskPollOutcome::Done { products })
}

// ── gemini-generate-content-image (core/media-adapters/gemini.ts) ────────

const GEMINI_25_RATIOS: [&str; 10] = [
    "1:1", "3:2", "2:3", "3:4", "4:3", "4:5", "5:4", "9:16", "16:9", "21:9",
];
const GEMINI_31_FLASH_RATIOS: [&str; 14] = [
    "1:1", "1:4", "1:8", "2:3", "3:2", "3:4", "4:1", "4:3", "4:5", "5:4", "8:1", "9:16", "16:9",
    "21:9",
];
const GEMINI_3_PRO_RATIOS: [&str; 10] = [
    "1:1", "2:3", "3:2", "3:4", "4:3", "4:5", "5:4", "9:16", "16:9", "21:9",
];
const GEMINI_3_SIZES: [&str; 3] = ["1K", "2K", "4K"];
const GEMINI_31_FLASH_SIZES: [&str; 4] = ["512", "1K", "2K", "4K"];

struct GeminiImageCapabilities {
    ratios: &'static [&'static str],
    image_sizes: &'static [&'static str],
    default_image_size: Option<&'static str>,
    max_reference_images: usize,
    supports_image_size: bool,
}

/// `geminiImageCapabilities` (the incumbent substring rules, in order).
fn gemini_image_capabilities(model: &str) -> GeminiImageCapabilities {
    let id = model.to_lowercase();
    if id.contains("3.1-flash") {
        return GeminiImageCapabilities {
            ratios: &GEMINI_31_FLASH_RATIOS,
            image_sizes: &GEMINI_31_FLASH_SIZES,
            default_image_size: Some("1K"),
            max_reference_images: 14,
            supports_image_size: true,
        };
    }
    if id.contains("3-pro") {
        return GeminiImageCapabilities {
            ratios: &GEMINI_3_PRO_RATIOS,
            image_sizes: &GEMINI_3_SIZES,
            default_image_size: Some("1K"),
            max_reference_images: 14,
            supports_image_size: true,
        };
    }
    GeminiImageCapabilities {
        ratios: &GEMINI_25_RATIOS,
        image_sizes: &[],
        default_image_size: None,
        max_reference_images: 3,
        supports_image_size: false,
    }
}

/// `normalizeGeminiImageSize`: unsupported on the 2.5 family (loud),
/// `0.5K` canonicalizes to `512`, membership is exact after uppercasing.
fn normalize_gemini_image_size(
    value: Option<&str>,
    field_name: &str,
    capabilities: &GeminiImageCapabilities,
    model: &str,
) -> Result<Option<String>, ProtocolError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    if !capabilities.supports_image_size {
        return Err(parse::invalid(format!(
            "Gemini 2.5 image model \"{model}\" does not support image size"
        )));
    }
    let raw = value.trim();
    let normalized = raw.to_uppercase();
    let canonical = if normalized == "0.5K" {
        "512"
    } else {
        normalized.as_str()
    };
    if capabilities.image_sizes.contains(&canonical) {
        return Ok(Some(canonical.to_string()));
    }
    Err(parse::invalid(format!(
        "Gemini image {field_name} \"{raw}\" is unsupported"
    )))
}

fn build_gemini_image(
    route: &ResolvedModelRoute,
    auth: &ApplicableAuth,
    request: &ImageRequest,
    provider_defaults: &serde_json::Value,
) -> Result<OperationRequestPlan, ProtocolError> {
    let _ = provider_defaults;
    let model = route.model.as_str();
    let capabilities = gemini_image_capabilities(model);
    if request.references.len() > capabilities.max_reference_images {
        return Err(parse::invalid(format!(
            "Gemini model \"{model}\" supports at most {} reference images",
            capabilities.max_reference_images
        )));
    }
    let mut parts = vec![serde_json::json!({ "text": request.prompt })];
    for reference in &request.references {
        // The incumbent `imagePart`: a malformed data URL yields a null
        // part that is SILENTLY skipped (ported); a remote URL is the
        // incumbent's download-and-inline — here the service layer must
        // pre-fetch it through the egress guard, so a URL reaching the
        // dialect is a wiring refusal, never an unguarded fetch.
        match reference {
            ImageReference::DataUrl(data_url) => {
                if let Some((mime, payload)) = parse_data_url(data_url) {
                    parts.push(serde_json::json!({
                        "inline_data": { "mime_type": mime, "data": payload }
                    }));
                }
            }
            ImageReference::Bytes { bytes, mime, .. } => {
                parts.push(serde_json::json!({
                    "inline_data": {
                        "mime_type": mime,
                        "data": base64::engine::general_purpose::STANDARD.encode(bytes),
                    }
                }));
            }
            ImageReference::RemoteUrl(_) => {
                return Err(parse::invalid(
                    "gemini image references must be bytes/data URLs by the time the dialect \
                     runs: the service layer downloads remote references through the egress \
                     guard (the incumbent downloads them inside the adapter); reaching here \
                     with a URL is a wiring bug"
                        .to_string(),
                ));
            }
            ImageReference::ProviderRef(uri) => {
                parts.push(serde_json::json!({ "file_data": { "file_uri": uri } }));
            }
        }
    }

    // normalizeGeminiImageConfig: the ratio defaults to "3:2" and is
    // ALWAYS emitted; size/resolution conflict refuses; the 2.5 family
    // refuses any image-size input.
    let ratio_value = request
        .ratio
        .as_deref()
        .filter(|v| !v.is_empty())
        .unwrap_or("3:2");
    let ratio_trimmed = ratio_value.trim();
    if !capabilities.ratios.contains(&ratio_trimmed) {
        return Err(parse::invalid(format!(
            "Gemini image ratio \"{ratio_trimmed}\" is unsupported"
        )));
    }
    let mut image_config = serde_json::json!({ "aspectRatio": ratio_trimmed });
    let size = normalize_gemini_image_size(request.size.as_deref(), "size", &capabilities, model)?;
    let default_resolution = if request.size.is_some() {
        None
    } else {
        capabilities.default_image_size
    };
    let resolution = normalize_gemini_image_size(
        request
            .resolution
            .as_deref()
            .filter(|v| !v.is_empty())
            .or(default_resolution),
        "resolution",
        &capabilities,
        model,
    )?;
    if let (Some(size), Some(resolution)) = (size.as_ref(), resolution.as_ref()) {
        if size != resolution {
            return Err(parse::invalid(format!(
                "Gemini image size \"{}\" conflicts with resolution \"{}\"",
                request.size.as_deref().unwrap_or_default(),
                request.resolution.as_deref().unwrap_or_default()
            )));
        }
    }
    if let Some(image_size) = size.or(resolution) {
        image_config["imageSize"] = serde_json::Value::from(image_size);
    }

    let body = serde_json::json!({
        "contents": [{ "parts": parts }],
        "generationConfig": {
            "responseModalities": ["TEXT", "IMAGE"],
            "imageConfig": image_config,
        },
    });
    let url = format!(
        "{}/models/{}:generateContent",
        route.endpoint.trim_end_matches('/'),
        encode_uri_component(model)
    );
    Ok(OperationRequestPlan::post_json(url, body)
        .with_auth(auth, BearerStyle::NamedHeader("x-goog-api-key")))
}

/// The incumbent data-URL split (`^data:([^;]+);base64,(.*)$` — the MIME
/// segment is one-or-more non-`;` characters).
fn parse_data_url(data_url: &str) -> Option<(&str, &str)> {
    let rest = data_url.strip_prefix("data:")?;
    let (mime, payload) = rest.split_once(";base64,")?;
    if mime.is_empty() || mime.contains(';') {
        return None;
    }
    Some((mime, payload))
}

fn parse_gemini_image(body: &serde_json::Value) -> Result<ImageSubmitOutcome, ProtocolError> {
    let mut products: Vec<MediaProductRef> = Vec::new();
    if let Some(candidates) = body.get("candidates").and_then(|v| v.as_array()) {
        for candidate in candidates {
            if let Some(parts) = candidate
                .pointer("/content/parts")
                .and_then(|v| v.as_array())
            {
                for part in parts {
                    if part.get("thought").and_then(|v| v.as_bool()) == Some(true) {
                        continue;
                    }
                    let inline = first_truthy(&[part.get("inlineData"), part.get("inline_data")]);
                    let Some(inline) = inline else {
                        continue;
                    };
                    let Some(data) = inline.get("data").and_then(|v| v.as_str()) else {
                        continue;
                    };
                    let mime = first_truthy(&[inline.get("mimeType"), inline.get("mime_type")])
                        .and_then(|v| v.as_str())
                        .unwrap_or("image/png");
                    products.push(MediaProductRef::Bytes {
                        bytes: decode_base64_strict(data, "gemini inline image")?,
                        mime: mime.to_string(),
                    });
                }
            }
        }
    }
    if products.is_empty() {
        return Err(parse::invalid(gemini_no_image_detail(body)));
    }
    Ok(ImageSubmitOutcome::Done { products })
}

/// `noImageResponseDetail`: blockReason (when set and not UNSPECIFIED) and
/// the deduped finishReasons join the refusal text.
fn gemini_no_image_detail(body: &serde_json::Value) -> String {
    let mut details = Vec::new();
    let block_reason = first_truthy(&[
        body.pointer("/promptFeedback/blockReason"),
        body.pointer("/prompt_feedback/block_reason"),
    ])
    .and_then(|v| v.as_str())
    .filter(|s| !s.is_empty() && *s != "BLOCK_REASON_UNSPECIFIED");
    if let Some(reason) = block_reason {
        details.push(format!("promptFeedback.blockReason={reason}"));
    }
    let mut seen = std::collections::HashSet::new();
    let mut finish_reasons = Vec::new();
    if let Some(candidates) = body.get("candidates").and_then(|v| v.as_array()) {
        for candidate in candidates {
            let reason = first_truthy(&[
                candidate.get("finishReason"),
                candidate.get("finish_reason"),
            ])
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());
            if let Some(reason) = reason {
                if seen.insert(reason.to_string()) {
                    finish_reasons.push(reason.to_string());
                }
            }
        }
    }
    if !finish_reasons.is_empty() {
        details.push(format!("finishReason={}", finish_reasons.join(",")));
    }
    if details.is_empty() {
        "Gemini API returned no images".to_string()
    } else {
        format!("Gemini API returned no images ({})", details.join("; "))
    }
}

// ── agnes-images (core/media-adapters/agnes.ts) ──────────────────────────

const AGNES_IMAGE_SIZES: [(&str, &str); 8] = [
    ("1:1", "1024x1024"),
    ("4:3", "1024x768"),
    ("3:4", "768x1024"),
    ("3:2", "1152x768"),
    ("2:3", "768x1152"),
    ("16:9", "1344x768"),
    ("9:16", "768x1344"),
    ("21:9", "1536x640"),
];

/// `agnesV1Base` (shared with the video dialect): trailing slashes
/// stripped; a `/v1` suffix (case-sensitive, like `endsWith`) kept;
/// otherwise `/v1` appended.
pub(crate) fn agnes_v1_base(endpoint: &str) -> String {
    let base = endpoint.trim_end_matches('/');
    if base.ends_with("/v1") {
        base.to_string()
    } else {
        format!("{base}/v1")
    }
}

/// `agnesRootBase` (the video query's base): trailing slashes stripped,
/// then a trailing `/v1` (case-INSENSITIVE, the incumbent's regex) removed.
pub(crate) fn agnes_root_base(endpoint: &str) -> String {
    let base = endpoint.trim_end_matches('/');
    if base.len() >= 3 && base[base.len() - 3..].eq_ignore_ascii_case("/v1") {
        base[..base.len() - 3].to_string()
    } else {
        base.to_string()
    }
}

/// `resolveImageSize` (agnes): an explicit `WxH` must be one of the eight
/// table values; an explicit tier must be `1K`; otherwise the ratio maps
/// through the table (default `3:2`).
fn resolve_agnes_image_size(
    request: &ImageRequest,
    provider_defaults: &serde_json::Value,
) -> Result<String, ProtocolError> {
    let explicit = request
        .size
        .as_deref()
        .filter(|v| !v.is_empty())
        .or_else(|| request.resolution.as_deref().filter(|v| !v.is_empty()))
        .map(str::to_string)
        .or_else(|| tiers::truthy_json_string(provider_defaults.get("size")))
        .or_else(|| tiers::truthy_json_string(provider_defaults.get("resolution")));
    let ratio = request
        .ratio
        .as_deref()
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .or_else(|| tiers::truthy_json_string(provider_defaults.get("aspect_ratio")))
        .or_else(|| tiers::truthy_json_string(provider_defaults.get("ratio")));
    if let Some(explicit) = explicit {
        let trimmed = explicit.trim();
        if is_plain_pixel_size(trimmed) {
            let size = trimmed.to_string();
            if AGNES_IMAGE_SIZES.iter().any(|(_, s)| *s == size) {
                return Ok(size);
            }
            return Err(parse::invalid(format!(
                "Agnes image size \"{size}\" is unsupported"
            )));
        }
        let resolution = trimmed.to_uppercase();
        if resolution != "1K" {
            return Err(parse::invalid(format!(
                "Agnes image resolution \"{explicit}\" is unsupported; supported resolutions: 1K"
            )));
        }
    }
    let effective_ratio = ratio.unwrap_or_else(|| "3:2".to_string());
    AGNES_IMAGE_SIZES
        .iter()
        .find(|(label, _)| *label == effective_ratio)
        .map(|(_, size)| size.to_string())
        .ok_or_else(|| {
            parse::invalid(format!(
                "Agnes image ratio \"{effective_ratio}\" is unsupported"
            ))
        })
}

/// `^\d+x\d+$` (case-insensitive x — the incumbent's agnes regex).
fn is_plain_pixel_size(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut seen_separator = false;
    let mut left = 0usize;
    let mut right = 0usize;
    for &b in bytes {
        if b.is_ascii_digit() {
            if seen_separator {
                right += 1;
            } else {
                left += 1;
            }
        } else if (b == b'x' || b == b'X') && !seen_separator {
            seen_separator = true;
        } else {
            return false;
        }
    }
    seen_separator && left > 0 && right > 0
}

fn build_agnes_image(
    route: &ResolvedModelRoute,
    auth: &ApplicableAuth,
    request: &ImageRequest,
    provider_defaults: &serde_json::Value,
) -> Result<OperationRequestPlan, ProtocolError> {
    let mut extra_body = serde_json::json!({ "response_format": "b64_json" });
    if !request.references.is_empty() {
        extra_body["image"] = serde_json::Value::Array(
            request
                .references
                .iter()
                .map(|r| serde_json::Value::from(r.wire_string()))
                .collect(),
        );
    }
    let mut body = serde_json::json!({
        "model": route.model,
        "prompt": request.prompt,
        "extra_body": extra_body,
    });
    body["size"] = serde_json::Value::from(resolve_agnes_image_size(request, provider_defaults)?);
    let url = format!("{}/images/generations", agnes_v1_base(&route.endpoint));
    Ok(
        OperationRequestPlan::post_json(url, body)
            .with_auth(auth, BearerStyle::AuthorizationBearer),
    )
}

fn parse_agnes_image(body: &serde_json::Value) -> Result<ImageSubmitOutcome, ProtocolError> {
    let mut base64: Vec<&str> = Vec::new();
    let mut urls: Vec<String> = Vec::new();
    if let Some(data) = body.get("data").and_then(|v| v.as_array()) {
        for item in data {
            if let Some(b64) = item.get("b64_json").and_then(|v| v.as_str()) {
                let trimmed = b64.trim();
                if !trimmed.is_empty() {
                    base64.push(trimmed);
                }
            }
            if let Some(url) = item.get("url").and_then(|v| v.as_str()) {
                let trimmed = url.trim();
                if !trimmed.is_empty() {
                    urls.push(trimmed.to_string());
                }
            }
        }
    }
    if !base64.is_empty() {
        let mut products = Vec::with_capacity(base64.len());
        for b64 in base64 {
            products.push(MediaProductRef::Bytes {
                bytes: decode_base64_strict(b64, "agnes image b64_json")?,
                mime: "image/png".to_string(),
            });
        }
        return Ok(ImageSubmitOutcome::Done { products });
    }
    if !urls.is_empty() {
        return Ok(ImageSubmitOutcome::Done {
            products: urls
                .into_iter()
                .map(|url| MediaProductRef::Url {
                    url,
                    mime_hint: None,
                })
                .collect(),
        });
    }
    Err(parse::invalid(
        "Agnes image API returned no images".to_string(),
    ))
}

// ── shared helpers ───────────────────────────────────────────────────────

/// The strict base64 decode every dialect's parse path uses (the
/// incumbent's `Buffer.from(b64, "base64")` tolerates garbage; the port
/// refuses it — a corrupt payload is never materialized).
pub fn decode_base64_strict(b64: &str, what: &str) -> Result<Vec<u8>, ProtocolError> {
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|err| parse::invalid(format!("{what} is not valid base64: {err}")))
}
