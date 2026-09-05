import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import Database from "better-sqlite3";
import { KnowledgeIndexStore } from "../lib/knowledge/knowledge-index-store.ts";

const homes: string[] = [];
const stores: KnowledgeIndexStore[] = [];
afterEach(() => {
  for (const store of stores.splice(0)) store.close();
  for (const home of homes.splice(0)) fs.rmSync(home, { recursive: true, force: true });
});
function fixture() {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-metadata-migrate-"));
  homes.push(home);
  const store = new KnowledgeIndexStore({ dbPath: path.join(home, "fts.db") });
  stores.push(store);
  store.replaceArtifactChunks({
    parseArtifactId: "artifact", chunkProfileHash: "a".repeat(16), blockFingerprint: "fingerprint",
    chunks: [{ id: "chunk", parseArtifactId: "artifact", ordinal: 0, text: "AuroraQuokka", tokenCount: 5,
      spans: [{ blockId: "block", blockStartOffset: 0, blockEndOffset: 12, chunkStartOffset: 0, chunkEndOffset: 12 }] }],
  });
  return store;
}

describe("查询目录元数据迁移", () => {
  it("v2 升级 v3 保留变体、块、定位和 FTS；迁移阶段不扫正文回填", () => {
    const home = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-metadata-v2-")); homes.push(home);
    const dbPath = path.join(home, "fts.db");
    const old = new Database(dbPath);
    old.exec(fs.readFileSync(new URL("./fixtures/knowledge-index-v3.sql", import.meta.url), "utf8"));
    const variants = old.prepare("SELECT * FROM chunk_index_variants").all();
    const chunks = old.prepare("SELECT * FROM knowledge_chunks").all();
    // 真实 v3 只去掉目录增量，得到 v2；不从新 v4 降版本伪造旧库。
    old.exec("DROP TABLE chunk_index_variant_metadata; PRAGMA user_version = 2");
    old.close();
    const migrated = new KnowledgeIndexStore({ dbPath }); stores.push(migrated);
    expect(migrated.db.pragma("user_version", { simple: true })).toBe(4);
    expect(migrated.db.prepare("SELECT * FROM chunk_index_variants").all()).toEqual(variants);
    expect(migrated.db.prepare("SELECT row_id,id,parse_artifact_id,chunk_index_variant_id,ordinal,text,token_count,search_text,spans_json FROM knowledge_chunks").all()).toEqual(chunks);
    expect(migrated.db.prepare("SELECT section_id FROM knowledge_chunks").all()).toEqual(chunks.map(() => ({ section_id: null })));
    expect(migrated.db.prepare("SELECT COUNT(*) AS n FROM chunk_index_variant_metadata").get().n).toBe(0);
    expect(migrated.searchReadyVariantIds({ query: "AuroraQuokka", chunkIndexVariantIds: variants.map((variant: { id: string }) => variant.id), limit: 8 })).toHaveLength(1);
    expect(migrated.getReadyVariantMetadata({ parseArtifactId: "artifact-v2", chunkProfileHash: "2".repeat(16) }))
      .toMatchObject({ chunkCount: 1, metadataMissing: true, firstHeadingPath: null, sectionKeys: [] });
    expect(migrated.db.prepare("SELECT name FROM sqlite_master WHERE name = 'idx_chunk_variant_metadata_artifact'").get()).toBeTruthy();
  });

  it("块与目录一起提交，目录非法时整批回滚；删除产物同时清目录", () => {
    const store = fixture();
    const original = store.listArtifactChunks("artifact");
    expect(() => store.replaceArtifactChunks({
      parseArtifactId: "artifact", chunkProfileHash: "a".repeat(16), blockFingerprint: "replacement",
      chunks: original.map(chunk => ({ ...chunk, text: "已修改" })),
      metadata: { firstHeadingPath: [], sectionKeys: [] },
    })).toThrowError(/metadata is corrupt/);
    expect(store.listArtifactChunks("artifact")).toEqual(original);
    const variant = store.resolveChunkIndexVariant("artifact", "a".repeat(16))!;
    store.writeVariantMetadata(variant.id, { firstHeadingPath: ["标题"], sectionKeys: ["标题"] });
    store.removeArtifact("artifact");
    expect(store.db.prepare("SELECT COUNT(*) AS n FROM chunk_index_variant_metadata").get().n).toBe(0);
  });
});
