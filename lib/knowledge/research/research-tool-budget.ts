import { KnowledgeError, isKnowledgeError } from "../errors.ts";
import type { KnowledgeResearchAction, KnowledgeResearchRun } from "../types.ts";
import type { EvidenceLedger } from "./evidence-ledger.ts";
import type { ResearchStore } from "./research-store.ts";

/** 来自隔离会话的宿主身份，不接受工具参数自行声明。 */
export interface KnowledgeResearchActorContext {
  runId: string;
  scopeId: string;
  actorSessionId: string | null;
  actorAgentId: string;
  role: "root" | "worker";
  allowedNeedIds?: string[];
  allowedSourceIds?: string[];
}

export interface KnowledgeResearchToolDeps {
  research: ResearchStore;
  ledger: EvidenceLedger;
  budget: ResearchToolBudget;
  resolveContext: (ctx: unknown) => KnowledgeResearchActorContext | null;
}

export function requireResearchToolContext(
  deps: Pick<KnowledgeResearchToolDeps, "research" | "resolveContext">,
  ctx: unknown,
  runId: unknown,
): KnowledgeResearchActorContext {
  const context = deps.resolveContext(ctx);
  if (!context || typeof runId !== "string" || context.runId !== runId
    || !["root", "worker"].includes(context.role)) {
    throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Research tool requires its host-bound run");
  }
  const run = deps.research.requireRun(runId);
  const scope = deps.research.knowledgeStore.getTurnScope({ scopeId: context.scopeId });
  if (!scope || scope.id !== run.turnScopeId || scope.status !== "active") {
    throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Research tool scope is unavailable");
  }
  if (context.role === "worker" && (!context.allowedNeedIds || context.allowedNeedIds.length === 0)) {
    throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Research worker requires assigned needs");
  }
  for (const id of context.allowedNeedIds ?? []) deps.research.getNeed(run.id, id);
  if (context.allowedSourceIds?.some(id => !scope.sources.some(source => source.sourceId === id))) {
    throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Research worker source is outside the frozen scope");
  }
  return context;
}

interface ActiveRun {
  workers: number;
  controllers: Set<AbortController>;
}
// 同一个宿主存储的所有预算器共享活动调用，避免不同工作会话各获一份并发额度。
const activeByStore = new WeakMap<object, Map<string, ActiveRun>>();

function budgetError(reason: string): KnowledgeError {
  return new KnowledgeError("KNOWLEDGE_CONFLICT", "Research budget does not allow another operation", { stopReason: reason });
}

export class ResearchToolBudget {
  private readonly nowMs: () => number;
  private readonly active: Map<string, ActiveRun>;

  constructor(private readonly research: ResearchStore, options: { nowMs?: () => number } = {}) {
    this.nowMs = options.nowMs ?? Date.now;
    let active = activeByStore.get(research.knowledgeStore);
    if (!active) activeByStore.set(research.knowledgeStore, active = new Map());
    this.active = active;
  }

  elapsedMs(runId: string): number {
    return Math.max(0, this.nowMs() - Date.parse(this.research.requireRun(runId).createdAt));
  }

  private state(runId: string): ActiveRun {
    let state = this.active.get(runId);
    if (!state) this.active.set(runId, state = { workers: 0, controllers: new Set() });
    return state;
  }

  private releaseState(runId: string): void {
    const state = this.active.get(runId);
    if (state?.workers === 0 && state.controllers.size === 0) this.active.delete(runId);
  }

  private requireActive(runId: string): KnowledgeResearchRun {
    const run = this.research.requireRun(runId);
    if (!["planning", "running", "synthesizing"].includes(run.status)) {
      throw budgetError(run.stopReason ?? "research_not_active");
    }
    const scope = this.research.knowledgeStore.getTurnScope({ scopeId: run.turnScopeId });
    if (scope?.status !== "active") throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Research scope is closed");
    return run;
  }

  private stop(runId: string, reason: string, status: "partial" | "cancelled" = "partial"): void {
    this.research.knowledgeStore.db.prepare(`UPDATE knowledge_research_runs SET status = ?, stop_reason = ?,
      updated_at = ?, completed_at = ? WHERE id = ? AND status IN ('planning', 'running', 'synthesizing')`)
      .run(status, reason, this.research.now(), this.research.now(), runId);
    for (const controller of this.active.get(runId)?.controllers ?? []) {
      if (!controller.signal.aborted) controller.abort(budgetError(reason));
    }
  }

  /** 取消和超时传递给正在等待的每一个调查，不重新发放轮次时间。 */
  cancel(runId: string): void { this.stop(runId, "cancelled", "cancelled"); }

  async withWorkerSlots<T>(runId: string, count: number, work: () => Promise<T>): Promise<T> {
    const run = this.requireActive(runId);
    if (!Number.isSafeInteger(count) || count < 1) throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Worker count is invalid");
    const state = this.state(runId);
    if (state.workers + count > run.budget.maxParallelAgents) throw budgetError("parallel_agent_limit");
    state.workers += count;
    try {
      this.research.knowledgeStore.db.prepare("UPDATE knowledge_research_runs SET delegated_agents = delegated_agents + ?, updated_at = ? WHERE id = ?")
        .run(count, this.research.now(), runId);
      return await work();
    } finally { state.workers -= count; this.releaseState(runId); }
  }

