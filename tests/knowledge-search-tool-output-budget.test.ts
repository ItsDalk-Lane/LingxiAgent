import { expect, it, vi } from "vitest";
import { searchToolFixture } from "./helpers/knowledge-search-tool-fixture.ts";

it("拒绝超长问题及非法 limit，不静默夹取，也不开始检索", async () => {
  const f = await searchToolFixture();
  try {
    const search = vi.spyOn(f.manager.searchService, "searchWithEvidence"), tool = f.makeTool();
    for (const patch of [{ query: "" }, { query: "  " }, { query: "x".repeat(4001) }, { query: 1 },
      { limit: 0 }, { limit: 25 }, { limit: 1.5 }, { limit: null }, { limit: NaN }, { channel: "remote" }]) {
      expect((await tool.execute("invalid", { ...f.params, ...patch })).details).toMatchObject({ errorCode: "KNOWLEDGE_INVALID_ARGUMENT" });
    }
    expect(search).not.toHaveBeenCalled();
    expect((await tool.execute("boundary", { ...f.params, query: "n".repeat(4000), limit: 1 })).isError).toBeUndefined();
  } finally { vi.restoreAllMocks(); await f.close(); }
});

it("大量长块按条数与消息体积双重分页，每条原文最多800字符", async () => {
  const f = await searchToolFixture(true);
  try {
    const tool = f.makeTool();
    for (const limit of [undefined, 24]) {
      const result = await tool.execute("budget", { ...f.params, channel: "fts", ...(limit ? { limit } : {}) });
      expect(result.isError).toBeUndefined(); const payload = JSON.parse(result.content[0].text);
      expect(payload.totalHits).toBe(limit ?? 12);
      expect(payload.hits.length).toBeGreaterThan(0);
      expect(payload.hits.length).toBeLessThanOrEqual(limit ?? 12);
      expect(payload.truncated).toBe(payload.hits.length < payload.totalHits);
      expect(payload.hits.every(hit => hit.spans.reduce((sum, span) => sum + [...span.text].length, 0) <= 800)).toBe(true);
      expect(payload.hits.some(hit => hit.originalTextTruncated)).toBe(true);
      expect(Buffer.byteLength(JSON.stringify(payload.hits))).toBeLessThanOrEqual(24000);
      expect(payload.citationNotice).toContain("来自冻结原文");
    }
  } finally { await f.close(); }
});
