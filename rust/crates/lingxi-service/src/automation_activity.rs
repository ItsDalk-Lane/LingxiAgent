//! 活动页只投影正式触发、真实运行和统一历史；升格保留同一会话与原始谱系。
use crate::{Principal, ServiceState};
use axum::{
    extract::{Path, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use lingxi_adapters::storage::{
    automation as store, automation_activity as promotion, session_tree,
};
use serde_json::{json, Value};

struct ActivityError {
    status: StatusCode,
    code: &'static str,
    message: String,
}
impl ActivityError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
    fn storage(error: impl std::fmt::Debug) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "activity_read_failed",
            format!("{error:?}"),
        )
    }
    fn missing() -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "activity_not_found",
            "activity not found",
        )
    }
    fn denied() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "activity_access_revoked",
            "activity authorization is unavailable",
        )
    }
    fn response(self) -> Response {
        (
            self.status,
            Json(json!({"error":self.code,"message":self.message})),
        )
            .into_response()
    }
}
struct ActivityRead {
    activity: Value,
    session_id: Option<String>,
    messages: Vec<Value>,
}

async fn check_session(
    state: &ServiceState,
    principal: &Principal,
    session: &str,
    agent: &str,
) -> Result<session_tree::SessionTreeRow, ActivityError> {
    match state
        .sessions()
        .get_for(principal, session)
        .await
        .map_err(ActivityError::storage)?
    {
        crate::sessions::SessionAccess::Ok(_) => {}
        crate::sessions::SessionAccess::NotFound => return Err(ActivityError::missing()),
        crate::sessions::SessionAccess::Forbidden => return Err(ActivityError::denied()),
    }
    let row = session_tree::get_session_row(state.storage(), session)
        .await
        .map_err(ActivityError::storage)?
        .ok_or_else(ActivityError::missing)?;
    if row.lifecycle != "active"
        || row.agent_id != agent
        || row.owner_user_id != lingxi_kernel::LOCAL_OWNER_SUBJECT
    {
        return Err(ActivityError::denied());
    }
    Ok(row)
}

async fn transcript(
    state: &ServiceState,
    principal: &Principal,
    session: &str,
) -> Result<Vec<Value>, ActivityError> {
    // 沿用正式历史投影，不能把后台输入重新制造成用户消息，也不重复加入流式正文。
    let response = crate::history::session_export_route(
        Extension(principal.clone()),
        State(state.clone()),
        Path(session.into()),
        RawQuery(None),
        HeaderMap::new(),
    )
    .await;
    if !response.status().is_success() {
        return Err(ActivityError::new(
            response.status(),
            "activity_session_unavailable",
            "activity history could not be read",
        ));
    }
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .map_err(ActivityError::storage)?;
    let history: Value = serde_json::from_slice(&bytes).map_err(ActivityError::storage)?;
    let items = history["items"]
        .as_array()
        .ok_or_else(|| ActivityError::storage("canonical history items missing"))?;
    let mut messages = Vec::new();
    for item in items {
        let (role, text) = match item["kind"].as_str() {
            Some("user_message") => ("user", item["text"].as_str().unwrap_or("").to_owned()),
            Some("final_message") => {
                let content = item["content"]
                    .as_array()
                    .ok_or_else(|| ActivityError::storage("canonical message content missing"))?;
                let text = content
                    .iter()
                    .filter(|block| block["type"] == "text")
                    .filter_map(|block| block["text"].as_str())
                    .collect::<String>();
                ("assistant", text)
            }
            _ => continue,
        };
        if !text.is_empty() {
            messages.push(json!({"role":role,"content":text}));
        }
    }
    Ok(messages)
}

