//! 设置页供应商协议适配；全部配置变更与探测均进入唯一发布者。

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Extension, Json, Router,
};
use lingxi_kernel::model_exchange::{AuxiliarySlot, ModelOperation};
use serde_json::{json, Map, Value};

use crate::{
    model_configuration::{
        ModelSelection, ModelSettingsChange, ProviderProbeDraft, ProviderProbeKind,
        ProviderProbeRequest,
    },
    provider_probe, Principal, ServiceState,
};

pub struct ExtractedProviderChanges {
    pub changes: Vec<ModelSettingsChange>,
    pub agent_patch: Value,
}

fn clean_string(value: Option<&Value>, field: &str) -> Result<Option<String>, String> {
    match value {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(String::new())),
        Some(Value::String(value)) => Ok(Some(value.trim().to_string())),
        _ => Err(format!("{field} must be a string or null")),
    }
}

fn selection(
    value: &Value,
    fallback_provider: Option<&str>,
) -> Result<Option<ModelSelection>, String> {
    if value.is_null() || value.as_str().is_some_and(|v| v.trim().is_empty()) {
        return Ok(None);
    }
    let (provider, model) = if let Some(text) = value.as_str() {
        let text = text.trim();
        text.split_once('/')
            .map(|(p, m)| (p.to_owned(), m.to_owned()))
            .unwrap_or_else(|| {
                (
                    fallback_provider.unwrap_or_default().to_owned(),
                    text.to_owned(),
                )
            })
    } else if let Some(object) = value.as_object() {
        let provider = clean_string(object.get("provider"), "model.provider")?
            .unwrap_or_else(|| fallback_provider.unwrap_or_default().into());
        let id = clean_string(object.get("id").or_else(|| object.get("model")), "model.id")?
            .unwrap_or_default();
        (provider, id)
    } else {
        return Err("model selection must be {id,provider}, provider/id or null".into());
    };
    if provider.trim().is_empty() || model.trim().is_empty() {
        return Err("model selection requires provider and id".into());
    }
    Ok(Some(ModelSelection {
        provider: provider.trim().into(),
        model: model.trim().into(),
    }))
}

