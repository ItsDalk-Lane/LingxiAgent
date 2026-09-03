import type {
  LocalEmbedRequest,
  LocalEmbeddingOutput,
  LocalModelBackend,
  LocalModelDescriptor,
  LocalModelDiagnostics,
  LocalModelRef,
  LocalModelResult,
  LocalModelRuntime,
  LocalOcrOutput,
  LocalOcrRequest,
  LocalSynthesisOutput,
  LocalSynthesisRequest,
  LocalTranscriptionOutput,
  LocalTranscriptionRequest,
} from "./contracts.ts";
import os from "node:os";
import { execFile as execFileCallback } from "node:child_process";
import { localModelKey } from "./contracts.ts";
import { LocalModelError, localModelAbortError, throwIfAborted } from "./errors.ts";
import {
  InstanceManager,
  type ManagedInstanceLease,
  type ManagedInstanceLoader,
} from "./instance-manager.ts";
import { LargeSlot } from "./large-slot.ts";
import { promisify } from "node:util";

const execFile = promisify(execFileCallback);
import { MemoryGovernor } from "./memory-governor.ts";
import { LocalModelRegistry, type LocalModelRegistryEntry } from "./registry.ts";

export interface LocalModelLoadedInstance {
  backend: LocalModelBackend;
  protocolId: string;
  diagnostics?: LocalModelDiagnostics;
  embed?(request: LocalEmbedRequest): Promise<LocalEmbeddingOutput>;
  ocr?(request: LocalOcrRequest): Promise<LocalOcrOutput>;
  transcribe?(request: LocalTranscriptionRequest): Promise<LocalTranscriptionOutput>;
  synthesize?(request: LocalSynthesisRequest): Promise<LocalSynthesisOutput>;
}

export interface LocalModelInstanceFactory {
  load(
    descriptor: LocalModelDescriptor,
    installed: LocalModelRegistryEntry,
    signal: AbortSignal,
  ): Promise<LocalModelLoadedInstance>;
  unload(
    instance: LocalModelLoadedInstance,
    descriptor: LocalModelDescriptor,
    signal: AbortSignal,
  ): Promise<void>;
  rssMb?(instance: LocalModelLoadedInstance): number | Promise<number>;
}

export interface LocalModelCallObservation {
  phase: "start" | "success" | "failure";
  operation: "embed" | "ocr" | "transcribe" | "synthesize";
  modelKey: string;
  protocolId?: string;
  backend?: LocalModelBackend;
  durationMs?: number;
  inputBytes: number;
  peakRssMb?: number;
  errorCode?: string;
}

export interface LocalModelRuntimeServiceOptions {
  registry: LocalModelRegistry;
  factory: LocalModelInstanceFactory;
  largeSlot?: LargeSlot;
  memoryGovernor?: MemoryGovernor;
  idleUnloadMs?: { small?: number; large?: number };
  smallBudgetMb?: number;
  onObservation?: (observation: LocalModelCallObservation) => void;
  now?: () => number;
}

export class LocalModelRuntimeService implements LocalModelRuntime {
  private readonly registry: LocalModelRegistry;
  private readonly factory: LocalModelInstanceFactory;
  private readonly manager: InstanceManager<LocalModelLoadedInstance>;
  private readonly onObservation: (observation: LocalModelCallObservation) => void;
  private readonly now: () => number;
  private readonly installedByKey = new Map<string, LocalModelRegistryEntry>();
  private readonly loadedMetadata = new Map<string, Pick<LocalModelLoadedInstance, "backend" | "protocolId" | "diagnostics">>();
  private readonly largeSlot: LargeSlot;

