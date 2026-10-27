//! 本会话余期的精确 MCP 能力授权，与单次动作批准分开保存。
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};

use axum::{
    Extension, Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use lingxi_adapters::storage::{
    RunDatabase,
    session_tree::{SessionTreeRow, get_session_row},
};
use lingxi_protocol::{McpSessionPermissionRequest, McpSessionPermissionResponse};
use serde_json::{Value, json};

use crate::{
    ServiceState,
    auth::{AuthService, CredentialKind, Principal},
};

const CAPACITY: usize = 4096;
/// 默认不装配；测试仅观察真实会话读取完成，不替换身份、存储或结果。
pub type GrantReadObserver = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Clone, PartialEq)]
pub(crate) struct CapabilityBinding {
    pub principal: Principal,
    pub session_id: String,
    pub agent_id: String,
    pub target_id: String,
    pub target_generation: u64,
    pub capability: String,
    pub review_scope: Value,
}

#[derive(Clone)]
struct Grant {
    id: u64,
    binding: CapabilityBinding,
    auth: Weak<AuthService>,
    management: Weak<crate::management::ManagementState>,
    storage: Weak<RunDatabase>,
    created_at_unix_ms: i64,
    secure_transport: bool,
}
impl Grant {
    fn live(&self, now: u64) -> bool {
        let Some(auth) = self.auth.upgrade() else {
            return false;
        };
        let principal = &self.binding.principal;
        if !principal.has_scope("sessions.write") || auth.validate_principal(principal).is_err() {
            return false;
        }
        if principal.credential_kind == CredentialKind::WebSession {
            return self.management.upgrade().is_some_and(|management| {
                matches!(
                    management.web_session_current(principal, now, self.secure_transport),
                    Ok(true)
                )
            });
        }
        true
    }
}
#[derive(Default)]
struct StateInner {
    next_id: u64,
    closed: bool,
    grants: Vec<Grant>,
    epochs: HashMap<String, SessionEpoch>,
}
#[derive(Default)]
struct SessionEpoch {
    generation: u64,
    in_flight: usize,
}

/// 在首次异步读取前冻结会话代次；只保留在途请求与现存授权所需的条目。
pub(crate) struct GrantEpoch {
    state: Arc<Mutex<StateInner>>,
    session: String,
    generation: u64,
}
impl Drop for GrantEpoch {
    fn drop(&mut self) {
        let mut inner = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(epoch) = inner.epochs.get_mut(&self.session) {
            epoch.in_flight = epoch.in_flight.saturating_sub(1);
        }
        prune_epochs(&mut inner);
    }
}
fn prune_epochs(inner: &mut StateInner) {
    let grants = &inner.grants;
    inner.epochs.retain(|session, epoch| {
        epoch.in_flight > 0
            || grants
                .iter()
                .any(|grant| &grant.binding.session_id == session)
    });
}

pub(crate) enum GrantError {
    Conflict,
    Denied(String),
}
impl From<String> for GrantError {
    fn from(message: String) -> Self {
        Self::Denied(message)
    }
}
impl From<&str> for GrantError {
    fn from(message: &str) -> Self {
        Self::Denied(message.into())
    }
}
#[derive(Default)]
pub(crate) struct SessionCapabilities {
    state: Arc<Mutex<StateInner>>,
    read_observer: std::sync::RwLock<Option<GrantReadObserver>>,
}
impl SessionCapabilities {
    pub(crate) fn begin(&self, session: &str, now: u64) -> Result<GrantEpoch, GrantError> {
        let mut inner = self.state.lock().unwrap_or_else(|p| p.into_inner());
        inner.grants.retain(|grant| grant.live(now));
        prune_epochs(&mut inner);
        if inner.closed {
            return Err(GrantError::Conflict);
        }
        if inner
            .epochs
            .values()
            .map(|epoch| epoch.in_flight)
            .sum::<usize>()
            >= CAPACITY
            || (!inner.epochs.contains_key(session) && inner.epochs.len() >= CAPACITY)
        {
            return Err("本机会话授权容量已满".into());
        }
        let epoch = inner.epochs.entry(session.into()).or_default();
        epoch.in_flight += 1;
        Ok(GrantEpoch {
            state: self.state.clone(),
            session: session.into(),
            generation: epoch.generation,
        })
    }

