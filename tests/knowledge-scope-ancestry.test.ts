import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { SessionManifestStore } from "../core/session-manifest/store.ts";
import { resolveKnowledgeScopeSessionContext } from "../core/session-manifest/knowledge-ancestry.ts";
import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import { ResearchStore } from "../lib/knowledge/research/research-store.ts";
import { createKnowledgeOutlineTool } from "../lib/tools/knowledge-outline-tool.ts";
import { resolveKnowledgeTurnScope, type KnowledgeToolSessionContext } from "../lib/tools/knowledge-scope.ts";

const cleanup: Array<() => void | Promise<void>> = [];
afterEach(async () => {
  for (const close of cleanup.splice(0).reverse()) await close();
});

function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-scope-ancestry-"));
  const manifests = new SessionManifestStore({ dbPath: path.join(root, "session-manifest.db") });
  cleanup.push(() => { manifests.close(); fs.rmSync(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 }); });
  const studioId = "ancestry-studio";
  function create(name: string, kind = "chat", parentSessionId?: string, provenance: Record<string, unknown> = {}) {
    const sessionPath = path.join(root, `${name}.jsonl`);
    fs.writeFileSync(sessionPath, "");
    return manifests.createForPath({ sessionPath, kind, domain: kind === "chat" ? "desktop" : "subagent",
      provenance: { ...(parentSessionId === undefined ? {} : { parentSessionId }), ...provenance },
    });
  }
  const getSessionIdForPath = (sessionPath: string) => manifests.resolveByLocatorPath(sessionPath)?.sessionId ?? null;
  const getSessionManifest = (sessionId: string) => manifests.getBySessionId(sessionId);
  function resolve(sessionPath: string | null) {
    return resolveKnowledgeScopeSessionContext({ sessionPath, studioId, getSessionIdForPath, getSessionManifest });
  }
  function changeProvenance(sessionId: string, value: unknown) {
    manifests.db.prepare("UPDATE session_manifests SET provenance_json = ? WHERE session_id = ?")
      .run(JSON.stringify(value), sessionId);
  }
  return { root, studioId, manifests, create, resolve, changeProvenance, getSessionIdForPath, getSessionManifest };
}