  constructor(options: LocalModelRuntimeServiceOptions) {
    this.registry = options.registry;
    this.factory = options.factory;
    this.onObservation = options.onObservation ?? (() => {});
    this.now = options.now ?? Date.now;
    const loader: ManagedInstanceLoader<LocalModelLoadedInstance> = {
      load: async (descriptor, signal) => {
        const installed = this.installedByKey.get(localModelKey(descriptor));
        if (!installed) {
          throw new LocalModelError("LOCAL_MODEL_NOT_INSTALLED", "local model disappeared before loading", {
            modelKey: localModelKey(descriptor),
          });
        }
        const instance = await this.factory.load(descriptor, installed, signal);
        this.loadedMetadata.set(localModelKey(descriptor), {
          backend: instance.backend,
          protocolId: instance.protocolId,
          diagnostics: instance.diagnostics,
        });
        return instance;
      },
      unload: async (instance, descriptor, signal) => {
        await this.factory.unload(instance, descriptor, signal);
        this.loadedMetadata.delete(localModelKey(descriptor));
      },
      getRssMb: this.factory.rssMb ? (instance) => instance ? this.factory.rssMb!(instance) : 0 : undefined,
    };
    this.largeSlot = options.largeSlot ?? new LargeSlot();
    this.manager = new InstanceManager({
      loader,
      largeSlot: this.largeSlot,
      memoryGovernor: options.memoryGovernor ?? new MemoryGovernor({
        smallBudgetMb: options.smallBudgetMb,
        getAvailableMemoryMb: () => getAvailableMemoryMb(),
      }),
      idleUnloadMs: options.idleUnloadMs,
    });
    this.registry.on("changed", ({ models }) => this.indexInstalled(models));
    this.indexInstalled(this.registry.snapshot().models);
  }

  async initialize(options: { signal: AbortSignal }): Promise<void> {
    const scan = await this.registry.scan(options);
    this.indexInstalled(scan.models);
  }

  async embed(request: LocalEmbedRequest): Promise<LocalModelResult<LocalEmbeddingOutput>> {
    const inputBytes = request.texts.reduce((sum, text) => sum + Buffer.byteLength(text, "utf8"), 0);
    return this.invoke("embed", "embedding", request, inputBytes, (instance) => instance.embed?.(request));
  }

  async ocr(request: LocalOcrRequest): Promise<LocalModelResult<LocalOcrOutput>> {
    return this.invoke("ocr", "ocr", request, request.image.byteLength, (instance) => instance.ocr?.(request));
  }

  async transcribe(request: LocalTranscriptionRequest): Promise<LocalModelResult<LocalTranscriptionOutput>> {
    const inputBytes = await fileSize(request.filePath);
    return this.invoke("transcribe", "stt", request, inputBytes, (instance) => instance.transcribe?.(request));
  }

  async synthesize(request: LocalSynthesisRequest): Promise<LocalModelResult<LocalSynthesisOutput>> {
    return this.invoke(
      "synthesize",
      "tts",
      request,
      Buffer.byteLength(request.text, "utf8"),
      (instance) => instance.synthesize?.(request),
    );
  }

  snapshot() {
    return this.manager.snapshot().map((entry) => ({
      ...entry,
      ...(this.loadedMetadata.get(entry.key) ?? {}),
    }));
  }

  resourceSnapshot() {
    return {
      memoryBudgetSmallMb: this.manager.memoryBudgetSmallMb(),
      reservations: this.manager.memorySnapshot(),
      largeSlot: this.largeSlot.snapshot(),
    };
  }

  reconfigure(options: {
    idleUnloadMs: { small: number; large: number };
    smallBudgetMb: number;
  }): void {
    this.manager.reconfigure(options);
  }

  async unloadIdle(): Promise<string[]> {
    return this.manager.unloadIdle();
  }

