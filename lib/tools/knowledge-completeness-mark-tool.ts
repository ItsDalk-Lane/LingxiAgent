import { Type } from "../pi-sdk/index.ts";
import { isKnowledgeError, KnowledgeError } from "../knowledge/errors.ts";
import type { LinkResearchEvidenceInput } from "../knowledge/research/evidence-ledger.ts";
import type { KnowledgeCompletenessExecutor } from "../knowledge/research/knowledge-completeness-executor.ts";
import { requireResearchToolContext, type KnowledgeResearchToolDeps } from "../knowledge/research/research-tool-budget.ts";
import { toolError, toolOk } from "./tool-result.ts";

type CompletenessToolDeps = KnowledgeResearchToolDeps & {
  completeness: Pick<KnowledgeCompletenessExecutor, "readAssignedShard" | "markAssignedUnits">;
};
interface MarkInput {
  checkId: string;
  results: Array<{ unitId: string; status: "relevant" | "irrelevant" | "unavailable"; receiptId?: string;
    evidence?: Array<Omit<LinkResearchEvidenceInput, "runId">> }>;
}

function invalid(): never { throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Invalid completeness mark arguments"); }
function scopeViolation(): never { throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Completeness mark requires its assigned shard"); }
function object(value: unknown, allowed: string[]): asserts value is Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)
    || ![Object.prototype, null].includes(Object.getPrototypeOf(value)) || Object.keys(value).some(key => !allowed.includes(key))) invalid();
}
function text(value: unknown, max: number): asserts value is string {
  if (typeof value !== "string" || !value.trim() || value.length > max) invalid();
}
function validate(params: unknown): MarkInput {
  object(params, ["checkId", "results"]); text(params.checkId, 128);
  if (!Array.isArray(params.results) || params.results.length === 0) invalid();
  const unitIds = new Set<string>();
  for (const result of params.results) {
    object(result, ["unitId", "status", "receiptId", "evidence"]); text(result.unitId, 128);
    if (unitIds.has(result.unitId)) invalid();
    unitIds.add(result.unitId);
    if (typeof result.status !== "string" || !["relevant", "irrelevant", "unavailable"].includes(result.status)) invalid();
    if (result.status !== "unavailable" || result.receiptId !== undefined) text(result.receiptId, 128);
    if (result.evidence !== undefined && (!Array.isArray(result.evidence) || result.status === "unavailable")) invalid();
    for (const link of (result.evidence ?? []) as unknown[]) {
      object(link, ["needId", "receiptId", "quote", "occurrenceIndex", "relation", "rationale"]);
      text(link.needId, 128); text(link.receiptId, 128); text(link.quote, 2000); text(link.rationale, 1000);
      if (typeof link.relation !== "string" || !["supports", "contradicts", "context"].includes(link.relation)) invalid();
      if (link.occurrenceIndex !== undefined && (!Number.isSafeInteger(link.occurrenceIndex) || Number(link.occurrenceIndex) < 0)) invalid();
    }
  }
  return params as unknown as MarkInput;
}

/** 工作会话只提交逐单元判断，范围、原文引用和完成计数由宿主一次性核验入账。 */
export function createKnowledgeCompletenessMarkTool(deps: CompletenessToolDeps) {
  return {
    name: "knowledge_completeness_mark",
    label: "Knowledge Completeness Mark",
    description: "提交已分配覆盖单元的相关性判断及可选原文引用；检查进度和完整性由宿主核验计算。",
    parameters: Type.Object({
      checkId: Type.String(),
      results: Type.Array(Type.Object({
        unitId: Type.String(),
        status: Type.Union([Type.Literal("relevant"), Type.Literal("irrelevant"), Type.Literal("unavailable")]),
        receiptId: Type.Optional(Type.String()),
        evidence: Type.Optional(Type.Array(Type.Object({
          needId: Type.String(), receiptId: Type.String(), quote: Type.String(), occurrenceIndex: Type.Optional(Type.Number()),
          relation: Type.Union([Type.Literal("supports"), Type.Literal("contradicts"), Type.Literal("context")]),
          rationale: Type.String(),
        }, { additionalProperties: false }))),
      }, { additionalProperties: false })),
    }, { additionalProperties: false }),
    sessionPermission: {
      // 只更新完整性台账，不修改知识原文或用户设置。
      resolveInvocation: () => ({ action: "read", kind: "read", capability: "knowledge_completeness_mark.read" }),
    },
    execute: async (_toolCallId: string, params: Record<string, unknown> = {}, signal?: AbortSignal, _onUpdate?: unknown, ctx?: unknown) => {
      try {
        signal?.throwIfAborted();
        const resolved = deps.resolveContext(ctx);
        const context = requireResearchToolContext({ ...deps, resolveContext: () => resolved }, ctx, resolved?.runId);
        if (context.role !== "worker" || !context.completenessCheckId || !context.completenessShardId
          || context.completenessCheckId !== params?.checkId) scopeViolation();
        return await deps.budget.execute({ context, toolName: "knowledge_completeness_mark", requestSummary: {}, signal }, activeSignal => {
          const input = validate(params);
          activeSignal.throwIfAborted();
          const summary = deps.completeness.markAssignedUnits(context, input);
          activeSignal.throwIfAborted();
          return { value: toolOk(JSON.stringify(summary), { checkId: summary.checkId }),
            summary: { count: input.results.length, status: summary.status } };
        });
      } catch (error) {
        signal?.throwIfAborted();
        if (error instanceof Error && error.name === "AbortError") throw error;
        if (isKnowledgeError(error)) return toolError(`knowledge_completeness_mark failed: ${error.code}`, { errorCode: error.code });
        return toolError("knowledge_completeness_mark failed: retrieval unavailable", { errorCode: "KNOWLEDGE_RETRIEVAL_UNAVAILABLE" });
      }
    },
  };
}
