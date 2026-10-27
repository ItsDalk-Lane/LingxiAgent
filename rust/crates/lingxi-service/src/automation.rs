//! 自动化入口与调度宿主：触发记录在执行前落盘，运行权始终属于既有 Supervisor。
use crate::{automation_schedule, Principal, ServiceState};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use lingxi_adapters::storage::{automation as store, session_tree};
use lingxi_kernel::{
    ports::StoragePort,
    subagent::{RunOrigin, SessionPermissionMode},
};
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

const CRON_TIMEOUT_MS: u64 = 20 * 60_000;
const MAX_DUE_BATCH: usize = 256;

fn error(value: impl std::fmt::Debug) -> String {
    format!("{value:?}")
}
pub(crate) fn local_owner_principal() -> Principal {
    crate::auth::Principal::scheduler_owner()
}
fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{key} required"))
}
pub fn trigger_id(studio: &str, job: &str, revision: u64, due: i64) -> String {
    let input = serde_json::to_vec(&(studio, job, revision, due)).expect("触发标识可序列化");
    format!(
        "trigger-{}",
        lingxi_kernel::session_tree::sha256_hex(&input)
    )
}
fn utc_text(ms: i64) -> Result<String, String> {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .ok_or_else(|| "automation timestamp out of range".into())
}
fn config_of(job: &store::AutomationJob) -> Result<Value, String> {
    serde_json::from_str(&job.config_json).map_err(error)
}
pub fn job_view(job: &store::AutomationJob) -> Result<Value, String> {
    let mut value = config_of(job)?;
    value["id"] = json!(job.id);
    value["studioId"] = json!(job.studio_id);
    value["actorAgentId"] = json!(job.actor_id);
    value["schemaVersion"] = json!(job.schema_version);
    value["configRevision"] = json!(job.config_revision);
    value["nextRunAt"] = match job.next_run_at {
        Some(ms) => json!(utc_text(ms)?),
        None => Value::Null,
    };
    Ok(value)
}

/// 从受认证配置建立计划，正文不能选择所有者、工作室或已用过的任务身份。
#[derive(Clone)]
pub struct JobApi {
    pub storage: Arc<lingxi_adapters::storage::RunDatabase>,
    pub memory: Arc<crate::memory::MemoryService>,
    pub clock: Arc<dyn crate::inject::ServiceClock>,
    pub request_ids: Arc<dyn crate::inject::RequestIdGen>,
    pub studio_id: String,
}
impl JobApi {
    pub fn from_state(state: &ServiceState) -> Self {
        Self {
            storage: state.storage().clone(),
            memory: state.memory().clone(),
            clock: state.clock().clone(),
            request_ids: state.request_ids().clone(),
            studio_id: state.management.identity_ids().1,
        }
    }
}
pub async fn prepare_job(
    api: &JobApi,
    principal: &Principal,
    mut value: Value,
    prior: Option<&store::AutomationJob>,
) -> Result<store::AutomationJobInput, String> {
    if !principal.is_local_owner() {
        return Err("automation requires local owner".into());
    }
    if !value.is_object() {
        return Err("automation job must be an object".into());
    }
    let now = api.clock.now_unix_ms();
    if let Some(kind) = value.get("scheduleType").cloned() {
        value["type"] = kind;
        value.as_object_mut().unwrap().remove("scheduleType");
    }
    let actor = required(&value, "actorAgentId")?.to_string();
    let roots = api.memory.roots(&actor).map_err(error)?;
    if !roots.agent_dir.is_dir() || api.memory.is_agent_deleted_checked(&actor).map_err(error)? {
        return Err("automation actor not found".into());
    }
    let kind = required(&value, "type")?.to_string();
    let schedule = value.get("schedule").cloned().ok_or("schedule required")?;
    if kind == "every" {
        value["schedule"] = json!(automation_schedule::every_interval(&schedule)?);
    }
    if let Some(executor) = value.get("executor") {
        if !executor.is_null()
            && executor.get("kind").and_then(Value::as_str) != Some("agent_session")
        {
            return Err("automation executor must be agent_session; legacy actions require normalized Agent prompts".into());
        }
    }
    if value.get("enabled").is_some_and(|v| !v.is_boolean()) {
        return Err("enabled must be boolean".into());
    }
    value["origin"] = json!("cron");
    let enabled = value.get("enabled") != Some(&Value::Bool(false));
    if enabled {
        required(&value, "prompt")?;
    }
    if !value.get("executionContext").is_some_and(Value::is_object) {
        return Err("executionContext required".into());
    }
    validate_source(&api.storage, &actor, &value).await?;
    let next = automation_schedule::next_run(&kind, &value["schedule"], now)?;
    if enabled && kind == "at" && next.is_none() {
        return Err("one-shot schedule must be in the future".into());
    }
    if let Some(schema) = value.get("schemaVersion") {
        if schema.as_u64() != Some(4) {
            return Err("unsupported_automation_schema".into());
        }
    }
    let id = prior
        .map(|j| j.id.clone())
        .unwrap_or_else(|| format!("job-{}", api.request_ids.next_request_id()));
    let same_schedule = prior.map(config_of).transpose()?.is_some_and(|old| {
        old["type"] == value["type"]
            && old["schedule"] == value["schedule"]
            && old["enabled"] == value["enabled"]
    });
    value["enabled"] = json!(enabled);
    value["schemaVersion"] = json!(4);
    let data = store::AutomationJobInput {
        id: id.clone(),
        studio_id: api.studio_id.clone(),
        actor_id: actor,
        owner_kind: "local_user".into(),
        owner_subject: lingxi_kernel::LOCAL_OWNER_SUBJECT.into(),
        schema_version: 4,
        config_json: value.to_string(),
        next_run_at: if !enabled {
            None
        } else if same_schedule {
            prior.and_then(|j| j.next_run_at)
        } else {
            next.map(|v| v as i64)
        },
    };
    Ok(data)
}
pub async fn save_job(
    api: &JobApi,
    principal: &Principal,
    value: Value,
    prior: Option<&store::AutomationJob>,
) -> Result<store::AutomationJob, String> {
    let data = prepare_job(api, principal, value, prior).await?;
    match prior {
        None => store::create_job(&api.storage, &data, api.clock.now_unix_ms() as i64)
            .await
            .map_err(error),
        Some(old) => store::update_job(
            &api.storage,
            &data.id,
            &data,
            old.config_revision,
            api.clock.now_unix_ms() as i64,
        )
        .await
        .map_err(error),
    }
}

