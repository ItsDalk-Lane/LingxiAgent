//! 原生提示和草稿只消费真实运行事件；每次外部操作先取得持久授权和唯一回执。
use crate::{bridge_effect_text::VisibleDraft, bridge_worker::BridgeWorkerProcess, ServiceState};
use lingxi_adapters::storage::{bridge as store, bridge_effect as effects};
use lingxi_kernel::ports::StoragePort;
use lingxi_protocol::{
    BridgeConversationKind, BridgeEffectCapabilities, BridgeEffectOperation as Op,
    BridgeEffectReceipt, BridgeEffectStatus, BridgeReceiptStatus, BridgeStreamCapability,
    BridgeStreamMode, BridgeWaitingReceiptMode, BridgeWorkerCommand, BridgeWorkerEvent, RunId,
};
use std::{collections::BTreeMap, time::Duration};
use tokio::time::Instant;

const MAX_INFLIGHT: usize = 8;
const MAX_STREAMS: usize = 128;
const EFFECT_TIMEOUT: Duration = Duration::from_secs(30);
const CLEANUP_BUDGET: Duration = Duration::from_secs(5);

struct LiveStream {
    stream: effects::BridgeEffectStream,
    cap: Option<BridgeStreamCapability>,
    draft: VisibleDraft,
    last_text: String,
    last_update: Option<Instant>,
    last_receipt: Option<Instant>,
    receipt_started: bool,
    receipt_stopped: bool,
    preview_possible: bool,
    cancel_attempted: bool,
    terminal: bool,
    final_confirmed: bool,
}
struct Pending {
    effect: effects::BridgeEffect,
    started: Instant,
}
#[derive(Default)]
pub(crate) struct BridgeEffects {
    capabilities: Option<BridgeEffectCapabilities>,
    streams: BTreeMap<String, LiveStream>,
    pending: BTreeMap<String, Pending>,
    owner: Option<(String, u64)>,
}
fn selected(
    capabilities: &BridgeEffectCapabilities,
    scope: BridgeConversationKind,
    rich: bool,
) -> Option<BridgeStreamCapability> {
    let native = |cap: &&BridgeStreamCapability| {
        cap.scopes.contains(&scope)
            && matches!(
                cap.mode,
                BridgeStreamMode::Draft
                    | BridgeStreamMode::RichDraft
                    | BridgeStreamMode::EditMessage
                    | BridgeStreamMode::CardkitStream
            )
    };
    let caps: Vec<_> = capabilities.streams.iter().filter(native).collect();
    rich.then(|| caps.iter().find(|cap| cap.rich).copied())
        .flatten()
        .or_else(|| caps.iter().find(|cap| !cap.rich).copied())
        .cloned()
}
fn limited_text(text: &str, max_chars: u32) -> String {
    text.chars()
        .take(max_chars.clamp(1, 100_000) as usize)
        .collect()
}
fn unknown_receipt(effect: &effects::BridgeEffect, reason: &str) -> BridgeEffectReceipt {
    BridgeEffectReceipt {
        effect_id: effect.request.effect_id.clone(),
        stream_id: effect.request.stream_id.clone(),
        sequence: effect.request.sequence,
        operation: effect.request.operation,
        delivery_id: effect.request.delivery_id.clone(),
        status: BridgeEffectStatus::Unknown,
        platform_ack: false,
        platform_message_ids: vec![],
        error: Some(reason.into()),
    }
}
impl BridgeEffects {
    pub fn set_capabilities(&mut self, capabilities: Option<BridgeEffectCapabilities>) {
        self.capabilities = capabilities;
    }
    fn bind(&mut self, cfg: &store::BridgeConnector) -> Result<(), String> {
        let binding = (cfg.input.id.clone(), cfg.generation);
        if self.owner.as_ref().is_some_and(|old| old != &binding) {
            return Err("Bridge effects cannot switch connector or generation".into());
        }
        self.owner = Some(binding);
        Ok(())
    }
    fn inflight(&self, stream: &str) -> bool {
        self.pending
            .values()
            .any(|item| item.effect.request.stream_id == stream)
    }
    pub fn busy_for(&self, outbox: &store::BridgeOutbox) -> bool {
        self.pending.len() >= MAX_INFLIGHT
            || self.pending.values().any(|item| {
                outbox.input.inbox_id.as_deref() == Some(item.effect.request.inbox_id.as_str())
                    && outbox.input.run_id.as_deref() == Some(item.effect.request.run_id.as_str())
            })
            || self.streams.values().any(|live| {
                outbox.input.inbox_id.as_deref() == Some(live.stream.inbox_id.as_str())
                    && outbox.input.run_id.as_deref() == Some(live.stream.run_id.as_str())
                    && live.cap.as_ref().is_some_and(|cap| {
                        live.last_update.is_some_and(|at| {
                            at.elapsed()
                                < Duration::from_millis(u64::from(
                                    cap.min_interval_ms.clamp(500, 60_000),
                                ))
                        })
                    })
            })
    }
    // 授权范围与效果字段分别传递，避免把可信连接事实混入外部请求载荷。
    #[allow(clippy::too_many_arguments)]
    async fn send(
        &mut self,
        state: &ServiceState,
        cfg: &store::BridgeConnector,
        worker: &mut BridgeWorkerProcess,
        stream: &effects::BridgeEffectStream,
        operation: Op,
        key: String,
        text: String,
        delivery_id: Option<String>,
    ) -> Result<bool, String> {
        if self.pending.len() >= MAX_INFLIGHT || self.inflight(&stream.stream_id) {
            return Ok(false);
        }
        if stream.connector_id != cfg.input.id || stream.connector_generation != cfg.generation {
            return Err("Bridge effect connector scope mismatch".into());
        }
        let claim = effects::claim_effect(
            state.storage(),
            &effects::BridgeEffectInput {
                stream_id: stream.stream_id.clone(),
                request_key: key,
                operation,
                text,
                delivery_id,
            },
            state.clock().now_unix_ms(),
        )
        .await
        .map_err(|e| e.to_string())?;
        if !claim.claimed {
            return Ok(false);
        }
        let effect = claim.effect;
        let cleanup = matches!(operation, Op::ReceiptStop | Op::StreamCancel);
        if !cleanup {
            let actual = store::get_connector(state.storage(), &cfg.input.id)
                .await
                .map_err(|e| e.to_string())?;
            let authorized = actual.is_some_and(|current| {
                current.generation == cfg.generation
                    && current.input.enabled
                    && current.input.actor_id == cfg.input.actor_id
            }) && !state
                .memory()
                .is_agent_deleted_checked(&cfg.input.actor_id)
                .map_err(|e| format!("{e:?}"))?
                && state
                    .memory()
                    .agent_dir_checked(&cfg.input.actor_id)
                    .is_ok();
            if !authorized {
                let mut receipt = unknown_receipt(&effect, "bridge_effect_revoked_before_send");
                receipt.status = BridgeEffectStatus::Failed;
                effects::settle_effect(state.storage(), &receipt, state.clock().now_unix_ms())
                    .await
                    .map_err(|e| e.to_string())?;
                return Err("Bridge effect authorization changed before send".into());
            }
        }
        let command = BridgeWorkerCommand::Effect {
            dispatch_seq: None,
            connector_id: cfg.input.id.clone(),
            generation: u32::try_from(cfg.generation)
                .map_err(|_| "Bridge generation exceeds protocol")?,
            effect: effect.request.clone(),
        };
        self.pending.insert(
            effect.request.effect_id.clone(),
            Pending {
                effect,
                started: Instant::now(),
            },
        );
        // 管道写入开始后即可能已经执行，错误交由统一退出收口为 Unknown。
        worker
            .send(&serde_json::to_value(command).map_err(|_| "Bridge effect encoding failed")?)
            .await?;
        Ok(true)
    }
    async fn live(
        &mut self,
        state: &ServiceState,
        inbox: &store::BridgeInbox,
        run: &str,
        rich: bool,
    ) -> Result<Option<String>, String> {
        let Some(caps) = self.capabilities.as_ref() else {
            return Ok(None);
        };
        let cap = selected(caps, inbox.input.conversation_kind, rich);
        let mode = if cap.is_some() {
            effects::BridgeEffectMode::NativeStream
        } else {
            effects::BridgeEffectMode::ReceiptOnly
        };
        let receipt_independent = caps.receipt.scopes.contains(&inbox.input.conversation_kind)
            && !cap.as_ref().is_some_and(|cap| cap.receipt_folded);
        let stream = effects::ensure_stream_with_receipt(
            state.storage(),
            &inbox.id,
            run,
            mode,
            cap.as_ref().is_some_and(|cap| cap.rich),
            receipt_independent,
            state.clock().now_unix_ms(),
        )
        .await
        .map_err(|e| e.to_string())?;
        let id = stream.stream_id.clone();
        if !self.streams.contains_key(&id) {
            if self.streams.len() >= MAX_STREAMS {
                return Err("Bridge native stream capacity exhausted".into());
            }
            if stream.last_sequence > 0 && !stream.unknown && !stream.closed {
                return Err(
                    "Bridge native stream cannot be reconstructed without its worker state".into(),
                );
            }
            let cap = selected(caps, inbox.input.conversation_kind, stream.rich);
            self.streams.insert(
                id.clone(),
                LiveStream {
                    stream,
                    cap,
                    draft: VisibleDraft::default(),
                    last_text: String::new(),
                    last_update: None,
                    last_receipt: None,
                    receipt_started: false,
                    receipt_stopped: false,
                    preview_possible: false,
                    cancel_attempted: false,
                    terminal: false,
                    final_confirmed: false,
                },
            );
        }
        Ok(Some(id))
    }
    pub async fn expire_pending(&mut self, state: &ServiceState) -> Result<(), String> {
        let expired: Vec<_> = self
            .pending
            .values()
            .filter(|pending| pending.started.elapsed() >= EFFECT_TIMEOUT)
            .map(|pending| unknown_receipt(&pending.effect, "native_effect_receipt_timeout"))
            .collect();
        for receipt in expired {
            self.accept(state, receipt).await?;
        }
        Ok(())
    }
    pub async fn tick(
        &mut self,
        state: &ServiceState,
        cfg: &store::BridgeConnector,
        worker: &mut BridgeWorkerProcess,
    ) -> Result<(), String> {
        self.bind(cfg)?;
        self.expire_pending(state).await?;
        let preferences = state.memory().read_preferences_root()?;
        let receipts =
            preferences.pointer("/bridge/receiptEnabled") != Some(&serde_json::Value::Bool(false));
        let rich = preferences.pointer("/bridge/richStreamingEnabled")
            != Some(&serde_json::Value::Bool(false));
        if let Some(caps) = self.capabilities.clone() {
            for inbox in store::list_pending_inbox(state.storage())
                .await
                .map_err(|e| e.to_string())?
                .into_iter()
                .filter(|inbox| {
                    inbox.input.connector_id == cfg.input.id
                        && inbox.connector_generation == cfg.generation
                })
            {
                let Some(run) = inbox.run_id.as_deref() else {
                    continue;
                };
                if selected(&caps, inbox.input.conversation_kind, rich).is_none()
                    && (!receipts || !caps.receipt.scopes.contains(&inbox.input.conversation_kind))
                {
                    continue;
                }
                self.live(state, &inbox, run, rich).await?;
            }
        }
        let ids: Vec<_> = self.streams.keys().cloned().collect();
        for id in ids {
            if self.inflight(&id) || self.pending.len() >= MAX_INFLIGHT {
                continue;
            }
            let mut live = self.streams.remove(&id).expect("known stream");
            let result = self
                .tick_stream(state, cfg, worker, &mut live, receipts)
                .await;
            // 即使失败，也保留同一外部材料的清理责任。
            self.streams.insert(id, live);
            result?;
        }
        self.streams.retain(|id, live| {
            let cleaned = !live.preview_possible || live.cancel_attempted || live.final_confirmed;
            let retired = (live.stream.closed && cleaned)
                || (live.terminal && live.stream.unknown && cleaned);
            !(retired
                && (!live.receipt_started || live.receipt_stopped)
                && !self
                    .pending
                    .values()
                    .any(|item| &item.effect.request.stream_id == id))
        });
        Ok(())
    }
    async fn tick_stream(
        &mut self,
        state: &ServiceState,
        cfg: &store::BridgeConnector,
        worker: &mut BridgeWorkerProcess,
        live: &mut LiveStream,
        receipts: bool,
    ) -> Result<(), String> {
        live.stream = effects::get_stream(state.storage(), &live.stream.stream_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("Bridge effect stream disappeared")?;
        let terminal = state
            .storage()
            .load_run(&RunId::new(&live.stream.run_id))
            .await
            .map_err(|e| e.to_string())?
            .ok_or("Bridge effect Run disappeared")?
            .status
            .is_terminal();
        live.terminal = terminal;
        if live.receipt_started
            && !live.receipt_stopped
            && (!receipts || terminal || live.stream.unknown || live.stream.closed)
        {
            self.send(
                state,
                cfg,
                worker,
                &live.stream,
                Op::ReceiptStop,
                "receipt:stop".into(),
                String::new(),
                None,
            )
            .await?;
            live.receipt_stopped = true;
            return Ok(());
        }
        if live.stream.unknown || (live.stream.closed && !live.final_confirmed) {
            if live.stream.mode == effects::BridgeEffectMode::NativeStream
                && live.preview_possible
                && !live.cancel_attempted
            {
                self.send(
                    state,
                    cfg,
                    worker,
                    &live.stream,
                    Op::StreamCancel,
                    "stream:cancel".into(),
                    String::new(),
                    None,
                )
                .await?;
                live.cancel_attempted = true;
            }
            return Ok(());
        }
        if terminal || live.stream.closed {
            return Ok(());
        }
        let Some(caps) = self.capabilities.clone() else {
            return Ok(());
        };
        let scope = if live.stream.reply_context.is_group == Some(true) {
            BridgeConversationKind::Group
        } else {
            BridgeConversationKind::Dm
        };
        if receipts && caps.receipt.scopes.contains(&scope) && !live.receipt_started {
            self.send(
                state,
                cfg,
                worker,
                &live.stream,
                Op::ReceiptStart,
                "receipt:start".into(),
                "正在准备回复…".into(),
                None,
            )
            .await?;
            live.receipt_started = true;
            live.last_receipt = Some(Instant::now());
            live.preview_possible |= live.cap.as_ref().is_some_and(|cap| {
                cap.receipt_folded
                    && matches!(
                        cap.mode,
                        BridgeStreamMode::EditMessage | BridgeStreamMode::CardkitStream
                    )
            });
            if live.preview_possible {
                live.last_update = Some(Instant::now());
            }
            return Ok(());
        }
        let refresh = Duration::from_millis(u64::from(caps.receipt.refresh_interval_ms.max(500)));
        if receipts
            && live.receipt_started
            && !live.receipt_stopped
            && caps.receipt.mode == BridgeWaitingReceiptMode::NativeTyping
            && caps.receipt.refresh_interval_ms > 0
            && live.last_receipt.is_some_and(|at| at.elapsed() >= refresh)
        {
            self.send(
                state,
                cfg,
                worker,
                &live.stream,
                Op::ReceiptStart,
                format!(
                    "receipt:refresh:{}",
                    live.stream.last_sequence.saturating_add(1)
                ),
                String::new(),
                None,
            )
            .await?;
            live.last_receipt = Some(Instant::now());
            return Ok(());
        }
        let Some(cap) = live.cap.as_ref() else {
            return Ok(());
        };
        if live.last_update.is_some_and(|at| {
            at.elapsed() < Duration::from_millis(u64::from(cap.min_interval_ms.clamp(500, 60_000)))
        }) {
            return Ok(());
        }
        let inbox = store::get_inbox(state.storage(), &live.stream.inbox_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("Bridge draft inbox disappeared")?;
        let session = inbox
            .session_id
            .as_deref()
            .ok_or("Bridge draft session missing")?;
        live.draft
            .refresh(state.storage(), session, &live.stream.run_id)
            .await?;
        let Some(text) = live.draft.visible() else {
            return Ok(());
        };
        let text = limited_text(&text, cap.max_chars);
        if text.is_empty() || text == live.last_text {
            return Ok(());
        }
        self.send(
            state,
            cfg,
            worker,
            &live.stream,
            Op::StreamUpdate,
            format!(
                "stream:update:{}",
                live.stream.last_sequence.saturating_add(1)
            ),
            text.clone(),
            None,
        )
        .await?;
        live.last_text = text;
        live.last_update = Some(Instant::now());
        live.preview_possible = true;
        Ok(())
    }
    pub async fn finish(
        &mut self,
        state: &ServiceState,
        cfg: &store::BridgeConnector,
        worker: &mut BridgeWorkerProcess,
        outbox: &store::BridgeOutbox,
    ) -> Result<bool, String> {
        self.bind(cfg)?;
        let (Some(inbox_id), Some(run)) = (
            outbox.input.inbox_id.as_deref(),
            outbox.input.run_id.as_deref(),
        ) else {
            return Ok(false);
        };
        let mut stream = effects::find_stream(state.storage(), inbox_id, run)
            .await
            .map_err(|e| e.to_string())?;
        if stream.is_none() && self.capabilities.is_some() {
            let inbox = store::get_inbox(state.storage(), inbox_id)
                .await
                .map_err(|e| e.to_string())?
                .ok_or("Bridge final inbox missing")?;
            let preferences = state.memory().read_preferences_root()?;
            let rich = preferences.pointer("/bridge/richStreamingEnabled")
                != Some(&serde_json::Value::Bool(false));
            if self
                .capabilities
                .as_ref()
                .and_then(|caps| selected(caps, inbox.input.conversation_kind, rich))
                .is_some()
            {
                self.live(state, &inbox, run, rich).await?;
            }
            stream = effects::find_stream(state.storage(), inbox_id, run)
                .await
                .map_err(|e| e.to_string())?;
        }
        let Some(stream) = stream else {
            return Ok(false);
        };
        if stream.mode == effects::BridgeEffectMode::ReceiptOnly {
            return Ok(false);
        }
        if stream.unknown || stream.closed || !outbox.input.attachments.is_empty() {
            store::settle_outbox(
                state.storage(),
                &outbox.input.delivery_id,
                store::BridgeOutboxStatus::Unknown,
                None,
                Some("native stream outcome unavailable; plain resend forbidden".into()),
                state.clock().now_unix_ms(),
            )
            .await
            .map_err(|e| e.to_string())?;
            return Ok(true);
        }
        if self.inflight(&stream.stream_id) {
            return Err("Bridge native final is waiting for its prior effect receipt".into());
        }
        let payload: serde_json::Value = serde_json::from_str(&outbox.input.payload_json)
            .map_err(|_| "Bridge final payload damaged")?;
        let text = payload["text"]
            .as_str()
            .ok_or("Bridge final text missing")?;
        if let Some(live) = self.streams.get_mut(&stream.stream_id) {
            live.preview_possible |= live.cap.as_ref().is_some_and(|cap| {
                matches!(
                    cap.mode,
                    BridgeStreamMode::EditMessage | BridgeStreamMode::CardkitStream
                )
            });
        }
        if !self
            .send(
                state,
                cfg,
                worker,
                &stream,
                Op::StreamFinish,
                format!("stream:finish:{}", outbox.input.delivery_id),
                text.into(),
                Some(outbox.input.delivery_id.clone()),
            )
            .await?
        {
            return Err("Bridge native final could not claim its unique effect".into());
        }
        Ok(true)
    }
    pub async fn accept(
        &mut self,
        state: &ServiceState,
        receipt: BridgeEffectReceipt,
    ) -> Result<Option<store::BridgeOutbox>, String> {
        receipt.validate().map_err(str::to_owned)?;
        let Some(known) = self.pending.get(&receipt.effect_id) else {
            let old = effects::get_effect(state.storage(), &receipt.effect_id)
                .await
                .map_err(|e| e.to_string())?
                .ok_or("Bridge worker returned an unsolicited effect receipt")?;
            if self.owner.as_ref() != Some(&(old.connector_id.clone(), old.connector_generation))
                || old.request.stream_id != receipt.stream_id
                || old.request.sequence != receipt.sequence
                || old.request.operation != receipt.operation
                || old.request.delivery_id != receipt.delivery_id
                || old.status == effects::StoredEffectStatus::Started
            {
                return Err("Bridge late effect receipt identity mismatch".into());
            }
            // 超时 Unknown 不因迟到回执被改成成功；已观察终态的重投同样不重复发布。
            return Ok(None);
        };
        if known.effect.request.stream_id != receipt.stream_id {
            return Err("Bridge effect receipt stream mismatch".into());
        }
        let effect = effects::settle_effect(state.storage(), &receipt, state.clock().now_unix_ms())
            .await
            .map_err(|e| e.to_string())?;
        self.pending.remove(&receipt.effect_id);
        if let Some(live) = self.streams.get_mut(&receipt.stream_id) {
            if receipt.operation == Op::StreamFinish
                && receipt.status == BridgeEffectStatus::Confirmed
            {
                live.final_confirmed = true;
            }
            live.stream = effects::get_stream(state.storage(), &receipt.stream_id)
                .await
                .map_err(|e| e.to_string())?
                .ok_or("Bridge settled stream missing")?;
        }
        if effect.request.operation == Op::StreamFinish {
            let id = effect
                .request
                .delivery_id
                .as_deref()
                .ok_or("Bridge native final delivery missing")?;
            return store::get_outbox(state.storage(), id)
                .await
                .map_err(|e| e.to_string());
        }
        Ok(None)
    }
    async fn drain_frame(
        &mut self,
        state: &ServiceState,
        cfg: &store::BridgeConnector,
        frame: serde_json::Value,
    ) -> Result<(), String> {
        if frame["connectorId"].as_str() != Some(cfg.input.id.as_str())
            || frame["generation"].as_u64() != Some(cfg.generation)
        {
            return Err("Bridge cleanup frame identity mismatch".into());
        }
        match serde_json::from_value::<BridgeWorkerEvent>(frame)
            .map_err(|_| "Bridge cleanup frame violates protocol")?
        {
            BridgeWorkerEvent::EffectReceipt { receipt, .. } => {
                if let Some(outbox) = self.accept(state, receipt).await? {
                    if outbox.status == store::BridgeOutboxStatus::Confirmed {
                        crate::bridge::confirmed_delivery(state, &outbox).await?;
                    }
                }
            }
            BridgeWorkerEvent::Receipt {
                delivery_id,
                status,
                platform_message_ids,
                error,
                ..
            } => {
                let row = store::get_outbox(state.storage(), &delivery_id)
                    .await
                    .map_err(|e| e.to_string())?
                    .ok_or("Bridge cleanup delivery missing")?;
                if row.input.connector_id != cfg.input.id
                    || row.input.connector_generation != cfg.generation
                    || row.status != store::BridgeOutboxStatus::Started
                {
                    return Err("Bridge cleanup received unrelated delivery receipt".into());
                }
                let (status, receipt) = match status {
                    BridgeReceiptStatus::Confirmed if !platform_message_ids.is_empty() => (
                        store::BridgeOutboxStatus::Confirmed,
                        Some(store::BridgePlatformReceipt {
                            platform: cfg.input.platform.clone(),
                            message_ids: platform_message_ids,
                            raw_json: None,
                        }),
                    ),
                    BridgeReceiptStatus::Failed => (store::BridgeOutboxStatus::Failed, None),
                    _ => (store::BridgeOutboxStatus::Unknown, None),
                };
                let settled = store::settle_outbox(
                    state.storage(),
                    &delivery_id,
                    status,
                    receipt,
                    error,
                    state.clock().now_unix_ms(),
                )
                .await
                .map_err(|e| e.to_string())?;
                if settled.status == store::BridgeOutboxStatus::Confirmed {
                    crate::bridge::confirmed_delivery(state, &settled).await?;
                }
            }
            BridgeWorkerEvent::ConnectionStatus { .. } => {}
            BridgeWorkerEvent::Error { .. } | BridgeWorkerEvent::Stopped { .. } => {
                return Err("Bridge worker stopped during native cleanup".into())
            }
            // 退出期间不再认领新消息和媒体；只消费旧操作的真实回执。
            _ => {}
        }
        Ok(())
    }
    pub async fn shutdown(
        &mut self,
        state: &ServiceState,
        cfg: &store::BridgeConnector,
        worker: &mut BridgeWorkerProcess,
    ) -> Result<(), String> {
        self.bind(cfg)?;
        let deadline = Instant::now() + CLEANUP_BUDGET;
        let cleanup = async {
            loop {
                if self.pending.is_empty() {
                    let mut planned = false;
                    let ids: Vec<_> = self.streams.keys().cloned().collect();
                    for id in ids {
                        if self.pending.len() >= MAX_INFLIGHT {
                            break;
                        }
                        let mut live = self.streams.remove(&id).expect("known stream");
                        live.stream = effects::get_stream(state.storage(), &id)
                            .await
                            .map_err(|e| e.to_string())?
                            .ok_or("Bridge cleanup stream missing")?;
                        let action = if live.receipt_started && !live.receipt_stopped {
                            live.receipt_stopped = true;
                            Some((Op::ReceiptStop, "receipt:stop"))
                        } else if live.stream.mode == effects::BridgeEffectMode::NativeStream
                            && !live.final_confirmed
                            && live.preview_possible
                            && !live.cancel_attempted
                        {
                            live.cancel_attempted = true;
                            Some((Op::StreamCancel, "stream:cancel"))
                        } else {
                            None
                        };
                        planned |= action.is_some();
                        let sent = if let Some((op, key)) = action {
                            self.send(
                                state,
                                cfg,
                                worker,
                                &live.stream,
                                op,
                                key.into(),
                                String::new(),
                                None,
                            )
                            .await
                        } else {
                            Ok(false)
                        };
                        self.streams.insert(id, live);
                        sent?;
                    }
                    if self.pending.is_empty() && !planned {
                        break;
                    }
                    if self.pending.is_empty() {
                        continue;
                    }
                }
                let frame = worker
                    .next()
                    .await?
                    .ok_or("Bridge worker closed during native cleanup")?;
                self.drain_frame(state, cfg, frame).await?;
            }
            Ok(())
        };
        // 外层 future 取消也保留持久 Started；最终由同代次 abandon 诚实记 Unknown。
        let result = tokio::time::timeout_at(deadline, cleanup)
            .await
            .map_err(|_| "Bridge native cleanup deadline exceeded".to_string())
            .and_then(|result| result);
        let abandoned = effects::abandon_connector_streams(
            state.storage(),
            &cfg.input.id,
            cfg.generation,
            state.clock().now_unix_ms(),
        )
        .await
        .map_err(|e| e.to_string());
        self.pending.clear();
        self.streams.clear();
        match (result, abandoned) {
            (Ok(()), Ok(_)) => Ok(()),
            (Err(error), Ok(_)) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Err(error), Err(cleanup)) => Err(format!("{error}; native cleanup: {cleanup}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lingxi_protocol::BridgeWaitingReceiptCapability;
    fn cap(
        mode: BridgeStreamMode,
        scope: BridgeConversationKind,
        rich: bool,
    ) -> BridgeStreamCapability {
        BridgeStreamCapability {
            mode,
            scopes: vec![scope],
            min_interval_ms: 1,
            max_chars: 4096,
            rich,
            receipt_folded: true,
        }
    }
    #[test]
    fn native_selection_requires_actual_scope_and_supported_mode_and_honors_rich_preference() {
        let caps = BridgeEffectCapabilities {
            receipt: BridgeWaitingReceiptCapability {
                mode: BridgeWaitingReceiptMode::Text,
                scopes: vec![BridgeConversationKind::Dm],
                refresh_interval_ms: 0,
                cancellable: false,
            },
            streams: vec![
                cap(
                    BridgeStreamMode::Batch,
                    BridgeConversationKind::Group,
                    false,
                ),
                cap(
                    BridgeStreamMode::Block,
                    BridgeConversationKind::Group,
                    false,
                ),
                cap(BridgeStreamMode::Draft, BridgeConversationKind::Dm, false),
                cap(
                    BridgeStreamMode::CardkitStream,
                    BridgeConversationKind::Dm,
                    true,
                ),
            ],
        };
        assert!(selected(&caps, BridgeConversationKind::Group, true).is_none());
        assert_eq!(
            selected(&caps, BridgeConversationKind::Dm, false)
                .unwrap()
                .mode,
            BridgeStreamMode::Draft
        );
        assert_eq!(
            selected(&caps, BridgeConversationKind::Dm, true)
                .unwrap()
                .mode,
            BridgeStreamMode::CardkitStream
        );
        let caps = BridgeEffectCapabilities {
            streams: vec![cap(
                BridgeStreamMode::RichDraft,
                BridgeConversationKind::Dm,
                true,
            )],
            ..caps
        };
        assert!(selected(&caps, BridgeConversationKind::Dm, false).is_none());
    }
    #[test]
    fn draft_character_limit_preserves_complete_unicode_characters() {
        assert_eq!(limited_text("真实🙂草稿", 3), "真实🙂");
        assert_eq!(limited_text("abc", 0), "a");
    }
}
