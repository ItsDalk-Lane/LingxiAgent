/**
 * autolearn-handler 总线记账测试。
 *
 * 覆盖：turn_start/tool_execution_end/turn_end 的账本聚合（调用数 + 轨迹头部）、
 * 中止轮丢弃账本、轨迹条数与单条长度上限、observeTurn 异常只告警不抛回总线、
 * 无 turn_start 的孤儿 turn_end 静默忽略。
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { registerAutolearnHandler } from "../server/autolearn-handler.ts";
import { AUTOLEARN_MAX_TRACE_ENTRIES } from "../lib/autolearn/autolearn-service.ts";

function makeBus() {
  const listeners: Array<(event: any, sessionPath?: string) => void> = [];
  return {
    subscribe: (fn: any) => { listeners.push(fn); return () => {}; },
    emit: (event: any, sessionPath?: string) => { for (const fn of listeners) fn(event, sessionPath); },
  };
}

describe("autolearn-handler", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  function setup(opts: any = {}) {
    const bus = makeBus();
    const observeTurn = vi.fn(async (_summary: any) => {});
    if (opts.observeThrows) observeTurn.mockRejectedValue(new Error("boom"));
    const log = { warn: vi.fn() };
    registerAutolearnHandler(bus, {
      observeTurn,
      getPermissionMode: () => opts.mode ?? "standard",
      log,
    });
    return { bus, observeTurn, log };
  }

  it("一轮结束：聚合计数与轨迹交给 observeTurn", () => {
    const { bus, observeTurn } = setup();
    bus.emit({ type: "turn_start" }, "/tmp/s.jsonl");
    bus.emit({ type: "tool_execution_end", toolName: "read", isError: false, result: "file body" }, "/tmp/s.jsonl");
    bus.emit({
      type: "tool_execution_end",
      toolName: "exec_command",
      isError: true,
      result: { content: [{ type: "text", text: "permission denied" }] },
    }, "/tmp/s.jsonl");
    bus.emit({ type: "turn_end" }, "/tmp/s.jsonl");

    expect(observeTurn).toHaveBeenCalledTimes(1);
    const summary = observeTurn.mock.calls[0]?.[0] as any;
    expect(summary.sessionPath).toBe("/tmp/s.jsonl");
    expect(summary.toolCalls).toBe(2);
    expect(summary.permissionMode).toBe("standard");
    expect(summary.aborted).toBe(false);
    expect(summary.trace).toEqual([
      { name: "read", ok: true, head: "file body" },
      { name: "exec_command", ok: false, head: "permission denied" },
    ]);
  });

  it("中止轮：账本丢弃，不触发观察", () => {
    const { bus, observeTurn } = setup();
    bus.emit({ type: "turn_start" }, "/tmp/s.jsonl");
    bus.emit({ type: "tool_execution_end", toolName: "read", isError: false }, "/tmp/s.jsonl");
    bus.emit({ type: "turn_end", aborted: true }, "/tmp/s.jsonl");
    expect(observeTurn).not.toHaveBeenCalled();
    // 后续孤儿 turn_end 也不触发
    bus.emit({ type: "turn_end" }, "/tmp/s.jsonl");
    expect(observeTurn).not.toHaveBeenCalled();
  });

  it("轨迹条目封顶且不带 args", () => {
    const { bus, observeTurn } = setup();
    bus.emit({ type: "turn_start" }, "/tmp/s.jsonl");
    for (let i = 0; i < AUTOLEARN_MAX_TRACE_ENTRIES + 10; i++) {
      bus.emit({
        type: "tool_execution_end",
        toolName: `t${i}`,
        isError: false,
        args: { path: "/secret/key.pem" },
        result: "x".repeat(500),
      }, "/tmp/s.jsonl");
    }
    bus.emit({ type: "turn_end" }, "/tmp/s.jsonl");
    const summary = observeTurn.mock.calls[0]?.[0] as any;
    expect(summary).toBeTruthy();
    expect(summary.toolCalls).toBe(AUTOLEARN_MAX_TRACE_ENTRIES + 10);
    expect(summary.trace.length).toBe(AUTOLEARN_MAX_TRACE_ENTRIES);
    expect(JSON.stringify(summary.trace)).not.toContain("key.pem");
    expect(summary.trace[0].head.length).toBeLessThanOrEqual(200);
  });

  it("observeTurn 抛错：只告警，不向总线传播", () => {
    const { bus, log } = setup({ observeThrows: true });
    bus.emit({ type: "turn_start" }, "/tmp/s.jsonl");
    expect(() => bus.emit({ type: "turn_end" }, "/tmp/s.jsonl")).not.toThrow();
    return new Promise((r) => setImmediate(r)).then(() => {
      expect(log.warn).toHaveBeenCalledWith(expect.stringContaining("observeTurn failed"));
    });
  });

  it("无 sessionPath 或无账本的事件一律忽略", () => {
    const { bus, observeTurn } = setup();
    bus.emit({ type: "turn_start" });
    bus.emit({ type: "tool_execution_end", toolName: "read" }, "/tmp/no-start.jsonl");
    bus.emit({ type: "turn_end" }, "/tmp/no-start.jsonl");
    bus.emit(null, "/tmp/s.jsonl");
    expect(observeTurn).not.toHaveBeenCalled();
  });
});