/// 每次派发重查当前稳定会话，不从保存的路径和文件夹列表恢复旧权限。
async fn validate_source(
    storage: &lingxi_adapters::storage::RunDatabase,
    actor: &str,
    value: &Value,
) -> Result<Option<session_tree::SessionTreeRow>, String> {
    let context = value
        .get("executionContext")
        .ok_or("executionContext required")?;
    if let Some(source) = context
        .get("sourceSessionId")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        let source = session_tree::get_session_row(storage, source)
            .await
            .map_err(error)?
            .ok_or("automation source session unavailable")?;
        if source.agent_id != actor
            || source.lifecycle != "active"
            || source.owner_user_id != lingxi_kernel::LOCAL_OWNER_SUBJECT
        {
            return Err("automation source session no longer belongs to actor or owner".into());
        }
        Ok(Some(source))
    } else {
        if context
            .get("sourceSessionPath")
            .is_some_and(|p| !p.is_null())
        {
            return Err("sourceSessionPath requires stable sourceSessionId".into());
        }
        Ok(None)
    }
}

fn chat_for_job(
    state: &ServiceState,
    cfg: &Value,
) -> Result<Option<crate::runs::ChatRunInput>, String> {
    let Some(model) = cfg
        .get("model")
        .filter(|m| !m.is_null() && *m != &Value::String(String::new()))
    else {
        return Ok(None);
    };
    let (provider, id) = if let Some(id) = model.as_str() {
        match id.split_once('/') {
            Some((provider, id)) => (Some(provider.to_string()), id.to_string()),
            None => (None, id.to_string()),
        }
    } else {
        (
            model["provider"].as_str().map(str::to_string),
            required(model, "id")?.to_string(),
        )
    };
    let route = state
        .model_gateway()
        .ok_or("model gateway unavailable")?
        .resolve_dispatch(&lingxi_kernel::model_exchange::ModelRouteRequest {
            operation: lingxi_kernel::model_exchange::ModelOperation::Chat,
            provider,
            model: Some(id),
        })
        .map_err(error)?;
    Ok(Some(crate::runs::ChatRunInput {
        images: vec![],
        files: vec![],
        client_message_id: None,
        snapshot_version: None,
        display_text: None,
        quoted_text: None,
        session_refs: vec![],
        options: lingxi_kernel::model_exchange::ModelTurnOptions {
            model: Some((route.route.provider, route.route.model)),
            thinking_level: None,
        },
    }))
}

