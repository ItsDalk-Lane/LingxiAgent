import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";

vi.mock("../lib/i18n.js", () => ({
  getLocale: () => "zh-CN",
  t: (key: string, vars?: Record<string, string>) => {
    if (key === "error.memorySearchEmpty") return "记忆库里没有找到相关内容。";
    if (key === "error.memorySearchTermCounts") return `各检索词独立命中数：${vars?.counts}。`;
    if (key === "error.memorySearchRetryHint") return "部分检索词无命中……";
    if (key === "error.memorySearchSemanticUnavailable") return "语义检索未启用（未配置）。";
    if (key === "error.memorySearchSemanticTimeout") return "语义检索因嵌入超时被跳过。";
    if (key === "error.memorySearchSemanticNoCoverage") return "事实库尚无已嵌入向量。";
    return key;
  },
}));

import {
  cosineSimilarity,
  factEmbeddingModelKey,
  parseVector,
  serializeVector,
  rrfFuse,
} from "../lib/memory/fact-embeddings.ts";
import { FactStore } from "../lib/memory/fact-store.ts";
import { createMemorySearchTool } from "../lib/memory/memory-search.ts";

describe("fact-embeddings 纯函数", () => {
  it("serialize/parse roundtrip", () => {
    const vector = [0.1, -0.5, 0.999, 0];
    expect(parseVector(serializeVector(vector)).map((v) => Math.round(v * 1e5) / 1e5))
      .toEqual(vector.map((v) => Math.round(v * 1e5) / 1e5));
  });

  it("cosine：同向 1、反向 -1、正交 0、异长 -1", () => {
    expect(cosineSimilarity([1, 0], [2, 0])).toBeCloseTo(1);
    expect(cosineSimilarity([1, 0], [-1, 0])).toBeCloseTo(-1);
    expect(cosineSimilarity([1, 0], [0, 1])).toBeCloseTo(0);
    expect(cosineSimilarity([1, 0], [1])).toBe(-1);
  });

  it("modelKey：ref/protocol 决定，维度与字段顺序无关地稳定", () => {
    const a = factEmbeddingModelKey({ provider: "sf", id: "bge-m3" }, "openai-embeddings");
    const b = factEmbeddingModelKey({ provider: "sf", id: "bge-m3" }, "openai-embeddings");
    const c = factEmbeddingModelKey({ provider: "sf", id: "bge-m3" }, "ollama-embed");
    const d = factEmbeddingModelKey({ provider: "sf", id: "bge-large" }, "openai-embeddings");
    expect(a).toBe(b);
    expect(a).not.toBe(c);
    expect(a).not.toBe(d);
  });

  it("rrfFuse：双路命中者胜，各路按名次贡献分", () => {
    const fused = rrfFuse([[1, 2, 3], [3, 4]]);
    expect(fused[0][0]).toBe(3); // 双路命中
    const ids = fused.map(([id]) => id);
    expect(new Set(ids)).toEqual(new Set([1, 2, 3, 4]));
  });
});

describe("FactStore v3 语义层", () => {
  let tmpDir;
  let store;

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-facts-vec-"));
    store = new FactStore(path.join(tmpDir, "facts.db"));
  });

  afterEach(() => {
    store.close();
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  it("schema 升到 v3，fact_embeddings 表存在", () => {
    const version = store.db.pragma("user_version", { simple: true });
    expect(version).toBe(3);
    const tables = store.db
      .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='fact_embeddings'")
      .all();
    expect(tables).toHaveLength(1);
  });

  it("coverage / factsNeedingEmbedding / upsert / purge", () => {
    store.addBatch([
      { fact: "事实一", tags: ["a"], time: "2026-08-01" },
      { fact: "事实二", tags: ["b"], time: "2026-08-02" },
    ]);
    const modelKey = factEmbeddingModelKey({ provider: "p", id: "m" }, "openai-embeddings");
    expect(store.embeddingCoverage(modelKey)).toEqual({ embedded: 0, total: 2 });
    expect(store.factsNeedingEmbedding(modelKey, 10)).toHaveLength(2);

    const pending = store.factsNeedingEmbedding(modelKey, 10);
    store.upsertFactEmbeddings(pending.map((f, i) => ({
      factId: f.id,
      modelKey,
      dimensions: 3,
      vector: serializeVector([1, 0, i]),
    })));
    expect(store.embeddingCoverage(modelKey)).toEqual({ embedded: 2, total: 2 });
    expect(store.factsNeedingEmbedding(modelKey, 10)).toHaveLength(0);

    // 换模型分区：旧 key 的向量存在但不算新 key 覆盖
    const otherKey = factEmbeddingModelKey({ provider: "p", id: "m2" }, "openai-embeddings");
    expect(store.embeddingCoverage(otherKey)).toEqual({ embedded: 0, total: 2 });
    expect(store.purgeFactEmbeddingsOtherThan(otherKey)).toBe(2);
  });

  it("semanticSearch：按余弦排序，维度不匹配的行显式跳过", () => {
    store.addBatch([
      { fact: "用户不喜欢香菜", tags: ["饮食"], time: "2026-08-01" },
      { fact: "项目 deadline 九月底", tags: ["工作"], time: "2026-08-02" },
    ]);
    const modelKey = "mk";
    const rows = store.getAll(); // ORDER BY time DESC：rows[0]=九月底(08-02)，rows[1]=香菜(08-01)
    store.upsertFactEmbeddings([
      { factId: rows[0].id, modelKey, dimensions: 3, vector: serializeVector([0.1, 0.9, 0]) },
      { factId: rows[1].id, modelKey, dimensions: 4, vector: serializeVector([0.95, 0.05, 0, 0]) }, // 维度不符 → 跳过
    ]);
    const results = store.semanticSearch(modelKey, [0.2, 0.8, 0], 10);
    expect(results).toHaveLength(1);
    expect(results[0].fact).toBe("项目 deadline 九月底");
    expect(results[0].similarity).toBeGreaterThan(0.9);
  });

  it("删除 fact 时级联清掉向量", () => {
    store.addBatch([{ fact: "将被删除", tags: [], time: "2026-08-01" }]);
    const fact = store.getAll()[0];
    store.upsertFactEmbeddings([{ factId: fact.id, modelKey: "mk", dimensions: 2, vector: serializeVector([1, 0]) }]);
    store.delete(fact.id);
    expect(store.embeddingCoverage("mk")).toEqual({ embedded: 0, total: 0 });
  });
});

