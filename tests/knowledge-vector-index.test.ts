import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import Database from "better-sqlite3";
import { afterEach, describe, expect, it } from "vitest";

import {
  knowledgeChunkIndexVariantId,
  knowledgeVectorIndexVariantId,
  LEGACY_UNKNOWN_CHUNK_PROFILE_HASH,
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
const modelB: VectorIndexModelIdentity = {
  key: "provider-b/embed-model-b/openai-embeddings/3",
  provider: "provider-b",
  modelId: "embed-model-b",
  protocol: "openai-embeddings",
  dimensions: 3,
};

function createAdapter(options?: { profileHashResolver?: (parseArtifactId: string) => string | null }) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-vector-index-"));
  roots.push(root);
  const dbPath = path.join(root, "knowledge-vector.db");
  return {
    dbPath,
    adapter: new PortableVectorIndexAdapter({ dbPath, ...options }),
  };
}

function encodeVector(values: number[]): Buffer {
  const buffer = Buffer.allocUnsafe(values.length * 4);
  values.forEach((value, index) => buffer.writeFloatLE(value, index * 4));
  return buffer;
}

/** 按 v1 schema 建库并写入旧格式数据（vector_artifacts + chunk_vectors，user_version=1）。 */
function createV1Database(dbPath: string, input: {
  artifacts: Array<{
    parseArtifactId: string;
    modelKey: string;
    chunkFingerprint: string;
    dimensions: number;
    indexedAt: string;
  }>;
  vectors: Array<{
    parseArtifactId: string;
    modelKey: string;
    chunkId: string;
    ordinal: number;
    dimensions: number;
    vector: number[];
  }>;
}) {
  const db = new Database(dbPath);
  db.exec(`
    CREATE TABLE vector_artifacts (
      parse_artifact_id TEXT NOT NULL,
      model_key TEXT NOT NULL,
      chunk_fingerprint TEXT NOT NULL,
      dimensions INTEGER NOT NULL CHECK(dimensions > 0),
      indexed_at TEXT NOT NULL,
      PRIMARY KEY(parse_artifact_id, model_key)
    );
    CREATE TABLE chunk_vectors (
      parse_artifact_id TEXT NOT NULL,
      model_key TEXT NOT NULL,
      chunk_id TEXT NOT NULL,
      ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
      dimensions INTEGER NOT NULL CHECK(dimensions > 0),
      vector BLOB NOT NULL,
      PRIMARY KEY(model_key, chunk_id),
      UNIQUE(parse_artifact_id, model_key, ordinal)
    );
    CREATE INDEX idx_chunk_vectors_scope
      ON chunk_vectors(model_key, parse_artifact_id, ordinal);
  `);
  const insertArtifact = db.prepare(`
    INSERT INTO vector_artifacts (parse_artifact_id, model_key, chunk_fingerprint, dimensions, indexed_at)
    VALUES (?, ?, ?, ?, ?)
  `);
  const insertVector = db.prepare(`
    INSERT INTO chunk_vectors (parse_artifact_id, model_key, chunk_id, ordinal, dimensions, vector)
    VALUES (?, ?, ?, ?, ?, ?)
  `);
  for (const row of input.artifacts) {
    insertArtifact.run(row.parseArtifactId, row.modelKey, row.chunkFingerprint, row.dimensions, row.indexedAt);
  }
  for (const row of input.vectors) {
    insertVector.run(
      row.parseArtifactId, row.modelKey, row.chunkId, row.ordinal, row.dimensions, encodeVector(row.vector),
    );
  }
  db.pragma("user_version = 1");
  db.close();
}

afterEach(() => {
  for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
});

