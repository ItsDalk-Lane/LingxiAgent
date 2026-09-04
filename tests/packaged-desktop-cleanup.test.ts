import fs from "node:fs";
import vm from "node:vm";
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
