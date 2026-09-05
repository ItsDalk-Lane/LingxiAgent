import { afterEach, describe, expect, it, vi } from "vitest";
import { createRerankFixture } from "./helpers/knowledge-rerank-fixture.ts";
import { KNOWLEDGE_RERANK_DISABLED_POLICY } from "../lib/knowledge/rerank-policy.ts";
const cleanups: Array<() => void> = [];
afterEach(() => { vi.restoreAllMocks(); for (const close of cleanups.splice(0)) close(); });
const a = { provider: "fixture", id: "a" }, b = { provider: "fixture", id: "b" };

describe("多个重排模型按名次合并", () => {
  it("每个 distinct 引用仅一次，跨模型原始分数变化不影响结果", async () => {
    let scale = 1;
    const rerank = vi.fn(async (input: { modelRef: { id: string }; documents: string[] }) => ({ results: input.documents.map((_, index) => ({
      index: input.documents.length - index - 1, score: (input.modelRef.id === "a" ? scale : -scale) * (index + 1),
    })) }));
    const data = await createRerankFixture([a, a, b, b, null], rerank); cleanups.push(data.close);
    const first = await data.manager.searchService.search(data.request);
    expect(first.hits).toHaveLength(5);
    expect(first.rerankGroups).toBe(2);
    expect(rerank.mock.calls.map(([input]) => input.modelRef.id).sort()).toEqual(["a", "b"]);
    data.manager.searchService.clearResults(); scale = 1_000_000;
    const second = await data.manager.searchService.search(data.request);
    expect(second.hits.map(hit => hit.chunkId)).toEqual(first.hits.map(hit => hit.chunkId));
    expect(second.hits.map(hit => hit.score)).toEqual(first.hits.map(hit => hit.score));
  });

  it("全部未配置重排时不调用，部分组失败时保留全局融合顺序", async () => {
    const unused = vi.fn(async () => ({ results: [] }));
    const plain = await createRerankFixture([null, null], unused); cleanups.push(plain.close);
    const simple = await plain.manager.searchService.search(plain.request);
    expect(unused).not.toHaveBeenCalled(); expect(simple.rerankGroups).toBe(0);
    const data = await createRerankFixture([a, b], async input => {
      if (input.modelRef.id === "b") throw new Error("unavailable");
      return { results: input.documents.map((_, index) => ({ index, score: 1 })) };
    }); cleanups.push(data.close);
    const baseline = await data.manager.searchService.search({ ...data.request, rerankPolicy: KNOWLEDGE_RERANK_DISABLED_POLICY });
    const result = await data.manager.searchService.search(data.request);
    expect(result.hits.map(hit => hit.chunkId)).toEqual(baseline.hits.map(hit => hit.chunkId));
    expect(result.degradedReasons.some(reason => reason.includes("unavailable"))).toBe(true);
  });
});
