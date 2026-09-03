import { createHash, randomUUID } from "node:crypto";
import fs from "node:fs";
import fsp from "node:fs/promises";
import path from "node:path";
import type { LocalModelCategory } from "./contracts.ts";
import type { DownloadAsset, ResumableDownloader } from "./downloader.ts";
import { LocalModelError, throwIfAborted } from "./errors.ts";
import { extractLocalModelZip } from "./secure-archive.ts";
import type {
  LocalModelManifestEntry,
  LocalModelManifestVariant,
  LocalModelPackageAsset,
  LocalModelsManifest,
  LocalRuntimeManifestEntry,
  LocalRuntimePlatformAsset,
} from "./manifest.ts";
import {
  type LocalModelInstalledFile,
  type LocalModelInstallMetadata,
  type LocalModelRegistryEntry,
  LocalModelRegistry,
} from "./registry.ts";

export interface InstalledLocalRuntimeMetadata {
  schemaVersion: 1;
  id: string;
  version: string;
  platform: string;
  kind: "in-process" | "sidecar";
  entrypoint: string | null;
  packageSha256: string;
  installedAt: string;
  files: LocalModelInstalledFile[];
}

export interface LocalModelInstallEvent {
  kind: "runtime_download" | "runtime_extract" | "runtime_ready" | "model_download" | "model_extract" | "model_ready";
  modelId: string;
  quant: string;
  runtimeId?: string;
}

export interface LocalModelInstallerOptions {
  registry: LocalModelRegistry;
  downloader: ResumableDownloader;
  runtimeRoot: string;
  workRoot: string;
  platform?: string;
  onEvent?: (event: LocalModelInstallEvent) => void;
}

export class LocalModelInstaller {
  private readonly registry: LocalModelRegistry;
  private readonly downloader: ResumableDownloader;
  private readonly runtimeRoot: string;
  private readonly workRoot: string;
  private readonly platform: string;
  private readonly onEvent: (event: LocalModelInstallEvent) => void;
  private readonly inFlight = new Map<string, Promise<LocalModelRegistryEntry>>();

  constructor(options: LocalModelInstallerOptions) {
    this.registry = options.registry;
    this.downloader = options.downloader;
    this.runtimeRoot = path.resolve(options.runtimeRoot);
    this.workRoot = path.resolve(options.workRoot);
    this.platform = options.platform ?? `${process.platform}-${process.arch}`;
    if (!/^(win32|darwin|linux)-(x64|arm64)$/.test(this.platform)) {
      throw new LocalModelError("LOCAL_MODEL_UNSUPPORTED", `unsupported local model platform ${this.platform}`);
    }
    this.onEvent = options.onEvent ?? (() => {});
  }

  async install(
    manifest: LocalModelsManifest,
    modelId: string,
    quant: string,
    options: { signal: AbortSignal },
  ): Promise<LocalModelRegistryEntry> {
    const key = `${manifest.manifestVersion}:${modelId}@${quant}:${this.platform}`;
    const existing = this.inFlight.get(key);
    if (existing) return waitWithAbort(existing, options.signal);
    throwIfAborted(options.signal);
    const promise = this.performInstall(manifest, modelId, quant, options.signal)
      .finally(() => this.inFlight.delete(key));
    this.inFlight.set(key, promise);
    return promise;
  }

  runtimeDirectory(runtimeId: string, version: string): string {
    return path.join(this.runtimeRoot, runtimeId, version, this.platform);
  }

