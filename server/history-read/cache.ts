/**
 * 历史读取 —— 目录缓存（阶段 B / B04）
 *
 * 实例私有（在 route/runtime 内创建，不设模块级单例）；稳定键
 * runtimeId\0studioId\0(sessionId ?? "path:"+resolve(path))，目录条目内部再绑定
 * normalizedPath + 文件身份 + locator + 分支头——键定位槽位，身份决定有效性
 * （I01、X18）。每会话一槽保存当前目录版本；被替换的旧版本进入 retired（仍被
 * 在途请求引用时计入驻留预算，经 release() 显式归还）；预算淘汰/失效/dispose 均
 * 调用目录 released() 释放临时引用（I11）。构建经全局信号量（默认 2）+ per-key
 * single-flight（同 key 并发 miss 共享同一 lease，X15）；publish 做版本校验，旧构建
 * 晚完成不得覆盖新版本（X16）；probe 四条件（fstat 五元组 / revision / head 三态 /
 * locator）任一不过即 invalidate(reason)。
 */
import path from "path";
import { sessionFileMutationEpoch } from "../../core/session-file-mutation-epoch.ts";
import type {
  HistoryDirectory,
  HistoryProbeVerdict,
  HistoryReadContext,
  InvalidationReason,
} from "./types.ts";

export const DEFAULT_MAX_SESSIONS = 8;
export const DEFAULT_MAX_RESIDENT_INDEX_BYTES = 64 * 1024 * 1024;
export const DEFAULT_MAX_SINGLE_DIRECTORY_BYTES = 16 * 1024 * 1024;
export const DEFAULT_MAX_CONCURRENT_BUILDS = 2;

export interface HistoryDirectoryCacheOptions {
  maxSessions?: number;
  maxResidentIndexBytes?: number;
  maxSingleDirectoryBytes?: number;
  maxConcurrentBuilds?: number;
}

/** 稳定缓存键（I01）：runtime/studio 命名空间 + 业务 sessionId（无则路径命名空间）。 */
export function historyDirectoryCacheKey(ctx: {
  runtimeId?: string | null;
  studioId?: string | null;
  sessionId: string | null;
  sessionPath: string;
}): string {
  return `${ctx.runtimeId || "default"}\0${ctx.studioId || "default"}\0${
    ctx.sessionId ?? `path:${path.resolve(ctx.sessionPath)}`
  }`;
}

export interface HistoryBuildLease {
  readonly key: string;
  /** 槽位版本号：begin 时 +1，invalidate 时 +1；publish 仅接受当前版本（X16）。 */
  readonly generation: number;
}

interface CacheEntry {
  directory: HistoryDirectory;
  released: () => void;
  locatorPath: string | null;
}

interface Slot {
  key: string;
  generation: number;
  entry: CacheEntry | null;
  retired: CacheEntry[];
  building: { lease?: HistoryBuildLease; promise?: Promise<HistoryBuildLease | null> } | null;
  noCache: boolean;
}

export interface HistoryDirectoryCacheStats {
  sessions: number;
  residentBytes: number;
  retiredBytes: number;
  builds: number;
  hits: number;
  evictions: number;
  inFlightBuilds: number;
  noCacheSessions: number;
  invalidations: Record<string, number>;
  /** C05：可信追加增量更新成功次数（每次追加走增量而非重建的证据）。 */
  incrementalUpdates: number;
  /** C03：增量失败放弃次数（走全量重建）。 */
  incrementalFailures: number;
  /** C01：分支视图重建次数（文件不变、head 选择改变）。 */
  branchViewRebuilds: number;
}

const RETIRED_PER_SLOT_LIMIT = 2;

export class HistoryDirectoryCache {
  #opts: Required<HistoryDirectoryCacheOptions>;
  #slots = new Map<string, Slot>();
  #activeBuilds = 0;
  #waiters: Array<() => void> = [];
  #disposed = false;
  #counters = { builds: 0, hits: 0, evictions: 0, incrementalUpdates: 0, incrementalFailures: 0, branchViewRebuilds: 0 };
  #invalidations = new Map<InvalidationReason, number>();

