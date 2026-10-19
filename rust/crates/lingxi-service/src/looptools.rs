//! R07-T07-δ7 loop_control 工具（现役 `lib/tools/loop-control-tool.ts` 的端口）。
//!
//! 两动作 schedule/complete；会话键取 `RunContext.session_id`（内核上下文
//! 恒定携带会话身份，现役 "No active session." 分支结构上不可达）；控制器
//! 由组合根持有，工具仅保留弱引用；服务退出后的旧绑定明确返回失败。
//! description、参数 schema（含各字段 description）与全部结果文本逐字平移
//! 现役 en 源串；教育性拒绝按现役以正常文本结果返回，服务已退出则明确失败。
//!
//! 权限：现役 sessionPermission.resolveInvocation 恒定 {kind:"routine",
//! capability:"loop_control.control"} → 固定 PermissionKind::Execute +
//! loop_control.control（checkpoint/patrol 先例：routine 档对应 Execute）。
//!
//! 边角登记：现役 delay_seconds 保留小数（JS Number），Rust 侧闹钟槽为
//! 整秒粒度（u64），小数输入按 Math.round 同义四舍五入（与 goal 预算的
//! u64 取舍同规）；effectiveDelaySec/wakeAt 语义不变。wakeAt 按
//! toISOString 同形输出（RFC3339 millis UTC）。

use std::pin::Pin;
use std::sync::{Arc, Weak};

use lingxi_kernel::ports::{ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest};
use lingxi_kernel::toolcatalog::{
    Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
    ToolManifest, ToolOrigin, ToolRegistry,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{ErrorCode, ProtocolError, ToolCallId, ToolSchemaDocument};
use serde_json::{json, Value};

use crate::loopx::{LoopController, LoopStatus};
use crate::toolgateway::ToolInvocationGateway;

const DESCRIPTION: &str = "Control this session's active recurring loop (started with /loop). Only usable while a loop is active; calls outside an active loop fail. action=schedule sets the single fallback alarm that wakes this session after delay_seconds — background work completion wakes the session automatically, so never schedule alarms to poll it; alarms are for external state the system cannot observe, and for long hang-protection fallbacks. A new schedule replaces the previous alarm. action=complete declares the loop task achieved and ends the loop.";

struct LoopControlToolExecutor {
    controller: Weak<LoopController>,
}

impl LoopControlToolExecutor {
    /// 执行期间临时持有服务，网关登记不能反向延长整个服务的生命周期。
    async fn run(&self, ctx: &RunContext, args: &Value) -> ToolOutcome {
        let Some(controller) = self.controller.upgrade() else {
            return ToolOutcome::Failed {
                error: ProtocolError::new(
                    ErrorCode::InvalidMessage,
                    "循环服务已退出，不能执行旧工具调用",
                    false,
                ),
            };
        };
        let session_id = ctx.session_id.as_str();
        // 现役硬闸：仅 running/paused 的活跃循环可用。
        let loop_record = controller.tool_status(session_id);
        let active = loop_record.as_ref().is_some_and(|record| {
            matches!(record.status, LoopStatus::Running | LoopStatus::Paused)
        });
        if !active {
            return text_outcome("No active loop in this session. Start one with /loop <task>.");
        }
        let action = args.get("action").and_then(Value::as_str).unwrap_or("");
        if action == "schedule" {
            let Some(delay) = args.get("delay_seconds").and_then(Value::as_f64) else {
                return text_outcome("delay_seconds is required for action=schedule.");
            };
            let reason = args.get("reason").and_then(Value::as_str).unwrap_or("");
            // u64 整秒粒度：小数 Math.round 同义取舍，负数落到 0 后由
            // min_delay 钳制（现役 Math.max(delay, minDelay) 同效）。
            let delay_sec = delay.max(0.0).round() as u64;
            return match controller
                .tool_schedule_for_run(session_id, ctx.run_id.as_str(), delay_sec, reason)
                .await
            {
                Ok(outcome) => {
                    let wake_at = chrono::DateTime::from_timestamp_millis(outcome.wake_at as i64)
                        .map(|instant| instant.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
                        .unwrap_or_else(|| outcome.wake_at.to_string());
                    text_outcome(format!(
                        "Wakeup scheduled in {}s (fires at {wake_at}). It replaces any previous alarm.",
                        outcome.effective_delay_sec
                    ))
                }
                // 教育性拒绝（如带活跃后台任务的短闹钟）以工具结果返回。
                Err(err) => text_outcome(err.message),
            };
        }
        if action == "complete" {
            let reason = args.get("reason").and_then(Value::as_str).unwrap_or("");
            return match controller
                .tool_complete_for_run(session_id, ctx.run_id.as_str(), reason)
                .await
            {
                Ok(_) => text_outcome("Loop completed and closed."),
                Err(err) => text_outcome(err.message),
            };
        }
        // schema required+enum 之外的防线（结构上是响亮拒绝，不猜测意图）。
        text_outcome(format!("unknown action: {action}"))
    }
}

/// 现役 text()：正常文本结果（无 details）。
fn text_outcome(text: impl Into<String>) -> ToolOutcome {
    ToolOutcome::Success {
        result: lingxi_kernel::ports::ToolSuccess::text(text.into()),
    }
}

impl ToolExecutorPort for LoopControlToolExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            ToolExecutionResult::of_ctx(ctx, self.run(ctx, request.arguments.as_value()).await)
        })
    }
}