pub struct AutomationHost {
    import_report: Mutex<Value>,
    patrol: Arc<crate::automation_patrol::PatrolHost>,
    cancellation: std::sync::atomic::AtomicBool,
    cron_check: tokio::sync::Mutex<()>,
    stop: tokio::sync::watch::Sender<bool>,
    tasks: Mutex<Vec<tokio::task::JoinHandle<Result<(), String>>>>,
    shutdown_wait: tokio::sync::Mutex<()>,
    shutdown_failures: Mutex<Vec<String>>,
    last_error: Mutex<Option<String>>,
}
impl Default for AutomationHost {
    fn default() -> Self {
        Self {
            import_report: Mutex::new(Value::Null),
            patrol: Arc::new(crate::automation_patrol::PatrolHost::default()),
            cancellation: std::sync::atomic::AtomicBool::new(false),
            cron_check: tokio::sync::Mutex::new(()),
            stop: tokio::sync::watch::channel(false).0,
            tasks: Mutex::new(Vec::new()),
            shutdown_wait: tokio::sync::Mutex::new(()),
            shutdown_failures: Mutex::new(Vec::new()),
            last_error: Mutex::new(None),
        }
    }
}
impl AutomationHost {
    pub fn set_import_report(&self, report: Value) {
        *self.import_report.lock().expect("导入报告锁") = report;
    }
    pub fn import_report(&self) -> Value {
        self.import_report.lock().expect("导入报告锁").clone()
    }
    pub fn patrol(&self) -> &Arc<crate::automation_patrol::PatrolHost> {
        &self.patrol
    }
    pub fn start(self: &Arc<Self>, state: ServiceState) {
        let Ok(_waiting) = self.shutdown_wait.try_lock() else {
            return;
        };
        let mut tasks = self.tasks.lock().expect("调度宿主状态");
        if !tasks.is_empty() {
            return;
        }
        if !self.patrol.start(state.clone()) {
            return;
        }
        self.shutdown_failures
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        self.stop.send_replace(false);
        self.cancellation
            .store(false, std::sync::atomic::Ordering::Release);
        let daily_state = state.clone();
        let daily_host = Arc::clone(self);
        tasks.push(tokio::spawn(async move {
            let local_now=chrono::DateTime::from_timestamp_millis(daily_state.clock().now_unix_ms() as i64).ok_or("daily clock out of range")?.with_timezone(&chrono::Local);
            let mut delay=crate::automation_heartbeat::initial_daily_delay_ms(&local_now).map_err(error)?;
            let mut stop=daily_host.stop.subscribe();
            loop {
                tokio::select! {biased;_ = stop.wait_for(|v|*v)=>break,_=tokio::time::sleep(Duration::from_millis(delay))=>{}}
                let report=crate::automation_daily::check_daily_cancellable(&daily_state,daily_state.clock().now_unix_ms(),&daily_host.cancellation).await;
                let now=chrono::DateTime::from_timestamp_millis(daily_state.clock().now_unix_ms() as i64).ok_or("daily clock out of range")?.with_timezone(&chrono::Local);
                let result=match report {
                    Ok(report)=>crate::automation_heartbeat::DailyFreshCompactResult{stale_remaining:report["staleRemaining"].as_u64().unwrap_or(0),retry:report["retry"].as_bool().unwrap_or(false),failed:report["failed"].as_u64().unwrap_or(0)>0},
                    Err(err)=>{tracing::error!(error=%err,"每日整理失败");*daily_host.last_error.lock().map_err(error)?=Some(err);crate::automation_heartbeat::DailyFreshCompactResult{failed:true,..Default::default()}}
                };
                delay=crate::automation_heartbeat::next_daily_delay_ms(&now,result).map_err(error)?;
            }
            Ok(())
        }));
        let host = Arc::clone(self);
        tasks.push(tokio::spawn(async move {
            host.recover_pending(&state).await?;
            let mut stopped = host.stop.subscribe();
            let mut ticks = tokio::time::interval(Duration::from_secs(60));
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    biased;
                    _=stopped.changed()=>break,
                    _=ticks.tick()=> {
                        if *stopped.borrow(){break;}
                        if let Err(err)=host.check_due(&state).await {
                            tracing::error!(error=%err,"自动化调度失败");
                            *host.last_error.lock().expect("调度错误状态")=Some(err);
                        }
                    }
                }
            }
            Ok(())
        }));
    }
    pub fn stop(&self) {
        self.patrol.stop();
        self.cancellation
            .store(true, std::sync::atomic::Ordering::Release);
        self.stop.send_replace(true);
    }
    pub async fn wait_stopped(&self) -> Result<(), String> {
        let _waiting = self.shutdown_wait.lock().await;
        loop {
            // Pending时句柄仍在宿主内；外层timeout取消本future不会detach任务。
            let outcome = std::future::poll_fn(|cx| {
                let mut tasks = match self.tasks.lock() {
                    Ok(tasks) => tasks,
                    Err(poisoned) => {
                        let mut failures = self
                            .shutdown_failures
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        let detail = "automation task registry lock poisoned".to_string();
                        if !failures.contains(&detail) {
                            failures.push(detail);
                        }
                        poisoned.into_inner()
                    }
                };
                let Some(task) = tasks.last_mut() else {
                    return std::task::Poll::Ready(None);
                };
                match std::future::Future::poll(std::pin::Pin::new(task), cx) {
                    std::task::Poll::Pending => std::task::Poll::Pending,
                    std::task::Poll::Ready(outcome) => {
                        tasks.pop();
                        std::task::Poll::Ready(Some(outcome))
                    }
                }
            })
            .await;
            let Some(outcome) = outcome else {
                break;
            };
            match outcome {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    self.shutdown_failures
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .push(error);
                }
                Err(join) => {
                    self.shutdown_failures
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .push(error(join));
                }
            }
        }
        if let Err(error) = self.patrol.wait_stopped().await {
            let mut failures = self
                .shutdown_failures
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if !failures.contains(&error) {
                failures.push(error);
            }
        }
        let failures = self
            .shutdown_failures
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }
    pub fn status(&self) -> Value {
        json!({"running":self.tasks.lock().expect("调度宿主状态").iter().any(|task|!task.is_finished()),"lastError":*self.last_error.lock().expect("调度错误状态"),"importReport":self.import_report()})
    }

    pub async fn recover_pending(&self, state: &ServiceState) -> Result<(), String> {
        store::recover_started_notifications(state.storage(), state.clock().now_unix_ms() as i64)
            .await
            .map_err(error)?;
        for pending in store::list_pending_triggers(state.storage())
            .await
            .map_err(error)?
        {
            let run = store::get_trigger_run(state.storage(), &pending.trigger_id)
                .await
                .map_err(error)?;
            let (status, result, finished_at) = match run {
                Some((run, sid, status)) => {
                    store::bind_run(
                        state.storage(),
                        &pending.trigger_id,
                        &sid,
                        &run,
                        state.clock().now_unix_ms() as i64,
                    )
                    .await
                    .map_err(error)?;
                    let finished = if matches!(
                        status.as_str(),
                        "completed" | "failed" | "cancelled"
                    ) {
                        let timestamp=state.storage().query_one_text("SELECT CAST(updated_at_unix_ms AS TEXT) FROM runs WHERE run_id=?1 AND session_id=?2 AND status IN ('completed','failed','cancelled')",vec![run.clone(),sid.clone()]).await.map_err(error)?.ok_or("recovered automation completion missing")?;
                        Some(
                            timestamp
                                .parse::<u64>()
                                .map_err(|_| "recovered automation completion time is corrupt")?,
                        )
                    } else {
                        None
                    };
                    let settled = match status.as_str() {
                        "completed" => store::TriggerStatus::Succeeded,
                        "failed" => store::TriggerStatus::Failed,
                        "cancelled" => store::TriggerStatus::Cancelled,
                        _ => store::TriggerStatus::Unknown,
                    };
                    (
                        settled,
                        json!({"runId":run,"sessionId":sid,"status":status,"recovered":true}),
                        finished,
                    )
                }
                None => (
                    store::TriggerStatus::Unknown,
                    json!({"reason":"dispatch_unconfirmed","recovered":true}),
                    None,
                ),
            };
            let next = if matches!(status, store::TriggerStatus::Unknown) {
                None
            } else {
                let cfg: Value = serde_json::from_str(&pending.config_json).map_err(error)?;
                let history =
                    store::list_trigger_history(state.storage(), Some(&pending.job_id), 100)
                        .await
                        .map_err(error)?;
                let previous_errors = history
                    .iter()
                    .filter(|t| t.trigger_id != pending.trigger_id)
                    .take_while(|t| matches!(t.status, store::TriggerStatus::Failed))
                    .count() as u32;
                automation_schedule::next_after_result(
                    required(&cfg, "type")?,
                    &cfg["schedule"],
                    finished_at
                        .ok_or("recovered completion time missing")?
                        .max(pending.due_at.max(0) as u64),
                    matches!(status, store::TriggerStatus::Succeeded),
                    previous_errors,
                )?
                .next_run_at_ms
                .map(|n| n as i64)
            };
            self.publish_activity(state, &pending.trigger_id).await;
            store::settle_trigger(
                state.storage(),
                &pending.trigger_id,
                status,
                Some(result),
                Some("recovered without replaying external effects".into()),
                if matches!(status, store::TriggerStatus::Unknown) {
                    None
                } else {
                    next
                },
                state.clock().now_unix_ms() as i64,
            )
            .await
            .map_err(error)?;
        }
        Ok(())
    }

    /// 全批串行；长任务期间不会再次抢到本批检查权。
    pub async fn check_due(&self, state: &ServiceState) -> Result<usize, String> {
        let Ok(_check) = self.cron_check.try_lock() else {
            return Ok(0);
        };
        let jobs = store::list_due_jobs(
            state.storage(),
            state.clock().now_unix_ms() as i64,
            MAX_DUE_BATCH,
        )
        .await
        .map_err(error)?;
        let mut count = 0;
        for listed in jobs {
            if *self.stop.borrow() || state.submission_intake().is_closed() {
                break;
            }
            let Some(job) = store::get_job(state.storage(), &listed.id)
                .await
                .map_err(error)?
            else {
                continue;
            };
            let cfg = config_of(&job)?;
            if cfg.get("enabled") == Some(&Value::Bool(false)) || cfg["origin"] == "heartbeat" {
                continue;
            }
            let Some(due) = job.next_run_at else { continue };
            if due > state.clock().now_unix_ms() as i64 {
                continue;
            }
            let id = trigger_id(&job.studio_id, &job.id, job.config_revision, due);
            let claim = store::claim_trigger(
                state.storage(),
                &store::TriggerInput {
                    trigger_id: id.clone(),
                    job_id: job.id.clone(),
                    due_at: due,
                    config_revision: job.config_revision,
                },
                state.clock().now_unix_ms() as i64,
            )
            .await
            .map_err(error)?;
            if !claim.created {
                continue;
            }
            let outcome = if job.schema_version != 4 {
                Ok((
                    store::TriggerStatus::Skipped,
                    json!({"reason":"unsupported_automation_schema","schemaVersion":job.schema_version}),
                ))
            } else {
                self.execute_job(state, &job, &cfg, &id).await
            };
            let (status, result, failure) = match outcome {
                Ok((status, result)) => (status, Some(result), None),
                Err(err) => {
                    // 错误可能发生在真实受理之后；先按不可变因果关系查账，绝不盲目重跑。
                    match store::get_trigger_run(state.storage(), &id).await {
                        Ok(Some((run, sid, status))) => {
                            let terminal = match status.as_str() {
                                "completed" => store::TriggerStatus::Succeeded,
                                "failed" => store::TriggerStatus::Failed,
                                "cancelled" => store::TriggerStatus::Cancelled,
                                _ => store::TriggerStatus::Unknown,
                            };
                            (
                                terminal,
                                Some(json!({"runId":run,"sessionId":sid,"status":status})),
                                Some(err),
                            )
                        }
                        Ok(None) => {
                            let count = state.storage().count_runs(&format!("activity-{id}")).await;
                            let status = if matches!(count, Ok(0)) {
                                store::TriggerStatus::Failed
                            } else {
                                store::TriggerStatus::Unknown
                            };
                            (status, None, Some(err))
                        }
                        Err(read_error) => (
                            store::TriggerStatus::Unknown,
                            None,
                            Some(format!("{err}; run lookup: {read_error:?}")),
                        ),
                    }
                }
            };
            let success = matches!(status, store::TriggerStatus::Succeeded);
            let history = store::list_trigger_history(state.storage(), Some(&job.id), 100)
                .await
                .map_err(error)?;
            let previous_errors = history
                .iter()
                .filter(|t| t.trigger_id != id)
                .take_while(|t| matches!(t.status, store::TriggerStatus::Failed))
                .count() as u32;
            // 运行中系统时钟可能回拨；游标不退回本次已经领取的到期时间之前。
            let next_run_at = if matches!(
                status,
                store::TriggerStatus::Skipped | store::TriggerStatus::Unknown
            ) {
                None
            } else {
                automation_schedule::next_after_result(
                    required(&cfg, "type")?,
                    &cfg["schedule"],
                    state.clock().now_unix_ms().max(due.max(0) as u64),
                    success,
                    previous_errors,
                )?
                .next_run_at_ms
                .map(|n| n as i64)
            };
            store::settle_trigger(
                state.storage(),
                &id,
                status,
                result,
                failure,
                next_run_at,
                state.clock().now_unix_ms() as i64,
            )
            .await
            .map_err(error)?;
            state.app_events.publish(
                "cron_job_done",
                json!({"jobId":job.id,"triggerId":id,"actorAgentId":job.actor_id,"status":status}),
            );
            count += 1;
        }
        Ok(count)
    }

    async fn publish_activity(&self, state: &ServiceState, id: &str) {
        match crate::automation_activity::activity_for_trigger(state, id).await {
            Ok(Some(activity)) => {
                if let Err(error) = queue_completion(state, &activity).await {
                    tracing::error!(%error,"后台完成提醒排队失败");
                    *self.last_error.lock().expect("调度错误状态") = Some(error);
                }
                state
                    .app_events()
                    .publish("activity_update", json!({"activity":activity}));
            }
            Ok(None) => {}
            Err(error) => {
                tracing::error!(%error,"后台活动投影失败");
                *self.last_error.lock().expect("调度错误状态") = Some(error);
            }
        }
    }

    pub(crate) async fn execute_job(
        &self,
        state: &ServiceState,
        job: &store::AutomationJob,
        cfg: &Value,
        id: &str,
    ) -> Result<(store::TriggerStatus, Value), String> {
        let fresh = store::get_job(state.storage(), &job.id)
            .await
            .map_err(error)?;
        if !fresh.as_ref().is_some_and(|j| {
            j.config_revision == job.config_revision
                && config_of(j).is_ok_and(|c| c.get("enabled") != Some(&Value::Bool(false)))
        }) {
            return Ok((
                store::TriggerStatus::Skipped,
                json!({"reason":"automation_configuration_changed_before_dispatch"}),
            ));
        }
        let source = validate_source(state.storage(), &job.actor_id, cfg).await?;
        if state
            .memory()
            .is_agent_deleted_checked(&job.actor_id)
            .map_err(error)?
        {
            return Err("automation actor deleted".into());
        }
        let prefs = state.memory().read_preferences_root()?;
        let mode = match prefs
            .pointer("/automation/permissionMode")
            .and_then(Value::as_str)
            .unwrap_or("auto")
        {
            "operate" => SessionPermissionMode::Operate,
            "auto" => SessionPermissionMode::Ask,
            "read_only" => SessionPermissionMode::ReadOnly,
            _ => return Err("invalid automation permission mode".into()),
        };
        let mode = if source
            .as_ref()
            .is_some_and(|s| s.permission_mode.as_deref() == Some("read_only"))
        {
            SessionPermissionMode::ReadOnly
        } else {
            mode
        };
        let folders = source
            .as_ref()
            .map(|s| s.authorized_folders.clone())
            .unwrap_or_default();
        let session_id = format!("activity-{id}");
        state
            .session_admin()
            .create_session(
                &session_id,
                &job.actor_id,
                lingxi_kernel::LOCAL_OWNER_SUBJECT,
                cfg["label"].as_str().unwrap_or("定时任务"),
                Some(mode.wire_name()),
                &folders,
                true,
                state.clock().now_unix_ms(),
            )
            .await
            .map_err(error)?;
        lingxi_adapters::storage::automation_activity::register_activity_session(
            state.storage(),
            id,
            &session_id,
            &job.actor_id,
            state.clock().now_unix_ms(),
        )
        .await
        .map_err(error)?;
        let principal = state.auth().scheduler_principal();
        let input=format!("[定时任务 {}]\n这是自动执行的后台任务，不是新的用户消息。不要在本轮创建重复计划。\n\n{}",job.id,required(cfg,"prompt")?);
        let origin = if cfg.get("origin").and_then(Value::as_str) == Some("heartbeat") {
            RunOrigin::Heartbeat
        } else {
            RunOrigin::Cron
        };
        let accepted = state
            .sessions()
            .execute_scheduled_for(
                state.storage(),
                state.events(),
                state.runs(),
                state.background(),
                &principal,
                &session_id,
                &input,
                id,
                origin,
                chat_for_job(state, cfg)?,
                None,
                state.clock().now_unix_ms(),
            )
            .await
            .map_err(error)?;
        let mut stopped = self.stop.subscribe();
        let timeout = if cfg.get("origin").and_then(Value::as_str) == Some("heartbeat") {
            crate::automation_heartbeat::HEARTBEAT_RUN_TIMEOUT_MS
        } else {
            CRON_TIMEOUT_MS
        };
        let original_permission = prefs.pointer("/automation/permissionMode").cloned();
        let terminal = async {
            store::bind_run(
                state.storage(),
                id,
                &session_id,
                &accepted.run_id,
                state.clock().now_unix_ms() as i64,
            )
            .await
            .map_err(error)?;
            self.publish_activity(state, id).await;
            loop {
                let run = state
                    .storage()
                    .load_run(&lingxi_protocol::RunId::new(&accepted.run_id))
                    .await
                    .map_err(error)?
                    .ok_or("scheduled run missing")?;
                if run.status.is_terminal() {
                    return Ok::<_, String>(run.status);
                }
                let current = store::get_job(state.storage(), &job.id)
                    .await
                    .map_err(error)?;
                let config_current = current
                    .as_ref()
                    .is_some_and(|j| j.config_revision == job.config_revision);
                let source_current = validate_source(state.storage(), &job.actor_id, cfg).await;
                let permissions = state.memory().read_preferences_root()?;
                let actor_deleted = state
                    .memory()
                    .is_agent_deleted_checked(&job.actor_id)
                    .map_err(error)?;
                if !config_current
                    || actor_deleted
                    || permissions.pointer("/automation/permissionMode").cloned()
                        != original_permission
                    || source_current
                        .as_ref()
                        .ok()
                        .and_then(|row| row.as_ref())
                        .map(source_authority)
                        != source.as_ref().map(source_authority)
                    || source_current.is_err()
                {
                    return Err("automation authority changed while running".into());
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        };
        let resolution = tokio::select! {
            result=terminal=>result,
            _=tokio::time::sleep(Duration::from_millis(timeout))=>Err("automation execution timed out".into()),
            _=stopped.wait_for(|value|*value)=>Err("automation host stopped".into()),
        };
        let (status, cleanup_reason) = match resolution {
            Ok(status) => (status, None),
            Err(reason) => {
                // 受理后的所有失败都先请求同一Run取消，并等待真实终态；不能只放弃等待。
                if let Err(cancel) = state
                    .sessions()
                    .cancel_run_for(
                        state.storage().as_ref(),
                        state.runs(),
                        &principal,
                        &accepted.run_id,
                    )
                    .await
                {
                    return Ok((
                        store::TriggerStatus::Unknown,
                        json!({"runId":accepted.run_id,"sessionId":session_id,"reason":reason,"cleanupError":format!("{cancel:?}")}),
                    ));
                }
                let cleanup = async {
                    loop {
                        let run = state
                            .storage()
                            .load_run(&lingxi_protocol::RunId::new(&accepted.run_id))
                            .await
                            .map_err(error)?
                            .ok_or("scheduled run missing during cleanup")?;
                        if run.status.is_terminal() {
                            return Ok::<_, String>(run.status);
                        }
                        tokio::time::sleep(Duration::from_millis(25)).await;
                    }
                };
                match tokio::time::timeout(Duration::from_secs(5), cleanup).await {
                    Ok(Ok(status)) => (status, Some(reason)),
                    outcome => {
                        return Ok((
                            store::TriggerStatus::Unknown,
                            json!({"runId":accepted.run_id,"sessionId":session_id,"reason":reason,"cleanupError":format!("{outcome:?}")}),
                        ))
                    }
                }
            }
        };
        self.publish_activity(state, id).await;
        let status = match status {
            lingxi_protocol::RunStatus::Completed => store::TriggerStatus::Succeeded,
            lingxi_protocol::RunStatus::Cancelled => store::TriggerStatus::Cancelled,
            _ => store::TriggerStatus::Failed,
        };
        Ok((
            status,
            json!({"runId":accepted.run_id,"sessionId":session_id,"cleanupReason":cleanup_reason}),
        ))
    }
}

fn failure(message: String) -> Response {
    let status = if message.contains("Conflict") || message.contains("revision") {
        StatusCode::CONFLICT
    } else {
        StatusCode::BAD_REQUEST
    };
    (status, Json(json!({"error":message}))).into_response()
}
pub async fn cron_get_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
) -> Response {
    if !principal.is_local_owner() {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"local_owner_required"})),
        )
            .into_response();
    }
    match list_views(&state).await {
        Ok(jobs) => Json(json!({"jobs":jobs,"importReport":state.automation().import_report()}))
            .into_response(),
        Err(e) => failure(e),
    }
}
async fn list_views(state: &ServiceState) -> Result<Vec<Value>, String> {
    visible_jobs(&JobApi::from_state(state)).await
}
async fn visible_jobs(api: &JobApi) -> Result<Vec<Value>, String> {
    store::list_jobs(&api.storage, Some(&api.studio_id))
        .await
        .map_err(error)?
        .iter()
        .filter(|job| config_of(job).map_or(true, |cfg| cfg["origin"] != "heartbeat"))
        .map(job_view)
        .collect()
}
pub async fn cron_post_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> Response {
    if !principal.is_local_owner() {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"local_owner_required"})),
        )
            .into_response();
    }
    match manage(&JobApi::from_state(&state), &principal, &body).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => failure(e),
    }
}
fn normalize_api_every(value: &mut Value) -> Result<(), String> {
    if value
        .get("scheduleType")
        .or_else(|| value.get("type"))
        .and_then(Value::as_str)
        != Some("every")
    {
        return Ok(());
    }
    let schedule = value.get("schedule").ok_or("schedule required")?;
    let millis = if let Some(text) = schedule.as_str() {
        let text = text
            .trim_start()
            .strip_prefix('+')
            .unwrap_or(text.trim_start());
        let digits = text
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>();
        let minutes = digits
            .parse::<u64>()
            .ok()
            .filter(|v| *v > 0)
            .ok_or("every schedule must be positive minutes")?;
        minutes
            .checked_mul(60_000)
            .ok_or("every schedule overflow")?
    } else {
        let millis = schedule
            .as_f64()
            .filter(|v| v.is_finite() && *v >= 60_000.0 && *v < u64::MAX as f64)
            .ok_or("every schedule must be at least 60000 milliseconds")?;
        millis.trunc() as u64
    };
    value["schedule"] = json!(millis);
    Ok(())
}

