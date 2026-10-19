//! loopsvc.rs — 循环面的生产装配（R07-T07-δ6）：LoopHostHooks 的真实投递/
//! 记录/探测实现与 LoopStore/LoopController/LoopAlarmService 的组合根构造。
//! 对照现役 `core/engine.ts:1395-1500`（setLoopServices 挂钩）与
//! `server/index.ts:488-491`（LoopStore 落盘位置）。
//!
//! 现役映射登记：
//! - deliverLoopMessage 桌面臂（session-coordinator deliverCustomMessage
//!   {triggerTurn:true}）→ `SessionStore::execute_scheduled_for(origin=Loop,
//!   trigger_id=循环键)`：循环轮走完整 admission 链的普通后台轮，grant Full
//!   → UserSession 权限上下文（交互审批保留，现役循环轮即普通会话轮）。
//!   会话忙 → Delivery::Busy（闹钟顺延；admission 的 Busy 回绝是竞态背靠背
//!   闸）；会话行缺失 → loop_target_reset（现役 session_manifest_not_found
//!   → targetResetError）；其余回绝如实 deliver_failed。
//! - recordNotice（deliverCustomMessage {triggerTurn:false}，现役落持久
//!   display:false custom 条目）→ `SessionSupervisor::deliver_retained`
//!   转向收件箱（易失、下一轮模型可见、不触发轮）。Rust 没有「无 run 归属
//!   的持久模型可见行」类型（HostNotice 绑定桥接收据），循环状态文件本身
//!   持久保 paused+reason，无状态丢失——偏差登记于 T07_STATUS δ5。
//! - recordUserPrompt（recordCustomEntry loop-user-prompt +
//!   session_user_message 事件）→ messages 行 entry_type=
//!   "loop-user-prompt"（≠"message" → 模型上下文恒定排除，
//!   model_history.rs:917 闸）+ ChatWire 展示气泡广播；历史加载层投影成
//!   普通用户气泡（history_projection 的 LoopUserPrompt 臂，对照现役
//!   message-utils.ts:229）。
//! - hasLiveBackgroundWork（taskRegistry 活跃 + deferred 待投递）→
//!   TaskTools::list 三源同口径：subagent_threads 非终态 + operations 在册
//!   任务 pending + media_jobs 未结算。库读错误 → warn + false（显式降级：
//!   现役软解析失败同归 false；保守方向是多兜一次底闹钟，状态无损）。
//! - isTargetMidStream（isSessionStreaming）→ SessionSupervisor::is_busy。
//! - isTargetRunnable（resolveDesktopPath 成功）→ 会话行存在；任何错误
//!   → false（现役 try/catch → false 同语义）。
//! - emitLoopStatus（_emitEvent loop_status → chat.ts broadcast 扁平帧）→
//!   ChatWire 广播（不进 replay 缓存，同现役「不属于任何流」）；时间戳由
//!   服务时钟统一供给。

use std::path::Path;
use std::sync::{Arc, Mutex};

use lingxi_adapters::storage::{media, session_tree, subagent_threads, RunDatabase};
use lingxi_kernel::subagent::RunOrigin;
use serde_json::json;

use crate::auth::Principal;
use crate::chat_wire::ChatWire;
use crate::events::EventService;
use crate::inject::ServiceClock;
use crate::loopx::{
    DeliverLoopMessage, DeliverWakeup, Delivery, DeliveryMode, EmitLoopStatus, LoopAlarmService,
    LoopController, LoopError, LoopHostHooks, LoopMessage, LoopRecord, LoopStatus, LoopStore,
    LoopTarget, LoopTimings, OnDeliveryExhausted, RecordOnly, RecordUserPrompt, TargetProbe,
    LOOP_USER_PROMPT_MESSAGE_TYPE,
};
use crate::operations::OperationService;
use crate::sessions::{SessionExecuteError, SessionStore};

