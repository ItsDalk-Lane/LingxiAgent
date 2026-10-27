//! 媒体与语音识别管理面：目录读写沿唯一 ModelConfiguration Source 发布，
//! 四类默认选择仍沿现有 preferences 规则，不把偏好写入冒充操作槽位重绑。
//! 授权：读 = settings.read，配置写 = settings.write，模型增删改 = providers.manage。
use crate::{
    agents_admin::agent_config_readback as readback, model_configuration::ModelSettingsChange,
    security_audit, EndpointError, Principal, ServiceState,
};
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Extension, Json, Router,
};
use lingxi_kernel::model_exchange::{OperationClass, ProtocolFamily};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Capability {
    Image,
    Video,
    Speech,
    Asr,
}
impl Capability {
    fn parse_kind(kind: &str) -> Option<Self> {
        Some(match kind {
            "image" => Self::Image,
            "video" => Self::Video,
            "speech" => Self::Speech,
            _ => return None,
        })
    }
    /// 媒体目录类别标识，沿用现役 mediaUserConfigKey（snake_case）。
    fn overlay_key(self) -> &'static str {
        match self {
            Self::Image => "image_generation",
            Self::Video => "video_generation",
            Self::Speech => "speech_generation",
            Self::Asr => "speech_recognition",
        }
    }
    /// preferences.json 的顶层键与内置声明（media_metadata 生成物）的能力键。
    fn config_key(self) -> &'static str {
        match self {
            Self::Image => "imageGeneration",
            Self::Video => "videoGeneration",
            Self::Speech => "speechGeneration",
            Self::Asr => "speechRecognition",
        }
    }
    fn default_field(self) -> &'static str {
        match self {
            Self::Image => "defaultImageModel",
            Self::Video => "defaultVideoModel",
            Self::Speech => "defaultSpeechModel",
            Self::Asr => "defaultModel",
        }
    }
    fn operation_class(self) -> OperationClass {
        match self {
            Self::Image => OperationClass::Image,
            Self::Video => OperationClass::Video,
            Self::Speech => OperationClass::Speech,
            Self::Asr => OperationClass::SpeechRecognition,
        }
    }
    /// media_route/describe 的 kind 词；ASR 不走媒体生成路由。
    fn route_kind(self) -> Option<&'static str> {
        match self {
            Self::Image => Some("image"),
            Self::Video => Some("video"),
            Self::Speech => Some("speech"),
            Self::Asr => None,
        }
    }
    fn audit_action(self) -> &'static str {
        match self {
            Self::Image => "settings.imageGeneration.update",
            Self::Video => "settings.videoGeneration.update",
            Self::Speech => "settings.speechGeneration.update",
            Self::Asr => "settings.speechRecognition.update",
        }
    }
}

// ── 模型归一化与投影（core/provider-registry.ts normalizeMediaModel +
//    universal-media-manager.ts projectMediaProviderModel）──

fn pick_str<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value[*key].as_str().filter(|v| !v.is_empty()))
}

/// normalizeMediaModel：id 必填（trim 后非空）；displayName 回退链
/// displayName||display_name||name||fallback.displayName||fallback.name||id；
/// protocolId 取 model.protocolId||protocol_id||fallback 同名，仅在可解析时携带。
fn normalize_media_model(model: &Value, fallback: Option<&Value>) -> Option<Value> {
    let raw_id = if model.is_object() {
        model["id"].as_str()
    } else {
        model.as_str()
    }?;
    let id = raw_id.trim();
    if id.is_empty() {
        return None;
    }
    let mut out = if model.is_object() {
        model.as_object().cloned().unwrap_or_default()
    } else {
        Map::new()
    };
    let display = pick_str(model, &["displayName", "display_name", "name"])
        .or_else(|| fallback.and_then(|f| pick_str(f, &["displayName", "name"])))
        .unwrap_or(id)
        .to_owned();
    out.insert("id".into(), json!(id));
    out.insert("displayName".into(), json!(display));
    let protocol = pick_str(model, &["protocolId", "protocol_id"])
        .or_else(|| fallback.and_then(|f| pick_str(f, &["protocolId", "protocol_id"])));
    if let Some(protocol) = protocol {
        out.insert("protocolId".into(), json!(protocol));
    }
    Some(Value::Object(out))
}