pub async fn manage(api: &JobApi, principal: &Principal, body: &Value) -> Result<Value, String> {
    if !principal.is_local_owner() {
        return Err("local_owner_required".into());
    }
    if body.get("action").and_then(Value::as_str) == Some("apply_suggestion") {
        let sid = required(body, "sessionId")?;
        let id = required(body, "suggestionId")?;
        if let Some(job) = lingxi_adapters::storage::automation_suggestions::applied_job(
            &api.storage,
            id,
            &api.studio_id,
            sid,
            lingxi_kernel::LOCAL_OWNER_SUBJECT,
        )
        .await
        .map_err(error)?
        {
            return Ok(json!({"ok":true,"job":job_view(&job)?}));
        }
        let suggestion = lingxi_adapters::storage::automation_suggestions::get(
            &api.storage,
            id,
            &api.studio_id,
            sid,
            lingxi_kernel::LOCAL_OWNER_SUBJECT,
            api.clock.now_unix_ms() as i64,
        )
        .await
        .map_err(error)?;
        let prior = match &suggestion.job_id {
            Some(id) => store::get_job(&api.storage, id).await.map_err(error)?,
            None => None,
        };
        if suggestion.job_id.is_some() && prior.is_none() {
            return Err("suggestion target no longer available".into());
        }
        let mut config: Value = serde_json::from_str(&suggestion.job.config_json).map_err(error)?;
        if let Some(edits) = body.get("jobData") {
            for (k, v) in edits.as_object().ok_or("jobData must be object")? {
                if k == "targetAgentId" {
                    if v.as_str() != Some(suggestion.job.actor_id.as_str()) {
                        return Err("suggestion target agent cannot change".into());
                    }
                    continue;
                }
                if ![
                    "type",
                    "scheduleType",
                    "schedule",
                    "prompt",
                    "label",
                    "model",
                    "enabled",
                ]
                .contains(&k.as_str())
                {
                    return Err("unsupported suggestion edit".into());
                }
                config[k] = v.clone();
            }
        }
        if body
            .get("jobData")
            .and_then(|v| v.get("schedule"))
            .is_some()
        {
            normalize_api_every(&mut config)?;
        }
        let mut prepared = prepare_job(api, principal, config, prior.as_ref()).await?;
        prepared.id = suggestion.job.id.clone();
        let job = lingxi_adapters::storage::automation_suggestions::apply(
            &api.storage,
            &suggestion,
            &prepared,
            api.clock.now_unix_ms() as i64,
        )
        .await
        .map_err(error)?;
        return Ok(json!({"ok":true,"job":job_view(&job)?}));
    }
    let action = required(body, "action")?;
    let job = if action == "add" {
        let mut data = body.clone();
        data.as_object_mut().unwrap().remove("action");
        normalize_api_every(&mut data)?;
        Some(save_job(api, principal, data, None).await?)
    } else {
        let id = required(body, "id")?;
        let prior = store::get_job(&api.storage, id)
            .await
            .map_err(error)?
            .ok_or("cron job not found")?;
        if prior.studio_id != api.studio_id {
            return Err("cron job studio mismatch".into());
        }
        if let Some(revision) = body
            .get("expectedConfigRevision")
            .or_else(|| body.get("configRevision"))
        {
            if revision.as_u64() != Some(prior.config_revision) {
                return Err("cron_job_revision_conflict".into());
            }
        }
        match action {
            "remove" => {
                store::delete_job(
                    &api.storage,
                    id,
                    prior.config_revision,
                    api.clock.now_unix_ms() as i64,
                )
                .await
                .map_err(error)?;
                None
            }
            "toggle" | "update" => {
                let mut cfg = config_of(&prior)?;
                if action == "toggle" {
                    cfg["enabled"] = json!(cfg.get("enabled") == Some(&Value::Bool(false)));
                } else {
                    for (k, v) in body.as_object().ok_or("job object required")? {
                        if !["action", "id", "expectedConfigRevision", "configRevision"]
                            .contains(&k.as_str())
                        {
                            cfg[k] = v.clone();
                        }
                    }
                }
                if body.get("schedule").is_some() {
                    normalize_api_every(&mut cfg)?;
                }
                Some(save_job(api, principal, cfg, Some(&prior)).await?)
            }
            "history" => {
                return Ok(
                    json!({"runs":store::list_trigger_history(&api.storage,Some(id),100).await.map_err(error)?}),
                )
            }
            _ => return Err("unknown_cron_action".into()),
        }
    };
    Ok(
        json!({"ok":true,"job":job.as_ref().map(job_view).transpose()?,"jobs":visible_jobs(api).await?}),
    )
}

