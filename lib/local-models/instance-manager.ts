import type { LocalModelDescriptor, LocalModelPriority, LocalModelTier } from "./contracts.ts";
import { localModelKey } from "./contracts.ts";
import { LargeSlot } from "./large-slot.ts";
import { MemoryGovernor } from "./memory-governor.ts";
import { isLocalModelAbort, localModelAbortError, throwIfAborted } from "./errors.ts";

export interface ManagedInstanceLoader<T> {
  load(spec: LocalModelDescriptor, signal: AbortSignal): Promise<T>;
  unload(instance: T, spec: LocalModelDescriptor, signal: AbortSignal): Promise<void>;
  getRssMb?(instance: T | null, spec: LocalModelDescriptor): number | Promise<number>;
}

export interface ManagedInstanceLease<T> {
  readonly key: string;
  readonly instance: T;
  release(): Promise<void>;
}

export interface InstanceManagerOptions<T> {
  loader: ManagedInstanceLoader<T>;
  largeSlot: LargeSlot;
  memoryGovernor: MemoryGovernor;
  idleUnloadMs?: Partial<Record<LocalModelTier, number>>;
  onEvent?: (event: Record<string, unknown>) => void;
  setTimer?: typeof setTimeout;
  clearTimer?: typeof clearTimeout;
}

type RecordPhase = "queued" | "loading" | "ready" | "unloading" | "failed";

interface InstanceRecord<T> {
  key: string;
  spec: LocalModelDescriptor;
  phase: RecordPhase;
  refs: number;
  waiters: number;
  instance: T | null;
  activationController: AbortController;
  activationPromise: Promise<T>;
  unloadController: AbortController | null;
  unloadPromise: Promise<boolean> | null;
  unloadTimer: ReturnType<typeof setTimeout> | null;
  slotOwned: boolean;
  calls: number;
  totalDurationMs: number;
  lastUsedAt: number;
}

export interface InstanceManagerSnapshotEntry {
  key: string;
  phase: RecordPhase;
  tier: LocalModelTier;
  refs: number;
  waiters: number;
  calls: number;
  averageDurationMs: number;
  lastUsedAt: number;
}

export class InstanceManager<T> {
  private readonly loader: ManagedInstanceLoader<T>;
  private readonly largeSlot: LargeSlot;
  private readonly memoryGovernor: MemoryGovernor;
  private idleUnloadMs: Record<LocalModelTier, number>;
  private readonly onEvent: (event: Record<string, unknown>) => void;
  private readonly setTimer: typeof setTimeout;
  private readonly clearTimer: typeof clearTimeout;
  private readonly records = new Map<string, InstanceRecord<T>>();
  private disposed = false;

  constructor(options: InstanceManagerOptions<T>) {
    this.loader = options.loader;
    this.largeSlot = options.largeSlot;
    this.memoryGovernor = options.memoryGovernor;
    this.idleUnloadMs = {
      small: options.idleUnloadMs?.small ?? 300_000,
      large: options.idleUnloadMs?.large ?? 120_000,
    };
    this.onEvent = options.onEvent ?? (() => {});
    this.setTimer = options.setTimer ?? setTimeout;
    this.clearTimer = options.clearTimer ?? clearTimeout;
  }

