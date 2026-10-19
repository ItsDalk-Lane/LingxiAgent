//! R07-T09：聊天客户端（CLI/mobile）共享的只读上下文面。
//!
//! 两个入口：
//! - `GET /lingxi/v1/chat/models?agentId=&sessionId=`：最小模型目录与实际
//!   选择（chat scope），不透传供应商地址、凭据或自由模型元数据。
//! - `GET /lingxi/v1/mobile/bootstrap?agentId=&sessionId=`：手机首屏握手
//!   （chat scope），按身份投影、脱敏；顺带对目标助手执行真实的过期
//!   工作区持久化清理（清理器失败即明确失败，不虚报已清理）。
//!
//! 目标解析规则（与 chat_wire 归属链同一口径）：
//! - 显式 sessionId：先验会话归属，agentId 取会话实际归属；若同时给的
//!   agentId 与会话不符则拒绝。
//! - 仅显式 agentId：助手必须存在、未删除；不回退到其它助手。
//! - 都不给：当前偏好助手（current → primary）。
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use lingxi_protocol::client_context::{
    ClientAgentSummary, ClientAvatarAvailability, ClientContextQuery, ClientModelContext,
    ClientModelSelection, ClientModelState, ClientModelSummary, MobileBootstrapResponse,
};
use serde_json::{json, Value};

use crate::auth::Principal;
use crate::ServiceState;

/// 统一错误面：与聊天设置面相同的 `{ok:false,error}` 形状。
fn failure(code: axum::http::StatusCode, message: impl Into<String>) -> Response {
    (
        code,
        Json(json!({"ok":false,"error":message.into()})),
    )
        .into_response()
}

struct ContextTarget {
    owner: String,
    agent_id: Option<String>,
    session_id: Option<String>,
}

