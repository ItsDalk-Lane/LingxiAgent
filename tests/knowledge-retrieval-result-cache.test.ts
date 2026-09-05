import { describe, expect, it, vi } from "vitest";
import { RetrievalResultCache, type RetrievalResultCacheKey } from "../lib/knowledge/retrieval-result-cache.ts";

const key: RetrievalResultCacheKey = { scopeSnapshotHash: "scope-a", normalizedQuery: "查询", channel: "hybrid", filters: {}, limit: 8, rerank: false, retrievalImplementationVersion: "v1" };

describe("检索结果缓存", () => {
  it("256 条 LRU 和两分钟期限，不靠墙钟速度判断", async () => {
    let now = 0;
    const cache = new RetrievalResultCache<number>(() => now), load = vi.fn(async () => 1);
    for (let i = 0; i < 256; i += 1) await cache.getOrCreate({ ...key, normalizedQuery: String(i) }, load);
    await cache.getOrCreate({ ...key, normalizedQuery: "0" }, load);
    await cache.getOrCreate({ ...key, normalizedQuery: "256" }, load);
    expect((await cache.getOrCreate({ ...key, normalizedQuery: "0" }, load)).hit).toBe(true);
    expect((await cache.getOrCreate({ ...key, normalizedQuery: "1" }, load)).hit).toBe(false);
    now = 119_999; expect((await cache.getOrCreate({ ...key, normalizedQuery: "0" }, load)).hit).toBe(true);
    now = 120_000; expect((await cache.getOrCreate({ ...key, normalizedQuery: "0" }, load)).hit).toBe(false);
  });

  it("范围、全部过滤、条数、重排和版本均隔离；排序等价的过滤共享", async () => {
    const cache = new RetrievalResultCache<number>(), load = vi.fn(async () => 1);
    await cache.getOrCreate(key, load);
    const changes: Partial<RetrievalResultCacheKey>[] = [
      { scopeSnapshotHash: "scope-b" }, { channel: "fts" }, { normalizedQuery: "另一问" }, { limit: 12 }, { rerank: true },
      { retrievalImplementationVersion: "v2" }, { filters: { notebookIds: ["n"] } }, { filters: { sourceIds: ["s"] } },
      { filters: { sectionKeys: ["h"] } }, { filters: { sourceIds: [] } },
      { filters: { sectionsBySourceId: [["s", ["h"]]] } },
    ];
    for (const change of changes) expect((await cache.getOrCreate({ ...key, ...change }, load)).hit).toBe(false);
    await cache.getOrCreate({ ...key, filters: { sourceIds: ["a", "b"] } }, load);
    expect((await cache.getOrCreate({ ...key, filters: { sourceIds: ["b", "a", "a"] } }, load)).hit).toBe(true);
    expect(load).toHaveBeenCalledTimes(13);
  });

  it("single-flight、清除和失败重试", async () => {
    const cache = new RetrievalResultCache<number>(), gate = Promise.withResolvers<number>();
    const load = vi.fn(async () => gate.promise);
    const results = Array.from({ length: 5 }, () => cache.getOrCreate(key, load));
    gate.resolve(42);
    expect((await Promise.all(results)).map(result => result.value)).toEqual([42, 42, 42, 42, 42]);
    expect(load).toHaveBeenCalledTimes(1);
    cache.clear();
    await expect(cache.getOrCreate(key, async () => { throw new Error("retry"); })).rejects.toThrow("retry");
    expect((await cache.getOrCreate(key, async () => 7)).value).toBe(7);
  });
});
