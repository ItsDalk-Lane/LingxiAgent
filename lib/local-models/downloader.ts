import { createHash } from "node:crypto";
import fs from "node:fs";
import fsp from "node:fs/promises";
import path from "node:path";
import { fetch as undiciFetch } from "undici";
import { fetchDispatcherForUrl } from "../net/outbound-proxy.ts";
import { LocalModelError, localModelAbortError, throwIfAborted } from "./errors.ts";

export interface DownloadAsset {
  id: string;
  uri: string;
  bytes: number;
  sha256: string;
}

export type DownloadStatus =
  | "queued"
  | "downloading"
  | "paused"
  | "interrupted"
  | "verifying"
  | "completed"
  | "failed";

export interface DownloadProgress {
  taskId: string;
  status: DownloadStatus;
  downloadedBytes: number;
  totalBytes: number;
  bytesPerSecond: number;
  remainingMs: number | null;
}

export interface DownloadResult {
  taskId: string;
  filePath: string;
  bytes: number;
  sha256: string;
}

interface DownloadPart {
  index: number;
  start: number;
  end: number;
  received: number;
}

interface DownloadState {
  schemaVersion: 1;
  taskId: string;
  asset: DownloadAsset;
  status: DownloadStatus;
  attempt: number;
  parts: DownloadPart[];
  updatedAt: string;
  error?: string;
}

interface FetchInit extends RequestInit {
  dispatcher?: unknown;
}

export interface ResumableDownloaderOptions {
  rootDir: string;
  concurrency?: number;
  mirrorBaseUrl?: string;
  minPartBytes?: number;
  fetchImpl?: (url: string, init: FetchInit) => Promise<Response>;
  dispatcherForUrl?: (url: string) => { dispatcher: unknown | null; proxyUrl?: string };
  getFreeBytes?: (directory: string) => number | Promise<number>;
  now?: () => number;
  onProgress?: (progress: DownloadProgress) => void;
}

interface TaskControl {
  controller: AbortController;
  intent: "pause" | "cancel" | "interrupted" | null;
}

interface InFlightDownload {
  promise: Promise<DownloadResult>;
  control: TaskControl;
  waiters: number;
  settled: boolean;
}

const DEFAULT_MIN_PART_BYTES = 8 * 1024 * 1024;

export class ResumableDownloader {
  private readonly rootDir: string;
  private readonly concurrency: number;
  private readonly mirrorBaseUrl: string;
  private readonly minPartBytes: number;
  private readonly fetchImpl: (url: string, init: FetchInit) => Promise<Response>;
  private readonly dispatcherForUrl: (url: string) => { dispatcher: unknown | null; proxyUrl?: string };
  private readonly getFreeBytes: (directory: string) => number | Promise<number>;
  private readonly now: () => number;
  private readonly onProgress: (progress: DownloadProgress) => void;
  private readonly inFlight = new Map<string, InFlightDownload>();
  private readonly controls = new Map<string, TaskControl>();
  private readonly stateWriteQueues = new Map<string, Promise<void>>();

  constructor(options: ResumableDownloaderOptions) {
    this.rootDir = path.resolve(options.rootDir);
    this.concurrency = boundedInteger(options.concurrency, 1, 16, 4);
    this.mirrorBaseUrl = options.mirrorBaseUrl?.trim() || "";
    this.minPartBytes = boundedInteger(options.minPartBytes, 64 * 1024, 1024 * 1024 * 1024, DEFAULT_MIN_PART_BYTES);
    // npm undici 与 Node 全局 fetch 的 Response 类型定义来自不同声明包，运行时
    // 契约相同；先经 unknown 收口，避免把不安全的 any 暴露到下载器公共接口。
    this.fetchImpl = options.fetchImpl ?? ((url, init) =>
      undiciFetch(url, init as Parameters<typeof undiciFetch>[1]) as unknown as Promise<Response>);
    this.dispatcherForUrl = options.dispatcherForUrl ?? fetchDispatcherForUrl;
    this.getFreeBytes = options.getFreeBytes ?? defaultFreeBytes;
    this.now = options.now ?? Date.now;
    this.onProgress = options.onProgress ?? (() => {});
  }

