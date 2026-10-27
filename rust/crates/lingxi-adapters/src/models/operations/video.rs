//! Video dialect (R05-T06): agnes-videos — the incumbent
//! `core/media-adapters/agnes.ts` video surface, ported rule for rule.
//! Agnes is the only video family: an async submit (`POST {v1}/videos`)
//! plus a task query (`GET {root}/agnesapi?video_id=…`, with the incumbent
//! legacy fallback onto `GET {v1}/videos/{taskId}` when the primary answer
//! is not 2xx and a distinct legacy task id exists).
//!
//! The incumbent downloads the finished video INSIDE `query`; the port
//! surfaces the URL as a [`MediaProductRef::Url`] and leaves the download
//! to the service layer's egress guard (C11B) — the adapters never fetch
//! unguarded.

use lingxi_kernel::model_exchange::{ProtocolFamily, ResolvedModelRoute};
use lingxi_protocol::ProtocolError;

use super::image::{agnes_root_base, agnes_v1_base};
use super::tiers::truthy_json_string;
use super::{
    encode_uri_component, form_urlencoded_of, parse, MediaProductRef, OperationDispatcher,
    OperationRequestPlan, TaskPollOutcome,
};
use crate::models::credentials::ApplicableAuth;
use crate::models::dispatch::BearerStyle;

/// The incumbent frame bounds (`MIN_VIDEO_FRAMES`/`MAX_VIDEO_FRAMES`) and
/// the `8n+1` rule.
pub const AGNES_MIN_VIDEO_FRAMES: u64 = 81;
pub const AGNES_MAX_VIDEO_FRAMES: u64 = 441;
/// The single supported resolution and size (`DEFAULT_VIDEO_RESOLUTION`,
/// `AGNES_VIDEO_SIZES`).
pub const AGNES_VIDEO_RESOLUTION: &str = "720p";
pub const AGNES_VIDEO_WIDTH: u32 = 1152;
pub const AGNES_VIDEO_HEIGHT: u32 = 768;

/// One video-generation request; the service layer collapses the
/// incumbent's aliases (`video_resolution`/`videoResolution`/`resolution`
/// → `resolution`, `frameRate`/`frame_rate` → `frame_rate`,
/// `numFrames`/`num_frames` → `num_frames`, `duration`/`seconds` →
/// `duration`, aspect aliases → `ratio`) before the dialect sees them.
/// The numeric knobs stay `f64` so the incumbent's `Number.isInteger`
/// checks can refuse non-integers with the incumbent's own texts.
#[derive(Debug, Clone, Default)]
pub struct VideoRequest {
    pub prompt: String,
    pub resolution: Option<String>,
    pub size: Option<String>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub ratio: Option<String>,
    pub frame_rate: Option<f64>,
    pub num_frames: Option<f64>,
    pub duration: Option<f64>,
    pub references: Vec<super::ImageReference>,
}

/// A settled agnes video submit: the task id the poller uses and the
/// provider-facing id when the response distinguishes them (the incumbent
/// `{ taskId, providerTaskId }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoSubmitOutcome {
    pub task_id: String,
    pub provider_task_id: String,
}

/// One agnes video query: the task id under poll, the legacy fallback id
/// (the incumbent `task.taskId` when distinct), and the model name the
/// primary query carries when known.
#[derive(Debug, Clone)]
pub struct AgnesVideoQuery {
    pub task_id: String,
    pub legacy_task_id: Option<String>,
    pub model_name: Option<String>,
}

/// The incumbent JS `Number(x)` rendering inside refusal texts (`NaN`,
/// integers without a fraction, everything else shortest-round-trip).
fn js_number_display(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if value.fract() == 0.0 && value.abs() < 1e15 {
        return format!("{}", value as i64);
    }
    format!("{value}")
}

/// A JSON number source with JS `Number(...)` coercion semantics for the
/// numeric knobs (strings parse, booleans count, null/absent is absent).
fn json_number(value: Option<&serde_json::Value>) -> Option<f64> {
    match value? {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Some(0.0)
            } else {
                Some(trimmed.parse::<f64>().unwrap_or(f64::NAN))
            }
        }
        serde_json::Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// `resolveVideoSize` (agnes): the resolution must be `720p`; an explicit