  async preloadInstalledSmall(options: { signal: AbortSignal }): Promise<{
    loaded: string[];
    failed: Array<{ key: string; code: string }>;
  }> {
    const loaded: string[] = [];
    const failed: Array<{ key: string; code: string }> = [];
    const models = this.registry.snapshot().models.filter((entry) => entry.tier === "small");
    for (const installed of models) {
      throwIfAborted(options.signal);
      const descriptor: LocalModelDescriptor = {
        id: installed.id,
        quant: installed.quant,
        manifestVersion: installed.version,
        category: installed.category,
        tier: installed.tier,
        runtimeId: installed.runtimeId,
        runtimeVersion: installed.runtimeVersion,
        estimatedPeakRssMb: installed.estimatedPeakRssMb,
      };
      const key = localModelKey(descriptor);
      this.installedByKey.set(key, installed);
      try {
        await this.manager.preload(descriptor, { signal: options.signal, priority: "batch" });
        loaded.push(key);
      } catch (error) {
        if (options.signal.aborted) throw error;
        failed.push({ key, code: errorCode(error) });
      }
    }
    return { loaded, failed };
  }

  async unload(model: LocalModelRef): Promise<boolean> {
    return this.manager.unloadNow(localModelKey(model));
  }

  async dispose(): Promise<void> {
    await this.manager.dispose();
  }

  private async invoke<TRequest extends { model: LocalModelRef; signal: AbortSignal; priority?: "interactive" | "normal" | "batch" }, TOutput>(
    operation: LocalModelCallObservation["operation"],
    expectedCategory: LocalModelRegistryEntry["category"],
    request: TRequest,
    inputBytes: number,
    execute: (instance: LocalModelLoadedInstance) => Promise<TOutput> | undefined,
  ): Promise<LocalModelResult<TOutput>> {
    throwIfAborted(request.signal);
    const { descriptor, installed } = await this.resolveInstalled(request.model, expectedCategory, request.signal);
    const modelKey = localModelKey(descriptor);
    this.installedByKey.set(modelKey, installed);
    const startedAt = this.now();
    this.onObservation({ phase: "start", operation, modelKey, inputBytes });
    let lease: ManagedInstanceLease<LocalModelLoadedInstance> | null = null;
    try {
      lease = await this.manager.acquire(descriptor, {
        signal: request.signal,
        priority: request.priority,
      });
      const pending = execute(lease.instance);
      if (!pending) {
        throw new LocalModelError("LOCAL_MODEL_UNSUPPORTED", `installed model does not support ${operation}`, {
          modelKey,
          operation,
        });
      }
      const output = await pending;
      throwIfAborted(request.signal);
      const durationMs = Math.max(0, this.now() - startedAt);
      const peakRssMb = await this.sampleRss(lease.instance);
      this.onObservation({
        phase: "success",
        operation,
        modelKey,
        protocolId: lease.instance.protocolId,
        backend: lease.instance.backend,
        durationMs,
        inputBytes,
        peakRssMb,
      });
      return {
        modelId: installed.id,
        variant: installed.quant,
        backend: lease.instance.backend,
        durationMs,
        inputBytes,
        output,
        diagnostics: {
          ...lease.instance.diagnostics,
          protocolId: lease.instance.protocolId,
          runtimeId: installed.runtimeId,
          runtimeVersion: installed.runtimeVersion,
          peakRssMb,
        },
      };
    } catch (error) {
      // 原生层协作结束可能返回普通错误；取消请求应保留取消语义，不误报模型故障。
      const failure = request.signal.aborted ? localModelAbortError() : error;
      this.onObservation({
        phase: "failure",
        operation,
        modelKey,
        durationMs: Math.max(0, this.now() - startedAt),
        inputBytes,
        errorCode: errorCode(failure),
      });
      throw failure;
    } finally {
      await lease?.release();
    }
  }