  async download(asset: DownloadAsset, options: { signal: AbortSignal }): Promise<DownloadResult> {
    const normalized = validateAsset(asset);
    const taskId = taskIdFor(normalized);
    const existing = this.inFlight.get(taskId);
    if (existing) return waitForDownload(existing, options.signal);
    throwIfAborted(options.signal);
    const control: TaskControl = { controller: new AbortController(), intent: null };
    this.controls.set(taskId, control);
    const task: InFlightDownload = {
      promise: Promise.resolve(null as never),
      control,
      waiters: 0,
      settled: false,
    };
    task.promise = this.run(normalized, taskId, control)
      .finally(() => {
        task.settled = true;
        this.inFlight.delete(taskId);
        this.controls.delete(taskId);
      });
    this.inFlight.set(taskId, task);
    return waitForDownload(task, options.signal);
  }

  pause(taskId: string): boolean {
    const control = this.controls.get(taskId);
    if (!control) return false;
    control.intent = "pause";
    control.controller.abort();
    return true;
  }

  async cancel(taskId: string): Promise<boolean> {
    const control = this.controls.get(taskId);
    if (control) {
      control.intent = "cancel";
      control.controller.abort();
      await this.inFlight.get(taskId)?.promise.catch(() => {});
      return true;
    }
    const taskDir = this.taskDir(taskId);
    const exists = await fsp.lstat(taskDir).then(() => true, () => false);
    if (exists) await fsp.rm(taskDir, { recursive: true, force: true });
    return exists;
  }

  async listTasks(): Promise<DownloadState[]> {
    const entries = await fsp.readdir(this.rootDir, { withFileTypes: true }).catch(() => []);
    const states: DownloadState[] = [];
    for (const entry of entries) {
      if (!entry.isDirectory() || !/^[a-f0-9]{64}$/.test(entry.name)) continue;
      const state = await readState(path.join(this.rootDir, entry.name, "state.json")).catch(() => null);
      if (state) states.push(state);
    }
    return states.sort((left, right) => right.updatedAt.localeCompare(left.updatedAt));
  }

  hasActiveTasks(): boolean {
    return this.inFlight.size > 0;
  }

  private async run(asset: DownloadAsset, taskId: string, control: TaskControl): Promise<DownloadResult> {
    await fsp.mkdir(this.rootDir, { recursive: true, mode: 0o700 });
    const freeBytes = await this.getFreeBytes(this.rootDir);
    if (!Number.isFinite(freeBytes) || freeBytes < asset.bytes * 2) {
      throw new LocalModelError(
        "LOCAL_MODEL_DISK_SPACE",
        `not enough disk space to download ${asset.id}`,
        { requiredBytes: asset.bytes * 2, freeBytes },
      );
    }
    const taskDir = this.taskDir(taskId);
    await ensureOwnedTaskDirectory(this.rootDir, taskDir);
    let state = await this.loadOrCreateState(taskDir, taskId, asset, control.controller.signal);
    if (state.status === "completed") {
      return {
        taskId,
        filePath: path.join(taskDir, "artifact.bin"),
        bytes: asset.bytes,
        sha256: asset.sha256,
      };
    }
    try {
      state = await this.downloadAttempt(taskDir, state, control);
      return {
        taskId,
        filePath: path.join(taskDir, "artifact.bin"),
        bytes: asset.bytes,
        sha256: asset.sha256,
      };
    } catch (error) {
      if (control.controller.signal.aborted) {
        if (control.intent === "cancel") {
          await fsp.rm(taskDir, { recursive: true, force: true });
        } else {
          state.status = control.intent === "pause" ? "paused" : "interrupted";
          state.updatedAt = new Date(this.now()).toISOString();
          await this.persistState(taskDir, state).catch(() => {});
        }
        throw localModelAbortError(control.intent === "pause" ? "download paused" : "download cancelled");
      }
      state.status = "failed";
      state.error = error instanceof Error ? error.message : String(error);
      state.updatedAt = new Date(this.now()).toISOString();
      await this.persistState(taskDir, state).catch(() => {});
      if (error instanceof LocalModelError) throw error;
      throw new LocalModelError("LOCAL_MODEL_DOWNLOAD_NETWORK", `download failed for ${asset.id}`, {
        cause: state.error,
      });
    }
  }

