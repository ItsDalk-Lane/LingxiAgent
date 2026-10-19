//! 频道、只读私聊与电话设置的 HTTP 边界；身份与业务事实交由正式 ConversationApi 复核。
use crate::{conversations::ConversationApi, Principal, ServiceState};
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};

const MAX_WRITE_BYTES: usize = 128 * 1024;

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConversationQuery {
    agent_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(dead_code)] // 仅校验入站字段，原始 JSON 保留缺省与显式 null 的区别。
struct CreateChannel {
    name: String,
    members: Vec<String>,
    description: Option<String>,
    intro: Option<String>,
    request_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(dead_code)]
struct ChannelMessage {
    body: String,
    request_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(dead_code)]
struct ChannelMember {
    member_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct ReadBookmark {
    timestamp: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct ToggleChannels {
    enabled: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct PhoneMode {
    mode: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct PhoneModel {
    id: String,
    provider: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(dead_code)]
struct PhoneSettings {
    mode: Option<String>,
    // 现役字段允许 null 与数值归一；具体范围由业务入口统一校验。
    reply_min_chars: Option<Value>,
    reply_max_chars: Option<Value>,
    proactive_enabled: Option<bool>,
    reminder_interval_minutes: Option<Value>,
    guard_limit: Option<Value>,
    model_override_enabled: Option<bool>,
    model_override_model: Option<PhoneModel>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyWrite {}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(dead_code)]
struct ResetWrite {
    request_id: Option<String>,
}

fn failure(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": message.into() }))).into_response()
}
fn checked_payload<T: DeserializeOwned>(body: &[u8]) -> Result<Value, Box<Response>> {
    if body.len() > MAX_WRITE_BYTES {
        return Err(Box::new(failure(
            StatusCode::PAYLOAD_TOO_LARGE,
            "conversation payload exceeds budget",
        )));
    }
    let payload: Value = if body.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(body)
            .map_err(|_| Box::new(failure(StatusCode::BAD_REQUEST, "invalid JSON")))?
    };
    serde_json::from_value::<T>(payload.clone()).map_err(|error| {
        Box::new(failure(
            StatusCode::BAD_REQUEST,
            format!("invalid conversation payload: {error}"),
        ))
    })?;
    Ok(payload)
}

async fn request(
    state: &ServiceState,
    principal: &Principal,
    action: &str,
    id: Option<&str>,
    peer: Option<&str>,
    agent: Option<&str>,
    payload: Value,
) -> Response {
    match ConversationApi::from_state(state)
        .request(principal, action, id, peer, agent, payload)
        .await
    {
        Ok(value) => Json(value).into_response(),
        Err(error) => {
            let status = StatusCode::from_u16(error.status)
                .ok()
                .filter(|status| status.is_client_error() || status.is_server_error())
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            failure(status, error.message)
        }
    }
}
async fn write<T: DeserializeOwned>(
    state: &ServiceState,
    principal: &Principal,
    action: &str,
    id: Option<&str>,
    peer: Option<&str>,
    agent: Option<&str>,
    body: Bytes,
) -> Response {
    match checked_payload::<T>(&body) {
        Ok(payload) => request(state, principal, action, id, peer, agent, payload).await,
        Err(error) => *error,
    }
}

macro_rules! channel_read {
    ($name:ident, $action:literal) => {
        async fn $name(
            State(state): State<ServiceState>,
            Extension(principal): Extension<Principal>,
            Path(id): Path<String>,
            Query(query): Query<ConversationQuery>,
        ) -> Response {
            request(
                &state,
                &principal,
                $action,
                Some(&id),
                None,
                query.agent_id.as_deref(),
                json!({}),
            )
            .await
        }
    };
}
macro_rules! channel_write {
    ($name:ident, $action:literal, $payload:ty) => {
        async fn $name(
            State(state): State<ServiceState>,
            Extension(principal): Extension<Principal>,
            Path(id): Path<String>,
            Query(query): Query<ConversationQuery>,
            body: Bytes,
        ) -> Response {
            write::<$payload>(
                &state,
                &principal,
                $action,
                Some(&id),
                None,
                query.agent_id.as_deref(),
                body,
            )
            .await
        }
    };
}
channel_read!(channel_read, "channel.read");
channel_read!(phone_settings_get, "phone.settings.get");
channel_read!(phone_mode_get, "phone.mode.get");
channel_read!(phone_activities, "phone.activities");
channel_write!(channel_delete, "channel.delete", EmptyWrite);
channel_write!(channel_post, "channel.post", ChannelMessage);
channel_write!(channel_add_member, "channel.add_member", ChannelMember);
channel_write!(channel_read_mark, "channel.read_mark", ReadBookmark);
channel_write!(phone_settings_set, "phone.settings.set", PhoneSettings);
channel_write!(phone_mode_set, "phone.mode.set", PhoneMode);

async fn channel_list(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<ConversationQuery>,
) -> Response {
    request(
        &state,
        &principal,
        "channel.list",
        None,
        None,
        query.agent_id.as_deref(),
        json!({}),
    )
    .await
}
async fn channel_create(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<ConversationQuery>,
    body: Bytes,
) -> Response {
    write::<CreateChannel>(
        &state,
        &principal,
        "channel.create",
        None,
        None,
        query.agent_id.as_deref(),
        body,
    )
    .await
}
async fn channel_toggle(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<ConversationQuery>,
    body: Bytes,
) -> Response {
    write::<ToggleChannels>(
        &state,
        &principal,
        "channel.toggle",
        None,
        None,
        query.agent_id.as_deref(),
        body,
    )
    .await
}
async fn channel_remove_member(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path((id, member)): Path<(String, String)>,
    Query(query): Query<ConversationQuery>,
    body: Bytes,
) -> Response {
    if let Err(error) = checked_payload::<EmptyWrite>(&body) {
        return *error;
    }
    request(
        &state,
        &principal,
        "channel.remove_member",
        Some(&id),
        None,
        query.agent_id.as_deref(),
        json!({"memberId":member}),
    )
    .await
}
async fn dm_list(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<ConversationQuery>,
) -> Response {
    request(
        &state,
        &principal,
        "dm.list",
        None,
        None,
        query.agent_id.as_deref(),
        json!({}),
    )
    .await
}
async fn dm_read(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path(peer): Path<String>,
    Query(query): Query<ConversationQuery>,
) -> Response {
    request(
        &state,
        &principal,
        "dm.read",
        Some(&format!("dm:{peer}")),
        Some(&peer),
        query.agent_id.as_deref(),
        json!({}),
    )
    .await
}
async fn dm_reset(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path(peer): Path<String>,
    Query(query): Query<ConversationQuery>,
    body: Bytes,
) -> Response {
    write::<ResetWrite>(
        &state,
        &principal,
        "dm.reset",
        Some(&format!("dm:{peer}")),
        Some(&peer),
        query.agent_id.as_deref(),
        body,
    )
    .await
}

fn markdown_export(value: Value) -> Response {
    let Some(content) = value.get("content").and_then(Value::as_str) else {
        return failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "conversation export content missing",
        );
    };
    let Some(filename) = value.get("filename").and_then(Value::as_str) else {
        return failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "conversation export filename missing",
        );
    };
    if filename.is_empty()
        || filename.len() > 1024
        || filename
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\')
    {
        return failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid conversation export filename",
        );
    }
    let disposition = format!(
        "attachment; filename=\"hana-chat-archive.md\"; filename*=UTF-8''{}",
        utf8_percent_encode(filename, NON_ALPHANUMERIC)
    );
    let Ok(disposition) = HeaderValue::from_str(&disposition) else {
        return failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid conversation export header",
        );
    };
    let mut response = content.to_owned().into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/markdown; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(header::CONTENT_DISPOSITION, disposition);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
}
async fn conversation_export(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
    Query(query): Query<ConversationQuery>,
) -> Response {
    match ConversationApi::from_state(&state)
        .request(
            &principal,
            "conversation.export",
            Some(&id),
            None,
            query.agent_id.as_deref(),
            json!({}),
        )
        .await
    {
        Ok(value) => markdown_export(value),
        Err(error) => failure(
            StatusCode::from_u16(error.status)
                .ok()
                .filter(|status| status.is_client_error() || status.is_server_error())
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            error.message,
        ),
    }
}

