import { describe, expect, it, vi } from "vitest";
import { QueryEmbeddingCache, type QueryEmbeddingCacheKey } from "../lib/knowledge/query-embedding-cache.ts";

const key: QueryEmbeddingCacheKey = { normalizedQuery: "知识 查询", provider: "p", modelId: "m", modelConfigurationRevision: "1", inputType: "query" };
const value = () => ({ vectors: [[1, 0]], dimensions: 2, model: { provider: "p", id: "m", api: "openai" } });

describe("查询嵌入缓存", () => {
  it("规范化查询共享结果，十分钟到期；返回值修改不污染缓存", async () => {
    let now = 0;
    const cache = new QueryEmbeddingCache(() => now), load = vi.fn(async () => value());
    const first = await cache.getOrCreate(key, load);
    first.value.vectors[0][0] = 9;
    expect((await cache.getOrCreate({ ...key, normalizedQuery: "  知识\n 查询  " }, load)).value.vectors).toEqual([[1, 0]]);
    now = 599_999; expect((await cache.getOrCreate(key, load)).hit).toBe(true);
    now = 600_000; expect((await cache.getOrCreate(key, load)).hit).toBe(false);
    expect(load).toHaveBeenCalledTimes(2);
  });

  it("512 条 LRU，读取更新访问次序", async () => {
    const cache = new QueryEmbeddingCache(), load = vi.fn(async () => value());
    for (let i = 0; i < 512; i += 1) await cache.getOrCreate({ ...key, normalizedQuery: String(i) }, load);
    await cache.getOrCreate({ ...key, normalizedQuery: "0" }, load);
    await cache.getOrCreate({ ...key, normalizedQuery: "512" }, load);
    expect((await cache.getOrCreate({ ...key, normalizedQuery: "0" }, load)).hit).toBe(true);
    expect((await cache.getOrCreate({ ...key, normalizedQuery: "1" }, load)).hit).toBe(false);
    expect(load).toHaveBeenCalledTimes(514);
  });

  it("模型引用和配置修订隔离，定向失效不清除其他模型", async () => {
    const cache = new QueryEmbeddingCache(), load = vi.fn(async () => value());
    const keys = [key, { ...key, modelId: "other" }, { ...key, provider: "other" }, { ...key, modelConfigurationRevision: "2" }];
    for (const item of keys) await cache.getOrCreate(item, load);
    expect(load).toHaveBeenCalledTimes(4);
    cache.invalidateModel("p", "m");
    expect((await cache.getOrCreate(keys[1], load)).hit).toBe(true);
    expect((await cache.getOrCreate(keys[2], load)).hit).toBe(true);
    expect((await cache.getOrCreate(key, load)).hit).toBe(false);
    expect((await cache.getOrCreate(keys[3], load)).hit).toBe(false);
  });

  it("并发只执行一次，一个等待者取消不影响另一个", async () => {
    const cache = new QueryEmbeddingCache(), gate = Promise.withResolvers<ReturnType<typeof value>>();
    let underlying: AbortSignal | undefined;
    const load = vi.fn(async (signal: AbortSignal) => { underlying = signal; return gate.promise; });
    const controller = new AbortController();
    const first = cache.getOrCreate(key, load, controller.signal);
    const second = cache.getOrCreate(key, load);
    await Promise.resolve(); controller.abort();
    await expect(first).rejects.toMatchObject({ name: "AbortError" });
    expect(underlying!.aborted).toBe(false);
    gate.resolve(value());
    expect((await second).value).toEqual(value()); expect(load).toHaveBeenCalledTimes(1);
  });

  it("全部等待者取消后晚到结果不写缓存，失败也不缓存", async () => {
    const cache = new QueryEmbeddingCache(), gate = Promise.withResolvers<ReturnType<typeof value>>();
    let underlying: AbortSignal | undefined;
    const controller = new AbortController();
    const pending = cache.getOrCreate(key, async signal => { underlying = signal; return gate.promise; }, controller.signal);
    await Promise.resolve(); controller.abort();
    await expect(pending).rejects.toMatchObject({ name: "AbortError" });
    expect(underlying!.aborted).toBe(true);
    gate.resolve(value()); await gate.promise;
    const load = vi.fn(async () => { throw new Error("failed"); });
    await expect(cache.getOrCreate(key, load)).rejects.toThrow("failed");
    await expect(cache.getOrCreate(key, load)).rejects.toThrow("failed");
    expect(load).toHaveBeenCalledTimes(2);
    expect((await cache.getOrCreate(key, async () => value())).hit).toBe(false);
  });
});
