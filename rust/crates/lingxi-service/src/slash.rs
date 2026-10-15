//! 桌面与 Bridge 共用命令派发；会话归属来自服务端资料，接管状态重启清空。
use crate::{EndpointError, Principal, ServiceState};
use axum::http::StatusCode;
use axum::{
    extract::{Path as RoutePath, State},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::Mutex,
};
use tokio::sync::mpsc;

const MAX_BRIDGES: usize = 1024;
#[derive(Default)]
struct Attachments {
    targets: HashMap<String, String>,
}
/// Bridge 标识持久化；接管关系只在本次宿主生命周期内有效。
pub struct SlashService {
    index: PathBuf,
    files: crate::file_safety::FileSafety,
    mutation: Mutex<()>,
    attachments: Mutex<Attachments>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BridgeRecord {
    agent_id: String,
    owner_user_id: String,
    channel_key: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SlashRequest {
    pub text: String,
    #[serde(default)]
    pub agent_id: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlashResult {
    #[serde(rename = "type")]
    kind: &'static str,
    session_id: String,
    target_session_id: String,
    level: &'static str,
    text: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    history_cleared: bool,
}
impl SlashService {
    pub fn new(home: &Path) -> Self {
        let resolved = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
        let home = resolved.as_path();
        Self {
            index: home.join("bridge-slash/sessions.json"),
            files: crate::file_safety::FileSafety::new(home),
            mutation: Mutex::new(()),
            attachments: Mutex::new(Attachments::default()),
        }
    }
    fn read_index(&self) -> Result<BTreeMap<String, BridgeRecord>, String> {
        match std::fs::symlink_metadata(&self.index) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(e.to_string()),
            Ok(_) => {}
        }
        let snapshot = crate::file_safety::Snapshot::read(&self.index)
            .map_err(|e| format!("bridge index: {e:?}"))?;
        let rows: BTreeMap<String, BridgeRecord> =
            serde_json::from_slice(&snapshot.bytes).map_err(|e| format!("bridge index: {e}"))?;
        if rows.len() > MAX_BRIDGES {
            return Err("bridge index exceeds capacity".into());
        }
        Ok(rows)
    }
    fn register(
        &self,
        id: &str,
        facts: &crate::sessions::SessionFacts,
        channel_key: &str,
    ) -> Result<(), String> {
        let _guard = self
            .mutation
            .lock()
            .map_err(|_| "bridge index lock poisoned")?;
        let mut rows = self.read_index()?;
        if rows.contains_key(id) {
            return Err("bridge session already registered".into());
        }
        if rows.len() >= MAX_BRIDGES {
            return Err("bridge registry full".into());
        }
        if rows
            .values()
            .any(|r| r.owner_user_id == facts.owner_user_id && r.channel_key == channel_key)
        {
            return Err("bridge channel already registered".into());
        }
        rows.insert(
            id.into(),
            BridgeRecord {
                agent_id: facts.agent_id.clone(),
                owner_user_id: facts.owner_user_id.clone(),
                channel_key: channel_key.into(),
            },
        );
        let directory = self.index.parent().unwrap();
        if directory.exists() && directory.canonicalize().ok().as_deref() != Some(directory) {
            return Err("bridge index directory changed".into());
        }
        crate::paths::ensure_private_dir(directory).map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec(&rows).map_err(|e| e.to_string())?;
        match std::fs::symlink_metadata(&self.index) {
            Ok(_) => {
                let snapshot = crate::file_safety::Snapshot::read(&self.index)
                    .map_err(|e| format!("{e:?}"))?;
                self.files
                    .replace("bridge-registry", &self.index, &snapshot, &bytes, || true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.files
                    .create("bridge-registry", &self.index, &bytes, || true)
            }
            Err(e) => return Err(e.to_string()),
        }
        .map_err(|e| format!("bridge registration failed: {e:?}"))
    }
    fn bridge(&self, facts: &crate::sessions::SessionFacts) -> Result<bool, String> {
        let rows = self.read_index()?;
        match rows.get(&facts.session_id) {
            Some(row)
                if row.agent_id == facts.agent_id && row.owner_user_id == facts.owner_user_id =>
            {
                Ok(true)
            }
            Some(_) => Err("bridge session identity mismatch".into()),
            None => Ok(false),
        }
    }
    fn attachment(&self, bridge: &str) -> Result<Option<String>, String> {
        Ok(self
            .attachments
            .lock()
            .map_err(|_| "bridge attachment lock poisoned")?
            .targets
            .get(bridge)
            .cloned())
    }
}
fn failure(message: impl Into<String>) -> Response {
    EndpointError::resource_failure(StatusCode::CONFLICT, "slash_refused", message.into())
        .into_response()
}
async fn owned(
    state: &ServiceState,
    principal: &Principal,
    id: &str,
) -> Result<crate::sessions::SessionFacts, Box<Response>> {
    let facts = crate::get_session_owned(state, principal, id)
        .await
        .map_err(Box::new)?;
    crate::gate_agent_not_deleted(state, &facts.agent_id).map_err(Box::new)?;
    if let Err(e) = state.memory().agent_dir_checked(&facts.agent_id) {
        return Err(Box::new(e.response()));
    }
    match lingxi_adapters::storage::session_tree::get_session_row(state.storage(), id).await {
        Ok(Some(row)) if row.lifecycle == "active" => Ok(facts),
        Ok(_) => Err(Box::new(failure("session not active"))),
        Err(e) => Err(Box::new(EndpointError::storage(&e).into_response())),
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeRegistration {
    channel_key: String,
}
/// 实际 Bridge 宿主把已建立、已授权会话登记为平台 channel；不信任命令自报来源。
pub async fn register_bridge_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    RoutePath(id): RoutePath<String>,
    body: Result<Json<BridgeRegistration>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(v) => v,
        Err(e) => return EndpointError::from_json_rejection(&e).into_response(),
    };
    if req.channel_key.trim().is_empty() || req.channel_key.len() > 256 {
        return EndpointError::invalid_message("channelKey must be 1..256 bytes").into_response();
    }
    let facts = match owned(&state, &principal, &id).await {
        Ok(v) => v,
        Err(e) => return *e,
    };
    match state.slash.register(&id, &facts, &req.channel_key) {
        Ok(()) => (
            StatusCode::CREATED,
            Json(json!({"sessionId":id,"kind":"bridge"})),
        )
            .into_response(),
        Err(e) => failure(e),
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttachRequest {
    desktop_session_id: String,
}
pub async fn attach_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    RoutePath(id): RoutePath<String>,
    body: Result<Json<AttachRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(v) => v,
        Err(e) => return EndpointError::from_json_rejection(&e).into_response(),
    };
    let bridge = match owned(&state, &principal, &id).await {
        Ok(v) => v,
        Err(e) => return *e,
    };
    let desktop = match owned(&state, &principal, &req.desktop_session_id).await {
        Ok(v) => v,
        Err(e) => return *e,
    };
    match (state.slash.bridge(&bridge), state.slash.bridge(&desktop)) {
        (Ok(true), Ok(false))
            if bridge.agent_id == desktop.agent_id
                && bridge.owner_user_id == desktop.owner_user_id => {}
        (Err(e), _) | (_, Err(e)) => return failure(e),
        _ => {
            return failure(
                "attachment requires an owned Bridge and same-owner same-agent desktop session",
            )
        }
    }
    let mut attachments = match state.slash.attachments.lock() {
        Ok(v) => v,
        Err(_) => return failure("attachment lock poisoned"),
    };
    if attachments
        .targets
        .iter()
        .any(|(b, t)| b != &id && t == &req.desktop_session_id)
    {
        return failure("desktop session already attached");
    }
    attachments
        .targets
        .insert(id.clone(), req.desktop_session_id.clone());
    Json(json!({"attached":true,"sessionId":id,"desktopSessionId":req.desktop_session_id}))
        .into_response()
}
pub async fn detach_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    RoutePath(id): RoutePath<String>,
) -> Response {
    if let Err(e) = owned(&state, &principal, &id).await {
        return *e;
    }
    match state.slash.attachments.lock() {
        Ok(mut a) => {
            let old = a.targets.remove(&id);
            Json(json!({"detached":old.is_some(),"desktopSessionId":old})).into_response()
        }
        Err(_) => failure("attachment lock poisoned"),
    }
}
async fn send(
    tx: &mpsc::Sender<SlashResult>,
    source: &str,
    target: &str,
    level: &'static str,
    text: impl Into<String>,
) {
    let _ = tx
        .send(SlashResult {
            kind: "slash_result",
            session_id: source.into(),
            target_session_id: target.into(),
            level,
            text: text.into(),
            history_cleared: false,
        })
        .await;
}
/// HTTP、WS 与 Bridge 宿主只有这一处执行入口；资料归属先于所有副作用。
pub async fn dispatch(
    state: &ServiceState,
    principal: &Principal,
    id: &str,
    req: SlashRequest,
    tx: mpsc::Sender<SlashResult>,
) {
    let source = match owned(state, principal, id).await {
        Ok(v) => v,
        Err(e) => {
            send_response_error(&tx, id, id, *e, "命令拒绝").await;
            return;
        }
    };
    if !principal.has_scope("chat") {
        send(&tx, id, id, "error", "命令拒绝：缺少 chat 权限").await;
        return;
    }
    if req
        .agent_id
        .as_deref()
        .is_some_and(|agent| agent != source.agent_id)
    {
        send(&tx, id, id, "error", "命令拒绝：Agent 身份不匹配").await;
        return;
    }
    if req.text.len() > 8192 {
        send(&tx, id, id, "error", "命令过长").await;
        return;
    }
    let bridge = match state.slash.bridge(&source) {
        Ok(v) => v,
        Err(e) => {
            send(&tx, id, id, "error", e).await;
            return;
        }
    };
    let attachment = if bridge {
        match state.slash.attachment(id) {
            Ok(v) => v,
            Err(e) => {
                send(&tx, id, id, "error", e).await;
                return;
            }
        }
    } else {
        None
    };
    let target = attachment.as_deref().unwrap_or(id);
    let target_facts = match owned(state, principal, target).await {
        Ok(v) if v.agent_id == source.agent_id && v.owner_user_id == source.owner_user_id => v,
        Ok(_) => {
            send(&tx, id, target, "error", "命令拒绝：接管会话归属已改变").await;
            return;
        }
        Err(e) => {
            send_response_error(&tx, id, target, *e, "命令拒绝").await;
            return;
        }
    };
    let raw = req.text.trim();
    let (command, args) = raw.split_once(char::is_whitespace).unwrap_or((raw, ""));
    let command = command.to_ascii_lowercase();
    match command.as_str() {
        "/compact" | "/fresh-compact" | "/freshcompact" | "/fresh_compact" => {
            let fresh = command != "/compact";
            if fresh && attachment.is_some() {
                send(
                    &tx,
                    id,
                    target,
                    "error",
                    "接管桌面会话期间暂不支持 /fresh-compact，请先退出接管",
                )
                .await;
                return;
            }
            if state.compaction().is_none() || state.context_compiler().is_none() {
                send(
                    &tx,
                    id,
                    target,
                    "error",
                    "命令系统未就绪：压缩或提示词服务不可用",
                )
                .await;
                return;
            }
            send(
                &tx,
                id,
                target,
                "success",
                if fresh {
                    "（正在 fresh-compact：刷新提示词与记忆状态后压缩上下文，请稍候...）"
                } else {
                    "（正在压缩上下文，请稍候...）"
                },
            )
            .await;
            let body = if args.trim().is_empty() {
                json!({})
            } else {
                json!({"customInstructions":args.trim()})
            };
            let response = crate::history::history_content::manual_compact(
                state,
                principal,
                target,
                &serde_json::to_vec(&body).unwrap(),
                fresh,
            )
            .await;
            if !response.status().is_success() {
                send_response_error(
                    &tx,
                    id,
                    target,
                    response,
                    if fresh {
                        "fresh-compact 失败"
                    } else {
                        "压缩失败"
                    },
                )
                .await;
                return;
            }
            match response_json(response).await {
                Ok(v) => {
                    let text = if let Some(reason) = v["noopReason"].as_str() {
                        format!(
                            "（{}完成：{}）",
                            if fresh { "fresh-compact " } else { "压缩" },
                            reason
                        )
                    } else {
                        format!(
                            "（{}已完成：{} → {} tokens{}）",
                            if fresh {
                                "fresh-compact "
                            } else {
                                "上下文压缩"
                            },
                            v["tokensBefore"],
                            v["tokensAfter"],
                            if v["degraded"] == true {
                                "，明确降级为截断"
                            } else {
                                ""
                            }
                        )
                    };
                    send(&tx, id, target, "success", text).await;
                }
                Err(e) => send(&tx, id, target, "error", e).await,
            }
        }
        "/reset" | "/clear" => {
            if attachment.is_some() {
                send(
                    &tx,
                    id,
                    target,
                    "error",
                    "接管桌面会话期间禁止使用 /reset，请先退出接管",
                )
                .await;
                return;
            }
            match lingxi_adapters::storage::session_admin::clear_current_history(
                state.storage(),
                id,
                &target_facts.owner_user_id,
                state.clock().now_unix_ms(),
            )
            .await
            {
                Ok((count, event)) => {
                    state.events().publish_committed(&[event]);
                    if let Some(c) = state.context_compiler() {
                        c.invalidate_session(id);
                    }
                    state.app_events().publish(
                        "session-history-cleared",
                        json!({"sessionId":id,"deletedMessages":count}),
                    );
                    let _ = tx
                        .send(SlashResult {
                            kind: "slash_result",
                            session_id: id.into(),
                            target_session_id: id.into(),
                            level: "success",
                            text: "已重置会话，历史已清除".into(),
                            history_cleared: true,
                        })
                        .await;
                }
                Err(e) => send(&tx, id, id, "error", format!("清除失败：{e}")).await,
            }
        }
        "/stop" | "/abort" | "/halt" => match state.storage().list_active_runs().await {
            Err(e) => send(&tx, id, target, "error", format!("停止失败：{e}")).await,
            Ok(rows) => {
                let current: Vec<_> = rows
                    .into_iter()
                    .filter(|r| r.session_id == target)
                    .collect();
                if current.is_empty() {
                    send(&tx, id, target, "success", "已停止（当前无活动回复）").await;
                    return;
                }
                for row in current {
                    match state
                        .sessions
                        .cancel_run_for(
                            state.storage().as_ref(),
                            state.runs(),
                            principal,
                            &row.run_id,
                        )
                        .await
                    {
                        Ok(crate::sessions::CancelRunOutcome::Accepted { .. }) => {
                            send(&tx, id, target, "success", "正在停止当前回复").await
                        }
                        Ok(crate::sessions::CancelRunOutcome::AlreadyTerminal { .. }) => {
                            send(&tx, id, target, "success", "已停止（当前无活动回复）").await
                        }
                        Ok(other) => {
                            send(&tx, id, target, "error", format!("停止未完成：{other:?}")).await
                        }
                        Err(e) => send(&tx, id, target, "error", format!("停止失败：{e:?}")).await,
                    }
                }
            }
        },
        _ => send(&tx, id, target, "error", format!("未知命令：{raw}")).await,
    }
}
async fn response_json(response: Response) -> Result<Value, String> {
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
async fn send_response_error(
    tx: &mpsc::Sender<SlashResult>,
    source: &str,
    target: &str,
    response: Response,
    prefix: &str,
) {
    let status = response.status();
    let detail = response_json(response)
        .await
        .unwrap_or_else(|e| json!({"error":e}));
    send(
        tx,
        source,
        target,
        "error",
        format!("{prefix}（{status}）：{detail}"),
    )
    .await;
}
pub async fn slash_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    RoutePath(id): RoutePath<String>,
    body: Result<Json<SlashRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(v) => v,
        Err(e) => return EndpointError::from_json_rejection(&e).into_response(),
    };
    // 路由保持404/403等鉴权结局；业务拒绝通过明确 slash_result 回执。
    if let Err(e) = owned(&state, &principal, &id).await {
        return *e;
    }
    let (tx, mut rx) = mpsc::channel(8);
    let (_, results) = tokio::join!(dispatch(&state, &principal, &id, req, tx), async move {
        let mut results = Vec::new();
        while let Some(result) = rx.recv().await {
            results.push(result);
        }
        results
    });
    Json(json!({"results":results})).into_response()
}
