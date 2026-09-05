import { expect, it, vi } from "vitest";
import { searchVectorBackend } from "../lib/knowledge/vector-search-backend.ts";
import { KNOWLEDGE_ANN_INDEX_OPTIONS, KNOWLEDGE_ANN_MAX_LOADED_BYTES } from "../lib/knowledge/usearch-vector-backend.ts";
import { annFixture } from "./helpers/knowledge-ann-fixture.ts";

it("真实原生建图、ordinal+1 身份、跨变体稳定合并；热查询不读全量 BLOB", async () => {
  const f = annFixture();
  try {
    const a = f.add("a"), b = f.add("b"), backend = f.start(); await backend.whenIdle();
    expect(KNOWLEDGE_ANN_INDEX_OPTIONS).toEqual({ metric: "cos", quantization: "f32", connectivity: 16, expansion_add: 128, expansion_search: 64, multi: false });
    const blobs = vi.spyOn(f.portable, "readReadyVectorBatch"); const exact = vi.spyOn(f.portable, "search");
    const result = await searchVectorBackend(backend, { vectorIndexVariantIds: [b, a], model: f.model, queryVector: [1, 0, 0], limit: 10 });
    expect(result.vectorBackend).toBe("hnsw"); expect(result.degradedReasons).toEqual([]);
    expect(result.results).toHaveLength(6); expect(result.results.slice(0, 2).map(row => row.chunkId)).toEqual(["a-0", "b-0"]);
    expect(result.results[0].ordinal).toBe(0); expect(result.results[0].score).toBeCloseTo(1);
    expect(blobs).not.toHaveBeenCalled(); expect(exact).not.toHaveBeenCalled(); expect(backend.cacheStats.indexes).toBe(2);
  } finally { await f.close(); }
});

it("LRU 超过 32 个淘汰最旧索引，达到估算 512MB 也淘汰", async () => {
  const f = annFixture();
  try {
    const ids = Array.from({ length: 33 }, (_, index) => f.add(`source-${index}`));
    const backend = f.start(); await backend.whenIdle();
    for (const id of ids) await backend.search({ vectorIndexVariantIds: [id], model: f.model, queryVector: [1, 0, 0], limit: 1 });
    expect(backend.cacheStats.indexes).toBe(32); expect(backend.cacheStats.estimatedBytes).toBeLessThan(KNOWLEDGE_ANN_MAX_LOADED_BYTES);
    const native = (backend as any).loadNative();
    const load = vi.spyOn(native.Index.prototype, "load");
    await backend.search({ vectorIndexVariantIds: [ids[0]], model: f.model, queryVector: [1, 0, 0], limit: 1 });
    expect(load).toHaveBeenCalledTimes(1); load.mockRestore();
    // 注入一个已占满估算容量的缓存项，再真实加载冷索引，必须清出容量。
    (backend as any).loaded.clear();
    (backend as any).loaded.set("old-large", { estimatedBytes: KNOWLEDGE_ANN_MAX_LOADED_BYTES, signature: "old", index: {} });
    const result = await searchVectorBackend(backend, { vectorIndexVariantIds: [ids[1]], model: f.model, queryVector: [1, 0, 0], limit: 1 });
    expect(result.vectorBackend).toBe("hnsw"); expect(backend.cacheStats.indexes).toBe(1);
    expect(backend.cacheStats.estimatedBytes).toBeLessThan(KNOWLEDGE_ANN_MAX_LOADED_BYTES);
  } finally { vi.restoreAllMocks(); await f.close(); }
}, 30000);