pub async fn status_route(State(state): State<ServiceState>) -> Response {
    Json(state.automation().status()).into_response()
}
pub async fn history_route(
    State(state): State<ServiceState>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    match store::list_trigger_history(
        state.storage(),
        query
            .get("jobId")
            .or_else(|| query.get("id"))
            .map(String::as_str),
        100,
    )
    .await
    {
        Ok(runs) => Json(json!({"runs":runs})).into_response(),
        Err(e) => failure(error(e)),
    }
}

fn source_authority(
    row: &session_tree::SessionTreeRow,
) -> (&str, &str, &str, &[String], Option<&str>) {
    (
        &row.agent_id,
        &row.owner_user_id,
        &row.lifecycle,
        &row.authorized_folders,
        row.permission_mode.as_deref(),
    )
}

pub async fn notifications_get_route(State(state): State<ServiceState>) -> Response {
    match store::list_pending_notifications(state.storage()).await {
        Ok(deliveries) => Json(json!({"deliveries":deliveries})).into_response(),
        Err(e) => failure(error(e)),
    }
}
pub async fn notifications_post_route(
    State(state): State<ServiceState>,
    Json(body): Json<Value>,
) -> Response {
    let result = async {
        use lingxi_protocol::AutomationNotificationCommand;
        let command = serde_json::from_value::<AutomationNotificationCommand>(body)
            .map_err(|error| format!("invalid notification command: {error}"))?;
        command.validate().map_err(str::to_owned)?;
        let now = state.clock().now_unix_ms() as i64;
        match command {
            AutomationNotificationCommand::Claim { delivery_id } => Ok(json!({
                "claim": store::claim_notification(state.storage(), &delivery_id, now).await.map_err(error)?
            })),
            AutomationNotificationCommand::Ack { delivery_id, platform_receipt } => {
                let receipt = serde_json::to_value(platform_receipt).map_err(error)?;
                let row = store::settle_notification(state.storage(), &delivery_id,
                    store::NotificationStatus::Confirmed, Some(receipt), None, now).await.map_err(error)?;
                Ok(json!({"ok":true,"delivery":row}))
            }
            AutomationNotificationCommand::Failed { delivery_id, error: detail } => Ok(json!({
                "ok":true,"delivery":store::settle_notification(state.storage(), &delivery_id,
                    store::NotificationStatus::Failed, None, Some(detail), now).await.map_err(error)?
            })),
            AutomationNotificationCommand::Unknown { delivery_id, error: detail } => Ok(json!({
                "ok":true,"delivery":store::settle_notification(state.storage(), &delivery_id,
                    store::NotificationStatus::Unknown, None, Some(detail), now).await.map_err(error)?
            })),
        }
    }.await;
    match result {
        Ok(value) => Json(value).into_response(),
        Err(err) => failure(err),
    }
}