async fn read_activity(
    state: &ServiceState,
    principal: &Principal,
    id: &str,
    include_promoted: bool,
) -> Result<ActivityRead, ActivityError> {
    if !principal.is_local_owner() {
        return Err(ActivityError::denied());
    }
    let trigger = store::get_trigger(state.storage(), id)
        .await
        .map_err(ActivityError::storage)?
        .ok_or_else(ActivityError::missing)?;
    if !include_promoted
        && promotion::get_promotion(state.storage(), id)
            .await
            .map_err(ActivityError::storage)?
            .is_some()
    {
        return Err(ActivityError::missing());
    }
    let authority=state.storage().query_one_text("SELECT json_object('studioId',studio_id,'actorId',actor_id,'ownerKind',owner_kind,'ownerSubject',owner_subject) FROM automation_jobs_v1 WHERE id=?1",vec![trigger.job_id.clone()]).await.map_err(ActivityError::storage)?.ok_or_else(ActivityError::missing)?;
    let authority: Value = serde_json::from_str(&authority).map_err(ActivityError::storage)?;
    if authority["studioId"] != state.management.identity_ids().1
        || authority["ownerKind"] != "local_user"
        || authority["ownerSubject"] != lingxi_kernel::LOCAL_OWNER_SUBJECT
    {
        return Err(ActivityError::denied());
    }
    let cfg: Value = serde_json::from_str(&trigger.config_json).map_err(ActivityError::storage)?;
    let activity_type = match cfg["origin"].as_str() {
        None | Some("cron") => "cron",
        Some("heartbeat") => "heartbeat",
        _ => return Err(ActivityError::missing()),
    };
    let agent = cfg["actorAgentId"]
        .as_str()
        .or_else(|| authority["actorId"].as_str())
        .ok_or_else(|| ActivityError::storage("activity actor missing"))?;
    if state
        .memory()
        .is_agent_deleted_checked(agent)
        .map_err(ActivityError::storage)?
    {
        return Err(ActivityError::denied());
    }
    let summaries = state
        .memory()
        .list_agent_summaries()
        .map_err(|error| ActivityError::storage(error.message))?;
    let agent_summary = summaries
        .iter()
        .find(|value| value["id"] == agent)
        .ok_or_else(ActivityError::denied)?;
    let agent_name = agent_summary["name"].as_str().unwrap_or(agent);
    let actual = store::get_trigger_run(state.storage(), id)
        .await
        .map_err(ActivityError::storage)?;
    let mut session_id = None;
    let mut messages = Vec::new();
    let mut started = trigger.created_at;
    let mut finished = None;
    let mut run_id = None;
    let mut error = trigger.error.clone();
    let status = if let Some((run, session, status)) = actual {
        if trigger.run_id.as_ref().is_some_and(|bound| bound != &run)
            || trigger
                .session_id
                .as_ref()
                .is_some_and(|bound| bound != &session)
        {
            return Err(ActivityError::storage(
                "activity run binding disagrees with durable cause",
            ));
        }
        let row = check_session(state, principal, &session, agent).await?;
        if let Some(source) = cfg
            .pointer("/executionContext/sourceSessionId")
            .and_then(Value::as_str)
        {
            let source = check_session(state, principal, source, agent).await?;
            if source.authorized_folders != row.authorized_folders {
                return Err(ActivityError::denied());
            }
        }
        let metadata=state.storage().query_one_text("SELECT json_object('startedAt',r.created_at_unix_ms,'updatedAt',r.updated_at_unix_ms,'terminalReason',r.terminal_reason,'origin',l.origin) FROM runs r JOIN run_lineage l ON l.run_id=r.run_id WHERE r.run_id=?1 AND r.session_id=?2",vec![run.clone(),session.clone()]).await.map_err(ActivityError::storage)?.ok_or_else(||ActivityError::storage("activity run disappeared"))?;
        let metadata: Value = serde_json::from_str(&metadata).map_err(ActivityError::storage)?;
        if metadata["origin"] != activity_type {
            return Err(ActivityError::storage(
                "activity origin disagrees with durable Run",
            ));
        }
        started = metadata["startedAt"]
            .as_i64()
            .ok_or_else(|| ActivityError::storage("activity start missing"))?;
        let terminal = [
            "completed",
            "failed",
            "cancelled",
            "interrupted_needs_attention",
        ]
        .contains(&status.as_str());
        if terminal {
            finished = metadata["updatedAt"].as_i64();
        }
        if status == "interrupted_needs_attention" {
            // 重启恢复仍保留真实未知终态，同时给活动页可读的中断原因。
            error = Some("interrupted".to_owned());
        } else if error.is_none() {
            error = metadata["terminalReason"].as_str().map(str::to_owned);
        }
        messages = transcript(state, principal, &session).await?;
        // 读取期间撤销归属或授权时，不把刚读到的内容返回给调用者。
        let current = check_session(state, principal, &session, agent).await?;
        if let Some(source) = cfg
            .pointer("/executionContext/sourceSessionId")
            .and_then(Value::as_str)
        {
            let source = check_session(state, principal, source, agent).await?;
            if source.authorized_folders != current.authorized_folders {
                return Err(ActivityError::denied());
            }
        }
        if state
            .memory()
            .is_agent_deleted_checked(agent)
            .map_err(ActivityError::storage)?
        {
            return Err(ActivityError::denied());
        }
        session_id = Some(session);
        run_id = Some(run);
        match status.as_str() {
            "completed" => "done",
            "failed" => "error",
            "cancelled" if error.as_deref() == Some("timeout") => "error",
            "cancelled" => "cancelled",
            "interrupted_needs_attention" => "unknown",
            _ => "running",
        }
    } else {
        if let Some(source) = cfg
            .pointer("/executionContext/sourceSessionId")
            .and_then(Value::as_str)
        {
            check_session(state, principal, source, agent).await?;
        }
        match trigger.status {
            store::TriggerStatus::Claimed => "running",
            store::TriggerStatus::Running => "unknown",
            store::TriggerStatus::Succeeded => "unknown",
            store::TriggerStatus::Skipped => {
                finished = Some(trigger.updated_at);
                "skipped"
            }
            store::TriggerStatus::Failed => {
                finished = Some(trigger.updated_at);
                "error"
            }
            store::TriggerStatus::Cancelled => {
                finished = Some(trigger.updated_at);
                "cancelled"
            }
            store::TriggerStatus::Unknown => "unknown",
        }
    };
    let mut summary = messages
        .iter()
        .rev()
        .find(|message| message["role"] == "assistant")
        .and_then(|message| message["content"].as_str())
        .map(|text| text.chars().take(400).collect::<String>())
        .or_else(|| error.clone());
    if status == "error" && error.as_deref() == Some("timeout") {
        summary = Some(format!(
            "{} 已超时（超过 20 分钟）",
            cfg["label"].as_str().unwrap_or("后台任务")
        ));
    } else if error.as_deref() == Some("interrupted") {
        summary = Some(format!(
            "{} 已中断：应用已关闭或重启，执行结果需确认",
            cfg["label"].as_str().unwrap_or("后台任务")
        ));
    }
    let locator = session_id.as_deref().map(crate::session_locator::native);
    let activity = json!({"id":id,"type":activity_type,"label":cfg.get("label").filter(|value|value.is_string()),"agentId":agent,"agentName":agent_name,"summary":summary,"startedAt":started,"finishedAt":finished,"status":status,"error":error,"sessionFile":locator,"sessionPath":locator,"sessionId":session_id,"runId":run_id,"jobId":trigger.job_id,"triggerId":id});
    Ok(ActivityRead {
        activity,
        session_id,
        messages,
    })
}

