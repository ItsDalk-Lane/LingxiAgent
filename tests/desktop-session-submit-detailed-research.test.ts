import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { abortPendingDesktopSubmission, submitDesktopSessionInterjection, submitDesktopSessionMessage } from "../core/desktop-session-submit.ts";
import { SessionManifestStore } from "../core/session-manifest/store.ts";
import { KnowledgeError } from "../lib/knowledge/errors.ts";
import type { KnowledgeRetrievalStats } from "../shared/knowledge-refs.ts";

const cleanups: Array<() => void> = [];
afterEach(() => { for (const close of cleanups.splice(0)) close(); });

function result(status: "completed" | "partial" | "failed" | "cancelled" = "completed") {
  const stats: KnowledgeRetrievalStats = {
    mode: "detailed", executionPath: "detailed_research", scopeId: "scope", retrievalMode: "hybrid",
    subQueries: [], subQueryHits: [], degraded: status !== "completed", fusedChunks: 1, injectedChunks: 1,
    truncated: false, usedTokens: 200, budgetTokens: 6000,
    research: { runId: "run", status, completenessPolicy: "source_diverse", rounds: 2, toolCalls: 8,
      delegatedAgents: 1, needsTotal: 2, needsSupported: status === "completed" ? 2 : 1,
      needsPartial: status === "partial" ? 1 : 0, needsConflicted: 0,
      unresolvedNeedIds: status === "completed" ? [] : ["need-2"], stopReason: status === "completed" ? "complete" : "max_rounds" },
  };
  return { block: `[KnowledgeResearchContext]\nResearch status: ${status}\n[K1] 九月十五日交付\n[/KnowledgeResearchContext]`,
    stats, evidence: { entries: [], searchedVectorVariants: [] } };
}

function fixture(interject: boolean) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "desktop-research-submit-"));
  const sessionPath = path.join(root, "main.jsonl"); fs.writeFileSync(sessionPath, "");
  const manifests = new SessionManifestStore({ dbPath: path.join(root, "manifests.db") });
  const manifest = manifests.createForPath({ sessionPath, ownerAgentId: "real-agent", domain: "desktop", kind: "chat" });
  cleanups.push(() => { manifests.close(); fs.rmSync(root, { recursive: true, force: true }); });
  const session = { model: null, subscribe: vi.fn(() => () => {}), sessionManager: { appendCustomEntry: vi.fn() } };
  const engine = {
    getSessionIdForPath: vi.fn((file: string) => manifests.resolveByLocatorPath(file)?.sessionId ?? null),
    getSessionManifest: vi.fn((id: string) => manifests.getBySessionId(id)),
    ensureSessionLoaded: vi.fn(async () => session), isSessionStreaming: vi.fn(() => interject),
    promptSession: vi.fn(async () => {}), steerSession: vi.fn(() => true), emitEvent: vi.fn(),
    buildDetailedKnowledgeResearchContext: vi.fn<(input: { signal: AbortSignal }) => Promise<ReturnType<typeof result>>>(async () => result()),
    buildKnowledgeContextInjection: vi.fn(async () => { throw new Error("详细模式不得进入旧调查路径"); }),
    buildFastKnowledgeContext: vi.fn(async () => { throw new Error("详细模式不得进入快速路径"); }),
    recordKnowledgeEvidenceManifest: vi.fn(),
  };
  return { engine, session, manifest, sessionPath, manifests, opts: { sessionId: manifest.sessionId, sessionPath,
    text: "什么时候交付？", clientMessageId: "accepted-client-turn", knowledgeRefs: { notebookIds: ["notebook"], mode: "detailed" as const } } };
}

