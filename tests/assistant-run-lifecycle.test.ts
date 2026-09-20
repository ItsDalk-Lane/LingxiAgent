import { describe, expect, it, vi } from "vitest";
import { createChatRoute } from "../server/routes/chat.ts";
import { installDesktopInputCommitObserver } from "../lib/pi-sdk/desktop-input-commit.ts";

/**
 * Assistant Run 生命周期回归测试（任务 §四十二/§三十九）。
 *
 * 关键不变量：
 *   1. Pi turn_start/turn_end 只描述 Model Turn，不描述 Assistant Run。
 *   2. agent_start 幂等创建 Assistant Run；多个 Model Turn 复用同一个 runId。
 *   3. agent_end 只记录低层 run 结果，绝不 finalize。
 *   4. agent_settled 才 finalize，且 exactly-once。
 *   5. 工具/思考/mood 跨 Model Turn 持续累计，绝不 reset。
 *   6. 整个 Run 只用一个 streamId。
 */

function makeHarness(sessionPath = "/tmp/assistant-run.jsonl") {
  let createHandlers;
  let subscriber;
  const upgradeWebSocket = vi.fn((factory) => {
    createHandlers = factory;
    return () => new Response(null);
  });
  const hub = {
    subscribe: vi.fn((fn) => { subscriber = fn; }),
    send: vi.fn(async () => {}),
    eventBus: { emit: vi.fn() },
  };
  const engine = {
    agentName: "Ming",
    abortAllStreaming: vi.fn(async () => {}),
    getSessionByPath: vi.fn(() => ({ entries: [] })),
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

function emitTool(subscriber, sessionPath, toolCallId, name = "read") {
  subscriber?.({ type: "tool_execution_start", toolCallId, toolName: name, args: {} }, sessionPath);
  subscriber?.({
    type: "tool_execution_end",
    toolCallId,
    toolName: name,
    result: { content: [{ type: "text", text: "ok" }] },
    isError: false,
  }, sessionPath);
}

function of(payloads, type) {
  return payloads().filter((p) => p.type === type);
}

function streamIds(payloads) {
  return [...new Set(payloads().map((p) => p.streamId).filter(Boolean))];
}

describe("Assistant Run 生命周期", () => {
  it("agent_start 创建 Run，多 Model Turn 复用 runId，agent_settled 才 finalize", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();
    const msg = assistantMessage();

    // ── Run 开始 ──
    subscriber?.({ type: "agent_start" }, sessionPath);

    // Model Turn 1: mood + thinking + tool 1
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message: msg }, sessionPath);
    subscriber?.({
      type: "message_update", message: msg,
      assistantMessageEvent: { type: "thinking_delta", delta: "先想想" },
    }, sessionPath);
    emitText(subscriber, sessionPath, msg, "<reflect>AAA</reflect>");
    emitTool(subscriber, sessionPath, "t1");
    subscriber?.({ type: "message_end", message: msg }, sessionPath);
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);

    // 第一个 turn_end 后：Run 必须仍然 active，绝不能 finalize
    expect(of(payloads, "assistant_run_start")).toHaveLength(1);
    expect(of(payloads, "assistant_run_end")).toHaveLength(0);
    expect(of(payloads, "turn_end")).toHaveLength(0);
    expect(streamIds(payloads)).toHaveLength(1);

    // Model Turn 2: mood + tool 2
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message: msg }, sessionPath);
    emitText(subscriber, sessionPath, msg, "<reflect>BBB</reflect>");
    emitTool(subscriber, sessionPath, "t2");
    subscriber?.({ type: "message_end", message: msg }, sessionPath);
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);

    expect(of(payloads, "assistant_run_end")).toHaveLength(0);
    expect(streamIds(payloads)).toHaveLength(1);
    // 工具持续累计：两个 tool_end 都在
    expect(of(payloads, "tool_end").map((p) => p.id)).toEqual(["t1", "t2"]);
    // 两个 mood 段都在
    expect(of(payloads, "mood_text").map((p) => p.delta)).toEqual(["AAA", "BBB"]);

    // Model Turn 3: thinking + tool 3
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message: msg }, sessionPath);
    subscriber?.({
      type: "message_update", message: msg,
      assistantMessageEvent: { type: "thinking_delta", delta: "再想想" },
    }, sessionPath);
    emitTool(subscriber, sessionPath, "t3");
    subscriber?.({ type: "message_end", message: msg }, sessionPath);
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);

    expect(of(payloads, "assistant_run_end")).toHaveLength(0);
    expect(of(payloads, "tool_end").map((p) => p.id)).toEqual(["t1", "t2", "t3"]);

    // Model Turn 4: final answer
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message: msg }, sessionPath);
    emitText(subscriber, sessionPath, msg, "最终回答。");
    subscriber?.({ type: "message_end", message: msg }, sessionPath);
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);

    // agent_end：即使有 final answer 也不得 finalize
    subscriber?.({ type: "agent_end", messages: [msg], willRetry: false }, sessionPath);
    expect(of(payloads, "assistant_run_end")).toHaveLength(0);

    // agent_settled：现在才 finalize，且 exactly-once
    subscriber?.({ type: "agent_settled" }, sessionPath);
    expect(of(payloads, "assistant_run_end")).toHaveLength(1);
    expect(of(payloads, "assistant_run_start")).toHaveLength(1);
    expect(streamIds(payloads)).toHaveLength(1);

    const runStart = of(payloads, "assistant_run_start")[0];
    const runEnd = of(payloads, "assistant_run_end")[0];
    expect(runEnd.runId).toBe(runStart.runId);
    expect(runEnd.status).toBe("completed");
  });

  it("retry：第一个 agent_end willRetry=true 后 Process summary/missing 都不得 finalize", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();
    const msg = assistantMessage();

    subscriber?.({ type: "agent_start" }, sessionPath);
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message: msg }, sessionPath);
    subscriber?.({
      type: "message_update", message: msg,
      assistantMessageEvent: { type: "error", error: "boom" },
    }, sessionPath);
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);
    subscriber?.({ type: "agent_end", messages: [msg], willRetry: true }, sessionPath);

    // 第一次 agent_end(willRetry=true) 后：绝不能 finalize
    expect(of(payloads, "assistant_run_end")).toHaveLength(0);

    subscriber?.({ type: "auto_retry_start", attempt: 1, maxAttempts: 3, delayMs: 0, errorMessage: "boom" }, sessionPath);
    subscriber?.({ type: "agent_start" }, sessionPath);
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message: msg }, sessionPath);
    emitTool(subscriber, sessionPath, "r1");
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message: msg }, sessionPath);
    emitText(subscriber, sessionPath, msg, "恢复后的答案");
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);
    subscriber?.({ type: "agent_end", messages: [msg], willRetry: false }, sessionPath);
    subscriber?.({ type: "agent_settled" }, sessionPath);

    // 第二次 agent_start 不得新建 Run（幂等）：整个 retry 只一个 Run
    expect(of(payloads, "assistant_run_start")).toHaveLength(1);
    expect(of(payloads, "assistant_run_end")).toHaveLength(1);
  });

  it("abort 路径：agent_settled 后以 aborted 状态 finalize 一次", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();
    const msg = assistantMessage();

    subscriber?.({ type: "agent_start" }, sessionPath);
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message: msg }, sessionPath);
    emitTool(subscriber, sessionPath, "a1");
    subscriber?.({ type: "turn_end", message: msg, toolResults: [], aborted: true }, sessionPath);
    subscriber?.({ type: "agent_end", messages: [msg], willRetry: false }, sessionPath);
    subscriber?.({ type: "agent_settled" }, sessionPath);

    const runEnd = of(payloads, "assistant_run_end")[0];
    expect(runEnd).toBeDefined();
    expect(runEnd.status).toBe("aborted");
    expect(of(payloads, "assistant_run_end")).toHaveLength(1);
  });

  it("插话（steer）落盘切开 Assistant Run：旧 Run 以 completed 收口，新 Run 复用 streamId", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();
    const msg = assistantMessage();

    subscriber?.({ type: "agent_start" }, sessionPath);
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message: msg }, sessionPath);
    emitTool(subscriber, sessionPath, "t1");
    subscriber?.({ type: "message_end", message: msg }, sessionPath);
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);

    expect(of(payloads, "assistant_run_start")).toHaveLength(1);
    expect(of(payloads, "assistant_run_end")).toHaveLength(0);

    // 插话走真实 steer 路径打标（SDK 包装层认出 steer 入队的 user 消息）：
    // 历史投影以 user 消息为回合边界，实时 Run 必须在插话落盘点切开。
    const steerSession = {
      agent: { prompt: async () => {}, steer: (..._args: any[]) => {}, subscribe: () => () => {} },
      prompt: async () => {},
    };
    installDesktopInputCommitObserver(steerSession);
    const steered = { role: "user", content: "有一点需要纠正" };
    steerSession.agent.steer(steered);
    subscriber?.({ type: "message_end", message: steered }, sessionPath);

    const runEnds = of(payloads, "assistant_run_end");
    const runStarts = of(payloads, "assistant_run_start");
    expect(runEnds).toHaveLength(1);
    expect(runStarts).toHaveLength(2);
    // 旧 Run 以 completed 收口（不是 aborted：它被新输入取代，终态裁决交给前端结局裁决）
    expect(runEnds[0].runId).toBe(runStarts[0].runId);
    expect(runEnds[0].status).toBe("completed");
    // 新 Run 有新 runId，但整个会话复用同一个 streamId
    expect(runStarts[1].runId).not.toBe(runStarts[0].runId);
    expect(streamIds(payloads)).toHaveLength(1);

    // 插话之后的输出属于新 Run；agent_settled 只终结新 Run（exactly-once）
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message: msg }, sessionPath);
    emitText(subscriber, sessionPath, msg, "改完了。");
    subscriber?.({ type: "message_end", message: msg }, sessionPath);
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);
    subscriber?.({ type: "agent_end", messages: [msg], willRetry: false }, sessionPath);
    subscriber?.({ type: "agent_settled" }, sessionPath);

    const finalEnds = of(payloads, "assistant_run_end");
    expect(finalEnds).toHaveLength(2);
    expect(finalEnds[1].runId).toBe(runStarts[1].runId);
    expect(finalEnds[1].status).toBe("completed");
    expect(streamIds(payloads)).toHaveLength(1);
  });

  it("正常 prompt 的用户消息 commit 不触发 Run 切分（即使晚于 agent_start 到达）", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();

    // 未激活 Run 时的 message_end(user)：不得制造任何 Run 事件。
    subscriber?.({ type: "message_end", message: { role: "user", content: "你好" } }, sessionPath);
    expect(of(payloads, "assistant_run_start")).toHaveLength(0);
    expect(of(payloads, "assistant_run_end")).toHaveLength(0);

    subscriber?.({ type: "agent_start" }, sessionPath);
    expect(of(payloads, "assistant_run_start")).toHaveLength(1);

    // 真实 SDK 中 prompt 的用户消息 message_end 可能晚于 agent_start：
    // 没有 steer 标记，不得误切 Run。
    subscriber?.({ type: "message_end", message: { role: "user", content: "你好" } }, sessionPath);
    expect(of(payloads, "assistant_run_start")).toHaveLength(1);
    expect(of(payloads, "assistant_run_end")).toHaveLength(0);
  });
});
