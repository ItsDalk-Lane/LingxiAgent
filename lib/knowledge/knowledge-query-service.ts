import { searchVectorBackend, type KnowledgeVectorSearchBackend } from "./vector-search-backend.ts";
import crypto from "node:crypto";
import { QueryEmbeddingCache, normalizeKnowledgeQuery } from "./query-embedding-cache.ts";
import { resolveReadyKnowledgeQueryVariant, type CompiledKnowledgeScope, type CompiledKnowledgeNotebook } from "./scope-snapshot-compiler.ts";
import { estimateTextTokens } from "../llm/estimate-text-tokens.ts";
import { EvidenceSpanExtractor } from "./evidence-span-extractor.ts";

import {
  buildKnowledgeChunks,
  buildKnowledgeSections,
  knowledgeBlockFingerprint,
  resolveKnowledgeChunkerConfig,
  type KnowledgeChunkDraft,
} from "./chunker.ts";
import { KnowledgeError, isKnowledgeError } from "./errors.ts";
import {
  KnowledgeIndexStore,
  knowledgeChunkIndexVariantId,
  type IndexedKnowledgeChunk,
  type KnowledgeOrdinalRange,
  type StoredKnowledgeChunk,
  type KnowledgeChunkVariantMetadata,
} from "./knowledge-index-store.ts";
import { KnowledgeStore } from "./knowledge-store.ts";
import type { KnowledgeBlock, KnowledgeIngestionEmbeddingStats, KnowledgeModelRef } from "./types.ts";
import { MODEL_OPERATION_RERANK_MAX_DOCS } from "../../shared/model-operations.ts";
import type { KnowledgeDegradeReason } from "../../shared/knowledge-reason-codes.ts";
import {
  knowledgeVectorIndexVariantId,
  type VectorIndexAdapter,
  type VectorIndexModelIdentity,
} from "./vector-index-adapter.ts";

export interface KnowledgeEmbeddingResult {
  vectors: number[][];
  dimensions: number;
  model: {
    provider: string;
    id: string;
    api: string;
    dimensions?: number;
  };
}

export type KnowledgeEmbedder = (request: {
  runId: string;
  texts: string[];
  signal?: AbortSignal;
}) => Promise<KnowledgeEmbeddingResult | null>;

export type KnowledgeReranker = (request: {
  runId: string;
  query: string;
  documents: string[];
  topN: number;
  signal?: AbortSignal;
}) => Promise<{ results: Array<{ index: number; score: number }> } | null>;

/** retrieveForNotebooks 的候选：检索核心 chunk + 注入块定位头所需的元数据。 */
export interface NotebookRetrievalChunk extends IndexedKnowledgeChunk {
  notebookId: string;
  notebookName: string;
  sourceId: string;
  sourceName: string;
  headingPath: string[] | null;
  pageNumber: number | null;
}

/** 被引笔记本内一个 ready 源的清单条目（chunk 总数 + 首章节标题）。 */
export interface NotebookRetrievalSource {
  notebookId: string;
  notebookName: string;
  sourceId: string;
  sourceName: string;
  parseArtifactId: string;
  chunkCount: number;
  firstHeadingPath: string[] | null;
  /**
   * 源清单条目对应的检索锚：该笔记本生效分块配置的 chunkProfileHash
   * （任务书 §六十七 EvidenceManifest 身份链字段；旧调用方可不携带）。
   */
  chunkProfileHash?: string;
  /**
   * 源内可用 section 键（headingPath join " > "，Phase 8 broad §三十九）：
   * distinct 且按源内出现序。可选字段——旧调用方/无结构元数据时不携带，
   * section coverage 检测对缺席者按"无可用章节"处理（不猜测）。
   */
  sections?: string[];
}

/**
 * 本轮实际参与向量检索的变体身份（任务书 §六十七 EvidenceManifest）：
 * (parseArtifactId, chunkProfileHash) 的 ChunkIndexVariant 与叠加嵌入模型
 * 身份后的 VectorIndexVariant。fts-only / 向量未就绪的检索不产生条目。
 */
export interface SearchedVectorVariantIdentity {
  parseArtifactId: string;
  chunkProfileHash: string;
  chunkIndexVariantId: string;
  vectorIndexVariantId: string;
}

export interface RetrieveForNotebooksResult {
  vectorBackend?: "hnsw" | "portable" | "none";
  vectorDegradedReasons?: string[];
  embeddingGroups?: number;
  rerankGroups?: number;
  queryEmbeddingCacheHit?: boolean;
  retrievalResultCacheHit?: boolean;
  candidates: NotebookRetrievalChunk[];
  sources: NotebookRetrievalSource[];
  retrievalMode: "fts" | "hybrid";
  /** 本轮请求的检索模式（任一笔记本配置了可路由的嵌入模型即 "hybrid"；§十二留痕）。 */
  retrievalModeRequested: "fts" | "hybrid";
  /** 逐 scope 降级明细（缺失/未就绪的索引变体已幂等入队后台构建）；无降级为空数组。 */
  degraded: KnowledgeDegradedRetrievalScope[];
  /** 各笔记本本轮实际搜索过的向量变体身份（去重）；缺省/空 = 纯 FTS 轮。 */
  searchedVectorVariants?: SearchedVectorVariantIdentity[];
  /**
   * rerank 降级留痕（每笔记本一条，含笔记本名；空/缺省 = 全部正常重排或未尝试）。
   * 见 KNOWLEDGE_RERANK_DEADLINE_MS。
   */
  rerankDegradeReasons?: string[];
  /** @deprecated 仅用于历史读取，生产写入侧不再生成。 */
  rerankSkippedReasons?: string[];
  /**
   * 检索分段计时（2026-08-31 观测补齐）：多笔记本为各段跨笔记本最大值
   * （反映对关键路径的贡献）；纯增量可选，旧调用方不携带。
   */
  stageTimings?: KnowledgeRetrievalStageTimings;
}

/** 检索侧各段墙钟（毫秒）；未执行的段不携带。 */
export interface KnowledgeRetrievalStageTimings {
  ftsMs?: number;
  embedMs?: number;
  vectorMs?: number;
  fuseMs?: number;
  rerankMs?: number;
}

/** retrieve() 产出的逐 scope 降级条目（尚未附带 notebook/source 归属，由调用方映射）。 */
export interface KnowledgeDegradedRetrievalScope {
  parseArtifactId: string;
  chunkProfileHash: string;
  reason: KnowledgeDegradeReason;
  /** 人类可读补充（如变体状态、损坏自愈）；判定只用 reason，禁止匹配本字段。 */
  detail?: string;
  /** 归属信息（retrieveForNotebooks 填充，供注入块/stats 留痕）。 */
  notebookId?: string;
  notebookName?: string;
  sourceId?: string;
  sourceName?: string;
}

function isAbortLike(error: any): boolean {
  return error?.name === "AbortError" || error?.name === "TimeoutError" || error?.type === "aborted";
}

/**
 * 检索锚点（索引库 schema v2 契约）：检索范围精确到 (parseArtifactId, chunkProfileHash)
 * 一个 ChunkIndexVariant；chunkProfileHash 即 chunker 的 configId（跨库身份键）。
 * blockFingerprint 是解析锚点时同批 blocks 的内容指纹：查询只读 ready 且指纹一致的
 * 变体，指纹过期（源已变更、重建在途）按 KNOWLEDGE_INDEX_BUILDING 降级。
 */
interface KnowledgeRetrievalScope {
  parseArtifactId: string;
  chunkProfileHash: string;
  blockFingerprint: string;
}

function chunkFingerprint(chunks: KnowledgeChunkDraft[]): string {
  const hash = crypto.createHash("sha256");
  for (const chunk of chunks) {
    hash.update(chunk.id, "utf8");
    hash.update("\0", "utf8");
    hash.update(chunk.text, "utf8");
    hash.update("\0", "utf8");
  }
  return hash.digest("hex");
}

function vectorModelIdentity(result: KnowledgeEmbeddingResult): VectorIndexModelIdentity {
  const provider = result?.model?.provider;
  const modelId = result?.model?.id;
  const protocol = result?.model?.api;
  const dimensions = Number(result?.dimensions);
  if (
    typeof provider !== "string" || !provider
    || typeof modelId !== "string" || !modelId
    || typeof protocol !== "string" || !protocol
    || !Number.isSafeInteger(dimensions) || dimensions <= 0
    || (result.model.dimensions !== undefined && result.model.dimensions !== dimensions)
  ) {
    throw new KnowledgeError("KNOWLEDGE_RETRIEVAL_UNAVAILABLE", "Embedding model identity is invalid");
  }
  const descriptor = JSON.stringify([provider, modelId, protocol, dimensions]);
  return {
    key: crypto.createHash("sha256").update(descriptor, "utf8").digest("hex"),
    provider,
    modelId,
    protocol,
    dimensions,
  };
}

