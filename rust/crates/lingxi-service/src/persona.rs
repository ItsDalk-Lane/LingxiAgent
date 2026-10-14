//! R06-T06 PersonaService: agent persona files (AGENTS.md / identity.md /
//! AGENTS.public.md) and avatars — the write faces of the T01 material chain
//! (writing these files changes what the next context compile reads).
//!
//! Anchors (RC-2; design doc §3):
//! - fallback chain: `core/persona-source.ts:58-73,87-116` (read side already
//!   lives in `context_compiler.rs:475`; this module shares the same file
//!   names and fallback order through the adapters store).
//! - routes: `server/routes/agents.ts` (+ `config.ts` ishiki aliases):
//!   GET/PUT agents-md (+ `/ishiki` alias), public-agents-md
//!   (+ `/public-ishiki`), identity.
//! - avatars: png/jpeg/webp, 15MB body cap; read chat scope; write
//!   settings.write (`route-security.ts:100-101,139-141`).

use std::path::PathBuf;

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, RawQuery, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use serde_json::Value;

use lingxi_adapters::storage::agent_memory as am;
use lingxi_adapters::storage::agent_memory::{validate_agent_id, MemoryError};

use crate::ServiceState;

pub struct PersonaService {
    agents_root: PathBuf,
    product_dir: PathBuf,
}

impl PersonaService {
    pub fn new(agents_root: PathBuf, product_dir: PathBuf) -> Self {
        Self {
            agents_root,
            product_dir,
        }
    }

    fn store(&self) -> am::PersonaFiles {
        am::PersonaFiles::new(&self.agents_root, &self.product_dir)
    }
}

fn error_response(err: &MemoryError) -> Response {
    let (status, body): (StatusCode, Value) = match err {
        MemoryError::InvalidRequest(msg) => (StatusCode::BAD_REQUEST, json!({ "error": msg })),
        MemoryError::AgentNotFound(msg) | MemoryError::NotFound(msg) => {
            (StatusCode::NOT_FOUND, json!({ "error": msg }))
        }
        MemoryError::Conflict { code, message } => (
            StatusCode::CONFLICT,
            json!({ "error": message, "code": code }),
        ),
        MemoryError::Io(detail) | MemoryError::Store(detail) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "error": detail }),
        ),
    };
    (status, Json(body)).into_response()
}

fn locale_of(state: &ServiceState, agent_id: &str) -> Result<String, MemoryError> {
    // config locale → prefs locale → en (shared chain with MemoryService).
    let home = state.memory().home().to_path_buf();
    let config_locale =
        am::config::read_agent_config(&home.join("agents").join(agent_id).join("config.yaml"))?
            .scan()?
            .locale;
    let prefs_locale = crate::context_compiler::scan_preferences(
        am::read_json_file(&home.join("user").join("preferences.json")).as_ref(),
    )
    .locale;
    Ok(config_locale
        .filter(|l| !l.trim().is_empty())
        .or(prefs_locale.filter(|l| !l.trim().is_empty()))
        .unwrap_or_else(|| "en".to_string()))
}

fn yuan_of(state: &ServiceState, agent_id: &str) -> Result<String, MemoryError> {
    let home = state.memory().home();
    Ok(
        am::config::read_agent_config(&home.join("agents").join(agent_id).join("config.yaml"))?
            .scan()?
            .yuan
            .unwrap_or_else(|| "yuan".to_string()),
    )
}

macro_rules! persona_get_route {
    ($handler:ident, $kind:expr) => {
        /// GET persona file (fallback chain; fromTemplate flag included).
        pub async fn $handler(
            State(state): State<ServiceState>,
            AxumPath(agent_id): AxumPath<String>,
        ) -> Response {
            if let Err(err) = validate_agent_id(&agent_id) {
                if matches!($kind, am::PersonaKind::Agents) {
                    return (StatusCode::NOT_FOUND, Json(json!({ "error": err.to_string() })))
                        .into_response();
                }
                return error_response(&err);
            }
            let locale = match locale_of(&state, &agent_id) { Ok(value) => value, Err(err) => return error_response(&err) };
            let yuan = match yuan_of(&state, &agent_id) { Ok(value) => value, Err(err) => return error_response(&err) };
            match state.persona().store().resolve_persona(&agent_id, &yuan, &locale, $kind) {
                Ok((content, from_template)) => {
                    Json(json!({ "content": content, "fromTemplate": from_template })).into_response()
                }
                Err(err) => error_response(&err),
            }
        }
    };
}

macro_rules! persona_put_route {
    ($handler:ident, $kind:expr) => {
        /// PUT persona file (persist = user customization).
        pub async fn $handler(
            State(state): State<ServiceState>,
            AxumPath(agent_id): AxumPath<String>,
            body: Bytes,
        ) -> Response {
            if let Err(err) = validate_agent_id(&agent_id) {
                return error_response(&err);
            }
            let payload = match serde_json::from_slice::<Value>(&body) {
                Ok(value) => value,
                Err(err) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": format!("invalid JSON body: {err}") })),
                    )
                        .into_response()
                }
            };
            let Some(content) = payload.get("content").and_then(Value::as_str) else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "content must be a string" })),
                )
                    .into_response();
            };
            match state.persona().store().write_persona(&agent_id, $kind, content) {
                Ok(()) => Json(json!({ "ok": true })).into_response(),
                Err(err) => error_response(&err),
            }
        }
    };
}

