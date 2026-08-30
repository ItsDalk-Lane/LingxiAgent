import fs from "node:fs";
import path from "node:path";

import { isKnowledgeError } from "./errors.ts";

/** 事件 → 防抖 → refresh 的合并窗口；编辑器一次保存常触发多个目录事件。 */
export const KNOWLEDGE_WATCH_DEBOUNCE_MS = 1500;
/** fs.watch 不可靠事件（目录删除后静默、网络盘丢事件等）的 mtime/size 兜底轮询周期。 */
export const KNOWLEDGE_WATCH_POLL_INTERVAL_MS = 5 * 60_000;
/** watcher error 重挂退避：retryBaseMs * 2^attempt，封顶 retryMaxMs。 */
export const KNOWLEDGE_WATCH_RETRY_BASE_MS = 1000;
export const KNOWLEDGE_WATCH_RETRY_MAX_MS = 60_000;

export interface KnowledgeWatchHandle {
  close(): void;
}

/**
 * 目录 watch 工厂（生产缺省包装 fs.watch；测试注入假工厂确定性驱动事件/错误）。
 * watch 父目录而不是文件本身：编辑器 atomic-rename 保存会废掉文件级 watcher。
 */
export type KnowledgeWatchDirectoryFactory = (input: {
  dir: string;
  onEvent: (eventType: string, fileName: string | null) => void;
  onError: (error: Error) => void;
}) => KnowledgeWatchHandle;

export type KnowledgeStatFileFn = (filePath: string) => Promise<{ mtimeMs: number; size: number }>;

export interface KnowledgeSourceFileRefreshResult {
  changed: boolean;
  parseArtifact?: { id: string } | null;
}

/** 计时器/IO 参数化：测试用 fake timers + 假 watch/stat 工厂驱动。 */
export interface KnowledgeSourceFileWatcherTuning {
  debounceMs?: number;
  pollIntervalMs?: number;
  retryBaseMs?: number;
  retryMaxMs?: number;
  watchDirectory?: KnowledgeWatchDirectoryFactory;
  statFile?: KnowledgeStatFileFn;
}

export interface KnowledgeSourceFileWatcherDeps extends KnowledgeSourceFileWatcherTuning {
  /**
   * 绑定到 KnowledgeManager.refreshFileSource：内部 sha256 比对天然去重
   * （changed=false 不产生新快照也不入队）；changed=true 时已为触发笔记本入队摄入。
   */
  refresh: (input: {
    studioId: string;
    notebookId: string;
    sourceId: string;
  }) => Promise<KnowledgeSourceFileRefreshResult>;
  /** 同一源的其他笔记本 membership 各自入队摄入（分块配置按各自笔记本解析）。 */
  enqueueForNotebook: (input: {
    studioId: string;
    notebookId: string;
    sourceId: string;
    artifactId?: string | null;
  }) => void;
  log?: (message: string) => void;
  now?: () => string;
}

export interface KnowledgeSourceWatchState {
  sourceId: string;
  studioId: string;
  notebooks: string[];
  /** 目录 watcher 当前是否挂着。 */
  watching: boolean;
  /** 源文件不可达（消失/不可读）：显式状态，不是错误，文件恢复后自动清除并刷新。 */
  unreachable: boolean;
  unreachableReason: string | null;
  unreachableSince: string | null;
}

interface WatchEntry {
  sourceId: string;
  studioId: string;
  filePath: string;
  dir: string;
  fileName: string;
  /** 活跃 membership：一源可被多个笔记本引用；空集即整个 entry 摘除。 */
  notebooks: Set<string>;
  handle: KnowledgeWatchHandle | null;
  debounceTimer: ReturnType<typeof setTimeout> | null;
  retryTimer: ReturnType<typeof setTimeout> | null;
  retryAttempt: number;
  refreshing: boolean;
  /** refresh 期间又到事件：完成后补一轮（仍走防抖+sha 去重）。 */
  pendingRefresh: boolean;
  lastFileState: { mtimeMs: number; size: number } | null;
  unreachable: boolean;
  unreachableReason: string | null;
  unreachableSince: string | null;
}

function describeWatchError(error: unknown): string {
  const raw = error instanceof Error ? error.message : String(error);
  const code = (error as NodeJS.ErrnoException | null)?.code;
  const prefixed = isKnowledgeError(error)
    ? `${error.code}: ${raw}`
    : typeof code === "string" && code
      ? `${code}: ${raw}`
      : raw;
  return prefixed.slice(0, 512);
}