describe("PortableVectorIndexAdapter（legacy 过渡锚：裸 parseArtifactId）", () => {
  it("builds, replaces, searches, removes, and reports health", () => {
    const { adapter } = createAdapter();
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
    const { adapter } = createAdapter();
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
    const { adapter } = createAdapter();
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
});

describe("variant 确定性身份", () => {
  it("derives stable civ/viv ids from their identity inputs", () => {
    const civ = knowledgeChunkIndexVariantId("artifact-a", "profile-hash-a");
    expect(civ).toMatch(/^civ_[0-9a-f]{32}$/);
    expect(knowledgeChunkIndexVariantId("artifact-a", "profile-hash-a")).toBe(civ);
    expect(knowledgeChunkIndexVariantId("artifact-a", "profile-hash-b")).not.toBe(civ);
    expect(knowledgeChunkIndexVariantId("artifact-b", "profile-hash-a")).not.toBe(civ);

    const viv = knowledgeVectorIndexVariantId(civ, model.key);
    expect(viv).toMatch(/^viv_[0-9a-f]{32}$/);
    expect(knowledgeVectorIndexVariantId(civ, model.key)).toBe(viv);
    expect(knowledgeVectorIndexVariantId(civ, modelB.key)).not.toBe(viv);
  });
});

describe("schema v1→v2 迁移", () => {
  const v1Data = {
    artifacts: [
      {
        parseArtifactId: "artifact-a",
        modelKey: model.key,
        chunkFingerprint: "fingerprint-a",
        dimensions: 3,
        indexedAt: "2026-08-01T00:00:00.000Z",
      },
      {
        parseArtifactId: "artifact-b",
        modelKey: model.key,
        chunkFingerprint: "fingerprint-b",
        dimensions: 3,
        indexedAt: "2026-08-02T00:00:00.000Z",
      },
    ],
    vectors: [
      { parseArtifactId: "artifact-a", modelKey: model.key, chunkId: "chunk-a", ordinal: 0, dimensions: 3, vector: [1, 0, 0] },
      { parseArtifactId: "artifact-a", modelKey: model.key, chunkId: "chunk-b", ordinal: 1, dimensions: 3, vector: [0, 1, 0] },
      { parseArtifactId: "artifact-b", modelKey: model.key, chunkId: "chunk-c", ordinal: 0, dimensions: 3, vector: [0, 0, 1] },
    ],
  };

  it("backfills variant identity with the resolver and keeps every vector", () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-vector-index-"));
    roots.push(root);
    const dbPath = path.join(root, "knowledge-vector.db");
    createV1Database(dbPath, v1Data);

    const adapter = new PortableVectorIndexAdapter({
      dbPath,
      profileHashResolver: (parseArtifactId) => (
        parseArtifactId === "artifact-a" ? "profile-hash-a" : null
      ),
    });

    expect(adapter.db.pragma("user_version", { simple: true })).toBe(2);
    const tables = adapter.db.prepare(
      `SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name`,
    ).all().map((row: any) => row.name);
    expect(tables).toContain("vector_index_variants");
    expect(tables).toContain("chunk_vectors");
    expect(tables).not.toContain("vector_artifacts");

    const civA = knowledgeChunkIndexVariantId("artifact-a", "profile-hash-a");
    const vivA = knowledgeVectorIndexVariantId(civA, model.key);
    expect(adapter.getVariant(vivA)).toEqual({
      id: vivA,
      chunkIndexVariantId: civA,
      parseArtifactId: "artifact-a",
      modelKey: model.key,
      chunkFingerprint: "fingerprint-a",
      dimensions: 3,
      status: "ready",
      createdAt: "2026-08-01T00:00:00.000Z",
      updatedAt: expect.any(String),
    });

    // resolver 返回 null 的 artifact-b 以 legacy_unknown 建档，向量不丢。
    const civB = knowledgeChunkIndexVariantId("artifact-b", LEGACY_UNKNOWN_CHUNK_PROFILE_HASH);
    const vivB = knowledgeVectorIndexVariantId(civB, model.key);
    expect(adapter.getVariant(vivB)).toEqual(expect.objectContaining({
      chunkIndexVariantId: civB,
      chunkFingerprint: "fingerprint-b",
      status: "ready",
    }));

    const vectorRows = adapter.db.prepare(`
      SELECT vector_index_variant_id, chunk_id FROM chunk_vectors ORDER BY chunk_id
    `).all();
    expect(vectorRows).toEqual([
      { vector_index_variant_id: vivA, chunk_id: "chunk-a" },
      { vector_index_variant_id: vivA, chunk_id: "chunk-b" },
      { vector_index_variant_id: vivB, chunk_id: "chunk-c" },
    ]);

    // 迁移后向量逐字节可用：variant scope 搜索命中原向量。
    expect(adapter.search({
      vectorIndexVariantIds: [vivA],
      model,
      queryVector: [1, 0, 0],
    })).toEqual([
      expect.objectContaining({ chunkId: "chunk-a", score: 1, vectorIndexVariantId: vivA }),
      expect.objectContaining({ chunkId: "chunk-b", vectorIndexVariantId: vivA }),
    ]);
    // 幂等判定在迁移后仍然命中（不触发重嵌）：显式锚与 legacy 锚都成立。
    expect(adapter.hasArtifact({
      chunkIndexVariantId: civA,
      chunkFingerprint: "fingerprint-a",
      model,
    })).toBe(true);
    expect(adapter.hasArtifact({
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-a",
      model,
    })).toBe(true);
    adapter.close();
  });

  it("falls back to legacy_unknown for every artifact when no resolver is injected", () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-vector-index-"));
    roots.push(root);
    const dbPath = path.join(root, "knowledge-vector.db");
    createV1Database(dbPath, v1Data);

    const adapter = new PortableVectorIndexAdapter({ dbPath });
    const rows = adapter.db.prepare(`
      SELECT id, chunk_index_variant_id, parse_artifact_id FROM vector_index_variants ORDER BY parse_artifact_id
    `).all();
    expect(rows).toEqual(["artifact-a", "artifact-b"].map((parseArtifactId) => {
      const civ = knowledgeChunkIndexVariantId(parseArtifactId, LEGACY_UNKNOWN_CHUNK_PROFILE_HASH);
      return {
        id: knowledgeVectorIndexVariantId(civ, model.key),
        chunk_index_variant_id: civ,
        parse_artifact_id: parseArtifactId,
      };
    }));
    expect(adapter.db.prepare(`SELECT COUNT(*) AS count FROM chunk_vectors`).get().count).toBe(3);
    adapter.close();
  });
});

