//! Transcription (ASR) dialects (R05-T06): openai-audio-transcriptions /
//! mimo-chat-completions-asr / dashscope-qwen-asr-chat /
//! volcengine-bigasr. `system-speech-recognition` is a loud
//! ProtocolNotImplemented at the gateway/service layer (§29.13 — the
//! Swift/TCC helper belongs to the desktop host's TCC authorization, the
//! bare service process cannot hold it) and never reaches this layer.
//!
//! Fidelity notes (registered in §30):
//! - URLs are the incumbent's plain concat over the trailing-slash-trimmed
//!   base (NO `/v1` merge, NO case-insensitive target dedup — the
//!   speech-recognition adapters do not use the chat plane's join).
//! - `language` rides only when TRUTHY (an empty string falls to mimo's
//!   `"auto"`, is omitted by openai/dashscope).
//! - response texts are `String(text || "").trim()` / the chat-content
//!   `?? ""` ladder — a missing text is an EMPTY transcript, not an error.
//! - the bigasr `X-Api-Status-Code` header check fires only when the
//!   header is PRESENT and non-empty (`if (statusCode && statusCode !==
//!   "20000000")` — a missing header is NOT a failure).
//! - the incumbent echoes the request `language` in its result object;
//!   the port leaves that to the service layer (it owns the request).
//! - the service layer resolves the multipart filename (the audio path's
//!   basename, default `audio.wav`) and wire MIME (default `audio/wav`)
//!   when it reads the host file; the dialect consumes them as given.

use base64::Engine as _;
use lingxi_kernel::model_exchange::{ProtocolFamily, ResolvedModelRoute};
use lingxi_protocol::{ErrorCode, ProtocolError};

use super::{encode_multipart, parse, MultipartPart, OperationRequestPlan};
use crate::models::credentials::ApplicableAuth;
use crate::models::dispatch::BearerStyle;

/// One transcription request (host-read audio bytes, bounded upstream).
#[derive(Debug, Clone)]
pub struct TranscriptionRequest {
    pub audio: Vec<u8>,
    pub mime: String,
    pub filename: String,
    pub language: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TranscriptionOutcome {
    pub text: String,
    /// volcengine reports `audio_info.duration` (ms, `Number(...)`-coerced
    /// — a JSON null coerces to 0 and IS reported, ported exactly); other
    /// families omit.
    pub duration_ms: Option<f64>,
}

/// `String(value || "").trim()` for the openai/volcengine text fields:
/// falsy values (missing, null, empty, 0, false) yield "".
fn js_string_or_empty_trimmed(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(s)) => s.trim().to_string(),
        Some(serde_json::Value::Number(n)) if n.as_f64() != Some(0.0) => {
            n.to_string().trim().to_string()
        }
        Some(serde_json::Value::Bool(true)) => "true".to_string(),
        _ => String::new(),
    }
}

/// The incumbent `extractChatCompletionText`: `choices[0].message.content
/// ?? choices[0].delta.content ?? ""`, then `String(...).trim()` (the
/// ladder is NULLISH — a null content passes through to delta).
fn extract_chat_completion_text(body: &serde_json::Value) -> String {
    let choice = body.pointer("/choices/0");
    let content = choice
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .filter(|v| !v.is_null())
        .or_else(|| {
            choice
                .and_then(|c| c.get("delta"))
                .and_then(|d| d.get("content"))
                .filter(|v| !v.is_null())
        });
    match content {
        Some(serde_json::Value::String(s)) => s.trim().to_string(),
        Some(value @ (serde_json::Value::Number(_) | serde_json::Value::Bool(_))) => {
            value.to_string().trim().to_string()
        }
        _ => String::new(),
    }
}

