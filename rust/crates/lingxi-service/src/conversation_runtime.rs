//! 手机后台复用正式运行链，宿主只负责投递、核对决策与有界传播。
use crate::{
    conversations::{digest, members, timestamp, ConversationApi, ConversationError},
    ServiceState,
};
use lingxi_adapters::{
    conversation_files::ConversationMutation,
    storage::{conversations as store, session_tree},
};
use lingxi_kernel::{
    ports::StoragePort,
    subagent::{RunLineage, RunOrigin},
};
use lingxi_protocol::RunId;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

pub struct ConversationHost {
    stop: tokio::sync::watch::Sender<bool>,
    wake: tokio::sync::Notify,
    task: Mutex<Option<tokio::task::JoinHandle<Result<(), String>>>>,
    tick_lock: tokio::sync::Mutex<()>,
    pub(crate) reset_lock: Arc<tokio::sync::Mutex<()>>,
    next_discovery: std::sync::atomic::AtomicU64,
    join_lock: tokio::sync::Mutex<()>,
    error: Mutex<Option<String>>,
    shutdown_error: Mutex<Option<String>>,
    /// 启动恢复结果：None=尚未完成；服务就绪前必须等到 Some。
    recovery: tokio::sync::watch::Sender<Option<Result<(), String>>>,
}
impl Default for ConversationHost {
    fn default() -> Self {
        Self {
            stop: tokio::sync::watch::channel(false).0,
            wake: tokio::sync::Notify::new(),
            task: Mutex::new(None),
            tick_lock: tokio::sync::Mutex::new(()),
            reset_lock: Arc::new(tokio::sync::Mutex::new(())),
            next_discovery: std::sync::atomic::AtomicU64::new(0),
            join_lock: tokio::sync::Mutex::new(()),
            error: Mutex::new(None),
            shutdown_error: Mutex::new(None),
            recovery: tokio::sync::watch::channel(None).0,
        }
    }
}
impl ConversationHost {
    pub fn wake(&self) {
        self.wake.notify_one();
    }
    pub fn status(&self) -> Value {
        json!({"running":self.task.lock().expect("手机宿主锁").as_ref().is_some_and(|h|!h.is_finished()),"lastError":self.error.lock().expect("手机状态锁").clone()})
    }
    pub fn start(self: &Arc<Self>, state: ServiceState) {
        let mut task = self.task.lock().expect("手机宿主锁");
        if task.is_some() {
            return;
        }
        *self.shutdown_error.lock().expect("手机退出状态锁") = None;
        self.stop.send_replace(false);
        let host = self.clone();
        *task = Some(tokio::spawn(async move {
            let result:Result<(),String>=async{
  let mut stop=host.stop.subscribe();
  // 恢复阶段整体成败先发布给 wait_recovery（服务就绪门），再决定循环去留。
  let recovery=async{
   let files=state.conversation_files().recover(state.storage(),state.clock().now_unix_ms()).await.map_err(|e|format!("conversation recover files: {e}"))?;
   for receipt in files{if receipt.status!=lingxi_adapters::conversation_files::FileReceiptStatus::Applied{tracing::warn!(operation=%receipt.operation_id,status=?receipt.status,"重启发现未确认的对话文件动作");}}
   store::recover_deliveries(state.storage(),state.clock().now_unix_ms()).await.map_err(|e|format!("conversation recover_deliveries: {e}"))?;
   crate::session_collab::recover(&state).await.map_err(|e|format!("conversation session_collab recover: {e}"))?;
   Ok::<(),String>(())
  }.await;
  host.recovery.send_replace(Some(recovery.clone()));
  recovery?;
  let mut ticks=tokio::time::interval(std::time::Duration::from_millis(500));ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
  loop{if *stop.borrow(){break;}tokio::select!{biased;_=stop.wait_for(|v|*v)=>break,_=host.wake.notified()=>{},_=ticks.tick()=>{}}
   if *stop.borrow(){break;}
if let Err(e)=host.tick(&state).await{tracing::error!(error=%e,"手机后台检查失败");*host.error.lock().expect("手机状态锁")=Some(e.to_string());}
  }
  ConversationApi::from_state(&state).cancel_scope(None,None).await.map_err(|e|e.to_string())?;
  crate::session_collab::shutdown(&state).await.map_err(|e|e.to_string())?;Ok(())
  }.await;
            if let Err(e) = &result {
                *host.error.lock().expect("手机状态锁") = Some(e.clone());
            }
            result
        }));
    }
    pub fn stop(&self) {
        self.stop.send_replace(true);
        self.wake();
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
                return Err("conversation host died before publishing its recovery outcome".into());
            }
        }
    }
    pub async fn wait_stopped(&self) -> Result<(), String> {
        let _join = self.join_lock.lock().await;
        let result = std::future::poll_fn(|cx| {
            let mut task = self.task.lock().expect("手机宿主锁");
            let Some(h) = task.as_mut() else {
                return std::task::Poll::Ready(Ok(Ok(())));
            };
            match std::future::Future::poll(std::pin::Pin::new(h), cx) {
                std::task::Poll::Ready(r) => {
                    *task = None;
                    std::task::Poll::Ready(r)
                }
                std::task::Poll::Pending => std::task::Poll::Pending,
            }
        })
        .await;
        let result = result.map_err(|e| e.to_string()).and_then(|r| r);
        if let Err(e) = &result {
            *self.shutdown_error.lock().expect("手机退出状态锁") = Some(e.clone());
            *self.error.lock().expect("手机状态锁") = Some(e.clone());
        }
        if let Some(e) = self.shutdown_error.lock().expect("手机退出状态锁").clone() {
            Err(e)
        } else {
            Ok(())
        }
    }
    pub async fn tick(&self, state: &ServiceState) -> Result<(), ConversationError> {
        let _tick = self.tick_lock.lock().await;
        crate::session_collab::tick(state)
            .await
            .map_err(ConversationError::from)?;
        let api = ConversationApi::from_state(state);
        if !api.enabled()? {
            api.cancel_scope(None, None).await?;
            return Ok(());
        }
        for d in store::unpublished_decisions(state.storage()).await? {
            let current = match api.delivery_current(&d) {
                Ok(current) => current,
                Err(e) => {
                    *self.error.lock().expect("手机状态锁") = Some(e.to_string());
                    tracing::error!(delivery=%d.input.delivery_id,error=%e,"待发布回复权限无法核实");
                    false
                }
            };
            if d.status != store::PhoneDeliveryStatus::Cancelled && current {
                if let Err(e) = api.publish_decision(&d, store::PhoneDecision::Reply).await {
                    *self.error.lock().expect("手机状态锁") = Some(e.to_string());
                    tracing::error!(delivery=%d.input.delivery_id,error=%e,"待发布回复未完成");
                }
            }
        }
        for d in store::list_pending_deliveries(state.storage()).await? {
            let actual = store::get_delivery_run(state.storage(), &d.input.delivery_id).await?;
            if let Some((run, sid, status)) = actual {
                if d.run_id.is_none() {
                    store::bind_delivery_run(
                        state.storage(),
                        &d.input.delivery_id,
                        &sid,
                        &run,
                        state.clock().now_unix_ms(),
                    )
                    .await?;
                }
                let current = match api.delivery_current(&d) {
                    Ok(current) => current,
                    Err(e) => {
                        tracing::error!(delivery=%d.input.delivery_id,error=%e,"手机权限事实无法核实");
                        false
                    }
                };
                if !current {
                    state
                        .runs()
                        .cancel_run(&run, "conversation authority revoked or unavailable");
                }
                if current {
                    if let Some(decision) = d.decision {
                        if let Err(e) = api.publish_decision(&d, decision).await {
                            *self.error.lock().expect("手机状态锁") = Some(e.to_string());
                            tracing::error!(delivery=%d.input.delivery_id,error=%e,"本次回复未完成");
                        }
                    }
                }
                let terminal = match status.as_str() {
                    "completed" => Some(store::PhoneDeliveryStatus::Completed),
                    "failed" => Some(store::PhoneDeliveryStatus::Failed),
                    "cancelled" => Some(store::PhoneDeliveryStatus::Cancelled),
                    "interrupted_needs_attention" => Some(store::PhoneDeliveryStatus::Unknown),
                    _ => None,
                };
                if let Some(status) = terminal {
                    store::settle_delivery(
                        state.storage(),
                        &d.input.delivery_id,
                        status,
                        None,
                        state.clock().now_unix_ms(),
                    )
                    .await?;
                    if current {
                        if status != store::PhoneDeliveryStatus::Cancelled {
                            if let Err(e) = api.bookmark_delivery(&d).await {
                                *self.error.lock().expect("手机状态锁") = Some(e.to_string());
                                tracing::error!(error=%e,"手机书签未完成");
                            }
                        }
                        if let Err(e) = api
                            .activity(
                                &d,
                                if status == store::PhoneDeliveryStatus::Cancelled {
                                    "cancelled"
                                } else if d.decision == Some(store::PhoneDecision::Reply) {
                                    "replied"
                                } else if status == store::PhoneDeliveryStatus::Completed {
                                    "passed"
                                } else {
                                    "error"
                                },
                                &format!("运行结果：{}", status_name(status)),
                            )
                            .await
                        {
                            *self.error.lock().expect("手机状态锁") = Some(e.to_string());
                            tracing::error!(error=%e,"手机动态未完成");
                        }
                    }
                }
                continue;
            }
            let current = match api.delivery_current(&d) {
                Ok(current) => current,
                Err(e) => {
                    tracing::error!(delivery=%d.input.delivery_id,error=%e,"排队手机任务权限事实无法核实");
                    false
                }
            };
            if !current {
                store::settle_delivery(
                    state.storage(),
                    &d.input.delivery_id,
                    store::PhoneDeliveryStatus::Failed,
                    Some("conversation authority changed before admission".into()),
                    state.clock().now_unix_ms(),
                )
                .await?;
                continue;
            }
            let Some(sid) = d.input.input_json["sessionId"].as_str() else {
                store::settle_delivery(
                    state.storage(),
                    &d.input.delivery_id,
                    store::PhoneDeliveryStatus::Failed,
                    Some("phone session missing".into()),
                    state.clock().now_unix_ms(),
                )
                .await?;
                continue;
            };
            if let Err(e) = prepare_phone_session(state, &d).await {
                store::settle_delivery(
                    state.storage(),
                    &d.input.delivery_id,
                    store::PhoneDeliveryStatus::Failed,
                    Some(format!("phone preparation: {e}")),
                    state.clock().now_unix_ms(),
                )
                .await?;
                *self.error.lock().expect("手机状态锁") = Some(e.to_string());
                if let Err(error) = api
                    .activity(&d, "error", &format!("无法准备本次对话：{e}"))
                    .await
                {
                    tracing::error!(error=%error,"手机准备失败动态未完成");
                }
                continue;
            }
            match state
                .sessions()
                .execute_phone_for(
                    state.storage(),
                    state.events(),
                    state.runs(),
                    state.background(),
                    &state.auth().scheduler_principal(),
                    sid,
                    &d.input.delivery_id,
                    state.clock().now_unix_ms(),
                )
                .await
            {
                Ok(accepted) => {
                    let bound = store::bind_delivery_run(
                        state.storage(),
                        &d.input.delivery_id,
                        sid,
                        &accepted.run_id,
                        state.clock().now_unix_ms(),
                    )
                    .await?;
                    api.activity(&bound, "running", "正在查看对话").await?;
                }
                Err(crate::sessions::SessionExecuteError::Busy) => {}
                Err(e) => {
                    api.bookmark_delivery(&d).await?;
                    store::settle_delivery(
                        state.storage(),
                        &d.input.delivery_id,
                        store::PhoneDeliveryStatus::Failed,
                        Some(format!("phone admission: {e:?}")),
                        state.clock().now_unix_ms(),
                    )
                    .await?;
                }
            }
        }
        api.discover(state).await?;
        let now = state.clock().now_unix_ms();
        if now
            >= self
                .next_discovery
                .load(std::sync::atomic::Ordering::Acquire)
        {
            self.next_discovery.store(
                now.saturating_add(60_000),
                std::sync::atomic::Ordering::Release,
            );
            api.discover_legacy_and_proactive().await?;
        }
        Ok(())
    }
}
pub(crate) fn phone_reply_text(raw: &str) -> (String, bool) {
    let pattern = regex::Regex::new(r"(?i)<done\s*/?>").expect("固定结束标记");
    let stopped = pattern.is_match(raw) || raw.trim() == "NO_REPLY";
    let body = if raw.trim() == "NO_REPLY" {
        String::new()
    } else {
        pattern.replace_all(raw, "").trim().to_string()
    };
    (body, stopped)
}

