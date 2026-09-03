import { createHash, randomUUID } from "node:crypto";
import { EventEmitter } from "node:events";
import fs from "node:fs";
import fsp from "node:fs/promises";
import path from "node:path";
import { pipeline } from "node:stream/promises";
import type {
  InstalledLocalModel,
  LocalModelCategory,
  LocalModelIntegrity,
  LocalModelSource,
  LocalModelTier,
} from "./contracts.ts";
import { LocalModelError, throwIfAborted } from "./errors.ts";

export const LOCAL_MODEL_INSTALL_SCHEMA_VERSION = 1;

export interface LocalModelInstalledFile {
  path: string;
  bytes: number;
  sha256: string;
}

export interface LocalModelInstallMetadata {
  schemaVersion: 1;
  id: string;
  category: LocalModelCategory;
  quant: string;
  tier: LocalModelTier;
  version: string;
  runtimeId: string;
  runtimeVersion: string;
  runtimeKind: "in-process" | "sidecar";
  estimatedPeakRssMb: number;
  runtimeArgs: string[];
  capabilities: Record<string, unknown>;
  licenseFile?: string | null;
  source: LocalModelSource;
  installedAt: string;
  integrity: LocalModelIntegrity;
  bytes: number;
  sha256Manifest: string;
  files: LocalModelInstalledFile[];
}

export interface LocalModelRegistryEntry extends InstalledLocalModel {
  tier: LocalModelTier;
  runtimeId: string;
  runtimeVersion: string;
  runtimeKind: "in-process" | "sidecar";
  estimatedPeakRssMb: number;
  directory: string;
  files: readonly LocalModelInstalledFile[];
  runtimeArgs: readonly string[];
  capabilities: Readonly<Record<string, unknown>>;
  licenseFile?: string | null;
}

export interface LocalModelRegistryRejection {
  directory: string;
  reason: string;
}

export interface LocalModelRegistryScan {
  models: LocalModelRegistryEntry[];
  rejected: LocalModelRegistryRejection[];
}

export interface UnmanagedLocalModelInspection {
  files: LocalModelInstalledFile[];
  totalBytes: number;
  formatHints: Array<"onnx" | "gguf" | "transformers">;
}

export type UnmanagedLocalModelMetadata = Omit<
  LocalModelInstallMetadata,
  "schemaVersion" | "version" | "source" | "installedAt" | "integrity" | "bytes" | "sha256Manifest" | "files"
>;

export interface LocalModelRegistryEvents {
  changed: [scan: LocalModelRegistryScan];
}

export class LocalModelRegistry extends EventEmitter<LocalModelRegistryEvents> {
  private readonly rootDir: string;
  private current: LocalModelRegistryScan = { models: [], rejected: [] };

  constructor(rootDir: string) {
    super();
    this.rootDir = path.resolve(rootDir);
  }

  snapshot(): LocalModelRegistryScan {
    return {
      models: [...this.current.models],
      rejected: [...this.current.rejected],
    };
  }

  async scan(options: { signal: AbortSignal }): Promise<LocalModelRegistryScan> {
    throwIfAborted(options.signal);
    await ensureSafeDirectory(this.rootDir);
    const models: LocalModelRegistryEntry[] = [];
    const rejected: LocalModelRegistryRejection[] = [];
    const categories: LocalModelCategory[] = ["embedding", "ocr", "stt", "tts"];
    for (const category of categories) {
      throwIfAborted(options.signal);
      const categoryDir = path.join(this.rootDir, category);
      const categoryStat = await fsp.lstat(categoryDir).catch(() => null);
      if (!categoryStat) continue;
      if (!categoryStat.isDirectory() || categoryStat.isSymbolicLink()) {
        rejected.push({ directory: categoryDir, reason: "category path is not a safe directory" });
        continue;
      }
      const entries = await fsp.readdir(categoryDir, { withFileTypes: true });
      for (const entry of entries) {
        if (entry.name.startsWith(".staging-")) continue;
        const modelDir = path.join(categoryDir, entry.name);
        if (!entry.isDirectory() || entry.isSymbolicLink()) {
          rejected.push({ directory: modelDir, reason: "model path is not a safe directory" });
          continue;
        }
        try {
          const metadata = await verifyInstalledDirectory(modelDir, { category, directoryName: entry.name }, options.signal);
          models.push(toRegistryEntry(modelDir, metadata));
        } catch (error) {
          rejected.push({ directory: modelDir, reason: error instanceof Error ? error.message : String(error) });
        }
      }
    }
    models.sort((left, right) => `${left.category}/${left.id}@${left.quant}`.localeCompare(
      `${right.category}/${right.id}@${right.quant}`,
    ));
    rejected.sort((left, right) => left.directory.localeCompare(right.directory));
    this.current = { models, rejected };
    this.emit("changed", this.snapshot());
    return this.snapshot();
  }