  private async downloadAttempt(
    taskDir: string,
    state: DownloadState,
    control: TaskControl,
  ): Promise<DownloadState> {
    const url = resolveMirrorUrl(state.asset.uri, this.mirrorBaseUrl);
    const rangeSupported = await this.probeRangeSupport(url, state.asset.bytes, control.controller.signal);
    const desiredParts = rangeSupported
      ? Math.max(1, Math.min(this.concurrency, Math.ceil(state.asset.bytes / this.minPartBytes)))
      : 1;
    state.parts = reconcileParts(state.parts, state.asset.bytes, desiredParts, taskDir);
    state.status = "downloading";
    state.error = undefined;
    state.updatedAt = new Date(this.now()).toISOString();
    await this.persistState(taskDir, state);
    const startedAt = this.now();
    await Promise.all(state.parts.map((part) => this.downloadPart({
      taskDir,
      state,
      part,
      url,
      rangeSupported,
      signal: control.controller.signal,
      startedAt,
    })));
    throwIfAborted(control.controller.signal);
    state.status = "verifying";
    state.updatedAt = new Date(this.now()).toISOString();
    await this.persistState(taskDir, state);
    const assembled = path.join(taskDir, "artifact.tmp");
    await assembleParts(taskDir, state.parts, assembled, control.controller.signal);
    const digest = await hashFile(assembled, control.controller.signal);
    if (digest !== state.asset.sha256) {
      await fsp.rm(assembled, { force: true });
      if (state.attempt < 1) {
        state.attempt += 1;
        state.parts = [];
        await removePartFiles(taskDir);
        await this.persistState(taskDir, state);
        return this.downloadAttempt(taskDir, state, control);
      }
      await removePartFiles(taskDir);
      throw new LocalModelError(
        "LOCAL_MODEL_DOWNLOAD_INTEGRITY",
        `SHA-256 verification failed twice for ${state.asset.id}`,
        { expected: state.asset.sha256, actual: digest },
      );
    }
    const finalPath = path.join(taskDir, "artifact.bin");
    await fsp.rm(finalPath, { force: true });
    await fsp.rename(assembled, finalPath);
    await removePartFiles(taskDir);
    state.status = "completed";
    state.updatedAt = new Date(this.now()).toISOString();
    await this.persistState(taskDir, state);
    this.emitProgress(state, startedAt);
    return state;
  }

  private async probeRangeSupport(url: string, expectedBytes: number, signal: AbortSignal): Promise<boolean> {
    const response = await this.fetch(url, { method: "HEAD", signal });
    if (!response.ok && response.status !== 405 && response.status !== 501) {
      throw new Error(`HEAD ${response.status}`);
    }
    const contentLength = Number(response.headers.get("content-length"));
    if (Number.isFinite(contentLength) && contentLength > 0 && contentLength !== expectedBytes) {
      throw new LocalModelError("LOCAL_MODEL_DOWNLOAD_INTEGRITY", "download size differs from manifest", {
        expectedBytes,
        contentLength,
      });
    }
    return response.headers.get("accept-ranges")?.toLowerCase() === "bytes";
  }

