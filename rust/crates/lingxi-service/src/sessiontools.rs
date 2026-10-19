//! 模型会话检查点和回退：沿正式目录、确认范围与同会话事务执行。
use crate::{
    auth::{ConnectionKindSerde, CredentialKind, Principal, PrincipalKind, TrustState},
    session_tree::{RewindRequest, SessionService, SessionTreeError},
    toolgateway::{ReviewInput, ToolInvocationGateway},
};
use lingxi_adapters::storage::{session_tree as store, RunDatabase};
use lingxi_kernel::{
    ports::{ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, ToolSuccess},
    toolcatalog::{
        Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
        ToolManifest, ToolOrigin, ToolRegistry,
    },
    RunContext,
};
use lingxi_protocol::{ErrorCode, ProtocolError, ToolCallId, ToolSchemaDocument};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

pub struct Dependencies {
    pub storage: Arc<RunDatabase>,
    pub tree: Arc<SessionService>,
    pub tasks: Arc<crate::task_supervisor::TaskSupervisor>,
    pub memory: Arc<crate::memory::MemoryService>,
    pub clock: Arc<dyn crate::inject::ServiceClock>,
    pub intake: Arc<crate::shutdown::SubmissionIntake>,
}
pub struct SessionTools {
    deps: Dependencies,
    process: String,
    pending: Mutex<()>,
    // 与调度锁分开，串行本进程的回执原子替换和授权快照读取。
    receipt_io: Mutex<()>,
    latest: Mutex<std::collections::HashMap<String, Value>>,
}
fn failed(message: impl Into<String>) -> ToolOutcome {
    ToolOutcome::Failed {
        error: ProtocolError::new(ErrorCode::InvalidMessage, message.into(), false),
    }
}
fn success(text: impl Into<String>, details: Value) -> ToolOutcome {
    let mut result = ToolSuccess::text(text.into());
    result.details = details.as_object().cloned();
    ToolOutcome::Success { result }
}
/// 这里只把已验证的内核运行身份转为会话事务所需形状，不参与凭证认证。
fn principal(ctx: &RunContext) -> Result<Principal, String> {
    let (kind, device) = match &ctx.principal {
        lingxi_kernel::Principal::LocalUser => (PrincipalKind::LocalUser, None),
        lingxi_kernel::Principal::Device { device_id, .. } => {
            (PrincipalKind::Device, Some(device_id.clone()))
        }
        _ => return Err("this principal cannot request an interactive session mutation".into()),
    };
    Ok(Principal {
        schema_version: 1,
        principal_id: format!("run:{}", ctx.principal.storage_subject()),
        kind,
        user_id: Some(ctx.principal.storage_subject()),
        studio_id: None,
        server_node_id: None,
        device_id: device,
        credential_id: None,
        web_session_id: None,
        connection_kind: ConnectionKindSerde::Local,
        credential_kind: CredentialKind::None,
        trust_state: TrustState::Unknown,
        scopes: vec!["chat".into()],
    })
}
fn name(args: &Value, key: &str, default: bool) -> Result<String, String> {
    match args.get(key) {
        None if default => Ok("latest".into()),
        Some(Value::String(value))
            if !value.trim().is_empty()
                && value.len() <= 200
                && !value.chars().any(char::is_control) =>
        {
            Ok(value.trim().into())
        }
        _ => Err(
            "checkpoint name must be nonempty, at most 200 bytes and contain no control characters"
                .into(),
        ),
    }
}
impl SessionTools {
    pub fn new(deps: Dependencies) -> Result<Self, String> {
        Ok(Self {
            deps,
            process: crate::auth::hex_random_public(16).map_err(|e| e.to_string())?,
            pending: Mutex::new(()),
            receipt_io: Mutex::new(()),
            latest: Mutex::new(std::collections::HashMap::new()),
        })
    }
    fn path(&self, sid: &str) -> PathBuf {
        self.deps
            .storage
            .db_path()
            .parent()
            .unwrap()
            .join("deferred-rewinds")
            .join(format!(
                "{}.json",
                lingxi_kernel::session_tree::sha256_hex(sid.as_bytes())
            ))
    }
    fn save(&self, sid: &str, value: &Value) -> Result<(), String> {
        let _guard = self.receipt_io.lock().unwrap_or_else(|p| p.into_inner());
        lingxi_adapters::storage::agent_memory::atomic_write(
            &self.path(sid),
            &serde_json::to_string(value).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }
    pub fn receipt(&self, sid: &str) -> Result<Option<Value>, String> {
        let _guard = self.receipt_io.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(value) = self
            .latest
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(sid)
            .cloned()
        {
            return Ok(Some(value));
        }
        let path = self.path(sid);
        match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.to_string()),
            Ok(_) => {}
        }
        let snapshot = crate::file_safety::Snapshot::read(&path).map_err(|e| e.to_string())?;
        let mut v: Value = serde_json::from_slice(&snapshot.bytes).map_err(|e| e.to_string())?;
        if v["sessionId"] != sid || !v["operationId"].is_string() {
            return Err("invalid deferred rewind receipt".into());
        }
        if v["status"] == "scheduled" && v["process"] != self.process {
            v["status"] = json!("unconfirmed_after_restart");
            v["rewind"] = json!("unknown");
            v["error"] = json!(
                "service restarted before the final deferred receipt was confirmed; inspect session operation/history before requesting a new rewind"
            );
        }
        Ok(Some(v))
    }
    async fn checkpoint(&self, ctx: &RunContext, args: &Value) -> ToolOutcome {
        let p = match principal(ctx) {
            Ok(p) => p,
            Err(e) => return failed(e),
        };
        let row = match store::get_session_row(&self.deps.storage, ctx.session_id.as_str()).await {
            Ok(Some(row)) => row,
            Ok(None) => return failed("no active session"),
            Err(e) => return failed(e.to_string()),
        };
        if !crate::sessions::SessionStore::can_access(&p, &row.owner_user_id) {
            return failed("checkpoint belongs to another principal");
        }
        let action = args["action"].as_str().unwrap_or("create");
        if action == "list" {
            return match self
                .deps
                .tree
                .list_checkpoints(ctx.session_id.as_str())
                .await
            {
                Ok(rows) => success(
                    if rows.is_empty() {
                        "no checkpoints yet".to_string()
                    } else {
                        format!(
                            "session checkpoints:\n{}",
                            rows.iter()
                                .map(|r| format!(
                                    "- {} (created {}, messages={})",
                                    r.name, r.created_at_unix_ms, r.message_count
                                ))
                                .collect::<Vec<_>>()
                                .join("\n")
                        )
                    },
                    json!({"count":rows.len(),"checkpoints":rows.into_iter().map(|r|json!({"name":r.name,"createdAt":r.created_at_unix_ms,"messageCount":r.message_count,"targetMessageId":r.target_message_id})).collect::<Vec<_>>()}),
                ),
                Err(e) => failed(format!("cannot list checkpoints: {e:?}")),
            };
        }
        let n = match name(args, "name", action == "create") {
            Ok(n) => n,
            Err(e) => return failed(e),
        };
        if action == "drop" {
            return match self
                .deps
                .tree
                .tool_drop_checkpoint(ctx, &self.deps.tasks, &n)
                .await
            {
                Ok(true) => success(
                    format!("checkpoint {n} dropped"),
                    json!({"dropped":true,"checkpoint":n}),
                ),
                Ok(false) => failed(format!(
                    "checkpoint {n} not found; no checkpoint was removed"
                )),
                Err(e) => failed(format!("checkpoint removal failed: {e:?}")),
            };
        }
        if action != "create" {
            return failed("unknown checkpoint action");
        }
        // 在读取文件之前先验证当前真实 Run 许可及用户锚点，拒绝不能留下快照。
        if row.permission_mode.as_deref() == Some("read_only") {
            return failed("checkpoint writes are blocked in read-only mode");
        }
        if !matches!(
            self.deps.tasks.parent_run_permit(
                ctx.run_id.as_str(),
                ctx.session_id.as_str(),
                &row.agent_id,
                &ctx.principal,
                self.deps.storage.session_admission()
            ),
            Ok(Some(_))
        ) {
            return failed("current Run does not own this session");
        }
        let messages =
            match store::list_branch_messages(&self.deps.storage, ctx.session_id.as_str()).await {
                Ok(rows) => rows,
                Err(e) => return failed(e.to_string()),
            };
        if !messages
            .iter()
            .any(|m| m.role == "user" && m.entry_type == "message")
        {
            return failed("no user turn on the active branch; no snapshot was written");
        }
        let snapshot = args["snapshot"] != false;
        let (files, degraded) = if snapshot {
            match collect_workspace_files(&row.authorized_folders) {
                Ok(files) => (files, None),
                Err(e) => (vec![], Some(e)),
            }
        } else {
            (vec![], None)
        };
        let saved = self
            .deps
            .tree
            .tool_create_checkpoint(
                &p,
                ctx,
                &self.deps.tasks,
                &n,
                &files,
                self.deps.clock.now_unix_ms(),
            )
            .await;
        match saved {
            Ok(record) => success(
                format!(
                    "checkpoint {} created{}",
                    record.name,
                    degraded
                        .as_ref()
                        .map(|e| format!("; workspace snapshot degraded: {e}"))
                        .unwrap_or_default()
                ),
                json!({"checkpoint":record.name,"createdAt":record.created_at_unix_ms,"snapshotCommit":if snapshot&&degraded.is_none(){Some(record.checkpoint_id)}else{None},"snapshotDegraded":degraded.is_some(),"snapshotError":degraded,"fileCount":files.len()}),
            ),
            Err(e) => failed(format!("checkpoint creation failed: {e:?}")),
        }
    }
    async fn rewind(
        self: &Arc<Self>,
        ctx: &RunContext,
        call: &ToolCallId,
        args: &Value,
    ) -> ToolOutcome {
        let p = match principal(ctx) {
            Ok(p) => p,
            Err(e) => return failed(e),
        };
        let checkpoint = match name(args, "checkpoint", true) {
            Ok(n) => n,
            Err(e) => return failed(e),
        };
        let approved = match crate::toolgateway::review_scope() {
            Some(value) => value,
            None => return failed("rewind requires a fresh bound user confirmation"),
        };
        let sid = ctx.session_id.to_string();
        let op = format!(
            "rewind:{}",
            lingxi_kernel::session_tree::sha256_hex(format!("{}:{}", ctx.run_id, call).as_bytes())
        );
        let restore = args["restoreFiles"] == true;
        let receipt = json!({"sessionId":sid,"operationId":op,"process":self.process,"checkpoint":checkpoint,"status":"scheduled","rewind":"scheduled","restoreFiles":restore,"sourceRunId":ctx.run_id.as_str()});
        {
            let _guard = self.pending.lock().unwrap_or_else(|p| p.into_inner());
            match self.receipt(&sid) {
                Ok(Some(v)) if v["status"] == "scheduled" => {
                    return failed("another confirmed rewind is already pending");
                }
                Err(e) => return failed(e),
                _ => {}
            }
            if let Err(e) = self.save(&sid, &receipt) {
                return failed(format!("cannot persist deferred rewind: {e}"));
            }
            self.latest
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&sid);
        }
        let this = Arc::clone(self);
        let task_receipt = receipt.clone();
        let task_sid = sid.clone();
        let task_op = op.clone();
        let spawn =
            self.deps
                .tasks
                .spawn_detached(format!("confirmed-rewind:{sid}:{op}"), async move {
                    let until = tokio::time::Instant::now() + Duration::from_secs(300);
                    let result: Result<Value, String> = loop {
                        if this.deps.intake.is_closed() {
                            break Err(
                                "service is shutting down; deferred rewind was not started".into()
                            );
                        }
                        if tokio::time::Instant::now() >= until {
                            break Err(
                            "session stayed busy; deferred rewind expired without changing history"
                                .into(),
                        );
                        }
                        let enabled = match this.deps.memory.rollback_file_changes() {
                            Ok(value) => value,
                            Err(e) => break Err(e.to_string()),
                        };
                        match this
                            .deps
                            .tree
                            .rewind_reviewed(
                                &task_sid,
                                RewindRequest {
                                    checkpoint_name: &checkpoint,
                                    restore_files: restore,
                                    file_rollback_enabled: enabled,
                                    principal: &p,
                                    request_id: Some(&task_op),
                                    now_unix_ms: this.deps.clock.now_unix_ms(),
                                },
                                &approved,
                            )
                            .await
                        {
                            Err(SessionTreeError::Busy) => {
                                tokio::time::sleep(Duration::from_millis(50)).await
                            }
                            Err(error) => break Err(format!("{error:?}")),
                            Ok(value) => {
                                break serde_json::to_value(value).map_err(|e| e.to_string());
                            }
                        }
                    };
                    let mut final_receipt = task_receipt;
                    match result {
                        Ok(value) => {
                            final_receipt["status"] = json!("completed");
                            final_receipt["rewind"] = json!(true);
                            final_receipt["receipt"] = value;
                        }
                        Err(error) => {
                            final_receipt["status"] = json!("failed");
                            final_receipt["rewind"] = json!(false);
                            final_receipt["error"] = json!(error);
                        }
                    }
                    let _guard = this.pending.lock().unwrap_or_else(|p| p.into_inner());
                    match this.save(&task_sid, &final_receipt) {
                        Ok(()) => Ok(()),
                        Err(error) => {
                            final_receipt["receiptPersisted"] = json!(false);
                            final_receipt["persistenceError"] = json!(error);
                            this.latest
                                .lock()
                                .unwrap_or_else(|p| p.into_inner())
                                .insert(task_sid, final_receipt);
                            Err(error)
                        }
                    }
                });
        match spawn {
            Ok(_handle) => success(
                "User confirmed. Rewind is scheduled for when this turn ends; read the deferred receipt for the actual outcome.",
                receipt,
            ),
            Err(e) => {
                let mut receipt = receipt;
                receipt["status"] = json!("failed");
                receipt["error"] = json!(format!("supervisor refused deferred rewind: {e:?}"));
                let _ = self.save(&sid, &receipt);
                failed(format!("cannot schedule rewind: {e:?}"))
            }
        }
    }
}

