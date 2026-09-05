import fs from "node:fs";
import vm from "node:vm";
import path from "node:path";
import assert from "node:assert/strict";
import { describe, expect, it, vi } from "vitest";

const source = fs.readFileSync("scripts/smoke-packaged-desktop.mjs", "utf8");
const stopSource = source.slice(source.indexOf("async function stopPid("), source.indexOf("\ntry {\n  for (;;)"));

function cleanupHarness(platform: string, treeKillFails = false) {
  // 模拟 Windows：只结束主进程时，子进程仍持有临时资料文件。
  const live = new Set([101, 102]);
  const kill = vi.fn((pid: number, signal: string | number) => {
    if (!live.has(pid)) throw Object.assign(new Error("进程已退出"), { code: "ESRCH" });
    if (signal !== 0) live.delete(pid);
  });
  const execFileSync = vi.fn(() => {
    if (treeKillFails) throw new Error("进程树清理失败");
    live.clear();
  });
  const stop = vm.runInNewContext(`${stopSource}\nstopPid`, {
    platform, process: { kill }, execFileSync, sleep: async () => {},
  }) as (pid: number | undefined) => Promise<void>;
  return { live, kill, execFileSync, stop };
}

describe("打包桌面测试清理", () => {
  it("Windows 在主进程退出前结束本次进程树，释放子进程文件锁", async () => {
    const h = cleanupHarness("win32");
    await h.stop(101);
    expect(h.live.size).toBe(0);
    expect(h.execFileSync).toHaveBeenCalledWith("taskkill", ["/PID", "101", "/T", "/F"], {
      stdio: "pipe", windowsHide: true, timeout: 15_000,
    });
    expect(h.kill.mock.calls.every(([, signal]) => signal === 0)).toBe(true);
  });

  it("Windows 结束失败且进程仍在时必须报错，不能报告清理成功", async () => {
    const h = cleanupHarness("win32", true);
    await expect(h.stop(101)).rejects.toThrow("进程树清理失败");
    expect(h.live.has(102)).toBe(true);
  });

  it("无效编号和已退出的进程不执行进程树命令", async () => {
    const h = cleanupHarness("win32");
    for (const pid of [undefined, 0, -1, 1.5, 999]) await h.stop(pid);
    expect(h.execFileSync).not.toHaveBeenCalled();
    expect(h.live.size).toBe(2);
  });

  it("其他平台仍按原有信号流程退出目标进程", async () => {
    const h = cleanupHarness("darwin");
    await h.stop(101);
    expect(h.live.has(101)).toBe(false);
    expect(h.execFileSync).not.toHaveBeenCalled();
    expect(h.kill).toHaveBeenCalledWith(101, "SIGTERM");
  });
});

describe("打包桌面就绪探测", () => {
  const inspectSource = source.slice(source.indexOf("async function inspectRenderer()"), source.indexOf("\nasync function stopPid("));
  const loopSource = source.slice(source.indexOf("  for (;;) {"), source.indexOf('  report.status = "passed";'));

  it("调试列表尚未响应时记录本次探测超时，下轮继续读取", async () => {
    const report = { rendererProbeTimeouts: 0 };
    const fetch = vi.fn().mockRejectedValueOnce(new DOMException("探测未就绪", "TimeoutError"))
      .mockResolvedValueOnce({ json: async () => [] });
    const inspect = vm.runInNewContext(`${inspectSource}\ninspectRenderer`, {
      fs: { existsSync: () => true, readFileSync: () => "1234\n" }, path,
      userData: "/tmp/owned-profile", fetch, AbortSignal, report,
    }) as () => Promise<unknown>;
    await expect(inspect()).resolves.toBeNull();
    expect(report.rendererProbeTimeouts).toBe(1);
    await expect(inspect()).resolves.toBeNull();
    expect(fetch).toHaveBeenCalledTimes(2);
  });

  it("非法调试响应仍然报错，不能当作未就绪吞掉", async () => {
    const inspect = vm.runInNewContext(`${inspectSource}\ninspectRenderer`, {
      fs: { existsSync: () => true, readFileSync: () => "1234\n" }, path,
      userData: "/tmp/owned-profile", AbortSignal, report: {},
      fetch: async () => ({ json: async () => { throw new SyntaxError("非法响应"); } }),
    }) as () => Promise<unknown>;
    await expect(inspect()).rejects.toThrow("非法响应");
  });

  function runLoop(options: { neverReady?: boolean; crash?: boolean } = {}) {
    let elapsed = options.neverReady ? 90_001 : 0;
    const report = { packaged: false, rendererReady: false, serverReady: false };
    const inspectRenderer = vi.fn().mockResolvedValueOnce(null).mockResolvedValue({ ready: "complete", textLength: 93, controls: 7 });
    const promise = vm.runInNewContext(`(async () => {${loopSource}})()`, {
      spawnError: undefined, child: { exitCode: null, signalCode: null }, stderr: "", assert, report,
      launchEvents: () => options.crash ? [{ event: "render-process-gone" }] : [{ event: "desktop-launch-start", details: { packaged: true } }],
      inspectRenderer, path, home: "/tmp/owned-profile", AbortSignal,
      readJson: () => ({ port: 1234, token: "test-only" }),
      fetch: async () => ({ status: 200, json: async () => ({ notebooks: [] }) }),
      performance: { now: () => elapsed }, start: 0, sleep: async (ms: number) => { elapsed += ms; },
    }) as Promise<void>;
    return { promise, report, inspectRenderer };
  }

  it("短暂未就绪后仍须实际页面和后台都成功才能通过", async () => {
    const h = runLoop();
    await h.promise;
    expect(h.inspectRenderer).toHaveBeenCalledTimes(2);
    expect(h.report).toMatchObject({ packaged: true, rendererReady: true, serverReady: true });
  });

  it("持续未就绪仍在原有 90 秒期限失败", async () => {
    await expect(runLoop({ neverReady: true }).promise).rejects.toThrow("Packaged desktop did not become ready");
  });

  it("探测未就绪时真实渲染崩溃仍立即失败", async () => {
    const h = runLoop({ crash: true });
    await expect(h.promise).rejects.toThrow("Packaged desktop startup reported a failed renderer phase");
    expect(h.inspectRenderer).toHaveBeenCalledTimes(1);
  });
});
