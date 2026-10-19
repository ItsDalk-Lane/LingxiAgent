//! 全局设置沿已有偏好写锁合并；模型变更只进入统一发布入口。
use crate::model_configuration::{ModelApplyError, ModelSelection, ModelSettingsChange};

/// 显式用户代理偏好覆盖同一网络平面的代理部分；证书信任仍取原模型配置。
pub(crate) fn effective_network_policy(
    base: &lingxi_adapters::models::network::NetworkConfigSection,
    prefs: &Value,
) -> Result<lingxi_adapters::models::network::NetworkPolicy, String> {
    let mut network = base.clone();
    if let Some(proxy) = prefs.get("network_proxy") {
        let fields = proxy.as_object().ok_or("network_proxy must be object")?;
        let clean_url = |field: &str| -> Result<Option<String>, String> {
            match fields.get(field) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(raw)) if raw.is_empty() => Ok(None),
                Some(Value::String(raw)) => {
                    let parsed = reqwest::Url::parse(raw)
                        .map_err(|_| format!("invalid {field} proxy URL"))?;
                    if !parsed.username().is_empty() || parsed.password().is_some() {
                        return Err(format!("{field} proxy URL credentials are not supported by the shared service network"));
                    }
                    Ok(Some(raw.clone()))
                }
                _ => Err(format!("{field} proxy URL must be string")),
            }
        };
        // WebSocket专用地址保留给桌面；不能把它冒充HTTP服务已采用的地址。
        clean_url("wsProxy")?;
        clean_url("wssProxy")?;
        network.proxy = lingxi_adapters::models::network::NetworkProxyConfig {
            mode: fields
                .get("mode")
                .and_then(Value::as_str)
                .unwrap_or("system")
                .into(),
            http_proxy: clean_url("httpProxy")?,
            https_proxy: clean_url("httpsProxy")?,
            no_proxy: fields
                .get("noProxy")
                .map(|value| {
                    value
                        .as_str()
                        .map(String::from)
                        .ok_or("noProxy must be string")
                })
                .transpose()?
                .or_else(|| Some(lingxi_adapters::models::network::DEFAULT_NO_PROXY.into())),
        };
    }
    network
        .validate()
        .map_err(|_| "network proxy or trusted certificate configuration is invalid".to_string())?;
    Ok(lingxi_adapters::models::network::NetworkPolicy::from_config(&network))
}
use crate::{agents_admin::agent_config_readback as config, auth::Principal, ServiceState};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use lingxi_adapters::storage::agent_memory as am;
use lingxi_kernel::model_exchange::{AuxiliarySlot, ModelOperation};
use serde::Deserialize;
use serde_json::{json, Map, Value};

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({"error":message.into()}))).into_response()
}
fn bad(message: impl Into<String>) -> Response {
    error(StatusCode::BAD_REQUEST, message)
}
fn failed(message: impl Into<String>) -> Response {
    error(StatusCode::INTERNAL_SERVER_ERROR, message)
}
fn denied(principal: &Principal, scope: &str) -> Option<Response> {
    (!principal.has_scope(scope)).then(|| {
        (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"insufficient_scope","scope":scope})),
        )
            .into_response()
    })
}
fn check_secrets(principal: &Principal, patch: &Value) -> Option<Response> {
    let mut paths = Vec::new();
    config::collect_secret_patch_paths(patch, "", &mut paths);
    if !paths.is_empty() && !principal.has_scope("secrets.write") {
        Some((StatusCode::FORBIDDEN, Json(json!({"error":"secret_write_scope_required","scope":"secrets.write","fields":paths}))).into_response())
    } else {
        None
    }
}
fn model_error(state: &ServiceState, error: ModelApplyError, preferences_saved: bool) -> Response {
    if error.report.applied {
        state
            .app_events()
            .publish("models-changed", json!({"agentId":null}));
    }
    (StatusCode::from_u16(error.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), Json(json!({"error":error.message,"code":error.code,"result":error.report,"preferencesSaved":preferences_saved}))).into_response()
}
fn validate_global(patch: &Value) -> Result<Vec<(&'static str, Value)>, String> {
    if !patch.is_object() {
        return Err("configuration patch must be object".into());
    }
    let (globals, rest) = config::split_by_scope(patch);
    let unexpected: Vec<_> = rest
        .as_object()
        .expect("已确认对象")
        .keys()
        .filter(|key| key.as_str() != "providers")
        .cloned()
        .collect();
    if !unexpected.is_empty() {
        return Err(format!(
            "{} belong to a specific agent; use PUT /api/agents/{{agentId}}/config",
            unexpected.join(", ")
        ));
    }
    // 保留 T01 活动总开关已有的严格输入规则。
    for (key, value) in &globals {
        if *key == "desk.heartbeat_master" && !value.is_boolean() {
            return Err("heartbeat_master must be boolean".into());
        }
        if *key == "automation.permissionMode"
            && !value
                .as_str()
                .is_some_and(|v| ["auto", "operate", "read_only"].contains(&v))
        {
            return Err("invalid automation permission mode".into());
        }
    }
    Ok(globals)
}
fn apply_globals(
    state: &ServiceState,
    fields: &[(&str, Value)],
    persist: bool,
) -> Result<Value, String> {
    let _lock = crate::agents_admin::preferences_locked();
    let mut prefs = state.memory().read_preferences_root()?;
    for (key, value) in fields {
        config::apply_global_setter(&mut prefs, key, value)?;
    }
    if persist && !fields.is_empty() {
        state.memory().write_preferences_root(&prefs)?;
    }
    Ok(prefs)
}
fn emit_globals(state: &ServiceState, fields: &[(&str, Value)], saved_preferences: &Value) {
    if fields.is_empty() {
        return;
    }
    for (field, _) in fields {
        let event = match *field {
            "locale" => Some("locale-changed"),
            "editor" => Some("editor-typography-changed"),
            "network_proxy" => Some("network-proxy-changed"),
            "keep_awake" => Some("keep-awake-changed"),
            _ => None,
        };
        if let Some(event) = event {
            let mut payload = Map::new();
            payload.insert(
                (*field).into(),
                config::global_field_value(saved_preferences, field),
            );
            state.app_events().publish(event, Value::Object(payload));
        }
    }
    state
        .app_events()
        .publish("config_changed", json!({"global":true}));
}