  async acquire(
    spec: LocalModelDescriptor,
    options: { signal: AbortSignal; priority?: LocalModelPriority },
  ): Promise<ManagedInstanceLease<T>> {
    throwIfAborted(options.signal);
    if (this.disposed) throw new Error("InstanceManager is disposed");
    const key = localModelKey(spec);
    let record = this.records.get(key);
    if (record) this.assertSameDescriptor(record.spec, spec);

    if (record?.phase === "unloading") {
      record.unloadController?.abort();
      const unloaded = await waitWithAbort(record.unloadPromise!, options.signal);
      const resumed = this.records.get(key);
      if (!unloaded && resumed === record && resumed.phase === "ready" && resumed.instance) {
        this.cancelUnloadTimer(resumed);
        resumed.refs += 1;
        resumed.calls += 1;
        resumed.lastUsedAt = Date.now();
        this.memoryGovernor.touch(key);
        return this.createLease(resumed, resumed.instance);
      }
      record = this.records.get(key);
    }

    if (!record) {
      record = this.startRecord(spec, options.priority ?? "normal");
    }
    this.cancelUnloadTimer(record);
    record.waiters += 1;
    try {
      const instance = await waitWithAbort(record.activationPromise, options.signal);
      throwIfAborted(options.signal);
      if (record.phase !== "ready" || !record.instance) {
        throw new Error(`local model ${key} did not reach ready state`);
      }
      record.refs += 1;
      record.calls += 1;
      record.lastUsedAt = Date.now();
      this.memoryGovernor.touch(key);
      this.emit(record, "acquired");
      return this.createLease(record, instance);
    } finally {
      record.waiters = Math.max(0, record.waiters - 1);
      if (record.refs === 0 && record.waiters === 0) {
        if (record.phase === "queued" || record.phase === "loading") {
          record.activationController.abort();
        } else if (record.phase === "ready") {
          this.scheduleUnload(record);
        }
      }
    }
  }

  async preload(
    spec: LocalModelDescriptor,
    options: { signal: AbortSignal; priority?: LocalModelPriority },
  ): Promise<void> {
    const lease = await this.acquire(spec, options);
    await lease.release();
  }

  async unloadNow(key: string): Promise<boolean> {
    const record = this.records.get(key);
    if (!record) return false;
    if (record.refs > 0) throw new Error(`cannot unload in-use local model ${key}`);
    if (record.phase === "queued" || record.phase === "loading") {
      record.activationController.abort();
      await record.activationPromise.catch(() => {});
      return !this.records.has(key);
    }
    if (record.phase === "unloading") return record.unloadPromise!;
    return this.performUnload(record);
  }

  snapshot(): ReadonlyArray<Readonly<InstanceManagerSnapshotEntry>> {
    return Object.freeze([...this.records.values()].map((record) => Object.freeze({
      key: record.key,
      phase: record.phase,
      tier: record.spec.tier,
      refs: record.refs,
      waiters: record.waiters,
      calls: record.calls,
      averageDurationMs: record.calls > 0 ? record.totalDurationMs / record.calls : 0,
      lastUsedAt: record.lastUsedAt,
    })));
  }

  reconfigure(options: {
    idleUnloadMs?: Partial<Record<LocalModelTier, number>>;
    smallBudgetMb?: number;
  }): void {
    if (options.smallBudgetMb !== undefined) {
      this.memoryGovernor.setSmallBudgetMb(options.smallBudgetMb);
    }
    if (options.idleUnloadMs) {
      this.idleUnloadMs = {
        small: options.idleUnloadMs.small ?? this.idleUnloadMs.small,
        large: options.idleUnloadMs.large ?? this.idleUnloadMs.large,
      };
      for (const record of this.records.values()) {
        if (record.phase !== "ready" || record.refs > 0 || record.waiters > 0) continue;
        this.cancelUnloadTimer(record);
        this.scheduleUnload(record);
      }
    }
  }

  async unloadIdle(): Promise<string[]> {
    const candidates = [...this.records.values()].filter((record) =>
      record.phase === "ready" && record.refs === 0 && record.waiters === 0);
    const unloaded: string[] = [];
    for (const record of candidates) {
      if (await this.performUnload(record)) unloaded.push(record.key);
    }
    return unloaded;
  }

  memorySnapshot(): ReadonlyArray<Readonly<import("./memory-governor.ts").MemoryReservation>> {
    return this.memoryGovernor.snapshot();
  }

  memoryBudgetSmallMb(): number {
    return this.memoryGovernor.getSmallBudgetMb();
  }

