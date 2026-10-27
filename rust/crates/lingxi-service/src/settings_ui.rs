//! 当前桌面助手设置页的严格读面。每项来自真实文件或当前发布模型快照，失败显式返回。

use crate::ServiceState;
use axum::{
    extract::{Path as RoutePath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};

fn failure(message: impl Into<String>) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": message.into()})),
    )
        .into_response()
}

pub(crate) fn model_preferences(state: &ServiceState) -> Result<Value, String> {
    let preferences = state.memory().read_preferences_root()?;
    let mut models = Map::new();
    for key in [
        "title",
        "summarize",
        "memory",
        "knowledge",
        "vision",
        "approval",
        "guard",
    ] {
        models.insert(key.into(), Value::Null);
    }
    models.insert(
        "vision_enabled".into(),
        preferences
            .get("vision_auxiliary_enabled")
            .cloned()
            .unwrap_or(json!(false)),
    );
    let mut operation_models: Vec<Value> = Vec::new();
    if let Some(gateway) = state.model_gateway() {
        for (provider, entries) in gateway.catalog_models() {
            for model in entries {
                let operations = model
                    .metadata
                    .get("operations")
                    .cloned()
                    .unwrap_or(json!([]));
                if !operations
                    .as_array()
                    .is_some_and(|values| values.iter().all(Value::is_string))
                {
                    return Err("catalog model operations must be string array".into());
                }
                operation_models.push(json!({"id":model.id,"provider":provider,
                    "label":model.metadata.get("name").or_else(||model.metadata.get("displayName")).and_then(Value::as_str).unwrap_or(&model.id),
                    "dimensions":model.metadata.get("dimensions"),"operations":operations}));
            }
        }
        for (operation, binding) in gateway.configured_routes() {
            let slot = operation.strip_prefix("auxiliary.").unwrap_or(&operation);
            if models.contains_key(slot) {
                models.insert(
                    slot.into(),
                    json!({"provider": binding.provider, "id": binding.model}),
                );
            }
            if let Some(existing) = operation_models
                .iter_mut()
                .find(|m| m["provider"] == binding.provider && m["id"] == binding.model)
            {
                let operations = existing["operations"].as_array_mut().expect("操作列表");
                if !operations.contains(&json!(operation)) {
                    operations.push(json!(operation));
                }
            } else {
                operation_models.push(json!({"id": binding.model, "provider": binding.provider, "label": binding.model, "operations": [operation]}));
            }
        }
    }
    Ok(
        json!({"models": models, "operation_models": operation_models,
        "thinking_level": preferences.get("thinking_level").cloned().unwrap_or(json!("medium")),
        "search": crate::credentials::search::SearchCredentials::from_preferences(&preferences)?.masked()}),
    )
}

pub async fn preferences_models_route(State(state): State<ServiceState>) -> Response {
    match model_preferences(&state) {
        Ok(value) => Json(value).into_response(),
        Err(error) => failure(error),
    }
}

pub async fn models_route(State(state): State<ServiceState>) -> Response {
    let mut models = Vec::new();
    if let Some(gateway) = state.model_gateway() {
        for (provider, entries) in gateway.catalog_models() {
            for model in entries {
                let selection = lingxi_kernel::model_exchange::ModelRouteRequest {
                    operation: lingxi_kernel::model_exchange::ModelOperation::Chat,
                    provider: Some(provider.clone()),
                    model: Some(model.id.clone()),
                };
                // 设置选择范围以当前目录和真实可派发协议为准；旧槽不能复活已删除模型。
                if gateway.resolve_dispatch(&selection).is_err() {
                    continue;
                }
                models.push(json!({"id":model.id,"provider":provider,
                    "name":model.metadata.get("name").or_else(||model.metadata.get("displayName")).and_then(Value::as_str).unwrap_or(&model.id),
                    "contextWindow":model.metadata.get("contextWindow").or_else(||model.metadata.get("context")),
                    "input":model.metadata.get("inputs").or_else(||model.metadata.get("input"))}));
            }
        }
    }
    Json(json!({"models":models})).into_response()
}

pub async fn providers_summary_route(State(state): State<ServiceState>) -> Response {
    let providers = match crate::agents_admin::agent_config_readback::merged_catalog_providers_with(
        state.memory().home(),
        false,
    ) {
        Ok(value) => value,
        Err(error) => return failure(error),
    };
    let mut summaries = Map::new();
    let statuses = if let Some(credentials) = state.credential_service() {
        credentials.status().await
    } else {
        Vec::new()
    };
    let Some(gateway) = state.model_gateway() else {
        return Json(json!({"providers": summaries})).into_response();
    };
    let bindings = gateway.configured_routes();
    for (id, protocol, endpoint) in gateway.configured_provider_views() {
        let raw = providers.get(&id).cloned().unwrap_or_else(|| json!({}));
        let entries = raw.get("models").cloned().unwrap_or_else(|| json!([]));
        let Some(entries) = entries.as_array() else {
            return failure(format!("provider {id} models must be an array"));
        };
        let mut models = Vec::new();
        for entry in entries {
            let Some(model_id) = entry
                .as_str()
                .or_else(|| entry.get("id").and_then(Value::as_str))
                .filter(|id| !id.is_empty())
            else {
                return failure(format!("provider {id} has malformed model entry"));
            };
            models.push(json!({"id": model_id, "name": entry.get("name").and_then(Value::as_str).unwrap_or(model_id)}));
        }
        for binding in bindings.values().filter(|binding| binding.provider == id) {
            if !models.iter().any(|model| model["id"] == binding.model) {
                models.push(json!({"id": binding.model, "name": binding.model}));
            }
        }
        let credential = statuses.iter().find(|status| status.provider == id);
        let kind = credential
            .map(|status| status.kind)
            .unwrap_or("unavailable");
        let credential_state = credential
            .map(|status| status.state.as_str())
            .unwrap_or("unavailable");
        let has_credentials = credential_state == "ready";
        // 凭证状态来自现役凭证管理器；OAuth、无密钥与撤销不能冒充普通 API key。
        summaries.insert(id.clone(), json!({"display_name": raw.get("display_name").and_then(Value::as_str).unwrap_or(&id),
            "base_url": endpoint, "api": protocol, "api_key": if has_credentials && kind == "apiKey" { "***" } else { "" },
            "models": models, "has_credentials": has_credentials, "type": kind, "auth_type": kind,
            "credential_state": credential_state, "loggedIn": credential.is_some_and(|status| status.logged_in)}));
    }
    Json(json!({"providers": summaries})).into_response()
}