pub async fn config_get(state: &ServiceState, principal: &Principal) -> Response {
    let prefs = match state.memory().read_preferences_root() {
        Ok(value) => value,
        Err(e) => return failed(e),
    };
    let snapshot = match state.model_configuration().snapshot(principal).await {
        Ok(value) => value,
        Err(e) => return e.into_response(),
    };
    let providers = snapshot.get("providers").cloned().unwrap_or(json!({}));
    let mut value = Map::new();
    config::inject_global_fields(&mut value, &prefs);
    value.insert("providers".into(), json!(providers));
    value.insert(
        "modelConfiguration".into(),
        json!({"activated":snapshot["activated"],"generation":snapshot["generation"]}),
    );
    Json(config::mask_object_secrets(&Value::Object(value))).into_response()
}

fn experiment_changes(patch: &Value) -> Result<Map<String, Value>, String> {
    let requested = patch.as_object().ok_or("experiments must be object")?;
    let definitions = config::experiment_definitions();
    let mut result = Map::new();
    for (id, value) in requested {
        let definition = definitions
            .iter()
            .find(|d| d["id"] == *id && d["scope"] == "global")
            .ok_or_else(|| format!("unknown global experiment {id}"))?;
        if definition
            .pointer("/valueSchema/type")
            .and_then(Value::as_str)
            .unwrap_or("boolean")
            == "boolean"
            && !value.is_boolean()
        {
            return Err(format!("experiment {id} must be boolean"));
        }
        // 已停用实验仍沿现有规则保持 off，导入不能令旧实现复活。
        let normalized = if id == "memory.cache_snapshot_reflection" {
            json!("off")
        } else {
            config::normalize_experiment_value(definition, value)?
        };
        result.insert(id.clone(), normalized);
    }
    Ok(result)
}

