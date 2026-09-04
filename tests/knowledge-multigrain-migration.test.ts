import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import Database from "better-sqlite3";
import { afterEach, describe, expect, it } from "vitest";
import { KnowledgeIndexStore, knowledgeChunkIndexVariantId } from "../lib/knowledge/knowledge-index-store.ts";
import { PortableVectorIndexAdapter, type VectorIndexModelIdentity } from "../lib/knowledge/vector-index-adapter.ts";

const roots: string[] = []; const stores: KnowledgeIndexStore[] = [];
afterEach(() => { for (const store of stores.splice(0)) store.close(); for (const dir of roots.splice(0)) fs.rmSync(dir, { recursive: true, force: true }); });
const fixtureSql = fs.readFileSync(new URL("./fixtures/knowledge-index-v3.sql", import.meta.url), "utf8");
function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-multigrain-migration-")); roots.push(root);
  const dbPath = path.join(root, "knowledge-fts.db"); const db = new Database(dbPath); db.exec(fixtureSql); db.close(); return { root, dbPath };
}
function oldRows(db: Database.Database) {
  return { variants: db.prepare("SELECT * FROM chunk_index_variants ORDER BY id").all(),
    chunks: db.prepare("SELECT row_id,id,parse_artifact_id,chunk_index_variant_id,ordinal,text,token_count,search_text,spans_json FROM knowledge_chunks ORDER BY id").all(),
    metadata: db.prepare("SELECT * FROM chunk_index_variant_metadata ORDER BY chunk_index_variant_id").all(),
    fts: db.prepare("SELECT rowid,text,search_text FROM knowledge_chunks_fts ORDER BY rowid").all() };
}
function open(dbPath: string) { const store = new KnowledgeIndexStore({ dbPath }); stores.push(store); return store; }
function legacyHit(store: KnowledgeIndexStore) { return store.search({ scopes: [{ parseArtifactId: "artifact-v2", chunkProfileHash: "2222222222222222" }], query: "AuroraQuokka" }); }