pub async fn heartbeat_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<std::collections::HashMap<String, String>>,
    body: Option<Json<Value>>,
) -> Response {
    if !principal.is_local_owner() {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"local_owner_required"})),
        )
            .into_response();
    }
    let body = body.map(|body| body.0).unwrap_or(Value::Null);
    let agent = query
        .get("agentId")
        .map(String::as_str)
        .or_else(|| body["agentId"].as_str())
        .filter(|agent| !agent.trim().is_empty())
        .ok_or_else(|| "agentId required".to_string());
    let result = match agent {
        Ok(agent) => crate::automation_patrol::trigger_now(&state, state.automation(), agent).await,
        Err(error) => Err(error),
    };
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => failure(error),
    }
}

/// 现役活动页的总开关写入；其他设置由各自管理接口处理。
pub async fn preferences_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Json(patch): Json<Value>,
) -> Response {
    if !principal.is_local_owner() {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"local_owner_required"})),
        )
            .into_response();
    }
    let result = (|| -> Result<(), String> {
        let fields = patch
            .as_object()
            .ok_or("configuration patch must be object")?;
        if fields.is_empty() {
            return Err("configuration patch required".into());
        }
        let _lock = crate::agents_admin::preferences_locked();
        let mut prefs = state.memory().read_preferences_root()?;
        for (key, value) in fields {
            match key.as_str() {
                "desk" => {
                    let desk = value.as_object().ok_or("desk must be object")?;
                    if desk.len() != 1 || !desk.contains_key("heartbeat_master") {
                        return Err("unsupported automation preference".into());
                    }
                    let enabled = desk["heartbeat_master"]
                        .as_bool()
                        .ok_or("heartbeat_master must be boolean")?;
                    prefs["heartbeat_master"] = json!(enabled);
                }
                "automation" => {
                    let fields = value.as_object().ok_or("automation must be object")?;
                    if fields.len() != 1 || !fields.contains_key("permissionMode") {
                        return Err("unsupported automation preference".into());
                    }
                    let mode = fields["permissionMode"]
                        .as_str()
                        .ok_or("permissionMode must be string")?;
                    if !["auto", "operate", "read_only"].contains(&mode) {
                        return Err("invalid automation permission mode".into());
                    }
                    if prefs.get("automation").is_some_and(|v| !v.is_object()) {
                        return Err("saved automation preferences must be object".into());
                    }
                    if prefs.get("automation").is_none() {
                        prefs["automation"] = json!({});
                    }
                    prefs["automation"]["permissionMode"] = json!(mode);
                }
                _ => return Err("unsupported automation preference".into()),
            }
        }
        state.memory().write_preferences_root(&prefs)
    })();
    match result {
        Ok(()) => {
            state
                .app_events()
                .publish("config_changed", json!({"automation":true}));
            Json(json!({"ok":true})).into_response()
        }
        Err(error) => failure(error),
    }
}

