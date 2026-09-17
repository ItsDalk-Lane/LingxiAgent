/**
 * goal-engine.ts — 会话预算引擎（阶段二·10）。
 *
 * 每会话至多一个 goal（token 预算 / 时间预算 / 两者），状态机
 * active ⇄ paused → completed/dropped。用量经 server/goal-handler 订阅
 * token_usage 事件累计（Pi SDK usage.totalTokens）；时间预算在检查点
 * 懒算（pause 时长不计）。超支提醒「恰好一次」：token 或时间越限时
 * 置 overBudgetNotified，经注入回调提醒模型 + 通知回调给用户卡片；
 * 之后 goal 保持 active（预算是提醒不是硬闸——是否继续由模型/用户定，
 * 状态随时可查）。用户中止轮 → 自动 pause（reason=user_abort）。
 *
 * 状态持久化在会话旁 `${sessionPath}.goal.json` 侧车（原子写），server
 * 重启后恢复；会话删除/回档不清理它——goal 只是会话的伴随账本，孤儿
 * 侧车无副作用。诚实边界：token_usage 只在 assistant run 收尾发出，
 * 中止且未落盘的 run 不记账（与聊天页口径一致）。
 */
import fs from "node:fs";
import path from "node:path";

export type GoalStatus = "active" | "paused" | "completed" | "dropped";

export interface GoalState {
  schemaVersion: 1;
  name: string;
  tokenBudget: number | null;
  timeBudgetMs: number | null;
  tokensUsed: number;
  startedAt: number;
  /** pause 累计（ms）：活动时长 = now - startedAt - pausedMs - (paused 中则 currentPausedAt 起算)。 */
  pausedMs: number;
  currentPausedAt: number | null;
  status: GoalStatus;
  pausedReason: string | null;
  completedAt: number | null;
  overBudgetNotified: boolean;
  lastActiveAt: number;
}

export interface GoalEngineDeps {
  /** 超支提醒（恰好一次）：注入模型可见消息 + 用户通知。 */
  notifyOverBudget: (sessionPath: string, goal: GoalState, kind: "tokens" | "time") => void;
  now?: () => number;
}

export function goalSidecarPath(sessionPath: string): string {
  return `${sessionPath}.goal.json`;
}

function defaultGoal(partial: { name?: string; tokenBudget?: number | null; timeBudgetMs?: number | null }, now: number): GoalState {
  return {
    schemaVersion: 1,
    name: partial.name?.trim() || "goal",
    tokenBudget: partial.tokenBudget ?? null,
    timeBudgetMs: partial.timeBudgetMs ?? null,
    tokensUsed: 0,
    startedAt: now,
    pausedMs: 0,
    currentPausedAt: null,
    status: "active",
    pausedReason: null,
    completedAt: null,
    overBudgetNotified: false,
    lastActiveAt: now,
  };
}

function readGoal(sessionPath: string): GoalState | null {
  try {
    const raw = JSON.parse(fs.readFileSync(goalSidecarPath(sessionPath), "utf8"));
    if (!raw || typeof raw !== "object" || typeof raw.status !== "string") return null;
    return raw as GoalState;
  } catch {
    return null;
  }
}

function writeGoal(sessionPath: string, goal: GoalState): void {
  const sidecarPath = goalSidecarPath(sessionPath);
  const tmp = `${sidecarPath}.tmp-${process.pid}-${Date.now()}`;
  fs.mkdirSync(path.dirname(sidecarPath), { recursive: true });
  fs.writeFileSync(tmp, JSON.stringify(goal, null, 2));
  fs.renameSync(tmp, sidecarPath);
}

/** 活动时长（不含暂停）。 */
export function goalActiveMs(goal: GoalState, now: number): number {
  const pausedCurrent = goal.currentPausedAt != null ? Math.max(0, now - goal.currentPausedAt) : 0;
  return Math.max(0, now - goal.startedAt - goal.pausedMs - pausedCurrent);
}

export function goalProgress(goal: GoalState, now: number): {
  tokens: { used: number; budget: number | null; pct: number | null; over: boolean };
  time: { activeMs: number; budgetMs: number | null; pct: number | null; over: boolean };
  overBudget: boolean;
} {
  const tokens = {
    used: goal.tokensUsed,
    budget: goal.tokenBudget,
    pct: goal.tokenBudget ? Math.round((goal.tokensUsed / goal.tokenBudget) * 100) : null,
    over: goal.tokenBudget != null && goal.tokensUsed > goal.tokenBudget,
  };
  const activeMs = goalActiveMs(goal, now);
  const time = {
    activeMs,
    budgetMs: goal.timeBudgetMs,
    pct: goal.timeBudgetMs ? Math.round((activeMs / goal.timeBudgetMs) * 100) : null,
    over: goal.timeBudgetMs != null && activeMs > goal.timeBudgetMs,
  };
  return { tokens, time, overBudget: tokens.over || time.over };
}

