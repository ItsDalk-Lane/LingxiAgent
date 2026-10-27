//! 心跳宿主只编排真实材料、持久触发与既有后台Run；日志目标不能由模型指定。
use crate::{
    automation::AutomationHost,
    automation_heartbeat as plan,
    resourceio::{OpContext, ResourceIoService},
    ServiceState,
};
use lingxi_adapters::storage::{automation as store, session_tree, RunDatabase};
use lingxi_kernel::{
    ports::{
        StoragePort, ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, ToolSuccess,
    },
    toolcatalog::{
        Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
        ToolManifest, ToolOrigin,
    },
    RunContext,
};
use lingxi_protocol::{ErrorCode, ProtocolError, ToolCallId, ToolSchemaDocument};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
    },
    time::Duration,
};

const MAX_ACTIVE_TARGETS: usize = 512;

#[derive(Default)]
struct AgentState {
    running: bool,
    next_slot: Option<u64>,
    last_manual: Option<u64>,
    blocked_run: Option<String>,
    last_error: Option<String>,
    interval_ms: u64,
}
#[derive(Clone)]
struct LogBinding {
    agent_id: String,
    source_session_id: String,
    trigger_id: String,
    target: plan::HeartbeatLogTarget,
    config_hash: String,
    automatic: bool,
    execution_returned: bool,
}
pub struct PatrolHost {
    agents: Mutex<BTreeMap<String, AgentState>>,
    targets: Mutex<BTreeMap<String, LogBinding>>,
    stopped: AtomicBool,
    wake: tokio::sync::Notify,
    coordinator: Mutex<Option<tokio::task::JoinHandle<()>>>,
    beats: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    shutdown_wait: tokio::sync::Mutex<()>,
    shutdown_failures: Mutex<Vec<String>>,
}
impl Default for PatrolHost {
    fn default() -> Self {
        Self {
            agents: Mutex::new(BTreeMap::new()),
            targets: Mutex::new(BTreeMap::new()),
            stopped: AtomicBool::new(false),
            wake: tokio::sync::Notify::new(),
            coordinator: Mutex::new(None),
            beats: Mutex::new(Vec::new()),
            shutdown_wait: tokio::sync::Mutex::new(()),
            shutdown_failures: Mutex::new(Vec::new()),
        }
    }
}

fn hash(bytes: &[u8]) -> String {
    lingxi_kernel::session_tree::sha256_hex(bytes)
}
fn reference(path: &Path) -> Value {
    json!({"kind":"local-file","path":path})
}
fn operation(session: &str) -> OpContext {
    let mut op = OpContext::local_owner(session);
    op.source = "heartbeat".into();
    op.reason = Some("心跳巡检受权材料与固定日志".into());
    op
}
fn config(
    memory: &crate::memory::MemoryService,
    agent: &str,
) -> Result<(Value, String, PathBuf), String> {
    if memory
        .is_agent_deleted_checked(agent)
        .map_err(|e| e.to_string())?
    {
        return Err("heartbeat agent deleted".into());
    }
    let dir = memory.agent_dir_checked(agent).map_err(|e| e.to_string())?;
    let value = lingxi_adapters::storage::agent_memory::config::read_yaml_mapping_as_json(
        &dir.join("config.yaml"),
    )
    .map_err(|e| e.to_string())?
    .ok_or("heartbeat agent config missing")?;
    let rendered = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
    Ok((value, hash(&rendered), dir))
}

