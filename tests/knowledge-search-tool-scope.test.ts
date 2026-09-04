import { expect, it, vi } from "vitest";
import { searchToolFixture } from "./helpers/knowledge-search-tool-fixture.ts";

it("未知、关闭、跨工作室、跨会话范围全部拒绝；普通子会话仅继承真实父范围", async () => {
  const f = await searchToolFixture();
  try {
    const search = vi.spyOn(f.manager.searchService, "search");
    for (const [tool, params] of [
      [f.makeTool(), { ...f.params, scopeId: "missing" }],
      [f.makeTool("other-studio"), f.params],
      [f.makeTool(f.studioId, { sessionPath: "/tmp/other.jsonl", parentSessionPath: null }), f.params],
    ] as const) expect((await tool.execute("invalid", params)).details).toMatchObject({ errorCode: "KNOWLEDGE_SCOPE_VIOLATION" });
    expect(search).not.toHaveBeenCalled();
    const child = f.makeTool(f.studioId, { sessionPath: "/tmp/child.jsonl", parentSessionPath: f.session.sessionPath });
    expect((await child.execute("child", f.params)).isError).toBeUndefined();
    f.manager.closeTurnScope({ scopeId: f.scope.id });
    expect((await child.execute("closed", f.params)).details).toMatchObject({ errorCode: "KNOWLEDGE_SCOPE_VIOLATION" });
  } finally { vi.restoreAllMocks(); await f.close(); }
});

it("范围过滤必须全为子集，混入越权源不得部分放行，模型不能提供 studioId", async () => {
  const f = await searchToolFixture();
  try {
    const tool = f.makeTool();
    for (const filter of [{ sourceIds: [f.source.id, "outside"] }, { notebookIds: [f.notebook.id, "outside"] },
      { sectionKeys: ["outside-heading"] }, { sourceIds: "not-array" }, { notebookIds: [1] }]) {
      const result = await tool.execute("filter", { ...f.params, ...filter });
      expect(result.isError).toBe(true); expect(result.details).toMatchObject({ errorCode: "KNOWLEDGE_SCOPE_VIOLATION" });
    }
    expect((await tool.execute("spoof", { ...f.params, studioId: "other" })).details).toMatchObject({ errorCode: "KNOWLEDGE_INVALID_ARGUMENT" });
    const narrowed = await tool.execute("narrow", { ...f.params, sourceIds: [f.source.id], notebookIds: [f.notebook.id] });
    expect(narrowed.isError).toBeUndefined(); expect(JSON.parse(narrowed.content[0].text).hits).toHaveLength(1);
    const empty = await tool.execute("empty", { ...f.params, sourceIds: [] });
    expect(JSON.parse(empty.content[0].text).hits).toEqual([]);
  } finally { await f.close(); }
});
