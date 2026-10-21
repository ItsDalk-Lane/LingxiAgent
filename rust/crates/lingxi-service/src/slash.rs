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
    actual: HashMap<String, ActualBridgeAttachment>,
    pending: HashMap<String, ActualBridgeSelection>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct ActualBridgeIdentity {
    connector_id: String,
    generation: u64,
    sender_id: String,
    actor_id: String,
    actor_subject: String,
    owner_inbox_id: String,
}
#[derive(Debug, Clone)]
struct ActualBridgeAttachment {
    identity: ActualBridgeIdentity,
    target: String,
}
#[derive(Debug, Clone)]
struct ActualBridgeSelection {
    identity: ActualBridgeIdentity,
    options: Vec<(String, String)>,
    expires: u64,
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
/// 每日整理只消费真实登记且归本机所有者的 Bridge，会话身份随后仍须查库复核。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DailyBridgeTarget {
    pub session_id: String,
    pub agent_id: String,
    pub owner_user_id: String,
    pub channel_key: String,
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
    pub(crate) text: String,
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
    pub(crate) fn daily_owner_targets(&self) -> Result<Vec<DailyBridgeTarget>, String> {
        let rows = self.read_index()?;
        let attached = self
            .attachments
            .lock()
            .map_err(|_| "bridge attachment lock poisoned")?;
        Ok(rows
            .into_iter()
            .filter(|(id, row)| {
                (row.owner_user_id == crate::auth::LOCAL_OWNER_USER_ID
                    || lingxi_protocol::valid_bridge_subject(&row.owner_user_id))
                    && !attached.targets.contains_key(id)
                    && !attached.actual.contains_key(id)
            })
            .map(|(session_id, row)| DailyBridgeTarget {
                session_id,
                agent_id: row.agent_id,
                owner_user_id: row.owner_user_id,
                channel_key: row.channel_key,
            })
            .collect())
    }
    /// 排队期间可能删除、换归属或进入接管；执行前及执行期间重新核实名册。
    pub(crate) fn daily_target_is_current(
        &self,
        target: &DailyBridgeTarget,
    ) -> Result<bool, String> {
        let rows = self.read_index()?;
        let attached = self
            .attachments
            .lock()
            .map_err(|_| "bridge attachment lock poisoned")?;
        Ok(rows.get(&target.session_id).is_some_and(|row| {
            (row.owner_user_id == crate::auth::LOCAL_OWNER_USER_ID
                || lingxi_protocol::valid_bridge_subject(&row.owner_user_id))
                && row.owner_user_id == target.owner_user_id
                && row.agent_id == target.agent_id
                && row.channel_key == target.channel_key
                && !attached.targets.contains_key(&target.session_id)
                && !attached.actual.contains_key(&target.session_id)
        }))
    }
    /// 仅真实宿主在数据库核准 owner 私聊后调用，不开放新的 HTTP 权限。
    pub(crate) fn register_actual_bridge(
        &self,
        id: &str,
        facts: &crate::sessions::SessionFacts,
        channel_key: &str,
    ) -> Result<(), String> {
        if !lingxi_protocol::valid_bridge_subject(&facts.owner_user_id)
            || facts.session_id != id
            || channel_key.trim().is_empty()
            || channel_key.len() > 256
        {
            return Err("invalid actual Bridge registration".into());
        }
        self.register_inner(id, facts, channel_key, true)
    }
    fn register(
        &self,
        id: &str,
        facts: &crate::sessions::SessionFacts,
        channel_key: &str,
    ) -> Result<(), String> {
        self.register_inner(id, facts, channel_key, false)
    }
    fn register_inner(
        &self,
        id: &str,
        facts: &crate::sessions::SessionFacts,
        channel_key: &str,
        idempotent: bool,
    ) -> Result<(), String> {
        let _guard = self
            .mutation
            .lock()
            .map_err(|_| "bridge index lock poisoned")?;
        let mut rows = self.read_index()?;
        if let Some(row) = rows.get(id) {
            return if idempotent
                && row.agent_id == facts.agent_id
                && row.owner_user_id == facts.owner_user_id
                && row.channel_key == channel_key
            {
                Ok(())
            } else {
                Err("bridge session already registered".into())
            };
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
    /// 宿主先凭此只读分类领取命令收据，再调用有副作用的派发。
    /// 收据名 = 实际首词元：/abort、/halt 是 /stop 同族别名，
    /// claim_command 要求收据名与冻结消息逐字对应。
    pub(crate) fn bridge_command_kind(
        &self,
        source: &str,
        text: &str,
        now: u64,
    ) -> Result<Option<String>, String> {
        let command = text
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(command.as_str(), "/loop" | "/stop" | "/abort" | "/halt") {
            return Ok(Some(command.trim_start_matches('/').to_string()));
        }
        if command == "/rc" {
            return Ok(Some("rc".into()));
        }
        if command == "/exitrc" {
            return Ok(Some("exitrc".into()));
        }
        if command.starts_with('/') {
            return Ok(None);
        }
        let attached = self
            .attachments
            .lock()
            .map_err(|_| "attachment lock poisoned")?;
        Ok(attached
            .pending
            .get(source)
            .filter(|selection| now < selection.expires)
            .map(|_| "rc".into()))
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
        "/apply" => {
            if !principal.is_local_owner() {
                send(
                    &tx,
                    id,
                    target,
                    "error",
                    "应用定时任务建议需要本机所有者权限",
                )
                .await;
                return;
            }
            let reference = args.trim();
            if reference.is_empty() || reference.split_whitespace().count() != 1 {
                send(&tx, id, target, "error", "用法：/apply 建议编号").await;
                return;
            }
            let result = crate::automation::manage(
                &crate::automation::JobApi::from_state(state),
                principal,
                &json!({"action":"apply_suggestion","suggestionId":reference,"sessionId":target}),
            )
            .await;
            match result {
                Ok(applied) => {
                    let title = applied["job"]["label"]
                        .as_str()
                        .filter(|title| !title.trim().is_empty())
                        .or_else(|| applied["job"]["id"].as_str())
                        .unwrap_or("定时任务");
                    send(
                        &tx,
                        id,
                        target,
                        "success",
                        format!("定时任务「{title}」已应用"),
                    )
                    .await;
                }
                Err(error) => {
                    send(&tx, id, target, "error", format!("应用建议失败：{error}")).await
                }
            }
        }

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
        "/stop" | "/abort" | "/halt" => {
            for (level, text) in stop_run_texts(state, principal, target).await {
                send(&tx, id, target, level, text).await;
            }
        }
        // 现役 core/slash-commands/bridge-commands.ts 的 /loop
        // （permission:"owner"；bridge-commands.ts:249-284 逐字文本）。
        "/loop" => {
            if !principal.is_local_owner() {
                send(&tx, id, target, "error", "管理循环任务需要本机所有者权限").await;
                return;
            }
            // 桥会话未挂接桌面时循环跑在桥会话本体（回执经循环投递台账
            // 走 outbox 链）；挂接态 target 已解析为桌面会话。
            let loop_target = if bridge && attachment.is_none() {
                crate::loopx::LoopTarget::bridge(target)
            } else {
                crate::loopx::LoopTarget::desktop(target)
            };
            let (level, text) = loop_command(state, &loop_target, args).await;
            send(&tx, id, target, level, text).await;
        }

        _ => send(&tx, id, target, "error", format!("未知命令：{raw}")).await,
    }
}
/// /loop 共享实现（HTTP 入口与桥命令入口同一文本，现役
/// bridge-commands.ts:251-284 逐字同位；storage_error 段为 Rust 侧
/// 持久化失败显式呈现，对全部入口一致）。
async fn loop_command(
    state: &ServiceState,
    loop_target: &crate::loopx::LoopTarget,
    args: &str,
) -> (&'static str, String) {
    let controller = &state.loop_services().controller;
    let target = loop_target.session_id.as_str();
    let args_trimmed = args.trim();
    let first_token = args_trimmed
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if args_trimmed.is_empty() || first_token == "status" {
        return match controller.status_for_target(loop_target) {
            Some(record) => (
                if controller.storage_error(target).is_some() {
                    "error"
                } else {
                    "success"
                },
                match controller.storage_error(target) {
                    Some(error) => format!(
                        "{}\n自动推进已停止，状态尚未保存；修复后用 /loop resume 重试。\n{error}",
                        crate::loopsvc::render_loop_status(&record)
                    ),
                    None => crate::loopsvc::render_loop_status(&record),
                },
            ),
            None => {
                if args_trimmed.is_empty() {
                    (
                        "error",
                        "用法：/loop <任务> | /loop stop | pause | resume | status".into(),
                    )
                } else {
                    ("success", "（该会话没有循环）".into())
                }
            }
        };
    }
    if first_token == "stop" {
        return match controller.stop(loop_target).await {
            Ok(_) => ("success", "（循环已终止）".into()),
            Err(err) => ("error", err.message),
        };
    }
    if first_token == "pause" {
        return match controller.pause(loop_target, "user", None).await {
            Ok(_) => ("success", "（循环已暂停，/loop resume 恢复）".into()),
            Err(err) => ("error", err.message),
        };
    }
    if first_token == "resume" {
        return match controller.resume(loop_target).await {
            Ok(_) => ("success", "（循环已恢复）".into()),
            Err(err) => ("error", err.message),
        };
    }
    // 现役 isStart：有参数且首词非子命令 → 以全部参数为任务启动。
    match controller.start(loop_target, args_trimmed).await {
        Ok(_) => (
            "success",
            "（循环已启动，/loop status 查看进度，/loop stop 终止）".into(),
        ),
        Err(err) => ("error", err.message),
    }
}
/// /stop 逐 run 结果文本（HTTP 入口逐条发送；桥入口合并为单条回执，
/// 文本逐字一致）。
async fn stop_run_texts(
    state: &ServiceState,
    principal: &Principal,
    target: &str,
) -> Vec<(&'static str, String)> {
    match state.storage().list_active_runs().await {
        Err(e) => vec![("error", format!("停止失败：{e}"))],
        Ok(rows) => {
            let current: Vec<_> = rows
                .into_iter()
                .filter(|r| r.session_id == target)
                .collect();
            if current.is_empty() {
                return vec![("success", "已停止（当前无活动回复）".into())];
            }
            let mut out = Vec::new();
            for row in current {
                out.push(
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
                            ("success", "正在停止当前回复".to_string())
                        }
                        Ok(crate::sessions::CancelRunOutcome::AlreadyTerminal { .. }) => {
                            ("success", "已停止（当前无活动回复）".to_string())
                        }
                        Ok(other) => ("error", format!("停止未完成：{other:?}")),
                        Err(e) => ("error", format!("停止失败：{e:?}")),
                    },
                );
            }
            out
        }
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

async fn current_actual_owner(
    state: &ServiceState,
    inbox: &lingxi_adapters::storage::bridge::BridgeInbox,
    source: &str,
) -> Result<ActualBridgeIdentity, String> {
    use lingxi_adapters::storage::bridge as store;
    let actual = store::get_inbox(state.storage(), &inbox.id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Bridge owner receipt missing")?;
    if actual.input != inbox.input
        || actual.role != lingxi_protocol::BridgeRole::Owner
        || actual.input.conversation_kind != lingxi_protocol::BridgeConversationKind::Dm
    {
        return Err("Bridge owner DM required".into());
    }
    let connector = store::get_connector(state.storage(), &actual.input.connector_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Bridge connector missing")?;
    if !connector.input.enabled
        || connector.generation != actual.connector_generation
        || connector.input.owner_sender_id.as_deref() != Some(actual.input.sender_id.as_str())
        || connector.input.actor_id != actual.actor_id
        || connector.input.platform != actual.input.platform
        || connector.input.owner_subject != crate::auth::LOCAL_OWNER_USER_ID
        || actual.owner_subject != connector.input.owner_subject
    {
        return Err("Bridge owner authorization revoked".into());
    }
    let actor_subject = lingxi_protocol::bridge_subject(
        &actual.input.platform,
        &actual.input.connector_id,
        &actual.input.sender_id,
    )
    .map_err(str::to_owned)?;
    let row = lingxi_adapters::storage::session_tree::get_session_row(state.storage(), source)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Bridge source session missing")?;
    if row.owner_user_id != actor_subject
        || row.agent_id != actual.actor_id
        || row.lifecycle != "active"
    {
        return Err("Bridge source session authority mismatch".into());
    }
    if state
        .memory()
        .is_agent_deleted_checked(&actual.actor_id)
        .map_err(|e| e.to_string())?
    {
        return Err("Bridge actor deleted".into());
    }
    let resolution = store::resolve_conversation(
        state.storage(),
        &actual.id,
        actual.connector_generation,
        state.clock().now_unix_ms(),
    )
    .await
    .map_err(|e| e.to_string())?;
    // 已准备的接管消息 session_id 是桌面目标，source 仍须来自持久会话映射。
    let conversations =
        store::list_conversations(state.storage(), &actual.input.connector_id, 1000)
            .await
            .map_err(|e| e.to_string())?;
    if !conversations.iter().any(|row| {
        row.conversation_id == actual.input.conversation_id
            && row.sender_id == actual.input.sender_id
            && row.actor_id == actual.actor_id
            && row.current_session_id.as_deref() == Some(source)
    }) || (!actual.prepared && resolution.session_id != source)
    {
        return Err("Bridge source conversation changed".into());
    }
    Ok(ActualBridgeIdentity {
        connector_id: actual.input.connector_id,
        generation: actual.connector_generation,
        sender_id: actual.input.sender_id,
        actor_id: actual.actor_id,
        actor_subject,
        owner_inbox_id: actual.id,
    })
}
fn same_actual_actor(a: &ActualBridgeIdentity, b: &ActualBridgeIdentity) -> bool {
    a.connector_id == b.connector_id
        && a.generation == b.generation
        && a.sender_id == b.sender_id
        && a.actor_id == b.actor_id
        && a.actor_subject == b.actor_subject
}
async fn actual_desktop_target(
    state: &ServiceState,
    id: &str,
    agent: &str,
) -> Result<bool, String> {
    let valid=state.storage().query_one_text("SELECT s.session_id FROM sessions s WHERE s.session_id=?1 AND s.owner_user_id='user_local' AND s.agent_id=?2 AND s.lifecycle='active' AND NOT EXISTS(SELECT 1 FROM bridge_conversations_v1 c WHERE c.current_session_id=s.session_id) AND NOT EXISTS(SELECT 1 FROM bridge_inbox_v1 i WHERE i.session_id=s.session_id AND i.prepared_takeover=0) AND NOT EXISTS(SELECT 1 FROM automation_internal_sessions_v1 i WHERE i.session_id=s.session_id AND NOT EXISTS(SELECT 1 FROM automation_activity_promotions_v1 p WHERE p.session_id=i.session_id))",vec![id.into(),agent.into()]).await.map_err(|e|e.to_string())?;
    Ok(valid.is_some() && !state.slash.read_index()?.contains_key(id))
}
fn publish_actual_detached(state: &ServiceState, source: &str, target: &str) {
    state.app_events().publish(
        "bridge_rc_detached",
        json!({"sessionKey":source,"sessionPath":format!("native:{target}")}),
    );
}
pub(crate) fn clear_bridge_takeover(state: &ServiceState, source: &str) -> Result<(), String> {
    let removed = {
        let mut attached = state
            .slash
            .attachments
            .lock()
            .map_err(|_| "attachment lock poisoned")?;
        attached.pending.remove(source);
        attached.actual.remove(source)
    };
    if let Some(old) = removed {
        publish_actual_detached(state, source, &old.target);
    }
    Ok(())
}
pub(crate) fn revoke_bridge_takeovers(
    state: &ServiceState,
    connector_id: &str,
) -> Result<(), String> {
    let sources = {
        let attached = state
            .slash
            .attachments
            .lock()
            .map_err(|_| "attachment lock poisoned")?;
        attached
            .actual
            .iter()
            .filter(|(_, row)| row.identity.connector_id == connector_id)
            .map(|(source, _)| source.clone())
            .chain(
                attached
                    .pending
                    .iter()
                    .filter(|(_, row)| row.identity.connector_id == connector_id)
                    .map(|(source, _)| source.clone()),
            )
            .collect::<Vec<_>>()
    };
    for source in sources {
        clear_bridge_takeover(state, &source)?;
    }
    Ok(())
}
/// 返回下一条真实消息的唯一目标，失效绑定立即撤横幅，不重新授权其它会话。
pub(crate) async fn resolve_takeover(
    state: &ServiceState,
    inbox: &lingxi_adapters::storage::bridge::BridgeInbox,
    source: &str,
) -> Result<Option<String>, String> {
    let binding = {
        state
            .slash
            .attachments
            .lock()
            .map_err(|_| "attachment lock poisoned")?
            .actual
            .get(source)
            .cloned()
    };
    let Some(binding) = binding else {
        return Ok(None);
    };
    let identity = match current_actual_owner(state, inbox, source).await {
        Ok(identity) => identity,
        Err(error) => {
            clear_bridge_takeover(state, source)?;
            return Err(error);
        }
    };
    if !same_actual_actor(&binding.identity, &identity)
        || !actual_desktop_target(state, &binding.target, &identity.actor_id).await?
    {
        clear_bridge_takeover(state, source)?;
        return Ok(None);
    }
    // 建立绑定时的真实 owner DM 仍须存在，不能用自报 sessionKey 转移授权。
    let origin = lingxi_adapters::storage::bridge::get_inbox(
        state.storage(),
        &binding.identity.owner_inbox_id,
    )
    .await
    .map_err(|e| e.to_string())?
    .ok_or("Bridge takeover origin missing")?;
    if origin.role != lingxi_protocol::BridgeRole::Owner
        || origin.connector_generation != identity.generation
        || origin.input.sender_id != identity.sender_id
    {
        clear_bridge_takeover(state, source)?;
        return Ok(None);
    }
    Ok(Some(binding.target))
}
/// 只供可信宿主消费已领取的命令收据；普通消息返回 None，随后走正式 Run。
pub(crate) async fn dispatch_bridge_message(
    state: &ServiceState,
    inbox: &lingxi_adapters::storage::bridge::BridgeInbox,
    source: &str,
    text: &str,
) -> Result<Option<SlashResult>, String> {
    let now = state.clock().now_unix_ms();
    let Some(kind) = state.slash.bridge_command_kind(source, text, now)? else {
        return Ok(None);
    };
    let identity = current_actual_owner(state, inbox, source).await?;
    let result = |level, text: String, target: &str| {
        Some(SlashResult {
            kind: "slash_result",
            session_id: source.into(),
            target_session_id: target.into(),
            level,
            text,
            history_cleared: false,
        })
    };
    let raw = text.trim();
    let (command, args) = raw.split_once(char::is_whitespace).unwrap_or((raw, ""));
    if kind == "exitrc" {
        clear_bridge_takeover(state, source)?;
        return Ok(result("success", "已退出接管桌面会话".into(), source));
    }
    if kind == "loop" {
        // 现役桥 /loop 不随接管改目标（_handleLoopCommand 直接用 bridge
        // sessionRef）：循环恒跑在桥会话本体上。
        let loop_target = crate::loopx::LoopTarget::bridge(source);
        let (level, text) = loop_command(state, &loop_target, args).await;
        return Ok(result(level, text, source));
    }
    if matches!(kind.as_str(), "stop" | "abort" | "halt") {
        let takeover = resolve_takeover(state, inbox, source).await?;
        let target = takeover.as_deref().unwrap_or(source);
        // 接管态目标是桌面会话，与现役 sessionOps.abort(desktopRef) 同位，
        // 以调度主体执行（owner 私聊 + 新鲜接管绑定即授权，/rc 列表臂同
        // 先例）；未接管时桥主体只能停自己桥会话的 run。
        let principal = if takeover.is_some() {
            state.auth().scheduler_principal()
        } else {
            crate::auth::principal_for_bridge_inbox(inbox)?
        };
        let texts = stop_run_texts(state, &principal, target).await;
        let level = if texts.iter().any(|(level, _)| *level == "error") {
            "error"
        } else {
            "success"
        };
        let text = texts
            .into_iter()
            .map(|(_, text)| text)
            .collect::<Vec<_>>()
            .join("\n");
        return Ok(result(level, text, target));
    }
    if command.eq_ignore_ascii_case("/rc") && args.trim().is_empty() {
        if resolve_takeover(state, inbox, source).await?.is_some() {
            return Ok(result(
                "error",
                "当前已处于接管态，请先 /exitrc 再切换".into(),
                source,
            ));
        }
        let mut sessions = state
            .sessions()
            .list_for(&state.auth().scheduler_principal())
            .await
            .map_err(|e| e.to_string())?;
        sessions.sort_by(|a, b| {
            b.last_runs
                .first()
                .map(|r| r.started_at_unix_ms)
                .cmp(&a.last_runs.first().map(|r| r.started_at_unix_ms))
                .then_with(|| a.session_id.cmp(&b.session_id))
        });
        let mut options = Vec::new();
        for session in sessions {
            if session.agent_id != identity.actor_id
                || session.owner_user_id != crate::auth::LOCAL_OWNER_USER_ID
                || !actual_desktop_target(state, &session.session_id, &identity.actor_id).await?
            {
                continue;
            }
            let occupied = {
                let attached = state
                    .slash
                    .attachments
                    .lock()
                    .map_err(|_| "attachment lock poisoned")?;
                attached
                    .targets
                    .values()
                    .any(|id| id == &session.session_id)
                    || attached
                        .actual
                        .values()
                        .any(|row| row.target == session.session_id)
            };
            if !occupied {
                options.push((
                    session.session_id,
                    if session.title.is_empty() {
                        "未命名会话".into()
                    } else {
                        session.title
                    },
                ));
                if options.len() == 10 {
                    break;
                }
            }
        }
        if options.is_empty() {
            return Ok(result(
                "success",
                "当前助手没有可接管的桌面会话".into(),
                source,
            ));
        }
        let lines = options
            .iter()
            .enumerate()
            .map(|(i, (_, title))| format!("{}. {title}", i + 1))
            .collect::<Vec<_>>()
            .join("\n");
        state
            .slash
            .attachments
            .lock()
            .map_err(|_| "attachment lock poisoned")?
            .pending
            .insert(
                source.into(),
                ActualBridgeSelection {
                    identity,
                    options,
                    expires: now.saturating_add(5 * 60 * 1000),
                },
            );
        return Ok(result("success",format!("选择要接管的桌面会话（回复编号）：\n{lines}\n\n5 分钟内不选则自动取消。/exitrc 退出接管。"),source));
    }
    let selection = {
        state
            .slash
            .attachments
            .lock()
            .map_err(|_| "attachment lock poisoned")?
            .pending
            .get(source)
            .cloned()
    };
    let Some(selection) =
        selection.filter(|row| now < row.expires && same_actual_actor(&row.identity, &identity))
    else {
        clear_bridge_takeover(state, source)?;
        return Ok(result(
            "error",
            "请重新发送 /rc 选择桌面会话".into(),
            source,
        ));
    };
    let value = if command.eq_ignore_ascii_case("/rc") {
        args.trim()
    } else {
        raw
    };
    let index = value
        .parse::<usize>()
        .ok()
        .filter(|index| *index >= 1 && *index <= selection.options.len());
    let Some(index) = index else {
        return Ok(result(
            "error",
            format!("请输入数字编号（1-{}）", selection.options.len()),
            source,
        ));
    };
    let (target, title) = selection.options[index - 1].clone();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let current = current_actual_owner(state, inbox, source).await?;
        if !same_actual_actor(&current, &identity)
            || !actual_desktop_target(state, &target, &identity.actor_id).await?
        {
            clear_bridge_takeover(state, source)?;
            return Ok(result(
                "error",
                "目标会话或桥接授权已改变，接管取消".into(),
                source,
            ));
        }
        if !state.sessions().session_supervisor().is_busy(&target) {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            clear_bridge_takeover(state, source)?;
            return Ok(result(
                "error",
                "目标会话持续在回复中（>30s），接管取消。稍后重试 /rc".into(),
                source,
            ));
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    {
        let mut attached = state
            .slash
            .attachments
            .lock()
            .map_err(|_| "attachment lock poisoned")?;
        if attached
            .actual
            .iter()
            .any(|(other, row)| other != source && row.target == target)
            || attached.targets.values().any(|id| id == &target)
        {
            return Ok(result(
                "error",
                "目标会话已被另一个桥接会话接管".into(),
                source,
            ));
        }
        attached.pending.remove(source);
        attached.actual.insert(
            source.into(),
            ActualBridgeAttachment {
                identity,
                target: target.clone(),
            },
        );
    }
    state.app_events().publish("bridge_rc_attached",json!({"sessionKey":source,"sessionPath":format!("native:{target}"),"title":title,"platform":inbox.input.platform}));
    Ok(result(
        "success",
        format!("已接管桌面会话《{title}》"),
        &target,
    ))
}