/// 保留秘密补丁原文和字段存在性，掩码只由唯一发布者针对当前Source解析。
pub fn extract_legacy_provider_changes(
    partial: &Value,
    focus_config: &Value,
) -> Result<ExtractedProviderChanges, String> {
    let object = partial
        .as_object()
        .ok_or_else(|| "configuration patch must be an object".to_string())?;
    let mut agent_patch = partial.clone();
    let mut changes = Vec::new();
    if let Some(providers) = object.get("providers") {
        let providers = providers
            .as_object()
            .ok_or_else(|| "providers must be an object".to_string())?;
        for (provider, patch) in providers {
            if provider.trim().is_empty() || provider.trim() != provider {
                return Err("provider id must be non-empty and trimmed".into());
            }
            if patch.is_null() {
                changes.push(ModelSettingsChange::ProviderRemove {
                    provider: provider.clone(),
                });
            } else if patch.is_object() {
                changes.push(ModelSettingsChange::ProviderPatch {
                    provider: provider.clone(),
                    patch: patch.clone(),
                });
            } else {
                return Err(format!("providers.{provider} must be an object or null"));
            }
        }
        agent_patch
            .as_object_mut()
            .expect("已确认对象")
            .remove("providers");
    }
    for field in ["api", "embedding_api"] {
        let Some(block) = object.get(field) else {
            continue;
        };
        if block.is_null() {
            continue;
        }
        let block = block
            .as_object()
            .ok_or_else(|| format!("{field} must be an object"))?;
        let has_update = ["api_key", "base_url", "headers"]
            .iter()
            .any(|key| block.contains_key(*key));
        if !has_update {
            continue;
        }
        let fallback = focus_config
            .get(field)
            .and_then(|b| b.get("provider"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let provider = clean_string(block.get("provider"), &format!("{field}.provider"))?
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| fallback.trim().into());
        if provider.is_empty() {
            return Err(format!(
                "{field}.provider is required when saving provider settings"
            ));
        }
        let mut patch = Map::new();
        for key in ["api_key", "base_url", "headers", "api"] {
            if let Some(value) = block.get(key) {
                patch.insert(key.into(), value.clone());
            }
        }
        changes.push(ModelSettingsChange::ProviderPatch {
            provider,
            patch: Value::Object(patch),
        });
        let cleaned = agent_patch
            .get_mut(field)
            .and_then(Value::as_object_mut)
            .expect("已确认内联对象");
        cleaned.insert("api_key".into(), json!(""));
        cleaned.insert("base_url".into(), json!(""));
        // null能覆盖旧YAML对象，空对象在深合并时不能删除原秘密头。
        if block.contains_key("headers")
            || focus_config
                .get(field)
                .is_some_and(|b| b.get("headers").is_some())
        {
            cleaned.insert("headers".into(), Value::Null);
        }
        // api/provider只是旧兼容描述，不是模型选择；不据此重绑chat或embedding。
    }
    if let Some(models) = object.get("models") {
        let models = models
            .as_object()
            .ok_or_else(|| "models must be an object".to_string())?;
        for (field, operation) in [
            ("title", ModelOperation::Auxiliary(AuxiliarySlot::Title)),
            (
                "summarize",
                ModelOperation::Auxiliary(AuxiliarySlot::Summarize),
            ),
            ("memory", ModelOperation::Auxiliary(AuxiliarySlot::Memory)),
            (
                "knowledge",
                ModelOperation::Auxiliary(AuxiliarySlot::Knowledge),
            ),
            ("vision", ModelOperation::Auxiliary(AuxiliarySlot::Vision)),
            (
                "approval",
                ModelOperation::Auxiliary(AuxiliarySlot::Approval),
            ),
            ("guard", ModelOperation::Auxiliary(AuxiliarySlot::Guard)),
        ] {
            if let Some(value) = models.get(field) {
                changes.push(ModelSettingsChange::SetBinding {
                    operation,
                    selection: selection(value, None)?,
                });
            }
        }
    }
    Ok(ExtractedProviderChanges {
        changes,
        agent_patch,
    })
}

fn bad_request(message: impl Into<String>) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error":message.into()})),
    )
        .into_response()
}
// 沿用路由辅助函数直接携带已构造的拒绝响应，避免再包装或重建错误内容。
#[allow(clippy::result_large_err)]
fn json_object(body: &Bytes) -> Result<Value, Response> {
    let value: Value =
        serde_json::from_slice(body).map_err(|_| bad_request("invalid JSON body"))?;
    if !value.is_object() {
        return Err(bad_request("body must be an object"));
    }
    Ok(value)
}
#[allow(clippy::result_large_err)] // 与上方解析入口保持相同的标准Axum拒绝响应类型。
fn require_scope(principal: &Principal, scope: &str) -> Result<(), Response> {
    if crate::agents_admin::agent_config_readback::scope_denied(Some(principal), scope) {
        Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error":"insufficient_scope","scope":scope})),
        )
            .into_response())
    } else {
        Ok(())
    }
}

fn probe_request(body: &Value, kind: ProviderProbeKind) -> Result<ProviderProbeRequest, String> {
    let provider = clean_string(body.get("name"), "name")?.filter(|p| !p.is_empty());
    let mut draft = ProviderProbeDraft {
        endpoint: clean_string(body.get("base_url"), "base_url")?,
        protocol: clean_string(body.get("api"), "api")?,
        api_key: clean_string(body.get("api_key"), "api_key")?,
        headers: body.get("headers").cloned(),
    };
    if kind == ProviderProbeKind::Connection {
        draft.api_key = draft.api_key.map(|key| {
            key.chars()
                .filter(|ch| ch.is_ascii() && !ch.is_ascii_control())
                .collect::<String>()
                .trim()
                .into()
        });
    }
    let model = clean_string(body.get("model_id"), "model_id")?.filter(|m| !m.is_empty());
    let has_draft = ["base_url", "api", "api_key", "headers"]
        .iter()
        .any(|key| body.get(*key).is_some());
    Ok(ProviderProbeRequest {
        kind,
        provider,
        model,
        draft: has_draft.then_some(draft),
    })
}

