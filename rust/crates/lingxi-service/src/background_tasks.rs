//! 后台任务查询及停止复用真实子 Run、原取消树和媒体交付栅栏。
use std::sync::{Arc, Weak};

use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Extension, Json,
};
use lingxi_adapters::storage::{subagent_threads, RunDatabase};
use lingxi_kernel::{
    ports::{ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, ToolSuccess},
    toolcatalog::{
        Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
        ToolManifest, ToolOrigin, ToolRegistry,
    },
    RunContext,
};
use lingxi_protocol::{ErrorCode, ProtocolError, ToolCallId, ToolSchemaDocument};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{auth::Principal, cancel::FireOutcome, runs::RunSupervisor, ServiceState};

pub struct TaskTools {
    storage: Arc<RunDatabase>,
    runs: Weak<RunSupervisor>,
    operations: Option<Arc<crate::operations::OperationService>>,
}
impl TaskTools {
    pub fn new(
        storage: Arc<RunDatabase>,
        runs: Weak<RunSupervisor>,
        operations: Option<Arc<crate::operations::OperationService>>,
    ) -> Self {
        Self {
            storage,
            runs,
            operations,
        }
    }
    async fn list(&self, session: &str, status: Option<&str>) -> Result<Value, String> {
        let mut tasks = subagent_threads::background_tasks(&self.storage, Some(session), None)
            .await
            .map_err(|e| e.to_string())?;
        if let Some(operations) = &self.operations {
            tasks.extend(operations.owned_background_tasks(session));
        }
        for task in &mut tasks {
            let date = task["deferredAtUnixMs"]
                .as_i64()
                .and_then(chrono::DateTime::from_timestamp_millis)
                .ok_or_else(|| "background task has invalid creation time".to_string())?;
            task["deferredAt"] = json!(date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        }
        if let Some(status) = status {
            tasks.retain(|task| task["status"] == status);
        }
        Ok(json!({"tasks":tasks,"available":true}))
    }
    async fn stop(&self, session: &str, task: &str, subagent_only: bool) -> Result<Value, String> {
        let Some(record) = subagent_threads::background_tasks(&self.storage, None, Some(task))
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .next()
        else {
            if !subagent_only {
                if let Some(result) = self
                    .operations
                    .as_ref()
                    .and_then(|operations| operations.stop_owned_background_task(task, session))
                {
                    return Ok(result);
                }
            }
            return Ok(json!({"status":"rejected","reason":"not_found"}));
        };
        if record["parentSessionId"] != session {
            return Ok(json!({"status":"rejected","reason":"session_mismatch"}));
        }
        if record["status"] != "pending" {
            return Ok(json!({"status":"already_stopped"}));
        }
        let Some(runs) = self.runs.upgrade() else {
            return Ok(json!({"status":"rejected","reason":"no_handler"}));
        };
        // taskId就是实际child Run的不可复用id，不按线程当前Run重新定位。
        Ok(
            match runs.cancel_run(task, "user stopped background task") {
                FireOutcome::Fired => json!({"status":"aborted"}),
                FireOutcome::AlreadyCancelling => json!({"status":"already_stopped"}),
                FireOutcome::TooLate => json!({"status":"already_stopped"}),
                FireOutcome::NotLive => json!({"status":"rejected","reason":"no_handler"}),
            },
        )
    }
    async fn invoke(&self, ctx: &RunContext, request: &ToolRequest) -> ToolOutcome {
        let owned=self.storage.query_one_text(
            "SELECT r.run_id FROM runs r JOIN sessions s ON s.session_id=r.session_id JOIN run_attempts a ON a.run_id=r.run_id WHERE r.run_id=?1 AND r.session_id=?2 AND r.owner_kind=?3 AND r.owner_subject=?4 AND s.owner_user_id=?4 AND r.generation=?5 AND a.attempt=?6 AND a.generation=r.generation AND r.status IN ('running','waiting_approval') AND a.attempt=(SELECT attempt FROM run_attempts WHERE run_id=r.run_id ORDER BY started_at_unix_ms DESC,rowid DESC LIMIT 1)",
            vec![ctx.run_id.to_string(),ctx.session_id.to_string(),ctx.principal.storage_kind().into(),ctx.principal.storage_subject(),ctx.generation.to_string(),ctx.attempt.to_string()]
        ).await;
        match owned {
            Ok(Some(_)) => {}
            Ok(None) => return failed("no active session with a stable caller identity"),
            Err(error) => return failed(error.to_string()),
        }
        let args = request.arguments.as_value();
        let value = match request.target.as_str() {
            "tool:first-party:check_pending_tasks" => {
                self.list(ctx.session_id.as_str(), args["status"].as_str())
                    .await
            }
            "tool:first-party:stop_task" => {
                let Some(task) = args["task_id"].as_str().filter(|id| !id.trim().is_empty()) else {
                    return failed("task_id is required");
                };
                self.stop(ctx.session_id.as_str(), task.trim(), false).await
            }
            _ => return failed("unknown background task tool"),
        };
        match value {
            Ok(value)
                if value["reason"] == "session_mismatch"
                    || value["reason"] == "stable_session_required" =>
            {
                failed("background task does not belong to this session")
            }
            Ok(value) => {
                let mut result = ToolSuccess::text(value.to_string());
                result.details = value.as_object().cloned();
                ToolOutcome::Success { result }
            }
            Err(error) => failed(error),
        }
    }
}
fn failed(message: impl Into<String>) -> ToolOutcome {
    ToolOutcome::Failed {
        error: ProtocolError::new(ErrorCode::InvalidMessage, message.into(), false),
    }
}
struct Executor(Arc<TaskTools>);
impl ToolExecutorPort for Executor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move { ToolExecutionResult::of_ctx(ctx, self.0.invoke(ctx, request).await) })
    }
}
pub fn register(
    registry: &ToolRegistry,
    gateway: &crate::toolgateway::ToolInvocationGateway,
    tools: Arc<TaskTools>,
    budget: &SchemaBudget,
) {
    for(name,description,kind,schema)in[
        ("check_pending_tasks","Read actual background task status and result in this conversation only.",PermissionKind::Read,json!({"type":"object","properties":{"status":{"type":"string","enum":["pending","resolved","failed"]}},"additionalProperties":false})),
        ("stop_task","Stop an actual background task belonging to this conversation. Missing, finished and unavailable stop handlers are reported distinctly.",PermissionKind::Execute,json!({"type":"object","properties":{"task_id":{"type":"string","minLength":1}},"required":["task_id"],"additionalProperties":false})),
    ] {
        let manifest=ToolManifest{origin:ToolOrigin::FirstParty,local_name:name.into(),display_name:name.into(),aliases:vec![],version:"1.0.0".into(),description:description.into(),input_schema:ToolSchemaDocument{dialect:"json-schema/2020-12".into(),schema},output_schema:None,permission:PermissionContract{kind,capability_base:format!("{name}.apply")},availability:Availability::Available,timeout_ms:Some(30_000),max_concurrency:Some(4),declared_permission:DeclaredPermission::None,recovery:lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE};
        let registered=registry.register(manifest,budget).expect("后台任务工具必须登记成功");
        gateway.bind_executor(registered.target_id,Arc::new(Executor(Arc::clone(&tools))),"真实会话后台任务");
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StopBody {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub session_path: Option<String>,
}
/// HTTP及WebSocket共享同一身份解析和实际停止入口。
pub async fn stop_owned(
    state: &ServiceState,
    principal: &Principal,
    body: &StopBody,
    task: &str,
) -> Result<Value, Box<Response>> {
    let session = crate::session_locator::resolve(
        state.storage(),
        body.session_id.as_deref(),
        body.session_path.as_deref(),
    )
    .await
    .map_err(|error| {
        Box::new(match error {
            lingxi_kernel::ports::StorageError::InvalidRequest { .. } => {
                crate::EndpointError::invalid_message("stable_session_required").into_response()
            }
            other => crate::EndpointError::storage(&other).into_response(),
        })
    })?;
    crate::gate_session_owned(state, principal, &session)
        .await
        .map_err(Box::new)?;
    let tools = TaskTools::new(
        Arc::clone(state.storage()),
        Arc::downgrade(state.runs()),
        state.operations().cloned(),
    );
    let result = tools.stop(&session, task, true).await.map_err(|error| {
        Box::new(
            crate::EndpointError::new(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::Internal,
                error,
            )
            .into_response(),
        )
    })?;
    Ok(
        json!({"sessionId":session,"sessionPath":crate::session_locator::native(&session),"taskId":task,"status":result["status"],"reason":result["reason"]}),
    )
}
pub async fn stop_route(
    Extension(principal): Extension<Principal>,
    State(state): State<ServiceState>,
    Path(task): Path<String>,
    body: Result<Json<StopBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(error) => return crate::EndpointError::from_json_rejection(&error).into_response(),
    };
    match stop_owned(&state, &principal, &body, &task).await {
        Err(response) => *response,
        Ok(result) => {
            let ok = result["status"] == "aborted" || result["status"] == "already_stopped";
            let code = if ok {
                axum::http::StatusCode::OK
            } else if result["reason"] == "not_found" {
                axum::http::StatusCode::NOT_FOUND
            } else {
                axum::http::StatusCode::FORBIDDEN
            };
            let mut result = result;
            result["ok"] = json!(ok);
            (code, Json(result)).into_response()
        }
    }
}

pub async fn ws_stop(
    state: &ServiceState,
    principal: &Principal,
    body: StopBody,
    task: &str,
    request_id: Option<String>,
) -> Value {
    let mut result = match stop_owned(state, principal, &body, task).await {
        Ok(result) => result,
        Err(response) => {
            let status = response.status();
            let reason = if status == axum::http::StatusCode::FORBIDDEN {
                "session_forbidden"
            } else if status == axum::http::StatusCode::BAD_REQUEST
                || status == axum::http::StatusCode::NOT_FOUND
            {
                "stable_session_required"
            } else {
                "storage_failure"
            };
            json!({"taskId":task,"status":"rejected","reason":reason})
        }
    };
    result["type"] = json!("subagent_stop_result");
    if let Some(id) = request_id {
        result["requestId"] = json!(id);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::{MediaTaskStatus, OperationCallContext, OperationService};
    use lingxi_adapters::models::{gateway::ConfigModelGateway, operations::video::VideoRequest};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn owned_media_query_and_cancel_use_actual_supplier_and_fence_late_products() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let entered = Arc::new(tokio::sync::Semaphore::new(0));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let downloads = Arc::new(AtomicUsize::new(0));
        let submits = Arc::new(AtomicUsize::new(0));
        let router=axum::Router::new().route("/v1/videos",axum::routing::post({let submits=submits.clone();move ||{let submits=submits.clone();async move{submits.fetch_add(1,Ordering::SeqCst);Json(json!({"task_id":"supplier-reused-id","video_id":"actual-supplier-id"}))}}}))
            .route("/agnesapi",axum::routing::get({let entered=entered.clone();let release=release.clone();let endpoint=endpoint.clone();move||{let entered=entered.clone();let release=release.clone();let endpoint=endpoint.clone();async move{entered.add_permits(1);release.acquire().await.unwrap().forget();Json(json!({"status":"completed","video_url":format!("{endpoint}/product")}))}}}))
            .route("/product",axum::routing::get({let downloads=downloads.clone();move||{let downloads=downloads.clone();async move{downloads.fetch_add(1,Ordering::SeqCst);([("content-type","video/mp4")],vec![1u8,2,3,4])}}}));
        let supplier = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let home = std::env::temp_dir().join(format!(
            "r06-owned-media-{}",
            crate::auth::hex_random_public(12).unwrap()
        ));
        let layout = crate::prepare_layout(&home).unwrap();
        let state = crate::ServiceState::bootstrap_with_deps(
            crate::ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home.clone(),
                home_source: crate::HomeSource::Cli,
                network_mode: crate::NetworkMode::Loopback,
                shutdown_timeout_ms: 10000,
            },
            &layout,
            Default::default(),
        )
        .await
        .unwrap();
        let plane=lingxi_adapters::models::config::ModelPlaneConfig::parse_and_validate(&json!({"providers":{"text":{"protocol":"openai-completions","endpoint":format!("{endpoint}/v1"),"auth":{"kind":"apiKey","apiKey":"synthetic-test-only"}},"video":{"protocol":"agnes-videos","endpoint":format!("{endpoint}/v1"),"auth":{"kind":"apiKey","apiKey":"synthetic-video-only"}}},"models":{"chat":{"provider":"text","model":"test"},"summarize":{"provider":"text","model":"test"},"video":{"provider":"video","model":"video-test"}}}).to_string()).unwrap();
        let credentials = Arc::new(
            crate::credentials::CredentialService::bootstrap(
                &plane,
                &layout.runtime_dir,
                Arc::new(crate::inject::SystemClock),
            )
            .unwrap(),
        );
        let products = home.join("products");
        let operations = Arc::new(
            OperationService::new(
                Arc::new(ConfigModelGateway::from_validated(plane)),
                credentials,
                Arc::new(crate::quotas::QuotaManager::new(Default::default())),
                vec![endpoint.clone()],
                products.clone(),
            )
            .unwrap(),
        );
        let context = |session: &str| OperationCallContext {
            session_id: session.into(),
            run_id: format!("trusted-host-{session}"),
            attempt: None,
            cause_ref: None,
        };
        let request = || VideoRequest {
            prompt: "synthetic video".into(),
            ..Default::default()
        };
        let a = operations
            .submit_video_for_context(request(), None, &context("sess_local_alpha"))
            .await
            .unwrap();
        let b = operations
            .submit_video_for_context(request(), None, &context("sess_local_beta"))
            .await
            .unwrap();
        let independent = operations.submit_video(request(), None).await.unwrap();
        assert_ne!(a.task_id, b.task_id);
        assert_ne!(b.task_id, independent.task_id);
        assert_eq!(submits.load(Ordering::SeqCst), 3);
        let tools = TaskTools::new(
            state.storage().clone(),
            Arc::downgrade(state.runs()),
            Some(operations.clone()),
        );
        let all = tools.list("sess_local_alpha", None).await.unwrap();
        assert_eq!(all["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(all["tasks"][0]["taskId"], a.task_id);
        assert_eq!(all["tasks"][0]["type"], "video");
        assert_eq!(all["tasks"][0]["status"], "pending");
        assert!(all["tasks"][0]["deferredAt"]
            .as_str()
            .unwrap()
            .ends_with('Z'));
        assert_eq!(
            tools
                .stop("sess_local_alpha", &b.task_id, false)
                .await
                .unwrap()["reason"],
            "session_mismatch"
        );
        assert_eq!(
            tools
                .stop("sess_local_alpha", &independent.task_id, false)
                .await
                .unwrap()["reason"],
            "stable_session_required"
        );
        assert_eq!(
            tools
                .stop("sess_local_alpha", &a.task_id, true)
                .await
                .unwrap()["reason"],
            "not_found",
            "仅子代理入口不能终止媒体任务"
        );
        let poll = tokio::spawn({
            let operations = operations.clone();
            let id = a.task_id.clone();
            async move { operations.query_video(&id, None).await }
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
        let stop = tools
            .stop("sess_local_alpha", &a.task_id, false)
            .await
            .unwrap();
        assert_eq!(stop["status"], "aborted");
        assert_eq!(stop["remoteCancel"], "unsupported");
        assert_eq!(stop["localDeliveryStopped"], true);
        release.add_permits(1);
        assert!(matches!(
            poll.await.unwrap().unwrap(),
            MediaTaskStatus::CancelledLocally { .. }
        ));
        assert_eq!(downloads.load(Ordering::SeqCst), 0);
        assert!(
            !products.exists() || std::fs::read_dir(&products).unwrap().next().is_none(),
            "迟到供应商结果不得产出文件"
        );
        assert_eq!(
            operations.owned_background_tasks("sess_local_beta")[0]["status"],
            "pending"
        );
        assert!(tools
            .list("sess_local_alpha", Some("pending"))
            .await
            .unwrap()["tasks"]
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(
            tools
                .list("sess_local_alpha", Some("failed"))
                .await
                .unwrap()["tasks"][0]["reason"],
            "cancelled_locally"
        );
        assert_eq!(
            tools
                .stop("sess_local_alpha", &a.task_id, false)
                .await
                .unwrap()["status"],
            "already_stopped"
        );
        let next = operations
            .submit_video_for_context(request(), None, &context("sess_local_alpha"))
            .await
            .unwrap();
        assert_ne!(
            next.task_id, a.task_id,
            "供应商复用id不能重用会话宿主taskId"
        );
        assert_eq!(
            tools
                .stop("sess_local_alpha", &a.task_id, false)
                .await
                .unwrap()["status"],
            "already_stopped"
        );
        assert!(
            operations
                .owned_background_tasks("sess_local_alpha")
                .iter()
                .any(|task| task["taskId"] == next.task_id && task["status"] == "pending"),
            "旧id停止不影响新一轮媒体任务"
        );
        assert_eq!(submits.load(Ordering::SeqCst), 4);
        supplier.abort();
        let _ = supplier.await;
        state.storage().close().await.unwrap();
    }
}
