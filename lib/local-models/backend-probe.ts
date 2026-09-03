import fsp from "node:fs/promises";
import path from "node:path";
import type { LocalModelBackend } from "./contracts.ts";
import { LocalModelError, throwIfAborted } from "./errors.ts";

const KNOWN_BACKENDS = new Set(["cpu", "coreml", "metal", "cuda", "vulkan", "directml"]);

export interface ProbeCache {
  read(key: string): Promise<LocalModelBackend | null>;
  write(key: string, backend: LocalModelBackend): Promise<void>;
}

/**
 * 探测结果按进程内存缓存之外再落一份磁盘：省掉每次应用重启后第一次加载
 * 多付的一轮「候选后端真实启停」。缓存只作捷径——用缓存后端启动失败时
 * 调用方必须落回完整探测并覆写缓存。
 */
export function createFileProbeCache(filePath: string): ProbeCache {
  return {
    async read(key: string): Promise<LocalModelBackend | null> {
      try {
        const raw = JSON.parse(await fsp.readFile(filePath, "utf8")) as { schemaVersion?: unknown; entries?: unknown };
        if (raw.schemaVersion !== 1 || !raw.entries || typeof raw.entries !== "object" || Array.isArray(raw.entries)) return null;
        const backend = (raw.entries as Record<string, unknown>)[key];
        return typeof backend === "string" && KNOWN_BACKENDS.has(backend) ? backend as LocalModelBackend : null;
      } catch {
        return null;
      }
    },
    async write(key: string, backend: LocalModelBackend): Promise<void> {
      let entries: Record<string, unknown> = {};
      try {
        const raw = JSON.parse(await fsp.readFile(filePath, "utf8")) as { schemaVersion?: unknown; entries?: unknown };
        if (raw.schemaVersion === 1 && raw.entries && typeof raw.entries === "object" && !Array.isArray(raw.entries)) {
          entries = raw.entries as Record<string, unknown>;
        }
      } catch { /* 首次写入或旧文件损坏：从空表重建 */ }
      entries[key] = backend;
      await fsp.mkdir(path.dirname(filePath), { recursive: true }).catch(() => {});
      await fsp.writeFile(filePath, JSON.stringify({ schemaVersion: 1, entries }, null, 2));
    },
  };
}

export interface BackendValidationResult {
  available: boolean;
  reason?: string;
  diagnostics?: Record<string, unknown>;
}

export interface BackendProbeRequest {
  platform?: NodeJS.Platform;
  arch?: string;
  hasNvidiaGpu?: boolean;
  cacheScope?: string;
  forcedBackend?: "auto" | LocalModelBackend;
  signal: AbortSignal;
  validate: (backend: LocalModelBackend, signal: AbortSignal) => Promise<BackendValidationResult>;
}

export interface BackendProbeResult {
  backend: LocalModelBackend;
  attempts: ReadonlyArray<{
    backend: LocalModelBackend;
    available: boolean;
    reason?: string;
    diagnostics?: Record<string, unknown>;
  }>;
}

export class BackendProbe {
  private readonly cache = new Map<string, BackendProbeResult>();

  async probe(request: BackendProbeRequest): Promise<BackendProbeResult> {
    const platform = request.platform ?? process.platform;
    const arch = request.arch ?? process.arch;
    const forced = request.forcedBackend ?? "auto";
    const cacheKey = `${request.cacheScope ?? "default"}:${platform}:${arch}:${request.hasNvidiaGpu === true}:${forced}`;
    const cached = this.cache.get(cacheKey);
    if (cached) return cached;
    const candidates = forced === "auto"
      ? backendCandidates(platform, arch, request.hasNvidiaGpu === true)
      : [forced];
    const attempts: BackendProbeResult["attempts"][number][] = [];
    for (const backend of candidates) {
      throwIfAborted(request.signal);
      try {
        const result = await request.validate(backend, request.signal);
        attempts.push({ backend, ...result });
        if (result.available) {
          const frozenAttempts: BackendProbeResult["attempts"] = Object.freeze(
            attempts.map((attempt) => Object.freeze({ ...attempt })),
          );
          const resolved: BackendProbeResult = Object.freeze({ backend, attempts: frozenAttempts });
          this.cache.set(cacheKey, resolved);
          return resolved;
        }
      } catch (error) {
        attempts.push({
          backend,
          available: false,
          reason: error instanceof Error ? error.message : String(error),
        });
      }
    }
    throw new LocalModelError(
      "LOCAL_MODEL_BACKEND_UNAVAILABLE",
      `no usable local inference backend for ${platform}-${arch}`,
      { platform, arch, attempts },
    );
  }

  clearCache(): void {
    this.cache.clear();
  }
}

export function backendCandidates(
  platform: NodeJS.Platform,
  arch: string,
  hasNvidiaGpu: boolean,
): LocalModelBackend[] {
  if (platform === "win32") {
    return hasNvidiaGpu
      ? ["cuda", "vulkan", "directml", "cpu"]
      : ["directml", "vulkan", "cpu"];
  }
  if (platform === "darwin" && arch === "arm64") return ["metal", "coreml", "cpu"];
  if (platform === "darwin") return ["metal", "cpu"];
  if (platform === "linux") return ["cuda", "vulkan", "cpu"];
  return ["cpu"];
}
