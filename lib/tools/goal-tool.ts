/**
 * goal-tool.ts — 预算目标工具（阶段二·10）。
 *
 * 五动作：create（token/时间预算，至少给一项；未显式给时用设置里的
 * 全局默认）/status/pause/resume/complete/drop。超支提醒由引擎在
 * token_usage/检查点驱动（恰好一次），工具侧只读写状态。
 * 权限：status=read；其余=write（写侧车账本）。
 */
import { Type, StringEnum } from "../pi-sdk/index.ts";
import type { GoalEngine } from "../goal/goal-engine.ts";
import { goalProgress } from "../goal/goal-engine.ts";

export interface GoalToolDeps {
  getSessionPath: () => string | null;
  /** 惰性取引擎：server 侧在 engine 起来后才挂 goalEngine，工具构造早于它。 */
  getGoalEngine: () => GoalEngine | null;
  /** 全局默认预算（preferences.goal）。 */
  getDefaultBudgets: () => { tokenBudget: number | null; timeBudgetMs: number | null };
}

function fmtBudget(tokenBudget: number | null, timeBudgetMs: number | null): string {
  const parts: string[] = [];
  if (tokenBudget != null) parts.push(`${tokenBudget} tokens`);
  if (timeBudgetMs != null) parts.push(`${Math.round(timeBudgetMs / 60_000)} min`);
  return parts.join(" + ") || "none";
}

export function createGoalTool(deps: GoalToolDeps) {
  return {
    name: "goal",
    description: "Track a budget for a long-running task: create a goal with a token budget and/or wall-clock time budget, then check status anytime. When the budget is exceeded you get exactly one over-budget notice (the user sees one too) — wrap up and hand the decision back instead of silently continuing. User aborts pause the goal automatically. Budgets are advisory reminders, not hard stops; status/pause/resume/complete/drop manage the lifecycle.",
    parameters: Type.Object({
      action: Type.Optional(StringEnum(["create", "status", "pause", "resume", "complete", "drop"], { description: "create: start a budgeted goal (replaces a finished/dropped one; only one active at a time). status: progress readout (default). pause/resume/complete/drop: lifecycle" })),
      name: Type.Optional(Type.String({ description: "Short goal name shown in status" })),
      token_budget: Type.Optional(Type.Number({ description: "Total token budget for the goal (omit to use the global default, or set null with time_budget_minutes given)" })),
      time_budget_minutes: Type.Optional(Type.Number({ description: "Wall-clock time budget in minutes (pause time excluded)" })),
    }),
    sessionPermission: {
      resolveInvocation: (input: any = {}) => {
        if (input?.action === "status" || input?.action == null) {
          return { action: "status", kind: "read", capability: "goal.status" };
        }
        return { action: input?.action || "create", kind: "routine", capability: `goal.${input?.action || "create"}` };
      },
    },
    async execute(_toolCallId: string, params: any = {}, ..._rest: any[]) {
      const sessionPath = deps.getSessionPath?.() || null;
      if (!sessionPath) {
        return { isError: true, content: [{ type: "text", text: "no active session — goals are session-scoped" }] };
      }
      const engine = deps.getGoalEngine?.() || null;
      if (!engine) {
        return { isError: true, content: [{ type: "text", text: "goal engine unavailable in this runtime" }] };
      }
      const action = ["create", "status", "pause", "resume", "complete", "drop"].includes(params?.action)
        ? params.action
        : "status";

      if (action === "status") {
        const { text, goal } = engine.status(sessionPath);
        const details: Record<string, any> = { hasGoal: !!goal };
        if (goal) {
          const p = goalProgress(goal, Date.now());
          details.goal = { name: goal.name, status: goal.status, tokensUsed: goal.tokensUsed, tokenBudget: goal.tokenBudget, timeBudgetMs: goal.timeBudgetMs, overBudget: p.overBudget };
        }
        return { content: [{ type: "text", text }], details };
      }

      if (action === "create") {
        const defaults = deps.getDefaultBudgets?.() || { tokenBudget: null, timeBudgetMs: null };
        let tokenBudget = typeof params?.token_budget === "number" && params.token_budget > 0
          ? Math.round(params.token_budget)
          : defaults.tokenBudget;
        if (params?.token_budget === null) tokenBudget = null;
        let timeBudgetMs = typeof params?.time_budget_minutes === "number" && params.time_budget_minutes > 0
          ? Math.round(params.time_budget_minutes * 60_000)
          : defaults.timeBudgetMs;
        if (params?.time_budget_minutes === null) timeBudgetMs = null;
        const result = engine.create(sessionPath, {
          name: typeof params?.name === "string" ? params.name : undefined,
          tokenBudget,
          timeBudgetMs,
        });
        if (!result.ok) {
          return { isError: true, content: [{ type: "text", text: result.error || "goal create failed" }] };
        }
        return {
          content: [{
            type: "text",
            text: `goal "${result.goal!.name}" created — budget: ${fmtBudget(result.goal!.tokenBudget, result.goal!.timeBudgetMs)}. You will get exactly one over-budget notice; check status anytime.`,
          }],
          details: { goal: result.goal },
        };
      }

      if (action === "pause") {
        const ok = engine.pause(sessionPath);
        return { content: [{ type: "text", text: ok ? "goal paused (time budget stops counting)" : "no active goal to pause" }], details: { ok } };
      }
      if (action === "resume") {
        const ok = engine.resume(sessionPath);
        return { content: [{ type: "text", text: ok ? "goal resumed" : "no paused goal to resume" }], details: { ok } };
      }
      if (action === "complete") {
        const ok = engine.complete(sessionPath);
        const { text } = engine.status(sessionPath);
        return { content: [{ type: "text", text: ok ? `goal completed. Final readout:\n${text}` : "no active/paused goal to complete" }], details: { ok } };
      }
      const ok = engine.drop(sessionPath);
      return { content: [{ type: "text", text: ok ? "goal dropped" : "no active/paused goal to drop" }], details: { ok } };
    },
  };
}
