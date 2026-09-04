import { KnowledgeError, isKnowledgeError } from "../errors.ts";
import type { KnowledgeResearchAction, KnowledgeResearchRun } from "../types.ts";
import type { EvidenceLedger } from "./evidence-ledger.ts";
import type { ResearchStore } from "./research-store.ts";
import type { KnowledgeResearchProgressUpdate } from "../../../shared/knowledge-research.ts";
import { createModuleLogger } from "../../debug-log.ts";

const progressLog = createModuleLogger("knowledge-research-progress");

/** 进度发送失败必须留痕，但不能取消真实调查或提前释放工作会话的共享额度。 */
export function notifyResearchProgress<T extends KnowledgeResearchProgressUpdate>(
  notify: ((event: T) => void) | undefined, event: T,
): void {
  if (!notify) return;
  try { notify(event); } catch { progressLog.warn(`Research progress listener failed: ${event.type}`); }
}

/** 来自隔离会话的宿主身份，不接受工具参数自行声明。 */
export interface KnowledgeResearchActorContext {
  runId: string;
  scopeId: string;
  actorSessionId: string | null;
  actorAgentId: string;
  role: "root" | "worker";
  allowedNeedIds?: string[];
  allowedSourceIds?: string[];
  completenessCheckId?: string;
  completenessShardId?: string;
}

export interface KnowledgeResearchToolDeps {
  research: ResearchStore;
  ledger: EvidenceLedger;
  budget: ResearchToolBudget;
  resolveContext: (ctx: unknown) => KnowledgeResearchActorContext | null;
  onProgress?: (update: KnowledgeResearchProgressUpdate) => void;
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
  if ((context.completenessCheckId !== undefined || context.completenessShardId !== undefined)
    && (context.role !== "worker" || !context.completenessCheckId?.trim() || !context.completenessShardId?.trim())) {
    throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Completeness worker requires its host-bound check and shard");
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
  pending: Set<Promise<void>>;
}
// 同一个宿主存储的所有预算器共享活动调用，避免不同工作会话各获一份并发额度。
const activeByStore = new WeakMap<object, Map<string, ActiveRun>>();

/** 只有运行中的调查、工具与资源清理全部退出后，宿主才能进入最终材料合成。 */
export function hasActiveResearchExecution(knowledgeStore: object, runId: string): boolean {
  const state = activeByStore.get(knowledgeStore)?.get(runId);
  return !!state && (state.workers > 0 || state.controllers.size > 0 || state.pending.size > 0);
}

function budgetError(reason: string): KnowledgeError {
  const message = reason === "evidence_update_required"
    ? "已读取到原文，请先调用 knowledge_research_update 处理本批材料：用 linkEvidence 登记相关的准确引文；全部无关时通过 unresolvedGaps 说明实际缺口，没有新增缺口可提交空更新。更新失败时先按逐条反馈纠正，成功处理后才能继续采集。"
    : reason === "round_search_limit"
      ? "本轮搜索次数已用完。请阅读已有命中的原文并登记证据，不要继续重复搜索。"
      : reason === "round_read_limit"
        ? "本轮读取次数已用完。请先登记已经读到的证据并说明仍未核实的部分。"
        : "Research budget does not allow another operation";
  return new KnowledgeError("KNOWLEDGE_CONFLICT", message, { stopReason: reason });
}

export class ResearchToolBudget {
  private readonly nowMs: () => number;
  private readonly active: Map<string, ActiveRun>;
  private readonly research: ResearchStore;

  constructor(research: ResearchStore, options: { nowMs?: () => number } = {}) {
    this.research = research;
    this.nowMs = options.nowMs ?? Date.now;
    let active = activeByStore.get(research.knowledgeStore);
    if (!active) activeByStore.set(research.knowledgeStore, active = new Map());
    this.active = active;
  }

  elapsedMs(runId: string): number {
    return Math.max(0, this.nowMs() - Date.parse(this.research.requireRun(runId).createdAt));
  }

  deadlineMs(runId: string): number {
    const run = this.research.requireRun(runId);
    return Date.parse(run.createdAt) + run.budget.maxWallClockMs;
  }

  /** 返回全体研究会话共同消耗的实时快照，不向任何工作会话另发额度。 */
  remainingBudget(runId: string): { shared: true; toolCalls: number; wallClockMs: number } {
    const run = this.research.requireRun(runId);
    return { shared: true, toolCalls: Math.max(0, run.budget.maxToolCalls - run.toolCallsUsed),
      wallClockMs: Math.max(0, Date.parse(run.createdAt) + run.budget.maxWallClockMs - this.nowMs()) };
  }

  private state(runId: string): ActiveRun {
    let state = this.active.get(runId);
    if (!state) this.active.set(runId, state = { workers: 0, controllers: new Set(), pending: new Set() });
    return state;
  }

  private releaseState(runId: string): void {
    const state = this.active.get(runId);
    if (state?.workers === 0 && state.controllers.size === 0 && state.pending.size === 0) this.active.delete(runId);
  }

  private trackPending(state: ActiveRun): () => void {
    let resolve!: () => void;
    const pending = new Promise<void>(done => { resolve = done; });
    state.pending.add(pending);
    return () => { state.pending.delete(pending); resolve(); };
  }