fn status_name(status: store::PhoneDeliveryStatus) -> &'static str {
    match status {
        store::PhoneDeliveryStatus::Completed => "completed",
        store::PhoneDeliveryStatus::Failed => "failed",
        store::PhoneDeliveryStatus::Cancelled => "cancelled",
        store::PhoneDeliveryStatus::Unknown => "unknown",
        _ => "active",
    }
}
impl ConversationApi {
    pub(crate) async fn source_delivery(
        &self,
        source: Option<&str>,
    ) -> Result<Option<store::PhoneDelivery>, ConversationError> {
        let Some(source) = source else {
            return Ok(None);
        };
        let mut run = RunId::new(source);
        let mut seen = std::collections::BTreeSet::new();
        loop {
            if !seen.insert(run.to_string()) || seen.len() > 64 {
                return Err(ConversationError::new(409, "invalid phone causal lineage"));
            }
            let lineage = self
                .storage
                .load_run_lineage(&run)
                .await?
                .ok_or_else(|| ConversationError::new(409, "source lineage missing"))?;
            if matches!(lineage.origin, RunOrigin::Channel | RunOrigin::Dm) {
                return Ok(Some(
                    store::get_delivery(
                        &self.storage,
                        lineage.cause_id.as_deref().ok_or_else(|| {
                            ConversationError::new(409, "source phone cause missing")
                        })?,
                    )
                    .await?
                    .ok_or_else(|| ConversationError::new(409, "source phone delivery missing"))?,
                ));
            }
            if !matches!(
                lineage.origin,
                RunOrigin::Subagent | RunOrigin::SessionCollab
            ) {
                return Ok(None);
            }
            run = lineage
                .parent_run_id
                .ok_or_else(|| ConversationError::new(409, "source parent missing"))?;
        }
    }
    pub async fn enqueue_message(
        &self,
        conversation: &str,
        from: &str,
        cause: &str,
        source_run: Option<&str>,
    ) -> Result<(), ConversationError> {
        self.enqueue_message_status(conversation, from, cause, source_run)
            .await
            .map(|_| ())
    }
    async fn enqueue_message_status(
        &self,
        conversation: &str,
        from: &str,
        cause: &str,
        source_run: Option<&str>,
    ) -> Result<bool, ConversationError> {
        let mut complete = true;
        if !self.enabled()? {
            return Err(ConversationError::new(503, "Agent phone is disabled"));
        }
        let parent = self.source_delivery(source_run).await?;
        let targets = if let Some(to) = conversation.strip_prefix("dm:") {
            self.actor(Some(from))?;
            vec![(self.actor(Some(to))?, format!("dm:{from}"))]
        } else {
            let d = self.files.read_channel(conversation)?;
            let mut roster = members(&d)?;
            roster.retain(|m| m != from);
            let body = d.messages.last().map(|m| m.body.as_str()).unwrap_or("");
            roster.sort_by_key(|id| (!body.contains(&format!("@{id}")), id.clone()));
            roster
                .into_iter()
                .map(|a| (a, conversation.into()))
                .collect()
        };
        for (actor, conversation) in targets {
            let id = format!("phone-{}", digest(json!([cause, actor, conversation])));
            if store::get_delivery(&self.storage, &id).await?.is_some() {
                continue;
            }
            let dm = conversation.starts_with("dm:");
            let d = if dm {
                self.visible_dm(&actor, from, self.files.read_dm(&actor, from)?)?
            } else {
                self.require_member(&conversation, &actor)?
            };
            let settings = self.settings(&actor, &conversation)?;
            let membership = if dm {
                let p = self.phone_document(&actor, &conversation)?;
                crate::conversations::dm_scope_digest(&actor, from, &p.meta)
            } else {
                digest(json!(members(&d)?))
            };
            let root = parent
                .as_ref()
                .map(|d| d.input.root_chain_id.clone())
                .unwrap_or_else(|| {
                    format!(
                        "phone-chain-{}",
                        digest(json!([cause, if dm { "dm" } else { "channel" }]))
                    )
                });
            let budget = parent
                .as_ref()
                .map(|d| d.input.budget_limit)
                .unwrap_or(if dm { 3 } else { settings.guard_limit });
            let now = self.clock.now_unix_ms();
            let old = store::get_phone_binding(
                &self.storage,
                &actor,
                &conversation,
                lingxi_kernel::LOCAL_OWNER_SUBJECT,
            )
            .await?;
            let epoch = store::phone_binding_epoch(
                &self.storage,
                &actor,
                &conversation,
                lingxi_kernel::LOCAL_OWNER_SUBJECT,
            )
            .await?;
            let reusable = if let Some(binding) = old.as_ref() {
                binding.expires_at > now
                    && binding.input.mode == settings.permission().wire_name()
                    && session_tree::get_session_row(&self.storage, &binding.input.session_id)
                        .await?
                        .is_some_and(|row| {
                            row.lifecycle == "active"
                                && row.agent_id == actor
                                && row.owner_user_id == lingxi_kernel::LOCAL_OWNER_SUBJECT
                        })
            } else {
                false
            };
            let sid = if reusable {
                old.as_ref().expect("绑定已核实").input.session_id.clone()
            } else {
                format!(
                    "phone-session-{}",
                    digest(json!([actor, conversation, epoch]))
                )
            };
            // 创建会话在正式宿主中完成；投递事实先固定其身份，受理前必须核对真实会话。
            let boundary = if dm {
                self.phone_document(&actor, &conversation)?.meta["lastViewedTimestamp"]
                    .as_str()
                    .map(String::from)
            } else {
                self.files
                    .read_bookmarks(&actor)?
                    .get(&conversation)
                    .cloned()
            };
            let mut unread = d
                .messages
                .iter()
                .filter(|m| boundary.as_ref().is_none_or(|b| m.timestamp > *b))
                .collect::<Vec<_>>();
            // 旧书签精度为秒；正式新动作仍须保留最新消息，不能因同秒时间被漏掉。
            if unread.is_empty() {
                if let Some(last) = d.messages.last() {
                    unread.push(last);
                }
            }
            let total = unread.len();
            let mut tail = unread
                .into_iter()
                .rev()
                .take(20)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .map(|m| json!({"sender":m.sender,"timestamp":m.timestamp,"body":m.body}))
                .collect::<Vec<_>>();
            while tail.len() > 1 && serde_json::to_vec(&tail).expect("消息序列").len() > 400 * 1024
            {
                tail.remove(0);
            }

            let prompt=format!("[助手手机对话]\n对话：{conversation}。只通过 channel_reply 发表一次回复，或 channel_pass 明确跳过。普通结尾和内省只记私人动态，不会发送到对话。禁止重新委派同一消息；本传播链总预算为{budget}。\n回复字数建议：{:?}到{:?}，只作为写作提示。\n未读总数{}，展示最后{}条，省略{}条。\n{}",settings.reply_min_chars,settings.reply_max_chars,total,tail.len(),total.saturating_sub(tail.len()),serde_json::to_string(&tail).expect("消息列表"));
            let input = store::PhoneDeliveryInput {
                delivery_id: id,
                cause_id: cause.into(),
                actor_id: actor,
                conversation_id: conversation,
                conversation_type: if dm { "dm" } else { "channel" }.into(),
                batch_digest: digest(json!(tail)),
                root_chain_id: root,
                budget_limit: budget,
                dm_direction: if dm {
                    Some((from.into(), d.meta["peer"].as_str().unwrap_or("").into()))
                } else {
                    None
                },
                source_run_id: source_run.map(String::from),
                source_message_id: cause.into(),
                membership_digest: membership,
                owner_subject: lingxi_kernel::LOCAL_OWNER_SUBJECT.into(),
                input_json: json!({"prompt":prompt,"sessionId":sid,"settings":settings,"bookmark":d.messages.last().map(|m|&m.timestamp),"messageCount":d.messages.len(),"bindingBaseEpoch":epoch}),
            };
            let mut input = input;
            if dm {
                input.dm_direction = Some((from.into(), input.actor_id.clone()));
            }
            let claim = store::claim_delivery(&self.storage, &input, now).await?;
            if claim.delivery.is_none()
                && claim.reason.as_deref() != Some("root_chain_budget_exhausted")
            {
                complete = false;
            }
            if claim.reason.as_deref() == Some("root_chain_budget_exhausted") {
                tracing::info!(chain=%input.root_chain_id,"手机循环达到预算，停止传播");
            }
        }
        self.wake();
        Ok(complete)
    }
    pub(crate) fn delivery_current(
        &self,
        d: &store::PhoneDelivery,
    ) -> Result<bool, ConversationError> {
        if !self.enabled()? {
            return Ok(false);
        }
        if self.actor(Some(&d.input.actor_id)).is_err() {
            return Ok(false);
        }
        let settings = self.settings(&d.input.actor_id, &d.input.conversation_id)?;
        if settings.mode == "read_only" && d.input.input_json["settings"]["mode"] == "write" {
            return Ok(false);
        }

        if d.input.conversation_type == "channel" {
            let doc = match self.require_member(&d.input.conversation_id, &d.input.actor_id) {
                Ok(d) => d,
                Err(e) if matches!(e.status, 403 | 404) => return Ok(false),
                Err(e) => return Err(e),
            };
            Ok(digest(json!(members(&doc)?)) == d.input.membership_digest)
        } else {
            let peer = d
                .input
                .conversation_id
                .strip_prefix("dm:")
                .ok_or_else(|| ConversationError::new(409, "bad DM identity"))?;
            self.actor(Some(peer))?;
            let p = self.phone_document(&d.input.actor_id, &d.input.conversation_id)?;
            Ok(
                crate::conversations::dm_scope_digest(&d.input.actor_id, peer, &p.meta)
                    == d.input.membership_digest,
            )
        }
    }
    pub(crate) async fn publish_decision(
        &self,
        d: &store::PhoneDelivery,
        decision: store::PhoneDecision,
    ) -> Result<(), ConversationError> {
        if decision == store::PhoneDecision::Pass {
            return Ok(());
        }
        if !self.delivery_current(d)? {
            return Err(ConversationError::new(
                403,
                "phone publication authority revoked",
            ));
        }
        let raw = d
            .decision_body
            .as_deref()
            .ok_or_else(|| ConversationError::new(409, "reply body missing"))?;
        let (body, stopped) = phone_reply_text(raw);
        if body.is_empty() {
            return Ok(());
        }
        let stamp = timestamp(
            d.decision_at
                .ok_or_else(|| ConversationError::new(409, "phone decision timestamp missing"))?,
        )?;
        let op = format!("phone-reply-{}", d.input.delivery_id);
        let was_applied = store::get_operation(&self.storage, &op)
            .await?
            .is_some_and(|r| {
                r.status == lingxi_adapters::conversation_files::FileReceiptStatus::Applied
            });
        let mutation = if let Some(peer) = d.input.conversation_id.strip_prefix("dm:") {
            ConversationMutation::AppendDm {
                from: d.input.actor_id.clone(),
                to: peer.into(),
                body: body.clone(),
                timestamp: stamp.clone(),
            }
        } else {
            ConversationMutation::AppendChannel {
                id: d.input.conversation_id.clone(),
                actor: Some(d.input.actor_id.clone()),
                sender: d.input.actor_id.clone(),
                body: body.clone(),
                timestamp: stamp.clone(),
            }
        };
        self.apply(&op, &mutation).await?;
        if !was_applied {
            if let Some(peer) = d.input.conversation_id.strip_prefix("dm:") {
                self.events
                    .publish("dm_new_message", json!({"from":d.input.actor_id,"to":peer}));
            } else {
                self.events.publish("channel_new_message",json!({"channelName":d.input.conversation_id,"message":{"sender":d.input.actor_id,"timestamp":stamp,"body":body}}));
            }
        }
        // 显式结束标识仍留下已发送事实，但不再唤醒下一轮。
        if !stopped {
            self.enqueue_message(
                &d.input.conversation_id,
                &d.input.actor_id,
                &op,
                d.run_id.as_deref(),
            )
            .await?;
        }
        Ok(())
    }
    pub(crate) async fn bookmark_delivery(
        &self,
        d: &store::PhoneDelivery,
    ) -> Result<(), ConversationError> {
        if d.input.conversation_type == "channel" {
            self.apply(
                &format!("phone-bookmark-{}", d.input.delivery_id),
                &ConversationMutation::Bookmark {
                    actor: d.input.actor_id.clone(),
                    channel: d.input.conversation_id.clone(),
                    timestamp: d.input.input_json["bookmark"].as_str().map(String::from),
                },
            )
            .await?;
        }
        self.apply(&format!("phone-viewed-{}",d.input.delivery_id),&ConversationMutation::PhonePatch{actor:d.input.actor_id.clone(),conversation:d.input.conversation_id.clone(),conversation_type:d.input.conversation_type.clone(),patch:json!({"lastViewedTimestamp":d.input.input_json["bookmark"],"lastViewedMessageCount":d.input.input_json["messageCount"]}),timestamp:timestamp(d.created_at)?}).await?;
        Ok(())
    }

