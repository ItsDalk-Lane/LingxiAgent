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

/** 身份、权限和存储错误仍整批回滚；可纠正的单条引文错误明确拒收，其余真实证据保留。 */
export function createKnowledgeResearchUpdateTool(deps: KnowledgeResearchToolDeps) {
  return {
    name: "knowledge_research_update",
    label: "Knowledge Research Update",
    description: "更新本轮研究的证据需求、原文凭据和未解决缺口。需求状态由宿主核验计算；完整性要求只能提高。"
      + "读到原文后立即登记相关引文。引文逐条核验，返回已接受和被拒条目；只纠正被拒项，不必重复提交已接受项。"
      + "quote 必须逐字位于同一 receiptId 对应的 text 中；跨段落分开登记。本批材料无关时记录真实缺口，没有新增缺口可提交空更新。",
    parameters: Type.Object({
      runId: Type.String(),
      createNeeds: Type.Optional(Type.Array(Type.Object({
        claim: Type.String(),
        kind: Type.Union([Type.Literal("fact"), Type.Literal("comparison"), Type.Literal("cause"),
          Type.Literal("timeline"), Type.Literal("counterexample"), Type.Literal("completeness")]),
        required: Type.Boolean(), minIndependentSources: Type.Number({
          description: "按不同资料计数，不能超过本轮冻结范围中的来源数量。同一小说的不同章节、不同时间点仍是一个来源。",
        }),
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
          const scope = deps.research.knowledgeStore.getTurnScope({ scopeId: context.scopeId })!;
          const availableSourceCount = new Set(scope.sources.filter(source => context.allowedSourceIds === undefined
            || context.allowedSourceIds.includes(source.sourceId)).map(source => source.sourceId)).size;
          for (const [index, need] of (input.createNeeds ?? []).entries()) {
            if (need.minIndependentSources > availableSourceCount) {
              throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Requested independent sources exceed the frozen scope", {
                reason: "min_independent_sources_exceeds_scope", needIndex: index + 1,
                requestedSourceCount: need.minIndependentSources, availableSourceCount,
              });
            }
          }
          return deps.research.transaction(() => {
            const accepted: Array<{ submissionIndex: number; needId: string; receiptId: string; evidenceId: string }> = [];
            const rejected: Array<{ submissionIndex: number; needId: string; receiptId: string;
              errorCode: string; correction: string }> = [];
            for (const need of input.createNeeds ?? []) { activeSignal.throwIfAborted(); deps.research.createNeed(context.runId, need); }
            for (const [index, link] of (input.linkEvidence ?? []).entries()) {
              activeSignal.throwIfAborted();
              try {
                const linked = deps.ledger.linkEvidence({ ...link, runId: context.runId }, {
                  allowedSourceIds: context.allowedSourceIds, allowedNeedIds: context.allowedNeedIds,
                });
                accepted.push({ submissionIndex: index + 1, needId: link.needId, receiptId: link.receiptId, evidenceId: linked.evidence.id });
              } catch (error) {
                // 只识别核验器生成的三种局部引文诊断；越权、损坏、未知异常仍让整个事务回滚。
                if (!isKnowledgeError(error)) throw error;
                const reason = error.details.reason;
                let correction: string;
                if (error.code === "KNOWLEDGE_MODEL_OUTPUT_INVALID" && reason === "quote_not_in_receipt") {
                  correction = `该凭据覆盖 ${error.details.receiptTextLength} 个字符，提交的引文没有完整、逐字出现在其中。请从该凭据的 text 复制连续原文；需要完整句段时先改用覆盖它的凭据，不得自行补字。`;
                } else if (error.code === "KNOWLEDGE_MODEL_OUTPUT_INVALID" && reason === "quote_occurrence_required") {
                  correction = `该引文在凭据中出现 ${error.details.occurrenceCount} 次，请提供从 0 开始的 occurrenceIndex，明确引用哪一次。`;
                } else if (error.code === "KNOWLEDGE_INVALID_ARGUMENT" && reason === "quote_occurrence_out_of_range") {
                  correction = `该引文在凭据中出现 ${error.details.occurrenceCount} 次，occurrenceIndex 超出范围，请使用从 0 开始的有效序号。`;
                } else throw error;
                rejected.push({ submissionIndex: index + 1, needId: link.needId, receiptId: link.receiptId,
                  errorCode: error.code, correction });
              }
              activeSignal.throwIfAborted();
            }
            if (rejected.length > 0 && accepted.length === 0) {
              throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "No submitted quote passed validation", {
                reason: "evidence_quotes_rejected", rejectedEvidence: rejected,
              });
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
            const evidenceUpdate = {
              status: rejected.length > 0 ? "partially_accepted" : accepted.length > 0 ? "accepted" : "not_requested",
              submittedCount: input.linkEvidence?.length ?? 0, acceptedCount: accepted.length, rejectedCount: rejected.length,
              accepted, rejected,
            };
            return { value: toolOk(JSON.stringify({ runId: context.runId, needs, evidenceUpdate,
              remainingBudget: deps.budget.remainingBudget(context.runId),
              ...(rejected.length > 0 ? { notice: "已接受条目已经保存。请只纠正被拒项；若确实无法形成引用，用缺口更新说明限制，不能声称资料不存在。" } : {}),
              completenessPolicy: deps.research.requireRun(context.runId).completenessPolicy }), { runId: context.runId }),
            summary: { count: needs.length, status: rejected.length > 0 ? "partial" : "completed",
              ...(rejected.length > 0 ? { errorCode: "KNOWLEDGE_MODEL_OUTPUT_INVALID" } : {}) } };
          });
        });
        // 事务和工具动作均已成功提交，才发布可见计划及进度。
        if (Array.isArray(params.createNeeds) && params.createNeeds.length > 0) notifyResearchProgress(deps.onProgress, { type: "knowledge_research_plan_updated" });
        notifyResearchProgress(deps.onProgress, { type: "knowledge_research_ledger_updated", phase: "investigating" });
        return result;
      } catch (error) {
        signal?.throwIfAborted();
        if (error instanceof Error && error.name === "AbortError") throw error;
        if (isKnowledgeError(error) && error.details.reason === "min_independent_sources_exceeds_scope") {
          return toolError(`第 ${error.details.needIndex} 个待核查问题要求 ${error.details.requestedSourceCount} 个独立来源，但本轮只有 ${error.details.availableSourceCount} 个资料来源。请按实际来源数重新创建需求；同一资料的不同章节不增加来源数。`,
            { errorCode: error.code });
        }
        if (isKnowledgeError(error) && error.details.reason === "evidence_quotes_rejected") {
          return toolError(JSON.stringify({ errorCode: error.code, notice: "本次提交的引文均未通过核验，没有保存新证据。请按逐条反馈纠正，不要重复提交原错误内容。",
            rejectedEvidence: error.details.rejectedEvidence }), { errorCode: error.code });
        }
        if (isKnowledgeError(error)) return toolError(`knowledge_research_update failed: ${error.code}`, { errorCode: error.code });
        return toolError("knowledge_research_update failed: retrieval unavailable", { errorCode: "KNOWLEDGE_RETRIEVAL_UNAVAILABLE" });
      }
    },
  };
}