/// 组合根产物（slash 臂 / loop_control 工具 / 会话列表注入共用同一组
/// 单例，对照现役 engine.loopController 单例）。
#[derive(Clone)]
pub struct LoopServices {
    pub store: Arc<LoopStore>,
    pub controller: Arc<LoopController>,
    pub alarm: Arc<LoopAlarmService>,
    workflows: Arc<std::sync::OnceLock<std::sync::Weak<crate::workflow::WorkflowService>>>,
    idle_listener: Arc<IdleReleaseListener>,
}

impl LoopServices {
    /// 退出先关闭调度口；与正在释放许可的线程串行，返回后不会新建空闲检查任务。
    pub fn stop_idle_listener(&self) {
        self.idle_listener.stop();
    }

    /// 存储关闭钩子只捕获弱引用，避免存储、循环和会话之间形成强引用环。
    pub(crate) fn idle_listener_stopper(&self) -> impl Fn() + Send + Sync + 'static {
        let weak = Arc::downgrade(&self.idle_listener);
        move || {
            if let Some(listener) = weak.upgrade() {
                listener.stop();
            }
        }
    }

    pub fn attach_workflows(
        &self,
        service: &Arc<crate::workflow::WorkflowService>,
    ) -> Result<(), String> {
        self.workflows
            .set(Arc::downgrade(service))
            .map_err(|_| "循环工作流探测重复绑定".into())
    }
}

struct IdleReleaseListener {
    active: Mutex<bool>,
    registration: Mutex<Option<lingxi_kernel::session_admission::ReleaseListenerRegistration>>,
}

impl IdleReleaseListener {
    fn stop(&self) {
        // 回调只在此锁内做同步登记，不持锁等待任务执行。
        *self.active.lock().unwrap_or_else(|p| p.into_inner()) = false;
        self.registration
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
    }
}

/// 将通用同步释放事件接入既有任务监督器；释放线程无需自行拥有 Tokio 上下文。
fn attach_idle_release_listener(
    gate: &Arc<lingxi_kernel::session_admission::SessionAdmission>,
    controller: &Arc<LoopController>,
    tasks: &Arc<crate::task_supervisor::TaskSupervisor>,
) -> std::io::Result<Arc<IdleReleaseListener>> {
    let runtime = tokio::runtime::Handle::try_current()
        .map_err(|error| std::io::Error::other(format!("循环空闲监听缺少服务运行时：{error}")))?;
    let listener = Arc::new(IdleReleaseListener {
        active: Mutex::new(true),
        registration: Mutex::new(None),
    });
    let weak_listener = Arc::downgrade(&listener);
    let weak_controller = Arc::downgrade(controller);
    let weak_tasks = Arc::downgrade(tasks);
    let registration = gate
        .register_release_listener(Arc::new(move |sessions| {
            let Some(listener) = weak_listener.upgrade() else {
                return;
            };
            let active = listener.active.lock().unwrap_or_else(|p| p.into_inner());
            if !*active {
                return;
            }
            let (Some(controller), Some(tasks)) = (weak_controller.upgrade(), weak_tasks.upgrade())
            else {
                return;
            };
            if controller.is_disposed() {
                return;
            }
            for session in sessions {
                let Some(record) = controller
                    .tool_status(session)
                    .filter(|record| record.status == LoopStatus::Running)
                else {
                    continue;
                };
                let weak_controller = Arc::downgrade(&controller);
                let weak_listener = Arc::downgrade(&listener);
                let session = session.clone();
                // spawn_detached 内部使用 Tokio；进入启动时保存的运行时而非依赖释放线程。
                let _entered = runtime.enter();
                let scheduled =
                    tasks.spawn_detached(format!("loop-session-idle:{session}"), async move {
                        let (Some(listener), Some(controller)) =
                            (weak_listener.upgrade(), weak_controller.upgrade())
                        else {
                            return;
                        };
                        if !*listener.active.lock().unwrap_or_else(|p| p.into_inner()) {
                            return;
                        }
                        // 真忙态、换代、后台工作和已有闹钟由控制器在异步探测前后复核。
                        controller.on_session_idle(&session).await;
                    });
                if let Err(error) = scheduled {
                    let message = format!("循环空闲检查未能登记，自动推进尚未确认：{error:?}");
                    tracing::error!(session = %record.key, %message);
                    controller.on_idle_dispatch_failed(&record.key, &message);
                }
            }
        }))
        .map_err(std::io::Error::other)?;
    *listener
        .registration
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = Some(registration);
    Ok(listener)
}