describe("真实 v3 来源／章节索引迁移", () => {
  it("fixture 真实旧库无章节列，迁移后原变体、正文、定位、目录与 FTS 不变", () => {
    const f = fixture(); const raw = new Database(f.dbPath); expect(raw.pragma("user_version", { simple: true })).toBe(3);
    expect(raw.prepare("PRAGMA table_info(knowledge_chunks)").all().some((row: any) => row.name === "section_id")).toBe(false);
    expect(raw.prepare("SELECT name FROM sqlite_master WHERE name = 'knowledge_sections'").get()).toBeUndefined();
    const before = oldRows(raw); raw.close(); const migrated = open(f.dbPath);
    expect(migrated.db.pragma("user_version", { simple: true })).toBe(4); expect(oldRows(migrated.db)).toEqual(before);
    expect(migrated.db.prepare("SELECT section_id FROM knowledge_chunks").all()).toEqual([{ section_id: null }, { section_id: null }]);
    expect(migrated.listSourceDocuments(["artifact-v2", "artifact-other"])).toEqual([]); expect(migrated.listArtifactSections("artifact-v2")).toEqual([]);
    expect(legacyHit(migrated)).toHaveLength(1); expect(legacyHit(migrated)[0]).toMatchObject({ id: "chunk-artifact-v2", spans: [{ blockId: "block-artifact-v2", blockStartOffset: 0, blockEndOffset: 20, chunkStartOffset: 0, chunkEndOffset: 20 }] });
  });

  it("实际建出来源表后 SQLite 章节 DDL 故障整批回滚，不删除旧库，修复后可重试", () => {
    const f = fixture(); const raw = new Database(f.dbPath); const before = oldRows(raw); const beforeSchema = raw.prepare("SELECT name,sql FROM sqlite_master ORDER BY name").all(); raw.close();
    let attempts = 0; let sourceTableBuilt = false;
    class FaultDatabase extends Database {
      constructor(file: string) { super(file); attempts++; }
      exec(sql: string): this {
        if (!sql.includes("CREATE TABLE knowledge_sections (")) return super.exec(sql);
        try { return super.exec(sql.replace("CREATE TABLE knowledge_sections (", "CREATE TABLE knowledge_sections (?")); }
        catch (error) { sourceTableBuilt = !!(this as unknown as Database.Database).prepare("SELECT name FROM sqlite_master WHERE name='knowledge_source_documents'").get(); throw error; }
      }
    }
    expect(() => new KnowledgeIndexStore({ dbPath: f.dbPath, Database: FaultDatabase })).toThrow(/migration or open failed/);
    expect(attempts).toBe(1); expect(sourceTableBuilt).toBe(true); expect(fs.existsSync(f.dbPath)).toBe(true);
    const rolled = new Database(f.dbPath); expect(rolled.pragma("user_version", { simple: true })).toBe(3); expect(oldRows(rolled)).toEqual(before);
    expect(rolled.prepare("SELECT name,sql FROM sqlite_master ORDER BY name").all()).toEqual(beforeSchema); rolled.close();
    const recovered = open(f.dbPath); expect(recovered.db.pragma("user_version", { simple: true })).toBe(4); expect(legacyHit(recovered)).toHaveLength(1);
  });

  it("v3 重建期间旧 v2 全文与付费向量仍可读，完成后双变体并存且重开不变", () => {
    const f = fixture(); const vectorPath = path.join(f.root, "knowledge-vector.db");
    const model: VectorIndexModelIdentity = { key: "paid/embedding/openai/3", provider: "paid", modelId: "embedding", protocol: "openai-embeddings", dimensions: 3 };
    const vector = new PortableVectorIndexAdapter({ dbPath: vectorPath });
    vector.buildOrReplaceArtifact({ parseArtifactId: "artifact-v2", chunkIndexVariantId: knowledgeChunkIndexVariantId("artifact-v2", "2222222222222222"), chunkFingerprint: "paid-original", model,
      entries: [{ chunkId: "chunk-artifact-v2", parseArtifactId: "artifact-v2", ordinal: 0, vector: [1, 0, 0] }] }); vector.close();
    const vectorHash = crypto.createHash("sha256").update(fs.readFileSync(vectorPath)).digest("hex");
    const store = open(f.dbPath); const old = legacyHit(store)[0];
    store.ensureChunkIndexVariant({ parseArtifactId: "artifact-v2", chunkProfileHash: "3333333333333333", blockFingerprint: "v3" });
    expect(legacyHit(store).map(hit => hit.id)).toEqual([old.id]);
    expect(store.search({ scopes: [{ parseArtifactId: "artifact-v2", chunkProfileHash: "3333333333333333" }], query: "AuroraQuokka" })).toEqual([]);
    store.replaceArtifactChunks({ parseArtifactId: "artifact-v2", chunkProfileHash: "3333333333333333", blockFingerprint: "v3",
      chunks: [{ ...old, id: "chunk-v3", sectionId: "section-v3" }],
      sourceDocument: { parseArtifactId: "artifact-v2", title: "旧资料新索引", outlineText: "旧版章节", searchText: old.text },
      sections: [{ id: "section-v3", parseArtifactId: "artifact-v2", sectionOrdinal: 0, headingPath: ["旧版章节"], startBlockOrdinal: 0, endBlockOrdinal: 0, text: old.text, tokenCount: old.tokenCount }] });
    expect(legacyHit(store).map(hit => hit.id)).toEqual([old.id]); expect(store.listChunkIndexVariantsByArtifact("artifact-v2")).toHaveLength(2);
    store.close(); const reopened = open(f.dbPath); expect(legacyHit(reopened).map(hit => hit.id)).toEqual([old.id]);
    expect(reopened.search({ scopes: [{ parseArtifactId: "artifact-v2", chunkProfileHash: "3333333333333333" }], query: "AuroraQuokka" })[0].sectionId).toBe("section-v3");
    expect(crypto.createHash("sha256").update(fs.readFileSync(vectorPath)).digest("hex")).toBe(vectorHash);
    const vectors = new PortableVectorIndexAdapter({ dbPath: vectorPath });
    try { expect(vectors.search({ parseArtifactIds: ["artifact-v2"], model, queryVector: [1, 0, 0] })).toEqual([expect.objectContaining({ chunkId: old.id, score: 1 })]); }
    finally { vectors.close(); }
  });

  it("真实损坏缓存仍可精确重建，邻接事实文件不受影响", () => {
    const f = fixture(); fs.writeFileSync(f.dbPath, "not a sqlite database"); const facts = path.join(f.root, "knowledge.db"); fs.writeFileSync(facts, "事实保留");
    const recovered = open(f.dbPath); expect(recovered.health()).toEqual({ status: "ready" }); expect(recovered.db.pragma("user_version", { simple: true })).toBe(4);
    expect(fs.readFileSync(facts, "utf8")).toBe("事实保留");
  });
});
