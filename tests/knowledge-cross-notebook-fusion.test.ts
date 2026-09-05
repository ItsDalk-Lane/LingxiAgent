import { afterEach, describe, expect, it, vi } from "vitest";
import { createRerankFixture } from "./helpers/knowledge-rerank-fixture.ts";
const cleanups: Array<() => Promise<void>> = [];
afterEach(async () => { vi.restoreAllMocks(); for (const close of cleanups.splice(0)) await close(); });

describe("全文检索跨笔记本一次融合", () => {
  it("共享来源不会因多挂几个笔记本重复增加 FTS 分数", async () => {
    const data = await createRerankFixture([null], null); cleanups.push(data.close);
    const baseline = await data.manager.searchService.search(data.request);
    const ids = [data.notebooks[0].id];
    for (let i = 0; i < 4; i += 1) {
      const notebook = data.manager.createNotebook({ studioId: "rerank", name: `共享 ${i}` }); ids.push(notebook.id);
      data.manager.updateNotebookSettings({ studioId: "rerank", notebookId: notebook.id, chunkTargetChars: 200 });
      data.manager.addSourceToNotebook({ studioId: "rerank", notebookId: notebook.id, sourceId: data.sourceIds[0] });
      data.manager.enqueueSourceIngestion({ studioId: "rerank", notebookId: notebook.id, sourceId: data.sourceIds[0] });
    }
    await data.manager.ingestion.drainQueue();
    const scope = data.manager.createTurnScope({ studioId: "rerank", sessionPath: "/tmp/shared-fusion.jsonl", notebookIds: ids });
    const compiledScope = await data.manager.compileTurnScope(scope);
    const fts = vi.spyOn(data.manager.indexStore, "search");
    const result = await data.manager.searchService.search({ ...data.request, compiledScope });
    expect(fts).toHaveBeenCalledTimes(1);
    expect(result.hits).toHaveLength(1);
    expect(result.hits[0].notebookIds).toHaveLength(5);
    expect(result.hits[0].score).toBe(baseline.hits[0].score);
    expect(result.hits[0].score).toBeCloseTo(1 / 61);
    const cached = await data.manager.searchService.search({ ...data.request, compiledScope });
    expect(cached).toMatchObject({ retrievalResultCacheHit: true, remoteModelCalls: 0, embeddingGroups: 0, rerankGroups: 0 });
  });
});
