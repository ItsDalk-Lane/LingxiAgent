import { afterEach, describe, expect, it, vi } from "vitest";
import { createKnowledgeReadTool } from "../lib/tools/knowledge-read-tool.ts";
import { EvidenceReceiptService } from "../lib/knowledge/evidence-receipt-service.ts";
import { ResearchStore } from "../lib/knowledge/research/research-store.ts";
import { createHierarchicalFixture, hierarchicalStudio } from "./helpers/knowledge-hierarchical-fixture.ts";
const fixtures: Array<Awaited<ReturnType<typeof createHierarchicalFixture>>> = [];
afterEach(async () => { vi.restoreAllMocks(); for (const f of fixtures.splice(0)) await f.close(); });
async function fixture(large = false) {
  const f = await createHierarchicalFixture([
    { name: "parent.txt", sections: [{ heading: "父章节", text: large ? "甲乙丙丁戊己庚辛壬癸😀".repeat(1800) : "needle 发布条款。".repeat(700) }] },
    { name: "outside.txt", sections: [{ heading: "外部章节", text: "另一个来源" }] },
  ]); fixtures.push(f);
  const research = new ResearchStore(f.manager.store);
  const run = research.createRun({ turnScopeId: f.scope.id, turnId: f.scope.turnId, parentSessionPath: f.sessionPath, question: "条款" });
  const deps = { getKnowledge: () => f.manager, getStudioId: () => hierarchicalStudio,
    resolveSessionContext: () => ({ sessionPath: f.sessionPath, scopeOwnerSessionPath: f.sessionPath }),
    resolveResearchContext: () => ({ runId: run.id, actorSessionId: "parent-reader" }) };
  return { ...f, research, run, tool: createKnowledgeReadTool(deps), receipts: new EvidenceReceiptService(research),
    params: { scopeId: f.scope.id, sourceId: f.sources[0].imported.source.id } };
}
function payload(result: Awaited<ReturnType<ReturnType<typeof createKnowledgeReadTool>["execute"]>>) {
  expect(result.isError, result.content[0].text).not.toBe(true); return JSON.parse(result.content[0].text);
}

describe("命中片段的父章节与相邻原文读取", () => {
  it("巨原块被分节后只读取该节精确区间，每张凭据均能重读相同原文", async () => {
    const f = await fixture(true), source = f.sources[0];
    expect(source.sections.length).toBeGreaterThan(1);
    const section = source.sections[1];
    const result = payload(await f.tool.execute("parent", { ...f.params, sectionId: section.id }));
    expect(result).toMatchObject({ mode: "section", sectionId: section.id, parentSectionHeading: ["父章节"] });
    expect(result.chunks[0].text).toBe(section.text);
    expect(result.chunks[0].text.length).toBeLessThan(source.blocks[0].text.length);
    expect(result.chunks[0].spans[0].startOffset).toBeGreaterThan(0);
    for (const span of result.chunks[0].spans) {
      const block = source.blocks.find(block => block.id === span.blockId)!;
      expect(span.text).toBe(block.text.slice(span.startOffset, span.endOffset));
      expect(span.text.isWellFormed()).toBe(true);
      const read = f.receipts.read({ runId: f.run.id, receiptId: span.receiptId });
      expect(read.text).toBe(span.text); expect(read.receipt.chunkId).toBeNull();
    }
  });

  it.each([0, 1, 2, 3])("相邻窗口%d精确读目标附近片段、不扫描所有正文，旧序号读取仍兼容", async window => {
    const f = await fixture(), source = f.sources[0];
    const target = source.chunks[2]; expect(target).toBeDefined();
    const full = vi.spyOn(f.manager.indexStore, "listVariantChunks");
    const result = payload(await f.tool.execute("neighbors", { ...f.params, aroundChunkId: target.id, neighborWindow: window }));
    const expected = source.chunks.slice(Math.max(0, 2 - window), 3 + window);
    expect(result.mode).toBe("around-chunk");
    expect(result.chunks.map((chunk: { ordinal: number }) => chunk.ordinal)).toEqual(expected.map(chunk => chunk.ordinal + 1));
    expect(full).not.toHaveBeenCalled();
    const legacy = payload(await f.tool.execute("old", { ...f.params, fromOrdinal: 3, toOrdinal: 3 }));
    expect(legacy.chunks[0].text).toBe(target.text);
    for (const chunk of result.chunks) for (const span of chunk.spans) {
      expect(f.receipts.read({ runId: f.run.id, receiptId: span.receiptId }).text).toBe(span.text);
    }
  });

  it("跨来源章节和命中ID、非法窗口、互斥选择器均在写凭据前拒绝", async () => {
    const f = await fixture();
    const invalid = [
      { sectionId: f.sources[1].sections[0].id }, { aroundChunkId: f.sources[1].chunks[0].id },
      ...[-1, 4, 1.5].map(neighborWindow => ({ aroundChunkId: f.sources[0].chunks[0].id, neighborWindow })),
      { neighborWindow: 1 }, { sectionId: f.sources[0].sections[0].id, query: "needle" },
      { sectionId: f.sources[0].sections[0].id, aroundChunkId: f.sources[0].chunks[0].id },
    ];
    for (const selection of invalid) expect((await f.tool.execute("bad", { ...f.params, ...selection })).isError).toBe(true);
    expect(f.manager.store.db.prepare("SELECT COUNT(*) AS count FROM knowledge_research_read_receipts").get().count).toBe(0);
  });

  it("被篡改的章节缓存不能作为原文或领取凭据", async () => {
    const f = await fixture(), section = f.sources[0].sections[0];
    f.manager.indexStore.db.prepare("UPDATE knowledge_sections SET text=? WHERE id=?").run("伪造内容", section.id);
    const result = await f.tool.execute("tampered", { ...f.params, sectionId: section.id });
    expect(result.isError).toBe(true); expect(result.details).toMatchObject({ errorCode: "KNOWLEDGE_INDEX_INVALID" });
    expect(f.manager.store.db.prepare("SELECT COUNT(*) AS count FROM knowledge_research_read_receipts").get().count).toBe(0);
  });
});
