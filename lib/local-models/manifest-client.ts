import fsp from "node:fs/promises";
import path from "node:path";
import { fetch as undiciFetch } from "undici";
import { fetchDispatcherForUrl } from "../net/outbound-proxy.ts";
import {
  BUILTIN_LOCAL_MODEL_CATALOG,
  type LocalModelCatalogEntry,
} from "./catalog.ts";
import { LocalModelError, throwIfAborted } from "./errors.ts";
import { parseLocalModelsManifest, type LocalModelsManifest } from "./manifest.ts";

interface ManifestFetchInit extends RequestInit {
  dispatcher?: unknown;
}

interface ManifestCacheFile {
  schemaVersion: 1;
  url: string;
  etag: string | null;
  fetchedAt: string;
  body: string;
}

export interface LocalModelsManifestResolution {
  manifest: LocalModelsManifest | null;
  catalog: readonly LocalModelCatalogEntry[];
  source: "remote" | "cache" | "builtin";
  etag: string | null;
  warning: string | null;
}

export interface LocalModelsManifestClientOptions {
  url: string;
  cacheDir: string;
  fetchImpl?: (url: string, init: ManifestFetchInit) => Promise<Response>;
  dispatcherForUrl?: (url: string) => { dispatcher: unknown | null; proxyUrl?: string };
  maxBytes?: number;
  now?: () => number;
  builtinCatalog?: readonly LocalModelCatalogEntry[];
}

export class LocalModelsManifestClient {
  private readonly url: string;
  private readonly cacheDir: string;
  private readonly fetchImpl: (url: string, init: ManifestFetchInit) => Promise<Response>;
  private readonly dispatcherForUrl: (url: string) => { dispatcher: unknown | null; proxyUrl?: string };
  private readonly maxBytes: number;
  private readonly now: () => number;
  private readonly builtinCatalog: readonly LocalModelCatalogEntry[];

  constructor(options: LocalModelsManifestClientOptions) {
    const parsed = new URL(options.url);
    if (parsed.protocol !== "https:") {
      throw new LocalModelError("LOCAL_MODEL_MANIFEST_INVALID", "local model manifest URL must use HTTPS");
    }
    this.url = parsed.toString();
    this.cacheDir = path.resolve(options.cacheDir);
    this.fetchImpl = options.fetchImpl ?? ((url, init) =>
      undiciFetch(url, init as Parameters<typeof undiciFetch>[1]) as unknown as Promise<Response>);
    this.dispatcherForUrl = options.dispatcherForUrl ?? fetchDispatcherForUrl;
    this.maxBytes = boundedInteger(options.maxBytes, 1024, 20 * 1024 * 1024, 5 * 1024 * 1024);
    this.now = options.now ?? Date.now;
    this.builtinCatalog = options.builtinCatalog ?? BUILTIN_LOCAL_MODEL_CATALOG;
  }

  async refresh(options: { signal: AbortSignal }): Promise<LocalModelsManifestResolution> {
    throwIfAborted(options.signal);
    await ensureSafeDirectory(this.cacheDir);
    const cached = await this.readCache().catch(() => null);
    try {
      const headers: Record<string, string> = { accept: "application/json", "cache-control": "no-cache" };
      if (cached?.etag) headers["if-none-match"] = cached.etag;
      const { dispatcher } = this.dispatcherForUrl(this.url);
      const response = await this.fetchImpl(this.url, {
        method: "GET",
        headers,
        signal: options.signal,
        ...(dispatcher ? { dispatcher } : {}),
      });
      if (response.status === 304) {
        if (!cached) throw new Error("manifest server returned 304 without a local cache");
        const manifest = parseLocalModelsManifest(JSON.parse(cached.body) as unknown);
        return this.resolution(manifest, "cache", cached.etag, null);
      }
      if (!response.ok) throw new Error(`manifest request returned HTTP ${response.status}`);
      const body = await readBoundedBody(response, this.maxBytes, options.signal);
      const manifest = parseLocalModelsManifest(JSON.parse(body) as unknown);
      const etag = normalizeEtag(response.headers.get("etag"));
      await this.writeCache({
        schemaVersion: 1,
        url: this.url,
        etag,
        fetchedAt: new Date(this.now()).toISOString(),
        body,
      });
      return this.resolution(manifest, "remote", etag, null);
    } catch (error) {
      if (options.signal.aborted) throw error;
      if (cached) {
        try {
          const manifest = parseLocalModelsManifest(JSON.parse(cached.body) as unknown);
          return this.resolution(manifest, "cache", cached.etag, safeErrorMessage(error));
        } catch {}
      }
      return {
        manifest: null,
        catalog: this.builtinCatalog,
        source: "builtin",
        etag: null,
        warning: safeErrorMessage(error),
      };
    }
  }

