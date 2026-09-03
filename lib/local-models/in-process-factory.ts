import fsp from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { BackendProbe } from "./backend-probe.ts";
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
import type { LocalModelRegistryEntry } from "./registry.ts";
import type { LocalModelInstanceFactory, LocalModelLoadedInstance } from "./runtime-service.ts";
import { readInstalledRuntimeMetadata } from "./sidecar-factory.ts";

export interface InProcessRuntimeSession {
  backend: LocalModelBackend;
  protocolId?: string;
  embed?(request: LocalEmbedRequest): Promise<LocalEmbeddingOutput>;
  ocr?(request: LocalOcrRequest): Promise<LocalOcrOutput>;
  transcribe?(request: LocalTranscriptionRequest): Promise<LocalTranscriptionOutput>;
  synthesize?(request: LocalSynthesisRequest): Promise<LocalSynthesisOutput>;
  dispose(signal: AbortSignal): Promise<void>;
  rssMb?(): number | Promise<number>;
}

export interface InProcessRuntimeModule {
  probeBackend?(options: InProcessRuntimeCreateOptions): Promise<boolean | { available: boolean; reason?: string }>;
  createLocalModelRuntime(options: InProcessRuntimeCreateOptions): Promise<InProcessRuntimeSession>;
}

export interface InProcessRuntimeCreateOptions {
  modelDirectory: string;
  backend: LocalModelBackend;
  threads: "auto" | number;
  capabilities: Readonly<Record<string, unknown>>;
  signal: AbortSignal;
}

export interface InProcessFactoryOptions {
  runtimeRoot: string;
  config: () => LocalModelsConfig;
  platform?: NodeJS.Platform;
  arch?: string;
  hasNvidiaGpu?: () => boolean | Promise<boolean>;
  backendProbe?: BackendProbe;
  importModule?: (entrypoint: string) => Promise<unknown>;
}

interface ManagedInProcessInstance extends LocalModelLoadedInstance {
  session: InProcessRuntimeSession;
}

export class InProcessInstanceFactory implements LocalModelInstanceFactory {
  private readonly runtimeRoot: string;
  private readonly config: () => LocalModelsConfig;
  private readonly platform: NodeJS.Platform;
  private readonly arch: string;
  private readonly hasNvidiaGpu: () => boolean | Promise<boolean>;
  private readonly backendProbe: BackendProbe;
  private readonly importModule: (entrypoint: string) => Promise<unknown>;

  constructor(options: InProcessFactoryOptions) {
    this.runtimeRoot = path.resolve(options.runtimeRoot);
    this.config = options.config;
    this.platform = options.platform ?? process.platform;
    this.arch = options.arch ?? process.arch;
    this.hasNvidiaGpu = options.hasNvidiaGpu ?? (() => false);
    this.backendProbe = options.backendProbe ?? new BackendProbe();
    this.importModule = options.importModule ?? ((entrypoint) => import(pathToFileURL(entrypoint).href));
  }