/// adapterAvailable/unavailableReason/unavailableMessage：协议族登记且服务该
/// 操作类别 = 可用；已识别但不服务该类别 = adapter_unavailable；无法识别 =
/// protocol_unrecognized（消息文案沿用现役中文表述）。
fn adapter_verdict(protocol: Option<&str>, class: OperationClass) -> (bool, Value, Value) {
    match protocol {
        Some(name) => match ProtocolFamily::parse(name) {
            Some(ProtocolFamily::SystemSpeechRecognition) => (
                false,
                json!("adapter_unavailable"),
                json!("系统语音识别尚未实现"),
            ),
            Some(family) if family.serves(class) => (true, Value::Null, Value::Null),
            _ => (
                false,
                json!("adapter_unavailable"),
                json!(format!("暂不支持此模型的调用方式（{name}）")),
            ),
        },
        None => (
            false,
            json!("protocol_unrecognized"),
            json!("尚未识别此模型的调用方式，暂时无法调用"),
        ),
    }
}

fn project_model(model: &Value, class: OperationClass) -> Value {
    let mut out = model.as_object().cloned().unwrap_or_default();
    let id = out
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let name = pick_str(model, &["displayName", "name"])
        .unwrap_or(&id)
        .to_owned();
    let (available, reason, message) =
        adapter_verdict(out.get("protocolId").and_then(Value::as_str), class);
    out.insert("id".into(), json!(id));
    out.insert("name".into(), json!(name));
    out.insert("displayName".into(), json!(name));
    out.insert("adapterAvailable".into(), json!(available));
    out.insert("unavailableReason".into(), reason);
    out.insert("unavailableMessage".into(), message);
    Value::Object(out)
}

/// 供应商凭据投影：ready/keyless（无需凭据）= 可用；其余状态如实映射，
/// 不在列表面泄露任何凭据材料。
fn credential_projection(
    status: Option<&crate::credentials::ProviderCredentialStatus>,
) -> (bool, Value, Value) {
    match status.map(|s| s.state.as_str()) {
        Some("ready") | Some("keyless") => (true, Value::Null, Value::Null),
        Some(state) => {
            let message = match state {
                "not_logged_in" => "供应商尚未登录",
                "revoked" => "供应商凭据已撤销",
                _ => "供应商凭据不可用",
            };
            (false, json!(state), json!(message))
        }
        None => (
            false,
            json!("provider_not_found"),
            json!("供应商未在模型面登记"),
        ),
    }
}

// ── 列表（listImageProviders/listVideoProviders/listSpeechProviders/
//    speech-recognition listProviders）──