  async loadCached(): Promise<LocalModelsManifestResolution> {
    const cached = await this.readCache().catch(() => null);
    if (!cached) {
      return {
        manifest: null,
        catalog: this.builtinCatalog,
        source: "builtin",
        etag: null,
        warning: "no cached local model manifest is available",
      };
    }
    try {
      const manifest = parseLocalModelsManifest(JSON.parse(cached.body) as unknown);
      return this.resolution(manifest, "cache", cached.etag, null);
    } catch (error) {
      return {
        manifest: null,
        catalog: this.builtinCatalog,
        source: "builtin",
        etag: null,
        warning: safeErrorMessage(error),
      };
    }
  }

  private resolution(
    manifest: LocalModelsManifest,
    source: "remote" | "cache",
    etag: string | null,
    warning: string | null,
  ): LocalModelsManifestResolution {
    const available = new Set(manifest.models.map((entry) => entry.id));
    const catalog = this.builtinCatalog.map((entry) => Object.freeze({
      ...entry,
      distributionStatus: available.has(entry.id) ? "manifest-available" as const : "catalog-only" as const,
    }));
    return { manifest, catalog, source, etag, warning };
  }

  private async readCache(): Promise<ManifestCacheFile> {
    const file = path.join(this.cacheDir, "manifest-cache.json");
    const stat = await fsp.lstat(file);
    if (!stat.isFile() || stat.isSymbolicLink() || stat.size > this.maxBytes + 1024 * 1024) {
      throw new Error("unsafe local model manifest cache");
    }
    const value = JSON.parse(await fsp.readFile(file, "utf8")) as ManifestCacheFile;
    if (value.schemaVersion !== 1 || value.url !== this.url || typeof value.body !== "string"
      || (value.etag !== null && typeof value.etag !== "string") || !Number.isFinite(Date.parse(value.fetchedAt))) {
      throw new Error("invalid local model manifest cache");
    }
    if (Buffer.byteLength(value.body, "utf8") > this.maxBytes) throw new Error("cached manifest is too large");
    return value;
  }

  private async writeCache(cache: ManifestCacheFile): Promise<void> {
    const target = path.join(this.cacheDir, "manifest-cache.json");
    const temporary = path.join(this.cacheDir, "manifest-cache.json.tmp");
    await fsp.writeFile(temporary, `${JSON.stringify(cache)}\n`, { mode: 0o600 });
    await fsp.rename(temporary, target);
  }
}

async function readBoundedBody(response: Response, maxBytes: number, signal: AbortSignal): Promise<string> {
  const declared = Number(response.headers.get("content-length"));
  if (Number.isFinite(declared) && declared > maxBytes) {
    throw new LocalModelError("LOCAL_MODEL_MANIFEST_INVALID", "local model manifest exceeds the size limit");
  }
  if (!response.body) throw new Error("manifest response has no body");
  const chunks: Uint8Array[] = [];
  let bytes = 0;
  const reader = response.body.getReader();
  try {
    while (true) {
      throwIfAborted(signal);
      const { done, value } = await reader.read();
      if (done) break;
      if (!value) continue;
      bytes += value.byteLength;
      if (bytes > maxBytes) {
        throw new LocalModelError("LOCAL_MODEL_MANIFEST_INVALID", "local model manifest exceeds the size limit");
      }
      chunks.push(value);
    }
  } finally {
    await reader.cancel().catch(() => {});
  }
  const output = new Uint8Array(bytes);
  let offset = 0;
  for (const chunk of chunks) {
    output.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return new TextDecoder("utf-8", { fatal: true }).decode(output);
}

function normalizeEtag(value: string | null): string | null {
  if (!value || value.length > 512 || /[\r\n]/.test(value)) return null;
  return value;
}

async function ensureSafeDirectory(directory: string): Promise<void> {
  await fsp.mkdir(directory, { recursive: true, mode: 0o700 });
  const stat = await fsp.lstat(directory);
  if (!stat.isDirectory() || stat.isSymbolicLink()) {
    throw new LocalModelError("LOCAL_MODEL_MANIFEST_INVALID", "manifest cache path is not a safe directory");
  }
}

function safeErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function boundedInteger(value: unknown, min: number, max: number, fallback: number): number {
  return Number.isSafeInteger(value) && Number(value) >= min && Number(value) <= max ? Number(value) : fallback;
}
