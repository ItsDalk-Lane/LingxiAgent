//! 媒体读面按真实会话归属返回同一持久回执；取消不接受正文声明的身份。
use crate::{EndpointError, Principal, ServiceState};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use serde_json::json;
async fn owner(
    state: &ServiceState,
    principal: &Principal,
    sid: &str,
) -> Result<String, Box<Response>> {
    match state.sessions().get_for(principal, sid).await {
        Ok(crate::sessions::SessionAccess::Ok(_)) => {}
        Ok(crate::sessions::SessionAccess::NotFound) => {
            return Err(Box::new(EndpointError::not_found().into_response()))
        }
        Ok(crate::sessions::SessionAccess::Forbidden) => {
            return Err(Box::new(
                EndpointError::forbidden("cross_principal_access").into_response(),
            ))
        }
        Err(error) => return Err(Box::new(EndpointError::storage(&error).into_response())),
    }
    let session = lingxi_adapters::storage::session_tree::get_session_row(state.storage(), sid)
        .await
        .map_err(|error| Box::new(EndpointError::storage(&error).into_response()))?
        .ok_or_else(|| Box::new(EndpointError::not_found().into_response()))?;
    if session.lifecycle == "deleted" {
        return Err(Box::new(EndpointError::not_found().into_response()));
    }
    Ok(session.owner_user_id)
}
async fn list(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path(sid): Path<String>,
) -> Response {
    let owner = match owner(&state, &principal, &sid).await {
        Ok(owner) => owner,
        Err(error) => return *error,
    };
    match state.media().list_owned(&sid, &owner).await {
        Ok(jobs) => Json(json!({"jobs":jobs})).into_response(),
        Err(message) => EndpointError::invalid_message(message).into_response(),
    }
}
async fn read(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path((sid, id)): Path<(String, String)>,
) -> Response {
    let owner = match owner(&state, &principal, &sid).await {
        Ok(owner) => owner,
        Err(error) => return *error,
    };
    match state.media().get_owned(&id, &sid, &owner).await {
        Ok(Some(job)) => Json(json!({"job":job})).into_response(),
        Ok(None) => EndpointError::not_found().into_response(),
        Err(message) => EndpointError::invalid_message(message).into_response(),
    }
}
async fn cancel(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path((sid, id)): Path<(String, String)>,
    Json(_): Json<lingxi_protocol::MediaJobCancelRequest>,
) -> Response {
    let owner = match owner(&state, &principal, &sid).await {
        Ok(owner) => owner,
        Err(error) => return *error,
    };
    if !principal.is_local_owner() && principal.user_id.as_deref() != Some(owner.as_str()) {
        return EndpointError::forbidden("media_cancel_requires_actual_owner").into_response();
    }
    match state.media().cancel_owned(&id, &sid, &owner).await {
        Ok(job) => Json(json!({"job":job})).into_response(),
        Err(message) => EndpointError::invalid_message(message).into_response(),
    }
}
pub fn router() -> Router<ServiceState> {
    Router::new()
        .route("/lingxi/v1/sessions/{id}/media/jobs", get(list))
        .route("/lingxi/v1/sessions/{id}/media/jobs/{taskId}", get(read))
        .route(
            "/lingxi/v1/sessions/{id}/media/jobs/{taskId}/cancel",
            post(cancel),
        )
}
