import fs from "fs";
import path from "path";
import { atomicWriteSync } from "../../shared/safe-fs.ts";
import { normalizeLlmUsage } from "./usage-observer.ts";
import {
  attributionSessionId,
  attributionSessionPath,
  isUnknownUsageContext as isUnknownUsageContextValue,
  normalizeUsageContext,
} from "./usage-context.ts";

const DEFAULT_MAX_ENTRIES = 5_000;
const STORAGE_VERSION = 1;

type UsageLedgerEventBus = {
  emit(event: { type: "llm_usage"; entry: Record<string, any> }, sessionPath?: string | null): void;
};

type UsageLedgerLogger = { warn?(message: string): void };

type UsageLedgerOptions = {
  maxEntries?: number;
  eventBus?: UsageLedgerEventBus | null;
  logger?: UsageLedgerLogger | null;
  now?: () => number;
  requestIdFactory?: (() => unknown) | null;
  storagePath?: string | null;
};

export function createUsageLedger({
  maxEntries = DEFAULT_MAX_ENTRIES,
  eventBus = null,
  logger = null,
  now = () => Date.now(),
  requestIdFactory = null,
  storagePath = null,
}: UsageLedgerOptions = {}) {
  const entries = loadPersistedEntries({ storagePath, maxEntries, logger, now });
  const pending = new Map<string, Record<string, any>>();
  let sequence = 0;

  const nextRequestId = () => {
    if (typeof requestIdFactory === "function") return String(requestIdFactory());
    sequence += 1;
    return `llm_${now().toString(36)}_${sequence.toString(36)}`;
  };

  const append = (entry: Record<string, any>) => {
    const normalizedEntry = normalizeEntry(entry, now);
    if (entryHasUnknownUsageContext(normalizedEntry)) {
      warn(logger, `unknown usage context for LLM request ${normalizedEntry.requestId}`);
    }
    entries.push(normalizedEntry);
    while (entries.length > maxEntries) entries.shift();
    persistEntries({ storagePath, entries, logger });
    emit(eventBus, normalizedEntry);
    return normalizedEntry;
  };

  return {
    start(meta: Record<string, any> = {}) {
      const requestId = meta.requestId ? String(meta.requestId) : nextRequestId();
      const startedMs = now();
      const usageContext = normalizeUsageContext(meta.usageContext);
      const pendingEntry = {
        requestId,
        startedMs,
        startedAt: toIso(startedMs),
        source: usageContext.source,
        attribution: usageContext.attribution,
        metadata: isPlainObject(meta.metadata) ? { ...meta.metadata } : null,
        model: normalizeModel(meta.model),
        costRates: meta.costRates ?? null,
      };
      if (entryHasUnknownUsageContext(pendingEntry)) {
        warn(logger, `unknown usage context for LLM request ${requestId}`);
      }
      pending.set(requestId, pendingEntry);
      return { requestId, startedAt: pendingEntry.startedAt };
    },

    finish(requestId: any, result: Record<string, any> = {}) {
      const pendingEntry = pending.get(requestId);
      if (!pendingEntry) return null;
      pending.delete(requestId);
      const endedMs = now();
      const usage = normalizeUsage(result.usage, {
        costRates: result.costRates ?? pendingEntry.costRates,
        cacheSupport: result.cacheSupport,
      });
      return append({
        schemaVersion: 1,
        requestId,
        startedAt: pendingEntry.startedAt,
        endedAt: toIso(endedMs),
        durationMs: Math.max(0, endedMs - pendingEntry.startedMs),
        status: usage ? "ok" : "usage_missing",
        source: pendingEntry.source,
        attribution: pendingEntry.attribution,
        metadata: pendingEntry.metadata,
        model: normalizeModel(result.model ?? pendingEntry.model),
        usage,
        rawUsageShape: rawUsageShape(result.usage),
        error: null,
      });
    },

    recordError(requestId: any, error: any, status = "error", result: Record<string, any> = {}) {
      const pendingEntry = pending.get(requestId);
      if (!pendingEntry) return null;
      pending.delete(requestId);
      const endedMs = now();
      const usage = normalizeUsage(result.usage, {
        costRates: result.costRates ?? pendingEntry.costRates,
        cacheSupport: result.cacheSupport,
      });
      return append({
        schemaVersion: 1,
        requestId,
        startedAt: pendingEntry.startedAt,
        endedAt: toIso(endedMs),
        durationMs: Math.max(0, endedMs - pendingEntry.startedMs),
        status: status === "aborted" ? "aborted" : "error",
        source: pendingEntry.source,
        attribution: pendingEntry.attribution,
        metadata: pendingEntry.metadata,
        model: pendingEntry.model,
        usage,
        rawUsageShape: rawUsageShape(result.usage),
        error: normalizeError(error),
      });
    },

    record(meta: Record<string, any> = {}) {
      const request = this.start(meta);
      return this.finish(request.requestId, {
        usage: meta.usage,
        model: meta.model,
        costRates: meta.costRates,
        cacheSupport: meta.cacheSupport,
      });
    },

    list(filter: Record<string, any> = {}) {
      const limit = normalizeLimit(filter.limit);
      const filtered = entries.filter(entry => matchesFilter(entry, filter));
      const limited = limit ? filtered.slice(Math.max(0, filtered.length - limit)) : filtered;
      return {
        entries: limited.map(clone),
        nextCursor: null,
      };
    },

    clear() {
      entries.length = 0;
      pending.clear();
      persistEntries({ storagePath, entries, logger });
    },
  };
}

