/**
 * P02-T03 取消传播边界（A05/A06）：
 *  - A05 等待审批时取消：真实 ConfirmStore + 真实权限 wrapper——abortBySession
 *    使待审请求失效（promise 以 aborted 收口 → 工具执行 0 次）；审批晚到
 *    （resolve 已消失的 confirmId）不能授权旧调用。
 *  - A06 命令取消：真实子进程——spawnAndStream（沙盒 exec 的 kill 路径）取消
 *    后本任务进程树整组退出，旁边独立进程组的哨兵进程存活。
 */

import { spawn } from "node:child_process";
import { describe, expect, it, vi } from "vitest";
import { ConfirmStore } from "../lib/confirm-store.ts";
import { wrapWithSessionPermission } from "../lib/tools/session-permission-wrapper.ts";
import { spawnAndStream } from "../lib/sandbox/exec-helper.ts";

const SESSION_PATH = "/tmp/p02-cancel-session.jsonl";

function isAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (err) {
    return err?.code === "EPERM";
  }
}

async function waitFor(condition, { timeoutMs = 5000, stepMs = 50 } = {}, label = "condition") {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    if (await condition()) return;
    if (Date.now() > deadline) throw new Error(`timeout waiting for ${label}`);
    await new Promise((r) => setTimeout(r, stepMs));
  }
}

describe("A05 等待审批时取消（真实 ConfirmStore + 真实权限 wrapper）", () => {
  it("abortBySession 使待审请求失效：工具执行 0 次，结果显式 aborted", async () => {
    const confirmStore = new ConfirmStore();
    const tool = {
      name: "write",
      execute: vi.fn(async () => ({ content: [{ type: "text", text: "executed" }] })),
    };
    const [wrapped] = wrapWithSessionPermission([tool], {
      getPermissionMode: () => "ask",
      getConfirmStore: () => confirmStore,
      emitEvent: vi.fn(),
    });

    const pending = wrapped.execute("call-a5", { path: "x" }, null, null, {
      sessionManager: { getSessionFile: () => SESSION_PATH },
    });

    await waitFor(() => confirmStore.size === 1, {}, "confirmation to be pending");

    // 会话中止 → _cleanupAbortedSessionSidecars 的 confirmStore.abortBySession。
    confirmStore.abortBySession(SESSION_PATH);

    const result = await pending;
    expect(tool.execute).not.toHaveBeenCalled();
    expect(result.isError).toBe(true);
    expect(result.details.confirmed).toBe(false);
    expect(result.details.confirmation.status).toBe("aborted");
  });

  it("审批晚到不能授权旧调用：已 abort 的 confirmId 再 resolve 返回 false", async () => {
    const confirmStore = new ConfirmStore();
    const { confirmId } = confirmStore.create("tool_action_approval", { toolName: "write" }, SESSION_PATH);
    confirmStore.abortBySession(SESSION_PATH);

    expect(confirmStore.resolve(confirmId, "confirmed", undefined)).toBe(false);
    expect(confirmStore.size).toBe(0);
  });

  it("未经 abort 的正常审批仍可 confirmed（对照面：取消只失效被取消的请求）", async () => {
    const confirmStore = new ConfirmStore();
    const { confirmId, promise } = confirmStore.create("tool_action_approval", { toolName: "write" }, SESSION_PATH);
    confirmStore.resolve(confirmId, "confirmed", undefined);
    await expect(promise).resolves.toEqual({ action: "confirmed" });
  });
});

describe("A06 命令取消：本任务进程树退出、哨兵进程存活（真实子进程）", () => {
  it("abort 后整组进程树退出；独立进程组的哨兵不受影响", async () => {
    const controller = new AbortController();
    // 本任务进程树：node 子进程再 spawn 同组孙进程（不 detached → 继承 pgid）。
    const treeScript = `
      const { spawn } = require("node:child_process");
      const grandchild = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], { stdio: "ignore" });
      process.stdout.write(JSON.stringify({ grandchildPid: grandchild.pid }) + "\\n");
      setInterval(() => {}, 1000);
    `;

    let grandchildPid = null;
    const chunks = [];
    const execution = spawnAndStream(process.execPath, ["-e", treeScript], {
      cwd: process.cwd(),
      env: process.env,
      onData: (buf) => chunks.push(buf),
      signal: controller.signal,
      timeout: 0,
    }).then(
      (value) => ({ outcome: "resolved" as const, value }),
      (error) => ({ outcome: "rejected" as const, error }),
    );

    await waitFor(() => {
      const text = Buffer.concat(chunks).toString("utf8");
      const match = text.match(/"grandchildPid":(\d+)/);
      if (match) {
        grandchildPid = Number(match[1]);
        return true;
      }
      return false;
    }, {}, "grandchild pid handshake");

    // 哨兵：独立进程组（detached + unref），与本任务无关的用户侧进程替身。
    const sentinel = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], {
      detached: true,
      stdio: "ignore",
    });
    sentinel.unref();
    expect(isAlive(sentinel.pid)).toBe(true);
    expect(isAlive(grandchildPid)).toBe(true);

    controller.abort();
    const result = await execution;
    if (result.outcome !== "rejected") throw new Error("expected abort rejection");
    expect(result.error.message).toBe("aborted");

    // 进程树（直接子进程随 spawn 句柄结束 + 同组孙进程）整组退出。
    await waitFor(() => !isAlive(grandchildPid), {}, "grandchild tree exit");
    // 哨兵存活：取消不按进程名/时间窗误杀无关进程。
    expect(isAlive(sentinel.pid)).toBe(true);

    try { process.kill(-sentinel.pid, "SIGKILL"); } catch { /* 已退出则忽略 */ }
  }, 15000);

  it("超时清理走同一 kill 路径：timeout 后 reject timeout 标签", async () => {
    const result = await spawnAndStream(process.execPath, ["-e", "setInterval(() => {}, 1000)"], {
      cwd: process.cwd(),
      env: process.env,
      onData: () => {},
      signal: null,
      timeout: 1,
      timeoutErrorValue: 1,
    }).then(
      (value) => ({ outcome: "resolved" as const, value }),
      (error) => ({ outcome: "rejected" as const, error }),
    );
    if (result.outcome !== "rejected") throw new Error("expected timeout rejection");
    expect(result.error.message).toBe("timeout:1");
  }, 15000);
});