pub async fn export_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
) -> Response {
    if !principal.is_local_owner() {
        return error(StatusCode::FORBIDDEN, "local_owner_required");
    }
    let prefs = match state.memory().read_preferences_root() {
        Ok(value) => value,
        Err(e) => return failed(e),
    };
    let mut global = Map::new();
    config::inject_global_fields(&mut global, &prefs);
    let providers = match state.model_configuration().snapshot(&principal).await {
        Ok(value) => value.get("providers").cloned().unwrap_or(json!({})),
        Err(e) => return e.into_response(),
    };
    global.insert("providers".into(), providers);
    let preferences = match crate::settings_ui::model_preferences(&state) {
        Ok(value) => value,
        Err(e) => return failed(e),
    };
    let experiments: Map<String, Value> = config::experiment_definitions()
        .iter()
        .filter(|d| d["scope"] == "global")
        .filter_map(|definition| {
            Some((
                definition["id"].as_str()?.into(),
                config::resolved_experiment_value(&prefs, definition),
            ))
        })
        .collect();
    let export = json!({"format":"lingxi-settings","version":1,"config":global,"modelPreferences":{"models":preferences["models"],"search":preferences["search"]},"experiments":experiments});
    let mut response = Json(config::mask_object_secrets(&export)).into_response();
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        axum::http::header::CONTENT_DISPOSITION,
        axum::http::HeaderValue::from_static("attachment; filename=\"lingxi-settings.json\""),
    );
    response
}

fn apply_import_preferences(
    state: &ServiceState,
    globals: &[(&str, Value)],
    model_preferences: &Value,
    experiments: &Map<String, Value>,
    persist: bool,
) -> Result<Value, String> {
    let _lock = crate::agents_admin::preferences_locked();
    let mut prefs = state.memory().read_preferences_root()?;
    for (key, value) in globals {
        config::apply_global_setter(&mut prefs, key, value)?;
    }
    if let Some(search) = model_preferences.get("search") {
        merge_search(&mut prefs, search)?;
    }
    if let Some(enabled) = model_preferences.pointer("/models/vision_enabled") {
        prefs["vision_auxiliary_enabled"] = enabled.clone();
    }
    if !experiments.is_empty() {
        if prefs.get("experiments").is_some_and(|v| !v.is_object()) {
            return Err("saved experiments must be object".into());
        }
        if prefs.get("experiments").is_none() {
            prefs["experiments"] = json!({});
        }
        for (id, value) in experiments {
            prefs["experiments"][id] = value.clone();
        }
    }
    if persist {
        state.memory().write_preferences_root(&prefs)?;
    }
    Ok(prefs)
}

