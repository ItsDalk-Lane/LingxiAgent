import crypto from "node:crypto";
import { KnowledgeError } from "./errors.ts";
import type { ResearchStore } from "./research/research-store.ts";
import type { KnowledgeBlock, KnowledgeResearchReadReceipt } from "./types.ts";

/** 只由执行工具的宿主提供，模型不能通过工具参数指定研究身份或扩大来源范围。 */
export interface KnowledgeResearchToolContext {
  runId: string;
  actorSessionId: string | null;
  allowedSourceIds?: string[];
}

export interface IssueKnowledgeReadReceipt extends KnowledgeResearchToolContext {
  sourceId: string;
  contentSnapshotId: string;
  parseArtifactId: string;
  chunkIndexVariantId?: string | null;
  chunkId?: string | null;
  blockId: string;
  startOffset: number;
  endOffset: number;
  channel: "knowledge_read" | "knowledge_grep";
}

function digest(text: string): string {
  return crypto.createHash("sha256").update(text, "utf8").digest("hex");
}

function violation(): never {
  throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Research receipt is outside the active frozen scope");
}

/** 凭据只记录真正读过的原文位置；写入与读回都复核冻结身份链和原文摘要。 */
export class EvidenceReceiptService {
  private readonly researchStore: ResearchStore;
  constructor(researchStore: ResearchStore) { this.researchStore = researchStore; }

  issue(input: IssueKnowledgeReadReceipt): KnowledgeResearchReadReceipt {
    return this.issueWithText(input).receipt;
  }

  /** 工具直接交付本次事务刚核验的原文，不再为立即读回凭据重复查询同一段。 */
  issueWithText(input: IssueKnowledgeReadReceipt): {
    receipt: KnowledgeResearchReadReceipt; block: KnowledgeBlock; text: string;
  } {
    return this.researchStore.transaction(() => {
      if (!["knowledge_read", "knowledge_grep"].includes(input.channel)) {
        throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Only raw knowledge reads can issue receipts");
      }
      const { block, text } = this.resolveFrozenText(input);
      const receipt = this.researchStore.insertReceipt({
        id: this.researchStore.newId("krr"),
        runId: input.runId,
        actorSessionId: input.actorSessionId,
        sourceId: input.sourceId,
        contentSnapshotId: input.contentSnapshotId,
        parseArtifactId: input.parseArtifactId,
        chunkIndexVariantId: input.chunkIndexVariantId ?? null,
        chunkId: input.chunkId ?? null,
        blockId: input.blockId,
        startOffset: input.startOffset,
        endOffset: input.endOffset,
        canonicalTextSha256: digest(text),
        channel: input.channel,
        createdAt: this.researchStore.now(),
        consumedAt: null,
      });
      return { receipt, block, text };
    });
  }

  read(input: {
    runId: string;
    receiptId: string;
    allowedSourceIds?: string[];
    actorSessionId?: string | null;
  }): { receipt: KnowledgeResearchReadReceipt; block: KnowledgeBlock; text: string } {
    return this.researchStore.transaction(() => {
      const receipt = this.researchStore.getReceipt(input.runId, input.receiptId);
      if (receipt.runId !== input.runId
        || (input.actorSessionId !== undefined && receipt.actorSessionId !== input.actorSessionId)) violation();
      const { block, text } = this.resolveFrozenText({ ...receipt, allowedSourceIds: input.allowedSourceIds });
      if (digest(text) !== receipt.canonicalTextSha256) {
        throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Research receipt text hash no longer matches");
      }
      return { receipt, block, text };
    });
  }

  private resolveFrozenText(input: IssueKnowledgeReadReceipt): { block: KnowledgeBlock; text: string } {
    const run = this.researchStore.requireRun(input.runId);
    const store = this.researchStore.knowledgeStore;
    const scope = store.getTurnScope({ scopeId: run.turnScopeId });
    if (!scope || scope.status !== "active" || !["planning", "running", "synthesizing"].includes(run.status)
      || scope.turnId !== run.turnId || scope.sessionPath !== run.parentSessionPath) violation();
    const frozen = scope.sources.find(source => source.sourceId === input.sourceId);
    if (!frozen || frozen.contentSnapshotId !== input.contentSnapshotId || frozen.parseArtifactId !== input.parseArtifactId) violation();
    if (input.allowedSourceIds !== undefined && (!Array.isArray(input.allowedSourceIds)
      || input.allowedSourceIds.some(sourceId => !scope.sources.some(source => source.sourceId === sourceId))
      || !input.allowedSourceIds.includes(input.sourceId))) violation();
    const source = store.getSource({ studioId: scope.studioId, sourceId: input.sourceId });
    const snapshot = store.getContentSnapshot({ studioId: scope.studioId, snapshotId: input.contentSnapshotId });
    const artifact = store.getParseArtifact({ studioId: scope.studioId, parseArtifactId: input.parseArtifactId });
    if (snapshot.sourceId !== source.id || artifact.contentSnapshotId !== snapshot.id) violation();
    if (artifact.status !== "ready") {
      throw new KnowledgeError("KNOWLEDGE_PARSE_NOT_READY", "Research receipt requires a ready frozen parse artifact");
    }
    const block = store.getArtifactBlocksByIds({
      studioId: scope.studioId, parseArtifactId: artifact.id, blockIds: [input.blockId],
    })[0];
    if (!block || block.id !== input.blockId || block.parseArtifactId !== artifact.id) violation();
    if (!Number.isSafeInteger(input.startOffset) || !Number.isSafeInteger(input.endOffset)
      || input.startOffset < 0 || input.endOffset <= input.startOffset || input.endOffset > block.text.length) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Research receipt offsets are outside the frozen block");
    }
    if (digest(block.text) !== block.textSha256) {
      throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Frozen research block text hash no longer matches");
    }
    return { block, text: block.text.slice(input.startOffset, input.endOffset) };
  }
}