function assertEmbeddingBatch(
  result: KnowledgeEmbeddingResult | null,
  expectedCount: number,
  expectedModel?: VectorIndexModelIdentity,
): { result: KnowledgeEmbeddingResult; model: VectorIndexModelIdentity } {
  if (!result || !Array.isArray(result.vectors) || result.vectors.length !== expectedCount) {
    throw new KnowledgeError("KNOWLEDGE_RETRIEVAL_UNAVAILABLE", "Embedding response does not match the requested batch");
  }
  const model = vectorModelIdentity(result);
  if (
    expectedModel
    && (model.key !== expectedModel.key || model.dimensions !== expectedModel.dimensions)
  ) {
    throw new KnowledgeError("KNOWLEDGE_RETRIEVAL_UNAVAILABLE", "Embedding model changed during one Knowledge run");
  }
  for (const vector of result.vectors) {
    if (
      !Array.isArray(vector)
      || vector.length !== model.dimensions
      || vector.some(value => typeof value !== "number" || !Number.isFinite(value))
    ) {
      throw new KnowledgeError("KNOWLEDGE_RETRIEVAL_UNAVAILABLE", "Embedding response contains an invalid vector");
    }
  }
  return { result, model };
}

/** RRF 融合常数（k=60）：FTS/向量通道（fuseCandidates）与跨笔记本名次融合共用同一公式。 */
export const KNOWLEDGE_RRF_K = 60;

function fuseCandidates(
  fts: IndexedKnowledgeChunk[],
  vector: IndexedKnowledgeChunk[],
  limit = 12,
): IndexedKnowledgeChunk[] {
  const fused = new Map<string, { chunk: IndexedKnowledgeChunk; score: number }>();
  const add = (chunks: IndexedKnowledgeChunk[], channel: "fts" | "vector") => {
    chunks.forEach((chunk, index) => {
      const current = fused.get(chunk.id) || { chunk, score: 0 };
      current.score += 1 / (KNOWLEDGE_RRF_K + index + 1);
      current.chunk = { ...current.chunk, channels: [...new Set([...(current.chunk.channels ?? []), channel])] };
      fused.set(chunk.id, current);
    });
  };
  add(fts, "fts");
  add(vector, "vector");
  return [...fused.values()]
    .sort((left, right) => (
      right.score - left.score
      || left.chunk.parseArtifactId.localeCompare(right.chunk.parseArtifactId)
      || left.chunk.ordinal - right.chunk.ordinal
    ))
    .slice(0, limit)
    .map(entry => ({ ...entry.chunk, score: entry.score }));
}

/**
 * 跨笔记本 rank-based RRF 融合（§二十五）：各笔记本的名次序列（已各自 rerank
 * 或内部 RRF 名次）作为独立序列，按与 fuseCandidates 相同的公式
 * （KNOWLEDGE_RRF_K=60）跨序列融合——只消费名次，绝不读跨笔记本的 raw
 * score（rerank 分数按各自模型归一、cosine 分数按各自嵌入模型，跨模型不可比）；
 * 多序列同时命中的 chunk 靠贡献求和自然靠前。同 chunk.id 去重：首个出现的
 * 序列（notebookIds 顺序）保留 chunk 载荷与归属（载荷上的 score 仍是归属
 * 笔记本的本地分，仅供留痕，不参与跨笔记本比较）。并列按 notebook 序 /
 * parseArtifactId / ordinal 稳定排序，与各检索的完成顺序无关。
 */
export function fuseNotebookRankings(
  rankings: IndexedKnowledgeChunk[][],
  useFusedScores = false,
): Array<{ chunk: IndexedKnowledgeChunk; notebookIndex: number }> {
  const fused = new Map<string, { chunk: IndexedKnowledgeChunk; notebookIndex: number; score: number }>();
  rankings.forEach((ranking, notebookIndex) => {
    ranking.forEach((chunk, rank) => {
      const contribution = 1 / (KNOWLEDGE_RRF_K + rank + 1);
      const current = fused.get(chunk.id);
      if (current) {
        current.score += contribution;
        current.chunk = { ...current.chunk, channels: [...new Set([...(current.chunk.channels ?? []), ...(chunk.channels ?? [])])] };
        return;
      }
      fused.set(chunk.id, { chunk, notebookIndex, score: contribution });
    });
  });
  return [...fused.values()]
    .sort((left, right) => (
      right.score - left.score
      || left.notebookIndex - right.notebookIndex
      || left.chunk.parseArtifactId.localeCompare(right.chunk.parseArtifactId)
      || left.chunk.ordinal - right.chunk.ordinal
    ))
    .map(entry => ({ chunk: useFusedScores ? { ...entry.chunk, score: entry.score } : entry.chunk, notebookIndex: entry.notebookIndex }));
}

export interface KnowledgeBlockLocator {
  headingPath: string[] | null;
  pageNumber: number | null;
}

/**
 * section 分桶键（Phase 8 broad §三十九）：headingPath 以 " > " 连接的规范化键，
 * 与注入块定位头的 heading 呈现一致；无 heading 结构元数据的 chunk 归入 null 桶
 * （不参与 section coverage 的分子分母）。
 */
export function knowledgeSectionKeyOf(headingPath: string[] | null | undefined): string | null {
  if (!headingPath || headingPath.length === 0) return null;
  return headingPath.join(" > ");
}

/**
 * blockId → 定位信息（headingPath / 页码）的索引构建（纯函数）。
 * 检索核心（annotateRetrievalChunk）与分块卡片门面（listArtifactChunkCards）
 * 共用，避免复制粘贴 locator 解析规则。
 */
export function buildKnowledgeBlockLocatorIndex(blocks: KnowledgeBlock[]): Map<string, KnowledgeBlockLocator> {
  const index = new Map<string, KnowledgeBlockLocator>();
  for (const block of blocks) {
    const raw = block.locator?.headingPath;
    const headingPath = Array.isArray(raw)
      ? raw.filter((entry): entry is string => typeof entry === "string" && entry.trim().length > 0)
      : null;
    const rawPage = block.locator?.pageNumber;
    const pageNumber = typeof rawPage === "number" && Number.isFinite(rawPage) ? rawPage : null;
    index.set(block.id, {
      headingPath: headingPath && headingPath.length > 0 ? headingPath : null,
      pageNumber,
    });
  }
  return index;
}

/** 摄入与后台回填共用，按块在变体中首次出现的顺序记录目录。 */
function buildVariantMetadata(chunks: KnowledgeChunkDraft[], blocks: KnowledgeBlock[]): KnowledgeChunkVariantMetadata {
  const locators = buildKnowledgeBlockLocatorIndex(blocks);
  let firstHeadingPath: string[] | null = null;
  const sections = new Set<string>();
  for (const chunk of chunks) {
    for (const span of chunk.spans) {
      const heading = locators.get(span.blockId)?.headingPath ?? null;
      firstHeadingPath ??= heading;
      const key = knowledgeSectionKeyOf(heading);
      if (key != null) sections.add(key);
    }
  }
  return { firstHeadingPath, sectionKeys: [...sections] };
}

/**
 * "无上限"召回的物理边界：与笔记本 retrieval_top_k 的 sanity 上限一致
 * （knowledge-store MAX_RETRIEVAL_TOP_K=1000），防病态全表膨胀。
 */
export const KNOWLEDGE_UNCAPPED_RETRIEVAL_LIMIT = 1000;
/**
 * rerank 输入文档数防护：无上限召回后候选可达千级，但 rerank 精度在远小于
 * 100 时已饱和且多数 rerank API 有文档数上限；超出部分保持 RRF 名次不再重排。
 * 与 RerankClient 校验共用 shared/model-operations 的同一上限，杜绝两侧各自常量打架。
 */
export const KNOWLEDGE_RERANK_MAX_DOCS = MODEL_OPERATION_RERANK_MAX_DOCS;

/**
 * rerank 执行期限（2026-08-30 延迟加固）：单次 rerank 调用超过该时长即放弃，
 * 候选保持 RRF 名次继续检索（与「超出 MAX_DOCS 的尾部保持 RRF 名次」同一降级
 * 语义），rerankDegradeReason 显式留痕。动机：远程 rerank 供应商排队方差大
 * （实测单次 11–56s），无期限时一次知识提问的重排尾巴可达一分钟以上；重排是
 * 精排增强层，不该拖死整条检索。传输类失败（网络/HTTP/供应商 5xx）同路径
 * 降级；KnowledgeError 与用户 abort 仍然上抛（禁静默吞真实错误）。
 */
export const KNOWLEDGE_RERANK_DEADLINE_MS = 15_000;

/**
 * 查询嵌入执行期限（2026-08-30 延迟加固）：单次查询侧嵌入调用超过该时长即
 * 放弃，候选保持 FTS 名次继续检索并显式留痕（KNOWLEDGE_EMBEDDING_FAILED）。
 * 与 rerank 期限对称（见 KNOWLEDGE_RERANK_DEADLINE_MS）；动机：engine 嵌入
 * 闭包的 HTTP 超时 300s 全额放行，挂着的嵌入供应商 = 一次提问卡 5 分钟。
 * 摄入侧批量嵌入不受影响（不经过本竞速，走闭包自身超时）。本地 Ollama 实测
 * 0.1–2s、远程 API 数秒，15s 充裕。
 */
export const KNOWLEDGE_EMBEDDING_DEADLINE_MS = 15_000;

