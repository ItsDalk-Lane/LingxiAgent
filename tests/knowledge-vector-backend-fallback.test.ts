import fs from "node:fs";
import path from "node:path";
import Database from "better-sqlite3";
import { expect, it } from "vitest";
import { searchVectorBackend } from "../lib/knowledge/vector-search-backend.ts";
import { createKnowledgeVectorSearchBackend } from "../lib/knowledge/vector-search-backend-factory.ts";
import { annFixture } from "./helpers/knowledge-ann-fixture.ts";

it.each(["missing", "corrupt", "graph-level", "graph-key", "graph-neighbor", "fingerprint", "count", "native", "query"])("%s 显式回退 exact，保留 BLOB，并异步安排重建", async kind => {
  const f = annFixture(kind === "native" ? () => { throw new Error("native unavailable"); } : undefined);
  try {
    const id = f.add(), before = f.blobs(), backend = f.start(); await backend.whenIdle();
    if (kind !== "native") {
      const row = f.store.get(id)!;
      if (kind === "missing") fs.unlinkSync(path.join(f.root, row.fileName));
      if (kind === "corrupt") fs.writeFileSync(path.join(f.root, row.fileName), "corrupt ANN");
      if (kind.startsWith("graph-")) {
        const file = path.join(f.root, row.fileName), bytes = fs.readFileSync(file);
        const levels = 8 + 3 * 3 * 4 + 104, node = levels + 3 * 2;
        if (kind === "graph-level") bytes.writeInt16LE(-1, levels);
        if (kind === "graph-key") bytes.writeBigUInt64LE(999n, node);
        if (kind === "graph-neighbor") { bytes.writeUInt32LE(1, node + 10); bytes.writeUInt32LE(999, node + 14); }
        fs.writeFileSync(file, bytes);
      }
      if (kind === "fingerprint" || kind === "count") {
        const db = new Database(f.annPath); db.exec(kind === "count" ? "UPDATE ann_variants SET vector_count = 4" : "UPDATE ann_variants SET chunk_fingerprint = 'wrong'"); db.close();
      }
      if (kind === "query") (backend as any).loadIndex = () => ({ search() { throw new Error("query failed"); } });
    }
    const result = await searchVectorBackend(backend, { vectorIndexVariantIds: [id], model: f.model, queryVector: [1, 0, 0], limit: 1 });
    expect(result.vectorBackend).toBe("portable"); expect(result.degradedReasons[0]).toMatch(/^ANN_/);
    if (kind === "native") expect(result.degradedReasons[0]).toContain("ANN_NATIVE_UNAVAILABLE");
    expect(result.results[0].chunkId).toBe("a-0"); expect(f.blobs()).toEqual(before);
    await backend.whenIdle(); expect(f.blobs()).toEqual(before);
    expect(f.store.get(id)?.status).toBe(kind === "native" ? "failed" : "ready");
  } finally { await f.close(); }
});

it("ANN 目录损坏启动只重建派生库，不删除 portable 向量", async () => {
  const f = annFixture(); let backend;
  try {
    const id = f.add(), before = f.blobs(); f.store.close();
    fs.writeFileSync(f.annPath, "damaged database");
    backend = createKnowledgeVectorSearchBackend({ indexesRoot: f.home, portable: f.portable });
    const result = await searchVectorBackend(backend, { vectorIndexVariantIds: [id], model: f.model, queryVector: [1, 0, 0], limit: 1 });
    expect(result.results[0].chunkId).toBe("a-0"); expect(f.blobs()).toEqual(before);
  } finally { await backend?.close(); await f.close(); }
});