  async importDirectory(
    sourceDirectory: string,
    options: { signal: AbortSignal },
  ): Promise<LocalModelRegistryEntry> {
    throwIfAborted(options.signal);
    const source = path.resolve(sourceDirectory);
    const sourceStat = await fsp.lstat(source).catch(() => null);
    if (!sourceStat?.isDirectory() || sourceStat.isSymbolicLink()) {
      invalidInstall("manual import source must be a regular directory", { source });
    }
    const metadata = await readInstallMetadata(path.join(source, "model.json"));
    const normalized = parseInstallMetadata({
      ...metadata,
      source: "manual",
      installedAt: new Date().toISOString(),
    });
    return this.installPreparedDirectory(source, normalized, options.signal, true);
  }

  async inspectUnmanagedDirectory(
    sourceDirectory: string,
    options: { signal: AbortSignal },
  ): Promise<UnmanagedLocalModelInspection> {
    const source = path.resolve(sourceDirectory);
    const sourceStat = await fsp.lstat(source).catch(() => null);
    if (!sourceStat?.isDirectory() || sourceStat.isSymbolicLink()) {
      invalidInstall("manual import source must be a regular directory");
    }
    const relativeFiles = await listFiles(source, options.signal);
    if (relativeFiles.includes("model.json")) {
      invalidInstall("manual import already contains model.json");
    }
    if (relativeFiles.length === 0) invalidInstall("manual import directory is empty");
    const files: LocalModelInstalledFile[] = [];
    for (const relative of relativeFiles) {
      throwIfAborted(options.signal);
      const absolute = safeJoin(source, relative);
      const stat = await fsp.lstat(absolute);
      files.push({ path: relative, bytes: stat.size, sha256: await hashFile(absolute, options.signal) });
    }
    const lower = relativeFiles.map((entry) => entry.toLowerCase());
    const formatHints: UnmanagedLocalModelInspection["formatHints"] = [];
    if (lower.some((entry) => entry.endsWith(".onnx"))) formatHints.push("onnx");
    if (lower.some((entry) => entry.endsWith(".gguf"))) formatHints.push("gguf");
    if (lower.some((entry) => entry.endsWith(".safetensors")) && lower.some((entry) => path.basename(entry) === "config.json")) {
      formatHints.push("transformers");
    }
    if (formatHints.length === 0) {
      invalidInstall("manual import files are not recognized as ONNX, GGUF, or Transformers assets");
    }
    return {
      files,
      totalBytes: files.reduce((sum, file) => sum + file.bytes, 0),
      formatHints,
    };
  }

  async importUnmanagedDirectory(
    sourceDirectory: string,
    metadata: UnmanagedLocalModelMetadata,
    options: { signal: AbortSignal },
  ): Promise<LocalModelRegistryEntry> {
    const inspected = await this.inspectUnmanagedDirectory(sourceDirectory, options);
    const sha256Manifest = createHash("sha256").update(JSON.stringify(inspected.files)).digest("hex");
    const normalized = parseInstallMetadata({
      schemaVersion: 1,
      ...metadata,
      version: `manual-${sha256Manifest.slice(0, 12)}`,
      source: "manual",
      installedAt: new Date().toISOString(),
      integrity: "unknown",
      bytes: inspected.totalBytes,
      sha256Manifest,
      files: inspected.files,
    });
    return this.installPreparedDirectory(path.resolve(sourceDirectory), normalized, options.signal, false);
  }

