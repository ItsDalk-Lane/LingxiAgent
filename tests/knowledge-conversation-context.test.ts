import { afterEach, expect, it, vi } from "vitest";
import { LingxiEngine } from "../core/engine.ts";
import { searchToolFixture } from "./helpers/knowledge-search-tool-fixture.ts";

const fixtures: Array<Awaited<ReturnType<typeof searchToolFixture>>> = [];
afterEach(async () => { vi.restoreAllMocks(); for (const fixture of fixtures.splice(0)) await fixture.close(); });

async function setup() {
  const f = await searchToolFixture(); fixtures.push(f);
  const available = ["knowledge_search", "knowledge_read", "knowledge_grep", "knowledge_outline"];
  const getActiveToolNames = vi.fn(() => available);
  const engine = Object.assign(Object.create(LingxiEngine.prototype) as LingxiEngine, {
    _knowledge: f.manager, _runtimeContext: { studioId: f.studioId },
    getSessionManifest: vi.fn(() => ({ lifecycle: "active", currentLocator: { path: f.session.sessionPath } })),
    getSessionIdForPath: vi.fn((value: string) => value === f.session.sessionPath ? "chat" : null),
    getSessionByPath: vi.fn(() => ({ getActiveToolNames })),
  });
  const input = { sessionId: "chat", sessionPath: f.session.sessionPath!, turnId: "conversation-turn",
    knowledgeRefs: { notebookIds: [f.notebook.id] } };
  return { ...f, engine, input, getActiveToolNames };
}

it("准备只冻结范围，同轮重试复用；随后多次搜索都返回可引用原文", async () => {
  const f = await setup(), search = vi.spyOn(f.manager.searchService, "searchWithEvidence");
  const prepared = await f.engine.buildConversationKnowledgeContext(f.input);
  const repeated = await f.engine.buildConversationKnowledgeContext(f.input);
  expect(repeated.stats.scopeId).toBe(prepared.stats.scopeId);
  expect(search).not.toHaveBeenCalled();
  expect(prepared.stats).toMatchObject({ executionPath: "conversation", retrievalMode: "none", injectedChunks: 0 });
  expect(prepared.evidence).toEqual({ entries: [], searchedVectorVariants: [] });
  expect(prepared.block).not.toContain("每年十五天");
  for (const query of ["needle", "年假"]) {
    const result = await f.makeTool().execute(query, { scopeId: prepared.stats.scopeId, query, channel: "fts" });
    expect(result.isError).toBeUndefined();
    const found = JSON.parse(result.content[0].text);
    expect(found.hits.some(hit => hit.spans?.some(span => span.text.includes("每年十五天") && span.citationMarkdown))).toBe(true);
  }
  expect(search).toHaveBeenCalledTimes(2);
});

it("同轮不能替换笔记本，关闭的范围不能由重试复活", async () => {
  const f = await setup(), prepared = await f.engine.buildConversationKnowledgeContext(f.input);
  const other = f.manager.createNotebook({ studioId: f.studioId, name: "其他资料" });
  await expect(f.engine.buildConversationKnowledgeContext({ ...f.input, knowledgeRefs: { notebookIds: [other.id] } }))
    .rejects.toMatchObject({ code: "KNOWLEDGE_SCOPE_VIOLATION" });
  f.manager.closeTurnScope({ scopeId: prepared.stats.scopeId! });
  await expect(f.engine.buildConversationKnowledgeContext(f.input)).rejects.toMatchObject({ code: "KNOWLEDGE_SCOPE_VIOLATION" });
});

it("错误会话身份、缺失工具或已停止的提交均在创建范围前拒绝", async () => {
  const f = await setup(), create = vi.spyOn(f.manager, "createTurnScope");
  await expect(f.engine.buildConversationKnowledgeContext({ ...f.input, sessionId: "outside" }))
    .rejects.toMatchObject({ code: "KNOWLEDGE_SCOPE_VIOLATION" });
  f.getActiveToolNames.mockReturnValue(["knowledge_search"]);
  await expect(f.engine.buildConversationKnowledgeContext(f.input)).rejects.toMatchObject({ code: "KNOWLEDGE_MODEL_UNAVAILABLE" });
  const signal = AbortSignal.abort();
  await expect(f.engine.buildConversationKnowledgeContext({ ...f.input, signal })).rejects.toBe(signal.reason);
  expect(create).not.toHaveBeenCalled();
});