/// 现役三源后台工作探测（TaskTools::list 的同源同步判定）。
async fn live_background_work(
    storage: &RunDatabase,
    operations: Option<&OperationService>,
    workflows: Option<&crate::workflow::WorkflowService>,
    session: &str,
) -> bool {
    if workflows.is_some_and(|service| service.has_active_for_session(session)) {
        return true;
    }
    // 软解析：会话行缺失或读错 → false（现役 resolveTargetSessionPathSoft
    // → null → false 同语义）。
    let owner = match session_tree::get_session_row(storage, session).await {
        Ok(Some(row)) => row.owner_user_id,
        Ok(None) => return false,
        Err(err) => {
            tracing::warn!("[loop] live-work probe: session row unreadable for {session}: {err}");
            return false;
        }
    };
    match subagent_threads::background_tasks(storage, Some(session), None).await {
        Ok(tasks) => {
            if tasks
                .iter()
                .any(|task| task["status"].as_str() == Some("pending"))
            {
                return true;
            }
        }
        Err(err) => {
            tracing::warn!("[loop] live-work probe: subagent tasks unreadable for {session}: {err}")
        }
    }
    if let Some(operations) = operations {
        if operations
            .owned_background_tasks(session)
            .iter()
            .any(|task| task["status"].as_str() == Some("pending"))
        {
            return true;
        }
    }
    match media::list_session_unsettled(storage, session, &owner).await {
        Ok(jobs) => !jobs.is_empty(),
        Err(err) => {
            tracing::warn!("[loop] live-work probe: media jobs unreadable for {session}: {err}");
            false
        }
    }
}

/// 循环任务 prompt 的展示气泡落库（现役 recordCustomEntry +
/// session_user_message 事件）。
async fn record_loop_user_prompt(
    storage: &RunDatabase,
    chat_wire: &ChatWire,
    session: &str,
    prompt: &str,
    now: u64,
) -> Result<(), String> {
    // 与 DbUserMessageRecorder 同规：parent 取当前分支头。
    let parent = session_tree::list_branch_messages(storage, session)
        .await
        .map_err(|err| err.to_string())?
        .last()
        .map(|message| message.message_id.clone());
    let content = json!({ "prompt": prompt, "timestamp": now }).to_string();
    let message_id = format!("loop-user-prompt:{session}:{now}");
    session_tree::append_message(
        storage,
        session_tree::AppendMessage {
            session_id: session,
            message_id: &message_id,
            parent_message_id: parent.as_deref(),
            run_id: None,
            role: "user",
            content_json: &content,
            entry_type: LOOP_USER_PROMPT_MESSAGE_TYPE,
            model_call_id: None,
            now_unix_ms: now,
        },
    )
    .await
    .map_err(|err| err.to_string())?;
    chat_wire.publish_loop_user_prompt(session, prompt, now);
    Ok(())
}