async fn queue_completion(state: &ServiceState, activity: &Value) -> Result<(), String> {
    let status = activity["status"].as_str().unwrap_or("");
    if !["done", "error", "cancelled"].contains(&status) {
        return Ok(());
    }
    let prefs = state.memory().read_preferences_root()?;
    let kind = required(activity, "type")?;
    let preference = if kind == "heartbeat" {
        "patrolCompletion"
    } else {
        "scheduledTaskCompletion"
    };
    let notification_config = notification_preferences(&prefs)?;
    let mode = notification_config[preference]
        .as_str()
        .expect("通知偏好已验证");
    if !["always", "when_unfocused"].contains(&mode) {
        return Ok(());
    }
    let trigger = required(activity, "triggerId")?;
    let run = required(activity, "runId")?;
    let delivery = format!(
        "completion-{}",
        lingxi_kernel::session_tree::sha256_hex(trigger.as_bytes())
    );
    if let Some(existing) = store::get_notification(state.storage(), &delivery)
        .await
        .map_err(error)?
    {
        if existing.trigger_id.as_deref() != Some(trigger)
            || existing.run_id.as_deref() != Some(run)
        {
            return Err("completion notification identity mismatch".into());
        }
        return Ok(());
    }
    let locale = prefs["locale"].as_str().unwrap_or("zh");
    let failed = status != "done";
    let prefix = match (locale, kind == "heartbeat", failed) {
        ("zh-TW", false, false) => "排程任務已完成：",
        ("zh-TW", false, true) => "排程任務執行失敗或停止：",
        ("zh-TW", true, false) => "巡檢已完成：",
        ("zh-TW", true, true) => "巡檢執行失敗或停止：",
        ("ja", false, false) => "スケジュールタスクが完了しました：",
        ("ja", false, true) => "スケジュールタスクに失敗したか停止しました：",
        ("ja", true, false) => "巡回が完了しました：",
        ("ja", true, true) => "巡回に失敗したか停止しました：",
        ("ko", false, false) => "예약 작업이 완료됐어요: ",
        ("ko", false, true) => "예약 작업이 실패하거나 중지됐어요: ",
        ("ko", true, false) => "순찰이 완료됐어요: ",
        ("ko", true, true) => "순찰이 실패하거나 중지됐어요: ",
        (value, false, false) if value.starts_with("zh") => "计划任务已完成：",
        (value, false, true) if value.starts_with("zh") => "计划任务执行失败或停止：",
        (value, true, false) if value.starts_with("zh") => "巡检已完成：",
        (value, true, true) if value.starts_with("zh") => "巡检执行失败或停止：",
        (_, false, false) => "Scheduled task completed: ",
        (_, false, true) => "Scheduled task failed or stopped: ",
        (_, true, false) => "Patrol completed: ",
        (_, true, true) => "Patrol failed or stopped: ",
    };
    let label = activity["label"]
        .as_str()
        .or_else(|| activity["summary"].as_str())
        .unwrap_or(kind);
    let payload = json!({"title":activity["agentName"].as_str().unwrap_or("Lingxi"),"body":format!("{prefix}{label}"),"agentId":activity["agentId"],"sessionId":activity["sessionId"],"sessionPath":activity["sessionPath"],"runId":run,"desktopFocusPolicy":mode});
    store::reserve_notification(
        state.storage(),
        &store::NotificationInput {
            delivery_id: delivery,
            trigger_id: Some(trigger.into()),
            run_id: Some(run.into()),
            channel: "desktop".into(),
            recipient: lingxi_kernel::LOCAL_OWNER_SUBJECT.into(),
            payload_json: payload.to_string(),
        },
        state.clock().now_unix_ms() as i64,
    )
    .await
    .map_err(error)?;
    Ok(())
}

fn notification_mode(value: Option<&Value>) -> Result<&str, String> {
    match value {
        None => Ok("never"),
        Some(value) => value
            .as_str()
            .ok_or_else(|| "notification preference mode must be string".into()),
    }
}
/// PUT patch 白名单：只允许这三类完成通知偏好的已知键（turnCompletion 为旧客户端
/// 别名）；未知键不落盘（400），避免把任意 renderer 字段写进 preferences.json。
const NOTIFICATION_PATCH_KEYS: [&str; 4] = [
    "chatCompletion",
    "turnCompletion",
    "scheduledTaskCompletion",
    "patrolCompletion",
];
const CHAT_COMPLETION_MODES: [&str; 3] = ["never", "when_unfocused", "when_session_unfocused"];
const BACKGROUND_COMPLETION_MODES: [&str; 3] = ["never", "when_unfocused", "always"];