/// 合并后仍由统一认证中间件和 ConversationApi 校验真实所有者与会话归属。
pub fn router() -> Router<ServiceState> {
    Router::new()
        .route(
            "/lingxi/v1/channels",
            get(channel_list).post(channel_create),
        )
        .route("/lingxi/v1/channels/toggle", post(channel_toggle))
        .route(
            "/lingxi/v1/channels/{id}",
            get(channel_read).delete(channel_delete),
        )
        .route("/lingxi/v1/channels/{id}/members", post(channel_add_member))
        .route(
            "/lingxi/v1/channels/{id}/members/{member}",
            axum::routing::delete(channel_remove_member),
        )
        .route("/lingxi/v1/channels/{id}/messages", post(channel_post))
        .route("/lingxi/v1/channels/{id}/read", post(channel_read_mark))
        .route("/lingxi/v1/dm", get(dm_list))
        .route("/lingxi/v1/dm/{peer}", get(dm_read))
        .route("/lingxi/v1/dm/{peer}/reset", post(dm_reset))
        .route(
            "/lingxi/v1/conversations/{id}/agent-phone-settings",
            get(phone_settings_get).post(phone_settings_set),
        )
        .route(
            "/lingxi/v1/conversations/{id}/agent-phone-tool-mode",
            get(phone_mode_get).post(phone_mode_set),
        )
        .route(
            "/lingxi/v1/conversations/{id}/agent-activities",
            get(phone_activities),
        )
        .route(
            "/lingxi/v1/conversations/{id}/export",
            get(conversation_export),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_actions_preserve_request_ids_and_explicit_null_settings() {
        let message = json!({"body":"hello","requestId":"client-1"});
        assert_eq!(
            checked_payload::<ChannelMessage>(message.to_string().as_bytes()).unwrap(),
            message
        );
        let settings = json!({"mode":"read_only","replyMinChars":null,"replyMaxChars":80,
            "modelOverrideEnabled":false,"modelOverrideModel":null});
        assert_eq!(
            checked_payload::<PhoneSettings>(settings.to_string().as_bytes()).unwrap(),
            settings
        );
        assert!(checked_payload::<EmptyWrite>(&[]).is_ok());
    }
    #[test]
    fn unknown_identity_fields_are_rejected_on_every_write_shape() {
        assert!(checked_payload::<CreateChannel>(
            br#"{"name":"n","members":["a","b"],"ownerSubject":"other"}"#
        )
        .is_err());
        assert!(
            checked_payload::<ChannelMessage>(br#"{"body":"hello","sender":"owner"}"#).is_err()
        );
        assert!(checked_payload::<ChannelMember>(br#"{"memberId":"a","role":"owner"}"#).is_err());
        assert!(
            checked_payload::<ReadBookmark>(br#"{"timestamp":"now","agentId":"other"}"#).is_err()
        );
        assert!(
            checked_payload::<ToggleChannels>(br#"{"enabled":true,"principal":"owner"}"#).is_err()
        );
        assert!(checked_payload::<PhoneMode>(br#"{"mode":"write","owner":true}"#).is_err());
        assert!(checked_payload::<PhoneSettings>(br#"{"mode":"write","owner":true}"#).is_err());
        assert!(checked_payload::<PhoneSettings>(
            br#"{"modelOverrideModel":{"id":"m","provider":"p","apiKey":"untrusted"}}"#
        )
        .is_err());
        assert!(checked_payload::<EmptyWrite>(br#"{"owner":true}"#).is_err());
    }
    #[test]
    fn invalid_json_types_and_write_budget_fail_explicitly() {
        assert!(checked_payload::<ChannelMessage>(b"not json").is_err());
        assert!(checked_payload::<ChannelMessage>(br#"{"body":3}"#).is_err());
        assert!(checked_payload::<ToggleChannels>(br#"{"enabled":"true"}"#).is_err());
        assert!(checked_payload::<EmptyWrite>(&vec![b' '; MAX_WRITE_BYTES + 1]).is_err());
    }
    #[test]
    fn export_is_real_markdown_with_safe_download_headers() {
        let response = markdown_export(json!({"content":"# 聊天\n","filename":"频道记录.md"}));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/markdown; charset=utf-8"
        );
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "private, no-store"
        );
        let disposition = response.headers()[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap();
        assert!(disposition.starts_with("attachment;"));
        assert!(disposition.contains("filename*=UTF-8''%E9%A2%91"));
    }
    #[test]
    fn corrupt_exports_cannot_inject_headers_or_fake_an_empty_download() {
        for value in [
            json!({"filename":"ok.md"}),
            json!({"content":""}),
            json!({"content":"x","filename":"x\r\nInjected: yes"}),
            json!({"content":"x","filename":"../secret.md"}),
        ] {
            assert_eq!(
                markdown_export(value).status(),
                StatusCode::INTERNAL_SERVER_ERROR
            );
        }
    }
}