for (const [name, submit, interject] of [["普通发送", submitDesktopSessionMessage, false], ["追加消息", submitDesktopSessionInterjection, true]] as const) {
  describe(`${name}的详细调查`, () => {
    for (const status of ["completed", "partial"] as const) {
      it(`等待${status}材料再提交，身份来自真实会话登记且不调用旧路径`, async () => {
        const f = fixture(interject);
        let resolve!: (value: ReturnType<typeof result>) => void;
        f.engine.buildDetailedKnowledgeResearchContext.mockImplementation(() => new Promise(done => { resolve = done; }));
        const untrustedExtra = { ...f.opts, agentId: "模型伪造的Agent" };
        const pending = submit(f.engine, untrustedExtra);
        await vi.waitFor(() => expect(resolve).toBeTypeOf("function"));
        expect(f.engine.promptSession).not.toHaveBeenCalled();
        expect(f.engine.steerSession).not.toHaveBeenCalled();
        expect(f.session.sessionManager.appendCustomEntry).not.toHaveBeenCalled();
        expect(f.engine.buildDetailedKnowledgeResearchContext).toHaveBeenCalledWith({ question: f.opts.text,
          knowledgeRefs: f.opts.knowledgeRefs, sessionId: f.manifest.sessionId, sessionPath: f.sessionPath,
          agentId: f.manifest.ownerAgentId, turnId: f.opts.clientMessageId, signal: expect.any(AbortSignal) });
        const material = result(status); resolve(material); await pending;
        expect(interject ? f.engine.steerSession : f.engine.promptSession).toHaveBeenCalledWith(
          f.sessionPath, `${material.block}\n\n${f.opts.text}`, ...(interject ? [] : [undefined]));
        expect(f.engine.recordKnowledgeEvidenceManifest).toHaveBeenCalledWith({ sessionPath: f.sessionPath, stats: material.stats, evidence: material.evidence });
        expect(f.engine.buildKnowledgeContextInjection).not.toHaveBeenCalled();
        expect(f.engine.buildFastKnowledgeContext).not.toHaveBeenCalled();
      });
    }

    it("保留调查的真实失败，不用空知识继续回答，并释放提交登记", async () => {
      const f = fixture(interject);
      const failure = new KnowledgeError("KNOWLEDGE_MODEL_UNAVAILABLE", "调查模型不可用", { researchStatus: "failed" });
      f.engine.buildDetailedKnowledgeResearchContext.mockRejectedValueOnce(failure);
      await expect(submit(f.engine, f.opts)).rejects.toBe(failure);
      expect(f.engine.promptSession).not.toHaveBeenCalled(); expect(f.engine.steerSession).not.toHaveBeenCalled();
      expect(f.engine.recordKnowledgeEvidenceManifest).not.toHaveBeenCalled();
      expect(f.session.sessionManager.appendCustomEntry).not.toHaveBeenCalled();
      expect(abortPendingDesktopSubmission(f.engine, { sessionId: f.manifest.sessionId })).toBe(false);
      await submit(f.engine, f.opts);
      expect(interject ? f.engine.steerSession : f.engine.promptSession).toHaveBeenCalledOnce();
    });

    it("停止后等待研究真实清理，不进入主回答且保留中止结果", async () => {
      const f = fixture(interject);
      let signal!: AbortSignal; let cleanup!: () => void; let settled = false;
      f.engine.buildDetailedKnowledgeResearchContext.mockImplementationOnce(input => new Promise((_resolve, reject) => {
        signal = input.signal;
        signal.addEventListener("abort", () => { cleanup = () => reject(signal.reason); }, { once: true });
      }));
      const pending = submit(f.engine, f.opts).finally(() => { settled = true; });
      await vi.waitFor(() => expect(signal).toBeInstanceOf(AbortSignal));
      expect(abortPendingDesktopSubmission(f.engine, { sessionId: f.manifest.sessionId })).toBe(true);
      expect(signal.aborted).toBe(true); await Promise.resolve(); expect(settled).toBe(false);
      cleanup();
      expect(await pending).toEqual({ text: null, toolMedia: [], ...(interject ? { steered: false } : {}) });
      expect(f.engine.promptSession).not.toHaveBeenCalled(); expect(f.engine.steerSession).not.toHaveBeenCalled();
      expect(f.session.sessionManager.appendCustomEntry).not.toHaveBeenCalled();
      expect(f.engine.recordKnowledgeEvidenceManifest).not.toHaveBeenCalled();
      expect(f.engine.emitEvent).toHaveBeenLastCalledWith({ type: "session_status", isStreaming: false, aborted: true, reason: "user_abort" }, f.sessionPath);
      expect(abortPendingDesktopSubmission(f.engine, { sessionId: f.manifest.sessionId })).toBe(false);
    });

    it("停止期间清理失败仍原样报告，不伪装成正常中止", async () => {
      const f = fixture(interject);
      const failure = new Error("临时调查会话清理失败");
      let signal!: AbortSignal;
      f.engine.buildDetailedKnowledgeResearchContext.mockImplementationOnce(input => new Promise((_resolve, reject) => {
        signal = input.signal;
        signal.addEventListener("abort", () => reject(failure), { once: true });
      }));
      const pending = submit(f.engine, f.opts);
      const rejected = expect(pending).rejects.toBe(failure);
      await vi.waitFor(() => expect(signal).toBeInstanceOf(AbortSignal));
      expect(abortPendingDesktopSubmission(f.engine, { sessionId: f.manifest.sessionId })).toBe(true);
      await rejected;
      expect(f.engine.promptSession).not.toHaveBeenCalled(); expect(f.engine.steerSession).not.toHaveBeenCalled();
      expect(f.engine.recordKnowledgeEvidenceManifest).not.toHaveBeenCalled();
    });

    for (const status of ["failed", "cancelled"] as const) {
      it(`拒绝把${status}状态当成可回答材料`, async () => {
        const f = fixture(interject); f.engine.buildDetailedKnowledgeResearchContext.mockResolvedValueOnce(result(status));
        await expect(submit(f.engine, f.opts)).rejects.toThrow();
        expect(f.engine.promptSession).not.toHaveBeenCalled(); expect(f.engine.steerSession).not.toHaveBeenCalled();
        expect(f.session.sessionManager.appendCustomEntry).not.toHaveBeenCalled();
      });
    }

    it("会话登记失效时拒绝调查；没有客户轮号时由宿主生成轮号", async () => {
      const f = fixture(interject);
      f.engine.getSessionIdForPath.mockReturnValueOnce(null);
      await expect(submit(f.engine, f.opts)).rejects.toThrow();
      expect(f.engine.buildDetailedKnowledgeResearchContext).not.toHaveBeenCalled();
      await submit(f.engine, { ...f.opts, clientMessageId: undefined });
      expect(f.engine.buildDetailedKnowledgeResearchContext).toHaveBeenCalledWith(expect.objectContaining({
        sessionId: f.manifest.sessionId, agentId: "real-agent", turnId: expect.stringMatching(/^[0-9a-f]{8}-[0-9a-f-]{27}$/),
      }));
    });

    if (!interject) {
      it("会话加载期间已停止时不再启动调查", async () => {
        const f = fixture(interject);
        let loaded!: () => void;
        f.engine.ensureSessionLoaded.mockImplementationOnce(() => new Promise(resolve => { loaded = () => resolve(f.session); }));
        const pending = submit(f.engine, f.opts);
        expect(abortPendingDesktopSubmission(f.engine, { sessionId: f.manifest.sessionId })).toBe(true);
        loaded();
        expect(await pending).toEqual({ text: null, toolMedia: [] });
        expect(f.engine.buildDetailedKnowledgeResearchContext).not.toHaveBeenCalled();
        expect(f.engine.promptSession).not.toHaveBeenCalled();
      });
    }
  });
}
