import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LingxiEngine } from "../core/engine.ts";
import * as llm from "../core/llm-client.ts";
import * as coverage from "../lib/knowledge/knowledge-coverage-planner.ts";
import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import { EvidencePacker } from "../lib/knowledge/evidence-packer.ts";
import { abortPendingDesktopSubmission, submitDesktopSessionInterjection, submitDesktopSessionMessage } from "../core/desktop-session-submit.ts";

const cleanups: Array<() => void> = [];
afterEach(() => { vi.restoreAllMocks(); for (const cleanup of cleanups.splice(0)) cleanup(); });

async function fixture() {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-fast-zero-remote-"));
  const fail = (name: string) => vi.fn(() => { throw new Error(`禁止调用 ${name}`); });
  const embedTextsForModel = fail("embedTextsForModel");
  const rerankForModel = fail("rerankForModel");
  const knowledgeModel = vi.spyOn(llm, "callText").mockImplementation(fail("knowledge model"));
  const coveragePlanner = vi.spyOn(coverage, "planKnowledgeCoverage").mockImplementation(fail("coverage planner"));
  const rollupModel = fail("rollup model");
  const executeIsolated = fail("executeIsolated");
  const manager = new KnowledgeManager({ lingxiHome: home, embedTextsForModel, rerankForModel });
  cleanups.push(() => { manager.close(); fs.rmSync(home, { recursive: true, force: true }); });
  const studioId = "studio";
  const notebook = manager.createNotebook({ studioId, name: "资料" });
  const imported = await manager.importPastedText({ studioId, notebookId: notebook.id, displayName: "审批.txt", text: "发布必须完成审批。" });
  const artifact = await manager.parseSource({ studioId, sourceId: imported.source.id });
  manager.enqueueSourceIngestion({ studioId, notebookId: notebook.id, sourceId: imported.source.id, artifactId: artifact.id });
  await manager.ingestion.drainQueue();
  const session = { model: null, subscribe: vi.fn(() => () => {}), sessionManager: { appendCustomEntry: vi.fn() } };
  // 使用真实引擎方法和管理器；只替换会话传输与不允许触碰的远程入口。
  const engine = Object.assign(Object.create(LingxiEngine.prototype), {
    _knowledge: manager, _runtimeContext: { studioId },
    ensureSessionLoaded: vi.fn(async () => session), getSessionStreamFn: vi.fn(() => rollupModel),
    getSharedModels: vi.fn(() => ({ knowledge: { provider: "fake", id: "fake" } })),
    resolveAuxiliaryModelFresh: fail("knowledge model resolution"), executeIsolated,
    promptSession: vi.fn(async (_path, _text, _options, hooks) => { hooks.afterCachePreflight(); }),
    steerSession: vi.fn(() => true), emitEvent: vi.fn(),
    renderSessionReminderBlock: vi.fn(() => null), preflightSessionInput: vi.fn(),
    isSessionStreaming: vi.fn(() => false),
  });
  const legacy = vi.spyOn(engine, "buildKnowledgeContextInjection").mockImplementation(fail("旧检索入口"));
  return { engine, manager, session, notebook, prohibited: [embedTextsForModel, rerankForModel, knowledgeModel,
    coveragePlanner, rollupModel, executeIsolated, legacy, engine.resolveAuxiliaryModelFresh, engine.getSessionStreamFn] };
}

describe("生产快速链路零远程调用", () => {
  it("真实检索、精确证据和清单持久化完成，六类远程入口均未触碰", async () => {
    const { engine, manager, notebook, prohibited } = await fixture();
    await submitDesktopSessionMessage(engine, { sessionPath: "/tmp/zero-remote.jsonl", text: "审批", clientMessageId: "fast-turn",
      knowledgeRefs: { notebookIds: [notebook.id], mode: "fast" } });
    const event = engine.emitEvent.mock.calls.find(([event]) => event.type === "session_user_message")[0];
    expect(event.message.knowledgeRetrieval).toMatchObject({ executionPath: "fast_local", remoteModelCalls: 0,
      vectorQueries: 0, rerankCalls: 0, ftsQueries: 1, injectedChunks: 1 });
    expect(engine.promptSession.mock.calls[0][1]).toContain("发布必须完成审批。");
    const manifest = manager.getEvidenceManifestByScope({ scopeId: event.message.knowledgeRetrieval.scopeId });
    expect(manifest.entries).toHaveLength(1);
    expect(manifest.entries[0].blockSpans[0].spans).toHaveLength(1);
    for (const remote of prohibited) expect(remote).not.toHaveBeenCalled();
  });

  for (const [name, submit] of [["普通发送", submitDesktopSessionMessage], ["追加消息", submitDesktopSessionInterjection]] as const) {
    it(`${name} 在范围编译期间取消后，真实管线不再提取和打包`, async () => {
      const { engine, manager, notebook, session } = await fixture();
      engine.isSessionStreaming.mockReturnValue(name === "追加消息");
      const compile = manager.compileTurnScope.bind(manager);
      const gate = Promise.withResolvers<void>();
      const compileSpy = vi.spyOn(manager, "compileTurnScope").mockImplementation(async scope => {
        const compiled = await compile(scope);
        await gate.promise;
        return compiled;
      });
      const extract = vi.spyOn(manager.queryService, "extractEvidenceSpans");
      const pack = vi.spyOn(EvidencePacker.prototype, "pack");
      const sessionPath = `/tmp/zero-cancel-${name}.jsonl`;
      const pending = submit(engine, { sessionPath, text: "审批", knowledgeRefs: { notebookIds: [notebook.id], mode: "fast" } });
      await vi.waitFor(() => expect(compileSpy).toHaveBeenCalledOnce());
      expect(abortPendingDesktopSubmission(engine, { sessionPath })).toBe(true);
      gate.resolve();
      await pending;
      expect(extract).not.toHaveBeenCalled();
      expect(pack).not.toHaveBeenCalled();
      expect(engine.promptSession).not.toHaveBeenCalled();
      expect(engine.steerSession).not.toHaveBeenCalled();
      expect(session.sessionManager.appendCustomEntry).not.toHaveBeenCalled();
    });
  }
});