  private async downloadPart(input: {
    taskDir: string;
    state: DownloadState;
    part: DownloadPart;
    url: string;
    rangeSupported: boolean;
    signal: AbortSignal;
    startedAt: number;
  }): Promise<void> {
    const partPath = path.join(input.taskDir, `part-${input.part.index}.bin`);
    input.part.received = await safeExistingPartSize(partPath, input.part.end - input.part.start + 1);
    const nextByte = input.part.start + input.part.received;
    if (nextByte > input.part.end) return;
    const headers: Record<string, string> = {};
    if (input.rangeSupported) headers.Range = `bytes=${nextByte}-${input.part.end}`;
    const response = await this.fetch(input.url, { method: "GET", headers, signal: input.signal });
    if (input.rangeSupported && response.status !== 206) throw new Error(`range request returned ${response.status}`);
    if (!input.rangeSupported && response.status !== 200) throw new Error(`download returned ${response.status}`);
    if (!response.body) throw new Error("download response has no body");
    const expectedLength = input.part.end - nextByte + 1;
    const handle = await fsp.open(partPath, input.part.received > 0 ? "a" : "w", 0o600);
    const reader = response.body.getReader();
    let written = 0;
    try {
      while (true) {
        throwIfAborted(input.signal);
        const { done, value } = await reader.read();
        if (done) break;
        if (!value) continue;
        written += value.byteLength;
        if (written > expectedLength) throw new Error("download part exceeded its declared byte range");
        await handle.write(value);
        input.part.received += value.byteLength;
        input.state.updatedAt = new Date(this.now()).toISOString();
        this.emitProgress(input.state, input.startedAt);
      }
      if (written !== expectedLength) throw new Error("download part ended before its declared byte range");
      await handle.sync();
      await this.persistState(input.taskDir, input.state);
    } finally {
      await reader.cancel().catch(() => {});
      await handle.close();
    }
  }

  private async fetch(url: string, init: FetchInit): Promise<Response> {
    const { dispatcher } = this.dispatcherForUrl(url);
    return this.fetchImpl(url, { ...init, ...(dispatcher ? { dispatcher } : {}) });
  }

  private emitProgress(state: DownloadState, startedAt: number): void {
    const downloadedBytes = state.parts.reduce((sum, part) => sum + part.received, 0);
    const elapsedSeconds = Math.max(0.001, (this.now() - startedAt) / 1000);
    const bytesPerSecond = downloadedBytes / elapsedSeconds;
    const remaining = Math.max(0, state.asset.bytes - downloadedBytes);
    this.onProgress({
      taskId: state.taskId,
      status: state.status,
      downloadedBytes,
      totalBytes: state.asset.bytes,
      bytesPerSecond,
      remainingMs: bytesPerSecond > 0 ? Math.ceil((remaining / bytesPerSecond) * 1000) : null,
    });
  }

  private async loadOrCreateState(
    taskDir: string,
    taskId: string,
    asset: DownloadAsset,
    signal: AbortSignal,
  ): Promise<DownloadState> {
    throwIfAborted(signal);
    const existing = await readState(path.join(taskDir, "state.json")).catch(() => null);
    if (existing) {
      if (existing.taskId !== taskId || JSON.stringify(existing.asset) !== JSON.stringify(asset)) {
        throw new LocalModelError("LOCAL_MODEL_DOWNLOAD_INTEGRITY", "download state does not match requested asset");
      }
      if (existing.status === "completed") {
        const finalPath = path.join(taskDir, "artifact.bin");
        const stat = await fsp.lstat(finalPath).catch(() => null);
        if (stat?.isFile() && !stat.isSymbolicLink() && stat.size === asset.bytes) {
          const digest = await hashFile(finalPath, signal);
          if (digest === asset.sha256) return existing;
        }
        await fsp.rm(finalPath, { force: true });
      }
      return existing;
    }
    const state: DownloadState = {
      schemaVersion: 1,
      taskId,
      asset,
      status: "queued",
      attempt: 0,
      parts: [],
      updatedAt: new Date(this.now()).toISOString(),
    };
    await this.persistState(taskDir, state);
    return state;
  }

