import fs from "node:fs";
import path from "node:path";
import Database from "better-sqlite3";
import { describe, expect, it } from "vitest";
import { AnnIndexStore, knowledgeAnnFileName } from "../lib/knowledge/ann-index-store.ts";
import { annFixture } from "./helpers/knowledge-ann-fixture.ts";

describe("ANN 独立目录 v1", () => {
  it("固定表结构、幂等状态流转和文件身份，绝不写 portable BLOB", async () => {
    const f = annFixture();
    try {
      const id = f.add(), before = f.blobs();
      const row = f.store.begin({ vectorIndexVariantId: id, modelKey: f.model.key, dimensions: 3, chunkFingerprint: "fp", vectorCount: 3 });
      expect(row.status).toBe("building"); expect(f.store.listInterrupted()).toEqual([id]);
      expect(row.fileName).toBe(`${f.model.key.slice(0, 16)}/${id}.usearch`);
      f.store.markReady(id); expect(f.store.get(id)?.status).toBe("ready");
      expect(() => f.store.markReady(id)).toThrow();
      f.store.markFailed(id); expect(f.store.get(id)?.status).toBe("failed");
      expect(f.store.begin({ ...row, vectorCount: 4 }).createdAt).toBe(row.createdAt);
      const db = new Database(f.annPath);
      expect(db.pragma("user_version", { simple: true })).toBe(1);
      expect(db.prepare("PRAGMA table_info(ann_variants)").all().map((r: any) => r.name)).toEqual([
        "vector_index_variant_id", "model_key", "dimensions", "chunk_fingerprint", "vector_count", "index_format_version", "file_name", "status", "created_at", "updated_at",
      ]); db.close(); expect(f.blobs()).toEqual(before);
      expect(() => knowledgeAnnFileName("../../outside", id)).toThrow();
    } finally { await f.close(); }
  });
  it("拒绝损坏目录及越界文件名", async () => {
    const f = annFixture();
    try {
      const id = f.add(); f.store.begin({ vectorIndexVariantId: id, modelKey: f.model.key, dimensions: 3, chunkFingerprint: "fp", vectorCount: 3 });
      const db = new Database(f.annPath); db.prepare("UPDATE ann_variants SET file_name = '../outside'").run(); db.close();
      expect(() => f.store.get(id)).toThrow("ANN metadata is corrupt");
      const broken = path.join(f.home, "broken.db"); fs.writeFileSync(broken, "not sqlite");
      expect(() => new AnnIndexStore({ dbPath: broken })).toThrow();
    } finally { await f.close(); }
  });
});