/// 组合根：构建循环三件套并一次性绑定控制器↔闹钟（现役
/// setLoopServices 的相互引用同位）。状态文件损坏由 LoopStore::load
/// 按现役语义移备份并空态继续；目录创建失败等 IO 错误响亮上抛。
#[allow(clippy::too_many_arguments)]
pub fn build_loop_services(
    data_home: &Path,
    sessions: &Arc<SessionStore>,
    storage: &Arc<RunDatabase>,
    events: &Arc<EventService>,
    runs: &Arc<crate::runs::RunSupervisor>,
    background: &Arc<crate::background::BackgroundDriveRegistry>,
    operations: Option<Arc<OperationService>>,
    chat_wire: &Arc<ChatWire>,
    principal: &Principal,
    clock: &Arc<dyn ServiceClock>,
) -> std::io::Result<LoopServices> {
    let store = Arc::new(LoopStore::load(
        data_home.join(".ephemeral").join("loop-state.json"),
        {
            let clock = Arc::clone(clock);
            Box::new(move || clock.now_unix_ms())
        },
    )?);

    let workflows = Arc::new(std::sync::OnceLock::<
        std::sync::Weak<crate::workflow::WorkflowService>,
    >::new());
    let hooks = {
        let probe_workflows = Arc::clone(&workflows);
        let supervisor = Arc::clone(sessions.session_supervisor());
        // 探测面（异步查库）。
        let probe_storage = Arc::clone(storage);
        let probe_operations = operations.clone();
        let has_live_background_work: TargetProbe = Arc::new(move |target: LoopTarget| {
            let storage = Arc::clone(&probe_storage);
            let operations = probe_operations.clone();
            let workflows = probe_workflows.get().and_then(std::sync::Weak::upgrade);
            Box::pin(async move {
                live_background_work(
                    &storage,
                    operations.as_deref(),
                    workflows.as_deref(),
                    &target.session_id,
                )
                .await
            })
        });
        // 投递面（完整 admission 链的后台轮）。
        let deliver_sessions = Arc::clone(sessions);
        let deliver_storage = Arc::clone(storage);
        let deliver_events = Arc::clone(events);
        let deliver_runs = Arc::clone(runs);
        let deliver_background = Arc::clone(background);
        let deliver_principal = principal.clone();
        let deliver_clock = Arc::clone(clock);
        let deliver_supervisor = Arc::clone(&supervisor);
        let deliver_loop_message: DeliverLoopMessage =
            Arc::new(move |target: LoopTarget, message: LoopMessage| {
                let sessions = Arc::clone(&deliver_sessions);
                let storage = Arc::clone(&deliver_storage);
                let events = Arc::clone(&deliver_events);
                let runs = Arc::clone(&deliver_runs);
                let background = Arc::clone(&deliver_background);
                let principal = deliver_principal.clone();
                let supervisor = Arc::clone(&deliver_supervisor);
                let now = deliver_clock.now_unix_ms();
                Box::pin(async move {
                    let session_id = target.session_id.as_str();
                    if supervisor.is_busy(session_id) {
                        return Ok(Delivery::mode(DeliveryMode::Busy));
                    }
                    match sessions
                        .execute_scheduled_for(
                            &storage,
                            &events,
                            &runs,
                            &background,
                            &principal,
                            session_id,
                            &message.content,
                            session_id,
                            RunOrigin::Loop,
                            None,
                            message.run_identity(),
                            now,
                        )
                        .await
                    {
                        Ok(_) => Ok(Delivery::mode(DeliveryMode::TriggerTurn)),
                        // 受理闸的 Busy 回绝是 pre-check 之后的竞态背靠背。
                        Err(SessionExecuteError::Busy) => Ok(Delivery::mode(DeliveryMode::Busy)),
                        Err(SessionExecuteError::NotFound) => Err(LoopError::loop_target_reset()),
                        Err(other) => Err(LoopError::new(
                            "deliver_failed",
                            format!("loop deliver failed: {other:?}"),
                        )),
                    }
                })
            });
        // 通知面（转向收件箱；偏差登记见模块头）。
        let notice_supervisor = Arc::clone(&supervisor);
        let record_notice: RecordOnly =
            Arc::new(move |target: LoopTarget, message: LoopMessage| {
                let supervisor = Arc::clone(&notice_supervisor);
                Box::pin(async move {
                    supervisor
                        .deliver_retained(&target.session_id, &message.content)
                        .map_err(|err| format!("{err:?}"))
                })
            });
        // 展示气泡面。
        let prompt_storage = Arc::clone(storage);
        let prompt_wire = Arc::clone(chat_wire);
        let prompt_clock = Arc::clone(clock);
        let record_user_prompt: RecordUserPrompt =
            Arc::new(move |target: LoopTarget, prompt: String| {
                let storage = Arc::clone(&prompt_storage);
                let chat_wire = Arc::clone(&prompt_wire);
                let now = prompt_clock.now_unix_ms();
                Box::pin(async move {
                    record_loop_user_prompt(&storage, &chat_wire, &target.session_id, &prompt, now)
                        .await
                })
            });
        // 连续轮判定（同步在册忙态）。
        let mid_supervisor = Arc::clone(&supervisor);
        let is_target_mid_stream =
            Arc::new(move |target: &LoopTarget| mid_supervisor.is_busy(&target.session_id));
        // 可投递性（会话行存在；任何错误 → false）。
        let runnable_storage = Arc::clone(storage);
        let is_target_runnable: TargetProbe = Arc::new(move |target: LoopTarget| {
            let storage = Arc::clone(&runnable_storage);
            Box::pin(async move {
                matches!(
                    session_tree::get_session_row(&storage, &target.session_id).await,
                    Ok(Some(_))
                )
            })
        });
        // 状态广播面（现役扁平帧字段逐一对应）。
        let status_wire = Arc::clone(chat_wire);
        let status_clock = Arc::clone(clock);
        let emit_loop_status: EmitLoopStatus =
            Arc::new(move |target: &LoopTarget, record: &LoopRecord| {
                status_wire.publish_loop_status(
                    &target.session_id,
                    status_snapshot(record, None),
                    status_clock.now_unix_ms(),
                );
            });
        LoopHostHooks {
            has_live_background_work,
            deliver_loop_message,
            record_notice,
            record_user_prompt,
            is_target_mid_stream,
            is_target_runnable,
            emit_loop_status,
        }
    };

    let controller = LoopController::new(Arc::clone(&store), hooks);
    let error_wire = Arc::clone(chat_wire);
    let error_clock = Arc::clone(clock);
    let error_store = Arc::clone(&store);
    controller.set_storage_error_handler(Arc::new(move |session, error| {
        if let Some(record) = error_store.get(session) {
            error_wire.publish_loop_status(
                session,
                status_snapshot(
                    &record,
                    Some(format!(
                        "循环自动推进已停止；修复后用 /loop resume 重试。{error}"
                    )),
                ),
                error_clock.now_unix_ms(),
            );
        }
    }));
    let alarm = {
        let weak = Arc::downgrade(&controller);
        let deliver_wakeup: DeliverWakeup = Arc::new(move |key: String, reason: String| {
            let weak = weak.clone();
            Box::pin(async move {
                match weak.upgrade() {
                    Some(controller) => controller.deliver_wakeup_turn(&key, &reason).await,
                    // 控制器已释放（服务关停中）：如实按未投递处理。
                    None => Delivery::mode(DeliveryMode::Stopped),
                }
            })
        });
        let weak = Arc::downgrade(&controller);
        let on_delivery_exhausted: OnDeliveryExhausted =
            Arc::new(move |key: String, err: String, generation| {
                if let Some(controller) = weak.upgrade() {
                    controller.pause_for_delivery_failure(&key, &err, generation);
                }
            });
        LoopAlarmService::new(
            Arc::clone(&store),
            deliver_wakeup,
            on_delivery_exhausted,
            LoopTimings::default(),
            {
                let clock = Arc::clone(clock);
                Box::new(move || clock.now_unix_ms())
            },
        )
    };
    controller.attach_alarm(Arc::clone(&alarm));
    let idle_listener = attach_idle_release_listener(
        storage.session_admission(),
        &controller,
        runs.task_supervisor(),
    )?;
    Ok(LoopServices {
        store,
        controller,
        alarm,
        workflows,
        idle_listener,
    })
}

