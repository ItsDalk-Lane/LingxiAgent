import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import crypto from "node:crypto";
import Database from "better-sqlite3";
import { PortableVectorIndexAdapter, knowledgeChunkIndexVariantId } from "../../lib/knowledge/vector-index-adapter.ts";
import { AnnIndexStore } from "../../lib/knowledge/ann-index-store.ts";
import { UseArchVectorBackend, type KnowledgeNativeModule } from "../../lib/knowledge/usearch-vector-backend.ts";

export function annFixture(loadNative?: () => KnowledgeNativeModule) {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-ann-"));
  const portablePath = path.join(home, "knowledge-vector.db"), annPath = path.join(home, "knowledge-ann.db");
  const root = path.join(home, "knowledge-ann");
  let backend: UseArchVectorBackend | undefined;
  const portable = new PortableVectorIndexAdapter({ dbPath: portablePath,
    onReadyVariant: id => backend?.scheduleBuild(id), onInvalidateVariant: id => backend?.invalidate(id) });
  const store = new AnnIndexStore({ dbPath: annPath });
  const model = { provider: "fixture", modelId: "embedding", protocol: "openai", dimensions: 3,
    key: crypto.createHash("sha256").update("fixture").digest("hex") };
  function add(name = "a", vectors = [[1, 0, 0], [0, 1, 0], [0, 0, 1]], fingerprint = `fingerprint-${name}`) {
    return portable.buildOrReplaceArtifact({ parseArtifactId: name, chunkIndexVariantId: knowledgeChunkIndexVariantId(name, "profile"),
      model, chunkFingerprint: fingerprint, entries: vectors.map((vector, ordinal) => ({
        parseArtifactId: name, chunkId: `${name}-${ordinal}`, ordinal, vector,
      })) }).vectorIndexVariantId;
  }
  function start() { backend = new UseArchVectorBackend({ portable, store, root, loadNative }); return backend; }
  function blobs() {
    const db = new Database(portablePath);
    try { return db.prepare("SELECT vector_index_variant_id, chunk_id, hex(vector) AS bytes FROM chunk_vectors ORDER BY vector_index_variant_id, ordinal").all(); }
    finally { db.close(); }
  }
  return { home, portablePath, annPath, root, portable, store, model, add, start, blobs,
    async close() { if (backend) await backend.close(); else store.close(); portable.close(); fs.rmSync(home, { recursive: true, force: true }); },
  };
}
