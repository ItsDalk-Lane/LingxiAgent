import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { KnowledgeIndexStore, type KnowledgeSectionDraft } from "../lib/knowledge/knowledge-index-store.ts";
import { buildKnowledgeChunks, buildKnowledgeSections, type KnowledgeChunkDraft } from "../lib/knowledge/chunker.ts";
import type { KnowledgeBlock } from "../lib/knowledge/types.ts";

const roots: string[] = [];
const stores: KnowledgeIndexStore[] = [];
afterEach(() => { for (const store of stores.splice(0)) store.close(); for (const dir of roots.splice(0)) fs.rmSync(dir, { recursive: true, force: true }); });
const PROFILE = "3333333333333333";
function fixture() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-section-index-")); roots.push(dir);
  let tick = 0;
  const store = new KnowledgeIndexStore({ dbPath: path.join(dir, "knowledge-fts.db"), now: () => `2026-09-04T00:00:0${tick++}.000Z` });
  stores.push(store); return store;
}
function data(artifact = "artifact") {
  const texts = ["AuroraQuokka 第一节记载交付时间。", "SilverHeron 第二节记载核验方法。"];
  const sections: KnowledgeSectionDraft[] = texts.map((text, index) => ({ id: `${artifact}-section-${index}`, parseArtifactId: artifact,
    sectionOrdinal: index, headingPath: ["目录", `第${index + 1}节`], startBlockOrdinal: index, endBlockOrdinal: index, text, tokenCount: 20 }));
  const chunks: KnowledgeChunkDraft[] = texts.map((text, index) => ({ id: `${artifact}-chunk-${index}`, parseArtifactId: artifact,
    ordinal: index, sectionId: sections[index].id, text, tokenCount: 20,
    spans: [{ blockId: `${artifact}-block-${index}`, blockStartOffset: 0, blockEndOffset: text.length, chunkStartOffset: 0, chunkEndOffset: text.length }] }));
  return { parseArtifactId: artifact, chunkProfileHash: PROFILE, blockFingerprint: `fp-${artifact}`, chunks, sections,
    sourceDocument: { parseArtifactId: artifact, title: `项目${artifact}`, outlineText: "目录\n第一节\n第二节", searchText: "SourceDolphin 目录 摘录 文件类型 markdown" } };
}