/// size must be `1152x768`; a width/height pair must both be integers
/// forming exactly `1152x768`; otherwise the ratio (only `3:2` exists)
/// picks the table entry.
fn resolve_agnes_video_size(
    request: &VideoRequest,
    provider_defaults: &serde_json::Value,
) -> Result<(u32, u32), ProtocolError> {
    let resolution = request
        .resolution
        .as_deref()
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .or_else(|| truthy_json_string(provider_defaults.get("video_resolution")))
        .or_else(|| truthy_json_string(provider_defaults.get("videoResolution")))
        .or_else(|| truthy_json_string(provider_defaults.get("resolution")))
        .unwrap_or_else(|| AGNES_VIDEO_RESOLUTION.to_string());
    if resolution.trim().to_lowercase() != AGNES_VIDEO_RESOLUTION {
        return Err(parse::invalid(format!(
            "Agnes video resolution \"{resolution}\" is unsupported; supported resolutions: \
             {AGNES_VIDEO_RESOLUTION}"
        )));
    }
    let supported_sizes = format!("{AGNES_VIDEO_WIDTH}x{AGNES_VIDEO_HEIGHT}");

    let explicit_size = request
        .size
        .as_deref()
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .or_else(|| truthy_json_string(provider_defaults.get("size")));
    if let Some(explicit_size) = explicit_size {
        let size = explicit_size.trim().to_lowercase();
        if size != supported_sizes {
            return Err(parse::invalid(format!(
                "Agnes video size \"{explicit_size}\" is unsupported; supported sizes: \
                 {supported_sizes}"
            )));
        }
        return Ok((AGNES_VIDEO_WIDTH, AGNES_VIDEO_HEIGHT));
    }

    let width = request
        .width
        .or_else(|| json_number(provider_defaults.get("width")));
    let height = request
        .height
        .or_else(|| json_number(provider_defaults.get("height")));
    if width.is_some() || height.is_some() {
        let width = width.unwrap_or(f64::NAN);
        let height = height.unwrap_or(f64::NAN);
        let size = format!("{}x{}", js_number_display(width), js_number_display(height));
        let valid = width.fract() == 0.0
            && !width.is_nan()
            && height.fract() == 0.0
            && !height.is_nan()
            && size == supported_sizes;
        if !valid {
            return Err(parse::invalid(format!(
                "Agnes video size \"{size}\" is unsupported; supported sizes: {supported_sizes}"
            )));
        }
        return Ok((AGNES_VIDEO_WIDTH, AGNES_VIDEO_HEIGHT));
    }

    let ratio = request
        .ratio
        .as_deref()
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .or_else(|| truthy_json_string(provider_defaults.get("aspect_ratio")))
        .or_else(|| truthy_json_string(provider_defaults.get("aspectRatio")))
        .or_else(|| truthy_json_string(provider_defaults.get("ratio")))
        .unwrap_or_else(|| "3:2".to_string());
    if ratio != "3:2" {
        return Err(parse::invalid(format!(
            "Agnes video ratio \"{ratio}\" is unsupported; supported ratios: 3:2"
        )));
    }
    Ok((AGNES_VIDEO_WIDTH, AGNES_VIDEO_HEIGHT))
}

