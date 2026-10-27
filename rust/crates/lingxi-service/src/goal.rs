//! goal.rs — 会话预算引擎（R07-T07 δ1；现役 lib/goal/goal-engine.ts 平移）。
//!
//! 每会话至多一个 goal（token 预算 / 时间预算 / 两者），状态机
//! active ⇄ paused → completed/dropped。用量在模型调用账本落盘点累计
//! （runs.rs persist_model_call_completed 钩子）；时间预算在检查点懒算
//! （pause 时长不计）。超支提醒「恰好一次」：token 或时间越限时置
//! overBudgetNotified，经注入回调提醒模型 + 通知回调给用户；之后 goal
//! 保持 active（预算是提醒不是硬闸——是否继续由模型/用户定，状态随时
//! 可查）。用户中止轮 → 自动 pause（reason=user_abort）。
//!
//! 状态持久化在会话侧车（原子写），server 重启后恢复；会话删除/回档不
//! 清理它——goal 只是会话的伴随账本，孤儿侧车无副作用。诚实边界：账本
//! 落盘的调用才记账（中止且未落盘的 run 不记账，与聊天页口径一致）。
//!
//! 登记偏差（T07_STATUS δ1）：现役侧车位于 `${sessionPath}.goal.json`；
//! Rust 无 sessionPath（会话是 DB 行），侧车落
//! `data_home/goal-sidecars/<session_id>.goal.json`，JSON 内容逐字段平价
//! （camelCase、schemaVersion 1、2 空格缩进、无尾换行）。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// 现役 `defaultGoal` 的 schemaVersion 常量。
pub const GOAL_SCHEMA_VERSION: u32 = 1;

/// 超支提醒的种类（现役 `kind: "tokens" | "time"`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverBudgetKind {
    Tokens,
    Time,
}

impl OverBudgetKind {
    pub fn wire_name(self) -> &'static str {
        match self {
            OverBudgetKind::Tokens => "tokens",
            OverBudgetKind::Time => "time",
        }
    }
}

/// 现役 GoalStatus：active ⇄ paused → completed/dropped。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GoalStatus {
    Active,
    Paused,
    Completed,
    Dropped,
}

impl GoalStatus {
    pub fn wire_name(self) -> &'static str {
        match self {
            GoalStatus::Active => "active",
            GoalStatus::Paused => "paused",
            GoalStatus::Completed => "completed",
            GoalStatus::Dropped => "dropped",
        }
    }
}

/// 通知回调本机发布的回执；未确认时不自动重发，避免重复打扰。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalNoticeReceipt {
    Unconfirmed,
    Published,
}

/// 现役 GoalState 逐字段同形（camelCase JSON；字段序 = 现役对象字面量序，
/// serde 按声明序序列化，字节级平价）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalState {
    pub schema_version: u32,
    pub name: String,
    pub token_budget: Option<u64>,
    pub time_budget_ms: Option<u64>,
    pub tokens_used: u64,
    pub started_at: u64,
    /// pause 累计（ms）：活动时长 = now - startedAt - pausedMs -（paused 中则
    /// currentPausedAt 起算）。
    pub paused_ms: u64,
    pub current_paused_at: Option<u64>,
    pub status: GoalStatus,
    pub paused_reason: Option<String>,
    pub completed_at: Option<u64>,
    pub over_budget_notified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub over_budget_notice_receipt: Option<GoalNoticeReceipt>,
    pub last_active_at: u64,
}

/// 现役 create 的输入（goal-tool 归一化后传入）。
#[derive(Debug, Clone, Default)]
pub struct CreateGoalInput {
    pub name: Option<String>,
    pub token_budget: Option<u64>,
    pub time_budget_ms: Option<u64>,
}

/// 现役 create 的结果形状（{ok, error?, goal?}）。
#[derive(Debug, Clone, PartialEq)]
pub enum CreateGoalOutcome {
    Ok(GoalState),
    Err(String),
}

/// 现役 recordTokenUsage 的返回（{goal, newlyOver}）。
#[derive(Debug, Clone)]
pub struct RecordedUsage {
    pub goal: GoalState,
    pub newly_over: bool,
}

/// 超支提醒回调（恰好一次）：注入模型可见消息 + 用户通知。参数 =
/// （会话键， 超支后的 goal 快照， 越限种类）。实现约定：回调失败由调用方
/// 只记 tracing，不吞账本（现役 try/catch 语义）。
pub type NotifyOverBudget = Arc<dyn Fn(&str, &GoalState, OverBudgetKind) + Send + Sync>;

/// 进度读数（现役 goalProgress 返回形状）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GoalProgress {
    pub tokens_used: u64,
    pub token_budget: Option<u64>,
    pub token_pct: Option<u64>,
    pub token_over: bool,
    pub active_ms: u64,
    pub time_budget_ms: Option<u64>,
    pub time_pct: Option<u64>,
    pub time_over: bool,
    pub over_budget: bool,
}

