//! 焦点会话创建：先验证并准备本次独立资料，真实会话行和焦点成功后才广播。
use crate::{
    chat_settings,
    session_ui::{self, SessionMetadata},
    EndpointError, Principal, ServiceState,
};
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Extension, Json,
};
use lingxi_adapters::storage::session_admin::{self as store, SessionCreation};
use serde::Deserialize;
use serde_json::json;
use std::path::Path;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewBody {
    pub agent_id: Option<String>,
    pub current_agent_id: Option<String>,
    pub current_session_path: Option<String>,
    pub cwd: Option<String>,
    #[serde(default)]
    pub workspace_folders: Vec<String>,
    pub workspace_mount_id: Option<String>,
    pub project_id: Option<String>,
    pub memory_enabled: Option<bool>,
    pub thinking_level: Option<String>,
    pub permission_mode: Option<String>,
    pub title: Option<String>,
    pub model: Option<(String, String)>,
    pub record_workspace_history: Option<bool>,
    pub forked_from_session_id: Option<String>,
}
fn failure(message: impl Into<String>) -> Response {
    EndpointError::new(
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        lingxi_protocol::ErrorCode::Internal,
        message,
    )
    .into_response()
}
fn folder(value: &str) -> Result<String, String> {
    let path = Path::new(value);
    if !path.is_absolute() {
        return Err("工作区必须是绝对目录".into());
    }
    let resolved = path
        .canonicalize()
        .map_err(|error| format!("无法读取工作区：{error}"))?;
    if !resolved.is_dir() {
        return Err("工作区必须是目录".into());
    }
    Ok(resolved.to_string_lossy().into_owned())
}

pub async fn create_route(
    Extension(principal): Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<NewBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(value) => value,
        Err(error) => return EndpointError::from_json_rejection(&error).into_response(),
    };
    // 与现有管理写面相同：已授权完整请求的提交不被等待者断开切成半笔。
    match tokio::spawn(async move { create(&state, &principal, body, true).await }).await {
        Ok(response) => response,
        Err(error) => failure(format!("会话创建任务失败：{error}")),
    }
}
pub async fn create_detached_route(
    Extension(principal): Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<NewBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(value) => value,
        Err(error) => return EndpointError::from_json_rejection(&error).into_response(),
    };
    match tokio::spawn(async move { create(&state, &principal, body, false).await }).await {
        Ok(response) => response,
        Err(error) => failure(format!("独立会话创建任务失败：{error}")),
    }
}
async fn create(
    state: &ServiceState,
    principal: &Principal,
    body: NewBody,
    focus: bool,
) -> Response {
    let lock = state.focus_mutation.lock().await;
    match create_locked(state, principal, body, focus, &lock).await {
        Ok(value) => Json(value).into_response(),
        Err(response) => *response,
    }
}