  private async resolveInstalled(
    ref: LocalModelRef,
    expectedCategory: LocalModelRegistryEntry["category"],
    signal: AbortSignal,
  ): Promise<{ descriptor: LocalModelDescriptor; installed: LocalModelRegistryEntry }> {
    let installed = this.registry.snapshot().models.find((entry) =>
      entry.id === ref.id && entry.quant === ref.quant && entry.version === ref.manifestVersion);
    if (!installed) {
      const scan = await this.registry.scan({ signal });
      installed = scan.models.find((entry) =>
        entry.id === ref.id && entry.quant === ref.quant && entry.version === ref.manifestVersion);
    }
    if (!installed) {
      throw new LocalModelError("LOCAL_MODEL_NOT_INSTALLED", "requested local model variant is not installed", {
        id: ref.id,
        quant: ref.quant,
        manifestVersion: ref.manifestVersion,
      });
    }
    if (installed.category !== expectedCategory) {
      throw new LocalModelError("LOCAL_MODEL_UNSUPPORTED", "local model category does not match the requested operation", {
        id: ref.id,
        category: installed.category,
        expectedCategory,
      });
    }
    return {
      installed,
      descriptor: {
        id: installed.id,
        quant: installed.quant,
        manifestVersion: installed.version,
        category: installed.category,
        tier: installed.tier,
        runtimeId: installed.runtimeId,
        runtimeVersion: installed.runtimeVersion,
        estimatedPeakRssMb: installed.estimatedPeakRssMb,
      },
    };
  }

  private indexInstalled(models: LocalModelRegistryEntry[]): void {
    this.installedByKey.clear();
    for (const installed of models) {
      this.installedByKey.set(localModelKey({
        id: installed.id,
        quant: installed.quant,
        manifestVersion: installed.version,
      }), installed);
    }
  }

  private async sampleRss(instance: LocalModelLoadedInstance): Promise<number | undefined> {
    if (!this.factory.rssMb) return instance.diagnostics?.peakRssMb;
    const value = await Promise.resolve(this.factory.rssMb(instance));
    return Number.isFinite(value) && value >= 0 ? value : undefined;
  }
}

async function fileSize(filePath: string): Promise<number> {
  const stat = await import("node:fs/promises").then((module) => module.lstat(filePath));
  if (!stat.isFile() || stat.isSymbolicLink()) {
    throw new LocalModelError("LOCAL_MODEL_UNSUPPORTED", "local transcription input must be a regular file");
  }
  return stat.size;
}

function errorCode(error: unknown): string {
  if (error instanceof LocalModelError) return error.code;
  return "LOCAL_MODEL_RUNTIME_ERROR";
}

let darwinAvailableCache = { at: 0, mb: 0 };

/**
 * 可用内存准入的取数。macOS 的 os.freemem() 把可回收的 inactive/缓存页全算成已用，
 * 会大面积低估可用内存并误拒正常加载——改用原生 vm_stat 的 free+inactive+speculative
 * 分页统计，vm_stat 不可达（沙箱/解析失败）或非 darwin 平台回退 os.freemem()。结果缓存 1 秒。
 */
export async function getAvailableMemoryMb(): Promise<number> {
  if (process.platform !== "darwin") return os.freemem() / (1024 * 1024);
  const now = Date.now();
  if (now - darwinAvailableCache.at < 1_000) return darwinAvailableCache.mb;
  try {
    const { stdout } = await execFile("vm_stat", [], { timeout: 2_000 });
    const pageSize = Number(/page size of (\d+) bytes/.exec(stdout)?.[1]);
    const pageCount = (name: string): number => {
      const match = new RegExp(`${name}:\\s+(\\d+)`).exec(stdout);
      return match ? Number(match[1]) : 0;
    };
    if (!Number.isFinite(pageSize) || pageSize <= 0) throw new Error("vm_stat page size unparsable");
    const pages = pageCount("Pages free") + pageCount("Pages inactive") + pageCount("Pages speculative");
    darwinAvailableCache = { at: now, mb: (pages * pageSize) / (1024 * 1024) };
  } catch {
    darwinAvailableCache = { at: now, mb: os.freemem() / (1024 * 1024) };
  }
  return darwinAvailableCache.mb;
}
