import { describe, expect, it, vi } from "vitest";
import { createChatRoute } from "../server/routes/chat.ts";

describe("chat route skill message handling", () => {
  it("routes a prompt with only skills to hub.send (text empty, skills present)", async () => {
    let createHandlers;
    const upgradeWebSocket = vi.fn((factory) => {
      createHandlers = factory;
      return () => new Response(null);
    });
    const hub = {
      subscribe: vi.fn(() => () => {}),
      send: vi.fn(async (_promptText: string) => {}),
      eventBus: { emit: vi.fn() },
    };
    const engine = {
      agentName: "Hana",
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

    createChatRoute(engine, hub, { upgradeWebSocket });
    const handlers = createHandlers({});
    const ws = { readyState: 1, send: vi.fn() };
    handlers.onOpen({}, ws);

    // 发送只有 skills 的 prompt（模拟用户点击快捷技能按钮）
    handlers.onMessage({
      data: JSON.stringify({
        type: "prompt",
        text: "",
        skills: ["character-creator"],
        sessionPath: "/sessions/test.jsonl",
        agentId: "agent-test",
      }),
    }, ws);
    await new Promise((resolve) => setTimeout(resolve, 0));

    // 检查 hub.send 是否被调用
    expect(hub.send).toHaveBeenCalledTimes(1);
    const call = hub.send.mock.calls[0];
    // 验证 promptText 是否正确合并了技能提示
    expect(call[0]).toContain("[Use skill: character-creator]");
  });
});
