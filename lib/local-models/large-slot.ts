import type { LocalModelPriority } from "./contracts.ts";
import { localModelAbortError, throwIfAborted } from "./errors.ts";

export interface LargeSlotState {
  /** 兼容字段：当前持槽的第一个模型；多容量时以 activeKeys 为准。 */
  activeKey: string | null;
  activeKeys: ReadonlyArray<string>;
  capacity: number;
  queue: ReadonlyArray<{ key: string; priority: LocalModelPriority }>;
}

export interface LargeSlotRequest {
  key: string;
  priority?: LocalModelPriority;
  signal: AbortSignal;
}

interface QueueEntry {
  key: string;
  priority: LocalModelPriority;
  sequence: number;
  signal: AbortSignal;
  resolve: () => void;
  reject: (error: unknown) => void;
  onAbort: () => void;
}

const PRIORITY_ORDER: Record<LocalModelPriority, number> = {
  interactive: 0,
  normal: 1,
  batch: 2,
};

/**
 * 全局大模型槽。这里只发放“允许开始加载”的资格；真正的引用计数和卸载由
 * InstanceManager 管理。调用方必须在进程确认退出后再 release，才能保证卸载先于换模型。
 * capacity 默认 1（大模型零并存）；由设备自检给出更高容量时，允许 N 个大模型同时驻留。
 */
export class LargeSlot {
  private readonly capacity: number;
  private readonly activeKeys = new Set<string>();
  private queue: QueueEntry[] = [];
  private sequence = 0;
  private readonly onStateChange: (state: LargeSlotState) => void;
  private disposed = false;

  constructor(onStateChange: (state: LargeSlotState) => void = () => {}, options?: { capacity?: number }) {
    const capacity = options?.capacity ?? 1;
    if (!Number.isInteger(capacity) || capacity < 1 || capacity > 8) {
      throw new Error(`LargeSlot capacity must be an integer in [1, 8], got ${capacity}`);
    }
    this.capacity = capacity;
    this.onStateChange = onStateChange;
  }

  async request({ key, priority = "normal", signal }: LargeSlotRequest): Promise<void> {
    if (!key) throw new Error("LargeSlot request requires a model key");
    throwIfAborted(signal);
    if (this.disposed) throw new Error("LargeSlot is disposed");
    if (this.activeKeys.has(key)) return;
    if (this.activeKeys.size < this.capacity) {
      this.activeKeys.add(key);
      this.emitState();
      return;
    }

    await new Promise<void>((resolve, reject) => {
      const entry: QueueEntry = {
        key,
        priority,
        sequence: this.sequence++,
        signal,
        resolve,
        reject,
        onAbort: () => {
          const index = this.queue.indexOf(entry);
          if (index >= 0) this.queue.splice(index, 1);
          reject(localModelAbortError("large model request was cancelled while queued"));
          this.emitState();
        },
      };
      signal.addEventListener("abort", entry.onAbort, { once: true });
      this.queue.push(entry);
      this.emitState();
    });
  }

  /** 只允许已确认完全卸载的当前模型释放槽位。 */
  release(key: string): void {
    if (!this.activeKeys.has(key)) {
      throw new Error(`LargeSlot release mismatch: active=${[...this.activeKeys].join(",") || "none"}, requested=${key}`);
    }
    this.activeKeys.delete(key);
    this.grantNext();
    this.emitState();
  }

  snapshot(): LargeSlotState {
    return Object.freeze({
      activeKey: this.activeKeys.values().next().value ?? null,
      activeKeys: Object.freeze([...this.activeKeys]),
      capacity: this.capacity,
      queue: Object.freeze(this.sortedQueue().map(({ key, priority }) => Object.freeze({ key, priority }))),
    });
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    const error = new Error("LargeSlot is disposed");
    for (const entry of this.queue.splice(0)) {
      entry.signal.removeEventListener("abort", entry.onAbort);
      entry.reject(error);
    }
    this.emitState();
  }

  private grantNext(): void {
    while (this.activeKeys.size < this.capacity && this.queue.length > 0) {
      const sorted = this.sortedQueue();
      const first = sorted[0];
      const granted = this.queue.filter((entry) => entry.key === first.key);
      this.queue = this.queue.filter((entry) => entry.key !== first.key);
      this.activeKeys.add(first.key);
      for (const entry of granted) {
        entry.signal.removeEventListener("abort", entry.onAbort);
        entry.resolve();
      }
    }
  }

  private sortedQueue(): QueueEntry[] {
    return [...this.queue].sort((left, right) =>
      PRIORITY_ORDER[left.priority] - PRIORITY_ORDER[right.priority]
      || left.sequence - right.sequence);
  }

  private emitState(): void {
    this.onStateChange(this.snapshot());
  }
}

