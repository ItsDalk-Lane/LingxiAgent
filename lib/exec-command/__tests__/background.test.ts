/**
 * exec_command 长任务自动转后台（阶段二·12）测试：
 * waitForTtyWindow 窗口内完成/到窗口仍在跑两态；registerBackgroundExec
 * 登记 deferred+registry、退出后 resolve（触发回送续跑的链头）；
 * 工具 auto 分支的同步返回与转后台返回形状。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import {
  waitForTtyWindow,
  registerBackgroundExec,
  normalizeTtyOutput,
} from "../background.ts";
import { createExecCommandTools } from "../tool.ts";

const tempDirs: string[] = [];
function makeTempDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-execbg-"));
  tempDirs.push(dir);
  return dir;
}
afterEach(() => { for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true }); });

function makeManager(entries: Array<Record<string, any>>) {
  return {
    list: vi.fn(() => ({ sessionPath: "/tmp/s.jsonl", terminals: entries })),
    readTail: vi.fn(() => ({ output: "line1\r\nline2\r\n" })),
    close: vi.fn(),
  } as any;
}

describe("waitForTtyWindow", () => {
  beforeEach(() => { vi.clearAllMocks(); });

  it("窗口内退出：带 exitCode 与全部输出返回 finished", async () => {
    const manager = makeManager([{ terminalId: "t1", status: "exited", exitCode: 0 }]);
    const outcome = await waitForTtyWindow(manager, { sessionPath: "/tmp/s.jsonl", terminalId: "t1", windowMs: 2000 });
    expect(outcome.finished).toBe(true);
    expect(outcome.exitCode).toBe(0);
    expect(outcome.output).toContain("line1");
  });

  it("到窗口仍在跑：finished=false 带尾部输出", async () => {
    const manager = makeManager([{ terminalId: "t1", status: "running" }]);
    const outcome = await waitForTtyWindow(manager, { sessionPath: "/tmp/s.jsonl", terminalId: "t1", windowMs: 1200 });
    expect(outcome.finished).toBe(false);
    expect(outcome.output).toContain("line2");
  });

  it("会话消失（被清理）：按仍在跑收敛不抛", async () => {
    const manager = makeManager([]);
    const outcome = await waitForTtyWindow(manager, { sessionPath: "/tmp/s.jsonl", terminalId: "t1", windowMs: 1200 });
    expect(outcome.finished).toBe(false);
  });
});

describe("registerBackgroundExec", () => {
  beforeEach(() => { vi.clearAllMocks(); });

  function makeStores() {
    const resolved: any[] = [];
    const deferredStore = {
      defer: vi.fn(),
      resolve: vi.fn((id: string, result: any) => resolved.push({ id, result })),
    };
    const handlers: Record<string, any> = {};
    const taskRegistry = {
      registerHandler: vi.fn((type: string, handler: any) => { handlers[type] = handler; }),
      register: vi.fn(),
      complete: vi.fn(),
    };
    return { deferredStore, taskRegistry, handlers, resolved };
  }

  it("登记两套账本；进程退出后 deferred resolve（trigger 回送链头）", async () => {
    const { deferredStore, taskRegistry, resolved } = makeStores();
    const entries: Array<Record<string, any>> = [{ terminalId: "t1", status: "running" }];
    const manager = makeManager(entries);
    const reg = registerBackgroundExec(
      { manager, deferredStore, taskRegistry },
      { terminalId: "t1", sessionPath: "/tmp/s.jsonl", agentId: "a1", command: "npm test" },
    );
    expect(reg.registered).toBe(true);
    expect(deferredStore.defer).toHaveBeenCalledWith("t1", "/tmp/s.jsonl", expect.objectContaining({ type: "exec_command_background", deliveryIntent: "trigger_parent_turn" }));
    expect(taskRegistry.register).toHaveBeenCalledWith("t1", expect.objectContaining({ type: "exec_command_background" }));

    await new Promise((r) => setTimeout(r, 1300)); // 轮询周期 1s
    entries[0] = { terminalId: "t1", status: "exited", exitCode: 3 };
    await new Promise((r) => setTimeout(r, 1300));
    expect(resolved).toHaveLength(1);
    expect(resolved[0].result).toMatchObject({ exitCode: 3, ok: false, command: "npm test" });
    expect(resolved[0].result.output).toContain("line1");
  });

  it("缺 deferred store：registered=false 附原因", () => {
    const reg = registerBackgroundExec(
      { manager: makeManager([]), deferredStore: null, taskRegistry: null },
      { terminalId: "t1", sessionPath: "/tmp/s.jsonl", agentId: null, command: "x" },
    );
    expect(reg.registered).toBe(false);
    expect(reg.reason).toContain("unavailable");
  });
});

describe("exec_command wait_mode=auto 工具分支", () => {
  beforeEach(() => { vi.clearAllMocks(); });

  function makeTools(manager: any) {
    return createExecCommandTools({
      bashTool: null,
      escalatedBashTool: null,
      commandExec: null,
      escalatedCommandExec: null,
      getTerminalSessionManager: () => manager,
      getDeferredStore: () => ({ defer: vi.fn(), resolve: vi.fn() }),
      getTaskRegistry: () => ({ registerHandler: vi.fn(), register: vi.fn(), complete: vi.fn() }),
      getAgentId: () => "a1",
      getCwd: () => makeTempDir(),
      isOneShotSandboxEnforced: () => false,
      platform: process.platform,
      detectPowerShellFlavor: undefined,
      getToolSessionPath: undefined as any,
    } as any);
  }

  function makeCtx(sessionPath: string) {
    return {
      sessionManager: { getCwd: () => path.dirname(sessionPath), getSessionFile: () => sessionPath },
      sessionPath,
    } as any;
  }

  it("窗口内完成：同步返回输出与 exit 码", async () => {
    const dir = makeTempDir();
    const sessionPath = path.join(dir, "s.jsonl");
    const manager = {
      list: vi.fn(() => ({ sessionPath, terminals: [{ terminalId: "tty-1", status: "exited", exitCode: 0 }] })),
      readTail: vi.fn(() => ({ output: "build ok\r\n" })),
      start: vi.fn(async () => ({ terminalId: "tty-1" })),
      close: vi.fn(),
    } as any;
    const [execTool] = makeTools(manager);
    const r: any = await execTool.execute("c1", { cmd: "echo hi", wait_mode: "auto", background_after_seconds: 1 }, null, null, makeCtx(sessionPath));
    expect(r.details.backgrounded).toBe(false);
    expect(r.details.exitCode).toBe(0);
    expect(r.content[0].text).toContain("build ok");
    expect(r.content[0].text).toContain("[exit 0]");
  });

  it("到窗口仍在跑：转后台返回 task_id 与登记状态", async () => {
    const dir = makeTempDir();
    const sessionPath = path.join(dir, "s.jsonl");
    const manager = {
      list: vi.fn(() => ({ sessionPath, terminals: [{ terminalId: "tty-2", status: "running" }] })),
      readTail: vi.fn(() => ({ output: "compiling...\r\n" })),
      start: vi.fn(async () => ({ terminalId: "tty-2" })),
      close: vi.fn(),
    } as any;
    const [execTool] = makeTools(manager);
    const r: any = await execTool.execute("c1", { cmd: "npm run build", wait_mode: "auto", background_after_seconds: 1 }, null, null, makeCtx(sessionPath));
    expect(r.details.backgrounded).toBe(true);
    expect(r.details.taskId).toBe("tty-2");
    expect(r.details.deliveryRegistered).toBe(true);
    expect(r.content[0].text).toContain("task_id: tty-2");
    expect(r.content[0].text).toContain("check_pending_tasks");
  });

  it("wait 模式默认不受影响（不走 PTY）", async () => {
    const dir = makeTempDir();
    const sessionPath = path.join(dir, "s.jsonl");
    const manager = { start: vi.fn() } as any;
    const [execTool] = makeTools(manager);
    // 无 commandExec：wait 路径会走 runExecCommandOnce/bashTool 不可用 → 早退报错；
    // 这里只断言没有起 PTY。
    await execTool.execute("c1", { cmd: "echo hi" }, null, null, makeCtx(sessionPath)).catch(() => null);
    expect(manager.start).not.toHaveBeenCalled();
  });
});

describe("normalizeTtyOutput", () => {
  it("CRLF 规整为 LF", () => {
    expect(normalizeTtyOutput("a\r\nb\r\nc")).toBe("a\nb\nc");
  });
});