/// 广播使用同一正式投影；没有可见活动时不能编造一条扁平更新。
pub(crate) async fn activity_for_trigger(
    state: &ServiceState,
    id: &str,
) -> Result<Option<Value>, String> {
    match read_activity(
        state,
        &crate::automation::local_owner_principal(),
        id,
        false,
    )
    .await
    {
        Ok(read) => {
            if let Some(entry) = state
                .desk_logs()
                .record_activity(&read.activity, state.clock().now_unix_ms())
            {
                state.app_events().publish("devlog", entry);
            }
            Ok(Some(read.activity))
        }
        Err(error) if matches!(error.status, StatusCode::NOT_FOUND | StatusCode::FORBIDDEN) => {
            Ok(None)
        }
        Err(error) => Err(error.message),
    }
}

/// 列表读取前沿正式取消链收口逾期运行；超时不等于远端结果已知。
async fn reconcile_overdue(
    state: &ServiceState,
    principal: &Principal,
) -> Result<(), ActivityError> {
    const TIMEOUT_MS: u64 = 20 * 60_000;
    for trigger in store::list_pending_triggers(state.storage())
        .await
        .map_err(ActivityError::storage)?
    {
        let read = match read_activity(state, principal, &trigger.trigger_id, false).await {
            Ok(read) => read,
            Err(error) if matches!(error.status, StatusCode::NOT_FOUND | StatusCode::FORBIDDEN) => {
                continue
            }
            Err(error) => return Err(error),
        };
        if read.activity["status"] != "running" {
            continue;
        }
        let Some(run) = read.activity["runId"].as_str() else {
            continue;
        };
        if !promotion::claim_timeout(
            state.storage(),
            &trigger.trigger_id,
            state.clock().now_unix_ms(),
            TIMEOUT_MS,
        )
        .await
        .map_err(ActivityError::storage)?
            && trigger.error.as_deref() != Some("timeout")
        {
            continue;
        }
        state
            .sessions()
            .cancel_run_for(state.storage().as_ref(), state.runs(), principal, run)
            .await
            .map_err(ActivityError::storage)?;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let current = read_activity(state, principal, &trigger.trigger_id, false).await?;
            if current.activity["status"] != "running" {
                if let Some(entry) = state
                    .desk_logs()
                    .record_activity(&current.activity, state.clock().now_unix_ms())
                {
                    state.app_events().publish("devlog", entry);
                }
                state
                    .app_events()
                    .publish("activity_update", json!({"activity":current.activity}));
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                // 清理未完成时保留 running，不伪装成已停止；真实监督器继续接管。
                tracing::warn!(run_id = run, "活动已请求超时取消，仍等待统一清理");
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    }
    Ok(())
}

/// 日记只读取同一正式活动投影；不读取已升格、撤权或其他助手的活动。
pub(crate) async fn diary_activities(
    state: &ServiceState,
    agent: &str,
    start: i64,
    end: i64,
) -> Result<Vec<Value>, String> {
    let principal = crate::automation::local_owner_principal();
    reconcile_overdue(state, &principal)
        .await
        .map_err(|error| error.message)?;
    let triggers = store::list_trigger_history(
        state.storage(),
        None,
        usize::try_from(i64::MAX).map_err(|error| error.to_string())?,
    )
    .await
    .map_err(|error| format!("{error:?}"))?;
    let mut activities = Vec::new();
    for trigger in triggers {
        match read_activity(state, &principal, &trigger.trigger_id, false).await {
            Ok(read)
                if read.activity["agentId"] == agent
                    && read.activity["startedAt"]
                        .as_i64()
                        .is_some_and(|at| at >= start && at <= end) =>
            {
                activities.push(read.activity)
            }
            Ok(_) => {}
            Err(error) if matches!(error.status, StatusCode::NOT_FOUND | StatusCode::FORBIDDEN) => {
            }
            Err(error) => return Err(error.message),
        }
    }
    Ok(activities)
}

pub async fn list_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
) -> Response {
    if !principal.is_local_owner() {
        return ActivityError::denied().response();
    }
    let result = async {
        reconcile_overdue(&state, &principal).await?;
        let triggers = store::list_trigger_history(
            state.storage(),
            None,
            usize::try_from(i64::MAX).map_err(ActivityError::storage)?,
        )
        .await
        .map_err(ActivityError::storage)?;
        let mut activities = Vec::new();
        for trigger in triggers {
            match read_activity(&state, &principal, &trigger.trigger_id, false).await {
                Ok(read) => activities.push(read.activity),
                Err(error)
                    if matches!(error.status, StatusCode::NOT_FOUND | StatusCode::FORBIDDEN) => {}
                Err(error) => return Err(error),
            }
        }
        activities.sort_by(|a, b| {
            b["startedAt"]
                .as_i64()
                .cmp(&a["startedAt"].as_i64())
                .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
        });
        Ok(json!({"activities":activities}))
    }
    .await;
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => error.response(),
    }
}