export class KnowledgeQueryService {
  readonly queryEmbeddingCache = new QueryEmbeddingCache();
  private configurationRevision = 0;
  private readonly deps: {
    store: KnowledgeStore;
    indexStore: KnowledgeIndexStore;
    getModelConfigurationRevision?: (ref: KnowledgeModelRef) => string;
    vectorIndex?: VectorIndexAdapter | null;
    vectorSearchBackend?: KnowledgeVectorSearchBackend;
    /**
     * 按显式嵌入模型引用执行嵌入（engine 的 _embedKnowledgeTextsForModel 同根，
     * 与摄入侧共用）。查询侧由 retrieveForNotebooks 按笔记本配置构造闭包传入
     * retrieve()——引用不可解析返回 null，该笔记本走纯 FTS（禁静默换模型）。
     */
    embedTextsForModel?: ((request: {
      runId: string;
      texts: string[];
      modelRef: KnowledgeModelRef;
      signal?: AbortSignal;
      /** 查询侧固定 "query"（MiniMax db/query、Voyage input_type 算法分离），摄入侧缺省 document。 */
      inputType?: "document" | "query";
    }) => Promise<KnowledgeEmbeddingResult | null>) | null;
    rerank?: KnowledgeReranker | null;
    /**
     * 按笔记本显式引用路由的 rerank 执行回调（v8：全局 rerank 槽已退役）。
     * 配置类不可解析由回调侧记日志并返回 null → 检索显式降级 RRF 名次。
     */
    rerankForModel?: ((request: {
      runId: string;
      query: string;
      documents: string[];
      topN: number;
      signal?: AbortSignal;
      modelRef: KnowledgeModelRef;
    }) => Promise<{ results: Array<{ index: number; score: number }> } | null>) | null;
    /** 查嵌入模型输入上限；新分块粒度固定，不从窗口推导。 */
    getEmbeddingModelContextWindow?: ((modelRef: KnowledgeModelRef) => number | null) | null;
    /**
     * 后台补齐回调（§十二）：查询发现索引变体缺失/未就绪时幂等入队构建任务
     * （KnowledgeManager 接线到 ingestion.requestVariantBuild；活跃 job 去重）。
     * 同步、非阻塞：查询线程绝不等待构建。未接线（测试等）时只留痕不入队。
     */
    requestVariantBuild?: ((input: {
      studioId: string;
      notebookId: string;
      sourceId: string;
      parseArtifactId: string;
      reason: KnowledgeDegradeReason;
    }) => void) | null;
  };

  constructor(deps: KnowledgeQueryService["deps"]) {
    this.deps = deps;
  }

  /**
   * 向量保留策略 sweep：回收"旧版本"向量——(a) 非该源最新解析产物的全部向量；
   * (b) 最新产物下非最新嵌入身份的向量（换模型/改维度后的作废副本）。候选超过
   * 挂靠笔记本配置的保留天数（以向量库 last_used_at 计，查询命中即刷新）未
   * 被使用即删除；任一挂靠笔记本未配置保留策略 = 该源永久保留。由摄入循环
   * 周期调用（每小时量级）。返回删除的（artifact, modelKey）数。
   */
  sweepStaleVectorArtifacts(input?: { now?: () => string }): number {
    const vectorIndex = this.deps.vectorIndex;
    if (!vectorIndex || typeof vectorIndex.listArtifactUsage !== "function") return 0;
    const nowMs = Date.parse((input?.now ?? (() => new Date().toISOString()))());
    let removed = 0;
    // 历史孤儿源（无活跃挂靠笔记本，UI 不可达）：向量无人引用，直接回收。
    // 新删除已由 manager 即时清理，这里只兜底清理逻辑上线前的残留。
    for (const artifactId of this.deps.store.listOrphanArtifactIds()) {
      vectorIndex.removeArtifact(artifactId);
      removed += 1;
    }
    const ownership = new Map(
      this.deps.store.listArtifactVectorSweepRows().map((row) => [row.artifactId, row]),
    );
    // 最新产物按"最新身份保留"豁免：同 artifact 下 indexedAt 最新的 modelKey 不删。
    const usage = vectorIndex.listArtifactUsage();
    const latestKeyByArtifact = new Map<string, { modelKey: string; indexedAt: string }>();
    for (const entry of usage) {
      const current = latestKeyByArtifact.get(entry.parseArtifactId);
      if (!current || entry.indexedAt > current.indexedAt) {
        latestKeyByArtifact.set(entry.parseArtifactId, { modelKey: entry.modelKey, indexedAt: entry.indexedAt });
      }
    }
    for (const entry of usage) {
      const owner = ownership.get(entry.parseArtifactId);
      if (!owner || owner.retentionDays == null) continue;
      if (owner.isLatestForSource && latestKeyByArtifact.get(entry.parseArtifactId)?.modelKey === entry.modelKey) {
        continue;
      }
      const lastUsedMs = Date.parse(entry.lastUsedAt || entry.indexedAt);
      if (Number.isFinite(lastUsedMs) && nowMs - lastUsedMs < owner.retentionDays * 86_400_000) continue;
      vectorIndex.removeArtifactModel({
        parseArtifactId: entry.parseArtifactId,
        modelKey: entry.modelKey,
      });
      removed += 1;
    }
    return removed;
  }

  /**
   * 摄入管线的 chunk+fts_index 相位：分块与 FTS 索引在同一次幂等替换中原子完成。
   * 身份锚是 ChunkIndexVariant (parseArtifactId, chunkProfileHash)（chunkProfileHash
   * 即 chunkerConfigId）：指纹（blocks 内容）匹配且变体 ready 即整体跳过；不匹配只
   * 重建该变体自己的 chunk 集合，同 artifact 的其他分块配置变体并存、互不覆盖。
   * targetChars 由调用方（摄入 worker / 分块卡片门面）按笔记本生效值解析
   * （resolveEffectiveChunkTargetChars 同源），缺省才落 chunker 内置默认。
   *
   * Phase 2 边界：本方法是唯一的 chunk/FTS 构建入口，仅供摄入相位与管理视图
   * （listArtifactChunkCards）调用；查询路径（retrieve*）不再调用——变体缺失/
   * 未就绪时查询显式降级并幂等入队后台构建（§十一/§十二/§十三）。
   */
  indexArtifactForIngestion(
    studioId: string,
    parseArtifactId: string,
    options?: { targetChars?: number },
  ): { chunkerConfigId: string; chunkIndexVariantId: string; rebuilt: boolean } {
    const run = () => {
      const blocks = this.deps.store.listArtifactBlocks({ studioId, parseArtifactId });
      const fingerprint = knowledgeBlockFingerprint(blocks);
      const config = resolveKnowledgeChunkerConfig(blocks, { targetChars: options?.targetChars });
      const variant = this.deps.indexStore.ensureChunkIndexVariant({
        parseArtifactId,
        chunkProfileHash: config.configId,
        blockFingerprint: fingerprint,
      });
      // 实参顺序即新契约：(parseArtifactId, chunkProfileHash, fingerprint)——
      // 旧顺序 (parseArtifactId, fingerprint, configId) 三个都是 string，换序即静默误判。
      if (this.deps.indexStore.hasArtifactFingerprint(parseArtifactId, config.configId, fingerprint)) {
        return { chunkerConfigId: config.configId, chunkIndexVariantId: variant.id, rebuilt: false };
      }
      try {
        const chunks = buildKnowledgeChunks(parseArtifactId, blocks, { targetChars: options?.targetChars });
        if (chunks.length === 0) {
          throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Ready Knowledge source produced no searchable chunks");
        }
        this.deps.indexStore.replaceArtifactChunks({
          parseArtifactId,
          chunkProfileHash: config.configId,
          blockFingerprint: fingerprint,
          chunks,
          metadata: buildVariantMetadata(chunks, blocks),
          sections: buildKnowledgeSections(parseArtifactId, blocks),
          sourceDocument: this.buildSourceDocument(studioId, parseArtifactId, blocks),
        });
      } catch (error) {
        // 显式终态：构建失败的变体落 failed（查询侧只读 ready 变体，读不到半写状态）。
        try {
          this.deps.indexStore.setChunkIndexVariantStatus(variant.id, "failed");
        } catch {
          // 保留原始构建错误（索引库损坏走外层 reset 自愈，此时行已不存在）。
        }
        throw error;
      }
      return { chunkerConfigId: config.configId, chunkIndexVariantId: variant.id, rebuilt: true };
    };
    try {
      return run();
    } catch (error) {
      // 索引库是可重建缓存：损坏时重置后重试一次（§十三 exception recovery；
      // 正常 missing 不走这里——missing 由摄入补齐，见 retrieve 的降级路径）。
      if (!isKnowledgeError(error) || error.code !== "KNOWLEDGE_INDEX_INVALID") throw error;
      this.deps.indexStore.reset();
      return run();
    }
  }

  /** 仅由启动后的后台批次调用；归属从宿主事实库读取，不接收用户指定的工作室。 */
  backfillVariantMetadata(variant: { id: string; parseArtifactId: string }): string {
    const owner = this.deps.store.db.prepare(`
      SELECT s.studio_id, s.id AS source_id FROM parse_artifacts pa
      JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
      JOIN sources s ON s.id = cs.source_id WHERE pa.id = ?
    `).get(variant.parseArtifactId);
    if (!owner) throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Metadata source artifact is missing");
    const blocks = this.deps.store.listArtifactBlocks({ studioId: owner.studio_id, parseArtifactId: variant.parseArtifactId });
    const chunks = this.deps.indexStore.listVariantChunks(variant.id);
    this.deps.indexStore.writeVariantMetadata(variant.id, buildVariantMetadata(chunks, blocks));
    return owner.source_id;
  }

