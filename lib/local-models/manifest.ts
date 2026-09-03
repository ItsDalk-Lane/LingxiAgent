import { LocalModelError } from "./errors.ts";
import type { LocalModelCategory, LocalModelTier } from "./contracts.ts";

export const LOCAL_MODELS_MANIFEST_SCHEMA_VERSION = 1;

export interface LocalModelPackageAsset {
  platform: string;
  format: "zip" | "tar.zst";
  uri: string;
  bytes: number;
  sha256: string;
  entries: string[];
}

export interface LocalRuntimePlatformAsset {
  format: "zip" | "tar.zst";
  entrypoint?: string;
  uri: string;
  bytes: number;
  sha256: string;
  entries: string[];
}

export interface LocalRuntimeManifestEntry {
  id: string;
  version: string;
  kind: "in-process" | "sidecar";
  platforms: Record<string, LocalRuntimePlatformAsset>;
}

export interface LocalModelManifestVariant {
  quant: string;
  tier: LocalModelTier;
  estimatedPeakRssMb: number;
  runtimeArgs: string[];
  packages: LocalModelPackageAsset[];
  capabilities: Record<string, unknown>;
}

export interface LocalModelManifestEntry {
  id: string;
  category: LocalModelCategory;
  tier: LocalModelTier;
  runtime: string;
  runtimeVersion: string;
  license: string;
  licenseFile: string;
  variants: LocalModelManifestVariant[];
}

export interface LocalModelsManifest {
  schemaVersion: 1;
  manifestVersion: string;
  updatedAt: string;
  runtimes: LocalRuntimeManifestEntry[];
  models: LocalModelManifestEntry[];
}

export function parseLocalModelsManifest(value: unknown): LocalModelsManifest {
  const root = objectAt(value, "manifest");
  const schemaVersion = integerAt(root.schemaVersion, "manifest.schemaVersion");
  if (schemaVersion !== LOCAL_MODELS_MANIFEST_SCHEMA_VERSION) {
    invalid("manifest.schemaVersion", `unsupported schema version ${schemaVersion}`);
  }
  const manifestVersion = nonEmptyString(root.manifestVersion, "manifest.manifestVersion");
  const updatedAt = isoDate(root.updatedAt, "manifest.updatedAt");
  const runtimes = arrayAt(root.runtimes, "manifest.runtimes").map((item, index) =>
    parseRuntime(item, `manifest.runtimes[${index}]`));
  const models = arrayAt(root.models, "manifest.models").map((item, index) =>
    parseModel(item, `manifest.models[${index}]`));
  assertUnique(runtimes.map((entry) => `${entry.id}@${entry.version}`), "manifest.runtimes");
  assertUnique(models.map((entry) => entry.id), "manifest.models");
  const runtimeKeys = new Set(runtimes.map((entry) => `${entry.id}@${entry.version}`));
  for (const [index, model] of models.entries()) {
    if (!runtimeKeys.has(`${model.runtime}@${model.runtimeVersion}`)) {
      invalid(`manifest.models[${index}].runtime`, "references an undeclared runtime id/version");
    }
    assertUnique(model.variants.map((variant) => variant.quant), `manifest.models[${index}].variants`);
  }
  return { schemaVersion: 1, manifestVersion, updatedAt, runtimes, models };
}

function parseRuntime(value: unknown, at: string): LocalRuntimeManifestEntry {
  const input = objectAt(value, at);
  const id = safeId(input.id, `${at}.id`);
  const platformsInput = objectAt(input.platforms, `${at}.platforms`);
  const platforms: Record<string, LocalRuntimePlatformAsset> = {};
  for (const [platform, asset] of Object.entries(platformsInput)) {
    if (!/^(win32|darwin|linux)-(x64|arm64)$/.test(platform)) {
      invalid(`${at}.platforms.${platform}`, "platform key must be <os>-<arch>");
    }
    platforms[platform] = parseRuntimeAsset(asset, `${at}.platforms.${platform}`);
  }
  if (Object.keys(platforms).length === 0) invalid(`${at}.platforms`, "must not be empty");
  return {
    id,
    version: safeVersion(input.version, `${at}.version`),
    kind: runtimeKind(input.kind, id, `${at}.kind`),
    platforms,
  };
}

