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
    let mut operation_models: Vec<Value> = Vec::new();
    if let Some(gateway) = state.model_gateway() {
        for (operation, binding) in gateway.configured_routes() {
            models.insert(
                operation.clone(),
                json!({"provider": binding.provider, "id": binding.model}),
            );
            if let Some(existing) = operation_models
                .iter_mut()
                .find(|m| m["provider"] == binding.provider && m["id"] == binding.model)
            {
                existing["operations"]
                    .as_array_mut()
                    .expect("操作列表")
                    .push(json!(operation));
            } else {
                operation_models.push(json!({"id": binding.model, "provider": binding.provider, "label": binding.model, "operations": [operation]}));
            }
        }
    }
    Ok(
        json!({"models": models, "operation_models": operation_models,
        "thinking_level": preferences.get("thinking_level").cloned().unwrap_or(Value::Null)}),
    )
}

pub async fn preferences_models_route(State(state): State<ServiceState>) -> Response {
    match model_preferences(&state) {
        Ok(value) => Json(value).into_response(),
        Err(error) => failure(error),
    }
}

pub async fn models_route(State(state): State<ServiceState>) -> Response {
    let providers = match crate::agents_admin::agent_config_readback::merged_catalog_providers_with(
        state.memory().home(),
        false,
    ) {
        Ok(value) => value,
        Err(error) => return failure(error),
    };
    let mut models: Vec<Value> = Vec::new();
    let runtime_providers = state
        .model_gateway()
        .map(|gateway| gateway.provider_ids())
        .unwrap_or_default();
    for (provider, raw) in providers {
        if !runtime_providers.contains(&provider) {
            continue;
        }
        if let Some(entries) = raw.get("models").and_then(Value::as_array) {
            for entry in entries {
                let id = entry
                    .as_str()
                    .or_else(|| entry.get("id").and_then(Value::as_str));
                let Some(id) = id.filter(|id| !id.is_empty()) else {
                    return failure(format!("provider {provider} has malformed model entry"));
                };
                // 只公开目录显示字段；元数据中的密钥字段永不穿过此读取出口。
                models.push(json!({"id": id, "provider": provider,
                    "name": entry.get("name").and_then(Value::as_str).unwrap_or(id),
                    "contextWindow": entry.get("contextWindow"), "input": entry.get("input")}));
            }
        }
    }
    if let Some(gateway) = state.model_gateway() {
        for (purpose, binding) in gateway.configured_routes() {
            if [
                "embedding",
                "rerank",
                "image",
                "video",
                "speech",
                "speech_recognition",
            ]
            .contains(&purpose.as_str())
            {
                continue;
            }
            if !models
                .iter()
                .any(|m| m["provider"] == binding.provider && m["id"] == binding.model)
            {
                models.push(json!({"id": binding.model, "provider": binding.provider, "name": binding.model}));
            }
        }
    }
    Json(json!({"models": models})).into_response()
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
    let path = state.memory().user_dir().join("user.md");
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            return failure("user profile must be a regular file")
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Json(json!({"content": ""})).into_response()
        }
        Err(error) => return failure(format!("cannot inspect user profile: {error}")),
    }
    match std::fs::read_to_string(path) {
        Ok(content) => Json(json!({"content": content})).into_response(),
        Err(error) => failure(format!("cannot read user profile: {error}")),
    }
}

pub async fn config_route(State(state): State<ServiceState>) -> Response {
    match state.memory().read_preferences_root() {
        Ok(preferences) => Json(
            json!({"locale": preferences.get("locale").and_then(Value::as_str).unwrap_or("zh-CN")}),
        )
        .into_response(),
        Err(error) => failure(error),
    }
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
) -> Response {
    let id = query.agent_id;
    if let Err(error) = crate::agents_admin::agent_config_readback::merged_catalog_providers_with(
        state.memory().home(),
        false,
    ) {
        return failure(error);
    }
    // 聚合现役处理器，不另造人格回退链或经验读取规则；任一路失败，快照显式失败。
    let config = match response_value(
        crate::agents_admin::agent_config_readback::agent_config_get_route(
            State(state.clone()),
            RoutePath(id.clone()),
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
    Json(json!({"agentId": id, "config": config, "identity": identity["content"], "agents": agents["content"], "publicAgents": public_agents["content"], "userProfile": user["content"], "experience": experience, "globalModels": global_models, "preferences": preferences})).into_response()
}