pub async fn fetch_models_route(
    Extension(principal): Extension<Principal>,
    State(state): State<ServiceState>,
    body: Bytes,
) -> Response {
    let body = match json_object(&body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = require_scope(&principal, "providers.manage") {
        return e;
    }
    let request = match probe_request(&body, ProviderProbeKind::Catalog) {
        Ok(v) => v,
        Err(e) => return bad_request(e),
    };
    if request.provider.is_none()
        && request
            .draft
            .as_ref()
            .and_then(|d| d.endpoint.as_deref())
            .is_none_or(str::is_empty)
    {
        return bad_request("name or base_url is required");
    }
    match state.model_configuration().probe(&principal, request).await {
        Ok(report) => Json(report.value).into_response(),
        Err(error) => error.into_response(),
    }
}

pub async fn test_provider_route(
    Extension(principal): Extension<Principal>,
    State(state): State<ServiceState>,
    body: Bytes,
) -> Response {
    let body = match json_object(&body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = require_scope(&principal, "providers.manage") {
        return e;
    }
    let request = match probe_request(&body, ProviderProbeKind::Connection) {
        Ok(v) => v,
        Err(e) => return bad_request(e),
    };
    match state.model_configuration().probe(&principal, request).await {
        Ok(report) => Json(report.value).into_response(),
        Err(error) => error.into_response(),
    }
}

pub async fn model_health_route(
    Extension(principal): Extension<Principal>,
    State(state): State<ServiceState>,
    body: Bytes,
) -> Response {
    let body = match json_object(&body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let explicit_provider = match clean_string(body.get("provider"), "provider") {
        Ok(v) => v,
        Err(e) => return bad_request(e),
    };
    let raw = body
        .get("model")
        .or_else(|| body.get("modelId"))
        .unwrap_or(&Value::Null);
    let selected = match selection(raw, explicit_provider.as_deref()) {
        Ok(Some(v)) => v,
        Ok(None) => return bad_request("modelId required"),
        Err(e) => return bad_request(e),
    };
    if explicit_provider
        .as_ref()
        .is_some_and(|p| !p.is_empty() && p != &selected.provider)
    {
        return bad_request("provider mismatch");
    }
    match state
        .model_configuration()
        .probe(
            &principal,
            ProviderProbeRequest {
                kind: ProviderProbeKind::ModelHealth,
                provider: Some(selected.provider),
                model: Some(selected.model),
                draft: None,
            },
        )
        .await
    {
        Ok(report) => Json(report.value).into_response(),
        Err(error) => error.into_response(),
    }
}

pub async fn saved_api_key_route(
    Extension(principal): Extension<Principal>,
    State(state): State<ServiceState>,
    Path(provider): Path<String>,
) -> Response {
    for scope in ["providers.manage", "secrets.write"] {
        if let Err(e) = require_scope(&principal, scope) {
            return e;
        }
    }
    match state
        .model_configuration()
        .saved_api_key(&principal, &provider)
        .await
    {
        Ok(key) => Json(json!({"api_key":key})).into_response(),
        Err(error) => error.into_response(),
    }
}

pub async fn update_model_route(
    Extension(principal): Extension<Principal>,
    State(state): State<ServiceState>,
    Path((provider, model)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    if let Err(e) = require_scope(&principal, "providers.manage") {
        return e;
    }
    let body = match json_object(&body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let patch = match provider_probe::normalize_model_patch(&body) {
        Ok(v) => v,
        Err(e) => return bad_request(e),
    };
    match state
        .model_configuration()
        .save(
            &principal,
            &[ModelSettingsChange::ModelPatch {
                provider,
                model,
                patch,
            }],
        )
        .await
    {
        Ok(report) => {
            state
                .app_events()
                .publish("models-changed", json!({"agentId":null}));
            Json(json!({"ok":true,"result":report})).into_response()
        }
        Err(error) => {
            if error.report.applied {
                state
                    .app_events()
                    .publish("models-changed", json!({"agentId":null}));
            }
            error.into_response()
        }
    }
}

pub async fn delete_model_route(
    Extension(principal): Extension<Principal>,
    State(state): State<ServiceState>,
    Path((provider, model)): Path<(String, String)>,
) -> Response {
    if let Err(e) = require_scope(&principal, "providers.manage") {
        return e;
    }
    match state
        .model_configuration()
        .save(
            &principal,
            &[ModelSettingsChange::ModelRemove { provider, model }],
        )
        .await
    {
        Ok(report) => {
            state
                .app_events()
                .publish("models-changed", json!({"agentId":null}));
            Json(json!({"ok":true,"result":report})).into_response()
        }
        Err(error) => {
            if error.report.applied {
                state
                    .app_events()
                    .publish("models-changed", json!({"agentId":null}));
            }
            error.into_response()
        }
    }
}

pub async fn discovered_models_route(
    Extension(principal): Extension<Principal>,
    State(state): State<ServiceState>,
    Path(provider): Path<String>,
) -> Response {
    if let Err(e) = require_scope(&principal, "providers.manage") {
        return e;
    }
    let path = state.memory().home().join("models-cache.json");
    let cache = match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(v) if v.is_object() => v,
            _ => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error":"model discovery cache is invalid"})),
                )
                    .into_response()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":"model discovery cache is unreadable"})),
            )
                .into_response()
        }
    };
    let definition = provider_probe::builtin_provider_definition(&provider);
    let runtime_id = definition
        .as_ref()
        .and_then(|d| d["runtimeProviderId"].as_str())
        .unwrap_or(&provider);
    let entry = cache
        .get(&provider)
        .or_else(|| cache.get(runtime_id))
        .unwrap_or(&Value::Null);
    let models = entry["models"].as_array().cloned().unwrap_or_default();
    let filtered = provider_probe::filter_discovered_models(&provider, "", models);
    let mut value = json!({"models":filtered.models,"fetchedAt":entry.get("fetchedAt").cloned().unwrap_or(Value::Null)});
    if !filtered.ignored_models.is_empty() {
        value["ignoredModels"] = json!(filtered.ignored_models);
    }
    Json(value).into_response()
}

fn masked(value: Option<&Value>) -> Value {
    json!(
        if value.and_then(Value::as_str).is_some_and(|s| !s.is_empty()) {
            "********"
        } else {
            ""
        }
    )
}

fn media_capability_bindings() -> Result<Map<String, Value>, String> {
    let mut result = Map::new();
    for capability in [
        "imageGeneration",
        "videoGeneration",
        "speechRecognition",
        "speechGeneration",
    ] {
        for (runtime, _, block) in
            crate::operations::media_declaration_catalog(capability).map_err(|e| e.to_string())?
        {
            let lanes = block.get("credentialLanes").and_then(Value::as_array);
            let default_lane = json!([{ "id":runtime,"providerId":runtime }]);
            let lanes = lanes
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| default_lane.as_array().expect("固定列表"));
            for lane in lanes {
                let provider = lane
                    .get("providerId")
                    .and_then(Value::as_str)
                    .unwrap_or(&runtime);
                let lane_id = lane.get("id").and_then(Value::as_str).unwrap_or(provider);
                let mut binding = json!({"capability":capability,"runtime_provider_id":runtime});
                if lane_id != runtime || provider != runtime {
                    binding["credential_lane_id"] = json!(lane_id);
                }
                let rows = result
                    .entry(provider.to_owned())
                    .or_insert_with(|| json!([]))
                    .as_array_mut()
                    .expect("内部列表");
                if !rows.contains(&binding) {
                    rows.push(binding);
                }
            }
        }
    }
    // 原语音声明未进入七家图像/视频生成物；保留完整能力展示，执行可用性仍由Operation判定。
    for (provider, capability) in [
        ("system-speech", "speechGeneration"),
        ("system-speech", "speechRecognition"),
        ("volcengine-speech", "speechRecognition"),
        ("groq", "speechGeneration"),
        ("siliconflow", "speechGeneration"),
        ("mimo", "speechRecognition"),
        ("mimo-token-plan", "speechRecognition"),
    ] {
        let rows = result
            .entry(provider.to_owned())
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .expect("内部列表");
        let binding = json!({"capability":capability,"runtime_provider_id":provider});
        if !rows.contains(&binding) {
            rows.push(binding);
        }
    }
    Ok(result)
}

