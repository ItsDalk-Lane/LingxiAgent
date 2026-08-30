import { describe, expect, it, vi } from "vitest";
import { createChatRoute } from "../server/routes/chat.ts";

function setup() {
  let createHandlers: any;
  const upgradeWebSocket = vi.fn((factory: any) => {
    createHandlers = factory;
    return () => new Response(null);
  });
  const hub = {
    subscribe: vi.fn((_listener: (event: any, sessionPath: any) => void) => () => {}),
    send: vi.fn(async (_promptText: string, _opts?: any) => {}),
    eventBus: { emit: vi.fn() },
  };
  const engine = {
    agentName: "Lingxi",
    abortAllStreaming: vi.fn(async () => {}),
    getSessionByPath: vi.fn(() => ({
      entries: [],
      sessionManager: { getBranch: () => [] },
    })),
    isSessionStreaming: vi.fn(() => false),
    isSessionSwitching: vi.fn(() => false),
    steerSession: vi.fn(() => false),
    slashDispatcher: null,
  };
  createChatRoute(engine as any, hub as any, { upgradeWebSocket });
  const handlers = createHandlers({});
  const ws = { readyState: 1, send: vi.fn() };
  handlers.onOpen({}, ws);
  return { handlers, hub, ws };
}

function sentErrors(ws: { send: ReturnType<typeof vi.fn> }) {
  return ws.send.mock.calls
    .map((call) => {
      try { return JSON.parse(call[0] as string); } catch { return null; }
    })
    .filter((msg) => msg?.type === "error");
}

describe("chat route knowledgeRefs handling", () => {
  it("透传合法 knowledgeRefs 到 hub.send", async () => {
    const { handlers, hub, ws } = setup();
    handlers.onMessage({
      data: JSON.stringify({
        type: "prompt",
        text: "总结一下这个笔记本",
        sessionPath: "/sessions/test.jsonl",
        agentId: "agent-test",
        knowledgeRefs: { notebookIds: ["nb-1", "nb-2"], mode: "qa" },
      }),
    }, ws);
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(hub.send).toHaveBeenCalledTimes(1);
    expect(hub.send.mock.calls[0][1]).toEqual(expect.objectContaining({
      knowledgeRefs: { notebookIds: ["nb-1", "nb-2"], mode: "qa" },
    }));
    expect(sentErrors(ws)).toEqual([]);
  });

  it("未引用时不携带 knowledgeRefs 字段（null 透传，下游归一为无引用）", async () => {
    const { handlers, hub, ws } = setup();
    handlers.onMessage({
      data: JSON.stringify({
        type: "prompt",
        text: "普通消息",
        sessionPath: "/sessions/test.jsonl",
        agentId: "agent-test",
      }),
    }, ws);
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(hub.send).toHaveBeenCalledTimes(1);
    expect(hub.send.mock.calls[0][1].knowledgeRefs).toBeNull();
    expect(sentErrors(ws)).toEqual([]);
  });

  it("mode 非法时显式拒绝（invalid_knowledge_refs），不进入 hub.send", async () => {
    const { handlers, hub, ws } = setup();
    handlers.onMessage({
      data: JSON.stringify({
        type: "prompt",
        text: "你好",
        sessionPath: "/sessions/test.jsonl",
        agentId: "agent-test",
        knowledgeRefs: { notebookIds: ["nb-1"], mode: "strict" },
      }),
    }, ws);
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(hub.send).not.toHaveBeenCalled();
    const errors = sentErrors(ws);
    expect(errors).toHaveLength(1);
    expect(errors[0].code).toBe("invalid_knowledge_refs");
    expect(errors[0].sessionPath).toBe("/sessions/test.jsonl");
  });

  it("notebookIds 含非字符串/空串时显式拒绝", async () => {
    const { handlers, hub, ws } = setup();
    handlers.onMessage({
      data: JSON.stringify({
        type: "interject",
        text: "插话",
        sessionPath: "/sessions/test.jsonl",
        agentId: "agent-test",
        knowledgeRefs: { notebookIds: ["nb-1", 42], mode: "assist" },
      }),
    }, ws);
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(hub.send).not.toHaveBeenCalled();
    const errors = sentErrors(ws);
    expect(errors).toHaveLength(1);
    expect(errors[0].code).toBe("invalid_knowledge_refs");
  });

  it("knowledge_retrieval_started 引擎事件广播为同名 WS 消息", async () => {
    const { hub, ws } = setup();
    // setup() 里 hub.subscribe 被 mock；取出 createChatRoute 注册的事件监听器直接驱动。
    const listener = hub.subscribe.mock.calls[0][0];
    listener({ type: "knowledge_retrieval_started" }, "/sessions/test.jsonl");

    const sent = ws.send.mock.calls.map((call) => JSON.parse(call[0] as string));
    expect(sent).toContainEqual({
      type: "knowledge_retrieval_started",
      sessionPath: "/sessions/test.jsonl",
    });
  });

  it("knowledge_distill_progress 引擎事件广播为同名 WS 消息（带批数与模型）", async () => {
    const { hub, ws } = setup();
    const listener = hub.subscribe.mock.calls[0][0];
    listener({ type: "knowledge_distill_progress", done: 7, model: "opencode-go/deepseek-v4-flash" }, "/sessions/test.jsonl");

    const sent = ws.send.mock.calls.map((call) => JSON.parse(call[0] as string));
    expect(sent).toContainEqual({
      type: "knowledge_distill_progress",
      sessionPath: "/sessions/test.jsonl",
      done: 7,
      model: "opencode-go/deepseek-v4-flash",
    });
  });

  it("knowledge_coverage_progress 引擎事件广播为同名 WS 消息（带 runId/进度，可选 coverageStatus）", async () => {
    const { hub, ws } = setup();
    const listener = hub.subscribe.mock.calls[0][0];
    listener({
      type: "knowledge_coverage_progress",
      runId: "covrun_1",
      done: 3,
      total: 8,
      coverageStatus: "partial",
    }, "/sessions/test.jsonl");

    const sent = ws.send.mock.calls.map((call) => JSON.parse(call[0] as string));
    expect(sent).toContainEqual({
      type: "knowledge_coverage_progress",
      sessionPath: "/sessions/test.jsonl",
      runId: "covrun_1",
      done: 3,
      total: 8,
      coverageStatus: "partial",
    });
    // 缺省 coverageStatus 不强加字段（对齐 distill 进度的可选字段纪律）。
    ws.send.mockClear();
    listener({ type: "knowledge_coverage_progress", runId: "covrun_2", done: 8, total: 8 }, "/sessions/test.jsonl");
    const followup = ws.send.mock.calls.map((call) => JSON.parse(call[0] as string));
    expect(followup).toContainEqual({
      type: "knowledge_coverage_progress",
      sessionPath: "/sessions/test.jsonl",
      runId: "covrun_2",
      done: 8,
      total: 8,
    });
  });
});
