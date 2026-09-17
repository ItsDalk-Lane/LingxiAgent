/**
 * 计划模式收工硬闸测试：只读档下停轮但没写计划/没问用户 → 注入提醒并续跑；
 * 写计划/提问/退出/中止都满足闸门；连推上限防死循环；投递失败不外抛。
 */
import { describe, expect, it, vi } from "vitest";
import {
  MAX_CONSECUTIVE_NUDGES,
  PLAN_GATE_MESSAGE_TYPE,
  registerPlanGateHandler,
} from "../server/plan-gate.ts";
import { planFilePathForSession } from "../lib/plan-mode/plan-file.ts";

const SESSION = "/tmp/agent/sessions/sess-1.jsonl";
const PLAN = planFilePathForSession(SESSION)!;

function makeHarness({ mode = "read_only", fileExists = () => false }: { mode?: string; fileExists?: (p: string) => boolean } = {}) {
  const listeners: Array<(event: any, sessionPath: string) => void> = [];
  const bus = {
    subscribe: (fn: any) => { listeners.push(fn); return () => {}; },
    emit: (event: any, sessionPath: string = SESSION) => {
      for (const fn of listeners) fn(event, sessionPath);
    },
  };
  const deliver = vi.fn(async (_sessionPath: string, _message: any) => ({ ok: true }));
  const warn = vi.fn();
  registerPlanGateHandler(bus, {
    getPermissionMode: () => mode,
    deliver,
    fileExists,
    log: { warn },
  });
  return { bus, deliver, warn };
}

/** 一轮无交付的计划模式 turn：start → end */
function idleTurn(bus: any) {
  bus.emit({ type: "turn_start" });
  bus.emit({ type: "turn_end" });
}

describe("计划模式收工硬闸", () => {
  it("本轮写了计划文件：闸门满足，不提醒", () => {
    const { bus, deliver } = makeHarness();
    bus.emit({ type: "turn_start" });
    bus.emit({ type: "tool_execution_end", toolName: "edit", args: { path: PLAN }, isError: false });
    bus.emit({ type: "turn_end" });
    expect(deliver).not.toHaveBeenCalled();
  });

  it("本轮问过 ask_user：闸门满足，不提醒", () => {
    const { bus, deliver } = makeHarness();
    bus.emit({ type: "turn_start" });
    bus.emit({ type: "session_confirmation", request: { kind: "ask_user" } });
    bus.emit({ type: "turn_end" });
    expect(deliver).not.toHaveBeenCalled();
  });

  it("无交付停轮：注入提醒并带计划文件绝对路径", async () => {
    const { bus, deliver } = makeHarness();
    idleTurn(bus);
    await vi.waitFor(() => expect(deliver).toHaveBeenCalledTimes(1));
    const [sp, message] = deliver.mock.calls[0];
    expect(sp).toBe(SESSION);
    expect(message.customType).toBe(PLAN_GATE_MESSAGE_TYPE);
    expect(message.display).toBe(false);
    expect(message.content).toContain(PLAN);
    expect(message.content).toContain(`nudge="1/${MAX_CONSECUTIVE_NUDGES}"`);
  });

  it("计划文件已存在时提醒措辞转为更新/提问", async () => {
    const { bus, deliver } = makeHarness({ fileExists: () => true });
    idleTurn(bus);
    await vi.waitFor(() => expect(deliver).toHaveBeenCalledTimes(1));
    expect(deliver.mock.calls[0][1].content).toContain("already exists");
  });

  it("非计划模式：闸门休眠", () => {
    const { bus, deliver } = makeHarness({ mode: "auto" });
    idleTurn(bus);
    expect(deliver).not.toHaveBeenCalled();
  });

  it("用户中止：不提醒且计数复位", async () => {
    const { bus, deliver } = makeHarness();
    bus.emit({ type: "turn_start" });
    bus.emit({ type: "turn_end", aborted: true });
    expect(deliver).not.toHaveBeenCalled();
    // 下一轮重新从 nudge 1 计
    idleTurn(bus);
    await vi.waitFor(() => expect(deliver).toHaveBeenCalledTimes(1));
    expect(deliver.mock.calls[0][1].content).toContain(`nudge="1/${MAX_CONSECUTIVE_NUDGES}"`);
  });

  it("连推到上限后停手，不再续跑", async () => {
    const { bus, deliver } = makeHarness();
    for (let i = 0; i < MAX_CONSECUTIVE_NUDGES + 2; i++) idleTurn(bus);
    await vi.waitFor(() => expect(deliver).toHaveBeenCalledTimes(MAX_CONSECUTIVE_NUDGES));
    // 再多几轮也不再推
    idleTurn(bus);
    idleTurn(bus);
    await new Promise((r) => setTimeout(r, 20));
    expect(deliver).toHaveBeenCalledTimes(MAX_CONSECUTIVE_NUDGES);
  });

  it("中途交了决策，计数复位", async () => {
    const { bus, deliver } = makeHarness();
    idleTurn(bus);
    await vi.waitFor(() => expect(deliver).toHaveBeenCalledTimes(1));
    // 下一轮写了计划：闸门满足
    bus.emit({ type: "turn_start" });
    bus.emit({ type: "tool_execution_end", toolName: "write", args: { path: PLAN } });
    bus.emit({ type: "turn_end" });
    // 再空一轮：重新从 nudge 1 计
    idleTurn(bus);
    await vi.waitFor(() => expect(deliver).toHaveBeenCalledTimes(2));
    expect(deliver.mock.calls[1][1].content).toContain(`nudge="1/${MAX_CONSECUTIVE_NUDGES}"`);
  });

  it("投递失败只记日志，不外抛", async () => {
    const listeners: Array<(event: any, sessionPath: string) => void> = [];
    const bus = {
      subscribe: (fn: any) => { listeners.push(fn); return () => {}; },
      emit: (event: any) => { for (const fn of listeners) fn(event, SESSION); },
    };
    const warn = vi.fn();
    registerPlanGateHandler(bus, {
      getPermissionMode: () => "read_only",
      deliver: vi.fn(async () => { throw new Error("session gone"); }),
      fileExists: () => false,
      log: { warn },
    });
    bus.emit({ type: "turn_start" });
    expect(() => bus.emit({ type: "turn_end" })).not.toThrow();
    await vi.waitFor(() => expect(warn).toHaveBeenCalled());
  });

  it("无关会话的事件零开销忽略", () => {
    const { bus, deliver } = makeHarness();
    bus.emit({ type: "turn_start" }, "");
    bus.emit({ type: "turn_end" }, "");
    expect(deliver).not.toHaveBeenCalled();
  });
});