pub async fn session_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
) -> Response {
    match read_activity(&state, &principal, &id, false).await {
        Ok(read) if read.session_id.is_some() => {
            Json(json!({"activity":read.activity,"messages":read.messages})).into_response()
        }
        Ok(_) => ActivityError::new(
            StatusCode::NOT_FOUND,
            "activity_session_unavailable",
            "no activity session",
        )
        .response(),
        Err(error) => error.response(),
    }
}

pub async fn promote_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
) -> Response {
    let read = match read_activity(&state, &principal, &id, true).await {
        Ok(read) => read,
        Err(error) => return error.response(),
    };
    if read.session_id.is_none() {
        return ActivityError::new(
            StatusCode::NOT_FOUND,
            "activity_session_unavailable",
            "no activity session",
        )
        .response();
    }
    match state
        .session_admin()
        .promote_automation_activity(&principal, &id, state.clock().now_unix_ms())
        .await
    {
        Ok(session) => {
            Json(json!({"ok":true,"sessionPath":crate::session_locator::native(&session),"sessionId":session,"agentId":read.activity["agentId"]})).into_response()
        }
        Err(crate::session_admin::SessionAdminError::Busy) => ActivityError::new(
            StatusCode::CONFLICT,
            "session_busy",
            "activity is still running",
        )
        .response(),
        Err(crate::session_admin::SessionAdminError::NotFound) => {
            ActivityError::missing().response()
        }
        Err(error) => ActivityError::new(
            StatusCode::CONFLICT,
            "activity_promote_failed",
            format!("{error:?}"),
        )
        .response(),
    }
}

