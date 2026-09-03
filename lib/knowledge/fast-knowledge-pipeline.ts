import { performance } from "node:perf_hooks";
import type { KnowledgeEvidenceSpan } from "../../shared/knowledge-evidence.ts";
import type { KnowledgeRetrievalStats } from "../../shared/knowledge-refs.ts";
import type { KnowledgeInjectionEvidence } from "./knowledge-context-injector.ts";
import type { IndexedKnowledgeChunk } from "./knowledge-index-store.ts";
import type { CompiledKnowledgeScope } from "./scope-snapshot-compiler.ts";
import type { KnowledgeTurnScope } from "./types.ts";

export const KNOWLEDGE_FAST_TOTAL_DEADLINE_MS = 1_200;
export const KNOWLEDGE_FAST_FTS_CANDIDATE_LIMIT = 24;
export const KNOWLEDGE_FAST_MAX_EVIDENCE_SPANS = 8;
export const KNOWLEDGE_FAST_PER_SPAN_MAX_TOKENS = 320;
export const KNOWLEDGE_FAST_RENDER_BUDGET_TOKENS = 2_400;

export interface FastKnowledgePackedEvidence {
  block: string;
  spans: KnowledgeEvidenceSpan[];
  usedTokens: number;
  evidence: KnowledgeInjectionEvidence;
}

/** 证据提取和打包各自独立实现；必须提供真实处理函数，没有占位回退。 */
export interface FastKnowledgeEvidenceStages {
  extractSpans(input: {
    compiledScope: CompiledKnowledgeScope;
    hits: IndexedKnowledgeChunk[];
    query: string;
    signal?: AbortSignal;
  }): KnowledgeEvidenceSpan[];
  packEvidence(input: {
    compiledScope: CompiledKnowledgeScope;
    spans: KnowledgeEvidenceSpan[];
    hits: IndexedKnowledgeChunk[];
    deadlineMs: number;
    deadlineExceeded: boolean;
  }): FastKnowledgePackedEvidence;
}

interface FastKnowledgeDependencies extends FastKnowledgeEvidenceStages {
  compile(scope: KnowledgeTurnScope): Promise<CompiledKnowledgeScope>;
  search(input: { compiledScope: CompiledKnowledgeScope; query: string; limit: number }): IndexedKnowledgeChunk[];
  now?: () => number;
}

/** 1200ms 是阶段准入期限；已开始的同步 SQL 不伪装成可中途取消。 */
export class FastKnowledgePipeline {
  private readonly deps: FastKnowledgeDependencies;

  constructor(deps: FastKnowledgeDependencies) { this.deps = deps; }