describe("多 variant 共存（同 artifact 不同 profile / 不同 model）", () => {
  it("keeps variants for distinct profiles and models side by side", () => {
    const { adapter } = createAdapter();
    const civA = knowledgeChunkIndexVariantId("artifact-a", "profile-hash-a");
    const civB = knowledgeChunkIndexVariantId("artifact-a", "profile-hash-b");
    const vivA1 = knowledgeVectorIndexVariantId(civA, model.key);
    const vivA2 = knowledgeVectorIndexVariantId(civA, modelB.key);
    const vivB1 = knowledgeVectorIndexVariantId(civB, model.key);

    adapter.buildOrReplaceArtifact({
      chunkIndexVariantId: civA,
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-a",
      model,
      entries: [{ chunkId: "chunk-a", parseArtifactId: "artifact-a", ordinal: 0, vector: [1, 0, 0] }],
    });
    adapter.buildOrReplaceArtifact({
      chunkIndexVariantId: civA,
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-a",
      model: modelB,
      entries: [{ chunkId: "chunk-a", parseArtifactId: "artifact-a", ordinal: 0, vector: [0, 1, 0] }],
    });
    adapter.buildOrReplaceArtifact({
      chunkIndexVariantId: civB,
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-b",
      model,
      entries: [{ chunkId: "chunk-b", parseArtifactId: "artifact-a", ordinal: 0, vector: [0, 0, 1] }],
    });

    expect(adapter.db.prepare(`SELECT COUNT(*) AS count FROM vector_index_variants`).get().count).toBe(3);

    // search 以 variant id 列表为锚：只命中该 variant 的向量。
    expect(adapter.search({
      vectorIndexVariantIds: [vivA1],
      model,
      queryVector: [1, 0, 0],
    })).toEqual([expect.objectContaining({ chunkId: "chunk-a", vectorIndexVariantId: vivA1 })]);
    expect(adapter.search({
      vectorIndexVariantIds: [vivB1],
      model,
      queryVector: [1, 0, 0],
    })).toEqual([expect.objectContaining({ chunkId: "chunk-b", vectorIndexVariantId: vivB1 })]);
    expect(adapter.search({
      vectorIndexVariantIds: [vivA2],
      model: modelB,
      queryVector: [0, 1, 0],
    })).toEqual([expect.objectContaining({ chunkId: "chunk-a", vectorIndexVariantId: vivA2 })]);

    // 重建 civA/model 的 variant 不影响其他 variant 的向量。
    adapter.buildOrReplaceArtifact({
      chunkIndexVariantId: civA,
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-a2",
      model,
      entries: [{ chunkId: "chunk-a2", parseArtifactId: "artifact-a", ordinal: 0, vector: [1, 0, 0] }],
    });
    expect(adapter.search({
      vectorIndexVariantIds: [vivA1],
      model,
      queryVector: [1, 0, 0],
    })).toEqual([expect.objectContaining({ chunkId: "chunk-a2" })]);
    expect(adapter.search({
      vectorIndexVariantIds: [vivB1],
      model,
      queryVector: [0, 0, 1],
    })).toEqual([expect.objectContaining({ chunkId: "chunk-b", score: 1 })]);

    // removeVariant 只清指定 variant。
    adapter.removeVariant(vivB1);
    expect(adapter.getVariant(vivB1)).toBeNull();
    expect(adapter.search({
      vectorIndexVariantIds: [vivB1],
      model,
      queryVector: [0, 0, 1],
    })).toEqual([]);
    expect(adapter.hasArtifact({
      chunkIndexVariantId: civA,
      chunkFingerprint: "fingerprint-a2",
      model,
    })).toBe(true);
    adapter.close();
  });
});

