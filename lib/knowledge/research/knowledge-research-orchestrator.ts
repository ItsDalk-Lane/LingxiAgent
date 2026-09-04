import type { KnowledgeExecutionPolicy } from "../../../shared/knowledge-execution.ts";
import type { KnowledgeResearchRunStatus, KnowledgeResearchProgress, KnowledgeResearchProgressUpdate } from "../../../shared/knowledge-research.ts";
import { KnowledgeError } from "../errors.ts";
import type { CompiledKnowledgeScope } from "../scope-snapshot-compiler.ts";
import type { KnowledgeResearchAction, KnowledgeResearchRun } from "../types.ts";
import { EvidenceLedger, type EvaluatedEvidenceNeed } from "./evidence-ledger.ts";
import { ResearchContextRenderer } from "./research-context-renderer.ts";
import { buildResearchPrompt } from "./research-prompts.ts";
import { ResearchRoundRunner, type ResearchExecuteIsolated, type ResearchSearchSummary } from "./research-round-runner.ts";
import { evaluateResearchStopPolicy } from "./research-stop-policy.ts";
import { ResearchStore } from "./research-store.ts";
import { hasActiveResearchExecution, notifyResearchProgress, ResearchToolBudget } from "./research-tool-budget.ts";
import { KnowledgeCompletenessExecutor } from "./knowledge-completeness-executor.ts";

export interface KnowledgeResearchRequest {
  question: string;
  compiledScope: CompiledKnowledgeScope;
  policy: KnowledgeExecutionPolicy;
  parentSessionId: string;
  parentSessionPath: string;
  agentId: string;
  turnId: string;
  signal?: AbortSignal;
}

interface ResearchSearchPlan {
  query: string;
  needIds: string[];
  purpose?: "counterexample";
}

const activeRuns = new WeakMap<object, Set<string>>();
const activeStatuses = new Set<KnowledgeResearchRunStatus>(["planning", "running", "synthesizing"]);
const normalizeQuery = (query: string) => query.normalize("NFKC").trim().replace(/\s+/g, " ").toLowerCase();
const successful = (action: KnowledgeResearchAction) => action.status === "completed" && action.errorCode === null
  && action.responseSummary?.errorCode == null;

interface KnowledgeResearchDependencies {
  research: ResearchStore;
  executeIsolated: ResearchExecuteIsolated;
  nowMs?: () => number;
  isCompletenessSatisfied?: (runId: string, needId?: string) => boolean;
  onSearchCompleted?: (summary: ResearchSearchSummary) => void;
  onProgress?: (event: KnowledgeResearchProgress) => void;
}

/** 每轮都从宿主账本决定下一步；隔离模型的普通回复不参与完整性判断或最终回答。 */
export class KnowledgeResearchOrchestrator {
  private readonly budget: ResearchToolBudget;
  private readonly ledger: EvidenceLedger;
  private readonly runner: ResearchRoundRunner;
  private readonly renderer: ResearchContextRenderer;
  private readonly deps: KnowledgeResearchDependencies;
  private readonly completeness: KnowledgeCompletenessExecutor;

  constructor(deps: KnowledgeResearchDependencies) {
    this.deps = deps;
    this.budget = new ResearchToolBudget(deps.research, { nowMs: deps.nowMs });
    this.completeness = new KnowledgeCompletenessExecutor({ research: deps.research, budget: this.budget,
      executeIsolated: deps.executeIsolated });
    this.ledger = new EvidenceLedger(deps.research, { isCompletenessSatisfied: (runId, needId) => this.isCompletenessSatisfied(runId, needId) });
    this.runner = new ResearchRoundRunner({ ...deps, budget: this.budget });
    this.renderer = new ResearchContextRenderer({ research: deps.research });
  }

  private isCompletenessSatisfied(runId: string, needId?: string): boolean {
    return this.deps.isCompletenessSatisfied?.(runId, needId) ?? this.completeness.isSatisfied(runId, needId);
  }

