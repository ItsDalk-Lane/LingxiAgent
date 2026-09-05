import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { Worker } from "node:worker_threads";
import { AnnIndexStore, KNOWLEDGE_ANN_INDEX_FORMAT_VERSION, knowledgeAnnFileName } from "./ann-index-store.ts";
import { KnowledgeError } from "./errors.ts";
import { recordPortableVectorFallback, type KnowledgeVectorSearchBackend } from "./vector-search-backend.ts";
import type { PortableVectorIndexAdapter, VectorSearchResult } from "./vector-index-adapter.ts";

const require = createRequire(import.meta.url);
export const KNOWLEDGE_ANN_MAX_LOADED_INDEXES = 32;
export const KNOWLEDGE_ANN_MAX_LOADED_BYTES = 512 * 1024 * 1024;
export const KNOWLEDGE_ANN_INDEX_OPTIONS = {
  metric: "cos", quantization: "f32", connectivity: 16, expansion_add: 128, expansion_search: 64, multi: false,
} as const;

export interface KnowledgeNativeIndex {
  load(fileName: string): void;
  size(): number;
  dimensions(): number;
  search(vector: Float32Array, limit: number, threads: number): { keys: BigUint64Array; distances: Float32Array };
}
export interface KnowledgeNativeModule {
  Index: new (options: typeof KNOWLEDGE_ANN_INDEX_OPTIONS & { dimensions: number }) => KnowledgeNativeIndex;
  modulePath: string;
}
export function loadKnowledgeUseArch(): KnowledgeNativeModule {
  try { return { Index: require("usearch").Index, modulePath: require.resolve("usearch") }; }
  catch { throw new Error("ANN_NATIVE_UNAVAILABLE"); }
}

// 原生建图是同步计算，放在独立线程；参数只通过结构化消息传入，不拼接执行代码。
const BUILD_WORKER = `
const { parentPort, workerData } = require('node:worker_threads');
const fs = require('node:fs');
const { Index } = require(workerData.modulePath);
let index = new Index(workerData.options);
parentPort.on('message', message => {
  try {
    if (message.type === 'add') {
      index.add(message.keys, message.vectors, 1);
      parentPort.postMessage({ type: 'added' });
    } else if (message.type === 'save') {
      index.save(workerData.temporaryFile);
      // Windows 刷盘要求可写句柄；r+ 保留刚保存的索引内容。
      const fd = fs.openSync(workerData.temporaryFile, 'r+');
      try { fs.fsyncSync(fd); } finally { fs.closeSync(fd); }
      parentPort.postMessage({ type: 'saved', count: index.size() });
      index = null;
      parentPort.close();
    }
  } catch (error) {
    parentPort.postMessage({ type: 'failed', message: String(error?.message || error) });
    parentPort.close();
  }
});
parentPort.postMessage({ type: 'ready' });
`;