  async resolveManualImportContract(
    manifest: LocalModelsManifest,
    modelId: string,
    quant: string,
    signal: AbortSignal,
  ): Promise<{
    category: LocalModelCategory;
    tier: "small" | "large";
    runtimeId: string;
    runtimeVersion: string;
    runtimeKind: "in-process" | "sidecar";
    estimatedPeakRssMb: number;
    runtimeArgs: string[];
    capabilities: Record<string, unknown>;
    licenseFile: string;
  }> {
    const model = manifest.models.find((entry) => entry.id === modelId);
    const variant = model?.variants.find((entry) => entry.quant === quant);
    if (!model || !variant) {
      throw new LocalModelError("LOCAL_MODEL_MANIFEST_INVALID", "manual import selection is absent from manifest");
    }
    const runtime = manifest.runtimes.find((entry) => entry.id === model.runtime && entry.version === model.runtimeVersion);
    const asset = runtime?.platforms[this.platform];
    if (!runtime || !asset || !await verifyRuntimeDirectory(
      this.runtimeDirectory(runtime.id, runtime.version), runtime, asset, this.platform, signal,
    )) {
      throw new LocalModelError("LOCAL_MODEL_RUNTIME_MISSING", "install this model runtime before importing raw model files", {
        runtimeId: model.runtime,
        runtimeVersion: model.runtimeVersion,
        platform: this.platform,
      });
    }
    return {
      category: model.category,
      tier: variant.tier,
      runtimeId: runtime.id,
      runtimeVersion: runtime.version,
      runtimeKind: runtime.kind,
      estimatedPeakRssMb: variant.estimatedPeakRssMb,
      runtimeArgs: variant.runtimeArgs,
      capabilities: variant.capabilities,
      licenseFile: model.licenseFile,
    };
  }

  private async performInstall(
    manifest: LocalModelsManifest,
    modelId: string,
    quant: string,
    signal: AbortSignal,
  ): Promise<LocalModelRegistryEntry> {
    const installedScan = await this.registry.scan({ signal });
    const installed = installedScan.models.find((entry) => entry.id === modelId && entry.quant === quant);
    if (installed) {
      if (installed.version === manifest.manifestVersion) return installed;
      throw new LocalModelError("LOCAL_MODEL_ALREADY_INSTALLED", "a different manifest version of this model variant is installed", {
        modelId,
        quant,
        installedVersion: installed.version,
        requestedVersion: manifest.manifestVersion,
      });
    }
    const model = manifest.models.find((entry) => entry.id === modelId);
    const variant = model?.variants.find((entry) => entry.quant === quant);
    if (!model || !variant) {
      throw new LocalModelError("LOCAL_MODEL_MANIFEST_INVALID", "requested model variant is absent from manifest", {
        modelId,
        quant,
      });
    }
    const runtime = manifest.runtimes.find((entry) =>
      entry.id === model.runtime && entry.version === model.runtimeVersion);
    if (!runtime) throw new LocalModelError("LOCAL_MODEL_MANIFEST_INVALID", "model runtime is absent from manifest");
    await ensureSafeDirectory(this.runtimeRoot);
    await ensureSafeDirectory(this.workRoot);
    await this.ensureRuntime(runtime, modelId, quant, signal);
    return this.installModel(manifest, model, variant, runtime.kind, signal);
  }

  private async ensureRuntime(
    runtime: LocalRuntimeManifestEntry,
    modelId: string,
    quant: string,
    signal: AbortSignal,
  ): Promise<string> {
    const asset = runtime.platforms[this.platform];
    if (!asset) {
      throw new LocalModelError("LOCAL_MODEL_RUNTIME_MISSING", "runtime has no package for this platform", {
        runtimeId: runtime.id,
        runtimeVersion: runtime.version,
        platform: this.platform,
      });
    }
    const target = this.runtimeDirectory(runtime.id, runtime.version);
    if (await verifyRuntimeDirectory(target, runtime, asset, this.platform, signal)) return target;
    const targetParent = path.dirname(target);
    await ensureSafeDirectory(targetParent);
    if (await fsp.lstat(target).catch(() => null)) {
      throw new LocalModelError("LOCAL_MODEL_RUNTIME_MISSING", "installed runtime failed integrity validation", {
        runtimeId: runtime.id,
        runtimeVersion: runtime.version,
        platform: this.platform,
      });
    }
    requireZip(asset.format);
    this.onEvent({ kind: "runtime_download", modelId, quant, runtimeId: runtime.id });
    const result = await this.downloader.download(downloadAsset(
      `runtime-${runtime.id}@${runtime.version}-${this.platform}`,
      asset,
    ), { signal });
    const staging = path.join(targetParent, `.staging-${randomUUID()}`);
    try {
      this.onEvent({ kind: "runtime_extract", modelId, quant, runtimeId: runtime.id });
      await extractLocalModelZip(result.filePath, staging, { signal, expectedEntries: asset.entries });
      const files = await describeFiles(staging, asset.entries, signal);
      const metadata: InstalledLocalRuntimeMetadata = {
        schemaVersion: 1,
        id: runtime.id,
        version: runtime.version,
        platform: this.platform,
        kind: runtime.kind,
        entrypoint: asset.entrypoint ?? null,
        packageSha256: asset.sha256,
        installedAt: new Date().toISOString(),
        files,
      };
      await fsp.writeFile(path.join(staging, "runtime.json"), `${JSON.stringify(metadata, null, 2)}\n`, {
        flag: "wx",
        mode: 0o600,
      });
      if (process.platform !== "win32") {
        await Promise.all(asset.entries.filter((entry) => entry.startsWith("bin/"))
          .map((entry) => fsp.chmod(safeJoin(staging, entry), 0o755)));
      }
      await fsp.rename(staging, target);
      await this.downloader.cancel(result.taskId);
      this.onEvent({ kind: "runtime_ready", modelId, quant, runtimeId: runtime.id });
      return target;
    } catch (error) {
      await fsp.rm(staging, { recursive: true, force: true });
      throw error;
    }
  }