/** fs.watch 对缺失目录既可能同步抛也可能异步发 error 事件；统一收敛到 onError 单一路径。 */
const defaultWatchDirectory: KnowledgeWatchDirectoryFactory = ({ dir, onEvent, onError }) => {
  let watcher: fs.FSWatcher;
  try {
    watcher = fs.watch(dir, (eventType, fileName) => onEvent(eventType, fileName));
  } catch (error) {
    queueMicrotask(() => onError(error instanceof Error ? error : new Error(String(error))));
    return { close() {} };
  }
  watcher.on("error", onError);
  return { close: () => watcher.close() };
};

const defaultStatFile: KnowledgeStatFileFn = async (filePath) => {
  const stat = await fs.promises.stat(filePath);
  return { mtimeMs: stat.mtimeMs, size: stat.size };
};

/**
 * file 源文件 watcher：目录级 watch（按文件名过滤）+ 1500ms 防抖 + refresh 内 sha256
 * 去重三重防线；watcher error 指数退避重挂（封顶 60s）；5 分钟 mtime/size 兜底轮询。
 * 仅跟踪用户导入的外部 file 源——托管目录（lingxiHome 内）在导入安全层即被
 * PATH_BLOCKED 拒绝，web_snapshot/pasted_text 本就不是 file 源，天然不在范围内。
 * 文件消失标 unreachable（"源文件不可达"显式状态），不抛错、不产生失败 job；
 * 文件恢复（轮询/事件检出）自动清除并触发刷新。watcher 内部错误一律显式 log 不吞。
 */
export class KnowledgeSourceFileWatcher {
  private readonly deps: KnowledgeSourceFileWatcherDeps;
  private readonly now: () => string;
  private readonly log: (message: string) => void;
  private readonly debounceMs: number;
  private readonly pollIntervalMs: number;
  private readonly retryBaseMs: number;
  private readonly retryMaxMs: number;
  private readonly watchDirectory: KnowledgeWatchDirectoryFactory;
  private readonly statFile: KnowledgeStatFileFn;
  private readonly entries = new Map<string, WatchEntry>();
  private started = false;
  private pollTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(deps: KnowledgeSourceFileWatcherDeps) {
    if (!deps || typeof deps.refresh !== "function" || typeof deps.enqueueForNotebook !== "function") {
      throw new Error("KnowledgeSourceFileWatcher requires refresh and enqueueForNotebook");
    }
    this.deps = deps;
    this.now = deps.now || (() => new Date().toISOString());
    this.log = deps.log || (() => {});
    this.debounceMs = deps.debounceMs ?? KNOWLEDGE_WATCH_DEBOUNCE_MS;
    this.pollIntervalMs = deps.pollIntervalMs ?? KNOWLEDGE_WATCH_POLL_INTERVAL_MS;
    this.retryBaseMs = deps.retryBaseMs ?? KNOWLEDGE_WATCH_RETRY_BASE_MS;
    this.retryMaxMs = deps.retryMaxMs ?? KNOWLEDGE_WATCH_RETRY_MAX_MS;
    this.watchDirectory = deps.watchDirectory ?? defaultWatchDirectory;
    this.statFile = deps.statFile ?? defaultStatFile;
  }

  /** 跟踪一个 file 源（导入成功/加入笔记本/启动扫描时由 manager 调用）。幂等。 */
  trackSource(input: { studioId: string; notebookId: string; sourceId: string; filePath: string }) {
    const existing = this.entries.get(input.sourceId);
    if (existing) {
      existing.notebooks.add(input.notebookId);
      if (existing.filePath !== input.filePath) {
        // originalPath 导入后不可变；出现分歧说明上游数据异常，显式留痕不覆盖。
        this.log(`knowledge watch: conflicting file path for source ${input.sourceId}; keeping the original`);
      }
      return;
    }
    const entry: WatchEntry = {
      sourceId: input.sourceId,
      studioId: input.studioId,
      filePath: input.filePath,
      dir: path.dirname(input.filePath),
      fileName: path.basename(input.filePath),
      notebooks: new Set([input.notebookId]),
      handle: null,
      debounceTimer: null,
      retryTimer: null,
      retryAttempt: 0,
      refreshing: false,
      pendingRefresh: false,
      lastFileState: null,
      unreachable: false,
      unreachableReason: null,
      unreachableSince: null,
    };
    this.entries.set(entry.sourceId, entry);
    if (this.started) this.attach(entry);
  }

