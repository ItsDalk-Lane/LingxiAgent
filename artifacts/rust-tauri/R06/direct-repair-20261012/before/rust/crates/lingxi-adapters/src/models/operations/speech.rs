//! Speech-synthesis dialects (R05-T06): openai-audio-speech / minimax-tts /
//! dashscope-qwen-tts. (`system-speech` is dispatched service-side — the
//! macOS `/usr/bin/say` subprocess, §29.13 — and never reaches this layer.)
//!
//! Fidelity notes (registered in §30):
//! - the voice/speed/format resolution is the incumbent
//!   `resolveSpeechParameters` WITHOUT the provider-defaults fold (the
//!   service layer folds configured defaults into the request fields
//!   first): voice trims and falls back when empty, speed CLAMPS into the
//!   family range (never refuses; non-finite → 1.0), format falls back to
//!   `mp3` when not in the family set (no trim, no refusal).
//! - product MIMEs are derived from the wire FORMAT per family (never the
//!   response's content-type for openai/minimax; the dashscope download's
//!   content-type wins service-side with an `audio/wav` fallback).
//! - the minimax 200-envelope check is `typeof status_code === "number"
//!   && status_code !== 0` — a STRING status code does NOT fail (ported
//!   exactly); an empty `data.audio` refuses verbatim.
//! - `Buffer.from(hex, "hex")` is lenient in the incumbent; the port
//!   decodes STRICTLY (a corrupt payload is never materialized).

use lingxi_kernel::model_exchange::{ProtocolFamily, ResolvedModelRoute};
use lingxi_protocol::ProtocolError;

use super::embedding::origin_of;
use super::{encode_uri_component, parse, require_group_id, MediaProductRef, OperationRequestPlan};
use crate::models::credentials::ApplicableAuth;
use crate::models::dispatch::BearerStyle;

/// The openai-audio-speech format set (`OPENAI_SPEECH_FORMATS`).
pub const OPENAI_SPEECH_FORMATS: [&str; 6] = ["mp3", "opus", "aac", "flac", "wav", "pcm"];
/// The minimax-tts format set (`MINIMAX_SPEECH_FORMATS`).
pub const MINIMAX_SPEECH_FORMATS: [&str; 4] = ["mp3", "wav", "pcm", "flac"];

/// One synthesis request. The service layer folds the provider-defaults
/// overlay into these fields first; the dialect then applies the
/// incumbent's per-protocol normalization (trim/clamp/set-fallback).
#[derive(Debug, Clone)]
pub struct SpeechRequest {
    pub text: String,
    pub voice: Option<String>,
    pub speed: Option<f64>,
    pub format: Option<String>,
}

/// The incumbent `normalizedText`: a string whose trim is non-empty keeps
/// its TRIMMED form; anything else falls back.
fn normalized_text(value: Option<&str>, fallback: &str) -> String {
    match value.map(str::trim) {
        Some(trimmed) if !trimmed.is_empty() => trimmed.to_string(),
        _ => fallback.to_string(),
    }
}

/// The incumbent `normalizedSpeed`: a non-finite or absent value falls
/// back to 1.0; anything else CLAMPS into [min, max] (never refuses).
fn normalized_speed(value: Option<f64>, min: f64, max: f64) -> f64 {
    match value {
        Some(speed) if speed.is_finite() => speed.clamp(min, max),
        _ => 1.0,
    }
}

/// The incumbent format rule: exact (untrimmed, case-sensitive) membership
/// in the family set, else `mp3` — never a refusal.
fn normalized_format(value: Option<&str>, set: &[&str]) -> String {
    match value {
        Some(format) if set.contains(&format) => format.to_string(),
        _ => "mp3".to_string(),
    }
}

/// The EFFECTIVE openai-audio-speech wire format of a request (the service
/// layer's product-MIME derivation input — MIME follows the FORMAT, never
/// the response's content-type).
pub fn effective_openai_speech_format(request_format: Option<&str>) -> String {
    normalized_format(request_format, &OPENAI_SPEECH_FORMATS)
}

/// The EFFECTIVE minimax-tts wire format of a request (same derivation
/// rule, the minimax ladder).
pub fn effective_minimax_speech_format(request_format: Option<&str>) -> String {
    normalized_format(request_format, &MINIMAX_SPEECH_FORMATS)
}

/// The openai-audio-speech product MIME — derived from the wire FORMAT,
/// never from the response's content-type (the incumbent's ladder; pcm
/// and anything unexpected land on `audio/mpeg`).
pub fn openai_speech_mime_for_format(format: &str) -> &'static str {
    match format {
        "opus" => "audio/ogg",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        _ => "audio/mpeg",
    }
}

/// The minimax-tts product MIME (the incumbent's ladder: pcm is the raw
/// octet stream, mp3 and anything unexpected land on `audio/mpeg`).
pub fn minimax_speech_mime_for_format(format: &str) -> &'static str {
    match format {
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "pcm" => "application/octet-stream",
        _ => "audio/mpeg",
    }
}

