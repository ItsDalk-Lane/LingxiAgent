//! R06-T06 MemoryService: per-agent memory faces (facts / tenets / compiled
//! memory / experience / dream revisions / context notes).
//!
//! Route/permission anchors (RC-2, base `e89c91a2…`; design doc §3):
//! - `server/routes/config.ts:406-681` — memory CRUD faces; the explicit
//!   `agentId` is the ownership boundary (`getStoreForAgent` :419-427);
//!   writes require `settings.write` (`server/http/route-security.ts:464-493`),
//!   reads `settings.read` (:361-399).
//! - `server/routes/agents.ts:936-1188` — pinned shim / experience / tenets.
//! - `server/routes/memory-dream.ts:54-136` — dream faces (ticker-absent →
//!   `503 dream_unavailable`).
//!
//! Every face resolves the per-agent roots through
//! [`lingxi_adapters::storage::agent_memory::resolve_agent_roots`]: physical
//! per-agent isolation (A11). There is NO cross-agent handle anywhere in this
//! module — a request names exactly one agent and touches only that agent's
//! files.

use std::path::{Path, PathBuf};

use axum::extract::{Path as AxumPath, RawQuery, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use lingxi_adapters::storage::agent_memory as am;
use lingxi_adapters::storage::agent_memory::{
    resolve_agent_roots, validate_agent_id, AgentConfigFlags, AgentConfigPatch,
    CompiledMemoryStore, ContextNotesStore, DreamRevisionStore, ExperienceStore, FactStore,
    MemoryError, TenetsStore,
};

use crate::context_compiler::{scan_agent_config, scan_preferences};
use crate::ServiceState;

pub struct MemoryService {
    home: PathBuf,
    runtime_dir: PathBuf,
}

impl MemoryService {
    pub fn new(home: PathBuf, runtime_dir: PathBuf) -> Self {
        Self { home, runtime_dir }
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn agents_root(&self) -> PathBuf {
        self.home.join("agents")
    }

    pub fn user_dir(&self) -> PathBuf {
        self.home.join("user")
    }

    /// Locale chain (persona-source.ts:124-130 + agent.ts resolveLocale):
    /// agent config locale → global prefs locale → "en".
    pub fn locale_is_zh(&self, agent_dir: &Path) -> bool {
        let config_locale = std::fs::read_to_string(agent_dir.join("config.yaml"))
            .ok()
            .and_then(|text| scan_agent_config(&text).locale);
        let prefs_locale = scan_preferences(
            am::read_json_file(&self.user_dir().join("preferences.json")).as_ref(),
        )
        .locale;
        let locale = config_locale
            .filter(|l| !l.trim().is_empty())
            .or(prefs_locale.filter(|l| !l.trim().is_empty()))
            .unwrap_or_else(|| "en".to_string());
        locale.starts_with("zh")
    }

    pub fn roots(&self, agent_id: &str) -> Result<am::AgentMemoryRoots, MemoryError> {
        resolve_agent_roots(&self.agents_root(), agent_id)
    }

    pub fn fact_store(&self, agent_id: &str) -> Result<FactStore, MemoryError> {
        let roots = self.roots(agent_id)?;
        FactStore::open(&roots.memory_dir.join("facts.db"))
    }

    pub fn tenets_store(&self, agent_id: &str) -> Result<TenetsStore, MemoryError> {
        let roots = self.roots(agent_id)?;
        Ok(TenetsStore::new(&roots.memory_dir))
    }

    pub fn compiled_store(&self, agent_id: &str) -> Result<CompiledMemoryStore, MemoryError> {
        let roots = self.roots(agent_id)?;
        let is_zh = self.locale_is_zh(&roots.agent_dir);
        Ok(CompiledMemoryStore::new(&roots.memory_dir, is_zh))
    }

    pub fn experience_store(&self, agent_id: &str) -> Result<ExperienceStore, MemoryError> {
        let roots = self.roots(agent_id)?;
        Ok(ExperienceStore::new(&roots.agent_dir))
    }

    pub fn dream_store(&self, agent_id: &str) -> Result<DreamRevisionStore, MemoryError> {
        let roots = self.roots(agent_id)?;
        Ok(DreamRevisionStore::new(&roots.memory_dir))
    }

    pub fn context_notes(&self) -> ContextNotesStore {
        ContextNotesStore::new(&self.runtime_dir)
    }

    pub fn config_path(&self, agent_id: &str) -> Result<PathBuf, MemoryError> {
        let roots = self.roots(agent_id)?;
        Ok(roots.agent_dir.join("config.yaml"))
    }

    /// config.yaml flags read (memory.enabled / experience.enabled /
    /// workspace_context policy).
    pub fn read_flags(&self, agent_id: &str) -> Result<AgentConfigFlags, MemoryError> {
        let path = self.config_path(agent_id)?;
        let text = std::fs::read_to_string(&path).map_err(|e| MemoryError::Io(e.to_string()))?;
        Ok(AgentConfigPatch::read_flags(&text))
    }

    pub fn patch_flags(
        &self,
        agent_id: &str,
        patch: AgentConfigFlags,
    ) -> Result<String, MemoryError> {
        let path = self.config_path(agent_id)?;
        am::patch_agent_config(&path, |text| AgentConfigPatch::write_flags(text, &patch))
    }

    /// Primary agent id for tool binding: preferences `primary_agent_id` →
    /// first id-sorted agent dir with a config.yaml (deterministic).
    pub fn resolve_primary_agent(&self) -> Option<String> {
        let prefs = am::read_json_file(&self.user_dir().join("preferences.json"));
        if let Some(primary) = prefs
            .as_ref()
            .and_then(|p| p.get("primary_agent_id"))
            .and_then(Value::as_str)
        {
            if validate_agent_id(primary).is_ok() && self.agents_root().join(primary).is_dir() {
                return Some(primary.to_string());
            }
        }
        let mut candidates: Vec<String> = std::fs::read_dir(self.agents_root())
            .ok()?
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| {
                validate_agent_id(n).is_ok()
                    && self.agents_root().join(n).join("config.yaml").is_file()
            })
            .collect();
        candidates.sort();
        candidates.into_iter().next()
    }
}

/// MemoryError → HTTP response with the incumbent error vocabulary.
fn error_response(err: &MemoryError) -> Response {
    let (status, body): (StatusCode, Value) = match err {
        MemoryError::InvalidRequest(msg) => (StatusCode::BAD_REQUEST, json!({ "error": msg })),
        MemoryError::AgentNotFound(msg) | MemoryError::NotFound(msg) => {
            (StatusCode::NOT_FOUND, json!({ "error": msg }))
        }
        MemoryError::Conflict { code, message } => {
            let status = match code.as_str() {
                "TENET_NOT_FOUND" => StatusCode::NOT_FOUND,
                "TENET_LIMIT_REACHED" | "TENET_PENDING_FULL" | "TENET_INVALID" => {
                    StatusCode::CONFLICT
                }
                "CONTEXT_NOTES_OVERSIZE" => StatusCode::CONFLICT,
                _ => StatusCode::CONFLICT,
            };
            (status, json!({ "error": message, "code": code }))
        }
        MemoryError::Io(detail) | MemoryError::Store(detail) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "error": detail }),
        ),
    };
    (status, Json(body)).into_response()
}