/** 校验锁定版本的文件边界；原生读取器会先按文件内尺寸分配内存，不能直接喂畸形文件。 */
function validateNativeFile(fileName: string, count: number, dimensions: number, fileSize: number): void {
  const fail = (): never => { throw new Error("ANN_FILE_CORRUPT"); };
  const fd = fs.openSync(fileName, "r");
  try {
    const headOffset = 8 + count * dimensions * 4;
    if (!Number.isSafeInteger(headOffset) || headOffset + 104 + count * 2 > fileSize) fail();
    const matrix = Buffer.alloc(8), header = Buffer.alloc(104);
    if (fs.readSync(fd, matrix, 0, 8, 0) !== 8 || matrix.readUInt32LE(0) !== count || matrix.readUInt32LE(4) !== dimensions * 4) fail();
    if (fs.readSync(fd, header, 0, 104, headOffset) !== 104) fail();
    if (header.toString("ascii", 0, 7) !== "usearch" || header.readUInt16LE(7) !== 2
      || header.readUInt16LE(9) !== 26 || header.readUInt16LE(11) !== 0
      || header[13] !== 99 || header[14] !== 11 || header[15] !== 14 || header[16] !== 15
      || header.readBigUInt64LE(17) !== BigInt(count) || header.readBigUInt64LE(25) !== 0n
      || header.readBigUInt64LE(33) !== BigInt(dimensions) || header[41] !== 0
      || header.readBigUInt64LE(64) !== BigInt(count) || header.readBigUInt64LE(72) !== 16n
      || header.readBigUInt64LE(80) !== 32n) fail();
    const maxLevel = Number(header.readBigUInt64LE(88)), entry = Number(header.readBigUInt64LE(96));
    if (maxLevel > 32767 || entry >= count) fail();
    const levels = Buffer.alloc(count * 2);
    if (fs.readSync(fd, levels, 0, levels.length, headOffset + 104) !== levels.length) fail();
    let graphBytes = 0, actualMax = 0;
    for (let i = 0; i < count; i++) {
      const level = levels.readInt16LE(i * 2);
      if (level < 0 || level > maxLevel) fail();
      actualMax = Math.max(actualMax, level); graphBytes += 142 + 68 * level;
    }
    if (actualMax !== maxLevel || levels.readInt16LE(entry * 2) !== maxLevel
      || headOffset + 104 + levels.length + graphBytes !== fileSize) fail();
    // 只读图结构，不复制整个向量矩阵；逐节点验证键、层数和邻居边界。
    let position = headOffset + 104 + levels.length;
    for (let i = 0; i < count; i++) {
      const level = levels.readInt16LE(i * 2), node = Buffer.alloc(142 + 68 * level);
      if (fs.readSync(fd, node, 0, node.length, position) !== node.length
        || node.readBigUInt64LE(0) !== BigInt(i + 1) || node.readInt16LE(8) !== level) fail();
      let offset = 10;
      for (let layer = 0; layer <= level; layer++) {
        const capacity = layer === 0 ? 32 : 16, neighbors = node.readUInt32LE(offset);
        if (neighbors > capacity) fail();
        for (let neighbor = 0; neighbor < neighbors; neighbor++) {
          const slot = node.readUInt32LE(offset + 4 + neighbor * 4);
          if (slot >= count || levels.readInt16LE(slot * 2) < layer) fail();
        }
        offset += 4 + capacity * 4;
      }
      position += node.length;
    }
  } finally { fs.closeSync(fd); }
}

interface BuildJob { id: string; cancelled: boolean; worker?: Worker; exited?: Promise<number> }
interface LoadedIndex { index: KnowledgeNativeIndex; signature: string; estimatedBytes: number }

/** ANN 只加速查询，持久向量和身份验证始终由原数据库负责。 */
export class UseArchVectorBackend implements KnowledgeVectorSearchBackend {
  readonly kind = "hnsw" as const;
  private readonly portable: PortableVectorIndexAdapter;
  private readonly store: AnnIndexStore;
  private readonly root: string;
  private readonly loadNative: () => KnowledgeNativeModule;
  private readonly log: (message: string) => void;
  private readonly loaded = new Map<string, LoadedIndex>();
  private readonly pending = new Set<string>();
  private readonly failedBuilds = new Map<string, string>();
  private readonly idleWaiters: Array<() => void> = [];
  private wake: ReturnType<typeof setImmediate> | null = null;
  private active: BuildJob | null = null;
  private activePromise: Promise<void> | null = null;
  private closed = false;
  private closing: Promise<void> | null = null;
  private readonly recovery: Promise<void>;