function parseRuntimeAsset(value: unknown, at: string): LocalRuntimePlatformAsset {
  const input = objectAt(value, at);
  return {
    format: archiveFormat(input.format, `${at}.format`),
    ...(input.entrypoint === undefined ? {} : { entrypoint: safeRelativePath(input.entrypoint, `${at}.entrypoint`) }),
    uri: httpsUrl(input.uri, `${at}.uri`),
    bytes: positiveInteger(input.bytes, `${at}.bytes`),
    sha256: sha256(input.sha256, `${at}.sha256`),
    entries: safeEntries(input.entries, `${at}.entries`),
  };
}

function parseModel(value: unknown, at: string): LocalModelManifestEntry {
  const input = objectAt(value, at);
  const category = enumValue(input.category, ["embedding", "ocr", "stt", "tts"] as const, `${at}.category`);
  const tier = enumValue(input.tier, ["small", "large"] as const, `${at}.tier`);
  const variants = arrayAt(input.variants, `${at}.variants`).map((item, index) =>
    parseVariant(item, tier, `${at}.variants[${index}]`));
  if (variants.length === 0) invalid(`${at}.variants`, "must not be empty");
  return {
    id: safeId(input.id, `${at}.id`),
    category,
    tier,
    runtime: safeId(input.runtime, `${at}.runtime`),
    runtimeVersion: safeVersion(input.runtimeVersion, `${at}.runtimeVersion`),
    license: nonEmptyString(input.license, `${at}.license`),
    licenseFile: safeRelativePath(input.licenseFile, `${at}.licenseFile`),
    variants,
  };
}

function parseVariant(value: unknown, modelTier: LocalModelTier, at: string): LocalModelManifestVariant {
  const input = objectAt(value, at);
  const tier = enumValue(input.tier ?? modelTier, ["small", "large"] as const, `${at}.tier`);
  const packages = arrayAt(input.packages, `${at}.packages`).map((item, index) =>
    parsePackage(item, `${at}.packages[${index}]`));
  if (packages.length === 0) invalid(`${at}.packages`, "must not be empty");
  assertUnique(packages.map((entry) => entry.platform), `${at}.packages`);
  return {
    quant: safeQuant(input.quant, `${at}.quant`),
    tier,
    estimatedPeakRssMb: positiveInteger(input.estimatedPeakRssMb, `${at}.estimatedPeakRssMb`),
    runtimeArgs: stringArray(input.runtimeArgs ?? [], `${at}.runtimeArgs`),
    packages,
    capabilities: input.capabilities === undefined
      ? {}
      : objectAt(input.capabilities, `${at}.capabilities`),
  };
}

function parsePackage(value: unknown, at: string): LocalModelPackageAsset {
  const input = objectAt(value, at);
  const platform = nonEmptyString(input.platform, `${at}.platform`);
  if (platform !== "*" && !/^(win32|darwin|linux)-(x64|arm64)$/.test(platform)) {
    invalid(`${at}.platform`, "must be * or <os>-<arch>");
  }
  return {
    platform,
    format: archiveFormat(input.format, `${at}.format`),
    uri: httpsUrl(input.uri, `${at}.uri`),
    bytes: positiveInteger(input.bytes, `${at}.bytes`),
    sha256: sha256(input.sha256, `${at}.sha256`),
    entries: safeEntries(input.entries, `${at}.entries`),
  };
}

function objectAt(value: unknown, at: string): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) invalid(at, "must be an object");
  return value as Record<string, unknown>;
}

function arrayAt(value: unknown, at: string): unknown[] {
  if (!Array.isArray(value)) invalid(at, "must be an array");
  return value;
}

