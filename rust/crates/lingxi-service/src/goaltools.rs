//! R07-T07-δ3 goal 工具（现役 `lib/tools/goal-tool.ts` 的端口）。
//!
//! 五动作 create/status/pause/resume/complete/drop，会话键取
//! `RunContext.session_id`（内核上下文恒定携带会话身份，现役
//! "no active session" 分支在结构上不可达）；引擎由组合根注入（现役
//! "goal engine unavailable" 分支同样不可达）。description、参数 schema
//! （含各字段 description）与全部结果文本逐字平移现役。
//!
//! 权限：kernel `PermissionKind::ArgumentAwareGoal`（先例
//! ArgumentAwareCheckpoint/Channel/Browser）——action 缺省或 status →
//! Read/goal.status；其余 → Execute/goal.<action>（现役 routine 档对应
//! Execute，checkpoint 先例同）。
//!
//! 现役映射说明：
//! - 全局默认预算读 `user/preferences.json` 的 `goal.default_token_budget` /
//!   `default_time_budget_minutes`；现役 `preferences-manager._readFromDisk`
//!   原先损坏时回落无默认；当前按项目规则明确返回失败并保留损坏原件。
//! - 侧车账本 token 字段为 u64（δ1 设计）：默认预算若为分数则按
//!   Math.round 同义四舍五入（现役对显式工具参数本就 Math.round；仅默认
//!   值的分数边角按此取舍）。
//! - 归一化后 schema 的 type:"number" 不拒 null（kernel 既定行为），现役
//!   「显式 null 清除默认」路径保持可达。