  async load(
    descriptor: LocalModelDescriptor,
    installed: LocalModelRegistryEntry,
    signal: AbortSignal,
  ): Promise<LocalModelLoadedInstance> {
    if (descriptor.category !== "stt" && descriptor.category !== "tts") {
      throw new LocalModelError("LOCAL_MODEL_UNSUPPORTED", "in-process runtime only supports local STT and TTS");
    }
    const platformKey = `${this.platform}-${this.arch}`;
    const runtimeDir = path.join(this.runtimeRoot, installed.runtimeId, installed.runtimeVersion, platformKey);
    const runtime = await readInstalledRuntimeMetadata(runtimeDir, {
      id: installed.runtimeId,
      version: installed.runtimeVersion,
      platform: platformKey,
    }, signal);
    if (runtime.kind !== "in-process" || !runtime.entrypoint) {
      throw new LocalModelError("LOCAL_MODEL_RUNTIME_MISSING", "installed runtime is not an in-process runtime");
    }
    const entrypoint = safeJoin(runtimeDir, runtime.entrypoint);
    const stat = await fsp.lstat(entrypoint).catch(() => null);
    if (!stat?.isFile() || stat.isSymbolicLink()) {
      throw new LocalModelError("LOCAL_MODEL_RUNTIME_MISSING", "in-process runtime entrypoint is missing or unsafe");
    }
    const module = parseRuntimeModule(await this.importModule(entrypoint));
    const config = this.config();
    const probe = await this.backendProbe.probe({
      platform: this.platform,
      arch: this.arch,
      hasNvidiaGpu: await this.hasNvidiaGpu(),
      forcedBackend: config.backend,
      cacheScope: `${installed.runtimeId}@${installed.runtimeVersion}`,
      signal,
      validate: async (backend, probeSignal) => {
        const createOptions = runtimeOptions(installed, backend, config, probeSignal);
        if (module.probeBackend) {
          try {
            const result = await module.probeBackend(createOptions);
            return typeof result === "boolean" ? { available: result } : result;
          } catch (error) {
            return { available: false, reason: error instanceof Error ? error.message : String(error) };
          }
        }
        let probeSession: InProcessRuntimeSession | null = null;
        try {
          probeSession = await module.createLocalModelRuntime(createOptions);
          return { available: probeSession.backend === backend };
        } catch (error) {
          return { available: false, reason: error instanceof Error ? error.message : String(error) };
        } finally {
          await probeSession?.dispose(new AbortController().signal).catch(() => {});
        }
      },
    });
    throwIfAborted(signal);
    const session = await module.createLocalModelRuntime(runtimeOptions(installed, probe.backend, config, signal));
    if (!session || session.backend !== probe.backend || typeof session.dispose !== "function") {
      await session?.dispose?.(new AbortController().signal).catch(() => {});
      throw new LocalModelError("LOCAL_MODEL_BACKEND_UNAVAILABLE", "in-process runtime returned an invalid backend session");
    }
    const method = descriptor.category === "stt" ? session.transcribe : session.synthesize;
    if (typeof method !== "function") {
      await session.dispose(new AbortController().signal).catch(() => {});
      throw new LocalModelError("LOCAL_MODEL_UNSUPPORTED", `in-process runtime does not support ${descriptor.category}`);
    }
    const managed: ManagedInProcessInstance = {
      session,
      backend: session.backend,
      protocolId: session.protocolId ?? `local-sherpa-${descriptor.category}`,
      diagnostics: { runtimeId: installed.runtimeId, runtimeVersion: installed.runtimeVersion, runtimeKind: "in-process" },
      embed: session.embed?.bind(session),
      ocr: session.ocr?.bind(session),
      transcribe: session.transcribe?.bind(session),
      synthesize: session.synthesize?.bind(session),
    };
    return managed;
  }

  async unload(
    instance: LocalModelLoadedInstance,
    _descriptor: LocalModelDescriptor,
    signal: AbortSignal,
  ): Promise<void> {
    const managed = instance as ManagedInProcessInstance;
    if (!managed.session) throw new LocalModelError("LOCAL_MODEL_RUNTIME_MISSING", "in-process session is missing");
    await managed.session.dispose(signal);
  }

  async rssMb(instance: LocalModelLoadedInstance): Promise<number> {
    const managed = instance as ManagedInProcessInstance;
    const value = await Promise.resolve(managed.session.rssMb?.() ?? process.memoryUsage.rss() / (1024 * 1024));
    return Number.isFinite(value) && value >= 0 ? value : 0;
  }
}

function runtimeOptions(
  installed: LocalModelRegistryEntry,
  backend: LocalModelBackend,
  config: LocalModelsConfig,
  signal: AbortSignal,
): InProcessRuntimeCreateOptions {
  return {
    modelDirectory: installed.directory,
    backend,
    threads: config.threads,
    capabilities: installed.capabilities,
    signal,
  };
}

function parseRuntimeModule(value: unknown): InProcessRuntimeModule {
  if (!value || typeof value !== "object" || typeof (value as InProcessRuntimeModule).createLocalModelRuntime !== "function") {
    throw new LocalModelError("LOCAL_MODEL_RUNTIME_MISSING", "in-process runtime module has no factory export");
  }
  return value as InProcessRuntimeModule;
}

function safeJoin(root: string, relative: string): string {
  const normalized = relative.replaceAll("\\", "/");
  if (!normalized || normalized.startsWith("/") || normalized.split("/").some((part) => !part || part === "." || part === "..")) {
    throw new LocalModelError("LOCAL_MODEL_RUNTIME_MISSING", "in-process runtime path is unsafe");
  }
  const target = path.resolve(root, ...normalized.split("/"));
  if (!target.startsWith(`${root}${path.sep}`)) {
    throw new LocalModelError("LOCAL_MODEL_RUNTIME_MISSING", "in-process runtime path escaped its directory");
  }
  return target;
}
