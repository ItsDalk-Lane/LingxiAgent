import { Type } from "../pi-sdk/index.ts";
import { isKnowledgeError, KnowledgeError } from "../knowledge/errors.ts";
import { notifyResearchProgress, requireResearchToolContext, type KnowledgeResearchToolDeps } from "../knowledge/research/research-tool-budget.ts";
import type { LinkResearchEvidenceInput } from "../knowledge/research/evidence-ledger.ts";
import type { ResearchStore } from "../knowledge/research/research-store.ts";
import { toolError, toolOk } from "./tool-result.ts";

type NeedInput = Parameters<ResearchStore["createNeed"]>[1];
interface UpdateInput {
  runId: string;
  createNeeds?: NeedInput[];
  linkEvidence?: Omit<LinkResearchEvidenceInput, "runId">[];
  unresolvedGaps?: Array<{ needId: string; gaps: string[] }>;
  requestCompletenessPolicy?: "source_diverse" | "relevant_sections_complete" | "scope_complete";
}

function invalid(): never { throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Invalid research update arguments"); }
function scopeViolation(): never { throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Research update exceeds the worker assignment"); }
function object(value: unknown, allowed: string[]): asserts value is Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)
    || ![Object.prototype, null].includes(Object.getPrototypeOf(value)) || Object.keys(value).some(key => !allowed.includes(key))) invalid();
}
function text(value: unknown, max: number): asserts value is string {
  if (typeof value !== "string" || !value.trim() || value.length > max) invalid();
}
function validate(params: unknown): UpdateInput {
  object(params, ["runId", "createNeeds", "linkEvidence", "unresolvedGaps", "requestCompletenessPolicy"]);
  text(params.runId, 128);
  for (const key of ["createNeeds", "linkEvidence", "unresolvedGaps"]) {
    if (params[key] !== undefined && !Array.isArray(params[key])) invalid();
  }
  for (const need of (params.createNeeds ?? []) as unknown[]) {
    object(need, ["claim", "kind", "required", "minIndependentSources", "requireCounterEvidence", "requireAllRelevantUnits"]);
    text(need.claim, 1000);
    if (typeof need.kind !== "string" || !["fact", "comparison", "cause", "timeline", "counterexample", "completeness"].includes(need.kind)
      || !Number.isSafeInteger(need.minIndependentSources) || Number(need.minIndependentSources) < 1) invalid();
    for (const key of ["required", "requireCounterEvidence", "requireAllRelevantUnits"]) if (typeof need[key] !== "boolean") invalid();
  }
  for (const link of (params.linkEvidence ?? []) as unknown[]) {
    object(link, ["needId", "receiptId", "quote", "occurrenceIndex", "relation", "rationale"]);
    text(link.needId, 128); text(link.receiptId, 128); text(link.quote, 2000); text(link.rationale, 1000);
    if (typeof link.relation !== "string" || !["supports", "contradicts", "context"].includes(link.relation)) invalid();
    if (link.occurrenceIndex !== undefined && (!Number.isSafeInteger(link.occurrenceIndex) || Number(link.occurrenceIndex) < 0)) invalid();
  }
  for (const gap of (params.unresolvedGaps ?? []) as unknown[]) {
    object(gap, ["needId", "gaps"]); text(gap.needId, 128);
    if (!Array.isArray(gap.gaps) || gap.gaps.length > 8) invalid();
    for (const item of gap.gaps) text(item, 500);
  }
  if (params.requestCompletenessPolicy !== undefined
    && (typeof params.requestCompletenessPolicy !== "string"
      || !["source_diverse", "relevant_sections_complete", "scope_complete"].includes(params.requestCompletenessPolicy))) invalid();
  return params as unknown as UpdateInput;
}

