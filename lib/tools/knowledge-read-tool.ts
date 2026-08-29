/**
 * knowledge_read 工具 —— 读知识库笔记本源的分片（Phase 8 分片子 Agent 链路）。
 *
 * 消费方是子 Agent：[KnowledgeContext] 注入块超预算时只带分片清单，主模型用
 * `subagent` 工具并行派子 Agent，各用本工具按 ordinal 范围读一片（或按 query
 * 检索该源）再汇总。工具直连 engine 级 KnowledgeManager，跨会话可用。
 *
 * 权限边界：只读；studio 隔离（所有 store 查询都带 studioId，越界/不存在显式
 * 报错，不静默返回空）。
 */
import { Type } from "../pi-sdk/index.ts";
import { isKnowledgeError, KnowledgeError } from "../knowledge/errors.ts";
import type { KnowledgeManager } from "../knowledge/knowledge-manager.ts";
import type { KnowledgeModelRef } from "../knowledge/types.ts";
import { toolError, toolOk } from "./tool-result.ts";

/** 单次读片的防护上限：防止一次调用把整个大源灌进子 Agent 上下文。 */
const MAX_CHUNKS_PER_READ = 40;

export interface KnowledgeReadToolDeps {
  /** engine 级 KnowledgeManager（跨会话）；null = Knowledge 不可用。 */
  getKnowledge: () => KnowledgeManager | null;
  /** 当前 runtime studioId；null = 运行时上下文不可用。 */
  getStudioId: () => string | null;
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
 * 解析源的最新 ready parse artifact。notebookId 给出时校验 membership
 * （源不在该笔记本 → 显式报错）；artifact 非 ready → KNOWLEDGE_PARSE_NOT_READY。
 */
function resolveReadyArtifact(
  knowledge: KnowledgeManager,
  studioId: string,
  sourceId: string,
  notebookId: string | null,
): {
  artifactId: string;
  notebookId: string;
  sourceName: string;
  embeddingModelRef: KnowledgeModelRef | null;
  chunkTargetChars: number;
  rerankModelRef: KnowledgeModelRef | null;
} {
  const source = knowledge.getSource({ studioId, sourceId });
  if (notebookId) {
    knowledge.getNotebook({ studioId, notebookId });
    const inNotebook = knowledge.listNotebookSources({ studioId, notebookId })
      .some(entry => entry.source.id === sourceId);
    if (!inNotebook) {
      throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Knowledge source is not in this Notebook");
    }
  }
  // 源可能属于多个笔记本：找到含该源且最新 artifact ready 的 membership。
  const notebooks = knowledge.listNotebooks({ studioId });
  for (const notebook of notebooks) {
    const entry = knowledge.listNotebookSources({ studioId, notebookId: notebook.id })
      .find(item => item.source.id === sourceId);
    if (!entry) continue;
    if (entry.parseArtifact?.status !== "ready") continue;
    return {
      artifactId: entry.parseArtifact.id,
      notebookId: notebook.id,
      sourceName: entry.source.displayName,
      // owning notebook 的嵌入引用：源内检索按同一模型路由（与索引侧一致）。
      embeddingModelRef: knowledge.getNotebookConfig?.({ studioId, notebookId: notebook.id })
        .embeddingModelRef ?? null,
      // owning notebook 的生效分块尺寸：ensure 链与摄入侧同 configId 判定指纹。
      chunkTargetChars: knowledge.getNotebookEffectiveChunkTargetChars({ studioId, notebookId: notebook.id }),
      // owning notebook 的重排引用：按引用路由（不可解析 → 回调侧 null → RRF 降级）。
      rerankModelRef: knowledge.getNotebookConfig?.({ studioId, notebookId: notebook.id })
        .rerankModelRef ?? null,
    };
  }
  throw new KnowledgeError("KNOWLEDGE_PARSE_NOT_READY", "Knowledge source has no ready parse artifact");
}

export function createKnowledgeReadTool(deps: KnowledgeReadToolDeps) {
  return {
    name: "knowledge_read",
    label: "Knowledge Read",
    description: "Read chunks of a Knowledge notebook source by ordinal range, or search within one source by query. "
      + "Use when a [KnowledgeContext] shard manifest lists more content than was injected: read shards with "
      + "sourceId plus fromOrdinal/toOrdinal (1-based, both inclusive), or narrow with query. Read-only.",
    parameters: Type.Object({
      notebookId: Type.Optional(Type.String({
        description: "Optional notebook scope. When given, the source must belong to this notebook.",
      })),
      sourceId: Type.String({
        description: "Source to read, a sourceId from the [KnowledgeContext] shard manifest.",
      }),
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
      // 只读、无副作用：读片/检索不产生任何写入或外部请求（检索侧模型调用
      // 走注入链路自己的操作客户端，与本工具无关）。
      resolveInvocation: () => ({
        action: "read",
        kind: "read",
        capability: "knowledge_read.read",
      }),
    },
    execute: async (_toolCallId: any, params: Record<string, any> = {}) => {
      const knowledge = deps.getKnowledge();
      const studioId = deps.getStudioId();
      if (!knowledge || !studioId) {
        return toolError("knowledge_read unavailable: Knowledge is not accessible in this runtime.", {
          errorCode: "KNOWLEDGE_MODEL_UNAVAILABLE",
        });
      }
      try {
        const sourceId = requireNonEmptyString(params.sourceId, "sourceId");
        const notebookId = typeof params.notebookId === "string" && params.notebookId.trim()
          ? params.notebookId.trim()
          : null;
        const query = typeof params.query === "string" && params.query.trim() ? params.query.trim() : null;
        const resolved = resolveReadyArtifact(knowledge, studioId, sourceId, notebookId);

        if (query) {
          const result = await knowledge.queryService.retrieveForArtifacts({
            studioId,
            artifactIds: [resolved.artifactId],
            question: query,
            topK: 12,
            embeddingModelRef: resolved.embeddingModelRef,
            chunkTargetChars: resolved.chunkTargetChars,
            rerankModelRef: resolved.rerankModelRef,
          });
          const chunks = result.candidates.map(chunk => ({
            ordinal: chunk.ordinal + 1,
            text: chunk.text,
          }));
          return toolOk(JSON.stringify({
            source: resolved.sourceName,
            sourceId,
            notebookId: resolved.notebookId,
            mode: "search",
            retrievalMode: result.retrievalMode,
            matches: chunks,
          }, null, 2), { sourceId, mode: "search" });
        }

        const total = knowledge.indexStore.listArtifactChunks(resolved.artifactId).length;
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
        const chunks = knowledge.indexStore.listArtifactChunks(resolved.artifactId)
          .filter(chunk => chunk.ordinal >= from && chunk.ordinal < toExclusive)
          .sort((left, right) => left.ordinal - right.ordinal)
          .map(chunk => ({ ordinal: chunk.ordinal + 1, text: chunk.text }));
        return toolOk(JSON.stringify({
          source: resolved.sourceName,
          sourceId,
          notebookId: resolved.notebookId,
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