  private async installModel(
    manifest: LocalModelsManifest,
    model: LocalModelManifestEntry,
    variant: LocalModelManifestVariant,
    runtimeKind: "in-process" | "sidecar",
    signal: AbortSignal,
  ): Promise<LocalModelRegistryEntry> {
    const asset = selectModelPackage(variant.packages, this.platform);
    requireZip(asset.format);
    if (!asset.entries.includes(model.licenseFile)) {
      throw new LocalModelError("LOCAL_MODEL_MANIFEST_INVALID", "model package does not declare its license file", {
        modelId: model.id,
        licenseFile: model.licenseFile,
      });
    }
    this.onEvent({ kind: "model_download", modelId: model.id, quant: variant.quant });
    const result = await this.downloader.download(downloadAsset(`model-${model.id}@${variant.quant}`, asset), { signal });
    const staging = path.join(this.workRoot, `.model-${randomUUID()}`);
    try {
      this.onEvent({ kind: "model_extract", modelId: model.id, quant: variant.quant });
      await extractLocalModelZip(result.filePath, staging, { signal, expectedEntries: asset.entries });
      const files = await describeFiles(staging, asset.entries, signal);
      const metadata: LocalModelInstallMetadata = {
        schemaVersion: 1,
        id: model.id,
        category: model.category,
        quant: variant.quant,
        tier: variant.tier,
        version: manifest.manifestVersion,
        runtimeId: model.runtime,
        runtimeVersion: model.runtimeVersion,
        runtimeKind,
        estimatedPeakRssMb: variant.estimatedPeakRssMb,
        runtimeArgs: variant.runtimeArgs,
        capabilities: variant.capabilities,
        licenseFile: model.licenseFile,
        source: "remote",
        installedAt: new Date().toISOString(),
        integrity: "verified",
        bytes: files.reduce((sum, file) => sum + file.bytes, 0),
        sha256Manifest: createHash("sha256").update(JSON.stringify(manifest)).digest("hex"),
        files,
      };
      const installed = await this.registry.installRemoteDirectory(staging, metadata, { signal });
      await this.downloader.cancel(result.taskId);
      this.onEvent({ kind: "model_ready", modelId: model.id, quant: variant.quant });
      return installed;
    } finally {
      await fsp.rm(staging, { recursive: true, force: true });
    }
  }
}

function selectModelPackage(packages: LocalModelPackageAsset[], platform: string): LocalModelPackageAsset {
  const asset = packages.find((entry) => entry.platform === platform)
    ?? packages.find((entry) => entry.platform === "*");
  if (!asset) {
    throw new LocalModelError("LOCAL_MODEL_UNSUPPORTED", "model variant has no package for this platform", { platform });
  }
  return asset;
}

function downloadAsset(
  id: string,
  asset: Pick<LocalRuntimePlatformAsset, "uri" | "bytes" | "sha256">,
): DownloadAsset {
  return { id, uri: asset.uri, bytes: asset.bytes, sha256: asset.sha256 };
}