/// 兼容旧summary的显示字段；发布者快照即使将来增加字段，也不透传秘密。
pub fn summary_payload(snapshot: &Value) -> Result<Value, String> {
    let media_bindings = media_capability_bindings()?;
    let mut providers = Map::new();
    let configured = snapshot.get("providers").and_then(Value::as_object);
    let mut definitions = provider_probe::builtin_provider_definitions();
    if let Some(configured) = configured {
        for id in configured.keys() {
            if !definitions
                .iter()
                .any(|d| d["id"] == *id || d["runtimeProviderId"] == *id)
            {
                definitions.push(json!({"id":id,"displayName":id,"authType":"api-key"}));
            }
        }
    }
    for definition in definitions {
        let id = definition["id"].as_str().unwrap_or_default();
        let runtime_id = definition["runtimeProviderId"].as_str().unwrap_or(id);
        let row = configured.and_then(|p| p.get(id).or_else(|| p.get(runtime_id)));
        // OAuth身份取插件声明；凭证是否登录另由发布者的实时状态确认。
        let auth = if definition["authType"] == "oauth" {
            "oauth"
        } else {
            row.and_then(|r| r.get("auth_type"))
                .and_then(Value::as_str)
                .or_else(|| definition["authType"].as_str())
                .unwrap_or("api-key")
        };
        let oauth = auth == "oauth";
        let base = row
            .and_then(|r| r.get("base_url").or_else(|| r.get("endpoint")))
            .and_then(Value::as_str)
            .or_else(|| definition["baseUrl"].as_str())
            .unwrap_or_default();
        let api = row
            .and_then(|r| r.get("api").or_else(|| r.get("protocol")))
            .and_then(Value::as_str)
            .or_else(|| definition["api"].as_str())
            .unwrap_or_default();
        let mut headers = Map::new();
        if let Some(raw) = row
            .and_then(|r| r.get("headers"))
            .and_then(Value::as_object)
        {
            for (name, value) in raw {
                headers.insert(name.clone(), masked(Some(value)));
            }
        }
        let logged_in = row
            .and_then(|r| r.get("logged_in"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let has_credentials = row
            .and_then(|r| r.get("has_credentials"))
            .and_then(Value::as_bool)
            .unwrap_or_else(|| {
                oauth && logged_in
                    || auth == "none"
                    || row
                        .and_then(|r| r.get("api_key"))
                        .and_then(Value::as_str)
                        .is_some_and(|s| !s.is_empty())
                    || !headers.is_empty()
            });
        let models = row
            .and_then(|r| r.get("models"))
            .cloned()
            .unwrap_or_else(|| {
                if oauth {
                    definition["models"].clone()
                } else {
                    json!([])
                }
            });
        let error = row
            .and_then(|r| r.get("config_error"))
            .cloned()
            .unwrap_or(Value::Null);
        let mut missing = Vec::new();
        if row.is_none() && !oauth {
            if auth != "none" {
                missing.push("api_key");
            }
            missing.push("models");
        } else if oauth {
            if models.as_array().is_none_or(Vec::is_empty) {
                missing.push("models");
            }
        } else {
            if base.is_empty() && id != "system-speech" {
                missing.push("base_url");
            }
            if !has_credentials {
                missing.push("api_key");
            }
        }
        let status = if !error.is_null() {
            "invalid"
        } else if !missing.is_empty() || oauth && !logged_in {
            "needs_setup"
        } else {
            "ok"
        };
        let mut value = json!({"type":if oauth{"oauth"}else{"api-key"},"auth_type":auth,"display_name":definition["displayName"],"base_url":base,"api":api,"api_key":masked(row.and_then(|r|r.get("api_key"))),"headers":headers,"models":models,"custom_models":[],"has_credentials":has_credentials,"supports_oauth":oauth,"is_coding_plan":id.ends_with("-coding"),"is_configured":row.is_some()||oauth,"can_delete":row.is_some(),"config_status":status,"config_error":error,"missing_fields":missing,"media_capability_bindings":media_bindings.get(id).cloned().unwrap_or_else(||json!([]))});
        if oauth {
            value["logged_in"] = json!(logged_in);
        }
        providers.insert(id.into(), value);
    }
    Ok(json!({"providers":providers}))
}

pub async fn summary_route(
    Extension(principal): Extension<Principal>,
    State(state): State<ServiceState>,
) -> Response {
    if let Err(e) = require_scope(&principal, "settings.read") {
        return e;
    }
    match state.model_configuration().snapshot(&principal).await {
        Ok(snapshot) => match summary_payload(&snapshot) {
            Ok(value) => Json(value).into_response(),
            Err(error) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":error})),
            )
                .into_response(),
        },
        Err(error) => error.into_response(),
    }
}

pub async fn auxiliary_vision_route(
    Extension(principal): Extension<Principal>,
    State(state): State<ServiceState>,
) -> Response {
    if let Err(error) = require_scope(&principal, "chat") {
        return error;
    }
    let preferences = match state.memory().read_preferences_root() {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":"preferences unavailable"})),
            )
                .into_response()
        }
    };
    let enabled = preferences
        .get("vision_auxiliary_enabled")
        .and_then(Value::as_bool)
        .or_else(|| {
            preferences
                .pointer("/models/vision_enabled")
                .and_then(Value::as_bool)
        })
        .unwrap_or(false);
    let raw = state
        .model_gateway()
        .and_then(|gateway| {
            gateway
                .configured_routes()
                .get("vision")
                .map(|binding| json!({"id":binding.model,"provider":binding.provider}))
        })
        // 已激活的统一来源明确清除绑定后，不能让旧偏好把模型复活。
        .or_else(|| {
            (!state.model_configuration().is_active())
                .then(|| preferences.get("vision_model").cloned())
                .flatten()
        })
        .unwrap_or(Value::Null);
    let model = selection(&raw, None).ok().flatten();
    let configured = model.is_some();
    let resolved = model.as_ref().and_then(|m| {
        state.model_gateway().and_then(|g| {
            g.resolve_dispatch(&lingxi_kernel::model_exchange::ModelRouteRequest {
                operation: ModelOperation::Auxiliary(AuxiliarySlot::Vision),
                provider: Some(m.provider.clone()),
                model: Some(m.model.clone()),
            })
            .ok()
        })
    });
    let image_supported = resolved.as_ref().and_then(|d| d.capabilities.image_input) == Some(true);
    let reason = if !enabled {
        Some("disabled")
    } else if !configured {
        Some("not_configured")
    } else if resolved.is_none() {
        Some("model_not_found")
    } else if !image_supported {
        Some("model_without_image_input")
    } else {
        None
    };
    Json(json!({"auxiliaryVision":{"enabled":enabled,"configured":configured,"available":reason.is_none(),"unavailableReason":reason,"model":model.map(|m|json!({"id":m.model,"provider":m.provider}))}})).into_response()
}