pub async fn import_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<lingxi_protocol::SettingsArchive>,
) -> Response {
    let body = serde_json::to_value(body).expect("已解析设置交换结构可序列化");
    if !principal.is_local_owner() {
        return error(StatusCode::FORBIDDEN, "local_owner_required");
    }
    let Some(fields) = body.as_object() else {
        return bad("settings import must be object");
    };
    if body["format"] != "lingxi-settings"
        || body["version"] != 1
        || fields.keys().any(|k| {
            ![
                "format",
                "version",
                "config",
                "modelPreferences",
                "experiments",
            ]
            .contains(&k.as_str())
        })
    {
        return bad("unsupported settings import format");
    }
    if let Some(response) = check_secrets(&principal, &body) {
        return response;
    }
    let global = fields.get("config").cloned().unwrap_or(json!({}));
    let globals = match validate_global(&global) {
        Ok(value) => value,
        Err(e) => return bad(e),
    };
    let extracted =
        match crate::provider_routes::extract_legacy_provider_changes(&global, &json!({})) {
            Ok(value) => value,
            Err(e) => return bad(e),
        };
    let models = fields.get("modelPreferences").cloned().unwrap_or(json!({}));
    let Some(model_fields) = models.as_object() else {
        return bad("modelPreferences must be object");
    };
    if model_fields
        .keys()
        .any(|k| !["models", "search"].contains(&k.as_str()))
    {
        return bad("unknown modelPreferences section");
    }
    let mut changes = extracted.changes;
    if let Some(model_patch) = models.get("models") {
        match shared_changes(model_patch) {
            Ok(value) => changes.extend(value),
            Err(e) => return bad(e),
        }
    }
    let experiments = match fields
        .get("experiments")
        .map(experiment_changes)
        .transpose()
    {
        Ok(value) => value.unwrap_or_default(),
        Err(e) => return bad(e),
    };
    if let Err(e) = apply_import_preferences(&state, &globals, &models, &experiments, false) {
        return bad(e);
    }
    let mut report = if changes.is_empty() {
        None
    } else {
        match state.model_configuration().save(&principal, &changes).await {
            Ok(value) => Some(value),
            Err(e) => return model_error(&state, e, false),
        }
    };
    let saved_preferences =
        match apply_import_preferences(&state, &globals, &models, &experiments, true) {
            Ok(value) => value,
            Err(e) => {
                if report.as_ref().is_some_and(|value| value.applied) {
                    state
                        .app_events()
                        .publish("models-changed", json!({"agentId":null}));
                }
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error":e,"preferencesSaved":false,"modelConfiguration":report})),
                )
                    .into_response();
            }
        };
    emit_globals(&state, &globals, &saved_preferences);
    if globals.iter().any(|(key, _)| *key == "network_proxy") {
        match state
            .model_configuration()
            .refresh_network_preferences(&principal)
            .await
        {
            Ok(network_report) => {
                if report.is_none() {
                    report = Some(network_report);
                }
            }
            Err(e) => return model_error(&state, e, true),
        }
    }
    state
        .app_events()
        .publish("models-changed", json!({"agentId":null}));
    Json(json!({"ok":true,"preferencesSaved":true,"modelConfiguration":report,"experimentsApplied":experiments.keys().collect::<Vec<_>>()})).into_response()
}
pub async fn config_put_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Json(patch): Json<Value>,
) -> Response {
    if let Some(response) =
        denied(&principal, "settings.write").or_else(|| check_secrets(&principal, &patch))
    {
        return response;
    }
    let globals = match validate_global(&patch) {
        Ok(value) => value,
        Err(e) => return bad(e),
    };
    if !principal.is_local_owner()
        && globals
            .iter()
            .any(|(key, _)| ["desk.heartbeat_master", "automation.permissionMode"].contains(key))
    {
        return error(StatusCode::FORBIDDEN, "local_owner_required");
    }
    let extracted =
        match crate::provider_routes::extract_legacy_provider_changes(&patch, &json!({})) {
            Ok(value) => value,
            Err(e) => return bad(e),
        };
    if !extracted.changes.is_empty() {
        if let Some(response) = denied(&principal, "providers.manage") {
            return response;
        }
    }
    if let Err(e) = apply_globals(&state, &globals, false) {
        return failed(e);
    }
    let saved_preferences = match apply_globals(&state, &globals, true) {
        Ok(value) => value,
        Err(e) => return failed(e),
    };
    emit_globals(&state, &globals, &saved_preferences);
    let report = if extracted.changes.is_empty() {
        if globals.iter().any(|(key, _)| *key == "network_proxy") {
            match state
                .model_configuration()
                .refresh_network_preferences(&principal)
                .await
            {
                Ok(report) => Some(report),
                Err(e) => return model_error(&state, e, true),
            }
        } else {
            None
        }
    } else {
        match state
            .model_configuration()
            .save(&principal, &extracted.changes)
            .await
        {
            Ok(report) => Some(report),
            Err(e) => return model_error(&state, e, !globals.is_empty()),
        }
    };
    if report.is_some() {
        state
            .app_events()
            .publish("models-changed", json!({"agentId":null}));
    }
    Json(json!({"ok":true,"preferencesSaved":!globals.is_empty(),"modelConfiguration":report}))
        .into_response()
}