impl PatrolHost {
    pub fn start(self: &Arc<Self>, state: ServiceState) -> bool {
        let Ok(_waiting) = self.shutdown_wait.try_lock() else {
            return false;
        };
        let mut slot = self.coordinator.lock().expect("心跳宿主锁");
        if slot.is_some() || !self.beats.lock().expect("心跳运行登记锁").is_empty() {
            return false;
        }
        self.shutdown_failures
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        self.stopped.store(false, Ordering::Release);
        let host = self.clone();
        *slot = Some(tokio::spawn(async move {
            let mut ticks = tokio::time::interval(Duration::from_secs(1));
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _=host.wake.notified()=>{if host.stopped.load(Ordering::Acquire){break;}},
                    _=ticks.tick()=>{
                        if host.stopped.load(Ordering::Acquire)||state.submission_intake().is_closed(){break;}
                        if let Err(error)=host.scan(&state).await {tracing::error!(%error,"心跳调度检查失败");}
                    }
                }
            }
        }));
        true
    }
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        self.wake.notify_waiters();
    }
    pub async fn wait_stopped(&self) -> Result<(), String> {
        let _waiting = self.shutdown_wait.lock().await;
        // 每次poll只借用登记中的句柄；取消等待时原句柄与未收完的错误仍留在宿主内。
        let coordinator = std::future::poll_fn(|cx| {
            let mut task = match self.coordinator.lock() {
                Ok(task) => task,
                Err(poisoned) => {
                    let mut failures = self
                        .shutdown_failures
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let detail = "heartbeat coordinator lock poisoned".to_string();
                    if !failures.contains(&detail) {
                        failures.push(detail);
                    }
                    poisoned.into_inner()
                }
            };
            let Some(handle) = task.as_mut() else {
                return std::task::Poll::Ready(None);
            };
            match std::future::Future::poll(std::pin::Pin::new(handle), cx) {
                std::task::Poll::Pending => std::task::Poll::Pending,
                std::task::Poll::Ready(outcome) => {
                    task.take();
                    std::task::Poll::Ready(Some(outcome))
                }
            }
        })
        .await;
        if let Some(Err(error)) = coordinator {
            self.shutdown_failures
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(format!("heartbeat coordinator: {error}"));
        }
        loop {
            let outcome = std::future::poll_fn(|cx| {
                let mut beats = match self.beats.lock() {
                    Ok(beats) => beats,
                    Err(poisoned) => {
                        let mut failures = self
                            .shutdown_failures
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        let detail = "heartbeat beat lock poisoned".to_string();
                        if !failures.contains(&detail) {
                            failures.push(detail);
                        }
                        poisoned.into_inner()
                    }
                };
                let Some(handle) = beats.last_mut() else {
                    return std::task::Poll::Ready(None);
                };
                match std::future::Future::poll(std::pin::Pin::new(handle), cx) {
                    std::task::Poll::Pending => std::task::Poll::Pending,
                    std::task::Poll::Ready(outcome) => {
                        beats.pop();
                        std::task::Poll::Ready(Some(outcome))
                    }
                }
            })
            .await;
            match outcome {
                Some(Ok(())) => {}
                Some(Err(error)) => self
                    .shutdown_failures
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(format!("heartbeat beat: {error}")),
                None => break,
            }
        }
        let errors = self
            .shutdown_failures
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
    pub fn status(&self, agent: &str) -> Value {
        let agents = self.agents.lock().expect("心跳状态锁");
        match agents.get(agent) {
            Some(value) => {
                json!({"running":value.running,"nextRunAt":value.next_slot,"lastManualAt":value.last_manual,"blockedRunId":value.blocked_run,"lastError":value.last_error})
            }
            None => json!({"running":false,"nextRunAt":null,"lastError":null}),
        }
    }
    async fn scan(self: &Arc<Self>, state: &ServiceState) -> Result<(), String> {
        // 即使结算回执写入失败，也只在真实运行退出后回收固定工具目标。
        self.reap_targets(state).await;
        let live = state
            .memory()
            .ordered_live_agents()
            .map_err(|e| e.to_string())?;
        let prefs = state.memory().read_preferences_root()?;
        let now = state.clock().now_unix_ms();
        for (agent, _) in &live {
            let (cfg, _, _) = match config(state.memory(), agent) {
                Ok(v) => v,
                Err(error) => {
                    self.agents
                        .lock()
                        .unwrap()
                        .entry(agent.clone())
                        .or_default()
                        .last_error = Some(error);
                    continue;
                }
            };
            let settings = match plan::HeartbeatSettings::from_config(&cfg, &prefs) {
                Ok(settings) => settings,
                Err(error) => {
                    self.agents
                        .lock()
                        .unwrap()
                        .entry(agent.clone())
                        .or_default()
                        .last_error = Some(error.to_string());
                    continue;
                }
            };
            let blocked = self
                .agents
                .lock()
                .unwrap()
                .get(agent)
                .and_then(|a| a.blocked_run.clone());
            if let Some(run) = blocked {
                if state
                    .storage()
                    .load_run(&lingxi_protocol::RunId::new(&run))
                    .await
                    .map_err(|e| e.to_string())?
                    .is_some_and(|r| r.status.is_terminal())
                {
                    self.agents
                        .lock()
                        .unwrap()
                        .entry(agent.clone())
                        .or_default()
                        .blocked_run = None;
                    self.targets
                        .lock()
                        .unwrap()
                        .retain(|_, b| &b.agent_id != agent);
                }
            }
            let due = {
                let mut agents = self.agents.lock().unwrap();
                let current = agents.entry(agent.clone()).or_default();
                if current.interval_ms != settings.interval_ms {
                    current.interval_ms = settings.interval_ms;
                    current.next_slot = None;
                }
                if !settings.enabled {
                    current.next_slot = None;
                    false
                } else {
                    let next = current.next_slot.get_or_insert(
                        plan::next_heartbeat_slot_ms(now, settings.interval_ms)
                            .map_err(|e| e.to_string())?,
                    );
                    if now >= *next {
                        *next = plan::next_heartbeat_slot_ms(now, settings.interval_ms)
                            .map_err(|e| e.to_string())?;
                        !current.running && current.blocked_run.is_none()
                    } else {
                        false
                    }
                }
            };
            if due {
                self.launch(
                    state.clone(),
                    state.automation().clone(),
                    agent.clone(),
                    true,
                    now,
                )?;
            }
        }
        self.agents.lock().unwrap().retain(|agent, current| {
            current.running
                || current.blocked_run.is_some()
                || live.iter().any(|(id, _)| id == agent)
        });
        let finished = {
            let mut tasks = self.beats.lock().unwrap();
            let mut done = Vec::new();
            let mut i = 0;
            while i < tasks.len() {
                if tasks[i].is_finished() {
                    done.push(tasks.swap_remove(i));
                } else {
                    i += 1;
                }
            }
            done
        };
        for task in finished {
            task.await.map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    fn launch(
        self: &Arc<Self>,
        state: ServiceState,
        automation: Arc<AutomationHost>,
        agent: String,
        automatic: bool,
        now: u64,
    ) -> Result<bool, String> {
        let _waiting = self
            .shutdown_wait
            .try_lock()
            .map_err(|_| "heartbeat host is waiting for shutdown")?;
        if self.stopped.load(Ordering::Acquire) || state.submission_intake().is_closed() {
            return Err("heartbeat host stopped".into());
        }
        if self
            .targets
            .lock()
            .unwrap()
            .values()
            .any(|binding| binding.agent_id == agent)
        {
            return Ok(false);
        }
        {
            let mut agents = self.agents.lock().unwrap();
            let current = agents.entry(agent.clone()).or_default();
            if current.running || current.blocked_run.is_some() {
                return Ok(false);
            }
            if !automatic {
                let cooldown = plan::manual_heartbeat_delay_ms(now, current.last_manual);
                if cooldown > 0 {
                    return Err(format!("heartbeat cooldown: {cooldown}ms"));
                }
                current.last_manual = Some(now);
            }
            current.running = true;
        }
        let host = self.clone();
        // 与退出收集共用登记锁；停机边界上不能留下尚未登记的beat。
        let mut beats = self.beats.lock().unwrap();
        if self.stopped.load(Ordering::Acquire) || state.submission_intake().is_closed() {
            self.agents
                .lock()
                .unwrap()
                .entry(agent)
                .or_default()
                .running = false;
            return Err("heartbeat host stopped".into());
        }
        let task = tokio::spawn(async move {
            let result = host.beat(&state, &automation, &agent, automatic, now).await;
            let mut agents = host.agents.lock().unwrap();
            let current = agents.entry(agent).or_default();
            current.running = false;
            current.last_error = result.err();
        });
        beats.push(task);
        Ok(true)
    }
    pub async fn trigger_now_for(
        self: &Arc<Self>,
        state: &ServiceState,
        automation: &Arc<AutomationHost>,
        agent: &str,
    ) -> Result<Value, String> {
        // 手动入口保留现役调试含义：不等自动槽，但仍受运行锁、冷却、配置与资源闸约束。
        config(state.memory(), agent)?;
        let started = self.launch(
            state.clone(),
            automation.clone(),
            agent.into(),
            false,
            state.clock().now_unix_ms(),
        )?;
        Ok(json!({"ok":true,"started":started,"agentId":agent}))
    }
    async fn beat(
        self: &Arc<Self>,
        state: &ServiceState,
        automation: &Arc<AutomationHost>,
        agent: &str,
        automatic: bool,
        now: u64,
    ) -> Result<(), String> {
        let (cfg, config_hash, agent_dir) = config(state.memory(), agent)?;
        let prefs = state.memory().read_preferences_root()?;
        let settings =
            plan::HeartbeatSettings::from_config(&cfg, &prefs).map_err(|e| e.to_string())?;
        if automatic && !settings.enabled {
            return Err("heartbeat disabled".into());
        }
        let workspace = match cfg.pointer("/desk/home_folder") {
            Some(Value::String(path)) if !path.trim().is_empty() => PathBuf::from(path),
            Some(Value::Null) | None => state
                .resource_io()
                .workspace_root()
                .ok_or("heartbeat workspace unavailable")?,
            _ => return Err("heartbeat workspace config is invalid".into()),
        };
        if !workspace.is_absolute() {
            return Err("heartbeat workspace must be absolute".into());
        }
        let workspace = crate::resourceaccess::resolve_real(&workspace)
            .ok_or("heartbeat workspace cannot be resolved")?;
        let scope = self
            .scope_session(state, agent, &workspace, "workspace")
            .await?;
        let op = operation(&scope);
        let root = state
            .resource_io()
            .stat(&reference(&workspace), &op)
            .await
            .map_err(|e| e.to_string())?;
        if root["exists"] != true || root["isDirectory"] != true {
            return Err("heartbeat workspace is not a readable directory".into());
        }
        let workspace = PathBuf::from(
            root["filePath"]
                .as_str()
                .ok_or("heartbeat resource root missing")?,
        );
        let agent_name = cfg
            .pointer("/agent/name")
            .and_then(Value::as_str)
            .unwrap_or(agent);
        let locale = cfg["locale"].as_str().unwrap_or("");
        let (previous, base_job) = self.previous_snapshot(state, agent).await?;
        let mut materials = self.materials(state, &workspace, &op).await?;
        let desk = agent_dir.join("desk");
        // 服务自己的注意力清单使用独立收集会话，绝不把desk目录授权给模型执行会话。
        // 这里只核服务自有目录是否存在；清单正文仍由受权ResourceIO读取。
        let desk_exists = match std::fs::symlink_metadata(&desk) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => true,
            Ok(_) => return Err("heartbeat desk is not a real directory".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(format!("heartbeat desk metadata: {error}")),
        };
        if desk_exists {
            let collector = self.scope_session(state, agent, &desk, "overwatch").await?;
            materials.overwatch = read_optional(
                state.resource_io(),
                &desk.join("overwatch.md"),
                &operation(&collector),
            )
            .await?
            .map(|(text, _)| text);
        }
        let (_, log) = plan::output_paths(agent_name, locale);
        materials.patrol_log = read_optional(state.resource_io(), &workspace.join(log), &op)
            .await?
            .map(|(text, _)| text);
        let local = chrono::DateTime::from_timestamp_millis(
            i64::try_from(now).map_err(|_| "heartbeat clock overflow")?,
        )
        .ok_or("heartbeat clock out of range")?
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M")
        .to_string();
        let prepared =
            plan::build_heartbeat_plan(agent_name, locale, &local, &materials, &previous)
                .map_err(|e| e.to_string())?;
        let mut updated = previous;
        updated.desk_files = prepared.desk_snapshot.clone();
        let mut invocations = vec![prepared.patrol];
        invocations.extend(prepared.jian);
        for invocation in invocations {
            self.check_current(state, agent, &config_hash, automatic)?;
            let relative = invocation
                .cwd
                .strip_prefix(&workspace)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .into_owned();
            let phase = if invocation.kind == plan::HeartbeatKind::Patrol {
                "patrol".to_owned()
            } else {
                format!("jian-{relative}")
            };
            let job_id = format!(
                "heartbeat-{}-{}",
                &hash(agent.as_bytes())[..20],
                &hash(phase.as_bytes())[..20]
            );
            let context =
                json!({"sourceSessionId":scope,"createdByAgentId":agent,"cwd":invocation.cwd});
            let job_cfg = json!({"schemaVersion":4,"origin":"heartbeat","type":"every","schedule":settings.interval_ms,"enabled":true,"actorAgentId":agent,"prompt":invocation.prompt,"executionContext":context,"label":phase,"heartbeatSnapshot":updated});
            let job = self
                .save_job(state, agent, &job_id, job_cfg.clone())
                .await?;
            let trigger_id = crate::automation::trigger_id(
                &job.studio_id,
                &job.id,
                job.config_revision,
                now as i64,
            );
            let claim = store::claim_trigger(
                state.storage(),
                &store::TriggerInput {
                    trigger_id: trigger_id.clone(),
                    job_id: job.id.clone(),
                    due_at: now as i64,
                    config_revision: job.config_revision,
                },
                state.clock().now_unix_ms() as i64,
            )
            .await
            .map_err(|e| e.to_string())?;
            if !claim.created {
                return Err("heartbeat trigger already exists; not replayed".into());
            }
            let session = format!("activity-{trigger_id}");
            let bound = {
                let mut targets = self.targets.lock().unwrap();
                if targets.len() >= MAX_ACTIVE_TARGETS {
                    false
                } else {
                    targets.insert(
                        session.clone(),
                        LogBinding {
                            agent_id: agent.into(),
                            source_session_id: scope.clone(),
                            trigger_id: trigger_id.clone(),
                            target: invocation.log_target.clone(),
                            config_hash: config_hash.clone(),
                            automatic,
                            execution_returned: false,
                        },
                    );
                    true
                }
            };
            if !bound {
                store::settle_trigger(
                    state.storage(),
                    &trigger_id,
                    store::TriggerStatus::Skipped,
                    None,
                    Some("heartbeat log target capacity exhausted".into()),
                    None,
                    state.clock().now_unix_ms() as i64,
                )
                .await
                .map_err(|e| e.to_string())?;
                return Err("heartbeat log target capacity exhausted".into());
            }
            let execution = automation.execute_job(state, &job, &job_cfg, &trigger_id);
            tokio::pin!(execution);
            let mut ticks = tokio::time::interval(Duration::from_millis(100));
            let result = loop {
                tokio::select! {
                    result=&mut execution=>break result,
                    _=ticks.tick()=>{
                        if self.check_current(state,agent,&config_hash,automatic).is_err() {
                            match store::get_trigger_run(state.storage(),&trigger_id).await {
                                Ok(Some((run,_,_)))=>{
                                    let principal=state.auth().scheduler_principal();
                                    if let Err(error)=state.sessions().cancel_run_for(state.storage().as_ref(),state.runs(),&principal,&run).await {tracing::error!(?error,"撤回心跳后取消运行失败");}
                                }
                                Ok(None)=>{},
                                Err(error)=>tracing::error!(?error,"撤回心跳时查询运行失败，继续等原执行退出"),
                            }
                        }
                    }
                }
            };
            if let Some(binding) = self.targets.lock().unwrap().get_mut(&session) {
                binding.execution_returned = true;
            }
            let (status, value, error) = match result {
                Ok((status, value)) => (status, Some(value), None),
                Err(error) => (store::TriggerStatus::Unknown, None, Some(error)),
            };
            store::settle_trigger(
                state.storage(),
                &trigger_id,
                status,
                value,
                error,
                None,
                state.clock().now_unix_ms() as i64,
            )
            .await
            .map_err(|e| e.to_string())?;
            let actual = store::get_trigger_run(state.storage(), &trigger_id)
                .await
                .map_err(|e| e.to_string())?;
            if let Some((run, _, status)) = &actual {
                if ![
                    "completed",
                    "failed",
                    "cancelled",
                    "interrupted_needs_attention",
                ]
                .contains(&status.as_str())
                {
                    let principal = state.auth().scheduler_principal();
                    if let Err(error) = state
                        .sessions()
                        .cancel_run_for(state.storage().as_ref(), state.runs(), &principal, run)
                        .await
                    {
                        tracing::error!(?error, "心跳执行返回后仍有活运行，取消尚未确认");
                    }
                    self.agents
                        .lock()
                        .unwrap()
                        .entry(agent.into())
                        .or_default()
                        .blocked_run = Some(run.clone());
                    return Err("heartbeat run exit unconfirmed".into());
                }
            }
            self.targets.lock().unwrap().remove(&session);
            if status != store::TriggerStatus::Succeeded {
                return Err("heartbeat phase failed or cancelled".into());
            }
            if invocation.kind == plan::HeartbeatKind::Jian {
                let content =
                    read_optional(state.resource_io(), &invocation.cwd.join("jian.md"), &op)
                        .await?
                        .ok_or("jian disappeared after execution")?
                        .0;
                let listing = state
                    .resource_io()
                    .list(&reference(&invocation.cwd), &op)
                    .await
                    .map_err(|e| e.to_string())?;
                let entries =
                    plan::files_from_resource_listing(&listing).map_err(|e| e.to_string())?;
                updated.jian.insert(
                    if relative.is_empty() {
                        ".".into()
                    } else {
                        relative
                    },
                    plan::jian_fingerprint(&content, &entries, invocation.cwd == workspace)
                        .map_err(|e| e.to_string())?,
                );
            }
        }
        // 指纹只保存成功后的真实读回，放进内部job兼容字段，不直接写用户工作区。
        if let Some(job) = store::get_job(state.storage(), &base_job)
            .await
            .map_err(|e| e.to_string())?
        {
            let mut cfg: Value =
                serde_json::from_str(&job.config_json).map_err(|e| e.to_string())?;
            cfg["heartbeatSnapshot"] = serde_json::to_value(updated).map_err(|e| e.to_string())?;
            self.save_job(state, agent, &base_job, cfg).await?;
        }
        Ok(())
    }
    async fn reap_targets(&self, state: &ServiceState) {
        let candidates = self
            .targets
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, binding)| binding.execution_returned)
            .map(|(session, binding)| (session.clone(), binding.clone()))
            .collect::<Vec<_>>();
        for (session, binding) in candidates {
            match store::get_trigger_run(state.storage(), &binding.trigger_id).await {
                Ok(Some((_, _, status)))
                    if ![
                        "completed",
                        "failed",
                        "cancelled",
                        "interrupted_needs_attention",
                    ]
                    .contains(&status.as_str()) => {}
                Ok(_) => {
                    self.targets.lock().unwrap().remove(&session);
                    if let Some(agent) = self.agents.lock().unwrap().get_mut(&binding.agent_id) {
                        agent.blocked_run = None;
                    }
                }
                Err(error) => {
                    tracing::error!(?error, "查询心跳日志目标生命周期失败，保留运行锁");
                }
            }
        }
    }
    fn check_current(
        &self,
        state: &ServiceState,
        agent: &str,
        expected: &str,
        automatic: bool,
    ) -> Result<(), String> {
        if self.stopped.load(Ordering::Acquire) || state.submission_intake().is_closed() {
            return Err("heartbeat stopped".into());
        }
        let (cfg, current, _) = config(state.memory(), agent)?;
        if current != expected {
            return Err("heartbeat config changed".into());
        }
        if automatic
            && !plan::HeartbeatSettings::from_config(&cfg, &state.memory().read_preferences_root()?)
                .map_err(|e| e.to_string())?
                .enabled
        {
            return Err("heartbeat disabled".into());
        }
        Ok(())
    }
    async fn scope_session(
        &self,
        state: &ServiceState,
        agent: &str,
        root: &Path,
        kind: &str,
    ) -> Result<String, String> {
        let sid = format!(
            "heartbeat-scope-{}-{}-{}",
            &hash(agent.as_bytes())[..16],
            kind,
            &hash(root.to_string_lossy().as_bytes())[..16]
        );
        let folders = vec![root.to_string_lossy().into_owned()];
        match session_tree::get_session_row(state.storage(), &sid)
            .await
            .map_err(|e| e.to_string())?
        {
            Some(row)
                if row.agent_id == agent
                    && row.owner_user_id == lingxi_kernel::LOCAL_OWNER_SUBJECT
                    && row.lifecycle == "active"
                    && row.permission_mode.as_deref() == Some("operate")
                    && row.authorized_folders == folders =>
            {
                if !lingxi_adapters::storage::automation_activity::is_heartbeat_scope(
                    state.storage(),
                    &sid,
                    agent,
                )
                .await
                .map_err(|error| error.to_string())?
                {
                    return Err("heartbeat scope identity belongs to an ordinary session".into());
                }
            }
            Some(_) => return Err("heartbeat scope session was changed or retired".into()),
            None => {
                state
                    .session_admin()
                    .create_session(
                        &sid,
                        agent,
                        lingxi_kernel::LOCAL_OWNER_SUBJECT,
                        "心跳资源范围",
                        Some("operate"),
                        &folders,
                        true,
                        state.clock().now_unix_ms(),
                    )
                    .await
                    .map_err(|e| format!("{e:?}"))?;
                lingxi_adapters::storage::automation_activity::register_heartbeat_scope(
                    state.storage(),
                    &sid,
                    agent,
                    state.clock().now_unix_ms(),
                )
                .await
                .map_err(|error| error.to_string())?;
            }
        }
        state
            .resource_io()
            .restore_session_roots(&operation(&sid), &folders)
            .map_err(|e| e.to_string())?;
        Ok(sid)
    }
    async fn materials(
        &self,
        state: &ServiceState,
        workspace: &Path,
        op: &OpContext,
    ) -> Result<plan::HeartbeatMaterials, String> {
        let listed = state
            .resource_io()
            .list(&reference(workspace), op)
            .await
            .map_err(|e| e.to_string())?;
        let files = plan::files_from_resource_listing(&listed).map_err(|e| e.to_string())?;
        let mut dirs = vec![(".".to_string(), workspace.to_path_buf())];
        dirs.extend(
            files
                .iter()
                .filter(|f| {
                    f.is_directory
                        && !f.name.starts_with('.')
                        && f.name != plan::WORKSPACE_OUTPUT_ROOT
                })
                .map(|f| (f.name.clone(), workspace.join(&f.name))),
        );
        let mut jian = Vec::new();
        for (relative, dir) in dirs {
            if let Some((content, _)) =
                read_optional(state.resource_io(), &dir.join("jian.md"), op).await?
            {
                let listed = state
                    .resource_io()
                    .list(&reference(&dir), op)
                    .await
                    .map_err(|e| e.to_string())?;
                jian.push(plan::JianDirectory {
                    relative_dir: relative,
                    content,
                    files: plan::files_from_resource_listing(&listed).map_err(|e| e.to_string())?,
                });
            }
        }
        Ok(plan::HeartbeatMaterials {
            workspace: workspace.to_path_buf(),
            desk_files: files,
            overwatch: None,
            patrol_log: None,
            jian_dirs: jian,
        })
    }
    async fn previous_snapshot(
        &self,
        state: &ServiceState,
        agent: &str,
    ) -> Result<(plan::HeartbeatSnapshot, String), String> {
        let job = format!(
            "heartbeat-{}-{}",
            &hash(agent.as_bytes())[..20],
            &hash(b"patrol")[..20]
        );
        let snapshot = match store::get_job(state.storage(), &job)
            .await
            .map_err(|e| e.to_string())?
        {
            Some(old) => {
                let cfg: Value =
                    serde_json::from_str(&old.config_json).map_err(|e| e.to_string())?;
                cfg.get("heartbeatSnapshot")
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(|e| e.to_string())?
                    .unwrap_or_default()
            }
            None => Default::default(),
        };
        Ok((snapshot, job))
    }
    async fn save_job(
        &self,
        state: &ServiceState,
        agent: &str,
        id: &str,
        cfg: Value,
    ) -> Result<store::AutomationJob, String> {
        let input = store::AutomationJobInput {
            id: id.into(),
            studio_id: state.management.identity_ids().1,
            actor_id: agent.into(),
            owner_kind: "local_user".into(),
            owner_subject: lingxi_kernel::LOCAL_OWNER_SUBJECT.into(),
            schema_version: 4,
            config_json: cfg.to_string(),
            next_run_at: None,
        };
        match store::get_job(state.storage(), id)
            .await
            .map_err(|e| e.to_string())?
        {
            Some(old) => store::update_job(
                state.storage(),
                id,
                &input,
                old.config_revision,
                state.clock().now_unix_ms() as i64,
            )
            .await
            .map_err(|e| e.to_string()),
            None => store::create_job(state.storage(), &input, state.clock().now_unix_ms() as i64)
                .await
                .map_err(|e| e.to_string()),
        }
    }
}