  async installRemoteDirectory(
    sourceDirectory: string,
    metadata: LocalModelInstallMetadata,
    options: { signal: AbortSignal },
  ): Promise<LocalModelRegistryEntry> {
    const normalized = parseInstallMetadata(metadata);
    if (normalized.source !== "remote" || normalized.integrity !== "verified") {
      invalidInstall("remote installs must carry verified remote metadata");
    }
    return this.installPreparedDirectory(path.resolve(sourceDirectory), normalized, options.signal, false);
  }

  private async installPreparedDirectory(
    source: string,
    normalized: LocalModelInstallMetadata,
    signal: AbortSignal,
    metadataInSource: boolean,
  ): Promise<LocalModelRegistryEntry> {
    throwIfAborted(signal);
    const sourceStat = await fsp.lstat(source).catch(() => null);
    if (!sourceStat?.isDirectory() || sourceStat.isSymbolicLink()) {
      invalidInstall("install source must be a regular directory", { source });
    }
    await ensureSafeDirectory(this.rootDir);
    const targetParent = path.join(this.rootDir, normalized.category);
    const target = path.join(targetParent, `${normalized.id}@${normalized.quant}`);
    await ensureSafeDirectory(targetParent);
    const targetStat = await fsp.lstat(target).catch(() => null);
    if (targetStat) {
      throw new LocalModelError("LOCAL_MODEL_ALREADY_INSTALLED", "local model variant is already installed", {
        id: normalized.id,
        quant: normalized.quant,
      });
    }
    const staging = path.join(targetParent, `.staging-${randomUUID()}`);
    await fsp.mkdir(staging, { recursive: false, mode: 0o700 });
    try {
      await copyDeclaredFiles(source, staging, normalized.files, signal, metadataInSource);
      await writeInstallMetadata(staging, normalized);
      await verifyInstalledDirectory(staging, {
        category: normalized.category,
        directoryName: `${normalized.id}@${normalized.quant}`,
      }, signal);
      await fsp.rename(staging, target);
    } catch (error) {
      await fsp.rm(staging, { recursive: true, force: true });
      throw error;
    }
    await this.scan({ signal });
    const installed = this.current.models.find((entry) => entry.directory === target);
    if (!installed) invalidInstall("installed model disappeared during registry refresh");
    return installed!;
  }

  async remove(
    category: LocalModelCategory,
    id: string,
    quant: string,
    options: { signal: AbortSignal },
  ): Promise<boolean> {
    throwIfAborted(options.signal);
    if (!["embedding", "ocr", "stt", "tts"].includes(category)) invalidInstall("model category is invalid");
    const safeId = parseSafeId(id, "id");
    const safeQuant = parseSafeId(quant, "quant");
    const target = path.join(this.rootDir, category, `${safeId}@${safeQuant}`);
    if (path.dirname(path.dirname(target)) !== this.rootDir) invalidInstall("model removal escaped registry root");
    const stat = await fsp.lstat(target).catch(() => null);
    if (!stat) return false;
    if (!stat.isDirectory() || stat.isSymbolicLink()) invalidInstall("model removal target is unsafe");
    await fsp.rm(target, { recursive: true, force: false });
    await this.scan(options);
    return true;
  }

  async readLicense(category: LocalModelCategory, id: string, quant: string): Promise<string | null> {
    const entry = this.current.models.find((model) => model.category === category && model.id === id && model.quant === quant);
    if (!entry?.licenseFile) return null;
    const target = safeJoin(entry.directory, entry.licenseFile);
    const stat = await fsp.lstat(target).catch(() => null);
    if (!stat?.isFile() || stat.isSymbolicLink() || stat.size > 1024 * 1024) {
      invalidInstall("installed model license file is missing or unsafe");
    }
    const content = await fsp.readFile(target, "utf8");
    if (content.includes("\0")) invalidInstall("installed model license file is not text");
    return content;
  }
}