  async run(request: KnowledgeResearchRequest) {
    this.validateRequest(request);
    const research = this.deps.research;
    // 同一冻结轮次崩溃后继续原研究，绝对时限与预算不重新发放。
    const existing = research.knowledgeStore.db.prepare(`SELECT id FROM knowledge_research_runs
      WHERE turn_scope_id = ? AND turn_id = ? AND parent_session_path = ? AND question = ?
      ORDER BY created_at DESC, id LIMIT 1`)
      .get(request.compiledScope.scopeId, request.turnId, request.parentSessionPath, request.question) as { id: string } | undefined;
    const run = existing ? research.requireRun(existing.id) : research.createRun({
      turnScopeId: request.compiledScope.scopeId, turnId: request.turnId, parentSessionPath: request.parentSessionPath,
      question: request.question, completenessPolicy: request.policy.completenessPolicy,
    });
    let active = activeRuns.get(research.knowledgeStore);
    if (!active) activeRuns.set(research.knowledgeStore, active = new Set());
    if (active.has(run.id) || hasActiveResearchExecution(research.knowledgeStore, run.id)) {
      throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Research run is already executing");
    }
    active.add(run.id);
    try {
      this.publish(run.id, { type: "knowledge_research_started" });
      if (existing && !activeStatuses.has(run.status)) {
        const rendered = this.renderer.render({ runId: run.id, compiledScope: request.compiledScope,
          needs: this.needs(run.id), terminalStatus: run.status as "completed" | "partial" | "failed" | "cancelled" });
        this.publish(run.id, { type: "knowledge_research_completed", status: run.status as "completed" | "partial" | "failed" | "cancelled",
          stopReason: run.stopReason });
        return { ...rendered, run };
      }
      if (existing && run.status === "synthesizing") {
        const decision = this.stopDecision(run, this.needs(run.id));
        return this.finalize(request, run.id, decision.stopReason === "complete" ? "completed" : "partial",
          decision.stopReason ?? "research_synthesis_incomplete");
      }
      if (existing) this.recoverInterruptedActions(run.id);
      return await this.execute(request, run.id);
    } catch (error) {
      // 不把失败留成正在合成，否则重试可能创建新研究并重新获得整份预算。
      if (activeStatuses.has(research.requireRun(run.id).status)) {
        this.closeInterruptedRounds(run.id, "research_execution_failed");
        research.setRunState(run.id, { status: "failed", stopReason: "research_execution_failed" });
      }
      const stopped = research.requireRun(run.id);
      this.publish(run.id, { type: "knowledge_research_completed",
        status: stopped.status as "completed" | "partial" | "failed" | "cancelled", stopReason: stopped.stopReason });
      throw error;
    } finally {
      active.delete(run.id);
      if (active.size === 0) activeRuns.delete(research.knowledgeStore);
    }
  }

  private publish(runId: string, update: KnowledgeResearchProgressUpdate): void {
    if (!this.deps.onProgress) return;
    const run = this.deps.research.requireRun(runId), needs = this.needs(runId);
    notifyResearchProgress(this.deps.onProgress, { ...update, runId, scopeId: run.turnScopeId,
      rounds: run.roundsCompleted, maxRounds: run.budget.maxRounds,
      searchCalls: run.searchCalls, readCalls: run.readCalls, delegatedAgents: run.delegatedAgents,
      needsTotal: needs.length, needsSupported: needs.filter(need => need.status === "supported").length,
      needsPartial: needs.filter(need => need.status === "partial").length,
      needsConflicted: needs.filter(need => need.status === "conflicted").length,
      unresolvedNeedIds: needs.filter(need => !["supported", "not_applicable"].includes(need.status)).map(need => need.id),
    });
  }

  private validateRequest(request: KnowledgeResearchRequest): void {
    const compiled = request.compiledScope;
    const scope = this.deps.research.knowledgeStore.getTurnScope({ scopeId: compiled.scopeId });
    const same = (a: string[], b: string[]) => JSON.stringify([...a].sort()) === JSON.stringify([...b].sort());
    if (request.policy.path !== "detailed_research" || request.policy.mode !== "detailed"
      || !request.parentSessionId?.trim() || !request.agentId?.trim()
      || !scope || scope.status !== "active" || scope.sessionPath !== request.parentSessionPath
      || compiled.sessionPath !== scope.sessionPath || compiled.studioId !== scope.studioId
      || compiled.turnId !== scope.turnId || request.turnId !== scope.turnId
      || !same(compiled.notebookIds, scope.notebookIds) || compiled.sources.length !== scope.sources.length
      || !same(compiled.sources.map(source => source.sourceId), scope.sources.map(source => source.sourceId))
      || compiled.sources.some(source => !scope.sources.some(frozen => frozen.sourceId === source.sourceId
        && frozen.contentSnapshotId === source.contentSnapshotId && frozen.parseArtifactId === source.parseArtifactId
        && same(frozen.notebookIds, source.notebookIds)))) {
      throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Research request differs from its frozen turn scope");
    }
  }