#[allow(clippy::result_large_err)] // Err 直携构造好的响应（fsread.rs 同例）；拒绝路径非热路径。
fn agent_id_from_query(query: Option<&str>) -> Result<String, Response> {
    let raw = query.unwrap_or("");
    let mut agent_id: Option<String> = None;
    for pair in raw.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if key != "agentId" {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("unknown memory query parameter: {key}") })),
            )
                .into_response());
        }
        if agent_id.is_some() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "duplicate agentId query parameter" })),
            )
                .into_response());
        }
        agent_id = Some(urldecode(value));
    }
    match agent_id {
        Some(id) if !id.is_empty() => Ok(id),
        _ => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "agentId is required" })),
        )
            .into_response()),
    }
}

/// Percent-decodes one query value (`application/x-www-form-urlencoded`
/// rules: `+` → space, `%XX` → byte; a malformed escape stays literal).
/// Decoding happens at the byte level and is lifted lossily to UTF-8, so
/// multi-byte sequences survive and no slice can panic on a non-char
/// boundary; the caller re-validates the id (`validate_agent_id`).
///
/// Every arm advances the cursor by at least one byte — the loop is
/// structurally bounded by `bytes.len()`. The previous revision's
/// catch-all arm pushed without advancing and spun forever on any literal
/// byte (`agentId=agentA`), growing the output String unboundedly (the
/// R06-T06 machine-exhaustion incident).
fn urldecode(value: &str) -> String {
    fn hex_digit(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }
    let bytes = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                match (hex_digit(bytes[i + 1]), hex_digit(bytes[i + 2])) {
                    (Some(hi), Some(lo)) => {
                        out.push((hi << 4) | lo);
                        i += 3;
                    }
                    _ => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[allow(clippy::result_large_err)] // Err 直携构造好的响应（fsread.rs 同例）；拒绝路径非热路径。
fn body_json(bytes: &[u8]) -> Result<Value, Response> {
    serde_json::from_slice(bytes).map_err(|err| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("invalid JSON body: {err}") })),
        )
            .into_response()
    })
}

// ── facts faces ─────────────────────────────────────────────────────────────

/// GET /lingxi/v1/memories?agentId= (config.ts:443-448).
pub async fn memories_list_route(
    State(state): State<ServiceState>,
    RawQuery(query): RawQuery,
) -> Response {
    let agent_id = match agent_id_from_query(query.as_deref()) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    match state
        .memory()
        .fact_store(&agent_id)
        .and_then(|store| store.export_all())
    {
        Ok(rows) => Json(json!({ "memories": rows.iter().map(fact_row_json).collect::<Vec<_>>() }))
            .into_response(),
        Err(err) => error_response(&err),
    }
}