/// 助手切换与会话新建共用真实流程；调用者持锁覆盖来源读取和最终广播。
pub(crate) async fn create_locked(
    state: &ServiceState,
    principal: &Principal,
    body: NewBody,
    focus: bool,
    _focus_guard: &tokio::sync::MutexGuard<'_, ()>,
) -> Result<serde_json::Value, Box<Response>> {
    if body
        .workspace_mount_id
        .as_ref()
        .is_some_and(|id| !id.is_empty())
    {
        return Err((EndpointError::invalid_message(
            "当前服务尚未登记此工作区挂载；请选择实际绝对目录",
        )
        .into_response())
        .into());
    }
    if body.workspace_folders.len() > 64 {
        return Err((EndpointError::invalid_message("工作区目录过多").into_response()).into());
    }
    let agent = match body
        .agent_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        Some(value) => value.to_string(),
        None => match state.memory().current_agent_preference() {
            Ok(Some(value)) => value,
            Ok(None) => {
                return Err(
                    (EndpointError::invalid_message("agentId is required").into_response()).into(),
                )
            }
            Err(error) => return Err((failure(error.message)).into()),
        },
    };
    crate::gate_agent_not_deleted(state, &agent)?;
    if let Err(error) = state.memory().agent_dir_checked(&agent) {
        return Err((error.response()).into());
    }
    if let Some(path) = body
        .current_session_path
        .as_deref()
        .filter(|path| !path.is_empty())
    {
        let id = match crate::session_locator::resolve(state.storage(), None, Some(path)).await {
            Ok(id) => id,
            Err(error) => return Err((EndpointError::storage(&error).into_response()).into()),
        };
        crate::gate_session_owned(state, principal, &id).await?;
    }
    let cwd = match body.cwd.as_deref().filter(|value| !value.trim().is_empty()) {
        Some(value) => match folder(value) {
            Ok(value) => Some(value),
            Err(error) => {
                return Err((EndpointError::invalid_message(error).into_response()).into())
            }
        },
        None => match state.resource_io().workspace_root() {
            Some(path) => match folder(&path.to_string_lossy()) {
                Ok(value) => Some(value),
                Err(error) => return Err((failure(error)).into()),
            },
            None => None,
        },
    };
    if (cwd.is_some() || !body.workspace_folders.is_empty())
        && state.resource_io().workspace_root().is_none()
    {
        return Err((EndpointError::new(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            lingxi_protocol::ErrorCode::UpstreamUnavailable,
            "工作区工具未配置，不能声称已启用该目录",
        )
        .into_response())
        .into());
    }
    let mut folders = Vec::new();
    for raw in &body.workspace_folders {
        let normalized = match folder(raw) {
            Ok(value) => value,
            Err(error) => {
                return Err((EndpointError::invalid_message(error).into_response()).into())
            }
        };
        if cwd.as_deref() != Some(normalized.as_str()) && !folders.contains(&normalized) {
            folders.push(normalized);
        }
    }
    let mut roots = cwd.clone().into_iter().collect::<Vec<_>>();
    roots.extend(folders.clone());
    let mode = body.permission_mode.as_deref().unwrap_or("operate");
    if lingxi_kernel::subagent::SessionPermissionMode::parse(mode).is_none() {
        return Err((EndpointError::invalid_message(
            "permissionMode must be operate, ask or read_only",
        )
        .into_response())
        .into());
    }
    let project = body
        .project_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if project.is_some_and(|value| value.len() > 240 || value.chars().any(char::is_control)) {
        return Err((EndpointError::invalid_message("invalid projectId").into_response()).into());
    }
    let lineage = if let Some(parent) = body
        .forked_from_session_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        if focus {
            return Err(
                (EndpointError::invalid_message("焦点新建不接受谱系来源；请选择独立创建")
                    .into_response())
                .into(),
            );
        }
        crate::get_session_owned(state, principal, parent).await?;
        let row =
            match lingxi_adapters::storage::session_tree::get_session_row(state.storage(), parent)
                .await
            {
                Ok(Some(row)) => row,
                Ok(None) => return Err((EndpointError::not_found().into_response()).into()),
                Err(error) => return Err((EndpointError::storage(&error).into_response()).into()),
            };
        let Some(depth) = lingxi_kernel::session_tree::check_fork_depth(row.lineage_depth as u32)
        else {
            return Err((EndpointError::new(
                axum::http::StatusCode::CONFLICT,
                lingxi_protocol::ErrorCode::Conflict,
                "session fork depth limit reached",
            )
            .with_reason("session_fork_depth_limit")
            .into_response())
            .into());
        };
        Some((parent.to_string(), depth))
    } else {
        None
    };
    let sid = match crate::auth::hex_random_public(16) {
        Ok(token) => format!("session-{token}"),
        Err(error) => return Err((failure(error.to_string())).into()),
    };
    let owner = principal
        .user_id
        .as_deref()
        .unwrap_or(crate::auth::LOCAL_OWNER_USER_ID);
    // 显式无效模型仍按原请求错误返回400，不在推导思考档位时变成内部错误。
    if let Some(model) = body.model.as_ref() {
        let requested = chat_settings::ChatChoice {
            model: Some(model.clone()),
            thinking_level: None,
        };
        if let Err(error) = chat_settings::dispatch_choice(state, &requested) {
            return Err(EndpointError::invalid_message(error).into_response().into());
        }
    }
    let mut choice = match chat_settings::effective_choice_for_agent(
        state,
        owner,
        &sid,
        Some(&agent),
        body.model,
    ) {
        Ok(value) => value,
        Err(error) => return Err((failure(error)).into()),
    };
    if let Some(level) = body.thinking_level {
        choice.thinking_level = Some(level);
    }
    let dispatch = match chat_settings::dispatch_choice(state, &choice) {
        Ok(value) => value,
        Err(error) => return Err((EndpointError::invalid_message(error).into_response()).into()),
    };
    if let Err(error) = chat_settings::check_thinking(state, &choice) {
        return Err((EndpointError::invalid_message(error).into_response()).into());
    }
    // 无论默认以后如何变化，本会话保存本次实际可派发的完整模型身份。
    choice.model = Some((
        dispatch.route.provider.clone(),
        dispatch.route.model.clone(),
    ));
    // 序列化同一服务的焦点发布；新会话准入覆盖配置准备至提交/撤销。
    // 与角色卡应用、助手删除共用应用门；所有目录和模型参数已经校验完成。
    let _agent_application = crate::character_cards::CARD_APPLICATION_GATE.lock().await;
    crate::gate_agent_not_deleted(state, &agent)?;
    if let Err(error) = state.memory().agent_dir_checked(&agent) {
        return Err((error.response()).into());
    }
    let _permit = match state
        .storage()
        .session_admission()
        .try_acquire(&[&sid], 10_000)
    {
        Ok(permit) => permit,
        Err(_) => {
            return Err((EndpointError::new(
                axum::http::StatusCode::CONFLICT,
                lingxi_protocol::ErrorCode::Conflict,
                "new session identity is busy",
            )
            .into_response())
            .into())
        }
    };
    match state.storage().get_session(&sid).await {
        Ok(Some(_)) => {
            return Err((EndpointError::new(
                axum::http::StatusCode::CONFLICT,
                lingxi_protocol::ErrorCode::Conflict,
                "session already exists",
            )
            .into_response())
            .into())
        }
        Err(error) => return Err((EndpointError::storage(&error).into_response()).into()),
        Ok(None) => {}
    }
    let metadata = SessionMetadata {
        session_id: sid.clone(),
        cwd: cwd.clone(),
        workspace_folders: folders.clone(),
        project_id: project.map(str::to_string),
    };
    let title = body
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("新会话");
    match session_ui::read(state.storage(), &sid) {
        Ok(Some(_)) => {
            return Err((EndpointError::new(
                axum::http::StatusCode::CONFLICT,
                lingxi_protocol::ErrorCode::Conflict,
                "new session metadata already exists; refusing to overwrite",
            )
            .into_response())
            .into())
        }
        Err(error) => return Err((EndpointError::storage(&error).into_response()).into()),
        Ok(None) => {}
    }
    let mut created = false;
    let mut metadata_prepared = false;
    let mut choice_prepared = false;
    let mut workspace_recorded = false;
    let mut cwd_history = Vec::new();
    let result: Result<(), String> = async {
        metadata_prepared = true;
        session_ui::save(state.storage(), &metadata).map_err(|error| error.to_string())?;
        chat_settings::save_new_session_choice(state.memory().home(), owner, &sid, choice.clone())?;
        choice_prepared = true;
        store::create_session_prepared(
            state.storage(),
            SessionCreation {
                session_id: &sid,
                agent_id: &agent,
                owner_user_id: owner,
                title,
                permission_mode: Some(mode),
                authorized_folders: &roots,
                memory_enabled: body.memory_enabled.unwrap_or(true),
                now_unix_ms: state.clock.now_unix_ms(),
                project_id: project,
                proactive_delegation: state
                    .context_compiler()
                    .map(|compiler| compiler.new_session_proactive_delegation())
                    .transpose()
                    .map_err(|error| error.to_string())?
                    .unwrap_or(false),
                lineage: lineage
                    .as_ref()
                    .map(|(parent, depth)| (parent.as_str(), *depth)),
            },
        )
        .await
        .map_err(|error| error.to_string())?;
        created = true;
        if let Some(cwd) = cwd
            .as_deref()
            .filter(|_| focus || body.record_workspace_history == Some(true))
        {
            cwd_history = state
                .memory()
                .record_session_workspace(&agent, cwd, focus.then_some(sid.as_str()))
                .map_err(|error| error.message)?;
            workspace_recorded = true;
        }
        if focus && !workspace_recorded {
            state
                .memory()
                .set_current_focus(&agent, &sid)
                .map_err(|error| error.message)?;
        } else {
            state
                .memory()
                .agent_dir_checked(&agent)
                .map_err(|error| error.message)?;
        }
        Ok(())
    }
    .await;
    if let Err(error) = result {
        let mut cleanup_errors = Vec::new();
        if created {
            if let Err(error) = store::remove_unstarted_creation(state.storage(), &sid, owner).await
            {
                cleanup_errors.push(error.to_string());
            }
        }
        // 若行撤销不成功，保留资料供诊断，不能清掉可能已被其它真实活动使用的设置。
        if cleanup_errors.is_empty() {
            if choice_prepared {
                if let Err(error) =
                    chat_settings::remove_new_session_choice(state.memory().home(), owner, &sid)
                {
                    cleanup_errors.push(error);
                }
            }
            if metadata_prepared {
                if let Err(error) = session_ui::remove_uncreated(state.storage(), &sid) {
                    cleanup_errors.push(error.to_string());
                }
            }
        }
        return Err((failure(format!(
            "{}失败：{error}{}{}",
            if focus {
                "创建焦点会话"
            } else {
                "创建独立会话"
            },
            if workspace_recorded {
                "；助手工作目录历史已记录"
            } else {
                ""
            },
            if cleanup_errors.is_empty() {
                String::new()
            } else {
                format!("；初始化清理未完成：{}", cleanup_errors.join("；"))
            }
        )))
        .into());
    }
    let response = json!({"ok":true,"sessionId":sid,"path":crate::session_locator::native(&sid),"agentId":agent,"agentName":state.memory().agent_display_name(&agent).unwrap_or_else(||agent.clone()),"cwd":cwd,"cwdHistory":cwd_history,"workspaceFolders":folders,"authorizedFolders":roots,"memoryEnabled":body.memory_enabled.unwrap_or(true),"projectId":project,"forkedFromSessionId":lineage.as_ref().map(|(parent,_)|parent),"parentSessionId":lineage.as_ref().map(|(parent,_)|parent),"lineageDepth":lineage.as_ref().map_or(0,|(_,depth)|*depth),"permissionMode":mode,"accessMode":mode,"planMode":mode=="read_only","thinkingLevel":choice.thinking_level,"currentModelId":dispatch.route.model,"currentModelProvider":dispatch.route.provider,"currentModelName":dispatch.route.model});
    state.app_events().publish(
        "session-created",
        json!({"session":response,"sessionId":sid,"agentId":agent,"detached":!focus}),
    );
    if focus {
        state.app_events().publish(
            "session-focus-changed",
            json!({"sessionId":sid,"agentId":agent}),
        );
    }
    state
        .chat_wire
        .publish_session_created(&sid, response.clone(), state.clock().now_unix_ms());
    Ok(response)
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceQuery {
    pub subdir: Option<String>,
}

