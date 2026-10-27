//! Bridge 会话读面、重置和本机用户主动发文件；只操作真实会话、文件与持久投递事实。
use crate::{Principal, ServiceState};
use axum::{
    extract::{Path as RoutePath, Query, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use lingxi_adapters::storage::{bridge as store, session_tree};
use lingxi_kernel::ports::StorageError;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug)]
pub struct RouteError {
    status: StatusCode,
    code: &'static str,
    message: String,
}
impl RouteError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
}
impl IntoResponse for RouteError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"ok":false,"code":self.code,"error":self.message})),
        )
            .into_response()
    }
}
impl From<StorageError> for RouteError {
    fn from(error: StorageError) -> Self {
        let status = match error {
            StorageError::Conflict { .. } => StatusCode::CONFLICT,
            StorageError::InvalidRequest { .. } => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self::new(status, "bridge_storage", error.to_string())
    }
}
fn owner(principal: &Principal) -> Result<(), RouteError> {
    if !principal.is_local_owner() {
        return Err(RouteError::new(
            StatusCode::FORBIDDEN,
            "local_owner_required",
            "本操作需要本机所有者身份",
        ));
    }
    Ok(())
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionQuery {
    pub agent_id: String,
    pub platform: Option<String>,
    #[serde(alias = "senderId")]
    pub user_id: Option<String>,
}
async fn resolve(
    state: &ServiceState,
    query: &SessionQuery,
    key: &str,
    require_enabled: bool,
) -> Result<(store::BridgeConnector, store::BridgeConversation), RouteError> {
    state
        .memory()
        .agent_dir_checked(&query.agent_id)
        .map_err(|_| RouteError::new(StatusCode::NOT_FOUND, "agent_not_found", "助手不存在"))?;
    if state
        .memory()
        .is_agent_deleted_checked(&query.agent_id)
        .map_err(|e| {
            RouteError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "agent_state",
                format!("{e:?}"),
            )
        })?
    {
        return Err(RouteError::new(
            StatusCode::NOT_FOUND,
            "agent_not_found",
            "助手已删除",
        ));
    }
    let row = store::find_conversation(
        state.storage(),
        &query.agent_id,
        key,
        query.platform.as_deref(),
        query.user_id.as_deref(),
    )
    .await?
    .ok_or_else(|| {
        RouteError::new(
            StatusCode::NOT_FOUND,
            "bridge_session_not_found",
            "Bridge 会话不存在",
        )
    })?;
    let cfg = store::get_connector(state.storage(), &row.connector_id)
        .await?
        .ok_or_else(|| {
            RouteError::new(
                StatusCode::NOT_FOUND,
                "bridge_connector_missing",
                "连接不存在",
            )
        })?;
    if cfg.input.actor_id != query.agent_id
        || cfg.input.studio_id != state.management.identity_ids().1
        || cfg.input.owner_subject != lingxi_kernel::LOCAL_OWNER_SUBJECT
    {
        return Err(RouteError::new(
            StatusCode::FORBIDDEN,
            "bridge_scope_denied",
            "连接归属不匹配",
        ));
    }
    if require_enabled && !cfg.input.enabled {
        return Err(RouteError::new(
            StatusCode::CONFLICT,
            "bridge_connector_disabled",
            "连接已停用",
        ));
    }
    Ok((cfg, row))
}

pub async fn messages_route(
    Extension(principal): Extension<Principal>,
    State(state): State<ServiceState>,
    RoutePath(key): RoutePath<String>,
    Query(query): Query<SessionQuery>,
) -> Result<Response, RouteError> {
    owner(&principal)?;
    let (_, conversation) = resolve(&state, &query, &key, false).await?;
    let Some(sid) = conversation.current_session_id else {
        return Ok(Json(json!({"messages":[],"sessionId":null})).into_response());
    };
    let session = session_tree::get_session_row(state.storage(), &sid)
        .await?
        .ok_or_else(|| {
            RouteError::new(
                StatusCode::NOT_FOUND,
                "bridge_session_missing",
                "会话尚未建立或已删除",
            )
        })?;
    if session.owner_user_id != conversation.subject || session.agent_id != conversation.actor_id {
        return Err(RouteError::new(
            StatusCode::FORBIDDEN,
            "bridge_session_owner_mismatch",
            "会话归属已改变",
        ));
    }
    let response = crate::history::session_export_route(
        Extension(principal),
        State(state),
        RoutePath(sid.clone()),
        RawQuery(None),
        HeaderMap::new(),
    )
    .await;
    if !response.status().is_success() {
        return Ok(response);
    }
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .map_err(|_| {
            RouteError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "history_read_failed",
                "历史读取失败",
            )
        })?;
    let history: Value = serde_json::from_slice(&bytes).map_err(|_| {
        RouteError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "history_invalid",
            "历史格式无效",
        )
    })?;
    let items = history["items"].as_array().ok_or_else(|| {
        RouteError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "history_missing_items",
            "历史缺少权威条目",
        )
    })?;
    let mut messages = Vec::new();
    for item in items {
        let (role, content, media_count) = match item["kind"].as_str() {
            Some("user_message") => {
                let fields = &item["legacyFields"];
                let count = ["images", "attachments"]
                    .into_iter()
                    .map(|key| fields[key].as_array().map_or(0, Vec::len))
                    .sum::<usize>();
                (
                    "user",
                    item["text"].as_str().unwrap_or("").to_string(),
                    count,
                )
            }
            Some("final_message") => {
                let text = item["content"]
                    .as_array()
                    .ok_or_else(|| {
                        RouteError::new(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "history_content_invalid",
                            "回复内容无效",
                        )
                    })?
                    .iter()
                    .filter(|block| block["type"] == "text")
                    .filter_map(|block| block["text"].as_str())
                    .collect::<String>();
                let count = item["content"].as_array().map_or(0, |blocks| {
                    blocks
                        .iter()
                        .filter(|block| block["type"] == "resource_ref")
                        .count()
                });
                ("assistant", text, count)
            }
            _ => continue,
        };
        if content.is_empty() && media_count == 0 {
            continue;
        }
        messages.push(json!({"role":role,"content":content,"hasMedia":media_count>0,"mediaCount":media_count,"ts":item["committedAtUnixMs"]}));
    }
    Ok(Json(json!({"messages":messages,"sessionId":sid})).into_response())
}