fn fact_row_json(row: &am::FactRow) -> Value {
    json!({
        "id": row.id,
        "fact": row.fact,
        "tags": row.tags,
        "time": row.time,
        "session_id": row.session_id,
        "created_at": row.created_at,
    })
}

/// GET /lingxi/v1/memories/health?agentId= (config.ts:133-160 buildMemoryHealth
/// candidate shape; the ticker-dependent steps report their absence honestly).
pub async fn memories_health_route(
    State(state): State<ServiceState>,
    RawQuery(query): RawQuery,
) -> Response {
    let agent_id = match agent_id_from_query(query.as_deref()) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let service = state.memory();
    let flags = match service.read_flags(&agent_id) {
        Ok(flags) => flags,
        Err(err) => return error_response(&err),
    };
    let enabled = flags.memory_enabled.unwrap_or(true);
    let mut steps = Vec::new();
    let mut healthy = enabled;
    if !enabled {
        steps.push(json!({"step": "memory_ticker", "ok": false, "reason": "memory_disabled"}));
    } else {
        match service.fact_store(&agent_id) {
            Ok(store) => match store.size() {
                Ok(_) => steps.push(json!({"step": "fact_store", "ok": true})),
                Err(err) => {
                    healthy = false;
                    steps
                        .push(json!({"step": "fact_store", "ok": false, "error": err.to_string()}));
                }
            },
            Err(err) => {
                healthy = false;
                steps.push(json!({"step": "fact_store", "ok": false, "error": err.to_string()}));
            }
        }
        // The automated ticker (deep-memory/dream scheduling) is the R07
        // background face — its absence is reported, never faked (diff ledger).
        steps.push(
            json!({"step": "memory_ticker", "ok": false, "reason": "ticker_absent_in_candidate"}),
        );
    }
    Json(json!({
        "agentId": agent_id,
        "enabled": enabled,
        "healthy": healthy,
        "steps": steps,
    }))
    .into_response()
}

/// GET /lingxi/v1/memories/compiled?agentId= (config.ts:457-475).
pub async fn memories_compiled_get_route(
    State(state): State<ServiceState>,
    RawQuery(query): RawQuery,
) -> Response {
    let agent_id = match agent_id_from_query(query.as_deref()) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let service = state.memory();
    let compiled = match service.compiled_store(&agent_id) {
        Ok(store) => store,
        Err(err) => return error_response(&err),
    };
    if let Err(err) = compiled.migrate_legacy_editable_facts() {
        return error_response(&err);
    }
    match compiled.read_sections() {
        Ok(sections) => {
            let content = match compiled.read_assembled() {
                Ok(content) => content,
                Err(err) => return error_response(&err),
            };
            Json(json!({
                "content": content,
                "editableFactsEnabled": true,
                "sections": {
                    "facts": sections.facts,
                    "today": sections.today,
                    "week": sections.week,
                    "longterm": sections.longterm,
                },
            }))
            .into_response()
        }
        Err(err) => error_response(&err),
    }
}

macro_rules! compiled_put_route {
    ($handler:ident, $field:literal) => {
        /// PUT /lingxi/v1/memories/compiled/{facts,today,longterm}?agentId=
        pub async fn $handler(
            State(state): State<ServiceState>,
            RawQuery(query): RawQuery,
            body: axum::body::Bytes,
        ) -> Response {
            let agent_id = match agent_id_from_query(query.as_deref()) {
                Ok(id) => id,
                Err(resp) => return resp,
            };
            let payload = match body_json(&body) {
                Ok(value) => value,
                Err(resp) => return resp,
            };
            let Some(text) = payload.get($field).and_then(Value::as_str) else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": concat!($field, " must be a string") })),
                )
                    .into_response();
            };
            let service = state.memory();
            let compiled = match service.compiled_store(&agent_id) {
                Ok(store) => store,
                Err(err) => return error_response(&err),
            };
            if let Err(err) = compiled.migrate_legacy_editable_facts() {
                return error_response(&err);
            }
            let result = match $field {
                "facts" => compiled.write_facts(text),
                "today" => compiled.write_today(text),
                _ => compiled.write_longterm(text),
            };
            match result {
                Ok(normalized) => Json(json!({ "ok": true, $field: normalized })).into_response(),
                Err(err) => error_response(&err),
            }
        }
    };
}

compiled_put_route!(memories_compiled_facts_route, "facts");
compiled_put_route!(memories_compiled_today_route, "today");
compiled_put_route!(memories_compiled_longterm_route, "longterm");

/// GET /lingxi/v1/memories/compiled/week/days?agentId= (config.ts:546-556).
pub async fn memories_week_days_route(
    State(state): State<ServiceState>,
    RawQuery(query): RawQuery,
) -> Response {
    let agent_id = match agent_id_from_query(query.as_deref()) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    match state
        .memory()
        .compiled_store(&agent_id)
        .and_then(|store| store.list_week_days())
    {
        Ok(days) => Json(json!({
            "days": days.iter().map(|(date, body)| json!({"date": date, "body": body})).collect::<Vec<_>>()
        }))
        .into_response(),
        Err(err) => error_response(&err),
    }
}