pub fn build_speech(
    route: &ResolvedModelRoute,
    auth: &ApplicableAuth,
    request: &SpeechRequest,
) -> Result<OperationRequestPlan, ProtocolError> {
    // The media manager's upstream refusal ("prompt is required" — the
    // incumbent `textOrNull`: a string whose TRIM is empty is no text).
    if request.text.trim().is_empty() {
        return Err(parse::invalid("prompt is required".to_string()));
    }
    match route.protocol {
        ProtocolFamily::OpenAiAudioSpeech => {
            let voice = normalized_text(request.voice.as_deref(), "alloy");
            let speed = normalized_speed(request.speed, 0.25, 4.0);
            let format = normalized_format(request.format.as_deref(), &OPENAI_SPEECH_FORMATS);
            let body = serde_json::json!({
                "model": route.model,
                "input": request.text,
                "voice": voice,
                "response_format": format,
                // js_number_to_json: JSON.stringify renders an
                // integer-valued speed as `1`, not `1.0`.
                "speed": parse::js_number_to_json(speed),
            });
            // `${base}/audio/speech` — a plain concat of the
            // trailing-slash-trimmed base (the incumbent does NOT dedup a
            // base already ending in the target path).
            Ok(OperationRequestPlan::post_json(
                format!("{}/audio/speech", route.endpoint.trim_end_matches('/')),
                body,
            )
            .with_auth(auth, BearerStyle::AuthorizationBearer))
        }
        ProtocolFamily::MinimaxTts => {
            // Origin-only + mandatory GroupId (encodeURIComponent'd);
            // absent/empty refuses with the incumbent's verbatim text.
            let origin = origin_of(&route.endpoint)?;
            let group_id = require_group_id(
                route,
                "MiniMax speech requires a GroupId configured on the model entry (settings > \
                 providers > model > GroupId)",
            )?;
            let voice = normalized_text(request.voice.as_deref(), "male-qn-qingse");
            let speed = normalized_speed(request.speed, 0.5, 2.0);
            let format = normalized_format(request.format.as_deref(), &MINIMAX_SPEECH_FORMATS);
            let body = serde_json::json!({
                "model": route.model,
                "text": request.text,
                "voice_setting": {"voice_id": voice, "speed": parse::js_number_to_json(speed)},
                "audio_setting": {"format": format},
            });
            Ok(OperationRequestPlan::post_json(
                format!(
                    "{origin}/v1/t2a_v2?GroupId={}",
                    encode_uri_component(group_id)
                ),
                body,
            )
            .with_auth(auth, BearerStyle::AuthorizationBearer))
        }
        ProtocolFamily::DashscopeQwenTts => {
            // Origin-only /api/v1 path; voice default Cherry; the format
            // is FIXED wav (no wire field — it is the download product's
            // default, applied service-side).
            let origin = origin_of(&route.endpoint)?;
            let voice = normalized_text(request.voice.as_deref(), "Cherry");
            let body = serde_json::json!({
                "model": route.model,
                "input": {"text": request.text, "voice": voice},
            });
            Ok(OperationRequestPlan::post_json(
                format!("{origin}/api/v1/services/aigc/multimodal-generation/generation"),
                body,
            )
            .with_auth(auth, BearerStyle::AuthorizationBearer))
        }
        other => Err(parse::invalid(format!(
            "protocol family {} does not serve speech (wiring bug: the gateway matrix \
             refuses earlier)",
            other.config_name()
        ))),
    }
}

/// Parses a minimax t2a_v2 response: the 200-embedded `base_resp` check
/// (`typeof status_code === "number" && !== 0`; the failure text is the
/// incumbent's `MiniMax speech failed: …` with the `status 200` fallback),
/// then `data.audio` = HEX-encoded audio bytes (an empty string refuses).
pub fn parse_minimax_speech(
    request_format: Option<&str>,
    body: &serde_json::Value,
) -> Result<MediaProductRef, ProtocolError> {
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
                .unwrap_or_else(|| "status 200".to_string());
            return Err(parse::invalid(format!("MiniMax speech failed: {detail}")));
        }
    }
    let hex_audio = body
        .pointer("/data/audio")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| parse::invalid("MiniMax speech returned no audio data".to_string()))?;
    let bytes = decode_hex(hex_audio)?;
    let format = normalized_format(request_format, &MINIMAX_SPEECH_FORMATS);
    Ok(MediaProductRef::Bytes {
        bytes,
        mime: minimax_speech_mime_for_format(&format).to_string(),
    })
}

/// Parses a dashscope qwen-tts response: `output.audio.url` is a
/// time-limited signed address — the service layer downloads it through
/// the egress guard WITHOUT credentials (the download's own content-type,
/// split at `;`, wins; `audio/wav` is the incumbent's fallback — the
/// `mime_hint` here).
pub fn parse_dashscope_speech(body: &serde_json::Value) -> Result<MediaProductRef, ProtocolError> {
    let url = body
        .pointer("/output/audio/url")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| parse::invalid("DashScope speech returned no audio url".to_string()))?;
    Ok(MediaProductRef::Url {
        url: url.to_string(),
        mime_hint: Some("audio/wav".to_string()),
    })
}

/// The STRICT hex decode (the incumbent's `Buffer.from(hex, "hex")`
/// silently truncates at the first invalid pair; the port refuses — a
/// corrupt payload is never materialized).
fn decode_hex(hex: &str) -> Result<Vec<u8>, ProtocolError> {
    let bytes = hex.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return Err(parse::invalid("hex audio of odd length".to_string()));
    }
    let nibble = |b: u8| -> Result<u8, ProtocolError> {
        match b {
            b'0'..=b'9' => Ok(b - b'0'),
            b'a'..=b'f' => Ok(b - b'a' + 10),
            b'A'..=b'F' => Ok(b - b'A' + 10),
            other => Err(parse::invalid(format!(
                "hex audio carries non-hex byte {other}"
            ))),
        }
    };
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for [hi, lo] in bytes.as_chunks::<2>().0 {
        out.push((nibble(*hi)? << 4) | nibble(*lo)?);
    }
    Ok(out)
}
