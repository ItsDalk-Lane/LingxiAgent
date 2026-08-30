import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import {
  PortableVectorIndexAdapter,
  type VectorIndexModelIdentity,
} from "../lib/knowledge/vector-index-adapter.ts";

const roots: string[] = [];
const model: VectorIndexModelIdentity = {
  key: "provider-a/embed-model/openai-embeddings/3",
  provider: "provider-a",
  modelId: "embed-model",
  protocol: "openai-embeddings",
  dimensions: 3,
};

function createAdapter() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-vector-index-"));
  roots.push(root);
  return new PortableVectorIndexAdapter({ dbPath: path.join(root, "knowledge-vector.db") });
}

afterEach(() => {
  for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
});

describe("PortableVectorIndexAdapter", () => {
  it("builds, replaces, searches, removes, and reports health", () => {
    const adapter = createAdapter();
    adapter.buildOrReplaceArtifact({
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-a",
      model,
      entries: [
        { chunkId: "chunk-a", parseArtifactId: "artifact-a", ordinal: 0, vector: [1, 0, 0] },
        { chunkId: "chunk-b", parseArtifactId: "artifact-a", ordinal: 1, vector: [0, 1, 0] },
      ],
    });

    expect(adapter.hasArtifact({
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-a",
      model,
    })).toBe(true);
    expect(adapter.search({
      parseArtifactIds: ["artifact-a"],
      model,
      queryVector: [0.9, 0.1, 0],
      limit: 2,
    })).toEqual([
      expect.objectContaining({ chunkId: "chunk-a", score: expect.any(Number) }),
      expect.objectContaining({ chunkId: "chunk-b", score: expect.any(Number) }),
    ]);
    expect(adapter.health()).toEqual({ status: "ready" });

    adapter.buildOrReplaceArtifact({
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-b",
      model,
      entries: [
        { chunkId: "chunk-c", parseArtifactId: "artifact-a", ordinal: 0, vector: [0, 0, 1] },
      ],
    });
    expect(adapter.search({
      parseArtifactIds: ["artifact-a"],
      model,
      queryVector: [0, 0, 1],
    })).toEqual([expect.objectContaining({ chunkId: "chunk-c", score: 1 })]);

    adapter.removeArtifact("artifact-a");
    expect(adapter.hasArtifact({
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-b",
      model,
    })).toBe(false);
    expect(adapter.search({
      parseArtifactIds: ["artifact-a"],
      model,
      queryVector: [0, 0, 1],
    })).toEqual([]);
    adapter.close();
  });

  it("treats model dimension changes as a cache miss", () => {
    const adapter = createAdapter();
    adapter.buildOrReplaceArtifact({
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-a",
      model,
      entries: [{ chunkId: "chunk-a", parseArtifactId: "artifact-a", ordinal: 0, vector: [1, 0, 0] }],
    });
    expect(adapter.hasArtifact({
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-a",
      model: { ...model, key: "provider-a/embed-model/openai-embeddings/2", dimensions: 2 },
    })).toBe(false);
    adapter.close();
  });

  it("fails closed on corrupt vector bytes and can rebuild only the cache", () => {
    const adapter = createAdapter();
    adapter.buildOrReplaceArtifact({
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-a",
      model,
      entries: [{ chunkId: "chunk-a", parseArtifactId: "artifact-a", ordinal: 0, vector: [1, 0, 0] }],
    });
    adapter.db.prepare(`UPDATE chunk_vectors SET vector = ? WHERE chunk_id = ?`).run(Buffer.alloc(1), "chunk-a");
    expect(() => adapter.search({
      parseArtifactIds: ["artifact-a"],
      model,
      queryVector: [1, 0, 0],
    })).toThrow(/vector index is corrupt/i);

    adapter.rebuild();
    expect(adapter.health()).toEqual({ status: "ready" });
    expect(adapter.search({
      parseArtifactIds: ["artifact-a"],
      model,
      queryVector: [1, 0, 0],
    })).toEqual([]);
    adapter.close();
  });

  it("查询命中刷新 last_used_at，removeArtifactModel 细粒度删除保留同产物其他身份", () => {
    const adapter = createAdapter();
    const modelB: VectorIndexModelIdentity = { ...model, key: "provider-b/embed-b/openai-embeddings/3", modelId: "embed-b" };
    adapter.buildOrReplaceArtifact({
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-a",
      model,
      entries: [{ chunkId: "chunk-a", parseArtifactId: "artifact-a", ordinal: 0, vector: [1, 0, 0] }],
    });
    adapter.buildOrReplaceArtifact({
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-a",
      model: modelB,
      entries: [{ chunkId: "chunk-b", parseArtifactId: "artifact-a", ordinal: 0, vector: [0, 1, 0] }],
    });

    // 查询只命中 modelB 的向量，但使用时间按 artifact 维度刷新。
    const before = adapter.listArtifactUsage().find((row) => row.modelKey === model.key)!.lastUsedAt;
    adapter.search({ parseArtifactIds: ["artifact-a"], model: modelB, queryVector: [0, 1, 0] });
    const usage = adapter.listArtifactUsage();
    expect(usage).toHaveLength(2);
    for (const row of usage) {
      expect(Date.parse(row.lastUsedAt)).toBeGreaterThanOrEqual(Date.parse(before));
    }

    // 细粒度删除：只删 model 身份，modelB 保留。
    adapter.removeArtifactModel({ parseArtifactId: "artifact-a", modelKey: model.key });
    expect(adapter.listArtifactUsage().map((row) => row.modelKey)).toEqual([modelB.key]);
    expect(adapter.search({ parseArtifactIds: ["artifact-a"], model, queryVector: [1, 0, 0] })).toEqual([]);
    expect(adapter.search({ parseArtifactIds: ["artifact-a"], model: modelB, queryVector: [0, 1, 0] })).toHaveLength(1);
    adapter.close();
  });

  it("v1 库升级到 v2 时 last_used_at 回填 indexed_at", () => {
    const adapter = createAdapter();
    adapter.buildOrReplaceArtifact({
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-a",
      model,
      entries: [{ chunkId: "chunk-a", parseArtifactId: "artifact-a", ordinal: 0, vector: [1, 0, 0] }],
    });
    const indexedAt = adapter.listArtifactUsage()[0].indexedAt;
    // 压回 v1：抹掉 last_used_at 模拟旧库，重开触发 v1→v2 迁移。
    adapter.db.pragma("user_version = 1");
    adapter.db.prepare(`UPDATE vector_artifacts SET last_used_at = ''`).run();
    adapter.close();
    const reopened = new PortableVectorIndexAdapter({ dbPath: adapter.dbPath });
    expect(Number(reopened.db.pragma("user_version", { simple: true }))).toBe(2);
    expect(reopened.listArtifactUsage()[0].lastUsedAt).toBe(indexedAt);
    reopened.close();
  });
});