/// PUT /lingxi/v1/memories/compiled/week/days/{date}?agentId= (config.ts:559-586).
pub async fn memories_week_day_put_route(
    State(state): State<ServiceState>,
    AxumPath(date): AxumPath<String>,
    RawQuery(query): RawQuery,
    body: axum::body::Bytes,
) -> Response {
    let agent_id = match agent_id_from_query(query.as_deref()) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let payload = match body_json(&body) {
        Ok(value) => value,
        Err(resp) => return resp,
    };
    let Some(text) = payload.get("body").and_then(Value::as_str) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "body must be a string" })),
        )
            .into_response();
    };
    match state
        .memory()
        .compiled_store(&agent_id)
        .and_then(|store| store.write_week_day(&date, text))
    {
        Ok(normalized) => {
            Json(json!({ "ok": true, "date": date, "body": normalized })).into_response()
        }
        Err(err) => error_response(&err),
    }
}

/// DELETE /lingxi/v1/memories/compiled?agentId= (config.ts:590-604): reset
/// marker + clear compiled artifacts (fact sources clearing belongs to the
/// summary store the candidate has not migrated — reported, not faked).
pub async fn memories_compiled_delete_route(
    State(state): State<ServiceState>,
    RawQuery(query): RawQuery,
) -> Response {
    let agent_id = match agent_id_from_query(query.as_deref()) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let service = state.memory();
    let compiled = match service.compiled_store(&agent_id) {
        Ok(store) => store,
        Err(err) => return error_response(&err),
    };
    let result = (|| -> Result<(), MemoryError> {
        compiled.write_reset_marker()?;
        compiled.clear_artifacts()?;
        Ok(())
    })();
    match result {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(err) => error_response(&err),
    }
}

/// DELETE /lingxi/v1/memories?agentId= (config.ts:607-626): facts.db clearAll
/// + compiled reset/clear.
pub async fn memories_delete_route(
    State(state): State<ServiceState>,
    RawQuery(query): RawQuery,
) -> Response {
    let agent_id = match agent_id_from_query(query.as_deref()) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let service = state.memory();
    let result = (|| -> Result<(), MemoryError> {
        let compiled = service.compiled_store(&agent_id)?;
        compiled.write_reset_marker()?;
        let store = service.fact_store(&agent_id)?;
        store.clear_all()?;
        compiled.clear_artifacts()?;
        Ok(())
    })();
    match result {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(err) => error_response(&err),
    }
}

/// GET /lingxi/v1/memories/export?agentId= (config.ts:630-644).
pub async fn memories_export_route(
    State(state): State<ServiceState>,
    RawQuery(query): RawQuery,
) -> Response {
    let agent_id = match agent_id_from_query(query.as_deref()) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    match state
        .memory()
        .fact_store(&agent_id)
        .and_then(|store| store.export_all())
    {
        Ok(rows) => Json(json!({
            "version": 2,
            "exportedAt": am::now_iso(),
            "facts": rows.iter().map(fact_row_json).collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(err) => error_response(&err),
    }
}

/// POST /lingxi/v1/memories/import?agentId= (config.ts:648-681): accepts v2
/// `facts` and legacy v1 `memories` shapes.
pub async fn memories_import_route(
    State(state): State<ServiceState>,
    RawQuery(query): RawQuery,
    body: axum::body::Bytes,
) -> Response {
    let agent_id = match agent_id_from_query(query.as_deref()) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let payload = match body_json(&body) {
        Ok(value) => value,
        Err(resp) => return resp,
    };
    let entries = payload
        .get("facts")
        .or_else(|| payload.get("memories"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if entries.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "facts must be a non-empty array" })),
        )
            .into_response();
    }
    // Owned normalization (v1 memories / v2 facts shapes).
    let owned: Vec<(String, Vec<String>, Option<String>, String)> = entries
        .iter()
        .map(|entry| {
            (
                entry
                    .get("fact")
                    .or_else(|| entry.get("content"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                entry
                    .get("tags")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default(),
                entry
                    .get("time")
                    .or_else(|| entry.get("date"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                entry
                    .get("session_id")
                    .and_then(Value::as_str)
                    .map(|s| {
                        if s.is_empty() {
                            "imported".to_string()
                        } else {
                            s.to_string()
                        }
                    })
                    .unwrap_or_else(|| "imported".to_string()),
            )
        })
        .collect();
    let inserts: Vec<am::FactInsert<'_>> = owned
        .iter()
        .map(|(fact, tags, time, session)| am::FactInsert {
            fact,
            tags,
            time: time.as_deref(),
            session_id: Some(session.as_str()),
        })
        .collect();
    match state
        .memory()
        .fact_store(&agent_id)
        .and_then(|store| store.import_all(&inserts))
    {
        Ok(count) => Json(json!({ "ok": true, "imported": count })).into_response(),
        Err(err) => error_response(&err),
    }
}

// ── dream faces (memory-dream.ts) ───────────────────────────────────────────

/// GET /lingxi/v1/memories/dream/status — the automated dream ticker is the
/// R07 background face; its absence is `503 dream_unavailable` (the incumbent
/// reports the same code when `ticker?.getDreamStatus` is missing, :57-58).
pub async fn dream_status_route(
    State(state): State<ServiceState>,
    RawQuery(query): RawQuery,
) -> Response {
    let agent_id = match agent_id_from_query(query.as_deref()) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    if let Err(err) = state.memory().dream_store(&agent_id) {
        return error_response(&err);
    }
    dream_unavailable(agent_id)
}

fn dream_unavailable(agent_id: String) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "code": "dream_unavailable",
            "error": "Memory Dream is unavailable for this agent",
            "agentId": agent_id,
            "reason": "dream_ticker_absent_in_candidate",
        })),
    )
        .into_response()
}

