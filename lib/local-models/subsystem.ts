import os from "node:os";
import path from "node:path";
import fsp from "node:fs/promises";
import { BUILTIN_LOCAL_MODEL_CATALOG } from "./catalog.ts";
import { createFileProbeCache } from "./backend-probe.ts";
import { CompositeLocalModelInstanceFactory } from "./composite-factory.ts";
import { normalizeLocalModelsConfig, resolveLargeResidentCapacity, type LocalModelsConfig } from "./config.ts";
import { ResumableDownloader, type DownloadProgress } from "./downloader.ts";
import { LocalModelError } from "./errors.ts";
import { createNvidiaGpuDetector, type NvidiaGpuDetection, type NvidiaGpuDetector } from "./gpu-detect.ts";
import { InProcessInstanceFactory } from "./in-process-factory.ts";
import { LargeSlot } from "./large-slot.ts";
import { LocalModelInstaller } from "./installer.ts";
import { LocalModelsManifestClient, type LocalModelsManifestResolution } from "./manifest-client.ts";
import { LocalModelRegistry } from "./registry.ts";
import { LocalModelRuntimeService } from "./runtime-service.ts";
import { SidecarInstanceFactory } from "./sidecar-factory.ts";
import { parseLocalModelKey } from "./contracts.ts";

export interface LocalModelsSubsystemOptions {
  lingxiHome: string;
  manifestUrl?: string;
  getPreferences: () => Record<string, unknown>;
  savePreferences: (preferences: Record<string, unknown>) => unknown | Promise<unknown>;
  emitEvent?: (event: Record<string, unknown>) => void;
  detectNvidiaGpu?: NvidiaGpuDetector;
  totalMemoryBytes?: number;
}

export class LocalModelsSubsystem {
  readonly registry: LocalModelRegistry;
  readonly runtime: LocalModelRuntimeService;
  private readonly options: LocalModelsSubsystemOptions;
  private readonly modelsRoot: string;
  private readonly runtimeRoot: string;
  private readonly downloadsRoot: string;
  private readonly workRoot: string;
  private readonly emitEvent: (event: Record<string, unknown>) => void;
  private readonly manifestClient: LocalModelsManifestClient | null;
  private config: LocalModelsConfig;
  private manifestResolution: LocalModelsManifestResolution = {
    manifest: null,
    catalog: BUILTIN_LOCAL_MODEL_CATALOG,
    source: "builtin",
    etag: null,
    warning: "local model manifest URL is not configured",
  };
  private downloader: ResumableDownloader;
  private installer: LocalModelInstaller;
  private readonly detectNvidiaGpu: NvidiaGpuDetector;
  private readonly device: { totalMemoryMb: number; maxLargeResident: 1 | 2 };
  private readonly liveDownloadProgress = new Map<string, DownloadProgress>();
  private preloadController: AbortController | null = null;
  private preloadPromise: Promise<void> | null = null;

  constructor(options: LocalModelsSubsystemOptions) {
    this.options = options;
    this.modelsRoot = path.join(path.resolve(options.lingxiHome), "models");
    this.runtimeRoot = path.join(path.resolve(options.lingxiHome), "runtime", "local-inference");
    this.downloadsRoot = path.join(this.modelsRoot, ".downloads");
    this.workRoot = path.join(this.modelsRoot, ".install-work");
    this.emitEvent = options.emitEvent ?? (() => {});
    this.config = normalizeLocalModelsConfig(this.readPreferences().localModels);
    this.registry = new LocalModelRegistry(this.modelsRoot);
    this.downloader = this.createDownloader();
    this.installer = this.createInstaller();
    this.detectNvidiaGpu = options.detectNvidiaGpu ?? createNvidiaGpuDetector();
    // 设备自检：按物理内存决定大模型并存容量（auto：≥32GiB→2，否则 1），
    // 容量在 subsystem 构造时定格；加载时的可用内存准入检查独立兜底。
    const totalMemoryBytes = options.totalMemoryBytes ?? os.totalmem();
    this.device = { totalMemoryMb: Math.round(totalMemoryBytes / (1024 * 1024)), maxLargeResident: resolveLargeResidentCapacity(this.config.maxLargeResident, totalMemoryBytes) };
    // auto 后端候选链靠它决定 Windows 上是否把 cuda 排进候选（探测失败按无 NVIDIA 兜底）。
    const hasNvidiaGpu = () => this.detectNvidiaGpu().then((detection) => detection.hasNvidiaGpu);
    const inProcess = new InProcessInstanceFactory({
      runtimeRoot: this.runtimeRoot,
      config: () => this.config,
      hasNvidiaGpu,
    });
    const sidecar = new SidecarInstanceFactory({
      runtimeRoot: this.runtimeRoot,
      logRoot: path.join(path.resolve(options.lingxiHome), "logs", "local-models"),
      config: () => this.config,
      hasNvidiaGpu,
      probeCache: createFileProbeCache(path.join(this.runtimeRoot, "probe-cache.json")),
    });
    this.runtime = new LocalModelRuntimeService({
      registry: this.registry,
      factory: new CompositeLocalModelInstanceFactory(inProcess, sidecar),
      largeSlot: new LargeSlot(() => {}, { capacity: this.device.maxLargeResident }),
      idleUnloadMs: this.config.idleUnloadMs,
      smallBudgetMb: this.config.memoryBudgetSmallMb,
      onObservation: (observation) => this.notify("observation", observation),
    });
    this.registry.on("changed", (scan) => this.notify("registry_changed", {
      installedCount: scan.models.length,
      rejectedCount: scan.rejected.length,
    }));
    this.manifestClient = options.manifestUrl
      ? new LocalModelsManifestClient({
        url: options.manifestUrl,
        cacheDir: path.join(this.modelsRoot, ".manifest-cache"),
      })
      : null;
  }