describe("不重复 embedding 语义", () => {
  it("hasArtifact hit lets the caller skip re-embedding; fingerprint or model change misses", () => {
    const { adapter } = createAdapter();
    const civ = knowledgeChunkIndexVariantId("artifact-a", "profile-hash-a");
    adapter.buildOrReplaceArtifact({
      chunkIndexVariantId: civ,
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-a",
      model,
      entries: [{ chunkId: "chunk-a", parseArtifactId: "artifact-a", ordinal: 0, vector: [1, 0, 0] }],
    });

    // 命中：同 (civ, model, fingerprint, dimensions) → 调用方跳过嵌入。
    expect(adapter.hasArtifact({
      chunkIndexVariantId: civ,
      chunkFingerprint: "fingerprint-a",
      model,
    })).toBe(true);
    expect(adapter.hasArtifact({
      vectorIndexVariantId: knowledgeVectorIndexVariantId(civ, model.key),
      chunkFingerprint: "fingerprint-a",
      model,
    })).toBe(true);

    // 未命中：chunk 内容变了（新 fingerprint）、chunk profile 变了（新 civ）、模型变了。
    expect(adapter.hasArtifact({
      chunkIndexVariantId: civ,
      chunkFingerprint: "fingerprint-b",
      model,
    })).toBe(false);
    expect(adapter.hasArtifact({
      chunkIndexVariantId: knowledgeChunkIndexVariantId("artifact-a", "profile-hash-b"),
      chunkFingerprint: "fingerprint-a",
      model,
    })).toBe(false);
    expect(adapter.hasArtifact({
      chunkIndexVariantId: civ,
      chunkFingerprint: "fingerprint-a",
      model: modelB,
    })).toBe(false);

    // 非 ready 状态不算命中。
    const viv = knowledgeVectorIndexVariantId(civ, model.key);
    adapter.setVariantStatus(viv, "retiring");
    expect(adapter.hasArtifact({
      chunkIndexVariantId: civ,
      chunkFingerprint: "fingerprint-a",
      model,
    })).toBe(false);
    expect(adapter.getVariant(viv)?.status).toBe("retiring");
    adapter.close();
  });

  it("rejects a vectorIndexVariantId that does not match its identity inputs", () => {
    const { adapter } = createAdapter();
    const civ = knowledgeChunkIndexVariantId("artifact-a", "profile-hash-a");
    expect(() => adapter.buildOrReplaceArtifact({
      vectorIndexVariantId: knowledgeVectorIndexVariantId(civ, modelB.key),
      chunkIndexVariantId: civ,
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-a",
      model,
      entries: [{ chunkId: "chunk-a", parseArtifactId: "artifact-a", ordinal: 0, vector: [1, 0, 0] }],
    })).toThrow(/does not match its identity inputs/i);
    adapter.close();
  });
});

