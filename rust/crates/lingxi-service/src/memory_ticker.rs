//! 自动Dream的真实周期宿主；其它每日编译/摘要份额仍由各自业务实现承担。
use crate::{
    dream_service::{clock_iso, logical_date},
    ServiceState,
};
use lingxi_adapters::storage::agent_memory as am;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

pub const DEFAULT_CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);
struct Runtime {
    running: bool,
    started_at: Option<String>,
    interval: Duration,
    agents: HashMap<String, Value>,
    config_epochs: HashMap<String, u64>,
    host_error: Option<String>,
}
impl Default for Runtime {
    fn default() -> Self {
        Self {
            running: false,
            started_at: None,
            interval: DEFAULT_CHECK_INTERVAL,
            agents: HashMap::new(),
            config_epochs: HashMap::new(),
            host_error: None,
        }
    }
}
#[derive(Default)]
pub struct MemoryTickerHost {
    runtime: Mutex<Runtime>,
    stop_requested: AtomicBool,
    wake: tokio::sync::Notify,
    handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
    publication_hook: Mutex<Option<PublicationHook>>,
}
#[doc(hidden)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TickerPublicationPhase {
    BeforePublish,
    AfterPublish,
}
type PublicationHook = Arc<dyn Fn(TickerPublicationPhase, &str, &Value) + Send + Sync>;
#[doc(hidden)]
pub struct TickerPublicationHookGuard<'a> {
    host: &'a MemoryTickerHost,
    previous: Option<PublicationHook>,
}
impl Drop for TickerPublicationHookGuard<'_> {
    fn drop(&mut self) {
        *self.host.publication_hook.lock().unwrap() = self.previous.take();
    }
}
impl MemoryTickerHost {
    /// 可信时序探针位于真实周期的状态发布边界，HTTP不能设置此探针。
    #[doc(hidden)]
    pub fn publication_test_hook(&self, hook: PublicationHook) -> TickerPublicationHookGuard<'_> {
        let previous = self.publication_hook.lock().unwrap().replace(hook);
        TickerPublicationHookGuard {
            host: self,
            previous,
        }
    }
    fn probe(&self, phase: TickerPublicationPhase, agent: &str, outcome: &Value) {
        let hook = self.publication_hook.lock().unwrap().clone();
        if let Some(hook) = hook {
            hook(phase, agent, outcome);
        }
    }
    pub(crate) fn config_epoch(&self, agent: &str) -> u64 {
        *self
            .runtime
            .lock()
            .unwrap()
            .config_epochs
            .get(agent)
            .unwrap_or(&0)
    }
    pub(crate) fn automatic_started(&self, agent: &str, report: &Value, epoch: u64) {
        let mut runtime = self.runtime.lock().unwrap();
        let current = *runtime.config_epochs.get(agent).unwrap_or(&0);
        let entry = runtime
            .agents
            .entry(agent.into())
            .or_insert_with(|| json!({"checkCount":0}));
        entry["currentRunId"] = report["runId"].clone();
        entry["lastRunId"] = report["runId"].clone();
        entry["executionState"] = json!("running");
        if current == epoch && entry["paused"] != true && entry["state"] != "stopped" {
            entry["state"] = json!("automatic_running");
        }
    }
    pub(crate) fn automatic_finished(&self, agent: &str, report: &Value, epoch: Option<u64>) {
        let mut runtime = self.runtime.lock().unwrap();
        let current = *runtime.config_epochs.get(agent).unwrap_or(&0);
        let entry = runtime
            .agents
            .entry(agent.into())
            .or_insert_with(|| json!({"checkCount":0}));
        if entry["currentRunId"].is_string() && entry["currentRunId"] != report["runId"] {
            return;
        }
        if entry["completedRunId"] == report["runId"] {
            return;
        }
        entry["completedRunId"] = report["runId"].clone();
        entry["lastRunId"] = report["runId"].clone();
        entry["currentRunId"] = Value::Null;
        entry["executionState"] = report["status"].clone();
        entry["lastExecutionFinishedAt"] = report["finishedAt"].clone();
        if report["status"] == "succeeded" {
            entry["lastExecutionError"] = Value::Null;
            entry["lastExecutionErrorCode"] = Value::Null;
            entry["lastAdmissionError"] = Value::Null;
            entry["lastFailedRunId"] = Value::Null;
        } else {
            entry["lastExecutionError"] = report["error"].clone();
            entry["lastExecutionErrorCode"] = report["errorCode"].clone();
            entry["lastFailedRunId"] = report["runId"].clone();
        }
        // 晚到终态只更新真实诊断，不能把新配置的paused/stopped覆写为运行。
        if epoch == Some(current) && entry["paused"] != true && entry["state"] != "stopped" {
            entry["state"] = json!(if report["status"] == "succeeded" {
                "automatic_succeeded"
            } else {
                "automatic_failed"
            });
        }
    }
    /// 可信宿主时间调优，生产默认一小时；不向HTTP开放。
    pub(crate) fn configure(&self, interval: Option<Duration>) -> Result<(), am::MemoryError> {
        let interval = interval.unwrap_or(DEFAULT_CHECK_INTERVAL);
        if interval < Duration::from_millis(1)
            || tokio::time::Instant::now().checked_add(interval).is_none()
        {
            return Err(am::MemoryError::Store(
                "memory ticker interval must be at least 1ms".into(),
            ));
        }
        self.runtime.lock().unwrap().interval = interval;
        Ok(())
    }
    pub fn is_running(&self) -> bool {
        self.runtime.lock().unwrap().running
    }
    pub fn snapshot(&self, agent: &str) -> Value {
        let runtime = self.runtime.lock().unwrap();
        let mut value = runtime.agents.get(agent).cloned().unwrap_or_else(|| json!({"state":"awaiting_first_check","paused":null,"automaticEnabled":null,"lastCheckAt":null,"logicalDate":null,"checkCount":0,"lastError":null}));
        value["lastError"] = value["lastExecutionError"]
            .as_str()
            .map(|error| json!(error))
            .or_else(|| {
                value["lastAdmissionError"]
                    .as_str()
                    .map(|error| json!(error))
            })
            .or_else(|| value["clockError"].as_str().map(|error| json!(error)))
            .unwrap_or(Value::Null);
        value["hostRunning"] = json!(runtime.running);
        value["available"] = json!(runtime.running && value["state"] != "stopped");
        value["checkIntervalMs"] = json!(runtime.interval.as_millis() as u64);
        value["hostStartedAt"] = json!(runtime.started_at);
        value["hostError"] = json!(runtime.host_error);
        if !runtime.running {
            value["state"] = json!(if self.stop_requested.load(Ordering::SeqCst) {
                "stopped"
            } else {
                "ticker_absent"
            });
        }
        value
    }
    pub fn configuration_changed(&self, agent: &str, enabled: Option<bool>) {
        let mut runtime = self.runtime.lock().unwrap();
        let epoch = runtime.config_epochs.entry(agent.into()).or_default();
        *epoch = epoch.saturating_add(1);
        let entry = runtime
            .agents
            .entry(agent.into())
            .or_insert_with(|| json!({"checkCount":0,"lastCheckAt":null}));
        if entry["state"] != "stopped" {
            if let Some(enabled) = enabled {
                entry["paused"] = json!(!enabled);
                entry["state"] = json!(if enabled {
                    "awaiting_next_check"
                } else {
                    "paused"
                });
            } else {
                entry["state"] = json!("awaiting_next_check");
                entry["automaticEnabled"] = Value::Null;
            }
        }
        drop(runtime);
        // 配置成功后唤醒；旧周期不得覆盖这次真实写入带来的暂停/恢复。
        self.wake.notify_one();
    }
    pub fn stop_agent(&self, agent: &str) {
        let mut runtime = self.runtime.lock().unwrap();
        let entry = runtime
            .agents
            .entry(agent.into())
            .or_insert_with(|| json!({"checkCount":0,"lastCheckAt":null}));
        entry["state"] = json!("stopped");
        entry["paused"] = json!(true);
    }
    pub fn stop(&self) {
        self.stop_requested.store(true, Ordering::SeqCst);
        self.wake.notify_one();
    }
    pub(crate) fn start(&self, state: ServiceState) {
        let mut handle = self.handle.lock().unwrap();
        if handle.is_some() || self.stop_requested.load(Ordering::SeqCst) {
            return;
        }
        let interval = {
            let mut runtime = self.runtime.lock().unwrap();
            runtime.running = true;
            match clock_iso(&state) {
                Ok(at) => runtime.started_at = Some(at),
                Err(error) => runtime.host_error = Some(error.to_string()),
            }
            runtime.interval
        };
        let registration = state.clone();
        *handle = Some(tokio::spawn(async move {
            let host = state.memory().dream().ticker();
            let mut timer =
                tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                if host.stop_requested.load(Ordering::SeqCst) {
                    break;
                }
                tokio::select! {
                    biased;
                    _ = host.wake.notified() => {},
                    _ = timer.tick() => {},
                }
                if host.stop_requested.load(Ordering::SeqCst) {
                    break;
                }
                host.tick(&state);
            }
            host.runtime.lock().unwrap().running = false;
        }));
        drop(handle);
        self.register(&registration);
    }
    fn register(&self, state: &ServiceState) {
        match state.memory().ordered_live_agents() {
            Ok(agents) => {
                for (agent, _) in agents {
                    match state
                        .memory()
                        .dream()
                        .persisted_automatic_report(state, &agent)
                    {
                        Ok(Some(report)) => self.automatic_finished(&agent, &report, None),
                        Ok(None) => {}
                        Err(error) => {
                            self.runtime.lock().unwrap().host_error = Some(error.to_string())
                        }
                    }
                    let status = state
                        .memory()
                        .read_flags(&agent)
                        .map(|flags| flags.memory_enabled.unwrap_or(true));
                    let mut runtime = self.runtime.lock().unwrap();
                    match status {
                        Ok(enabled) => {
                            let entry = runtime.agents.entry(agent).or_insert_with(|| json!({"automaticEnabled":null,"lastCheckAt":null,"logicalDate":null,"checkCount":0}));
                            if entry["state"] != "stopped" {
                                entry["paused"] = json!(!enabled);
                                entry["state"] = if !enabled {
                                    json!("paused")
                                } else {
                                    match entry["executionState"].as_str() {
                                        Some("failed") => json!("automatic_failed"),
                                        Some("succeeded") => json!("automatic_succeeded"),
                                        _ => json!("awaiting_first_check"),
                                    }
                                };
                            }
                        }
                        Err(error) => {
                            runtime.host_error = Some(error.to_string());
                        }
                    }
                }
            }
            Err(error) => {
                self.runtime.lock().unwrap().host_error = Some(error.to_string());
            }
        }
    }
    fn tick(&self, state: &ServiceState) {
        let agents = match state.memory().ordered_live_agents() {
            Ok(agents) => {
                self.runtime.lock().unwrap().host_error = None;
                agents
            }
            Err(error) => {
                self.runtime.lock().unwrap().host_error = Some(error.to_string());
                state
                    .app_events()
                    .publish("memory-ticker-error", json!({"error":error.to_string()}));
                tracing::error!(error=%error, "memory ticker could not enumerate agents");
                return;
            }
        };
        for (agent, _) in agents {
            if self.stop_requested.load(Ordering::SeqCst) {
                break;
            }
            let epoch = *self
                .runtime
                .lock()
                .unwrap()
                .config_epochs
                .get(&agent)
                .unwrap_or(&0);
            let result = self.check_agent(state, &agent, epoch);
            let outcome = match &result {
                Ok(value) => value.clone(),
                Err(error) => json!({"state":"failed","error":error.to_string()}),
            };
            self.probe(TickerPublicationPhase::BeforePublish, &agent, &outcome);
            let mut runtime = self.runtime.lock().unwrap();
            let current_epoch = *runtime.config_epochs.get(&agent).unwrap_or(&0);
            let entry = runtime
                .agents
                .entry(agent.clone())
                .or_insert_with(|| json!({"checkCount":0}));
            if entry["state"] == "stopped" {
                continue;
            }
            entry["checkCount"] = json!(entry["checkCount"].as_u64().unwrap_or(0) + 1);
            match (clock_iso(state), logical_date(state)) {
                (Ok(at), Ok(date)) => {
                    entry["lastCheckAt"] = json!(at);
                    entry["logicalDate"] = json!(date);
                    entry["clockError"] = Value::Null;
                }
                (at, date) => {
                    entry["lastCheckAt"] = Value::Null;
                    entry["logicalDate"] = Value::Null;
                    entry["clockError"] =
                        json!(format!("memory ticker clock invalid: {at:?}; {date:?}"));
                    entry["lastError"] = entry["clockError"].clone();
                    tracing::error!(agent_id=%agent, "memory ticker clock is outside supported range");
                    continue;
                }
            }
            if current_epoch != epoch {
                drop(runtime);
                self.probe(TickerPublicationPhase::AfterPublish, &agent, &outcome);
                continue;
            }
            match result {
                Ok(value) => {
                    entry["paused"] = value["paused"].clone();
                    entry["automaticEnabled"] = value["automaticEnabled"].clone();
                    entry["eligibility"] = value["state"].clone();
                    entry["lastAdmissionError"] = Value::Null;
                    if matches!(
                        value["state"].as_str(),
                        Some(
                            "already_attempted_today"
                                | "manual_succeeded_today"
                                | "automatic_started"
                        )
                    ) {
                        entry["state"] = match entry["executionState"].as_str() {
                            Some("failed") => json!("automatic_failed"),
                            Some("succeeded") => json!("automatic_succeeded"),
                            Some("running") => json!("automatic_running"),
                            _ => value["state"].clone(),
                        };
                    } else {
                        entry["state"] = value["state"].clone();
                    }
                }
                Err(error) => {
                    entry["state"] = json!("failed");
                    entry["lastAdmissionError"] = json!(error.to_string());
                    state.app_events().publish(
                        "memory-ticker-error",
                        json!({"agentId":agent,"error":error.to_string()}),
                    );
                    tracing::error!(agent_id=%agent, error=%error, "automatic memory dream admission failed");
                }
            }
            drop(runtime);
            self.probe(TickerPublicationPhase::AfterPublish, &agent, &outcome);
        }
    }
    fn check_agent(
        &self,
        state: &ServiceState,
        agent: &str,
        epoch: u64,
    ) -> Result<Value, am::MemoryError> {
        state.memory().dream().lifecycle(state, agent)?;
        if let Some(report) = state
            .memory()
            .dream()
            .persisted_automatic_report(state, agent)?
        {
            self.automatic_finished(agent, &report, None);
        }
        let enabled = state
            .memory()
            .read_flags(agent)?
            .memory_enabled
            .unwrap_or(true);
        let config = am::config::read_yaml_mapping_as_json(&state.memory().config_path(agent)?)?
            .unwrap_or_else(|| json!({}));
        let auto = config
            .pointer("/memory/dream/auto_enabled")
            .and_then(Value::as_bool)
            == Some(true);
        {
            let mut runtime = self.runtime.lock().unwrap();
            if *runtime.config_epochs.get(agent).unwrap_or(&0) == epoch {
                let entry = runtime
                    .agents
                    .entry(agent.into())
                    .or_insert_with(|| json!({"checkCount":0}));
                if entry["state"] != "stopped" {
                    entry["paused"] = json!(!enabled);
                    entry["automaticEnabled"] = json!(auto);
                }
            }
        }
        if !enabled {
            return Ok(json!({"state":"paused","paused":true,"automaticEnabled":auto}));
        }
        if !auto {
            return Ok(
                json!({"state":"automatic_disabled","paused":false,"automaticEnabled":false}),
            );
        }
        match state
            .memory()
            .dream()
            .start_automatic(state.clone(), agent.into())
        {
            Ok(value) => Ok(
                json!({"state":if value["status"]=="running" {"automatic_started"} else {value["reason"].as_str().unwrap_or("skipped")},"paused":false,"automaticEnabled":true,"runId":value["runId"]}),
            ),
            Err(am::MemoryError::Conflict { code, .. })
                if matches!(
                    code.as_str(),
                    "dream_no_memory"
                        | "dream_memory_busy"
                        | "dream_already_running"
                        | "dream_memory_disabled"
                ) =>
            {
                Ok(
                    json!({"state":code,"paused":code=="dream_memory_disabled","automaticEnabled":true}),
                )
            }
            Err(error) => Err(error),
        }
    }
    pub(crate) async fn wait_stopped(&self) -> Result<(), am::MemoryError> {
        // 单助手删除后的旧任务结算不能等待仍服务其它助手的定时宿主。
        if !self.stop_requested.load(Ordering::SeqCst) {
            return Ok(());
        }
        let handle = self.handle.lock().unwrap().take();
        if let Some(handle) = handle {
            if let Err(error) = handle.await {
                let mut runtime = self.runtime.lock().unwrap();
                runtime.running = false;
                runtime.host_error = Some(error.to_string());
                return Err(am::MemoryError::Store(format!(
                    "memory ticker task failed: {error}"
                )));
            }
        }
        Ok(())
    }
}