/// POST /lingxi/v1/memories/dream/runs — same 503 semantics (:66-85).
pub async fn dream_run_route(
    State(state): State<ServiceState>,
    RawQuery(query): RawQuery,
) -> Response {
    let agent_id = match agent_id_from_query(query.as_deref()) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    if let Err(err) = state.memory().dream_store(&agent_id) {
        return error_response(&err);
    }
    dream_unavailable(agent_id)
}

fn dream_revision_summary(revision: &Value) -> Value {
    let empty = vec![];
    let sections = revision.get("before");
    let text_len = |key: &str| {
        sections
            .and_then(|s| s.get(key))
            .and_then(Value::as_str)
            .map(str::len)
            .unwrap_or(0)
    };
    let week: usize = sections
        .and_then(|s| s.get("weekDays"))
        .and_then(Value::as_array)
        .unwrap_or(&empty)
        .iter()
        .map(|d| {
            d.get("body")
                .and_then(Value::as_str)
                .map(str::len)
                .unwrap_or(0)
        })
        .sum();
    let mut summary = revision.clone();
    summary["bodyChars"] =
        json!(text_len("facts") + text_len("today") + week + text_len("longterm"));
    summary["sectionChars"] = json!({
        "facts": text_len("facts"),
        "today": text_len("today"),
        "week": week,
        "longterm": text_len("longterm"),
    });
    if let Some(obj) = summary.as_object_mut() {
        obj.remove("before");
    }
    summary
}

/// GET /lingxi/v1/memories/dream/revisions?agentId= (:87-97).
pub async fn dream_revisions_route(
    State(state): State<ServiceState>,
    RawQuery(query): RawQuery,
) -> Response {
    let agent_id = match agent_id_from_query(query.as_deref()) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    match state
        .memory()
        .dream_store(&agent_id)
        .and_then(|store| store.list())
    {
        Ok(revisions) => Json(json!({
            "agentId": agent_id,
            "revisions": revisions.iter().map(dream_revision_summary).collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(err) => error_response(&err),
    }
}

/// GET /lingxi/v1/memories/dream/revisions/{id}?agentId= (:99-118): revision
/// plus the CURRENT compiled snapshot read live.
pub async fn dream_revision_detail_route(
    State(state): State<ServiceState>,
    AxumPath(revision_id): AxumPath<String>,
    RawQuery(query): RawQuery,
) -> Response {
    let agent_id = match agent_id_from_query(query.as_deref()) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let service = state.memory();
    let store = match service.dream_store(&agent_id) {
        Ok(store) => store,
        Err(err) => return error_response(&err),
    };
    match store.read(&revision_id) {
        Ok(revision) => {
            let current = match store.snapshot_sections() {
                Ok(current) => current,
                Err(err) => return error_response(&err),
            };
            Json(json!({
                "agentId": agent_id,
                "revision": revision,
                "current": current,
            }))
            .into_response()
        }
        Err(err) => dream_error_response(err, "dream_unavailable"),
    }
}

fn dream_error_response(err: MemoryError, fallback: &str) -> Response {
    let code = match &err {
        MemoryError::NotFound(_) => "dream_revision_not_found",
        _ => fallback,
    };
    let status = match code {
        "dream_revision_not_found" => StatusCode::NOT_FOUND,
        "dream_unavailable" => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(json!({ "code": code, "error": err.to_string() })),
    )
        .into_response()
}

/// POST /lingxi/v1/memories/dream/revisions/{id}/restore?agentId= (:120-136).
pub async fn dream_revision_restore_route(
    State(state): State<ServiceState>,
    AxumPath(revision_id): AxumPath<String>,
    RawQuery(query): RawQuery,
) -> Response {
    let agent_id = match agent_id_from_query(query.as_deref()) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    match state
        .memory()
        .dream_store(&agent_id)
        .and_then(|store| store.restore(&revision_id))
    {
        Ok(mut result) => {
            if let Some(obj) = result.as_object_mut() {
                obj.insert("agentId".into(), json!(agent_id));
                obj.insert("ok".into(), json!(true));
            }
            Json(result).into_response()
        }
        Err(err) => dream_error_response(err, "dream_restore_failed"),
    }
}

// ── tenets faces (agents.ts:1111-1188 + :936-988) ───────────────────────────

fn tenet_json(tenet: &lingxi_kernel::memory::Tenet) -> Value {
    am::tenet_to_json(tenet)
}

/// GET /lingxi/v1/agents/{id}/tenets.
pub async fn tenets_list_route(
    State(state): State<ServiceState>,
    AxumPath(agent_id): AxumPath<String>,
) -> Response {
    match state.memory().tenets_store(&agent_id) {
        Ok(store) => Json(json!({
            "tenets": store.list().tenets.iter().map(tenet_json).collect::<Vec<_>>()
        }))
        .into_response(),
        Err(err) => error_response(&err),
    }
}

/// POST /lingxi/v1/agents/{id}/tenets — user direct add (immediately active).
pub async fn tenets_post_route(
    State(state): State<ServiceState>,
    AxumPath(agent_id): AxumPath<String>,
    body: axum::body::Bytes,
) -> Response {
    let payload = match body_json(&body) {
        Ok(value) => value,
        Err(resp) => return resp,
    };
    let Some(content) = payload
        .get("content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|c| !c.is_empty())
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "content must be a non-empty string" })),
        )
            .into_response();
    };
    let priority = payload.get("priority").and_then(Value::as_str);
    let new_id = format!("tenet-{}", am::random_token_suffix());
    match state
        .memory()
        .tenets_store(&agent_id)
        .and_then(|store| store.add_direct(content, priority, new_id))
    {
        Ok((tenet, duplicate)) => Json(json!({
            "ok": true,
            "tenet": tenet_json(&tenet),
            "duplicate": duplicate,
        }))
        .into_response(),
        Err(err) => error_response(&err),
    }
}

