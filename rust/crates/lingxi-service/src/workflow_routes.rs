//! 工作流活动快照与实时广播共用同一持久记录，重连不借用当前焦点归属。
use crate::{Principal, ServiceState};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Extension, Json,
};

pub async fn activities_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path(session): Path<String>,
) -> Response {
    if let Err(response) = crate::gate_session_owned(&state, &principal, &session).await {
        return response;
    }
    match state.workflows().activities_for_session(&session) {
        Ok(entries) => match entries
            .into_iter()
            .map(serde_json::from_value::<lingxi_protocol::AgentActivityEntry>)
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(entries) => Json(lingxi_protocol::AgentActivitySnapshot { entries }).into_response(),
            Err(error) => crate::EndpointError::new(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                lingxi_protocol::ErrorCode::Internal,
                format!("invalid activity receipt: {error}"),
            )
            .into_response(),
        },
        Err(message) => crate::EndpointError::new(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            lingxi_protocol::ErrorCode::Internal,
            message,
        )
        .into_response(),
    }
}