  private async persistState(taskDir: string, state: DownloadState): Promise<void> {
    const previous = this.stateWriteQueues.get(taskDir) ?? Promise.resolve();
    const next = previous.catch(() => {}).then(() => writeState(taskDir, state));
    this.stateWriteQueues.set(taskDir, next);
    try {
      await next;
    } finally {
      if (this.stateWriteQueues.get(taskDir) === next) this.stateWriteQueues.delete(taskDir);
    }
  }

  private taskDir(taskId: string): string {
    if (!/^[a-f0-9]{64}$/.test(taskId)) throw new Error("invalid download task id");
    return path.join(this.rootDir, taskId);
  }
}

function validateAsset(asset: DownloadAsset): DownloadAsset {
  if (!asset || typeof asset !== "object") throw new Error("download asset is required");
  const uri = new URL(asset.uri);
  if (uri.protocol !== "https:") {
    throw new LocalModelError("LOCAL_MODEL_DOWNLOAD_NETWORK", "local model downloads require HTTPS", { uri: asset.uri });
  }
  if (!Number.isSafeInteger(asset.bytes) || asset.bytes <= 0) throw new Error("download asset bytes must be positive");
  const sha256 = String(asset.sha256 || "").toLowerCase();
  if (!/^[a-f0-9]{64}$/.test(sha256)) throw new Error("download asset SHA-256 is invalid");
  if (!/^[A-Za-z0-9][A-Za-z0-9._@+-]{0,191}$/.test(asset.id)) throw new Error("download asset id is invalid");
  return { id: asset.id, uri: uri.toString(), bytes: asset.bytes, sha256 };
}

function taskIdFor(asset: DownloadAsset): string {
  return createHash("sha256").update(`${asset.uri}\n${asset.bytes}\n${asset.sha256}`).digest("hex");
}

function resolveMirrorUrl(original: string, mirrorBaseUrl: string): string {
  if (!mirrorBaseUrl) return original;
  const source = new URL(original);
  const mirror = new URL(mirrorBaseUrl);
  if (mirror.protocol !== "https:") {
    throw new LocalModelError("LOCAL_MODEL_DOWNLOAD_NETWORK", "download mirror must use HTTPS");
  }
  const prefix = mirror.pathname.replace(/\/$/, "");
  mirror.pathname = `${prefix}/${source.hostname}${source.pathname}`.replace(/\/+/g, "/");
  mirror.search = source.search;
  mirror.hash = "";
  return mirror.toString();
}

function reconcileParts(
  existing: DownloadPart[],
  totalBytes: number,
  count: number,
  taskDir: string,
): DownloadPart[] {
  const desired = createParts(totalBytes, count);
  if (samePartLayout(existing, desired)) return existing;
  for (const entry of fs.readdirSync(taskDir, { withFileTypes: true })) {
    if (entry.isFile() && /^part-\d+[.]bin$/.test(entry.name)) fs.rmSync(path.join(taskDir, entry.name));
  }
  return desired;
}

function createParts(totalBytes: number, count: number): DownloadPart[] {
  const partSize = Math.ceil(totalBytes / count);
  const parts: DownloadPart[] = [];
  for (let index = 0; index < count; index += 1) {
    const start = index * partSize;
    if (start >= totalBytes) break;
    parts.push({ index, start, end: Math.min(totalBytes - 1, start + partSize - 1), received: 0 });
  }
  return parts;
}

function samePartLayout(left: DownloadPart[], right: DownloadPart[]): boolean {
  return left.length === right.length && left.every((part, index) =>
    part.index === right[index].index && part.start === right[index].start && part.end === right[index].end);
}