pub fn build_transcription(
    route: &ResolvedModelRoute,
    auth: &ApplicableAuth,
    request: &TranscriptionRequest,
) -> Result<OperationRequestPlan, ProtocolError> {
    if request.audio.is_empty() {
        return Err(parse::invalid(
            "transcription audio is empty (nothing to recognize)".to_string(),
        ));
    }
    match route.protocol {
        ProtocolFamily::OpenAiAudioTranscriptions => {
            // multipart/form-data: model + language? + the file part.
            let mut parts = vec![MultipartPart::field("model", &route.model)];
            if let Some(language) = request.language.as_deref().filter(|l| !l.is_empty()) {
                parts.push(MultipartPart::field("language", language));
            }
            parts.push(MultipartPart::file(
                "file",
                &request.filename,
                &request.mime,
                request.audio.clone(),
            ));
            let (body, content_type) = encode_multipart(&parts)?;
            let plan = OperationRequestPlan {
                method: "POST",
                url: format!(
                    "{}/audio/transcriptions",
                    route.endpoint.trim_end_matches('/')
                ),
                headers: vec![("content-type".to_string(), content_type)],
                body,
            };
            Ok(plan.with_auth(auth, BearerStyle::AuthorizationBearer))
        }
        ProtocolFamily::MimoChatCompletionsAsr => {
            // `api-key: <key>` (NOT a bearer) + the chat-completions
            // envelope with input_audio + asr_options (NO stream field).
            let data_url = format!(
                "data:{};base64,{}",
                request.mime,
                base64::engine::general_purpose::STANDARD.encode(&request.audio)
            );
            let body = serde_json::json!({
                "model": route.model,
                "messages": [{
                    "role": "user",
                    "content": [{
                        "type": "input_audio",
                        "input_audio": {"data": data_url},
                    }],
                }],
                "asr_options": {
                    "language": request
                        .language
                        .as_deref()
                        .filter(|l| !l.is_empty())
                        .unwrap_or("auto"),
                },
            });
            let plan = OperationRequestPlan::post_json(
                format!("{}/chat/completions", route.endpoint.trim_end_matches('/')),
                body,
            );
            Ok(match auth {
                ApplicableAuth::None => plan,
                ApplicableAuth::Bearer(token) => plan.with_header("api-key", &token.clone()),
                ApplicableAuth::Header { name, value } => {
                    plan.with_header(&name.clone(), &value.clone())
                }
            })
        }
        ProtocolFamily::DashscopeQwenAsrChat => {
            // Bearer + the chat-completions envelope with stream:false and
            // asr_options{language?, enable_itn:false} (language only when
            // truthy).
            let data_url = format!(
                "data:{};base64,{}",
                request.mime,
                base64::engine::general_purpose::STANDARD.encode(&request.audio)
            );
            let mut asr_options = serde_json::Map::new();
            if let Some(language) = request.language.as_deref().filter(|l| !l.is_empty()) {
                asr_options.insert("language".to_string(), serde_json::Value::from(language));
            }
            asr_options.insert("enable_itn".to_string(), serde_json::Value::from(false));
            let body = serde_json::json!({
                "model": route.model,
                "messages": [{
                    "role": "user",
                    "content": [{
                        "type": "input_audio",
                        "input_audio": {"data": data_url},
                    }],
                }],
                "stream": false,
                "asr_options": serde_json::Value::Object(asr_options),
            });
            Ok(OperationRequestPlan::post_json(
                format!("{}/chat/completions", route.endpoint.trim_end_matches('/')),
                body,
            )
            .with_auth(auth, BearerStyle::AuthorizationBearer))
        }
        ProtocolFamily::VolcengineBigAsr => {
            // The flash-recognize endpoint: X-Api-Key (NOT bearer) +
            // resource id + a fresh request uuid + sequence -1; the
            // `user.uid` field carries the API key (the incumbent's
            // verbatim shape).
            let token = match auth {
                ApplicableAuth::Bearer(token) => token.clone(),
                ApplicableAuth::Header { value, .. } => value.clone(),
                ApplicableAuth::None => {
                    return Err(parse::invalid(format!(
                        "provider {} (volcengine-bigasr) resolved with auth kind none — the \
                         flash endpoint requires the X-Api-Key material",
                        route.provider
                    )))
                }
            };
            let mut entropy = [0u8; 16];
            getrandom::getrandom(&mut entropy).map_err(|err| {
                ProtocolError::new(
                    ErrorCode::Internal,
                    format!("bigasr request id entropy failed: {err}"),
                    false,
                )
            })?;
            // UUID v4 layout (version/variant bits set) from CSPRNG bytes.
            entropy[6] = (entropy[6] & 0x0f) | 0x40;
            entropy[8] = (entropy[8] & 0x3f) | 0x80;
            let hex: String = entropy.iter().map(|b| format!("{b:02x}")).collect();
            let request_id = format!(
                "{}-{}-{}-{}-{}",
                &hex[0..8],
                &hex[8..12],
                &hex[12..16],
                &hex[16..20],
                &hex[20..32]
            );
            let body = serde_json::json!({
                "user": {"uid": token},
                "audio": {"data": base64::engine::general_purpose::STANDARD.encode(&request.audio)},
                "request": {"model_name": "bigmodel"},
            });
            Ok(OperationRequestPlan::post_json(
                format!(
                    "{}/api/v3/auc/bigmodel/recognize/flash",
                    route.endpoint.trim_end_matches('/')
                ),
                body,
            )
            .with_header("x-api-key", &token)
            .with_header("x-api-resource-id", "volc.bigasr.auc_turbo")
            .with_header("x-api-request-id", &request_id)
            .with_header("x-api-sequence", "-1"))
        }
        other => Err(parse::invalid(format!(
            "protocol family {} does not serve speech recognition (wiring bug: the gateway \
             matrix refuses earlier)",
            other.config_name()
        ))),
    }
}

/// Parses an openai-transcriptions / mimo / dashscope response body.
pub fn parse_transcription(
    family: ProtocolFamily,
    body: &serde_json::Value,
) -> Result<TranscriptionOutcome, ProtocolError> {
    match family {
        ProtocolFamily::OpenAiAudioTranscriptions => Ok(TranscriptionOutcome {
            text: js_string_or_empty_trimmed(body.get("text")),
            duration_ms: None,
        }),
        ProtocolFamily::MimoChatCompletionsAsr | ProtocolFamily::DashscopeQwenAsrChat => {
            Ok(TranscriptionOutcome {
                text: extract_chat_completion_text(body),
                duration_ms: None,
            })
        }
        ProtocolFamily::VolcengineBigAsr => {
            let duration = body
                .pointer("/audio_info/duration")
                .map(parse::js_number)
                .filter(|n| n.is_finite());
            Ok(TranscriptionOutcome {
                text: js_string_or_empty_trimmed(body.pointer("/result/text")),
                duration_ms: duration,
            })
        }
        other => Err(parse::invalid(format!(
            "protocol family {} does not serve speech recognition",
            other.config_name()
        ))),
    }
}

/// The volcengine-bigasr STATUS lives in the `X-Api-Status-Code` response
/// header; the incumbent check is `if (statusCode && statusCode !==
/// "20000000")` — a MISSING or EMPTY header is NOT a failure.
pub fn bigasr_status_ok(headers: &reqwest::header::HeaderMap) -> Result<(), ProtocolError> {
    let status = headers
        .get("x-api-status-code")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !status.is_empty() && status != "20000000" {
        return Err(parse::invalid(format!(
            "Volcengine transcription failed: {status}"
        )));
    }
    Ok(())
}
