/**
 * P02-T05 崩溃窗口与最小持久化证据（A09/A10/A11）：
 *  - A09 写后结果未存崩溃：真实子进程执行副作用后、终态落盘前退出（exit 70）；
 *    用同一持久化文件重建注册表（重启）→ 不自动重复写（副作用计数仍为 1）、
 *    任务进入可解释的 recovering（结果未知、可核对、可人工收口）。
 *  - A10 只读恢复：崩溃后关联保留、无伪造的已完成副作用；合法续执行同
 *    attempt 收口（recovering 属活跃态，register 不换代）。
 *  - A11 持久化失败：隔离目录注入磁盘写失败 → durable 交接不返回持久成功，
 *    原始错误与未保存状态可见；修复磁盘后原样重试成功（不重新生成结果）。
 *
 * 被验收对象全部真实（TaskRegistry / DeferredResultStore / 总线 handlers /
 * 真实子进程），只注入磁盘故障与进程退出。
 */

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it, vi } from "vitest";
import { TaskRegistry } from "../lib/task-registry.ts";
import { DeferredResultStore } from "../lib/deferred-result-store.ts";
import { registerDeferredResultBusHandlers } from "../server/deferred-result-bus-handlers.ts";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const CRASH_PROBE = path.join(REPO_ROOT, "artifacts", "refactor-2026", "P02", "tools", "crash-probe-child.mjs");

function sideEffectCount(file) {
  if (!fs.existsSync(file)) return 0;
  return fs.readFileSync(file, "utf8").split("\n").filter((line) => line.startsWith("side-effect:")).length;
}

describe("A09 写后结果未存崩溃（真实子进程退出 + 重启恢复）", () => {
  it("重启后不自动重复写、状态可解释未知、attempt 保留", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-p02-crash-"));
    const persistencePath = path.join(dir, "plugin-tasks.json");
    const sideEffectPath = path.join(dir, "side-effects.txt");

    // 子进程：register（已持久化接受）→ 副作用写 → 终态落盘前退出。
    const child = spawnSync(process.execPath, [CRASH_PROBE, persistencePath, sideEffectPath], {
      encoding: "utf8",
    });
    expect(child.status).toBe(70);
    expect(sideEffectCount(sideEffectPath)).toBe(1);
    expect(fs.existsSync(persistencePath)).toBe(true);

    // 重启：同一文件重建注册表。未闭合记录 → recovering；没有 handler 自动重放。
    const restarted = new TaskRegistry({ persistencePath });
    const task = restarted.query("task_probe_crash_1");
    expect(task).toMatchObject({ status: "recovering", attempt: 1 });
    // 结果未知：无 result、无 error（键不存在），可核对。
    expect(task).not.toHaveProperty("result");
    expect(task).not.toHaveProperty("error");

    // 恢复扫描与查询都不产生新的副作用写。
    restarted.listAll({});
    restarted.query("task_probe_crash_1");
    expect(sideEffectCount(sideEffectPath)).toBe(1);

    // 结果未知可核对并可人工收口：操作员以原 attempt 落终态（不重发外部动作）。
    const settled = restarted.complete("task_probe_crash_1", { outcome: "unknown-manually-checked" }, { expectedAttempt: 1 });
    expect(settled).toMatchObject({ status: "completed" });

    fs.rmSync(dir, { recursive: true, force: true });
  }, 30000);
});

describe("A10 只读恢复（可安全重试的中途退出）", () => {
  it("崩溃后关联（父会话/类型）保留，恢复不伪造已完成副作用；同 attempt 合法续执行", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-p02-readrecover-"));
    const persistencePath = path.join(dir, "plugin-tasks.json");
    const registry = new TaskRegistry({ persistencePath });
    registry.registerHandler("readonly-probe", { abort: () => {} });
    registry.register("task_read_probe", {
      type: "readonly-probe",
      parentSessionPath: "/tmp/p02-parent.jsonl",
      parentSessionId: "sess_p02parent",
    });

    // 模拟进程退出：用同一持久化文件重建（真实重启等价）。
    const restarted = new TaskRegistry({ persistencePath });
    const task = restarted.query("task_read_probe");
    expect(task).toMatchObject({
      status: "recovering",
      parentSessionId: "sess_p02parent",
      parentSessionPath: "/tmp/p02-parent.jsonl",
    });
    expect(task).not.toHaveProperty("result");

    // 续执行按既定策略：register 不换代（recovering 属活跃态），完成后同 attempt 收口。
    const resumed = restarted.register("task_read_probe", { type: "readonly-probe" });
    expect(resumed.attempt).toBe(1);
    const done = restarted.complete("task_read_probe", { rows: 3 }, { expectedAttempt: 1 });
    expect(done).toMatchObject({ status: "completed", result: { rows: 3 } });

    fs.rmSync(dir, { recursive: true, force: true });
  });
});

describe("A11 持久化失败（隔离磁盘注入失败，不返回持久成功）", () => {
  it("deferred durable 交接在 flush 失败时显式 ok:false，状态未保存可见；修复后原样重试成功", async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-p02-durable-"));
    const persistPath = path.join(dir, "blocked-dir", "deferred.json");
    // 注入磁盘故障：目标目录路径是一个普通文件 → mkdir/write 必失败。
    fs.writeFileSync(path.join(dir, "blocked-dir"), "not a directory", "utf8");

    const store = new DeferredResultStore(null, persistPath);
    const handlers = new Map();
    const eventBus = {
      handle: (topic, fn) => handlers.set(topic, fn),
      request: (topic, payload) => handlers.get(topic)?.(payload),
    };
    registerDeferredResultBusHandlers(eventBus, store);

    const registerReceipt = eventBus.request("deferred:register", {
      taskId: "task_durable_probe",
      sessionPath: "/tmp/p02-durable-session.jsonl",
      meta: { type: "probe" },
      durable: true,
    });
    // register 阶段目录尚不可写 → 不给持久成功回执。
    expect(registerReceipt).toMatchObject({ ok: false, durable: false });

    const resolveReceipt = eventBus.request("deferred:resolve", {
      taskId: "task_durable_probe",
      sessionPath: "/tmp/p02-durable-session.jsonl",
      result: { ok: true },
      durable: true,
    });
    expect(resolveReceipt).toMatchObject({ ok: false, durable: false, error: "deferred result persistence failed" });
    // 未保存状态可见：内存已 resolve（回执明确否认持久成功），上游据此重试交接。
    expect(store.query("task_durable_probe")).toMatchObject({ status: "resolved", result: { ok: true } });
    expect(fs.existsSync(persistPath)).toBe(false);

    // 修复磁盘后同一状态原样重试成功（不需要重新生成结果）。
    fs.rmSync(path.join(dir, "blocked-dir"));
    const retryReceipt = eventBus.request("deferred:resolve", {
      taskId: "task_durable_probe",
      sessionPath: "/tmp/p02-durable-session.jsonl",
      result: { ok: true },
      durable: true,
    });
    expect(retryReceipt).toMatchObject({ ok: true, durable: true });

    fs.rmSync(dir, { recursive: true, force: true });
  });
});