export function parseInstallMetadata(value: unknown): LocalModelInstallMetadata {
  const input = objectValue(value, "model.json");
  const allowed = new Set([
    "schemaVersion", "id", "category", "quant", "tier", "version", "runtimeId", "runtimeVersion", "runtimeKind",
    "estimatedPeakRssMb", "runtimeArgs", "capabilities", "licenseFile", "source", "installedAt", "integrity", "bytes",
    "sha256Manifest", "files",
  ]);
  for (const key of Object.keys(input)) if (!allowed.has(key)) invalidInstall(`model.json contains unknown field ${key}`);
  if (input.schemaVersion !== LOCAL_MODEL_INSTALL_SCHEMA_VERSION) invalidInstall("unsupported model.json schemaVersion");
  const category = enumValue(input.category, ["embedding", "ocr", "stt", "tts"] as const, "category");
  const tier = enumValue(input.tier, ["small", "large"] as const, "tier");
  const source = enumValue(input.source, ["remote", "manual"] as const, "source");
  const integrity = enumValue(input.integrity, ["verified", "unknown"] as const, "integrity");
  const filesInput = arrayValue(input.files, "files");
  if (filesInput.length === 0) invalidInstall("model.json files must not be empty");
  const files = filesInput.map((item, index) => parseInstalledFile(item, index));
  assertUniqueCaseInsensitive(files.map((entry) => entry.path));
  const bytes = positiveInteger(input.bytes, "bytes");
  const fileBytes = files.reduce((sum, entry) => sum + entry.bytes, 0);
  if (bytes !== fileBytes) invalidInstall("model.json bytes does not match its declared files");
  return {
    schemaVersion: 1,
    id: parseSafeId(input.id, "id"),
    category,
    quant: parseSafeId(input.quant, "quant"),
    tier,
    version: parseVersion(input.version, "version"),
    runtimeId: parseSafeId(input.runtimeId, "runtimeId"),
    runtimeVersion: parseVersion(input.runtimeVersion, "runtimeVersion"),
    runtimeKind: input.runtimeKind === undefined
      ? inferRuntimeKind(String(input.runtimeId || ""))
      : enumValue(input.runtimeKind, ["in-process", "sidecar"] as const, "runtimeKind"),
    estimatedPeakRssMb: positiveInteger(input.estimatedPeakRssMb, "estimatedPeakRssMb"),
    runtimeArgs: input.runtimeArgs === undefined
      ? []
      : arrayValue(input.runtimeArgs, "runtimeArgs").map((value, index) => stringValue(value, `runtimeArgs[${index}]`)),
    capabilities: input.capabilities === undefined ? {} : objectValue(input.capabilities, "capabilities"),
    licenseFile: input.licenseFile === undefined || input.licenseFile === null
      ? null
      : safeRelativePath(input.licenseFile, "licenseFile"),
    source,
    installedAt: parseIsoDate(input.installedAt, "installedAt"),
    integrity,
    bytes,
    sha256Manifest: parseSha256(input.sha256Manifest, "sha256Manifest"),
    files,
  };
}

async function verifyInstalledDirectory(
  directory: string,
  expected: { category: LocalModelCategory; directoryName: string },
  signal: AbortSignal,
): Promise<LocalModelInstallMetadata> {
  throwIfAborted(signal);
  const rootStat = await fsp.lstat(directory);
  if (!rootStat.isDirectory() || rootStat.isSymbolicLink()) invalidInstall("model root is unsafe");
  const metadata = await readInstallMetadata(path.join(directory, "model.json"));
  if (metadata.category !== expected.category) invalidInstall("model category does not match its directory");
  if (`${metadata.id}@${metadata.quant}` !== expected.directoryName) {
    invalidInstall("model id and quant do not match the directory name");
  }
  const declared = new Set(metadata.files.map((entry) => entry.path.toLowerCase()));
  const actual = await listFiles(directory, signal);
  for (const relative of actual) {
    if (relative === "model.json") continue;
    if (!declared.has(relative.toLowerCase())) invalidInstall(`undeclared installed file ${relative}`);
  }
  if (actual.length !== metadata.files.length + 1) invalidInstall("installed file set does not match model.json");
  for (const file of metadata.files) {
    throwIfAborted(signal);
    const absolute = safeJoin(directory, file.path);
    const stat = await fsp.lstat(absolute).catch(() => null);
    if (!stat?.isFile() || stat.isSymbolicLink()) invalidInstall(`installed file is missing or unsafe: ${file.path}`);
    if (stat.size !== file.bytes) invalidInstall(`installed file size mismatch: ${file.path}`);
    const digest = await hashFile(absolute, signal);
    if (digest !== file.sha256) invalidInstall(`installed file hash mismatch: ${file.path}`);
  }
  return metadata;
}

