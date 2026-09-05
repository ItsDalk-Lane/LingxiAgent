import { afterEach, describe, expect, it, vi } from "vitest";
import { createHierarchicalFixture } from "./helpers/knowledge-hierarchical-fixture.ts";
const fixtures: Array<Awaited<ReturnType<typeof createHierarchicalFixture>>> = [];
async function fixture(...args: Parameters<typeof createHierarchicalFixture>) { const f = await createHierarchicalFixture(...args); fixtures.push(f); return f; }
afterEach(async () => { vi.restoreAllMocks(); for (const f of fixtures.splice(0)) await f.close(); });

describe("来源、章节、片段分层检索", () => {
  it("快速仍只做片段FTS，不增加来源/章节查询或元数据扫描", async () => {
    const f = await fixture([{ name: "fast.txt", sections: [{ heading: "定位", text: "needle 原文。" }] }]);
    const spies = [vi.spyOn(f.manager.indexStore, "searchSourceDocuments"), vi.spyOn(f.manager.indexStore, "searchSections"),
      vi.spyOn(f.manager.indexStore, "listArtifactSectionMetadata"), vi.spyOn(f.manager.queryService, "retrieveCompiledGroup")];
    const result = await f.manager.searchService.search({ ...f.request, channel: "fts" });
    expect(result.hits).toHaveLength(1);
    expect(result.hits[0]).toMatchObject({ grain: "span", sectionId: f.sources[0].sections[0].id, parentSectionHeading: ["定位"] });
    expect(result.remoteModelCalls).toBe(0); for (const spy of spies) expect(spy).not.toHaveBeenCalled();
  });

  it("章节导航上限12，正文补漏保留其余命中与另一来源标题线索", async () => {
    const f = await fixture([
      { name: "many.txt", sections: Array.from({ length: 16 }, (_, index) => ({ heading: `章节${index}`, text: `needle 事实 ${index}` })) },
      { name: "needle-title.txt", sections: [{ heading: "只有标题命中", text: "正文不含查询词" }] },
    ]);
    const sectionSearch = vi.spyOn(f.manager.indexStore, "searchSections");
    const spanSearch = vi.spyOn(f.manager.indexStore, "search");
    const result = await f.manager.searchService.search(f.request);
    expect(sectionSearch).toHaveBeenCalledWith(expect.objectContaining({ limit: 12 }));
    const filters = spanSearch.mock.calls[0][0].sectionIdsByChunkIndexVariantId!;
    expect(filters.get(f.sources[0].variantId)).toHaveLength(12);
    expect(filters.get(f.sources[1].variantId)).toEqual([f.sources[1].sections[0].id]);
    expect(result.hits.filter(hit => hit.grain === "span")).toHaveLength(16);
    expect(result.hits.filter(hit => hit.grain === "source")).toHaveLength(1);
    expect(result.hits.filter(hit => hit.grain === "span").map(hit => hit.sectionId).sort()).toEqual(f.sources[0].sections.map(section => section.id).sort());
    expect(result.hits.filter(hit => hit.grain === "source").every(hit => filters.get(hit.chunkIndexVariantId)?.includes(hit.sectionId!))).toBe(true);
  });

  it("显式章节过滤在SQL的LIMIT之前执行，其他高频章节不能挤掉目标", async () => {
    const f = await fixture([{ name: "filter.txt", sections: [
      { heading: "第一章", text: "needle ".repeat(1500) }, { heading: "第二章", text: "needle 目标条款" },
    ] }]);
    const sql = vi.spyOn(f.manager.indexStore.db, "prepare");
    const fullChunks = vi.spyOn(f.manager.indexStore, "listVariantChunks");
    const result = await f.manager.searchService.search({ ...f.request, channel: "fts", sectionKeys: ["第二章"], limit: 1 });
    expect(result.hits).toHaveLength(1); expect(result.hits[0].snippet).toContain("目标条款");
    expect(result.hits[0].sectionId).toBe(f.sources[0].sections[1].id);
    expect(sql.mock.calls.some(([statement]) => typeof statement === "string"
      && /knowledge_chunks_fts MATCH[\s\S]+c\.section_id IN[\s\S]+LIMIT/u.test(statement))).toBe(true);
    expect(fullChunks).not.toHaveBeenCalled();
    expect((await f.manager.searchService.search({ ...f.request, sectionKeys: [] })).hits).toEqual([]);
  });

  it("来源过滤在分层查询前收紧范围；未命中的语义查询仍保留来源级召回", async () => {
    const f = await fixture([
      { name: "a.txt", sections: [{ heading: "甲", text: "相关语义事实" }] },
      { name: "b.txt", sections: [{ heading: "乙", text: "unrelated 独立事实" }] },
    ], true);
    const coarse = vi.spyOn(f.manager.indexStore, "searchSections");
    const result = await f.manager.searchService.search({ ...f.request, query: "semantic-query", sourceIds: [f.sources[0].imported.source.id] });
    expect(coarse.mock.calls[0][0].parseArtifactIds).toEqual([f.sources[0].artifact.id]);
    expect(result.hits.length).toBeGreaterThan(0);
    expect(result.hits.every(hit => hit.sourceId === f.sources[0].imported.source.id && hit.channels.includes("vector"))).toBe(true);
  });
});