  private recoverInterruptedActions(runId: string): void {
    const research = this.deps.research;
    for (const action of research.listActions(runId).filter(action => action.status === "running")) {
      research.finishAction(runId, action.id, { status: "cancelled", responseSummary: null, errorCode: "RESEARCH_HOST_RESTARTED" });
    }
    research.setRunState(runId, { status: "running", degradedReason: "research_round_restarted" });
  }

  private needs(runId: string): EvaluatedEvidenceNeed[] {
    const research = this.deps.research;
    return activeStatuses.has(research.requireRun(runId).status) ? this.ledger.recompute(runId)
      : research.listNeeds(runId).map(need => this.ledger.evaluateNeed(runId, need.id));
  }

  private async execute(request: KnowledgeResearchRequest, runId: string) {
    const research = this.deps.research;
    let protocolFailures = 0, criticalFailures = 0;
    let finalStatus: "completed" | "partial" | "failed" | "cancelled" = "partial";
    let stopReason = "round_budget_exhausted";
    while (true) {
      let run = research.requireRun(runId);
      if (request.signal?.aborted) this.budget.cancel(runId);
      run = research.requireRun(runId);
      if (!activeStatuses.has(run.status)) {
        finalStatus = run.status === "cancelled" ? "cancelled" : run.status === "failed" ? "failed" : "partial";
        stopReason = run.stopReason ?? "research_stopped"; break;
      }
      const beforeNeeds = this.needs(runId);
      const before = this.stopDecision(run, beforeNeeds);
      // 首轮不能从空账本直接结束；恢复时也要受原始绝对预算限制。
      if (before.shouldStop && before.stopReason !== "complete") {
        finalStatus = before.status!; stopReason = before.stopReason!; break;
      }
      const focus = beforeNeeds.filter(need => need.status !== "not_applicable" && (need.status !== "supported"
        || (need.requireCounterEvidence && !need.counterEvidenceChecked)
        || (need.requireAllRelevantUnits && !need.completenessSatisfied)));
      const actions = research.listActions(runId);
      const searchPlan = this.searchPlan(focus, actions);
      const fullScopeIds = request.compiledScope.sources.map(source => source.sourceId).sort().join("\0");
      const forbiddenQueries = actions.filter(action => successful(action) && action.actionType === "knowledge_search"
        && action.requestSummary.sectionKeys === undefined
        && Array.isArray(action.requestSummary.sourceIds)
        && [...action.requestSummary.sourceIds].sort().join("\0") === fullScopeIds)
        .flatMap(action => typeof action.requestSummary.query === "string" ? [action.requestSummary.query] : []);
      const rounds = research.listRounds(runId);
      const round = rounds.find(round => round.status === "running") ?? research.beginRound(runId, { focus: focus.map(need => need.id) });
      this.publish(runId, { type: "knowledge_research_round_started", roundId: round.id, round: round.ordinal + 1 });
      const beforeIds = new Set(research.listEvidence(runId).map(item => item.id));
      const prompt = buildResearchPrompt({ question: request.question, compiledScope: request.compiledScope,
        run: research.requireRun(runId), needs: beforeNeeds, evidence: research.listEvidence(runId), relations: research.listRelations(runId),
        actions, previousNewEvidenceCount: rounds.filter(item => item.status !== "running").at(-1)?.newEvidenceCount ?? null,
        focusNeedIds: focus.map(need => need.id), searchPlan,
      });
      const result = await this.runner.run({ runId, roundId: round.id, agentId: request.agentId,
        parentSessionId: request.parentSessionId, parentSessionPath: request.parentSessionPath,
        studioId: request.compiledScope.studioId, scopeId: request.compiledScope.scopeId,
        prompt, signal: request.signal, searchPlan, forbiddenQueries,
        isCompletenessSatisfied: id => this.isCompletenessSatisfied(id), completeness: this.completeness,
        ensureCompleteness: (context, sessionPath, signal) => this.completeness.ensure({
          runId, compiledScope: request.compiledScope, parentSessionId: context.actorSessionId!,
          parentSessionPath: sessionPath, agentId: context.actorAgentId, signal,
          onProgress: update => this.publish(runId, update),
        }),
        onSearchCompleted: this.deps.onSearchCompleted,
        onProgress: update => this.publish(runId, update),
      });
      run = research.requireRun(runId);
      const newEvidenceCount = research.listEvidence(runId).filter(item => !beforeIds.has(item.id)).length;
      const roundActions = research.listActions(runId).filter(action => action.roundId === round.id);
      const retrievalActions = roundActions.filter(action => ["knowledge_search", "knowledge_read", "knowledge_grep"].includes(action.actionType));
      const toolUnavailable = roundActions.some(action => ["knowledge_outline", "knowledge_search", "knowledge_read", "knowledge_grep"].includes(action.actionType)
        && action.errorCode === "KNOWLEDGE_RETRIEVAL_UNAVAILABLE")
        && !retrievalActions.some(successful) && newEvidenceCount === 0;
      const critical = Boolean(result.errorCode && !["wall_clock_exhausted", "tool_budget_exhausted", "KNOWLEDGE_RESEARCH_CANCELLED"].includes(result.errorCode))
        || toolUnavailable;
      criticalFailures = critical ? criticalFailures + 1 : 0;
      let fallback = false;
      if (round.ordinal === 0 && research.listNeeds(runId).length === 0 && activeStatuses.has(run.status)) {
        research.createFallbackNeed(runId); fallback = true;
        this.publish(runId, { type: "knowledge_research_plan_updated" });
      }
      const needs = this.needs(runId);
      const outlineFirst = round.ordinal !== 0 || (roundActions[0]?.actionType === "knowledge_outline" && successful(roundActions[0]));
      const finishAttempted = roundActions.some(action => action.actionType === "knowledge_research_finish" && successful(action));
      const investigationValid = this.independentInvestigation(runId, needs);
      const protocolValid = outlineFirst && finishAttempted && investigationValid;
      protocolFailures = fallback ? 0 : protocolValid ? 0 : protocolFailures + 1;
      const roundError = result.errorCode ?? (!protocolValid && !fallback ? "KNOWLEDGE_RESEARCH_PROTOCOL_FAILED" : null);
      const roundStatus = run.status === "cancelled" ? "cancelled" : roundError ? "failed" : "completed";
      research.finishRound(runId, round.id, { status: roundStatus, newEvidenceCount, errorCode: roundError });
      this.publish(runId, { type: "knowledge_research_ledger_updated", phase: "reviewing",
        roundStatus, roundStopReason: run.stopReason ?? roundError });
      run = research.requireRun(runId);
      if (!activeStatuses.has(run.status)) {
        finalStatus = run.status === "cancelled" ? "cancelled" : run.status === "failed" ? "failed" : "partial";
        stopReason = run.stopReason ?? "research_stopped"; break;
      }
      const decision = this.stopDecision(run, needs);
      if (decision.shouldStop && ["tool_budget_exhausted", "wall_clock_exhausted", "round_budget_exhausted"].includes(decision.stopReason!)) {
        finalStatus = "partial"; stopReason = decision.stopReason!; break;
      }
      if (criticalFailures >= 2) {
        finalStatus = research.listEvidence(runId).length ? "partial" : "failed";
        stopReason = "critical_tools_unavailable"; break;
      }
      if (protocolFailures >= 2) { stopReason = "agent_protocol_failure"; break; }
      if (!fallback && decision.shouldStop && (decision.stopReason !== "complete" || protocolValid)) {
        finalStatus = decision.status!; stopReason = decision.stopReason!; break;
      }
    }
    return this.finalize(request, runId, finalStatus, stopReason);
  }