/** 模型只提出待核查的需求、凭据和缺口；整批更新由宿主取证后原子落库。 */
export function createKnowledgeResearchUpdateTool(deps: KnowledgeResearchToolDeps) {
  return {
    name: "knowledge_research_update",
    label: "Knowledge Research Update",
    description: "更新本轮研究的证据需求、原文凭据和未解决缺口。需求状态由宿主核验计算；完整性要求只能提高。",
    parameters: Type.Object({
      runId: Type.String(),
      createNeeds: Type.Optional(Type.Array(Type.Object({
        claim: Type.String(),
        kind: Type.Union([Type.Literal("fact"), Type.Literal("comparison"), Type.Literal("cause"),
          Type.Literal("timeline"), Type.Literal("counterexample"), Type.Literal("completeness")]),
        required: Type.Boolean(), minIndependentSources: Type.Number(),
        requireCounterEvidence: Type.Boolean(), requireAllRelevantUnits: Type.Boolean(),
      }, { additionalProperties: false }))),
      linkEvidence: Type.Optional(Type.Array(Type.Object({
        needId: Type.String(), receiptId: Type.String(), quote: Type.String(), occurrenceIndex: Type.Optional(Type.Number()),
        relation: Type.Union([Type.Literal("supports"), Type.Literal("contradicts"), Type.Literal("context")]),
        rationale: Type.String(),
      }, { additionalProperties: false }))),
      unresolvedGaps: Type.Optional(Type.Array(Type.Object({ needId: Type.String(), gaps: Type.Array(Type.String()) }, { additionalProperties: false }))),
      requestCompletenessPolicy: Type.Optional(Type.Union([Type.Literal("source_diverse"), Type.Literal("relevant_sections_complete"), Type.Literal("scope_complete")])),
    }, { additionalProperties: false }),
    sessionPermission: {
      // 只更新研究台账，不修改知识原文或用户设置。
      resolveInvocation: () => ({ action: "read", kind: "read", capability: "knowledge_research_update.read" }),
    },
    execute: async (_toolCallId: string, params: Record<string, unknown> = {}, signal?: AbortSignal, _onUpdate?: unknown, ctx?: unknown) => {
      try {
        signal?.throwIfAborted();
        const context = requireResearchToolContext(deps, ctx, params.runId);
        // 请求摘要只列已经存在的需求编号；未知编号的拒绝由事务内部记录，不把任何正文放进动作台账。
        const knownIds = new Set(deps.research.listNeeds(context.runId).map(need => need.id));
        const requested = [params.linkEvidence, params.unresolvedGaps].flatMap(items => Array.isArray(items) ? items : []);
        const needIds = [...new Set(requested.map(item => item && typeof item === "object" ? item.needId : undefined)
          .filter((id): id is string => typeof id === "string" && knownIds.has(id)))];
        const result = await deps.budget.execute({ context, toolName: "knowledge_research_update", requestSummary: { needIds }, signal }, activeSignal => {
          activeSignal.throwIfAborted();
          const input = validate(params);
          if (context.role === "worker") {
            if (!Array.isArray(context.allowedNeedIds) || input.createNeeds?.length || input.requestCompletenessPolicy !== undefined) scopeViolation();
            for (const target of [...input.linkEvidence ?? [], ...input.unresolvedGaps ?? []]) {
              if (!context.allowedNeedIds.includes(target.needId)) scopeViolation();
            }
          }
          return deps.research.transaction(() => {
            for (const need of input.createNeeds ?? []) { activeSignal.throwIfAborted(); deps.research.createNeed(context.runId, need); }
            for (const link of input.linkEvidence ?? []) {
              activeSignal.throwIfAborted();
              deps.ledger.linkEvidence({ ...link, runId: context.runId }, {
                allowedSourceIds: context.allowedSourceIds, allowedNeedIds: context.allowedNeedIds,
              });
              activeSignal.throwIfAborted();
            }
            for (const gap of input.unresolvedGaps ?? []) {
              activeSignal.throwIfAborted();
              const need = deps.research.getNeed(context.runId, gap.needId);
              deps.research.setNeedState(context.runId, need.id, { status: need.status, unresolvedGaps: gap.gaps });
            }
            if (input.requestCompletenessPolicy !== undefined) deps.research.upgradeCompletenessPolicy(context.runId, input.requestCompletenessPolicy);
            const needs = deps.research.listNeeds(context.runId)
              .filter(need => context.role === "root" || context.allowedNeedIds!.includes(need.id))
              .map(need => deps.ledger.recomputeNeed(context.runId, need.id));
            activeSignal.throwIfAborted();
            return { value: toolOk(JSON.stringify({ runId: context.runId, needs,
              completenessPolicy: deps.research.requireRun(context.runId).completenessPolicy }), { runId: context.runId }),
            summary: { count: needs.length, status: "completed" } };
          });
        });
        // 事务和工具动作均已成功提交，才发布可见计划及进度。
        if (Array.isArray(params.createNeeds) && params.createNeeds.length > 0) notifyResearchProgress(deps.onProgress, { type: "knowledge_research_plan_updated" });
        notifyResearchProgress(deps.onProgress, { type: "knowledge_research_ledger_updated", phase: "investigating" });
        return result;
      } catch (error) {
        signal?.throwIfAborted();
        if (error instanceof Error && error.name === "AbortError") throw error;
        if (isKnowledgeError(error)) return toolError(`knowledge_research_update failed: ${error.code}`, { errorCode: error.code });
        return toolError("knowledge_research_update failed: retrieval unavailable", { errorCode: "KNOWLEDGE_RETRIEVAL_UNAVAILABLE" });
      }
    },
  };
}
