/**
 * knowledge_read 工具 —— 读知识库笔记本源的分片（Phase 8 分片子 Agent 链路，
 * Phase 4 KnowledgeTurnScope 权限天花板）。
 *
 * 消费方是子 Agent：[KnowledgeContext] 注入块超预算时只带分片清单，主模型用
 * `subagent` 工具并行派子 Agent，各用本工具按 ordinal 范围读一片（或按 query
 * 检索该源）再汇总。工具直连 engine 级 KnowledgeManager，跨会话可用。
 *
 * 权限边界（任务书 §二十~§二十二）：
 * - 只读；studio 隔离（所有 store 查询都带 studioId）；
 * - scopeId 必填且服务端逐次复核：scope 存在、active、属于当前会话（subagent
 *   子会话经 manifest provenance 继承父会话 scope——scope 只能缩小）；
 * - sourceId/notebookId 必须在 scope 冻结集合内，不信任模型传入的任何 id；
 * - 读取锚定 scope 冻结的 snapshot/artifact（§四十三：watcher 轮内产生的新
 *   版本下一轮才生效）。任何一项失败 → KNOWLEDGE_SCOPE_VIOLATION / 显式错误，
 *   不回落到旧的全 studio 扫描行为。
 */
import { Type } from "../pi-sdk/index.ts";
import { resolveKnowledgeChunkerConfig } from "../knowledge/chunker.ts";
import { isKnowledgeError, KnowledgeError } from "../knowledge/errors.ts";
import { EvidenceReceiptService, type KnowledgeResearchToolContext } from "../knowledge/evidence-receipt-service.ts";
import { ResearchStore } from "../knowledge/research/research-store.ts";
import type { StoredKnowledgeChunk } from "../knowledge/knowledge-index-store.ts";
import type { KnowledgeManager } from "../knowledge/knowledge-manager.ts";
import type { KnowledgeTurnScope } from "../knowledge/types.ts";
import {
  knowledgeScopeViolation,
  requireKnowledgeScopeSource,
  requireKnowledgeSessionContext,
  resolveKnowledgeOwningNotebookId,
  resolveKnowledgeTurnScope,
} from "./knowledge-scope.ts";
import { toolError, toolOk } from "./tool-result.ts";

/** 单次读片的防护上限：防止一次调用把整个大源灌进子 Agent 上下文。 */
const MAX_CHUNKS_PER_READ = 40;

export interface KnowledgeReadToolDeps {
  /** engine 级 KnowledgeManager（跨会话）；null = Knowledge 不可用。 */
  getKnowledge: () => KnowledgeManager | null;
  /** 当前 runtime studioId；null = 运行时上下文不可用。 */
  getStudioId: () => string | null;
  /**
   * 工具执行会话的 scope 归属上下文（Pi SDK execute 第 5 参 ctx 解析）：
   * sessionPath = 当前执行会话的 JSONL 路径；parentSessionPath = subagent
   * 子会话的父会话路径（主会话为 null）。缺失 → 无 scope 上下文的 surface
   * （显式 KNOWLEDGE_MODEL_UNAVAILABLE，不静默放行）。
   */
  resolveSessionContext?: (ctx: unknown) => {
    sessionPath: string | null;
    parentSessionPath: string | null;
  };
  /** 可选研究上下文由宿主提供；普通工具调用不创建研究凭据。 */
  resolveResearchContext?: (ctx: unknown) => KnowledgeResearchToolContext | null;
}

function requireNonEmptyString(value: unknown, label: string): string {
  if (typeof value !== "string" || !value.trim()) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${label} is required`);
  }
  return value.trim();
}

function optionalOrdinal(value: unknown, label: string): number | null {
  if (value == null) return null;
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${label} must be a non-negative integer`);
  }
  return value;
}

/**
 * scope 校验 + 冻结 artifact 解析（§二十二，服务端复核，不信任模型传入的 id；
 * 校验链本体在 lib/tools/knowledge-scope.ts，Phase 11 起多工具共享）：
 * 1. scopeId 存在、active、属于当前 studio 与当前会话（或其 subagent 父会话）；
 * 2. sourceId 在 scope 冻结集合内；notebookId 给出时必须同时属于 scope 选中
 *    集合与该源的冻结引用集合；缺失时 owning notebook 取冻结集合内第一个
 *    引用笔记本（限选中集合，不再扫全 studio）；
 * 3. 读取锚定冻结的 parseArtifactId（非 ready → KNOWLEDGE_PARSE_NOT_READY）。
 */