/// 工作台列表复用真实 ResourceIO 和授权边界；不将故障转换成空目录。
pub async fn workspace_files_route(
    Extension(principal): Extension<Principal>,
    State(state): State<ServiceState>,
    axum::extract::Path(sid): axum::extract::Path<String>,
    axum::extract::Query(query): axum::extract::Query<WorkspaceQuery>,
) -> Response {
    if let Err(response) = crate::get_session_owned(&state, &principal, &sid).await {
        return response;
    }
    let row = match lingxi_adapters::storage::session_tree::get_session_row(state.storage(), &sid)
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return EndpointError::not_found().into_response(),
        Err(error) => return EndpointError::storage(&error).into_response(),
    };
    let metadata = match session_ui::read(state.storage(), &sid) {
        Ok(Some(value)) => value,
        Ok(None) => return EndpointError::invalid_message("会话尚未指定工作区").into_response(),
        Err(error) => return EndpointError::storage(&error).into_response(),
    };
    let Some(root) = metadata.cwd else {
        return EndpointError::invalid_message("会话尚未指定工作区").into_response();
    };
    let subdir = query.subdir.unwrap_or_default();
    let relative = Path::new(&subdir);
    if relative.components().any(|part| {
        !matches!(
            part,
            std::path::Component::Normal(_) | std::path::Component::CurDir
        )
    }) {
        return EndpointError::invalid_message("子目录必须位于会话工作区内").into_response();
    }
    let mut ctx = crate::resourceio::OpContext::local_owner(&sid);
    let kernel_principal = crate::sessions::kernel_principal_of(&principal);
    ctx.principal_kind = kernel_principal.storage_kind();
    ctx.principal_subject = kernel_principal.storage_subject();
    if let Err(error) = state
        .resource_io()
        .restore_session_roots(&ctx, &row.authorized_folders)
    {
        return crate::resourceio::error_response(error);
    }
    let path = Path::new(&root).join(relative);
    match state
        .resource_io()
        .list(&json!({"kind":"local-file","path":path}), &ctx)
        .await
    {
        Ok(value) => {
            let Some(items) = value["items"].as_array() else {
                return failure("目录服务返回格式无效");
            };
            let files: Vec<_> = items.iter().map(|item| json!({"name":item["name"],"isDir":item["isDirectory"],"size":item["size"],"mtime":item["mtimeMs"].as_u64().and_then(|ms|chrono::DateTime::from_timestamp_millis(ms as i64).map(|date|date.to_rfc3339()))})).collect();
            Json(json!({"sessionId":sid,"basePath":root,"subdir":subdir,"files":files}))
                .into_response()
        }
        Err(error) => crate::resourceio::error_response(error),
    }
}