/// 只从已发布快照识别模型；旧目录投影不参与运行目录的权威判断。
fn model_protocol<'a>(model: &'a Value, provider: &'a Value) -> Option<&'a str> {
    pick_str(model, &["protocol", "api", "protocolId", "protocol_id"])
        .or_else(|| pick_str(provider, &["api", "protocol"]))
}
fn model_matches(model: &Value, provider: &Value, cap: Capability) -> bool {
    if let Some(declared) = model.get("mediaCapability").and_then(Value::as_str) {
        if declared != cap.overlay_key() {
            return false;
        }
    }
    model_protocol(model, provider)
        .and_then(ProtocolFamily::parse)
        .is_some_and(|family| family.serves(cap.operation_class()))
}
fn published_models(provider: &Value, cap: Capability) -> Vec<Value> {
    provider["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|model| model_matches(model, provider, cap))
        .filter_map(|model| {
            let mut model = normalize_media_model(model, None)?;
            let protocol = model_protocol(&model, provider)?.to_owned();
            model["protocolId"] = json!(protocol);
            Some(model)
        })
        .collect()
}
async fn list_payload(
    state: &ServiceState,
    principal: &Principal,
    cap: Capability,
) -> Result<Value, Box<Response>> {
    let snapshot = state
        .model_configuration()
        .snapshot(principal)
        .await
        .map_err(|error| Box::new(error.into_response()))?;
    let merged = snapshot["providers"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let declarations =
        crate::operations::media_declaration_catalog(cap.config_key()).map_err(|failure| {
            Box::new(EndpointError::invalid_message(failure.message).into_response())
        })?;
    let credential_status: BTreeMap<String, crate::credentials::ProviderCredentialStatus> =
        match state.credential_service() {
            Some(service) => service
                .status()
                .await
                .into_iter()
                .map(|status| (status.provider.clone(), status))
                .collect(),
            None => BTreeMap::new(),
        };
    let declared_by_provider: BTreeMap<String, (String, Vec<Value>)> = declarations
        .into_iter()
        .map(|(id, display, block)| {
            let models = block["models"].as_array().cloned().unwrap_or_default();
            (id, (display, models))
        })
        .collect();
    let mut provider_ids: Vec<String> = declared_by_provider.keys().cloned().collect();
    for (id, entry) in &merged {
        if !published_models(entry, cap).is_empty() && !provider_ids.contains(id) {
            provider_ids.push(id.clone());
        }
    }
    let class = cap.operation_class();
    let mut providers = Map::new();
    for pid in provider_ids {
        let (declared_display, declared) = declared_by_provider
            .get(&pid)
            .cloned()
            .unwrap_or_else(|| (pid.clone(), Vec::new()));
        let added = merged
            .get(&pid)
            .map(|provider| published_models(provider, cap))
            .unwrap_or_default();
        let added_ids: Vec<&str> = added.iter().filter_map(|m| m["id"].as_str()).collect();
        // 声明只列为候选；删除后的运行模型不能再被静态声明补回。
        let effective = added.clone();
        let catalog: Vec<Value> = declared
            .iter()
            .filter(|model| {
                model["id"]
                    .as_str()
                    .is_some_and(|id| !added_ids.contains(&id))
            })
            .cloned()
            .collect();
        let models: Vec<Value> = effective
            .iter()
            .map(|m| {
                let mut model = project_model(m, class);
                if state.operations().is_none() {
                    model["adapterAvailable"] = json!(false);
                    model["unavailableReason"] = json!("model_plane_inactive");
                    model["unavailableMessage"] = json!("当前宿主的模型执行链尚未激活");
                }
                model
            })
            .collect();
        let available: Vec<Value> = catalog.iter().map(|m| project_model(m, class)).collect();
        // 现役跳过规则：有效模型与目录候选皆空的供应商不出现在列表。
        if models.is_empty() && available.is_empty() {
            continue;
        }
        let display = merged
            .get(&pid)
            .and_then(|entry| pick_str(entry, &["display_name", "displayName"]))
            .unwrap_or(&declared_display)
            .to_owned();
        let (has_credentials, reason, message) = credential_projection(credential_status.get(&pid));
        let mut entry = Map::new();
        entry.insert("providerId".into(), json!(pid));
        entry.insert("displayName".into(), json!(display));
        entry.insert("hasCredentials".into(), json!(has_credentials));
        entry.insert("unavailableReason".into(), reason);
        entry.insert("unavailableMessage".into(), message);
        entry.insert("models".into(), json!(models));
        // 语音识别面的 availableModels 与 models 同列，目录候选放 catalogModels。
        let available_models = if cap == Capability::Asr {
            json!(models.clone())
        } else {
            json!(available)
        };
        entry.insert("availableModels".into(), available_models);
        if cap == Capability::Asr {
            let catalog_models: Vec<Value> =
                catalog.iter().map(|m| project_model(m, class)).collect();
            entry.insert("catalogModels".into(), json!(catalog_models));
        }
        providers.insert(pid, Value::Object(entry));
    }
    let preferences = state
        .memory()
        .read_preferences_root()
        .map_err(|message| Box::new(EndpointError::invalid_message(message).into_response()))?;
    Ok(json!({
        "providers": Value::Object(providers),
        "config": normalized_config(&preferences, cap),
    }))
}

/// normalizeImageGenerationConfig/Video/Speech 与 getSpeechRecognitionConfig：
/// 只保留合法 default 引用与 providerDefaults；ASR 额外保留 enabled。
fn normalized_config(preferences: &Value, cap: Capability) -> Value {
    let raw = &preferences[cap.config_key()];
    let mut out = Map::new();
    if cap == Capability::Asr {
        out.insert("enabled".into(), json!(raw["enabled"] == true));
    }
    let field = cap.default_field();
    if let Some(model) = raw[field].as_object() {
        let provider = model["provider"].as_str().unwrap_or("").trim();
        let id = model["id"].as_str().unwrap_or("").trim();
        if !provider.is_empty() && !id.is_empty() {
            out.insert(field.into(), json!({"provider": provider, "id": id}));
        }
    }
    if cap != Capability::Asr {
        if let Some(defaults) = raw["providerDefaults"].as_object() {
            out.insert("providerDefaults".into(), Value::Object(defaults.clone()));
        }
    }
    Value::Object(out)
}

// ── 配置写（setImageConfig/setVideoConfig/setSpeechConfig/setConfig）──

fn config_put(
    state: &ServiceState,
    principal: &Principal,
    cap: Capability,
    body: Value,
    asr_list: Option<&Value>,
) -> Result<Value, Box<Response>> {
    if readback::scope_denied(Some(principal), "settings.write") {
        return Err((
            axum::http::StatusCode::FORBIDDEN,
            Json(json!({ "error": "insufficient_scope", "scope": "settings.write" })),
        )
            .into_response()
            .into());
    }
    let values = if body["values"].is_object() {
        body["values"].clone()
    } else {
        body
    };
    let Some(patch) = values.as_object() else {
        return Err(
            EndpointError::invalid_message("config values must be an object")
                .into_response()
                .into(),
        );
    };
    let memory = state.memory();
    let home = memory.home().to_path_buf();
    let field = cap.default_field();
    // 现役只处理默认模型键与 providerDefaults（ASR 为 enabled/defaultModel），
    // 未知键静默忽略。
    let keys: &[&str] = if cap == Capability::Asr {
        &["enabled", "defaultModel"]
    } else {
        &[field, "providerDefaults"]
    };
    let mut updates = Map::new();
    for key in keys {
        if let Some(value) = patch.get(*key) {
            updates.insert((*key).into(), value.clone());
        }
    }
    let _guard = crate::agents_admin::preferences_locked();
    let mut preferences = memory
        .read_preferences_root()
        .map_err(|message| Box::new(EndpointError::invalid_message(message).into_response()))?;
    let current = normalized_config(&preferences, cap);
    let mut next = current.as_object().cloned().unwrap_or_default();
    for (key, value) in &updates {
        if value.is_null() {
            // null/undefined = 删除语义。
            next.remove(key);
            continue;
        }
        match key.as_str() {
            "enabled" => {
                next.insert("enabled".into(), json!(value == &Value::Bool(true)));
            }
            "providerDefaults" => {
                let Some(defaults) = value.as_object() else {
                    return Err(EndpointError::invalid_message(
                        "providerDefaults must be an object",
                    )
                    .into_response()
                    .into());
                };
                next.insert(key.clone(), Value::Object(defaults.clone()));
            }
            _ => {
                // 默认模型：图片/视频非法形状抛错，语音静默丢弃（现役差异保留）；
                // ASR 用现役专有措辞。
                match value.as_object() {
                    None if cap == Capability::Speech => {}
                    None => {
                        let detail = if cap == Capability::Asr {
                            "speechRecognition.defaultModel must be an object".to_owned()
                        } else {
                            format!("{field} must be an object with provider and id")
                        };
                        return Err(EndpointError::invalid_message(detail)
                            .into_response()
                            .into());
                    }
                    Some(model) => {
                        let provider = model["provider"].as_str().unwrap_or("").trim();
                        let id = model["id"].as_str().unwrap_or("").trim();
                        if provider.is_empty() || id.is_empty() {
                            if cap == Capability::Speech {
                                continue;
                            }
                            let detail = if cap == Capability::Asr {
                                "speechRecognition.defaultModel requires provider and id".to_owned()
                            } else {
                                format!("{field} requires provider and id")
                            };
                            return Err(EndpointError::invalid_message(detail)
                                .into_response()
                                .into());
                        }
                        next.insert(key.clone(), json!({"provider": provider, "id": id}));
                    }
                }
            }
        }
    }
    // 默认模型必须可解析（resolve*ModelRef）：图片/视频/语音经 operation 面
    // 路由解析；ASR 要求在当前列表内且适配可用。
    if let Some(model) = next.get(field).and_then(Value::as_object).cloned() {
        let provider = model["provider"].as_str().unwrap_or_default();
        let id = model["id"].as_str().unwrap_or_default();
        if cap == Capability::Asr {
            // 列表快照由调用方在进入偏好写锁之前异步算出（list_payload）。
            let listed = asr_list.ok_or_else(|| {
                Box::new(EndpointError::invalid_message("语音识别供应商列表不可用").into_response())
            })?;
            let usable = listed["providers"][provider]["models"]
                .as_array()
                .is_some_and(|models| {
                    models
                        .iter()
                        .any(|m| m["id"] == id && m["adapterAvailable"] == true)
                });
            if !usable {
                return Err(EndpointError::invalid_message(
                    "speech recognition default model is unavailable",
                )
                .into_response()
                .into());
            }
        } else {
            let operations = state.operations().ok_or_else(|| {
                Box::new(EndpointError::invalid_message("媒体模型面未配置").into_response())
            })?;
            operations
                .media_route(
                    cap.route_kind().expect("生成类能力有路由 kind"),
                    Some(provider.to_owned()),
                    Some(id.to_owned()),
                )
                .map_err(|failure| {
                    Box::new(EndpointError::invalid_message(failure.message).into_response())
                })?;
        }
    }
    preferences[cap.config_key()] = Value::Object(next.clone());
    memory
        .write_preferences_root(&preferences)
        .map_err(|message| Box::new(EndpointError::invalid_message(message).into_response()))?;
    let now = state.clock().now_unix_ms();
    let metadata = if cap == Capability::Asr {
        json!({"enabled": next.get("enabled") == Some(&Value::Bool(true))})
    } else {
        let has_default = next.contains_key(field);
        match cap {
            Capability::Image => json!({"hasDefaultImageModel": has_default}),
            Capability::Video => json!({"hasDefaultVideoModel": has_default}),
            Capability::Speech => json!({"hasDefaultSpeechModel": has_default}),
            Capability::Asr => unreachable!(),
        }
    };
    security_audit::project(
        &home,
        "media-admin",
        &[json!({
            "action": cap.audit_action(),
            "target": cap.config_key(),
            "atUnixMs": now,
            "metadata": metadata,
        })],
    )
    .map_err(|message| Box::new(EndpointError::invalid_message(message).into_response()))?;
    Ok(json!({ "ok": true, "config": Value::Object(next.clone()), "values": Value::Object(next) }))
}

// ── 模型增删改（addMediaModel/updateMediaModelEntry/removeMediaModel）──

/// 媒体模型编辑补丁（shared/media-model-edit.ts buildMediaModelEditPatch）：
/// 只允许 displayName/inputs/outputs，模态值受限且按类别不变量校验。
fn build_edit_patch(body: &Value, cap: Capability) -> Result<Map<String, Value>, Box<Response>> {
    let Some(map) = body.as_object() else {
        return Err(EndpointError::invalid_message(
            "request body must be an object with displayName, inputs, or outputs",
        )
        .into_response()
        .into());
    };
    let mut patch = Map::new();
    for (key, value) in map {
        match key.as_str() {
            "displayName" | "display_name" | "name" => {
                let Some(name) = value.as_str().map(str::trim).filter(|v| !v.is_empty()) else {
                    return Err(EndpointError::invalid_message(
                        "displayName must be a non-empty string",
                    )
                    .into_response().into());
                };
                patch.insert("displayName".into(), json!(name));
            }
            "inputs" | "outputs" => {
                let Some(list) = value.as_array() else {
                    return Err(EndpointError::invalid_message(format!(
                        "{key} must be a non-empty modality array with only known values (text|image|video|audio)"
                    ))
                    .into_response().into());
                };
                let mut modalities: Vec<&str> = Vec::new();
                for entry in list {
                    let Some(modality) = entry.as_str() else {
                        return Err(EndpointError::invalid_message(format!(
                            "{key} must be a non-empty modality array with only known values (text|image|video|audio)"
                        ))
                        .into_response().into());
                    };
                    if !["text", "image", "video", "audio"].contains(&modality) {
                        return Err(EndpointError::invalid_message(format!(
                            "{key} must be a non-empty modality array with only known values (text|image|video|audio)"
                        ))
                        .into_response().into());
                    }
                    if !modalities.contains(&modality) {
                        modalities.push(modality);
                    }
                }
                if modalities.is_empty() {
                    return Err(EndpointError::invalid_message(format!(
                        "{key} must be a non-empty modality array with only known values (text|image|video|audio)"
                    ))
                    .into_response().into());
                }
                modalities.sort_unstable();
                patch.insert(
                    key.clone(),
                    Value::Array(modalities.into_iter().map(|m| json!(m)).collect()),
                );
            }
            other => {
                return Err(EndpointError::invalid_message(format!(
                    "field \"{other}\" is not editable on an added model (editable: displayName, inputs, outputs)"
                ))
                .into_response().into())
            }
        }
    }
    if patch.is_empty() {
        return Err(EndpointError::invalid_message(
            "nothing to update: provide displayName, inputs, or outputs",
        )
        .into_response()
        .into());
    }
    // 类别不变量（shared/modality.ts）：只校验本次补丁实际给出的集合。
    let requires = |set: &str, modality: &str, message: &str| -> Result<(), Box<Response>> {
        if let Some(list) = patch.get(set).and_then(Value::as_array) {
            if !list.iter().any(|m| m.as_str() == Some(modality)) {
                return Err(EndpointError::invalid_message(message)
                    .into_response()
                    .into());
            }
        }
        Ok(())
    };
    match cap {
        Capability::Image => requires(
            "outputs",
            "image",
            "image generation models must include \"image\" in outputs",
        )?,
        Capability::Video => requires(
            "outputs",
            "video",
            "video generation models must include \"video\" in outputs",
        )?,
        Capability::Speech => requires(
            "outputs",
            "audio",
            "speech generation models must include \"audio\" in outputs",
        )?,
        Capability::Asr => {
            requires(
                "inputs",
                "audio",
                "speech recognition models must include \"audio\" in inputs",
            )?;
            requires(
                "outputs",
                "text",
                "speech recognition models must include \"text\" in outputs",
            )?;
        }
    }
    Ok(patch)
}

fn conflict(message: impl Into<String>) -> Box<Response> {
    Box::new(
        (
            axum::http::StatusCode::CONFLICT,
            Json(json!({"error":message.into(),"code":"media_model_conflict"})),
        )
            .into_response(),
    )
}
async fn catalog_snapshot(
    state: &ServiceState,
    principal: &Principal,
) -> Result<(Value, u64), Box<Response>> {
    let snapshot = state
        .model_configuration()
        .snapshot(principal)
        .await
        .map_err(|error| Box::new(error.into_response()))?;
    let generation = snapshot["generation"]
        .as_str()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| conflict("模型快照代次不可确认"))?;
    Ok((snapshot, generation))
}
fn catalog_provider<'a>(snapshot: &'a Value, provider: &str) -> Result<&'a Value, Box<Response>> {
    snapshot["providers"].get(provider).ok_or_else(|| {
        Box::new(
            (
                axum::http::StatusCode::NOT_FOUND,
                Json(json!({"error":"provider_not_found"})),
            )
                .into_response(),
        )
    })
}
fn catalog_model<'a>(provider: &'a Value, id: &str) -> Option<&'a Value> {
    provider["models"]
        .as_array()?
        .iter()
        .find(|model| model["id"].as_str() == Some(id))
}
async fn publish_model_change(
    state: &ServiceState,
    principal: &Principal,
    generation: u64,
    change: ModelSettingsChange,
) -> Result<Value, Box<Response>> {
    match state
        .model_configuration()
        .save_at_generation(principal, &[change], generation)
        .await
    {
        Ok(report) => {
            state
                .app_events()
                .publish("models-changed", json!({"agentId":null}));
            Ok(json!({"ok":true,"result":report}))
        }
        Err(error) => {
            if error.report.applied {
                state
                    .app_events()
                    .publish("models-changed", json!({"agentId":null}));
            }
            Err(Box::new(error.into_response()))
        }
    }
}

