import crypto from "node:crypto";

import {
  buildKnowledgeChunks,
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
} from "./knowledge-index-store.ts";
import { KnowledgeStore } from "./knowledge-store.ts";
import {
  resolveEffectiveChunkTargetChars,
  resolveNotebookConfig,
} from "./knowledge-store.ts";
import type { KnowledgeBlock, KnowledgeIngestionEmbeddingStats, KnowledgeModelRef, KnowledgeSource } from "./types.ts";
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
  candidates: NotebookRetrievalChunk[];
  sources: NotebookRetrievalSource[];
  retrievalMode: "fts" | "hybrid";
  /** 本轮请求的检索模式（任一笔记本配置了可路由嵌入模型即 "hybrid"；§十二留痕）。 */
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
  const fusionLimit = Math.min(limit, KNOWLEDGE_FUSION_BUDGET);
  const fused = new Map<string, { chunk: IndexedKnowledgeChunk; score: number }>();
  const add = (chunks: IndexedKnowledgeChunk[]) => {
    chunks.forEach((chunk, index) => {
      const current = fused.get(chunk.id) || { chunk, score: 0 };
      current.score += 1 / (KNOWLEDGE_RRF_K + index + 1);
      fused.set(chunk.id, current);
    });
  };
  add(fts);
  add(vector);
  return [...fused.values()]
    .sort((left, right) => (
      right.score - left.score
      || left.chunk.parseArtifactId.localeCompare(right.chunk.parseArtifactId)
      || left.chunk.ordinal - right.chunk.ordinal
    ))
    .slice(0, fusionLimit)
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
function fuseNotebookRankings(
  rankings: IndexedKnowledgeChunk[][],
): Array<{ chunk: IndexedKnowledgeChunk; notebookIndex: number }> {
  const fused = new Map<string, { chunk: IndexedKnowledgeChunk; notebookIndex: number; score: number }>();
  rankings.forEach((ranking, notebookIndex) => {
    ranking.forEach((chunk, rank) => {
      const contribution = 1 / (KNOWLEDGE_RRF_K + rank + 1);
      const current = fused.get(chunk.id);
      if (current) {
        current.score += contribution;
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
    // §二十六 fusionBudget：跨笔记本融合池同样封顶（预算链独立于 topK 生效）。
    .slice(0, KNOWLEDGE_FUSION_BUDGET)
    .map(entry => ({ chunk: entry.chunk, notebookIndex: entry.notebookIndex }));
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
 * Retrieval Candidate Budgets（任务书 §二十六，Phase 8）：候选生成预算与
 * Context Injection Budget（budgetTokens，注入层）分离。topK（含 NULL→1000 的
 * "无上限"列语义）不再作为覆盖机制——预算链对每一步独立封顶，topK 只在预算
 * 之内进一步收紧（min 取小）。留痕计数见 injector 的 coverage footprint stats。
 */
export const KNOWLEDGE_CANDIDATE_GENERATION_BUDGET = 60;
/** RRF 融合池上限：fuseCandidates / fuseNotebookRankings / injector 跨子查询融合的输出封顶。 */
export const KNOWLEDGE_FUSION_BUDGET = 60;
/** 进入证据组装（蒸馏 / 注入循环）的锚点候选上限（injector 消费）。 */
export const KNOWLEDGE_EVIDENCE_BUDGET = 40;

export class KnowledgeQueryService {
  private readonly deps: {
    store: KnowledgeStore;
    indexStore: KnowledgeIndexStore;
    vectorIndex?: VectorIndexAdapter | null;
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
    /** 查嵌入模型上下文窗口（token 数）：与摄入侧同源，用于自动分块尺寸解析。 */
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
      return { fts, readyScopes: ready, degraded };
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
      throw new KnowledgeError("KNOWLEDGE_RETRIEVAL_UNAVAILABLE", "Knowledge embedding request failed");
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
  }): Promise<{
    status: "embedded" | "skipped" | "unavailable";
    chunkCount: number;
    embeddingStats: KnowledgeIngestionEmbeddingStats;
  }> {
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
        const batch = remaining().slice(0, 64);
        const response = await this.invokeEmbedding({
          runId: input.runId,
          texts: batch.map(chunk => chunk.text),
          signal: input.signal,
        }, input.embedTexts);
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
      // 向量库损坏：重建后重试一次（§十三 exception recovery，仅限摄入相位）。
      // 重建清空全部变体，第二趟 build 从头嵌入——stats 只报第二趟的真实开销。
      if (!isKnowledgeError(error) || error.code !== "KNOWLEDGE_INDEX_INVALID") throw error;
      vectorIndex.rebuild();
      const outcome = await build();
      return { status: outcome.status, chunkCount: chunks.length, embeddingStats: outcome.stats };
    }
  }

  /**
   * 检索锚点解析（纯函数式，只读）：逐 artifact 由 blocks 派发分块策略（策略依赖
   * artifact 内容，同一笔记本的不同源可能落在不同策略），锚点 chunkProfileHash =
   * resolveKnowledgeChunkerConfig(...).configId——与摄入侧同一解析链（同一 configId），
   * 两侧 targetChars 均经 resolveEffectiveChunkTargetChars 同源解析，不会漂移。
   *
   * Phase 2（§八十九.4）：查询不得改变 ChunkProfile——此处不再调
   * resolveNotebookRetrievalProfile（find-or-create + 绑定切换是写操作）；
   * Notebook → RetrievalProfile 的惰性建绑前移到摄入侧（enqueue/chunk 相位）。
   */
  private resolveRetrievalScopes(input: {
    studioId: string;
    artifactIds: string[];
    chunkTargetChars?: number | null;
  }): KnowledgeRetrievalScope[] {
    const seen = new Set<string>();
    const scopes: KnowledgeRetrievalScope[] = [];
    for (const parseArtifactId of input.artifactIds) {
      const blocks = this.deps.store.listArtifactBlocks({ studioId: input.studioId, parseArtifactId });
      const config = resolveKnowledgeChunkerConfig(blocks, {
        ...(input.chunkTargetChars != null ? { targetChars: input.chunkTargetChars } : {}),
      });
      const chunkProfileHash = config.configId;
      const key = `${parseArtifactId}\0${chunkProfileHash}`;
      if (seen.has(key)) continue;
      seen.add(key);
      scopes.push({
        parseArtifactId,
        chunkProfileHash,
        blockFingerprint: knowledgeBlockFingerprint(blocks),
      });
    }
    return scopes;
  }

  // FTS + 向量 RRF + 可选 rerank 的检索核心。retrieveForNotebooks（笔记本引用注入）
  // 与 knowledge_read 工具（单源检索）复用本方法；topK/rerank 由调用方按各自配置解析。
  // 检索锚是 scopes：(parseArtifactId, chunkProfileHash, blockFingerprint)，由调用方经
  // resolveRetrievalScopes 逐 artifact 纯解析（只读，不建绑不建索引）。
  //
  // Phase 2（§十一/§十二）Query Plane 边界：本方法只读索引（read index / FTS /
  // vector search / RRF / rerank），绝不触发 chunk 重建或批量 embedding——
  // chunk 变体缺失/未就绪 → 该 scope 无结果 + KNOWLEDGE_INDEX_* 降级留痕；
  // 向量变体未就绪 → 跳过向量通道，retrievalMode 降 "fts" + KNOWLEDGE_VECTOR_NOT_READY
  // 留痕；缺失变体由调用方幂等入队后台构建（查询不等待）。仅存的写路径是
  // KNOWLEDGE_INDEX_INVALID 损坏自愈（reset/rebuild，§十三 exception recovery）。
  protected async retrieve(input: {
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
  }): Promise<{
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
  }> {
    const { scopes, question, runId, signal } = input;
    const retrievalModeRequested: "fts" | "hybrid" = input.embedTexts ? "hybrid" : "fts";
    const topK = input.topK == null || input.topK <= 0
      ? KNOWLEDGE_UNCAPPED_RETRIEVAL_LIMIT
      : Math.min(input.topK, KNOWLEDGE_UNCAPPED_RETRIEVAL_LIMIT);
    // §二十六 candidateGenerationBudget：每查询每通道（FTS/向量）的候选生成上限，
    // 独立于 topK 生效（topK=NULL→1000 不再作为覆盖机制，只在预算内进一步收紧）。
    const generationLimit = Math.min(topK, KNOWLEDGE_CANDIDATE_GENERATION_BUDGET);
    const ordinalRanges = input.ordinalRangesByChunkIndexVariantId ?? null;
    const ftsResult = this.retrieveFts(scopes, question, generationLimit, ordinalRanges ?? undefined);
    const { fts, readyScopes, degraded } = ftsResult;
    if (!input.embedTexts || readyScopes.length === 0) {
      return { candidates: fts, retrievalMode: "fts", retrievalModeRequested, degraded, searchedVectorVariants: [] };
    }

    const questionEmbedding = await this.invokeEmbedding({ runId, texts: [question], signal }, input.embedTexts);
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
      };
    }
    const embeddedQuestion = assertEmbeddingBatch(questionEmbedding, 1);

    // 向量通道只读判定：逐就绪 scope 校验 VectorIndexVariant（viv = f(civ, modelKey)
    // 确定性派生，与写入侧同一算法）；指纹+维度命中才参与检索，否则降级留痕。
    const vectorIndex = this.deps.vectorIndex;
    const chunksById = new Map<string, StoredKnowledgeChunk>();
    const vectorVariantIds: string[] = [];
    const searchedVectorVariants: SearchedVectorVariantIdentity[] = [];
    const vectorDegraded: KnowledgeDegradedRetrievalScope[] = [];
    if (vectorIndex) {
      for (const scope of readyScopes) {
        const chunkIndexVariantId = knowledgeChunkIndexVariantId(scope.parseArtifactId, scope.chunkProfileHash);
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
    try {
      vectorRows = vectorIndex!.search({
        vectorIndexVariantIds: vectorVariantIds,
        model: embeddedQuestion.model,
        queryVector: embeddedQuestion.result.vectors[0],
        limit: Math.max(1, Math.min(generationLimit, KNOWLEDGE_UNCAPPED_RETRIEVAL_LIMIT)),
      });
    } catch (error) {
      if (!isKnowledgeError(error) || error.code !== "KNOWLEDGE_INDEX_INVALID") throw error;
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
    const vector = vectorRows
      // §三十九 section 约束对向量通道同样生效：约束了区间集合的变体只保留
      // 落在区间内的命中（未约束的变体不受影响）。
      .filter(row => {
        if (!ordinalRanges) return true;
        const chunk = chunksById.get(row.chunkId);
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
        return { ...chunk, score: row.score };
      });
    let candidates = fuseCandidates(fts, vector, topK);
    // rerank 输入防护：超出 KNOWLEDGE_RERANK_MAX_DOCS 的尾部保持 RRF 名次。
    const rerankCandidates = candidates.length > KNOWLEDGE_RERANK_MAX_DOCS
      ? candidates.slice(0, KNOWLEDGE_RERANK_MAX_DOCS)
      : candidates;
    const rerankTail = candidates.length > KNOWLEDGE_RERANK_MAX_DOCS
      ? candidates.slice(KNOWLEDGE_RERANK_MAX_DOCS)
      : [];
    const reranker = input.reranker !== undefined ? input.reranker : this.deps.rerank;
    let rerankDegradeReason: string | undefined;
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
    return {
      candidates,
      retrievalMode: "hybrid",
      retrievalModeRequested,
      degraded: allDegraded,
      searchedVectorVariants,
      ...(rerankDegradeReason ? { rerankDegradeReason } : {}),
    };
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
    const controller = new AbortController();
    const onExternalAbort = () => controller.abort();
    input.signal?.addEventListener("abort", onExternalAbort, { once: true });
    let timer: ReturnType<typeof setTimeout> | undefined;
    const deadline = new Promise<never>((_, reject) => {
      timer = setTimeout(() => {
        controller.abort();
        const error = new Error(`rerank deadline exceeded after ${KNOWLEDGE_RERANK_DEADLINE_MS}ms`);
        error.name = "KnowledgeRerankDeadlineError";
        reject(error);
      }, KNOWLEDGE_RERANK_DEADLINE_MS);
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

  /**
   * knowledge_read 工具的单源检索入口：按显式 artifact 集合执行同一检索核心。
   * 只读、studio 隔离由调用方先解析 artifact 边界保证。索引变体未就绪时
   * 显式降级（degraded 清单），后台补齐入队由调用方（工具侧有 sourceId）执行。
   */
  async retrieveForArtifacts(input: {
    studioId: string;
    artifactIds: string[];
    question: string;
    topK?: number;
    rerank?: boolean;
    signal?: AbortSignal;
    /** owning notebook：检索锚按笔记本生效分块配置解析（缺省 = 无笔记本上下文）。 */
    notebookId?: string | null;
    /** owning notebook 的嵌入引用：按同一模型路由查询向量；null → 纯 FTS。 */
    embeddingModelRef?: KnowledgeModelRef | null;
    /** owning notebook 的生效分块尺寸：与摄入侧同 configId（缺省 = chunker 默认）。 */
    chunkTargetChars?: number | null;
    /** owning notebook 的重排引用：按引用路由执行；null/缺省 → 不重排。 */
    rerankModelRef?: KnowledgeModelRef | null;
  }): Promise<{
    candidates: IndexedKnowledgeChunk[];
    retrievalMode: "fts" | "hybrid";
    retrievalModeRequested: "fts" | "hybrid";
    degraded: KnowledgeDegradedRetrievalScope[];
  }> {
    if (!Array.isArray(input.artifactIds) || input.artifactIds.length === 0) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge retrieval scope must not be empty");
    }
    const embedTexts = input.embeddingModelRef && this.deps.embedTextsForModel
      ? ((request: { runId: string; texts: string[]; signal?: AbortSignal }) =>
        this.deps.embedTextsForModel!({
          runId: request.runId,
          texts: request.texts,
          signal: request.signal,
          modelRef: input.embeddingModelRef!,
        })) as KnowledgeEmbedder
      : null;
    const reranker = input.rerankModelRef && this.deps.rerankForModel
      ? ((request: { runId: string; query: string; documents: string[]; topN: number; signal?: AbortSignal }) =>
        this.deps.rerankForModel!({
          runId: request.runId,
          query: request.query,
          documents: request.documents,
          topN: request.topN,
          signal: request.signal,
          modelRef: input.rerankModelRef!,
        })) as KnowledgeReranker
      : null;
    return this.retrieve({
      studioId: input.studioId,
      scopes: this.resolveRetrievalScopes({
        studioId: input.studioId,
        artifactIds: input.artifactIds,
        chunkTargetChars: input.chunkTargetChars,
      }),
      question: input.question,
      runId: `kbread_${crypto.randomUUID()}`,
      signal: input.signal,
      ...(input.topK != null ? { topK: input.topK } : {}),
      ...(input.rerank != null ? { rerank: input.rerank } : {}),
      reranker,
      embedTexts,
    });
  }

  /**
   * 主界面笔记本引用注入的检索入口（Phase 8）。
   *
   * artifact 硬边界：被引用笔记本内全部 active 源的最新 ready parse artifact——
   * 边界来自 store 的 studio 过滤 SQL（listNotebookSources 的 JOIN 链），未引用
   * 笔记本与其他 studio 的源不可能进入候选。
   * 检索锚（Phase 2 起只读）：逐 artifact 纯解析 (parseArtifactId, chunkProfileHash)
   * （与摄入侧同一 configId 解析链），查询不改变 ChunkProfile/不建索引
   * （§八十九.4/§十一）；变体缺失/未就绪 → per-scope 显式降级（degraded 清单）
   * 并经 deps.requestVariantBuild 幂等入队后台构建（§十二，查询不等待）。
   * topK/rerank 按笔记本各自配置解析（resolveNotebookConfig：仅笔记本列；topK
   * NULL = 无上限召回）；多笔记本合并采用 rank-based RRF 融合（§二十五）：各
   * 笔记本的名次序列（已各自 rerank 或内部 RRF 名次）作为独立序列，按与
   * fuseCandidates 相同的公式（k=60）跨序列融合——跨笔记本的分数不可直接比较
   * （rerank 分数按各自模型归一、cosine 按各自嵌入模型），融合只消费名次，
   * 多序列同时命中的 chunk 靠贡献求和自然靠前。
   *
   * 各笔记本的 retrieve 并行执行（降时延；串行最坏 = 笔记本数 × 检索耗时）；
   * rerank 闭包按笔记本注入（各自捕获本笔记本解析出的 rerankModelRef，无共享
   * 可变状态），并发互不影响。确定性合并：Promise.all 按 notebookIds 顺序映射
   * 结果，合并阶段只消费各笔记本的名次序列做 RRF 融合 + 去重（并列按 notebook
   * 序 / artifact / ordinal 稳定排序），与各检索的完成顺序无关；任一笔记本
   * 检索抛错仍整体抛出（错误语义与串行版一致，由子查询层分类留痕）。
   */
  async retrieveForNotebooks(input: {
    studioId: string;
    notebookIds: string[];
    question: string;
    topK?: number;
    rerank?: boolean;
    signal?: AbortSignal;
    /**
     * KnowledgeTurnScope 冻结集合（§四十三，Phase 4）：sourceId → 冻结的
     * snapshot/artifact。给定时本轮检索只覆盖冻结版本——watcher 在轮内产生的
     * 新 snapshot/artifact 不参与；缺省保持旧行为（各源最新 ready artifact）。
     */
    frozenArtifacts?: ReadonlyMap<string, {
      contentSnapshotId: string;
      parseArtifactId: string | null;
    }>;
    /**
     * source 约束（Phase 8 broad §三十八，Source Coverage Floor）：只检索这些
     * sourceId（零命中源的 constrained secondary retrieval）。缺省 = 全部被引源。
     */
    sourceIds?: string[];
    /**
     * section 约束（Phase 8 broad §三十九，Section Coverage）：sourceId → 该源内
     * 限定检索的 section 键（knowledgeSectionKeyOf(headingPath)）。给定源的检索
     * 只命中落入选中 section 的 chunk（FTS 在 SQL 层过滤，向量通道后过滤）。
     */
    sectionsBySourceId?: ReadonlyMap<string, string[]>;
  }): Promise<RetrieveForNotebooksResult> {
    const studioId = input.studioId;
    // locator 索引缓存（跨 notebook/源复用）：section 约束解析与源清单/邻接注解共用。
    const locatorCache = new Map<string, Map<string, KnowledgeBlockLocator>>();

    const notebookResults = await Promise.all(input.notebookIds.map(async (notebookId) => {
      const notebook = this.deps.store.getNotebook({ studioId, notebookId });
      const allEntries = this.deps.store.listNotebookSources({ studioId, notebookId });
      // scope 冻结：逐 membership 以冻结的 artifact 替换「最新」视图；不在冻结
      // 集合内的源（membership 在冻结后被移除等）本轮排除。冻结 artifact 的状态
      // 按当前行读取（同一 artifact 身份；parsing→ready 的推进对本轮可见）。
      const thawedEntries = input.frozenArtifacts
        ? allEntries.flatMap((entry) => {
          const frozen = input.frozenArtifacts!.get(entry.source.id);
          if (!frozen) return [];
          const parseArtifact = frozen.parseArtifactId
            ? this.deps.store.getParseArtifact({ studioId, parseArtifactId: frozen.parseArtifactId })
            : null;
          return [{ ...entry, parseArtifact }];
        })
        : allEntries;
      // §三十八 source 约束：Source Coverage Floor 的二次检索只覆盖指定源。
      const entries = input.sourceIds
        ? thawedEntries.filter(entry => input.sourceIds!.includes(entry.source.id))
        : thawedEntries;
      const resolved = resolveNotebookConfig(
        this.deps.store.getNotebookConfig({ studioId, notebookId }),
      );
      const topK = input.topK ?? resolved.retrievalTopK;
      // rerank 路由（v8）：笔记本显式引用 → 按引用执行（配置不可解析由回调侧
      // 记日志并返回 null，检索显式降级 RRF 名次）；引用未配置或执行面未接线 → 不重排。
      const reranker = resolved.rerankModelRef && this.deps.rerankForModel
        ? ((request: { runId: string; query: string; documents: string[]; topN: number; signal?: AbortSignal }) =>
          this.deps.rerankForModel!({
            runId: request.runId,
            query: request.query,
            documents: request.documents,
            topN: request.topN,
            signal: request.signal,
            modelRef: resolved.rerankModelRef!,
          })) as KnowledgeReranker
        : null;
      const rerank = input.rerank ?? reranker != null;
      // 生效分块尺寸与摄入侧同源解析（显式列 > 嵌入模型上下文 ×80%）：检索锚按
      // 同一 configId 纯解析，只读命中摄入侧建出的变体。
      const chunkTargetChars = resolveEffectiveChunkTargetChars(
        resolved,
        this.deps.getEmbeddingModelContextWindow,
      );
      // 查询嵌入按笔记本配置路由（与摄入侧同一模型 → 向量命中同一 model_key 分区）；
      // 笔记本未配置嵌入模型 → 该笔记本纯 FTS（禁静默换模型）。
      const embedTexts = resolved.embeddingModelRef && this.deps.embedTextsForModel
        ? ((request: { runId: string; texts: string[]; signal?: AbortSignal }) =>
          this.deps.embedTextsForModel!({
            runId: request.runId,
            texts: request.texts,
            signal: request.signal,
            modelRef: resolved.embeddingModelRef!,
          })) as KnowledgeEmbedder
        : null;
      const readyArtifacts = entries
        .filter(entry => entry.parseArtifact?.status === "ready")
        .map(entry => ({ entry, artifactId: entry.parseArtifact!.id }));
      const artifactIds = readyArtifacts.map(item => item.artifactId);
      const sourceByArtifact = new Map(readyArtifacts.map(({ entry, artifactId }) => [
        artifactId,
        { sourceId: entry.source.id, sourceName: entry.source.displayName },
      ]));
      // 非 ready 源的显式留痕（不再静默排除）：needs_ocr 终态 / 解析在途 / 解析失败。
      const degraded: KnowledgeDegradedRetrievalScope[] = entries
        .filter(entry => entry.parseArtifact?.status !== "ready")
        .map(entry => ({
          parseArtifactId: entry.parseArtifact?.id ?? "",
          chunkProfileHash: "",
          reason: entry.parseArtifact?.status === "needs_ocr"
            ? "KNOWLEDGE_SOURCE_NEEDS_OCR" as const
            : entry.parseArtifact?.status === "failed"
              ? "KNOWLEDGE_INDEX_FAILED" as const
              : "KNOWLEDGE_INDEX_BUILDING" as const,
          detail: entry.parseArtifact
            ? `parse artifact status: ${entry.parseArtifact.status}`
            : "parse pending",
          notebookId,
          notebookName: notebook.name,
          sourceId: entry.source.id,
          sourceName: entry.source.displayName,
        }));
      // 检索锚逐 artifact 纯解析（策略依赖 artifact 内容；只读，不再惰性建绑）。
      const scopes = this.resolveRetrievalScopes({ studioId, artifactIds, chunkTargetChars });
      const scopeByArtifact = new Map(scopes.map(scope => [scope.parseArtifactId, scope]));
      // §三十九 section 约束：把选中 section 的 headingPath 分桶解析为各变体的
      // ordinal 区间（SQL 层过滤 FTS、后过滤向量命中），约束后为空 = 该源本轮无果。
      const ordinalRanges = new Map<string, KnowledgeOrdinalRange[]>();
      if (input.sectionsBySourceId && input.sectionsBySourceId.size > 0) {
        for (const { entry, artifactId } of readyArtifacts) {
          const sectionKeys = input.sectionsBySourceId.get(entry.source.id);
          if (!sectionKeys || sectionKeys.length === 0) continue;
          const chunkProfileHash = scopeByArtifact.get(artifactId)?.chunkProfileHash;
          if (!chunkProfileHash) continue;
          ordinalRanges.set(
            knowledgeChunkIndexVariantId(artifactId, chunkProfileHash),
            this.resolveSectionOrdinalRanges({
              studioId,
              artifactId,
              chunkIndexVariantId: knowledgeChunkIndexVariantId(artifactId, chunkProfileHash),
              sectionKeys: new Set(sectionKeys),
              locatorCache,
            }),
          );
        }
      }
      let ranked: IndexedKnowledgeChunk[] = [];
      let retrievalMode: "fts" | "hybrid" = "fts";
      let retrievalModeRequested: "fts" | "hybrid" = embedTexts ? "hybrid" : "fts";
      let searchedVectorVariants: SearchedVectorVariantIdentity[] = [];
      let rerankDegradeReason: string | null = null;
      if (scopes.length > 0) {
        const result = await this.retrieve({
          studioId,
          scopes,
          question: input.question,
          runId: `kbctx_${crypto.randomUUID()}`,
          signal: input.signal,
          topK,
          rerank,
          reranker,
          embedTexts,
          ...(ordinalRanges.size > 0 ? { ordinalRangesByChunkIndexVariantId: ordinalRanges } : {}),
        });
        if (result.retrievalMode === "hybrid") retrievalMode = "hybrid";
        retrievalModeRequested = result.retrievalModeRequested;
        ranked = result.candidates;
        searchedVectorVariants = result.searchedVectorVariants;
        if (result.rerankDegradeReason) {
          rerankDegradeReason = `${notebook.name}: ${result.rerankDegradeReason}`;
        }
        // 逐 scope 降级：附上 notebook/source 归属，并幂等入队后台构建（§十二）。
        // KNOWLEDGE_INDEX_FAILED 是显式终态：不自动重试（UI 手动 reingest），只留痕。
        for (const item of result.degraded) {
          const owner = sourceByArtifact.get(item.parseArtifactId);
          degraded.push({
            ...item,
            notebookId,
            notebookName: notebook.name,
            ...(owner ?? {}),
          });
          if (item.reason === "KNOWLEDGE_INDEX_FAILED" || !owner) continue;
          this.deps.requestVariantBuild?.({
            studioId,
            notebookId,
            sourceId: owner.sourceId,
            parseArtifactId: item.parseArtifactId,
            reason: item.reason,
          });
        }
      }
      const sources = readyArtifacts.map(({ entry, artifactId }) => this.describeRetrievalSource({
        studioId,
        notebookId,
        notebookName: notebook.name,
        source: entry.source,
        artifactId,
        chunkProfileHash: scopeByArtifact.get(artifactId)!.chunkProfileHash,
        locatorCache,
      }));
      return {
        notebookId,
        notebookName: notebook.name,
        ranked,
        retrievalMode,
        retrievalModeRequested,
        sources,
        degraded,
        searchedVectorVariants,
        ...(rerankDegradeReason ? { rerankDegradeReason } : {}),
      };
    }));

    const sources: NotebookRetrievalSource[] = notebookResults.flatMap(item => item.sources);
    const retrievalMode: "fts" | "hybrid" = notebookResults.some(item => item.retrievalMode === "hybrid")
      ? "hybrid"
      : "fts";
    const retrievalModeRequested: "fts" | "hybrid" = notebookResults.some(
      item => item.retrievalModeRequested === "hybrid",
    )
      ? "hybrid"
      : "fts";
    const degraded = notebookResults.flatMap(item => item.degraded);
    // rerank 降级留痕跨笔记本汇总（保持笔记本归属，供注入块/stats 显式呈现）。
    const rerankDegradeReasons = notebookResults
      .map(item => item.rerankDegradeReason)
      .filter((reason): reason is string => typeof reason === "string");
    // 向量变体身份跨笔记本汇总去重（同源被多笔记本共享时各结果重复携带）。
    const searchedVectorVariants: SearchedVectorVariantIdentity[] = [];
    const seenVectorVariantIds = new Set<string>();
    for (const item of notebookResults) {
      for (const variant of item.searchedVectorVariants) {
        if (seenVectorVariantIds.has(variant.vectorIndexVariantId)) continue;
        seenVectorVariantIds.add(variant.vectorIndexVariantId);
        searchedVectorVariants.push(variant);
      }
    }

    // 跨笔记本 rank-based RRF 融合（§二十五）：只消费各笔记本的名次序列，
    // 绝不读跨笔记本的 raw score；同 chunk.id 去重，并列按 notebook 序稳定排序。
    const candidates: NotebookRetrievalChunk[] = fuseNotebookRankings(
      notebookResults.map(item => item.ranked),
    ).map(entry => this.annotateRetrievalChunk(
      entry.chunk,
      notebookResults[entry.notebookIndex].notebookId,
      notebookResults[entry.notebookIndex].notebookName,
      sources,
      studioId,
      locatorCache,
    ));

    return {
      candidates,
      sources,
      retrievalMode,
      retrievalModeRequested,
      degraded,
      searchedVectorVariants,
      ...(rerankDegradeReasons.length > 0 ? { rerankDegradeReasons } : {}),
    };
  }

  /**
   * 源清单条目：该笔记本 profile 变体的总 chunk 数 + 首 chunk 标题（超预算分片
   * 清单与 knowledge_read 的数据源）。Phase 2 起只读：变体未就绪时 chunkCount=0
   * （分片清单显示 "no indexed chunks"），原因由同轮 degraded 清单显式留痕。
   */
  private describeRetrievalSource(input: {
    studioId: string;
    notebookId: string;
    notebookName: string;
    source: KnowledgeSource;
    artifactId: string;
    /** 检索锚：该笔记本生效分块配置的 chunkProfileHash（retrieve 调用前已纯解析）。 */
    chunkProfileHash: string;
    locatorCache?: Map<string, Map<string, KnowledgeBlockLocator>>;
  }): NotebookRetrievalSource {
    let chunks: ReturnType<KnowledgeIndexStore["listVariantChunks"]> = [];
    const variant = this.deps.indexStore.resolveChunkIndexVariant(input.artifactId, input.chunkProfileHash);
    if (variant?.status === "ready") {
      try {
        chunks = this.deps.indexStore.listVariantChunks(variant.id);
      } catch (error) {
        // chunk 行损坏：retrieve 的自愈路径已/将 reset 重建；清单按空处理不抛错。
        if (!isKnowledgeError(error) || error.code !== "KNOWLEDGE_INDEX_INVALID") throw error;
        chunks = [];
      }
    }
    // §三十九：源内可用 section 键（distinct、按源内出现序）——broad 档 section
    // coverage 的分母。chunks 已整体在手上（chunkCount 同源），只做 locator 查表。
    const locatorIndex = chunks.length > 0
      ? this.blockLocatorIndex(input.studioId, input.artifactId, input.locatorCache)
      : null;
    const sections: string[] = [];
    const seenSections = new Set<string>();
    let firstHeadingPath: string[] | null = null;
    for (const chunk of chunks) {
      const locator = locatorIndex?.get(chunk.spans?.[0]?.blockId ?? "") ?? null;
      if (firstHeadingPath == null && locator?.headingPath) firstHeadingPath = locator.headingPath;
      const key = knowledgeSectionKeyOf(locator?.headingPath ?? null);
      if (key != null && !seenSections.has(key)) {
        seenSections.add(key);
        sections.push(key);
      }
    }
    return {
      notebookId: input.notebookId,
      notebookName: input.notebookName,
      sourceId: input.source.id,
      sourceName: input.source.displayName,
      parseArtifactId: input.artifactId,
      chunkCount: chunks.length,
      firstHeadingPath,
      sections,
      chunkProfileHash: input.chunkProfileHash,
    };
  }

  /**
   * §三十九 section 约束 → ordinal 区间（纯只读）：列出变体全部 chunk，按首
   * span blockId 的 headingPath 分桶，选中 section 的 ordinal 合并为相邻闭区间。
   * 变体未建/损坏 → 空区间（该源本轮无结果；原因由同轮 degraded 清单留痕）。
   */
  private resolveSectionOrdinalRanges(input: {
    studioId: string;
    artifactId: string;
    chunkIndexVariantId: string;
    sectionKeys: ReadonlySet<string>;
    locatorCache: Map<string, Map<string, KnowledgeBlockLocator>>;
  }): KnowledgeOrdinalRange[] {
    let chunks: ReturnType<KnowledgeIndexStore["listVariantChunks"]>;
    try {
      chunks = this.deps.indexStore.listVariantChunks(input.chunkIndexVariantId);
    } catch (error) {
      if (!isKnowledgeError(error) || error.code !== "KNOWLEDGE_INDEX_INVALID") throw error;
      return [];
    }
    const locatorIndex = this.blockLocatorIndex(input.studioId, input.artifactId, input.locatorCache);
    const ordinals = chunks
      .map(chunk => knowledgeSectionKeyOf(
        locatorIndex.get(chunk.spans?.[0]?.blockId ?? "")?.headingPath ?? null,
      ))
      .map((key, index) => (key != null && input.sectionKeys.has(key) ? chunks[index].ordinal : -1))
      .filter(ordinal => ordinal >= 0)
      .sort((left, right) => left - right);
    const ranges: Array<[number, number]> = [];
    for (const ordinal of ordinals) {
      const last = ranges[ranges.length - 1];
      if (last && ordinal <= last[1] + 1) {
        last[1] = ordinal;
        continue;
      }
      ranges.push([ordinal, ordinal]);
    }
    return ranges;
  }

  /**
   * 邻接块定点回读（Phase 8 §三十六，neighbor expansion 数据面）：同变体内按
   * ordinal 读块并附 notebook/source/locator 注解（与检索候选同一注解规则）。
   * 只读；越界 ordinal 自然缺席。行损坏时返回空——锚点证据与降级留痕由主检索
   * 路径负责，邻接块只是上下文连续性增强，不在此触发自愈。
   */
  readAdjacentChunks(input: {
    studioId: string;
    anchor: {
      notebookId: string;
      notebookName: string;
      sourceId: string;
      sourceName: string;
      parseArtifactId: string;
      chunkIndexVariantId: string;
    };
    ordinals: number[];
  }): NotebookRetrievalChunk[] {
    if (input.ordinals.length === 0) return [];
    let chunks: ReturnType<KnowledgeIndexStore["readVariantChunks"]>;
    try {
      chunks = this.deps.indexStore.readVariantChunks(input.anchor.chunkIndexVariantId, input.ordinals);
    } catch (error) {
      if (!isKnowledgeError(error) || error.code !== "KNOWLEDGE_INDEX_INVALID") throw error;
      return [];
    }
    const locatorIndex = this.blockLocatorIndex(input.studioId, input.anchor.parseArtifactId);
    return chunks.map(chunk => {
      const locator = locatorIndex.get(chunk.spans?.[0]?.blockId ?? "");
      return {
        ...chunk,
        // 邻接块不是检索命中：score 置 0，绝不参与任何名次比较。
        score: 0,
        notebookId: input.anchor.notebookId,
        notebookName: input.anchor.notebookName,
        sourceId: input.anchor.sourceId,
        sourceName: input.anchor.sourceName,
        headingPath: locator?.headingPath ?? null,
        pageNumber: locator?.pageNumber ?? null,
      };
    });
  }

  /** blockId → 定位信息（headingPath / 页码）。同一 artifact 的块只查一次库。 */
  private blockLocatorIndex(
    studioId: string,
    parseArtifactId: string,
    cache: Map<string, Map<string, KnowledgeBlockLocator>> = new Map(),
  ): Map<string, KnowledgeBlockLocator> {
    const cached = cache.get(parseArtifactId);
    if (cached) return cached;
    const index = buildKnowledgeBlockLocatorIndex(
      this.deps.store.listArtifactBlocks({ studioId, parseArtifactId }),
    );
    cache.set(parseArtifactId, index);
    return index;
  }

  private annotateRetrievalChunk(
    chunk: IndexedKnowledgeChunk,
    notebookId: string,
    notebookName: string,
    sources: NotebookRetrievalSource[],
    studioId: string,
    locatorCache: Map<string, Map<string, KnowledgeBlockLocator>>,
  ): NotebookRetrievalChunk {
    const source = sources.find(item => item.parseArtifactId === chunk.parseArtifactId);
    const locator = this.blockLocatorIndex(studioId, chunk.parseArtifactId, locatorCache)
      .get(chunk.spans?.[0]?.blockId ?? "");
    return {
      ...chunk,
      notebookId,
      notebookName,
      sourceId: source?.sourceId ?? chunk.parseArtifactId,
      sourceName: source?.sourceName ?? chunk.parseArtifactId,
      headingPath: locator?.headingPath ?? null,
      pageNumber: locator?.pageNumber ?? null,
    };
  }
}