    pub(crate) async fn activity(
        &self,
        d: &store::PhoneDelivery,
        state: &str,
        summary: &str,
    ) -> Result<(), ConversationError> {
        let stamp = chrono::DateTime::from_timestamp_millis(self.clock.now_unix_ms() as i64)
            .ok_or_else(|| ConversationError::new(400, "clock out of range"))?
            .to_rfc3339();
        self.apply(&format!("phone-activity-{}-{state}",d.input.delivery_id),&ConversationMutation::PhoneActivity{actor:d.input.actor_id.clone(),conversation:d.input.conversation_id.clone(),conversation_type:d.input.conversation_type.clone(),state:state.into(),summary:summary.into(),details:Some(json!({"runId":d.run_id,"deliveryId":d.input.delivery_id,"sessionId":d.session_id,"sessionPath":d.session_id.as_ref().map(|sid|format!("native:{sid}"))})),timestamp:stamp.clone()}).await?;
        self.events.publish("conversation_agent_activity",json!({"activity":{"conversationId":d.input.conversation_id,"agentId":d.input.actor_id,"timestamp":stamp,"state":state,"summary":summary,"conversationType":d.input.conversation_type,"details":{"runId":d.run_id,"deliveryId":d.input.delivery_id,"sessionId":d.session_id,"sessionPath":d.session_id.as_ref().map(|sid|format!("native:{sid}"))}}}));
        Ok(())
    }
    async fn discover(&self, _state: &ServiceState) -> Result<(), ConversationError> {
        // 写入与创建投递之间崩溃时，由原文件动作回执恢复；不能把每个模型回复当作新用户根。
        let operations = store::pending_message_operations(&self.storage).await?;
        for (op, mutation) in operations {
            let result = if let Some(delivery) = op.strip_prefix("phone-reply-") {
                if let Some(d) = store::get_delivery(&self.storage, delivery).await? {
                    if d.status != store::PhoneDeliveryStatus::Cancelled
                        && d.decision_body
                            .as_deref()
                            .is_some_and(|body| !phone_reply_text(body).1)
                        && self.delivery_current(&d)?
                    {
                        self.enqueue_message_status(
                            &d.input.conversation_id,
                            &d.input.actor_id,
                            &op,
                            d.run_id.as_deref(),
                        )
                        .await
                    } else {
                        Ok(true)
                    }
                } else {
                    Err(ConversationError::new(
                        409,
                        "published phone reply has no delivery fact",
                    ))
                }
            } else {
                let source = store::message_operation_source(&self.storage, &op).await?;
                match mutation {
                    ConversationMutation::AppendChannel { id, sender, .. } => {
                        self.enqueue_message_status(&id, &sender, &op, source.as_deref())
                            .await
                    }
                    ConversationMutation::CreateChannel { id, intro, .. } => {
                        if intro.as_deref().is_some_and(|text| !text.trim().is_empty()) {
                            self.enqueue_message_status(&id, "user", &op, source.as_deref())
                                .await
                        } else {
                            Ok(true)
                        }
                    }
                    ConversationMutation::AppendDm { from, to, .. } => {
                        self.enqueue_message_status(
                            &format!("dm:{to}"),
                            &from,
                            &op,
                            source.as_deref(),
                        )
                        .await
                    }
                    _ => Ok(true),
                }
            };
            match result {
                Ok(true) => {
                    store::finish_message_operation(
                        &self.storage,
                        &op,
                        None,
                        self.clock.now_unix_ms(),
                    )
                    .await?
                }
                Ok(false) => {}
                Err(e) if matches!(e.status, 403 | 404) => {
                    store::finish_message_operation(
                        &self.storage,
                        &op,
                        Some(&format!("conversation unavailable: {e}")),
                        self.clock.now_unix_ms(),
                    )
                    .await?
                }
                Err(e) => {
                    tracing::error!(operation=%op,error=%e,"手机消息投递尚未完成");
                }
            }
        }
        Ok(())
    }
    async fn discover_legacy_and_proactive(&self) -> Result<(), ConversationError> {
        let active = store::list_active_deliveries(&self.storage, None, None).await?;
        for doc in self.files.list_channels()? {
            let id = crate::conversations::field(&doc.meta, "id")?;
            self.discover_document(
                id,
                None,
                &doc,
                active.iter().any(|d| d.input.conversation_id == id),
            )
            .await?;
        }
        let agents = self
            .memory
            .list_agent_summaries()
            .map_err(|e| ConversationError::new(409, e.message))?;
        for actor in &agents {
            let actor = crate::conversations::field(actor, "id")?;
            for peer in &agents {
                let peer = crate::conversations::field(peer, "id")?;
                if actor == peer {
                    continue;
                }
                let doc = match self.files.read_dm(actor, peer) {
                    Ok(d) => self.visible_dm(actor, peer, d)?,
                    Err(lingxi_kernel::ports::StorageError::InvalidRequest { detail })
                        if detail.contains("conversation file missing:") =>
                    {
                        continue
                    }
                    Err(e) => return Err(e.into()),
                };
                let id = format!("dm:{peer}");
                self.discover_document(
                    &id,
                    Some(actor),
                    &doc,
                    active
                        .iter()
                        .any(|d| d.input.conversation_id == id && d.input.actor_id == actor),
                )
                .await?;
            }
        }
        Ok(())
    }
    async fn discover_document(
        &self,
        id: &str,
        owner: Option<&str>,
        doc: &lingxi_adapters::conversation_files::ConversationDocument,
        busy: bool,
    ) -> Result<(), ConversationError> {
        use lingxi_adapters::storage::conversation_discovery as discovery;
        let Some(last) = doc.messages.last() else {
            return Ok(());
        };
        let key = match owner {
            Some(a) => format!("{a}:{id}"),
            None => id.into(),
        };
        let fp = digest(json!([doc.messages.len(), last]));
        let cursor = discovery::cursor(&self.storage, &key).await?;
        let now = self.clock.now_unix_ms();
        let mut slot = cursor.as_ref().map(|c| c.proactive_slot).unwrap_or(0);
        let incoming = owner.is_none_or(|actor| last.sender != actor);
        if incoming
            && cursor.as_ref().is_none_or(|c| c.fingerprint != fp)
            && discovery::message_operation(
                &self.storage,
                id,
                &last.sender,
                &last.body,
                &last.timestamp,
            )
            .await?
            .is_none()
        {
            let target = if let Some(actor) = owner {
                format!("dm:{actor}")
            } else {
                id.into()
            };
            if !self
                .enqueue_message_status(
                    &target,
                    &last.sender,
                    &format!("legacy-{}", digest(json!([key, fp]))),
                    None,
                )
                .await?
            {
                return Ok(());
            }
        }
        let settings = if let Some(actor) = owner {
            self.settings(actor, id)?
        } else {
            crate::phone_settings::PhoneSettings::read(&doc.meta, members(doc)?.len())?
        };
        let interval = u64::from(settings.reminder_interval_minutes) * 60_000;
        let parsed = chrono::NaiveDateTime::parse_from_str(&last.timestamp, "%Y-%m-%d %H:%M:%S")
            .or_else(|_| chrono::NaiveDateTime::parse_from_str(&last.timestamp, "%Y-%m-%d %H:%M"))
            .map_err(|e| ConversationError::new(409, e))?;
        let actual = chrono::TimeZone::from_local_datetime(&chrono::Local, &parsed)
            .latest()
            .ok_or_else(|| {
                ConversationError::new(409, "conversation timestamp is not a local clock instant")
            })?
            .timestamp_millis();
        let completed = if owner.is_none() {
            self.last_phone_completion_ms(doc).await?.unwrap_or(0)
        } else {
            0
        };
        let reference = (actual.max(0) as u64).max(completed);
        let due = actual >= 0 && now.saturating_sub(reference) >= interval;
        let current_slot = now / interval;
        if owner.is_none() && settings.proactive_enabled && due && !busy && current_slot > slot {
            let cause = format!("phone-proactive-{}", digest(json!([key, current_slot])));
            let (target, from) = if let Some(actor) = owner {
                (
                    format!("dm:{actor}"),
                    id.strip_prefix("dm:").expect("私信身份"),
                )
            } else {
                (id.into(), "")
            };
            if self
                .enqueue_message_status(&target, from, &cause, None)
                .await?
            {
                slot = current_slot;
            }
        }
        discovery::save_cursor(&self.storage, &key, &fp, slot, now).await?;
        Ok(())
    }
}