/// POST /lingxi/v1/agents/{id}/tenets/{tenetId}/decide — approve/reject.
pub async fn tenets_decide_route(
    State(state): State<ServiceState>,
    AxumPath((agent_id, tenet_id)): AxumPath<(String, String)>,
    body: axum::body::Bytes,
) -> Response {
    let payload = match body_json(&body) {
        Ok(value) => value,
        Err(resp) => return resp,
    };
    let approve = payload
        .get("approve")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    match state
        .memory()
        .tenets_store(&agent_id)
        .and_then(|store| store.decide(&tenet_id, approve))
    {
        Ok(tenet) => Json(json!({ "ok": true, "tenet": tenet_json(&tenet) })).into_response(),
        Err(err) => error_response(&err),
    }
}

/// DELETE /lingxi/v1/agents/{id}/tenets/{tenetId}.
pub async fn tenets_delete_route(
    State(state): State<ServiceState>,
    AxumPath((agent_id, tenet_id)): AxumPath<(String, String)>,
) -> Response {
    match state
        .memory()
        .tenets_store(&agent_id)
        .and_then(|store| store.remove(&tenet_id))
    {
        Ok(true) => Json(json!({ "ok": true })).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "tenet not found" })),
        )
            .into_response(),
        Err(err) => error_response(&err),
    }
}

/// GET /lingxi/v1/agents/{id}/pinned — active contents only (compat shim).
pub async fn pinned_get_route(
    State(state): State<ServiceState>,
    AxumPath(agent_id): AxumPath<String>,
) -> Response {
    match state.memory().tenets_store(&agent_id) {
        Ok(store) => Json(json!({
            "pins": store.active().iter().map(|t| Value::String(t.content.clone())).collect::<Vec<_>>()
        }))
        .into_response(),
        Err(err) => error_response(&err),
    }
}

/// PUT /lingxi/v1/agents/{id}/pinned — full replace.
pub async fn pinned_put_route(
    State(state): State<ServiceState>,
    AxumPath(agent_id): AxumPath<String>,
    body: axum::body::Bytes,
) -> Response {
    let payload = match body_json(&body) {
        Ok(value) => value,
        Err(resp) => return resp,
    };
    let Some(pins) = payload.get("pins").and_then(Value::as_array) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "pins must be an array" })),
        )
            .into_response();
    };
    let pins: Vec<String> = pins
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    let new_id = format!("pin-{}", am::random_token_suffix());
    match state
        .memory()
        .tenets_store(&agent_id)
        .and_then(|store| store.replace_pinned(&pins, new_id))
    {
        Ok(created) => Json(json!({
            "ok": true,
            "pins": state
                .memory()
                .tenets_store(&agent_id)
                .map(|s| s.active().iter().map(|t| Value::String(t.content.clone())).collect::<Vec<_>>())
                .unwrap_or_default(),
            "added": created.iter().map(tenet_json).collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(err) => error_response(&err),
    }
}

// ── experience faces (agents.ts:991-1108) ───────────────────────────────────

fn experience_enabled(flags: &AgentConfigFlags) -> bool {
    flags.experience_enabled.unwrap_or(false)
}

/// GET /lingxi/v1/agents/{id}/experience — paused → 403 (:1000-1002).
pub async fn experience_get_route(
    State(state): State<ServiceState>,
    AxumPath(agent_id): AxumPath<String>,
) -> Response {
    let service = state.memory();
    let flags = match service.read_flags(&agent_id) {
        Ok(flags) => flags,
        Err(err) => return error_response(&err),
    };
    if !experience_enabled(&flags) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "experience is paused" })),
        )
            .into_response();
    }
    let store = match service.experience_store(&agent_id) {
        Ok(store) => store,
        Err(err) => return error_response(&err),
    };
    let (merged, _files) = store.read_merged();
    Json(json!({ "content": merged })).into_response()
}