fn shared_changes(patch: &Value) -> Result<Vec<ModelSettingsChange>, String> {
    let fields = patch
        .as_object()
        .ok_or("shared models patch must be an object")?;
    let mut changes = Vec::new();
    for (key, value) in fields {
        if key == "vision_enabled" {
            if !value.is_boolean() {
                return Err("shared model vision_enabled must be a boolean".into());
            }
            continue;
        }
        let slot = AuxiliarySlot::from_purpose(key)
            .ok_or_else(|| format!("unknown shared model field {key}"))?;
        let selection = if value.is_null() || value.as_str() == Some("") {
            None
        } else {
            let model = value
                .as_object()
                .ok_or_else(|| format!("shared model {key} must be a model reference"))?;
            if model
                .keys()
                .any(|k| !["id", "provider"].contains(&k.as_str()))
            {
                return Err(format!("shared model {key} has unknown fields"));
            }
            let provider = model
                .get("provider")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .ok_or("model provider required")?;
            let id = model
                .get("id")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .ok_or("model id required")?;
            Some(ModelSelection {
                provider: provider.into(),
                model: id.into(),
            })
        };
        changes.push(ModelSettingsChange::SetBinding {
            operation: ModelOperation::Auxiliary(slot),
            selection,
        });
    }
    Ok(changes)
}
fn merge_search(prefs: &mut Value, patch: &Value) -> Result<(), String> {
    use crate::credentials::search::{self, SearchCredentials};
    let fields = patch.as_object().ok_or("search must be object")?;
    if fields
        .keys()
        .any(|k| !["provider", "api_key", "api_keys"].contains(&k.as_str()))
    {
        return Err("unknown search field".into());
    }
    let current = SearchCredentials::from_preferences(prefs)?;
    let selected = match fields.get("provider") {
        None => current.provider.clone(),
        Some(Value::String(s)) if s.trim().is_empty() => "auto".into(),
        Some(Value::String(s)) if search::known(s.trim()) => s.trim().into(),
        _ => return Err("unknown search provider".into()),
    };
    let mut keys = prefs.get("search_api_keys").cloned().unwrap_or(json!({}));
    let keys_map = keys
        .as_object_mut()
        .ok_or("saved search keys must be object")?;
    for id in search::PAID {
        if let Some(key) = current.key(id) {
            keys_map.insert(id.into(), json!(key));
        }
    }
    let mut updates = Vec::new();
    if let Some(raw) = fields.get("api_keys") {
        let map = raw.as_object().ok_or("search.api_keys must be object")?;
        for (provider, value) in map {
            if !search::PAID.contains(&provider.as_str()) {
                return Err("unknown search key provider".into());
            }
            updates.push((provider.clone(), value));
        }
    }
    if let Some(value) = fields.get("api_key") {
        if search::PAID.contains(&selected.as_str()) {
            updates.push((selected.clone(), value));
        } else if !value.is_null()
            && value
                .as_str()
                .is_none_or(|s| !s.is_empty() && s != "********")
        {
            return Err("selected search provider does not accept api_key".into());
        }
    }
    for (provider, value) in updates {
        if value.is_null() || value.as_str().is_some_and(|s| s.trim().is_empty()) {
            keys_map.remove(&provider);
        } else {
            let raw = value.as_str().ok_or("search key must be string or null")?;
            let key = current.candidate(&provider, raw)?;
            keys_map.insert(provider, json!(key));
        }
    }
    prefs["search_provider"] = json!(selected);
    prefs["search_api_key"] = keys.get(&selected).cloned().unwrap_or(json!(""));
    prefs["search_api_keys"] = keys;
    Ok(())
}
fn apply_model_preferences(
    state: &ServiceState,
    patch: &Value,
    persist: bool,
) -> Result<(), String> {
    let _guard = crate::agents_admin::preferences_locked();
    let mut prefs = state.memory().read_preferences_root()?;
    if let Some(search) = patch.get("search") {
        merge_search(&mut prefs, search)?;
    }
    if let Some(enabled) = patch.pointer("/models/vision_enabled") {
        prefs["vision_auxiliary_enabled"] = enabled.clone();
    }
    if persist {
        state.memory().write_preferences_root(&prefs)?;
    }
    Ok(())
}
pub async fn models_put_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Json(patch): Json<Value>,
) -> Response {
    if let Some(response) =
        denied(&principal, "settings.write").or_else(|| check_secrets(&principal, &patch))
    {
        return response;
    }
    let Some(fields) = patch.as_object() else {
        return bad("models preferences must be object");
    };
    if fields.is_empty()
        || fields
            .keys()
            .any(|k| !["models", "search"].contains(&k.as_str()))
    {
        return bad("unsupported model preference section");
    }
    let changes = match fields.get("models").map(shared_changes).transpose() {
        Ok(value) => value.unwrap_or_default(),
        Err(e) => return bad(e),
    };
    if let Err(e) = apply_model_preferences(&state, &patch, false) {
        return bad(e);
    }
    let report = if changes.is_empty() {
        None
    } else {
        match state.model_configuration().save(&principal, &changes).await {
            Ok(value) => Some(value),
            Err(e) => return model_error(&state, e, false),
        }
    };
    if let Err(e) = apply_model_preferences(&state, &patch, true) {
        if report.as_ref().is_some_and(|value| value.applied) {
            state
                .app_events()
                .publish("models-changed", json!({"agentId":null}));
        }
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":e,"preferencesSaved":false,"modelConfiguration":report})),
        )
            .into_response();
    }
    state
        .app_events()
        .publish("models-changed", json!({"agentId":null}));
    Json(json!({"ok":true,"preferencesSaved":true,"modelConfiguration":report})).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentQuery {
    agent_id: String,
}
fn recent(
    state: &ServiceState,
    query: &AgentQuery,
    path: Option<&Value>,
    add: bool,
    clear: bool,
) -> Response {
    let dir = match state.memory().agent_dir_checked(&query.agent_id) {
        Ok(value) => value,
        Err(_) => return error(StatusCode::NOT_FOUND, "agent not found"),
    };
    let folder = if clear {
        None
    } else {
        let Some(folder) = path
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return bad("path must be a non-empty string");
        };
        let folder = folder.replace('\\', "/");
        let folder = if folder == "/" {
            folder
        } else {
            folder.trim_end_matches('/').into()
        };
        if add && !std::fs::metadata(&folder).is_ok_and(|m| m.is_dir()) {
            return bad("path must be an existing directory");
        }
        Some(folder)
    };
    // 与现有设置保存共用串行保护；读取和合并在同一临界区，避免覆盖别人的单项修改。
    let _guard = crate::agents_admin::preferences_locked();
    let saved = match am::config::read_yaml_mapping_as_json(&dir.join("config.yaml")) {
        Ok(Some(value)) => value,
        Ok(None) => return failed("agent config missing"),
        Err(e) => return failed(e.to_string()),
    };
    let mut history = Vec::<String>::new();
    if add {
        history.push(folder.clone().expect("非清空操作有路径"));
    }
    if !clear {
        if let Some(raw) = saved.get("cwd_history") {
            let Some(items) = raw.as_array() else {
                return failed("saved cwd_history must be array");
            };
            for item in items {
                let Some(item) = item.as_str() else {
                    return failed("saved cwd_history entries must be strings");
                };
                let item = item.trim().replace('\\', "/");
                let item = if item == "/" {
                    item
                } else {
                    item.trim_end_matches('/').into()
                };
                if !item.is_empty() && (!add && folder.as_deref() == Some(&item)) {
                    continue;
                }
                if !item.is_empty() && !history.contains(&item) {
                    history.push(item);
                }
            }
        }
    }
    history.truncate(10);
    if let Err(e) = config::update_config_for_agent(
        state.memory(),
        Some(&query.agent_id),
        &json!({"cwd_history":history}),
    ) {
        return failed(e);
    }
    state
        .app_events()
        .publish("agent-updated", json!({"agentId":query.agent_id}));
    Json(json!({"ok":true,"cwd_history":history})).into_response()
}
pub async fn recent_add_route(
    State(state): State<ServiceState>,
    Query(query): Query<AgentQuery>,
    Json(body): Json<Value>,
) -> Response {
    let _authority = state.media().authority_mutation_lease().await;
    recent(&state, &query, body.get("path"), true, false)
}
pub async fn recent_remove_route(
    State(state): State<ServiceState>,
    Query(query): Query<AgentQuery>,
    Json(body): Json<Value>,
) -> Response {
    let _authority = state.media().authority_mutation_lease().await;
    recent(&state, &query, body.get("path"), false, false)
}
pub async fn recent_clear_route(
    State(state): State<ServiceState>,
    Query(query): Query<AgentQuery>,
) -> Response {
    let _authority = state.media().authority_mutation_lease().await;
    recent(&state, &query, None, false, true)
}
fn default_workspace() -> Result<std::path::PathBuf, String> {
    // 默认书桌属于操作系统用户；不受当前助手或服务数据根切换影响。
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|s| !s.is_empty())
        .ok_or("system home unavailable")?;
    Ok(std::path::PathBuf::from(home)
        .join("Desktop")
        .join("OH-WorkSpace"))
}
pub async fn default_workspace_route() -> Response {
    match default_workspace() {
        Ok(path) => Json(json!({"path":path})).into_response(),
        Err(e) => failed(e),
    }
}
pub async fn ensure_default_workspace_route() -> Response {
    match default_workspace().and_then(|path| {
        std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
        Ok(path)
    }) {
        Ok(path) => Json(json!({"ok":true,"path":path})).into_response(),
        Err(e) => failed(e),
    }
}
pub async fn user_profile_put_route(
    State(state): State<ServiceState>,
    Json(body): Json<Value>,
) -> Response {
    let Some(content) = body.get("content").and_then(Value::as_str) else {
        return bad("content must be a string");
    };
    if !body.as_object().is_some_and(|m| m.len() == 1) || content.len() > 1024 * 1024 {
        return bad("user profile requires only content, at most 1 MiB");
    }
    let _mutation = am::ContextMaterialMutation::for_home(state.memory().home());
    let home = state.memory().home();
    let parent = match crate::filetools::fsio::DirFd::open(home)
        .and_then(|root| root.child_directory(std::ffi::OsStr::new("user"), true))
    {
        Ok(value) => value,
        Err(_) => {
            return failed(
                "user profile directory must stay inside the original home without links",
            )
        }
    };
    let path = state.memory().user_dir().join("user.md");
    let safety = crate::file_safety::FileSafety::new(home);
    let authorize = || {
        parent
            .same_directory(&state.memory().user_dir())
            .unwrap_or(false)
    };
    let outcome = match std::fs::symlink_metadata(&path) {
        Ok(_) => match crate::file_safety::Snapshot::read_bounded(&path, 1024 * 1024) {
            Ok(snapshot) => safety.replace(
                "user-profile",
                &path,
                &snapshot,
                content.as_bytes(),
                authorize,
            ),
            Err(e) => Err(e),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            safety.create("user-profile", &path, content.as_bytes(), authorize)
        }
        Err(e) => Err(crate::file_safety::WriteError::Failed(e.to_string())),
    };
    if let Err(e) = outcome {
        return (StatusCode::INTERNAL_SERVER_ERROR,Json(json!({"error":e.to_string(),"writeMayHaveOccurred":matches!(e,crate::file_safety::WriteError::Unknown(_))}))).into_response();
    }
    state
        .app_events()
        .publish("config_changed", json!({"userProfile":true}));
    Json(json!({"ok":true})).into_response()
}

pub(crate) fn read_user_profile(state: &ServiceState) -> Result<String, String> {
    match crate::filetools::fsio::DirFd::open(state.memory().home())
        .and_then(|root| root.child_directory(std::ffi::OsStr::new("user"), false))
    {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(_) => {
            return Err(
                "user profile directory must stay inside the original home without links".into(),
            )
        }
    }
    let path = state.memory().user_dir().join("user.md");
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e.to_string()),
        Ok(_) => crate::file_safety::Snapshot::read_bounded(&path, 1024 * 1024)
            .map_err(|e| e.to_string())
            .and_then(|snapshot| {
                String::from_utf8(snapshot.bytes)
                    .map_err(|_| "user profile must be valid UTF-8".into())
            }),
    }
}
