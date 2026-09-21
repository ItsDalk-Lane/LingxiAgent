/**
 * P02-T01 任务身份契约测试：taskId 统一铸造 + TaskRegistry attempt 栅栏。
 *
 * 对应验收场景 P02-A03（合法 task 复用，旧回调不能覆盖新状态）及
 * IDENTITY_CONTRACT.md 的兼容规则（旧格式 taskId 原值读、缺失 attempt 按 1）。
 */

import fs from "fs";
import os from "os";
import path from "path";
import { describe, expect, it, vi } from "vitest";

import { TaskRegistry } from "../lib/task-registry.ts";
import { createTaskIdentityFactory, mintTaskId } from "../lib/tasks/task-identity.ts";
import {
  asTaskId,
  requireTaskId,
} from "../shared/identity-brands.ts";
import { registerTaskRegistryBusHandlers } from "../server/task-bus-handlers.ts";

describe("taskId 统一铸造厂（task-identity）", () => {
  it("新铸造 taskId 满足 task_ 品牌：前缀 + kind 段 + ts36/seq36/rand6", () => {
    const id = mintTaskId("subagent");
    expect(typeof id).toBe("string");
    expect(asTaskId(id)).toBe(id);
    expect(id).toMatch(/^task_subagent_[a-z0-9]+_[a-z0-9]+_[a-z0-9]+$/);
  });

  it("kind 归一化：非法字符剥离，空 kind 落到 gen 段", () => {
    expect(mintTaskId()).toMatch(/^task_gen_/);
    expect(mintTaskId("Media-Gen!")).toMatch(/^task_mediagen_/);
  });

  it("同工厂注入确定性 now/random 时输出可复现，且进程内 seq 递增防碰撞", () => {
    const factory = createTaskIdentityFactory({ now: () => 1234, random: () => "abc123" });
    expect(factory.mint("workflow")).toBe("task_workflow_ya_1_abc123");
    expect(factory.mint("workflow")).toBe("task_workflow_ya_2_abc123");
  });

  it("品牌守卫拒绝跨身份误用：sess_/mc_/mt_ 与任意字符串都不能当 TaskId", () => {
    expect(asTaskId("sess_abc")).toBeNull();
    expect(asTaskId("mc_abc_def")).toBeNull();
    expect(asTaskId("mt_abc")).toBeNull();
    expect(asTaskId("subagent-legacy-1")).toBeNull();
    expect(() => requireTaskId("mt_abc")).toThrow(TypeError);
  });
});

