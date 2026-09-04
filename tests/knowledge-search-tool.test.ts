import { expect, it, vi } from "vitest";
import fs from "node:fs";
import { createKnowledgeSearchTool } from "../lib/tools/knowledge-search-tool.ts";
import { classifySessionPermission } from "../core/session-permission-mode.ts";
import { STANDARD_TOOL_NAMES } from "../shared/tool-categories.ts";
import { searchToolFixture } from "./helpers/knowledge-search-tool-fixture.ts";

it("默认 hybrid/12，复用统一服务，返回候选且明确禁止直接引用", async () => {
  const f = await searchToolFixture();
  try {
    const search = vi.spyOn(f.manager.searchService, "search"), tool = f.makeTool();
    const result = await tool.execute("search", f.params);
    expect(result.isError).toBeUndefined(); const payload = JSON.parse(result.content[0].text);
    expect(search.mock.calls[0][0]).toMatchObject({ channel: "hybrid", limit: 12, rerank: true, compiledScope: { studioId: f.studioId } });
    expect(payload).toMatchObject({ scopeId: f.scope.id, query: "needle", mode: "fts", vectorBackend: "none", degradedReasons: [] });
    expect(payload.hits[0]).toMatchObject({ sourceId: f.source.id, notebookIds: [f.notebook.id], channels: ["fts"] });
    expect(payload.hits[0].candidateId).toMatch(/^kc_/); expect(payload.hits[0]).not.toHaveProperty("evidenceId");
    expect(payload.citationNotice).toContain("必须调用 knowledge_read 或 knowledge_grep 后才能引用");
    expect(payload.citationNotice).toContain("candidateId 不是证据 ID");
    expect(tool.parameters.properties).not.toHaveProperty("studioId");
    for (const mode of ["auto", "ask", "operate", "read_only"] as const) {
      expect(classifySessionPermission({ mode, toolName: tool.name, context: { toolInvocation: tool.sessionPermission.resolveInvocation() } }).action).toBe("allow");
    }
    expect(STANDARD_TOOL_NAMES).toContain("knowledge_search");
    const source = fs.readFileSync("core/agent.ts", "utf8");
    expect(source).toContain("this._knowledgeSearchTool = createKnowledgeSearchTool({");
    expect(source).toContain("      this._knowledgeSearchTool,");
  } finally { vi.restoreAllMocks(); await f.close(); }
});

it("fts 明确禁止远程重排并传递取消，不可用环境显式报错", async () => {
  const f = await searchToolFixture();
  try {
    const search = vi.spyOn(f.manager.searchService, "search"), controller = new AbortController();
    expect((await f.makeTool().execute("search", { ...f.params, channel: "fts" }, controller.signal)).isError).toBeUndefined();
    expect(search.mock.calls[0][0]).toMatchObject({ channel: "fts", rerank: false, signal: controller.signal });
    controller.abort(); await expect(f.makeTool().execute("cancel", f.params, controller.signal)).rejects.toMatchObject({ name: "AbortError" });
    const missing = createKnowledgeSearchTool({ getKnowledge: () => null, getStudioId: () => null });
    expect((await missing.execute("missing", f.params)).details).toMatchObject({ errorCode: "KNOWLEDGE_MODEL_UNAVAILABLE" });
  } finally { vi.restoreAllMocks(); await f.close(); }
});