pub async fn user_profile_route(State(state): State<ServiceState>) -> Response {
    match crate::settings_configuration::read_user_profile(&state) {
        Ok(content) => Json(json!({"content":content})).into_response(),
        Err(e) => failure(e),
    }
}

pub async fn config_route(
    State(state): State<ServiceState>,
    axum::Extension(principal): axum::Extension<crate::auth::Principal>,
) -> Response {
    crate::settings_configuration::config_get(&state, &principal).await
}

pub async fn avatars_health_route(State(state): State<ServiceState>) -> Response {
    // 用户头像的新写入路径与现役角色头像处理器保持一致。
    for extension in ["png", "jpg", "jpeg", "webp"] {
        let path = state
            .memory()
            .user_dir()
            .join("avatars")
            .join(format!("user.{extension}"));
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_file() => {
                return Json(json!({"avatars": {"user": true}})).into_response()
            }
            Ok(_) => return failure("user avatar must be a regular file"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return failure(format!("cannot inspect user avatar: {error}")),
        }
    }
    Json(json!({"avatars": {"user": false}})).into_response()
}

async fn response_value(response: Response) -> Result<Value, Box<Response>> {
    if !response.status().is_success() {
        return Err(Box::new(response));
    }
    let bytes = match axum::body::to_bytes(response.into_body(), 16 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(error) => {
            return Err(Box::new(failure(format!(
                "cannot read settings response: {error}"
            ))))
        }
    };
    serde_json::from_slice(&bytes)
        .map_err(|error| Box::new(failure(format!("invalid settings response: {error}"))))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotQuery {
    agent_id: String,
}

pub async fn snapshot_route(
    State(state): State<ServiceState>,
    Query(query): Query<SnapshotQuery>,
    axum::Extension(principal): axum::Extension<crate::auth::Principal>,
) -> Response {
    let id = query.agent_id;
    // 聚合现役处理器，不另造人格回退链或经验读取规则；任一路失败，快照显式失败。
    let config = match response_value(
        crate::agents_admin::agent_config_readback::agent_config_get_route(
            State(state.clone()),
            RoutePath(id.clone()),
            axum::Extension(principal.clone()),
        )
        .await,
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let identity = match response_value(
        crate::persona::identity_get_route(State(state.clone()), RoutePath(id.clone())).await,
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let agents = match response_value(
        crate::persona::agents_md_get_route(State(state.clone()), RoutePath(id.clone())).await,
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let public_agents = match response_value(
        crate::persona::public_agents_md_get_route(State(state.clone()), RoutePath(id.clone()))
            .await,
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let user = match response_value(user_profile_route(State(state.clone())).await).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let experience = if config["experience"]["enabled"] == true {
        match response_value(
            crate::memory::experience_get_route(State(state.clone()), RoutePath(id.clone())).await,
        )
        .await
        {
            Ok(value) => value["content"].clone(),
            Err(response) => return *response,
        }
    } else {
        json!("")
    };
    let global_models = match model_preferences(&state) {
        Ok(value) => value,
        Err(error) => return failure(error),
    };
    let preferences = match state.memory().read_preferences_root() {
        Ok(value) => value,
        Err(error) => return failure(error),
    };
    // 快照只投影现役界面需要的偏好；兼容未知字段保留在磁盘，不默认公开。
    let mut preferences = Value::Object(
        [
            "quickChat",
            "notifications",
            "keybindings",
            "browser",
            "bridge",
            "speechRecognition",
            "appearance",
            "network_proxy",
            "modelObservability",
        ]
        .into_iter()
        .filter_map(|key| {
            preferences
                .get(key)
                .map(|value| (key.into(), value.clone()))
        })
        .collect(),
    );
    let computer = match crate::computer_use_settings::projection(&state) {
        Ok(value) => value,
        Err(error) => return failure(error),
    };
    preferences["computerUse"] = computer;
    let definitions =
        crate::agents_admin::agent_config_readback::experiments_list_route(State(state.clone()))
            .await;
    let experiments = match response_value(definitions).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    preferences["experiments"] = experiments["experiments"].clone();
    let preferences = crate::agents_admin::agent_config_readback::mask_object_secrets(&preferences);
    Json(json!({"agentId": id, "config": config, "identity": identity["content"], "agents": agents["content"], "publicAgents": public_agents["content"], "userProfile": user["content"], "experience": experience, "globalModels": global_models, "preferences": preferences})).into_response()
}