function loadPersistedEntries({
  storagePath,
  maxEntries,
  logger,
  now,
}: {
  storagePath: string | null;
  maxEntries: number;
  logger: UsageLedgerLogger | null;
  now: () => number;
}): Array<Record<string, any>> {
  if (!storagePath) return [];
  try {
    const raw = JSON.parse(fs.readFileSync(storagePath, "utf-8"));
    const rawEntries = Array.isArray(raw?.entries) ? raw.entries : [];
    return rawEntries
      .map((entry: any) => normalizeEntry(entry, now))
      .filter((entry: Record<string, any>) => entry.requestId)
      .slice(-maxEntries);
  } catch (err: unknown) {
    const fsError = err as NodeJS.ErrnoException;
    if (fsError.code === "ENOENT") return [];
    warn(logger, `failed to read usage ledger storage ${storagePath}: ${fsError.message}`);
    return [];
  }
}

function persistEntries({
  storagePath,
  entries,
  logger,
}: {
  storagePath: string | null;
  entries: Array<Record<string, any>>;
  logger: UsageLedgerLogger | null;
}): void {
  if (!storagePath) return;
  try {
    fs.mkdirSync(path.dirname(storagePath), { recursive: true });
    atomicWriteSync(storagePath, `${JSON.stringify({
      version: STORAGE_VERSION,
      entries,
    }, null, 2)}\n`);
  } catch (err: unknown) {
    const fsError = err as NodeJS.ErrnoException;
    warn(logger, `failed to write usage ledger storage ${storagePath}: ${fsError.message}`);
  }
}

function normalizeEntry(entry: Record<string, any>, now: () => number): Record<string, any> {
  const usageContext = normalizeUsageContext({
    source: entry.source,
    attribution: entry.attribution,
  });
  const startedAt = typeof entry.startedAt === "string" ? entry.startedAt : toIso(now());
  const endedAt = entry.endedAt === null || typeof entry.endedAt === "string" ? entry.endedAt : toIso(now());
  return {
    schemaVersion: 1,
    requestId: String(entry.requestId || ""),
    startedAt,
    endedAt,
    durationMs: numberOrNull(entry.durationMs),
    status: normalizeStatus(entry.status),
    source: usageContext.source,
    attribution: usageContext.attribution,
    metadata: isPlainObject(entry.metadata) ? { ...entry.metadata } : null,
    model: normalizeModel(entry.model),
    usage: entry.usage ?? null,
    rawUsageShape: typeof entry.rawUsageShape === "string" ? entry.rawUsageShape : null,
    error: entry.error ?? null,
  };
}

