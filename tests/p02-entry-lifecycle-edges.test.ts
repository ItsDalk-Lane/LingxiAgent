/**
 * P02-T06 入口生命周期边界：WS 断线（窗口关闭/网络断开）的服务端行为。
 *
 * 产品语义：窗口关闭 ≠ 立即用户取消——断线进入宽限窗口；宽限内重连（焦点
 * 回归/重连）不中止流式；宽限到期仍无客户端才 abortAllStreaming。每个
 * WebSocket 连接由 upgradeWebSocket 工厂返回独立 handlers（closed 状态按
 * 连接隔离），与真实 @hono/node-ws 适配器一致。
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createChatRoute } from "../server/routes/chat.ts";

function makeRoute(sessionPath = "/tmp/p02-disconnect.jsonl") {
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
    getSessionByPath: vi.fn(() => null),
    isSessionStreaming: vi.fn(() => false),
    isSessionSwitching: vi.fn(() => false),
    steerSession: vi.fn(() => false),
    slashDispatcher: null,
  };
  createChatRoute(engine, hub, { upgradeWebSocket });
  return {
    subscriber,
    sessionPath,
    engine,
    hub,
    connect: () => {
      const handlers = createHandlers({});
      const ws = { readyState: 1, send: vi.fn() };
      handlers.onOpen({}, ws);
      return { handlers, ws };
    },
  };
}

describe("WS 断线宽限（窗口关闭不自动等同用户取消）", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    process.env.LINGXI_WS_DISCONNECT_ABORT_GRACE_MS = "5000";
  });

  afterEach(() => {
    vi.useRealTimers();
    delete process.env.LINGXI_WS_DISCONNECT_ABORT_GRACE_MS;
  });

  it("断线后宽限到期仍无客户端 → abortAllStreaming 恰一次", () => {
    const route = makeRoute();
    route.subscriber?.({ type: "agent_start" }, route.sessionPath);
    const conn = route.connect();

    conn.handlers.onClose({}, conn.ws);
    expect(route.engine.abortAllStreaming).not.toHaveBeenCalled();

    vi.advanceTimersByTime(4999);
    expect(route.engine.abortAllStreaming).not.toHaveBeenCalled();

    vi.advanceTimersByTime(1);
    expect(route.engine.abortAllStreaming).toHaveBeenCalledTimes(1);
  });

  it("宽限期内重连（窗口切换/焦点回归）→ 不中止流式", () => {
    const route = makeRoute();
    route.subscriber?.({ type: "agent_start" }, route.sessionPath);
    const first = route.connect();

    first.handlers.onClose({}, first.ws);
    const second = route.connect();

    vi.advanceTimersByTime(60000);
    expect(route.engine.abortAllStreaming).not.toHaveBeenCalled();
    second.handlers.onClose({}, second.ws);
  });

  it("宽限到期中止后，新连接再全断线会再次进入宽限兜底", () => {
    const route = makeRoute();
    route.subscriber?.({ type: "agent_start" }, route.sessionPath);
    const first = route.connect();

    first.handlers.onClose({}, first.ws);
    vi.advanceTimersByTime(5000);
    expect(route.engine.abortAllStreaming).toHaveBeenCalledTimes(1);

    const second = route.connect();
    second.handlers.onClose({}, second.ws);
    vi.advanceTimersByTime(5000);
    expect(route.engine.abortAllStreaming).toHaveBeenCalledTimes(2);
  });
});