/// `resolveVideoFrameCount` (agnes): explicit `num_frames` must be an
/// `8n+1` integer in 81..=441; otherwise `duration * frame_rate` must
/// round onto such a count.
fn resolve_agnes_video_frame_count(
    request: &VideoRequest,
    provider_defaults: &serde_json::Value,
) -> Result<(u64, u64), ProtocolError> {
    let frame_rate = request
        .frame_rate
        .or_else(|| json_number(provider_defaults.get("frameRate")))
        .or_else(|| json_number(provider_defaults.get("frame_rate")))
        .unwrap_or(24.0);
    if frame_rate.fract() != 0.0 || frame_rate.is_nan() || frame_rate < 1.0 || frame_rate > 60.0 {
        return Err(parse::invalid(format!(
            "Agnes video frame_rate \"{}\" is unsupported; supported range: 1-60",
            js_number_display(frame_rate)
        )));
    }
    let frame_rate = frame_rate as u64;

    let explicit = request
        .num_frames
        .or_else(|| json_number(provider_defaults.get("numFrames")))
        .or_else(|| json_number(provider_defaults.get("num_frames")));
    if let Some(explicit) = explicit {
        let num_frames = explicit.floor();
        if explicit.fract() != 0.0
            || explicit.is_nan()
            || num_frames < AGNES_MIN_VIDEO_FRAMES as f64
            || num_frames > AGNES_MAX_VIDEO_FRAMES as f64
            || !(num_frames as u64 - 1).is_multiple_of(8)
        {
            return Err(parse::invalid(format!(
                "Agnes video num_frames \"{}\" is unsupported; it must be 8n+1 between \
                 {AGNES_MIN_VIDEO_FRAMES} and {AGNES_MAX_VIDEO_FRAMES}",
                js_number_display(explicit)
            )));
        }
        return Ok((frame_rate, num_frames as u64));
    }

    let duration = request
        .duration
        .or_else(|| json_number(provider_defaults.get("duration")))
        .or_else(|| json_number(provider_defaults.get("seconds")))
        .unwrap_or(5.0);
    if duration.fract() != 0.0 || duration.is_nan() || duration < 3.0 || duration > 18.0 {
        return Err(parse::invalid(format!(
            "Agnes video duration \"{}\" is unsupported; supported range: 3-18 seconds",
            js_number_display(duration)
        )));
    }
    let target_frames = (duration * frame_rate as f64).round() as i64 + 1;
    if target_frames < AGNES_MIN_VIDEO_FRAMES as i64
        || target_frames > AGNES_MAX_VIDEO_FRAMES as i64
        || (target_frames - 1) % 8 != 0
    {
        return Err(parse::invalid(format!(
            "Agnes video duration \"{}\" with frame_rate \"{frame_rate}\" cannot be \
             represented as a supported 8n+1 frame count",
            js_number_display(duration)
        )));
    }
    Ok((frame_rate, target_frames as u64))
}

/// Builds the agnes video submit plan (`POST {v1}/videos`).
pub fn build_video_submit(
    route: &ResolvedModelRoute,
    auth: &ApplicableAuth,
    request: &VideoRequest,
    provider_defaults: &serde_json::Value,
) -> Result<OperationRequestPlan, ProtocolError> {
    match route.protocol {
        ProtocolFamily::AgnesVideos => {}
        other => {
            return Err(parse::invalid(format!(
                "protocol family {} does not serve video generation (the gateway's \
                 serves-matrix refusal precedes dispatch; reaching here is a wiring bug)",
                other.config_name()
            )))
        }
    }
    let (width, height) = resolve_agnes_video_size(request, provider_defaults)?;
    let (frame_rate, num_frames) = resolve_agnes_video_frame_count(request, provider_defaults)?;
    let mut body = serde_json::json!({
        "model": route.model,
        "prompt": request.prompt,
        "width": width,
        "height": height,
        "frame_rate": frame_rate,
        "num_frames": num_frames,
    });
    if request.references.len() == 1 {
        body["image"] = serde_json::Value::from(request.references[0].wire_string());
    } else if request.references.len() > 1 {
        body["extra_body"] = serde_json::json!({
            "image": request.references.iter().map(|r| r.wire_string()).collect::<Vec<_>>(),
        });
    }
    let url = format!("{}/videos", agnes_v1_base(&route.endpoint));
    Ok(
        OperationRequestPlan::post_json(url, body)
            .with_auth(auth, BearerStyle::AuthorizationBearer),
    )
}