  async execute<T>(input: {
    context: KnowledgeResearchActorContext;
    toolName: string;
    requestSummary: Record<string, unknown>;
    signal?: AbortSignal;
  }, work: (signal: AbortSignal) => Promise<{ value: T; summary: Record<string, unknown> }>
    | { value: T; summary: Record<string, unknown> }): Promise<T> {
    const { context, toolName } = input;
    requireResearchToolContext({ research: this.research, resolveContext: () => context }, undefined, context.runId);
    if (input.signal?.aborted) { this.cancel(context.runId); input.signal.throwIfAborted(); }
    const admitted = this.research.transaction(() => {
      const run = this.requireActive(context.runId);
      const reason = run.toolCallsUsed >= run.budget.maxToolCalls ? "tool_budget_exhausted"
        : this.elapsedMs(run.id) >= run.budget.maxWallClockMs ? "wall_clock_exhausted" : null;
      if (reason) return { run, reason, action: null };
      const round = this.research.listRounds(run.id).filter(item => item.status === "running").at(-1);
      const actions = this.research.listActions(run.id);
      const perRound = actions.filter(action => action.roundId === (round?.id ?? null) && action.actionType === toolName).length;
      const perRoundReason = toolName === "knowledge_search" && perRound >= run.budget.maxSearchesPerRound ? "round_search_limit"
        : toolName === "knowledge_read" && perRound >= run.budget.maxReadsPerRound ? "round_read_limit" : null;
      const action = this.research.insertAction({
        id: this.research.newId("kra"), runId: run.id, roundId: round?.id ?? null,
        ordinal: Math.max(-1, ...actions.map(item => item.ordinal)) + 1,
        actorSessionId: context.actorSessionId, actorAgentId: context.actorAgentId, actionType: toolName,
        requestSummary: input.requestSummary, responseSummary: null, status: "running",
        startedAt: this.research.now(), completedAt: null, errorCode: null,
      });
      this.research.knowledgeStore.db.prepare(`UPDATE knowledge_research_runs SET tool_calls_used = tool_calls_used + 1,
        search_calls = search_calls + ?, read_calls = read_calls + ?, grep_calls = grep_calls + ?, updated_at = ? WHERE id = ?`)
        .run(Number(toolName === "knowledge_search"), Number(toolName === "knowledge_read"), Number(toolName === "knowledge_grep"), this.research.now(), run.id);
      return { run, reason: perRoundReason, action };
    });
    if (admitted.reason) {
      if (admitted.action) this.finishAction(admitted.action, "failed", null, admitted.reason);
      if (admitted.reason === "tool_budget_exhausted" || admitted.reason === "wall_clock_exhausted") this.stop(context.runId, admitted.reason);
      else this.stopIfToolBudgetReached(context.runId);
      throw budgetError(admitted.reason);
    }
    const action = admitted.action!;
    const controller = new AbortController();
    const state = this.state(context.runId);
    state.controllers.add(controller);
    const onAbort = () => this.cancel(context.runId);
    input.signal?.addEventListener("abort", onAbort, { once: true });
    const remaining = Math.max(0, admitted.run.budget.maxWallClockMs - this.elapsedMs(context.runId));
    const timer = setTimeout(() => this.stop(context.runId, "wall_clock_exhausted"), remaining);
    timer.unref?.();
    try {
      controller.signal.throwIfAborted();
      const result = await work(controller.signal);
      controller.signal.throwIfAborted();
      if (this.elapsedMs(context.runId) >= admitted.run.budget.maxWallClockMs) {
        this.stop(context.runId, "wall_clock_exhausted");
        controller.signal.throwIfAborted();
      }
      this.finishAction(action, "completed", result.summary, null);
      this.stopIfToolBudgetReached(context.runId);
      return result.value;
    } catch (error) {
      // 同步工具事务可能把取消写入一起回滚；等工具回滚后再次落实宿主终态。
      if (controller.signal.aborted) {
        const reason = controller.signal.reason;
        const stopReason = isKnowledgeError(reason) ? reason.details.stopReason : undefined;
        if (stopReason === "cancelled") this.cancel(context.runId);
        else if (stopReason === "tool_budget_exhausted" || stopReason === "wall_clock_exhausted") {
          this.stop(context.runId, stopReason);
        } else if (input.signal?.aborted) this.cancel(context.runId);
      }
      this.finishAction(action, controller.signal.aborted ? "cancelled" : "failed", null,
        isKnowledgeError(error) ? error.code : controller.signal.aborted ? "RESEARCH_CANCELLED" : "RESEARCH_TOOL_FAILED");
      this.stopIfToolBudgetReached(context.runId);
      throw error;
    } finally {
      clearTimeout(timer);
      input.signal?.removeEventListener("abort", onAbort);
      state.controllers.delete(controller);
      this.releaseState(context.runId);
    }
  }

  private stopIfToolBudgetReached(runId: string): void {
    const run = this.research.requireRun(runId);
    if (run.toolCallsUsed >= run.budget.maxToolCalls) this.stop(runId, "tool_budget_exhausted");
  }

  private finishAction(action: KnowledgeResearchAction, status: "completed" | "failed" | "cancelled",
    responseSummary: Record<string, unknown> | null, errorCode: string | null): void {
    this.research.finishAction(action.runId, action.id, { status, responseSummary, errorCode });
  }
}
