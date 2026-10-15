//! 手动记忆整理业务：真实五阶段模型、付费用量、助手排他门与持久结果。
use crate::ServiceState;
use am::{MemoryError, MemoryOperationGuard};
use lingxi_adapters::{
    models::auxiliary::{AuxiliaryExecutor, AuxiliaryRequest, AuxiliaryStep},
    storage::agent_memory as am,
};
use lingxi_kernel::{
    model_exchange::{AuxiliarySlot, ModelOperation, ModelRouteRequest},
    ports::StoragePort,
    usage::{CallOutcome, ModelCallUsageRecord, ReportedUsage},
};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

#[derive(Default)]
pub struct DreamService {
    active: Mutex<HashMap<String, Arc<AtomicBool>>>,
    stopped: Mutex<HashSet<String>>,
    stopped_all: AtomicBool,
    settled: tokio::sync::Notify,
    ticker: crate::memory_ticker::MemoryTickerHost,
}
fn fail(code: &str, message: impl Into<String>) -> MemoryError {
    MemoryError::Conflict {
        code: code.into(),
        message: message.into(),
    }
}
fn store_error(error: impl std::fmt::Display) -> MemoryError {
    MemoryError::Store(error.to_string())
}
fn state_path(memory: &Path) -> PathBuf {
    memory.join("dream/state.json")
}
fn read_state(memory: &Path) -> Result<Value, MemoryError> {
    match std::fs::read_to_string(state_path(memory)) {
        Ok(text) => {
            let value: Value = serde_json::from_str(&text).map_err(store_error)?;
            if value["schemaVersion"] != 1 || !value.is_object() {
                return Err(store_error("梦境状态格式损坏"));
            }
            Ok(value)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(
            json!({"schemaVersion":1,"lastAutomaticAttemptDate":null,"lastSuccessfulManualDate":null,"lastRun":null,"runtime":null,"updatedAt":am::now_iso()}),
        ),
        Err(error) => Err(error.into()),
    }
}
fn persist(memory: &Path, value: &mut Value) -> Result<(), MemoryError> {
    value["updatedAt"] = json!(am::now_iso());
    am::atomic_write(
        &state_path(memory),
        &serde_json::to_string_pretty(value).map_err(store_error)?,
    )
    .map_err(MemoryError::from)
}
fn availability(state: &ServiceState) -> Result<(AuxiliaryExecutor, ModelOperation), MemoryError> {
    let gateway = state.model_gateway().cloned().ok_or_else(|| {
        fail(
            "dream_unavailable",
            "dream_model_unconfigured: 记忆模型未配置",
        )
    })?;
    let credentials = state.credential_service().cloned().ok_or_else(|| {
        fail(
            "dream_unavailable",
            "dream_credentials_unavailable: 模型凭证不可用",
        )
    })?;
    let operation = ModelOperation::Auxiliary(AuxiliarySlot::Memory);
    gateway
        .resolve_dispatch(&ModelRouteRequest::for_operation(operation))
        .map_err(|error| {
            fail(
                "dream_unavailable",
                format!("dream_model_unconfigured: 记忆模型未配置: {error}"),
            )
        })?;
    let network = state.network_plane().cloned().ok_or_else(|| {
        fail(
            "dream_unavailable",
            "dream_network_unavailable: 网络配置不可用",
        )
    })?;
    Ok((
        AuxiliaryExecutor::new_with_network(
            gateway,
            credentials,
            lingxi_kernel::toolcatalog::SchemaBudget::default(),
            network,
        )
        .map_err(store_error)?,
        operation,
    ))
}
impl DreamService {
    /// 与宿主停止及助手销毁共用的真实生命周期；停止后不能继续披露修订。
    pub fn ticker(&self) -> &crate::memory_ticker::MemoryTickerHost {
        &self.ticker
    }
    pub fn stop_agent(&self, agent: &str) {
        self.ticker.stop_agent(agent);
        let active = self.active.lock().unwrap();
        self.stopped.lock().unwrap().insert(agent.into());
        if let Some(token) = active.get(agent) {
            token.store(true, Ordering::SeqCst);
        }
    }
    /// 删除与最终正文/状态发布共用排他门；原子暂存形成逻辑删除后才永久关闭核心。
    /// 失败时不恢复已取消旧任务，新任务仍需等旧任务结算并释放记忆门。
    pub fn delete_agent_files<T, P, E>(
        &self,
        agent: &str,
        prepare: impl FnOnce() -> Result<P, E>,
        delete: impl FnOnce(P) -> Result<T, E>,
    ) -> Result<T, E> {
        let active = self.active.lock().unwrap();
        let prepared = prepare()?;
        if let Some(token) = active.get(agent) {
            token.store(true, Ordering::SeqCst);
        }
        let removed = delete(prepared)?;
        self.stopped.lock().unwrap().insert(agent.into());
        self.ticker.stop_agent(agent);
        Ok(removed)
    }
    pub fn stop_all(&self) {
        self.ticker.stop();
        let active = self.active.lock().unwrap();
        self.stopped_all.store(true, Ordering::SeqCst);
        for token in active.values() {
            token.store(true, Ordering::SeqCst);
        }
    }
    pub async fn wait_stopped(&self) {
        self.wait_stopped_checked()
            .await
            .expect("memory ticker settlement failed");
    }
    pub async fn wait_stopped_checked(&self) -> Result<(), MemoryError> {
        self.ticker.wait_stopped().await?;
        loop {
            let notified = self.settled.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.active.lock().unwrap().is_empty() {
                return Ok(());
            }
            notified.await;
        }
    }
    pub fn lifecycle(&self, state: &ServiceState, agent: &str) -> Result<(), MemoryError> {
        state.memory().roots(agent)?;
        if self.stopped_all.load(Ordering::SeqCst) || self.stopped.lock().unwrap().contains(agent) {
            return Err(fail(
                "dream_unavailable",
                "dream_core_stopped: 该助手记忆整理服务已停止",
            ));
        }
        Ok(())
    }

    /// 配置保存与最终发布串行；关闭总开关不能在最后检查后悄悄穿过。
    pub fn save_config<T, E>(
        &self,
        agent: &str,
        enabled: Option<bool>,
        save: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        let active = self.active.lock().unwrap();
        let result = save()?;
        if enabled == Some(false) {
            if let Some(token) = active.get(agent) {
                token.store(true, Ordering::SeqCst);
            }
        }
        self.ticker.configuration_changed(agent, enabled);
        Ok(result)
    }
    pub fn cancel(&self, agent: &str) -> Result<(), MemoryError> {
        let active = self.active.lock().unwrap();
        let token = active
            .get(agent)
            .ok_or_else(|| fail("dream_not_running", "当前没有记忆整理"))?;
        token.store(true, Ordering::SeqCst);
        Ok(())
    }
    pub fn pause(&self, agent: &str) {
        if let Some(token) = self.active.lock().unwrap().get(agent) {
            token.store(true, Ordering::SeqCst);
        }
    }
    pub fn status(&self, state: &ServiceState, agent: &str) -> Result<Value, MemoryError> {
        let roots = state.memory().roots(agent)?;
        self.lifecycle(state, agent)?;
        availability(state)?;
        let active = self.active.lock().unwrap();
        let mut saved = read_state(&roots.memory_dir)?;
        if saved["runtime"].is_object() && !active.contains_key(agent) {
            let guard =
                am::begin_memory_operation(&roots.memory_dir, am::MemoryOperationKind::Dream)?;
            let store = am::DreamRevisionStore::new(&roots.memory_dir);
            store.recover_for_operation(&guard)?;
            let mut last = saved["runtime"].clone();
            last["status"] = json!("failed");
            last["finishedAt"] = json!(am::now_iso());
            last["errorCode"] = json!("dream_run_interrupted");
            last["error"] = json!("服务重启中断了记忆整理，未报告成功");
            saved["lastRun"] = last.clone();
            if last["trigger"] == "automatic" {
                saved["lastAutomaticRun"] = last.clone();
            }
            saved["runtime"] = Value::Null;
            persist(&roots.memory_dir, &mut saved)?;
        }
        let flags = state.memory().read_flags(agent)?;
        let config = am::config::read_yaml_mapping_as_json(&roots.agent_dir.join("config.yaml"))?
            .unwrap_or_else(|| json!({}));
        let running = saved["runtime"].is_object();
        let ticker = self.ticker.snapshot(agent);
        Ok(
            json!({"ticker":ticker,"tickerAvailable":self.ticker.is_running(),"agentId":agent,"status":if running {"running"} else { saved["lastRun"]["status"].as_str().unwrap_or("idle") },"runId":saved["runtime"]["runId"],"startedAt":saved["runtime"]["startedAt"],"lastRun":saved["lastRun"],"memoryEnabled":flags.memory_enabled.unwrap_or(true),"tickerPaused":ticker["paused"],"coreAvailable":true,"disabledSince":config["memory"]["disabledSince"],"reenableAt":config["memory"]["reenableAt"]}),
        )
    }
    /// 定时服务只读真实既有终态；重启遗留runtime先通过同一记忆门恢复并标注中断。
    pub(crate) fn persisted_automatic_report(
        &self,
        state: &ServiceState,
        agent: &str,
    ) -> Result<Option<Value>, MemoryError> {
        let roots = state.memory().roots(agent)?;
        match std::fs::metadata(state_path(&roots.memory_dir)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        let active = self.active.lock().unwrap();
        let mut saved = read_state(&roots.memory_dir)?;
        if saved["runtime"]["trigger"] == "automatic" && !active.contains_key(agent) {
            let guard =
                am::begin_memory_operation(&roots.memory_dir, am::MemoryOperationKind::Dream)?;
            am::DreamRevisionStore::new(&roots.memory_dir).recover_for_operation(&guard)?;
            let mut report = saved["runtime"].clone();
            report["status"] = json!("failed");
            report["errorCode"] = json!("dream_run_interrupted");
            report["error"] = json!("服务重启中断了自动记忆整理，未报告成功");
            report["finishedAt"] = json!(clock_iso(state)?);
            saved["runtime"] = Value::Null;
            saved["lastRun"] = report.clone();
            saved["lastAutomaticRun"] = report;
            persist(&roots.memory_dir, &mut saved)?;
        }
        let report = if saved["lastAutomaticRun"].is_object() {
            saved["lastAutomaticRun"].clone()
        } else {
            saved["lastRun"].clone()
        };
        Ok((report["trigger"] == "automatic"
            && matches!(report["status"].as_str(), Some("failed" | "succeeded")))
        .then_some(report))
    }
    pub fn start(&self, state: ServiceState, agent: String) -> Result<Value, MemoryError> {
        self.start_with_trigger(state, agent, "manual")
    }
    pub(crate) fn start_automatic(
        &self,
        state: ServiceState,
        agent: String,
    ) -> Result<Value, MemoryError> {
        self.start_with_trigger(state, agent, "automatic")
    }
    fn start_with_trigger(
        &self,
        state: ServiceState,
        agent: String,
        trigger: &'static str,
    ) -> Result<Value, MemoryError> {
        let roots = state.memory().roots(&agent)?;
        if !state
            .memory()
            .read_flags(&agent)?
            .memory_enabled
            .unwrap_or(true)
        {
            return Err(fail("dream_memory_disabled", "该助手记忆已关闭"));
        }
        self.lifecycle(&state, &agent)?;
        let manual_executor = if trigger == "manual" {
            Some(availability(&state)?)
        } else {
            None
        };
        let mut active = self.active.lock().unwrap();
        self.lifecycle(&state, &agent)?;
        if !state
            .memory()
            .read_flags(&agent)?
            .memory_enabled
            .unwrap_or(true)
        {
            return Err(fail("dream_memory_disabled", "该助手记忆已关闭"));
        }
        if active.contains_key(&agent) {
            if trigger == "automatic" {
                return Ok(json!({"status":"skipped","reason":"dream_already_running"}));
            }
            return Err(fail("dream_already_running", "该助手正在整理记忆"));
        }
        if trigger == "automatic" {
            let config =
                am::config::read_yaml_mapping_as_json(&roots.agent_dir.join("config.yaml"))?
                    .unwrap_or_else(|| json!({}));
            if config
                .pointer("/memory/dream/auto_enabled")
                .and_then(Value::as_bool)
                != Some(true)
            {
                return Ok(json!({"status":"skipped","reason":"automatic_disabled"}));
            }
        }
        let guard = am::begin_memory_operation(&roots.memory_dir, am::MemoryOperationKind::Dream)?;
        let store = am::DreamRevisionStore::new(&roots.memory_dir);
        store.recover_for_operation(&guard)?;
        let mut saved = read_state(&roots.memory_dir)?;
        let logical_date = logical_date(&state)?;
        if trigger == "automatic" {
            if saved["lastAutomaticAttemptDate"] == logical_date {
                return Ok(json!({"status":"skipped","reason":"already_attempted_today"}));
            }
            if saved["lastSuccessfulManualDate"] == logical_date {
                return Ok(json!({"status":"skipped","reason":"manual_succeeded_today"}));
            }
        }
        if saved["runtime"].is_object() {
            let mut last = saved["runtime"].clone();
            last["status"] = json!("failed");
            last["errorCode"] = json!("dream_run_interrupted");
            last["finishedAt"] = json!(clock_iso(&state)?);
            last["error"] = json!("服务重启中断了记忆整理，未报告成功");
            if last["trigger"] == "automatic" {
                saved["lastAutomaticRun"] = last.clone();
            }
            saved["lastRun"] = last;
        }
        let before = store.snapshot_for_operation(&guard)?;
        let at = clock_iso(&state)?;
        let ticker_epoch = self.ticker.config_epoch(&agent);
        if trigger == "automatic" {
            saved["lastAutomaticAttemptDate"] = json!(logical_date);
        }
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let run_id = format!(
            "dream-{}-{}-{}",
            std::process::id(),
            now_ms(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        );
        let is_zh = state.memory().locale_is_zh(&roots.agent_dir)?;
        let resolved = if trigger == "automatic" && source_blocks(&before).is_empty() {
            Err(fail("dream_no_memory", "没有可整理的当前记忆"))
        } else {
            manual_executor
                .map(Ok)
                .unwrap_or_else(|| availability(&state))
        };
        let (executor, operation) = match resolved {
            Ok(value) => value,
            Err(error) => {
                let report = json!({"trigger":trigger,"status":"failed","runId":run_id,"startedAt":at,"finishedAt":at,"logicalDate":logical_date,"beforeChars":compiled_chars(&before,is_zh),"afterChars":compiled_chars(&before,is_zh),"model":null,"changed":false,"changedSections":[],"appliedOperationCount":0,"mergedCount":0,"forgottenCount":0,"reviewedCount":0,"notes":[],"revisionId":null,"errorCode":match &error { MemoryError::Conflict{code,..} => code.as_str(), _=>"dream_run_failed" },"error":error.to_string()});
                saved["runtime"] = Value::Null;
                saved["lastRun"] = report.clone();
                if trigger == "automatic" {
                    saved["lastAutomaticRun"] = report.clone();
                }
                persist(&roots.memory_dir, &mut saved)?;
                if trigger == "automatic" {
                    self.ticker
                        .automatic_finished(&agent, &report, Some(ticker_epoch));
                }
                return Err(error);
            }
        };
        let model = state
            .model_gateway()
            .unwrap()
            .resolve_dispatch(&ModelRouteRequest::for_operation(operation))
            .map_err(store_error)?
            .route
            .model;
        let report = json!({"runId":run_id,"trigger":trigger,"status":"running","startedAt":at,"logicalDate":logical_date,"beforeChars":compiled_chars(&before,is_zh),"afterChars":compiled_chars(&before,is_zh),"model":model,"revisionId":null,"changed":false,"changedSections":[],"appliedOperationCount":0,"mergedCount":0,"forgottenCount":0,"reviewedCount":0,"notes":[]});
        saved["runtime"] = report.clone();
        persist(&roots.memory_dir, &mut saved)?;
        let token = Arc::new(AtomicBool::new(false));
        active.insert(agent.clone(), token.clone());
        if trigger == "automatic" {
            self.ticker.automatic_started(&agent, &report, ticker_epoch);
        }
        drop(active);
        let started = json!({"agentId":agent,"status":"running","runId":run_id,"startedAt":at,"lastRun":saved["lastRun"]});
        tokio::spawn(async move {
            let mut report = report;
            let result = execute(
                &state,
                &agent,
                &executor,
                operation,
                &run_id,
                &store,
                &guard,
                &before,
                &token,
                &mut report,
            )
            .await;
            match clock_iso(&state) {
                Ok(at) => report["finishedAt"] = json!(at),
                Err(error) => {
                    report["finishedAt"] = Value::Null;
                    report["clockError"] = json!(error.to_string());
                }
            }
            match result {
                Ok(()) => {
                    report["status"] = json!("succeeded");
                }
                Err(error) => {
                    report["status"] = json!("failed");
                    report["errorCode"] = json!(match &error {
                        MemoryError::Conflict { code, .. } => code.as_str(),
                        _ => "dream_run_failed",
                    });
                    report["error"] = json!(error.to_string());
                    if error.to_string().contains("回滚失败") {
                        report["recoveryRequired"] = json!(true);
                        report["changed"] = Value::Null;
                        report["afterChars"] = Value::Null;
                    }
                }
            }
            // 结算先于发布；即使本地提交或状态落盘失败也不抹去真实调用费用。
            let finish = {
                // 同一门保护存在性检查与状态写入，删除不能穿过检查后重建目录。
                let _publication = state.memory().dream().active.lock().unwrap();
                (|| -> Result<(), MemoryError> {
                    state.memory().roots(&agent)?;
                    let mut saved = read_state(&roots.memory_dir)?;
                    saved["runtime"] = Value::Null;
                    saved["lastRun"] = report.clone();
                    if report["trigger"] == "automatic" {
                        saved["lastAutomaticRun"] = report.clone();
                    }
                    if report["status"] == "succeeded" && report["trigger"] == "manual" {
                        saved["lastSuccessfulManualDate"] = report["logicalDate"].clone();
                    }
                    persist(&roots.memory_dir, &mut saved)
                })()
            };
            if let Err(error) = finish {
                report["status"] = json!("failed");
                report["errorCode"] = json!("dream_state_write_failed");
                report["error"] = json!(error.to_string());
                state.app_events().publish(
                    "dream-state-error",
                    json!({"agentId":agent,"runId":run_id,"error":error.to_string()}),
                );
            }
            if report["trigger"] == "automatic" {
                state.memory().dream().ticker().automatic_finished(
                    &agent,
                    &report,
                    Some(ticker_epoch),
                );
            }
            drop(guard);
            state.memory().dream().active.lock().unwrap().remove(&agent);
            state.memory().dream().settled.notify_waiters();
            state
                .app_events()
                .publish("dream-finished", json!({"agentId":agent,"report":report}));
        });
        Ok(started)
    }
}
pub(crate) fn clock_iso(state: &ServiceState) -> Result<String, MemoryError> {
    let ms = i64::try_from(state.clock().now_unix_ms()).map_err(store_error)?;
    let now = chrono::DateTime::from_timestamp_millis(ms)
        .ok_or_else(|| store_error("memory clock is outside supported range"))?;
    Ok(now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}
pub(crate) fn logical_date(state: &ServiceState) -> Result<String, MemoryError> {
    use chrono::Timelike;
    let ms = i64::try_from(state.clock().now_unix_ms()).map_err(store_error)?;
    let now = chrono::DateTime::from_timestamp_millis(ms)
        .ok_or_else(|| store_error("memory clock is outside supported range"))?
        .with_timezone(&chrono::Local);
    let logical = if now.hour() < 4 {
        now - chrono::Duration::hours(4)
    } else {
        now
    };
    Ok(logical.format("%Y-%m-%d").to_string())
}
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn check(state: &ServiceState, agent: &str, token: &AtomicBool) -> Result<(), MemoryError> {
    if token.load(Ordering::SeqCst) {
        return Err(fail("dream_cancelled", "整理已取消或因记忆关闭而暂停"));
    }
    if !state
        .memory()
        .read_flags(agent)?
        .memory_enabled
        .unwrap_or(true)
    {
        return Err(fail("dream_memory_disabled", "记忆已关闭"));
    }
    Ok(())
}
fn compiled_chars(value: &Value, is_zh: bool) -> usize {
    let week = value["weekDays"]
        .as_array()
        .map(|days| {
            days.iter()
                .map(|day| {
                    format!(
                        "### {}\n\n{}",
                        day["date"].as_str().unwrap_or_default(),
                        day["body"].as_str().unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n\n")
        })
        .unwrap_or_default();
    lingxi_kernel::memory::build_compiled_memory_markdown(
        value["facts"].as_str().unwrap_or_default(),
        value["today"].as_str().unwrap_or_default(),
        &week,
        value["longterm"].as_str().unwrap_or_default(),
        "",
        is_zh,
    )
    .encode_utf16()
    .count()
}
fn body_chars(value: &Value) -> usize {
    ["facts", "today", "longterm"]
        .iter()
        .map(|k| value[k].as_str().unwrap_or_default().encode_utf16().count())
        .sum::<usize>()
        + value["weekDays"]
            .as_array()
            .map(|days| {
                days.iter()
                    .map(|d| {
                        d["body"]
                            .as_str()
                            .unwrap_or_default()
                            .encode_utf16()
                            .count()
                    })
                    .sum::<usize>()
            })
            .unwrap_or(0)
}
#[allow(clippy::too_many_arguments)]
async fn call(
    state: &ServiceState,
    agent: &str,
    executor: &AuxiliaryExecutor,
    operation: ModelOperation,
    run_id: &str,
    stage: &str,
    payload: &Value,
    instruction: &str,
) -> Result<Value, MemoryError> {
    let _permit = state
        .runs()
        .quotas()
        .acquire(crate::quotas::QuotaResource::Model, agent, run_id)
        .await
        .map_err(store_error)?;
    let action = format!("{run_id}-{stage}-{}", now_ms());
    let at = now_ms();
    let ctx = lingxi_kernel::RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("memory-dream"),
        run_id: lingxi_protocol::RunId::new(run_id),
        attempt: lingxi_protocol::AttemptId::new(run_id),
        generation: 1,
    };
    let request = AuxiliaryRequest {
        prompt: payload.to_string(),
        images: vec![],
        max_output_tokens: Some(8192),
        deadline_unix_ms: Some(at + 90_000),
        system_prompt: Some(instruction.into()),
        prior: vec![],
        tools: None,
    };
    let result = executor
        .complete_step_for_operation(
            &ctx,
            operation,
            &format!("memory.dream.{stage}"),
            &lingxi_protocol::ModelCallId::new(&action),
            &request,
        )
        .await;
    let (usage, attempts, served, protocol, outcome) = match &result {
        Ok(AuxiliaryStep::Text(answer)) => (
            answer.usage_report.clone(),
            answer.transport_attempts,
            answer.served_by.clone(),
            answer.served_protocol.clone(),
            CallOutcome::Succeeded,
        ),
        Ok(AuxiliaryStep::ToolRequests {
            usage_report,
            transport_attempts,
            served_by,
            served_protocol,
            ..
        }) => (
            usage_report.clone(),
            *transport_attempts,
            served_by.clone(),
            served_protocol.clone(),
            CallOutcome::Failed,
        ),
        Err(error) => (
            error.usage_report.clone(),
            error.transport_attempts,
            error.served_by.clone(),
            error.served_protocol.clone(),
            CallOutcome::Failed,
        ),
    };
    let settled = now_ms();
    state
        .storage()
        .record_model_call_usage(
            ModelCallUsageRecord {
                session_id: None,
                run_id: None,
                attempt: None,
                model_call_id: action,
                purpose: "auxiliary.memory".into(),
                origin: format!("memory.dream.{stage}"),
                parent_run_id: Some(run_id.into()),
                cause_ref: Some(format!("agent:{agent}")),
                parent_tool_call_id: None,
                provider: served.provider,
                model: served.model,
                protocol: protocol.unwrap_or_else(|| "unknown".into()),
                usage: if let ReportedUsage::Known(value) = usage.clone() {
                    Some(value)
                } else {
                    None
                },
                invalid_detail: if let ReportedUsage::Invalid { detail } = usage {
                    Some(detail)
                } else {
                    None
                },
                transport_attempts: Some(attempts),
                outcome,
                started_at_unix_ms: Some(at),
                settled_at_unix_ms: Some(settled),
                emitted_tool_calls: vec![],
                cost_basis: None,
            },
            settled,
        )
        .await
        .map_err(store_error)?;
    let answer = match result {
        Ok(AuxiliaryStep::Text(answer)) => answer.text,
        Ok(_) => return Err(store_error("梦境模型请求了未许可工具，未执行")),
        Err(error) => return Err(MemoryError::Io(format!("模型调用失败: {error}"))),
    };
    let text = answer.trim();
    let text = if text.starts_with("```") {
        text.trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
    } else {
        text
    };
    let value: Value = serde_json::from_str(text).map_err(store_error)?;
    if !value.is_object() {
        return Err(store_error("梦境模型必须返回JSON对象"));
    }
    Ok(value)
}
fn array<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>, MemoryError> {
    value[key]
        .as_array()
        .ok_or_else(|| store_error(format!("梦境响应缺少{key}数组")))
}
fn ids(value: &Value, key: &str) -> Result<Vec<String>, MemoryError> {
    let mut seen = HashSet::new();
    array(value, key)?
        .iter()
        .map(|id| {
            let id = id
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| store_error("来源ID必须为非空字符串"))?;
            if !seen.insert(id.to_owned()) {
                return Err(store_error("来源ID重复"));
            }
            Ok(id.to_owned())
        })
        .collect()
}
fn plain(value: &Value, key: &str, max: usize) -> Result<String, MemoryError> {
    let text = value[key].as_str().unwrap_or_default().trim();
    if text.is_empty()
        || text.contains(['\n', '\r'])
        || text.starts_with(['#', '-', '*', '+'])
        || text.encode_utf16().count() > max
    {
        return Err(store_error(format!(
            "梦境{key}必须为有效单行文本，上限{max}"
        )));
    }
    Ok(text.into())
}
fn atomic_text(value: &Value) -> Result<String, MemoryError> {
    let text = plain(value, "text", 240)?;
    if text
        .trim_end_matches(['。', '！', '？', '!', '?', '；', ';'])
        .contains(['。', '！', '？', '!', '?', '；', ';'])
    {
        return Err(store_error("原子记忆不得包含多个句子"));
    }
    Ok(text)
}
fn normalized(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric())
        .collect()
}
fn source_blocks(before: &Value) -> Vec<Value> {
    let mut blocks = Vec::new();
    for section in ["facts", "longterm"] {
        let mut heading = String::new();
        let mut index = 0;
        for line in before[section].as_str().unwrap_or_default().lines() {
            let text = line.trim();
            if text.starts_with('#') {
                heading = text.trim_start_matches('#').trim().to_owned();
                continue;
            }
            let text = text.trim_start_matches(['-', '*', '+']).trim();
            if text.is_empty() || matches!(text, "（暂无）" | "(none)" | "none") {
                continue;
            }
            let mut clause = String::new();
            for ch in text.chars().chain(std::iter::once('\0')) {
                if ch != '\0' {
                    clause.push(ch);
                }
                if ch == '\0' || matches!(ch, '。' | '！' | '？' | '!' | '?' | '；' | ';') {
                    let text = clause.trim();
                    if !text.is_empty() {
                        let text = if !heading.is_empty()
                            && !normalized(text).starts_with(&normalized(&heading))
                        {
                            format!("{heading}: {text}")
                        } else {
                            text.into()
                        };
                        blocks.push(json!({"id":format!("source:{section}:{index}"),"section":section,"text":text,"order":blocks.len()}));
                        index += 1;
                    }
                    clause.clear();
                }
            }
        }
    }
    blocks
}
fn atoms(raw: &Value, blocks: &[Value]) -> Result<Vec<Value>, MemoryError> {
    let known: HashMap<_, _> = blocks
        .iter()
        .map(|b| (b["id"].as_str().unwrap(), b))
        .collect();
    let mut covered = HashSet::new();
    let mut per = HashMap::new();
    let mut out = Vec::new();
    for item in array(raw, "units")? {
        let id = item["sourceBlockId"].as_str().unwrap_or_default();
        let block = known
            .get(id)
            .ok_or_else(|| store_error("原子化引用未知来源"))?;
        if item["section"] != block["section"] {
            return Err(store_error("原子化不得跨段移动"));
        }
        covered.insert(id);
        let order = per.entry(id).or_insert(0);
        out.push(json!({"id":format!("atom:{}",out.len()),"sourceBlockIds":[id],"section":block["section"],"text":atomic_text(item)?,"order":block["order"].as_u64().unwrap()*1000+*order}));
        *order += 1;
    }
    if covered.len() != known.len() {
        return Err(store_error("原子化遗漏来源"));
    }
    Ok(out)
}
fn exact(units: &[Value]) -> (Vec<Value>, Vec<Value>) {
    let mut grouped: std::collections::BTreeMap<String, Vec<&Value>> =
        std::collections::BTreeMap::new();
    for unit in units {
        grouped
            .entry(normalized(unit["text"].as_str().unwrap()))
            .or_default()
            .push(unit);
    }
    let mut removed_ids = HashSet::new();
    let mut replacements = HashMap::new();
    let mut operations = Vec::new();
    for group in grouped.values() {
        if group.len() < 2 {
            continue;
        }
        let canonical = *group
            .iter()
            .min_by_key(|unit| {
                (
                    usize::from(unit["section"] != "facts"),
                    unit["order"].as_u64().unwrap(),
                )
            })
            .unwrap();
        let mut source_ids = Vec::new();
        let mut removed = Vec::new();
        for unit in group {
            for id in unit["sourceBlockIds"].as_array().unwrap() {
                if !source_ids.contains(id) {
                    source_ids.push(id.clone());
                }
            }
            if unit["id"] != canonical["id"] {
                removed_ids.insert(unit["id"].as_str().unwrap().to_owned());
                removed.push(unit["id"].clone());
            }
        }
        let mut next = canonical.clone();
        next["sourceBlockIds"] = json!(source_ids);
        replacements.insert(canonical["id"].as_str().unwrap().to_owned(), next);
        operations.push(json!({"kind":"remove_exact_duplicate","canonicalUnitId":canonical["id"],"removedUnitIds":removed}));
    }
    let retained = units
        .iter()
        .filter(|unit| !removed_ids.contains(unit["id"].as_str().unwrap()))
        .map(|unit| {
            replacements
                .get(unit["id"].as_str().unwrap())
                .cloned()
                .unwrap_or_else(|| unit.clone())
        })
        .collect();
    (retained, operations)
}

fn groups(raw: &Value, units: &[Value]) -> Result<Vec<Value>, MemoryError> {
    let known: HashMap<_, _> = units
        .iter()
        .map(|u| (u["id"].as_str().unwrap(), u))
        .collect();
    let mut covered = HashSet::new();
    let mut out = Vec::new();
    for item in array(raw, "groups")? {
        let refs = ids(item, "sourceUnitIds")?;
        let relation = item["relation"].as_str().unwrap_or_default();
        if refs.is_empty()
            || !matches!(relation, "distinct" | "same_meaning" | "subsumes")
            || (relation == "distinct" && refs.len() != 1)
            || (relation != "distinct" && refs.len() < 2)
        {
            return Err(store_error("去重组关系或数量无效"));
        }
        let mut sources = Vec::new();
        for id in &refs {
            let u = known
                .get(id.as_str())
                .ok_or_else(|| store_error("去重引用未知来源"))?;
            if !covered.insert(id.clone()) {
                return Err(store_error("去重重复覆盖来源"));
            }
            sources.push(*u);
        }
        let mut block_ids = Vec::new();
        for u in &sources {
            for id in u["sourceBlockIds"].as_array().unwrap() {
                if !block_ids.contains(id) {
                    block_ids.push(id.clone());
                }
            }
        }
        out.push(json!({"id":format!("group:{}",out.len()),"sourceUnitIds":refs,"sourceBlockIds":block_ids,"section":if sources.iter().any(|u|u["section"]=="facts"){"facts"}else{"longterm"},"relation":relation,"order":sources.iter().map(|u|u["order"].as_u64().unwrap()).min().unwrap(),"sources":sources}));
    }
    if covered.len() != known.len() {
        return Err(store_error("去重遗漏来源"));
    }
    Ok(out)
}
fn optimize(raw: &Value, groups: &[Value]) -> Result<(Vec<Value>, Vec<Value>), MemoryError> {
    let known: HashMap<_, _> = groups
        .iter()
        .map(|g| (g["id"].as_str().unwrap(), g))
        .collect();
    let mut covered = HashSet::new();
    let mut out = Vec::new();
    let mut removed = Vec::new();
    for item in array(raw, "units")? {
        let id = item["groupId"].as_str().unwrap_or_default();
        let group = known.get(id).ok_or_else(|| store_error("优化引用未知组"))?;
        if !covered.insert(id) || item["section"] != group["section"] {
            return Err(store_error("优化重复或跨段移动"));
        }
        out.push(json!({"id":format!("result:{}",out.len()),"groupId":id,"sourceUnitIds":group["sourceUnitIds"],"sourceBlockIds":group["sourceBlockIds"],"section":group["section"],"order":group["order"],"text":atomic_text(item)?}));
    }
    let empty = Vec::new();
    for item in raw
        .get("removedGroups")
        .map(|_| array(raw, "removedGroups"))
        .transpose()?
        .unwrap_or(&empty)
    {
        let id = item["groupId"].as_str().unwrap_or_default();
        let group = known.get(id).ok_or_else(|| store_error("遗忘引用未知组"))?;
        let reason = item["reason"].as_str().unwrap_or_default();
        if !covered.insert(id)
            || !matches!(
                reason,
                "completed_transient" | "obsolete" | "operational_noise"
            )
        {
            return Err(store_error("遗忘原因或覆盖无效"));
        }
        removed.push(json!({"groupId":id,"sourceUnitIds":group["sourceUnitIds"],"sourceBlockIds":group["sourceBlockIds"],"reason":reason}));
    }
    if covered.len() != known.len() {
        return Err(store_error("优化遗漏来源组"));
    }
    Ok((out, removed))
}
fn compose(
    raw: &Value,
    units: &[Value],
    before: &Value,
) -> Result<(Vec<Value>, Value), MemoryError> {
    let known: HashMap<_, _> = units
        .iter()
        .map(|u| (u["id"].as_str().unwrap(), u))
        .collect();
    let mut covered = HashSet::new();
    let mut paragraphs = Vec::new();
    for item in array(raw, "paragraphs")? {
        let refs = ids(item, "sourceUnitIds")?;
        if refs.is_empty() {
            return Err(store_error("编写段落没有来源"));
        }
        let mut order = u64::MAX;
        for id in &refs {
            let unit = known
                .get(id.as_str())
                .ok_or_else(|| store_error("段落引用未知来源"))?;
            if !covered.insert(id.clone()) || item["section"] != unit["section"] {
                return Err(store_error("段落重复来源或跨段"));
            }
            order = order.min(unit["order"].as_u64().unwrap());
        }
        paragraphs.push(json!({"id":format!("paragraph:{}",paragraphs.len()),"section":item["section"],"topic":plain(item,"topic",80)?,"text":plain(item,"text",500)?,"sourceUnitIds":refs,"order":order}));
    }
    if covered.len() != known.len() {
        return Err(store_error("段落遗漏保留的记忆"));
    }
    paragraphs.sort_by_key(|p| p["order"].as_u64().unwrap());
    let mut next = before.clone();
    for section in ["facts", "longterm"] {
        next[section] = json!(paragraphs
            .iter()
            .filter(|p| p["section"] == section)
            .map(|p| p["text"].as_str().unwrap())
            .collect::<Vec<_>>()
            .join("\n\n"));
    }
    if body_chars(&next) > 5000.max(body_chars(before)) {
        return Err(store_error("整理正文超过安全上限"));
    }
    Ok((paragraphs, next))
}
fn verified(raw: &Value) -> Result<Vec<String>, MemoryError> {
    let fields = [
        "missingClaims",
        "compoundUnits",
        "incorrectMerges",
        "unsupportedClaims",
        "subjectLeaks",
        "unsafeRemovals",
        "duplicateClaims",
        "fragmentedTopics",
        "incoherentParagraphs",
    ];
    if raw["ok"] != true {
        return Err(store_error("梦境语义检查失败"));
    }
    for field in fields {
        if !array(raw, field)?.is_empty() {
            return Err(store_error(format!("梦境语义检查失败: {field}")));
        }
    }
    ids(raw, "insufficientCompression")
}
#[allow(clippy::too_many_arguments)]
async fn stage<T>(
    state: &ServiceState,
    agent: &str,
    executor: &AuxiliaryExecutor,
    operation: ModelOperation,
    run_id: &str,
    name: &str,
    payload: Value,
    instruction: &str,
    token: &AtomicBool,
    validate: impl Fn(&Value) -> Result<T, MemoryError>,
) -> Result<T, MemoryError> {
    let mut input = payload;
    for attempt in 0..2 {
        check(state, agent, token)?;
        let name = if attempt == 0 {
            name.into()
        } else {
            format!("{name}_validation_repair")
        };
        let answer = call(
            state,
            agent,
            executor,
            operation,
            run_id,
            &name,
            &input,
            instruction,
        )
        .await;
        check(state, agent, token)?;
        match answer.and_then(|answer| validate(&answer)) {
            Ok(value) => return Ok(value),
            Err(error) if attempt == 0 && !matches!(error, MemoryError::Io(_)) => {
                input["validationRepair"] = json!({"error":error.to_string(),"directive":"只修正格式和覆盖错误；不添加外部来源，不遗漏有效信息。返回完整正确JSON对象。"});
            }
            Err(error) => return Err(error),
        }
    }
    Err(store_error("梦境验证失败"))
}
#[allow(clippy::too_many_arguments)]
async fn execute(
    state: &ServiceState,
    agent: &str,
    executor: &AuxiliaryExecutor,
    operation: ModelOperation,
    run_id: &str,
    store: &am::DreamRevisionStore,
    guard: &MemoryOperationGuard,
    before: &Value,
    token: &AtomicBool,
    report: &mut Value,
) -> Result<(), MemoryError> {
    check(state, agent, token)?;
    let blocks = source_blocks(before);
    if blocks.is_empty() {
        return Err(fail("dream_no_memory", "没有可整理的当前记忆"));
    }
    let config_path = state.memory().config_path(agent)?;
    let config_before = am::config::read_config_text(&config_path).map_err(store_error)?;
    let atoms=stage(state,agent,executor,operation,run_id,"atomize",json!({"sourceBlocks":blocks}),"memory.dream.atomize：只拆分当前 facts 与 longterm。每个来源恰好覆盖，可拆成多条；每条独立单句、240字符内、不含markdown、不移动段落、不添加外部信息。JSON {units:[{sourceBlockId,section,text}]}。",token,|raw|atoms(raw,&blocks)).await?;
    let (units, exact_ops) = exact(&atoms);
    let groups=stage(state,agent,executor,operation,run_id,"dedupe",json!({"units":units,"exactDuplicateOperations":exact_ops}),"memory.dream.dedupe：只判断语义重复，每个来源恰好覆盖。distinct只含一条，同义same_meaning或完整包含subsumes至少两条；相关或冲突不合并。JSON {groups:[{sourceUnitIds,relation}]}。",token,|raw|groups(raw,&units)).await?;
    let (optimized,removed)=stage(state,agent,executor,operation,run_id,"optimize",json!({"groups":groups}),"memory.dream.optimize：忠实简化，每组恰好保留或删除。保留所有稳定身份、偏好、边界、关系、未完成事项。不跨段、不凭年龄比例遗忘、不添加信息。text单句240字符。删除只限明确completed_transient、obsolete或operational_noise。JSON {units:[{groupId,section,text}],removedGroups:[{groupId,reason}]}。",token,|raw|optimize(raw,&groups)).await?;
    let payload = json!({"units":optimized,"constraints":{"maxParagraphChars":500,"maxTopicChars":80,"softTargets":{"facts":400,"longterm":800},"maxTotalBodyChars":5000.max(body_chars(before))}});
    let compose_instruction="memory.dream.compose：将保留记忆自然编写，每个sourceUnitId恰好覆盖一次，不跨段、不加信息、不丢独特信息。段落纯单行无markdown，text不超过500字符，topic不超过80字符。JSON {paragraphs:[{sourceUnitIds,section,topic,text}]}。";
    let (mut paragraphs, mut next) = stage(
        state,
        agent,
        executor,
        operation,
        run_id,
        "compose",
        payload.clone(),
        compose_instruction,
        token,
        |raw| compose(raw, &optimized, before),
    )
    .await?;
    let verification_payload = |paragraphs: &Vec<Value>, next: &Value| json!({"currentSections":before,"sourceBlocks":blocks,"atomicUnits":atoms,"exactDuplicateOperations":exact_ops,"dedupeGroups":groups,"optimizedUnits":optimized,"removedGroups":removed,"composedParagraphs":paragraphs,"proposedSections":next});
    let verify_instruction="memory.dream.verify：独立核查来源与拟写正文。禁止遗漏唯一信息、复合单位、错误合并、无来源事实、主体泄露、不安全遗忘、重复、主题碎片、段落不连贯。JSON {ok:boolean,missingClaims:[],compoundUnits:[],incorrectMerges:[],unsupportedClaims:[],subjectLeaks:[],unsafeRemovals:[],duplicateClaims:[],fragmentedTopics:[],incoherentParagraphs:[],insufficientCompression:[]}。只把可改善压缩措辞作为软建议insufficientCompression，不允许建议丢信息。";
    let feedback = stage(
        state,
        agent,
        executor,
        operation,
        run_id,
        "verify",
        verification_payload(&paragraphs, &next),
        verify_instruction,
        token,
        verified,
    )
    .await?;
    if !feedback.is_empty() {
        let mut repair = payload;
        repair["compressionRepair"] = json!({"previousParagraphs":paragraphs,"feedback":feedback});
        (paragraphs, next) = stage(
            state,
            agent,
            executor,
            operation,
            run_id,
            "compose_compression_repair",
            repair,
            compose_instruction,
            token,
            |raw| compose(raw, &optimized, before),
        )
        .await?;
        stage(
            state,
            agent,
            executor,
            operation,
            run_id,
            "verify_compression_repair",
            verification_payload(&paragraphs, &next),
            verify_instruction,
            token,
            verified,
        )
        .await?;
    }
    let _commit = state.memory().dream().active.lock().unwrap();
    check(state, agent, token)?;
    if am::config::read_config_text(&config_path).map_err(store_error)? != config_before {
        return Err(fail("dream_memory_changed", "整理期间助手配置已改变"));
    }
    let revision = store.apply_for_operation(
        guard,
        run_id,
        &am::dream_snapshot_hash(before)?,
        next["facts"].as_str().unwrap(),
        next["longterm"].as_str().unwrap(),
        state
            .memory()
            .locale_is_zh(&state.memory().roots(agent)?.agent_dir)?,
    )?;
    let changed: Vec<_> = ["facts", "longterm"]
        .into_iter()
        .filter(|key| next[*key] != before[*key])
        .collect();
    report["changed"] = json!(revision.is_some());
    report["changedSections"] = json!(changed);
    report["revisionId"] = revision
        .as_ref()
        .map(|r| r["revisionId"].clone())
        .unwrap_or(Value::Null);
    report["afterChars"] = json!(compiled_chars(
        &next,
        state
            .memory()
            .locale_is_zh(&state.memory().roots(agent)?.agent_dir)?
    ));
    report["mergedCount"] = json!(
        exact_ops
            .iter()
            .map(|operation| operation["removedUnitIds"].as_array().unwrap().len())
            .sum::<usize>()
            + groups
                .iter()
                .filter(|g| g["relation"] != "distinct")
                .map(|g| g["sourceUnitIds"].as_array().unwrap().len() - 1)
                .sum::<usize>()
    );
    report["forgottenCount"] = json!(removed.len());
    report["reviewedCount"] = json!(0);
    let split_count = blocks
        .iter()
        .filter(|block| {
            atoms
                .iter()
                .filter(|unit| {
                    unit["sourceBlockIds"]
                        .as_array()
                        .unwrap()
                        .contains(&block["id"])
                })
                .count()
                > 1
        })
        .count();
    let merge_count = groups
        .iter()
        .filter(|group| group["relation"] != "distinct")
        .count();
    let rewrite_count = optimized
        .iter()
        .filter(|unit| {
            let refs = unit["sourceUnitIds"].as_array().unwrap();
            refs.len() == 1
                && units
                    .iter()
                    .find(|source| source["id"] == refs[0])
                    .is_some_and(|source| {
                        normalized(source["text"].as_str().unwrap())
                            != normalized(unit["text"].as_str().unwrap())
                    })
        })
        .count();
    report["appliedOperationCount"] = json!(if revision.is_some() {
        exact_ops.len()
            + split_count
            + merge_count
            + rewrite_count
            + usize::from(!removed.is_empty())
            + paragraphs.len()
    } else {
        0
    });
    if revision.is_some() {
        state
            .app_events()
            .publish("memory-compiled", json!({"agentId":agent,"runId":run_id}));
    }
    Ok(())
}