async function safeExistingPartSize(partPath: string, maxBytes: number): Promise<number> {
  const stat = await fsp.lstat(partPath).catch(() => null);
  if (!stat) return 0;
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size > maxBytes) {
    throw new LocalModelError("LOCAL_MODEL_DOWNLOAD_INTEGRITY", "unsafe or oversized partial download file");
  }
  return stat.size;
}

async function assembleParts(
  taskDir: string,
  parts: DownloadPart[],
  destination: string,
  signal: AbortSignal,
): Promise<void> {
  const handle = await fsp.open(destination, "w", 0o600);
  try {
    for (const part of [...parts].sort((a, b) => a.index - b.index)) {
      for await (const chunk of fs.createReadStream(path.join(taskDir, `part-${part.index}.bin`))) {
        throwIfAborted(signal);
        await handle.write(chunk as Buffer);
      }
    }
    await handle.sync();
  } finally {
    await handle.close();
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

async function removePartFiles(taskDir: string): Promise<void> {
  const entries = await fsp.readdir(taskDir, { withFileTypes: true }).catch(() => []);
  await Promise.all(entries
    .filter((entry) => entry.isFile() && /^part-\d+[.]bin$/.test(entry.name))
    .map((entry) => fsp.rm(path.join(taskDir, entry.name), { force: true })));
}

async function ensureOwnedTaskDirectory(rootDir: string, taskDir: string): Promise<void> {
  if (path.dirname(taskDir) !== rootDir) throw new Error("download task escaped its root");
  const stat = await fsp.lstat(taskDir).catch(() => null);
  if (stat?.isSymbolicLink() || (stat && !stat.isDirectory())) {
    throw new LocalModelError("LOCAL_MODEL_DOWNLOAD_INTEGRITY", "download task path is unsafe");
  }
  await fsp.mkdir(taskDir, { recursive: true, mode: 0o700 });
}

async function readState(filePath: string): Promise<DownloadState> {
  const stat = await fsp.lstat(filePath);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size > 1024 * 1024) throw new Error("unsafe download state file");
  const value = JSON.parse(await fsp.readFile(filePath, "utf8")) as DownloadState;
  if (value.schemaVersion !== 1 || !/^[a-f0-9]{64}$/.test(value.taskId) || !Array.isArray(value.parts)) {
    throw new Error("invalid download state file");
  }
  return value;
}

async function writeState(taskDir: string, state: DownloadState): Promise<void> {
  const target = path.join(taskDir, "state.json");
  const temporary = path.join(taskDir, "state.json.tmp");
  await fsp.writeFile(temporary, `${JSON.stringify(state, null, 2)}\n`, { mode: 0o600 });
  await fsp.rename(temporary, target);
}

async function defaultFreeBytes(directory: string): Promise<number> {
  const stat = await fsp.statfs(directory);
  return Number(stat.bavail) * Number(stat.bsize);
}

function boundedInteger(value: unknown, min: number, max: number, fallback: number): number {
  return Number.isSafeInteger(value) && Number(value) >= min && Number(value) <= max ? Number(value) : fallback;
}

function waitForDownload(task: InFlightDownload, signal: AbortSignal): Promise<DownloadResult> {
  throwIfAborted(signal);
  task.waiters += 1;
  return new Promise<DownloadResult>((resolve, reject) => {
    let completed = false;
    const finishWaiter = () => {
      if (completed) return;
      completed = true;
      task.waiters = Math.max(0, task.waiters - 1);
    };
    const onAbort = () => {
      finishWaiter();
      if (task.waiters === 0 && !task.settled && task.control.intent === null) {
        task.control.intent = "interrupted";
        task.control.controller.abort();
      }
      reject(localModelAbortError());
    };
    signal.addEventListener("abort", onAbort, { once: true });
    task.promise.then(
      (value) => {
        signal.removeEventListener("abort", onAbort);
        finishWaiter();
        resolve(value);
      },
      (error) => {
        signal.removeEventListener("abort", onAbort);
        finishWaiter();
        reject(error);
      },
    );
  });
}