/// 校验 patch 自身：键必须在白名单内、值必须是字符串且属于对应枚举。
/// 在加锁/读盘/写入之前调用，保证非法 patch 不产生任何持久化副作用。
fn validate_notification_patch(patch: &serde_json::Map<String, Value>) -> Result<(), String> {
    for (key, value) in patch {
        if !NOTIFICATION_PATCH_KEYS.contains(&key.as_str()) {
            return Err(format!("unknown notification preference key: {key}"));
        }
        let allowed: &[&str] = match key.as_str() {
            "chatCompletion" | "turnCompletion" => &CHAT_COMPLETION_MODES,
            _ => &BACKGROUND_COMPLETION_MODES,
        };
        let mode = value
            .as_str()
            .ok_or_else(|| format!("notification preference {key} must be string"))?;
        if !allowed.contains(&mode) {
            return Err(format!("notification preference {key} is invalid"));
        }
    }
    Ok(())
}
fn notification_preferences(prefs: &Value) -> Result<Value, String> {
    let source = prefs.get("notifications").unwrap_or(&Value::Null);
    if !source.is_null() && !source.is_object() {
        return Err("saved notification preferences must be object".into());
    }
    let chat = notification_mode(
        source
            .get("chatCompletion")
            .or_else(|| source.get("turnCompletion")),
    )?;
    let cron = notification_mode(source.get("scheduledTaskCompletion"))?;
    let patrol = notification_mode(source.get("patrolCompletion"))?;
    if !["never", "when_unfocused", "when_session_unfocused"].contains(&chat)
        || !["never", "when_unfocused", "always"].contains(&cron)
        || !["never", "when_unfocused", "always"].contains(&patrol)
    {
        return Err("saved notification preference mode is invalid".into());
    }
    Ok(json!({"chatCompletion":chat,"scheduledTaskCompletion":cron,"patrolCompletion":patrol}))
}
pub async fn notification_preferences_get_route(State(state): State<ServiceState>) -> Response {
    match state
        .memory()
        .read_preferences_root()
        .and_then(|prefs| notification_preferences(&prefs))
    {
        Ok(notifications) => Json(json!({"notifications":notifications})).into_response(),
        Err(error) => failure(error),
    }
}
pub async fn notification_preferences_put_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> Response {
    if !principal.is_local_owner() {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"local_owner_required"})),
        )
            .into_response();
    }
    let result = (|| -> Result<Value, String> {
        let patch = body
            .get("notifications")
            .unwrap_or(&body)
            .as_object()
            .ok_or("notification preference patch must be object")?;
        // 先只读校验 patch：未知键/非字符串/越界枚举一律 400，且不触碰任何存储。
        validate_notification_patch(patch)?;
        let _lock = crate::agents_admin::preferences_locked();
        let mut prefs = state.memory().read_preferences_root()?;
        notification_preferences(&prefs)?;
        if !prefs.get("notifications").is_some_and(Value::is_object) {
            prefs["notifications"] = json!({});
        }
        for (key, value) in patch {
            let key = if key == "turnCompletion" && !patch.contains_key("chatCompletion") {
                "chatCompletion"
            } else {
                key
            };
            if key == "turnCompletion" {
                continue;
            }
            // 只合并已知键；preferences.notifications 里既有的未知/历史字段保持不动。
            prefs["notifications"][key] = value.clone();
        }
        let normalized = notification_preferences(&prefs)?;
        state.memory().write_preferences_root(&prefs)?;
        Ok(normalized)
    })();
    match result {
        Ok(notifications) => Json(json!({"ok":true,"notifications":notifications})).into_response(),
        Err(error) => failure(error),
    }
}

#[cfg(test)]
mod shutdown_tests {
    use super::*;

    #[tokio::test]
    async fn shutdown_timeout_keeps_pending_task_and_prior_error_for_all_waiters() {
        let host = Arc::new(AutomationHost::default());
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let completed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let permit = release.clone();
        let finished = completed.clone();
        let pending = tokio::spawn(async move {
            permit.acquire().await.unwrap().forget();
            finished.store(true, std::sync::atomic::Ordering::Release);
            Ok(())
        });
        let failed = tokio::spawn(async { Err("先退出的lane确实失败".to_string()) });
        while !failed.is_finished() {
            tokio::task::yield_now().await;
        }
        host.tasks.lock().unwrap().extend([pending, failed]);
        host.stop();
        assert!(
            tokio::time::timeout(Duration::from_millis(10), host.wait_stopped())
                .await
                .is_err()
        );
        assert_eq!(
            host.tasks.lock().unwrap().len(),
            1,
            "超时后仍须登记未退出的真实任务"
        );
        assert_eq!(
            host.shutdown_failures.lock().unwrap().len(),
            1,
            "已取得的失败也不能随等待future丢失"
        );
        let first_host = host.clone();
        let mut first = tokio::spawn(async move { first_host.wait_stopped().await });
        let second_host = host.clone();
        let mut second = tokio::spawn(async move { second_host.wait_stopped().await });
        assert!(tokio::time::timeout(Duration::from_millis(10), &mut first)
            .await
            .is_err());
        assert!(tokio::time::timeout(Duration::from_millis(10), &mut second)
            .await
            .is_err());
        assert!(!completed.load(std::sync::atomic::Ordering::Acquire));
        release.add_permits(1);
        for waiter in [first, second] {
            let error = tokio::time::timeout(Duration::from_secs(1), waiter)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err();
            assert!(error.contains("先退出的lane确实失败"), "{error}");
        }
        assert!(completed.load(std::sync::atomic::Ordering::Acquire));
        assert!(host.tasks.lock().unwrap().is_empty());
    }
}

#[cfg(test)]
mod notification_patch_tests {
    use super::*;
    use serde_json::Map;

    fn patch(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap_or_default()
    }

    #[test]
    fn notification_patch_whitelist_rejects_unknown_keys_and_invalid_values() {
        assert!(validate_notification_patch(&patch(json!({
            "chatCompletion": "when_unfocused",
            "turnCompletion": "when_session_unfocused",
            "scheduledTaskCompletion": "always",
            "patrolCompletion": "never",
        })))
        .is_ok());
        // 未知键（宿主侧通知载荷字段不是 server 偏好键）。
        assert_eq!(
            validate_notification_patch(&patch(json!({"desktopFocusPolicy": "when_unfocused"})))
                .unwrap_err(),
            "unknown notification preference key: desktopFocusPolicy"
        );
        // 类型错误。
        assert_eq!(
            validate_notification_patch(&patch(json!({"scheduledTaskCompletion": true})))
                .unwrap_err(),
            "notification preference scheduledTaskCompletion must be string"
        );
        // 枚举越界。
        assert_eq!(
            validate_notification_patch(&patch(json!({"patrolCompletion": "invented"})))
                .unwrap_err(),
            "notification preference patrolCompletion is invalid"
        );
        // 空 patch 合法（不写任何键）。
        assert!(validate_notification_patch(&Map::new()).is_ok());
    }
}
