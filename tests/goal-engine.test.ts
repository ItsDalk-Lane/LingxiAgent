/**
 * goal 预算引擎测试：状态机全链、超支恰好一次提醒、中止自动暂停、
 * pause 时长不计、侧车持久化恢复、工具动作与权限契约。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { createGoalEngine, goalActiveMs, goalSidecarPath } from "../lib/goal/goal-engine.ts";
import { createGoalTool } from "../lib/tools/goal-tool.ts";
import { registerGoalHandler } from "../server/goal-handler.ts";

const roots: string[] = [];
function freshSession() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-goal-"));
  roots.push(root);
  return path.join(root, "s.jsonl");
}
afterEach(() => { while (roots.length) fs.rmSync(roots.pop()!, { recursive: true, force: true }); });

describe("goal-engine", () => {
  it("create → 记账 → status → complete 全链", () => {
    const sp = freshSession();
    const notified: any[] = [];
    let clock = 1_000_000;
    const engine = createGoalEngine({ notifyOverBudget: (...a) => notified.push(a), now: () => clock });
    const created = engine.create(sp, { name: "重构", tokenBudget: 1000, timeBudgetMs: null });
    expect(created.ok).toBe(true);

    engine.recordTokenUsage(sp, 400);
    engine.recordTokenUsage(sp, 300);
    const { text, goal } = engine.status(sp);
    expect(text).toContain('goal "重构" — active');
    expect(goal?.tokensUsed).toBe(700);
    expect(text).toContain("700 / 1000");

    expect(engine.complete(sp)).toBe(true);
    expect(engine.status(sp).goal?.status).toBe("completed");
  });

  it("超支提醒恰好一次；继续记账不再重复提醒", () => {
    const sp = freshSession();
    const notified: any[] = [];
    const engine = createGoalEngine({ notifyOverBudget: (...a) => notified.push(a), now: () => 1_000_000 });
    engine.create(sp, { tokenBudget: 1000 });
    const r1 = engine.recordTokenUsage(sp, 1500);
    expect(r1?.newlyOver).toBe(true);
    expect(notified).toHaveLength(1);
    expect(notified[0][2]).toBe("tokens");
    engine.recordTokenUsage(sp, 500);
    expect(notified).toHaveLength(1); // 不重复
  });

  it("时间预算：pause 不计时；tickTimeBudget 越限提醒一次", () => {
    const sp = freshSession();
    const notified: any[] = [];
    let clock = 1_000_000;
    const engine = createGoalEngine({ notifyOverBudget: (...a) => notified.push(a), now: () => clock });
    engine.create(sp, { timeBudgetMs: 60_000 });
    clock += 30_000;
    expect(engine.pause(sp, "manual")).toBe(true);
    clock += 10 * 60_000; // 暂停中很长
    expect(engine.resume(sp)).toBe(true);
    expect(goalActiveMs(engine.status(sp).goal!, clock)).toBe(30_000); // 暂停不计
    clock += 40_000; // 活动累计 70s > 60s
    expect(engine.tickTimeBudget(sp)).toBe(true);
    expect(notified).toHaveLength(1);
    expect(notified[0][2]).toBe("time");
    expect(engine.tickTimeBudget(sp)).toBe(false); // 不重复
  });

  it("用户中止 → 自动暂停（reason=user_abort）", () => {
    const sp = freshSession();
    const engine = createGoalEngine({ notifyOverBudget: () => {}, now: () => 1_000_000 });
    engine.create(sp, { tokenBudget: 1000 });
    expect(engine.pauseOnAbort(sp)).toBe(true);
    expect(engine.status(sp).goal?.status).toBe("paused");
    expect(engine.status(sp).goal?.pausedReason).toBe("user_abort");
    expect(engine.pauseOnAbort(sp)).toBe(false); // 已暂停不重复
  });

  it("同时只允许一个 active；完成/丢弃后可再建", () => {
    const sp = freshSession();
    const engine = createGoalEngine({ notifyOverBudget: () => {}, now: () => 1_000_000 });
    engine.create(sp, { name: "a", tokenBudget: 100 });
    const dup = engine.create(sp, { name: "b", tokenBudget: 100 });
    expect(dup.ok).toBe(false);
    expect(dup.error).toContain("already active");
    engine.drop(sp);
    const again = engine.create(sp, { name: "b", tokenBudget: 100 });
    expect(again.ok).toBe(true);
  });

  it("至少一项预算；侧车持久化重启恢复", () => {
    const sp = freshSession();
    const engine = createGoalEngine({ notifyOverBudget: () => {}, now: () => 1_000_000 });
    expect(engine.create(sp, {}).ok).toBe(false);
    engine.create(sp, { name: "持久", tokenBudget: 5000 });
    engine.recordTokenUsage(sp, 1234);
    // 「重启」：新实例（新闭包）读同一侧车
    const engine2 = createGoalEngine({ notifyOverBudget: () => {}, now: () => 1_000_000 });
    expect(engine2.status(sp).goal?.tokensUsed).toBe(1234);
    expect(fs.existsSync(goalSidecarPath(sp))).toBe(true);
  });
});

describe("goal 工具", () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { while (roots.length) fs.rmSync(roots.pop()!, { recursive: true, force: true }); });

  function makeTool(sp: string, defaults: any = { tokenBudget: null, timeBudgetMs: null }) {
    const engine = createGoalEngine({ notifyOverBudget: () => {}, now: () => 1_000_000 });
    return createGoalTool({
      getSessionPath: () => sp,
      getGoalEngine: () => engine,
      getDefaultBudgets: () => defaults,
    });
  }

  it("create 用显式预算；省略时用全局默认", async () => {
    const sp = freshSession();
    const t1 = makeTool(sp);
    const r1: any = await t1.execute("c1", { action: "create", name: "x", token_budget: 2000 });
    expect(r1.content[0].text).toContain("2000 tokens");
    const t2 = makeTool(path.join(path.dirname(sp), "s2.jsonl"), { tokenBudget: 7777, timeBudgetMs: null });
    fs.writeFileSync(path.join(path.dirname(sp), "s2.jsonl"), "h\n");
    const r2: any = await t2.execute("c2", { action: "create", name: "y" });
    expect(r2.content[0].text).toContain("7777 tokens");
  });

  it("status/pause/resume/drop 动作流转", async () => {
    const sp = freshSession();
    const tool = makeTool(sp);
    await tool.execute("c1", { action: "create", name: "z", token_budget: 1000 });
    const paused: any = await tool.execute("c2", { action: "pause" });
    expect(paused.content[0].text).toContain("paused");
    const resumed: any = await tool.execute("c3", { action: "resume" });
    expect(resumed.content[0].text).toContain("resumed");
    const status: any = await tool.execute("c4", { action: "status" });
    expect(status.content[0].text).toContain("active");
    const dropped: any = await tool.execute("c5", { action: "drop" });
    expect(dropped.content[0].text).toContain("dropped");
  });

  it("权限契约：status=read；其余=write", () => {
    const tool = makeTool("/tmp/s.jsonl");
    expect(tool.sessionPermission.resolveInvocation({ action: "status" }).kind).toBe("read");
    expect(tool.sessionPermission.resolveInvocation(undefined).kind).toBe("read");
    expect(tool.sessionPermission.resolveInvocation({ action: "create" }).kind).toBe("write");
    expect(tool.sessionPermission.resolveInvocation({ action: "pause" }).kind).toBe("write");
  });
});

describe("goal-handler 总线", () => {
  it("token_usage 记账、turn_start 懒检查、aborted 自动暂停", () => {
    const listeners: Array<(e: any, sp?: string) => void> = [];
    const bus = { subscribe: (fn: any) => { listeners.push(fn); return () => {}; } };
    const sp = freshSession();
    let clock = 1_000_000;
    const engine = createGoalEngine({ notifyOverBudget: () => {}, now: () => clock });
    registerGoalHandler(bus as any, { goalEngine: engine, isEnabled: () => true });
    const emit = (e: any, s: string) => { for (const fn of listeners) fn(e, s); };

    engine.create(sp, { name: "总线", tokenBudget: 1000, timeBudgetMs: 60_000 });
    emit({ type: "token_usage", usage: { totalTokens: 250 } }, sp);
    expect(engine.status(sp).goal?.tokensUsed).toBe(250);

    emit({ type: "turn_start" }, sp);
    clock += 120_000;
    emit({ type: "turn_start" }, sp); // 时间越限 → 提醒一次（notify 空实现，不炸即可）

    emit({ type: "turn_end", aborted: true }, sp);
    expect(engine.status(sp).goal?.status).toBe("paused");
  });

  it("总开关关闭：不记账", () => {
    const listeners: Array<(e: any, sp?: string) => void> = [];
    const bus = { subscribe: (fn: any) => { listeners.push(fn); return () => {}; } };
    const sp = freshSession();
    const engine = createGoalEngine({ notifyOverBudget: () => {}, now: () => 1_000_000 });
    registerGoalHandler(bus as any, { goalEngine: engine, isEnabled: () => false });
    engine.create(sp, { tokenBudget: 1000 });
    for (const fn of listeners) fn({ type: "token_usage", usage: { totalTokens: 250 } }, sp);
    expect(engine.status(sp).goal?.tokensUsed).toBe(0);
  });
});
