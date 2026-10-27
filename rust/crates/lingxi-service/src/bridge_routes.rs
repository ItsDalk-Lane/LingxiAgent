//! 管理页面沿旧字段含义接入持久 Bridge 配置；读取只返回掩码。
use crate::ServiceState;
use axum::{
    extract::{Query, State},
    response::{IntoResponse, Response},
    Json,
};
use lingxi_adapters::storage::{agent_memory::config, bridge as store};
use serde::Deserialize;
use serde_json::{json, Value};

pub const PLATFORMS: [&str; 5] = ["telegram", "feishu", "dingtalk", "qq", "wechat"];
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentQuery {
    agent_id: String,
    platform: Option<String>,
    limit: Option<usize>,
}
fn failure(error: impl ToString) -> Response {
    (
        axum::http::StatusCode::BAD_REQUEST,
        Json(json!({"ok":false,"error":error.to_string()})),
    )
        .into_response()
}
fn connector_id(state: &ServiceState, actor: &str, platform: &str) -> String {
    format!(
        "connector-{}",
        lingxi_kernel::session_tree::sha256_hex(
            json!([state.management.identity_ids().1, actor, platform])
                .to_string()
                .as_bytes()
        )
    )
}
pub(crate) fn credentials(platform: &str) -> &'static [&'static str] {
    match platform {
        "telegram" => &["token"],
        "feishu" => &["appId", "appSecret", "region"],
        "dingtalk" => &[
            "corpId",
            "clientId",
            "clientSecret",
            "robotCode",
            "apiBaseUrl",
            "authMode",
            "appKey",
            "appSecret",
            "restBaseUrl",
            "streamOpenUrl",
        ],
        "qq" => &["appID", "appId", "appSecret", "secret"],
        "wechat" => &["botToken", "token", "baseUrl", "apiBaseUrl", "accountId"],
        _ => &[],
    }
}
fn secret(key: &str) -> bool {
    matches!(
        key,
        "token" | "botToken" | "appSecret" | "clientSecret" | "secret"
    )
}
fn masked(value: &str) -> bool {
    value.starts_with("***") || value.starts_with('•')
}
fn credential_aliases(platform: &str) -> &'static [(&'static str, &'static str)] {
    match platform {
        "dingtalk" => &[
            ("appKey", "clientId"),
            ("appSecret", "clientSecret"),
            ("restBaseUrl", "apiBaseUrl"),
        ],
        "qq" => &[("appId", "appID"), ("secret", "appSecret")],
        "wechat" => &[("token", "botToken"), ("apiBaseUrl", "baseUrl")],
        _ => &[],
    }
}
fn canonical_credentials(platform: &str, source: &Value) -> Result<Value, String> {
    let mut fields = source
        .as_object()
        .ok_or("Bridge credentials must be an object")?
        .clone();
    for (alias, canonical) in credential_aliases(platform) {
        if let Some(value) = fields.remove(*alias) {
            // 同一份输入中正式名称优先，包括空串和显式清除，不能复活旧凭据。
            fields.entry((*canonical).to_owned()).or_insert(value);
        }
    }
    Ok(Value::Object(fields))
}
#[derive(Clone, Copy)]
enum CredentialPatchUse {
    Save,
    Verify { use_saved_credentials: bool },
}
fn merge_credentials(
    platform: &str,
    stored: &Value,
    patch: Option<&Value>,
    usage: CredentialPatchUse,
) -> Result<Value, String> {
    let mut merged = canonical_credentials(platform, stored)?;
    let Some(patch) = patch else {
        return Ok(merged);
    };
    let fields = patch
        .as_object()
        .ok_or("Bridge credentials must be an object")?;
    for (key, value) in fields {
        if !credentials(platform).contains(&key.as_str()) {
            return Err(format!("unsupported {platform} credential field"));
        }
        if !value.is_string() && !(matches!(usage, CredentialPatchUse::Save) && value.is_null()) {
            return Err("Bridge credential fields must be strings".into());
        }
        if value.as_str().is_some_and(|s| s.len() > 16 * 1024) {
            return Err("Bridge credential exceeds budget".into());
        }
    }
    // 两份输入分开归一后再合并：新别名必须覆盖已存正式名称，而非被旧值盖过。
    let normalized = canonical_credentials(platform, patch)?;
    let object = merged
        .as_object_mut()
        .ok_or("Bridge configuration must be an object")?;
    for (key, value) in normalized
        .as_object()
        .ok_or("Bridge credentials must be an object")?
    {
        if secret(key) && value.as_str().is_some_and(masked) {
            if matches!(
                usage,
                CredentialPatchUse::Verify {
                    use_saved_credentials: false
                }
            ) {
                return Err("masked credentials require useSavedCredentials=true".into());
            }
            if !object
                .get(key)
                .and_then(Value::as_str)
                .is_some_and(|s| !s.trim().is_empty() && !masked(s))
            {
                return Err("masked credential has no stored value".into());
            }
            continue;
        }
        if value.is_null() {
            object.remove(key);
        } else {
            object.insert(key.to_owned(), value.clone());
        }
    }
    Ok(merged)
}
fn actor_exists(state: &ServiceState, actor: &str) -> Result<(), String> {
    state
        .memory()
        .agent_dir_checked(actor)
        .map_err(|e| e.message)?;
    if state
        .memory()
        .is_agent_deleted_checked(actor)
        .map_err(|e| format!("{e:?}"))?
    {
        return Err("agent is deleted".into());
    }
    Ok(())
}
async fn old_config(state: &ServiceState, actor: &str, platform: &str) -> Result<Value, String> {
    let dir = state
        .memory()
        .agent_dir_checked(actor)
        .map_err(|e| e.message)?;
    let value = config::read_yaml_mapping_as_json(&dir.join("config.yaml"))
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| json!({}));
    Ok(value
        .get("bridge")
        .and_then(|row| row.get(platform))
        .filter(|row| row.is_object())
        .cloned()
        .unwrap_or_else(|| json!({})))
}
async fn configured(
    state: &ServiceState,
    actor: &str,
    platform: &str,
) -> Result<(Option<store::BridgeConnector>, Value), String> {
    let existing = store::get_connector(state.storage(), &connector_id(state, actor, platform))
        .await
        .map_err(|e| e.to_string())?;
    let config = match &existing {
        Some(row) => serde_json::from_str(&row.input.config_json)
            .map_err(|_| "Bridge configuration damaged".to_string())?,
        None => old_config(state, actor, platform).await?,
    };
    Ok((existing, config))
}
fn permission(state: &ServiceState) -> Result<Value, String> {
    let root = state.memory().read_preferences_root()?;
    let mode = crate::bridge_permission::parse(&root)?.wire_name();
    Ok(
        json!({"permissionMode":mode,"readOnly":mode=="read_only","receiptEnabled":root.pointer("/bridge/receiptEnabled")!=Some(&Value::Bool(false)),"richStreamingEnabled":root.pointer("/bridge/richStreamingEnabled")!=Some(&Value::Bool(false))}),
    )
}
pub(crate) async fn status(state: &ServiceState, actor: &str) -> Result<Value, String> {
    actor_exists(state, actor)?;
    let mut value = permission(state)?;
    value["agentId"] = json!(actor);
    value["owner"] = json!({});
    value["knownUsers"] = json!({});
    value["bridgeReady"] = json!(true);
    value["bridgeInitializing"] = json!(false);
    value["bridgeError"] = Value::Null;
    for platform in PLATFORMS {
        let (existing, config) = configured(state, actor, platform).await?;
        let mut item = state
            .bridge()
            .status_value(&connector_id(state, actor, platform));
        item["agentId"] = json!(actor);
        item["enabled"] = json!(existing
            .as_ref()
            .map(|row| row.input.enabled)
            .unwrap_or(config["enabled"] == true));
        let present = |keys: &[&str]| {
            keys.iter().any(|key| {
                config[*key]
                    .as_str()
                    .is_some_and(|s| !s.trim().is_empty() && !masked(s))
            })
        };
        item["configured"] = json!(match platform {
            "telegram" => present(&["token"]),
            "wechat" => present(&["botToken", "token"]),
            "feishu" => present(&["appId"]) && present(&["appSecret"]),
            "dingtalk" =>
                present(&["clientId", "appKey"]) && present(&["clientSecret", "appSecret"]),
            "qq" => present(&["appID", "appId"]) && present(&["appSecret", "secret"]),
            _ => false,
        });
        for key in credentials(platform) {
            if let Some(field) = config.get(*key) {
                if secret(key) {
                    let present = field.as_str().is_some_and(|s| !s.is_empty());
                    item[*key] = json!(if present { "********" } else { "" });
                    item[match *key {
                        "token" => "hasToken",
                        "botToken" => "hasBotToken",
                        "clientSecret" => "hasClientSecret",
                        _ => "hasAppSecret",
                    }] = json!(present);
                } else {
                    item[*key] = field.clone();
                }
            }
        }
        if platform == "wechat" {
            // 保留现有设置页的已登录含义，公开值始终为掩码。
            let present = present(&["botToken", "token"]);
            item["hasToken"] = json!(present);
            item["token"] = json!(if present { "********" } else { "" });
        }
        value[platform] = item;
        value["owner"][platform] = json!(existing
            .and_then(|row| row.input.owner_sender_id)
            .or_else(|| config["owner"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_string)));
        value["knownUsers"][platform] = json!([]);
    }
    value["whatsapp"] = json!({"status":"unsupported","enabled":false,"error":"not part of the five-platform runtime"});
    // 已识别发送者只展示公开身份，不返回平台回复令牌或原始正文。
    let users = lingxi_adapters::storage::bridge_reads::known_senders(state.storage(), actor)
        .await
        .map_err(|e| e.to_string())?;
    for user in users {
        if let Some(users) = value["knownUsers"][&user.platform].as_array_mut() {
            users.push(json!({"userId":user.user_id,"displayName":user.user_id}));
        }
    }
    Ok(value)
}
pub async fn status_route(
    State(state): State<ServiceState>,
    Query(query): Query<AgentQuery>,
) -> Response {
    match status(&state, &query.agent_id).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => failure(error),
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigRequest {
    platform: String,
    credentials: Option<Value>,
    enabled: Option<bool>,
}
async fn save(
    state: &ServiceState,
    actor: &str,
    platform: &str,
    patch: Option<Value>,
    enabled: Option<bool>,
    owner: Option<Option<String>>,
) -> Result<(), String> {
    actor_exists(state, actor)?;
    if !PLATFORMS.contains(&platform) {
        return Err("invalid platform".into());
    }
    let (existing, config) = configured(state, actor, platform).await?;
    let mut config =
        merge_credentials(platform, &config, patch.as_ref(), CredentialPatchUse::Save)?;
    let object = config
        .as_object_mut()
        .ok_or("Bridge configuration must be an object")?;
    let enabled = enabled.unwrap_or_else(|| {
        existing
            .as_ref()
            .map(|row| row.input.enabled)
            .unwrap_or(object.get("enabled") == Some(&Value::Bool(true)))
    });
    let owner = owner.unwrap_or_else(|| {
        existing
            .as_ref()
            .and_then(|row| row.input.owner_sender_id.clone())
            .or_else(|| {
                object
                    .get("owner")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            })
    });
    object.remove("owner");
    object.remove("enabled");
    if enabled {
        let required = match platform {
            "telegram" => vec!["token"],
            "wechat" => vec!["botToken"],
            "feishu" => vec!["appId", "appSecret"],
            "dingtalk" => vec!["clientId", "clientSecret"],
            "qq" => vec!["appID", "appSecret"],
            _ => vec![],
        };
        for key in required {
            let aliases = match key {
                "botToken" => vec!["botToken", "token"],
                "clientId" => vec!["clientId", "appKey"],
                "clientSecret" => vec!["clientSecret", "appSecret"],
                "appID" => vec!["appID", "appId"],
                "appSecret" if platform == "qq" => vec!["appSecret", "secret"],
                _ => vec![key],
            };
            if !aliases.iter().any(|key| {
                object
                    .get(*key)
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.trim().is_empty() && !masked(s))
            }) {
                return Err(format!("{platform} {key} is required"));
            }
        }
    }
    let input = store::BridgeConnectorInput {
        id: connector_id(state, actor, platform),
        platform: platform.into(),
        studio_id: state.management.identity_ids().1,
        actor_id: actor.into(),
        owner_subject: lingxi_kernel::LOCAL_OWNER_SUBJECT.into(),
        owner_sender_id: owner,
        enabled,
        config_json: config.to_string(),
    };
    store::put_connector(
        state.storage(),
        &input,
        existing.as_ref().map(|row| row.generation),
        state.clock().now_unix_ms(),
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}
pub async fn config_route(
    State(state): State<ServiceState>,
    Query(query): Query<AgentQuery>,
    Json(body): Json<ConfigRequest>,
) -> Response {
    match save(
        &state,
        &query.agent_id,
        &body.platform,
        body.credentials,
        body.enabled,
        None,
    )
    .await
    {
        Ok(()) => match status(&state, &query.agent_id).await {
            Ok(value) => Json(json!({"ok":true,"status":value})).into_response(),
            Err(error) => failure(error),
        },
        Err(error) => failure(error),
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnerRequest {
    platform: String,
    user_id: Option<String>,
}
pub async fn owner_route(
    State(state): State<ServiceState>,
    Query(query): Query<AgentQuery>,
    Json(body): Json<OwnerRequest>,
) -> Response {
    let owner = body.user_id.filter(|s| !s.is_empty());
    match save(
        &state,
        &query.agent_id,
        &body.platform,
        None,
        None,
        Some(owner),
    )
    .await
    {
        Ok(()) => match status(&state, &query.agent_id).await {
            Ok(value) => Json(json!({"ok":true,"status":value})).into_response(),
            Err(error) => failure(error),
        },
        Err(error) => failure(error),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopRequest {
    platform: String,
}
pub async fn stop_route(
    State(state): State<ServiceState>,
    Query(query): Query<AgentQuery>,
    Json(body): Json<StopRequest>,
) -> Response {
    match save(
        &state,
        &query.agent_id,
        &body.platform,
        None,
        Some(false),
        None,
    )
    .await
    {
        Ok(()) => Json(json!({"ok":true})).into_response(),
        Err(error) => failure(error),
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsRequest {
    permission_mode: Option<String>,
    read_only: Option<bool>,
    receipt_enabled: Option<bool>,
    rich_streaming_enabled: Option<bool>,
}
pub async fn settings_route(
    State(state): State<ServiceState>,
    Json(body): Json<SettingsRequest>,
) -> Response {
    let result = (|| -> Result<Value, String> {
        let _guard = crate::agents_admin::preferences_locked();
        let mut root = state.memory().read_preferences_root()?;
        let mode = body.permission_mode.or_else(|| {
            body.read_only.map(|value| {
                if value {
                    "read_only".into()
                } else {
                    "auto".into()
                }
            })
        });
        if let Some(mode) = mode {
            if !matches!(mode.as_str(), "auto" | "read_only" | "operate") {
                return Err("invalid Bridge permission mode".into());
            }
            root["bridge"]["permissionMode"] = json!(mode);
        }
        if let Some(value) = body.receipt_enabled {
            root["bridge"]["receiptEnabled"] = json!(value);
        }
        if let Some(value) = body.rich_streaming_enabled {
            root["bridge"]["richStreamingEnabled"] = json!(value);
        }
        state.memory().write_preferences_root(&root)?;
        permission(&state)
    })();
    match result {
        Ok(mut value) => {
            value["ok"] = json!(true);
            Json(value).into_response()
        }
        Err(error) => failure(error),
    }
}
pub async fn messages_route(
    State(state): State<ServiceState>,
    Query(query): Query<AgentQuery>,
) -> Response {
    if let Err(error) = actor_exists(&state, &query.agent_id) {
        return failure(error);
    }
    match lingxi_adapters::storage::bridge_reads::messages(
        state.storage(),
        &query.agent_id,
        query.limit.unwrap_or(50),
    )
    .await
    {
        Ok(messages) => Json(json!({"messages":messages})).into_response(),
        Err(error) => failure(error),
    }
}
pub async fn sessions_route(
    State(state): State<ServiceState>,
    Query(query): Query<AgentQuery>,
) -> Response {
    if let Err(error) = actor_exists(&state, &query.agent_id) {
        return failure(error);
    }
    match lingxi_adapters::storage::bridge_reads::sessions(
        state.storage(),
        &query.agent_id,
        query.platform.as_deref(),
    )
    .await
    {
        Ok(sessions) => Json(json!({"sessions":sessions})).into_response(),
        Err(error) => failure(error),
    }
}

/// 首次接入只创建尚不存在的连接记录；已有配置、撤权和禁用结果不会被旧文件覆盖。
pub(crate) async fn import_existing(state: &ServiceState) -> Result<(), String> {
    let agents = state
        .memory()
        .list_agent_summaries()
        .map_err(|error| format!("Bridge agent list unavailable: {}", error.message))?;
    for agent in agents {
        let actor = agent["id"]
            .as_str()
            .ok_or("Bridge agent identity missing")?;
        for platform in PLATFORMS {
            if store::get_connector(state.storage(), &connector_id(state, actor, platform))
                .await
                .map_err(|e| e.to_string())?
                .is_some()
            {
                continue;
            }
            let config = old_config(state, actor, platform).await?;
            if config.as_object().is_none_or(|row| row.is_empty()) {
                continue;
            }
            save(state, actor, platform, None, None, None).await?;
        }
    }
    Ok(())
}

pub(crate) fn worker_config(platform: &str, source: &Value, actor: &str) -> Result<Value, String> {
    let source = canonical_credentials(platform, source)?;
    let mut value = json!({"agentId":actor});
    for key in credentials(platform) {
        if let Some(field) = source.get(*key) {
            value[*key] = field.clone();
        }
    }
    Ok(value)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TestRequest {
    platform: String,
    credentials: Option<Value>,
    #[serde(default)]
    use_saved_credentials: bool,
}
pub async fn test_route(
    State(state): State<ServiceState>,
    Query(query): Query<AgentQuery>,
    Json(body): Json<TestRequest>,
) -> Response {
    let result = async {
        actor_exists(&state, &query.agent_id)?;
        if !PLATFORMS.contains(&body.platform.as_str()) {
            return Err("invalid platform".to_string());
        }
        let config = if body.use_saved_credentials {
            configured(&state, &query.agent_id, &body.platform).await?.1
        } else {
            if body.credentials.is_none() {
                return Err("platform and credentials required".into());
            }
            json!({})
        };
        let config = merge_credentials(
            &body.platform,
            &config,
            body.credentials.as_ref(),
            CredentialPatchUse::Verify {
                use_saved_credentials: body.use_saved_credentials,
            },
        )?;
        let config = worker_config(&body.platform, &config, &query.agent_id)?;
        state
            .bridge()
            .diagnostic(lingxi_protocol::BridgeDiagnosticCommand::Verify {
                platform: body.platform,
                config,
            })
            .await
    }
    .await;
    diagnostic_response(result)
}
fn diagnostic_response(result: Result<Value, String>) -> Response {
    match result {
        Ok(mut value) => {
            if let Some(object) = value.as_object_mut() {
                object.remove("kind");
            }
            Json(value).into_response()
        }
        Err(error) => failure(error),
    }
}
pub async fn qrcode_route(State(state): State<ServiceState>) -> Response {
    diagnostic_response(
        state
            .bridge()
            .diagnostic(lingxi_protocol::BridgeDiagnosticCommand::WechatQrcode)
            .await,
    )
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QrcodeStatusRequest {
    qrcode_id: String,
}
pub async fn qrcode_status_route(
    State(state): State<ServiceState>,
    Json(body): Json<QrcodeStatusRequest>,
) -> Response {
    if body.qrcode_id.trim().is_empty()
        || body.qrcode_id.len() > 4096
        || body.qrcode_id.contains('\0')
    {
        return failure("invalid QR code identity");
    }
    diagnostic_response(
        state
            .bridge()
            .diagnostic(
                lingxi_protocol::BridgeDiagnosticCommand::WechatQrcodeStatus {
                    qrcode_id: body.qrcode_id,
                },
            )
            .await,
    )
}

#[cfg(test)]
mod credential_alias_tests {
    use super::*;

    #[test]
    fn new_alias_patch_replaces_saved_canonical_for_save_verify_and_worker() {
        for (platform, alias, canonical) in [
            ("dingtalk", "appKey", "clientId"),
            ("dingtalk", "appSecret", "clientSecret"),
            ("dingtalk", "restBaseUrl", "apiBaseUrl"),
            ("qq", "appId", "appID"),
            ("qq", "secret", "appSecret"),
            ("wechat", "token", "botToken"),
            ("wechat", "apiBaseUrl", "baseUrl"),
        ] {
            let mut stored = json!({"legacyUnknown":{"preserve":true}});
            stored[canonical] = json!("old-value");
            let mut patch = json!({});
            patch[alias] = json!("new-value");
            for usage in [
                CredentialPatchUse::Save,
                CredentialPatchUse::Verify {
                    use_saved_credentials: true,
                },
            ] {
                let merged = merge_credentials(platform, &stored, Some(&patch), usage).unwrap();
                assert_eq!(merged[canonical], "new-value");
                assert!(merged.get(alias).is_none());
                assert_eq!(merged["legacyUnknown"], json!({"preserve":true}));
                let worker = worker_config(platform, &merged, "agent").unwrap();
                assert_eq!(worker[canonical], "new-value");
                assert!(worker.get(alias).is_none());
                assert!(worker.get("legacyUnknown").is_none());
            }
        }
    }

    #[test]
    fn same_patch_canonical_wins_including_empty_and_clear_without_resurrection() {
        let stored = json!({"clientId":"old-id","clientSecret":"old-secret","appKey":"older-id","appSecret":"older-secret"});
        for usage in [
            CredentialPatchUse::Save,
            CredentialPatchUse::Verify {
                use_saved_credentials: true,
            },
        ] {
            let merged = merge_credentials("dingtalk", &stored, Some(&json!({"appKey":"alias-id","clientId":"canonical-id","appSecret":"alias-secret","clientSecret":""})), usage).unwrap();
            assert_eq!(merged["clientId"], "canonical-id");
            assert_eq!(merged["clientSecret"], "");
            assert!(merged.get("appKey").is_none());
            assert!(merged.get("appSecret").is_none());
        }
        let cleared = merge_credentials(
            "dingtalk",
            &stored,
            Some(&json!({"appSecret":"alias-secret","clientSecret":null})),
            CredentialPatchUse::Save,
        )
        .unwrap();
        assert!(cleared.get("clientSecret").is_none());
        assert!(cleared.get("appSecret").is_none());
        assert!(worker_config("dingtalk", &cleared, "agent")
            .unwrap()
            .get("clientSecret")
            .is_none());
        assert!(merge_credentials(
            "dingtalk",
            &stored,
            Some(&json!({"clientSecret":null})),
            CredentialPatchUse::Verify {
                use_saved_credentials: true
            }
        )
        .is_err());
    }

    #[test]
    fn masks_require_a_real_saved_value_after_alias_normalization() {
        let stored = json!({"appSecret":"stored-secret"});
        for usage in [
            CredentialPatchUse::Save,
            CredentialPatchUse::Verify {
                use_saved_credentials: true,
            },
        ] {
            let merged = merge_credentials(
                "dingtalk",
                &stored,
                Some(&json!({"appSecret":"********"})),
                usage,
            )
            .unwrap();
            assert_eq!(merged["clientSecret"], "stored-secret");
            for missing in [
                json!({}),
                json!({"clientSecret":""}),
                json!({"clientSecret":"********"}),
            ] {
                assert!(merge_credentials(
                    "dingtalk",
                    &missing,
                    Some(&json!({"appSecret":"********"})),
                    usage
                )
                .unwrap_err()
                .contains("no stored value"));
            }
        }
        assert!(merge_credentials(
            "dingtalk",
            &stored,
            Some(&json!({"clientSecret":"********"})),
            CredentialPatchUse::Verify {
                use_saved_credentials: false
            }
        )
        .unwrap_err()
        .contains("useSavedCredentials"));
    }

    #[test]
    fn unknown_saved_fields_survive_but_new_fields_and_invalid_values_are_rejected() {
        let stored = json!({"unknownOldField":["preserved"],"token":"stored"});
        let merged = merge_credentials("wechat", &stored, None, CredentialPatchUse::Save).unwrap();
        assert_eq!(merged["unknownOldField"], stored["unknownOldField"]);
        assert_eq!(merged["botToken"], "stored");
        for patch in [
            json!({"unknownNewField":"value"}),
            json!({"botToken":123}),
            json!({"token":"x".repeat(16 * 1024 + 1)}),
        ] {
            assert!(
                merge_credentials("wechat", &stored, Some(&patch), CredentialPatchUse::Save)
                    .is_err()
            );
        }
        assert!(worker_config("wechat", &merged, "agent")
            .unwrap()
            .get("unknownOldField")
            .is_none());
    }

    #[test]
    fn aliases_do_not_infer_or_replace_dingtalk_authentication_mode() {
        let stored = json!({"authMode":"legacy_app","clientId":"old-id","clientSecret":"old-secret","robotCode":"robot"});
        let merged = merge_credentials("dingtalk", &stored, Some(&json!({"appKey":"new-id","appSecret":"new-secret","restBaseUrl":"https://api.dingtalk.io/v1.0"})), CredentialPatchUse::Save).unwrap();
        let worker = worker_config("dingtalk", &merged, "agent").unwrap();
        assert_eq!(worker["authMode"], "legacy_app");
        assert_eq!(worker["apiBaseUrl"], "https://api.dingtalk.io/v1.0");
        assert!(worker.get("corpId").is_none());
        let current = merge_credentials(
            "dingtalk",
            &json!({}),
            Some(&json!({"appKey":"id","appSecret":"secret","corpId":"corp","robotCode":"robot"})),
            CredentialPatchUse::Save,
        )
        .unwrap();
        assert!(current.get("authMode").is_none());
        assert_eq!(
            worker_config("dingtalk", &current, "agent").unwrap()["corpId"],
            "corp"
        );
    }
}