fn summary_response(summary: Option<String>) -> Response {
    Json(lingxi_protocol::ActivitySummaryResponse::Summary { summary }).into_response()
}

fn summary_error(error: impl Into<String>) -> Response {
    Json(lingxi_protocol::ActivitySummaryResponse::Error {
        error: error.into(),
    })
    .into_response()
}

/// DevTools 快速摘要只读真实活动材料，不覆写原活动摘要或触发新的计划执行。
pub async fn summarize_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> Response {
    if !principal.is_local_owner() {
        return ActivityError::denied().response();
    }
    let body: lingxi_protocol::ActivitySummaryRequest = match serde_json::from_value(body) {
        Ok(body) => body,
        Err(_) => return summary_error("id must be a string"),
    };
    let preferences = match state.memory().read_preferences_root() {
        Ok(value) => value,
        Err(error) => return summary_error(error),
    };
    let zh = preferences["locale"]
        .as_str()
        .unwrap_or("zh-CN")
        .starts_with("zh");
    let mut repair = None;
    let word_regex =
        regex::Regex::new(r"[\p{L}\p{N}]+(?:['’.-][\p{L}\p{N}]+)*").expect("固定摘要词数表达式");
    // 现役长度契约最多修复两次；每次真实调用独立记账，不截断后冒充模型原答。
    for attempt in 0..=2 {
        let response = summarize_once(&state, &principal, &body, repair.as_deref()).await;
        if !response.status().is_success() {
            return response;
        }
        let bytes = match axum::body::to_bytes(response.into_body(), 1024 * 1024).await {
            Ok(bytes) => bytes,
            Err(error) => return ActivityError::storage(error).response(),
        };
        let value: Value = match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(error) => return ActivityError::storage(error).response(),
        };
        let Some(text) = value["summary"].as_str() else {
            return Json(value).into_response();
        };
        let text = text.trim();
        let length = if zh {
            text.chars().count()
        } else {
            word_regex.find_iter(text).count()
        };
        if length >= 1 && length <= (if zh { 60 } else { 30 }) {
            return summary_response(Some(text.to_owned()));
        }
        if attempt < 2 {
            repair = Some(text.to_owned());
        }
    }
    summary_error("activity summary length repair exhausted")
}