  /** 本地检索入口只接收冻结编译结果，不触发嵌入、向量、重排或索引恢复。 */
  searchCompiledScopeFts(input: {
    compiledScope: CompiledKnowledgeScope;
    query: string;
    limit: number;
  }): IndexedKnowledgeChunk[] {
    return this.deps.indexStore.searchReadyVariantIds({
      chunkIndexVariantIds: input.compiledScope.readyChunkVariantIds,
      query: input.query,
      limit: input.limit,
    });
  }

  private buildSourceDocument(studioId: string, parseArtifactId: string, blocks: KnowledgeBlock[]) {
    const artifact = this.deps.store.getParseArtifact({ studioId, parseArtifactId });
    const snapshot = this.deps.store.getContentSnapshot({ studioId, snapshotId: artifact.contentSnapshotId });
    const source = this.deps.store.getSource({ studioId, sourceId: snapshot.sourceId });
    const outlineText = [...new Set(buildKnowledgeSections(parseArtifactId, blocks)
      .filter(section => section.headingPath.length > 0).map(section => section.headingPath.join(" > ")))].join("\n");
    return { parseArtifactId, title: source.displayName, outlineText,
      searchText: [source.displayName, outlineText, blocks[0]?.text.slice(0, 2048) ?? "",
        blocks.at(-1)?.text.slice(-2048) ?? "", snapshot.mimeType, source.sourceType].join("\n") };
  }

  getModelConfigurationRevision(ref: KnowledgeModelRef): string {
    return this.deps.getModelConfigurationRevision?.(ref) ?? String(this.configurationRevision);
  }

  onModelConfigMayHaveChanged(): void { this.configurationRevision += 1; }

  /** 同一嵌入引用的一组笔记本共用查询向量，全部变体一次搜索；重排另由宿主安排。 */
  retrieveCompiledGroup(input: {
    compiledScope: CompiledKnowledgeScope;
    notebooks: CompiledKnowledgeNotebook[];
    variantIds: string[];
    query: string;
    limit: number;
    signal?: AbortSignal;
    ordinalRanges?: ReadonlyMap<string, KnowledgeOrdinalRange[]>;
    sectionIds?: ReadonlyMap<string, readonly string[]>;
    requiredSectionIds?: ReadonlyMap<string, readonly string[]>;
    onRemoteCall: () => void;
    onEmbeddingCacheHit: () => void;
  }) {
    const scopes = new Map<string, KnowledgeRetrievalScope>();
    for (const notebook of input.notebooks) {
      for (const source of input.compiledScope.sources) {
        if (!source.parseArtifactId || !notebook.chunkProfileHash || !source.notebookIds.includes(notebook.notebookId)) continue;
        const variant = resolveReadyKnowledgeQueryVariant({ ...this.deps,
          parseArtifactId: source.parseArtifactId, chunkProfileHash: notebook.chunkProfileHash, readyChunkVariantIds: input.variantIds,
        });
        if (variant && input.variantIds.includes(variant.id)) scopes.set(variant.id, {
          parseArtifactId: variant.parseArtifactId, chunkProfileHash: variant.chunkProfileHash,
          blockFingerprint: variant.blockFingerprint,
        });
      }
    }
    const embeddingRef = input.notebooks[0]?.embeddingModelRef;
    return this.retrieve({
      studioId: input.compiledScope.studioId, scopes: [...scopes.values()], question: input.query,
      runId: `knowledge_search_${crypto.randomUUID()}`, topK: input.limit,
      signal: input.signal, rerank: false, reranker: null, ftsCandidates: [],
      ordinalRangesByChunkIndexVariantId: input.ordinalRanges,
      sectionIdsByChunkIndexVariantId: input.sectionIds,
      requiredSectionIdsByChunkIndexVariantId: input.requiredSectionIds,
      embedTexts: embeddingRef && this.deps.embedTextsForModel ? async request => {
        const cached = await this.queryEmbeddingCache.getOrCreate({
          normalizedQuery: normalizeKnowledgeQuery(input.query), provider: embeddingRef.provider,
          modelId: embeddingRef.id, modelConfigurationRevision: this.getModelConfigurationRevision(embeddingRef), inputType: "query",
        }, async signal => {
          input.onRemoteCall();
          const result = await this.deps.embedTextsForModel!({ ...request, signal,
            texts: [normalizeKnowledgeQuery(input.query)], modelRef: embeddingRef, inputType: "query" });
          return assertEmbeddingBatch(result, 1).result;
        }, request.signal);
        if (cached.hit) input.onEmbeddingCacheHit();
        return cached.value;
      } : null,
    }).then(outcome => {
      if (embeddingRef && !this.deps.embedTextsForModel) {
        outcome.degraded.push(...[...scopes.values()].map(scope => ({ ...scope,
          reason: "KNOWLEDGE_VECTOR_NOT_READY" as const, detail: "configured embedding model is unavailable",
        })));
      }
      return outcome;
    });
  }

  async rerankCompiledCandidates(input: {
    candidates: IndexedKnowledgeChunk[]; modelRef: KnowledgeModelRef | null; query: string;
    signal?: AbortSignal; onRemoteCall: () => void;
  }) {
    if (!input.modelRef) return { candidates: input.candidates, rerankMs: 0 };
    if (!this.deps.rerankForModel) return { candidates: input.candidates, rerankMs: 0,
      rerankDegradeReason: "configured rerank model is unavailable; kept retrieval ranking" };
    try {
      return await this.rankCandidates({
        candidates: input.candidates, question: input.query, signal: input.signal,
        runId: `knowledge_rerank_${crypto.randomUUID()}`, rerank: true,
        reranker: async request => {
          input.onRemoteCall();
          const result = await this.deps.rerankForModel!({ ...request, modelRef: input.modelRef! });
          if (!result) throw new Error("configured rerank model returned no result");
          return result;
        },
      });
    } catch (error) {
      input.signal?.throwIfAborted();
      return { candidates: input.candidates, rerankMs: 0,
        rerankDegradeReason: `rerank failed: ${error instanceof Error ? error.message : String(error)}; kept RRF ranking` };
    }
  }

  extractEvidenceSpans(input: Parameters<EvidenceSpanExtractor["extract"]>[0]) {
    return new EvidenceSpanExtractor(this.deps.store).extract(input);
  }

  /**
   * 查询侧只读的就绪判定（§十一：查询不建索引）：逐 scope 解析 ChunkIndexVariant，
   * 只有 status=ready 且 block 指纹一致的变体参与 FTS/向量检索；其余显式降级
   * （missing/building/failed/stale），由调用方幂等入队后台构建。
   * 损坏（KNOWLEDGE_INDEX_INVALID）不在此判定——由检索执行处的自愈路径处理。
   */
  private splitScopesByReadiness(scopes: KnowledgeRetrievalScope[]): {
    ready: KnowledgeRetrievalScope[];
    degraded: KnowledgeDegradedRetrievalScope[];
  } {
    const ready: KnowledgeRetrievalScope[] = [];
    const degraded: KnowledgeDegradedRetrievalScope[] = [];
    for (const scope of scopes) {
      const variant = this.deps.indexStore.resolveChunkIndexVariant(
        scope.parseArtifactId,
        scope.chunkProfileHash,
      );
      const anchor = {
        parseArtifactId: scope.parseArtifactId,
        chunkProfileHash: scope.chunkProfileHash,
      };
      if (!variant) {
        degraded.push({ ...anchor, reason: "KNOWLEDGE_INDEX_MISSING" });
        continue;
      }
      if (variant.status === "failed") {
        degraded.push({ ...anchor, reason: "KNOWLEDGE_INDEX_FAILED" });
        continue;
      }
      if (variant.status !== "ready") {
        degraded.push({ ...anchor, reason: "KNOWLEDGE_INDEX_BUILDING", detail: `variant status: ${variant.status}` });
        continue;
      }
      if (variant.blockFingerprint !== scope.blockFingerprint) {
        degraded.push({ ...anchor, reason: "KNOWLEDGE_INDEX_BUILDING", detail: "index stale (source changed, rebuild pending)" });
        continue;
      }
      ready.push(scope);
    }
    return { ready, degraded };
  }