function nonEmptyString(value: unknown, at: string): string {
  if (typeof value !== "string" || !value.trim()) invalid(at, "must be a non-empty string");
  return value.trim();
}

function integerAt(value: unknown, at: string): number {
  if (!Number.isSafeInteger(value)) invalid(at, "must be a safe integer");
  return Number(value);
}

function positiveInteger(value: unknown, at: string): number {
  const parsed = integerAt(value, at);
  if (parsed <= 0) invalid(at, "must be greater than zero");
  return parsed;
}

function isoDate(value: unknown, at: string): string {
  const text = nonEmptyString(value, at);
  if (!Number.isFinite(Date.parse(text))) invalid(at, "must be an ISO date-time");
  return text;
}

function httpsUrl(value: unknown, at: string): string {
  const text = nonEmptyString(value, at);
  let parsed: URL;
  try {
    parsed = new URL(text);
  } catch {
    invalid(at, "must be an absolute HTTPS URL");
  }
  if (parsed!.protocol !== "https:") invalid(at, "must use HTTPS");
  return parsed!.toString();
}

function sha256(value: unknown, at: string): string {
  const text = nonEmptyString(value, at).toLowerCase();
  if (!/^[a-f0-9]{64}$/.test(text)) invalid(at, "must be a 64-character SHA-256 hex digest");
  return text;
}

function safeId(value: unknown, at: string): string {
  const text = nonEmptyString(value, at);
  if (!/^[a-z0-9][a-z0-9._-]{0,127}$/.test(text)) invalid(at, "contains unsupported characters");
  return text;
}

function safeQuant(value: unknown, at: string): string {
  return safeId(value, at);
}

function safeVersion(value: unknown, at: string): string {
  const text = nonEmptyString(value, at);
  if (!/^[A-Za-z0-9][A-Za-z0-9._+-]{0,127}$/.test(text)) invalid(at, "contains unsupported characters");
  return text;
}

function safeRelativePath(value: unknown, at: string): string {
  const text = nonEmptyString(value, at).replaceAll("\\", "/");
  if (text.startsWith("/") || /^[A-Za-z]:\//.test(text) || text.split("/").includes("..")) {
    invalid(at, "must stay inside the asset root");
  }
  return text;
}

function safeEntries(value: unknown, at: string): string[] {
  const entries = arrayAt(value, at).map((entry, index) => safeRelativePath(entry, `${at}[${index}]`));
  if (entries.length === 0) invalid(at, "must not be empty");
  assertUnique(entries, at);
  return entries;
}

function stringArray(value: unknown, at: string): string[] {
  return arrayAt(value, at).map((item, index) => nonEmptyString(item, `${at}[${index}]`));
}

function archiveFormat(value: unknown, at: string): "zip" | "tar.zst" {
  if (value === undefined || value === "zip") return "zip";
  if (value === "tar.zst") return value;
  invalid(at, "must be zip or tar.zst");
}

function runtimeKind(value: unknown, id: string, at: string): "in-process" | "sidecar" {
  if (value === "in-process" || value === "sidecar") return value;
  if (value === undefined) return id === "sherpa-onnx" || id === "sherpa-onnx-node" ? "in-process" : "sidecar";
  invalid(at, "must be in-process or sidecar");
}

function enumValue<const T extends readonly string[]>(value: unknown, values: T, at: string): T[number] {
  if (typeof value !== "string" || !values.includes(value)) invalid(at, `must be one of ${values.join(", ")}`);
  return value as T[number];
}

function assertUnique(values: string[], at: string): void {
  const seen = new Set<string>();
  for (const value of values) {
    if (seen.has(value)) invalid(at, `contains duplicate value ${value}`);
    seen.add(value);
  }
}

function invalid(at: string, reason: string): never {
  throw new LocalModelError(
    "LOCAL_MODEL_MANIFEST_INVALID",
    `invalid local model manifest at ${at}: ${reason}`,
    { field: at, reason },
  );
}