/// Parses the agnes video submit answer: `task_id`/`id`/`video_id` (in
/// that order for the tracker id; `video_id`/`task_id`/`id` for the
/// provider-facing id). A response without ANY task id is a loud failure
/// (the incumbent fabricates a local tracking id there — polling a
/// fabricated id is unverifiable).
pub fn parse_video_submit(body: &serde_json::Value) -> Result<VideoSubmitOutcome, ProtocolError> {
    let pick = |keys: &[&str]| -> Option<String> {
        keys.iter().find_map(|key| {
            body.get(*key)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
    };
    let provider_task_id = pick(&["video_id", "task_id", "id"]);
    let task_id = pick(&["task_id", "id", "video_id"]).or_else(|| provider_task_id.clone());
    let (Some(task_id), Some(provider_task_id)) = (task_id, provider_task_id) else {
        return Err(parse::invalid(
            "agnes video submit returned no task id (task_id/id/video_id all absent); refusing \
             instead of tracking a fabricated local id"
                .to_string(),
        ));
    };
    Ok(VideoSubmitOutcome {
        task_id,
        provider_task_id,
    })
}

/// The incumbent `videoUrlFromResponse`: the first http(s) string among
/// `remixed_from_video_id`/`video_url`/`url`/`output_url`, else the same
/// search over each `data[]` entry (recursively).
fn agnes_video_url_from_response(data: &serde_json::Value) -> Option<String> {
    for key in ["remixed_from_video_id", "video_url", "url", "output_url"] {
        if let Some(value) = data.get(key).and_then(|v| v.as_str()) {
            let lower = value.to_lowercase();
            if lower.starts_with("http://") || lower.starts_with("https://") {
                return Some(value.to_string());
            }
        }
    }
    if let Some(items) = data.get("data").and_then(|v| v.as_array()) {
        for item in items {
            if let Some(url) = agnes_video_url_from_response(item) {
                return Some(url);
            }
        }
    }
    None
}

/// Parses one agnes query answer body (the incumbent `query` status map):
/// failed/error/cancelled/canceled → failed with the provider's reason;
/// completed/success/succeeded/done without a video URL stays honestly
/// pending; anything else is pending.
pub fn parse_agnes_video_query_body(body: &serde_json::Value) -> TaskPollOutcome {
    let status = body
        .get("status")
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    if ["failed", "error", "cancelled", "canceled"].contains(&status.as_str()) {
        let reason = body
            .pointer("/error/message")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                body.get("message")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or("Agnes video generation failed");
        return TaskPollOutcome::Failed {
            reason: reason.to_string(),
        };
    }
    if !["completed", "success", "succeeded", "done"].contains(&status.as_str()) {
        return TaskPollOutcome::Pending;
    }
    let Some(url) = agnes_video_url_from_response(body) else {
        return TaskPollOutcome::Pending;
    };
    TaskPollOutcome::Done {
        products: vec![MediaProductRef::Url {
            url,
            mime_hint: None,
        }],
    }
}

/// Executes one agnes video query with the incumbent's legacy fallback:
/// the primary `GET {root}/agnesapi?video_id=…[&model_name=…]`; on a
/// non-2xx primary answer, a distinct legacy task id retries against
/// `GET {v1}/videos/{legacyTaskId}`; a non-2xx without a fallback
/// classifies through the shared table (the incumbent throws a bare
/// `API error {status}` — the shared classification is the registered
/// discipline of this crate).
pub async fn execute_agnes_video_query(
    dispatcher: &OperationDispatcher,
    route: &ResolvedModelRoute,
    auth: &ApplicableAuth,
    query: &AgnesVideoQuery,
    deadline_unix_ms: Option<u64>,
) -> Result<TaskPollOutcome, (ProtocolError, bool)> {
    let mut parameters = format!("video_id={}", form_urlencoded_of(&query.task_id));
    if let Some(model_name) = query.model_name.as_deref().filter(|v| !v.is_empty()) {
        parameters.push_str(&format!("&model_name={}", form_urlencoded_of(model_name)));
    }
    let primary = OperationRequestPlan::get(format!(
        "{}/agnesapi?{parameters}",
        agnes_root_base(&route.endpoint)
    ))
    .with_auth(auth, BearerStyle::AuthorizationBearer);
    let response = dispatcher
        .send_unclassified(&primary, deadline_unix_ms)
        .await?;
    if response.status().is_success() {
        let body = dispatcher
            .read_json_body(response, deadline_unix_ms)
            .await?;
        return Ok(parse_agnes_video_query_body(&body));
    }
    let legacy_task_id = query
        .legacy_task_id
        .as_deref()
        .filter(|legacy| !legacy.is_empty() && *legacy != query.task_id);
    let Some(legacy_task_id) = legacy_task_id else {
        return Err(dispatcher
            .classify_error_response(response, auth, deadline_unix_ms)
            .await);
    };
    let legacy = OperationRequestPlan::get(format!(
        "{}/videos/{}",
        agnes_v1_base(&route.endpoint),
        encode_uri_component(legacy_task_id)
    ))
    .with_auth(auth, BearerStyle::AuthorizationBearer);
    let response = dispatcher
        .send_unclassified(&legacy, deadline_unix_ms)
        .await?;
    if !response.status().is_success() {
        return Err(dispatcher
            .classify_error_response(response, auth, deadline_unix_ms)
            .await);
    }
    let body = dispatcher
        .read_json_body(response, deadline_unix_ms)
        .await?;
    Ok(parse_agnes_video_query_body(&body))
}