  /** 源从某笔记本移除后摘掉该 membership；最后一个 membership 消失时整个 entry 摘除。 */
  untrackSourceMembership(input: { sourceId: string; notebookId: string }) {
    const entry = this.entries.get(input.sourceId);
    if (!entry) return;
    entry.notebooks.delete(input.notebookId);
    if (entry.notebooks.size === 0) {
      this.detach(entry);
      this.entries.delete(entry.sourceId);
    }
  }

  /** 笔记本删除后摘掉其全部 membership（manager.deleteNotebook 挂钩）。 */
  untrackNotebook(notebookId: string) {
    for (const entry of [...this.entries.values()]) {
      if (entry.notebooks.has(notebookId)) {
        this.untrackSourceMembership({ sourceId: entry.sourceId, notebookId });
      }
    }
  }

  /** 整源摘除（manager.deleteSource 挂钩）：源被显式删除后不再 watch 其外部文件。 */
  untrackSource(sourceId: string) {
    const entry = this.entries.get(sourceId);
    if (!entry) return;
    this.detach(entry);
    this.entries.delete(sourceId);
  }

  /** 启动：挂载全部已跟踪源 + 启动兜底轮询。幂等。 */
  start() {
    if (this.started) return;
    this.started = true;
    for (const entry of this.entries.values()) this.attach(entry);
    this.schedulePoll();
  }

  /** 停止：摘全部 watcher 与计时器；entry 保留（restart 时重新挂载）。 */
  stop() {
    this.started = false;
    if (this.pollTimer) {
      clearTimeout(this.pollTimer);
      this.pollTimer = null;
    }
    for (const entry of this.entries.values()) this.detach(entry);
  }

  getWatchStates(): KnowledgeSourceWatchState[] {
    return [...this.entries.values()].map((entry) => ({
      sourceId: entry.sourceId,
      studioId: entry.studioId,
      notebooks: [...entry.notebooks],
      watching: entry.handle != null,
      unreachable: entry.unreachable,
      unreachableReason: entry.unreachableReason,
      unreachableSince: entry.unreachableSince,
    }));
  }

  private attach(entry: WatchEntry) {
    if (!this.started || entry.handle) return;
    entry.handle = this.watchDirectory({
      dir: entry.dir,
      onEvent: (eventType, fileName) => this.onDirectoryEvent(entry, eventType, fileName),
      onError: (error) => this.onWatcherError(entry, error),
    });
    // 挂载即 seed 一次 mtime/size 基线（轮询变更检测的参照），顺带检出初始不可达。
    void this.statFile(entry.filePath).then(
      (state) => {
        entry.lastFileState = state;
        this.markReachable(entry);
      },
      (error) => this.markUnreachable(entry, error),
    );
  }

  private detach(entry: WatchEntry) {
    if (entry.debounceTimer) {
      clearTimeout(entry.debounceTimer);
      entry.debounceTimer = null;
    }
    if (entry.retryTimer) {
      clearTimeout(entry.retryTimer);
      entry.retryTimer = null;
    }
    if (entry.handle) {
      try {
        entry.handle.close();
      } catch (error) {
        this.log(`knowledge watch: watcher close failed for ${entry.sourceId}: ${describeWatchError(error)}`);
      }
      entry.handle = null;
    }
    entry.pendingRefresh = false;
  }

  private onDirectoryEvent(entry: WatchEntry, _eventType: string, fileName: string | null) {
    if (!this.started || !this.entries.has(entry.sourceId)) return;
    // watcher 还活着在投递事件：重挂退避归零。
    entry.retryAttempt = 0;
    // 目录内其他文件的事件过滤掉；fileName 为 null（部分平台）时无法过滤，
    // 放行由防抖 + sha 去重兜底。
    if (fileName && fileName !== entry.fileName) return;
    this.scheduleDebouncedRefresh(entry);
  }

  private onWatcherError(entry: WatchEntry, error: unknown) {
    if (entry.handle) {
      try {
        entry.handle.close();
      } catch {
        // 错误路径上的 best-effort 清理；原始 error 下面显式记录。
      }
      entry.handle = null;
    }
    if (!this.started || !this.entries.has(entry.sourceId)) return;
    const delayMs = Math.min(this.retryBaseMs * 2 ** entry.retryAttempt, this.retryMaxMs);
    entry.retryAttempt += 1;
    this.log(
      `knowledge watch: watcher error for ${entry.sourceId}: ${describeWatchError(error)}; `
      + `re-attaching in ${delayMs}ms`,
    );
    entry.retryTimer = setTimeout(() => {
      entry.retryTimer = null;
      this.attach(entry);
    }, delayMs);
  }