describe("search_memory 语义融合", () => {
  let tmpDir;
  let store;

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-mem-sem-"));
    store = new FactStore(path.join(tmpDir, "facts.db"));
    store.addBatch([
      { fact: "用户不喜欢香菜", tags: ["饮食偏好"], time: "2026-08-01T10:00" },
      { fact: "项目交付日期是九月底", tags: ["工作"], time: "2026-08-02T10:00" },
    ]);
  });

  afterEach(() => {
    store.close();
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  function embedAll(modelKey: string) {
    const vectors = [[0.95, 0.05], [0.1, 0.9]];
    const rows = store.getAll();
    store.upsertFactEmbeddings(rows.map((f, i) => ({
      factId: f.id,
      modelKey,
      dimensions: 2,
      vector: serializeVector(vectors[i]),
    })));
  }

  it("改述查询：FTS 零命中时语义路兜住，source=semantic", async () => {
    const modelKey = "mk-semantic";
    embedAll(modelKey);
    const tool = createMemorySearchTool(store, {
      embedQuery: async () => ({ status: "ok", vector: [0.9, 0.1], modelKey }),
    });
    // 「饮食忌口」与两条事实都无字面重叠（FTS 零命中），但向量贴近香菜事实
    const res = await tool.execute("t1", { query: "饮食忌口是什么" });
    expect((res.details as any).semantic).toBe("used");
    expect(res.content[0].text).toContain("用户不喜欢香菜");
    expect(res.content[0].text).toContain("1.");
  });

  it("FTS 与语义都命中时 RRF 融合且去重，双路命中标 semantic+fts", async () => {
    const modelKey = "mk-fuse";
    embedAll(modelKey);
    const tool = createMemorySearchTool(store, {
      embedQuery: async () => ({ status: "ok", vector: [0.95, 0.05], modelKey }),
    });
    const res = await tool.execute("t1", { query: "香菜" });
    expect((res.details as any).semantic).toBe("used");
    const text = res.content[0].text;
    expect(text).toContain("用户不喜欢香菜");
    expect(text.match(/用户不喜欢香菜/g)?.length).toBe(1); // 不重复罗列
  });

  it("未配置模型：details 标 unavailable_no_model，行为同旧版 FTS", async () => {
    const tool = createMemorySearchTool(store, {
      embedQuery: async () => ({ status: "unavailable", reason: "no_model" }),
    });
    const res = await tool.execute("t1", { query: "香菜" });
    expect((res.details as any).semantic).toBe("unavailable_no_model");
    expect(res.content[0].text).toContain("用户不喜欢香菜");
  });

  it("模型配置了但解析失败：unavailable_unresolvable", async () => {
    const tool = createMemorySearchTool(store, {
      embedQuery: async () => ({ status: "unavailable", reason: "unresolvable" }),
    });
    const res = await tool.execute("t1", { query: "香菜" });
    expect((res.details as any).semantic).toBe("unavailable_unresolvable");
  });

  it("嵌入超时：零结果时文本里带显式跳过说明", async () => {
    const tool = createMemorySearchTool(store, {
      embedQuery: async () => ({ status: "timeout" }),
    });
    const res = await tool.execute("t1", { query: "完全不相关的词xyz" });
    expect((res.details as any).semantic).toBe("skipped_timeout");
    expect(res.content[0].text).toContain("语义检索因嵌入超时被跳过");
  });

  it("零覆盖：配置了模型但一条向量都没有 → skipped_no_coverage", async () => {
    const tool = createMemorySearchTool(store, {
      embedQuery: async () => ({ status: "ok", vector: [1, 0], modelKey: "mk-empty" }),
    });
    const res = await tool.execute("t1", { query: "完全不相关的词xyz" });
    expect((res.details as any).semantic).toBe("skipped_no_coverage");
    expect(res.content[0].text).toContain("事实库尚无已嵌入向量");
  });

  it("不传 embedQuery：不出现 semantic 标记（兼容旧接线）", async () => {
    const tool = createMemorySearchTool(store);
    const res = await tool.execute("t1", { query: "香菜" });
    expect((res.details as any).semantic).toBeUndefined();
  });

  it("标签优先级不变：tag 命中仍置顶", async () => {
    const modelKey = "mk-tags";
    embedAll(modelKey);
    const tool = createMemorySearchTool(store, {
      embedQuery: async () => ({ status: "ok", vector: [0.1, 0.9], modelKey }),
    });
    const res = await tool.execute("t1", { query: "项目", tags: ["饮食偏好"] });
    const text = res.content[0].text;
    expect(text.indexOf("用户不喜欢香菜")).toBeLessThan(text.indexOf("项目交付日期"));
  });
});