async fn summarize_once(
    state: &ServiceState,
    principal: &Principal,
    body: &lingxi_protocol::ActivitySummaryRequest,
    repair: Option<&str>,
) -> Response {
    use lingxi_adapters::models::auxiliary::{AuxiliaryExecutor, AuxiliaryRequest, AuxiliaryStep};
    use lingxi_kernel::{
        model_exchange::{AuxiliarySlot, ModelGatewayError, ModelOperation, ModelRouteRequest},
        ports::StoragePort,
        usage::{CallOutcome, ModelCallUsageRecord, ReportedUsage},
    };
    if !principal.is_local_owner() {
        return ActivityError::denied().response();
    }
    let Some(id) = body.id.as_deref().filter(|id| !id.trim().is_empty()) else {
        return summary_error("id required");
    };
    let read = match read_activity(state, principal, id, false).await {
        Ok(read) => read,
        Err(error) if error.status == StatusCode::NOT_FOUND => return summary_response(None),
        Err(error) => return error.response(),
    };
    if read.session_id.is_none() {
        return summary_response(None);
    }
    let trigger = match store::get_trigger(state.storage(), id).await {
        Ok(Some(trigger)) => trigger,
        Ok(None) => return summary_response(None),
        Err(error) => return ActivityError::storage(error).response(),
    };
    let config: Value = match serde_json::from_str(&trigger.config_json) {
        Ok(config) => config,
        Err(error) => return ActivityError::storage(error).response(),
    };
    let assistant = read
        .messages
        .iter()
        .filter(|message| message["role"] == "assistant")
        .filter_map(|message| message["content"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let context = config["prompt"].as_str().unwrap_or("");
    if context.is_empty() && assistant.is_empty() {
        return summary_response(None);
    }
    let Some(gateway) = state.model_gateway().cloned() else {
        return summary_error("summarize model unavailable");
    };
    let requested = ModelOperation::Auxiliary(AuxiliarySlot::Summarize);
    let operation = match gateway.resolve_dispatch(&ModelRouteRequest::for_operation(requested)) {
        Ok(_) => requested,
        Err(ModelGatewayError::RouteNotConfigured { .. }) => ModelOperation::Chat,
        Err(error) => return summary_error(error.to_string()),
    };
    if let Err(error) = gateway.resolve_dispatch(&ModelRouteRequest::for_operation(operation)) {
        return summary_error(error.to_string());
    }
    let (Some(credentials), Some(network)) = (
        state.credential_service().cloned(),
        state.network_plane().cloned(),
    ) else {
        return summary_error("summarize model unavailable");
    };
    let executor = match AuxiliaryExecutor::new_with_network(
        gateway,
        credentials,
        lingxi_kernel::toolcatalog::SchemaBudget::default(),
        network,
    ) {
        Ok(executor) => executor,
        Err(error) => return summary_error(error.to_string()),
    };
    let agent = read.activity["agentId"].as_str().unwrap_or("");
    let call_id = format!(
        "activity-summary-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    );
    let _permit = match state
        .runs()
        .quotas()
        .acquire(crate::quotas::QuotaResource::Model, agent, &call_id)
        .await
    {
        Ok(permit) => permit,
        Err(error) => return summary_error(error.to_string()),
    };
    let preferences = match state.memory().read_preferences_root() {
        Ok(value) => value,
        Err(error) => return summary_error(error),
    };
    let zh = preferences["locale"]
        .as_str()
        .unwrap_or("zh-CN")
        .starts_with("zh");
    // 现役按 UTF-16 截断材料；不能截出半个代理项，Rust 在边界处保留完整字符。
    let clip = |text: &str| {
        let mut units = 0;
        text.chars()
            .take_while(|ch| {
                units += ch.len_utf16();
                units <= 400
            })
            .collect::<String>()
    };
    let prompt = if zh {
        format!(
            "巡检上下文：\n{}\n\nAgent 回复：\n{}",
            clip(context),
            clip(&assistant)
        )
    } else {
        format!(
            "Patrol context：\n{}\n\nAgent reply：\n{}",
            clip(context),
            clip(&assistant)
        )
    };
    let prompt = if let Some(previous) = repair {
        format!("{prompt}\n\nPrevious summary: {previous}\nRewrite only the summary within {}. Preserve its meaning; output no explanations.",if zh {"1-60 Chinese characters, aiming for 30"}else{"1-30 words, aiming for 15"})
    } else {
        prompt
    };
    let request=AuxiliaryRequest {
        prompt,images:vec![],max_output_tokens:Some(500),
        deadline_unix_ms:Some(state.clock().now_unix_ms().saturating_add(20_000)),
        system_prompt:Some(if zh {"根据 Agent 的巡检上下文和执行结果，用一两句话概括它做了什么。目标约 30 字，可在 18-60 字之间自然浮动。中文，直接输出。"} else {"Based on the Agent's patrol context and execution results, summarize what it did in one or two sentences. Aim for about 15 words; 9-30 words is acceptable. English, output directly."}.into()),
        prior:vec![],tools:None,
    };
    let ctx = lingxi_kernel::RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new(read.session_id.as_deref().unwrap_or("")),
        run_id: lingxi_protocol::RunId::new(&call_id),
        attempt: lingxi_protocol::AttemptId::new(&call_id),
        generation: 1,
    };
    let model_call_id = lingxi_protocol::ModelCallId::new(&call_id);
    let started = state.clock().now_unix_ms();
    let call = executor.complete_step_for_operation(
        &ctx,
        operation,
        "activity_summary_quick",
        &model_call_id,
        &request,
    );
    let authority = async {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            if let Err(error) = read_activity(state, principal, id, false).await {
                break error.message;
            }
        }
    };
    let result = tokio::select! {
        result=call=>Some(result),
        reason=authority=>{tracing::warn!(activity_id=id,%reason,"活动摘要授权撤销，取消辅助调用");None},
        _=tokio::time::sleep(std::time::Duration::from_secs(20))=>None,
    };
    let (usage, attempts, provider, model, protocol, outcome) = match &result {
        Some(Ok(AuxiliaryStep::Text(answer))) => (
            answer.usage_report.clone(),
            Some(answer.transport_attempts),
            answer.served_by.provider.clone(),
            answer.served_by.model.clone(),
            answer.served_protocol.clone(),
            CallOutcome::Succeeded,
        ),
        Some(Ok(AuxiliaryStep::ToolRequests {
            usage_report,
            transport_attempts,
            served_by,
            served_protocol,
            ..
        })) => (
            usage_report.clone(),
            Some(*transport_attempts),
            served_by.provider.clone(),
            served_by.model.clone(),
            served_protocol.clone(),
            CallOutcome::Failed,
        ),
        Some(Err(error)) => (
            error.usage_report.clone(),
            Some(error.transport_attempts),
            error.served_by.provider.clone(),
            error.served_by.model.clone(),
            error.served_protocol.clone(),
            CallOutcome::Failed,
        ),
        // 中止时无法确认实际服务身份、发送次数及远端用量，明确记为未知。
        None => (
            ReportedUsage::Unknown,
            None,
            "unknown".into(),
            "unknown".into(),
            None,
            CallOutcome::Cancelled,
        ),
    };
    let settled = state.clock().now_unix_ms();
    if let Err(error) = state
        .storage()
        .record_model_call_usage(
            ModelCallUsageRecord {
                session_id: None,
                run_id: None,
                attempt: None,
                model_call_id: call_id,
                purpose: "auxiliary.summarize".into(),
                origin: "activity_summary_quick".into(),
                parent_run_id: read.activity["runId"].as_str().map(str::to_owned),
                cause_ref: Some(format!("activity:{id}")),
                parent_tool_call_id: None,
                provider,
                model,
                protocol: protocol.unwrap_or_else(|| "unknown".into()),
                usage: match &usage {
                    ReportedUsage::Known(value) => Some(value.clone()),
                    _ => None,
                },
                invalid_detail: match usage {
                    ReportedUsage::Invalid { detail } => Some(detail),
                    _ => None,
                },
                transport_attempts: attempts,
                outcome,
                started_at_unix_ms: Some(started),
                settled_at_unix_ms: Some(settled),
                emitted_tool_calls: vec![],
                cost_basis: None,
            },
            settled,
        )
        .await
    {
        return ActivityError::storage(error).response();
    }
    // 模型返回和撤权同时发生时，结算真实用量，但不交出已经失去权限的摘要。
    if let Err(error) = read_activity(state, principal, id, false).await {
        return error.response();
    }
    match result {
        Some(Ok(AuxiliaryStep::Text(answer))) if !answer.text.trim().is_empty() => {
            summary_response(Some(answer.text))
        }
        Some(Ok(_)) => summary_error("activity summary returned no text"),
        Some(Err(error)) => summary_error(error.to_string()),
        None => summary_error("activity summary cancelled; remote result unknown"),
    }
}