function requireZip(format: "zip" | "tar.zst"): void {
  if (format !== "zip") {
    throw new LocalModelError("LOCAL_MODEL_UNSUPPORTED", "tar.zst local model packages are not supported by this build");
  }
}

async function verifyRuntimeDirectory(
  directory: string,
  runtime: LocalRuntimeManifestEntry,
  asset: LocalRuntimePlatformAsset,
  platform: string,
  signal: AbortSignal,
): Promise<boolean> {
  const stat = await fsp.lstat(directory).catch(() => null);
  if (!stat) return false;
  if (!stat.isDirectory() || stat.isSymbolicLink()) return false;
  try {
    const metadataPath = path.join(directory, "runtime.json");
    const metadataStat = await fsp.lstat(metadataPath);
    if (!metadataStat.isFile() || metadataStat.isSymbolicLink() || metadataStat.size > 1024 * 1024) return false;
    const metadata = JSON.parse(await fsp.readFile(metadataPath, "utf8")) as InstalledLocalRuntimeMetadata;
    if (metadata.schemaVersion !== 1 || metadata.id !== runtime.id || metadata.version !== runtime.version
      || metadata.platform !== platform || metadata.kind !== runtime.kind
      || metadata.entrypoint !== (asset.entrypoint ?? null) || metadata.packageSha256 !== asset.sha256
      || !Array.isArray(metadata.files) || metadata.files.length !== asset.entries.length) return false;
    const expected = new Set(asset.entries);
    for (const file of metadata.files) {
      throwIfAborted(signal);
      if (!expected.has(file.path)) return false;
      const absolute = safeJoin(directory, file.path);
      const fileStat = await fsp.lstat(absolute);
      if (!fileStat.isFile() || fileStat.isSymbolicLink() || fileStat.size !== file.bytes) return false;
      if (await hashFile(absolute, signal) !== file.sha256) return false;
    }
    return true;
  } catch {
    return false;
  }
}

async function describeFiles(
  root: string,
  entries: readonly string[],
  signal: AbortSignal,
): Promise<LocalModelInstalledFile[]> {
  const files: LocalModelInstalledFile[] = [];
  for (const relative of entries) {
    throwIfAborted(signal);
    const absolute = safeJoin(root, relative);
    const stat = await fsp.lstat(absolute);
    if (!stat.isFile() || stat.isSymbolicLink()) {
      throw new LocalModelError("LOCAL_MODEL_ARCHIVE_UNSAFE", `extracted model entry is not a regular file: ${relative}`);
    }
    files.push({ path: relative, bytes: stat.size, sha256: await hashFile(absolute, signal) });
  }
  return files.sort((left, right) => left.path.localeCompare(right.path));
}

async function hashFile(filePath: string, signal: AbortSignal): Promise<string> {
  const hash = createHash("sha256");
  for await (const chunk of fs.createReadStream(filePath)) {
    throwIfAborted(signal);
    hash.update(chunk as Buffer);
  }
  return hash.digest("hex");
}

async function ensureSafeDirectory(directory: string): Promise<void> {
  await fsp.mkdir(directory, { recursive: true, mode: 0o700 });
  const stat = await fsp.lstat(directory);
  if (!stat.isDirectory() || stat.isSymbolicLink()) {
    throw new LocalModelError("LOCAL_MODEL_INSTALL_INVALID", "local model install path is unsafe", { directory });
  }
}

function safeJoin(root: string, relative: string): string {
  const target = path.resolve(root, ...relative.replaceAll("\\", "/").split("/"));
  if (!target.startsWith(`${root}${path.sep}`)) {
    throw new LocalModelError("LOCAL_MODEL_INSTALL_INVALID", "local model path escaped its root");
  }
  return target;
}

function waitWithAbort<T>(promise: Promise<T>, signal: AbortSignal): Promise<T> {
  throwIfAborted(signal);
  return new Promise<T>((resolve, reject) => {
    const onAbort = () => reject(new LocalModelError("LOCAL_MODEL_ABORTED", "local model install wait was cancelled"));
    signal.addEventListener("abort", onAbort, { once: true });
    promise.then(
      (value) => {
        signal.removeEventListener("abort", onAbort);
        resolve(value);
      },
      (error) => {
        signal.removeEventListener("abort", onAbort);
        reject(error);
      },
    );
  });
}