fn collect_workspace_files(roots: &[String]) -> Result<Vec<String>, String> {
    if roots.is_empty() {
        return Err("no authorized workspace is attached".into());
    }
    let mut files = std::collections::BTreeSet::new();
    let mut dirs: Vec<PathBuf> = roots.iter().map(PathBuf::from).collect();
    let mut total = 0u64;
    while let Some(dir) = dirs.pop() {
        if dir.canonicalize().ok().as_deref() != Some(dir.as_path()) {
            return Err("workspace contains a redirected directory".into());
        }
        for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let meta = entry.metadata().map_err(|e| e.to_string())?;
            if entry.file_type().map_err(|e| e.to_string())?.is_symlink() {
                return Err("workspace snapshot cannot safely capture a symbolic link".into());
            }
            if meta.is_dir() {
                if entry.file_name() != ".git" {
                    dirs.push(entry.path());
                }
            } else if meta.is_file() {
                total = total.saturating_add(meta.len());
                if total > 64 * 1024 * 1024 || files.len() >= 500 {
                    return Err("workspace snapshot exceeds the bounded capture budget".into());
                }
                files.insert(entry.path().to_string_lossy().to_string());
            } else {
                return Err("workspace contains a non-regular file".into());
            }
        }
    }
    Ok(files.into_iter().collect())
}

