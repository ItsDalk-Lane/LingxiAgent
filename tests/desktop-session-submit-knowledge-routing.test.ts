import { describe, expect, it, vi } from "vitest";
import { abortPendingDesktopSubmission, submitDesktopSessionInterjection, submitDesktopSessionMessage } from "../core/desktop-session-submit.ts";

function fixture() {
  let currentSessionPath = "";
  const stats = { mode: "fast", scopeId: "scope", retrievalMode: "fts", subQueries: [], subQueryHits: [],
    degraded: false, fusedChunks: 1, injectedChunks: 1, truncated: false, usedTokens: 32, budgetTokens: 2400 };
  const evidence = { entries: [], searchedVectorVariants: [] };
  const session = { model: null, subscribe: vi.fn(() => () => {}), sessionManager: { appendCustomEntry: vi.fn() } };
  const engine = {
    getSessionIdForPath: vi.fn((sessionPath: string) => { currentSessionPath = sessionPath; return "session"; }),
    getSessionManifest: vi.fn(() => ({ sessionId: "session", ownerAgentId: "agent", lifecycle: "active", currentLocator: { path: currentSessionPath } })),
    ensureSessionLoaded: vi.fn(async () => session),
    promptSession: vi.fn(async () => {}), steerSession: vi.fn(() => true),
    isSessionStreaming: vi.fn(() => true), emitEvent: vi.fn(),
    buildFastKnowledgeContext: vi.fn(),
    buildConversationKnowledgeContext: vi.fn<(input: { signal: AbortSignal }) => Promise<{ block: string; stats: typeof stats; evidence: typeof evidence }>>(async () => ({ block: "[KnowledgeContext]\nlocal\n[/KnowledgeContext]", stats, evidence })),
    buildDetailedKnowledgeResearchContext: vi.fn(async () => ({ block: "[KnowledgeResearchContext]\ndetailed\n[/KnowledgeResearchContext]", stats: { ...stats, mode: "detailed", research: { status: "completed" } }, evidence })),
    buildKnowledgeContextInjection: vi.fn(),
    recordKnowledgeEvidenceManifest: vi.fn(),
  };
  return { engine, session, stats, evidence };
}

for (const [name, submit] of [["普通发送", submitDesktopSessionMessage], ["追加消息", submitDesktopSessionInterjection]] as const) {
  describe(name, () => {
    for (const mode of ["auto", "fast", "detailed"] as const) {
      it(`${mode} 统一使用当前聊天入口并传递取消信号，证据清单只在消息接受后保存`, async () => {
        const { engine, session } = fixture();
        // 普通发送需要空闲会话；追加消息必须维持正在生成状态。
        engine.isSessionStreaming.mockReturnValue(name === "追加消息");
        await submit(engine, { sessionPath: `/tmp/route-${name}-${mode}.jsonl`, text: "审批", clientMessageId: "turn",
          knowledgeRefs: { notebookIds: ["notebook"], mode } });
        const selected = engine.buildConversationKnowledgeContext;
        const unused = engine.buildDetailedKnowledgeResearchContext;
        expect(selected).toHaveBeenCalledOnce();
        expect(unused).not.toHaveBeenCalled();
        expect(engine.buildKnowledgeContextInjection).not.toHaveBeenCalled();
        expect(engine.buildFastKnowledgeContext).not.toHaveBeenCalled();
        expect(selected).toHaveBeenCalledWith({ knowledgeRefs: { notebookIds: ["notebook"], mode: "auto" },
          sessionPath: `/tmp/route-${name}-${mode}.jsonl`, turnId: "turn", signal: expect.any(AbortSignal),
          sessionId: "session",
        });
        expect(engine.recordKnowledgeEvidenceManifest).toHaveBeenCalledOnce();
        expect(session.sessionManager.appendCustomEntry.mock.invocationCallOrder[0])
          .toBeLessThan(engine.recordKnowledgeEvidenceManifest.mock.invocationCallOrder[0]);
      });
    }

    it("停止后不投影、不生成、不保存清单，并可立即再次发送", async () => {
      const { engine, session } = fixture();
      engine.isSessionStreaming.mockReturnValue(name === "追加消息");
      const sessionPath = `/tmp/cancel-${name}.jsonl`;
      let signal: AbortSignal;
      engine.buildConversationKnowledgeContext.mockImplementationOnce((input: { signal: AbortSignal }) => new Promise<never>((_resolve, reject) => {
        signal = input.signal;
        signal.addEventListener("abort", () => reject(signal.reason), { once: true });
      }));
      const pending = submit(engine, { sessionPath, text: "审批", knowledgeRefs: { notebookIds: ["notebook"], mode: "fast" } });
      await vi.waitFor(() => expect(signal).toBeInstanceOf(AbortSignal));
      expect(abortPendingDesktopSubmission(engine, { sessionPath })).toBe(true);
      expect(signal.aborted).toBe(true);
      expect(await pending).toEqual({ text: null, toolMedia: [], ...(name === "追加消息" ? { steered: false } : {}) });
      expect(engine.promptSession).not.toHaveBeenCalled();
      expect(engine.steerSession).not.toHaveBeenCalled();
      expect(session.sessionManager.appendCustomEntry).not.toHaveBeenCalled();
      expect(engine.recordKnowledgeEvidenceManifest).not.toHaveBeenCalled();
      expect(engine.emitEvent.mock.calls.map(([event]) => event.type)).not.toContain("session_user_message");
      expect(engine.emitEvent).toHaveBeenLastCalledWith({ type: "session_status", isStreaming: false, aborted: true, reason: "user_abort" }, sessionPath);
      expect(abortPendingDesktopSubmission(engine, { sessionPath })).toBe(false);
      await submit(engine, { sessionPath, text: "再次审批", knowledgeRefs: { notebookIds: ["notebook"], mode: "fast" } });
      expect(name === "追加消息" ? engine.steerSession : engine.promptSession).toHaveBeenCalledOnce();
    });
  });
}