  /**
   * 纯 FTS 检索（只读）：只在就绪变体上搜索；无就绪变体返回空（降级已留痕）。
   * 损坏自愈（§十三 exception recovery）：search/读 chunk 抛 KNOWLEDGE_INDEX_INVALID
   * 时 reset 索引库一次，并把全部 scope 降级为 missing 交给后台摄入补齐——
   * 损坏后的内容重建属于 ingestion，查询不现场重建。
   */
  private retrieveFts(
    scopes: KnowledgeRetrievalScope[],
    question: string,
    limit: number,
    ordinalRangesByChunkIndexVariantId?: ReadonlyMap<string, KnowledgeOrdinalRange[]>,
  ): { fts: IndexedKnowledgeChunk[]; readyScopes: KnowledgeRetrievalScope[]; degraded: KnowledgeDegradedRetrievalScope[] } {
    // 底层 FTS/向量 search 的 sanity 上限 1000：无上限召回（retrieval_top_k NULL）
    // 的物理边界即此值，防病态全表膨胀。
    const searchLimit = Math.max(1, Math.min(limit, KNOWLEDGE_UNCAPPED_RETRIEVAL_LIMIT));
    const { ready, degraded } = this.splitScopesByReadiness(scopes);
    if (ready.length === 0) return { fts: [], readyScopes: ready, degraded };
    try {
      const fts = this.deps.indexStore.search({
        scopes: ready,
        query: question,
        limit: searchLimit,
        ...(ordinalRangesByChunkIndexVariantId ? { ordinalRangesByChunkIndexVariantId } : {}),
      });
      return { fts: fts.map(chunk => ({ ...chunk, channels: ["fts"] })), readyScopes: ready, degraded };
    } catch (error) {
      if (isKnowledgeError(error) && error.code === "KNOWLEDGE_INVALID_ARGUMENT") {
        // 查询无可检索词：合法空结果，不算降级。
        return { fts: [], readyScopes: ready, degraded };
      }
      if (!isKnowledgeError(error) || error.code !== "KNOWLEDGE_INDEX_INVALID") throw error;
      this.deps.indexStore.reset();
      return {
        fts: [],
        readyScopes: [],
        degraded: scopes.map(scope => ({
          parseArtifactId: scope.parseArtifactId,
          chunkProfileHash: scope.chunkProfileHash,
          reason: "KNOWLEDGE_INDEX_MISSING" as const,
          detail: "index reset after corruption; rebuild enqueued in background",
        })),
      };
    }
  }

  private async invokeEmbedding(
    request: Parameters<KnowledgeEmbedder>[0],
    embedder: KnowledgeEmbedder | null | undefined,
  ) {
    try {
      return await embedder?.(request) ?? null;
    } catch (error) {
      if (isAbortLike(error)) throw error;
      if (isKnowledgeError(error)) throw error;
      const cause = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
      throw new KnowledgeError(
        "KNOWLEDGE_RETRIEVAL_UNAVAILABLE",
        `Knowledge embedding request failed (${cause})`,
      );
    }
  }

  /**
   * 查询嵌入执行 + 期限竞速（KNOWLEDGE_EMBEDDING_DEADLINE_MS）：超时即 abort
   * 底层请求并抛 KnowledgeEmbeddingDeadlineError（调用方降级处理）；外部
   * signal 的 abort 原样穿透（用户取消语义）。竞速落败方的 rejection 就地
   * 吞掉，不允许变成 unhandled rejection。与 invokeRerankerWithDeadline 同构。
   */
  private async invokeEmbeddingWithDeadline(input: {
    runId: string;
    question: string;
    signal?: AbortSignal;
    embedder: KnowledgeEmbedder;
  }): Promise<KnowledgeEmbeddingResult | null> {
    const controller = new AbortController();
    const onExternalAbort = () => controller.abort();
    input.signal?.addEventListener("abort", onExternalAbort, { once: true });
    let timer: ReturnType<typeof setTimeout> | undefined;
    const deadline = new Promise<never>((_, reject) => {
      timer = setTimeout(() => {
        controller.abort();
        const error = new Error(`query embedding deadline exceeded after ${KNOWLEDGE_EMBEDDING_DEADLINE_MS}ms`);
        error.name = "KnowledgeEmbeddingDeadlineError";
        reject(error);
      }, KNOWLEDGE_EMBEDDING_DEADLINE_MS);
    });
    const attempt = Promise.resolve().then(() => this.invokeEmbedding({
      runId: input.runId,
      texts: [input.question],
      signal: controller.signal,
    }, input.embedder));
    deadline.catch(() => {});
    attempt.catch(() => {});
    try {
      return await Promise.race([attempt, deadline]);
    } finally {
      if (timer !== undefined) clearTimeout(timer);
      input.signal?.removeEventListener("abort", onExternalAbort);
    }
  }

  /**
   * 摄入管线的 embed 相位：FTS chunk 已就绪后执行向量嵌入（64/批），批级 checkpoint
   * 持久化（任务书 §十四/§十五，Phase 3）——每批嵌入成功后立即 upsertChunkVectorBatch
   * 落库，abort/进程退出/批次失败时已落库向量保留在 building 变体里，重试/重启
   * 只 diff 出缺失 chunk 续嵌，已付费向量绝不重嵌、绝不删除。
   *
   * 锚点是 chunkProfileHash 确定的 ChunkIndexVariant（与 chunk 相位 ensure 的变体
   * 同一身份）；向量侧再叠加模型身份为 VectorIndexVariant (civ, modelKey)。
   * 幂等：chunkFingerprint + 模型身份已 ready 时整体跳过（status "skipped"）。
   *
   * 模型身份（protocol/dimensions）只能由真实嵌入响应确认，因此无断点可恢复时
   * 仍保留「第一批探测」契约（已嵌入场景最多多跑一批）；唯一 building 断点变体
   * 的恢复则免探测——其记录的 model_key/dimensions 在 begin 当时已由真实响应确认，
   * 恢复后第一批响应会再次核对，身份不符（断点期间换了模型）时显式放弃旧变体
   * （failVectorVariantBuild，向量保留）并转入按新模型的构建，不混写。
   * embedder 返回 null（模型在检查与执行之间被摘除的竞态）→ status "unavailable"，
   * 由调用方落显式 pending_embedding（禁静默降级）。
   *
   * 并发防护（Phase 5 §十六）：摄入 worker 池按 ChunkIndexVariant 锁键串行化——
   * 同一 variant 的 job 不会并行执行（异键 job 并行，见 ingestion-service 的
   * keyed locking）；adapter 侧的指纹/维度/modelKey 守卫（KNOWLEDGE_CONFLICT）
   * 是交叉写的最后防线。
   */
  async embedArtifactForIngestion(input: {
    runId: string;
    parseArtifactId: string;
    /** chunk 相位 ensure 的 ChunkIndexVariant 身份（= chunkerConfigId 同源值）。 */
    chunkProfileHash: string;
    embedTexts: KnowledgeEmbedder;
    signal?: AbortSignal;
    /** 每批嵌入成功并持久化后回调（done/total 均为累计块数，含断点恢复的部分）；抛错按嵌入失败处理。 */
    onProgress?: (done: number, total: number) => void;
    modelInputMaxTokens?: number | null;
  }): Promise<{
    status: "embedded" | "skipped" | "unavailable";
    chunkCount: number;
    embeddingStats: KnowledgeIngestionEmbeddingStats;
  }> {
    input.signal?.throwIfAborted();
    const vectorIndex = this.deps.vectorIndex;
    if (!vectorIndex) {
      throw new KnowledgeError("KNOWLEDGE_RETRIEVAL_UNAVAILABLE", "Knowledge vector index is unavailable");
    }
    const chunkVariant = this.deps.indexStore.resolveChunkIndexVariant(
      input.parseArtifactId,
      input.chunkProfileHash,
    );
    if (!chunkVariant || chunkVariant.status !== "ready") {
      throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Indexed Knowledge artifact has no ready chunk variant to embed");
    }
    const chunkIndexVariantId = chunkVariant.id;
    const chunks = this.deps.indexStore.listVariantChunks(chunkIndexVariantId);
    if (chunks.length === 0) {
      throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Indexed Knowledge artifact has no chunks to embed");
    }
    if (input.modelInputMaxTokens != null && Number.isFinite(input.modelInputMaxTokens) && input.modelInputMaxTokens > 0
      && chunks.some(chunk => estimateTextTokens(chunk.text) > input.modelInputMaxTokens!)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge span exceeds the embedding model input limit");
    }
    const fingerprint = chunkFingerprint(chunks);
    const build = async (): Promise<{
      status: "embedded" | "skipped" | "unavailable";
      stats: KnowledgeIngestionEmbeddingStats;
    }> => {
      const stats: KnowledgeIngestionEmbeddingStats = {
        chunksNewlyEmbedded: 0,
        chunksResumedFromCheckpoint: 0,
        chunksReusedFromReadyVariant: 0,
        requestCount: 0,
        model: null,
        resetStaleVectors: false,
        abandonedStaleVariantId: null,
      };
      // 断点恢复锚：该 civ 下唯一 building 且指纹一致的变体。failed 变体只可能来自
      // 「断点期间换模型」的显式放弃（见下），不属于可恢复断点，不参与自动续嵌。
      const resumable = vectorIndex.listVariantsByChunkIndexVariant(chunkIndexVariantId)
        .filter(variant => variant.status === "building" && variant.chunkFingerprint === fingerprint);
      let model: VectorIndexModelIdentity | null = null;
      let variantId: string | null = null;
      let identityHint: { key: string; dimensions: number } | null = null;
      // 本轮已嵌入（在内存、待/已随批落库）与此前断点已落库的 chunk 集合。
      const embedded = new Map<string, number[]>();
      let persisted = new Set<string>();
      if (resumable.length === 1) {
        const variant = resumable[0];
        variantId = variant.id;
        identityHint = { key: variant.modelKey, dimensions: variant.dimensions };
        persisted = new Set(vectorIndex.listVariantVectorChunkIds(variantId));
        stats.chunksResumedFromCheckpoint = chunks.filter(chunk => persisted.has(chunk.id)).length;
        if (stats.chunksResumedFromCheckpoint > 0) {
          input.onProgress?.(stats.chunksResumedFromCheckpoint, chunks.length);
        }
      }
      const remaining = () => chunks.filter(chunk => !persisted.has(chunk.id) && !embedded.has(chunk.id));
      while (remaining().length > 0) {
        input.signal?.throwIfAborted();
        const batch = remaining().slice(0, 64);
        const response = await this.invokeEmbedding({
          runId: input.runId,
          texts: batch.map(chunk => chunk.text),
          signal: input.signal,
        }, input.embedTexts);
        // 请求可能在取消后才返回；停止后不再写入本批，也不派发后续批次。
        input.signal?.throwIfAborted();
        if (!response) return { status: "unavailable", stats };
        stats.requestCount += 1;
        const checked = assertEmbeddingBatch(response, batch.length, model ?? undefined);
        if (!model) {
          model = checked.model;
          stats.model = model;
          if (identityHint && (model.key !== identityHint.key || model.dimensions !== identityHint.dimensions)) {
            // 断点期间嵌入模型被更换：旧 building 变体显式落 failed（已落库向量保留，
            // 付费产物不删），转入按新模型身份的构建；本批响应对新变体有效，继续复用。
            vectorIndex.failVectorVariantBuild(variantId!);
            stats.abandonedStaleVariantId = variantId;
            variantId = null;
            identityHint = null;
            persisted = new Set();
            stats.chunksResumedFromCheckpoint = 0;
          }
          if (!variantId) {
            // 模型身份刚由真实响应确认：整体命中已 ready 变体 → 跳过。探测批的
            // 嵌入成本如实计入 requestCount；chunksNewlyEmbedded 只计落库写入。
            if (vectorIndex.hasArtifact({
              chunkIndexVariantId,
              parseArtifactId: input.parseArtifactId,
              chunkFingerprint: fingerprint,
              model,
            })) {
              stats.chunksReusedFromReadyVariant = chunks.length;
              return { status: "skipped", stats };
            }
            const begin = vectorIndex.beginVectorVariantBuild({
              chunkIndexVariantId,
              parseArtifactId: input.parseArtifactId,
              chunkFingerprint: fingerprint,
              model,
            });
            variantId = begin.vectorIndexVariantId;
            stats.resetStaleVectors = begin.resetStaleVectors;
            persisted = new Set(vectorIndex.listVariantVectorChunkIds(variantId));
            stats.chunksResumedFromCheckpoint = chunks.filter(chunk => persisted.has(chunk.id)).length;
          }
        }
        // 批级 checkpoint：本批立即单事务落库；中断后这些向量就是恢复锚点。
        vectorIndex.upsertChunkVectorBatch({
          vectorIndexVariantId: variantId!,
          chunkFingerprint: fingerprint,
          model,
          entries: batch.map((chunk, index) => ({
            chunkId: chunk.id,
            ordinal: chunk.ordinal,
            vector: checked.result.vectors[index],
          })),
        });
        batch.forEach((chunk, index) => embedded.set(chunk.id, checked.result.vectors[index]));
        stats.chunksNewlyEmbedded += batch.length;
        input.onProgress?.(chunks.length - remaining().length, chunks.length);
      }
      input.signal?.throwIfAborted();
      // 免探测恢复且零缺失时 model 为 null：complete 以 variant 行记录的身份校验。
      vectorIndex.completeVectorVariantBuild({
        vectorIndexVariantId: variantId!,
        chunkFingerprint: fingerprint,
        expectedChunkCount: chunks.length,
        ...(model ? { model } : {}),
      });
      return { status: "embedded", stats };
    };
    try {
      const outcome = await build();
      return { status: outcome.status, chunkCount: chunks.length, embeddingStats: outcome.stats };
    } catch (error) {
      input.signal?.throwIfAborted();
      // 向量库损坏：重建后重试一次（§十三 exception recovery，仅限摄入相位）。
      // 重建清空全部变体，第二趟 build 从头嵌入——stats 只报第二趟的真实开销。
      if (!isKnowledgeError(error) || error.code !== "KNOWLEDGE_INDEX_INVALID") throw error;
      vectorIndex.rebuild();
      const outcome = await build();
      return { status: outcome.status, chunkCount: chunks.length, embeddingStats: outcome.stats };
    }
  }