async function copyDeclaredFiles(
  source: string,
  destination: string,
  files: readonly LocalModelInstalledFile[],
  signal: AbortSignal,
  metadataInSource: boolean,
): Promise<void> {
  const declared = new Set(files.map((entry) => entry.path.toLowerCase()));
  const actual = await listFiles(source, signal);
  for (const relative of actual) {
    if (relative === "model.json") continue;
    if (!declared.has(relative.toLowerCase())) invalidInstall(`manual import contains undeclared file ${relative}`);
  }
  const metadataFiles = actual.filter((relative) => relative === "model.json").length;
  if (metadataFiles !== (metadataInSource ? 1 : 0) || actual.length !== files.length + metadataFiles) {
    invalidInstall("install source file set does not match model.json");
  }
  for (const file of files) {
    throwIfAborted(signal);
    const from = safeJoin(source, file.path);
    const stat = await fsp.lstat(from).catch(() => null);
    if (!stat?.isFile() || stat.isSymbolicLink()) invalidInstall(`manual import file is missing or unsafe: ${file.path}`);
    if (stat.size !== file.bytes) invalidInstall(`manual import file size mismatch: ${file.path}`);
    const to = safeJoin(destination, file.path);
    await fsp.mkdir(path.dirname(to), { recursive: true, mode: 0o700 });
    await pipeline(fs.createReadStream(from), fs.createWriteStream(to, { flags: "wx", mode: 0o600 }), { signal });
    const digest = await hashFile(to, signal);
    if (digest !== file.sha256) invalidInstall(`manual import file hash mismatch: ${file.path}`);
  }
}

async function listFiles(root: string, signal: AbortSignal): Promise<string[]> {
  const files: string[] = [];
  const visit = async (directory: string): Promise<void> => {
    throwIfAborted(signal);
    for (const entry of await fsp.readdir(directory, { withFileTypes: true })) {
      const absolute = path.join(directory, entry.name);
      const relative = path.relative(root, absolute).split(path.sep).join("/");
      if (entry.isSymbolicLink()) invalidInstall(`symbolic links are not allowed: ${relative}`);
      if (entry.isDirectory()) await visit(absolute);
      else if (entry.isFile()) files.push(relative);
      else invalidInstall(`non-regular entries are not allowed: ${relative}`);
    }
  };
  await visit(root);
  files.sort((left, right) => left.localeCompare(right));
  return files;
}

async function readInstallMetadata(filePath: string): Promise<LocalModelInstallMetadata> {
  const stat = await fsp.lstat(filePath).catch(() => null);
  if (!stat?.isFile() || stat.isSymbolicLink() || stat.size > 1024 * 1024) {
    invalidInstall("model.json is missing or unsafe");
  }
  let value: unknown;
  try {
    value = JSON.parse(await fsp.readFile(filePath, "utf8"));
  } catch (error) {
    invalidInstall("model.json is not valid JSON", { cause: error instanceof Error ? error.message : String(error) });
  }
  return parseInstallMetadata(value!);
}

async function writeInstallMetadata(directory: string, metadata: LocalModelInstallMetadata): Promise<void> {
  const target = path.join(directory, "model.json");
  const temporary = path.join(directory, "model.json.tmp");
  await fsp.writeFile(temporary, `${JSON.stringify(metadata, null, 2)}\n`, { flag: "wx", mode: 0o600 });
  await fsp.rename(temporary, target);
}

function parseInstalledFile(value: unknown, index: number): LocalModelInstalledFile {
  const input = objectValue(value, `files[${index}]`);
  const allowed = new Set(["path", "bytes", "sha256"]);
  for (const key of Object.keys(input)) if (!allowed.has(key)) invalidInstall(`files[${index}] contains unknown field ${key}`);
  return {
    path: safeRelativePath(input.path, `files[${index}].path`),
    bytes: positiveInteger(input.bytes, `files[${index}].bytes`),
    sha256: parseSha256(input.sha256, `files[${index}].sha256`),
  };
}