/// 活动时长（不含暂停；现役 goalActiveMs）。
pub fn goal_active_ms(goal: &GoalState, now: u64) -> u64 {
    let paused_current = match goal.current_paused_at {
        Some(at) => now.saturating_sub(at),
        None => 0,
    };
    now.saturating_sub(goal.started_at)
        .saturating_sub(goal.paused_ms)
        .saturating_sub(paused_current)
}

/// 现役 goalProgress：pct = Math.round(used/budget*100)（budget 缺省或 0 →
/// None；JS Math.round 对正数等价 f64::round 的 half-away-from-zero）。
/// over 严格大于。
pub fn goal_progress(goal: &GoalState, now: u64) -> GoalProgress {
    let token_pct = match goal.token_budget {
        Some(budget) if budget > 0 => {
            Some(((goal.tokens_used as f64 / budget as f64) * 100.0).round() as u64)
        }
        _ => None,
    };
    let token_over = match goal.token_budget {
        Some(budget) => goal.tokens_used > budget,
        None => false,
    };
    let active_ms = goal_active_ms(goal, now);
    let time_pct = match goal.time_budget_ms {
        Some(budget) if budget > 0 => {
            Some(((active_ms as f64 / budget as f64) * 100.0).round() as u64)
        }
        _ => None,
    };
    let time_over = match goal.time_budget_ms {
        Some(budget) => active_ms > budget,
        None => false,
    };
    GoalProgress {
        tokens_used: goal.tokens_used,
        token_budget: goal.token_budget,
        token_pct,
        token_over,
        active_ms,
        time_budget_ms: goal.time_budget_ms,
        time_pct,
        time_over,
        over_budget: token_over || time_over,
    }
}

/// 现役 describeGoal 文本逐行同构。
pub fn describe_goal(goal: &GoalState, now: u64) -> String {
    let progress = goal_progress(goal, now);
    let fmt_ms = |ms: u64| {
        let m = ms / 60_000;
        if m < 60 {
            format!("{m}m")
        } else {
            format!("{}h{}m", m / 60, m % 60)
        }
    };
    let mut lines = vec![
        format!("goal \"{}\" — {}", goal.name, goal.status.wire_name()),
        match (goal.token_budget, progress.token_pct) {
            (Some(budget), Some(pct)) => {
                format!("tokens: {} / {} ({}%)", progress.tokens_used, budget, pct)
            }
            _ => format!("tokens: {} (no budget)", progress.tokens_used),
        },
        match (goal.time_budget_ms, progress.time_pct) {
            (Some(budget), Some(pct)) => format!(
                "active time: {} / {} ({}%)",
                fmt_ms(progress.active_ms),
                fmt_ms(budget),
                pct
            ),
            _ => format!("active time: {} (no budget)", fmt_ms(progress.active_ms)),
        },
    ];
    if goal.status == GoalStatus::Paused {
        if let Some(reason) = goal.paused_reason.as_deref() {
            lines.push(format!("paused reason: {reason}"));
        }
    }
    if progress.over_budget {
        lines.push("OVER BUDGET — wrap up and hand the decision back to the user".to_string());
    }
    if goal.over_budget_notice_receipt == Some(GoalNoticeReceipt::Unconfirmed) {
        lines.push("预算提醒的发布回执未确认，为避免重复提醒不会自动重发。".into());
    }
    lines.join("\n")
}

/// 侧车路径：`data_home/goal-sidecars/<session_id>.goal.json`。
/// 会话键含路径分隔或父目录引用时拒绝；工具面必须明确报告存储错误。
fn goal_sidecar_path(root: &Path, session_id: &str) -> Option<PathBuf> {
    if session_id.is_empty()
        || session_id.contains('/')
        || session_id.contains('\\')
        || session_id.contains("..")
    {
        return None;
    }
    Some(
        root.join("goal-sidecars")
            .join(format!("{session_id}.goal.json")),
    )
}

fn default_goal(input: &CreateGoalInput, now: u64) -> GoalState {
    GoalState {
        schema_version: GOAL_SCHEMA_VERSION,
        name: input
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or("goal")
            .to_string(),
        token_budget: input.token_budget,
        time_budget_ms: input.time_budget_ms,
        tokens_used: 0,
        started_at: now,
        paused_ms: 0,
        current_paused_at: None,
        status: GoalStatus::Active,
        paused_reason: None,
        completed_at: None,
        over_budget_notified: false,
        over_budget_notice_receipt: None,
        last_active_at: now,
    }
}

