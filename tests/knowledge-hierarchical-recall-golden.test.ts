import { afterEach, describe, expect, it, vi } from "vitest";
import { EvidenceSpanExtractor } from "../lib/knowledge/evidence-span-extractor.ts";
import { createHierarchicalFixture } from "./helpers/knowledge-hierarchical-fixture.ts";
const fixtures: Array<Awaited<ReturnType<typeof createHierarchicalFixture>>> = [];
afterEach(async () => { vi.restoreAllMocks(); for (const f of fixtures.splice(0)) await f.close(); });

describe("分层召回固定资料集", () => {
  it("只有来源标题或章节标题命中时仍返回可读线索，但原文证据候选保持为空", async () => {
    const f = await createHierarchicalFixture([
      { name: "needle-title.txt", sections: [{ heading: "资料", text: "来源正文没有那个英文词" }] },
      { name: "chapter.txt", sections: [{ heading: "needle", text: "章节正文也没有查询词" }] },
    ]); fixtures.push(f);
    const result = await f.manager.searchService.searchWithEvidence(f.request);
    expect(new Set(result.response.hits.map(hit => hit.grain))).toEqual(new Set(["source", "section"]));
    expect(result.evidence.candidates).toEqual([]);
    for (const hit of result.response.hits) {
      expect(f.manager.indexStore.getChunkLocation(hit.chunkId)).toMatchObject({ parseArtifactId: hit.parseArtifactId,
        chunkIndexVariantId: hit.chunkIndexVariantId, ordinal: hit.chunkOrdinal, sectionId: hit.sectionId });
      expect(hit).not.toHaveProperty("receiptId");
    }
  });

  it("跨来源同词、同来源反例及第二章线索均能定位，显式章节不混入其他条款", async () => {
    const f = await createHierarchicalFixture([
      { name: "制度.txt", sections: [
        { heading: "第一章", text: "needle 常规交付日期九月十五日。后续例外见第二章。" },
        { heading: "第二章", text: "needle 反例：特殊项目交付推迟到十月一日。" },
      ] },
      { name: "复核.txt", sections: [{ heading: "独立复核", text: "needle 复核确认特殊项目延期。" }] },
    ], true); fixtures.push(f);
    const searched = await f.manager.searchService.searchWithEvidence(f.request);
    expect(new Set(searched.response.hits.map(hit => hit.sourceId))).toEqual(new Set(f.sources.map(source => source.imported.source.id)));
    expect(new Set(searched.response.hits.map(hit => hit.sectionId))).toEqual(new Set(f.sources.flatMap(source => source.sections.map(section => section.id))));
    expect(searched.evidence.candidates.every(candidate => searched.response.hits.some(hit => hit.grain === "span" && hit.chunkId === candidate.id))).toBe(true);
    const next = await f.manager.searchService.search({ ...f.request, query: "反例", sourceIds: [f.sources[0].imported.source.id], sectionKeys: ["第二章"] });
    expect(next.hits).toHaveLength(1); expect(next.hits[0].snippet).toContain("十月一日");
    expect(next.hits[0].parentSectionHeading).toEqual(["第二章"]);
    expect(next.hits[0]).not.toHaveProperty("receiptId"); expect(next.hits[0]).not.toHaveProperty("evidenceId");
  });

  it.each(["source", "section"])("%s搜索线索不能直接转成证据；片段仍必须回读冻结原文", async grain => {
    const f = await createHierarchicalFixture([{ name: "source.txt", sections: [{ heading: "章节", text: "needle 冻结原文。" }] }]); fixtures.push(f);
    const searched = await f.manager.searchService.searchWithEvidence({ ...f.request, channel: "fts" });
    const extractor = new EvidenceSpanExtractor(f.manager.store);
    const input = { compiledScope: f.compiledScope, query: "needle", hits: searched.evidence.candidates.map(hit => ({ ...hit, grain, text: "伪造摘要" })) };
    expect(() => extractor.extract(input)).toThrow(/must be read/u);
    const spans = extractor.extract({ ...input, hits: searched.evidence.candidates.map(hit => ({ ...hit, grain: "span", text: "伪造摘要" })) });
    expect(spans).toHaveLength(1); expect(spans[0].text).toBe("needle 冻结原文。");
  });
});