  private async drain(state: ActiveRun): Promise<void> {
    while (state.pending.size > 0) await Promise.allSettled([...state.pending]);
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

  private stopFromSignal(runId: string, signal: AbortSignal): void {
    const reason = signal.reason;
    const stopReason = isKnowledgeError(reason) ? reason.details.stopReason : undefined;
    if (stopReason === "tool_budget_exhausted" || stopReason === "wall_clock_exhausted") this.stop(runId, stopReason);
    else this.cancel(runId);
  }

  /** 整轮根会话也使用创建研究时确定的截止时间，并等待所有工具与工作会话真正清理完毕。 */
  async withRunController<T>(runId: string, signal: AbortSignal | undefined, work: (signal: AbortSignal) => Promise<T>): Promise<T> {
    if (signal?.aborted) { this.stopFromSignal(runId, signal); signal.throwIfAborted(); }
    const run = this.requireActive(runId);
    const remaining = this.deadlineMs(runId) - this.nowMs();
    const reason = remaining <= 0 ? "wall_clock_exhausted"
      : run.toolCallsUsed >= run.budget.maxToolCalls ? "tool_budget_exhausted" : null;
    if (reason) { this.stop(runId, reason); throw budgetError(reason); }
    const controller = new AbortController(), state = this.state(runId);
    state.controllers.add(controller);
    const onAbort = () => this.stopFromSignal(runId, signal!);
    signal?.addEventListener("abort", onAbort, { once: true });
    const timer = setTimeout(() => this.stop(runId, "wall_clock_exhausted"), remaining);
    timer.unref?.();
    try {
      const result = await work(controller.signal);
      await this.drain(state);
      if (this.nowMs() >= this.deadlineMs(runId)) this.stop(runId, "wall_clock_exhausted");
      controller.signal.throwIfAborted();
      return result;
    } catch (error) {
      if (controller.signal.aborted) {
        this.stopFromSignal(runId, controller.signal);
        throw controller.signal.reason;
      }
      throw error;
    } finally {
      await this.drain(state);
      clearTimeout(timer);
      signal?.removeEventListener("abort", onAbort);
      state.controllers.delete(controller);
      this.releaseState(runId);
    }
  }

  async withWorkerSlots<T>(runId: string, count: number, work: () => Promise<T>): Promise<T> {
    const run = this.requireActive(runId);
    if (!Number.isSafeInteger(count) || count < 1) throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Worker count is invalid");
    const state = this.state(runId);
    if (state.workers + count > run.budget.maxParallelAgents) throw budgetError("parallel_agent_limit");
    state.workers += count;
    const finishPending = this.trackPending(state);
    try {
      this.research.knowledgeStore.db.prepare("UPDATE knowledge_research_runs SET delegated_agents = delegated_agents + ?, updated_at = ? WHERE id = ?")
        .run(count, this.research.now(), runId);
      return await work();
    } finally { state.workers -= count; finishPending(); this.releaseState(runId); }
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
    if (input.signal?.aborted) { this.stopFromSignal(context.runId, input.signal); input.signal.throwIfAborted(); }
    const admitted = this.research.transaction(() => {
      const run = this.requireActive(context.runId);
      const reason = run.toolCallsUsed >= run.budget.maxToolCalls ? "tool_budget_exhausted"
        : this.elapsedMs(run.id) >= run.budget.maxWallClockMs ? "wall_clock_exhausted" : null;
      if (reason) return { run, reason, action: null };
      const round = this.research.listRounds(run.id).filter(item => item.status === "running").at(-1);
      const actions = this.research.listActions(run.id);
      const actorActions = actions.filter(action => action.actorSessionId === context.actorSessionId
        && action.actorAgentId === context.actorAgentId);
      const lastUpdateOrdinal = Math.max(-1, ...actorActions.filter(action => action.actionType === "knowledge_research_update"
        && action.status === "completed" && action.errorCode === null && action.responseSummary?.status === "completed")
        .map(action => action.ordinal));
      // 已读材料先处理再采集；缺口或空更新可以确认材料无关，但绝不产生证据或支持状态。
      const mustUpdateEvidence = !context.completenessCheckId
        && ["knowledge_outline", "knowledge_search", "knowledge_read", "knowledge_grep", "knowledge_delegate"].includes(toolName)
        && actorActions.some(action => action.ordinal > lastUpdateOrdinal
          && ["knowledge_read", "knowledge_grep"].includes(action.actionType)
          && action.status === "completed" && action.errorCode === null
          && Array.isArray(action.responseSummary?.receiptIds) && action.responseSummary.receiptIds.length > 0);
      const isRead = toolName === "knowledge_read" || toolName === "knowledge_coverage_read";
      const perRound = actions.filter(action => action.roundId === (round?.id ?? null)
        && (isRead ? ["knowledge_read", "knowledge_coverage_read"].includes(action.actionType) : action.actionType === toolName)).length;
      const perRoundReason = toolName === "knowledge_search" && perRound >= run.budget.maxSearchesPerRound ? "round_search_limit"
        : isRead && perRound >= run.budget.maxReadsPerRound ? "round_read_limit" : null;
      const action = this.research.insertAction({
        id: this.research.newId("kra"), runId: run.id, roundId: round?.id ?? null,
        ordinal: Math.max(-1, ...actions.map(item => item.ordinal)) + 1,
        actorSessionId: context.actorSessionId, actorAgentId: context.actorAgentId, actionType: toolName,
        requestSummary: input.requestSummary, responseSummary: null, status: "running",
        startedAt: this.research.now(), completedAt: null, errorCode: null,
      });
      this.research.knowledgeStore.db.prepare(`UPDATE knowledge_research_runs SET tool_calls_used = tool_calls_used + 1,
        search_calls = search_calls + ?, read_calls = read_calls + ?, grep_calls = grep_calls + ?, updated_at = ? WHERE id = ?`)
        .run(Number(toolName === "knowledge_search"), Number(isRead), Number(toolName === "knowledge_grep"), this.research.now(), run.id);
      return { run, reason: mustUpdateEvidence ? "evidence_update_required" : perRoundReason, action };
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
    const finishPending = this.trackPending(state);
    const onAbort = () => this.stopFromSignal(context.runId, input.signal!);
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
      finishPending();
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