  async dispose(): Promise<void> {
    if (this.disposed) return;
    this.disposed = true;
    const records = [...this.records.values()];
    for (const record of records) {
      this.cancelUnloadTimer(record);
      if (record.phase === "queued" || record.phase === "loading") record.activationController.abort();
      if (record.phase === "unloading") record.unloadController?.abort();
    }
    await Promise.allSettled(records.map(async (record) => {
      if (record.phase === "queued" || record.phase === "loading") {
        await record.activationPromise.catch(() => {});
      }
      const current = this.records.get(record.key);
      if (current?.phase === "unloading") {
        const unloaded = await current.unloadPromise?.catch(() => false);
        const resumed = this.records.get(record.key);
        if (!unloaded && resumed?.phase === "ready") await this.performUnload(resumed, true);
      } else if (current?.phase === "ready") {
        await this.performUnload(current, true);
      }
    }));
  }

  private startRecord(spec: LocalModelDescriptor, priority: LocalModelPriority): InstanceRecord<T> {
    const key = localModelKey(spec);
    const activationController = new AbortController();
    const record: InstanceRecord<T> = {
      key,
      spec: { ...spec },
      phase: "queued" as RecordPhase,
      refs: 0,
      waiters: 0,
      instance: null,
      activationController,
      activationPromise: Promise.resolve(null as T),
      unloadController: null,
      unloadPromise: null,
      unloadTimer: null,
      slotOwned: false,
      calls: 0,
      totalDurationMs: 0,
      lastUsedAt: Date.now(),
    };
    this.records.set(key, record);
    record.activationPromise = this.activate(record, priority);
    // 无等待者的预热/排队取消路径也必须消费拒绝，避免 unhandled rejection。
    void record.activationPromise.catch(() => {});
    return record;
  }

  private async activate(record: InstanceRecord<T>, priority: LocalModelPriority): Promise<T> {
    const startedAt = Date.now();
    try {
      if (record.spec.tier === "large") {
        await this.largeSlot.request({ key: record.key, priority, signal: record.activationController.signal });
        record.slotOwned = true;
      }
      throwIfAborted(record.activationController.signal);
      await this.memoryGovernor.reserve({
        key: record.key,
        tier: record.spec.tier,
        estimatedPeakRssMb: record.spec.estimatedPeakRssMb,
      });
      record.phase = "loading";
      this.emit(record, "loading");
      const instance = await this.loader.load(record.spec, record.activationController.signal);
      // 先挂到记录上再检查取消：若最后一个等待者在 load 即将完成时取消，
      // cleanupFailedActivation 仍能确定性卸载这个刚创建的实例。
      record.instance = instance;
      throwIfAborted(record.activationController.signal);
      record.phase = "ready";
      record.totalDurationMs += Date.now() - startedAt;
      this.memoryGovernor.registerResident({
        key: record.key,
        isInUse: () => record.refs > 0 || record.waiters > 0,
        evict: () => this.unloadNow(record.key).then(() => undefined),
      });
      this.emit(record, "ready");
      return instance;
    } catch (error) {
      record.phase = "failed";
      await this.cleanupFailedActivation(record);
      if (record.activationController.signal.aborted && !isLocalModelAbort(error)) {
        throw localModelAbortError();
      }
      throw error;
    } finally {
      if (record.phase === "ready" && record.refs === 0 && record.waiters === 0) {
        this.scheduleUnload(record);
      }
    }
  }

  private async cleanupFailedActivation(record: InstanceRecord<T>): Promise<void> {
    if (record.instance) {
      const cleanupController = new AbortController();
      await this.loader.unload(record.instance, record.spec, cleanupController.signal).catch(() => {});
      record.instance = null;
    }
    this.memoryGovernor.release(record.key);
    if (record.slotOwned) {
      record.slotOwned = false;
      this.largeSlot.release(record.key);
    }
    if (this.records.get(record.key) === record) this.records.delete(record.key);
    this.emit(record, "failed");
  }

  private createLease(record: InstanceRecord<T>, instance: T): ManagedInstanceLease<T> {
    let released = false;
    return Object.freeze({
      key: record.key,
      instance,
      release: async () => {
        if (released) return;
        released = true;
        const current = this.records.get(record.key);
        if (current !== record || record.refs <= 0) return;
        record.refs -= 1;
        record.lastUsedAt = Date.now();
        this.memoryGovernor.touch(record.key);
        this.emit(record, "released");
        if (record.refs === 0 && record.waiters === 0 && record.phase === "ready") {
          this.scheduleUnload(record);
        }
      },
    });
  }