    pub(crate) fn set_read_observer(&self, observer: Option<GrantReadObserver>) {
        *self
            .read_observer
            .write()
            .unwrap_or_else(|p| p.into_inner()) = observer;
    }
    pub(crate) fn observe_read(&self, session: &str) {
        let observer = self
            .read_observer
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some(observer) = observer {
            observer(session);
        }
    }
    pub(crate) fn grant(
        &self,
        binding: CapabilityBinding,
        epoch: &GrantEpoch,
        state: &ServiceState,
        row: &SessionTreeRow,
        secure_transport: bool,
    ) -> Result<(), GrantError> {
        let mut inner = self.state.lock().unwrap_or_else(|p| p.into_inner());
        // 撤销、归档再恢复都使旧在途请求失效；核对与插入必须处于同一把锁内。
        if inner.closed
            || !Arc::ptr_eq(&self.state, &epoch.state)
            || epoch.session != binding.session_id
            || !inner
                .epochs
                .get(&epoch.session)
                .is_some_and(|current| current.generation == epoch.generation)
        {
            return Err(GrantError::Conflict);
        }
        inner
            .grants
            .retain(|grant| grant.live(state.clock().now_unix_ms()));
        if inner.grants.iter().any(|grant| {
            grant.binding == binding && grant.created_at_unix_ms == row.created_at_unix_ms
        }) {
            return Ok(());
        }
        // 同一项重复授权幂等；新代次替换旧项，不累积已失效的权利。
        inner.grants.retain(|grant| {
            !(grant.binding.principal == binding.principal
                && grant.binding.session_id == binding.session_id
                && grant.binding.target_id == binding.target_id)
        });
        if inner.grants.len() >= CAPACITY {
            return Err("本机会话授权容量已满".into());
        }
        inner.next_id = inner.next_id.checked_add(1).ok_or("会话授权编号已耗尽")?;
        let id = inner.next_id;
        let grant = Grant {
            id,
            binding,
            auth: Arc::downgrade(&state.auth),
            management: Arc::downgrade(&state.management),
            storage: Arc::downgrade(state.storage()),
            created_at_unix_ms: row.created_at_unix_ms,
            secure_transport,
        };
        if !grant.live(state.clock().now_unix_ms()) {
            return Err("会话授权的原始身份已失效".into());
        }
        inner.grants.push(grant);
        Ok(())
    }
    pub(crate) fn find(&self, binding: &CapabilityBinding, now: u64) -> Option<u64> {
        let mut inner = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if inner.closed {
            return None;
        }
        inner.grants.retain(|grant| grant.live(now));
        prune_epochs(&mut inner);
        inner
            .grants
            .iter()
            .find(|grant| &grant.binding == binding)
            .map(|grant| grant.id)
    }
    pub(crate) async fn validate(
        &self,
        id: u64,
        binding: &CapabilityBinding,
        clock: &dyn crate::inject::ServiceClock,
    ) -> bool {
        let grant = {
            let inner = self.state.lock().unwrap_or_else(|p| p.into_inner());
            if inner.closed {
                return false;
            }
            inner
                .grants
                .iter()
                .find(|grant| grant.id == id && &grant.binding == binding)
                .cloned()
        };
        let Some(grant) = grant else { return false };
        if !grant.live(clock.now_unix_ms()) {
            return false;
        }
        let Some(storage) = grant.storage.upgrade() else {
            return false;
        };
        let Ok(Some(row)) = get_session_row(&storage, &binding.session_id).await else {
            return false;
        };
        if row.lifecycle != "active"
            || row.created_at_unix_ms != grant.created_at_unix_ms
            || row.agent_id != binding.agent_id
            || !crate::sessions::SessionStore::can_access_session(
                &binding.principal,
                &row.owner_user_id,
                &row.session_id,
            )
            || !grant.live(clock.now_unix_ms())
        {
            return false;
        }
        let inner = self.state.lock().unwrap_or_else(|p| p.into_inner());
        !inner.closed
            && inner
                .grants
                .iter()
                .any(|current| current.id == id && current.binding == *binding)
    }
    pub(crate) fn revoke_session(&self, session: &str) {
        let mut inner = self.state.lock().unwrap_or_else(|p| p.into_inner());
        inner
            .grants
            .retain(|grant| grant.binding.session_id != session);
        if let Some(epoch) = inner.epochs.get_mut(session) {
            if let Some(next) = epoch.generation.checked_add(1) {
                epoch.generation = next;
            } else {
                inner.closed = true;
                inner.grants.clear();
            }
        }
        prune_epochs(&mut inner);
    }
    pub(crate) fn close(&self) {
        let mut inner = self.state.lock().unwrap_or_else(|p| p.into_inner());
        inner.closed = true;
        inner.grants.clear();
        inner.epochs.clear();
    }
}

