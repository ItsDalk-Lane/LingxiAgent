import type { KnowledgeResearchRun } from "../types.ts";
import type { EvaluatedEvidenceNeed } from "./evidence-ledger.ts";

export type KnowledgeResearchRequestedStopReason = "complete" | "budget_exhausted" | "no_progress";

export type KnowledgeResearchStopReason =
  | "complete"
  | "round_budget_exhausted"
  | "tool_budget_exhausted"
  | "wall_clock_exhausted"
  | "no_progress"
  | "cancelled";

export interface KnowledgeResearchStopPolicyInput {
  /** 必须由宿主重新计算，不能传入模型声明的需求状态。 */
  needs: readonly EvaluatedEvidenceNeed[];
  run: Pick<KnowledgeResearchRun, "budget" | "roundsCompleted" | "toolCallsUsed" | "completenessPolicy">;
  /** 从整个研究开始累计，各轮和工作会话共享，不能每轮归零。 */
  elapsedMs: number;
  /** 已完成轮次的新增有效证据数量，按轮次先后排列。 */
  recentRoundEvidenceCounts: readonly number[];
  /** 完整性执行器提供的范围检查结果，不能采用模型自报的完成结论。 */
  completenessSatisfied: boolean;
  cancelled?: boolean;
  requestedStopReason?: KnowledgeResearchRequestedStopReason;
}

export interface KnowledgeResearchStopPolicyResult {
  shouldStop: boolean;
  status: "completed" | "partial" | "cancelled" | null;
  stopReason: KnowledgeResearchStopReason | null;
  requestedStopAllowed: boolean;
}

/** 模型只能申请停止；是否完整、预算是否用尽都按宿主记录判断。 */
export function evaluateResearchStopPolicy(input: KnowledgeResearchStopPolicyInput): KnowledgeResearchStopPolicyResult {
  const { run, needs } = input;
  const requiresScopeCompleteness = run.completenessPolicy === "relevant_sections_complete"
    || run.completenessPolicy === "scope_complete"
    || needs.some(need => need.status !== "not_applicable" && need.requireAllRelevantUnits);
  const complete = needs.length > 0
    && (!requiresScopeCompleteness || input.completenessSatisfied)
    && needs.every(need => {
      // 不适用必须已经由宿主接受，不再要求为不适用的结论收集来源。
      if (need.status === "not_applicable") return true;
      if (need.required && need.status !== "supported") return false;
      return need.independentSourceCount >= need.minIndependentSources
        && (!need.requireCounterEvidence || need.counterEvidenceChecked)
        && (!need.requireAllRelevantUnits || need.completenessSatisfied);
    });
  const budgetReason: KnowledgeResearchStopReason | null = run.toolCallsUsed >= run.budget.maxToolCalls
    ? "tool_budget_exhausted"
    : run.roundsCompleted >= run.budget.maxRounds ? "round_budget_exhausted"
      : input.elapsedMs >= run.budget.maxWallClockMs ? "wall_clock_exhausted" : null;
  const recent = input.recentRoundEvidenceCounts;
  const noProgress = run.roundsCompleted >= 2 && recent.length >= 2
    && recent[recent.length - 1] === 0 && recent[recent.length - 2] === 0;

  let stopReason: KnowledgeResearchStopReason | null = null;
  if (input.cancelled) {
    stopReason = "cancelled";
  } else if (budgetReason !== null) {
    // 预算是宿主的硬停止条件，即使模型申请完整也不能越过。
    stopReason = budgetReason;
  } else if (input.requestedStopReason === "complete") {
    stopReason = complete ? "complete" : null;
  } else if (input.requestedStopReason === "budget_exhausted") {
    stopReason = budgetReason;
  } else if (input.requestedStopReason === "no_progress") {
    stopReason = noProgress ? "no_progress" : null;
  } else if (input.requestedStopReason === undefined) {
    stopReason = complete ? "complete" : budgetReason ?? (noProgress ? "no_progress" : null);
  }

  return {
    shouldStop: stopReason !== null,
    status: stopReason === null ? null
      : stopReason === "complete" ? "completed"
        : stopReason === "cancelled" ? "cancelled" : "partial",
    stopReason,
    requestedStopAllowed: (input.requestedStopReason === "complete" && stopReason === "complete")
      || (input.requestedStopReason === "budget_exhausted" && budgetReason !== null && stopReason === budgetReason)
      || (input.requestedStopReason === "no_progress" && stopReason === "no_progress"),
  };
}
