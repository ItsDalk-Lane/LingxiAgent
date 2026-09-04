import { Type } from "../pi-sdk/index.ts";
import { isKnowledgeError, KnowledgeError } from "../knowledge/errors.ts";
import {
  evaluateResearchStopPolicy,
  type KnowledgeResearchRequestedStopReason,
  type KnowledgeResearchStopPolicyResult,
} from "../knowledge/research/research-stop-policy.ts";
import {
  requireResearchToolContext,
  type KnowledgeResearchToolDeps,
} from "../knowledge/research/research-tool-budget.ts";
import { toolError, toolOk } from "./tool-result.ts";

export interface KnowledgeResearchFinishDecision {
  runId: string;
  accepted: boolean;
  requestedStopReason: KnowledgeResearchRequestedStopReason;
  stopReason: KnowledgeResearchStopPolicyResult["stopReason"];
  status: KnowledgeResearchStopPolicyResult["status"];
}

export interface KnowledgeResearchFinishToolDeps extends KnowledgeResearchToolDeps {
  /** 完整性证明只能由宿主提供，模型的结论摘要不作为证明。 */
  isCompletenessSatisfied?: (runId: string) => boolean;
  /** 只传递已获准的结构化结果，最终材料合成和运行收口由控制器完成。 */
  onFinishAccepted?: (decision: KnowledgeResearchFinishDecision,
    context: ReturnType<typeof requireResearchToolContext>) => void;
}

/** 结束申请不是最终回答；需求、预算与停止资格都由宿主重新核验。 */
export function createKnowledgeResearchFinishTool(deps: KnowledgeResearchFinishToolDeps) {
  return {
    name: "knowledge_research_finish",
    label: "Knowledge Research Finish",
    description: "申请结束当前研究。必须由宿主确认需求、证据和实际预算符合停止条件；"
      + "未获准时继续研究。结论摘要不作为最终答案，工作会话不能调用。",
    parameters: Type.Object({
      runId: Type.String(),
      conclusionSummary: Type.String(),
      requestedStopReason: Type.Union([
        Type.Literal("complete"), Type.Literal("budget_exhausted"), Type.Literal("no_progress"),
      ]),
    }),
    sessionPermission: {
      resolveInvocation: () => ({ action: "read", kind: "read", capability: "knowledge_research_finish.read" }),
    },
    execute: async (_toolCallId: string, params: Record<string, unknown> = {}, signal?: AbortSignal,
      _onUpdate?: unknown, ctx?: unknown) => {
      try {
        signal?.throwIfAborted();
        if (!params || typeof params !== "object" || Array.isArray(params)
          || typeof params.runId !== "string" || !params.runId.trim()) {
          throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Invalid research finish parameters");
        }
        const runId = params.runId;
        const context = requireResearchToolContext(deps, ctx, runId);
        if (context.role !== "root") {
          throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Only the research root can request finishing");
        }
        const decision = await deps.budget.execute({
          context, toolName: "knowledge_research_finish", requestSummary: {}, signal,
        }, () => {
          if (Object.keys(params).some(key => !["runId", "conclusionSummary", "requestedStopReason"].includes(key))
            || typeof params.conclusionSummary !== "string"
            || typeof params.requestedStopReason !== "string"
            || !["complete", "budget_exhausted", "no_progress"].includes(params.requestedStopReason)) {
            throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Invalid research finish parameters");
          }
          const requestedStopReason = params.requestedStopReason as KnowledgeResearchRequestedStopReason;
          const needs = deps.ledger.recompute(runId);
          const run = deps.research.requireRun(runId);
          const rounds = deps.research.listRounds(runId).filter(round => round.status === "completed");
          const result = evaluateResearchStopPolicy({
            needs, run, elapsedMs: deps.budget.elapsedMs(runId),
            recentRoundEvidenceCounts: rounds.map(round => round.newEvidenceCount),
            completenessSatisfied: deps.isCompletenessSatisfied?.(runId) === true,
            requestedStopReason,
          });
          const value: KnowledgeResearchFinishDecision = {
            runId, accepted: result.requestedStopAllowed, requestedStopReason,
            stopReason: result.stopReason, status: result.status,
          };
          // 只记宿主裁定与需求数量，结论原文既不落库也不传给最终回答。
          return { value, summary: { count: needs.length, status: value.accepted ? "accepted" : "rejected" } };
        });
        if (decision.accepted) deps.onFinishAccepted?.(decision, context);
        return toolOk(JSON.stringify(decision), decision);
      } catch (error) {
        if (signal?.aborted) throw error;
        if (isKnowledgeError(error)) return toolError(`knowledge_research_finish failed: ${error.code}: ${error.message}`, {
          errorCode: error.code,
        });
        return toolError("knowledge_research_finish failed: research validation unavailable", {
          errorCode: "KNOWLEDGE_RETRIEVAL_UNAVAILABLE",
        });
      }
    },
  };
}
