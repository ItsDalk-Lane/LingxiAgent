import { createHash } from "node:crypto";
import { execFile } from "node:child_process";
import fs from "node:fs";
import fsp from "node:fs/promises";
import path from "node:path";
import { BackendProbe, type ProbeCache } from "./backend-probe.ts";
import type {
  LocalEmbedRequest,
  LocalEmbeddingOutput,
  LocalModelBackend,
  LocalModelDescriptor,
  LocalOcrOutput,
  LocalOcrRequest,
  LocalSynthesisOutput,
  LocalSynthesisRequest,
  LocalTranscriptionOutput,
  LocalTranscriptionRequest,
} from "./contracts.ts";
import type { LocalModelsConfig } from "./config.ts";
import { LocalModelError, throwIfAborted } from "./errors.ts";
import type { InstalledLocalRuntimeMetadata } from "./installer.ts";
import type { LocalModelRegistryEntry } from "./registry.ts";
import type { LocalModelInstanceFactory, LocalModelLoadedInstance } from "./runtime-service.ts";
import { SidecarManager, type SidecarManagerOptions } from "./sidecar-manager.ts";

export interface SidecarFactoryOptions {
  runtimeRoot: string;
  logRoot: string;
  config: () => LocalModelsConfig;
  platform?: NodeJS.Platform;
  arch?: string;
  hasNvidiaGpu?: () => boolean | Promise<boolean>;
  backendProbe?: BackendProbe;
  probeCache?: ProbeCache;
  createManager?: (options: SidecarManagerOptions) => SidecarManager;
}

interface SidecarRuntimeInstance extends LocalModelLoadedInstance {
  manager: SidecarManager;
}

export class SidecarInstanceFactory implements LocalModelInstanceFactory {
  private readonly runtimeRoot: string;
  private readonly logRoot: string;
  private readonly config: () => LocalModelsConfig;
  private readonly platform: NodeJS.Platform;
  private readonly arch: string;
  private readonly hasNvidiaGpu: () => boolean | Promise<boolean>;
  private readonly backendProbe: BackendProbe;
  private readonly probeCache: ProbeCache | null;
  private readonly createManager: (options: SidecarManagerOptions) => SidecarManager;

  constructor(options: SidecarFactoryOptions) {
    this.runtimeRoot = path.resolve(options.runtimeRoot);
    this.logRoot = path.resolve(options.logRoot);
    this.config = options.config;
    this.platform = options.platform ?? process.platform;
    this.arch = options.arch ?? process.arch;
    this.hasNvidiaGpu = options.hasNvidiaGpu ?? (() => false);
    this.backendProbe = options.backendProbe ?? new BackendProbe();
    this.probeCache = options.probeCache ?? null;
    this.createManager = options.createManager ?? ((managerOptions) => new SidecarManager(managerOptions));
  }

