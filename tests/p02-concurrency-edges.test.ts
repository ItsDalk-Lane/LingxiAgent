/**
 * P02-T04 并发与父子任务边界（A07/A08）：
 *  - A07 跨会话隔离：会话 A/B 并发登记工具执行与待审确认，取消 A 并"切换 UI"
 *    （A 的取消只作用于 A 的登记）→ B 的执行 signal、待审确认、任务状态不变。
 *  - A08 父子规则：取消父会话按登记策略终止附属任务；无父登记的独立后台任务
 *    不被误杀。
 *
 * 全部走真实登记处（SessionExecutionRegistry / ConfirmStore / TaskRegistry），
 * 不 mock 被验收对象。
 */

import { describe, expect, it, vi } from "vitest";
import { SessionExecutionRegistry } from "../lib/session-execution-registry.ts";
import { ConfirmStore } from "../lib/confirm-store.ts";
import { TaskRegistry } from "../lib/task-registry.ts";
import { mintTaskId } from "../lib/tasks/task-identity.ts";

const SESSION_A = "/tmp/p02-a.jsonl";
const SESSION_B = "/tmp/p02-b.jsonl";
const SESSION_ID_A = "sess_p02a";
const SESSION_ID_B = "sess_p02b";

describe("A07 跨会话隔离（并发工具与模型替身，取消 A 不改 B）", () => {
  it("abortBySession(A) 只中止 A 的工具执行；B 的 signal 未中止、结果与状态不变", async () => {
    const registry = new SessionExecutionRegistry();
    const execA = registry.begin({ sessionId: SESSION_ID_A, toolName: "exec_command", toolCallId: "tc-a" });
    const execB = registry.begin({ sessionId: SESSION_ID_B, toolName: "exec_command", toolCallId: "tc-b" });

    const resultA = registry.abortBySession({ sessionId: SESSION_ID_A }, "user_abort");

    expect(resultA).toEqual({ matched: 1, aborted: 1 });
    expect(execA.signal.aborted).toBe(true);
    // B 完全不受影响：signal 活着、登记仍在、activeCount 不变。
    expect(execB.signal.aborted).toBe(false);
    expect(registry.activeCount(SESSION_ID_B)).toBe(1);

    // B 正常完成并释放；A 释放后 A 的登记清空。
    let bObservedAbort = false;
    execB.signal.addEventListener("abort", () => { bObservedAbort = true; });
    execB.release();
    execA.release();
    expect(bObservedAbort).toBe(false);
    expect(registry.activeCount(SESSION_ID_A)).toBe(0);
    expect(registry.activeCount(SESSION_ID_B)).toBe(0);
  });

  it("confirmStore.abortBySession(A) 只失效 A 的待审；B 的待审仍可正常批准", async () => {
    const confirmStore = new ConfirmStore();
    const pendingA = confirmStore.create("tool_action_approval", { toolName: "write" }, {
      sessionId: SESSION_ID_A,
      sessionPath: SESSION_A,
    });
    const pendingB = confirmStore.create("tool_action_approval", { toolName: "write" }, {
      sessionId: SESSION_ID_B,
      sessionPath: SESSION_B,
    });

    confirmStore.abortBySession({ sessionId: SESSION_ID_A, sessionPath: SESSION_A });

    await expect(pendingA.promise).resolves.toEqual({ action: "aborted" });
    // B 的待审不受 A 取消影响，且晚到的批准对 B 依然有效。
    expect(confirmStore.resolve(pendingB.confirmId, "confirmed", undefined)).toBe(true);
    await expect(pendingB.promise).resolves.toEqual({ action: "confirmed" });
  });

  it("TaskRegistry.abortByParentSession(A) 不触碰 B 会话名下的任务", () => {
    const registry = new TaskRegistry();
    registry.registerHandler("media-generation", { abort: vi.fn() });
    const abortA = vi.fn();
    const abortB = vi.fn();
    registry.registerHandler("kind-a", { abort: abortA });
    registry.registerHandler("kind-b", { abort: abortB });

    const taskA = mintTaskId("a");
    const taskB = mintTaskId("b");
    registry.register(taskA, { type: "kind-a", parentSessionId: SESSION_ID_A, parentSessionPath: SESSION_A });
    registry.register(taskB, { type: "kind-b", parentSessionId: SESSION_ID_B, parentSessionPath: SESSION_B });

    const summary = registry.abortByParentSession(SESSION_A, "user_abort");

    expect(summary).toMatchObject({ matched: 1, aborted: 1 });
    expect(abortA).toHaveBeenCalledWith(taskA);
    expect(abortB).not.toHaveBeenCalled();
    expect(registry.query(taskA)).toMatchObject({ status: "aborted" });
    expect(registry.query(taskB)).toMatchObject({ status: "running" });
  });
});

describe("A08 父子规则（附属子任务随父终止；独立后台任务不误杀）", () => {
  it("取消父会话终止登记了该父的附属任务（按 parentSessionId 与移动后的 path 双匹配）", () => {
    const registry = new TaskRegistry();
    registry.registerHandler("subagent", { abort: vi.fn() });
    const attached = mintTaskId("subagent");
    registry.register(attached, {
      type: "subagent",
      parentSessionId: SESSION_ID_A,
      parentSessionPath: SESSION_A,
    });

    const summary = registry.abortByParentSession(SESSION_A, "parent aborted");
    expect(summary).toMatchObject({ matched: 1, aborted: 1 });
    expect(registry.query(attached)).toMatchObject({ status: "aborted", error: "parent aborted" });
  });

  it("独立后台任务（无父会话登记）不受无关会话取消影响", () => {
    const registry = new TaskRegistry();
    const abortHandler = vi.fn();
    registry.registerHandler("cron", { abort: abortHandler });
    const independent = mintTaskId("cron");
    registry.register(independent, { type: "cron" }); // 独立根：无 parentSession*

    const summary = registry.abortByParentSession(SESSION_A, "user_abort");

    expect(summary.matched).toBe(0);
    expect(abortHandler).not.toHaveBeenCalled();
    expect(registry.query(independent)).toMatchObject({ status: "running" });
  });

  it("父会话已终态的附属任务跳过（skippedFinal），不重复终止", () => {
    const registry = new TaskRegistry();
    registry.registerHandler("subagent", { abort: vi.fn() });
    const finished = mintTaskId("subagent");
    registry.register(finished, { type: "subagent", parentSessionId: SESSION_ID_A, parentSessionPath: SESSION_A });
    registry.complete(finished, "done");

    const summary = registry.abortByParentSession(SESSION_A, "parent aborted");
    expect(summary).toMatchObject({ matched: 1, skippedFinal: 1, aborted: 0 });
    expect(registry.query(finished)).toMatchObject({ status: "completed", result: "done" });
  });
});