fn manage_scope(principal: &Principal) -> Result<(), Box<Response>> {
    if readback::scope_denied(Some(principal), "providers.manage") {
        return Err((
            axum::http::StatusCode::FORBIDDEN,
            Json(json!({ "error": "insufficient_scope", "scope": "providers.manage" })),
        )
            .into_response()
            .into());
    }
    Ok(())
}

async fn add_model(
    state: &ServiceState,
    principal: &Principal,
    cap: Capability,
    provider: &str,
    body: Value,
) -> Result<Value, Box<Response>> {
    manage_scope(principal)?;
    let model = if body["model"].is_object() {
        body["model"].clone()
    } else {
        body
    };
    // 归一化回退：同供应商内置声明中的同名条目提供 protocolId。
    let declared =
        crate::operations::media_declaration_catalog(cap.config_key()).map_err(|failure| {
            Box::new(EndpointError::invalid_message(failure.message).into_response())
        })?;
    let raw_id = model["id"].as_str().unwrap_or("").trim().to_owned();
    let fallback = declared
        .iter()
        .find(|(id, _, _)| id == provider)
        .and_then(|(_, _, block)| block["models"].as_array().cloned())
        .and_then(|models| {
            models
                .into_iter()
                .find(|entry| entry["id"].as_str() == Some(raw_id.as_str()))
        });
    let Some(normalized) = normalize_media_model(&model, fallback.as_ref()) else {
        return Err(EndpointError::invalid_message("media model id is required")
            .into_response()
            .into());
    };
    let id = normalized["id"].as_str().expect("归一化保证 id").to_owned();
    if normalized["protocolId"].as_str().is_none() {
        return Err(EndpointError::invalid_message(format!(
            "Media model \"{provider}/{id}\" missing protocolId"
        ))
        .into_response()
        .into());
    }
    let protocol = normalized["protocolId"].as_str().expect("已检查协议");
    for field in ["api", "protocol", "operationProtocol"] {
        if normalized
            .get(field)
            .is_some_and(|value| value.as_str() != Some(protocol))
        {
            return Err(conflict("模型包含相互冲突的调用方式"));
        }
    }
    if normalized.get("operations").is_some() {
        return Err(conflict("媒体模型不能使用向量或重排操作标签"));
    }
    if !ProtocolFamily::parse(protocol).is_some_and(|family| family.serves(cap.operation_class())) {
        return Err(conflict("模型调用方式不支持当前媒体类别"));
    }
    let (snapshot, generation) = catalog_snapshot(state, principal).await?;
    let entry = catalog_provider(&snapshot, provider)?;
    if let Some(existing) = catalog_model(entry, &id) {
        if !model_matches(existing, entry, cap) || model_protocol(existing, entry) != Some(protocol)
        {
            return Err(conflict("同名模型已登记为其他类别或调用方式"));
        }
        return Ok(json!({"ok":true}));
    }
    let mut patch = normalized.as_object().cloned().expect("已归一化为对象");
    patch.insert("api".into(), json!(protocol));
    patch.insert("mediaCapability".into(), json!(cap.overlay_key()));
    publish_model_change(
        state,
        principal,
        generation,
        ModelSettingsChange::ModelPatch {
            provider: provider.into(),
            model: id,
            patch: Value::Object(patch),
        },
    )
    .await
}

