import crypto from "node:crypto";

import {
  buildKnowledgeChunks,
  knowledgeBlockFingerprint,
  resolveKnowledgeChunkerConfig,
  type KnowledgeChunkDraft,
} from "./chunker.ts";
import { KnowledgeError, isKnowledgeError } from "./errors.ts";
import { KnowledgeIndexStore, type IndexedKnowledgeChunk } from "./knowledge-index-store.ts";
import { KnowledgeStore } from "./knowledge-store.ts";
import { resolveNotebookConfig } from "./knowledge-store.ts";
import type { KnowledgeBlock, KnowledgeModelRef, KnowledgeSource } from "./types.ts";
import {
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
}

export interface RetrieveForNotebooksResult {
  candidates: NotebookRetrievalChunk[];
  sources: NotebookRetrievalSource[];
  retrievalMode: "fts" | "hybrid";
}

function isAbortLike(error: any): boolean {
  return error?.name === "AbortError" || error?.name === "TimeoutError" || error?.type === "aborted";
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

function fuseCandidates(
  fts: IndexedKnowledgeChunk[],
  vector: IndexedKnowledgeChunk[],
  limit = 12,
): IndexedKnowledgeChunk[] {
  const fused = new Map<string, { chunk: IndexedKnowledgeChunk; score: number }>();
  const add = (chunks: IndexedKnowledgeChunk[]) => {
    chunks.forEach((chunk, index) => {
      const current = fused.get(chunk.id) || { chunk, score: 0 };
      current.score += 1 / (60 + index + 1);
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
    .slice(0, limit)
    .map(entry => ({ ...entry.chunk, score: entry.score }));
}

export interface KnowledgeBlockLocator {
  headingPath: string[] | null;
  pageNumber: number | null;
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
 * 200 时已饱和且多数 rerank API 有文档数上限；超出部分保持 RRF 名次不再重排。
 */
export const KNOWLEDGE_RERANK_MAX_DOCS = 200;

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
  };

  constructor(deps: KnowledgeQueryService["deps"]) {
    this.deps = deps;
  }

  /**
   * 摄入管线的 chunk+fts_index 相位：分块与 FTS 索引在同一次幂等替换中原子完成。
   * 指纹（blocks 内容 + chunkerConfigId）匹配即整体跳过；不匹配才重建。
   * 笔记本级 targetChars 由摄入侧传入；查询侧懒构建（ensureArtifactIndexed）仍用默认配置，
   * 是摄入未跑时的兜底，不删。
   */
  indexArtifactForIngestion(
    studioId: string,
    parseArtifactId: string,
    options?: { targetChars?: number },
  ): { chunkerConfigId: string; rebuilt: boolean } {
    const run = () => {
      const blocks = this.deps.store.listArtifactBlocks({ studioId, parseArtifactId });
      const fingerprint = knowledgeBlockFingerprint(blocks);
      // chunker_version 列语义已改为 chunkerConfigId：分块策略/尺寸变化即不匹配并整体重建。
      const config = resolveKnowledgeChunkerConfig(blocks, { targetChars: options?.targetChars });
      if (this.deps.indexStore.hasArtifactFingerprint(parseArtifactId, fingerprint, config.configId)) {
        return { chunkerConfigId: config.configId, rebuilt: false };
      }
      const chunks = buildKnowledgeChunks(parseArtifactId, blocks, { targetChars: options?.targetChars });
      if (chunks.length === 0) {
        throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Ready Knowledge source produced no searchable chunks");
      }
      this.deps.indexStore.replaceArtifactChunks({
        parseArtifactId,
        blockFingerprint: fingerprint,
        chunkerVersion: config.configId,
        chunks,
      });
      return { chunkerConfigId: config.configId, rebuilt: true };
    };
    try {
      return run();
    } catch (error) {
      // 索引库是可重建缓存：损坏时重置后重试一次（与查询侧 ensureScopeIndexed 同一自愈语义）。
      if (!isKnowledgeError(error) || error.code !== "KNOWLEDGE_INDEX_INVALID") throw error;
      this.deps.indexStore.reset();
      return run();
    }
  }

  private ensureArtifactIndexed(studioId: string, parseArtifactId: string) {
    // 查询侧懒构建兜底保留：摄入管线未覆盖的 artifact 在查询时按默认分块配置补齐。
    this.indexArtifactForIngestion(studioId, parseArtifactId);
  }

  private ensureScopeIndexed(studioId: string, artifactIds: string[]) {
    const ensureAll = () => artifactIds.forEach(id => this.ensureArtifactIndexed(studioId, id));
    try {
      ensureAll();
    } catch (error) {
      if (!isKnowledgeError(error) || error.code !== "KNOWLEDGE_INDEX_INVALID") throw error;
      this.deps.indexStore.reset();
      ensureAll();
    }
  }

  private retrieveFts(studioId: string, artifactIds: string[], question: string, limit: number): IndexedKnowledgeChunk[] {
    this.ensureScopeIndexed(studioId, artifactIds);
    // 底层 FTS/向量 search 的 sanity 上限 1000：无上限召回（retrieval_top_k NULL）
    // 的物理边界即此值，防病态全表膨胀。
    const searchLimit = Math.max(1, Math.min(limit, KNOWLEDGE_UNCAPPED_RETRIEVAL_LIMIT));
    const search = () => this.deps.indexStore.search({ parseArtifactIds: artifactIds, query: question, limit: searchLimit });
    try {
      return search();
    } catch (error) {
      if (isKnowledgeError(error) && error.code === "KNOWLEDGE_INVALID_ARGUMENT") return [];
      if (!isKnowledgeError(error) || error.code !== "KNOWLEDGE_INDEX_INVALID") throw error;
      this.deps.indexStore.reset();
      this.ensureScopeIndexed(studioId, artifactIds);
      return search();
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
   * 摄入管线的 embed 相位：FTS chunk 已就绪后执行向量嵌入（64/批）。
   * 幂等：chunkFingerprint + 模型身份已在向量库时整体跳过（status "skipped"）。
   * 模型身份（含 dimensions）只能由真实嵌入响应确认，因此第一批先嵌入、确认身份后才做
   * 跳过判断——已嵌入场景最多多跑一批，换取无需预测 provider 维度配置的简单性。
   * embedder 返回 null（模型在检查与执行之间被摘除的竞态）→ status "unavailable"，
   * 由调用方落显式 pending_embedding（禁静默降级）。
   */
  async embedArtifactForIngestion(input: {
    runId: string;
    parseArtifactId: string;
    embedTexts: KnowledgeEmbedder;
    signal?: AbortSignal;
    /** 每批嵌入成功后回调（done/total 均为累计块数）；抛错按嵌入失败处理。 */
    onProgress?: (done: number, total: number) => void;
  }): Promise<{ status: "embedded" | "skipped" | "unavailable"; chunkCount: number }> {
    const vectorIndex = this.deps.vectorIndex;
    if (!vectorIndex) {
      throw new KnowledgeError("KNOWLEDGE_RETRIEVAL_UNAVAILABLE", "Knowledge vector index is unavailable");
    }
    const chunks = this.deps.indexStore.listArtifactChunks(input.parseArtifactId);
    if (chunks.length === 0) {
      throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Indexed Knowledge artifact has no chunks to embed");
    }
    const fingerprint = chunkFingerprint(chunks);
    const build = async (): Promise<{ status: "embedded" | "skipped" | "unavailable" }> => {
      const vectors: number[][] = [];
      let model: VectorIndexModelIdentity | null = null;
      for (let start = 0; start < chunks.length; start += 64) {
        const batch = chunks.slice(start, start + 64);
        const response = await this.invokeEmbedding({
          runId: input.runId,
          texts: batch.map(chunk => chunk.text),
          signal: input.signal,
        }, input.embedTexts);
        if (!response) return { status: "unavailable" };
        const embedded = assertEmbeddingBatch(response, batch.length, model ?? undefined);
        if (!model) {
          model = embedded.model;
          if (vectorIndex.hasArtifact({
            parseArtifactId: input.parseArtifactId,
            chunkFingerprint: fingerprint,
            model,
          })) {
            return { status: "skipped" };
          }
        }
        vectors.push(...embedded.result.vectors);
        input.onProgress?.(Math.min(start + 64, chunks.length), chunks.length);
      }
      vectorIndex.buildOrReplaceArtifact({
        parseArtifactId: input.parseArtifactId,
        chunkFingerprint: fingerprint,
        model: model!,
        entries: chunks.map((chunk, index) => ({
          chunkId: chunk.id,
          parseArtifactId: input.parseArtifactId,
          ordinal: chunk.ordinal,
          vector: vectors[index],
        })),
      });
      return { status: "embedded" };
    };
    try {
      return { ...(await build()), chunkCount: chunks.length };
    } catch (error) {
      // 向量库损坏：重建后重试一次（与 retrieve 的 buildAndSearch 自愈同一语义）。
      if (!isKnowledgeError(error) || error.code !== "KNOWLEDGE_INDEX_INVALID") throw error;
      vectorIndex.rebuild();
      return { ...(await build()), chunkCount: chunks.length };
    }
  }

  private async ensureVectorArtifacts(input: {
    runId: string;
    artifactIds: string[];
    chunksByArtifact: Map<string, KnowledgeChunkDraft[]>;
    model: VectorIndexModelIdentity;
    signal?: AbortSignal;
    embedTexts: KnowledgeEmbedder;
  }) {
    const vectorIndex = this.deps.vectorIndex;
    if (!vectorIndex) {
      throw new KnowledgeError("KNOWLEDGE_RETRIEVAL_UNAVAILABLE", "Knowledge vector index is unavailable");
    }
    for (const parseArtifactId of input.artifactIds) {
      const chunks = input.chunksByArtifact.get(parseArtifactId) || [];
      const fingerprint = chunkFingerprint(chunks);
      if (vectorIndex.hasArtifact({ parseArtifactId, chunkFingerprint: fingerprint, model: input.model })) continue;
      const vectors: number[][] = [];
      for (let start = 0; start < chunks.length; start += 64) {
        const batch = chunks.slice(start, start + 64);
        const embedded = assertEmbeddingBatch(
          await this.invokeEmbedding({
            runId: input.runId,
            texts: batch.map(chunk => chunk.text),
            signal: input.signal,
          }, input.embedTexts),
          batch.length,
          input.model,
        );
        vectors.push(...embedded.result.vectors);
      }
      vectorIndex.buildOrReplaceArtifact({
        parseArtifactId,
        chunkFingerprint: fingerprint,
        model: input.model,
        entries: chunks.map((chunk, index) => ({
          chunkId: chunk.id,
          parseArtifactId,
          ordinal: chunk.ordinal,
          vector: vectors[index],
        })),
      });
    }
  }

  // FTS + 向量 RRF + 可选 rerank 的检索核心。retrieveForNotebooks（笔记本引用注入）
  // 与 knowledge_read 工具（单源检索）复用本方法；topK/rerank 由调用方按各自配置解析。
  protected async retrieve(input: {
    studioId: string;
    artifactIds: string[];
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
     * 查询嵌入回调（v8 起由调用方按笔记本解析出的嵌入模型构造闭包传入）。
     * 缺省 → 纯 FTS（retrievalMode 如实 "fts"，禁静默换模型）。
     */
    embedTexts?: KnowledgeEmbedder | null;
  }): Promise<{ candidates: IndexedKnowledgeChunk[]; retrievalMode: "fts" | "hybrid" }> {
    const { studioId, artifactIds, question, runId, signal } = input;
    const topK = input.topK == null || input.topK <= 0
      ? KNOWLEDGE_UNCAPPED_RETRIEVAL_LIMIT
      : Math.min(input.topK, KNOWLEDGE_UNCAPPED_RETRIEVAL_LIMIT);
    const fts = this.retrieveFts(studioId, artifactIds, question, topK);
    if (!input.embedTexts) return { candidates: fts, retrievalMode: "fts" };

    const questionEmbedding = await this.invokeEmbedding({ runId, texts: [question], signal }, input.embedTexts);
    if (!questionEmbedding) return { candidates: fts, retrievalMode: "fts" };
    const embeddedQuestion = assertEmbeddingBatch(questionEmbedding, 1);
    const chunksByArtifact = new Map<string, KnowledgeChunkDraft[]>();
    const chunksById = new Map<string, KnowledgeChunkDraft>();
    for (const artifactId of artifactIds) {
      const chunks = this.deps.indexStore.listArtifactChunks(artifactId);
      chunksByArtifact.set(artifactId, chunks);
      chunks.forEach(chunk => chunksById.set(chunk.id, chunk));
    }

    const buildAndSearch = async () => {
      await this.ensureVectorArtifacts({
        runId,
        artifactIds,
        chunksByArtifact,
        model: embeddedQuestion.model,
        signal,
        embedTexts: input.embedTexts!,
      });
      return this.deps.vectorIndex!.search({
        parseArtifactIds: artifactIds,
        model: embeddedQuestion.model,
        queryVector: embeddedQuestion.result.vectors[0],
        limit: Math.max(1, Math.min(topK, KNOWLEDGE_UNCAPPED_RETRIEVAL_LIMIT)),
      });
    };
    let vectorRows;
    try {
      vectorRows = await buildAndSearch();
    } catch (error) {
      if (!isKnowledgeError(error) || error.code !== "KNOWLEDGE_INDEX_INVALID") throw error;
      this.deps.vectorIndex!.rebuild();
      vectorRows = await buildAndSearch();
    }
    const vector = vectorRows.map(row => {
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
    if (input.rerank !== false && rerankCandidates.length > 0 && this.deps.rerank) {
      let reranked;
      try {
        reranked = await this.deps.rerank({
          runId,
          query: question,
          documents: rerankCandidates.map(candidate => candidate.text),
          topN: rerankCandidates.length,
          signal,
        });
      } catch (error) {
        if (isAbortLike(error)) throw error;
        if (isKnowledgeError(error)) throw error;
        throw new KnowledgeError("KNOWLEDGE_RETRIEVAL_UNAVAILABLE", "Knowledge rerank request failed");
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
    return { candidates, retrievalMode: "hybrid" };
  }

  /**
   * knowledge_read 工具的单源检索入口：按显式 artifact 集合执行同一检索核心。
   * 只读、studio 隔离由调用方先解析 artifact 边界保证。
   */
  async retrieveForArtifacts(input: {
    studioId: string;
    artifactIds: string[];
    question: string;
    topK?: number;
    rerank?: boolean;
    signal?: AbortSignal;
    /** owning notebook 的嵌入引用：按同一模型路由查询向量；null → 纯 FTS。 */
    embeddingModelRef?: KnowledgeModelRef | null;
  }): Promise<{ candidates: IndexedKnowledgeChunk[]; retrievalMode: "fts" | "hybrid" }> {
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
    return this.retrieve({
      studioId: input.studioId,
      artifactIds: input.artifactIds,
      question: input.question,
      runId: `kbread_${crypto.randomUUID()}`,
      signal: input.signal,
      ...(input.topK != null ? { topK: input.topK } : {}),
      ...(input.rerank != null ? { rerank: input.rerank } : {}),
      embedTexts,
    });
  }

  /**
   * 主界面笔记本引用注入的检索入口（Phase 8）。
   *
   * artifact 硬边界：被引用笔记本内全部 active 源的最新 ready parse artifact——
   * 边界来自 store 的 studio 过滤 SQL（listNotebookSources 的 JOIN 链），未引用
   * 笔记本与其他 studio 的源不可能进入候选。
   * topK/rerank 按笔记本各自配置解析（resolveNotebookConfig：笔记本列 →
   * 全局偏好 → 内置默认 12）；多笔记本合并采用轮转交错（每个笔记本按名次
   * 依次取一名）：跨笔记本的分数不可直接比较（rerank 分数按笔记本各自归一），
   * 轮转保证每个被引笔记本公平占用注入预算，而不是让单个高分笔记本吞掉全部。
   *
   * 各笔记本的 retrieve 并行执行（降时延；串行最坏 = 笔记本数 × 检索耗时）。
   * 确定性合并：Promise.all 按 notebookIds 顺序映射结果，合并阶段只消费各
   * 笔记本的名次序列做轮转交错 + 去重，与各检索的完成顺序无关；任一笔记本
   * 检索抛错仍整体抛出（错误语义与串行版一致，由子查询层分类留痕）。
   */
  async retrieveForNotebooks(input: {
    studioId: string;
    notebookIds: string[];
    question: string;
    topK?: number;
    rerank?: boolean;
    signal?: AbortSignal;
  }): Promise<RetrieveForNotebooksResult> {
    const studioId = input.studioId;

    const notebookResults = await Promise.all(input.notebookIds.map(async (notebookId) => {
      const notebook = this.deps.store.getNotebook({ studioId, notebookId });
      const entries = this.deps.store.listNotebookSources({ studioId, notebookId });
      const resolved = resolveNotebookConfig(
        this.deps.store.getNotebookConfig({ studioId, notebookId }),
      );
      const topK = input.topK ?? resolved.retrievalTopK;
      // rerank 解析（v8 起）：显式入参 > 笔记本 rerank 列。引用本身只作开关；
      // 执行仍走 manager 级 reranker（本阶段无按引用路由的 rerank 执行面）。
      const rerank = input.rerank ?? resolved.rerankModelRef != null;
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
      let ranked: IndexedKnowledgeChunk[] = [];
      let retrievalMode: "fts" | "hybrid" = "fts";
      if (artifactIds.length > 0) {
        const result = await this.retrieve({
          studioId,
          artifactIds,
          question: input.question,
          runId: `kbctx_${crypto.randomUUID()}`,
          signal: input.signal,
          topK,
          rerank,
          embedTexts,
        });
        if (result.retrievalMode === "hybrid") retrievalMode = "hybrid";
        ranked = result.candidates;
      }
      const sources = readyArtifacts.map(({ entry, artifactId }) => this.describeRetrievalSource({
        studioId,
        notebookId,
        notebookName: notebook.name,
        source: entry.source,
        artifactId,
      }));
      return { notebookId, notebookName: notebook.name, ranked, retrievalMode, sources };
    }));

    const sources: NotebookRetrievalSource[] = notebookResults.flatMap(item => item.sources);
    const retrievalMode: "fts" | "hybrid" = notebookResults.some(item => item.retrievalMode === "hybrid")
      ? "hybrid"
      : "fts";

    const seen = new Set<string>();
    const candidates: NotebookRetrievalChunk[] = [];
    const maxRank = Math.max(0, ...notebookResults.map(item => item.ranked.length));
    const locatorCache = new Map<string, Map<string, KnowledgeBlockLocator>>();
    for (let rank = 0; rank < maxRank; rank += 1) {
      for (const item of notebookResults) {
        const chunk = item.ranked[rank];
        if (!chunk || seen.has(chunk.id)) continue;
        seen.add(chunk.id);
        candidates.push(this.annotateRetrievalChunk(chunk, item.notebookId, item.notebookName, sources, studioId, locatorCache));
      }
    }

    return { candidates, sources, retrievalMode };
  }

  /** 源清单条目：总 chunk 数 + 首 chunk 标题（超预算分片清单与 knowledge_read 的数据源）。 */
  private describeRetrievalSource(input: {
    studioId: string;
    notebookId: string;
    notebookName: string;
    source: KnowledgeSource;
    artifactId: string;
  }): NotebookRetrievalSource {
    this.ensureArtifactIndexed(input.studioId, input.artifactId);
    const chunks = this.deps.indexStore.listArtifactChunks(input.artifactId);
    let firstHeadingPath: string[] | null = null;
    if (chunks.length > 0) {
      const locator = this.blockLocatorIndex(input.studioId, input.artifactId).get(chunks[0].spans?.[0]?.blockId ?? "");
      firstHeadingPath = locator?.headingPath ?? null;
    }
    return {
      notebookId: input.notebookId,
      notebookName: input.notebookName,
      sourceId: input.source.id,
      sourceName: input.source.displayName,
      parseArtifactId: input.artifactId,
      chunkCount: chunks.length,
      firstHeadingPath,
    };
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