function resolveScopedArtifact(
  knowledge: KnowledgeManager,
  studioId: string,
  scopeId: string,
  sourceId: string,
  notebookId: string | null,
  sessionContext: { sessionPath: string | null; parentSessionPath: string | null },
): {
  artifactId: string;
  contentSnapshotId: string;
  notebookId: string;
  sourceName: string;
  scope: KnowledgeTurnScope;
  chunkTargetChars: number;
} {
  // 无会话上下文的 surface（如独立 CLI 调用）：显式不可用，不静默放行。
  requireKnowledgeSessionContext(sessionContext);
  const scope = resolveKnowledgeTurnScope({ knowledge, studioId, scopeId, sessionContext });
  const frozen = requireKnowledgeScopeSource(scope, sourceId);
  const owningNotebookId = resolveKnowledgeOwningNotebookId(scope, frozen, notebookId);
  if (!frozen.parseArtifactId) {
    throw new KnowledgeError("KNOWLEDGE_PARSE_NOT_READY", "Knowledge source has no frozen parse artifact");
  }
  const artifact = knowledge.store.getParseArtifact({ studioId, parseArtifactId: frozen.parseArtifactId });
  if (artifact.status !== "ready") {
    throw new KnowledgeError("KNOWLEDGE_PARSE_NOT_READY", "Knowledge source has no ready parse artifact");
  }
  const source = knowledge.getSource({ studioId, sourceId });
  return {
    artifactId: artifact.id,
    contentSnapshotId: frozen.contentSnapshotId,
    notebookId: owningNotebookId,
    sourceName: source.displayName,
    scope,
    // owning notebook 的生效分块尺寸：ensure 链与摄入侧同 configId 判定指纹。
    chunkTargetChars: knowledge.getNotebookEffectiveChunkTargetChars({ studioId, notebookId: owningNotebookId }),
  };
}