async fn prepare_phone_session(
    state: &ServiceState,
    d: &store::PhoneDelivery,
) -> Result<(), ConversationError> {
    let api = ConversationApi::from_state(state);
    api.apply(
        &format!("phone-projection-{}", d.input.delivery_id),
        &ConversationMutation::PhonePatch {
            actor: d.input.actor_id.clone(),
            conversation: d.input.conversation_id.clone(),
            conversation_type: d.input.conversation_type.clone(),
            patch: json!({}),
            timestamp: timestamp(d.created_at)?,
        },
    )
    .await?;
    let sid = d.input.input_json["sessionId"]
        .as_str()
        .ok_or_else(|| ConversationError::new(409, "phone session missing"))?;
    let settings: crate::phone_settings::PhoneSettings =
        serde_json::from_value(d.input.input_json["settings"].clone())
            .map_err(|e| ConversationError::new(409, e))?;
    if session_tree::get_session_row(state.storage(), sid)
        .await?
        .is_none()
    {
        let folders = if let Some(run) = d.input.source_run_id.as_deref() {
            let r = state
                .storage()
                .load_run(&RunId::new(run))
                .await?
                .ok_or_else(|| ConversationError::new(409, "phone source Run missing"))?;
            session_tree::get_session_row(state.storage(), r.session_id.as_str())
                .await?
                .ok_or_else(|| ConversationError::new(409, "phone source session missing"))?
                .authorized_folders
        } else {
            vec![]
        };
        state
            .session_admin()
            .create_session(
                sid,
                &d.input.actor_id,
                &d.input.owner_subject,
                "助手手机",
                Some(settings.permission().wire_name()),
                &folders,
                false,
                state.clock().now_unix_ms(),
            )
            .await
            .map_err(|e| ConversationError::new(409, format!("{e:?}")))?;
    }
    if settings.model_override_enabled {
        ConversationApi::from_state(state)
            .validate_model(settings.model_override_model.as_ref().expect("模型已验证"))?;
    }
    let row = session_tree::get_session_row(state.storage(), sid)
        .await?
        .ok_or_else(|| ConversationError::new(409, "phone session disappeared"))?;
    if row.agent_id != d.input.actor_id
        || row.owner_user_id != d.input.owner_subject
        || row.lifecycle != "active"
    {
        return Err(ConversationError::new(
            403,
            "phone session identity mismatch",
        ));
    }
    let old = store::get_phone_binding(
        state.storage(),
        &d.input.actor_id,
        &d.input.conversation_id,
        &d.input.owner_subject,
    )
    .await?;
    store::put_phone_binding_for_delivery(
        state.storage(),
        &store::PhoneBindingInput {
            actor_id: d.input.actor_id.clone(),
            conversation_id: d.input.conversation_id.clone(),
            conversation_type: d.input.conversation_type.clone(),
            owner_subject: d.input.owner_subject.clone(),
            session_id: sid.into(),
            mode: settings.permission().wire_name().into(),
            expected_epoch: old.map(|b| b.epoch),
        },
        &d.input.delivery_id,
        state.clock().now_unix_ms(),
    )
    .await?;
    Ok(())
}