async fn edit_model(
    state: &ServiceState,
    principal: &Principal,
    cap: Capability,
    provider: &str,
    model_id: &str,
    body: Value,
) -> Result<Value, Box<Response>> {
    manage_scope(principal)?;
    let patch_source = if body["model"].is_object() {
        body["model"].clone()
    } else {
        body
    };
    let mut patch = build_edit_patch(&patch_source, cap)?;
    let (snapshot, generation) = catalog_snapshot(state, principal).await?;
    let entry = catalog_provider(&snapshot, provider)?;
    let Some(existing) = catalog_model(entry, model_id) else {
        return Err(Box::new((axum::http::StatusCode::NOT_FOUND, Json(json!({"error":format!("model \"{provider}/{model_id}\" is not added under {}; use POST to add it first",cap.overlay_key())}))).into_response()));
    };
    if !model_matches(existing, entry, cap) {
        return Err(conflict("同名模型不属于当前媒体类别"));
    }
    patch.insert("mediaCapability".into(), json!(cap.overlay_key()));
    publish_model_change(
        state,
        principal,
        generation,
        ModelSettingsChange::ModelPatch {
            provider: provider.into(),
            model: model_id.into(),
            patch: Value::Object(patch),
        },
    )
    .await
}

async fn remove_model(
    state: &ServiceState,
    principal: &Principal,
    cap: Capability,
    provider: &str,
    model_id: &str,
) -> Result<Value, Box<Response>> {
    manage_scope(principal)?;
    let (snapshot, generation) = catalog_snapshot(state, principal).await?;
    let entry = catalog_provider(&snapshot, provider)?;
    let Some(existing) = catalog_model(entry, model_id) else {
        return Err(Box::new(
            EndpointError::invalid_message(format!(
                "media model \"{provider}/{model_id}\" not found in {}",
                cap.overlay_key()
            ))
            .into_response(),
        ));
    };
    if !model_matches(existing, entry, cap) {
        return Err(conflict("同名模型不属于当前媒体类别"));
    }
    publish_model_change(
        state,
        principal,
        generation,
        ModelSettingsChange::ModelRemove {
            provider: provider.into(),
            model: model_id.into(),
        },
    )
    .await
}