  private scheduleUnload(record: InstanceRecord<T>): void {
    if (this.disposed || record.unloadTimer || record.refs > 0 || record.phase !== "ready") return;
    const delay = this.idleUnloadMs[record.spec.tier];
    record.unloadTimer = this.setTimer(() => {
      record.unloadTimer = null;
      void this.performUnload(record).catch((error) => {
        this.onEvent({
          type: "local_model_unload_failed",
          key: record.key,
          message: error instanceof Error ? error.message : String(error),
        });
      });
    }, delay);
    this.emit(record, "idle");
  }

  private cancelUnloadTimer(record: InstanceRecord<T>): void {
    if (!record.unloadTimer) return;
    this.clearTimer(record.unloadTimer);
    record.unloadTimer = null;
  }

  private async performUnload(record: InstanceRecord<T>, ignoreRefs = false): Promise<boolean> {
    if (record.phase === "unloading") return record.unloadPromise!;
    if (record.phase !== "ready" || !record.instance) return false;
    if (!ignoreRefs && (record.refs > 0 || record.waiters > 0)) return false;
    this.cancelUnloadTimer(record);
    const instance = record.instance;
    const beforeRssMb = await Promise.resolve(this.loader.getRssMb?.(instance, record.spec)).catch(() => undefined);
    const controller = new AbortController();
    record.unloadController = controller;
    record.phase = "unloading";
    this.emit(record, "unloading");
    record.unloadPromise = (async () => {
      try {
        await this.loader.unload(instance, record.spec, controller.signal);
        record.instance = null;
        this.memoryGovernor.release(record.key);
        if (record.slotOwned) {
          record.slotOwned = false;
          this.largeSlot.release(record.key);
        }
        if (this.records.get(record.key) === record) this.records.delete(record.key);
        const afterRssMb = await Promise.resolve(this.loader.getRssMb?.(null, record.spec)).catch(() => undefined);
        this.onEvent({
          type: "local_model_unloaded",
          key: record.key,
          beforeRssMb,
          afterRssMb,
          ...(typeof beforeRssMb === "number" && beforeRssMb > 0 && typeof afterRssMb === "number"
            ? { rssDropRatio: Math.max(0, Math.min(1, (beforeRssMb - afterRssMb) / beforeRssMb)) }
            : {}),
        });
        return true;
      } catch (error) {
        if (controller.signal.aborted) {
          record.phase = "ready";
          record.unloadController = null;
          record.unloadPromise = null;
          this.emit(record, "unload_cancelled");
          return false;
        }
        record.phase = "ready";
        record.unloadController = null;
        record.unloadPromise = null;
        this.emit(record, "unload_failed");
        throw error;
      }
    })();
    return record.unloadPromise;
  }

  private emit(record: InstanceRecord<T>, state: string): void {
    this.onEvent({
      type: "local_model_instance_state",
      state,
      key: record.key,
      tier: record.spec.tier,
      refs: record.refs,
      waiters: record.waiters,
      phase: record.phase,
    });
  }

  private assertSameDescriptor(current: LocalModelDescriptor, next: LocalModelDescriptor): void {
    for (const field of ["category", "tier", "runtimeId", "runtimeVersion", "estimatedPeakRssMb"] as const) {
      if (current[field] !== next[field]) {
        throw new Error(`conflicting descriptor for ${localModelKey(current)}: ${field}`);
      }
    }
  }
}

function waitWithAbort<T>(promise: Promise<T>, signal: AbortSignal): Promise<T> {
  throwIfAborted(signal);
  return new Promise<T>((resolve, reject) => {
    const onAbort = () => reject(localModelAbortError());
    signal.addEventListener("abort", onAbort, { once: true });
    promise.then(
      (value) => {
        signal.removeEventListener("abort", onAbort);
        resolve(value);
      },
      (error) => {
        signal.removeEventListener("abort", onAbort);
        reject(error);
      },
    );
  });
}