  async initialize(options: { signal: AbortSignal }): Promise<void> {
    await this.runtime.initialize(options);
    if (this.manifestClient) this.manifestResolution = await this.manifestClient.loadCached();
    // 预热 GPU 探测缓存（探测自带超时、从不 reject），首次 load/state() 即取到结果。
    void this.detectNvidiaGpu();
    if (this.config.preloadSmall) this.startPreloadSmall();
  }

  getConfig(): LocalModelsConfig {
    return structuredClone(this.config);
  }

  async setConfig(value: unknown): Promise<LocalModelsConfig> {
    if (this.downloader.hasActiveTasks()) {
      throw new LocalModelError("LOCAL_MODEL_UNSUPPORTED", "cannot change local model download settings while a task is active");
    }
    const next = normalizeLocalModelsConfig(value);
    const reloadIdle = runtimeLoadConfigChanged(this.config, next);
    const preferences = this.readPreferences();
    await Promise.resolve(this.options.savePreferences({ ...preferences, localModels: next }));
    this.config = next;
    this.runtime.reconfigure({
      idleUnloadMs: next.idleUnloadMs,
      smallBudgetMb: next.memoryBudgetSmallMb,
    });
    if (reloadIdle) await this.runtime.unloadIdle();
    this.liveDownloadProgress.clear();
    this.downloader = this.createDownloader();
    this.installer = this.createInstaller();
    this.notify("config_changed", { config: publicConfig(next) });
    if (next.preloadSmall) this.startPreloadSmall();
    else this.stopPreloadSmall();
    return this.getConfig();
  }

  async refreshManifest(options: { signal: AbortSignal }): Promise<LocalModelsManifestResolution> {
    if (!this.manifestClient) {
      this.manifestResolution = {
        manifest: null,
        catalog: BUILTIN_LOCAL_MODEL_CATALOG,
        source: "builtin",
        etag: null,
        warning: "local model manifest URL is not configured",
      };
    } else {
      this.manifestResolution = await this.manifestClient.refresh(options);
    }
    this.notify("manifest_changed", {
      source: this.manifestResolution.source,
      manifestVersion: this.manifestResolution.manifest?.manifestVersion ?? null,
      warning: this.manifestResolution.warning,
    });
    return this.manifestResolution;
  }

  async state() {
    const tasks = await this.downloader.listTasks();
    const scan = this.registry.snapshot();
    return {
      config: publicConfig(this.config),
      gpu: await this.detectNvidiaGpu(),
      device: { ...this.device },
      manifest: {
        source: this.manifestResolution.source,
        version: this.manifestResolution.manifest?.manifestVersion ?? null,
        updatedAt: this.manifestResolution.manifest?.updatedAt ?? null,
        warning: this.manifestResolution.warning,
        configured: this.manifestClient !== null,
      },
      catalog: this.manifestResolution.catalog,
      installed: scan.models.map((entry) => ({
        id: entry.id,
        category: entry.category,
        quant: entry.quant,
        version: entry.version,
        tier: entry.tier,
        runtimeId: entry.runtimeId,
        runtimeVersion: entry.runtimeVersion,
        runtimeKind: entry.runtimeKind,
        estimatedPeakRssMb: entry.estimatedPeakRssMb,
        source: entry.source,
        installedAt: entry.installedAt,
        bytes: entry.bytes,
        integrity: entry.integrity,
        licenseAvailable: Boolean(entry.licenseFile),
      })),
      rejected: scan.rejected.map((entry) => ({ name: path.basename(entry.directory), reason: entry.reason })),
      instances: this.runtime.snapshot(),
      resources: this.runtime.resourceSnapshot(),
      downloads: tasks.map((task) => {
        const live = this.liveDownloadProgress.get(task.taskId);
        return {
          taskId: task.taskId,
          assetId: task.asset.id,
          status: live?.status ?? task.status,
          attempt: task.attempt,
          downloadedBytes: live?.downloadedBytes ?? task.parts.reduce((sum, part) => sum + part.received, 0),
          totalBytes: task.asset.bytes,
          bytesPerSecond: live?.bytesPerSecond ?? 0,
          remainingMs: live?.remainingMs ?? null,
          updatedAt: task.updatedAt,
          error: task.error ?? null,
        };
      }),
    };
  }