  constructor(opts: HistoryDirectoryCacheOptions = {}) {
    this.#opts = {
      maxSessions: opts.maxSessions ?? DEFAULT_MAX_SESSIONS,
      maxResidentIndexBytes: opts.maxResidentIndexBytes ?? DEFAULT_MAX_RESIDENT_INDEX_BYTES,
      maxSingleDirectoryBytes: opts.maxSingleDirectoryBytes ?? DEFAULT_MAX_SINGLE_DIRECTORY_BYTES,
      maxConcurrentBuilds: opts.maxConcurrentBuilds ?? DEFAULT_MAX_CONCURRENT_BUILDS,
    };
  }

  #ensureSlot(key: string): Slot {
    let slot = this.#slots.get(key);
    if (!slot) {
      slot = { key, generation: 0, entry: null, retired: [], building: null, noCache: false };
      this.#slots.set(key, slot);
    }
    return slot;
  }

  #acquireBuildPermit(): Promise<void> {
    if (this.#activeBuilds < this.#opts.maxConcurrentBuilds) {
      this.#activeBuilds += 1;
      return Promise.resolve();
    }
    return new Promise((resolve) => {
      this.#waiters.push(() => {
        // 许可直接传递给下一个等待者，activeBuilds 计数不变。
        resolve();
      });
    });
  }

  #releaseBuildPermit(): void {
    const next = this.#waiters.shift();
    if (next) next();
    else this.#activeBuilds = Math.max(0, this.#activeBuilds - 1);
  }

  /** 命中返回当前目录并提升 LRU；构建中/无缓存/缺失返回 null。 */
  get(key: string): { directory: HistoryDirectory } | null {
    const slot = this.#slots.get(key);
    if (!slot?.entry) return null;
    this.#slots.delete(key);
    this.#slots.set(key, slot);
    this.#counters.hits += 1;
    return { directory: slot.entry.directory };
  }

  /**
   * C01 变化分类（命中校验）——判定序（TASKBOOK C01 决策表逐项落地）：
   *  1. revision null → revision_unknown（不读不写缓存，I08）
   *  2. dev/ino 不等 → file_identity_changed（同路径替换，X04）
   *  3. locator 不等 → locator_changed
   *  4. size 缩短 → snapshot_changed（全量重建）
   *  5. size 相等：mtime/ctime/revision 全等且 head 三态全等 → valid；
   *     head 不等（文件不变分支选择改变）→ branch_view_stale（保留物理索引重建视图，
   *     不失效目录）；mtime/ctime 不等（同长度重写/归档 utimes，X02）→ untrusted_mutation
   *  6. size 增长：变更世代未变（无已插桩重写）→ append_candidate（可信追加，C03 增量；
   *     追加与分支变化同时发生也走此判定，增量构建按 head 规则重建视图，X09）；
   *     世代已变（重写后变大/无法区分，X03）→ untrusted_mutation
   * 失效类判定经 #fail 走 invalidate；append_candidate/branch_view_stale 保留目录。
   */
  probe(key: string, ctx: HistoryReadContext): HistoryProbeVerdict {
    const slot = this.#slots.get(key);
    const entry = slot?.entry;
    if (!entry) return this.#fail(key, "directory_invalid");
    const directory = entry.directory;
    const identity = ctx.fileIdentity;
    const dirIdentity = directory.key.fileIdentity;
    // 1. 公开 revision（I07/I08）
    if (ctx.publicRevision == null) return this.#fail(key, "revision_unknown");
    // 2. 底层文件身份（X04 同 size+mtime 原子替换改 ino/ctime）
    if (dirIdentity.dev != null && identity.dev != null && dirIdentity.dev !== identity.dev) {
      return this.#fail(key, "file_identity_changed");
    }
    if (dirIdentity.ino != null && identity.ino != null && dirIdentity.ino !== identity.ino) {
      return this.#fail(key, "file_identity_changed");
    }
    // 3. locator 绑定
    if ((entry.locatorPath ?? null) !== (ctx.locatorPath ?? null)) return this.#fail(key, "locator_changed");
    const branchStale =
      ctx.branchHeadRowExists !== directory.branch.headRowExists
      || (ctx.branchHeadRow?.leafId ?? null) !== directory.branch.persistedLeafId
      || (ctx.branchHeadRow?.observedTailLeafId ?? null) !== directory.branch.observedTailLeafId;
    // 4. 缩短 → 全量重建
    if (identity.size < dirIdentity.size) return this.#fail(key, "snapshot_changed");
    if (identity.size === dirIdentity.size) {
      // 5. 同长度：mtime/ctime 翻转 = 同长度重写或归档 utimes（X02），无法区分 → 全量重建
      if (dirIdentity.mtimeMs !== identity.mtimeMs) return this.#fail(key, "untrusted_mutation");
      if (dirIdentity.ctimeMs != null && identity.ctimeMs != null && dirIdentity.ctimeMs !== identity.ctimeMs) {
        return this.#fail(key, "untrusted_mutation");
      }
      if (ctx.publicRevision !== directory.session.publicRevision) return this.#fail(key, "snapshot_changed");
      // 文件不变、分支选择改变 → 保留有效物理索引，重建当前分支视图（C01；不失效目录）
      if (branchStale) return "branch_view_stale";
      return "valid";
    }
    // 6. size 增长：世代未变（无已插桩重写/修复/生命周期写入）→ 可信追加候选；
    //    世代已变 → 重写后变大或无法区分追加与重写（X03）→ 全量重建
    const epochNow = sessionFileMutationEpoch(ctx.sessionPath);
    const epochAtBuild = directory.session.mutationEpochAtBuild ?? 0;
    if (epochNow !== epochAtBuild) return this.#fail(key, "untrusted_mutation");
    return "append_candidate";
  }

  #fail(key: string, reason: InvalidationReason): InvalidationReason {
    this.invalidate(key, reason);
    return reason;
  }

  /**
   * per-key single-flight：构建中再次调用返回同一 lease 的共享 Promise（X15）；
   * 该会话已记 no-cache（budget_exceeded）时返回 null。
   */
  beginBuild(key: string): Promise<HistoryBuildLease | null> {
    if (this.#disposed) return Promise.resolve(null);
    const slot = this.#ensureSlot(key);
    if (slot.noCache) return Promise.resolve(null);
    if (slot.building) return slot.building.promise ?? Promise.resolve(null);
    const building: { lease?: HistoryBuildLease; promise?: Promise<HistoryBuildLease | null> } = {};
    building.promise = this.#acquireBuildPermit().then(() => {
      // 等待信号量期间被 invalidate/dispose：让出许可且不发起构建。
      if (this.#disposed || slot.building !== building) {
        this.#releaseBuildPermit();
        return null;
      }
      slot.generation += 1;
      const lease: HistoryBuildLease = { key, generation: slot.generation };
      building.lease = lease;
      this.#counters.builds += 1;
      return lease;
    });
    slot.building = building;
    return building.promise;
  }

  /**
   * 版本校验后原子发布：lease 不再是当前槽位的在途构建、或版本已被 invalidate
   * 推进时拒绝（旧构建晚完成不得覆盖新版本，X16）；拒绝/淘汰的目录立即 released()。
   * hooks.released 供缓存接管 I11 释放职责；hooks.locatorPath 记录 locator 绑定。
   */
  publish(
    key: string,
    lease: HistoryBuildLease,
    directory: HistoryDirectory,
    hooks: { released?: () => void; locatorPath?: string | null } = {},
  ): boolean {
    const slot = this.#slots.get(key);
    if (
      this.#disposed
      || !slot
      || !slot.building
      || slot.building.lease !== lease
      || lease.generation !== slot.generation
      || slot.noCache
      || directory.measuredBytes > this.#opts.maxSingleDirectoryBytes
    ) {
      if (process.env.HISTORY_READ_DEBUG === "1") {
        console.error(`[dbg] publish rejected: disposed=${this.#disposed} slot=${!!slot} building=${!!slot?.building} leaseMatch=${slot?.building?.lease === lease} gen=${lease.generation}/${slot?.generation} noCache=${slot?.noCache} bytes=${directory.measuredBytes}`);
      }
      hooks?.released?.();
      return false;
    }
    slot.building = null;
    this.#releaseBuildPermit();
    if (slot.entry) {
      slot.retired.push(slot.entry);
      while (slot.retired.length > RETIRED_PER_SLOT_LIMIT) {
        const dropped = slot.retired.shift();
        dropped?.released();
      }
    }
    slot.entry = {
      directory,
      released: hooks?.released ?? (() => {}),
      locatorPath: hooks?.locatorPath ?? null,
    };
    // LRU 提升 + 预算淘汰（刚发布的条目不淘汰）。
    this.#slots.delete(key);
    this.#slots.set(key, slot);
    this.#evictIfNeeded(key);
    return true;
  }

  /** 构建失败：从在途表移除该 lease（可重试），许可见 #releaseBuildPermit 语义。 */
  failBuild(key: string, lease: HistoryBuildLease): void {
    const slot = this.#slots.get(key);
    if (slot?.building?.lease === lease) slot.building = null;
    this.#releaseBuildPermit();
  }

  /**
   * 显式归还一个 retired 旧版本（在途请求完成时调用）；同时调用其 released()。
   * 预算中的 retiredBytes 随之下降。
   */
  release(key: string, directory: HistoryDirectory): void {
    const slot = this.#slots.get(key);
    if (!slot) return;
    const index = slot.retired.findIndex((entry) => entry.directory === directory);
    if (index === -1) return;
    const [entry] = slot.retired.splice(index, 1);
    entry.released();
  }

  /** 失效当前版本（probe 失败/生命周期边界）；budget_exceeded 使该会话记 no-cache。 */
  invalidate(key: string, reason: InvalidationReason): void {
    this.#invalidations.set(reason, (this.#invalidations.get(reason) ?? 0) + 1);
    // budget_exceeded 对尚无槽位的会话同样生效：先建槽并记 no-cache（B04：超预算
    // 会话走无缓存只读路径，不截断历史）。
    const slot = reason === "budget_exceeded" ? this.#ensureSlot(key) : this.#slots.get(key);
    if (!slot) return;
    if (slot.building) slot.building = null; // 等待信号量的在途构建在 resolve 时自让许可
    if (slot.entry) {
      slot.entry.released();
      slot.entry = null;
    }
    for (const entry of slot.retired) entry.released();
    slot.retired = [];
    slot.generation += 1;
    if (reason === "budget_exceeded") slot.noCache = true;
  }

  #evictIfNeeded(justPublishedKey: string): void {
    for (;;) {
      const { ready, retired } = this.#residentBytesInternal();
      let readyCount = 0;
      for (const slot of this.#slots.values()) if (slot.entry) readyCount += 1;
      // 驻留预算含 retired（在途旧版本计入驻留，I11）：retired 压满预算时也会淘汰
      // ready 条目；仅剩刚发布条目时停止（retired 由 release()/invalidate() 归还）。
      if (readyCount <= this.#opts.maxSessions && ready + retired <= this.#opts.maxResidentIndexBytes) {
        return;
      }
      // 插入序 = LRU 序；跳过刚发布的条目与无 ready 条目的槽位。
      let victimKey: string | null = null;
      for (const key of this.#slots.keys()) {
        if (key !== justPublishedKey && this.#slots.get(key)?.entry) {
          victimKey = key;
          break;
        }
      }
      if (!victimKey) return;
      const slot = this.#slots.get(victimKey)!;
      if (slot.entry) {
        slot.entry.released();
        slot.entry = null;
        this.#counters.evictions += 1;
      }
    }
  }

  #residentBytesInternal(): { ready: number; retired: number } {
    let ready = 0;
    let retired = 0;
    for (const slot of this.#slots.values()) {
      if (slot.entry) ready += slot.entry.directory.measuredBytes;
      for (const entry of slot.retired) retired += entry.directory.measuredBytes;
    }
    return { ready, retired };
  }

  /** 统计（验收配置观测用）：驻留 = ready + retired（在途旧版本计入驻留，I11）。 */
  stats(): HistoryDirectoryCacheStats {
    const { ready, retired } = this.#residentBytesInternal();
    let sessions = 0;
    let inFlightBuilds = 0;
    let noCacheSessions = 0;
    for (const slot of this.#slots.values()) {
      if (slot.entry) sessions += 1;
      if (slot.building?.lease) inFlightBuilds += 1;
      if (slot.noCache) noCacheSessions += 1;
    }
    return {
      sessions,
      residentBytes: ready + retired,
      retiredBytes: retired,
      builds: this.#counters.builds,
      hits: this.#counters.hits,
      evictions: this.#counters.evictions,
      inFlightBuilds,
      noCacheSessions,
      invalidations: Object.fromEntries(this.#invalidations),
      incrementalUpdates: this.#counters.incrementalUpdates,
      incrementalFailures: this.#counters.incrementalFailures,
      branchViewRebuilds: this.#counters.branchViewRebuilds,
    };
  }

  /** C05：增量更新成功/失败与分支视图重建计数。 */
  noteIncrementalUpdate(durationMs: number, metrics: { appendedRecords: number; affectedRelations: number }): void {
    this.#counters.incrementalUpdates += 1;
    this.#lastIncremental = { durationMs, ...metrics, at: new Date().toISOString() };
  }

  noteIncrementalFailure(): void {
    this.#counters.incrementalFailures += 1;
  }

  noteBranchViewRebuild(): void {
    this.#counters.branchViewRebuilds += 1;
  }

  #lastIncremental: Record<string, unknown> | null = null;

  /** 最近一次增量更新摘要（C05 证据）。 */
  lastIncremental(): Record<string, unknown> | null {
    return this.#lastIncremental;
  }

  /** 测试/诊断：按 key 读取当前已发布目录。 */
  lastPublished(key: string): HistoryDirectory | null {
    return this.#slots.get(key)?.entry?.directory ?? null;
  }

  /** 释放全部槽位（entry/retired 调用 released()）、清空在途与等待队列。 */
  dispose(): void {
    this.#disposed = true;
    for (const slot of this.#slots.values()) {
      if (slot.entry) {
        slot.entry.released();
        slot.entry = null;
      }
      for (const entry of slot.retired) entry.released();
      slot.retired = [];
      slot.building = null;
    }
    this.#slots.clear();
    const waiters = this.#waiters;
    this.#waiters = [];
    for (const wake of waiters) wake();
    this.#activeBuilds = 0;
  }
}