use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use lingxi_kernel::ports::{ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest};
use lingxi_kernel::toolcatalog::{
    Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
    ToolManifest, ToolOrigin, ToolRegistry,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{ErrorCode, ProtocolError, ToolCallId, ToolSchemaDocument};
use serde_json::{json, Value};

use crate::goal::{CreateGoalInput, CreateGoalOutcome, GoalEngine};
use crate::memory::MemoryService;
use crate::toolgateway::ToolInvocationGateway;

const DESCRIPTION: &str = "Track a budget for a long-running task: create a goal with a token budget and/or wall-clock time budget, then check status anytime. When the budget is exceeded you get exactly one over-budget notice (the user sees one too) — wrap up and hand the decision back instead of silently continuing. User aborts pause the goal automatically. Budgets are advisory reminders, not hard stops; status/pause/resume/complete/drop manage the lifecycle.";

/// 现役 fmtBudget：token 与时间预算各一段，" + " 连接，皆无 → "none"。
fn fmt_budget(token_budget: Option<u64>, time_budget_ms: Option<u64>) -> String {
    let mut parts = Vec::new();
    if let Some(tokens) = token_budget {
        parts.push(format!("{tokens} tokens"));
    }
    if let Some(ms) = time_budget_ms {
        parts.push(format!("{} min", (ms as f64 / 60_000.0).round() as u64));
    }
    if parts.is_empty() {
        "none".to_string()
    } else {
        parts.join(" + ")
    }
}

/// 现役 agent.ts getDefaultBudgets：preferences.goal 的
/// default_token_budget（正数）与 default_time_budget_minutes（正数 →
/// Math.round(min*60000)）。读取失败明确报告，不能悄悄改用其他预算。
fn default_budgets(home: &Path) -> Result<(Option<u64>, Option<u64>), String> {
    let root = MemoryService::read_preferences_root_at_home(home)
        .map_err(|error| format!("goal preferences read failed: {error}"))?;
    let cfg = &root["goal"];
    let token = cfg["default_token_budget"]
        .as_f64()
        .filter(|value| *value > 0.0)
        .map(|value| value.round() as u64);
    let time_ms = cfg["default_time_budget_minutes"]
        .as_f64()
        .filter(|value| *value > 0.0)
        .map(|value| (value * 60_000.0).round() as u64);
    Ok((token, time_ms))
}

/// 现役 create 参数归一化：显式正数优先；显式 null 清除默认；其余落全局
/// 默认（number 但 ≤0 也落默认）。
fn resolve_budgets(args: &Value, home: &Path) -> Result<(Option<u64>, Option<u64>), String> {
    let (def_token, def_time_ms) = default_budgets(home)?;
    let mut token_budget = match args.get("token_budget") {
        Some(Value::Number(number)) => number
            .as_f64()
            .filter(|value| *value > 0.0)
            .map(|value| value.round() as u64)
            .or(def_token),
        _ => def_token,
    };
    if args.get("token_budget") == Some(&Value::Null) {
        token_budget = None;
    }
    let mut time_budget_ms = match args.get("time_budget_minutes") {
        Some(Value::Number(number)) => number
            .as_f64()
            .filter(|value| *value > 0.0)
            .map(|value| (value * 60_000.0).round() as u64)
            .or(def_time_ms),
        _ => def_time_ms,
    };
    if args.get("time_budget_minutes") == Some(&Value::Null) {
        time_budget_ms = None;
    }
    Ok((token_budget, time_budget_ms))
}

fn failed(message: impl Into<String>) -> ToolOutcome {
    ToolOutcome::Failed {
        error: ProtocolError::new(ErrorCode::InvalidMessage, message.into(), false),
    }
}

fn success(text: impl Into<String>, details: Value) -> ToolOutcome {
    let mut result = lingxi_kernel::ports::ToolSuccess::text(text.into());
    result.details = details.as_object().cloned();
    ToolOutcome::Success { result }
}

struct GoalToolExecutor {
    engine: Arc<GoalEngine>,
    home: PathBuf,
}

impl GoalToolExecutor {
    fn run(&self, ctx: &RunContext, args: &Value) -> ToolOutcome {
        let session_id = ctx.session_id.as_str();
        // 现役：未识别 action 一律回落 status（schema 枚举校验之外的防线）。
        let raw_action = args
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("status");
        let action = if matches!(
            raw_action,
            "create" | "status" | "pause" | "resume" | "complete" | "drop"
        ) {
            raw_action
        } else {
            "status"
        };

        if action == "status" {
            let (text, goal) = match self.engine.status_checked(session_id) {
                Ok(status) => status,
                Err(error) => return failed(error),
            };
            let mut details = json!({ "hasGoal": goal.is_some() });
            if let Some(goal) = goal {
                let progress = crate::goal::goal_progress(&goal, self.engine.now_ms());
                details["goal"] = json!({
                    "name": goal.name,
                    "status": goal.status.wire_name(),
                    "tokensUsed": goal.tokens_used,
                    "tokenBudget": goal.token_budget,
                    "timeBudgetMs": goal.time_budget_ms,
                    "overBudget": progress.over_budget,
                });
            }
            return success(text, details);
        }

        if action == "create" {
            let (token_budget, time_budget_ms) = match resolve_budgets(args, &self.home) {
                Ok(budgets) => budgets,
                Err(error) => return failed(error),
            };
            let name = args.get("name").and_then(Value::as_str).map(str::to_string);
            return match self.engine.create(
                session_id,
                CreateGoalInput {
                    name,
                    token_budget,
                    time_budget_ms,
                },
            ) {
                CreateGoalOutcome::Err(message) => failed(message),
                CreateGoalOutcome::Ok(goal) => success(
                    format!(
                        "goal \"{}\" created — budget: {}. You will get exactly one over-budget notice; check status anytime.",
                        goal.name,
                        fmt_budget(goal.token_budget, goal.time_budget_ms)
                    ),
                    json!({ "goal": serde_json::to_value(&goal).expect("goal state must serialize") }),
                ),
            };
        }

        if action == "pause" {
            let ok = match self.engine.pause_checked(session_id, "") {
                Ok(ok) => ok,
                Err(error) => return failed(error),
            };
            return success(
                if ok {
                    "goal paused (time budget stops counting)"
                } else {
                    "no active goal to pause"
                },
                json!({ "ok": ok }),
            );
        }
        if action == "resume" {
            let ok = match self.engine.resume_checked(session_id) {
                Ok(ok) => ok,
                Err(error) => return failed(error),
            };
            return success(
                if ok {
                    "goal resumed"
                } else {
                    "no paused goal to resume"
                },
                json!({ "ok": ok }),
            );
        }
        if action == "complete" {
            let ok = match self.engine.complete_checked(session_id) {
                Ok(ok) => ok,
                Err(error) => return failed(error),
            };
            let text = if ok {
                let (text, _) = match self.engine.status_checked(session_id) {
                    Ok(status) => status,
                    Err(error) => return failed(error),
                };
                format!("goal completed. Final readout:\n{text}")
            } else {
                "no active/paused goal to complete".to_string()
            };
            return success(text, json!({ "ok": ok }));
        }
        let ok = match self.engine.drop_goal_checked(session_id) {
            Ok(ok) => ok,
            Err(error) => return failed(error),
        };
        success(
            if ok {
                "goal dropped"
            } else {
                "no active/paused goal to drop"
            },
            json!({ "ok": ok }),
        )
    }
}

impl ToolExecutorPort for GoalToolExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            ToolExecutionResult::of_ctx(ctx, self.run(ctx, request.arguments.as_value()))
        })
    }
}

