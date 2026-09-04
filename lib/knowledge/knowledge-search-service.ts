import crypto from "node:crypto";
import { performance } from "node:perf_hooks";
import { RetrievalResultCache } from "./retrieval-result-cache.ts";
import { normalizeKnowledgeQuery } from "./query-embedding-cache.ts";
import type { KnowledgeModelRef } from "./types.ts";
import { KnowledgeError } from "./errors.ts";
import type { CompiledKnowledgeScope } from "./scope-snapshot-compiler.ts";
import type { KnowledgeStore } from "./knowledge-store.ts";
import type { KnowledgeIndexStore, IndexedKnowledgeChunk, KnowledgeOrdinalRange } from "./knowledge-index-store.ts";
import {
  buildKnowledgeBlockLocatorIndex, knowledgeSectionKeyOf, fuseNotebookRankings, KNOWLEDGE_CANDIDATE_GENERATION_BUDGET,
  type KnowledgeQueryService, type RetrieveForNotebooksResult, type KnowledgeBlockLocator,
} from "./knowledge-query-service.ts";

export interface KnowledgeSearchRequest {
  compiledScope: CompiledKnowledgeScope;
  query: string;
  channel: "fts" | "hybrid";
  limit: number;
  notebookIds?: string[];
  sourceIds?: string[];
  sectionKeys?: string[];
  rerank: boolean;
  signal?: AbortSignal;
}

export interface KnowledgeSearchHit {
  candidateId: string;
  sourceId: string;
  sourceName: string;
  notebookIds: string[];
  contentSnapshotId: string;
  parseArtifactId: string;
  chunkIndexVariantId: string;
  chunkId: string;
  chunkOrdinal: number;
  headingPath: string[] | null;
  pageNumber: number | null;
  snippet: string;
  score: number;
  channels: Array<"fts" | "vector">;
}

export interface KnowledgeSearchResponse {
  hits: KnowledgeSearchHit[];
  retrievalMode: "fts" | "hybrid";
  vectorBackend: "hnsw" | "portable" | "none";
  timings: {
    scopeMs: number;
    ftsMs: number;
    embedMs?: number;
    vectorMs?: number;
    fuseMs: number;
    rerankMs?: number;
    totalMs: number;
  };
  remoteModelCalls: number;
  embeddingGroups: number;
  rerankGroups: number;
  queryEmbeddingCacheHit: boolean;
  retrievalResultCacheHit: boolean;
  degradedReasons: string[];
}

interface SearchDependencies {
  store: KnowledgeStore;
  indexStore: KnowledgeIndexStore;
  queryService: KnowledgeQueryService;
}

/** 自动注入和工具的共同入口；范围检查先于任何检索与远程调用。 */
export class KnowledgeSearchService {
  private readonly deps: SearchDependencies;
  private readonly resultCache = new RetrievalResultCache<{ response: KnowledgeSearchResponse; evidence: RetrieveForNotebooksResult }>();
  private readonly modelRevisions = new Map<string, { ref: KnowledgeModelRef; revision: string }>();

  clearResults(): void { this.resultCache.clear(); }

  refreshModelConfigurations(): void {
    for (const [key, previous] of this.modelRevisions) {
      const revision = this.deps.queryService.getModelConfigurationRevision(previous.ref);
      if (revision !== previous.revision) {
        this.deps.queryService.queryEmbeddingCache.invalidateModel(previous.ref.provider, previous.ref.id);
        this.resultCache.clear();
        this.modelRevisions.set(key, { ref: previous.ref, revision });
      }
    }
  }

  close(): void {
    this.resultCache.clear(); this.modelRevisions.clear();
    this.deps.queryService.queryEmbeddingCache.clear();
  }

  constructor(deps: SearchDependencies) { this.deps = deps; }

  async search(request: KnowledgeSearchRequest): Promise<KnowledgeSearchResponse> {
    return (await this.searchWithEvidence(request)).response;
  }

