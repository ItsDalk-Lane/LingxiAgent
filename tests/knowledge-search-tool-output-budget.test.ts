import { expect, it, vi } from "vitest";
import { searchToolFixture } from "./helpers/knowledge-search-tool-fixture.ts";

it("拒绝超长问题及非法 limit，不静默夹取，也不开始检索", async () => {
  const f = await searchToolFixture();
  try {
    const search = vi.spyOn(f.manager.searchService, "search"), tool = f.makeTool();
    for (const patch of [{ query: "" }, { query: "  " }, { query: "x".repeat(4001) }, { query: 1 },
      { limit: 0 }, { limit: 25 }, { limit: 1.5 }, { limit: null }, { limit: NaN }, { channel: "remote" }]) {
      expect((await tool.execute("invalid", { ...f.params, ...patch })).details).toMatchObject({ errorCode: "KNOWLEDGE_INVALID_ARGUMENT" });
    }
    expect(search).not.toHaveBeenCalled();
    expect((await tool.execute("boundary", { ...f.params, query: "n".repeat(4000), limit: 1 })).isError).toBeUndefined();
  } finally { vi.restoreAllMocks(); await f.close(); }
});

it("大量长块仍只返回默认 12 或最多 24 个候选，摘要不超过既有 1200 字符上限", async () => {
  const f = await searchToolFixture(true);
  try {
    const tool = f.makeTool();
    for (const limit of [undefined, 24]) {
      const result = await tool.execute("budget", { ...f.params, channel: "fts", ...(limit ? { limit } : {}) });
      expect(result.isError).toBeUndefined(); const payload = JSON.parse(result.content[0].text);
      expect(payload.hits).toHaveLength(limit ?? 12);
      expect(payload.hits.every((hit: { snippet: string }) => hit.snippet.length <= 1200)).toBe(true);
      expect(payload.hits.some((hit: { snippet: string }) => hit.snippet.length === 1200)).toBe(true);
      expect(result.content[0].text.length).toBeLessThan((limit ?? 12) * 2000 + 2000);
      expect(payload.citationNotice).toContain("必须调用 knowledge_read 或 knowledge_grep 后才能引用");
    }
  } finally { await f.close(); }
});