  async load(
    descriptor: LocalModelDescriptor,
    installed: LocalModelRegistryEntry,
    signal: AbortSignal,
  ): Promise<LocalModelLoadedInstance> {
    const platformKey = `${this.platform}-${this.arch}`;
    const runtimeDir = path.join(this.runtimeRoot, installed.runtimeId, installed.runtimeVersion, platformKey);
    const runtime = await readInstalledRuntimeMetadata(runtimeDir, {
      id: installed.runtimeId,
      version: installed.runtimeVersion,
      platform: platformKey,
    }, signal);
    if (runtime.kind !== "sidecar" || !runtime.entrypoint) {
      throw new LocalModelError("LOCAL_MODEL_RUNTIME_MISSING", "installed runtime is not a sidecar runtime", {
        runtimeId: runtime.id,
      });
    }
    const executable = safeJoin(runtimeDir, runtime.entrypoint);
    const executableStat = await fsp.lstat(executable);
    if (!executableStat.isFile() || executableStat.isSymbolicLink()) {
      throw new LocalModelError("LOCAL_MODEL_RUNTIME_MISSING", "sidecar entrypoint is missing or unsafe");
    }
    const config = this.config();
    const hasNvidiaGpu = await this.hasNvidiaGpu();
    const probeKey = `${installed.runtimeId}@${installed.runtimeVersion}|${platformKey}|${hasNvidiaGpu}|${config.backend}`;
    let backend = this.probeCache ? await this.probeCache.read(probeKey) : null;
    if (backend) {
      // 缓存捷径：跳过候选探测直接以已验证后端启动；失败落回完整探测。
      const manager = this.buildManager(executable, runtimeDir, installed, backend, config);
      try {
        const ready = await manager.start({ signal });
        if (ready.backend === backend) return this.wrapInstance(manager, descriptor, installed, backend);
        await manager.stop().catch(() => {});
      } catch (error) {
        if (signal.aborted) throw error;
        await manager.stop().catch(() => {});
      }
      backend = null;
    }
    const probe = await this.backendProbe.probe({
      platform: this.platform,
      arch: this.arch,
      hasNvidiaGpu,
      cacheScope: `${installed.runtimeId}@${installed.runtimeVersion}`,
      forcedBackend: config.backend,
      signal,
      validate: async (backend, probeSignal) => {
        const manager = this.buildManager(executable, runtimeDir, installed, backend, config);
        try {
          const ready = await manager.start({ signal: probeSignal });
          return {
            available: ready.backend === backend,
            ...(ready.backend === backend ? {} : { reason: `runtime selected ${ready.backend}` }),
          };
        } catch (error) {
          return { available: false, reason: error instanceof Error ? error.message : String(error) };
        } finally {
          await manager.stop().catch(() => {});
        }
      },
    });
    throwIfAborted(signal);
    backend = probe.backend;
    const manager = this.buildManager(executable, runtimeDir, installed, backend, config);
    const ready = await manager.start({ signal });
    if (ready.backend !== backend) {
      await manager.stop().catch(() => {});
      throw new LocalModelError("LOCAL_MODEL_BACKEND_UNAVAILABLE", "sidecar backend changed after probing", {
        probed: backend,
        loaded: ready.backend,
      });
    }
    await this.probeCache?.write(probeKey, backend).catch(() => {});
    return this.wrapInstance(manager, descriptor, installed, backend);
  }

  async unload(
    instance: LocalModelLoadedInstance,
    _descriptor: LocalModelDescriptor,
    _signal: AbortSignal,
  ): Promise<void> {
    const sidecar = instance as SidecarRuntimeInstance;
    if (!sidecar.manager) throw new LocalModelError("LOCAL_MODEL_SIDECAR_FAILED", "sidecar instance has no manager");
    await sidecar.manager.stop();
  }

  async rssMb(instance: LocalModelLoadedInstance): Promise<number> {
    const pid = instance.diagnostics?.pid;
    if (!Number.isSafeInteger(pid) || !pid || this.platform === "win32") return Number.NaN;
    return new Promise((resolve) => {
      try {
        execFile("/bin/ps", ["-o", "rss=", "-p", String(pid)], { timeout: 1000 }, (error, stdout) => {
          const kilobytes = Number(stdout.trim());
          resolve(!error && Number.isFinite(kilobytes) && kilobytes > 0 ? kilobytes / 1024 : Number.NaN);
        });
      } catch {
        // 沙箱可能同步拒绝系统监测命令；缺测不能使已完成的推理变成失败。
        resolve(Number.NaN);
      }
    });
  }

  private buildManager(
    executable: string,
    runtimeDir: string,
    installed: LocalModelRegistryEntry,
    backend: LocalModelBackend,
    config: LocalModelsConfig,
  ): SidecarManager {
    const args = expandRuntimeArgs(installed.runtimeArgs, {
      modelDir: installed.directory,
      backend,
      threads: config.threads === "auto" ? "auto" : String(config.threads),
      mmap: config.useMmap ? "true" : "false",
      mlock: config.mlock ? "true" : "false",
    });
    return this.createManager({
      executable,
      args,
      cwd: runtimeDir,
      runtimeId: installed.runtimeId,
      runtimeVersion: installed.runtimeVersion,
      // 首次系统校验和 GPU 初始化可能超过普通请求的加载时间，但等待必须有界。
      startupTimeoutMs: 60_000,
      terminateOnRequestFailure: installed.category === "stt" || installed.category === "tts",
      ...(installed.category === "tts" ? { maxResponseBytes: 64 * 1024 * 1024 } : {}),
      logDir: path.join(this.logRoot, `${installed.id}@${installed.quant}`),
      environment: {
        LINGXI_LOCAL_MODEL_BACKEND: backend,
        LINGXI_LOCAL_MODEL_DIRECTORY: installed.directory,
      },
      platform: this.platform,
    });
  }