/// PUT /lingxi/v1/agents/{id}/experience — whole-document sync by `#` headings.
pub async fn experience_put_route(
    State(state): State<ServiceState>,
    AxumPath(agent_id): AxumPath<String>,
    body: axum::body::Bytes,
) -> Response {
    let payload = match body_json(&body) {
        Ok(value) => value,
        Err(resp) => return resp,
    };
    let service = state.memory();
    let flags = match service.read_flags(&agent_id) {
        Ok(flags) => flags,
        Err(err) => return error_response(&err),
    };
    if !experience_enabled(&flags) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "experience is paused" })),
        )
            .into_response();
    }
    let Some(content) = payload.get("content").and_then(Value::as_str) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "content must be a string" })),
        )
            .into_response();
    };
    // Split merged markdown into `# category` sections (agents.ts:1046-1061).
    let mut categories: Vec<(String, String)> = Vec::new();
    let mut current: Option<(String, Vec<String>)> = None;
    for line in content.split('\n') {
        if let Some(title) = line.strip_prefix("# ").map(str::trim) {
            if let Some((name, lines)) = current.take() {
                categories.push((name, lines.join("\n")));
            }
            match lingxi_kernel::memory::normalize_experience_category(title) {
                Some(normalized) => current = Some((normalized, Vec::new())),
                None => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": "invalid experience category" })),
                    )
                        .into_response();
                }
            }
        } else if let Some((_, lines)) = current.as_mut() {
            lines.push(line.to_string());
        }
    }
    if let Some((name, lines)) = current.take() {
        categories.push((name, lines.join("\n")));
    }
    match service
        .experience_store(&agent_id)
        .and_then(|store| store.sync_categories(&categories))
    {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(err) => error_response(&err),
    }
}

/// POST /lingxi/v1/agents/{id}/experience/feedback (:1065-1108): rating
/// up/down, excerpt required + truncated to 200; not gated by
/// experience.enabled.
pub async fn experience_feedback_route(
    State(state): State<ServiceState>,
    AxumPath(agent_id): AxumPath<String>,
    body: axum::body::Bytes,
) -> Response {
    let payload = match body_json(&body) {
        Ok(value) => value,
        Err(resp) => return resp,
    };
    let rating = payload.get("rating").and_then(Value::as_str);
    let Some(rating) = rating.filter(|r| *r == "up" || *r == "down") else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "rating must be 'up' or 'down'" })),
        )
            .into_response();
    };
    let excerpt: String = payload
        .get("excerpt")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .chars()
        .take(200)
        .collect();
    if excerpt.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "excerpt is required" })),
        )
            .into_response();
    }
    let session_id: String = payload
        .get("sessionId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .chars()
        .take(128)
        .collect();
    let service = state.memory();
    let flags = service.read_flags(&agent_id).unwrap_or_default();
    let date = &am::now_iso()[..10];
    let category = if rating == "up" {
        "用户认可的做法"
    } else {
        "用户不认可的做法"
    };
    let mark = if rating == "up" { "✓" } else { "✗" };
    let session_note = if session_id.is_empty() {
        String::new()
    } else {
        let short: String = session_id.chars().take(8).collect();
        format!("（会话 {short}）")
    };
    let content = format!("[{date}] {mark} {excerpt}{session_note}");
    let store = match service.experience_store(&agent_id) {
        Ok(store) => store,
        Err(err) => return error_response(&err),
    };
    match store.record_entry(category, &content) {
        Ok((added, _)) => Json(json!({
            "ok": true,
            "added": added,
            "duplicate": !added,
            "category": category,
            "experienceEnabled": experience_enabled(&flags),
        }))
        .into_response(),
        Err(err) => error_response(&err),
    }
}

// ── agent config flags (memory/experience/workspace skill policy) ───────────

