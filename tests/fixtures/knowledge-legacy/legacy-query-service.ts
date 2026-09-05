/** 已退役查询编排的回归夹具。仅旧测试挂载，生产不导出、不加载；底层索引、向量和重排仍调用真实实现。 */
import crypto from "node:crypto";
import { KnowledgeQueryService as ProductionQueryService, buildKnowledgeBlockLocatorIndex,
  knowledgeSectionKeyOf, fuseNotebookRankings as fuseProductionRankings,
  type KnowledgeBlockLocator, type KnowledgeEmbedder, type KnowledgeReranker,
  type NotebookRetrievalChunk, type NotebookRetrievalSource, type RetrieveForNotebooksResult,
  type SearchedVectorVariantIdentity, type KnowledgeRetrievalStageTimings,
  type KnowledgeDegradedRetrievalScope } from "../../../lib/knowledge/knowledge-query-service.ts";
import { KnowledgeManager as ProductionManager } from "../../../lib/knowledge/knowledge-manager.ts";
import { resolveEffectiveChunkTargetChars, resolveNotebookConfig } from "../../../lib/knowledge/knowledge-store.ts";
import { knowledgeBlockFingerprint, legacyKnowledgeBlockFingerprint, resolveKnowledgeChunkerConfig } from "../../../lib/knowledge/chunker.ts";
import { KnowledgeIndexStore, knowledgeChunkIndexVariantId, type IndexedKnowledgeChunk, type KnowledgeOrdinalRange } from "../../../lib/knowledge/knowledge-index-store.ts";
import { KnowledgeError, isKnowledgeError } from "../../../lib/knowledge/errors.ts";
import type { KnowledgeSource, KnowledgeModelRef } from "../../../lib/knowledge/types.ts";

export type { KnowledgeManagerOptions } from "../../../lib/knowledge/knowledge-manager.ts";

