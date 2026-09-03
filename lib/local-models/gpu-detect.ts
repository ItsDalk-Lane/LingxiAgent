import { execFile as execFileCallback } from "node:child_process";
import fsp from "node:fs/promises";
import { promisify } from "node:util";

const execFile = promisify(execFileCallback);

const NVIDIA_SMI_ARGS = ["--query-gpu=name", "--format=csv,noheader"] as const;
const NVIDIA_SMI_TIMEOUT_MS = 3_000;
const WINDOWS_NVIDIA_SMI_PATHS = [
  "C:\\Windows\\System32\\nvidia-smi.exe",
  "C:\\Program Files\\NVIDIA Corporation\\NVSMI\\nvidia-smi.exe",
] as const;
const LINUX_PROC_GPU_DIR = "/proc/driver/nvidia/gpus";

export type NvidiaGpuDetectionSource =
  | "nvidia-smi-path"
  | "nvidia-smi"
  | "procfs"
  | "darwin-unsupported"
  | "not-detected";

export interface NvidiaGpuDetection {
  hasNvidiaGpu: boolean;
  /** false 时的降级依据：平台不支持 / 未检出 / 探测失败，供 state() 观测，不静默。 */
  source: NvidiaGpuDetectionSource;
}

export type NvidiaGpuDetector = () => Promise<NvidiaGpuDetection>;

export interface NvidiaGpuDetectorDeps {
  platform?: NodeJS.Platform;
  runNvidiaSmi?: () => Promise<unknown>;
  fileExists?: (candidate: string) => Promise<boolean>;
  directoryExists?: (candidate: string) => Promise<boolean>;
  procGpuDir?: string;
}

/**
 * 探测本机是否有可用的 NVIDIA GPU（决定 auto 后端候选链是否把 cuda 排在前面）。
 * 结果按进程生命周期缓存；探测失败显式降级为「无 NVIDIA」，不影响 CPU/DirectML/Vulkan 兜底。
 */
export function createNvidiaGpuDetector(deps: NvidiaGpuDetectorDeps = {}): NvidiaGpuDetector {
  const platform = deps.platform ?? process.platform;
  const runNvidiaSmi = deps.runNvidiaSmi ?? (() =>
    execFile("nvidia-smi", [...NVIDIA_SMI_ARGS], { timeout: NVIDIA_SMI_TIMEOUT_MS }));
  const fileExists = deps.fileExists ?? isFile;
  const directoryExists = deps.directoryExists ?? isDirectory;
  const procGpuDir = deps.procGpuDir ?? LINUX_PROC_GPU_DIR;
  let cached: Promise<NvidiaGpuDetection> | null = null;
  return () => {
    if (!cached) cached = detect();
    return cached;
  };

  async function detect(): Promise<NvidiaGpuDetection> {
    if (platform === "darwin") return { hasNvidiaGpu: false, source: "darwin-unsupported" };
    if (platform === "win32") {
      for (const candidate of WINDOWS_NVIDIA_SMI_PATHS) {
        if (await fileExists(candidate)) return { hasNvidiaGpu: true, source: "nvidia-smi-path" };
      }
      return await probeByCommand();
    }
    if (platform === "linux") {
      if (await directoryExists(procGpuDir)) return { hasNvidiaGpu: true, source: "procfs" };
      return await probeByCommand();
    }
    return { hasNvidiaGpu: false, source: "not-detected" };
  }

  async function probeByCommand(): Promise<NvidiaGpuDetection> {
    try {
      await runNvidiaSmi();
      return { hasNvidiaGpu: true, source: "nvidia-smi" };
    } catch {
      return { hasNvidiaGpu: false, source: "not-detected" };
    }
  }
}

async function isFile(candidate: string): Promise<boolean> {
  return await fsp.lstat(candidate).then((stat) => stat.isFile()).catch(() => false);
}

async function isDirectory(candidate: string): Promise<boolean> {
  return await fsp.lstat(candidate).then((stat) => stat.isDirectory()).catch(() => false);
}