  private finalize(request: KnowledgeResearchRequest, runId: string,
    finalStatus: "completed" | "partial" | "failed" | "cancelled", stopReason: string) {
    const research = this.deps.research;
    this.closeInterruptedRounds(runId, stopReason);
    const run = research.requireRun(runId);
    if (finalStatus === "cancelled" || finalStatus === "failed") {
      if (activeStatuses.has(run.status)) research.setRunState(runId, { status: finalStatus, stopReason });
    } else {
      research.beginSynthesis(runId);
      research.setRunState(runId, { status: "synthesizing", stopReason });
    }
    const rendered = this.renderer.render({ runId, compiledScope: request.compiledScope, needs: this.needs(runId), terminalStatus: finalStatus });
    if (finalStatus !== "cancelled" && finalStatus !== "failed") research.setRunState(runId, { status: finalStatus, stopReason });
    this.publish(runId, { type: "knowledge_research_completed", status: finalStatus, stopReason });
    return { ...rendered, run: research.requireRun(runId) };
  }

  private stopDecision(run: KnowledgeResearchRun, needs: EvaluatedEvidenceNeed[]) {
    return evaluateResearchStopPolicy({ needs, run, elapsedMs: this.budget.elapsedMs(run.id),
      recentRoundEvidenceCounts: this.deps.research.listRounds(run.id).filter(round => round.status !== "running").map(round => round.newEvidenceCount),
      completenessSatisfied: this.isCompletenessSatisfied(run.id),
    });
  }