export const KNOWLEDGE_EVIDENCE_BUDGET = 40;
export const KNOWLEDGE_FUSION_BUDGET = 60;
export const KNOWLEDGE_CANDIDATE_GENERATION_BUDGET = 60;
const fuseNotebookRankings = (rankings: IndexedKnowledgeChunk[][]) => fuseProductionRankings(rankings).slice(0, KNOWLEDGE_FUSION_BUDGET);
type KnowledgeRetrievalScope = { parseArtifactId: string; chunkProfileHash: string; blockFingerprint: string };
export class HistoricalKnowledgeQueryService extends ProductionQueryService {
  private get legacyDeps(): ConstructorParameters<typeof ProductionQueryService>[0] {
    return (this as unknown as { deps: ConstructorParameters<typeof ProductionQueryService>[0] }).deps;
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
    fallbackProfileHash?: string | null;
  }): KnowledgeRetrievalScope[] {
    const seen = new Set<string>();
    const scopes: KnowledgeRetrievalScope[] = [];
    for (const parseArtifactId of input.artifactIds) {
      const blocks = this.legacyDeps.store.listArtifactBlocks({ studioId: input.studioId, parseArtifactId });
      const config = resolveKnowledgeChunkerConfig(blocks, {
        ...(input.chunkTargetChars != null ? { targetChars: input.chunkTargetChars } : {}),
      });
      const preferred = config.configId;
      const candidateHashes = [...new Set([preferred, ...this.legacyDeps.store.getQueryChunkProfileCandidates(input.fallbackProfileHash ?? preferred)])];
      const selected = candidateHashes.find(hash => this.legacyDeps.indexStore.hasArtifactFingerprint(parseArtifactId, hash,
        hash === preferred ? knowledgeBlockFingerprint(blocks) : legacyKnowledgeBlockFingerprint(blocks)));
      const chunkProfileHash = selected ?? preferred;
      const key = `${parseArtifactId}\0${chunkProfileHash}`;
      if (seen.has(key)) continue;
      seen.add(key);
      scopes.push({
        parseArtifactId,
        chunkProfileHash,
        blockFingerprint: chunkProfileHash === preferred ? knowledgeBlockFingerprint(blocks) : legacyKnowledgeBlockFingerprint(blocks),
      });
    }
    return scopes;
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
    const embedTexts = input.embeddingModelRef && this.legacyDeps.embedTextsForModel
      ? ((request: { runId: string; texts: string[]; signal?: AbortSignal }) =>
        this.legacyDeps.embedTextsForModel!({
          runId: request.runId,
          texts: request.texts,
          signal: request.signal,
          modelRef: input.embeddingModelRef!,
          // 查询侧嵌入（MiniMax db/query、Voyage input_type 的算法分离）
          inputType: "query",
        })) as KnowledgeEmbedder
      : null;
    const reranker = input.rerankModelRef && this.legacyDeps.rerankForModel
      ? ((request: { runId: string; query: string; documents: string[]; topN: number; signal?: AbortSignal }) =>
        this.legacyDeps.rerankForModel!({
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
      topK: Math.min(input.topK ?? KNOWLEDGE_CANDIDATE_GENERATION_BUDGET, KNOWLEDGE_CANDIDATE_GENERATION_BUDGET),
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
      const notebook = this.legacyDeps.store.getNotebook({ studioId, notebookId });
      const allEntries = this.legacyDeps.store.listNotebookSources({ studioId, notebookId });
      // scope 冻结：逐 membership 以冻结的 artifact 替换「最新」视图；不在冻结
      // 集合内的源（membership 在冻结后被移除等）本轮排除。冻结 artifact 的状态
      // 按当前行读取（同一 artifact 身份；parsing→ready 的推进对本轮可见）。
      const thawedEntries = input.frozenArtifacts
        ? allEntries.flatMap((entry) => {
          const frozen = input.frozenArtifacts!.get(entry.source.id);
          if (!frozen) return [];
          const parseArtifact = frozen.parseArtifactId
            ? this.legacyDeps.store.getParseArtifact({ studioId, parseArtifactId: frozen.parseArtifactId })
            : null;
          return [{ ...entry, parseArtifact }];
        })
        : allEntries;
      // §三十八 source 约束：Source Coverage Floor 的二次检索只覆盖指定源。
      const entries = input.sourceIds
        ? thawedEntries.filter(entry => input.sourceIds!.includes(entry.source.id))
        : thawedEntries;
      const resolved = resolveNotebookConfig(
        this.legacyDeps.store.getNotebookConfig({ studioId, notebookId }),
      );
      const topK = Math.min(input.topK ?? resolved.retrievalTopK ?? KNOWLEDGE_CANDIDATE_GENERATION_BUDGET, KNOWLEDGE_CANDIDATE_GENERATION_BUDGET);
      // rerank 路由（v8）：笔记本显式引用 → 按引用执行（配置不可解析由回调侧
      // 记日志并返回 null，检索显式降级 RRF 名次）；引用未配置或执行面未接线 → 不重排。
      const reranker = resolved.rerankModelRef && this.legacyDeps.rerankForModel
        ? ((request: { runId: string; query: string; documents: string[]; topN: number; signal?: AbortSignal }) =>
          this.legacyDeps.rerankForModel!({
            runId: request.runId,
            query: request.query,
            documents: request.documents,
            topN: request.topN,
            signal: request.signal,
            modelRef: resolved.rerankModelRef!,
          })) as KnowledgeReranker
        : null;
      const rerank = input.rerank ?? reranker != null;
      // 生效分块配置与摄入侧同源解析（显式身份或固定默认）：检索锚按
      // 同一 configId 纯解析，只读命中摄入侧建出的变体。
      const chunkTargetChars = resolveEffectiveChunkTargetChars(
        resolved,
        this.legacyDeps.getEmbeddingModelContextWindow,
      );
      // 查询嵌入按笔记本配置路由（与摄入侧同一模型 → 向量命中同一 model_key 分区）；
      // 笔记本未配置嵌入模型 → 该笔记本纯 FTS（禁静默换模型）。
      const embedTexts = resolved.embeddingModelRef && this.legacyDeps.embedTextsForModel
        ? ((request: { runId: string; texts: string[]; signal?: AbortSignal }) =>
          this.legacyDeps.embedTextsForModel!({
            runId: request.runId,
            texts: request.texts,
            signal: request.signal,
            modelRef: resolved.embeddingModelRef!,
            // 查询侧嵌入（MiniMax db/query、Voyage input_type 的算法分离）
            inputType: "query",
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
      const scopes = this.resolveRetrievalScopes({ studioId, artifactIds, chunkTargetChars,
        fallbackProfileHash: this.legacyDeps.store.getNotebookRetrievalProfileSnapshot({ studioId, notebookId }).chunkProfileHash });
      const scopeByArtifact = new Map(scopes.map(scope => [scope.parseArtifactId, scope]));
      for (const selected of scopes) {
        const owner = sourceByArtifact.get(selected.parseArtifactId)!;
        const blocks = this.legacyDeps.store.listArtifactBlocks({ studioId, parseArtifactId: selected.parseArtifactId });
        const desired = resolveKnowledgeChunkerConfig(blocks, { targetChars: chunkTargetChars }).configId;
        if (selected.chunkProfileHash !== desired) {
          degraded.push({ ...selected, reason: "KNOWLEDGE_INDEX_BUILDING", notebookId, notebookName: notebook.name,
            ...owner, detail: "v3 rebuild pending; serving ready v2 index" });
          this.legacyDeps.requestVariantBuild?.({ studioId, notebookId, sourceId: owner.sourceId,
            parseArtifactId: selected.parseArtifactId, reason: "KNOWLEDGE_INDEX_BUILDING" });
        }
      }
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
      let stageTimings: KnowledgeRetrievalStageTimings | undefined;
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
        if (result.stageTimings) {
          stageTimings = result.stageTimings;
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
          this.legacyDeps.requestVariantBuild?.({
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
        ...(stageTimings ? { stageTimings } : {}),
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
    // 门控跳过留痕同构汇总（主动跳过≠降级；只进 stats）。
    // 分段计时跨笔记本取各段最大值（并行执行的笔记本里最慢的那个才是关键路径）。
    const stageTimings: KnowledgeRetrievalStageTimings = {};
    for (const item of notebookResults) {
      if (!item.stageTimings) continue;
      for (const [key, value] of Object.entries(item.stageTimings)) {
        if (typeof value !== "number") continue;
        if (value > ((stageTimings as Record<string, number | undefined>)[key] ?? 0)) {
          (stageTimings as Record<string, number | undefined>)[key] = value;
        }
      }
    }
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
      ...(Object.keys(stageTimings).length > 0 ? { stageTimings } : {}),
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
    const variant = this.legacyDeps.indexStore.resolveChunkIndexVariant(input.artifactId, input.chunkProfileHash);
    if (variant?.status === "ready") {
      try {
        chunks = this.legacyDeps.indexStore.listVariantChunks(variant.id);
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
      chunks = this.legacyDeps.indexStore.listVariantChunks(input.chunkIndexVariantId);
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
      chunks = this.legacyDeps.indexStore.readVariantChunks(input.anchor.chunkIndexVariantId, input.ordinals);
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
      this.legacyDeps.store.listArtifactBlocks({ studioId, parseArtifactId }),
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

export class KnowledgeManager extends ProductionManager {
  declare readonly queryService: HistoricalKnowledgeQueryService;
  constructor(options: ConstructorParameters<typeof ProductionManager>[0]) {
    super(options);
    // 保留同一真实索引和后台生命周期，仅旧测试增加已退役的查询入口。
    Object.setPrototypeOf(this.queryService, HistoricalKnowledgeQueryService.prototype);
  }
}