/// 现役 `_renderLoopStatus`（bridge-commands.ts:235-247）逐行平移；闹钟
/// 时间按已登记偏差取 RFC3339（现役 toLocaleString）。
pub fn render_loop_status(record: &LoopRecord) -> String {
    let mut lines = vec![
        format!(
            "循环状态：{}{}",
            record.status.wire_name(),
            record
                .paused_reason
                .as_deref()
                .map(|reason| format!("（{reason}）"))
                .unwrap_or_default()
        ),
        format!("任务：{}", record.prompt),
        format!("轮数：{}/{}", record.turn_count, record.limits.max_turns),
    ];
    if let Some(alarm) = &record.alarm {
        let when = chrono::DateTime::from_timestamp_millis(alarm.wake_at as i64)
            .map(|instant| instant.to_rfc3339())
            .unwrap_or_else(|| alarm.wake_at.to_string());
        let reason = if alarm.reason.is_empty() {
            "无说明"
        } else {
            alarm.reason.as_str()
        };
        lines.push(format!("闹钟：{when}（{reason}）"));
    } else {
        lines.push("闹钟：无".to_string());
    }
    if let Some(summary) = &record.completed_summary {
        lines.push(format!("完成摘要：{summary}"));
    }
    lines.join("\n")
}

/// 会话列表的冷启动注入形状（现役 sessions.ts:839）：仅 running/paused
/// 注入，其余状态如实缺省（null）。
fn status_snapshot(
    record: &LoopRecord,
    runtime_error: Option<String>,
) -> lingxi_protocol::LoopStatusSnapshot {
    lingxi_protocol::LoopStatusSnapshot {
        status: match record.status {
            LoopStatus::Running => lingxi_protocol::LoopState::Running,
            LoopStatus::Paused => lingxi_protocol::LoopState::Paused,
            LoopStatus::Stopped => lingxi_protocol::LoopState::Stopped,
            LoopStatus::Completed => lingxi_protocol::LoopState::Completed,
        },
        turn_count: record.turn_count,
        max_turns: Some(record.limits.max_turns),
        paused_reason: record.paused_reason.clone(),
        prompt: Some(record.prompt.clone()),
        runtime_error,
    }
}