/// 同一次只读事务冻结检查点锚点和全部文件版本；批准前后及实际持门后都比较。
pub fn review(
    storage: &RunDatabase,
    input: &ReviewInput<'_>,
    args: &Value,
) -> Result<Value, String> {
    let n = name(args, "checkpoint", true)?;
    let frozen = store::checkpoint_review(
        storage,
        input.session_id,
        input.agent_id,
        input.principal_kind,
        input.principal_subject,
        &n,
    )
    .map_err(|e| e.to_string())?;
    let target = &frozen["target"];
    let files = frozen["fileVersions"]
        .as_array()
        .ok_or("invalid checkpoint file versions")?;
    let restore = args["restoreFiles"] == true;
    let file_text = files
        .iter()
        .map(|file| format!("\n- {}", file[0].as_str().unwrap_or("")))
        .collect::<String>();
    Ok(
        json!({"kind":"rewind","agentId":input.agent_id,"sessionId":input.session_id,"checkpoint":n,"target":target,"restoreFiles":restore,"fileVersions":files,"summary":format!("回退当前会话到检查点「{n}」，丢弃其后分支。{}{}",if restore{format!("申请恢复{}份存档；执行时保留外部改动。",files.len())}else{"不恢复工作区文件。".into()},if restore{file_text}else{String::new()})}),
    )
}
struct Executor(Arc<SessionTools>);
impl ToolExecutorPort for Executor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            let runtime = crate::toolgateway::knowledge_runtime(ctx, call);
            let outcome = match runtime {
                Err(e) => ToolOutcome::Failed { error: e },
                Ok(_) => match request.target.as_str() {
                    "tool:first-party:checkpoint" => {
                        self.0.checkpoint(ctx, request.arguments.as_value()).await
                    }
                    "tool:first-party:rewind" => {
                        self.0.rewind(ctx, call, request.arguments.as_value()).await
                    }
                    _ => failed("unknown session tool"),
                },
            };
            ToolExecutionResult::of_ctx(ctx, outcome)
        })
    }
}
pub fn register(
    registry: &ToolRegistry,
    gateway: &ToolInvocationGateway,
    tools: Arc<SessionTools>,
    budget: &SchemaBudget,
) {
    for (name, kind, description, schema) in [
        (
            "checkpoint",
            PermissionKind::ArgumentAwareCheckpoint,
            "Create/list/drop a named checkpoint in the current session. create anchors the latest real user turn, optionally captures authorized workspace files; list reads; drop removes only the named checkpoint.",
            json!({"type":"object","properties":{"action":{"type":"string","enum":["create","list","drop"],"default":"create"},"name":{"type":"string","minLength":1,"maxLength":200},"snapshot":{"type":"boolean","default":true}},"additionalProperties":false}),
        ),
        (
            "rewind",
            PermissionKind::Review,
            "Rewind to a named checkpoint only after fresh human confirmation. While this turn is busy it schedules a supervised deferred rewind and reports the actual result after the turn ends. External file changes are preserved.",
            json!({"type":"object","properties":{"checkpoint":{"type":"string","minLength":1,"maxLength":200},"restoreFiles":{"type":"boolean","default":false},"risk_accepted":{"type":"boolean"}},"additionalProperties":false}),
        ),
    ] {
        let manifest = ToolManifest {
            origin: ToolOrigin::FirstParty,
            local_name: name.into(),
            display_name: name.into(),
            aliases: vec![],
            version: "1.0.0".into(),
            description: description.into(),
            input_schema: ToolSchemaDocument {
                dialect: "json-schema/2020-12".into(),
                schema,
            },
            output_schema: None,
            permission: PermissionContract {
                kind,
                capability_base: format!("{name}.apply"),
            },
            availability: Availability::Available,
            timeout_ms: Some(60_000),
            max_concurrency: None,
            declared_permission: DeclaredPermission::Execute,
            recovery: lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE,
        };
        let registered = registry
            .register(manifest, budget)
            .expect("会话工具必须登记成功");
        let executor: Arc<dyn ToolExecutorPort> = Arc::new(Executor(Arc::clone(&tools)));
        if name == "rewind" {
            let db = Arc::clone(&tools.deps.storage);
            gateway.bind_executor_with_review(
                registered.target_id,
                executor,
                Arc::new(move |input, args| review(&db, input, args.as_value())),
                "具体会话检查点确认后延迟回退",
            );
        } else {
            gateway.bind_executor(registered.target_id, executor, "当前Run的会话检查点");
        }
    }
}