// ── 路由处理器 ──

/// 现役 safeJson：空正文或非法 JSON 一律按 {} 处理，不产生 400。
fn lenient_body(body: &Bytes) -> Value {
    serde_json::from_slice(body).unwrap_or_else(|_| json!({}))
}

fn read_scope(principal: &Principal) -> Result<(), Box<Response>> {
    if readback::scope_denied(Some(principal), "settings.read") {
        return Err((
            axum::http::StatusCode::FORBIDDEN,
            Json(json!({ "error": "insufficient_scope", "scope": "settings.read" })),
        )
            .into_response()
            .into());
    }
    Ok(())
}

async fn providers_query(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    if let Err(response) = read_scope(&principal) {
        return *response;
    }
    let cap = match query.get("capability").map(String::as_str) {
        Some("speech_recognition") | Some("asr") | Some("transcription") => Capability::Asr,
        Some("video_generation") | Some("video") | Some("videoGeneration") => Capability::Video,
        Some("speech_generation") | Some("speechGeneration") => Capability::Speech,
        _ => Capability::Image,
    };
    match list_payload(&state, &principal, cap).await {
        Ok(payload) => Json(payload).into_response(),
        Err(response) => *response,
    }
}

async fn list_providers_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path(kind): Path<String>,
) -> Response {
    if let Err(response) = read_scope(&principal) {
        return *response;
    }
    let Some(cap) = Capability::parse_kind(&kind) else {
        return EndpointError::not_found().into_response();
    };
    match list_payload(&state, &principal, cap).await {
        Ok(payload) => Json(payload).into_response(),
        Err(response) => *response,
    }
}

