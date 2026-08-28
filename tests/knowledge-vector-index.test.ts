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
});