  // 统一搜索服务的模型组检索核心；范围来自冻结范围编译结果，只读指定索引。
  //
  // Phase 2（§十一/§十二）Query Plane 边界：本方法只读索引（read index / FTS /
  // vector search / RRF / rerank），绝不触发 chunk 重建或批量 embedding——
  // chunk 变体缺失/未就绪 → 该 scope 无结果 + KNOWLEDGE_INDEX_* 降级留痕；
  // 向量变体未就绪 → 跳过向量通道，retrievalMode 降 "fts" + KNOWLEDGE_VECTOR_NOT_READY
  // 留痕；缺失变体由调用方幂等入队后台构建（查询不等待）。仅存的写路径是
  // KNOWLEDGE_INDEX_INVALID 损坏自愈（reset/rebuild，§十三 exception recovery）。
  protected async retrieve(input: {
    /** 统一服务已完成全范围 FTS，向量分组传空列表避免重复执行。 */
    ftsCandidates?: IndexedKnowledgeChunk[];
    studioId: string;
    scopes: KnowledgeRetrievalScope[];
    question: string;
    runId: string;
    signal?: AbortSignal;
    /**
     * 候选数量上限；null/缺省 = 无上限（物理边界 KNOWLEDGE_UNCAPPED_RETRIEVAL_LIMIT）。
     * 底层 FTS/向量 search 各自夹到 sanity 上限。
     */
    topK?: number | null;
    /** 是否执行 rerank（默认 true）；false 时跳过即使 reranker 已接线。 */
    rerank?: boolean;
    /**
     * 本轮使用的 reranker（按笔记本引用构造的闭包）。undefined = 回落 deps.rerank
     * （全局路径，v8 后恒为不可解析 → null 跳过）；null = 显式不用。
     */
    reranker?: KnowledgeReranker | null;
    /**
     * 查询嵌入回调（v8 起由调用方按笔记本解析出的嵌入模型构造闭包传入）。
     * 缺省 → 纯 FTS（retrievalMode 如实 "fts"，禁静默换模型）。
     */
    embedTexts?: KnowledgeEmbedder | null;
    /**
     * ordinal 约束（Phase 8 broad §三十九）：chunkIndexVariantId → 允许参与检索的
     * ordinal 闭区间（section-constrained secondary retrieval）。FTS 在 SQL 层过滤；
     * 向量通道按同一约束后过滤（约束后为空的变体两通道都不产生结果）。
     */
    ordinalRangesByChunkIndexVariantId?: ReadonlyMap<string, KnowledgeOrdinalRange[]>;
    sectionIdsByChunkIndexVariantId?: ReadonlyMap<string, readonly string[]>;
    requiredSectionIdsByChunkIndexVariantId?: ReadonlyMap<string, readonly string[]>;
  }): Promise<{
    vectorBackend?: "hnsw" | "portable";
    vectorDegradedReasons?: string[];
    candidates: IndexedKnowledgeChunk[];
    retrievalMode: "fts" | "hybrid";
    retrievalModeRequested: "fts" | "hybrid";
    degraded: KnowledgeDegradedRetrievalScope[];
    /** 实际参与向量检索的变体身份（§六十七 EvidenceManifest；fts-only 为空）。 */
    searchedVectorVariants: SearchedVectorVariantIdentity[];
    /**
     * rerank 降级留痕（2026-08-30 延迟加固）：期限超时/传输类失败时携带，
     * 候选保持 RRF 名次；未尝试或成功重排时缺省。
     */
    rerankDegradeReason?: string;
    /** rerank 门控主动跳过留痕（2026-08-31 快速档）：头部清晰时跳过重排。 */
    /** 检索分段计时（2026-08-31 观测补齐）；未执行的段不携带。 */
    stageTimings?: KnowledgeRetrievalStageTimings;
  }> {
    const { scopes, question, runId, signal } = input;
    const retrievalModeRequested: "fts" | "hybrid" = input.embedTexts ? "hybrid" : "fts";
    const topK = input.topK == null || input.topK <= 0
      ? KNOWLEDGE_UNCAPPED_RETRIEVAL_LIMIT
      : Math.min(input.topK, KNOWLEDGE_UNCAPPED_RETRIEVAL_LIMIT);
    // 候选数量由统一搜索请求的有界额度决定，不另设旧详细模式固定候选池。
    const generationLimit = topK;
    const ordinalRanges = input.ordinalRangesByChunkIndexVariantId ?? null;
    // 分段计时（2026-08-31 观测补齐）：各段墙钟随结果携带，纯增量可选。
    const ftsStart = Date.now();
    const ftsResult = input.ftsCandidates === undefined
      ? this.retrieveFts(scopes, question, generationLimit, ordinalRanges ?? undefined)
      : (() => {
        const ready = this.splitScopesByReadiness(scopes);
        return { fts: input.ftsCandidates, readyScopes: ready.ready, degraded: ready.degraded };
      })();
    const ftsMs = Date.now() - ftsStart;
    let embedMs: number | undefined;
    let vectorMs: number | undefined;
    let fuseMs: number | undefined;
    const { fts, readyScopes, degraded } = ftsResult;
    if (!input.embedTexts || readyScopes.length === 0) {
      return {
        candidates: fts,
        retrievalMode: "fts",
        retrievalModeRequested,
        degraded,
        searchedVectorVariants: [],
        stageTimings: { ftsMs },
      };
    }

    // 查询嵌入带期限竞速（KNOWLEDGE_EMBEDDING_DEADLINE_MS）：超时/传输类失败/
    // 响应非法一律显式降级纯 FTS + KNOWLEDGE_EMBEDDING_FAILED 留痕（FTS 候选
    // 已算好、不再随异常丢弃——向量是检索增强层，失效不该炸掉整条检索，也
    // 不该挂满 engine 闭包 300s）；外部 signal abort 原样上抛（用户取消）。
    let embeddedQuestion: { result: KnowledgeEmbeddingResult; model: VectorIndexModelIdentity };
    const embedStart = Date.now();
    try {
      const questionEmbedding = await this.invokeEmbeddingWithDeadline({
        runId,
        question,
        signal,
        embedder: input.embedTexts,
      });
      embedMs = Date.now() - embedStart;
      if (!questionEmbedding) {
        // 查询嵌入在检查与执行之间变得不可用：显式降级纯 FTS 并留痕（禁静默）。
        return {
          candidates: fts,
          retrievalMode: "fts",
          retrievalModeRequested,
          degraded: [
            ...degraded,
            ...readyScopes.map(scope => ({
              parseArtifactId: scope.parseArtifactId,
              chunkProfileHash: scope.chunkProfileHash,
              reason: "KNOWLEDGE_VECTOR_NOT_READY" as const,
              detail: "query embedding unavailable",
            })),
          ],
          searchedVectorVariants: [],
          stageTimings: { ftsMs, embedMs },
        };
      }
      embeddedQuestion = assertEmbeddingBatch(questionEmbedding, 1);
    } catch (error) {
      embedMs = Date.now() - embedStart;
      // 外部 signal 的 abort = 用户取消，原样上抛；其余（期限超时/网络/HTTP/
      // 响应非法，含 deadline 竞速自身 abort 底层请求后逸出的 AbortError）一律
      // 降级纯 FTS + 留痕——向量是增强层，不炸检索。
      if (isAbortLike(error) && signal?.aborted) throw error;
      const cause = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
      return {
        candidates: fts,
        retrievalMode: "fts",
        retrievalModeRequested,
        degraded: [
          ...degraded,
          ...readyScopes.map(scope => ({
            parseArtifactId: scope.parseArtifactId,
            chunkProfileHash: scope.chunkProfileHash,
            reason: "KNOWLEDGE_EMBEDDING_FAILED" as const,
            detail: `query embedding request failed (${cause}); kept FTS candidates`,
          })),
        ],
        searchedVectorVariants: [],
        stageTimings: { ftsMs, embedMs },
      };
    }

    // 向量通道只读判定：逐就绪 scope 校验 VectorIndexVariant（viv = f(civ, modelKey)
    // 确定性派生，与写入侧同一算法）；指纹+维度命中才参与检索，否则降级留痕。
    const vectorStart = Date.now();
    const vectorIndex = this.deps.vectorIndex;
    const chunksById = new Map<string, StoredKnowledgeChunk>();
    const vectorVariantIds: string[] = [];
    const searchedVectorVariants: SearchedVectorVariantIdentity[] = [];
    const vectorDegraded: KnowledgeDegradedRetrievalScope[] = [];
    if (vectorIndex) {
      for (const scope of readyScopes) {
        const chunkIndexVariantId = knowledgeChunkIndexVariantId(scope.parseArtifactId, scope.chunkProfileHash);
        if (this.deps.vectorSearchBackend) {
          // 冻结的块变体由不可变原文和分块配置确定；热查询只读目录，命中后再取块正文。
          const id = knowledgeVectorIndexVariantId(chunkIndexVariantId, embeddedQuestion.model.key);
          const variant = vectorIndex.getVariant(id);
          if (variant?.status === "ready" && variant.parseArtifactId === scope.parseArtifactId
            && variant.chunkIndexVariantId === chunkIndexVariantId && variant.modelKey === embeddedQuestion.model.key
            && variant.dimensions === embeddedQuestion.model.dimensions) {
            vectorVariantIds.push(id);
            searchedVectorVariants.push({ ...scope, chunkIndexVariantId, vectorIndexVariantId: id });
          } else vectorDegraded.push({ ...scope, reason: "KNOWLEDGE_VECTOR_NOT_READY" });
          continue;
        }
        let chunks: StoredKnowledgeChunk[];
        try {
          chunks = this.deps.indexStore.listVariantChunks(chunkIndexVariantId);
        } catch (error) {
          // chunk 行损坏（spans 解析失败等）：与 search 同一自愈语义——reset 后
          // 全部 scope 交后台摄入补齐，本轮返回已拿到的降级清单。
          if (!isKnowledgeError(error) || error.code !== "KNOWLEDGE_INDEX_INVALID") throw error;
          this.deps.indexStore.reset();
          return {
            candidates: [],
            retrievalMode: "fts",
            retrievalModeRequested,
            degraded: scopes.map(item => ({
              parseArtifactId: item.parseArtifactId,
              chunkProfileHash: item.chunkProfileHash,
              reason: "KNOWLEDGE_INDEX_MISSING" as const,
              detail: "index reset after corruption; rebuild enqueued in background",
            })),
            searchedVectorVariants: [],
          };
        }
        chunks.forEach(chunk => chunksById.set(chunk.id, chunk));
        const fingerprint = chunkFingerprint(chunks);
        if (vectorIndex.hasArtifact({
          chunkIndexVariantId,
          parseArtifactId: scope.parseArtifactId,
          chunkFingerprint: fingerprint,
          model: embeddedQuestion.model,
        })) {
          const vectorIndexVariantId = knowledgeVectorIndexVariantId(chunkIndexVariantId, embeddedQuestion.model.key);
          vectorVariantIds.push(vectorIndexVariantId);
          searchedVectorVariants.push({
            parseArtifactId: scope.parseArtifactId,
            chunkProfileHash: scope.chunkProfileHash,
            chunkIndexVariantId,
            vectorIndexVariantId,
          });
        } else {
          vectorDegraded.push({
            parseArtifactId: scope.parseArtifactId,
            chunkProfileHash: scope.chunkProfileHash,
            reason: "KNOWLEDGE_VECTOR_NOT_READY",
          });
        }
      }
    } else {
      vectorDegraded.push(...readyScopes.map(scope => ({
        parseArtifactId: scope.parseArtifactId,
        chunkProfileHash: scope.chunkProfileHash,
        reason: "KNOWLEDGE_VECTOR_NOT_READY" as const,
        detail: "vector index unavailable",
      })));
    }
    const allDegraded = [...degraded, ...vectorDegraded];
    if (vectorVariantIds.length === 0) {
      return {
        candidates: fts,
        retrievalMode: "fts",
        retrievalModeRequested,
        degraded: allDegraded,
        searchedVectorVariants: [],
      };
    }

    let vectorRows;
    let exactVectorRows: ReturnType<VectorIndexAdapter["search"]> = [];
    let vectorBackend: "hnsw" | "portable" = "portable";
    let vectorDegradedReasons: string[] = [];
    try {
      const searchInput = {
        vectorIndexVariantIds: vectorVariantIds,
        model: embeddedQuestion.model,
        queryVector: embeddedQuestion.result.vectors[0],
        limit: Math.max(1, Math.min(generationLimit, KNOWLEDGE_UNCAPPED_RETRIEVAL_LIMIT)),
      };
      if (this.deps.vectorSearchBackend) {
        const result = await searchVectorBackend(this.deps.vectorSearchBackend, searchInput);
        signal?.throwIfAborted();
        vectorRows = result.results; vectorBackend = result.vectorBackend; vectorDegradedReasons = result.degradedReasons;
        for (const variant of searchedVectorVariants) {
          const ordinals = vectorRows.filter(row => row.vectorIndexVariantId === variant.vectorIndexVariantId).map(row => row.ordinal);
          if (ordinals.length) for (const chunk of this.deps.indexStore.readVariantChunks(variant.chunkIndexVariantId, ordinals)) chunksById.set(chunk.id, chunk);
        }
      } else vectorRows = vectorIndex!.search(searchInput);
      if (input.sectionIdsByChunkIndexVariantId) {
        const chunkIds = searchedVectorVariants.flatMap(variant => {
          const sectionIds = input.sectionIdsByChunkIndexVariantId!.get(variant.chunkIndexVariantId);
          return sectionIds?.length ? this.deps.indexStore.listSectionChunkIds({ chunkIndexVariantId: variant.chunkIndexVariantId, sectionIds }) : [];
        });
        if (chunkIds.length) {
          // 章节补查只按已选片段标识读向量，不读取整份资料或整个工作室的向量。
          exactVectorRows = vectorIndex!.search({ ...searchInput, chunkIds: [...new Set(chunkIds)] });
          for (const variant of searchedVectorVariants) {
            const ordinals = exactVectorRows.filter(row => row.vectorIndexVariantId === variant.vectorIndexVariantId).map(row => row.ordinal);
            if (ordinals.length) for (const chunk of this.deps.indexStore.readVariantChunks(variant.chunkIndexVariantId, ordinals)) chunksById.set(chunk.id, chunk);
          }
        }
      }
    } catch (error) {
      signal?.throwIfAborted();
      if (this.deps.vectorSearchBackend || !isKnowledgeError(error) || error.code !== "KNOWLEDGE_INDEX_INVALID") {
        // 向量库检索的意外错误（非损坏自愈路径）：向量通道本轮不可用——降级
        // 纯 FTS + VECTOR_NOT_READY 留痕，不炸检索（与查询嵌入失败降级同纪律）。
        const cause = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
        return {
          candidates: fts,
          retrievalMode: "fts",
          retrievalModeRequested,
          degraded: [
            ...allDegraded,
            ...readyScopes
              .filter(scope => !vectorDegraded.some(item => item.parseArtifactId === scope.parseArtifactId))
              .map(scope => ({
                parseArtifactId: scope.parseArtifactId,
                chunkProfileHash: scope.chunkProfileHash,
                reason: "KNOWLEDGE_VECTOR_NOT_READY" as const,
                detail: `vector search failed (${cause}); kept FTS candidates`,
              })),
          ],
          searchedVectorVariants: [],
        };
      }
      // 向量库损坏自愈（§十三）：rebuild 后变体必然全失（缓存级重建），本轮降级
      // 纯 FTS 并把原就绪 scope 记为 VECTOR_NOT_READY，由后台摄入重建向量。
      vectorIndex!.rebuild();
      return {
        candidates: fts,
        retrievalMode: "fts",
        retrievalModeRequested,
        degraded: [
          ...allDegraded,
          ...readyScopes
            .filter(scope => !vectorDegraded.some(item => item.parseArtifactId === scope.parseArtifactId))
            .map(scope => ({
              parseArtifactId: scope.parseArtifactId,
              chunkProfileHash: scope.chunkProfileHash,
              reason: "KNOWLEDGE_VECTOR_NOT_READY" as const,
              detail: "vector index rebuilt after corruption",
            })),
        ],
        searchedVectorVariants: [],
      };
    }
    const materialize = (rows: ReturnType<VectorIndexAdapter["search"]>) => rows
      // §三十九 section 约束对向量通道同样生效：约束了区间集合的变体只保留
      // 落在区间内的命中（未约束的变体不受影响）。
      .filter(row => {
        const chunk = chunksById.get(row.chunkId);
        const required = chunk && input.requiredSectionIdsByChunkIndexVariantId?.get(chunk.chunkIndexVariantId);
        if (required && (!chunk.sectionId || !required.includes(chunk.sectionId))) return false;
        if (!ordinalRanges) return true;
        if (!chunk) return true;
        const ranges = ordinalRanges.get(chunk.chunkIndexVariantId);
        if (!ranges) return true;
        return ranges.some(([low, high]) => chunk.ordinal >= low && chunk.ordinal <= high);
      })
      .map(row => {
        const chunk = chunksById.get(row.chunkId);
        if (!chunk || chunk.parseArtifactId !== row.parseArtifactId) {
          throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Knowledge vector index references an unknown chunk");
        }
        return { ...chunk, score: row.score, channels: ["vector"] as Array<"vector"> };
      });
    const semantic = materialize(vectorRows);
    const vector = exactVectorRows.length ? fuseNotebookRankings([semantic, materialize(exactVectorRows)], true).map(row => row.chunk) : semantic;
    let candidates;
    {
      vectorMs = Date.now() - vectorStart;
      const fuseStart = Date.now();
      candidates = fuseCandidates(fts, vector, topK);
      fuseMs = Date.now() - fuseStart;
    }
    const ranked = await this.rankCandidates({ ...input, candidates });
    candidates = ranked.candidates;
    const { rerankMs, rerankDegradeReason } = ranked;
    return {
      candidates,
      retrievalMode: "hybrid",
      vectorBackend, vectorDegradedReasons,
      retrievalModeRequested,
      degraded: allDegraded,
      searchedVectorVariants,
      ...(rerankDegradeReason ? { rerankDegradeReason } : {}),
      stageTimings: {
        ftsMs,
        ...(embedMs != null ? { embedMs } : {}),
        ...(vectorMs != null ? { vectorMs } : {}),
        ...(fuseMs != null ? { fuseMs } : {}),
        ...(rerankMs != null ? { rerankMs } : {}),
      },
    };
  }

