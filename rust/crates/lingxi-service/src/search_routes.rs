//! 搜索设置仅返回掩码；验证成功后才合并并原子保存偏好。
use crate::{credentials::search, Principal, ServiceState};
use axum::{
    extract::{rejection::JsonRejection, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyRequest {
    pub provider: String,
    #[serde(default)]
    pub api_key: String,
    pub search_provider: Option<String>,
}
fn owner(principal: &Principal) -> Result<(), Box<Response>> {
    if principal.is_local_owner() {
        Ok(())
    } else {
        Err(Box::new(
            (
                StatusCode::FORBIDDEN,
                Json(json!({"ok":false,"error":"search settings require the local studio owner"})),
            )
                .into_response(),
        ))
    }
}
pub async fn settings(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
) -> Response {
    if let Err(error) = owner(&principal) {
        return *error;
    }
    match state.web_tools().settings() {
        Ok(value) => Json(value).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok":false,"error":error.message,"code":error.code})),
        )
            .into_response(),
    }
}
pub async fn verify(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    body: Result<Json<VerifyRequest>, JsonRejection>,
) -> Response {
    if let Err(error) = owner(&principal) {
        return *error;
    }
    let Ok(Json(body)) = body else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok":false,"error":"invalid search verification request"})),
        )
            .into_response();
    };
    let provider = body.provider.trim();
    let selected = body.search_provider.as_deref().unwrap_or(provider).trim();
    if !search::known(provider)
        || !search::known(selected)
        || body.api_key.len() > 8192
        || search::PAID.contains(&provider) && body.api_key.trim().is_empty()
    {
        return(StatusCode::BAD_REQUEST,Json(json!({"ok":false,"error":"known provider, selected provider and required api_key are required"}))).into_response();
    }
    match state.web_tools().verify(provider,&body.api_key,selected).await{Ok(value)=>Json(value).into_response(),Err(error)=>Json(json!({"ok":false,"error":error.message,"code":error.code,"retryAfterMs":error.retry_after_ms})).into_response()}
}