describe("Knowledge 范围拥有者的真实会话祖先链", () => {
  it("普通主会话以自身为拥有者；分叉主会话不把旧分叉来源当作范围拥有者", () => {
    const f = fixture();
    const main = f.create("main");
    expect(f.resolve(main.currentLocator.path)).toEqual({ sessionPath: main.currentLocator.path, scopeOwnerSessionPath: main.currentLocator.path });
    const fork = f.create("fork", "chat", main.sessionId, { createdBy: "fork" });
    expect(f.resolve(fork.currentLocator.path)).toEqual({ sessionPath: fork.currentLocator.path, scopeOwnerSessionPath: fork.currentLocator.path });
  });

  it("Research Root 和 Worker 沿真实父会话编号追到原始主会话", () => {
    const f = fixture();
    const main = f.create("main");
    const root = f.create("root", "knowledge_research_root", main.sessionId,
      { createdBy: "knowledge_research_root", studioId: f.studioId, researchContext: { role: "root", runId: "run", scopeId: "scope" } });
    const worker = f.create("worker", "knowledge_research_worker", root.sessionId,
      { createdBy: "knowledge_research_worker", studioId: f.studioId, researchContext: { role: "worker", runId: "run", scopeId: "scope" } });
    expect(f.resolve(root.currentLocator.path)).toEqual({ sessionPath: root.currentLocator.path, scopeOwnerSessionPath: main.currentLocator.path });
    expect(f.resolve(worker.currentLocator.path)).toEqual({ sessionPath: worker.currentLocator.path, scopeOwnerSessionPath: main.currentLocator.path });
  });

  it("普通 subagent 继续沿父会话继承，不要求历史主会话新增 Studio 字段", () => {
    const f = fixture();
    const main = f.create("legacy-main");
    const child = f.create("child", "subagent_child", main.sessionId);
    expect(f.resolve(child.currentLocator.path)).toEqual({ sessionPath: child.currentLocator.path, scopeOwnerSessionPath: main.currentLocator.path });
  });

  it("八层父级跳转可解析，第九层明确拒绝", () => {
    const f = fixture();
    const main = f.create("main");
    let child = main;
    for (let index = 1; index <= 8; index++) child = f.create(`level-${index}`, "subagent_child", child.sessionId);
    expect(f.resolve(child.currentLocator.path).scopeOwnerSessionPath).toBe(main.currentLocator.path);
    const ninth = f.create("level-9", "subagent_child", child.sessionId);
    expect(() => f.resolve(ninth.currentLocator.path)).toThrow(/limit/);
  });

  it("循环父链拒绝，不能碰巧遇到同路径就放行", () => {
    const f = fixture();
    const first = f.create("first", "subagent_child", "missing");
    const second = f.create("second", "subagent_child", first.sessionId);
    f.changeProvenance(first.sessionId, { parentSessionId: second.sessionId });
    expect(() => f.resolve(first.currentLocator.path)).toThrow(/cyclic/);
  });

  it("当前或父级登记缺失、父编号无效以及无法解析的上下文均拒绝", () => {
    const f = fixture();
    const missing = f.create("missing-parent", "subagent_child", "absent-session-id");
    expect(() => f.resolve(missing.currentLocator.path)).toThrow(/manifest/);
    expect(() => f.resolve(path.join(f.root, "absent.jsonl"))).toThrow(/missing/);
    expect(() => f.resolve(null)).toThrow(/session-bound/);
    expect(() => f.resolve("relative.jsonl")).toThrow(/cannot be resolved/);
    for (const parentSessionId of [null, "", [], 12]) {
      f.changeProvenance(missing.sessionId, { parentSessionId });
      expect(() => f.resolve(missing.currentLocator.path)).toThrow(/no parent/);
    }
    const main = f.create("main");
    expect(() => resolveKnowledgeScopeSessionContext({ sessionPath: main.currentLocator.path, studioId: f.studioId,
      getSessionIdForPath: f.getSessionIdForPath, getSessionManifest: () => null })).toThrow(/manifest/);
    expect(() => resolveKnowledgeScopeSessionContext({ sessionPath: main.currentLocator.path, studioId: f.studioId,
      getSessionIdForPath: () => { throw new Error("内部路径与数据库错误不得泄漏"); }, getSessionManifest: f.getSessionManifest }))
      .toThrow("Knowledge session ancestry cannot be resolved");
  });

  it("任何已声明 Studio 的当前或祖先身份不符都拒绝", () => {
    const f = fixture();
    const main = f.create("main", "chat", undefined, { studioId: f.studioId });
    const child = f.create("child", "subagent_child", main.sessionId, { studioId: f.studioId });
    f.changeProvenance(child.sessionId, { parentSessionId: main.sessionId, studioId: "another-studio" });
    expect(() => f.resolve(child.currentLocator.path)).toThrow(/studios/);
    f.changeProvenance(child.sessionId, { parentSessionId: main.sessionId, studioId: f.studioId });
    f.changeProvenance(main.sessionId, { studioId: "another-studio" });
    expect(() => f.resolve(child.currentLocator.path)).toThrow(/studios/);
    f.changeProvenance(main.sessionId, { studioId: f.studioId });
    expect(() => resolveKnowledgeScopeSessionContext({ sessionPath: child.currentLocator.path, studioId: f.studioId,
      getSessionIdForPath: f.getSessionIdForPath,
      getSessionManifest: id => ({ ...f.getSessionManifest(id)!, studioId: "another-studio" }),
    })).toThrow(/studios/);
  });

  it("绑定当前 Studio 的登记库找不到别的 Studio 的主会话时拒绝，而不是仅按路径认领", () => {
    const f = fixture();
    const foreign = new SessionManifestStore({ dbPath: path.join(f.root, "foreign-manifest.db") });
    try {
      const sessionPath = path.join(f.root, "foreign-main.jsonl");
      fs.writeFileSync(sessionPath, "");
      foreign.createForPath({ sessionPath, domain: "desktop", kind: "chat" });
      expect(() => f.resolve(sessionPath)).toThrow(/missing/);
    } finally { foreign.close(); }
  });

  it("已删除或移动失配的会话定位、无法回指登记身份的祖先定位均拒绝", () => {
    const f = fixture();
    const main = f.create("main");
    const child = f.create("child", "subagent_child", main.sessionId);
    f.manifests.db.prepare("UPDATE session_manifests SET lifecycle = 'deleted' WHERE session_id = ?").run(main.sessionId);
    expect(() => f.resolve(child.currentLocator.path)).toThrow(/locator is unavailable/);
    f.manifests.db.prepare("UPDATE session_manifests SET lifecycle = 'active' WHERE session_id = ?").run(main.sessionId);
    expect(() => resolveKnowledgeScopeSessionContext({ sessionPath: child.currentLocator.path, studioId: f.studioId,
      getSessionIdForPath: sessionPath => sessionPath === main.currentLocator.path ? null : f.getSessionIdForPath(sessionPath),
      getSessionManifest: f.getSessionManifest,
    })).toThrow(/locator is unavailable/);
    const nextPath = path.join(f.root, "main-moved.jsonl");
    fs.renameSync(main.currentLocator.path, nextPath);
    f.manifests.updateLocator(main.sessionId, nextPath, "move");
    expect(() => f.resolve(main.currentLocator.path)).toThrow(/does not match/);
    expect(f.resolve(nextPath).scopeOwnerSessionPath).toBe(nextPath);
  });

  it("不能用另一会话的登记回答当前会话查询，损坏的来源元数据也拒绝", () => {
    const f = fixture();
    const main = f.create("main"), other = f.create("other");
    expect(() => resolveKnowledgeScopeSessionContext({ sessionPath: main.currentLocator.path, studioId: f.studioId,
      getSessionIdForPath: f.getSessionIdForPath, getSessionManifest: () => other,
    })).toThrow(/manifest/);
    for (const invalid of [[], "not-a-record", null]) {
      f.changeProvenance(main.sessionId, invalid);
      expect(() => f.resolve(main.currentLocator.path)).toThrow(/provenance/);
    }
  });

  it("真实祖先解析结果通过范围门禁；缺少已核验拥有者时当前同路径也不放行", async () => {
    const f = fixture();
    const manager = new KnowledgeManager({ lingxiHome: path.join(f.root, "knowledge-home") });
    cleanup.push(() => manager.close());
    const main = f.create("main");
    const root = f.create("root", "knowledge_research_root", main.sessionId, { studioId: f.studioId });
    const worker = f.create("worker", "knowledge_research_worker", root.sessionId, { studioId: f.studioId });
    const notebook = manager.createNotebook({ studioId: f.studioId, name: "资料" });
    const scope = manager.createTurnScope({ studioId: f.studioId, sessionPath: main.currentLocator.path, notebookIds: [notebook.id] });
    const resolve = (context: KnowledgeToolSessionContext, studioId = f.studioId) => resolveKnowledgeTurnScope({
      knowledge: manager, studioId, scopeId: scope.id, sessionContext: context,
    });
    for (const manifest of [main, root, worker]) expect(resolve(f.resolve(manifest.currentLocator.path)).id).toBe(scope.id);
    expect(() => resolve({ sessionPath: main.currentLocator.path, scopeOwnerSessionPath: null })).toThrow(/owner must be resolved/);
    expect(() => resolve({ sessionPath: worker.currentLocator.path, scopeOwnerSessionPath: root.currentLocator.path })).toThrow(/does not belong/);
    expect(() => resolve(f.resolve(worker.currentLocator.path), "another-studio")).toThrow(/different studio/);
    const legacyShape = { sessionPath: main.currentLocator.path, parentSessionPath: main.currentLocator.path } as unknown as KnowledgeToolSessionContext;
    expect(() => resolve(legacyShape)).toThrow(/owner must be resolved/);
    manager.closeTurnScope({ scopeId: scope.id });
    expect(() => resolve(f.resolve(worker.currentLocator.path))).toThrow(/closed/);
  });

  it("研究子任务的目录、来源总数和警告只包含分配来源，非法子集与跨运行范围整单拒绝", async () => {
    const f = fixture();
    const manager = new KnowledgeManager({ lingxiHome: path.join(f.root, "knowledge-home") });
    cleanup.push(() => manager.close());
    const main = f.create("main");
    const root = f.create("root", "knowledge_research_root", main.sessionId, { studioId: f.studioId });
    const worker = f.create("worker", "knowledge_research_worker", root.sessionId, { studioId: f.studioId });
    const notebook = manager.createNotebook({ studioId: f.studioId, name: "资料" });
    const first = await manager.importPastedText({ studioId: f.studioId, notebookId: notebook.id, displayName: "甲资料", text: "甲资料原文。" });
    const second = await manager.importPastedText({ studioId: f.studioId, notebookId: notebook.id, displayName: "乙资料", text: "乙资料原文。" });
    const scope = manager.createTurnScope({ studioId: f.studioId, sessionPath: main.currentLocator.path, notebookIds: [notebook.id] });
    const research = new ResearchStore(manager.store);
    const run = research.createRun({ turnScopeId: scope.id, turnId: scope.turnId,
      parentSessionPath: scope.sessionPath, question: "研究分配来源" });
    const deps = { getKnowledge: () => manager, getStudioId: () => f.studioId,
      resolveSessionContext: () => f.resolve(worker.currentLocator.path) };
    const params = { scopeId: scope.id };
    const ordinary = await createKnowledgeOutlineTool(deps).execute("ordinary", params);
    expect(ordinary.isError).not.toBe(true);
    const ordinaryPayload = JSON.parse(ordinary.content[0].text);
    expect(ordinaryPayload.totalSources).toBe(2);
    expect(ordinaryPayload.notebooks[0].sources).toHaveLength(2);
    expect(ordinaryPayload.warnings).toEqual(expect.arrayContaining([`${first.source.id}:parse_pending`, `${second.source.id}:parse_pending`]));

    let allowedSourceIds: string[] | undefined = [first.source.id];
    let runId = run.id;
    const tool = createKnowledgeOutlineTool({ ...deps, resolveResearchContext: () => ({
      runId, actorSessionId: worker.sessionId, allowedSourceIds,
    }) });
    const limited = await tool.execute("limited", params);
    expect(limited.isError).not.toBe(true);
    const limitedPayload = JSON.parse(limited.content[0].text);
    expect(limitedPayload.totalSources).toBe(1);
    expect(limitedPayload.notebooks[0].sources).toEqual([expect.objectContaining({ sourceId: first.source.id })]);
    expect(limitedPayload.warnings).toEqual([`${first.source.id}:parse_pending`]);
    expect(limited.content[0].text).not.toContain(second.source.id);
    expect(limited.content[0].text).not.toContain("乙资料");
    allowedSourceIds = [];
    const empty = JSON.parse((await tool.execute("empty", params)).content[0].text);
    expect(empty).toMatchObject({ totalSources: 0, warnings: [], notebooks: [{ sources: [] }] });
    allowedSourceIds = undefined;
    expect(await tool.execute("unrestricted", params)).toEqual(ordinary);

    for (const invalid of [[first.source.id, "outside-source"], null, "not-an-array"]) {
      allowedSourceIds = invalid as unknown as string[];
      expect(await tool.execute("invalid", params)).toMatchObject({ isError: true, details: { errorCode: "KNOWLEDGE_SCOPE_VIOLATION" } });
    }
    allowedSourceIds = [first.source.id];
    const otherMain = f.create("other-main");
    const otherScope = manager.createTurnScope({ studioId: f.studioId, sessionPath: otherMain.currentLocator.path, notebookIds: [notebook.id] });
    runId = research.createRun({ turnScopeId: otherScope.id, turnId: otherScope.turnId,
      parentSessionPath: otherScope.sessionPath, question: "另一个运行" }).id;
    expect(await tool.execute("cross-run", params)).toMatchObject({ isError: true, details: { errorCode: "KNOWLEDGE_SCOPE_VIOLATION" } });
    // 子集筛选不能污染共享编译目录，随后普通调用仍返回完整范围。
    expect(await createKnowledgeOutlineTool(deps).execute("ordinary-after", params)).toEqual(ordinary);
  });
});
