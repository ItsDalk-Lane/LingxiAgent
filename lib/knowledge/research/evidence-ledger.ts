import crypto from "node:crypto";
import type { KnowledgeEvidenceRelation } from "../../../shared/knowledge-research.ts";
import { EvidenceReceiptService } from "../evidence-receipt-service.ts";
import { KnowledgeError } from "../errors.ts";
import type { KnowledgeEvidenceItem, KnowledgeEvidenceNeedRecord } from "../types.ts";
import { ResearchStore } from "./research-store.ts";

export interface EvaluatedEvidenceNeed extends KnowledgeEvidenceNeedRecord {
  evidenceIds: string[];
  counterEvidenceIds: string[];
  independentSourceCount: number;
  counterEvidenceChecked: boolean;
  completenessSatisfied: boolean;
}

export interface LinkResearchEvidenceInput {
  runId: string;
  needId: string;
  receiptId: string;
  quote: string;
  /** 从零开始，按原文中出现的位置计数，包含互相重叠的匹配。 */
  occurrenceIndex?: number;
  relation: KnowledgeEvidenceRelation;
  rationale: string;
}

function requiredText(value: unknown, limit: number, field: string): asserts value is string {
  if (typeof value !== "string" || !value.trim() || value.length > limit) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} is empty or exceeds its limit`);
  }
}

interface EvidenceLedgerOptions {
  receipts?: EvidenceReceiptService;
  /** 由完整性执行器提供已核验结果；尚未接入时不视为完成。 */
  isCompletenessSatisfied?: (runId: string, needId: string) => boolean;
}

/** 只接收阅读凭据；模型不能直接提交证据位置、来源数量或最终需求状态。 */
export class EvidenceLedger {
  private readonly receipts: EvidenceReceiptService;
  private readonly store: ResearchStore;
  private readonly options: EvidenceLedgerOptions;

  constructor(
    store: ResearchStore,
    options: EvidenceLedgerOptions = {},
  ) {
    this.store = store;
    this.options = options;
    this.receipts = options.receipts ?? new EvidenceReceiptService(store);
  }

  linkEvidence(input: LinkResearchEvidenceInput, context: {
    allowedSourceIds?: string[];
    allowedNeedIds?: string[];
  } = {}): { evidence: KnowledgeEvidenceItem; need: EvaluatedEvidenceNeed } {
    requiredText(input.quote, 2000, "quote");
    requiredText(input.rationale, 1000, "rationale");
    if (!["supports", "contradicts", "context"].includes(input.relation)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Invalid evidence relation");
    }
    if (context.allowedNeedIds && !context.allowedNeedIds.includes(input.needId)) {
      throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Need is outside the worker assignment");
    }
    // 凭据校验、原文取证、关联、消费和状态重算共用同一事务，任一步失败全部回滚。
    return this.store.transaction(() => {
      this.store.getNeed(input.runId, input.needId);
      const { receipt, block, text } = this.receipts.read({
        runId: input.runId, receiptId: input.receiptId, allowedSourceIds: context.allowedSourceIds,
      });
      const occurrences: number[] = [];
      for (let offset = text.indexOf(input.quote); offset >= 0; offset = text.indexOf(input.quote, offset + 1)) {
        occurrences.push(offset);
      }
      if (occurrences.length === 0) {
        throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Quote does not occur in the receipt text");
      }
      if (input.occurrenceIndex == null && occurrences.length > 1) {
        throw new KnowledgeError("KNOWLEDGE_MODEL_OUTPUT_INVALID", "Repeated quote requires occurrenceIndex");
      }
      const occurrence = input.occurrenceIndex ?? 0;
      if (!Number.isSafeInteger(occurrence) || occurrence < 0 || occurrence >= occurrences.length) {
        throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "occurrenceIndex is outside the receipt text");
      }
      const startOffset = receipt.startOffset + occurrences[occurrence];
      const heading = block.locator.headingPath;
      const page = block.locator.pageNumber;
      const evidence = this.store.putEvidence({
        id: this.store.newId("kei"), runId: input.runId,
        sourceId: receipt.sourceId, contentSnapshotId: receipt.contentSnapshotId,
        parseArtifactId: receipt.parseArtifactId, chunkIndexVariantId: receipt.chunkIndexVariantId,
        chunkId: receipt.chunkId, blockId: receipt.blockId,
        startOffset, endOffset: startOffset + input.quote.length,
        canonicalText: input.quote,
        canonicalTextSha256: crypto.createHash("sha256").update(input.quote).digest("hex"),
        headingPath: Array.isArray(heading) && heading.every(value => typeof value === "string") ? heading : null,
        pageNumber: typeof page === "number" && Number.isInteger(page) && page > 0 ? page : null,
        createdAt: this.store.now(),
      });
      this.store.linkEvidence({
        needId: input.needId, evidenceId: evidence.id, relation: input.relation,
        rationale: input.rationale, sourceIndependenceKey: receipt.sourceId, createdAt: this.store.now(),
      });
      this.store.consumeReceipt(input.runId, input.receiptId);
      return { evidence, need: this.recomputeNeed(input.runId, input.needId) };
    });
  }

  /** 不采用数据库或模型声称的状态，每次都从有效关联和宿主动作重新计算。 */
  evaluateNeed(runId: string, needId: string, visited = new Set<string>()): EvaluatedEvidenceNeed {
    const need = this.store.getNeed(runId, needId);
    const relations = this.store.listRelations(runId, needId);
    const evidenceById = new Map(this.store.listEvidence(runId).map(item => [item.id, item]));
    const supports = relations.filter(item => item.relation === "supports");
    const contradicts = relations.filter(item => item.relation === "contradicts");
    const independentSourceCount = new Set(supports.map(item => evidenceById.get(item.evidenceId)!.sourceId)).size;
    const actions = this.store.listActions(runId).filter(action => action.status === "completed"
      && action.completedAt !== null && action.errorCode === null && action.responseSummary?.errorCode == null);
    const acceptedNotApplicable = actions.some(action => action.actionType === "host_not_applicable"
      && action.requestSummary.needIds instanceof Array && action.requestSummary.needIds[0] === needId
      && action.responseSummary?.status === "accepted");
    const searchedCounterexamples = actions.some(action => action.actionType === "knowledge_search"
      && (action.responseSummary?.status === undefined || action.responseSummary.status === "completed")
      && action.requestSummary.purpose === "counterexample"
      && typeof action.requestSummary.query === "string" && action.requestSummary.query.trim().length > 0
      && Array.isArray(action.requestSummary.needIds) && action.requestSummary.needIds.includes(needId)
      && action.responseSummary?.count === 0
      && Array.isArray(action.responseSummary.hitIds) && action.responseSummary.hitIds.length === 0);
    const completenessSatisfied = this.options.isCompletenessSatisfied?.(runId, needId) === true;
    const nextVisited = new Set(visited).add(needId);
    const conflictResolved = contradicts.length > 0 && !visited.has(needId) && actions.some(action => {
      if (action.actionType !== "host_resolve_conflict" || action.responseSummary?.status !== "accepted"
        || !Array.isArray(action.requestSummary.needIds) || action.requestSummary.needIds[0] !== needId) return false;
      const resolutionId = action.requestSummary.needIds[1];
      if (typeof resolutionId !== "string" || nextVisited.has(resolutionId)) return false;
      const resolution = this.store.listNeeds(runId).find(item => item.id === resolutionId);
      if (!resolution || resolution.ordinal <= need.ordinal) return false;
      const handled = this.store.listRelations(runId, resolutionId);
      return contradicts.every(counter => handled.some(item => item.evidenceId === counter.evidenceId))
        && this.evaluateNeed(runId, resolutionId, nextVisited).status === "supported";
    });
    const counterEvidenceChecked = searchedCounterexamples || conflictResolved || completenessSatisfied;
    const status = acceptedNotApplicable ? "not_applicable"
      : supports.length > 0 && contradicts.length > 0 && !conflictResolved ? "conflicted"
        : supports.length === 0 ? "uncovered"
          : independentSourceCount < need.minIndependentSources
            || (need.requireCounterEvidence && !counterEvidenceChecked)
            || (need.requireAllRelevantUnits && !completenessSatisfied) ? "partial" : "supported";
    return {
      ...need, status, independentSourceCount, counterEvidenceChecked, completenessSatisfied,
      evidenceIds: [...new Set(supports.map(item => item.evidenceId))],
      counterEvidenceIds: [...new Set(contradicts.map(item => item.evidenceId))],
    };
  }

  recomputeNeed(runId: string, needId: string): EvaluatedEvidenceNeed {
    const evaluated = this.evaluateNeed(runId, needId);
    this.store.setNeedState(runId, needId, { status: evaluated.status, unresolvedGaps: evaluated.unresolvedGaps });
    return evaluated;
  }

  recompute(runId: string): EvaluatedEvidenceNeed[] {
    return this.store.transaction(() => this.store.listNeeds(runId).map(need => this.recomputeNeed(runId, need.id)));
  }

  /** 仅宿主核定的不适用结论可以进入此入口，研究工具不直接暴露它。 */
  acceptNotApplicable(runId: string, needId: string, rationale: string): EvaluatedEvidenceNeed {
    requiredText(rationale, 1000, "rationale");
    return this.store.transaction(() => {
      this.store.getNeed(runId, needId);
      this.recordHostDecision(runId, "host_not_applicable", [needId]);
      return this.recomputeNeed(runId, needId);
    });
  }

  /** 解释需求必须新建、已获支持并关联所有反证，随后由宿主明确接受。 */
  acceptConflictResolution(runId: string, needId: string, resolutionNeedId: string): EvaluatedEvidenceNeed {
    return this.store.transaction(() => {
      const need = this.evaluateNeed(runId, needId);
      const resolution = this.evaluateNeed(runId, resolutionNeedId);
      const handled = this.store.listRelations(runId, resolutionNeedId);
      if (resolution.ordinal <= need.ordinal || resolution.status !== "supported"
        || need.counterEvidenceIds.length === 0
        || !need.counterEvidenceIds.every(id => handled.some(item => item.evidenceId === id))) {
        throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Conflict needs a supported new explanation with counterevidence");
      }
      this.recordHostDecision(runId, "host_resolve_conflict", [needId, resolutionNeedId]);
      return this.recomputeNeed(runId, needId);
    });
  }

  private recordHostDecision(runId: string, actionType: string, needIds: string[]): void {
    const actions = this.store.listActions(runId);
    this.store.insertAction({
      id: this.store.newId("kra"), runId, roundId: null,
      ordinal: Math.max(-1, ...actions.map(action => action.ordinal)) + 1,
      actorSessionId: null, actorAgentId: null, actionType, requestSummary: { needIds },
      responseSummary: { status: "accepted" }, status: "completed", startedAt: this.store.now(),
      completedAt: this.store.now(), errorCode: null,
    });
  }
}