  private wrapInstance(
    manager: SidecarManager,
    descriptor: LocalModelDescriptor,
    installed: LocalModelRegistryEntry,
    backend: LocalModelBackend,
  ): SidecarRuntimeInstance {
    const protocolId = `local-sidecar-${descriptor.category}`;
    return {
      manager,
      backend,
      protocolId,
      get diagnostics() {
        return { pid: manager.diagnostics()?.pid, runtimeId: installed.runtimeId, runtimeVersion: installed.runtimeVersion, runtimeKind: "sidecar" };
      },
      embed: descriptor.category === "embedding"
        ? async (request) => parseEmbedding(await manager.request("embed", {
          texts: request.texts,
          inputType: request.inputType,
        }, { signal: request.signal }))
        : undefined,
      ocr: descriptor.category === "ocr"
        ? async (request) => parseOcr(await manager.request("ocr", {
          imageBase64: Buffer.from(request.image).toString("base64"),
          mime: request.mime,
          language: request.language,
        }, { signal: request.signal }))
        : undefined,
      transcribe: descriptor.category === "stt"
        ? async (request) => parseTranscription(await manager.request("transcribe", {
          filePath: request.filePath,
          mime: request.mime,
          language: request.language,
        }, { signal: request.signal }))
        : undefined,
      synthesize: descriptor.category === "tts"
        ? async (request) => {
          const output = parseSynthesis(await manager.request("synthesize", {
            text: request.text,
            voice: request.voice,
            sampleRate: request.sampleRate,
          }, { signal: request.signal }));
          await request.onChunk?.(output.audio);
          return output;
        }
        : undefined,
    };
  }
}

export function expandRuntimeArgs(
  templates: readonly string[],
  values: Record<"modelDir" | "backend" | "threads" | "mmap" | "mlock", string>,
): string[] {
  return templates.map((template) => template.replace(/\{([A-Za-z][A-Za-z0-9]*)\}/g, (_match, key: string) => {
    if (!(key in values)) {
      throw new LocalModelError("LOCAL_MODEL_MANIFEST_INVALID", `unsupported runtime argument placeholder {${key}}`);
    }
    return values[key as keyof typeof values];
  }));
}

export async function readInstalledRuntimeMetadata(
  directory: string,
  expected: { id: string; version: string; platform: string },
  signal: AbortSignal,
): Promise<InstalledLocalRuntimeMetadata> {
  const metadataPath = path.join(directory, "runtime.json");
  const stat = await fsp.lstat(metadataPath).catch(() => null);
  if (!stat?.isFile() || stat.isSymbolicLink() || stat.size > 1024 * 1024) {
    throw new LocalModelError("LOCAL_MODEL_RUNTIME_MISSING", "runtime metadata is missing or unsafe");
  }
  const value = JSON.parse(await fsp.readFile(metadataPath, "utf8")) as InstalledLocalRuntimeMetadata;
  if (value.schemaVersion !== 1 || value.id !== expected.id || value.version !== expected.version
    || value.platform !== expected.platform || !["in-process", "sidecar"].includes(value.kind)
    || (value.entrypoint !== null && typeof value.entrypoint !== "string") || !Array.isArray(value.files)) {
    throw new LocalModelError("LOCAL_MODEL_RUNTIME_MISSING", "runtime metadata identity is invalid");
  }
  const paths = new Set<string>();
  for (const file of value.files) {
    throwIfAborted(signal);
    if (!file || typeof file.path !== "string" || !Number.isSafeInteger(file.bytes)
      || file.bytes <= 0 || !/^[a-f0-9]{64}$/.test(file.sha256) || paths.has(file.path.toLowerCase())) {
      throw new LocalModelError("LOCAL_MODEL_RUNTIME_MISSING", "runtime file manifest is invalid");
    }
    paths.add(file.path.toLowerCase());
    const absolute = safeJoin(directory, file.path);
    const fileStat = await fsp.lstat(absolute).catch(() => null);
    if (!fileStat?.isFile() || fileStat.isSymbolicLink() || fileStat.size !== file.bytes
      || await hashFile(absolute, signal) !== file.sha256) {
      throw new LocalModelError("LOCAL_MODEL_RUNTIME_MISSING", `runtime file failed integrity validation: ${file.path}`);
    }
  }
  if (value.entrypoint && !paths.has(value.entrypoint.toLowerCase())) {
    throw new LocalModelError("LOCAL_MODEL_RUNTIME_MISSING", "runtime entrypoint is absent from its file manifest");
  }
  return value;
}