  private scheduleDebouncedRefresh(entry: WatchEntry) {
    if (!this.started) return;
    if (entry.debounceTimer) clearTimeout(entry.debounceTimer);
    entry.debounceTimer = setTimeout(() => {
      entry.debounceTimer = null;
      void this.runRefresh(entry);
    }, this.debounceMs);
  }

  private async runRefresh(entry: WatchEntry) {
    if (!this.started || !this.entries.has(entry.sourceId)) return;
    if (entry.refreshing) {
      entry.pendingRefresh = true;
      return;
    }
    const notebookId = entry.notebooks.values().next().value;
    if (!notebookId) return;
    entry.refreshing = true;
    try {
      // 先 stat：文件消失标 unreachable（显式状态，不抛错、不触发摄入失败）。
      let state: { mtimeMs: number; size: number };
      try {
        state = await this.statFile(entry.filePath);
      } catch (error) {
        this.markUnreachable(entry, error);
        return;
      }
      entry.lastFileState = state;
      this.markReachable(entry);
      const result = await this.deps.refresh({
        studioId: entry.studioId,
        notebookId,
        sourceId: entry.sourceId,
      });
      if (result?.changed) {
        // refresh 已为触发笔记本入队；同一源的其他笔记本各自入队。
        const artifactId = result.parseArtifact?.id ?? null;
        for (const other of entry.notebooks) {
          if (other === notebookId) continue;
          this.deps.enqueueForNotebook({
            studioId: entry.studioId,
            notebookId: other,
            sourceId: entry.sourceId,
            artifactId,
          });
        }
      }
    } catch (error) {
      if (isKnowledgeError(error) && error.code === "KNOWLEDGE_IMPORT_NOT_FOUND") {
        // stat 与读取之间文件被删的竞态：同样落 unreachable。
        this.markUnreachable(entry, error);
      } else if (isKnowledgeError(error) && error.code === "KNOWLEDGE_NOT_FOUND") {
        // membership 在 refresh 途中被移除：untrack 会收敛 watch 项，留痕即可。
        this.log(`knowledge watch: refresh skipped for ${entry.sourceId}: ${describeWatchError(error)}`);
      } else {
        // 显式留痕不吞错（禁静默降级）；本次没有新快照，不产生摄入 job。
        this.log(`knowledge watch: refresh failed for ${entry.sourceId}: ${describeWatchError(error)}`);
      }
    } finally {
      entry.refreshing = false;
      if (entry.pendingRefresh) {
        entry.pendingRefresh = false;
        this.scheduleDebouncedRefresh(entry);
      }
    }
  }

  private schedulePoll() {
    if (!this.started) return;
    this.pollTimer = setTimeout(() => {
      this.pollTimer = null;
      void this.pollAll().finally(() => this.schedulePoll());
    }, this.pollIntervalMs);
  }

  private async pollAll() {
    await Promise.all([...this.entries.values()].map((entry) => this.pollEntry(entry)));
  }

  /** 兜底轮询：mtime/size 变化检出（fs.watch 丢事件时兜住），顺带检出文件消失/恢复。 */
  private async pollEntry(entry: WatchEntry) {
    if (!this.started || !this.entries.has(entry.sourceId)) return;
    let state: { mtimeMs: number; size: number };
    try {
      state = await this.statFile(entry.filePath);
    } catch (error) {
      this.markUnreachable(entry, error);
      return;
    }
    const changed = !entry.lastFileState
      || entry.lastFileState.mtimeMs !== state.mtimeMs
      || entry.lastFileState.size !== state.size;
    const wasUnreachable = entry.unreachable;
    entry.lastFileState = state;
    this.markReachable(entry);
    if (changed || wasUnreachable) this.scheduleDebouncedRefresh(entry);
  }

  private markUnreachable(entry: WatchEntry, error: unknown) {
    entry.lastFileState = null;
    const reason = describeWatchError(error);
    if (entry.unreachable && entry.unreachableReason === reason) return;
    entry.unreachable = true;
    entry.unreachableReason = reason;
    entry.unreachableSince = this.now();
    this.log(`knowledge watch: source file unreachable for ${entry.sourceId}: ${reason}`);
  }

  private markReachable(entry: WatchEntry) {
    if (!entry.unreachable) return;
    entry.unreachable = false;
    entry.unreachableReason = null;
    entry.unreachableSince = null;
    entry.retryAttempt = 0;
    this.log(`knowledge watch: source file reachable again for ${entry.sourceId}`);
  }
}
