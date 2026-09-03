import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { describe, expect, it, vi } from "vitest";
import { backendCandidates } from "../lib/local-models/backend-probe.ts";
import { createNvidiaGpuDetector } from "../lib/local-models/gpu-detect.ts";
import { LocalModelsSubsystem } from "../lib/local-models/subsystem.ts";

function okExec(): () => Promise<unknown> {
  return vi.fn(async () => ({ stdout: "NVIDIA GeForce RTX 4070", stderr: "" }));
}

function failingExec(): () => Promise<unknown> {
  return vi.fn(async () => {
    throw new Error("spawn nvidia-smi ENOENT");
  });
}

describe("backendCandidates", () => {
  it("prefers cuda on Windows only when an NVIDIA GPU was detected", () => {
    expect(backendCandidates("win32", "x64", true)).toEqual(["cuda", "vulkan", "directml", "cpu"]);
    expect(backendCandidates("win32", "x64", false)).toEqual(["directml", "vulkan", "cpu"]);
  });

  it("keeps platform chains independent of the NVIDIA flag elsewhere", () => {
    expect(backendCandidates("darwin", "arm64", false)).toEqual(["metal", "coreml", "cpu"]);
    expect(backendCandidates("darwin", "x64", true)).toEqual(["metal", "cpu"]);
    expect(backendCandidates("linux", "x64", false)).toEqual(["cuda", "vulkan", "cpu"]);
    expect(backendCandidates("sunos", "x64", true)).toEqual(["cpu"]);
  });
});

describe("createNvidiaGpuDetector", () => {
  it("short-circuits on darwin without spawning a process", async () => {
    const exec = okExec();
    const detector = createNvidiaGpuDetector({ platform: "darwin", runNvidiaSmi: exec });
    await expect(detector()).resolves.toEqual({ hasNvidiaGpu: false, source: "darwin-unsupported" });
    expect(exec).not.toHaveBeenCalled();
  });

  it("finds nvidia-smi on a known Windows path before falling back to PATH", async () => {
    const exec = okExec();
    const fileExists = vi.fn(async (candidate: string) => candidate.endsWith("System32\\nvidia-smi.exe"));
    const detector = createNvidiaGpuDetector({ platform: "win32", runNvidiaSmi: exec, fileExists });
    await expect(detector()).resolves.toEqual({ hasNvidiaGpu: true, source: "nvidia-smi-path" });
    expect(fileExists).toHaveBeenCalledTimes(1);
    expect(exec).not.toHaveBeenCalled();
  });

  it("falls back to running nvidia-smi from PATH on Windows", async () => {
    const exec = okExec();
    const detector = createNvidiaGpuDetector({ platform: "win32", runNvidiaSmi: exec, fileExists: async () => false });
    await expect(detector()).resolves.toEqual({ hasNvidiaGpu: true, source: "nvidia-smi" });
    expect(exec).toHaveBeenCalledTimes(1);
  });

  it("detects the NVIDIA driver through procfs on Linux before running nvidia-smi", async () => {
    const exec = okExec();
    const detector = createNvidiaGpuDetector({
      platform: "linux",
      runNvidiaSmi: exec,
      directoryExists: async () => true,
    });
    await expect(detector()).resolves.toEqual({ hasNvidiaGpu: true, source: "procfs" });
    expect(exec).not.toHaveBeenCalled();
  });

  it("degrades explicitly to not-detected when every probe misses or fails", async () => {
    const detector = createNvidiaGpuDetector({
      platform: "win32",
      runNvidiaSmi: failingExec(),
      fileExists: async () => false,
    });
    await expect(detector()).resolves.toEqual({ hasNvidiaGpu: false, source: "not-detected" });
    const linuxDetector = createNvidiaGpuDetector({
      platform: "linux",
      runNvidiaSmi: failingExec(),
      directoryExists: async () => false,
    });
    await expect(linuxDetector()).resolves.toEqual({ hasNvidiaGpu: false, source: "not-detected" });
  });

  it("memoizes the first successful probe", async () => {
    const exec = okExec();
    const detector = createNvidiaGpuDetector({ platform: "linux", runNvidiaSmi: exec, directoryExists: async () => false });
    await detector();
    await detector();
    expect(exec).toHaveBeenCalledTimes(1);
  });
});

describe("LocalModelsSubsystem GPU wiring", () => {
  it("passes detected NVIDIA availability to factories and reports it in state", async () => {
    const home = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-local-model-gpu-"));
    try {
      const subsystem = new LocalModelsSubsystem({
        lingxiHome: home,
        getPreferences: () => ({}),
        savePreferences: () => {},
        detectNvidiaGpu: async () => ({ hasNvidiaGpu: true, source: "nvidia-smi" }),
      });
      const state = await subsystem.state();
      expect(state.gpu).toEqual({ hasNvidiaGpu: true, source: "nvidia-smi" });
      await subsystem.dispose();
    } finally {
      fs.rmSync(home, { recursive: true, force: true });
    }
  });
});
