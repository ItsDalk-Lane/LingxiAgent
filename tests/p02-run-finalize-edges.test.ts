/**
 * P02 运行终态裁决边界（A02/A04）：
 *  - A02 同 generation 重复发送 finish 事件（agent_settled×N）→ 无双写结果、
 *    双通知或重复计费（assistant_run_end 与 token_usage 各恰一次）。
 *  - A04 取消后到达的迟到 delta / 重复 settled → 不启动后续工具投影、迟到数据
 *    不改终态（aborted 保持）、不再计费。
 *
 * 与 tests/assistant-run-lifecycle.test.ts 同一真实链路（真实 chat 路由 + 真实
 * hub 订阅分发），聚焦重复 finish 与迟到事件的终态幂等。
 */

import { describe, expect, it, vi } from "vitest";
import { createChatRoute } from "../server/routes/chat.ts";

function makeHarness(sessionPath = "/tmp/p02-run-edges.jsonl", { withAssistant = true } = {}) {
  let createHandlers;
  let subscriber;
  const upgradeWebSocket = vi.fn((factory) => {
    createHandlers = factory;
    return () => new Response(null);
  });
  const usageMessage = {
    role: "assistant",
    stopReason: "end_turn",
    timestamp: Date.now(),
    usage: { input: 10, output: 5, cacheRead: 0, cacheWrite: 0, totalTokens: 15 },
    content: [{ type: "text", text: "答案" }],
  };
  const branch = [
    { type: "message", id: "u1", message: { role: "user", content: "问" } },
    ...(withAssistant ? [{ type: "message", id: "a1", message: usageMessage }] : []),
  ];
  const hub = {
    subscribe: vi.fn((fn) => { subscriber = fn; }),
    send: vi.fn(async () => {}),
    eventBus: { emit: vi.fn() },
  };
  const engine = {
    agentName: "Ming",
    abortAllStreaming: vi.fn(async () => {}),
    getSessionByPath: vi.fn(() => ({
      entries: [{ type: "message", message: usageMessage }],
      sessionManager: { getBranch: () => branch },
      model: { api: "anthropic", id: "model-x", provider: "test", cost: null },
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
  const payloads = () => ws.send.mock.calls.map(([raw]) => JSON.parse(raw));
  return { subscriber, sessionPath, payloads, handlers, ws, engine, hub };
}

function assistantMessage() {
  return { role: "assistant" };
}

function emitText(subscriber, sessionPath, message, delta) {
  subscriber?.({ type: "message_update", message, assistantMessageEvent: { type: "text_delta", delta } }, sessionPath);
}

function emitToolStart(subscriber, sessionPath, toolCallId) {
  subscriber?.({ type: "tool_execution_start", toolCallId, toolName: "read", args: {} }, sessionPath);
}

function runOneTurn(subscriber, sessionPath, msg) {
  subscriber?.({ type: "agent_start" }, sessionPath);
  subscriber?.({ type: "turn_start" }, sessionPath);
  subscriber?.({ type: "message_start", message: msg }, sessionPath);
  emitText(subscriber, sessionPath, msg, "正文");
  subscriber?.({ type: "message_end", message: msg }, sessionPath);
  subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);
}

function of(payloads, type) {
  return payloads().filter((p) => p.type === type);
}

describe("P02 运行终态裁决边界", () => {
  it("A02：同 generation 重复 agent_settled → run_end 与 token_usage 各恰一次（无双写/重复计费）", () => {
    const { subscriber, sessionPath, payloads, hub } = makeHarness();
    const msg = assistantMessage();

    runOneTurn(subscriber, sessionPath, msg);
    subscriber?.({ type: "agent_end", messages: [msg], willRetry: false }, sessionPath);

    // 重复 finish：同 generation 连发三次 agent_settled。
    subscriber?.({ type: "agent_settled" }, sessionPath);
    subscriber?.({ type: "agent_settled" }, sessionPath);
    subscriber?.({ type: "agent_settled" }, sessionPath);

    expect(of(payloads, "assistant_run_start")).toHaveLength(1);
    const runEnds = of(payloads, "assistant_run_end");
    expect(runEnds).toHaveLength(1);
    expect(runEnds[0].status).toBe("completed");

    // 一次 agent run 只记一次账：token_usage 恰一条。
    const usageEmits = hub.eventBus.emit.mock.calls.filter(([event]) => event?.type === "token_usage");
    expect(usageEmits).toHaveLength(1);
    expect(usageEmits[0][0].usage.totalTokens).toBe(15);
  });

  it("A04：取消 finalize 后到达的迟到 delta/工具开始/重复 settled → 终态不被逆转、不再计费", () => {
    // 分支里只有 user 输入（abort 落在工具等待期，无已落盘 assistant）→ 计费跳过。
    const { subscriber, sessionPath, payloads, hub } = makeHarness("/tmp/p02-run-edges-a04.jsonl", { withAssistant: false });
    const msg = assistantMessage();

    subscriber?.({ type: "agent_start" }, sessionPath);
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message: msg }, sessionPath);
    emitToolStart(subscriber, sessionPath, "t1");
    subscriber?.({ type: "turn_end", message: msg, toolResults: [], aborted: true }, sessionPath);
    subscriber?.({ type: "agent_end", messages: [msg], willRetry: false }, sessionPath);
    subscriber?.({ type: "agent_settled" }, sessionPath);

    const abortedEnds = of(payloads, "assistant_run_end");
    expect(abortedEnds).toHaveLength(1);
    expect(abortedEnds[0].status).toBe("aborted");

    // 迟到事件：模型替身晚到的 delta、工具开始与重复 settled。
    emitText(subscriber, sessionPath, msg, "迟到片段");
    emitToolStart(subscriber, sessionPath, "t-late");
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);
    subscriber?.({ type: "agent_settled" }, sessionPath);

    // 终态不被逆转：仍只有一个 run_end，且保持 aborted；不产生新 Run。
    expect(of(payloads, "assistant_run_end")).toHaveLength(1);
    expect(of(payloads, "assistant_run_end")[0].status).toBe("aborted");
    expect(of(payloads, "assistant_run_start")).toHaveLength(1);
    // 中止且无已落盘 assistant → 不计费，迟到数据也不补账。
    const usageEmits = hub.eventBus.emit.mock.calls.filter(([event]) => event?.type === "token_usage");
    expect(usageEmits).toHaveLength(0);
  });

  it("A04 补充：aborted run 已有落盘 assistant 时计一次费，重复 settled 不双计", () => {
    const { subscriber, sessionPath, payloads, hub } = makeHarness();
    const msg = assistantMessage();

    // 模拟 abort 前已有 assistant 落盘（branch/entries 里带 usage 的 assistant）。
    runOneTurn(subscriber, sessionPath, msg);
    subscriber?.({ type: "message_update", message: msg, assistantMessageEvent: { type: "error", error: "x" } }, sessionPath);
    subscriber?.({ type: "turn_end", message: msg, toolResults: [], aborted: true }, sessionPath);
    subscriber?.({ type: "agent_settled" }, sessionPath);
    subscriber?.({ type: "agent_settled" }, sessionPath);

    expect(of(payloads, "assistant_run_end")).toHaveLength(1);
    expect(of(payloads, "assistant_run_end")[0].status).toBe("aborted");
    const usageEmits = hub.eventBus.emit.mock.calls.filter(([event]) => event?.type === "token_usage");
    expect(usageEmits).toHaveLength(1);
  });
});