  private async rankCandidates(input: {
    candidates: IndexedKnowledgeChunk[]; question: string; runId: string; signal?: AbortSignal;
    rerank?: boolean; reranker?: KnowledgeReranker | null;
  }) {
    let candidates = input.candidates;
    const { question, runId, signal } = input;
    // rerank 输入防护：超出 KNOWLEDGE_RERANK_MAX_DOCS 的尾部保持 RRF 名次。
    const rerankCandidates = candidates.length > KNOWLEDGE_RERANK_MAX_DOCS
      ? candidates.slice(0, KNOWLEDGE_RERANK_MAX_DOCS)
      : candidates;
    const rerankTail = candidates.length > KNOWLEDGE_RERANK_MAX_DOCS
      ? candidates.slice(KNOWLEDGE_RERANK_MAX_DOCS)
      : [];
    const reranker = input.reranker !== undefined ? input.reranker : this.deps.rerank;
    let rerankDegradeReason: string | undefined;
    const rerankStart = Date.now();
    if (input.rerank !== false && rerankCandidates.length > 0 && reranker) {
      let reranked;
      try {
        reranked = await this.invokeRerankerWithDeadline({
          reranker,
          runId,
          question,
          documents: rerankCandidates.map(candidate => candidate.text),
          signal,
        });
      } catch (error) {
        if (isAbortLike(error)) throw error;
        if (isKnowledgeError(error)) throw error;
        // 期限超时/传输类失败：重排是精排增强层，降级保 RRF 名次并显式留痕
        // （见 KNOWLEDGE_RERANK_DEADLINE_MS docstring），不炸整个检索。
        const cause = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
        rerankDegradeReason = `rerank degraded (${cause}); kept RRF ranking`;
      }
      if (reranked) {
        if (
          !Array.isArray(reranked.results)
          || reranked.results.length !== rerankCandidates.length
          || new Set(reranked.results.map(entry => entry.index)).size !== rerankCandidates.length
          || reranked.results.some(entry => (
            !Number.isSafeInteger(entry.index)
            || entry.index < 0
            || entry.index >= rerankCandidates.length
            || typeof entry.score !== "number"
            || !Number.isFinite(entry.score)
          ))
        ) {
          throw new KnowledgeError("KNOWLEDGE_RETRIEVAL_UNAVAILABLE", "Knowledge rerank response is invalid");
        }
        candidates = [
          ...reranked.results.map(entry => ({ ...rerankCandidates[entry.index], score: entry.score })),
          ...rerankTail,
        ];
      }
    }
    const rerankMs = Date.now() - rerankStart;
    return { candidates, rerankDegradeReason, rerankMs };
  }