pub async fn reset_route(
    Extension(principal): Extension<Principal>,
    State(state): State<ServiceState>,
    RoutePath(key): RoutePath<String>,
    Query(query): Query<SessionQuery>,
) -> Result<Response, RouteError> {
    owner(&principal)?;
    let (cfg, conversation) = resolve(&state, &query, &key, true).await?;
    let mut ids = Vec::new();
    if let Some(sid) = &conversation.current_session_id {
        ids.push(sid.clone());
    }
    for inbox in store::list_pending_inbox(state.storage()).await? {
        if inbox.input.connector_id == conversation.connector_id
            && inbox.input.conversation_id == conversation.conversation_id
            && inbox.input.sender_id == conversation.sender_id
        {
            if let Some(sid) = inbox.session_id {
                ids.push(sid);
            }
        }
    }
    ids.sort();
    ids.dedup();
    let refs: Vec<_> = ids.iter().map(String::as_str).collect();
    let _permit = state
        .storage()
        .session_admission()
        .try_acquire(
            &refs,
            crate::session_supervisor::SessionConcurrencyLimits::DEFAULT_REGISTRY_CAP,
        )
        .map_err(|_| {
            RouteError::new(
                StatusCode::CONFLICT,
                "session_busy",
                "会话正在执行，暂不能重置",
            )
        })?;
    let reset = store::reset_conversation(
        state.storage(),
        &cfg.input.id,
        &conversation.conversation_id,
        &conversation.sender_id,
        cfg.generation,
        state.clock().now_unix_ms(),
    )
    .await?;
    if let Some(source) = conversation.current_session_id {
        crate::slash::clear_bridge_takeover(&state, &source).map_err(|error| {
            RouteError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "reset_applied_takeover_cleanup_failed",
                error,
            )
        })?;
    }
    Ok(Json(json!({"ok":true,"epoch":reset.epoch,"sessionKey":store::conversation_key(&reset),"sessionId":null})).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SendMediaRequest {
    pub platform: String,
    pub chat_id: Option<String>,
    pub session_key: Option<String>,
    pub session_id: String,
    pub file_id: String,
    pub request_id: Option<String>,
    pub label: Option<String>,
}
pub async fn send_media_route(
    Extension(principal): Extension<Principal>,
    State(state): State<ServiceState>,
    Query(query): Query<SessionQuery>,
    body: Result<Json<SendMediaRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Response, RouteError> {
    owner(&principal)?;
    let Json(body) = body.map_err(|error| {
        RouteError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            error.body_text(),
        )
    })?;
    if query
        .platform
        .as_ref()
        .is_some_and(|platform| platform != &body.platform)
    {
        return Err(RouteError::new(
            StatusCode::BAD_REQUEST,
            "target_conflict",
            "请求中的平台不一致",
        ));
    }
    if body
        .label
        .as_ref()
        .is_some_and(|label| label.len() > 4096 || label.contains('\0'))
    {
        return Err(RouteError::new(
            StatusCode::BAD_REQUEST,
            "invalid_label",
            "文件说明无效或过长",
        ));
    }
    let key = body
        .session_key
        .as_deref()
        .or(body.chat_id.as_deref())
        .ok_or_else(|| {
            RouteError::new(
                StatusCode::BAD_REQUEST,
                "target_required",
                "请选择已知的 Bridge 会话",
            )
        })?;
    let scope = SessionQuery {
        agent_id: query.agent_id,
        platform: Some(body.platform),
        user_id: query.user_id,
    };
    let (cfg, conversation) = resolve(&state, &scope, key, false).await?;
    if body
        .chat_id
        .as_deref()
        .is_some_and(|chat| chat != conversation.conversation_id)
    {
        return Err(RouteError::new(
            StatusCode::BAD_REQUEST,
            "target_conflict",
            "请求中的会话目标不一致",
        ));
    }
    let id = match &body.request_id {
        Some(raw) => format!(
            "bridge-manual:{}",
            crate::dedup::validate_request_id(raw).map_err(|error| RouteError::new(
                StatusCode::BAD_REQUEST,
                "invalid_request_id",
                error
            ))?
        ),
        None => format!(
            "bridge-manual:{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ),
    };
    if let Some(action) = store::get_manual_action(state.storage(), &id).await? {
        let original = store::get_inbox(state.storage(), &action.input.target_inbox_id)
            .await?
            .ok_or_else(|| {
                RouteError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "manual_target_missing",
                    "原操作的目标记录不存在",
                )
            })?;
        if action.input.connector_id != cfg.input.id
            || action.input.source_session_id != body.session_id
            || action.input.file_id != body.file_id
            || action.input.label != body.label
            || original.input.conversation_id != conversation.conversation_id
            || original.input.sender_id != conversation.sender_id
        {
            return Err(RouteError::new(
                StatusCode::CONFLICT,
                "manual_request_conflict",
                "同一请求编号不能更换内容或目标",
            ));
        }
        let outbox = store::get_outbox(state.storage(), &format!("manual:{id}"))
            .await?
            .ok_or_else(|| {
                RouteError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "manual_outbox_missing",
                    "原操作缺少投递记录",
                )
            })?;
        return Ok(manual_response(&action, &outbox, true));
    }
    if !cfg.input.enabled {
        return Err(RouteError::new(
            StatusCode::CONFLICT,
            "bridge_connector_disabled",
            "连接已停用",
        ));
    }
    let target = store::latest_conversation_inbox(state.storage(), &conversation, cfg.generation)
        .await?
        .ok_or_else(|| {
            RouteError::new(
                StatusCode::CONFLICT,
                "current_target_missing",
                "当前连接没有可验证的会话目标",
            )
        })?;
    let source = session_tree::get_session_row(state.storage(), &body.session_id)
        .await?
        .ok_or_else(|| {
            RouteError::new(
                StatusCode::NOT_FOUND,
                "source_session_missing",
                "文件来源会话不存在",
            )
        })?;
    if source.owner_user_id != lingxi_kernel::LOCAL_OWNER_SUBJECT
        || source.agent_id != cfg.input.actor_id
        || source.lifecycle != "active"
    {
        return Err(RouteError::new(
            StatusCode::FORBIDDEN,
            "source_session_denied",
            "文件来源不属于当前助手的本机所有者",
        ));
    }
    let file = state
        .session_files()
        .get_file(&body.session_id, &body.file_id)
        .await
        .map_err(|_| {
            RouteError::new(
                StatusCode::NOT_FOUND,
                "file_unavailable",
                "文件不存在或不可用",
            )
        })?
        .ok_or_else(|| {
            RouteError::new(
                StatusCode::NOT_FOUND,
                "file_unavailable",
                "文件不存在或不可用",
            )
        })?;
    if file.status != "available"
        || file.is_directory
        || file.size_bytes > crate::bridgemedia::MAX_BRIDGE_MEDIA_SIZE
    {
        return Err(RouteError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "file_not_sendable",
            "文件不可发送或超过 50 MiB",
        ));
    }
    if cfg.input.platform == "qq"
        && target.input.conversation_kind == store::ConversationKind::Group
        && !matches!(file.kind.as_str(), "image" | "video" | "audio")
    {
        return Err(RouteError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "qq_group_file_unsupported",
            "QQ 群聊暂不支持普通文件发送",
        ));
    }
    let path = std::path::PathBuf::from(&file.real_path);
    let bytes = tokio::task::spawn_blocking(move || {
        read_bridge_file(&path, crate::bridgemedia::MAX_BRIDGE_MEDIA_SIZE)
    })
    .await
    .map_err(|_| {
        RouteError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "file_reader_failed",
            "文件读取任务失败",
        )
    })?
    .map_err(|error| {
        RouteError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "safe_file_read_failed",
            error,
        )
    })?;
    if bytes.len() as u64 != file.size_bytes {
        return Err(RouteError::new(
            StatusCode::CONFLICT,
            "file_changed",
            "文件内容已改变，请重新选择",
        ));
    }
    let input = store::BridgeManualActionInput {
        id,
        connector_id: cfg.input.id.clone(),
        connector_generation: cfg.generation,
        target_inbox_id: target.id,
        source_session_id: body.session_id,
        file_id: body.file_id,
        content_sha256: lingxi_kernel::session_tree::sha256_hex(&bytes),
        label: body.label,
    };
    let queued =
        store::enqueue_manual_action(state.storage(), &input, state.clock().now_unix_ms()).await?;
    Ok(manual_response(
        &queued.action,
        &queued.outbox,
        !queued.created,
    ))
}