fn read_goal(root: &Path, session_id: &str) -> Result<Option<GoalState>, String> {
    let path = checked_sidecar_path(root, session_id)?;
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("goal storage read failed: {error}")),
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err("goal storage must be a regular file".into())
        }
        Ok(_) => {}
    }
    let snapshot = crate::file_safety::Snapshot::read_bounded(&path, 1024 * 1024)
        .map_err(|error| format!("goal storage read failed: {error:?}"))?;
    let goal = serde_json::from_slice::<GoalState>(&snapshot.bytes)
        .map_err(|error| format!("goal storage is corrupt: {error}"))?;
    if goal.schema_version != GOAL_SCHEMA_VERSION {
        return Err("goal storage schema is unsupported".into());
    }
    Ok(Some(goal))
}

fn checked_sidecar_path(root: &Path, session_id: &str) -> Result<PathBuf, String> {
    let path = goal_sidecar_path(root, session_id).ok_or("goal storage: invalid session id")?;
    let parent = path.parent().ok_or("goal storage parent missing")?;
    match std::fs::symlink_metadata(parent) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(path),
        Err(error) => Err(format!("goal storage directory failed: {error}")),
        Ok(metadata) if !metadata.file_type().is_dir() => {
            Err("goal storage directory must be a real directory".into())
        }
        Ok(_) => Ok(parent
            .canonicalize()
            .map_err(|error| format!("goal storage directory failed: {error}"))?
            .join(path.file_name().ok_or("goal storage filename missing")?)),
    }
}

fn write_goal(root: &Path, session_id: &str, goal: &GoalState) -> Result<(), String> {
    let path = checked_sidecar_path(root, session_id)?;
    std::fs::create_dir_all(path.parent().ok_or("goal storage directory missing")?)
        .map_err(|error| format!("goal storage directory failed: {error}"))?;
    let path = checked_sidecar_path(root, session_id)?;
    let root = root
        .canonicalize()
        .map_err(|error| format!("goal storage root failed: {error}"))?;
    let body = serde_json::to_vec_pretty(goal)
        .map_err(|error| format!("goal storage serialization failed: {error}"))?;
    if body.len() > 1024 * 1024 {
        return Err("goal storage exceeds size limit".into());
    }
    let safety = crate::file_safety::FileSafety::new(&root);
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            safety.create("goal", &path, &body, || true)
        }
        Err(error) => return Err(format!("goal storage inspect failed: {error}")),
        Ok(_) => {
            // 外部改坏侧车时保留原件；不能把损坏文件当成空目标覆盖。
            read_goal(&root, session_id)?;
            let snapshot = crate::file_safety::Snapshot::read_bounded(&path, 1024 * 1024)
                .map_err(|error| format!("goal storage read failed: {error:?}"))?;
            serde_json::from_slice::<GoalState>(&snapshot.bytes)
                .map_err(|error| format!("goal storage is corrupt: {error}"))?;
            safety.replace("goal", &path, &snapshot, &body, || true)
        }
    }
    .map_err(|error| format!("goal storage write failed: {error:?}"))
}

/// 预算引擎句柄。现役是模块闭包（单线程）；Rust 轮并发，侧车的
/// 读-改-写经 per-engine Mutex 串行化（现役单线程语义的直接对应）。
pub struct GoalEngine {
    root: PathBuf,
    notify: NotifyOverBudget,
    now: Box<dyn Fn() -> u64 + Send + Sync>,
    io_lock: Mutex<()>,
}

impl GoalEngine {
    pub fn new(
        root: PathBuf,
        notify: NotifyOverBudget,
        now: Box<dyn Fn() -> u64 + Send + Sync>,
    ) -> Self {
        Self {
            root,
            notify,
            now,
            io_lock: Mutex::new(()),
        }
    }

    fn now(&self) -> u64 {
        (self.now)()
    }

    /// 工具细节读数（goalProgress 的 now 与引擎时钟同源）。
    pub fn now_ms(&self) -> u64 {
        self.now()
    }

    /// 现役 `_read`（测试/状态面用）。
    pub fn read(&self, session_id: &str) -> Option<GoalState> {
        match self.read_checked(session_id) {
            Ok(goal) => goal,
            Err(error) => {
                tracing::warn!(%error,session_id,"目标读取失败");
                None
            }
        }
    }

    pub fn read_checked(&self, session_id: &str) -> Result<Option<GoalState>, String> {
        let _guard = self
            .io_lock
            .lock()
            .map_err(|_| "goal engine lock unavailable")?;
        read_goal(&self.root, session_id)
    }

    fn read_logged(&self, session_id: &str) -> Option<GoalState> {
        match read_goal(&self.root, session_id) {
            Ok(goal) => goal,
            Err(error) => {
                tracing::warn!(%error,session_id,"目标读取失败，未更新预算");
                None
            }
        }
    }