/// 解析显式 sessionId / agentId 到拥有者与助手；越权、不存在、已删除、
/// 身份互相矛盾都是明确拒绝，绝不换目标或回退焦点助手冒充成功。
async fn resolve_target(
    state: &ServiceState,
    principal: &Principal,
    query: &ClientContextQuery,
) -> Result<ContextTarget, Response> {
    let owner_key = principal
        .user_id
        .as_deref()
        .unwrap_or(&principal.principal_id)
        .to_string();
    if let Some(session_id) = query.session_id.as_deref() {
        if session_id.trim().is_empty() || session_id.len() > 256 {
            return Err(failure(
                axum::http::StatusCode::BAD_REQUEST,
                "invalid sessionId",
            ));
        }
        let facts = match state.sessions().get_for(principal, session_id).await {
            Ok(crate::sessions::SessionAccess::Ok(facts)) => facts,
            Ok(crate::sessions::SessionAccess::NotFound) => {
                return Err(failure(
                    axum::http::StatusCode::NOT_FOUND,
                    "session not found",
                ))
            }
            Ok(crate::sessions::SessionAccess::Forbidden) => {
                return Err(failure(
                    axum::http::StatusCode::FORBIDDEN,
                    "session unavailable or not owned",
                ))
            }
            Err(err) => {
                return Err(failure(
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    err.to_string(),
                ))
            }
        };
        if let Some(agent_id) = query.agent_id.as_deref() {
            if agent_id != facts.agent_id {
                return Err(failure(
                    axum::http::StatusCode::CONFLICT,
                    "agentId does not match the session's agent",
                ));
            }
        }
        return Ok(ContextTarget {
            owner: facts.owner_user_id,
            agent_id: Some(facts.agent_id),
            session_id: Some(facts.session_id),
        });
    }
    if let Some(agent_id) = query.agent_id.as_deref() {
        if agent_id.trim().is_empty() || agent_id.len() > 128 {
            return Err(failure(
                axum::http::StatusCode::BAD_REQUEST,
                "invalid agentId",
            ));
        }
        // 显式助手必须真实存在且未删除；存在性以实际 config 目录为准。
        state
            .memory()
            .agent_dir_checked(agent_id)
            .map_err(|err| failure(axum::http::StatusCode::NOT_FOUND, err.message))?;
        if state
            .memory()
            .is_agent_deleted_checked(agent_id)
            .map_err(|err| failure(axum::http::StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        {
            return Err(failure(
                axum::http::StatusCode::CONFLICT,
                "agent deleted",
            ));
        }
        return Ok(ContextTarget {
            owner: owner_key,
            agent_id: Some(agent_id.to_string()),
            session_id: None,
        });
    }
    let current = state
        .memory()
        .current_agent_preference()
        .map_err(|err| failure(axum::http::StatusCode::INTERNAL_SERVER_ERROR, err.message))?;
    Ok(ContextTarget {
        owner: owner_key,
        agent_id: current,
        session_id: None,
    })
}

/// 与现役 `shared/model-capabilities.ts` 同口径的音频输入判定（只读投影，
/// 不写回任何目录事实）。
fn model_audio(metadata: &serde_json::Map<String, Value>) -> bool {
    if metadata.get("audio").and_then(Value::as_bool) == Some(true) {
        return true;
    }
    if metadata
        .get("compat")
        .and_then(|c| c.get("hanaAudioInput"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        return true;
    }
    metadata
        .get("input")
        .and_then(Value::as_array)
        .is_some_and(|inputs| inputs.iter().any(|v| v.as_str() == Some("audio")))
}

/// 显式 compat 声明优先；未知声明按 unsupported 如实上抛，不假装可直传。
fn model_audio_transport(metadata: &serde_json::Map<String, Value>) -> Option<String> {
    if !model_audio(metadata) {
        return None;
    }
    let explicit = metadata
        .get("compat")
        .and_then(|c| {
            c.get("audioTransport")
                .or_else(|| c.get("hanaAudioTransport"))
        })
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase);
    match explicit.as_deref() {
        Some("none") => None,
        Some("mimo-input-audio" | "openai-input-audio") => explicit,
        Some(_) => Some("unsupported".into()),
        None => Some("unsupported".into()),
    }
}

const VISIBLE_THINKING_LEVELS: [&str; 3] = ["off", "medium", "high"];
const KNOWN_THINKING_LEVELS: [&str; 6] = ["off", "low", "medium", "high", "xhigh", "max"];

/// 现役 getModelThinkingLevels 口径：显式列表（过滤未知档位）优先；
/// 否则支持 max 的模型在默认可见档位后追加 max。
fn model_thinking_levels(
    metadata: &serde_json::Map<String, Value>,
    xhigh: bool,
) -> Vec<String> {
    if let Some(explicit) = metadata.get("thinkingLevels").and_then(Value::as_array) {
        let levels: Vec<String> = explicit
            .iter()
            .filter_map(Value::as_str)
            .filter(|level| KNOWN_THINKING_LEVELS.contains(level))
            .map(str::to_string)
            .collect();
        if !levels.is_empty() {
            return levels;
        }
    }
    let mut levels: Vec<String> = VISIBLE_THINKING_LEVELS
        .iter()
        .map(|level| level.to_string())
        .collect();
    if xhigh {
        levels.push("max".into());
    }
    levels
}

/// 目录行投影：现役 settings/chat 目录共用的字段子集，不含端点与密钥。
fn model_summary(
    state: &ServiceState,
    provider: &str,
    model: &lingxi_adapters::models::config::CatalogModel,
    current: Option<&(String, String)>,
) -> Option<ClientModelSummary> {
    let gateway = state.model_gateway()?;
    let dispatch = gateway
        .resolve_dispatch(&lingxi_kernel::model_exchange::ModelRouteRequest {
            operation: lingxi_kernel::model_exchange::ModelOperation::Chat,
            provider: Some(provider.to_string()),
            model: Some(model.id.clone()),
        })
        .ok()?;
    let metadata = &model.metadata;
    let reasoning = dispatch
        .compat
        .as_ref()
        .and_then(|c| c.reasoning)
        .unwrap_or(false);
    let xhigh = metadata.get("xhigh").and_then(Value::as_bool) == Some(true)
        || metadata
            .get("thinkingLevelMap")
            .and_then(|map| map.get("xhigh"))
            .and_then(Value::as_str)
            .is_some();
    let thinking_levels = model_thinking_levels(metadata, xhigh);
    let default_thinking_level = metadata
        .get("defaultThinkingLevel")
        .and_then(Value::as_str)
        .filter(|level| KNOWN_THINKING_LEVELS.contains(level))
        .map(str::to_string);
    let audio_transport = model_audio_transport(metadata);
    let audio_transport_supported = matches!(
        audio_transport.as_deref(),
        Some("mimo-input-audio" | "openai-input-audio")
    );
    Some(ClientModelSummary {
        id: model.id.clone(),
        name: metadata
            .get("name")
            .or_else(|| metadata.get("displayName"))
            .and_then(Value::as_str)
            .unwrap_or(&model.id)
            .to_string(),
        provider: provider.to_string(),
        is_current: current.is_some_and(|(p, id)| *p == provider && *id == model.id),
        reasoning,
        xhigh,
        thinking_levels,
        default_thinking_level,
        audio: model_audio(metadata),
        audio_transport,
        audio_transport_supported,
        input: metadata
            .get("input")
            .and_then(Value::as_array)
            .map(|inputs| {
                inputs
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .filter(|inputs: &Vec<String>| !inputs.is_empty())
            .unwrap_or_else(|| {
                // 元数据未声明 input 时不得只报 "text"：渲染端把显式数组
                // 当能力真相（无 image 即按纯文本模型把图片降级为文件），
                // 这里必须与发送闸同一事实源（provider.rs/chat_wire.rs 的
                // capabilities.image_input 判定），否则声明了 imageInput
                // 的模型会被目录投影错误降级。
                let mut input = vec!["text".to_string()];
                if dispatch.capabilities.image_input == Some(true) {
                    input.push("image".into());
                }
                input
            }),
    })
}

/// 模型上下文：目录、实际选择、真实状态与代次。未配置、不可用、配置损坏
/// 分开表态；代次是真实十进制字符串，不为客户端数值精度让路。
fn model_context(
    state: &ServiceState,
    target: &ContextTarget,
) -> Result<ClientModelContext, Response> {
    // 无模型面时不能走 effective_choice_for_agent：其 thinking 归一会经
    // dispatch 校验并响亮失败。这里要的是只读投影——目录真实为空、状态
    // unconfigured、代次与配置报告的未激活口径一致（"0"），thinking 仍读
    // 用户真实选择文件的原始值。
    let Some(gateway) = state.model_gateway() else {
        let raw = crate::chat_settings::choice(
            state.memory().home(),
            &target.owner,
            target.session_id.as_deref().unwrap_or(""),
        )
        .map_err(|err| failure(axum::http::StatusCode::INTERNAL_SERVER_ERROR, err))?;
        return Ok(ClientModelContext {
            models: vec![],
            active_model: None,
            model_state: ClientModelState::Unconfigured,
            model_generation: "0".into(),
            thinking_level: raw.thinking_level.unwrap_or_else(|| "medium".into()),
        });
    };
    let choice = crate::chat_settings::effective_choice_for_agent(
        state,
        &target.owner,
        target.session_id.as_deref().unwrap_or(""),
        target.agent_id.as_deref(),
        None,
    )
    .map_err(|err| failure(axum::http::StatusCode::INTERNAL_SERVER_ERROR, err))?;
    let thinking_level = choice.thinking_level.clone().unwrap_or_else(|| "medium".into());
    let mut models = Vec::new();
    for (provider, entries) in gateway.catalog_models() {
        for model in &entries {
            if let Some(summary) = model_summary(state, &provider, model, choice.model.as_ref()) {
                models.push(summary);
            }
        }
    }
    let model_state = match &choice.model {
        None => ClientModelState::Unconfigured,
        Some(_) => match crate::chat_settings::dispatch_choice(state, &choice) {
            Ok(_) => ClientModelState::Ready,
            Err(_) => ClientModelState::Unavailable,
        },
    };
    let generation = gateway.upcoming_generation().saturating_sub(1);
    Ok(ClientModelContext {
        models,
        active_model: choice.model.as_ref().map(|(provider, id)| ClientModelSelection {
            id: id.clone(),
            provider: provider.clone(),
        }),
        model_state,
        model_generation: generation.to_string(),
        thinking_level,
    })
}

/// `GET /lingxi/v1/chat/models` — CLI/mobile 的最小模型目录与实际选择。
pub async fn chat_models_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Query(query): Query<ClientContextQuery>,
) -> Response {
    let target = match resolve_target(&state, &principal, &query).await {
        Ok(target) => target,
        Err(response) => return response,
    };
    let context = match model_context(&state, &target) {
        Ok(context) => context,
        Err(response) => return response,
    };
    // 前端现役消费形状（loadModels）：models + activeModel；currentModel 与
    // modelState/modelGeneration 供 CLI/手机状态面消费，不回推完整管理面。
    let current_model = match target.session_id.as_deref() {
        Some(_) => context.active_model.clone(),
        None => context.active_model.clone(),
    };
    Json(json!({
        "models": context.models,
        "activeModel": context.active_model,
        "currentModel": current_model,
        "modelState": context.model_state,
        "modelGeneration": context.model_generation,
        "thinkingLevel": context.thinking_level,
    }))
    .into_response()
}

/// 头像可用性：与 settings_ui::avatars_health_route 同一目录口径；
/// 助手侧与 agents_admin 列表扫描同形状。
fn avatar_availability(state: &ServiceState, agent_id: Option<&str>) -> ClientAvatarAvailability {
    let user = ["png", "jpg", "jpeg", "webp"].iter().any(|extension| {
        state
            .memory()
            .user_dir()
            .join("avatars")
            .join(format!("user.{extension}"))
            .is_file()
    });
    let agent = agent_id.is_some_and(|id| {
        state
            .memory()
            .agent_dir_checked(id)
            .map(|dir| {
                ["png", "jpg", "jpeg", "webp"].iter().any(|extension| {
                    dir.join("avatars")
                        .join(format!("agent.{extension}"))
                        .is_file()
                        || dir.join(format!("avatar.{extension}")).is_file()
                })
            })
            .unwrap_or(false)
    });
    ClientAvatarAvailability { agent, user }
}

/// `GET /lingxi/v1/mobile/bootstrap` — 手机首屏握手。只读、按身份投影、
/// 脱敏；顺带执行真实的过期工作区持久化清理（失败即明确失败）。
pub async fn mobile_bootstrap_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Query(query): Query<ClientContextQuery>,
) -> Response {
    let target = match resolve_target(&state, &principal, &query).await {
        Ok(target) => target,
        Err(response) => return response,
    };
    // 与 agent_config_readback 读取路径同口径：先清掉已消失目录的工作区
    // 历史再读。清理器真实失败不能伪装成已清理。
    if let Some(agent_id) = target.agent_id.as_deref() {
        if let Ok(agent_dir) = state.memory().agent_dir_checked(agent_id) {
            if let Err(message) = crate::agents_admin::agent_config_readback::gc_workspace_persistence(
                state.memory(),
                &agent_dir,
            ) {
                return failure(axum::http::StatusCode::INTERNAL_SERVER_ERROR, message);
            }
        }
    }
    let preferences = match state.memory().read_preferences_root() {
        Ok(value) => value,
        Err(err) => return failure(axum::http::StatusCode::INTERNAL_SERVER_ERROR, err),
    };
    let locale = preferences["locale"].as_str().unwrap_or("").trim().to_string();
    let locale = if locale.is_empty() {
        "zh-CN".to_string()
    } else {
        locale
    };
    let zh = locale.to_lowercase().starts_with("zh");
    let user_name = preferences["userName"]
        .as_str()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| if zh { "用户" } else { "User" }.to_string());
    let permission_mode = match preferences["session_permission_mode_default"].as_str() {
        Some(mode @ ("auto" | "operate" | "ask" | "read_only")) => mode.to_string(),
        _ => "auto".to_string(),
    };
    let cwd_history: Vec<String> = preferences["cwd_history"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let memory_enabled = preferences
        .get("memory")
        .and_then(|memory| memory.get("enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(true);

    // 助手列表投影：与 agents 集合同一 config.yaml 扫描，去掉描述等冗余字段。
    let summaries = match state.memory().list_agent_summaries() {
        Ok(rows) => rows,
        Err(err) => return failure(axum::http::StatusCode::INTERNAL_SERVER_ERROR, err.message),
    };
    let mut agents = Vec::with_capacity(summaries.len());
    for row in &summaries {
        let Some(id) = row.get("id").and_then(Value::as_str) else {
            return failure(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "agent summary missing id",
            );
        };
        agents.push(ClientAgentSummary {
            id: id.to_string(),
            name: row
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(id)
                .to_string(),
            yuan: row
                .get("yuan")
                .and_then(Value::as_str)
                .unwrap_or("lingxi")
                .to_string(),
            is_primary: row.get("isPrimary").and_then(Value::as_bool).unwrap_or(false),
            is_current: row.get("isCurrent").and_then(Value::as_bool).unwrap_or(false),
            has_avatar: row.get("hasAvatar").and_then(Value::as_bool).unwrap_or(false),
            home_folder: row
                .get("homeFolder")
                .and_then(Value::as_str)
                .map(str::to_string),
            effective_home_folder: row
                .get("effectiveHomeFolder")
                .and_then(Value::as_str)
                .map(str::to_string),
            memory_master_enabled: row
                .get("memoryEnabled")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        });
    }
    // 当前助手：显式目标优先，否则偏好；当前偏好缺失不回退其它助手。
    let current_agent = target
        .agent_id
        .as_deref()
        .and_then(|id| agents.iter().find(|agent| agent.id == id));
    let (agent_name, agent_yuan, home_folder, memory_master_enabled) = match current_agent {
        Some(agent) => (
            agent.name.clone(),
            agent.yuan.clone(),
            agent.effective_home_folder.clone().or_else(|| agent.home_folder.clone()),
            agent.memory_master_enabled,
        ),
        None => (
            "Lingxi".to_string(),
            "lingxi".to_string(),
            None,
            true,
        ),
    };

    let context = match model_context(&state, &target) {
        Ok(context) => context,
        Err(response) => return response,
    };
    let active_model = context.active_model.clone();

    Json(serde_json::to_value(MobileBootstrapResponse {
        locale,
        agent_name,
        user_name,
        current_agent_id: target.agent_id.clone(),
        agent_yuan,
        home_folder,
        cwd_history,
        memory_master_enabled,
        memory_enabled,
        thinking_level: context.thinking_level.clone(),
        permission_mode,
        // 只投影已知外观/排版/聊天布局键；其余偏好不公开给聊天面。
        appearance: preferences.get("appearance").cloned().unwrap_or_else(|| json!({})),
        editor: preferences.get("editor").cloned().unwrap_or_else(|| json!({})),
        chat: preferences.get("chat").cloned().unwrap_or_else(|| json!({})),
        avatars: avatar_availability(&state, target.agent_id.as_deref()),
        agents,
        models: context.models,
        active_model: active_model.clone(),
        current_model: active_model,
        model_state: context.model_state,
        model_generation: context.model_generation,
    })
    .unwrap_or_else(|_| json!({"ok":false,"error":"bootstrap serialization failed"})))
    .into_response()
}
