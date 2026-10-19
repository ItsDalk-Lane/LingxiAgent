//! 五平台接入宿主：外围进程只报告平台事实，Rust 持有身份、任务和投递收据。
use crate::{
    bridge_media_pipe::BridgeMediaPipe,
    bridge_worker::{BridgeWorkerLaunch, BridgeWorkerProcess},
    ServiceState,
};
use lingxi_adapters::storage::bridge as store;
use lingxi_kernel::ports::StoragePort;
use lingxi_protocol::{
    BridgeConnectionStatus, BridgeReceiptStatus, BridgeWorkerCommand, BridgeWorkerEvent,
    ContentBlock, NormalizedMessage, RunId, RunStatus,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    sync::{watch, Mutex as AsyncMutex},
    task::JoinHandle,
};

const MAX_CONNECTORS: usize = 64;
struct BridgeCommandJob {
    inbox: store::BridgeInbox,
    source: String,
    text: String,
}
#[derive(Clone)]
struct ConnectorLive {
    generation: u64,
    status: String,
    error: Option<String>,
}
pub struct BridgeHost {
    launch: Option<BridgeWorkerLaunch>,
    started: AtomicBool,
    diagnostic_slots: Arc<tokio::sync::Semaphore>,
    stopped: watch::Sender<bool>,
    tasks: Mutex<Vec<JoinHandle<Result<(), String>>>>,
    wait: AsyncMutex<()>,
    errors: Mutex<Vec<String>>,
    live: Mutex<BTreeMap<String, ConnectorLive>>,
    /// 启动恢复结果：None=尚未完成；服务就绪前必须等到 Some。
    recovery: watch::Sender<Option<Result<(), String>>>,
}
impl BridgeHost {
    pub fn new(launch: Option<BridgeWorkerLaunch>) -> Self {
        Self {
            launch,
            started: AtomicBool::new(false),
            diagnostic_slots: Arc::new(tokio::sync::Semaphore::new(4)),
            stopped: watch::channel(false).0,
            tasks: Mutex::new(Vec::new()),
            wait: AsyncMutex::new(()),
            errors: Mutex::new(Vec::new()),
            live: Mutex::new(BTreeMap::new()),
            recovery: watch::channel(None).0,
        }
    }
    fn status(
        &self,
        state: &ServiceState,
        connector: &store::BridgeConnector,
        status: &str,
        error: Option<String>,
    ) {
        self.live.lock().unwrap_or_else(|e| e.into_inner()).insert(
            connector.input.id.clone(),
            ConnectorLive {
                generation: connector.generation,
                status: status.into(),
                error,
            },
        );
        state
            .app_events()
            .publish("bridge_status", json!({"agentId":connector.input.actor_id}));
    }
    pub fn status_value(&self, id: &str) -> Value {
        self.live
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .map(|row| json!({"generation":row.generation,"status":row.status,"error":row.error}))
            .unwrap_or_else(|| json!({"status":"disconnected","error":null}))
    }
    pub async fn diagnostic(
        self: &Arc<Self>,
        command: lingxi_protocol::BridgeDiagnosticCommand,
    ) -> Result<Value, String> {
        let finished = {
            let mut tasks = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
            let mut finished = Vec::new();
            let mut index = 0;
            while index < tasks.len() {
                if tasks[index].is_finished() {
                    finished.push(tasks.swap_remove(index));
                } else {
                    index += 1;
                }
            }
            finished
        };
        for task in finished {
            let error = match task.await {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(error),
                Err(error) => Some(format!("Bridge host task failed: {error}")),
            };
            if let Some(error) = error {
                self.errors
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(error);
            }
        }
        if *self.stopped.borrow() {
            return Err("Bridge host is stopping".into());
        }
        let permit = self
            .diagnostic_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| "Bridge credential checks are busy")?;
        let launch = self
            .launch
            .as_ref()
            .ok_or("Bridge SDK worker runtime is not configured")?
            .diagnostic()?;
        // 微信一次正常扫码状态长轮询允许等待 40 秒。
        let response_timeout = if matches!(
            &command,
            lingxi_protocol::BridgeDiagnosticCommand::WechatQrcodeStatus { .. }
        ) {
            Duration::from_secs(41)
        } else {
            Duration::from_secs(20)
        };
        let frame = serde_json::to_value(command)
            .map_err(|_| "Bridge diagnostic command encoding failed")?;
        let mut stop = self.stopped.subscribe();
        let (send, received) = tokio::sync::oneshot::channel();
        {
            let mut tasks = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
            if *self.stopped.borrow() {
                return Err("Bridge host is stopping".into());
            }
            let task = tokio::spawn(async move {
                let _permit = permit;
                let mut worker = match BridgeWorkerProcess::spawn(&launch, &frame).await {
                    Ok(worker) => worker,
                    Err(error) => {
                        let _ = send.send(Err(error));
                        return Ok(());
                    }
                };
                let result = if *stop.borrow() {
                    Err("Bridge diagnostic cancelled during shutdown".into())
                } else {
                    tokio::select! {
                        _=stop.changed()=>Err("Bridge diagnostic cancelled during shutdown".into()),
                        result=tokio::time::timeout(response_timeout,worker.next())=>match result{
                            Ok(Ok(Some(value)))=>serde_json::from_value::<lingxi_protocol::BridgeDiagnosticResult>(value).map_err(|_|"Bridge diagnostic result violates the unified contract".to_string()).and_then(|value|serde_json::to_value(value).map_err(|_|"Bridge diagnostic result encoding failed".to_string())),
                            _=>Err("Bridge diagnostic returned no confirmed result".into()),
                        }
                    }
                };
                let cleanup = worker.stop().await;
                let response = match &cleanup {
                    Ok(()) => result,
                    Err(error) => Err(error.clone()),
                };
                let _ = send.send(response);
                cleanup
            });
            tasks.push(task);
        }
        received
            .await
            .map_err(|_| "Bridge diagnostic task failed".to_string())?
    }
    pub fn stop(&self) {
        // 与诊断进程登记共用边界：退出之后不再出现未登记的新子进程。
        let _tasks = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
        self.stopped.send_replace(true);
    }
    pub fn start(self: &Arc<Self>, state: ServiceState) {
        let mut tasks = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
        if *self.stopped.borrow() || self.started.swap(true, Ordering::AcqRel) {
            return;
        }
        let host = self.clone();
        tasks.push(tokio::spawn(async move { host.run(state).await }));
    }
    /// 服务就绪门：等启动恢复阶段的确定结果。宿主任务在发布前死亡（panic）
    /// 时通道关闭——响亮报错，不假装恢复完成。
    pub async fn wait_recovery(&self) -> Result<(), String> {
        let mut rx = self.recovery.subscribe();
        loop {
            if let Some(result) = rx.borrow().clone() {
                return result;
            }
            if rx.changed().await.is_err() {
                return Err("bridge host died before publishing its recovery outcome".into());
            }
        }
    }
    pub async fn wait_stopped(&self) -> Result<(), String> {
        let _wait = self.wait.lock().await;
        // 等待 future 被超时取消时，JoinHandle 仍保存在宿主，不会变成无人监督的任务。
        loop {
            let next = std::future::poll_fn(|cx| {
                let mut tasks = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
                let Some(task) = tasks.last_mut() else {
                    return std::task::Poll::Ready(None);
                };
                match std::future::Future::poll(std::pin::Pin::new(task), cx) {
                    std::task::Poll::Pending => std::task::Poll::Pending,
                    std::task::Poll::Ready(result) => {
                        tasks.pop();
                        std::task::Poll::Ready(Some(result))
                    }
                }
            })
            .await;
            match next {
                Some(Ok(Ok(()))) => {}
                Some(Ok(Err(error))) => self
                    .errors
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(error),
                Some(Err(error)) => self
                    .errors
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(format!("Bridge host task failed: {error}")),
                None => break,
            }
        }
        let errors = self.errors.lock().unwrap_or_else(|e| e.into_inner());
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
    async fn run(self: Arc<Self>, state: ServiceState) -> Result<(), String> {
        // 恢复阶段整体成败先发布给 wait_recovery（服务就绪门），再决定循环去留。
        let recovery = async {
            crate::bridge_routes::import_existing(&state)
                .await
                .map_err(|e| format!("bridge import_existing: {e}"))?;
            lingxi_adapters::storage::bridge_effect::recover_started_effects(
                state.storage(),
                state.clock().now_unix_ms(),
            )
            .await
            .map_err(|e| format!("bridge recover_started_effects: {e}"))?;
            store::recover_started_outbox(state.storage(), state.clock().now_unix_ms())
                .await
                .map_err(|e| format!("bridge recover_started_outbox: {e}"))?;
            // 运行恢复由唯一真实 Run 判定；未准备完整的附件不盲目补跑。
            store::recover_started_commands(state.storage(), state.clock().now_unix_ms())
                .await
                .map_err(|e| format!("bridge recover_started_commands: {e}"))?;
            store::recover_inbox(state.storage(), state.clock().now_unix_ms())
                .await
                .map_err(|e| format!("bridge recover_inbox: {e}"))?;
            for delivery in lingxi_adapters::storage::bridge_reads::pending_notices(state.storage())
                .await
                .map_err(|e| format!("bridge pending_notices: {e}"))?
            {
                record_notice(&state, &delivery).await;
            }
            Ok::<(), String>(())
        }
        .await;
        self.recovery.send_replace(Some(recovery.clone()));
        recovery?;
        let mut stop = self.stopped.subscribe();
        let mut workers = HashMap::<String, (u64, watch::Sender<bool>)>::new();
        let mut children = tokio::task::JoinSet::<(String, u64, Result<(), String>)>::new();
        let mut tick = tokio::time::interval(Duration::from_millis(200));
        let mut failed = HashMap::<String, u64>::new();
        let outcome=async {
        loop {
            // 恢复数据库期间可能已经收到退出请求；新订阅者不会再收到旧的 changed 通知。
            if *stop.borrow() { break; }
            tokio::select! {
                _ = stop.changed() => { if *stop.borrow() { break; } }
                done = children.join_next(), if !children.is_empty() => {
                    match done {
                        Some(Ok((id, generation, result))) => {
                            if workers.get(&id).is_some_and(|entry|entry.0==generation) { workers.remove(&id); }
                            if let Err(error) = result { failed.insert(id.clone(), generation); tracing::error!(connector=%id,error=%error,"Bridge connector stopped"); }
                        }
                        Some(Err(error)) => { self.errors.lock().unwrap_or_else(|e|e.into_inner()).push(format!("Bridge connector task panicked: {error}")); break; }
                        None => {},
                    }
                }
                _ = tick.tick() => {
                    let connectors = store::list_connectors(state.storage()).await.map_err(|e| format!("bridge tick list_connectors: {e}"))?;
                    for (id,(generation, stop)) in &workers {
                        if !connectors.iter().any(|row|row.input.id==*id && row.generation==*generation && row.input.enabled) { stop.send_replace(true); }
                    }
                    for connector in connectors {
                        if !connector.input.enabled || workers.contains_key(&connector.input.id) || failed.get(&connector.input.id)==Some(&connector.generation) { continue; }
                        if workers.len() >= MAX_CONNECTORS { self.status(&state,&connector,"error",Some("Bridge connector capacity exceeded".into())); continue; }
                        let Some(launch) = self.launch.clone() else { self.status(&state,&connector,"error",Some("Bridge SDK worker runtime is not configured".into())); failed.insert(connector.input.id.clone(),connector.generation); continue; };
                        let (cancel, stopped) = watch::channel(false);
                        workers.insert(connector.input.id.clone(), (connector.generation,cancel));
                        let (host,state) = (self.clone(),state.clone());
                        children.spawn(async move { let id=connector.input.id.clone();let generation=connector.generation;let result=host.run_connector(&state,&connector,&launch,stopped).await;if let Err(error)=&result{host.status(&state,&connector,"error",Some(error.clone()));}(id,generation,result) });
                    }
                }
            }
        }
        Ok(())
        }.await;
        for (_, (_, stop)) in workers {
            stop.send_replace(true);
        }
        // JoinSet 留在宿主任务内部；外部等待超时不会取消回收责任。
        while let Some(result) = children.join_next().await {
            match result {
                Ok((_, _, Ok(()))) => {}
                Ok((_, _, Err(error))) => self
                    .errors
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(error),
                Err(error) => self
                    .errors
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(error.to_string()),
            }
        }
        outcome
    }
    async fn run_connector(
        &self,
        state: &ServiceState,
        connector: &store::BridgeConnector,
        launch: &BridgeWorkerLaunch,
        mut stop: watch::Receiver<bool>,
    ) -> Result<(), String> {
        let generation = u32::try_from(connector.generation)
            .map_err(|_| "Bridge connector generation exceeds the pipe protocol")?;
        let source: Value = serde_json::from_str(&connector.input.config_json)
            .map_err(|_| "Bridge connector configuration damaged")?;
        let config = crate::bridge_routes::worker_config(
            &connector.input.platform,
            &source,
            &connector.input.actor_id,
        )?;
        let init = serde_json::to_value(BridgeWorkerCommand::Init {
            connector_id: connector.input.id.clone(),
            generation,
            platform: connector.input.platform.clone(),
            config,
        })
        .map_err(|e| e.to_string())?;
        self.status(state, connector, "connecting", None);
        let mut worker = BridgeWorkerProcess::spawn(launch, &init).await?;
        let mut media = BridgeMediaPipe::default();
        let mut effects = crate::bridge_effects::BridgeEffects::default();
        let mut inflight = HashMap::<String, tokio::time::Instant>::new();
        let mut commands = tokio::task::JoinSet::<(String, Result<Value, String>)>::new();
        let mut command_ids = std::collections::BTreeSet::new();
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        let ready_deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        let mut ready = false;
        let mut connected = false;
        let result=async {
            loop {
                if *stop.borrow() { break; }
                tokio::select! {
                    _=stop.changed()=>{if *stop.borrow(){break;}}
                    frame=worker.next()=>{
                        let frame=frame?.ok_or("Bridge SDK worker disconnected")?;
                        if frame["connectorId"]!=connector.input.id || frame["generation"].as_u64()!=Some(connector.generation) { return Err("Bridge SDK worker identity or generation mismatch".into()); }
                        let event:BridgeWorkerEvent=serde_json::from_value(frame.clone()).map_err(|_|"Bridge SDK worker event violates the unified contract")?;
                        match event {
                            BridgeWorkerEvent::Ready{capabilities,..}=>{if ready{return Err("duplicate Bridge worker readiness".into());}ready=true;connected=true;effects.set_capabilities(capabilities);self.status(state,connector,"connected",None);}
                            BridgeWorkerEvent::ConnectionStatus{status,error,..}=>{
                                connected = status == BridgeConnectionStatus::Connected;
                                if connected && !ready {return Err("Bridge connection became ready without initialization".into());}
                                self.status(state,connector,match status {BridgeConnectionStatus::Connecting=>"connecting",BridgeConnectionStatus::Connected=>"connected",BridgeConnectionStatus::Disconnected=>"disconnected"},error);
                            }
                            BridgeWorkerEvent::Message{..}=>{if !ready{return Err("Bridge message arrived before initialization".into());}
                            if let Some(job)=receive(state,connector,&frame,&mut media).await? {
                                if commands.len()>=16{return Err("Bridge command capacity exceeded".into());}
                                command_ids.insert(job.inbox.id.clone());
                                let state=state.clone();
                                commands.spawn(async move{let id=job.inbox.id.clone();let result=crate::slash::dispatch_bridge_message(&state,&job.inbox,&job.source,&job.text).await.and_then(|result|result.ok_or_else(||"Bridge command returned no result".into())).and_then(|result|serde_json::to_value(result).map_err(|_|"Bridge command result encoding failed".into()));(id,result)});
                            }}
                            BridgeWorkerEvent::MediaBegin{..}|BridgeWorkerEvent::MediaChunk{..}|BridgeWorkerEvent::MediaEnd{..}=>{if !ready{return Err("Bridge media arrived before initialization".into());}media.push(&frame)?;}
                            BridgeWorkerEvent::Receipt{delivery_id,status,platform_message_ids,error,..}=>{
                                if inflight.remove(&delivery_id).is_none(){return Err("Bridge worker returned an unsolicited delivery receipt".into());}
                                let (status,receipt)=match status {
                                    BridgeReceiptStatus::Confirmed if !platform_message_ids.is_empty()=>(store::BridgeOutboxStatus::Confirmed,Some(store::BridgePlatformReceipt{platform:connector.input.platform.clone(),message_ids:platform_message_ids,raw_json:None})),
                                    BridgeReceiptStatus::Confirmed=>(store::BridgeOutboxStatus::Unknown,None),
                                    BridgeReceiptStatus::Failed=>(store::BridgeOutboxStatus::Failed,None),
                                    BridgeReceiptStatus::Unknown=>(store::BridgeOutboxStatus::Unknown,None),
                                };
                                let delivered=store::settle_outbox(state.storage(),&delivery_id,status,receipt,error,state.clock().now_unix_ms()).await.map_err(|e|e.to_string())?;
                                if status==store::BridgeOutboxStatus::Confirmed {
                                    confirmed_delivery(state,&delivered).await?;
                                }
                            }
                            BridgeWorkerEvent::Error{error,..}=>return Err(error),
                            BridgeWorkerEvent::EffectReceipt{receipt,..}=>{
                                if let Some(outbox)=effects.accept(state,receipt).await? {
                                    if outbox.status==store::BridgeOutboxStatus::Confirmed {confirmed_delivery(state,&outbox).await?;}
                                }
                            },
                            BridgeWorkerEvent::Stopped{..}=>break,
                        }
                    }
                    done=commands.join_next(),if !commands.is_empty()=>{
                        match done {
                            Some(Ok((id,result)))=>{command_ids.remove(&id);finish_command(state,&id,result).await?;}
                            Some(Err(_))=>return Err("Bridge command task failed before a confirmed result".into()),
                            None=>{},
                        }
                    }
                    _=tick.tick()=>{
                        if !current(state,connector).await? { break; }
                        if media.expired() { return Err("Bridge media transfer timed out".into()); }
                        if !ready { if tokio::time::Instant::now()>=ready_deadline{return Err("Bridge SDK initialization timed out".into());}continue; }
                        queue_command_replies(state,connector).await?;
                        advance_inbox(state,connector).await?;
                        if inflight.values().any(|started|started.elapsed()>Duration::from_secs(30)){return Err("Bridge delivery receipt timed out; sending outcome is unknown".into());}
                        if !connected {effects.expire_pending(state).await?;continue;}
                        effects.tick(state,connector,&mut worker).await?;
                        if inflight.len()<8 {
                            for outbox in store::list_pending_outbox(state.storage()).await.map_err(|e|e.to_string())?.into_iter().filter(|row|row.input.connector_id==connector.input.id && row.input.connector_generation==connector.generation).take(8-inflight.len()) {
                                if effects.busy_for(&outbox) {continue;}
                                let claim=store::claim_outbox(state.storage(),&outbox.input.delivery_id,connector.generation,state.clock().now_unix_ms()).await.map_err(|e|e.to_string())?;
                                if !claim.claimed {continue;}
                                if effects.finish(state,connector,&mut worker,&claim.outbox).await? {continue;}
                                let prepared:Result<Value,String>=async {
                                    let payload:Value=serde_json::from_str(&claim.outbox.input.payload_json).map_err(|_|"Bridge outbox payload damaged")?;
                                    let context:lingxi_protocol::BridgeReplyContext=serde_json::from_str(&claim.outbox.reply_context_json).map_err(|_|"Bridge reply context damaged")?;
                                    let attachment_ids=prepare_outbound_media(state,connector,generation,&claim.outbox,&mut worker).await?;
                                    store::validate_started_outbox(state.storage(),&outbox.input.delivery_id,connector.generation).await.map_err(|e|e.to_string())?;
                                    if !current(state,connector).await? {return Err("Bridge connector authorization changed before send".into());}
                                    serde_json::to_value(BridgeWorkerCommand::Send{dispatch_seq:None,connector_id:connector.input.id.clone(),generation,delivery_id:outbox.input.delivery_id.clone(),conversation_id:claim.outbox.conversation_id.clone(),text:payload["text"].as_str().ok_or("Bridge outbox text missing")?.into(),reply_context:Some(context),attachments:attachment_ids}).map_err(|e|e.to_string())
                                }.await;
                                let command=match prepared {
                                    Ok(command)=>command,
                                    Err(error)=>{store::settle_outbox(state.storage(),&outbox.input.delivery_id,store::BridgeOutboxStatus::Failed,None,Some(error),state.clock().now_unix_ms()).await.map_err(|e|e.to_string())?;continue;}
                                };
                                // Send 一旦交给管道就可能已外发；此后的失败只能登记未知结果。
                                inflight.insert(outbox.input.delivery_id.clone(),tokio::time::Instant::now());
                                worker.send(&command).await?;
                            }
                        }
                    }
                }
            }
            Ok(())
        }.await;
        media.clear();
        let mut failures = Vec::new();
        commands.abort_all();
        while let Some(result) = commands.join_next().await {
            if let Ok((id, result)) = result {
                command_ids.remove(&id);
                if let Err(error) = finish_command(state, &id, result).await {
                    failures.push(error);
                }
            }
        }
        for id in command_ids {
            if let Err(error) = store::settle_command(
                state.storage(),
                &id,
                store::BridgeCommandStatus::Unknown,
                None,
                Some("Bridge command stopped before confirmed settlement".into()),
                state.clock().now_unix_ms(),
            )
            .await
            {
                failures.push(error.to_string());
            }
        }
        if let Err(error) = crate::slash::revoke_bridge_takeovers(state, &connector.input.id) {
            failures.push(error);
        }

        if let Err(error) = cancel_connector_runs(state, connector).await {
            failures.push(error);
        }
        if let Err(error) = effects.shutdown(state, connector, &mut worker).await {
            failures.push(error);
        }
        for (delivery, _) in inflight {
            let started = store::get_outbox(state.storage(), &delivery)
                .await
                .map(|row| row.is_some_and(|row| row.status == store::BridgeOutboxStatus::Started));
            match started {
                Ok(false) => continue,
                Err(error) => {
                    failures.push(error.to_string());
                    continue;
                }
                Ok(true) => {}
            }
            if let Err(error) = store::settle_outbox(
                state.storage(),
                &delivery,
                store::BridgeOutboxStatus::Unknown,
                None,
                Some("worker stopped before a confirmed receipt".into()),
                state.clock().now_unix_ms(),
            )
            .await
            {
                failures.push(error.to_string());
            }
        }
        if let Err(error) = worker.stop().await {
            failures.push(error);
        }
        let result = match (result, failures.is_empty()) {
            (Ok(()), true) => Ok(()),
            (Err(error), true) => Err(error),
            (result, false) => Err(format!(
                "{}; cleanup: {}",
                result.err().unwrap_or_default(),
                failures.join("; ")
            )),
        };
        self.status(
            state,
            connector,
            if result.is_ok() {
                "disconnected"
            } else {
                "error"
            },
            result.as_ref().err().cloned(),
        );
        result
    }
}
async fn current(state: &ServiceState, cfg: &store::BridgeConnector) -> Result<bool, String> {
    let row = store::get_connector(state.storage(), &cfg.input.id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(
        row.is_some_and(|row| row.generation == cfg.generation && row.input.enabled)
            && !state
                .memory()
                .is_agent_deleted_checked(&cfg.input.actor_id)
                .map_err(|e| format!("{e:?}"))?
            && state
                .memory()
                .agent_dir_checked(&cfg.input.actor_id)
                .is_ok(),
    )
}
fn bridge_session_mode(state: &ServiceState) -> Result<&'static str, String> {
    let preferences = state.memory().read_preferences_root()?;
    Ok(crate::bridge_permission::parse(&preferences)?
        .session_mode()
        .wire_name())
}
async fn receive(
    state: &ServiceState,
    cfg: &store::BridgeConnector,
    frame: &Value,
    media: &mut BridgeMediaPipe,
) -> Result<Option<BridgeCommandJob>, String> {
    if !current(state, cfg).await? {
        return Err("Bridge connector authorization changed".into());
    }
    let attachments = serde_json::from_value(frame["attachments"].clone())
        .map_err(|_| "Bridge attachments malformed")?;
    let mut payload = json!({"text":frame["text"],"replyContext":frame["replyContext"]});
    for key in ["displayName", "avatarUrl"] {
        if let Some(value) = frame[key]
            .as_str()
            .filter(|s| s.len() <= 4096 && !s.contains('\0'))
        {
            payload[key] = json!(value);
        }
    }
    let input = store::BridgeInboxInput {
        platform: cfg.input.platform.clone(),
        connector_id: cfg.input.id.clone(),
        conversation_id: frame["conversationId"]
            .as_str()
            .ok_or("Bridge conversation missing")?
            .into(),
        conversation_kind: serde_json::from_value(frame["conversationKind"].clone())
            .map_err(|_| "Bridge conversation kind invalid")?,
        message_id: frame["messageId"]
            .as_str()
            .ok_or("Bridge message identity missing")?
            .into(),
        sender_id: frame["senderId"]
            .as_str()
            .ok_or("Bridge sender missing")?
            .into(),
        payload_json: payload.to_string(),
        attachments,
    };
    let claim = store::claim_inbox(
        state.storage(),
        &input,
        cfg.generation,
        state.clock().now_unix_ms(),
    )
    .await
    .map_err(|e| e.to_string())?;
    let inbox = claim.inbox;
    if claim.claimed {
        publish_message(
            state,
            &inbox,
            "in",
            frame["text"]
                .as_str()
                .ok_or("Bridge message text missing")?,
        );
    }
    if !claim.claimed && (inbox.prepared || inbox.status != store::BridgeInboxStatus::Claimed) {
        // 重投附件仍需释放已收字节，不创建另一个任务或重复回发。
        for attachment in &input.attachments {
            if let (Some(size), Some(mime)) = (attachment.size_bytes, attachment.mime.as_deref()) {
                let _ = media.take(
                    &attachment.attachment_id,
                    &input.message_id,
                    u64::from(size),
                    mime,
                );
            }
        }
        return Ok(None);
    }
    let sid = store::resolve_conversation(
        state.storage(),
        &inbox.id,
        cfg.generation,
        state.clock().now_unix_ms(),
    )
    .await
    .map_err(|e| e.to_string())?
    .session_id;
    let principal = crate::auth::Principal::bridge_actor(
        &input.platform,
        &input.connector_id,
        &input.sender_id,
    )?;
    if state
        .storage()
        .get_session(&sid)
        .await
        .map_err(|e| e.to_string())?
        .is_none()
    {
        match state
            .session_admin()
            .create_session(
                &sid,
                &inbox.actor_id,
                principal
                    .user_id
                    .as_deref()
                    .ok_or("Bridge subject missing")?,
                "Bridge",
                Some(bridge_session_mode(state)?),
                &[],
                inbox.role == lingxi_protocol::BridgeRole::Owner,
                state.clock().now_unix_ms(),
            )
            .await
        {
            Ok(()) => {}
            Err(crate::session_admin::SessionAdminError::SessionExists { .. }) => {}
            Err(error) => return Err(format!("Bridge session preparation failed: {error:?}")),
        }
    }
    let owner_dm = inbox.role == lingxi_protocol::BridgeRole::Owner
        && inbox.input.conversation_kind == lingxi_protocol::BridgeConversationKind::Dm;
    let text = payload["text"]
        .as_str()
        .ok_or("Bridge message text missing")?;
    let command = if owner_dm {
        state
            .slash
            .bridge_command_kind(&sid, text, state.clock().now_unix_ms())?
    } else {
        None
    };
    let takeover = if owner_dm && command.is_none() {
        crate::slash::resolve_takeover(state, &inbox, &sid).await?
    } else {
        None
    };
    let target = takeover.as_deref().unwrap_or(&sid);
    let mut files = Vec::new();
    for attachment in &input.attachments {
        let bytes = media.take(
            &attachment.attachment_id,
            &input.message_id,
            u64::from(
                attachment
                    .size_bytes
                    .ok_or("Bridge attachment size missing")?,
            ),
            attachment
                .mime
                .as_deref()
                .ok_or("Bridge attachment MIME missing")?,
        )?;
        let registered = state
            .session_files()
            .register_bridge_blob(
                target,
                Some(&bytes.filename),
                &bytes.mime,
                &bytes.bytes,
                "attachment",
            )
            .await
            .map_err(|e| e.to_string())?;
        files.push(store::BridgeInboxFile {
            attachment_id: attachment.attachment_id.clone(),
            file_id: registered.file_id,
        });
    }
    if takeover.is_some() {
        store::prepare_inbox_takeover(
            state.storage(),
            &inbox.id,
            cfg.generation,
            target,
            &files,
            state.clock().now_unix_ms(),
        )
        .await
        .map_err(|e| e.to_string())?;
    } else {
        store::prepare_inbox_session(
            state.storage(),
            &inbox.id,
            cfg.generation,
            &sid,
            &files,
            state.clock().now_unix_ms(),
        )
        .await
        .map_err(|e| e.to_string())?;
    }
    if inbox.role == lingxi_protocol::BridgeRole::Owner
        && inbox.input.conversation_kind == lingxi_protocol::BridgeConversationKind::Dm
    {
        let facts = state
            .sessions()
            .get_for(&principal, &sid)
            .await
            .map_err(|error| format!("Bridge registered session unavailable: {error:?}"))?;
        let facts = match facts {
            crate::sessions::SessionAccess::Ok(facts) => facts,
            _ => return Err("Bridge registered session unavailable or not owned".into()),
        };
        let key = format!(
            "bridge:{}",
            lingxi_kernel::session_tree::sha256_hex(
                json!([
                    cfg.input.id,
                    inbox.input.conversation_id,
                    inbox.input.sender_id
                ])
                .to_string()
                .as_bytes()
            )
        );
        state.slash.register_actual_bridge(&sid, &facts, &key)?;
    }

    if let Some(kind) = command {
        let claim = store::claim_command(
            state.storage(),
            &inbox.id,
            cfg.generation,
            kind,
            state.clock().now_unix_ms(),
        )
        .await
        .map_err(|e| e.to_string())?;
        if claim.claimed {
            let inbox = store::get_inbox(state.storage(), &inbox.id)
                .await
                .map_err(|e| e.to_string())?
                .ok_or("Bridge command inbox disappeared")?;
            return Ok(Some(BridgeCommandJob {
                inbox,
                source: sid,
                text: text.into(),
            }));
        }
    }
    Ok(None)
}
async fn advance_inbox(state: &ServiceState, cfg: &store::BridgeConnector) -> Result<(), String> {
    for inbox in store::list_pending_inbox(state.storage())
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|row| {
            row.input.connector_id == cfg.input.id && row.connector_generation == cfg.generation
        })
    {
        if let Some(run_id) = inbox.run_id.as_ref() {
            let run = state
                .storage()
                .load_run(&RunId::new(run_id.clone()))
                .await
                .map_err(|e| e.to_string())?
                .ok_or("Bridge run disappeared")?;
            if !run.status.is_terminal() {
                continue;
            }
            let status = match run.status {
                RunStatus::Completed => store::BridgeInboxStatus::Succeeded,
                RunStatus::Cancelled => store::BridgeInboxStatus::Cancelled,
                RunStatus::Failed => store::BridgeInboxStatus::Failed,
                _ => store::BridgeInboxStatus::Unknown,
            };
            let reply = final_reply(
                state,
                inbox
                    .session_id
                    .as_deref()
                    .ok_or("Bridge session link missing")?,
                run_id,
            )
            .await?;
            let text = reply
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| match run.status {
                    RunStatus::Cancelled => "本次任务已取消。".into(),
                    RunStatus::Failed => "本次任务执行失败，请在灵犀中查看详情。".into(),
                    RunStatus::Completed => {
                        "任务已结束，但没有可发送的回复内容，请在灵犀中查看详情。".into()
                    }
                    _ => "本次任务的结果暂时无法确认，请在灵犀中查看详情。".into(),
                });
            {
                let input = store::BridgeOutboxInput {
                    delivery_id: format!("reply:{}", inbox.id),
                    connector_id: cfg.input.id.clone(),
                    connector_generation: cfg.generation,
                    inbox_id: Some(inbox.id.clone()),
                    manual_action_id: None,
                    run_id: Some(run_id.clone()),
                    payload_json: json!({"text":text}).to_string(),
                    attachments: vec![],
                };
                store::enqueue_outbox(state.storage(), &input, state.clock().now_unix_ms())
                    .await
                    .map_err(|e| e.to_string())?;
            }
            store::settle_inbox(
                state.storage(),
                &inbox.id,
                status,
                Some(json!({"runId":run_id})),
                None,
                state.clock().now_unix_ms(),
            )
            .await
            .map_err(|e| e.to_string())?;
        } else if inbox.prepared {
            store::validate_inbox_claim(state.storage(), &inbox.id, cfg.generation)
                .await
                .map_err(|e| e.to_string())?;
            let principal = crate::auth::Principal::bridge_actor(
                &inbox.input.platform,
                &inbox.input.connector_id,
                &inbox.input.sender_id,
            )?;
            let sid = inbox
                .session_id
                .as_deref()
                .ok_or("prepared Bridge inbox has no session")?;
            let payload: Value = serde_json::from_str(&inbox.input.payload_json)
                .map_err(|_| "Bridge inbox payload damaged")?;
            let text = payload["text"]
                .as_str()
                .ok_or("Bridge input text missing")?;
            let mut files = Vec::new();
            let mut images = Vec::new();
            let mut text_refs = Vec::new();
            for file in &inbox.resolved_files {
                let row = state
                    .session_files()
                    .get_file(sid, &file.file_id)
                    .await
                    .map_err(|e| e.to_string())?
                    .ok_or("Bridge attachment no longer available")?;
                if row.mime.starts_with("image/") {
                    let snapshot = crate::file_safety::Snapshot::read_bounded(
                        std::path::Path::new(&row.real_path),
                        15 * 1024 * 1024,
                    )
                    .map_err(|_| "Bridge image bytes could not be safely read")?;
                    images.push(
                        crate::chat_wire::image_bytes(sid, &row.mime, snapshot.bytes)
                            .map_err(|_| "Bridge image bytes do not match their declared type")?,
                    );
                }
                if !row.mime.starts_with("image/") {
                    text_refs.push(json!({"fileId":row.file_id,"sessionId":sid}));
                } else {
                    files.push(serde_json::to_value(row).map_err(|e| e.to_string())?);
                }
            }
            let req: crate::chat_wire::PromptRequest =
                serde_json::from_value(json!({"sessionId":sid,"sessionFileRefs":text_refs}))
                    .map_err(|_| "Bridge file preparation input malformed")?;
            let (file_context, mut prepared_files) =
                crate::chat_wire::prepare_files(state, &req, &Arc::new(AtomicBool::new(false)))
                    .await
                    .map_err(|error| format!("Bridge attachment preparation failed: {error}"))?;
            files.append(&mut prepared_files);
            let chat = crate::runs::ChatRunInput {
                images,
                files,
                client_message_id: Some(inbox.input.message_id.clone()),
                snapshot_version: None,
                display_text: Some(text.into()),
                quoted_text: payload
                    .pointer("/replyContext/text")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                session_refs: vec![],
                options: Default::default(),
            };
            match state
                .sessions()
                .execute_bridge_for(
                    state.storage(),
                    state.events(),
                    state.runs(),
                    state.background(),
                    &principal,
                    sid,
                    text,
                    Some(&file_context),
                    &inbox.id,
                    &inbox.input.message_id,
                    inbox.role,
                    match bridge_session_mode(state)? {
                        "ask" => lingxi_kernel::subagent::SessionPermissionMode::Ask,
                        "operate" => lingxi_kernel::subagent::SessionPermissionMode::Operate,
                        _ => lingxi_kernel::subagent::SessionPermissionMode::ReadOnly,
                    },
                    Some(chat),
                    state.clock().now_unix_ms(),
                )
                .await
            {
                Ok(accepted) => {
                    if let Err(error) = store::bind_inbox(
                        state.storage(),
                        &inbox.id,
                        cfg.generation,
                        sid,
                        &accepted.run_id,
                        state.clock().now_unix_ms(),
                    )
                    .await
                    {
                        // 已接受的运行不会因绑定失败而失去回收责任。
                        let cleanup = cancel_connector_runs(state, cfg).await;
                        return Err(format!(
                            "Bridge accepted run binding failed: {error}; cleanup: {}",
                            cleanup.err().unwrap_or_else(|| "confirmed".into())
                        ));
                    }
                }
                Err(crate::sessions::SessionExecuteError::Busy) => {}
                Err(error) => return Err(format!("Bridge task admission failed: {error:?}")),
            }
        }
    }
    Ok(())
}
async fn final_reply(
    state: &ServiceState,
    session: &str,
    run: &str,
) -> Result<Option<String>, String> {
    let rows =
        lingxi_adapters::storage::bridge_reads::final_messages(state.storage(), session, run)
            .await
            .map_err(|e| e.to_string())?;
    let mut text = Vec::new();
    for raw in rows {
        let message: NormalizedMessage =
            serde_json::from_str(&raw).map_err(|_| "Bridge final message damaged")?;
        for block in crate::streaming_norm::normalize_final_message(message).content {
            if let ContentBlock::Text { text: part } = block {
                text.push(part);
            }
        }
    }
    Ok(if text.is_empty() {
        None
    } else {
        Some(crate::history::history_content::sanitize_bridge_visible_text(&text.join("\n")))
    })
}
async fn cancel_connector_runs(
    state: &ServiceState,
    cfg: &store::BridgeConnector,
) -> Result<(), String> {
    let mut active = Vec::new();
    let mut errors = Vec::new();
    for (id, run, root_run) in lingxi_adapters::storage::bridge_reads::active_causal_runs(
        state.storage(),
        &cfg.input.id,
        cfg.generation,
    )
    .await
    .map_err(|e| e.to_string())?
    {
        let inbox = store::get_inbox(state.storage(), &id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("Bridge causal inbox disappeared during cancellation")?;
        let (actual_root, _, _) = store::get_inbox_run(state.storage(), &id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("Bridge causal run disappeared during cancellation")?;
        if actual_root != root_run {
            return Err("Bridge causal root identity changed during cancellation".into());
        }
        let principal = crate::auth::principal_for_bridge_inbox(&inbox)?;
        match state
            .sessions()
            .cancel_run_for(state.storage().as_ref(), state.runs(), &principal, &run)
            .await
        {
            Ok(_) => {}
            Err(error) => {
                let terminal = state
                    .storage()
                    .load_run(&RunId::new(&run))
                    .await
                    .map_err(|e| e.to_string())?
                    .is_some_and(|r| r.status.is_terminal());
                if !terminal {
                    return Err(format!("Bridge cancellation failed: {error:?}"));
                }
            }
        }
        active.push((inbox, run, root_run));
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !active.is_empty() {
        let mut remaining = Vec::new();
        for (inbox, run, root_run) in active {
            let row = state
                .storage()
                .load_run(&RunId::new(&run))
                .await
                .map_err(|e| e.to_string())?
                .ok_or("Bridge causal run disappeared during cleanup")?;
            if !row.status.is_terminal() {
                remaining.push((inbox, run, root_run));
            } else if run == root_run
                && matches!(
                    inbox.status,
                    store::BridgeInboxStatus::Claimed | store::BridgeInboxStatus::Running
                )
            {
                let status = match row.status {
                    RunStatus::Completed => store::BridgeInboxStatus::Succeeded,
                    RunStatus::Cancelled => store::BridgeInboxStatus::Cancelled,
                    RunStatus::Failed => store::BridgeInboxStatus::Failed,
                    _ => store::BridgeInboxStatus::Unknown,
                };
                store::settle_inbox(
                    state.storage(),
                    &inbox.id,
                    status,
                    Some(json!({"runId":run})),
                    Some("connector stopped; no new reply was sent".into()),
                    state.clock().now_unix_ms(),
                )
                .await
                .map_err(|e| e.to_string())?;
            }
        }
        if remaining.is_empty() {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            for (inbox, run, root_run) in remaining {
                if run == root_run
                    && matches!(
                        inbox.status,
                        store::BridgeInboxStatus::Claimed | store::BridgeInboxStatus::Running
                    )
                {
                    store::settle_inbox(
                        state.storage(),
                        &inbox.id,
                        store::BridgeInboxStatus::Unknown,
                        None,
                        Some("Bridge cancellation cleanup remains unconfirmed".into()),
                        state.clock().now_unix_ms(),
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                }
                errors.push(format!("{}:{run}", inbox.id));
            }
            break;
        }
        active = remaining;
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Bridge Run cleanup unconfirmed: {}",
            errors.join(", ")
        ))
    }
}

async fn finish_command(
    state: &ServiceState,
    id: &str,
    result: Result<Value, String>,
) -> Result<(), String> {
    let (status, result, error) = match result {
        Ok(value) => (store::BridgeCommandStatus::Confirmed, Some(value), None),
        Err(error) => (store::BridgeCommandStatus::Unknown, None, Some(error)),
    };
    store::settle_command(
        state.storage(),
        id,
        status,
        result,
        error,
        state.clock().now_unix_ms(),
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}
async fn queue_command_replies(
    state: &ServiceState,
    cfg: &store::BridgeConnector,
) -> Result<(), String> {
    for command in store::list_pending_command_replies(state.storage())
        .await
        .map_err(|e| e.to_string())?
    {
        let inbox = store::get_inbox(state.storage(), &command.inbox_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("Bridge command inbox missing")?;
        if inbox.input.connector_id != cfg.input.id || inbox.connector_generation != cfg.generation
        {
            continue;
        }
        let result = command
            .result
            .as_ref()
            .ok_or("Bridge command result missing")?;
        let text = result["text"]
            .as_str()
            .ok_or("Bridge command reply text missing")?;
        let input = store::BridgeOutboxInput {
            delivery_id: format!("command:{}", inbox.id),
            connector_id: cfg.input.id.clone(),
            connector_generation: cfg.generation,
            inbox_id: Some(inbox.id),
            manual_action_id: None,
            run_id: None,
            payload_json: json!({"text":text}).to_string(),
            attachments: vec![],
        };
        store::enqueue_outbox(state.storage(), &input, state.clock().now_unix_ms())
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

async fn prepare_outbound_media(
    state: &ServiceState,
    cfg: &store::BridgeConnector,
    generation: u32,
    outbox: &store::BridgeOutbox,
    worker: &mut BridgeWorkerProcess,
) -> Result<Vec<String>, String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let Some(id) = outbox.input.manual_action_id.as_deref() else {
        if !outbox.input.attachments.is_empty() {
            return Err("Bridge outbound attachment has no verified source".into());
        }
        return Ok(Vec::new());
    };
    let action = store::get_manual_action(state.storage(), id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Bridge manual file source missing")?;
    let _lease = state.session_files().payload_lease().await;
    let file = state
        .session_files()
        .get_file(&action.input.source_session_id, &action.input.file_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Bridge manual file is unavailable")?;
    let path = std::path::PathBuf::from(&file.real_path);
    let bytes = tokio::task::spawn_blocking(move || {
        crate::bridge_session_routes::read_bridge_file(
            &path,
            crate::bridgemedia::MAX_BRIDGE_MEDIA_SIZE,
        )
    })
    .await
    .map_err(|_| "Bridge file read task failed")??;
    if bytes.len() as u64 != action.size_bytes
        || lingxi_kernel::session_tree::sha256_hex(&bytes) != action.input.content_sha256
    {
        return Err("Bridge file content changed before sending".into());
    }
    let attachment = outbox
        .input
        .attachments
        .first()
        .ok_or("Bridge manual attachment metadata missing")?;
    if !current(state, cfg).await? {
        return Err("Bridge connector revoked before file transfer".into());
    }
    let frame = BridgeWorkerCommand::MediaBegin {
        connector_id: cfg.input.id.clone(),
        generation,
        attachment_id: attachment.attachment_id.clone(),
        message_id: None,
        size_bytes: u32::try_from(bytes.len()).map_err(|_| "Bridge file size exceeds protocol")?,
        mime: Some(file.mime),
        filename: Some(file.filename),
    };
    worker
        .send(&serde_json::to_value(frame).map_err(|e| e.to_string())?)
        .await?;
    for (seq, chunk) in bytes
        .chunks(crate::bridge_media_pipe::MAX_CHUNK_BYTES)
        .enumerate()
    {
        if !current(state, cfg).await? {
            return Err("Bridge connector revoked during file transfer".into());
        }
        let frame = BridgeWorkerCommand::MediaChunk {
            connector_id: cfg.input.id.clone(),
            generation,
            attachment_id: attachment.attachment_id.clone(),
            seq: u32::try_from(seq).map_err(|_| "Bridge media sequence overflow")?,
            data: STANDARD.encode(chunk),
        };
        worker
            .send(&serde_json::to_value(frame).map_err(|e| e.to_string())?)
            .await?;
    }
    worker
        .send(
            &serde_json::to_value(BridgeWorkerCommand::MediaEnd {
                connector_id: cfg.input.id.clone(),
                generation,
                attachment_id: attachment.attachment_id.clone(),
            })
            .map_err(|e| e.to_string())?,
        )
        .await?;
    Ok(vec![attachment.attachment_id.clone()])
}

async fn record_notice(state: &ServiceState, delivery: &str) {
    if let Err(error) = crate::model_history::record_delivered_bridge_notice(
        state.storage(),
        delivery,
        state.clock().now_unix_ms(),
    )
    .await
    {
        // 真实外发收据不回滚；上下文失败单独可见，重启仍可按相同投递编号补齐。
        tracing::error!(delivery,error=%error,"delivered Bridge notice context could not be recorded");
        state.app_events().publish(
            "bridge_context_error",
            json!({"deliveryId":delivery,"error":"已确认送达，但通知上下文尚未保存"}),
        );
    }
}
fn publish_message(state: &ServiceState, inbox: &store::BridgeInbox, direction: &str, text: &str) {
    let key = json!([
        inbox.input.connector_id,
        inbox.input.conversation_id,
        inbox.input.sender_id,
        inbox.actor_id
    ]);
    let key = format!(
        "bridge-conversation-{}",
        lingxi_protocol::canon::sha256_hex(&lingxi_protocol::canon::canonical_json_bytes(&key))
    );
    state.app_events().publish("bridge_message",json!({"message":{"platform":inbox.input.platform,"sessionKey":key,"direction":direction,"sender":inbox.input.sender_id,"text":text,"isGroup":inbox.input.conversation_kind==lingxi_protocol::BridgeConversationKind::Group,"ts":state.clock().now_unix_ms(),"agentId":inbox.actor_id}}));
}

#[cfg(test)]
#[path = "bridge_revoke_tests.rs"]
mod revoke_tests;

#[cfg(test)]
#[path = "bridge_child_revoke_tests.rs"]
mod child_revoke_tests;

pub(crate) async fn confirmed_delivery(
    state: &ServiceState,
    delivered: &store::BridgeOutbox,
) -> Result<(), String> {
    record_notice(state, &delivered.input.delivery_id).await;
    let inbox = store::get_inbox(state.storage(), &delivered.target_inbox_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Bridge delivery target disappeared")?;
    let payload: Value = serde_json::from_str(&delivered.input.payload_json)
        .map_err(|_| "Bridge delivery payload damaged")?;
    let text = payload["text"]
        .as_str()
        .ok_or("Bridge delivered text missing")?;
    publish_message(state, &inbox, "out", text);
    Ok(())
}