fn manual_response(
    action: &store::BridgeManualAction,
    outbox: &store::BridgeOutbox,
    replayed: bool,
) -> Response {
    Json(json!({"ok":true,"queued":matches!(outbox.status,store::BridgeOutboxStatus::Queued|store::BridgeOutboxStatus::Started),"manualActionId":action.input.id,"deliveryId":outbox.input.delivery_id,"fileId":action.input.file_id,"status":outbox.status,"replayed":replayed,"receipt":outbox.platform_receipt})).into_response()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeliveryQuery {
    pub agent_id: String,
}
pub async fn delivery_route(
    Extension(principal): Extension<Principal>,
    State(state): State<ServiceState>,
    RoutePath(id): RoutePath<String>,
    Query(query): Query<DeliveryQuery>,
) -> Result<Response, RouteError> {
    owner(&principal)?;
    let outbox = store::get_outbox(state.storage(), &id)
        .await?
        .ok_or_else(|| {
            RouteError::new(StatusCode::NOT_FOUND, "delivery_missing", "投递记录不存在")
        })?;
    let cfg = store::get_connector(state.storage(), &outbox.input.connector_id)
        .await?
        .ok_or_else(|| RouteError::new(StatusCode::NOT_FOUND, "connector_missing", "连接不存在"))?;
    if cfg.input.actor_id != query.agent_id
        || cfg.input.studio_id != state.management.identity_ids().1
    {
        return Err(RouteError::new(
            StatusCode::FORBIDDEN,
            "delivery_scope_denied",
            "投递归属不匹配",
        ));
    }
    Ok(Json(json!({"ok":true,"deliveryId":id,"status":outbox.status,"receipt":outbox.platform_receipt,"error":outbox.error})).into_response())
}

/// 固定父目录及文件句柄读取；用于本机用户主动发送和 Host 发出前的同内容复核。
#[cfg(unix)]
pub(crate) fn read_bridge_file(path: &Path, max_bytes: u64) -> Result<Vec<u8>, String> {
    use crate::filetools::fsio::{DirFd, FileIdentity};
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;
    if max_bytes > 50 * 1024 * 1024 || path.canonicalize().ok().as_deref() != Some(path) {
        return Err("文件路径已改变或读取范围无效".into());
    }
    let parent = path.parent().ok_or("文件缺少父目录")?;
    let name = path.file_name().ok_or("文件缺少名称")?;
    let dir = DirFd::open(parent).map_err(|_| "无法固定文件父目录")?;
    let mut file = dir
        .open_attachment_nofollow(name)
        .ok_or("文件不存在")?
        .map_err(|_| "文件不是可安全读取的普通文件")?;
    let before = file.metadata().map_err(|_| "文件状态读取失败")?;
    if !before.is_file() || before.len() > max_bytes {
        return Err("文件不是普通文件或超过大小限制".into());
    }
    let identity = FileIdentity::of(&file).map_err(|_| "文件身份读取失败")?;
    let ctime = (before.ctime(), before.ctime_nsec());
    let mut bytes = Vec::new();
    (&mut file)
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "文件读取失败")?;
    let after = file.metadata().map_err(|_| "文件状态复核失败")?;
    if bytes.len() as u64 > max_bytes
        || FileIdentity::of(&file).map_err(|_| "文件身份复核失败")? != identity
        || (after.ctime(), after.ctime_nsec()) != ctime
        || !dir.same_directory(parent).map_err(|_| "父目录复核失败")?
    {
        return Err("文件在读取期间发生改变".into());
    }
    let current = dir
        .open_attachment_nofollow(name)
        .ok_or("文件已移走")?
        .map_err(|_| "文件路径已改变")?;
    if FileIdentity::of(&current).map_err(|_| "最终文件身份复核失败")? != identity {
        return Err("文件路径已被替换".into());
    }
    Ok(bytes)
}
#[cfg(not(unix))]
pub(crate) fn read_bridge_file(_path: &Path, _max_bytes: u64) -> Result<Vec<u8>, String> {
    Err("当前平台尚无经过验证的固定目录句柄读取能力，不能安全发送本地文件".into())
}
