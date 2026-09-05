import fs from "node:fs";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  KNOWLEDGE_FAST_RERANK_DEADLINE_MS,
  KNOWLEDGE_RERANK_CLEAR_MARGIN,
  executeKnowledgeRerankPolicy,
  knowledgeRetrievalPolicyDigest,
  normalizeKnowledgeRerankPolicy,
} from "../lib/knowledge/rerank-policy.ts";
import { RetrievalResultCache } from "../lib/knowledge/retrieval-result-cache.ts";

const candidates = [
  { id: "first", text: "第一条", score: 0.02 },
  { id: "second", text: "第二条", score: 0.012 },
];

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("知识重排策略路径一致性", () => {
  it("兼容搜索和查询核心都收敛到共享执行器且正式入口显式传递策略", () => {
    const querySource = fs.readFileSync("lib/knowledge/knowledge-query-service.ts", "utf8");
    const searchSource = fs.readFileSync("lib/knowledge/knowledge-search-service.ts", "utf8");
    const engineSource = fs.readFileSync("core/engine.ts", "utf8");
    const managerSource = fs.readFileSync("lib/knowledge/knowledge-manager.ts", "utf8");
    const searchToolSource = fs.readFileSync("lib/tools/knowledge-search-tool.ts", "utf8");

    expect(querySource.match(/executeKnowledgeRerankPolicy\(\{/g)).toHaveLength(2);
    expect(querySource).not.toContain(">= KNOWLEDGE_RERANK_CLEAR_MARGIN");
    expect(searchSource).toContain("rerankPolicy: request.rerankPolicy");
    expect(managerSource).toContain("rerankPolicy: KNOWLEDGE_RERANK_DISABLED_POLICY");
    expect(searchToolSource).toContain("? KNOWLEDGE_RERANK_ENABLED_POLICY : KNOWLEDGE_RERANK_DISABLED_POLICY");
    expect(engineSource).not.toContain("searchService.searchWithEvidence({");
  });

  it("完整策略在兼容搜索与笔记本查询中作出相同的 0.008 门控决定", async () => {
    const reranker = vi.fn(async () => ({
      results: [
        { index: 1, score: 1 },
        { index: 0, score: 0 },
      ],
    }));
    const policy = normalizeKnowledgeRerankPolicy({
      enabled: true,
      marginGate: true,
      deadlineMs: KNOWLEDGE_FAST_RERANK_DEADLINE_MS,
    });

    const compiled = await executeKnowledgeRerankPolicy({
      candidates,
      question: "问题",
      runId: "compiled",
      reranker,
      policy,
    });
    const notebook = await executeKnowledgeRerankPolicy({
      candidates,
      question: "问题",
      runId: "notebook",
      reranker,
      policy,
    });

    expect(KNOWLEDGE_RERANK_CLEAR_MARGIN).toBe(0.008);
    expect(policy).toEqual({
      enabled: true,
      marginGate: true,
      clearMargin: 0.008,
      deadlineMs: 5000,
      maxDocuments: 50,
    });
    expect(compiled).toEqual(notebook);
    expect(compiled.candidates.map(candidate => candidate.id)).toEqual(["first", "second"]);
    expect(compiled.rerankSkippedReason).toContain("margin gate");
    expect(reranker).not.toHaveBeenCalled();
  });

  it("完整策略摘要隔离 channel、enabled、margin、deadline 和文档上限", () => {
    const base = {
      channel: "hybrid" as const,
      rerankPolicy: normalizeKnowledgeRerankPolicy({ enabled: true }),
    };
    const digests = [
      knowledgeRetrievalPolicyDigest(base),
      knowledgeRetrievalPolicyDigest({ ...base, channel: "fts" }),
      knowledgeRetrievalPolicyDigest({ ...base, rerankPolicy: { ...base.rerankPolicy, enabled: false } }),
      knowledgeRetrievalPolicyDigest({ ...base, rerankPolicy: { ...base.rerankPolicy, clearMargin: 0.01 } }),
      knowledgeRetrievalPolicyDigest({ ...base, rerankPolicy: { ...base.rerankPolicy, deadlineMs: 5000 } }),
      knowledgeRetrievalPolicyDigest({ ...base, rerankPolicy: { ...base.rerankPolicy, maxDocuments: 24 } }),
    ];

    expect(new Set(digests)).toHaveLength(digests.length);
  });

  it("不同完整策略不会共享检索结果缓存", async () => {
    const cache = new RetrievalResultCache<number>();
    const load = vi.fn(async () => 1);
    const base = {
      scopeSnapshotHash: "scope",
      normalizedQuery: "问题",
      filters: {},
      limit: 12,
      retrievalImplementationVersion: "knowledge-search-v3",
    };
    const defaultDigest = knowledgeRetrievalPolicyDigest({
      channel: "hybrid",
      rerankPolicy: normalizeKnowledgeRerankPolicy({ enabled: true }),
    });
    const fastDigest = knowledgeRetrievalPolicyDigest({
      channel: "hybrid",
      rerankPolicy: normalizeKnowledgeRerankPolicy({
        enabled: true,
        marginGate: true,
        deadlineMs: KNOWLEDGE_FAST_RERANK_DEADLINE_MS,
      }),
    });

    expect((await cache.getOrCreate({ ...base, policyDigest: defaultDigest }, load)).hit).toBe(false);
    expect((await cache.getOrCreate({ ...base, policyDigest: fastDigest }, load)).hit).toBe(false);
    expect((await cache.getOrCreate({ ...base, policyDigest: defaultDigest }, load)).hit).toBe(true);
    expect(load).toHaveBeenCalledTimes(2);
  });

  it("5 秒期限中止底层重排并以稳定原因保留原顺序", async () => {
    vi.useFakeTimers();
    let rerankSignal: AbortSignal | undefined;
    const pending = executeKnowledgeRerankPolicy({
      candidates,
      question: "问题",
      runId: "deadline",
      reranker: async input => {
        rerankSignal = input.signal;
        return new Promise(() => {});
      },
      policy: normalizeKnowledgeRerankPolicy({
        enabled: true,
        deadlineMs: KNOWLEDGE_FAST_RERANK_DEADLINE_MS,
      }),
    });

    await vi.advanceTimersByTimeAsync(0);
    expect(rerankSignal).toBeDefined();
    await vi.advanceTimersByTimeAsync(KNOWLEDGE_FAST_RERANK_DEADLINE_MS);
    const result = await pending;

    expect(rerankSignal!.aborted).toBe(true);
    expect(result.candidates).toEqual(candidates);
    expect(result.rerankDegradeReason).toContain("5000ms");
    expect(result.rerankDegradeReason).toContain("kept RRF ranking");
  });

  it("传输失败显式降级且不改写候选顺序", async () => {
    const result = await executeKnowledgeRerankPolicy({
      candidates,
      question: "问题",
      runId: "failure",
      reranker: async () => {
        throw new Error("network failed");
      },
      policy: normalizeKnowledgeRerankPolicy({ enabled: true }),
    });

    expect(result.candidates).toEqual(candidates);
    expect(result.rerankDegradeReason).toBe("rerank degraded (Error: network failed); kept RRF ranking");
  });
});