  /** 原始块只交给宿主证据加工，公共搜索结果只携带定位和摘要。 */
  async searchWithEvidence(request: KnowledgeSearchRequest, sectionsBySourceId?: ReadonlyMap<string, string[]>): Promise<{
    response: KnowledgeSearchResponse;
    evidence: RetrieveForNotebooksResult;
  }> {
    const started = performance.now();
    request.signal?.throwIfAborted();
    const callerSignal = request.signal;
    const { compiledScope: scope } = request;
    if (typeof request.query !== "string" || !request.query.trim() || request.query.length > 4000
      || !Number.isSafeInteger(request.limit) || request.limit < 1 || request.limit > 1000
      || !["fts", "hybrid"].includes(request.channel) || typeof request.rerank !== "boolean") {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Knowledge search request is invalid");
    }
    if (request.channel === "fts" && request.rerank) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Local FTS search cannot call rerank");
    }
    const frozen = this.deps.store.getTurnScope({ scopeId: scope.scopeId });
    if (!frozen || frozen.status !== "active" || frozen.studioId !== scope.studioId
      || frozen.sessionPath !== scope.sessionPath || frozen.turnId !== scope.turnId) this.violation();
    const validate = (filter: string[] | undefined, allowed: string[]) => {
      if (filter !== undefined && (!Array.isArray(filter)
        || filter.some(value => typeof value !== "string" || !allowed.includes(value)))) this.violation();
    };
    validate(scope.notebookIds, frozen!.notebookIds);
    for (const source of scope.sources) {
      const original = frozen!.sources.find(item => item.sourceId === source.sourceId);
      if (!original || source.contentSnapshotId !== original.contentSnapshotId
        || source.parseArtifactId !== original.parseArtifactId) this.violation();
      validate(source.notebookIds, original!.notebookIds);
    }
    validate(request.notebookIds, scope.notebookIds);
    validate(request.sourceIds, scope.sources.map(source => source.sourceId));
    const notebooks = scope.notebooks.filter(notebook => (request.notebookIds ?? scope.notebookIds).includes(notebook.notebookId));
    const sources = scope.sources.filter(source => (request.sourceIds ?? scope.sources.map(item => item.sourceId)).includes(source.sourceId)
      && source.notebookIds.some(id => notebooks.some(notebook => notebook.notebookId === id)));
    const variants = new Map<string, NonNullable<ReturnType<KnowledgeIndexStore["getReadyVariantMetadata"]>>>();
    for (const notebook of notebooks) {
      if (!scope.notebookIds.includes(notebook.notebookId)) this.violation();
      for (const source of sources) {
        if (source.status !== "ready" || !source.parseArtifactId || !notebook.chunkProfileHash
          || !source.notebookIds.includes(notebook.notebookId)) continue;
        const metadata = this.deps.indexStore.getReadyVariantMetadata({
          parseArtifactId: source.parseArtifactId, chunkProfileHash: notebook.chunkProfileHash,
        });
        if (metadata && scope.readyChunkVariantIds.includes(metadata.id)) variants.set(metadata.id, metadata);
      }
    }
    validate(request.sectionKeys, [...new Set([...variants.values()].flatMap(variant => variant.sectionKeys))]);
    if (sectionsBySourceId) {
      validate([...sectionsBySourceId.keys()], sources.map(source => source.sourceId));
      for (const [sourceId, keys] of sectionsBySourceId) {
        const source = sources.find(source => source.sourceId === sourceId)!;
        validate(keys, [...variants.values()].filter(variant => variant.parseArtifactId === source.parseArtifactId)
          .flatMap(variant => variant.sectionKeys));
      }
    }
    const variantIds = [...variants.keys()];
    this.refreshModelConfigurations();
    for (const notebook of notebooks) {
      for (const ref of [notebook.embeddingModelRef, notebook.rerankModelRef]) {
        if (ref) this.modelRevisions.set(JSON.stringify([ref.provider, ref.id]), {
          ref, revision: this.deps.queryService.getModelConfigurationRevision(ref),
        });
      }
    }
    const cached = await this.resultCache.getOrCreate({
      scopeSnapshotHash: scope.snapshotHash, normalizedQuery: normalizeKnowledgeQuery(request.query),
      channel: request.channel, filters: { notebookIds: request.notebookIds, sourceIds: request.sourceIds,
        sectionKeys: request.sectionKeys, sectionsBySourceId: sectionsBySourceId ? [...sectionsBySourceId] : undefined },
      limit: request.limit, rerank: request.rerank, retrievalImplementationVersion: "knowledge-search-v2",
    }, async signal => {
      request = { ...request, query: normalizeKnowledgeQuery(request.query), signal };
      let ordinalRanges: Map<string, KnowledgeOrdinalRange[]> | undefined;
      if (request.sectionKeys || sectionsBySourceId) {
        ordinalRanges = new Map();
        for (const variant of variants.values()) {
          const sourceId = sources.find(source => source.parseArtifactId === variant.parseArtifactId)!.sourceId;
          const keys = sectionsBySourceId?.get(sourceId) ?? request.sectionKeys;
          if (!keys) continue;
          const blocks = this.deps.store.listArtifactBlocks({ studioId: scope.studioId, parseArtifactId: variant.parseArtifactId });
          const locators = buildKnowledgeBlockLocatorIndex(blocks);
          const ranges: KnowledgeOrdinalRange[] = this.deps.indexStore.listVariantChunks(variant.id)
            .filter(chunk => chunk.spans.some(span => keys.includes(knowledgeSectionKeyOf(locators.get(span.blockId)?.headingPath) ?? "")))
            .map(chunk => [chunk.ordinal, chunk.ordinal]);
          ordinalRanges.set(variant.id, ranges);
        }
      }
      const timings: KnowledgeSearchResponse["timings"] = { scopeMs: performance.now() - started, ftsMs: 0, fuseMs: 0, totalMs: 0 };
      let remoteModelCalls = 0;
      let embeddingGroups = 0, rerankGroups = 0;
      let queryEmbeddingCacheHit = false;
      let candidates: IndexedKnowledgeChunk[];
      let retrievalMode: "fts" | "hybrid" = "fts";
      let searchedVectorVariants: NonNullable<RetrieveForNotebooksResult["searchedVectorVariants"]> = [];
      const degraded: RetrieveForNotebooksResult["degraded"] = [];
      const degradedReasons = [...scope.warnings];
      const rerankDegradeReasons: string[] = [];
      const ftsStart = performance.now();
      const generationLimit = request.channel === "hybrid" ? Math.min(request.limit, KNOWLEDGE_CANDIDATE_GENERATION_BUDGET) : request.limit;
      const fts = variantIds.length === 0 ? [] : ordinalRanges ? this.deps.indexStore.search({
        scopes: [...variants.values()].map(variant => ({ parseArtifactId: variant.parseArtifactId, chunkProfileHash: variant.chunkProfileHash })),
        query: request.query, limit: generationLimit, ordinalRangesByChunkIndexVariantId: ordinalRanges,
      }) : this.deps.queryService.searchCompiledScopeFts({
        compiledScope: { ...scope, readyChunkVariantIds: variantIds }, query: request.query, limit: generationLimit,
      });
      candidates = fts.map(candidate => ({ ...candidate, channels: ["fts"] }));
      timings.ftsMs = performance.now() - ftsStart;
      if (request.channel === "hybrid") {
        const groups = new Map<string, typeof notebooks>();
        for (const notebook of notebooks) {
          const ref = notebook.embeddingModelRef;
          const key = ref ? JSON.stringify([ref.provider, ref.id, this.deps.queryService.getModelConfigurationRevision(ref)]) : "none";
          groups.set(key, [...(groups.get(key) ?? []), notebook]);
        }
        const groupNotebooks = [...groups.values()].filter(members => members.some(notebook => sources.some(source =>
          source.notebookIds.includes(notebook.notebookId) && [...variants.values()].some(variant =>
            variant.parseArtifactId === source.parseArtifactId && variant.chunkProfileHash === notebook.chunkProfileHash))));
        embeddingGroups = groupNotebooks.filter(members => members[0].embeddingModelRef).length;
        const outcomes = await Promise.all(groupNotebooks.map(members => this.deps.queryService.retrieveCompiledGroup({
          compiledScope: scope, notebooks: members, variantIds, query: request.query, limit: request.limit,
          signal: request.signal, ordinalRanges, onRemoteCall: () => { remoteModelCalls += 1; },
          onEmbeddingCacheHit: () => { queryEmbeddingCacheHit = true; },
        })));
        for (const outcome of outcomes) {
          if (outcome.retrievalMode === "hybrid") retrievalMode = "hybrid";
          degraded.push(...outcome.degraded);
          searchedVectorVariants.push(...outcome.searchedVectorVariants);
          if (outcome.rerankDegradeReason) rerankDegradeReasons.push(outcome.rerankDegradeReason);
          for (const [key, value] of Object.entries(outcome.stageTimings ?? {})) {
            if (value != null) {
              const timingKey = key as keyof typeof timings;
              timings[timingKey] = Math.max(timings[timingKey] ?? 0, value);
            }
          }
        }
        const fuseStart = performance.now();
        candidates = fuseNotebookRankings([candidates, ...outcomes.map(outcome => outcome.candidates)], true)
          .map(entry => entry.chunk).slice(0, request.limit);
        timings.fuseMs += performance.now() - fuseStart;
        if (request.rerank && candidates.length > 0) {
          const rerankByRef = new Map<string, { ref: KnowledgeModelRef | null; candidates: IndexedKnowledgeChunk[] }>();
          for (const candidate of candidates) {
            const source = sources.find(source => source.parseArtifactId === candidate.parseArtifactId)!;
            const owners = notebooks.filter(notebook => source.notebookIds.includes(notebook.notebookId)
              && notebook.chunkProfileHash === variants.get(candidate.chunkIndexVariantId)!.chunkProfileHash);
            for (const notebook of owners) {
              const ref = notebook.rerankModelRef;
              const key = ref ? JSON.stringify([ref.provider, ref.id]) : "none";
              const group = rerankByRef.get(key) ?? { ref, candidates: [] };
              if (!group.candidates.some(item => item.id === candidate.id)) group.candidates.push(candidate);
              rerankByRef.set(key, group);
            }
          }
          const rerankGroupsList = [...rerankByRef.values()];
          rerankGroups = rerankGroupsList.filter(group => group.ref).length;
          if (rerankGroups > 0) {
            const ranked = await Promise.all(rerankGroupsList.map(group => this.deps.queryService.rerankCompiledCandidates({
              candidates: group.candidates, modelRef: group.ref, query: request.query, signal: request.signal,
              onRemoteCall: () => { remoteModelCalls += 1; },
            })));
            timings.rerankMs = Math.max(...ranked.map(group => group.rerankMs));
            rerankDegradeReasons.push(...ranked.flatMap(group => group.rerankDegradeReason ? [group.rerankDegradeReason] : []));
            // 任一组失败时保留整个融合序列，不能把半份重排结果伪装成全局成功。
            if (rerankDegradeReasons.length === 0) {
              const mergeStart = performance.now();
              candidates = ranked.length === 1 ? ranked[0].candidates
                : fuseNotebookRankings(ranked.map(group => group.candidates), true).map(entry => entry.chunk);
              timings.fuseMs += performance.now() - mergeStart;
            }
          }
        }
        searchedVectorVariants = [...new Map(searchedVectorVariants.map(variant => [variant.vectorIndexVariantId, variant])).values()];
        degradedReasons.push(...degraded.map(item => `${item.reason}:${item.parseArtifactId}`), ...rerankDegradeReasons);
      }
      request.signal?.throwIfAborted();
      const locators = new Map<string, KnowledgeBlockLocator>();
      for (const parseArtifactId of new Set(candidates.map(candidate => candidate.parseArtifactId))) {
        const blockIds = candidates.filter(candidate => candidate.parseArtifactId === parseArtifactId).flatMap(candidate => candidate.spans.map(span => span.blockId));
        for (const [id, locator] of buildKnowledgeBlockLocatorIndex(this.deps.store.getArtifactBlocksByIds({
          studioId: scope.studioId, parseArtifactId, blockIds,
        }))) locators.set(id, locator);
      }
      const annotated = candidates.map(candidate => {
        const source = sources.find(source => source.parseArtifactId === candidate.parseArtifactId);
        if (!source || !variants.has(candidate.chunkIndexVariantId)) this.violation();
        const notebook = notebooks.find(notebook => source!.notebookIds.includes(notebook.notebookId)
          && notebook.chunkProfileHash === variants.get(candidate.chunkIndexVariantId)!.chunkProfileHash);
        if (!notebook) this.violation();
        const locator = locators.get(candidate.spans[0]?.blockId);
        return { ...candidate, sourceId: source!.sourceId, sourceName: source!.sourceName,
          notebookId: notebook!.notebookId, notebookName: notebook!.notebookName,
          headingPath: locator?.headingPath ?? null, pageNumber: locator?.pageNumber ?? null };
      });
      const hits: KnowledgeSearchHit[] = annotated.map(candidate => {
        const source = sources.find(source => source.sourceId === candidate.sourceId)!;
        return {
          candidateId: `kc_${crypto.createHash("sha256").update(`${scope.scopeId}\0${candidate.chunkIndexVariantId}\0${candidate.id}`).digest("hex").slice(0, 32)}`,
          sourceId: source.sourceId, sourceName: source.sourceName,
          notebookIds: source.notebookIds.filter(id => notebooks.some(notebook => notebook.notebookId === id
            && notebook.chunkProfileHash === variants.get(candidate.chunkIndexVariantId)!.chunkProfileHash)),
          contentSnapshotId: source.contentSnapshotId, parseArtifactId: candidate.parseArtifactId,
          chunkIndexVariantId: candidate.chunkIndexVariantId, chunkId: candidate.id, chunkOrdinal: candidate.ordinal,
          headingPath: candidate.headingPath, pageNumber: candidate.pageNumber,
          snippet: candidate.text.slice(0, 1200), score: candidate.score, channels: candidate.channels ?? [],
        };
      });
      timings.totalMs = performance.now() - started;
      return {
        response: { hits, retrievalMode, vectorBackend: searchedVectorVariants.length ? "portable" : "none",
          timings, remoteModelCalls, embeddingGroups, rerankGroups, queryEmbeddingCacheHit, retrievalResultCacheHit: false,
          degradedReasons: [...new Set(degradedReasons)] },
        evidence: {
          embeddingGroups, rerankGroups, queryEmbeddingCacheHit, retrievalResultCacheHit: false,
          candidates: annotated,
          sources: notebooks.flatMap(notebook => sources.flatMap(source => {
            const variant = [...variants.values()].find(item => item.parseArtifactId === source.parseArtifactId
              && item.chunkProfileHash === notebook.chunkProfileHash);
            return source.notebookIds.includes(notebook.notebookId) && variant ? [{
              notebookId: notebook.notebookId, notebookName: notebook.notebookName, sourceId: source.sourceId,
              sourceName: source.sourceName, parseArtifactId: variant.parseArtifactId, chunkCount: variant.chunkCount,
              firstHeadingPath: variant.firstHeadingPath, sections: variant.sectionKeys, chunkProfileHash: variant.chunkProfileHash,
            }] : [];
          })),
          retrievalMode, retrievalModeRequested: request.channel, degraded,
          searchedVectorVariants, rerankDegradeReasons, stageTimings: timings,
        },
      };
    }, callerSignal);
    callerSignal?.throwIfAborted();
    if (this.deps.store.getTurnScope({ scopeId: scope.scopeId })?.status !== "active") this.violation();
    if (cached.hit) {
      cached.value.response.remoteModelCalls = 0;
      Object.assign(cached.value.response, { embeddingGroups: 0, rerankGroups: 0, queryEmbeddingCacheHit: false, retrievalResultCacheHit: true });
      Object.assign(cached.value.evidence, { embeddingGroups: 0, rerankGroups: 0, queryEmbeddingCacheHit: false, retrievalResultCacheHit: true });
      cached.value.response.timings = { scopeMs: performance.now() - started, ftsMs: 0, fuseMs: 0, totalMs: performance.now() - started };
      cached.value.evidence.stageTimings = { ftsMs: 0, fuseMs: 0 };
    }
    return cached.value;
  }

  private violation(): never {
    throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Knowledge search cannot expand the frozen scope");
  }
}