  async install(modelId: string, quant: string, options: { signal: AbortSignal }) {
    const manifest = this.manifestResolution.manifest;
    if (!manifest) {
      throw new LocalModelError("LOCAL_MODEL_MANIFEST_INVALID", "no verified local model manifest is available");
    }
    return this.installer.install(manifest, modelId, quant, options);
  }

  async inspectImportDirectory(directory: string, options: { signal: AbortSignal }) {
    const resolved = path.resolve(directory);
    const metadata = await fsp.lstat(path.join(resolved, "model.json")).catch(() => null);
    if (metadata?.isFile() && !metadata.isSymbolicLink()) return { hasModelJson: true, candidates: [] };
    const inspected = await this.registry.inspectUnmanagedDirectory(resolved, options);
    const manifest = this.manifestResolution.manifest;
    if (!manifest) {
      throw new LocalModelError("LOCAL_MODEL_MANIFEST_INVALID", "a verified manifest is required to identify raw model files");
    }
    const candidates = [];
    for (const model of manifest.models) {
      if (!matchesManualFormat(model.id, model.runtime, inspected.formatHints)) continue;
      const catalog = this.manifestResolution.catalog.find((entry) => entry.id === model.id);
      for (const variant of model.variants) {
        let runtimeReady = true;
        try {
          await this.installer.resolveManualImportContract(manifest, model.id, variant.quant, options.signal);
        } catch (error) {
          if (error instanceof LocalModelError && error.code === "LOCAL_MODEL_RUNTIME_MISSING") runtimeReady = false;
          else throw error;
        }
        candidates.push({
          id: model.id,
          category: model.category,
          displayName: catalog?.displayName ?? model.id,
          quant: variant.quant,
          tier: variant.tier,
          runtimeReady,
        });
      }
    }
    if (candidates.length === 0) {
      throw new LocalModelError("LOCAL_MODEL_INSTALL_INVALID", "raw model file format does not match any manifest model");
    }
    return { hasModelJson: false, formatHints: inspected.formatHints, totalBytes: inspected.totalBytes, candidates };
  }

  async importDirectory(
    directory: string,
    options: { signal: AbortSignal; selection?: { modelId: string; quant: string } },
  ) {
    const metadata = await fsp.lstat(path.join(path.resolve(directory), "model.json")).catch(() => null);
    let installed;
    if (metadata?.isFile() && !metadata.isSymbolicLink()) {
      installed = await this.registry.importDirectory(directory, options);
    } else {
      const manifest = this.manifestResolution.manifest;
      if (!manifest || !options.selection) {
        throw new LocalModelError("LOCAL_MODEL_INSTALL_INVALID", "raw model import requires an inspected model and quant selection");
      }
      const contract = await this.installer.resolveManualImportContract(
        manifest, options.selection.modelId, options.selection.quant, options.signal,
      );
      installed = await this.registry.importUnmanagedDirectory(directory, {
        id: options.selection.modelId,
        quant: options.selection.quant,
        ...contract,
      }, options);
    }
    this.notify("model_imported", { id: installed.id, quant: installed.quant, category: installed.category });
    if (this.config.preloadSmall && installed.tier === "small") this.startPreloadSmall();
    return installed;
  }

  async ocr(input: {
    image: Uint8Array;
    mime: string;
    modelId?: string;
    language?: string;
    signal?: AbortSignal;
  }) {
    const model = this.resolveConfiguredModel("ocr", input.modelId || this.config.ocr.defaultModel);
    return this.runtime.ocr({
      model,
      image: input.image,
      mime: input.mime,
      language: input.language,
      signal: input.signal ?? new AbortController().signal,
      priority: "interactive",
    });
  }

  async synthesize(input: {
    text: string;
    modelId?: string;
    voice?: string;
    sampleRate?: 16000 | 24000;
    signal?: AbortSignal;
    onChunk?: (chunk: Uint8Array) => void | Promise<void>;
  }) {
    const model = this.resolveConfiguredModel("tts", input.modelId || this.config.tts.defaultModel);
    return this.runtime.synthesize({
      model,
      text: input.text,
      voice: input.voice || this.config.tts.voice || undefined,
      sampleRate: input.sampleRate,
      signal: input.signal ?? new AbortController().signal,
      priority: "interactive",
      onChunk: input.onChunk,
    });
  }