persona_get_route!(agents_md_get_route, am::PersonaKind::Agents);
persona_put_route!(agents_md_put_route, am::PersonaKind::Agents);
persona_get_route!(identity_get_route, am::PersonaKind::Identity);
persona_put_route!(identity_put_route, am::PersonaKind::Identity);

/// GET public persona (file-only chain, no template).
pub async fn public_agents_md_get_route(
    State(state): State<ServiceState>,
    AxumPath(agent_id): AxumPath<String>,
) -> Response {
    if let Err(err) = validate_agent_id(&agent_id) {
        return error_response(&err);
    }
    match state.persona().store().read_public(&agent_id) {
        Ok(content) => Json(json!({ "content": content })).into_response(),
        Err(err) => error_response(&err),
    }
}

/// PUT public persona.
pub async fn public_agents_md_put_route(
    State(state): State<ServiceState>,
    AxumPath(agent_id): AxumPath<String>,
    body: Bytes,
) -> Response {
    if let Err(err) = validate_agent_id(&agent_id) {
        return error_response(&err);
    }
    let payload = match serde_json::from_slice::<Value>(&body) {
        Ok(value) => value,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("invalid JSON body: {err}") })),
            )
                .into_response()
        }
    };
    let Some(content) = payload.get("content").and_then(Value::as_str) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "content must be a string" })),
        )
            .into_response();
    };
    match state.persona().store().write_public(&agent_id, content) {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(err) => error_response(&err),
    }
}

pub const MAX_AVATAR_BYTES: usize = 15 * 1024 * 1024;

/// GET /lingxi/v1/agents/{id}/avatar — first of png/jpg/jpeg/webp; 404 when
/// absent (leaf #11).
pub async fn avatar_get_route(
    State(state): State<ServiceState>,
    AxumPath(agent_id): AxumPath<String>,
) -> Response {
    if let Err(err) = validate_agent_id(&agent_id) {
        return error_response(&err);
    }
    match state.persona().store().read_avatar(&agent_id) {
        Ok(Some((_path, mime, bytes))) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, mime),
                (header::CACHE_CONTROL, "private, max-age=60"),
            ],
            bytes,
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "avatar not found" })),
        )
            .into_response(),
        Err(err) => error_response(&err),
    }
}

/// POST /lingxi/v1/agents/{id}/avatar — base64 data URL (png/jpeg/webp), 15MB
/// cap (leaf #10).
pub async fn avatar_post_route(
    State(state): State<ServiceState>,
    AxumPath(agent_id): AxumPath<String>,
    body: Bytes,
) -> Response {
    if let Err(err) = validate_agent_id(&agent_id) {
        return error_response(&err);
    }
    if body.len() > MAX_AVATAR_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({ "error": "avatar body exceeds the 15MB limit" })),
        )
            .into_response();
    }
    let payload = match serde_json::from_slice::<Value>(&body) {
        Ok(value) => value,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("invalid JSON body: {err}") })),
            )
                .into_response()
        }
    };
    let Some(data_url) = payload.get("dataUrl").and_then(Value::as_str) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "dataUrl must be a base64 data URL" })),
        )
            .into_response();
    };
    let Some((mime, b64)) = data_url
        .strip_prefix("data:")
        .and_then(|rest| rest.split_once(";base64,"))
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "dataUrl must be a base64 data URL" })),
        )
            .into_response();
    };
    let ext = match mime {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpeg",
        "image/webp" => "webp",
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "avatar must be png, jpeg or webp" })),
            )
                .into_response()
        }
    };
    use base64::Engine as _;
    let bytes = match base64::engine::general_purpose::STANDARD.decode(b64) {
        Ok(bytes) => bytes,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid base64 payload" })),
            )
                .into_response()
        }
    };
    match state.persona().store().write_avatar(&agent_id, ext, &bytes) {
        Ok(()) => Json(json!({ "ok": true, "format": ext })).into_response(),
        Err(err) => error_response(&err),
    }
}

/// DELETE /lingxi/v1/agents/{id}/avatar — idempotent ok (leaf #9).
pub async fn avatar_delete_route(
    State(state): State<ServiceState>,
    AxumPath(agent_id): AxumPath<String>,
) -> Response {
    if let Err(err) = validate_agent_id(&agent_id) {
        return error_response(&err);
    }
    match state.persona().store().delete_avatar(&agent_id) {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(err) => error_response(&err),
    }
}

/// GET /lingxi/v1/agents/{id}/avatar-path — splash helper (leaf #0): the path
/// when a custom avatar exists, else null (default fallback is a renderer
/// concern).
pub async fn avatar_path_route(
    State(state): State<ServiceState>,
    AxumPath(agent_id): AxumPath<String>,
    RawQuery(_query): RawQuery,
) -> Response {
    if let Err(err) = validate_agent_id(&agent_id) {
        return error_response(&err);
    }
    match state.persona().store().read_avatar(&agent_id) {
        Ok(Some((path, _, _))) => Json(json!({ "path": path.to_string_lossy() })).into_response(),
        Ok(None) => Json(json!({ "path": Value::Null })).into_response(),
        Err(err) => error_response(&err),
    }
}