/// summary已存在，由组合根替换旧handler；此处只汇入此前缺少的路径。
pub fn router() -> Router<ServiceState> {
    Router::new()
        .route(
            "/lingxi/v1/providers/fetch-models",
            post(fetch_models_route),
        )
        .route("/lingxi/v1/providers/test", post(test_provider_route))
        .route(
            "/lingxi/v1/providers/{provider}/api-key",
            get(saved_api_key_route),
        )
        .route(
            "/lingxi/v1/providers/{provider}/discovered-models",
            get(discovered_models_route),
        )
        .route(
            "/lingxi/v1/providers/{provider}/models/{model}",
            put(update_model_route).delete(delete_model_route),
        )
        .route("/lingxi/v1/models/health", post(model_health_route))
        .route(
            "/lingxi/v1/models/auxiliary-vision",
            get(auxiliary_vision_route),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_preserves_masks_null_and_does_not_promote_agent_chat_selection() {
        let input = json!({"providers":{"p":{"api_key":"********","headers":null},"gone":null},"api":{"provider":"p","api_key":"********","headers":null},"models":{"chat":{"id":"agent-only","provider":"p"},"knowledge":{"id":"k","provider":"p"}},"custom":{"kept":true}});
        let result = extract_legacy_provider_changes(&input, &json!({})).unwrap();
        assert!(result.agent_patch.get("providers").is_none());
        assert_eq!(result.agent_patch["api"]["api_key"], "");
        assert_eq!(result.agent_patch["api"]["headers"], Value::Null);
        assert_eq!(result.agent_patch["models"]["chat"]["id"], "agent-only");
        assert_eq!(result.agent_patch["custom"], input["custom"]);
        assert_eq!(result.changes.len(), 4);
        assert!(result.changes.iter().any(|c|matches!(c,ModelSettingsChange::ProviderPatch{provider,patch} if provider=="p" && patch["api_key"]=="********" && patch.get("headers")==Some(&Value::Null))));
        assert!(!result.changes.iter().any(|c| matches!(
            c,
            ModelSettingsChange::SetBinding {
                operation: ModelOperation::Chat,
                ..
            }
        )));
    }

    #[test]
    fn inline_provider_fallback_uses_only_explicit_target_config() {
        assert!(
            extract_legacy_provider_changes(&json!({"api":{"api_key":"draft"}}), &json!({}))
                .is_err()
        );
        let result = extract_legacy_provider_changes(
            &json!({"api":{"api_key":"draft"}}),
            &json!({"api":{"provider":"target","headers":{"X-Secret":"old"}}}),
        )
        .unwrap();
        assert!(
            matches!(&result.changes[0],ModelSettingsChange::ProviderPatch{provider,..} if provider=="target")
        );
        assert_eq!(result.agent_patch["api"]["headers"], Value::Null);
        assert!(extract_legacy_provider_changes(&json!({"providers":[]} ), &json!({})).is_err());
    }

    #[test]
    fn request_headers_preserve_absence_clear_and_masked_values() {
        for (body, expected) in [
            (json!({"name":"p"}), None),
            (json!({"name":"p","headers":null}), Some(Value::Null)),
            (json!({"name":"p","headers":{}}), Some(json!({}))),
            (
                json!({"name":"p","headers":{"Authorization":"********"}}),
                Some(json!({"Authorization":"********"})),
            ),
        ] {
            let req = probe_request(&body, ProviderProbeKind::Catalog).unwrap();
            assert_eq!(req.draft.and_then(|d| d.headers), expected);
        }
    }
    #[test]
    fn legacy_wire_media_bindings_and_test_key_cleaning_are_preserved() {
        let summary = summary_payload(&json!({"providers":{}})).unwrap();
        let lanes = summary["providers"]["volcengine-coding"]["media_capability_bindings"]
            .as_array()
            .unwrap();
        assert!(lanes
            .iter()
            .any(|b| b["runtime_provider_id"] == "volcengine"
                && b["credential_lane_id"] == "volcengine-coding"));
        let local = summary["providers"]["system-speech"]["media_capability_bindings"]
            .as_array()
            .unwrap();
        assert_eq!(local.len(), 2);
        assert!(local.iter().all(|b| b.get("runtimeProviderId").is_none()));
        let draft = probe_request(
            &json!({"api_key":" a中b\tc\nd\u{007f} "}),
            ProviderProbeKind::Connection,
        )
        .unwrap()
        .draft
        .unwrap();
        assert_eq!(draft.api_key.as_deref(), Some("abcd"));
        let catalog = probe_request(&json!({"api_key":" a中b "}), ProviderProbeKind::Catalog)
            .unwrap()
            .draft
            .unwrap();
        assert_eq!(catalog.api_key.as_deref(), Some("a中b"));
    }
}