  async run(input: { question: string; scope: KnowledgeTurnScope; signal?: AbortSignal }) {
    const now = this.deps.now ?? (() => performance.now());
    const started = now();
    const deadline = started + KNOWLEDGE_FAST_TOTAL_DEADLINE_MS;
    const timings = { scopeCompileMs: 0, ftsMs: 0, spanExtractMs: 0, packMs: 0, totalMs: 0 };
    let compiledScope: CompiledKnowledgeScope | null = null;
    let hits: IndexedKnowledgeChunk[] = [];
    let spans: KnowledgeEvidenceSpan[] = [];
    let packed: FastKnowledgePackedEvidence | null = null;
    let ftsQueries = 0;
    let firstEvidenceMs: number | undefined;
    const admitted = () => {
      input.signal?.throwIfAborted();
      return now() < deadline;
    };

    if (admitted()) {
      const start = now();
      compiledScope = await this.deps.compile(input.scope);
      timings.scopeCompileMs = now() - start;
    }
    if (admitted() && compiledScope && compiledScope.readyChunkVariantIds.length > 0) {
      const start = now();
      ftsQueries = 1;
      hits = this.deps.search({
        compiledScope, query: input.question, limit: KNOWLEDGE_FAST_FTS_CANDIDATE_LIMIT,
      });
      timings.ftsMs = now() - start;
    }
    if (admitted() && compiledScope && hits.length > 0) {
      const start = now();
      spans = this.deps.extractSpans({ compiledScope, hits, query: input.question, signal: input.signal });
      timings.spanExtractMs = now() - start;
      if (spans.length > 0) firstEvidenceMs = now() - started;
    }
    if (admitted() && compiledScope && spans.length > 0) {
      const start = now();
      packed = this.deps.packEvidence({
        compiledScope, spans, hits, deadlineMs: KNOWLEDGE_FAST_TOTAL_DEADLINE_MS, deadlineExceeded: false,
      });
      timings.packMs = now() - start;
    }
    input.signal?.throwIfAborted();
    timings.totalMs = now() - started;
    const deadlineExceeded = timings.totalMs >= KNOWLEDGE_FAST_TOTAL_DEADLINE_MS;
    const selected = packed?.spans ?? [];
    const warnings = compiledScope?.warnings ?? [];
    const stats: KnowledgeRetrievalStats = {
      mode: "fast",
      scopeId: input.scope.id,
      executionPath: "fast_local",
      deadlineMs: KNOWLEDGE_FAST_TOTAL_DEADLINE_MS,
      deadlineExceeded,
      // 本管线没有任何远程模型入口，不能依据外部计数推测这个保证。
      remoteModelCalls: 0,
      vectorQueries: 0,
      rerankCalls: 0,
      vectorBackend: "none",
      ftsQueries,
      retrievalMode: ftsQueries ? "fts" : "none",
      retrievalModeRequested: "fts",
      subQueries: ftsQueries ? [input.question] : [],
      subQueryHits: ftsQueries ? [hits.length] : [],
      degraded: warnings.length > 0,
      ...(warnings.length > 0 ? { degradeReason: warnings.join("; ") } : {}),
      fusedChunks: hits.length,
      injectedChunks: selected.length,
      truncated: selected.length < spans.length || (deadlineExceeded && hits.length > 0),
      usedTokens: packed?.usedTokens ?? 0,
      budgetTokens: KNOWLEDGE_FAST_RENDER_BUDGET_TOKENS,
      scopeCompileMs: timings.scopeCompileMs,
      ...(firstEvidenceMs != null ? { timeToFirstEvidenceMs: firstEvidenceMs } : {}),
      stageTimings: {
        ftsMs: timings.ftsMs,
        plannerMs: 0,
        assembleMs: timings.spanExtractMs + timings.packMs,
        totalMs: timings.totalMs,
      },
      results: selected.map((span, index) => ({
        ordinal: index + 1,
        sourceName: span.sourceName,
        chunkOrdinal: (hits.find(hit => hit.id === span.chunkId)?.ordinal ?? 0) + 1,
        firstLine: span.text.split("\n")[0].slice(0, 240),
      })),
    };
    const block = packed?.block ?? [
      "[KnowledgeContext]",
      "Mode: fast",
      "Execution path: local FTS",
      `Scope: ${input.scope.id}`,
      `Retrieval deadline: ${KNOWLEDGE_FAST_TOTAL_DEADLINE_MS}ms`,
      `Deadline exceeded: ${deadlineExceeded ? "yes" : "no"}`,
      "",
      deadlineExceeded ? "本地快速检索达到时限，未能准备可引用证据。" : "本地快速检索未找到匹配证据。",
      "Instructions:",
      "- No validated knowledge evidence is available. State this explicitly; do not invent citations or claim a knowledge-grounded answer.",
      "[/KnowledgeContext]",
    ].join("\n");
    return {
      block: deadlineExceeded ? block.replace("Deadline exceeded: no", "Deadline exceeded: yes") : block,
      stats,
      evidence: packed?.evidence ?? { entries: [], searchedVectorVariants: [] },
      // 超时仍保留已完成阶段的候选供观测，但候选绝不冒充已校验引用。
      candidates: hits,
      timings,
    };
  }
}