describe("TaskRegistry attempt 栅栏（P02-A03）", () => {
  function makeRegistry() {
    const reg = new TaskRegistry();
    reg.registerHandler("render", { abort: vi.fn() });
    return reg;
  }

  it("新任务 attempt=1；活跃期重复 register 不递增（幂等）", () => {
    const reg = makeRegistry();
    const first = reg.register("t1", { type: "render" });
    expect(first.attempt).toBe(1);
    const again = reg.register("t1", { type: "render" });
    expect(again.attempt).toBe(1);
  });

  it("终态后重新 register 承接下一次执行：attempt+1、终态清零、progress 不继承", () => {
    const reg = makeRegistry();
    reg.register("t1", { type: "render" });
    reg.update("t1", { progress: { current: 3, total: 4 } });
    reg.complete("t1", { url: "a.png" });

    const second = reg.register("t1", { type: "render" });
    expect(second.attempt).toBe(2);
    expect(second.status).toBe("running");
    expect(second.result).toBeUndefined();
    expect(second.completedAt).toBeUndefined();
    expect(second.progress).toBeNull();
  });

  it("旧 attempt 的迟到 complete/fail/update 被拒绝且不落盘；新 attempt 正常收口", () => {
    const reg = makeRegistry();
    reg.register("t1", { type: "render" });
    reg.complete("t1", "first-run");
    const second = reg.register("t1", { type: "render" });
    expect(second.attempt).toBe(2);

    // 旧执行的迟到回调：attempt=1，发生在 attempt=2 运行中。
    expect(reg.complete("t1", { late: true }, { expectedAttempt: 1 })).toBeNull();
    expect(reg.fail("t1", "late failure", { expectedAttempt: 1 })).toBeNull();
    expect(reg.update("t1", { progress: { current: 99, total: 100 } }, { expectedAttempt: 1 })).toBeNull();

    const current = reg.query("t1");
    expect(current).toMatchObject({ attempt: 2, status: "running" });
    expect(current.result).toBeUndefined();
    expect(current.error).toBeUndefined();
    expect(current.progress).toBeNull();

    // 新执行自己的回调正常生效。
    const done = reg.complete("t1", { ok: true }, { expectedAttempt: 2 });
    expect(done).toMatchObject({ attempt: 2, status: "completed", result: { ok: true } });
  });

  it("不带 expectedAttempt 的调用保持旧语义（不栅栏）；非法 expectedAttempt 抛错", () => {
    const reg = makeRegistry();
    reg.register("t1", { type: "render" });
    expect(reg.update("t1", { progress: { current: 1, total: 2 } })).toMatchObject({ attempt: 1 });
    expect(() => reg.complete("t1", null, { expectedAttempt: 0 })).toThrow(/positive integer/);
    expect(() => reg.complete("t1", null, { expectedAttempt: "2" as any })).toThrow(/positive integer/);
  });

  it("终态 first-write-wins：重复 complete 不改写首次结果，fail 不能翻盘 completed", () => {
    const reg = makeRegistry();
    reg.register("t1", { type: "render" });
    reg.complete("t1", { url: "first.png" });
    expect(reg.complete("t1", { url: "second.png" })).toMatchObject({ result: { url: "first.png" } });
    expect(reg.fail("t1", "later failure")).toMatchObject({ status: "completed", result: { url: "first.png" } });
    // 新执行仍可通过合法复用开始。
    expect(reg.register("t1", { type: "render" })).toMatchObject({ attempt: 2, status: "running" });
  });

  it("持久化往返保留 attempt；重启恢复的活跃任务不因 attempt 改变栅栏语义", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-task-attempt-"));
    const persistencePath = path.join(dir, "tasks.json");
    const reg = new TaskRegistry({ persistencePath });
    reg.registerHandler("render", { abort: vi.fn() });
    reg.register("t1", { type: "render" });
    reg.complete("t1", "run-1");
    reg.register("t1", { type: "render" }); // attempt 2，运行中被重启打断

    const restored = new TaskRegistry({ persistencePath });
    expect(restored.query("t1")).toMatchObject({ attempt: 2, status: "recovering" });
    // 旧 attempt 回调依旧被拒。
    expect(restored.complete("t1", { late: true }, { expectedAttempt: 1 })).toBeNull();
    expect(restored.complete("t1", { ok: true }, { expectedAttempt: 2 })).toMatchObject({ status: "completed" });
    fs.rmSync(dir, { recursive: true, force: true });
  });

  it("缺失 attempt 的旧持久化记录按 attempt=1 读（兼容规则）", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-task-attempt-legacy-"));
    const persistencePath = path.join(dir, "tasks.json");
    fs.writeFileSync(persistencePath, JSON.stringify({
      tasks: [{ taskId: "subagent-legacy-1", type: "render", status: "running" }],
      schedules: [],
    }), "utf8");
    const restored = new TaskRegistry({ persistencePath });
    // 旧格式 taskId 原值保留，不迁移不重铸。
    expect(restored.query("subagent-legacy-1")).toMatchObject({ attempt: 1, status: "recovering" });
    fs.rmSync(dir, { recursive: true, force: true });
  });
});

describe("task:register / task:complete 总线透传 attempt（迟到回调栅栏接线）", () => {
  it("task:register 返回含 attempt 的快照；task:complete 带 expectedAttempt 时被栅栏", () => {
    const handlers = new Map();
    const eventBus = {
      handle: (topic, fn) => handlers.set(topic, fn),
      request: (topic, payload) => handlers.get(topic)?.(payload),
    };
    const registry = new TaskRegistry();
    registry.registerHandler("render", { abort: vi.fn() });
    registerTaskRegistryBusHandlers(eventBus, registry);

    const first = eventBus.request("task:register", { taskId: mintTaskId("render"), type: "render" });
    expect(first.ok).toBe(true);
    expect(first.task.attempt).toBe(1);

    expect(eventBus.request("task:complete", { taskId: first.task.taskId, result: "run-1" }).task.status)
      .toBe("completed");

    const second = eventBus.request("task:register", { taskId: first.task.taskId, type: "render" });
    expect(second.task.attempt).toBe(2);

    const late = eventBus.request("task:complete", { taskId: first.task.taskId, result: "late", expectedAttempt: 1 });
    expect(late.ok).toBe(true);
    expect(late.task).toBeNull();
    expect(registry.query(first.task.taskId)).toMatchObject({ attempt: 2, status: "running" });

    const fresh = eventBus.request("task:complete", { taskId: first.task.taskId, result: "run-2", expectedAttempt: 2 });
    expect(fresh.task).toMatchObject({ status: "completed", result: "run-2" });
  });
});