function toRegistryEntry(directory: string, metadata: LocalModelInstallMetadata): LocalModelRegistryEntry {
  const metadataDigest = createHash("sha256").update(JSON.stringify(metadata)).digest("hex");
  return {
    id: metadata.id,
    category: metadata.category,
    quant: metadata.quant,
    version: metadata.version,
    source: metadata.source,
    installedAt: metadata.installedAt,
    bytes: metadata.bytes,
    sha256Manifest: metadata.sha256Manifest || metadataDigest,
    integrity: metadata.integrity,
    tier: metadata.tier,
    runtimeId: metadata.runtimeId,
    runtimeVersion: metadata.runtimeVersion,
    runtimeKind: metadata.runtimeKind,
    estimatedPeakRssMb: metadata.estimatedPeakRssMb,
    directory,
    files: metadata.files,
    runtimeArgs: metadata.runtimeArgs,
    capabilities: metadata.capabilities,
    licenseFile: metadata.licenseFile ?? null,
  };
}

function objectValue(value: unknown, field: string): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) invalidInstall(`${field} must be an object`);
  return value as Record<string, unknown>;
}

function arrayValue(value: unknown, field: string): unknown[] {
  if (!Array.isArray(value)) invalidInstall(`${field} must be an array`);
  return value;
}

function enumValue<const T extends readonly string[]>(value: unknown, values: T, field: string): T[number] {
  if (typeof value !== "string" || !values.includes(value)) invalidInstall(`${field} is invalid`);
  return value as T[number];
}

function parseSafeId(value: unknown, field: string): string {
  if (typeof value !== "string" || !/^[a-z0-9][a-z0-9._-]{0,127}$/.test(value)) {
    invalidInstall(`${field} contains unsupported characters`);
  }
  return value;
}

function parseVersion(value: unknown, field: string): string {
  if (typeof value !== "string" || !/^[A-Za-z0-9][A-Za-z0-9._+-]{0,127}$/.test(value)) {
    invalidInstall(`${field} contains unsupported characters`);
  }
  return value;
}

function stringValue(value: unknown, field: string): string {
  if (typeof value !== "string" || !value) invalidInstall(`${field} must be a non-empty string`);
  return value;
}

function inferRuntimeKind(runtimeId: string): "in-process" | "sidecar" {
  return runtimeId === "sherpa-onnx" || runtimeId === "sherpa-onnx-node" ? "in-process" : "sidecar";
}

function positiveInteger(value: unknown, field: string): number {
  if (!Number.isSafeInteger(value) || Number(value) <= 0) invalidInstall(`${field} must be a positive integer`);
  return Number(value);
}

function parseIsoDate(value: unknown, field: string): string {
  if (typeof value !== "string" || !Number.isFinite(Date.parse(value))) invalidInstall(`${field} must be an ISO date-time`);
  return value;
}

function parseSha256(value: unknown, field: string): string {
  if (typeof value !== "string" || !/^[a-fA-F0-9]{64}$/.test(value)) invalidInstall(`${field} must be a SHA-256 digest`);
  return value.toLowerCase();
}

function safeRelativePath(value: unknown, field: string): string {
  if (typeof value !== "string" || !value) invalidInstall(`${field} must be a non-empty path`);
  const normalized = value.replaceAll("\\", "/");
  if (normalized === "model.json" || normalized.startsWith("/") || /^[A-Za-z]:\//.test(normalized)
    || normalized.split("/").some((part) => !part || part === "." || part === "..")) {
    invalidInstall(`${field} must stay inside the model directory`);
  }
  return normalized;
}

function safeJoin(root: string, relative: string): string {
  const target = path.resolve(root, ...relative.split("/"));
  if (target === root || !target.startsWith(`${root}${path.sep}`)) invalidInstall("model file escaped its directory");
  return target;
}

function assertUniqueCaseInsensitive(values: string[]): void {
  const seen = new Set<string>();
  for (const value of values) {
    const key = value.toLowerCase();
    if (seen.has(key)) invalidInstall(`duplicate model file path ${value}`);
    seen.add(key);
  }
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
  if (!stat.isDirectory() || stat.isSymbolicLink()) invalidInstall("registry path is not a safe directory", { directory });
}

function invalidInstall(message: string, details: Record<string, unknown> = {}): never {
  throw new LocalModelError("LOCAL_MODEL_INSTALL_INVALID", message, details);
}