    fn write_logged(&self, session_id: &str, goal: &GoalState) -> bool {
        match write_goal(&self.root, session_id, goal) {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(%error,session_id,"目标保存失败，未通知成功");
                false
            }
        }
    }

    fn fire_notify(&self, session_id: &str, goal: &GoalState, kind: OverBudgetKind) -> bool {
        let notify = Arc::clone(&self.notify);
        let key = session_id.to_string();
        let snapshot = goal.clone();
        // 通知失败不吞账本：回调内任何 panic/错误都不外溢到账本路径。
        let published = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            notify(&key, &snapshot, kind);
        }))
        .is_ok();
        if !published {
            tracing::warn!(session_id, "goal over-budget notify callback panicked");
        }
        published
    }

    fn publish_budget_notice(
        &self,
        session_id: &str,
        goal: &mut GoalState,
        kind: OverBudgetKind,
    ) -> bool {
        let published = self.fire_notify(session_id, goal, kind);
        if published {
            // 首次写入先保留未确认状态；提交后回执失败仍能在重启后如实显示。
            goal.over_budget_notice_receipt = Some(GoalNoticeReceipt::Published);
            if !self.write_logged(session_id, goal) {
                goal.over_budget_notice_receipt = Some(GoalNoticeReceipt::Unconfirmed);
            }
        }
        published
    }

    /// 用量进账（账本落盘点钩子）；active goal 才记。返回超支是否新发生。
    /// total 未报（None）时按 0 累计但仍触碰 lastActiveAt——与现役
    /// `Number.isFinite(totalTokens) ? ... : 0` 同义。
    pub fn record_token_usage(&self, session_id: &str, total_tokens: u64) -> Option<RecordedUsage> {
        let _guard = self.io_lock.lock().ok()?;
        let mut goal = self.read_logged(session_id)?;
        if goal.status != GoalStatus::Active {
            return None;
        }
        goal.tokens_used = goal.tokens_used.saturating_add(total_tokens);
        goal.last_active_at = self.now();
        let progress = goal_progress(&goal, self.now());
        let mut newly_over = false;
        if progress.over_budget && !goal.over_budget_notified {
            goal.over_budget_notified = true;
            goal.over_budget_notice_receipt = Some(GoalNoticeReceipt::Unconfirmed);
            newly_over = true;
        }
        if !self.write_logged(session_id, &goal) {
            return None;
        }
        if newly_over {
            let kind = if progress.token_over {
                OverBudgetKind::Tokens
            } else {
                OverBudgetKind::Time
            };
            self.publish_budget_notice(session_id, &mut goal, kind);
        }
        Some(RecordedUsage { goal, newly_over })
    }

    /// 检查点（每轮迭代顶调用）：时间预算懒检查。
    pub fn tick_time_budget(&self, session_id: &str) -> bool {
        let Ok(_guard) = self.io_lock.lock() else {
            return false;
        };
        let Some(mut goal) = self.read_logged(session_id) else {
            return false;
        };
        if goal.status != GoalStatus::Active {
            return false;
        }
        let progress = goal_progress(&goal, self.now());
        if progress.over_budget && !goal.over_budget_notified {
            goal.over_budget_notified = true;
            goal.over_budget_notice_receipt = Some(GoalNoticeReceipt::Unconfirmed);
            if !self.write_logged(session_id, &goal) {
                return false;
            }
            let kind = if progress.token_over {
                OverBudgetKind::Tokens
            } else {
                OverBudgetKind::Time
            };
            return self.publish_budget_notice(session_id, &mut goal, kind);
        }
        false
    }

    /// 用户中止 → 自动暂停。
    pub fn pause_on_abort(&self, session_id: &str) -> bool {
        let Ok(_guard) = self.io_lock.lock() else {
            return false;
        };
        match self.read_logged(session_id) {
            Some(goal) if goal.status == GoalStatus::Active => {
                drop(_guard);
                self.pause(session_id, "user_abort")
            }
            _ => false,
        }
    }

    /// 现役 create：active 排他；至少一项预算。
    pub fn create(&self, session_id: &str, input: CreateGoalInput) -> CreateGoalOutcome {
        let Ok(_guard) = self.io_lock.lock() else {
            return CreateGoalOutcome::Err("goal engine lock unavailable".to_string());
        };
        let existing = match read_goal(&self.root, session_id) {
            Ok(existing) => existing,
            Err(error) => return CreateGoalOutcome::Err(error),
        };
        if let Some(existing) = existing {
            if existing.status == GoalStatus::Active {
                return CreateGoalOutcome::Err(format!(
                    "goal \"{}\" is already active — complete or drop it first (status shows its progress)",
                    existing.name
                ));
            }
        }
        if input.token_budget.is_none() && input.time_budget_ms.is_none() {
            return CreateGoalOutcome::Err(
                "at least one budget is required (token_budget or time_budget_minutes)".to_string(),
            );
        }
        let goal = default_goal(&input, self.now());
        match write_goal(&self.root, session_id, &goal) {
            Ok(()) => CreateGoalOutcome::Ok(goal),
            Err(error) => CreateGoalOutcome::Err(error),
        }
    }

    fn change_checked(
        &self,
        session_id: &str,
        change: impl FnOnce(&mut GoalState, u64) -> bool,
    ) -> Result<bool, String> {
        let _guard = self
            .io_lock
            .lock()
            .map_err(|_| "goal engine lock unavailable")?;
        let Some(mut goal) = read_goal(&self.root, session_id)? else {
            return Ok(false);
        };
        if !change(&mut goal, self.now()) {
            return Ok(false);
        };
        write_goal(&self.root, session_id, &goal)?;
        Ok(true)
    }

    pub fn pause_checked(&self, session_id: &str, reason: &str) -> Result<bool, String> {
        self.change_checked(session_id, |goal, now| {
            if goal.status != GoalStatus::Active {
                return false;
            }
            goal.status = GoalStatus::Paused;
            goal.paused_reason = Some(if reason.is_empty() { "manual" } else { reason }.to_owned());
            goal.current_paused_at = Some(now);
            true
        })
    }
    pub fn resume_checked(&self, session_id: &str) -> Result<bool, String> {
        self.change_checked(session_id, |goal, now| {
            if goal.status != GoalStatus::Paused {
                return false;
            }
            if let Some(at) = goal.current_paused_at.take() {
                goal.paused_ms = goal.paused_ms.saturating_add(now.saturating_sub(at));
            }
            goal.status = GoalStatus::Active;
            goal.paused_reason = None;
            true
        })
    }
    fn finish_checked(&self, session_id: &str, status: GoalStatus) -> Result<bool, String> {
        self.change_checked(session_id, |goal, now| {
            if !matches!(goal.status, GoalStatus::Active | GoalStatus::Paused) {
                return false;
            }
            if let Some(at) = goal.current_paused_at.take() {
                goal.paused_ms = goal.paused_ms.saturating_add(now.saturating_sub(at));
            }
            goal.status = status;
            goal.completed_at = Some(now);
            true
        })
    }
    pub fn complete_checked(&self, session_id: &str) -> Result<bool, String> {
        self.finish_checked(session_id, GoalStatus::Completed)
    }
    pub fn drop_goal_checked(&self, session_id: &str) -> Result<bool, String> {
        self.finish_checked(session_id, GoalStatus::Dropped)
    }
    fn action_logged(&self, session_id: &str, result: Result<bool, String>) -> bool {
        match result {
            Ok(changed) => changed,
            Err(error) => {
                tracing::warn!(%error,session_id,"目标动作失败，未报告成功");
                false
            }
        }
    }
    pub fn pause(&self, session_id: &str, reason: &str) -> bool {
        self.action_logged(session_id, self.pause_checked(session_id, reason))
    }
    pub fn resume(&self, session_id: &str) -> bool {
        self.action_logged(session_id, self.resume_checked(session_id))
    }
    pub fn complete(&self, session_id: &str) -> bool {
        self.action_logged(session_id, self.complete_checked(session_id))
    }
    pub fn drop_goal(&self, session_id: &str) -> bool {
        self.action_logged(session_id, self.drop_goal_checked(session_id))
    }
    pub fn status_checked(&self, session_id: &str) -> Result<(String, Option<GoalState>), String> {
        Ok(match self.read_checked(session_id)? {
            None => ("no goal in this session".to_owned(), None),
            Some(goal) => (describe_goal(&goal, self.now()), Some(goal)),
        })
    }
    pub fn status(&self, session_id: &str) -> (String, Option<GoalState>) {
        match self.status_checked(session_id) {
            Ok(status) => status,
            Err(error) => (error, None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn test_root(tag: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("lingxi-r07-goal-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    struct ManualClock(Arc<AtomicU64>);
    impl ManualClock {
        fn new(start: u64) -> Self {
            Self(Arc::new(AtomicU64::new(start)))
        }
        fn handle(&self) -> Arc<AtomicU64> {
            Arc::clone(&self.0)
        }
        fn now_fn(&self) -> Box<dyn Fn() -> u64 + Send + Sync> {
            let clock = self.handle();
            Box::new(move || clock.load(Ordering::SeqCst))
        }
    }

    /// 测试通知记录：(session_id, kind) 序列。
    type NotifyLog = Arc<Mutex<Vec<(String, OverBudgetKind)>>>;

    fn engine(root: &Path, clock: &ManualClock) -> (GoalEngine, NotifyLog) {
        let notices = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&notices);
        let engine = GoalEngine::new(
            root.to_path_buf(),
            Arc::new(
                move |session: &str, _goal: &GoalState, kind: OverBudgetKind| {
                    sink.lock().unwrap().push((session.to_string(), kind));
                },
            ),
            clock.now_fn(),
        );
        (engine, notices)
    }

    #[test]
    fn create_requires_at_least_one_budget_and_active_is_exclusive() {
        let root = test_root("create");
        let clock = ManualClock::new(1_000);
        let (engine, _) = engine(&root, &clock);

        let outcome = engine.create("s1", CreateGoalInput::default());
        assert_eq!(
            outcome,
            CreateGoalOutcome::Err(
                "at least one budget is required (token_budget or time_budget_minutes)".to_string()
            )
        );

        let created = engine.create(
            "s1",
            CreateGoalInput {
                name: Some("  build ".to_string()),
                token_budget: Some(1_000),
                time_budget_ms: None,
            },
        );
        let goal = match created {
            CreateGoalOutcome::Ok(goal) => goal,
            other => panic!("expected Ok, got {other:?}"),
        };
        assert_eq!(goal.name, "build");
        assert_eq!(goal.status, GoalStatus::Active);
        assert_eq!(goal.schema_version, 1);

        let again = engine.create(
            "s1",
            CreateGoalInput {
                name: None,
                token_budget: None,
                time_budget_ms: Some(60_000),
            },
        );
        assert_eq!(
            again,
            CreateGoalOutcome::Err(
                "goal \"build\" is already active — complete or drop it first (status shows its progress)"
                    .to_string()
            )
        );

        // completed 之后可以再建（现役排他只挡 active）。
        assert!(engine.complete("s1"));
        let recreated = engine.create(
            "s1",
            CreateGoalInput {
                name: None,
                token_budget: Some(5),
                time_budget_ms: None,
            },
        );
        assert!(matches!(recreated, CreateGoalOutcome::Ok(_)));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sidecar_json_is_camel_case_two_space_no_trailing_newline() {
        let root = test_root("shape");
        let clock = ManualClock::new(42);
        let (engine, _) = engine(&root, &clock);
        let _ = engine.create(
            "s-json",
            CreateGoalInput {
                name: Some("shape".to_string()),
                token_budget: Some(100),
                time_budget_ms: Some(120_000),
            },
        );
        let path = root.join("goal-sidecars").join("s-json.goal.json");
        let raw = std::fs::read_to_string(&path).expect("sidecar exists");
        assert!(
            !raw.ends_with('\n'),
            "no trailing newline (JSON.stringify parity)"
        );
        let expected = concat!(
            "{\n",
            "  \"schemaVersion\": 1,\n",
            "  \"name\": \"shape\",\n",
            "  \"tokenBudget\": 100,\n",
            "  \"timeBudgetMs\": 120000,\n",
            "  \"tokensUsed\": 0,\n",
            "  \"startedAt\": 42,\n",
            "  \"pausedMs\": 0,\n",
            "  \"currentPausedAt\": null,\n",
            "  \"status\": \"active\",\n",
            "  \"pausedReason\": null,\n",
            "  \"completedAt\": null,\n",
            "  \"overBudgetNotified\": false,\n",
            "  \"lastActiveAt\": 42\n",
            "}"
        );
        assert_eq!(raw, expected, "byte-level incumbent parity");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn record_usage_only_for_active_and_notifies_exactly_once() {
        let root = test_root("usage");
        let clock = ManualClock::new(10_000);
        let (engine, notices) = engine(&root, &clock);
        let _ = engine.create(
            "s2",
            CreateGoalInput {
                name: None,
                token_budget: Some(100),
                time_budget_ms: None,
            },
        );

        let first = engine
            .record_token_usage("s2", 60)
            .expect("active goal records");
        assert!(!first.newly_over);
        assert_eq!(first.goal.tokens_used, 60);
        assert_eq!(first.goal.last_active_at, 10_000);

        // 越限：恰好一次通知，kind=tokens；goal 保持 active。
        let over = engine.record_token_usage("s2", 50).expect("still active");
        assert!(over.newly_over);
        assert_eq!(over.goal.tokens_used, 110);
        assert!(over.goal.over_budget_notified);
        assert_eq!(over.goal.status, GoalStatus::Active);
        assert_eq!(
            notices.lock().unwrap().as_slice(),
            &[("s2".to_string(), OverBudgetKind::Tokens)]
        );

        // 继续累计不再通知。
        let later = engine.record_token_usage("s2", 500).expect("active");
        assert!(!later.newly_over);
        assert_eq!(notices.lock().unwrap().len(), 1);

        // paused/completed 不记账。
        assert!(engine.pause("s2", "manual"));
        assert!(engine.record_token_usage("s2", 10).is_none());
        assert!(engine.resume("s2"));
        assert!(engine.complete("s2"));
        assert!(engine.record_token_usage("s2", 10).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn paused_time_is_excluded_from_active_ms() {
        let root = test_root("paused");
        let clock = ManualClock::new(0);
        let (engine, _) = engine(&root, &clock);
        let _ = engine.create(
            "s3",
            CreateGoalInput {
                name: None,
                token_budget: None,
                time_budget_ms: Some(120_000),
            },
        );
        clock.handle().store(60_000, Ordering::SeqCst);
        assert!(engine.pause("s3", "user_abort"));
        clock.handle().store(600_000, Ordering::SeqCst);
        let goal = engine.read("s3").expect("goal");
        assert_eq!(goal_active_ms(&goal, 600_000), 60_000);

        assert!(engine.resume("s3"));
        let goal = engine.read("s3").expect("goal");
        assert_eq!(goal.paused_ms, 540_000);
        assert!(goal.current_paused_at.is_none());
        clock.handle().store(660_000, Ordering::SeqCst);
        assert_eq!(goal_active_ms(&goal, 660_000), 120_000);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tick_time_budget_notifies_once_with_time_kind() {
        let root = test_root("tick");
        let clock = ManualClock::new(0);
        let (engine, notices) = engine(&root, &clock);
        let _ = engine.create(
            "s4",
            CreateGoalInput {
                name: None,
                token_budget: None,
                time_budget_ms: Some(60_000),
            },
        );
        clock.handle().store(61_000, Ordering::SeqCst);
        assert!(engine.tick_time_budget("s4"));
        assert!(
            !engine.tick_time_budget("s4"),
            "second tick does not re-notify"
        );
        assert_eq!(
            notices.lock().unwrap().as_slice(),
            &[("s4".to_string(), OverBudgetKind::Time)]
        );
        // token 优先规则：两种都越限时 kind=tokens。
        let _ = engine.create(
            "s5",
            CreateGoalInput {
                name: None,
                token_budget: Some(1),
                time_budget_ms: Some(1),
            },
        );
        engine.record_token_usage("s5", 5);
        assert_eq!(
            notices.lock().unwrap().as_slice()[1],
            ("s5".to_string(), OverBudgetKind::Tokens)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lifecycle_pause_resume_complete_drop_and_abort() {
        let root = test_root("lifecycle");
        let clock = ManualClock::new(5_000);
        let (engine, _) = engine(&root, &clock);

        assert!(!engine.pause("none", "manual"));
        assert!(!engine.resume("none"));
        assert!(!engine.complete("none"));
        assert!(!engine.drop_goal("none"));
        assert!(!engine.pause_on_abort("none"));

        let _ = engine.create(
            "s6",
            CreateGoalInput {
                name: None,
                token_budget: Some(10),
                time_budget_ms: None,
            },
        );
        assert!(engine.pause_on_abort("s6"));
        let goal = engine.read("s6").expect("goal");
        assert_eq!(goal.status, GoalStatus::Paused);
        assert_eq!(goal.paused_reason.as_deref(), Some("user_abort"));

        assert!(!engine.pause("s6", "manual"), "already paused");
        assert!(engine.resume("s6"));
        assert!(engine.complete("s6"));
        let goal = engine.read("s6").expect("goal");
        assert_eq!(goal.status, GoalStatus::Completed);
        assert!(goal.completed_at.is_some());
        assert!(!engine.drop_goal("s6"), "finished goal cannot drop");

        let _ = engine.create(
            "s7",
            CreateGoalInput {
                name: None,
                token_budget: Some(10),
                time_budget_ms: None,
            },
        );
        assert!(engine.drop_goal("s7"));
        assert_eq!(engine.read("s7").expect("goal").status, GoalStatus::Dropped);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn describe_goal_text_matches_incumbent_lines() {
        let root = test_root("describe");
        let clock = ManualClock::new(0);
        let (engine, _) = engine(&root, &clock);
        let (text, goal) = engine.status("missing");
        assert!(goal.is_none());
        assert_eq!(text, "no goal in this session");

        let _ = engine.create(
            "s8",
            CreateGoalInput {
                name: Some("watch".to_string()),
                token_budget: Some(200),
                time_budget_ms: Some(7_200_000),
            },
        );
        engine.record_token_usage("s8", 100);
        clock.handle().store(3_600_000, Ordering::SeqCst);
        let (text, goal) = engine.status("s8");
        assert!(goal.is_some());
        assert_eq!(
            text,
            "goal \"watch\" — active\ntokens: 100 / 200 (50%)\nactive time: 1h0m / 2h0m (50%)"
        );

        // 无预算行的形状。
        let _ = engine.create(
            "s9",
            CreateGoalInput {
                name: None,
                token_budget: Some(50),
                time_budget_ms: None,
            },
        );
        let (text, _) = engine.status("s9");
        assert_eq!(
            text,
            "goal \"goal\" — active\ntokens: 0 / 50 (0%)\nactive time: 0m (no budget)"
        );

        // 暂停原因与超支行。
        let _ = engine.create(
            "s10",
            CreateGoalInput {
                name: None,
                token_budget: Some(1),
                time_budget_ms: None,
            },
        );
        engine.record_token_usage("s10", 2);
        assert!(engine.pause("s10", "manual"));
        let (text, _) = engine.status("s10");
        assert_eq!(
            text,
            concat!(
                "goal \"goal\" — paused\n",
                "tokens: 2 / 1 (200%)\n",
                "active time: 0m (no budget)\n",
                "paused reason: manual\n",
                "OVER BUDGET — wrap up and hand the decision back to the user"
            )
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn corrupt_sidecar_returns_explicit_error_and_is_never_overwritten() {
        let root = test_root("corrupt");
        let clock = ManualClock::new(1);
        let (engine, _) = engine(&root, &clock);
        let dir = root.join("goal-sidecars");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bad.goal.json"), "{not json").unwrap();
        assert!(engine.read("bad").is_none());
        let (text, goal) = engine.status("bad");
        assert!(goal.is_none());
        assert!(text.contains("goal storage is corrupt"));
        assert!(engine.read_checked("bad").is_err());
        assert!(engine.pause_checked("bad", "").is_err());
        assert!(engine.resume_checked("bad").is_err());
        assert!(engine.complete_checked("bad").is_err());
        assert!(engine.drop_goal_checked("bad").is_err());
        assert!(matches!(
            engine.create(
                "bad",
                CreateGoalInput {
                    token_budget: Some(10),
                    ..Default::default()
                }
            ),
            CreateGoalOutcome::Err(_)
        ));
        assert_eq!(
            std::fs::read(dir.join("bad.goal.json")).unwrap(),
            b"{not json"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn write_fault_never_reports_changed_goal_or_sends_over_budget_notice() {
        let root = test_root("write-fault");
        let clock = ManualClock::new(1);
        let (engine, notices) = engine(&root, &clock);
        assert!(matches!(
            engine.create(
                "fault",
                CreateGoalInput {
                    token_budget: Some(1),
                    time_budget_ms: Some(1),
                    ..Default::default()
                }
            ),
            CreateGoalOutcome::Ok(_)
        ));
        let path = root.join("goal-sidecars/fault.goal.json");
        let before = std::fs::read(&path).unwrap();
        // 真实磁盘故障：恢复记录目录被非目录占用，替换必须在提交前拒绝。
        std::fs::write(root.join("file-recovery-records"), b"blocker").unwrap();
        assert!(engine
            .pause_checked("fault", "")
            .unwrap_err()
            .contains("goal storage write failed"));
        assert!(engine.record_token_usage("fault", 2).is_none());
        clock.handle().store(100, Ordering::SeqCst);
        assert!(!engine.tick_time_budget("fault"));
        assert!(notices.lock().unwrap().is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(
            engine.read_checked("fault").unwrap().unwrap().tokens_used,
            0
        );
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn committed_budget_write_with_failed_witness_retains_unconfirmed_notice_after_restart() {
        for by_time in [false, true] {
            let root = test_root(if by_time {
                "notice-unknown-time"
            } else {
                "notice-unknown-tokens"
            });
            let clock = ManualClock::new(1);
            let (engine, notices) = engine(&root, &clock);
            assert!(matches!(
                engine.create(
                    "fault",
                    CreateGoalInput {
                        token_budget: Some(1),
                        time_budget_ms: Some(1),
                        ..Default::default()
                    }
                ),
                CreateGoalOutcome::Ok(_)
            ));
            // 回收目录仍可写，但提交后的见证目录不可写，精确制造已落盘/回执未知。
            std::fs::remove_dir_all(root.join("file-write-witnesses")).unwrap();
            std::fs::write(root.join("file-write-witnesses"), b"blocker").unwrap();
            if by_time {
                clock.handle().store(100, Ordering::SeqCst);
                assert!(!engine.tick_time_budget("fault"));
            } else {
                assert!(engine.record_token_usage("fault", 2).is_none());
            }
            assert!(notices.lock().unwrap().is_empty());
            let actual = engine.read_checked("fault").unwrap().unwrap();
            assert!(actual.over_budget_notified);
            assert_eq!(
                actual.over_budget_notice_receipt,
                Some(GoalNoticeReceipt::Unconfirmed)
            );
            assert!(describe_goal(&actual, engine.now_ms()).contains("回执未确认"));
            std::fs::remove_file(root.join("file-write-witnesses")).unwrap();
            // 重建引擎后仍不把未知提醒说成已发布，也不自动重复发送。
            let (restored, restored_notices) = self::engine(&root, &clock);
            assert!(!restored.tick_time_budget("fault"));
            assert!(restored_notices.lock().unwrap().is_empty());
            assert_eq!(
                restored
                    .read_checked("fault")
                    .unwrap()
                    .unwrap()
                    .over_budget_notice_receipt,
                Some(GoalNoticeReceipt::Unconfirmed)
            );
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