/// 与其他正式工具同口径登记：网关 prepare → 权限/批准 → 执行。
pub fn register_goal_tool(
    registry: &ToolRegistry,
    gateway: &ToolInvocationGateway,
    engine: Arc<GoalEngine>,
    home: PathBuf,
    budget: &SchemaBudget,
) {
    let manifest = ToolManifest {
        origin: ToolOrigin::FirstParty,
        local_name: "goal".into(),
        display_name: "goal".into(),
        aliases: Vec::new(),
        version: "1.0.0".into(),
        description: DESCRIPTION.into(),
        input_schema: ToolSchemaDocument {
            dialect: "json-schema/2020-12".into(),
            schema: json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["create", "status", "pause", "resume", "complete", "drop"], "description": "create: start a budgeted goal (replaces a finished/dropped one; only one active at a time). status: progress readout (default). pause/resume/complete/drop: lifecycle"},
                    "name": {"type": "string", "description": "Short goal name shown in status"},
                    "token_budget": {"type": "number", "description": "Total token budget for the goal (omit to use the global default, or set null with time_budget_minutes given)"},
                    "time_budget_minutes": {"type": "number", "description": "Wall-clock time budget in minutes (pause time excluded)"},
                },
            }),
        },
        output_schema: None,
        permission: PermissionContract {
            kind: PermissionKind::ArgumentAwareGoal,
            capability_base: "goal.action".into(),
        },
        availability: Availability::Available,
        timeout_ms: Some(30_000),
        max_concurrency: None,
        declared_permission: DeclaredPermission::Execute,
        recovery: lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE,
    };
    let registered = registry
        .register(manifest, budget)
        .expect("goal 工具必须登记成功");
    gateway.bind_executor(
        registered.target_id,
        Arc::new(GoalToolExecutor { engine, home }),
        "当前会话的预算目标账本",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    use lingxi_protocol::{AttemptId, RunId, SessionId};

    fn test_home(tag: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("lingxi-r07-goaltool-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("home 目录必须可建");
        root
    }

    fn engine_at(root: &Path) -> Arc<GoalEngine> {
        Arc::new(GoalEngine::new(
            root.to_path_buf(),
            Arc::new(|_session, _goal, _kind| {}),
            Box::new(|| 10_000),
        ))
    }

    fn ctx() -> RunContext {
        RunContext {
            principal: lingxi_kernel::Principal::LocalUser,
            session_id: SessionId::new("sess-goal"),
            run_id: RunId::new("run-goal"),
            attempt: AttemptId::new("run-goal#a1"),
            generation: 1,
        }
    }

    fn request(args: Value) -> ToolRequest {
        ToolRequest::from_effective_arguments(
            "tool:first-party:goal",
            args,
            &SchemaBudget::default(),
        )
        .expect("参数必须在预算内")
    }

    fn execute(executor: &GoalToolExecutor, args: Value) -> ToolOutcome {
        let ctx = ctx();
        let call = ToolCallId::new("call-1");
        let request = request(args);
        let result = futures_lite_block_on(executor.execute(&ctx, &call, &request));
        result.outcome
    }

    fn futures_lite_block_on<F: std::future::Future>(future: F) -> F::Output {
        // 同步执行器即刻就绪；用最小 noop waker 避免给单测拉 tokio。
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
        fn noop(_: *const ()) {}
        fn clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
        let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
        let mut context = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("goal 工具执行器必须同步就绪"),
        }
    }

    fn success_text(outcome: &ToolOutcome) -> &str {
        match outcome {
            ToolOutcome::Success { result } => match &result.content[0] {
                lingxi_protocol::ContentBlock::Text { text } => text.as_str(),
                other => panic!("应为文本块: {other:?}"),
            },
            other => panic!("应为成功结果: {other:?}"),
        }
    }

    fn failed_message(outcome: &ToolOutcome) -> &str {
        match outcome {
            ToolOutcome::Failed { error } => error.message.as_str(),
            other => panic!("应为失败结果: {other:?}"),
        }
    }

    fn details(outcome: &ToolOutcome) -> &serde_json::Map<String, Value> {
        match outcome {
            ToolOutcome::Success { result } => result.details.as_ref().expect("应有 details"),
            other => panic!("应为成功结果: {other:?}"),
        }
    }

    fn write_prefs(home: &Path, goal: Value) {
        let user_dir = home.join("user");
        std::fs::create_dir_all(&user_dir).expect("user 目录必须可建");
        crate::paths::atomic_write(
            &user_dir.join("preferences.json"),
            serde_json::to_string_pretty(&json!({ "goal": goal }))
                .expect("偏好必须可序列化")
                .as_bytes(),
        )
        .expect("偏好写入必须成功");
    }

    #[test]
    fn status_without_goal_is_incumbent_text() {
        let home = test_home("status-empty");
        let executor = GoalToolExecutor {
            engine: engine_at(&home),
            home: home.clone(),
        };
        let outcome = execute(&executor, json!({}));
        assert_eq!(success_text(&outcome), "no goal in this session");
        assert_eq!(details(&outcome)["hasGoal"], json!(false));
        assert!(details(&outcome).get("goal").is_none());
    }

    #[test]
    fn create_with_explicit_budgets_reports_verbatim() {
        let home = test_home("create-explicit");
        let executor = GoalToolExecutor {
            engine: engine_at(&home),
            home: home.clone(),
        };
        let outcome = execute(
            &executor,
            json!({"action":"create","name":"ship it","token_budget":500,"time_budget_minutes":2}),
        );
        assert_eq!(
            success_text(&outcome),
            "goal \"ship it\" created — budget: 500 tokens + 2 min. You will get exactly one over-budget notice; check status anytime."
        );
        let goal = &details(&outcome)["goal"];
        assert_eq!(goal["name"], json!("ship it"));
        assert_eq!(goal["tokenBudget"], json!(500));
        assert_eq!(goal["timeBudgetMs"], json!(120_000));
        assert_eq!(goal["status"], json!("active"));
        // 侧车已落盘。
        assert!(home
            .join("goal-sidecars")
            .join("sess-goal.goal.json")
            .exists());
    }

    #[test]
    fn create_falls_back_to_global_defaults() {
        let home = test_home("create-defaults");
        write_prefs(
            &home,
            json!({"enabled":true,"default_token_budget":2500,"default_time_budget_minutes":45}),
        );
        let executor = GoalToolExecutor {
            engine: engine_at(&home),
            home: home.clone(),
        };
        let outcome = execute(&executor, json!({"action":"create"}));
        let goal = &details(&outcome)["goal"];
        assert_eq!(goal["tokenBudget"], json!(2500));
        assert_eq!(goal["timeBudgetMs"], json!(2_700_000));
        assert_eq!(
            success_text(&outcome),
            "goal \"goal\" created — budget: 2500 tokens + 45 min. You will get exactly one over-budget notice; check status anytime."
        );
    }

    #[test]
    fn create_null_budget_clears_default() {
        let home = test_home("create-null");
        write_prefs(
            &home,
            json!({"default_token_budget":2500,"default_time_budget_minutes":45}),
        );
        let executor = GoalToolExecutor {
            engine: engine_at(&home),
            home: home.clone(),
        };
        let outcome = execute(
            &executor,
            json!({"action":"create","token_budget":null,"time_budget_minutes":1}),
        );
        let goal = &details(&outcome)["goal"];
        assert_eq!(goal["tokenBudget"], Value::Null);
        assert_eq!(goal["timeBudgetMs"], json!(60_000));
    }

    #[test]
    fn create_without_any_budget_fails_verbatim() {
        let home = test_home("create-none");
        let executor = GoalToolExecutor {
            engine: engine_at(&home),
            home: home.clone(),
        };
        let outcome = execute(&executor, json!({"action":"create"}));
        assert_eq!(
            failed_message(&outcome),
            "at least one budget is required (token_budget or time_budget_minutes)"
        );
    }

    #[test]
    fn create_second_active_goal_fails_verbatim() {
        let home = test_home("create-twice");
        let executor = GoalToolExecutor {
            engine: engine_at(&home),
            home: home.clone(),
        };
        execute(&executor, json!({"action":"create","token_budget":100}));
        let outcome = execute(&executor, json!({"action":"create","token_budget":200}));
        assert_eq!(
            failed_message(&outcome),
            "goal \"goal\" is already active — complete or drop it first (status shows its progress)"
        );
    }

    #[test]
    fn corrupt_preferences_fail_explicitly_and_keep_original() {
        let home = test_home("create-corrupt");
        let user_dir = home.join("user");
        std::fs::create_dir_all(&user_dir).expect("user 目录必须可建");
        std::fs::write(user_dir.join("preferences.json"), b"{not json").expect("偏好写入必须成功");
        let executor = GoalToolExecutor {
            engine: engine_at(&home),
            home: home.clone(),
        };
        // 损坏偏好必须明确失败，显式预算也不能掩盖读取错误。
        let outcome = execute(&executor, json!({"action":"create"}));
        assert!(matches!(outcome, ToolOutcome::Failed { .. }));
        assert!(failed_message(&outcome).contains("goal preferences read failed"));
        let outcome = execute(&executor, json!({"action":"create","token_budget":10}));
        assert!(failed_message(&outcome).contains("goal preferences read failed"));
        assert_eq!(
            std::fs::read(user_dir.join("preferences.json")).unwrap(),
            b"{not json"
        );
        assert!(!home.join("goal-sidecars/sess-goal.goal.json").exists());
    }

    #[test]
    fn corrupt_goal_storage_fails_every_tool_action_and_preserves_original() {
        let home = test_home("corrupt-sidecar");
        std::fs::create_dir_all(home.join("goal-sidecars")).unwrap();
        let path = home.join("goal-sidecars/sess-goal.goal.json");
        std::fs::write(&path, b"{broken goal").unwrap();
        let executor = GoalToolExecutor {
            engine: engine_at(&home),
            home: home.clone(),
        };
        for action in ["status", "create", "pause", "resume", "complete", "drop"] {
            let outcome = execute(&executor, json!({"action":action,"token_budget":100}));
            assert!(
                failed_message(&outcome).contains("goal storage is corrupt"),
                "{action}: {outcome:?}"
            );
            assert_eq!(std::fs::read(&path).unwrap(), b"{broken goal");
        }
    }

    #[test]
    fn tool_write_failure_does_not_claim_pause_complete_or_drop_succeeded() {
        let home = test_home("tool-write-fault");
        let executor = GoalToolExecutor {
            engine: engine_at(&home),
            home: home.clone(),
        };
        assert!(matches!(
            execute(&executor, json!({"action":"create","token_budget":100})),
            ToolOutcome::Success { .. }
        ));
        let path = home.join("goal-sidecars/sess-goal.goal.json");
        let before = std::fs::read(&path).unwrap();
        std::fs::write(home.join("file-recovery-records"), b"blocker").unwrap();
        for action in ["pause", "complete", "drop"] {
            let outcome = execute(&executor, json!({"action":action}));
            assert!(
                failed_message(&outcome).contains("goal storage write failed"),
                "{action}: {outcome:?}"
            );
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
    }

    #[test]
    fn status_after_create_has_incumbent_details() {
        let home = test_home("status-full");
        let executor = GoalToolExecutor {
            engine: engine_at(&home),
            home: home.clone(),
        };
        execute(
            &executor,
            json!({"action":"create","name":"readout","token_budget":100}),
        );
        let outcome = execute(&executor, json!({"action":"status"}));
        assert_eq!(details(&outcome)["hasGoal"], json!(true));
        let goal = &details(&outcome)["goal"];
        assert_eq!(goal["name"], json!("readout"));
        assert_eq!(goal["status"], json!("active"));
        assert_eq!(goal["tokensUsed"], json!(0));
        assert_eq!(goal["tokenBudget"], json!(100));
        assert_eq!(goal["timeBudgetMs"], Value::Null);
        assert_eq!(goal["overBudget"], json!(false));
        assert!(success_text(&outcome).starts_with("goal \"readout\" — active\n"));
    }

    #[test]
    fn pause_resume_complete_drop_texts_verbatim() {
        let home = test_home("lifecycle");
        let executor = GoalToolExecutor {
            engine: engine_at(&home),
            home: home.clone(),
        };
        // 负向先行（无 goal）。
        let outcome = execute(&executor, json!({"action":"pause"}));
        assert_eq!(success_text(&outcome), "no active goal to pause");
        assert_eq!(details(&outcome)["ok"], json!(false));
        let outcome = execute(&executor, json!({"action":"resume"}));
        assert_eq!(success_text(&outcome), "no paused goal to resume");
        let outcome = execute(&executor, json!({"action":"complete"}));
        assert_eq!(success_text(&outcome), "no active/paused goal to complete");
        let outcome = execute(&executor, json!({"action":"drop"}));
        assert_eq!(success_text(&outcome), "no active/paused goal to drop");

        execute(&executor, json!({"action":"create","token_budget":100}));
        let outcome = execute(&executor, json!({"action":"pause"}));
        assert_eq!(
            success_text(&outcome),
            "goal paused (time budget stops counting)"
        );
        assert_eq!(details(&outcome)["ok"], json!(true));
        let outcome = execute(&executor, json!({"action":"resume"}));
        assert_eq!(success_text(&outcome), "goal resumed");
        let outcome = execute(&executor, json!({"action":"complete"}));
        let text = success_text(&outcome);
        assert!(
            text.starts_with("goal completed. Final readout:\ngoal \"goal\" — completed\n"),
            "{text}"
        );
        let outcome = execute(&executor, json!({"action":"drop"}));
        assert_eq!(success_text(&outcome), "no active/paused goal to drop");
    }

    #[test]
    fn drop_after_complete_removes_and_drop_texts() {
        let home = test_home("drop");
        let executor = GoalToolExecutor {
            engine: engine_at(&home),
            home: home.clone(),
        };
        execute(&executor, json!({"action":"create","token_budget":100}));
        let outcome = execute(&executor, json!({"action":"drop"}));
        assert_eq!(success_text(&outcome), "goal dropped");
        assert_eq!(details(&outcome)["ok"], json!(true));
        let outcome = execute(&executor, json!({"action":"status"}));
        assert!(success_text(&outcome).contains("— dropped"));
    }

    #[test]
    fn unknown_action_falls_back_to_status() {
        let home = test_home("unknown-action");
        let executor = GoalToolExecutor {
            engine: engine_at(&home),
            home: home.clone(),
        };
        let outcome = execute(&executor, json!({"action":"bogus"}));
        assert_eq!(success_text(&outcome), "no goal in this session");
    }

    #[test]
    fn fmt_budget_matches_incumbent() {
        assert_eq!(fmt_budget(None, None), "none");
        assert_eq!(fmt_budget(Some(500), None), "500 tokens");
        assert_eq!(fmt_budget(None, Some(90_000)), "2 min");
        assert_eq!(fmt_budget(Some(500), Some(120_000)), "500 tokens + 2 min");
    }

    #[test]
    fn registration_resolves_permission_per_action() {
        use crate::approval_service::ApprovalService;
        use crate::inject::{ServiceClock, SystemClock};
        use lingxi_kernel::toolcatalog::ToolTargetRef;

        let home = test_home("register");
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
        register_goal_tool(&registry, &gateway, engine_at(&home), home.clone(), &budget);

        let reference = ToolTargetRef::BySourceAndName {
            source_id: "first-party".to_string(),
            name: "goal".to_string(),
        };
        let prepare = |args: Value| {
            registry
                .prepare_invocation(&reference, None, &args, &budget)
                .expect("goal 调用必须可准备")
        };
        // 现役：action 缺省或 status → read/goal.status。
        for args in [json!({}), json!({"action":"status"})] {
            let prepared = prepare(args);
            assert_eq!(prepared.permission.kind, PermissionKind::Read);
            assert_eq!(prepared.permission.capability_base, "goal.status");
        }
        // 现役：其余动作 → routine（Execute）/goal.<action>。
        for (action, capability) in [
            ("create", "goal.create"),
            ("pause", "goal.pause"),
            ("resume", "goal.resume"),
            ("complete", "goal.complete"),
            ("drop", "goal.drop"),
        ] {
            let prepared = prepare(json!({"action": action}));
            assert_eq!(
                prepared.permission.kind,
                PermissionKind::Execute,
                "{action}"
            );
            assert_eq!(prepared.permission.capability_base, capability, "{action}");
        }
    }
}
