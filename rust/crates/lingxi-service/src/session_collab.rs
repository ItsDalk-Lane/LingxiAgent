//! 跨会话确认卡沿既有工具面、真实会话管理和后台受理链执行。
use crate::{history::history_content::SessionCollabTools, Principal, ServiceState};
use axum::{
    extract::{rejection::JsonRejection, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use lingxi_adapters::storage::{
    conversations as phone_store, conversations_collab as store, session_tree, RunDatabase,
};
use lingxi_kernel::{
    ports::{StorageError, StoragePort},
    subagent::SessionPermissionMode,
    LOCAL_OWNER_SUBJECT,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

pub fn available_models(state: &ServiceState) -> Result<Vec<String>, String> {
    let Some(gateway) = state.model_gateway() else {
        return Ok(vec![]);
    };
    let mut models = gateway.chat_model_keys();
    models.sort();
    models.dedup();
    Ok(models)
}
/// 组合根在State构成后调用；工具只抽取窄依赖，不持有State或运行闭包。
pub fn production(state: &ServiceState) -> Result<SessionCollabTools, String> {
    Ok(SessionCollabTools::production(
        state,
        available_models(state)?,
    ))
}

pub fn register(
    registry: &lingxi_kernel::toolcatalog::ToolRegistry,
    gateway: &crate::toolgateway::ToolInvocationGateway,
    tools: Arc<SessionCollabTools>,
    budget: &lingxi_kernel::toolcatalog::SchemaBudget,
) {
    use lingxi_kernel::toolcatalog::{
        Availability, DeclaredPermission, PermissionContract, PermissionKind, ToolManifest,
        ToolOrigin,
    };
    let schema = json!({"type":"object","properties":{"action":{"type":"string","enum":["?","list","read","send","create"]},"sessionId":{"type":"string"},"query":{"type":"string"},"mode":{"type":"string","enum":["summary","transcript"]},"cursor":{"type":"string"},"count":{"type":"number"},"message":{"type":"string"},"agent":{"type":"string"},"model":{"type":"string"},"title":{"type":"string"}},"required":["action"],"additionalProperties":false});
    let manifest=ToolManifest{origin:ToolOrigin::FirstParty,local_name:"session".into(),display_name:"Session Collaboration".into(),aliases:vec![],version:"1".into(),description:"Cross-session collaboration: list/read authorized sessions; send/create only produce a user confirmation card.".into(),input_schema:lingxi_protocol::ToolSchemaDocument{dialect:"json-schema/2020-12".into(),schema},output_schema:None,permission:PermissionContract{kind:PermissionKind::Read,capability_base:"session.execute".into()},availability:Availability::Available,timeout_ms:Some(30_000),max_concurrency:Some(16),declared_permission:DeclaredPermission::None,recovery:lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE};
    registry
        .register(manifest, budget)
        .expect("会话协作工具契约必须有效");
    gateway.bind_executor(
        lingxi_kernel::toolcatalog::ToolTargetId::parse("tool:first-party:session"),
        tools,
        "session collaboration canonical tool",
    );
}

#[derive(Debug)]
pub struct CollabRouteError {
    status: StatusCode,
    code: &'static str,
    message: String,
}
impl CollabRouteError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
}
impl IntoResponse for CollabRouteError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"ok":false,"code":self.code,"error":self.message})),
        )
            .into_response()
    }
}
impl From<StorageError> for CollabRouteError {
    fn from(e: StorageError) -> Self {
        let status = match e {
            StorageError::Conflict { .. } => StatusCode::CONFLICT,
            StorageError::InvalidRequest { .. } => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self::new(status, "session_collab_storage", e.to_string())
    }
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplyRequest {
    pub suggestion_id: String,
    pub draft: Option<Value>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RejectRequest {
    pub suggestion_id: String,
    pub source_session_id: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DraftsQuery {
    pub source_session_id: String,
}
fn owner(p: &Principal) -> Result<(), CollabRouteError> {
    if !p.is_local_owner() || p.studio_id.as_deref().is_some_and(|s| s != "local") {
        return Err(CollabRouteError::new(
            StatusCode::FORBIDDEN,
            "studio_owner_required",
            "会话协作确认需要当前工作室所有者身份",
        ));
    }
    Ok(())
}
async fn authorized_draft(
    state: &ServiceState,
    id: &str,
) -> Result<store::CollabDraft, CollabRouteError> {
    let draft = store::get_collab_draft(state.storage(), id)
        .await?
        .ok_or_else(|| {
            CollabRouteError::new(StatusCode::NOT_FOUND, "draft_expired", "草稿不存在或已失效")
        })?;
    if draft.input.studio_id != "local" || draft.input.owner_subject != LOCAL_OWNER_SUBJECT {
        return Err(CollabRouteError::new(
            StatusCode::FORBIDDEN,
            "draft_scope",
            "草稿不属于当前工作室",
        ));
    }
    let source = session_tree::get_session_row(state.storage(), &draft.input.source_session_id)
        .await?
        .ok_or_else(|| {
            CollabRouteError::new(
                StatusCode::NOT_FOUND,
                "source_session_missing",
                "草稿来源会话不存在",
            )
        })?;
    if source.owner_user_id != LOCAL_OWNER_SUBJECT || source.lifecycle != "active" {
        return Err(CollabRouteError::new(
            StatusCode::FORBIDDEN,
            "source_session_scope",
            "草稿来源会话当前不可确认",
        ));
    }
    Ok(draft)
}
fn edited_payload(
    d: &store::CollabDraft,
    edited: Option<Value>,
) -> Result<String, CollabRouteError> {
    if edited.is_none() {
        return Ok(d
            .frozen_apply_json
            .clone()
            .unwrap_or_else(|| d.input.draft_json.clone()));
    }
    let mut value: Value = serde_json::from_str(&d.input.draft_json).map_err(|_| {
        CollabRouteError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "draft_corrupt",
            "已保存草稿损坏",
        )
    })?;
    let patch = edited.and_then(|v| v.as_object().cloned()).ok_or_else(|| {
        CollabRouteError::new(StatusCode::BAD_REQUEST, "draft_shape", "编辑草稿必须是对象")
    })?;
    let map = value.as_object_mut().ok_or_else(|| {
        CollabRouteError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "draft_corrupt",
            "已保存草稿不是对象",
        )
    })?;
    for (key, value) in patch {
        map.insert(key, value);
    }
    Ok(store::normalize_collab_payload(
        &d.input.kind,
        &value.to_string(),
    )?)
}
fn draft_payload(d: &store::CollabDraft) -> Result<Value, StorageError> {
    serde_json::from_str(
        d.frozen_apply_json
            .as_deref()
            .unwrap_or(&d.input.draft_json),
    )
    .map_err(|_| StorageError::Corrupted {
        detail: "session collaboration frozen payload malformed".into(),
    })
}
fn invalid(detail: impl Into<String>) -> StorageError {
    StorageError::InvalidRequest {
        detail: detail.into(),
    }
}
fn current_model(state: &ServiceState, payload: &Value) -> Result<(), String> {
    if let Some(model) = payload
        .get("model")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        if !available_models(state)?.iter().any(|m| m == model) {
            return Err(format!(
                "session_create_invalid_model: model is not currently available: {model}"
            ));
        }
    }
    Ok(())
}
fn current_agent(state: &ServiceState, payload: &Value) -> Result<(), String> {
    let agent = payload["agentId"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("session_create_invalid_agent")?;
    state
        .memory()
        .agent_dir_checked(agent)
        .map_err(|e| e.message)?;
    if state
        .memory()
        .is_agent_deleted_checked(agent)
        .map_err(|e| format!("{e:?}"))?
    {
        return Err("session_create_invalid_agent: agent deleted".into());
    }
    Ok(())
}
/// 确认卡不会洗掉来源手机的成员和开关限制；每次确认与实际受理都重新核实。
async fn source_authority(state: &ServiceState, draft: &store::CollabDraft) -> Result<(), String> {
    let source = session_tree::get_session_row(state.storage(), &draft.input.source_session_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("collaboration source session missing")?;
    let run = state
        .storage()
        .load_run(&lingxi_protocol::RunId::new(&draft.input.source_run_id))
        .await
        .map_err(|e| e.to_string())?
        .ok_or("collaboration source Run missing")?;
    if source.owner_user_id != draft.input.owner_subject
        || source.lifecycle != "active"
        || run.session_id.as_str() != source.session_id
    {
        return Err("collaboration source identity changed".into());
    }
    let source_owned=state.storage().query_one_text("SELECT run_id FROM runs WHERE run_id=?1 AND session_id=?2 AND owner_subject=?3 AND owner_kind IN ('local_user','device')",vec![draft.input.source_run_id.clone(),draft.input.source_session_id.clone(),draft.input.owner_subject.clone()]).await.map_err(|e|e.to_string())?.is_some();
    if !source_owned {
        return Err("collaboration source Run ownership changed".into());
    }
    let api = crate::conversations::ConversationApi::from_state(state);
    let Some(delivery) = api
        .source_delivery(Some(&draft.input.source_run_id))
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    if delivery.input.owner_subject != draft.input.owner_subject
        || matches!(
            run.status,
            lingxi_protocol::RunStatus::Failed
                | lingxi_protocol::RunStatus::Cancelled
                | lingxi_protocol::RunStatus::Cancelling
                | lingxi_protocol::RunStatus::InterruptedNeedsAttention
        )
        || !api.delivery_current(&delivery).map_err(|e| e.to_string())?
        || matches!(
            delivery.status,
            phone_store::PhoneDeliveryStatus::Failed
                | phone_store::PhoneDeliveryStatus::Cancelled
                | phone_store::PhoneDeliveryStatus::Unknown
        )
    {
        return Err("phone source authority revoked".into());
    }
    let (root_id, root_sid, status) =
        phone_store::get_delivery_run(state.storage(), &delivery.input.delivery_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("phone source actual Run missing")?;
    let root = state
        .storage()
        .load_run(&lingxi_protocol::RunId::new(&root_id))
        .await
        .map_err(|e| e.to_string())?
        .ok_or("phone source root Run missing")?;
    let root_session = session_tree::get_session_row(state.storage(), &root_sid)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("phone source root session missing")?;
    if root.session_id.as_str() != root_sid
        || matches!(
            status.as_str(),
            "failed" | "cancelled" | "cancelling" | "interrupted_needs_attention"
        )
        || matches!(
            root.status,
            lingxi_protocol::RunStatus::Failed
                | lingxi_protocol::RunStatus::Cancelled
                | lingxi_protocol::RunStatus::Cancelling
                | lingxi_protocol::RunStatus::InterruptedNeedsAttention
        )
        || root_session.owner_user_id != draft.input.owner_subject
        || root_session.agent_id != delivery.input.actor_id
        || root_session.lifecycle != "active"
    {
        return Err("phone source root authority unavailable".into());
    }
    if !lingxi_adapters::storage::bridge_reads::has_run_identity(
        state.storage(),
        &root_id,
        &root_sid,
        "local_user",
        &draft.input.owner_subject,
    )
    .await
    .map_err(|e| e.to_string())?
    {
        return Err("phone source root identity mismatch".into());
    }
    let binding = phone_store::get_phone_binding(
        state.storage(),
        &delivery.input.actor_id,
        &delivery.input.conversation_id,
        &delivery.input.owner_subject,
    )
    .await
    .map_err(|e| e.to_string())?
    .ok_or("phone source current binding missing")?;
    if binding.input.session_id != root_sid
        || delivery.input.input_json["bindingEpoch"]
            .as_u64()
            .is_some_and(|epoch| epoch != binding.epoch)
    {
        return Err("phone source binding changed".into());
    }
    let current = api
        .settings(&delivery.input.actor_id, &delivery.input.conversation_id)
        .map_err(|e| e.to_string())?
        .permission();
    let frozen: crate::phone_settings::PhoneSettings =
        serde_json::from_value(delivery.input.input_json["settings"].clone())
            .map_err(|e| e.to_string())?;
    let frozen = frozen.permission();
    let source_mode = mode(source.permission_mode.as_deref()).map_err(|e| format!("{e:?}"))?;
    let root_mode = mode(root_session.permission_mode.as_deref()).map_err(|e| format!("{e:?}"))?;
    let bound_mode = mode(Some(&binding.input.mode)).map_err(|e| format!("{e:?}"))?;
    if bound_mode != frozen
        || root_mode != frozen
        || (current == SessionPermissionMode::ReadOnly
            && [source_mode, root_mode, bound_mode, frozen]
                .into_iter()
                .any(|mode| mode != SessionPermissionMode::ReadOnly))
    {
        return Err("phone source permission narrowed or frozen mode changed".into());
    }
    Ok(())
}
async fn publish_decision(state: &ServiceState, id: &str) -> bool {
    match store::record_collab_decision(state.storage(), id, state.clock().now_unix_ms()).await {
        Ok(Some(decision)) => {
            let source = store::get_collab_draft(state.storage(), id)
                .await
                .ok()
                .flatten()
                .map(|d| d.input.source_session_id);
            state.app_events().publish("session_collab_decision",json!({"sessionId":source,"sourceSessionId":source,"suggestionId":id,"result":decision["result"],"decision":decision}));
            true
        }
        Ok(None) => false,
        Err(error) => {
            tracing::warn!(suggestion_id=id,error=%error,"session collaboration decision was not persisted");
            false
        }
    }
}
fn response(draft: &store::CollabDraft, decision: bool) -> Response {
    let result = store::collab_result(draft);
    if draft.delivery_status == store::CollabDeliveryStatus::Unknown {
        return(StatusCode::CONFLICT,Json(json!({"ok":false,"code":"delivery_unknown","error":draft.error,"result":result,"decisionPersisted":decision}))).into_response();
    }
    if draft.delivery_status == store::CollabDeliveryStatus::Failed {
        return(StatusCode::INTERNAL_SERVER_ERROR,Json(json!({"ok":false,"partialSuccess":draft.target_created,"code":if draft.target_created{"first_message_failed"}else{"apply_failed"},"error":draft.error,"sessionId":draft.target_session_id,"result":result,"decisionPersisted":decision}))).into_response();
    }
    Json(json!({"ok":true,"result":result,"decisionPersisted":decision})).into_response()
}
pub async fn apply_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    body: Result<Json<ApplyRequest>, JsonRejection>,
) -> Result<Response, CollabRouteError> {
    owner(&principal)?;
    let Json(body) = body.map_err(|_| {
        CollabRouteError::new(StatusCode::BAD_REQUEST, "invalid_json", "无效的确认请求")
    })?;
    let draft = authorized_draft(&state, &body.suggestion_id).await?;
    if draft.state == store::CollabDraftState::Rejected {
        return Err(CollabRouteError::new(
            StatusCode::NOT_FOUND,
            "draft_expired",
            "草稿已拒绝",
        ));
    }
    if draft.state == store::CollabDraftState::Pending
        && draft.frozen_apply_json.is_none()
        && draft.input.expires_at <= state.clock().now_unix_ms() as i64
    {
        return Err(CollabRouteError::new(
            StatusCode::NOT_FOUND,
            "draft_expired",
            "草稿已过期",
        ));
    }
    if !matches!(
        draft.delivery_status,
        store::CollabDeliveryStatus::Accepted | store::CollabDeliveryStatus::Unknown
    ) {
        if let Err(reason) = source_authority(&state, &draft).await {
            store::fail_queued_collab_draft(
                state.storage(),
                &draft.input.id,
                &reason,
                state.clock().now_unix_ms(),
            )
            .await?;
            publish_decision(&state, &draft.input.id).await;
            return Err(CollabRouteError::new(
                StatusCode::FORBIDDEN,
                "source_authority_revoked",
                reason,
            ));
        }
    }
    let edited = edited_payload(&draft, body.draft)?;
    if draft.state == store::CollabDraftState::Pending {
        let payload: Value = serde_json::from_str(&edited).map_err(|_| {
            CollabRouteError::new(StatusCode::BAD_REQUEST, "draft_shape", "草稿无效")
        })?;
        if draft.input.kind == store::CollabDraftKind::Create {
            current_agent(&state, &payload)
                .and_then(|_| current_model(&state, &payload))
                .map_err(|e| CollabRouteError::new(StatusCode::BAD_REQUEST, "invalid_target", e))?;
        }
    }
    let claim = store::claim_collab_apply(
        state.storage(),
        &body.suggestion_id,
        "local",
        LOCAL_OWNER_SUBJECT,
        &edited,
        state.clock().now_unix_ms(),
    )
    .await?;
    let persisted = publish_decision(&state, &body.suggestion_id).await;
    Ok(response(&claim.draft, persisted))
}
pub async fn reject_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    body: Result<Json<RejectRequest>, JsonRejection>,
) -> Result<Response, CollabRouteError> {
    owner(&principal)?;
    let Json(body) = body.map_err(|_| {
        CollabRouteError::new(StatusCode::BAD_REQUEST, "invalid_json", "无效的拒绝请求")
    })?;
    let draft = authorized_draft(&state, &body.suggestion_id).await?;
    if body
        .source_session_id
        .as_ref()
        .is_some_and(|s| s != &draft.input.source_session_id)
    {
        return Err(CollabRouteError::new(
            StatusCode::FORBIDDEN,
            "source_mismatch",
            "请求来源与真实草稿不一致",
        ));
    }
    store::reject_collab_draft(
        state.storage(),
        &body.suggestion_id,
        "local",
        LOCAL_OWNER_SUBJECT,
        state.clock().now_unix_ms(),
    )
    .await?;
    Ok(Json(
        json!({"ok":true,"decisionPersisted":publish_decision(&state,&body.suggestion_id).await}),
    )
    .into_response())
}
pub async fn drafts_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<DraftsQuery>,
) -> Result<Response, CollabRouteError> {
    owner(&principal)?;
    let source = session_tree::get_session_row(state.storage(), &query.source_session_id)
        .await?
        .ok_or_else(|| {
            CollabRouteError::new(StatusCode::NOT_FOUND, "source_missing", "来源会话不存在")
        })?;
    if source.owner_user_id != LOCAL_OWNER_SUBJECT {
        return Err(CollabRouteError::new(
            StatusCode::FORBIDDEN,
            "source_scope",
            "来源会话不属于当前所有者",
        ));
    }
    let rows =
        store::list_collab_drafts_for_session(state.storage(), &query.source_session_id).await?;
    let drafts:Vec<Value>=rows.into_iter().map(|d|json!({"suggestionId":d.input.id,"kind":match d.input.kind{store::CollabDraftKind::Send=>"session_send_draft",store::CollabDraftKind::Create=>"session_create_draft"},"sourceSessionId":d.input.source_session_id,"draft":serde_json::from_str::<Value>(&d.input.draft_json).unwrap_or(Value::Null),"frozenDraft":d.frozen_apply_json.as_ref().and_then(|s|serde_json::from_str::<Value>(s).ok()),"state":d.state,"status":if d.state==store::CollabDraftState::Rejected{"rejected"}else if d.state==store::CollabDraftState::Pending&&d.frozen_apply_json.is_none(){"pending"}else{"approved"},"result":store::collab_result(&d),"decisionPersisted":d.decision_persisted})).collect();
    Ok(Json(json!({"drafts":drafts})).into_response())
}

async fn prepare_target(
    state: &ServiceState,
    draft: &store::CollabDraft,
) -> Result<String, String> {
    let payload = draft_payload(draft).map_err(|e| e.to_string())?;
    let target = draft
        .reserved_target_id
        .as_deref()
        .ok_or("collaboration target reservation missing")?;
    if draft.input.kind == store::CollabDraftKind::Create {
        current_agent(state, &payload)?;
        current_model(state, &payload)?;
        let agent = payload["agentId"].as_str().ok_or("created agent missing")?;
        let row = session_tree::get_session_row(state.storage(), target)
            .await
            .map_err(|e| e.to_string())?;
        match row {
            Some(row)
                if row.owner_user_id == draft.input.owner_subject
                    && row.agent_id == agent
                    && row.lifecycle == "active" => {}
            Some(_) => {
                return Err("reserved collaboration target conflicts with a real session".into())
            }
            None => state
                .session_admin()
                .create_session(
                    target,
                    agent,
                    &draft.input.owner_subject,
                    payload["title"]
                        .as_str()
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or("协作会话"),
                    None,
                    &[],
                    true,
                    state.clock().now_unix_ms(),
                )
                .await
                .map_err(|e| format!("{e:?}"))?,
        }
    }
    store::bind_collab_target(
        state.storage(),
        &draft.input.id,
        draft.attempt,
        target,
        draft.input.kind == store::CollabDraftKind::Create,
        state.clock().now_unix_ms(),
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(target.to_owned())
}
/// 统一ConversationHost调用一次；忙会话保持真实持久queued，绝不内存假确认。
pub async fn tick(state: &ServiceState) -> Result<(), StorageError> {
    for draft in store::list_queued_collab_drafts(state.storage()).await? {
        let id = draft.input.id.clone();
        if let Err(reason) = source_authority(state, &draft).await {
            store::fail_queued_collab_draft(
                state.storage(),
                &id,
                &reason,
                state.clock().now_unix_ms(),
            )
            .await?;
            publish_decision(state, &id).await;
            continue;
        }
        let claim =
            match store::claim_collab_delivery(state.storage(), &id, state.clock().now_unix_ms())
                .await
            {
                Ok(c) => c,
                Err(error) => {
                    store::fail_queued_collab_draft(
                        state.storage(),
                        &id,
                        &error.to_string(),
                        state.clock().now_unix_ms(),
                    )
                    .await?;
                    publish_decision(state, &id).await;
                    continue;
                }
            };
        if !claim.claimed {
            continue;
        }
        let draft = claim.draft;
        let target = match prepare_target(state, &draft).await {
            Ok(target) => target,
            Err(reason) => {
                store::settle_collab_delivery(
                    state.storage(),
                    &id,
                    draft.attempt,
                    store::CollabDeliveryResult::FailedBeforeAcceptance { reason },
                    state.clock().now_unix_ms(),
                )
                .await?;
                publish_decision(state, &id).await;
                continue;
            }
        };
        let payload = draft_payload(&draft)?;
        let input = match draft.input.kind {
            store::CollabDraftKind::Send => payload["message"].as_str(),
            store::CollabDraftKind::Create => payload["firstMessage"].as_str(),
        }
        .ok_or_else(|| invalid("collaboration body missing"))?;
        if let Err(reason) = source_authority(state, &draft).await {
            store::settle_collab_delivery(
                state.storage(),
                &id,
                draft.attempt,
                store::CollabDeliveryResult::FailedBeforeAcceptance { reason },
                state.clock().now_unix_ms(),
            )
            .await?;
            publish_decision(state, &id).await;
            continue;
        }
        let principal = Principal::scheduler_owner();
        let accepted = state
            .sessions()
            .execute_collaboration_for(
                state.storage(),
                state.events(),
                state.runs(),
                state.background(),
                &principal,
                &target,
                input,
                &draft.input.source_session_id,
                &draft.input.source_run_id,
                &id,
                &draft.input.source_tool_call_id,
                state.clock().now_unix_ms(),
            )
            .await;
        let result = match accepted {
            Ok(accepted) => store::CollabDeliveryResult::Accepted {
                run_id: accepted.run_id,
            },
            Err(
                crate::sessions::SessionExecuteError::Busy
                | crate::sessions::SessionExecuteError::AdmissionInFlight { .. }
                | crate::sessions::SessionExecuteError::ShuttingDown,
            ) => store::CollabDeliveryResult::RetryQueued {
                reason: "target_busy_or_intake_closed".into(),
            },
            Err(error) => {
                if let Some((run, _, _)) = store::get_collab_run(state.storage(), &id).await? {
                    store::CollabDeliveryResult::Accepted { run_id: run }
                } else if matches!(error, crate::sessions::SessionExecuteError::Storage(_)) {
                    store::CollabDeliveryResult::Unknown {
                        reason: format!("delivery acceptance unconfirmed: {error:?}"),
                    }
                } else {
                    store::CollabDeliveryResult::FailedBeforeAcceptance {
                        reason: format!("delivery not accepted: {error:?}"),
                    }
                }
            }
        };
        store::settle_collab_delivery(
            state.storage(),
            &id,
            draft.attempt,
            result,
            state.clock().now_unix_ms(),
        )
        .await?;
        publish_decision(state, &id).await;
    }
    for draft in store::list_unrecorded_collab_decisions(state.storage()).await? {
        publish_decision(state, &draft.input.id).await;
    }
    Ok(())
}
/// 启动或宿主停止时先找真实因果Run；开始过但没有事实的收据保持Unknown。
pub async fn recover(state: &ServiceState) -> Result<(), StorageError> {
    let mut drafts = store::list_started_collab_drafts(state.storage()).await?;
    drafts.extend(store::list_unknown_collab_drafts(state.storage()).await?);
    for mut draft in drafts {
        // 建会话与受理Run之间崩溃时，只认已经存在的预留会话，不再新建。
        if draft.target_session_id.is_none() {
            if let Some(target) = draft.reserved_target_id.as_deref() {
                if session_tree::get_session_row(state.storage(), target)
                    .await?
                    .is_some()
                {
                    draft = store::bind_collab_target(
                        state.storage(),
                        &draft.input.id,
                        draft.attempt,
                        target,
                        draft.input.kind == store::CollabDraftKind::Create,
                        state.clock().now_unix_ms(),
                    )
                    .await?;
                }
            }
        }
        let result = if let Some((run, _, _)) =
            store::get_collab_run(state.storage(), &draft.input.id).await?
        {
            store::CollabDeliveryResult::Accepted { run_id: run }
        } else if draft.delivery_status == store::CollabDeliveryStatus::Started {
            store::CollabDeliveryResult::Unknown {
                reason: "process ended before durable delivery acceptance".into(),
            }
        } else {
            continue;
        };
        store::settle_collab_delivery(
            state.storage(),
            &draft.input.id,
            draft.attempt,
            result,
            state.clock().now_unix_ms(),
        )
        .await?;
        publish_decision(state, &draft.input.id).await;
    }
    Ok(())
}
pub async fn shutdown(state: &ServiceState) -> Result<(), StorageError> {
    recover(state).await
}

fn mode(
    value: Option<&str>,
) -> Result<SessionPermissionMode, crate::sessions::SessionExecuteError> {
    match value {
        None | Some("ask") => Ok(SessionPermissionMode::Ask),
        Some("operate") => Ok(SessionPermissionMode::Operate),
        Some("read_only") => Ok(SessionPermissionMode::ReadOnly),
        _ => Err(crate::sessions::SessionExecuteError::Forbidden),
    }
}
pub(crate) fn collaboration_input(source_agent: &str, body: &str) -> String {
    format!("[来自助手 {source_agent} 的会话协作消息]\n{body}")
}
impl crate::sessions::SessionStore {
    /// 只由已经确认的持久草稿驱动，不接受HTTP或模型自报来源。
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn execute_collaboration_for(
        &self,
        storage: &Arc<RunDatabase>,
        events: &Arc<crate::events::EventService>,
        supervisor: &Arc<crate::runs::RunSupervisor>,
        background: &Arc<crate::background::BackgroundDriveRegistry>,
        principal: &Principal,
        target_session_id: &str,
        input: &str,
        source_session_id: &str,
        source_run_id: &str,
        suggestion_id: &str,
        source_tool_call_id: &str,
        now_ms: u64,
    ) -> Result<crate::sessions::ExecuteAccepted, crate::sessions::SessionExecuteError> {
        use crate::sessions::SessionExecuteError as E;
        let d = store::get_collab_draft(storage, suggestion_id)
            .await
            .map_err(E::Storage)?
            .ok_or(E::Forbidden)?;
        let subject = principal.user_id.as_deref().unwrap_or(LOCAL_OWNER_SUBJECT);
        if (!principal.is_local_owner() && principal.kind != crate::auth::PrincipalKind::Device)
            || subject != d.input.owner_subject
            || d.input.studio_id != "local"
            || d.state != store::CollabDraftState::Applying
            || d.delivery_status != store::CollabDeliveryStatus::Started
            || d.target_session_id.as_deref() != Some(target_session_id)
            || d.input.source_session_id != source_session_id
            || d.input.source_run_id != source_run_id
            || d.input.source_tool_call_id != source_tool_call_id
        {
            return Err(E::Forbidden);
        }
        let payload = draft_payload(&d).map_err(E::Storage)?;
        let expected = match d.input.kind {
            store::CollabDraftKind::Send => payload["message"].as_str(),
            store::CollabDraftKind::Create => payload["firstMessage"].as_str(),
        };
        if expected != Some(input) {
            return Err(E::Forbidden);
        }
        let source = session_tree::get_session_row(storage, source_session_id)
            .await
            .map_err(E::Storage)?
            .ok_or(E::Forbidden)?;
        let target = session_tree::get_session_row(storage, target_session_id)
            .await
            .map_err(E::Storage)?
            .ok_or(E::Forbidden)?;
        if source.owner_user_id != d.input.owner_subject
            || target.owner_user_id != d.input.owner_subject
            || source.lifecycle != "active"
            || target.lifecycle != "active"
            || source_session_id == target_session_id
        {
            return Err(E::Forbidden);
        }
        let source_mode = mode(source.permission_mode.as_deref())?;
        if source_mode == SessionPermissionMode::ReadOnly {
            return Err(E::Forbidden);
        }
        let target_mode = mode(target.permission_mode.as_deref())?;
        let constrained = match (source_mode, target_mode) {
            (SessionPermissionMode::ReadOnly, _) | (_, SessionPermissionMode::ReadOnly) => {
                SessionPermissionMode::ReadOnly
            }
            (SessionPermissionMode::Ask, _) | (_, SessionPermissionMode::Ask) => {
                SessionPermissionMode::Ask
            }
            _ => SessionPermissionMode::Operate,
        };
        if let Some((run, _, _)) = store::get_collab_run(storage, suggestion_id)
            .await
            .map_err(E::Storage)?
        {
            return Ok(crate::sessions::ExecuteAccepted {
                run_id: run,
                run_count: self
                    .run_count(target_session_id)
                    .await
                    .map_err(E::Storage)?,
                replayed: true,
            });
        }
        let chat = if let Some(model) = payload["model"].as_str().filter(|s| !s.is_empty()) {
            let (provider, model) = model
                .split_once('/')
                .filter(|(p, m)| !p.is_empty() && !m.is_empty())
                .ok_or(E::Forbidden)?;
            Some(crate::runs::ChatRunInput {
                images: vec![],
                files: vec![],
                client_message_id: None,
                snapshot_version: None,
                display_text: Some(input.into()),
                quoted_text: None,
                session_refs: vec![],
                options: lingxi_kernel::model_exchange::ModelTurnOptions {
                    model: Some((provider.into(), model.into())),
                    thinking_level: None,
                },
            })
        } else {
            None
        };
        let prepared = collaboration_input(&source.agent_id, input);
        self.execute_background_authorized_for(
            storage,
            events,
            supervisor,
            background,
            principal,
            target_session_id,
            &crate::sessions::ExecuteSubmission {
                input: &prepared,
                request_id: Some(&d.request_id),
                knowledge: None,
            },
            now_ms,
            chat,
            Some(lingxi_kernel::subagent::RunLineage {
                parent_run_id: Some(lingxi_protocol::RunId::new(source_run_id)),
                origin: lingxi_kernel::subagent::RunOrigin::SessionCollab,
                source_message_id: None,
                cause_id: Some(suggestion_id.into()),
            }),
            None,
            Some(constrained),
            None,
        )
        .await
    }
}