  private closeInterruptedRounds(runId: string, reason: string): void {
    const research = this.deps.research;
    for (const round of research.listRounds(runId).filter(round => round.status === "running")) {
      research.finishRound(runId, round.id, { status: reason === "cancelled" ? "cancelled" : "failed",
        newEvidenceCount: round.newEvidenceCount, errorCode: reason });
    }
  }

  private searchPlan(needs: EvaluatedEvidenceNeed[], actions: KnowledgeResearchAction[]): ResearchSearchPlan[] {
    const prior = new Set(actions.filter(successful).flatMap(action => typeof action.requestSummary.query === "string"
      ? [normalizeQuery(action.requestSummary.query)] : []));
    return needs.flatMap(need => {
      const counter = need.requireCounterEvidence && !need.counterEvidenceChecked;
      const suffix = counter ? " 反例 例外 不成立" : need.status === "conflicted" ? " 矛盾 差异 适用条件" : "";
      // 补建需求保留完整用户问题，具体查询仍遵守既有搜索工具的四千字符上限。
      const query = need.claim.slice(0, 4000 - suffix.length) + suffix;
      return prior.has(normalizeQuery(query)) ? [] : [{ query, needIds: [need.id], ...(counter ? { purpose: "counterexample" as const } : {}) }];
    });
  }

  private independentInvestigation(runId: string, needs: EvaluatedEvidenceNeed[]): boolean {
    const required = needs.filter(need => need.required && need.status !== "not_applicable");
    if (required.length < 2) return true;
    const research = this.deps.research, actions = research.listActions(runId).filter(successful);
    if (research.requireRun(runId).delegatedAgents > 0 && actions.some(action => action.actionType === "knowledge_delegate")) return true;
    const usedSearch = new Set<string>(), usedRead = new Set<string>();
    return required.every(need => {
      const evidence = research.listEvidence(runId).filter(item => [...need.evidenceIds, ...need.counterEvidenceIds].includes(item.id));
      const reads = actions.filter(action => action.actionType === "knowledge_read" && !usedRead.has(action.id)
        && Array.isArray(action.responseSummary?.receiptIds) && action.responseSummary.receiptIds.some(id => {
          const receipt = research.getReceipt(runId, String(id));
          return receipt.consumedAt !== null && evidence.some(item => item.blockId === receipt.blockId
            && item.startOffset >= receipt.startOffset && item.endOffset <= receipt.endOffset);
        }));
      for (const read of reads) {
        const receiptIds = read.responseSummary!.receiptIds as string[];
        const sources = new Set(receiptIds.flatMap(id => {
          const receipt = research.getReceipt(runId, id);
          return receipt.consumedAt !== null && evidence.some(item => item.blockId === receipt.blockId
            && item.startOffset >= receipt.startOffset && item.endOffset <= receipt.endOffset) ? [receipt.sourceId] : [];
        }));
        const search = actions.find(action => action.actionType === "knowledge_search" && !usedSearch.has(action.id)
          && action.ordinal < read.ordinal && action.actorSessionId === read.actorSessionId
          && Number(action.responseSummary?.count) > 0
          && (!Array.isArray(action.requestSummary.needIds) || action.requestSummary.needIds.includes(need.id))
          && Array.isArray(action.requestSummary.sourceIds) && action.requestSummary.sourceIds.some(id => sources.has(String(id))));
        if (search) { usedSearch.add(search.id); usedRead.add(read.id); return true; }
      }
      return false;
    });
  }
}
