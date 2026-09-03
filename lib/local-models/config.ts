import type { LocalModelBackend } from "./contracts.ts";

export interface LocalModelsConfig {
  backend: "auto" | LocalModelBackend;
  threads: "auto" | number;
  maxLargeResident: "auto" | 1 | 2;
  embedding: { batchSize: number };
  ocr: { defaultModel: string; maxPages: number; maxPixelsPerPage: number };
  stt: { vadEnabled: boolean; chunkMs: number };
  tts: { streaming: boolean; defaultModel: string; voice: string; bridgeReply: boolean };
  useMmap: boolean;
  mlock: boolean;
  quantPreference: "smallest" | "quality";
  idleUnloadMs: { small: number; large: number };
  memoryBudgetSmallMb: number;
  preloadSmall: boolean;
  download: { concurrency: number; mirrorBaseUrl: string };
}

export const DEFAULT_LOCAL_MODELS_CONFIG: Readonly<LocalModelsConfig> = Object.freeze({
  backend: "auto",
  threads: "auto",
  maxLargeResident: "auto",
  embedding: Object.freeze({ batchSize: 32 }),
  ocr: Object.freeze({ defaultModel: "", maxPages: 25, maxPixelsPerPage: 16_000_000 }),
  stt: Object.freeze({ vadEnabled: true, chunkMs: 30_000 }),
  tts: Object.freeze({ streaming: true, defaultModel: "", voice: "", bridgeReply: false }),
  useMmap: true,
  mlock: false,
  quantPreference: "smallest",
  idleUnloadMs: Object.freeze({ small: 300_000, large: 120_000 }),
  memoryBudgetSmallMb: 1536,
  preloadSmall: false,
  download: Object.freeze({ concurrency: 4, mirrorBaseUrl: "" }),
});

const BACKENDS = new Set(["auto", "cpu", "coreml", "metal", "cuda", "vulkan", "directml"]);

export function normalizeLocalModelsConfig(value: unknown): LocalModelsConfig {
  const input = isObject(value) ? value : {};
  const embedding = isObject(input.embedding) ? input.embedding : {};
  const ocr = isObject(input.ocr) ? input.ocr : {};
  const stt = isObject(input.stt) ? input.stt : {};
  const tts = isObject(input.tts) ? input.tts : {};
  const idleUnloadMs = isObject(input.idleUnloadMs) ? input.idleUnloadMs : {};
  const download = isObject(input.download) ? input.download : {};
  const backend = typeof input.backend === "string" && BACKENDS.has(input.backend)
    ? input.backend as LocalModelsConfig["backend"]
    : DEFAULT_LOCAL_MODELS_CONFIG.backend;
  return {
    backend,
    threads: input.threads === "auto"
      ? "auto"
      : boundedInteger(input.threads, 1, 256, DEFAULT_LOCAL_MODELS_CONFIG.threads),
    maxLargeResident: normalizeMaxLargeResident(input.maxLargeResident),
    embedding: { batchSize: boundedInteger(embedding.batchSize, 1, 256, 32) },
    ocr: {
      defaultModel: textOrEmpty(ocr.defaultModel),
      maxPages: boundedInteger(ocr.maxPages, 1, 100, 25),
      maxPixelsPerPage: boundedInteger(ocr.maxPixelsPerPage, 1_000_000, 100_000_000, 16_000_000),
    },
    stt: {
      vadEnabled: booleanOr(stt.vadEnabled, true),
      chunkMs: boundedInteger(stt.chunkMs, 1_000, 300_000, 30_000),
    },
    tts: {
      streaming: booleanOr(tts.streaming, true),
      defaultModel: textOrEmpty(tts.defaultModel),
      voice: textOrEmpty(tts.voice),
      bridgeReply: booleanOr(tts.bridgeReply, false),
    },
    useMmap: booleanOr(input.useMmap, true),
    mlock: booleanOr(input.mlock, false),
    quantPreference: input.quantPreference === "quality" ? "quality" : "smallest",
    idleUnloadMs: {
      small: boundedInteger(idleUnloadMs.small, 0, 86_400_000, 300_000),
      large: boundedInteger(idleUnloadMs.large, 0, 86_400_000, 120_000),
    },
    memoryBudgetSmallMb: boundedInteger(input.memoryBudgetSmallMb, 128, 1_048_576, 1536),
    preloadSmall: booleanOr(input.preloadSmall, false),
    download: {
      concurrency: boundedInteger(download.concurrency, 1, 16, 4),
      mirrorBaseUrl: typeof download.mirrorBaseUrl === "string" ? download.mirrorBaseUrl.trim() : "",
    },
  };
}

function normalizeMaxLargeResident(value: unknown): "auto" | 1 | 2 {
  if (value === 1 || value === 2) return value;
  if (typeof value === "string" && (value === "1" || value === "2")) return Number(value) as 1 | 2;
  return "auto";
}

/**
 * 设备自检：auto 模式按物理内存决定大模型并存容量——
 * ≥32GiB 允许 2 个大模型同时驻留，否则维持 1（加载时的可用内存准入检查仍然独立生效）。
 */
export function resolveLargeResidentCapacity(maxLargeResident: "auto" | 1 | 2, totalMemoryBytes: number): 1 | 2 {
  if (maxLargeResident === 1 || maxLargeResident === 2) return maxLargeResident;
  const THIRTY_TWO_GIB = 32 * 1024 * 1024 * 1024;
  return Number.isFinite(totalMemoryBytes) && totalMemoryBytes >= THIRTY_TWO_GIB ? 2 : 1;
}

function isObject(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function booleanOr(value: unknown, fallback: boolean): boolean {
  return typeof value === "boolean" ? value : fallback;
}

function textOrEmpty(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

function boundedInteger<T>(value: unknown, min: number, max: number, fallback: T): number | T {
  return Number.isSafeInteger(value) && Number(value) >= min && Number(value) <= max ? Number(value) : fallback;
}