impl ServiceState {
    /// 只读观测真实审批是否已经登记，供本机整合检查分离就绪时间与设备认证耗时。
    /// 不回答审批、不创建记录，仍使用原始活身份与审批服务自身的会话归属筛选。
    pub fn pending_approvals_for_authenticated(
        &self,
        principal: &Principal,
        session: &str,
        secure_transport: bool,
    ) -> Result<Vec<crate::approval_service::PendingView>, String> {
        if !principal.has_scope("chat") || self.auth().validate_principal(principal).is_err() {
            return Err("审批观测身份或范围已失效".into());
        }
        if principal.credential_kind == CredentialKind::WebSession
            && !matches!(
                self.management.web_session_current(
                    principal,
                    self.clock().now_unix_ms(),
                    secure_transport
                ),
                Ok(true)
            )
        {
            return Err("网页审批观测身份已失效".into());
        }
        let service = self.approvals.as_ref().ok_or("审批服务未配置")?;
        Ok(
            service
                .pending_for_principal(&crate::sessions::kernel_principal_of(principal), session),
        )
    }
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error":message}))).into_response()
}
async fn grant_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    secure: Option<Extension<crate::serve::SecureTransport>>,
    body: Result<Json<McpSessionPermissionRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Ok(Json(request)) = body else {
        return error(StatusCode::BAD_REQUEST, "会话授权请求格式无效");
    };
    if !principal.has_scope("sessions.write") {
        return error(StatusCode::FORBIDDEN, "需要 sessions.write 权限");
    }
    if [&request.session_id, &request.target_id, &request.capability]
        .iter()
        .any(|value| {
            value.is_empty()
                || value.len() > 512
                || value.trim() != value.as_str()
                || value.chars().any(char::is_control)
        })
    {
        return error(
            StatusCode::BAD_REQUEST,
            "会话、目标和能力必须为有效的非空标识",
        );
    }
    let Some(gateway) = state.tool_gateway() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "工具网关尚未初始化");
    };
    let epoch = match gateway.begin_mcp_session_grant(&request.session_id) {
        Ok(epoch) => epoch,
        Err(GrantError::Conflict) => return error(StatusCode::CONFLICT, "会话已改变，请重新确认"),
        Err(GrantError::Denied(message)) => return error(StatusCode::FORBIDDEN, &message),
    };
    match state
        .sessions()
        .get_for(&principal, &request.session_id)
        .await
    {
        Ok(crate::sessions::SessionAccess::Ok(_)) => {}
        Ok(crate::sessions::SessionAccess::NotFound) => {
            return error(StatusCode::NOT_FOUND, "会话不存在");
        }
        Ok(crate::sessions::SessionAccess::Forbidden) => {
            return error(StatusCode::FORBIDDEN, "此会话不属于当前身份");
        }
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "会话读取失败，未授予能力",
            );
        }
    }
    let row = match get_session_row(state.storage(), &request.session_id).await {
        Ok(Some(row)) if row.lifecycle == "active" => row,
        Ok(_) => return error(StatusCode::CONFLICT, "会话已关闭，未授予能力"),
        Err(_) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "会话读取失败，未授予能力",
            );
        }
    };
    gateway.observe_mcp_session_grant_read(&request.session_id);
    match gateway.grant_mcp_session_permission(
        &state,
        &principal,
        &row,
        &epoch,
        &request,
        secure.is_some_and(|Extension(value)| value.0),
    ) {
        Ok(()) => Json(McpSessionPermissionResponse {
            ok: true,
            session_id: request.session_id,
            target_id: request.target_id,
            capability: request.capability,
            persisted: false,
        })
        .into_response(),
        Err(GrantError::Conflict) => error(StatusCode::CONFLICT, "会话已改变，请重新确认"),
        Err(GrantError::Denied(message)) => error(StatusCode::FORBIDDEN, &message),
    }
}
pub fn router() -> Router<ServiceState> {
    Router::new().route("/lingxi/v1/mcp/session-permissions", post(grant_route))
}