function describeGoal(goal: GoalState, now: number): string {
  const p = goalProgress(goal, now);
  const fmtMs = (ms: number) => {
    const m = Math.floor(ms / 60_000);
    if (m < 60) return `${m}m`;
    return `${Math.floor(m / 60)}h${m % 60}m`;
  };
  return [
    `goal "${goal.name}" — ${goal.status}`,
    `tokens: ${p.tokens.used}${p.tokens.budget ? ` / ${p.tokens.budget} (${p.tokens.pct}%)` : " (no budget)"}`,
    `active time: ${fmtMs(p.time.activeMs)}${p.time.budgetMs ? ` / ${fmtMs(p.time.budgetMs)} (${p.time.pct}%)` : " (no budget)"}`,
    ...(goal.status === "paused" && goal.pausedReason ? [`paused reason: ${goal.pausedReason}`] : []),
    ...(p.overBudget ? ["OVER BUDGET — wrap up and hand the decision back to the user"] : []),
  ].join("\n");
}

export function createGoalEngine(deps: GoalEngineDeps) {
  const now = deps.now || (() => Date.now());

  function requireGoal(sessionPath: string): GoalState | null {
    return readGoal(sessionPath);
  }

  /** 用量进账（token_usage 事件）；active goal 才记。返回超支是否新发生。 */
  function recordTokenUsage(sessionPath: string, totalTokens: number): { goal: GoalState; newlyOver: boolean } | null {
    const goal = readGoal(sessionPath);
    if (!goal || goal.status !== "active") return null;
    goal.tokensUsed += Number.isFinite(totalTokens) ? Math.max(0, Math.round(totalTokens)) : 0;
    goal.lastActiveAt = now();
    const progress = goalProgress(goal, now());
    let newlyOver = false;
    if (progress.overBudget && !goal.overBudgetNotified) {
      goal.overBudgetNotified = true;
      newlyOver = true;
    }
    writeGoal(sessionPath, goal);
    if (newlyOver) {
      const kind = progress.tokens.over ? "tokens" : "time";
      try { deps.notifyOverBudget(sessionPath, goal, kind); } catch { /* 通知失败不吞账本 */ }
    }
    return { goal, newlyOver };
  }

  /** 检查点（turn_start 等处调用）：时间预算懒检查。 */
  function tickTimeBudget(sessionPath: string): boolean {
    const goal = readGoal(sessionPath);
    if (!goal || goal.status !== "active") return false;
    const progress = goalProgress(goal, now());
    if (progress.overBudget && !goal.overBudgetNotified) {
      goal.overBudgetNotified = true;
      writeGoal(sessionPath, goal);
      const kind = progress.tokens.over ? "tokens" : "time";
      try { deps.notifyOverBudget(sessionPath, goal, kind); } catch { /* 同上 */ }
      return true;
    }
    return false;
  }

  /** 用户中止 → 自动暂停。 */
  function pauseOnAbort(sessionPath: string): boolean {
    const goal = readGoal(sessionPath);
    if (!goal || goal.status !== "active") return false;
    return pause(sessionPath, "user_abort");
  }

  function create(sessionPath: string, input: { name?: string; tokenBudget?: number | null; timeBudgetMs?: number | null } = {}): { ok: boolean; error?: string; goal?: GoalState } {
    const existing = readGoal(sessionPath);
    if (existing && existing.status === "active") {
      return { ok: false, error: `goal "${existing.name}" is already active — complete or drop it first (status shows its progress)` };
    }
    if (input.tokenBudget == null && input.timeBudgetMs == null) {
      return { ok: false, error: "at least one budget is required (token_budget or time_budget_minutes)" };
    }
    const goal = defaultGoal(input, now());
    writeGoal(sessionPath, goal);
    return { ok: true, goal };
  }

  function pause(sessionPath: string, reason = "manual"): boolean {
    const goal = readGoal(sessionPath);
    if (!goal || goal.status !== "active") return false;
    goal.status = "paused";
    goal.pausedReason = reason || "manual";
    goal.currentPausedAt = now();
    writeGoal(sessionPath, goal);
    return true;
  }

  function resume(sessionPath: string): boolean {
    const goal = readGoal(sessionPath);
    if (!goal || goal.status !== "paused") return false;
    if (goal.currentPausedAt != null) {
      goal.pausedMs += Math.max(0, now() - goal.currentPausedAt);
      goal.currentPausedAt = null;
    }
    goal.status = "active";
    goal.pausedReason = null;
    writeGoal(sessionPath, goal);
    return true;
  }

  function finish(sessionPath: string, status: "completed" | "dropped"): boolean {
    const goal = readGoal(sessionPath);
    if (!goal || (goal.status !== "active" && goal.status !== "paused")) return false;
    if (goal.currentPausedAt != null) {
      goal.pausedMs += Math.max(0, now() - goal.currentPausedAt);
      goal.currentPausedAt = null;
    }
    goal.status = status;
    goal.completedAt = now();
    writeGoal(sessionPath, goal);
    return true;
  }

  function status(sessionPath: string): { text: string; goal: GoalState | null } {
    const goal = readGoal(sessionPath);
    if (!goal) return { text: "no goal in this session", goal: null };
    return { text: describeGoal(goal, now()), goal };
  }

  return {
    create,
    status,
    pause,
    resume,
    complete: (sp: string) => finish(sp, "completed"),
    drop: (sp: string) => finish(sp, "dropped"),
    recordTokenUsage,
    tickTimeBudget,
    pauseOnAbort,
    _read: requireGoal,
  };
}

export type GoalEngine = ReturnType<typeof createGoalEngine>;