/// PATCH /lingxi/v1/agents/{id}/config — the T06-owned boolean leaves
/// (`memory.enabled`, `experience.enabled`,
/// `workspace_context.discover_project_skills`,
/// `workspace_context.discover_compatible_project_skills`). The wider agent
/// config face (providers/models/tools) belongs to the R05/R07 config owner
/// and is NOT accepted here (loud 400 on unknown keys).
pub async fn agent_config_patch_route(
    State(state): State<ServiceState>,
    AxumPath(agent_id): AxumPath<String>,
    body: axum::body::Bytes,
) -> Response {
    let payload = match body_json(&body) {
        Ok(value) => value,
        Err(resp) => return resp,
    };
    let mut flags = match state.memory().read_flags(&agent_id) {
        Ok(flags) => flags,
        Err(err) => return error_response(&err),
    };
    let mut known = false;
    if let Some(v) = payload
        .get("memory")
        .and_then(|m| m.get("enabled"))
        .and_then(Value::as_bool)
    {
        flags.memory_enabled = Some(v);
        known = true;
    }
    if let Some(v) = payload
        .get("experience")
        .and_then(|m| m.get("enabled"))
        .and_then(Value::as_bool)
    {
        flags.experience_enabled = Some(v);
        known = true;
    }
    if let Some(v) = payload
        .get("workspace_context")
        .and_then(|m| m.get("discover_project_skills"))
        .and_then(Value::as_bool)
    {
        flags.discover_project_skills = Some(v);
        known = true;
    }
    if let Some(v) = payload
        .get("workspace_context")
        .and_then(|m| m.get("discover_compatible_project_skills"))
        .and_then(Value::as_bool)
    {
        flags.discover_compatible_project_skills = Some(v);
        known = true;
    }
    if !known {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "no T06-owned config key in patch (memory.enabled / experience.enabled / workspace_context.*)"})),
        )
            .into_response();
    }
    match state.memory().patch_flags(&agent_id, flags) {
        Ok(_text) => Json(json!({ "ok": true })).into_response(),
        Err(err) => error_response(&err),
    }
}

/// GET /lingxi/v1/agents/{id}/config-flags — read side of the T06-owned keys.
pub async fn agent_config_flags_route(
    State(state): State<ServiceState>,
    AxumPath(agent_id): AxumPath<String>,
) -> Response {
    match state.memory().read_flags(&agent_id) {
        Ok(flags) => Json(json!({
            "memoryEnabled": flags.memory_enabled.unwrap_or(true),
            "experienceEnabled": flags.experience_enabled.unwrap_or(false),
            "workspaceContext": {
                "discoverProjectSkills": flags.discover_project_skills.unwrap_or(true),
                "discoverCompatibleProjectSkills": flags.discover_compatible_project_skills.unwrap_or(false),
            },
        }))
        .into_response(),
        Err(err) => error_response(&err),
    }
}

// ── context notes (session-scoped; the tool's storage face) ─────────────────

pub async fn context_notes_get_route(
    State(state): State<ServiceState>,
    AxumPath(session_id): AxumPath<String>,
) -> Response {
    match state.memory().context_notes().read(&session_id) {
        Ok(notes) => Json(json!({ "notes": notes })).into_response(),
        Err(err) => error_response(&err),
    }
}

pub async fn context_notes_put_route(
    State(state): State<ServiceState>,
    AxumPath(session_id): AxumPath<String>,
    body: axum::body::Bytes,
) -> Response {
    let payload = match body_json(&body) {
        Ok(value) => value,
        Err(resp) => return resp,
    };
    let Some(text) = payload.get("notes").and_then(Value::as_str) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "notes must be a string" })),
        )
            .into_response();
    };
    match state.memory().context_notes().write(&session_id, text) {
        Ok(bytes) => Json(json!({ "ok": true, "bytes": bytes })).into_response(),
        Err(err) => error_response(&err),
    }
}

/// Re-exported for the memory tools' executor (single implementation, two
/// entries: HTTP + R04 tool).
pub use lingxi_kernel::memory as kernel_memory;

#[cfg(test)]
mod tests {
    use super::urldecode;

    /// R06-T06 leak regression: any literal byte must terminate (the old
    /// catch-all arm never advanced and spun forever, growing the output
    /// String until the process exhausted machine memory).
    #[test]
    fn urldecode_terminates_on_literal_bytes() {
        assert_eq!(urldecode("agentA"), "agentA");
        assert_eq!(urldecode("ghost"), "ghost");
        assert_eq!(urldecode("a1-2_3"), "a1-2_3");
        assert_eq!(urldecode(""), "");
    }

    #[test]
    fn urldecode_decodes_percent_escapes_and_plus() {
        assert_eq!(urldecode("..%2FagentA"), "../agentA");
        assert_eq!(urldecode("%5Cwin"), "\\win");
        assert_eq!(urldecode("a+b"), "a b");
        assert_eq!(urldecode("%e4%bd%a0"), "你");
        assert_eq!(urldecode("%E4%BD%A0"), "你");
    }

    #[test]
    fn urldecode_keeps_malformed_escapes_literal() {
        assert_eq!(urldecode("%zz"), "%zz");
        assert_eq!(urldecode("100%"), "100%");
        assert_eq!(urldecode("%2"), "%2");
        assert_eq!(urldecode("a%"), "a%");
    }
}