  /**
   * rerank 执行 + 期限竞速（KNOWLEDGE_RERANK_DEADLINE_MS）：超时即 abort 底层
   * 请求并抛 KnowledgeRerankDeadlineError（调用方降级处理）；外部 signal 的
   * abort 原样穿透（用户取消语义）。竞速落败方的 rejection 就地吞掉，不允许
   * 变成 unhandled rejection。
   */
  private async invokeRerankerWithDeadline(input: {
    reranker: KnowledgeReranker;
    runId: string;
    question: string;
    documents: string[];
    signal?: AbortSignal;
  }): Promise<{ results: Array<{ index: number; score: number }> } | null> {
    const deadlineMs = KNOWLEDGE_RERANK_DEADLINE_MS;
    const controller = new AbortController();
    const onExternalAbort = () => controller.abort();
    input.signal?.addEventListener("abort", onExternalAbort, { once: true });
    let timer: ReturnType<typeof setTimeout> | undefined;
    const deadline = new Promise<never>((_, reject) => {
      timer = setTimeout(() => {
        controller.abort();
        const error = new Error(`rerank deadline exceeded after ${deadlineMs}ms`);
        error.name = "KnowledgeRerankDeadlineError";
        reject(error);
      }, deadlineMs);
    });
    const attempt = Promise.resolve().then(() => input.reranker({
      runId: input.runId,
      query: input.question,
      documents: input.documents,
      topN: input.documents.length,
      signal: controller.signal,
    }));
    deadline.catch(() => {});
    attempt.catch(() => {});
    try {
      return await Promise.race([attempt, deadline]);
    } finally {
      if (timer !== undefined) clearTimeout(timer);
      input.signal?.removeEventListener("abort", onExternalAbort);
    }
  }

}