pub fn loop_status_inject(
    controller: &LoopController,
    session_id: &str,
) -> Option<serde_json::Value> {
    let record = controller.tool_status(session_id)?;
    if !matches!(record.status, LoopStatus::Running | LoopStatus::Paused) {
        return None;
    }
    Some(
        serde_json::to_value(status_snapshot(
            &record,
            controller.storage_error(session_id),
        ))
        .expect("循环状态只包含可序列化字段"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct IdleFixture {
        dir: std::path::PathBuf,
        gate: Arc<lingxi_kernel::session_admission::SessionAdmission>,
        controller: Arc<LoopController>,
        alarm: Arc<LoopAlarmService>,
        tasks: Arc<crate::TaskSupervisor>,
        listener: Arc<IdleReleaseListener>,
    }

    impl IdleFixture {
        fn new(cap: usize) -> Self {
            static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
            let dir = std::env::temp_dir().join(format!(
                "lingxi-loop-idle-{}-{}",
                std::process::id(),
                SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            let gate = Arc::new(lingxi_kernel::session_admission::SessionAdmission::default());
            let store = Arc::new(
                LoopStore::load(dir.join("loop-state.json"), Box::new(|| 1_000)).expect("store"),
            );
            let probe = Arc::clone(&gate);
            let controller = LoopController::new(
                Arc::clone(&store),
                LoopHostHooks {
                    has_live_background_work: Arc::new(|_| Box::pin(async { false })),
                    deliver_loop_message: Arc::new(|_, _| {
                        Box::pin(async { Ok(Delivery::mode(DeliveryMode::TriggerTurn)) })
                    }),
                    record_notice: Arc::new(|_, _| Box::pin(async { Ok(()) })),
                    record_user_prompt: Arc::new(|_, _| Box::pin(async { Ok(()) })),
                    is_target_mid_stream: Arc::new(move |target| probe.is_busy(&target.session_id)),
                    is_target_runnable: Arc::new(|_| Box::pin(async { true })),
                    emit_loop_status: Arc::new(|_, _| {}),
                },
            );
            let alarm = LoopAlarmService::new(
                store,
                Arc::new(|_, _| Box::pin(async { Delivery::mode(DeliveryMode::Stopped) })),
                Arc::new(|_, _, _| {}),
                LoopTimings::default(),
                Box::new(|| 1_000),
            );
            controller.attach_alarm(Arc::clone(&alarm));
            let tasks = Arc::new(crate::TaskSupervisor::new(cap));
            let listener =
                attach_idle_release_listener(&gate, &controller, &tasks).expect("listener");
            Self {
                dir,
                gate,
                controller,
                alarm,
                tasks,
                listener,
            }
        }

        fn create(&self, session: &str, status: LoopStatus) {
            self.controller
                .store()
                .create(&LoopTarget::desktop(session), "测试", None)
                .expect("create");
            self.controller
                .store()
                .update(session, |record| record.status = status)
                .expect("status");
        }

        async fn settle(&self) {
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                while self.tasks.tasks().iter().any(|task| task.exit.is_none()) {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("受监督空闲检查应实际结束");
        }
    }

    impl Drop for IdleFixture {
        fn drop(&mut self) {
            self.listener.stop();
            self.controller.dispose();
            self.alarm.dispose();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[tokio::test]
    async fn final_permit_drop_on_plain_thread_arms_real_alarm_through_supervised_listener() {
        let fixture = IdleFixture::new(8);
        fixture.create("running", LoopStatus::Running);
        fixture.create("paused", LoopStatus::Paused);
        let permit = Arc::new(
            fixture
                .gate
                .try_acquire(&["running", "paused", "ordinary"], 8)
                .expect("permit"),
        );
        let child = Arc::clone(&permit);
        fixture.controller.on_session_idle("running").await;
        assert!(fixture
            .controller
            .tool_status("running")
            .expect("record")
            .alarm
            .is_none());
        drop(permit);
        assert!(fixture.tasks.tasks().is_empty());
        assert!(fixture.gate.is_busy("running"));
        std::thread::spawn(move || drop(child))
            .join()
            .expect("普通线程释放最后许可");
        fixture.settle().await;
        let record = fixture.controller.tool_status("running").expect("record");
        assert!(record.alarm.is_some(), "最后许可释放后应真正补上兜底闹钟");
        let tasks = fixture.tasks.tasks();
        assert_eq!(tasks.len(), 1, "暂停和普通会话不应新建检查任务");
        assert_eq!(tasks[0].label, "loop-session-idle:running");
        assert_eq!(tasks[0].exit, Some(crate::TaskExit::Completed));
        assert!(!fixture.gate.is_busy("running"));
        drop(fixture.gate.try_acquire(&["running"], 8).expect("再次交互"));
        fixture.settle().await;
        assert_eq!(
            fixture
                .controller
                .tool_status("running")
                .expect("record")
                .alarm,
            record.alarm
        );
    }

    #[tokio::test]
    async fn idle_listener_shutdown_unregisters_and_disposed_controller_never_schedules() {
        let fixture = IdleFixture::new(8);
        fixture.create("running", LoopStatus::Running);
        let permit = fixture.gate.try_acquire(&["running"], 8).expect("permit");
        fixture.listener.stop();
        std::thread::spawn(move || drop(permit))
            .join()
            .expect("release");
        assert!(fixture.tasks.tasks().is_empty());
        assert!(fixture
            .controller
            .tool_status("running")
            .expect("record")
            .alarm
            .is_none());
        let replacement =
            attach_idle_release_listener(&fixture.gate, &fixture.controller, &fixture.tasks)
                .expect("停后可重新绑定");
        fixture.controller.dispose();
        drop(fixture.gate.try_acquire(&["running"], 8).expect("permit"));
        assert!(fixture.tasks.tasks().is_empty());
        replacement.stop();
    }

    #[tokio::test]
    async fn idle_listener_cap_failure_is_explicit_and_does_not_leave_running_task() {
        let fixture = IdleFixture::new(0);
        fixture.create("running", LoopStatus::Running);
        drop(fixture.gate.try_acquire(&["running"], 1).expect("permit"));
        assert!(fixture.tasks.tasks().is_empty());
        assert!(
            fixture.controller.storage_error("running").is_some(),
            "登记失败必须显式阻断并可读"
        );
        assert!(fixture
            .controller
            .tool_status("running")
            .expect("record")
            .alarm
            .is_none());
        assert!(!fixture.gate.is_busy("running"));
    }

    fn record_with_alarm(alarm: Option<crate::loopx::LoopAlarmSlot>) -> LoopRecord {
        LoopRecord {
            key: "sess".into(),
            target: LoopTarget::desktop("sess"),
            status: LoopStatus::Paused,
            prompt: "盯一下队列".into(),
            turn_count: 3,
            consecutive_failures: 0,
            alarm,
            limits: crate::loopx::LOOP_DEFAULT_LIMITS,
            paused_reason: Some("budget_exhausted".into()),
            completed_summary: None,
            created_at: 1,
            updated_at: 2,
        }
    }

    #[test]
    fn render_loop_status_mirrors_incumbent_lines() {
        let text = render_loop_status(&record_with_alarm(None));
        assert_eq!(
            text,
            "循环状态：paused（budget_exhausted）\n任务：盯一下队列\n轮数：3/50\n闹钟：无"
        );
        let mut record = record_with_alarm(Some(crate::loopx::LoopAlarmSlot {
            wake_at: 1_700_000_000_000,
            reason: String::new(),
        }));
        record.completed_summary = Some("done".into());
        let text = render_loop_status(&record);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "循环状态：paused（budget_exhausted）");
        assert!(lines[3].starts_with("闹钟：2023-11-14T22:13:20+00:00（无说明）"));
        assert_eq!(lines[4], "完成摘要：done");
    }

    #[test]
    fn inject_only_for_active_states() {
        let dir = std::env::temp_dir().join(format!("lingxi-r07-loopsvc-{}", std::process::id()));
        let store = Arc::new(
            LoopStore::load(dir.join("loop-state.json"), Box::new(|| 1_000)).expect("load"),
        );
        let hooks = LoopHostHooks {
            has_live_background_work: Arc::new(|_| Box::pin(async { false })),
            deliver_loop_message: Arc::new(|_, _| {
                Box::pin(async { Ok(Delivery::mode(DeliveryMode::TriggerTurn)) })
            }),
            record_notice: Arc::new(|_, _| Box::pin(async { Ok(()) })),
            record_user_prompt: Arc::new(|_, _| Box::pin(async { Ok(()) })),
            is_target_mid_stream: Arc::new(|_| false),
            is_target_runnable: Arc::new(|_| Box::pin(async { true })),
            emit_loop_status: Arc::new(|_, _| {}),
        };
        let controller = LoopController::new(Arc::clone(&store), hooks);
        store
            .create(&LoopTarget::desktop("run-loop"), "任务", None)
            .expect("create");
        let paused = {
            let mut paused_target = LoopTarget::desktop("paused-loop");
            paused_target.session_id = "paused-loop".into();
            store
                .create(&paused_target, "任务", None)
                .expect("create paused");
            store
                .update("paused-loop", |record| record.status = LoopStatus::Paused)
                .expect("pause")
        };
        let stopped_target = LoopTarget::desktop("stopped-loop");
        store
            .create(&stopped_target, "任务", None)
            .expect("create stopped");
        store
            .update("stopped-loop", |record| record.status = LoopStatus::Stopped)
            .expect("stop");

        let running = loop_status_inject(&controller, "run-loop").expect("running injected");
        assert_eq!(running["status"], json!("running"));
        assert_eq!(running["maxTurns"], json!(50));
        assert_eq!(paused.status, LoopStatus::Paused);
        assert!(loop_status_inject(&controller, "paused-loop").is_some());
        assert!(loop_status_inject(&controller, "stopped-loop").is_none());
        assert!(loop_status_inject(&controller, "no-loop").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