describe("批级 checkpoint 构建协议（Phase 3，§十四/§十五）", () => {
  const civ = knowledgeChunkIndexVariantId("artifact-a", "profile-hash-a");

  function makeEntries(count: number): Array<{ chunkId: string; ordinal: number; vector: number[] }> {
    return Array.from({ length: count }, (_, index) => ({
      chunkId: `chunk-${index}`,
      ordinal: index,
      vector: [1, (index % 10) / 10, 0],
    }));
  }

  it("80/20 断点：80 块落库后进程重启，diff 只补 81–100，完整性校验后 ready", () => {
    const { dbPath, adapter } = createAdapter();
    const viv = knowledgeVectorIndexVariantId(civ, model.key);
    const entries = makeEntries(100);

    // begin：ensure variant 行、status=building、记录指纹/维度。
    expect(adapter.beginVectorVariantBuild({
      chunkIndexVariantId: civ,
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-a",
      model,
    })).toEqual({ vectorIndexVariantId: viv, resetStaleVectors: false });
    expect(adapter.getVariant(viv)).toMatchObject({
      status: "building",
      chunkFingerprint: "fingerprint-a",
      dimensions: 3,
    });
    // building 中途不算命中（查询侧只读 ready）。
    expect(adapter.hasArtifact({ chunkIndexVariantId: civ, chunkFingerprint: "fingerprint-a", model })).toBe(false);

    // 80 块分批发成功并落库后进程退出（close 不重开同事务——每批已各自持久化）。
    adapter.upsertChunkVectorBatch({
      vectorIndexVariantId: viv,
      chunkFingerprint: "fingerprint-a",
      model,
      entries: entries.slice(0, 64),
    });
    adapter.upsertChunkVectorBatch({
      vectorIndexVariantId: viv,
      chunkFingerprint: "fingerprint-a",
      model,
      entries: entries.slice(64, 80),
    });
    expect(adapter.listVariantVectorChunkIds(viv)).toHaveLength(80);
    adapter.close();

    // 重启：重开同一库，指纹/维度一致 → begin 保留断点向量（不重建）。
    const reopened = new PortableVectorIndexAdapter({ dbPath });
    expect(reopened.beginVectorVariantBuild({
      chunkIndexVariantId: civ,
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-a",
      model,
    })).toEqual({ vectorIndexVariantId: viv, resetStaleVectors: false });
    const persisted = new Set(reopened.listVariantVectorChunkIds(viv));
    expect(persisted.size).toBe(80);
    // 恢复 diff：缺失集合恰好是 81–100（ordinal 80..99）。
    const missing = entries.filter(entry => !persisted.has(entry.chunkId));
    expect(missing.map(entry => entry.ordinal)).toEqual(
      Array.from({ length: 20 }, (_, index) => 80 + index),
    );
    // 与断点重叠的重试批幂等（INSERT OR REPLACE）：重写 0..79 不产生重复行。
    reopened.upsertChunkVectorBatch({
      vectorIndexVariantId: viv,
      chunkFingerprint: "fingerprint-a",
      model,
      entries: entries.slice(0, 80),
    });
    expect(reopened.listVariantVectorChunkIds(viv)).toHaveLength(80);
    // 只补缺失的 20 块，完整性校验通过后 ready。
    reopened.upsertChunkVectorBatch({
      vectorIndexVariantId: viv,
      chunkFingerprint: "fingerprint-a",
      model,
      entries: missing,
    });
    reopened.completeVectorVariantBuild({
      vectorIndexVariantId: viv,
      chunkFingerprint: "fingerprint-a",
      expectedChunkCount: 100,
      model,
    });
    expect(reopened.getVariant(viv)?.status).toBe("ready");
    expect(reopened.hasArtifact({ chunkIndexVariantId: civ, chunkFingerprint: "fingerprint-a", model })).toBe(true);
    expect(reopened.db.prepare(
      `SELECT COUNT(*) AS count FROM chunk_vectors WHERE vector_index_variant_id = ?`,
    ).get(viv).count).toBe(100);
    reopened.close();
  });

  it("防混写守卫：指纹/模型/维度与 variant 记录不符的写入被拒绝", () => {
    const { adapter } = createAdapter();
    const viv = knowledgeVectorIndexVariantId(civ, model.key);
    adapter.beginVectorVariantBuild({
      chunkIndexVariantId: civ,
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-a",
      model,
    });
    adapter.upsertChunkVectorBatch({
      vectorIndexVariantId: viv,
      chunkFingerprint: "fingerprint-a",
      model,
      entries: makeEntries(4),
    });
    // 指纹不符（内容漂移未走 begin 重置）→ 拒绝，已有向量不受影响。
    expect(() => adapter.upsertChunkVectorBatch({
      vectorIndexVariantId: viv,
      chunkFingerprint: "fingerprint-b",
      model,
      entries: [{ chunkId: "chunk-x", ordinal: 4, vector: [0, 0, 1] }],
    })).toThrow(/fingerprint mismatch/i);
    // 模型身份不符 → 拒绝。
    expect(() => adapter.upsertChunkVectorBatch({
      vectorIndexVariantId: viv,
      chunkFingerprint: "fingerprint-a",
      model: modelB,
      entries: [{ chunkId: "chunk-x", ordinal: 4, vector: [0, 0, 1] }],
    })).toThrow(/model mismatch/i);
    // 不存在的 variant → 拒绝。
    expect(() => adapter.upsertChunkVectorBatch({
      vectorIndexVariantId: knowledgeVectorIndexVariantId(civ, modelB.key),
      chunkFingerprint: "fingerprint-a",
      model: modelB,
      entries: [{ chunkId: "chunk-x", ordinal: 0, vector: [0, 0, 1] }],
    })).toThrow(/does not exist/i);
    expect(adapter.listVariantVectorChunkIds(viv)).toHaveLength(4);
    adapter.close();
  });

  it("complete 完整性校验不过则保持 building；fail 保留已落库向量", () => {
    const { adapter } = createAdapter();
    const viv = knowledgeVectorIndexVariantId(civ, model.key);
    adapter.beginVectorVariantBuild({
      chunkIndexVariantId: civ,
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-a",
      model,
    });
    adapter.upsertChunkVectorBatch({
      vectorIndexVariantId: viv,
      chunkFingerprint: "fingerprint-a",
      model,
      entries: makeEntries(80),
    });
    // 缺 20 块：完整性校验显式拒绝，variant 保持 building，向量不丢。
    expect(() => adapter.completeVectorVariantBuild({
      vectorIndexVariantId: viv,
      chunkFingerprint: "fingerprint-a",
      expectedChunkCount: 100,
      model,
    })).toThrow(/incomplete/i);
    expect(adapter.getVariant(viv)?.status).toBe("building");
    expect(adapter.listVariantVectorChunkIds(viv)).toHaveLength(80);
    // 指纹不符的 complete 同样拒绝（防混写）。
    expect(() => adapter.completeVectorVariantBuild({
      vectorIndexVariantId: viv,
      chunkFingerprint: "fingerprint-b",
      expectedChunkCount: 80,
    })).toThrow(/fingerprint mismatch/i);

    // 显式失败终态：status=failed，已落库向量保留（付费产物不删）。
    adapter.failVectorVariantBuild(viv);
    expect(adapter.getVariant(viv)?.status).toBe("failed");
    expect(adapter.listVariantVectorChunkIds(viv)).toHaveLength(80);
    expect(adapter.hasArtifact({ chunkIndexVariantId: civ, chunkFingerprint: "fingerprint-a", model })).toBe(false);
    adapter.close();
  });

  it("指纹/维度漂移：begin 显式清旧向量重建并报告 resetStaleVectors", () => {
    const { adapter } = createAdapter();
    const viv = knowledgeVectorIndexVariantId(civ, model.key);
    adapter.beginVectorVariantBuild({
      chunkIndexVariantId: civ,
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-a",
      model,
    });
    adapter.upsertChunkVectorBatch({
      vectorIndexVariantId: viv,
      chunkFingerprint: "fingerprint-a",
      model,
      entries: makeEntries(2),
    });
    adapter.completeVectorVariantBuild({
      vectorIndexVariantId: viv,
      chunkFingerprint: "fingerprint-a",
      expectedChunkCount: 2,
      model,
    });
    expect(adapter.hasArtifact({ chunkIndexVariantId: civ, chunkFingerprint: "fingerprint-a", model })).toBe(true);

    // chunk 内容变化（新指纹）：旧向量对新内容已失效，显式清除，不混入新构建。
    expect(adapter.beginVectorVariantBuild({
      chunkIndexVariantId: civ,
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-b",
      model,
    })).toEqual({ vectorIndexVariantId: viv, resetStaleVectors: true });
    expect(adapter.getVariant(viv)).toMatchObject({ status: "building", chunkFingerprint: "fingerprint-b" });
    expect(adapter.listVariantVectorChunkIds(viv)).toHaveLength(0);
    expect(adapter.hasArtifact({ chunkIndexVariantId: civ, chunkFingerprint: "fingerprint-a", model })).toBe(false);

    // 维度漂移（同 model_key 维度变了）同样显式重建。
    adapter.upsertChunkVectorBatch({
      vectorIndexVariantId: viv,
      chunkFingerprint: "fingerprint-b",
      model,
      entries: makeEntries(2),
    });
    expect(adapter.beginVectorVariantBuild({
      chunkIndexVariantId: civ,
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-b",
      model: { ...model, dimensions: 5 },
    }).resetStaleVectors).toBe(true);
    expect(adapter.listVariantVectorChunkIds(viv)).toHaveLength(0);
    expect(adapter.getVariant(viv)).toMatchObject({ status: "building", dimensions: 5 });
    adapter.close();
  });

  it("listVariantsByChunkIndexVariant 列出全部状态/模型的变体（恢复 diff 只读面）", () => {
    const { adapter } = createAdapter();
    adapter.beginVectorVariantBuild({
      chunkIndexVariantId: civ,
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-a",
      model,
    });
    adapter.beginVectorVariantBuild({
      chunkIndexVariantId: civ,
      parseArtifactId: "artifact-a",
      chunkFingerprint: "fingerprint-a",
      model: modelB,
    });
    const variants = adapter.listVariantsByChunkIndexVariant(civ);
    expect(variants.map(variant => variant.id).sort()).toEqual([
      knowledgeVectorIndexVariantId(civ, model.key),
      knowledgeVectorIndexVariantId(civ, modelB.key),
    ].sort());
    expect(variants.every(variant => variant.status === "building")).toBe(true);
    expect(adapter.listVariantsByChunkIndexVariant(
      knowledgeChunkIndexVariantId("artifact-b", "profile-hash-a"),
    )).toEqual([]);
    adapter.close();
  });
});