pub async fn trigger_now(
    state: &ServiceState,
    automation: &Arc<AutomationHost>,
    agent: &str,
) -> Result<Value, String> {
    automation
        .patrol()
        .trigger_now_for(state, automation, agent)
        .await
}

async fn read_optional(
    io: &ResourceIoService,
    path: &Path,
    ctx: &OpContext,
) -> Result<Option<(String, Value)>, String> {
    let stat = io
        .stat(&reference(path), ctx)
        .await
        .map_err(|e| e.to_string())?;
    if stat["exists"] == false {
        return Ok(None);
    }
    let read = io
        .read(&reference(path), ctx)
        .await
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8(read.content)
        .map_err(|_| "heartbeat material is not valid UTF-8; original retained")?;
    Ok(Some((text, read.meta["version"].clone())))
}

struct LogExecutor {
    host: Weak<PatrolHost>,
    io: Arc<ResourceIoService>,
    storage: Arc<RunDatabase>,
    memory: Arc<crate::memory::MemoryService>,
    clock: Arc<dyn crate::inject::ServiceClock>,
    name: &'static str,
}
impl LogExecutor {
    async fn invoke(&self, ctx: &RunContext, args: &Value) -> Result<Value, String> {
        let host = self.host.upgrade().ok_or("heartbeat host unavailable")?;
        let binding = host
            .targets
            .lock()
            .map_err(|e| e.to_string())?
            .get(ctx.session_id.as_str())
            .cloned()
            .ok_or("no heartbeat log target for this session")?;
        if binding.target.tool_name() != self.name {
            return Err("heartbeat tool does not match this phase".into());
        }
        if host.stopped.load(Ordering::Acquire) {
            return Err("heartbeat stopped".into());
        }
        let (cfg, current, _) = config(&self.memory, &binding.agent_id)?;
        if current != binding.config_hash
            || (binding.automatic
                && !plan::HeartbeatSettings::from_config(
                    &cfg,
                    &self.memory.read_preferences_root()?,
                )
                .map_err(|e| e.to_string())?
                .enabled)
        {
            return Err("heartbeat configuration authorization was withdrawn".into());
        }
        let actual=self.storage.query_one_text("SELECT r.run_id FROM runs r JOIN sessions s ON s.session_id=r.session_id JOIN run_attempts a ON a.run_id=r.run_id JOIN run_lineage l ON l.run_id=r.run_id WHERE r.run_id=?1 AND r.session_id=?2 AND r.owner_kind=?3 AND r.owner_subject=?4 AND s.owner_user_id=?4 AND s.agent_id=?5 AND s.lifecycle='active' AND r.generation=?6 AND a.attempt=?7 AND a.generation=r.generation AND l.origin='heartbeat' AND l.cause_id=?8 AND r.status IN ('running','waiting_approval')",vec![ctx.run_id.to_string(),ctx.session_id.to_string(),ctx.principal.storage_kind().into(),ctx.principal.storage_subject(),binding.agent_id.clone(),ctx.generation.to_string(),ctx.attempt.to_string(),binding.trigger_id.clone()]).await.map_err(|e|e.to_string())?;
        if actual.is_none() {
            return Err("heartbeat log requires the actual active owned Run".into());
        }
        let row = session_tree::get_session_row(&self.storage, ctx.session_id.as_str())
            .await
            .map_err(|e| e.to_string())?
            .ok_or("heartbeat activity session missing")?;
        let source = session_tree::get_session_row(&self.storage, &binding.source_session_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("heartbeat source authorization missing")?;
        if source.lifecycle != "active"
            || source.agent_id != binding.agent_id
            || source.owner_user_id != lingxi_kernel::LOCAL_OWNER_SUBJECT
            || source.permission_mode.as_deref() != Some("operate")
            || source.authorized_folders != row.authorized_folders
        {
            return Err("heartbeat source authorization was withdrawn".into());
        }
        let mut op = operation(ctx.session_id.as_str());
        op.principal_kind = ctx.principal.storage_kind();
        op.principal_subject = ctx.principal.storage_subject();
        op.source = "agent_tool".into();
        self.io
            .restore_session_roots(&op, &row.authorized_folders)
            .map_err(|e| e.to_string())?;
        crate::toolgateway::require_bound_resource(
            binding.target.path(),
            crate::resourceaccess::ResourceOp::Write,
        )?;
        let existing = read_optional(&self.io, binding.target.path(), &op).await?;
        let local = chrono::DateTime::from_timestamp_millis(
            i64::try_from(self.clock.now_unix_ms()).map_err(|_| "heartbeat clock overflow")?,
        )
        .ok_or("heartbeat clock outside range")?
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M")
        .to_string();
        let prepared = binding
            .target
            .prepare_write(
                existing.as_ref().map(|(text, _)| text.as_str()),
                args,
                &local,
            )
            .map_err(|e| e.to_string())?;
        let expected = existing
            .map(|(_, version)| version)
            .unwrap_or_else(|| json!({"exists":false}));
        let written = self
            .io
            .write_expected_version(&reference(&prepared.path), prepared.content, &expected, &op)
            .await
            .map_err(|e| e.to_string())?;
        if written["ok"] == false {
            return Err("heartbeat log changed concurrently; original preserved".into());
        }
        Ok(json!({"ok":true,"path":prepared.path,"capability":prepared.capability}))
    }
}
impl ToolExecutorPort for LogExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            let outcome = match self.invoke(ctx, request.arguments.as_value()).await {
                Ok(value) => ToolOutcome::Success {
                    result: ToolSuccess::text(value.to_string()),
                },
                Err(error) => ToolOutcome::Failed {
                    error: ProtocolError::new(ErrorCode::InvalidMessage, error, false),
                },
            };
            ToolExecutionResult::of_ctx(ctx, outcome)
        })
    }
}
pub fn register(
    gateway: &crate::toolgateway::ToolInvocationGateway,
    host: &Arc<PatrolHost>,
    state: &ServiceState,
) -> Result<(), String> {
    for name in ["patrol_update_log", "jian_update_status"] {
        let mut properties = json!({"status":{"type":"string","enum":["completed","in_progress","skipped","failed"]},"note":{"type":"string","minLength":1}});
        if name == "jian_update_status" {
            properties["progress"] = json!({"type":"string"});
        }
        let receipt=gateway.registry().register(ToolManifest {origin:ToolOrigin::FirstParty,local_name:name.into(),display_name:name.into(),aliases:vec![],version:"1.0.0".into(),description:if name=="patrol_update_log"{"Update this patrol's program-owned log. Submit status and one note; the host owns the exact path and timestamp."}else{"Update this Jian's program-owned status. Submit status, progress and one note; the host preserves the current task body and writes its original snapshot."}.into(),input_schema:ToolSchemaDocument {dialect:"json-schema/2020-12".into(),schema:json!({"type":"object","properties":properties,"required":["status","note"],"additionalProperties":false})},output_schema:None,permission:PermissionContract {kind:PermissionKind::Execute,capability_base:format!("{name}.update")},availability:Availability::Available,timeout_ms:Some(30_000),max_concurrency:Some(1),declared_permission:DeclaredPermission::Execute,recovery:lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE},&SchemaBudget::default()).map_err(|e|e.to_string())?;
        let weak = Arc::downgrade(host);
        let tool_name = name;
        let extractor = Arc::new(
            move |caller: &crate::toolgateway::ResourceExtractionInput,
                  _args: &lingxi_kernel::toolcatalog::EffectiveArguments| {
                let host = weak.upgrade().ok_or("heartbeat host unavailable")?;
                let targets = host.targets.lock().map_err(|e| e.to_string())?;
                let target = targets
                    .get(&caller.session_id)
                    .ok_or("no fixed heartbeat log target")?;
                if target.target.tool_name() != tool_name
                    || caller.principal_kind != "local_user"
                    || caller.principal_subject != lingxi_kernel::LOCAL_OWNER_SUBJECT
                {
                    return Err("heartbeat log caller mismatch".into());
                }
                Ok(vec![crate::resourceaccess::ResourceScope::new(
                    target.target.path().to_path_buf(),
                    crate::resourceaccess::ResourceOp::Write,
                )])
            },
        );
        gateway
            .registry()
            .add_model_resident(&receipt.target_id)
            .map_err(|e| e.to_string())?;
        gateway.bind_executor_with_resources(
            receipt.target_id,
            Arc::new(LogExecutor {
                host: Arc::downgrade(host),
                io: state.resource_io().clone(),
                storage: state.storage().clone(),
                memory: state.memory().clone(),
                clock: state.clock().clone(),
                name,
            }),
            Some(extractor),
            "心跳固定程序日志：真实Run与资源版本写",
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn wait_stopped_waits_for_every_beat_after_coordinator_and_earlier_beat_panic() {
        let host = PatrolHost::default();
        let completed = Arc::new(AtomicBool::new(false));
        *host.coordinator.lock().unwrap() = Some(tokio::spawn(async {
            panic!("合成coordinator退出失败")
        }));
        let finished = completed.clone();
        host.beats.lock().unwrap().extend([
            tokio::spawn(async { panic!("合成首个beat退出失败") }),
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(30)).await;
                finished.store(true, Ordering::Release);
            }),
        ]);
        assert!(
            tokio::time::timeout(Duration::from_millis(1), host.wait_stopped())
                .await
                .is_err()
        );
        let result = tokio::time::timeout(Duration::from_secs(1), host.wait_stopped())
            .await
            .unwrap()
            .unwrap_err();
        assert!(result.contains("heartbeat coordinator"), "{result}");
        assert!(result.contains("heartbeat beat"), "{result}");
        assert!(
            completed.load(Ordering::Acquire),
            "报错前仍须等最后一个beat真正退出"
        );
    }

    #[tokio::test]
    async fn shutdown_timeout_and_concurrent_waiters_keep_coordinator_and_beat_registered() {
        let host = Arc::new(PatrolHost::default());
        let coordinator_release = Arc::new(tokio::sync::Semaphore::new(0));
        let beat_release = Arc::new(tokio::sync::Semaphore::new(0));
        let completed = Arc::new(AtomicBool::new(false));
        let release = coordinator_release.clone();
        *host.coordinator.lock().unwrap() = Some(tokio::spawn(async move {
            release.acquire().await.unwrap().forget();
        }));
        let release = beat_release.clone();
        let finished = completed.clone();
        host.beats.lock().unwrap().push(tokio::spawn(async move {
            release.acquire().await.unwrap().forget();
            finished.store(true, Ordering::Release);
        }));
        host.stop();
        assert!(
            tokio::time::timeout(Duration::from_millis(10), host.wait_stopped())
                .await
                .is_err()
        );
        assert!(
            host.coordinator.lock().unwrap().is_some(),
            "首次超时不能移走coordinator"
        );
        assert_eq!(host.beats.lock().unwrap().len(), 1);
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
        coordinator_release.add_permits(1);
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut first)
                .await
                .is_err(),
            "coordinator退出仍须等beat"
        );
        assert!(tokio::time::timeout(Duration::from_millis(10), &mut second)
            .await
            .is_err());
        assert!(!completed.load(Ordering::Acquire));
        assert_eq!(
            host.beats.lock().unwrap().len(),
            1,
            "二次等待不能丢失同一beat"
        );
        beat_release.add_permits(1);
        tokio::time::timeout(Duration::from_secs(1), first)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), second)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(completed.load(Ordering::Acquire));
        assert!(host.coordinator.lock().unwrap().is_none());
        assert!(host.beats.lock().unwrap().is_empty());
    }
}