function parseEmbedding(value: unknown): LocalEmbeddingOutput {
  const input = objectValue(value, "embedding result");
  if (!Array.isArray(input.vectors) || !Number.isSafeInteger(input.dimensions) || typeof input.modelKey !== "string") {
    invalidOutput("invalid sidecar embedding result");
  }
  const vectors = input.vectors.map((vector) => {
    if (!Array.isArray(vector) || vector.some((number) => typeof number !== "number" || !Number.isFinite(number))) {
      invalidOutput("invalid sidecar embedding vector");
    }
    return vector as number[];
  });
  if (vectors.some((vector) => vector.length !== input.dimensions)) invalidOutput("sidecar embedding dimensions mismatch");
  return { vectors, dimensions: input.dimensions as number, modelKey: input.modelKey };
}

function parseOcr(value: unknown): LocalOcrOutput {
  const input = objectValue(value, "OCR result");
  if (typeof input.markdown !== "string" || typeof input.text !== "string" || !Array.isArray(input.warnings)
    || input.warnings.some((warning) => typeof warning !== "string")) invalidOutput("invalid sidecar OCR result");
  return { markdown: input.markdown, text: input.text, format: "ocr", warnings: input.warnings as string[] };
}

function parseTranscription(value: unknown): LocalTranscriptionOutput {
  const input = objectValue(value, "transcription result");
  if (typeof input.text !== "string" || (input.language !== undefined && typeof input.language !== "string")
    || (input.durationMs !== undefined && (typeof input.durationMs !== "number" || !Number.isFinite(input.durationMs)))) {
    invalidOutput("invalid sidecar transcription result");
  }
  return {
    text: input.text,
    ...(typeof input.language === "string" ? { language: input.language } : {}),
    ...(typeof input.durationMs === "number" ? { durationMs: input.durationMs } : {}),
  };
}

function parseSynthesis(value: unknown): LocalSynthesisOutput {
  const input = objectValue(value, "synthesis result");
  if ((input.sampleRate !== 16000 && input.sampleRate !== 24000)
    || (input.format !== "wav" && input.format !== "pcm_s16le") || typeof input.audioBase64 !== "string") {
    invalidOutput("invalid sidecar synthesis result");
  }
  const audio = Buffer.from(input.audioBase64, "base64");
  if (audio.length === 0) invalidOutput("sidecar synthesis returned empty audio");
  return { sampleRate: input.sampleRate, format: input.format, audio: Uint8Array.from(audio) };
}

function objectValue(value: unknown, label: string): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) invalidOutput(`${label} must be an object`);
  return value as Record<string, unknown>;
}

function invalidOutput(message: string): never {
  throw new LocalModelError("LOCAL_MODEL_SIDECAR_FAILED", message);
}

function safeJoin(root: string, relative: string): string {
  const normalized = relative.replaceAll("\\", "/");
  if (!normalized || normalized.startsWith("/") || normalized.split("/").some((part) => !part || part === "." || part === "..")) {
    throw new LocalModelError("LOCAL_MODEL_RUNTIME_MISSING", "runtime path is unsafe");
  }
  const target = path.resolve(root, ...normalized.split("/"));
  if (!target.startsWith(`${root}${path.sep}`)) {
    throw new LocalModelError("LOCAL_MODEL_RUNTIME_MISSING", "runtime path escaped its directory");
  }
  return target;
}

async function hashFile(filePath: string, signal: AbortSignal): Promise<string> {
  const hash = createHash("sha256");
  for await (const chunk of fs.createReadStream(filePath)) {
    throwIfAborted(signal);
    hash.update(chunk as Buffer);
  }
  return hash.digest("hex");
}