async fn get_speech_config(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
) -> Response {
    if let Err(response) = read_scope(&principal) {
        return *response;
    }
    match state.memory().read_preferences_root() {
        Ok(preferences) => Json(json!({
            "config": normalized_config(&preferences, Capability::Speech)
        }))
        .into_response(),
        Err(message) => EndpointError::invalid_message(message).into_response(),
    }
}

/// 语音配置写（静态路由段；参数化 {kind}/config 的 PUT 与静态 GET 同路径时
/// 静态优先，若只在参数化路由挂 PUT，speech 的 PUT 会被静态路由挡成 405）。
async fn put_speech_config(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    body: Bytes,
) -> Response {
    match config_put(
        &state,
        &principal,
        Capability::Speech,
        lenient_body(&body),
        None,
    ) {
        Ok(payload) => Json(payload).into_response(),
        Err(response) => *response,
    }
}

async fn put_config_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path(kind): Path<String>,
    body: Bytes,
) -> Response {
    let Some(cap) = Capability::parse_kind(&kind) else {
        return EndpointError::not_found().into_response();
    };
    // 生成类能力的默认模型校验走 operation 面路由解析（同步），无需列表快照。
    match config_put(&state, &principal, cap, lenient_body(&body), None) {
        Ok(payload) => Json(payload).into_response(),
        Err(response) => *response,
    }
}