function normalizeUsage(usage: any, options: Record<string, any>): any {
  if (!usage) return null;
  if (usage.input && usage.output && usage.cache && Object.prototype.hasOwnProperty.call(usage, "totalTokens")) {
    return usage;
  }
  return normalizeLlmUsage(usage, options);
}

function normalizeModel(model: Record<string, any> = {}) {
  return {
    provider: textOrNull(model?.provider),
    modelId: textOrNull(model?.modelId ?? model?.id),
    api: textOrNull(model?.api),
  };
}

function normalizeError(error: any): { name: string | null; message: string | null } {
  if (!error) return { name: null, message: null };
  return {
    name: typeof error.name === "string" ? error.name : null,
    message: typeof error.message === "string" ? error.message : String(error),
  };
}

function normalizeStatus(status: unknown): "ok" | "error" | "aborted" | "usage_missing" {
  if (status === "ok" || status === "error" || status === "aborted" || status === "usage_missing") {
    return status;
  }
  return "usage_missing";
}

function rawUsageShape(usage: unknown): string | null {
  if (!usage || typeof usage !== "object" || Array.isArray(usage)) return null;
  return Object.keys(usage).sort().join(",");
}

function isPlainObject(value: unknown): value is Record<string, any> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function matchesFilter(entry: Record<string, any>, filter: Record<string, any>): boolean {
  if (filter.since && entry.endedAt && entry.endedAt < filter.since) return false;
  if (filter.until && entry.startedAt && entry.startedAt > filter.until) return false;
  if (filter.status && entry.status !== filter.status) return false;
  if (filter.attributionKind && entry.attribution?.kind !== filter.attributionKind) return false;
  if (filter.sessionId && attributionSessionId(entry.attribution) !== filter.sessionId) return false;
  if (filter.sessionPath && attributionSessionPath(entry.attribution) !== filter.sessionPath) return false;
  if (filter.childSessionId && entry.attribution?.childSessionId !== filter.childSessionId) return false;
  if (filter.childSessionPath && entry.attribution?.childSessionPath !== filter.childSessionPath) return false;
  if (filter.agentId && entry.attribution?.agentId !== filter.agentId) return false;
  if (filter.subsystem && entry.source?.subsystem !== filter.subsystem) return false;
  if (filter.operation && entry.source?.operation !== filter.operation) return false;
  if (filter.modelId && entry.model?.modelId !== filter.modelId) return false;
  if (filter.provider && entry.model?.provider !== filter.provider) return false;
  return true;
}

function normalizeLimit(limit: unknown): number | null {
  const n = Number(limit);
  if (!Number.isFinite(n) || n <= 0) return null;
  return Math.floor(n);
}

function emit(eventBus: UsageLedgerEventBus | null, entry: Record<string, any>): void {
  if (!eventBus || typeof eventBus.emit !== "function") return;
  try {
    eventBus.emit({ type: "llm_usage", entry }, attributionSessionPath(entry.attribution));
  } catch {
    // Usage observation must not break the model request path.
  }
}

function warn(logger: UsageLedgerLogger | null, message: string): void {
  try {
    logger?.warn?.(message);
  } catch {
    // Diagnostics should never affect request accounting.
  }
}

function entryHasUnknownUsageContext(entry: Record<string, any>): boolean {
  return isUnknownUsageContextValue({
    source: entry.source,
    attribution: entry.attribution,
  });
}

function clone<T>(value: T): T {
  return typeof structuredClone === "function"
    ? structuredClone(value)
    : JSON.parse(JSON.stringify(value));
}

function toIso(ms: number): string {
  return new Date(ms).toISOString();
}

function textOrNull(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function numberOrNull(value: unknown): number | null {
  const n = Number(value);
  return Number.isFinite(n) ? n : null;
}