impl crate::sessions::SessionStore {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn execute_phone_for(
        &self,
        storage: &Arc<lingxi_adapters::storage::RunDatabase>,
        events: &Arc<crate::events::EventService>,
        supervisor: &Arc<crate::runs::RunSupervisor>,
        background: &Arc<crate::background::BackgroundDriveRegistry>,
        principal: &crate::auth::Principal,
        session_id: &str,
        delivery_id: &str,
        now: u64,
    ) -> Result<crate::sessions::ExecuteAccepted, crate::sessions::SessionExecuteError> {
        use crate::sessions::{ExecuteSubmission, SessionExecuteError as E};
        let d = store::get_delivery(storage, delivery_id)
            .await
            .map_err(E::Storage)?
            .ok_or(E::Forbidden)?;
        if !principal.is_local_owner()
            || d.input.owner_subject != lingxi_kernel::LOCAL_OWNER_SUBJECT
            || d.input.input_json["sessionId"] != session_id
            || !matches!(
                d.status,
                store::PhoneDeliveryStatus::Claimed | store::PhoneDeliveryStatus::Running
            )
        {
            return Err(E::Forbidden);
        }
        let binding = store::get_phone_binding(
            storage,
            &d.input.actor_id,
            &d.input.conversation_id,
            &d.input.owner_subject,
        )
        .await
        .map_err(E::Storage)?
        .ok_or(E::Forbidden)?;
        if binding.input.session_id != session_id {
            return Err(E::Forbidden);
        }
        let input = d.input.input_json["prompt"].as_str().ok_or(E::Forbidden)?;
        let request = format!("phone:{delivery_id}");
        if let Some((run_id, _, _)) = store::get_delivery_run(storage, delivery_id)
            .await
            .map_err(E::Storage)?
        {
            return Err(E::RequestIdBoundToEarlierRun {
                request_id: request,
                run_id,
            });
        }
        let settings: crate::phone_settings::PhoneSettings =
            serde_json::from_value(d.input.input_json["settings"].clone())
                .map_err(|_| E::Forbidden)?;
        let lineage = RunLineage {
            parent_run_id: d.input.source_run_id.as_deref().map(RunId::new),
            origin: if d.input.conversation_type == "dm" {
                RunOrigin::Dm
            } else {
                RunOrigin::Channel
            },
            source_message_id: Some(d.input.source_message_id),
            cause_id: Some(delivery_id.into()),
        };
        let chat = if settings.model_override_enabled {
            let m = settings.model_override_model.as_ref().ok_or(E::Forbidden)?;
            Some(crate::runs::ChatRunInput {
                images: vec![],
                files: vec![],
                client_message_id: None,
                snapshot_version: None,
                display_text: None,
                quoted_text: None,
                session_refs: vec![],
                options: lingxi_kernel::model_exchange::ModelTurnOptions {
                    model: Some((
                        m["provider"].as_str().ok_or(E::Forbidden)?.into(),
                        m["id"].as_str().ok_or(E::Forbidden)?.into(),
                    )),
                    thinking_level: None,
                },
            })
        } else {
            None
        };
        self.execute_background_authorized_for(
            storage,
            events,
            supervisor,
            background,
            principal,
            session_id,
            &ExecuteSubmission {
                input,
                request_id: Some(&request),
                knowledge: None,
            },
            now,
            chat,
            Some(lineage),
            None,
            Some(settings.permission()),
            None,
        )
        .await
    }
}