async fn add_model_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path((kind, provider)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let Some(cap) = Capability::parse_kind(&kind) else {
        return EndpointError::not_found().into_response();
    };
    match add_model(&state, &principal, cap, &provider, lenient_body(&body)).await {
        Ok(payload) => Json(payload).into_response(),
        Err(response) => *response,
    }
}

async fn edit_model_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path((kind, provider, model)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    let Some(cap) = Capability::parse_kind(&kind) else {
        return EndpointError::not_found().into_response();
    };
    match edit_model(
        &state,
        &principal,
        cap,
        &provider,
        &model,
        lenient_body(&body),
    )
    .await
    {
        Ok(payload) => Json(payload).into_response(),
        Err(response) => *response,
    }
}

async fn remove_model_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path((kind, provider, model)): Path<(String, String, String)>,
) -> Response {
    let Some(cap) = Capability::parse_kind(&kind) else {
        return EndpointError::not_found().into_response();
    };
    match remove_model(&state, &principal, cap, &provider, &model).await {
        Ok(payload) => Json(payload).into_response(),
        Err(response) => *response,
    }
}

// ── 语音识别面（/lingxi/v1/speech-recognition/*）──

async fn asr_list(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
) -> Response {
    if let Err(response) = read_scope(&principal) {
        return *response;
    }
    match list_payload(&state, &principal, Capability::Asr).await {
        Ok(payload) => Json(payload).into_response(),
        Err(response) => *response,
    }
}

async fn asr_add_model(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path(provider): Path<String>,
    body: Bytes,
) -> Response {
    match add_model(
        &state,
        &principal,
        Capability::Asr,
        &provider,
        lenient_body(&body),
    )
    .await
    {
        Ok(payload) => Json(payload).into_response(),
        Err(response) => *response,
    }
}

async fn asr_edit_model(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path((provider, model)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    match edit_model(
        &state,
        &principal,
        Capability::Asr,
        &provider,
        &model,
        lenient_body(&body),
    )
    .await
    {
        Ok(payload) => Json(payload).into_response(),
        Err(response) => *response,
    }
}

async fn asr_remove_model(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path((provider, model)): Path<(String, String)>,
) -> Response {
    match remove_model(&state, &principal, Capability::Asr, &provider, &model).await {
        Ok(payload) => Json(payload).into_response(),
        Err(response) => *response,
    }
}

async fn asr_put_config(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    body: Bytes,
) -> Response {
    let parsed = lenient_body(&body);
    // ASR 合并后的配置只要带默认模型就重验（现役语义：对结果配置校验，
    // 不仅针对补丁），而校验引用列表投影（异步）；统一在进入写锁前算出。
    let snapshot = match list_payload(&state, &principal, Capability::Asr).await {
        Ok(payload) => payload,
        Err(response) => return *response,
    };
    match config_put(&state, &principal, Capability::Asr, parsed, Some(&snapshot)) {
        Ok(payload) => Json(payload).into_response(),
        Err(response) => *response,
    }
}

pub fn router() -> Router<ServiceState> {
    Router::new()
        .route("/lingxi/v1/media/providers", get(providers_query))
        .route(
            "/lingxi/v1/media/{kind}/providers",
            get(list_providers_route),
        )
        .route("/lingxi/v1/media/{kind}/config", put(put_config_route))
        .route(
            "/lingxi/v1/media/speech/config",
            get(get_speech_config).put(put_speech_config),
        )
        .route(
            "/lingxi/v1/media/{kind}/providers/{provider}/models",
            post(add_model_route),
        )
        .route(
            "/lingxi/v1/media/{kind}/providers/{provider}/models/{model}",
            put(edit_model_route).delete(remove_model_route),
        )
        .route("/lingxi/v1/speech-recognition/providers", get(asr_list))
        .route(
            "/lingxi/v1/speech-recognition/providers/{provider}/models",
            post(asr_add_model),
        )
        .route(
            "/lingxi/v1/speech-recognition/providers/{provider}/models/{model}",
            put(asr_edit_model).delete(asr_remove_model),
        )
        .route("/lingxi/v1/speech-recognition/config", put(asr_put_config))
}