  async remove(
    category: "embedding" | "ocr" | "stt" | "tts",
    id: string,
    quant: string,
    options: { signal: AbortSignal },
  ): Promise<boolean> {
    const installed = this.registry.snapshot().models.find((entry) =>
      entry.category === category && entry.id === id && entry.quant === quant);
    if (installed) {
      await this.runtime.unload({ id, quant, manifestVersion: installed.version });
    }
    const removed = await this.registry.remove(category, id, quant, options);
    if (removed) this.notify("model_removed", { id, quant, category, bytes: installed?.bytes ?? 0 });
    return removed;
  }

  async readLicense(category: "embedding" | "ocr" | "stt" | "tts", id: string, quant: string) {
    const content = await this.registry.readLicense(category, id, quant);
    if (content === null) {
      throw new LocalModelError("LOCAL_MODEL_NOT_INSTALLED", "this installed model has no declared license file");
    }
    return content;
  }

  pauseDownload(taskId: string): boolean {
    return this.downloader.pause(taskId);
  }

  cancelDownload(taskId: string): Promise<boolean> {
    return this.downloader.cancel(taskId);
  }

  async dispose(): Promise<void> {
    this.stopPreloadSmall();
    await this.preloadPromise?.catch(() => {});
    await this.runtime.dispose();
  }

  private startPreloadSmall(): void {
    this.stopPreloadSmall();
    const controller = new AbortController();
    this.preloadController = controller;
    this.notify("preload_started", {});
    this.preloadPromise = this.runtime.preloadInstalledSmall({ signal: controller.signal })
      .then((result) => {
        if (!controller.signal.aborted) this.notify("preload_finished", {
          loadedCount: result.loaded.length,
          failedCount: result.failed.length,
          failures: result.failed,
        });
      })
      .catch((error) => {
        if (!controller.signal.aborted) this.notify("preload_failed", {
          code: error instanceof LocalModelError ? error.code : "LOCAL_MODEL_RUNTIME_FAILED",
        });
      })
      .finally(() => {
        if (this.preloadController === controller) this.preloadController = null;
      });
  }

  private stopPreloadSmall(): void {
    this.preloadController?.abort();
    this.preloadController = null;
  }

  private resolveConfiguredModel(category: "ocr" | "tts", identity: string) {
    const ref = parseLocalModelKey(identity);
    if (!ref) {
      throw new LocalModelError("LOCAL_MODEL_NOT_INSTALLED", `no valid default local ${category} model is configured`);
    }
    const installed = this.registry.snapshot().models.find((entry) => (
      entry.category === category
      && entry.id === ref.id
      && entry.quant === ref.quant
      && entry.version === ref.manifestVersion
    ));
    if (!installed) {
      throw new LocalModelError("LOCAL_MODEL_NOT_INSTALLED", `configured local ${category} model is not installed`);
    }
    return ref;
  }

  private createDownloader(): ResumableDownloader {
    return new ResumableDownloader({
      rootDir: this.downloadsRoot,
      concurrency: this.config.download.concurrency,
      mirrorBaseUrl: this.config.download.mirrorBaseUrl,
      onProgress: (progress) => {
        this.liveDownloadProgress.set(progress.taskId, progress);
        this.notify("download_progress", progress);
      },
    });
  }

  private createInstaller(): LocalModelInstaller {
    return new LocalModelInstaller({
      registry: this.registry,
      downloader: this.downloader,
      runtimeRoot: this.runtimeRoot,
      workRoot: this.workRoot,
      onEvent: (event) => this.notify("install_progress", event),
    });
  }

  private readPreferences(): Record<string, unknown> {
    const value = this.options.getPreferences();
    return value && typeof value === "object" && !Array.isArray(value) ? value : {};
  }

  private notify(kind: string, detail: object): void {
    this.emitEvent({ type: "local_models_event", kind, ...detail });
  }
}

function publicConfig(config: LocalModelsConfig): LocalModelsConfig {
  return structuredClone(config);
}

function runtimeLoadConfigChanged(current: LocalModelsConfig, next: LocalModelsConfig): boolean {
  return current.backend !== next.backend
    || current.threads !== next.threads
    || current.useMmap !== next.useMmap
    || current.mlock !== next.mlock;
}

function matchesManualFormat(modelId: string, runtimeId: string, hints: readonly string[]): boolean {
  if (hints.includes("onnx") && (runtimeId.includes("sherpa") || modelId === "sensevoice-small" || modelId === "kokoro-82m")) return true;
  if (hints.includes("gguf") && (runtimeId.includes("llama") || modelId === "qwen3-asr-1.7b")) return true;
  return hints.includes("transformers") && !runtimeId.includes("sherpa") && !runtimeId.includes("llama");
}