export function createKnowledgeReadTool(deps: KnowledgeReadToolDeps) {
  return {
    name: "knowledge_read",
    label: "Knowledge Read",
    description: "Read chunks of a Knowledge notebook source by ordinal range, or search within one source by query. "
      + "Use when a [KnowledgeContext] shard manifest lists more content than was injected: read shards with "
      + "the scopeId from the block's Scope line, plus sourceId and fromOrdinal/toOrdinal (1-based, both inclusive), "
      + "or narrow with query. The scopeId is this turn's knowledge permission ceiling: reads outside it are rejected. Read-only.",
    parameters: Type.Object({
      scopeId: Type.String({
        description: "Knowledge turn scope id from the [KnowledgeContext] block header (the Scope line). Required.",
      }),
      sourceId: Type.String({
        description: "Source to read, a sourceId from the [KnowledgeContext] shard manifest. Must be inside the scope.",
      }),
      notebookId: Type.Optional(Type.String({
        description: "Optional notebook scope. When given, it must be one of the scope's selected notebooks referencing this source.",
      })),
      fromOrdinal: Type.Optional(Type.Number({
        description: "First chunk ordinal to read (1-based). Defaults to 1; ignored when query is given.",
      })),
      toOrdinal: Type.Optional(Type.Number({
        description: "Last chunk ordinal to read (inclusive). Defaults to fromOrdinal. At most 40 chunks per call.",
      })),
      query: Type.Optional(Type.String({
        description: "Search within this source instead of reading an ordinal range.",
      })),
    }),
    sessionPermission: {
      // 只读原文；查询复用统一服务，可按知识配置调用嵌入和重排。
      resolveInvocation: () => ({
        action: "read",
        kind: "read",
        capability: "knowledge_read.read",
      }),
    },
    execute: async (_toolCallId: any, params: Record<string, any> = {}, signal?: AbortSignal, _onUpdate?: any, ctx?: any) => {
      const knowledge = deps.getKnowledge();
      const studioId = deps.getStudioId();
      if (!knowledge || !studioId) {
        return toolError("knowledge_read unavailable: Knowledge is not accessible in this runtime.", {
          errorCode: "KNOWLEDGE_MODEL_UNAVAILABLE",
        });
      }
      try {
        // scopeId 缺失 = 契约违例：显式拒绝，不得回落到旧的全 studio 行为（§二十二）。
        const scopeId = typeof params.scopeId === "string" && params.scopeId.trim()
          ? params.scopeId.trim()
          : null;
        if (!scopeId) {
          throw knowledgeScopeViolation(
            "scopeId is required: pass the scope id from the [KnowledgeContext] block header (Scope line)",
          );
        }
        const sourceId = requireNonEmptyString(params.sourceId, "sourceId");
        const notebookId = typeof params.notebookId === "string" && params.notebookId.trim()
          ? params.notebookId.trim()
          : null;
        const query = typeof params.query === "string" && params.query.trim() ? params.query.trim() : null;
        const sessionContext = deps.resolveSessionContext?.(ctx) ?? {
          sessionPath: null,
          parentSessionPath: null,
        };
        const resolved = resolveScopedArtifact(knowledge, studioId, scopeId, sourceId, notebookId, sessionContext);
        const researchContext = deps.resolveResearchContext?.(ctx) ?? null;
        const research = researchContext ? new ResearchStore(knowledge.store) : null;
        if (research && researchContext) {
          const run = research.requireRun(researchContext.runId);
          if (run.turnScopeId !== scopeId || !["planning", "running", "synthesizing"].includes(run.status)
            || (researchContext.allowedSourceIds !== undefined
              && (!Array.isArray(researchContext.allowedSourceIds)
                || researchContext.allowedSourceIds.some(id => !resolved.scope.sources.some(source => source.sourceId === id))
                || !researchContext.allowedSourceIds.includes(sourceId)))) {
            throw knowledgeScopeViolation("Knowledge read is outside the research scope");
          }
        }
        const prepareChunks = (selected: StoredKnowledgeChunk[]) => {
          if (!research || !researchContext) return selected.map(chunk => ({ ordinal: chunk.ordinal + 1, text: chunk.text }));
          const receipts = new EvidenceReceiptService(research);
          return research.transaction(() => selected.map(chunk => {
            if (chunk.parseArtifactId !== resolved.artifactId || chunk.spans.length === 0) {
              throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Research read requires frozen raw block positions");
            }
            // 检索摘要和索引正文不能直接成为凭据：逐段回到冻结原文读取，再把这段原文交给模型。
            const spans = chunk.spans.map(span => {
              const receipt = receipts.issue({
                ...researchContext, sourceId, contentSnapshotId: resolved.contentSnapshotId,
                parseArtifactId: resolved.artifactId, chunkIndexVariantId: chunk.chunkIndexVariantId, chunkId: chunk.id,
                blockId: span.blockId, startOffset: span.blockStartOffset, endOffset: span.blockEndOffset, channel: "knowledge_read",
              });
              const { text } = receipts.read({ runId: researchContext.runId, receiptId: receipt.id,
                allowedSourceIds: researchContext.allowedSourceIds, actorSessionId: researchContext.actorSessionId });
              return { receiptId: receipt.id, blockId: receipt.blockId, startOffset: receipt.startOffset, endOffset: receipt.endOffset, text };
            });
            return { ordinal: chunk.ordinal + 1, text: spans.map(span => span.text).join("\n"), spans };
          }));
        };

        if (query) {
          const compiledScope = await knowledge.compileTurnScope(resolved.scope);
          const { response, evidence: result } = await knowledge.searchService.searchWithEvidence({
            compiledScope, query, channel: "hybrid", limit: 12, sourceIds: [sourceId],
            notebookIds: [resolved.notebookId], rerank: true, signal,
          });
          // 降级显式标注（§十二）：向量变体未就绪/索引缺失时结果仍是合法 FTS
          // 答案，但 payload 携带 reason code；同时幂等入队后台补齐（去重由
          // 摄入层保证，重复检索不重复排队）。
          if (result.degraded.length > 0) {
            knowledge.requestVariantBuild({
              studioId,
              notebookId: resolved.notebookId,
              sourceId,
              artifactId: resolved.artifactId,
            });
          }
          if (research) signal?.throwIfAborted();
          const chunks = prepareChunks(result.candidates);
          return toolOk(JSON.stringify({
            source: resolved.sourceName,
            sourceId,
            notebookId: resolved.notebookId,
            scopeId,
            parseArtifactId: resolved.artifactId,
            contentSnapshotId: resolved.contentSnapshotId,
            mode: "search",
            retrievalMode: result.retrievalMode,
            vectorBackend: response.vectorBackend,
            degradedReasons: response.degradedReasons,
            retrievalModeRequested: result.retrievalModeRequested,
            ...(result.degraded.length > 0
              ? { degraded: result.degraded.map(({ reason, detail }) => ({ reason, ...(detail ? { detail } : {}) })) }
              : {}),
            matches: chunks,
          }, null, 2), { sourceId, mode: "search" });
        }

        // 索引身份锚（Phase 2 起纯解析、只读）：chunkProfileHash = 生效分块配置的
        // chunkerConfigId（与摄入侧同一解析链，查询不再惰性建绑/建索引）。
        // 变体缺失/未 ready → 幂等入队后台构建 + 显式报 KNOWLEDGE_PARSE_NOT_READY
        // （提示等摄入完成重试），不得静默当空。
        const blocks = knowledge.listArtifactBlocks({ studioId, parseArtifactId: resolved.artifactId });
        const chunkProfileHash = resolveKnowledgeChunkerConfig(blocks, {
          targetChars: resolved.chunkTargetChars,
        }).configId;
        const variant = knowledge.indexStore.resolveChunkIndexVariant(
          resolved.artifactId,
          chunkProfileHash,
        );
        if (!variant || variant.status !== "ready") {
          knowledge.requestVariantBuild({
            studioId,
            notebookId: resolved.notebookId,
            sourceId,
            artifactId: resolved.artifactId,
          });
          return toolError(
            `Knowledge source index is not ready yet (sourceId: ${sourceId}, variant status: ${variant?.status ?? "missing"}); `
            + "background build enqueued, retry after ingestion completes.",
            { errorCode: "KNOWLEDGE_PARSE_NOT_READY", sourceId },
          );
        }
        const indexedChunks = knowledge.indexStore.listVariantChunks(variant.id);
        const total = indexedChunks.length;
        if (total === 0) {
          return toolError(`Knowledge source has no indexed chunks (sourceId: ${sourceId}).`, {
            errorCode: "KNOWLEDGE_INDEX_INVALID",
            sourceId,
          });
        }
        const from = (optionalOrdinal(params.fromOrdinal, "fromOrdinal") ?? 1) - 1;
        const toExclusive = (optionalOrdinal(params.toOrdinal, "toOrdinal") ?? from + 1);
        if (toExclusive <= from) {
          return toolError(
            `toOrdinal must be >= fromOrdinal (received from=${from + 1}, to=${toExclusive}).`,
            { errorCode: "KNOWLEDGE_INVALID_ARGUMENT", sourceId },
          );
        }
        if (from >= total || toExclusive - from > MAX_CHUNKS_PER_READ) {
          return toolError(
            `Ordinal range out of bounds: source has ${total} chunks with ordinals 1-${total} `
            + `(requested ${from + 1}-${toExclusive}); at most ${MAX_CHUNKS_PER_READ} chunks per call.`,
            { errorCode: "KNOWLEDGE_INVALID_ARGUMENT", sourceId, totalChunks: total },
          );
        }
        const selected = indexedChunks
          .filter(chunk => chunk.ordinal >= from && chunk.ordinal < toExclusive)
          .sort((left, right) => left.ordinal - right.ordinal);
        if (research) signal?.throwIfAborted();
        const chunks = prepareChunks(selected);
        return toolOk(JSON.stringify({
          source: resolved.sourceName,
          sourceId,
          notebookId: resolved.notebookId,
          scopeId,
          parseArtifactId: resolved.artifactId,
          contentSnapshotId: resolved.contentSnapshotId,
          mode: "ordinal-range",
          requestedRange: [from + 1, Math.min(toExclusive, total)],
          totalChunks: total,
          chunks,
        }, null, 2), { sourceId, mode: "ordinal-range" });
      } catch (error) {
        if (isKnowledgeError(error)) {
          return toolError(`knowledge_read failed: ${error.code}: ${error.message}`, {
            errorCode: error.code,
          });
        }
        const message = error instanceof Error ? error.message : String(error);
        return toolError(`knowledge_read failed: ${message}`, {
          errorCode: "KNOWLEDGE_INTERNAL_ERROR",
        });
      }
    },
  };
}