  constructor(options: { portable: PortableVectorIndexAdapter; store: AnnIndexStore; root: string;
    loadNative?: () => KnowledgeNativeModule; log?: (message: string) => void }) {
    if (!path.isAbsolute(options.root)) throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "ANN root must be absolute");
    this.portable = options.portable; this.store = options.store; this.root = options.root;
    this.loadNative = options.loadNative ?? loadKnowledgeUseArch; this.log = options.log ?? (() => {});
    fs.mkdirSync(this.root, { recursive: true, mode: 0o700 });
    // 刚打开就关闭时不发起目录读取，避免 Windows 上尚未结束的读取占用目录。
    this.recovery = Promise.resolve().then(() => { if (!this.closed) return this.recover(); })
      .catch(() => { this.log("knowledge ANN: startup recovery failed"); });
    void this.recovery.then(() => this.queuePump());
  }

  get cacheStats() {
    return { indexes: this.loaded.size, estimatedBytes: [...this.loaded.values()].reduce((sum, item) => sum + item.estimatedBytes, 0) };
  }

  private async recover(): Promise<void> {
    // 只清理由本模块命名的临时文件，不跟随链接，不扫描知识原文目录。
    const directories = await fs.promises.readdir(this.root, { withFileTypes: true });
    for (const directory of directories) {
      if (this.closed) return;
      if (!directory.isDirectory() || !/^[a-zA-Z0-9_-]{1,16}$/.test(directory.name)) continue;
      const files = await fs.promises.readdir(path.join(this.root, directory.name), { withFileTypes: true });
      for (const file of files) {
        if (this.closed) return;
        if (file.isFile() && /^viv_[a-f0-9]{32}\.usearch\.tmp$/.test(file.name)) {
          await fs.promises.unlink(path.join(this.root, directory.name, file.name));
        }
      }
    }
    if (this.closed) return;
    for (const id of this.store.listInterrupted()) { this.store.markFailed(id); this.scheduleBuild(id); }
    let cursor = "";
    while (!this.closed) {
      const ids = this.portable.listReadyVectorVariantIds(cursor);
      for (const id of ids) {
        let ready = false;
        try {
          const metadata = this.store.get(id), truth = this.portable.getVariant(id);
          ready = metadata?.status === "ready" && metadata.chunkFingerprint === truth?.chunkFingerprint
            && metadata.indexFormatVersion === KNOWLEDGE_ANN_INDEX_FORMAT_VERSION
            && fs.existsSync(path.join(this.root, metadata.fileName));
        } catch { /* 目录损坏按该变体重建，保留原始向量。 */ }
        if (!ready) this.scheduleBuild(id);
      }
      if (ids.length < 20) break;
      cursor = ids.at(-1)!;
      await new Promise<void>(resolve => setImmediate(resolve));
    }
  }

  scheduleBuild(vectorIndexVariantId: string): void {
    if (this.closed || (this.active?.id === vectorIndexVariantId && !this.active.cancelled)) return;
    this.pending.add(vectorIndexVariantId); this.queuePump();
  }

  invalidate(vectorIndexVariantId: string): void {
    if (this.closed) return;
    this.loaded.delete(vectorIndexVariantId); this.pending.delete(vectorIndexVariantId);
    if (this.active?.id === vectorIndexVariantId) {
      this.active.cancelled = true;
      if (this.active.worker) void this.active.worker.terminate();
    }
    try {
      const row = this.store.get(vectorIndexVariantId);
      this.store.markFailed(vectorIndexVariantId);
      if (row) fs.rmSync(path.join(this.root, row.fileName), { force: true });
    } catch { this.log(`knowledge ANN: invalidation failed ${vectorIndexVariantId}`); }
  }

  private queuePump(): void {
    if (this.wake || this.activePromise) return;
    if (this.closed || this.pending.size === 0) {
      for (const resolve of this.idleWaiters.splice(0)) resolve();
      return;
    }
    this.wake = setImmediate(() => {
      this.wake = null;
      if (this.closed || this.pending.size === 0) { this.queuePump(); return; }
      const id = this.pending.values().next().value!;
      this.pending.delete(id);
      const job: BuildJob = { id, cancelled: false }; this.active = job;
      this.activePromise = this.build(job).catch(error => {
        if (!this.closed) {
          try { this.store.markFailed(id); } catch { /* 目录库失败已归入构建失败记录。 */ }
          const reason = error instanceof Error && /^ANN_[A-Z_]+$/.test(error.message) ? error.message : "ANN_BUILD_FAILED";
          this.failedBuilds.set(id, reason);
          this.log(`knowledge ANN: build failed ${reason}:${id}`);
        }
      }).finally(() => {
        this.active = null; this.activePromise = null; this.queuePump();
      });
    });
    this.wake.unref();
  }

  private assertBuilding(job: BuildJob): void {
    if (this.closed || job.cancelled) throw new Error("ANN_BUILD_CANCELLED");
  }

  private exchange(job: BuildJob, expected: string, message?: { type: string; keys?: BigUint64Array; vectors?: Float32Array }): Promise<{ count?: number }> {
    const worker = job.worker!;
    return new Promise((resolve, reject) => {
      const cleanup = () => { worker.off("message", onMessage); worker.off("error", onError); worker.off("exit", onExit); };
      const onError = (error: Error) => { cleanup(); reject(error); };
      const onExit = () => onError(new Error("ANN_WORKER_EXITED"));
      const onMessage = (result: { type: string; count?: number; message?: string }) => {
        if (result.type === "failed") { onError(new Error("ANN_NATIVE_BUILD_FAILED")); return; }
        if (result.type === expected) { cleanup(); resolve(result); }
      };
      worker.on("message", onMessage); worker.once("error", onError); worker.once("exit", onExit);
      if (message) {
        try { worker.postMessage(message, message.vectors && message.keys
          ? [message.vectors.buffer as ArrayBuffer, message.keys.buffer as ArrayBuffer] : []); }
        catch (error) { onError(error as Error); }
      }
    });
  }

  private async build(job: BuildJob): Promise<void> {
    await this.recovery; this.assertBuilding(job);
    const truth = this.portable.getVariant(job.id);
    if (!truth || truth.status !== "ready") return;
    const count = this.portable.getReadyVectorCount(job.id);
    const row = this.store.begin({ vectorIndexVariantId: job.id, modelKey: truth.modelKey,
      dimensions: truth.dimensions, chunkFingerprint: truth.chunkFingerprint, vectorCount: count });
    let native: KnowledgeNativeModule;
    try { native = this.loadNative(); } catch { throw new Error("ANN_NATIVE_UNAVAILABLE"); }
    const fileName = path.join(this.root, row.fileName), temporaryFile = `${fileName}.tmp`;
    this.loaded.delete(job.id);
    fs.mkdirSync(path.dirname(fileName), { recursive: true, mode: 0o700 });
    const worker = new Worker(BUILD_WORKER, { eval: true, workerData: {
      modulePath: native.modulePath, temporaryFile, options: { ...KNOWLEDGE_ANN_INDEX_OPTIONS, dimensions: truth.dimensions },
    } });
    job.worker = worker;
    // 交换消息的监听器负责传播错误；常驻监听防退出期间遗漏原生错误事件。
    worker.on("error", () => {});
    job.exited = new Promise(resolve => worker.once("exit", resolve));
    try {
      await this.exchange(job, "ready"); this.assertBuilding(job);
      let ordinal = -1, total = 0;
      while (true) {
        this.assertBuilding(job);
        const batch = this.portable.readReadyVectorBatch(job.id, ordinal);
        if (batch.length === 0) break;
        const keys = new BigUint64Array(batch.length), vectors = new Float32Array(batch.length * truth.dimensions);
        for (const [index, entry] of batch.entries()) {
          if (entry.ordinal !== total + index) throw new Error("ANN_VECTOR_ORDINAL_MISMATCH");
          keys[index] = BigInt(entry.ordinal + 1); vectors.set(entry.vector, index * truth.dimensions);
        }
        await this.exchange(job, "added", { type: "add", keys, vectors });
        total += batch.length; ordinal = batch.at(-1)!.ordinal;
      }
      if (total !== count) throw new Error("ANN_VECTOR_COUNT_MISMATCH");
      const saved = await this.exchange(job, "saved", { type: "save" });
      await job.exited; this.assertBuilding(job);
      const current = this.portable.getVariant(job.id);
      if (saved.count !== count || current?.status !== "ready" || current.chunkFingerprint !== truth.chunkFingerprint
        || current.modelKey !== truth.modelKey || current.dimensions !== truth.dimensions
        || this.portable.getReadyVectorCount(job.id) !== count) throw new Error("ANN_BUILD_BECAME_STALE");
      fs.renameSync(temporaryFile, fileName);
      this.store.markReady(job.id);
      this.failedBuilds.delete(job.id);
    } finally {
      await worker.terminate();
      try { fs.rmSync(temporaryFile, { force: true }); } catch { this.log(`knowledge ANN: temporary cleanup failed ${job.id}`); }
    }
  }

  private loadIndex(id: string, modelKey: string, dimensions: number): KnowledgeNativeIndex {
    const truth = this.portable.getVariant(id), metadata = this.store.get(id);
    if (!truth || truth.status !== "ready" || !metadata || metadata.status !== "ready") throw new Error(this.failedBuilds.get(id) ?? "ANN_NOT_READY");
    if (truth.modelKey !== modelKey || metadata.modelKey !== modelKey || truth.dimensions !== dimensions
      || metadata.dimensions !== dimensions || metadata.chunkFingerprint !== truth.chunkFingerprint
      || metadata.indexFormatVersion !== KNOWLEDGE_ANN_INDEX_FORMAT_VERSION) throw new Error("ANN_FINGERPRINT_MISMATCH");
    const count = this.portable.getReadyVectorCount(id);
    if (metadata.vectorCount !== count) throw new Error("ANN_VECTOR_COUNT_MISMATCH");
    const fileName = path.join(this.root, knowledgeAnnFileName(modelKey, id));
    const file = fs.lstatSync(fileName);
    if (!file.isFile() || file.size === 0) throw new Error("ANN_FILE_CORRUPT");
    const signature = JSON.stringify([truth.chunkFingerprint, count, metadata.updatedAt, file.size, file.mtimeMs]);
    const cached = this.loaded.get(id);
    if (cached?.signature === signature) { this.loaded.delete(id); this.loaded.set(id, cached); return cached.index; }
    this.loaded.delete(id);
    validateNativeFile(fileName, count, dimensions, file.size);
    const index = new (this.loadNative().Index)({ ...KNOWLEDGE_ANN_INDEX_OPTIONS, dimensions });
    index.load(fileName);
    if (index.size() !== count || index.dimensions() !== dimensions) throw new Error("ANN_FILE_IDENTITY_MISMATCH");
    const estimatedBytes = count * (dimensions * 4 + 16 * 8 + 64) + 1024;
    this.loaded.set(id, { index, signature, estimatedBytes });
    while (this.loaded.size > KNOWLEDGE_ANN_MAX_LOADED_INDEXES || this.cacheStats.estimatedBytes >= KNOWLEDGE_ANN_MAX_LOADED_BYTES) {
      this.loaded.delete(this.loaded.keys().next().value!);
    }
    return index;
  }

  async search(input: Parameters<KnowledgeVectorSearchBackend["search"]>[0]): Promise<VectorSearchResult[]> {
    if (this.closed) throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "ANN backend is closed");
    if (!Array.isArray(input.vectorIndexVariantIds) || !Number.isSafeInteger(input.limit) || input.limit < 1 || input.limit > 1000
      || input.queryVector.length !== input.model.dimensions || input.queryVector.some(value => !Number.isFinite(value))) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "ANN query is invalid");
    }
    const results: VectorSearchResult[] = [];
    for (const id of new Set(input.vectorIndexVariantIds)) {
      try {
        if (input.queryVector.every(value => value === 0)) throw new Error("ANN_QUERY_ZERO_VECTOR");
        const index = this.loadIndex(id, input.model.key, input.model.dimensions);
        const matches = index.search(new Float32Array(input.queryVector), input.limit, 1);
        if (matches.keys.length !== matches.distances.length) throw new Error("ANN_QUERY_INVALID_RESULT");
        const ordinals = Array.from(matches.keys, key => Number(key) - 1);
        if (new Set(ordinals).size !== ordinals.length || ordinals.some(ordinal => !Number.isSafeInteger(ordinal) || ordinal < 0)
          || Array.from(matches.distances).some(distance => !Number.isFinite(distance))) throw new Error("ANN_QUERY_INVALID_RESULT");
        const locations = new Map(this.portable.resolveVectorOrdinals(id, ordinals).map(row => [row.ordinal, row]));
        if (locations.size !== ordinals.length) throw new Error("ANN_QUERY_UNKNOWN_KEY");
        results.push(...ordinals.map((ordinal, index) => ({ ...locations.get(ordinal)!, score: Math.max(-1, Math.min(1, 1 - matches.distances[index])) })));
        this.portable.touchVectorVariants([id]);
      } catch (error) {
        this.loaded.delete(id);
        const reason = error instanceof Error && /^ANN_[A-Z_]+$/.test(error.message) ? error.message
          : (error as NodeJS.ErrnoException)?.code === "ENOENT" ? "ANN_FILE_MISSING" : "ANN_NATIVE_OR_QUERY_FAILED";
        recordPortableVectorFallback(`${reason}:${id}`);
        this.scheduleBuild(id);
        results.push(...this.portable.search({ ...input, vectorIndexVariantIds: [id] }));
      }
    }
    return results.sort((a, b) => b.score - a.score || a.parseArtifactId.localeCompare(b.parseArtifactId)
      || a.ordinal - b.ordinal || a.vectorIndexVariantId.localeCompare(b.vectorIndexVariantId)).slice(0, input.limit);
  }

  async whenIdle(): Promise<void> {
    await this.recovery;
    if (!this.activePromise && !this.wake && this.pending.size === 0) return;
    await new Promise<void>(resolve => this.idleWaiters.push(resolve));
  }

  close(): Promise<void> {
    if (this.closing) return this.closing;
    this.closed = true; this.pending.clear(); this.loaded.clear(); this.failedBuilds.clear();
    if (this.wake) clearImmediate(this.wake); this.wake = null;
    if (this.active) { this.active.cancelled = true; if (this.active.worker) void this.active.worker.terminate(); }
    // 所有恢复/构建的异步续段都先检查 closed；先同步释放数据库句柄，维持原有关闭语义。
    this.store.close();
    this.closing = (async () => {
      await this.recovery; await this.activePromise;
      for (const resolve of this.idleWaiters.splice(0)) resolve();
    })();
    return this.closing;
  }
}