describe("来源、章节、短片段索引", () => {
  it("四张锁定新表与短片段章节两索引精确建成", () => {
    const store = fixture();
    expect(store.db.pragma("user_version", { simple: true })).toBe(4);
    const names = (table: string) => store.db.prepare(`PRAGMA table_info(${table})`).all().map((row: { name: string }) => row.name);
    expect(names("knowledge_source_documents")).toEqual(["parse_artifact_id", "title", "outline_text", "search_text", "created_at", "updated_at"]);
    expect(names("knowledge_sections")).toEqual(["id", "parse_artifact_id", "section_ordinal", "heading_path_json", "start_block_ordinal", "end_block_ordinal", "text", "token_count", "search_text", "created_at", "updated_at"]);
    expect(names("knowledge_chunks").at(-1)).toBe("section_id");
    expect(store.db.prepare("PRAGMA index_info(idx_knowledge_chunks_variant_section)").all().map((row: { name: string }) => row.name)).toEqual(["chunk_index_variant_id", "section_id", "ordinal"]);
    expect(store.db.prepare("PRAGMA index_info(idx_knowledge_chunks_artifact_section)").all().map((row: { name: string }) => row.name)).toEqual(["parse_artifact_id", "section_id"]);
    for (const name of ["knowledge_source_documents_fts", "knowledge_sections_fts"]) expect(store.db.prepare("SELECT sql FROM sqlite_master WHERE name = ?").get(name).sql).toContain("USING fts5");
  });

  it("三种粒度原子写入、范围隔离，短片段精确位置可读", () => {
    const store = fixture(); const input = data(); store.replaceArtifactChunks(input); store.replaceArtifactChunks(data("outside"));
    expect(store.getSourceDocument("artifact")).toMatchObject({ parseArtifactId: "artifact", title: input.sourceDocument.title, outlineText: input.sourceDocument.outlineText });
    expect(store.listSourceDocuments(["artifact", "artifact"]).map(row => row.parseArtifactId)).toEqual(["artifact"]);
    expect(store.listArtifactSections("artifact").map(row => ({ id: row.id, sectionOrdinal: row.sectionOrdinal, headingPath: row.headingPath, startBlockOrdinal: row.startBlockOrdinal, endBlockOrdinal: row.endBlockOrdinal, text: row.text, tokenCount: row.tokenCount, parseArtifactId: row.parseArtifactId }))).toEqual(input.sections);
    expect(store.searchSourceDocuments({ parseArtifactIds: ["artifact"], query: "SourceDolphin" }).map(hit => hit.parseArtifactId)).toEqual(["artifact"]);
    expect(store.searchSections({ parseArtifactIds: ["artifact"], query: "AuroraQuokka" }).map(hit => hit.id)).toEqual(["artifact-section-0"]);
    const variant = store.resolveChunkIndexVariant("artifact", PROFILE)!;
    expect(store.listVariantChunks(variant.id)).toEqual(input.chunks.map(chunk => ({ ...chunk, chunkIndexVariantId: variant.id })));
    expect(store.readVariantChunks(variant.id, [0])[0].spans).toEqual(input.chunks[0].spans);
    expect(store.searchReadyVariantIds({ chunkIndexVariantIds: [variant.id], query: "SilverHeron", limit: 2 })[0]).toMatchObject({ sectionId: "artifact-section-1", spans: input.chunks[1].spans });
  });

  it("真实巨块拆成多节后可原子保存，共用原块边界而不丢精确引用", () => {
    const store = fixture();
    const text = "长篇中文资料用于验证原始块跨章节分片。".repeat(1000);
    const blocks: KnowledgeBlock[] = [{ id: "giant-block", parseArtifactId: "giant", ordinal: 0, text,
      textSha256: crypto.createHash("sha256").update(text).digest("hex"), locatorType: "text", locator: {} }];
    const sections = buildKnowledgeSections("giant", blocks); const chunks = buildKnowledgeChunks("giant", blocks);
    expect(sections.length).toBeGreaterThan(1); expect(sections.every(section => section.startBlockOrdinal === 0 && section.endBlockOrdinal === 0)).toBe(true);
    store.replaceArtifactChunks({ parseArtifactId: "giant", chunkProfileHash: PROFILE, blockFingerprint: "giant-fp", chunks, sections,
      sourceDocument: { parseArtifactId: "giant", title: "巨块", outlineText: "", searchText: "巨块资料" } });
    expect(store.listArtifactSections("giant").map(section => section.text).join("")).toBe(text);
    const stored = store.listArtifactChunks("giant"); expect(stored).toEqual(chunks);
    for (const chunk of stored) for (const span of chunk.spans) {
      expect(chunk.text.slice(span.chunkStartOffset, span.chunkEndOffset)).toBe(text.slice(span.blockStartOffset, span.blockEndOffset));
    }
  });

  it("幂等更新保持创建时间，全文索引移除旧词并读到新词", () => {
    const store = fixture(); const input = data(); store.replaceArtifactChunks(input);
    const beforeSource = store.getSourceDocument("artifact")!; const beforeSection = store.listArtifactSections("artifact")[0];
    store.upsertSourceDocument({ ...input.sourceDocument, searchText: "UpdatedFalcon" });
    store.upsertSections("artifact", input.sections.map(section => ({ ...section, text: "UpdatedBadger" })));
    expect(store.getSourceDocument("artifact")!.createdAt).toBe(beforeSource.createdAt);
    expect(store.getSourceDocument("artifact")!.updatedAt).not.toBe(beforeSource.updatedAt);
    expect(store.listArtifactSections("artifact")[0].createdAt).toBe(beforeSection.createdAt);
    expect(store.searchSourceDocuments({ parseArtifactIds: ["artifact"], query: "SourceDolphin" })).toEqual([]);
    expect(store.searchSourceDocuments({ parseArtifactIds: ["artifact"], query: "UpdatedFalcon" })).toHaveLength(1);
    expect(store.searchSections({ parseArtifactIds: ["artifact"], query: "AuroraQuokka" })).toEqual([]);
    expect(store.searchSections({ parseArtifactIds: ["artifact"], query: "UpdatedBadger" }).map(hit => hit.sectionOrdinal)).toEqual([0, 1]);
  });

  it("末尾短片段写失败时来源、章节、旧短片段和变体状态全部回滚", () => {
    const store = fixture(); const input = data(); store.replaceArtifactChunks(input);
    const before = [store.getSourceDocument("artifact"), store.listArtifactSections("artifact"), store.listArtifactChunks("artifact"), store.resolveChunkIndexVariant("artifact", PROFILE)];
    const changed = { ...input, blockFingerprint: "new-fingerprint", sourceDocument: { ...input.sourceDocument, title: "新标题" }, sections: input.sections.map(section => ({ ...section, text: "新章节" })), chunks: input.chunks.map((chunk, i) => ({ ...chunk, sectionId: i === 1 ? "other-artifact-section" : chunk.sectionId })) };
    expect(() => store.replaceArtifactChunks(changed)).toThrow(/section is outside/);
    expect([store.getSourceDocument("artifact"), store.listArtifactSections("artifact"), store.listArtifactChunks("artifact"), store.resolveChunkIndexVariant("artifact", PROFILE)]).toEqual(before);
    expect(store.searchSections({ parseArtifactIds: ["artifact"], query: "AuroraQuokka" })).toHaveLength(1);
  });

  it("章节身份不能串到另一个来源，重复章节序号保持唯一", () => {
    const store = fixture(); const input = data(); store.replaceArtifactChunks(input);
    expect(() => store.upsertSections("outside", [{ ...input.sections[0], parseArtifactId: "outside" }])).toThrow(/cannot be reassigned/);
    expect(() => store.upsertSections("artifact", [{ ...input.sections[0], id: "changed-id" }])).toThrow(/UNIQUE constraint/);
    expect(store.listArtifactSections("artifact")).toHaveLength(2); expect(store.listArtifactSections("outside")).toEqual([]);
  });

  it.each(["removeArtifact", "removeChunkIndexVariantsByArtifact"] as const)("%s 删除三种派生索引并保留范围外来源", method => {
    const store = fixture(); store.replaceArtifactChunks(data()); store.replaceArtifactChunks(data("outside")); store[method]("artifact");
    expect(store.getSourceDocument("artifact")).toBeNull(); expect(store.listArtifactSections("artifact")).toEqual([]); expect(store.listArtifactChunks("artifact")).toEqual([]);
    expect(store.searchSourceDocuments({ parseArtifactIds: ["artifact", "outside"], query: "SourceDolphin" }).map(hit => hit.parseArtifactId)).toEqual(["outside"]);
    expect(store.searchSections({ parseArtifactIds: ["artifact", "outside"], query: "AuroraQuokka" }).map(hit => hit.parseArtifactId)).toEqual(["outside"]);
  });

  it("两个检索入口严格限制输入，空范围没有全库回退，特殊语法仅按文字处理", () => {
    const store = fixture(); store.replaceArtifactChunks(data());
    for (const search of [store.searchSourceDocuments.bind(store), store.searchSections.bind(store)]) {
      expect(search({ parseArtifactIds: [], query: "AuroraQuokka" })).toEqual([]);
      expect(() => search({ parseArtifactIds: ["artifact"], query: "", limit: 12 })).toThrow(/query is invalid/);
      expect(() => search({ parseArtifactIds: ["artifact"], query: "AuroraQuokka", limit: 1001 })).toThrow(/limit is invalid/);
      expect(() => search({ parseArtifactIds: ["artifact"], query: '" OR * NOT NEAR( ?' })).not.toThrow();
    }
  });
});