pub async fn deferred_route(
    axum::Extension(principal): axum::Extension<Principal>,
    axum::extract::State(state): axum::extract::State<crate::ServiceState>,
    axum::extract::Path(sid): axum::extract::Path<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if let Err(response) = crate::gate_session_owned(&state, &principal, &sid).await {
        return response;
    }
    match state.session_tools.receipt(&sid) {
        Ok(receipt) => axum::Json(json!({"deferredRewind":receipt})).into_response(),
        Err(e) => {
            crate::EndpointError::storage(&lingxi_kernel::ports::StorageError::Io { detail: e })
                .into_response()
        }
    }
}

pub async fn approvals_route(
    axum::Extension(principal): axum::Extension<Principal>,
    axum::extract::State(state): axum::extract::State<crate::ServiceState>,
    axum::extract::Path(sid): axum::extract::Path<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if let Err(response) = crate::gate_session_owned(&state, &principal, &sid).await {
        return response;
    }
    let Some(service) = state.approvals.as_ref() else {
        return crate::EndpointError::new(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::Internal,
            "interactive approval service is not configured",
        )
        .into_response();
    };
    let kernel = crate::sessions::kernel_principal_of(&principal);
    let records = service.pending_for_principal(&kernel, &sid);
    axum::Json(json!({"approvals":records.into_iter().map(|r|json!({"approvalId":r.approval_id,"capability":state.tool_gateway().and_then(|gateway|gateway.mcp_session_capability(&r.target)),"target":r.target,"runId":r.run_id,"summary":r.args_summary,"resources":r.resources.iter().map(|s|json!({"path":s.path,"operation":s.op.wire_name()})).collect::<Vec<_>>(),"waitingSinceUnixMs":r.waiting_since_unix_ms,"expiresAtUnixMs":r.expires_at_unix_ms})).collect::<Vec<_>>()})).into_response()
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnswerBody {
    action: String,
}
pub async fn answer_route(
    axum::Extension(principal): axum::Extension<Principal>,
    axum::extract::State(state): axum::extract::State<crate::ServiceState>,
    axum::extract::Path((sid, id)): axum::extract::Path<(String, String)>,
    body: Result<axum::Json<AnswerBody>, axum::extract::rejection::JsonRejection>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let axum::Json(body) = match body {
        Ok(body) => body,
        Err(e) => return crate::EndpointError::from_json_rejection(&e).into_response(),
    };
    if let Err(response) = crate::gate_session_owned(&state, &principal, &sid).await {
        return response;
    }
    let answer = match body.action.as_str() {
        "confirmed" => crate::approval_service::Answer::Approve,
        "rejected" => crate::approval_service::Answer::Reject {
            reason: "user rejected the action".into(),
        },
        _ => {
            return crate::EndpointError::invalid_message(
                "approval action must be confirmed or rejected",
            )
            .into_response();
        }
    };
    let Some(service) = state.approvals.as_ref() else {
        return crate::EndpointError::new(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::Internal,
            "interactive approval service is not configured",
        )
        .into_response();
    };
    let outcome = service.answer_for_principal(
        &crate::sessions::kernel_principal_of(&principal),
        &sid,
        &id,
        answer,
    );
    match outcome {
        crate::approval_service::AnswerOutcome::Settled(decision)|crate::approval_service::AnswerOutcome::AlreadySettled(decision)=>axum::Json(json!({"ok":true,"decision":if matches!(decision,crate::approval_service::Answer::Approve){"confirmed"}else{"rejected"}})).into_response(),
        crate::approval_service::AnswerOutcome::Refused{reason}=>crate::EndpointError::new(axum::http::StatusCode::CONFLICT,ErrorCode::Conflict,reason).into_response(),
    }
}

#[cfg(test)]
mod receipt_tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_atomic_receipts_never_expose_a_partial_or_changed_snapshot() {
        let home = std::env::temp_dir().join(format!(
            "r06-receipt-io-{}",
            crate::auth::hex_random_public(12).unwrap()
        ));
        let layout = crate::prepare_layout(&home).unwrap();
        let state = crate::ServiceState::bootstrap_with_deps(
            crate::ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home,
                home_source: crate::HomeSource::Cli,
                network_mode: crate::NetworkMode::Loopback,
                shutdown_timeout_ms: 10000,
            },
            &layout,
            Default::default(),
        )
        .await
        .unwrap();
        let tools = Arc::clone(&state.session_tools);
        let value = |version: usize| json!({"sessionId":"sess_local_alpha","operationId":format!("receipt-{version}"),"process":tools.process,"status":"completed","version":version,"payload":format!("{version}:完整回执").repeat(256)});
        tools.save("sess_local_alpha", &value(0)).unwrap();
        let start = Arc::new(std::sync::Barrier::new(2));
        let writer = std::thread::spawn({
            let tools = Arc::clone(&tools);
            let start = Arc::clone(&start);
            move || {
                start.wait();
                for version in 1..=128usize {
                    let value = json!({"sessionId":"sess_local_alpha","operationId":format!("receipt-{version}"),"process":tools.process,"status":"completed","version":version,"payload":format!("{version}:完整回执").repeat(256)});
                    tools.save("sess_local_alpha", &value).unwrap();
                }
            }
        });
        let reader = std::thread::spawn({
            let tools = Arc::clone(&tools);
            move || {
                start.wait();
                let mut previous = 0;
                for _ in 0..512 {
                    let value = tools.receipt("sess_local_alpha").unwrap().unwrap();
                    let version = value["version"].as_u64().unwrap();
                    assert!(version >= previous, "回执版本不能倒退");
                    assert_eq!(value["operationId"], format!("receipt-{version}"));
                    assert_eq!(value["payload"], format!("{version}:完整回执").repeat(256));
                    previous = version;
                }
            }
        });
        writer.join().unwrap();
        reader.join().unwrap();
        assert_eq!(
            tools.receipt("sess_local_alpha").unwrap().unwrap()["version"],
            128
        );
        state.storage().close().await.unwrap();
    }
}
