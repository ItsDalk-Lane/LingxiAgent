import { afterEach, describe, expect, it, vi } from "vitest";
import { KNOWLEDGE_RERANK_DEADLINE_MS } from "../lib/knowledge/knowledge-query-service.ts";
import { createRerankFixture } from "./helpers/knowledge-rerank-fixture.ts";
import { KNOWLEDGE_RERANK_DISABLED_POLICY } from "../lib/knowledge/rerank-policy.ts";
const cleanups: Array<() => Promise<void>> = [];
afterEach(async () => { vi.useRealTimers(); vi.restoreAllMocks(); for (const close of cleanups.splice(0)) await close(); });
const ref = { provider: "fixture", id: "rerank" };

describe("全局融合后单次重排", () => {
  it("五个笔记本同一引用只调用一次，分层检索与补漏各执行一次", async () => {
    const rerank = vi.fn(async ({ documents }: { documents: string[] }) => ({ results: documents.map((_, i) => ({ index: documents.length - i - 1, score: documents.length - i })) }));
    const data = await createRerankFixture(Array(5).fill(ref), rerank); cleanups.push(data.close);
    const localFts = vi.spyOn(data.manager.indexStore, "search");
    const unfilteredFts = vi.spyOn(data.manager.indexStore, "searchReadyVariantIds");
    const result = await data.manager.searchService.search(data.request);
    expect(localFts).toHaveBeenCalledTimes(1); expect(unfilteredFts).toHaveBeenCalledTimes(1);
    expect(unfilteredFts.mock.calls[0][0].chunkIndexVariantIds).toEqual(expect.arrayContaining([...localFts.mock.calls[0][0].sectionIdsByChunkIndexVariantId!.keys()]));
    expect(localFts.mock.calls[0][0].sectionIdsByChunkIndexVariantId).toBeDefined();
    expect(rerank).toHaveBeenCalledTimes(1);
    expect(rerank.mock.calls[0][0].documents).toHaveLength(5);
    expect(result).toMatchObject({ rerankGroups: 1, embeddingGroups: 0, remoteModelCalls: 1, retrievalResultCacheHit: false });
  });

  it("重排只接收全局前 50 条，尾部十条保留", async () => {
    const rerank = vi.fn(async ({ documents }: { documents: string[] }) => ({ results: documents.map((_, index) => ({ index: documents.length - index - 1, score: index })) }));
    const data = await createRerankFixture([ref], rerank, true); cleanups.push(data.close);
    const baseline = await data.manager.searchService.search({ ...data.request, rerankPolicy: KNOWLEDGE_RERANK_DISABLED_POLICY });
    expect(baseline.hits).toHaveLength(60);
    const result = await data.manager.searchService.search(data.request);
    expect(rerank.mock.calls[0][0].documents).toHaveLength(50);
    expect(result.hits.slice(0, 50).map(hit => hit.chunkId)).toEqual(baseline.hits.slice(0, 50).reverse().map(hit => hit.chunkId));
    expect(result.hits.slice(50).map(hit => hit.chunkId)).toEqual(baseline.hits.slice(50).map(hit => hit.chunkId));
  });

  it.each(["network", "invalid", "null"])("重排 %s 失败保留融合次序并留痕", async kind => {
    const data = await createRerankFixture([ref, ref], async () => {
      if (kind === "network") throw new Error("network failed");
      return kind === "null" ? null : { results: [{ index: 999, score: 1 }] };
    }); cleanups.push(data.close);
    const baseline = await data.manager.searchService.search({ ...data.request, rerankPolicy: KNOWLEDGE_RERANK_DISABLED_POLICY });
    const result = await data.manager.searchService.search(data.request);
    expect(result.hits.map(hit => hit.chunkId)).toEqual(baseline.hits.map(hit => hit.chunkId));
    expect(result.degradedReasons.some(reason => reason.includes("rerank"))).toBe(true);
  });

  it("重排到固定期限后中止底层，返回已取得的全文检索结果", async () => {
    let signal: AbortSignal | undefined;
    const data = await createRerankFixture([ref], async input => { signal = input.signal; return new Promise(() => {}); }); cleanups.push(data.close);
    vi.useFakeTimers();
    const pending = data.manager.searchService.search(data.request);
    await vi.advanceTimersByTimeAsync(0);
    expect(signal).toBeDefined();
    await vi.advanceTimersByTimeAsync(KNOWLEDGE_RERANK_DEADLINE_MS);
    const result = await pending;
    expect(signal!.aborted).toBe(true);
    expect(result.hits).toHaveLength(1);
    expect(result.degradedReasons.some(reason => reason.includes("deadline"))).toBe(true);
  });
});