/// 与其他正式工具同口径登记：网关 prepare → 权限/批准 → 执行。
pub fn register_loop_control_tool(
    registry: &ToolRegistry,
    gateway: &ToolInvocationGateway,
    controller: Arc<LoopController>,
    budget: &SchemaBudget,
) {
    let manifest = ToolManifest {
        origin: ToolOrigin::FirstParty,
        local_name: "loop_control".into(),
        display_name: "Loop Control".into(),
        aliases: Vec::new(),
        version: "1.0.0".into(),
        description: DESCRIPTION.into(),
        input_schema: ToolSchemaDocument {
            dialect: "json-schema/2020-12".into(),
            schema: json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["schedule", "complete"], "description": "schedule: set the fallback wakeup alarm. complete: end the loop because its goal is achieved."},
                    "delay_seconds": {"type": "number", "description": "schedule only, required: seconds until the wakeup fires. Clamped to the loop's minimum; long fallbacks (>= 20 min) required while background work is live."},
                    "reason": {"type": "string", "description": "schedule: context shown back to you when the wakeup fires. complete: short completion summary."},
                },
                "required": ["action"],
            }),
        },
        output_schema: None,
        permission: PermissionContract {
            kind: PermissionKind::Execute,
            capability_base: "loop_control.control".into(),
        },
        availability: Availability::Available,
        timeout_ms: Some(30_000),
        max_concurrency: None,
        declared_permission: DeclaredPermission::Execute,
        recovery: lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE,
    };
    let registered = registry
        .register(manifest, budget)
        .expect("loop_control 工具必须登记成功");
    gateway.bind_executor(
        registered.target_id,
        Arc::new(LoopControlToolExecutor {
            controller: Arc::downgrade(&controller),
        }),
        "会话循环的控制面",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    use lingxi_kernel::toolcatalog::ToolTargetRef;
    use lingxi_protocol::{AttemptId, RunId, SessionId};
    use std::sync::atomic::AtomicBool;

    use crate::loopx::{
        Delivery, DeliveryMode, LoopAlarmService, LoopHostHooks, LoopStore, LoopTarget, LoopTimings,
    };

    fn test_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!("lingxi-r07-looptool-{}-{tag}", std::process::id()))
            .join("loop-state.json")
    }

    fn tiny_timings() -> LoopTimings {
        LoopTimings {
            delivery_retry_delays_ms: vec![1, 1, 1],
            busy_defer_ms: 1,
            max_busy_deferrals: 2,
            overdue_jitter_min_ms: 1,
            overdue_jitter_max_ms: 2,
        }
    }

    struct Harness {
        controller: Arc<LoopController>,
        #[allow(dead_code)]
        alarm: Arc<LoopAlarmService>,
        store: Arc<LoopStore>,
        live_work: Arc<AtomicBool>,
        identities: Arc<std::sync::Mutex<Vec<crate::loopx::LoopRunIdentity>>>,
        dir: std::path::PathBuf,
    }

    impl Harness {
        fn new(tag: &str) -> Self {
            let path = test_path(tag);
            let store = Arc::new(LoopStore::load(path.clone(), Box::new(|| 1_000)).expect("load"));
            let live_work = Arc::new(AtomicBool::new(false));
            let identities = Arc::new(std::sync::Mutex::new(Vec::new()));
            let hooks = {
                let live = Arc::clone(&live_work);
                let captured = identities.clone();
                LoopHostHooks {
                    has_live_background_work: Arc::new(move |_| {
                        let live = live.load(std::sync::atomic::Ordering::SeqCst);
                        Box::pin(async move { live })
                    }),
                    deliver_loop_message: Arc::new(move |_, message| {
                        captured
                            .lock()
                            .unwrap()
                            .push(message.run_identity().expect("真实inject身份"));
                        Box::pin(async { Ok(Delivery::mode(DeliveryMode::TriggerTurn)) })
                    }),
                    record_notice: Arc::new(|_, _| Box::pin(async { Ok(()) })),
                    record_user_prompt: Arc::new(|_, _| Box::pin(async { Ok(()) })),
                    is_target_mid_stream: Arc::new(|_| false),
                    is_target_runnable: Arc::new(|_| Box::pin(async { true })),
                    emit_loop_status: Arc::new(|_, _| {}),
                }
            };
            let controller = LoopController::new(Arc::clone(&store), hooks);
            let alarm = LoopAlarmService::new(
                Arc::clone(&store),
                Arc::new(|_, _| Box::pin(async { Delivery::mode(DeliveryMode::TriggerTurn) })),
                Arc::new(|_, _, _| {}),
                tiny_timings(),
                Box::new(|| 1_000),
            );
            controller.attach_alarm(Arc::clone(&alarm));
            let dir = path.parent().expect("夹具目录").to_path_buf();
            assert_eq!(dir.parent(), Some(std::env::temp_dir().as_path()));
            assert!(dir
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("lingxi-r07-looptool-"));
            Self {
                controller,
                alarm,
                store,
                live_work,
                identities,
                dir,
            }
        }
    }

    impl Drop for Harness {
        fn drop(&mut self) {
            self.alarm.dispose();
            // 只清本用例目录；共享临时目录和兄弟用例绝不可作为清理目标。
            assert_ne!(self.dir, std::env::temp_dir());
            assert_eq!(self.dir.parent(), Some(std::env::temp_dir().as_path()));
            assert!(self
                .dir
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("lingxi-r07-looptool-"));
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn ctx() -> RunContext {
        RunContext {
            principal: lingxi_kernel::Principal::LocalUser,
            session_id: SessionId::new("sess-loop"),
            run_id: RunId::new("run-loop-tool"),
            attempt: AttemptId::new("run-loop-tool#a1"),
            generation: 1,
        }
    }

    fn success_text(outcome: ToolOutcome) -> String {
        match outcome {
            ToolOutcome::Success { result } => match &result.content[0] {
                lingxi_protocol::ContentBlock::Text { text } => text.clone(),
                other => panic!("应为文本块: {other:?}"),
            },
            other => panic!("应为成功结果: {other:?}"),
        }
    }

    async fn execute(harness: &Harness, args: Value) -> ToolOutcome {
        let executor = LoopControlToolExecutor {
            controller: Arc::downgrade(&harness.controller),
        };
        let request = ToolRequest::from_effective_arguments(
            "tool:first-party:loop_control",
            args,
            &SchemaBudget::default(),
        )
        .expect("参数必须在预算内");
        executor
            .execute(&ctx(), &ToolCallId::new("call-loop"), &request)
            .await
            .outcome
    }

    #[tokio::test]
    async fn gate_rejects_when_no_active_loop() {
        let harness = Harness::new("gate");
        let outcome = execute(&harness, json!({"action":"complete"})).await;
        assert_eq!(
            success_text(outcome),
            "No active loop in this session. Start one with /loop <task>."
        );
        // stopped/completed 同样不在活跃集内。
        harness
            .store
            .create(&LoopTarget::desktop("sess-loop"), "任务", None)
            .expect("create");
        harness
            .store
            .update("sess-loop", |record| record.status = LoopStatus::Stopped)
            .expect("stop");
        let outcome = execute(&harness, json!({"action":"schedule","delay_seconds":60})).await;
        assert_eq!(
            success_text(outcome),
            "No active loop in this session. Start one with /loop <task>."
        );
    }

    #[tokio::test]
    async fn schedule_requires_delay_and_clamps_to_minimum() {
        let harness = Harness::new("schedule");
        harness
            .store
            .create(&LoopTarget::desktop("sess-loop"), "任务", None)
            .expect("create");
        let outcome = execute(&harness, json!({"action":"schedule"})).await;
        assert_eq!(
            success_text(outcome),
            "delay_seconds is required for action=schedule."
        );
        // 5s 钳到 minDelaySec=60；wakeAt 为 toISOString 同形。
        let outcome = execute(
            &harness,
            json!({"action":"schedule","delay_seconds":5,"reason":"看下队列"}),
        )
        .await;
        assert_eq!(
            success_text(outcome),
            "Wakeup scheduled in 60s (fires at 1970-01-01T00:01:01.000Z). It replaces any previous alarm."
        );
        let slot = harness.store.get("sess-loop").unwrap().alarm.unwrap();
        assert_eq!(slot.wake_at, 1_000 + 60_000);
        assert_eq!(slot.reason, "看下队列");
    }

    #[tokio::test]
    async fn schedule_refusal_returns_message_text() {
        let harness = Harness::new("refuse");
        harness
            .store
            .create(&LoopTarget::desktop("sess-loop"), "任务", None)
            .expect("create");
        harness
            .live_work
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let outcome = execute(&harness, json!({"action":"schedule","delay_seconds":300})).await;
        assert_eq!(
            success_text(outcome),
            "Background work in this session wakes it automatically on completion; do not schedule alarms to poll it. While background work is live, alarms must be long fallbacks (>= 1200s)."
        );
    }

    #[tokio::test]
    async fn complete_closes_loop_with_summary() {
        let harness = Harness::new("complete");
        harness
            .store
            .create(&LoopTarget::desktop("sess-loop"), "任务", None)
            .expect("create");
        let outcome = execute(&harness, json!({"action":"complete","reason":"目标达成"})).await;
        assert_eq!(success_text(outcome), "Loop completed and closed.");
        let record = harness.store.get("sess-loop").expect("record");
        assert_eq!(record.status, LoopStatus::Completed);
        assert_eq!(record.completed_summary.as_deref(), Some("目标达成"));
    }

    #[tokio::test]
    async fn registered_executor_releases_loop_owner_and_reports_shutdown_as_failure() {
        use crate::approval_service::ApprovalService;
        use crate::inject::{ServiceClock, SystemClock};

        let harness = Harness::new("weak-service-owner");
        let owner = Arc::downgrade(&harness.controller);
        let executor = LoopControlToolExecutor {
            controller: owner.clone(),
        };
        let registry = Arc::new(ToolRegistry::new());
        let clock: Arc<dyn ServiceClock> = Arc::new(SystemClock);
        let gateway = ToolInvocationGateway::new(
            registry.clone(),
            Arc::new(ApprovalService::new(clock.clone())),
            clock,
            SchemaBudget::default(),
            30_000,
            64,
        );
        register_loop_control_tool(
            &registry,
            &gateway,
            harness.controller.clone(),
            &SchemaBudget::default(),
        );
        // 真实网关及执行器绑定表仍存活；不能用解绑伪造服务释放。
        drop(harness);
        assert!(owner.upgrade().is_none());
        assert!(gateway
            .executor_binding_reasons()
            .iter()
            .any(|(target, _)| target.contains("loop_control")));
        let request = ToolRequest::from_effective_arguments(
            "tool:first-party:loop_control",
            json!({"action":"complete"}),
            &SchemaBudget::default(),
        )
        .unwrap();
        let result = executor
            .execute(&ctx(), &ToolCallId::new("after-shutdown"), &request)
            .await;
        match result.outcome {
            ToolOutcome::Failed { error } => assert!(error.message.contains("服务已退出")),
            other => panic!("服务释放后的旧执行器必须明确失败: {other:?}"),
        }
    }

    #[tokio::test]
    async fn old_loop_executor_calls_cannot_mutate_a_restarted_loop() {
        let harness = Harness::new("old-loop-control-identity");
        let target = crate::loopx::LoopTarget::desktop("sess-loop");
        harness.controller.start(&target, "old").await.unwrap();
        let identity = harness.identities.lock().unwrap()[0].clone();
        let guard = harness
            .controller
            .register_loop_run("sess-loop", ctx().run_id.as_str(), &identity)
            .unwrap();
        harness.controller.stop(&target).await.unwrap();
        harness.controller.start(&target, "new").await.unwrap();
        for args in [
            json!({"action":"schedule","delay_seconds":60}),
            json!({"action":"complete","reason":"old result"}),
        ] {
            let text = success_text(execute(&harness, args).await);
            assert!(text.contains("取代"), "{text}");
            let record = harness.store.get("sess-loop").unwrap();
            assert_eq!(record.status, LoopStatus::Running);
            assert_eq!(record.prompt, "new");
            assert!(record.alarm.is_none());
            assert!(record.completed_summary.is_none());
        }
        harness
            .controller
            .on_run_end("sess-loop", ctx().run_id.as_str(), true)
            .await;
        drop(guard);
        assert!(harness.controller.storage_error("sess-loop").is_none());
    }

    #[test]
    fn registration_resolves_fixed_execute_permission() {
        use crate::approval_service::ApprovalService;
        use crate::inject::{ServiceClock, SystemClock};

        let harness = Harness::new("register");
        let registry = Arc::new(ToolRegistry::new());
        let clock: Arc<dyn ServiceClock> = Arc::new(SystemClock);
        let approvals = Arc::new(ApprovalService::new(Arc::clone(&clock)));
        let budget = SchemaBudget::default();
        let gateway = ToolInvocationGateway::new(
            Arc::clone(&registry),
            approvals,
            clock,
            SchemaBudget::default(),
            30_000,
            64,
        );
        register_loop_control_tool(
            &registry,
            &gateway,
            Arc::clone(&harness.controller),
            &budget,
        );
        let reference = ToolTargetRef::BySourceAndName {
            source_id: "first-party".to_string(),
            name: "loop_control".to_string(),
        };
        let prepared = registry
            .prepare_invocation(
                &reference,
                None,
                &json!({"action":"schedule","delay_seconds":60}),
                &budget,
            )
            .expect("loop_control 调用必须可准备");
        assert_eq!(prepared.permission.kind, PermissionKind::Execute);
        assert_eq!(prepared.permission.capability_base, "loop_control.control");
    }
    #[tokio::test]
    async fn fixture_cleanup_preserves_shared_temporary_directory_and_siblings() {
        let sibling = std::env::temp_dir().join(format!(
            "lingxi-r07-cleanup-sentinel-{}",
            crate::auth::hex_random_public(8).unwrap()
        ));
        std::fs::create_dir_all(&sibling).unwrap();
        let sentinel = sibling.join("keep.txt");
        std::fs::write(&sentinel, "keep").unwrap();
        let fixture = Harness::new("cleanup-boundary");
        let own = fixture.dir.clone();
        std::fs::create_dir_all(&own).unwrap();
        drop(fixture);
        assert!(!own.exists());
        assert_eq!(std::fs::read_to_string(&sentinel).unwrap(), "keep");
        assert!(std::env::temp_dir().is_dir());
        std::fs::remove_dir_all(sibling).unwrap();
    }
}
