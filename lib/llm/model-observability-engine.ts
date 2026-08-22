/**
 * Model Observatory 运行时代际协调器。
 *
 * 每次安装得到一个独立代际。切换时先把旧代从全局注册表摘下，因此新调用只会
 * 绑定新代；已经持有旧 observer / sink 的调用仍可把终态写完。旧代在活动调用
 * 归零后关闭，异常永不结束的调用则由有界超时收口。
 */
import {
  getModelCallObserver,
  setModelCallObserver,
  type ModelCallEvent,
  type ModelCallObserver,
} from "./model-call-observer.ts";
import {
  getModelCallBlobExternalizer,
  getModelCallPayloadSink,
  setModelCallBlobExternalizer,
  setModelCallPayloadSink,
  type ModelCallPayloadSink,
} from "./model-call-payload-capture.ts";
import type { ModelCallPayloadRecord } from "./model-call-payload-types.ts";
import {
  installModelObservabilityPersistence,
  type ModelObservabilityPersistenceHandle,
  type ModelObservabilityPersistencePolicy,
} from "./model-observability-persistence.ts";

type PersistenceInstallOptions = {
  lingxiHome: string;
  policy: ModelObservabilityPersistencePolicy;
  /** 同一进程内换代不是重启，不能把旧代的在途调用标成重启中断。 */
  reconcileAfterRestart?: boolean;
};

type PersistenceInstaller = (options: PersistenceInstallOptions) => ModelObservabilityPersistenceHandle;

export type ModelObservabilityGenerationState = {
  activeGeneration: number | null;
  activeCalls: number;
  retiringGenerations: number;
};

export type ModelObservabilityGenerationManager = {
  readonly current: ModelObservabilityPersistenceHandle | null;
  reconfigure(policy: ModelObservabilityPersistencePolicy | null): ModelObservabilityPersistenceHandle | null;
  getState(): ModelObservabilityGenerationState;
  waitForRetired(): Promise<void>;
  close(): Promise<void>;
};

type Generation = {
  id: number;
  handle: ModelObservabilityPersistenceHandle;
  activeCallIds: Set<string>;
  observer: ModelCallObserver | null;
  sink: ModelCallPayloadSink | null;
  retire(): Promise<void>;
};

function safeForwardObserver(observer: ModelCallObserver | null, event: ModelCallEvent): void {
  if (!observer) return;
  try { observer.handleModelCallEvent(event); } catch { /* 旁路故障不影响模型调用 */ }
}

function safeForwardSink(sink: ModelCallPayloadSink | null, record: ModelCallPayloadRecord): void {
  if (!sink) return;
  try { sink.handleModelCallPayloadRecord(record); } catch { /* 旁路故障不影响模型调用 */ }
}

export function createModelObservabilityGenerationManager({
  lingxiHome,
  drainTimeoutMs = 5_000,
  install = installModelObservabilityPersistence,
}: {
  lingxiHome: string;
  drainTimeoutMs?: number;
  install?: PersistenceInstaller;
}): ModelObservabilityGenerationManager {
  const boundedDrainTimeoutMs = Number.isFinite(drainTimeoutMs) && drainTimeoutMs >= 1
    ? Math.floor(drainTimeoutMs)
    : 5_000;
  let nextGenerationId = 1;
  let active: Generation | null = null;
  let hasOpenedStoreInThisProcess = false;
  let closing = false;
  const retirementPromises = new Set<Promise<void>>();

  function createGeneration(policy: ModelObservabilityPersistencePolicy): {
    generation: Generation | null;
    handle: ModelObservabilityPersistenceHandle;
  } {
    const priorObserver = getModelCallObserver();
    const priorSink = getModelCallPayloadSink();
    const priorExternalizer = getModelCallBlobExternalizer();
    const handle = install({
      lingxiHome,
      policy,
      reconcileAfterRestart: !hasOpenedStoreInThisProcess,
    });
    if (handle.getHealth().status !== "active") return { generation: null, handle };
    hasOpenedStoreInThisProcess = true;

    const id = nextGenerationId;
    nextGenerationId += 1;
    const activeCallIds = new Set<string>();
    const installedObserver = handle.observer;
    const installedSink = handle.sink;
    const installedExternalizer = getModelCallBlobExternalizer();
    let retired = false;
    let finalizing = false;
    let closeScheduled = false;
    let retirementTimer: NodeJS.Timeout | null = null;
    let resolveRetirement: (() => void) | null = null;
    const retirement = new Promise<void>((resolve) => {
      resolveRetirement = resolve;
    });

    const finalizeClose = async (): Promise<void> => {
      if (finalizing) return;
      finalizing = true;
      if (retirementTimer) clearTimeout(retirementTimer);
      try {
        await handle.close();
      } catch {
        // 观测存储关闭失败不能影响模型调用或设置切换。
      } finally {
        resolveRetirement?.();
        resolveRetirement = null;
      }
    };

    const scheduleClose = (): void => {
      if (!retired || activeCallIds.size > 0 || closeScheduled || finalizing) return;
      closeScheduled = true;
      // logical_call_end 后的同一调用栈仍可能同步补记 usage；让它先走完。
      setImmediate(() => {
        closeScheduled = false;
        if (retired && activeCallIds.size === 0) void finalizeClose();
      });
    };

    const observer: ModelCallObserver = {
      handleModelCallEvent(event) {
        const isStart = event.eventType === "logical_call_start";
        const tracked = activeCallIds.has(event.callId);
        if (retired && !tracked) {
          // 旧 recorder 若在换代后才开始，不再进入旧代，但仍保留原旁路 observer。
          safeForwardObserver(priorObserver, event);
          return;
        }
        if (isStart) activeCallIds.add(event.callId);
        try {
          safeForwardObserver(installedObserver ?? priorObserver, event);
        } finally {
          if (event.eventType === "logical_call_end") {
            activeCallIds.delete(event.callId);
            scheduleClose();
          }
        }
      },
    };

    const sink: ModelCallPayloadSink | null = installedSink
      ? {
        handleModelCallPayloadRecord(record) {
          if (retired && !activeCallIds.has(record.callId)) {
            safeForwardSink(priorSink, record);
            return;
          }
          installedSink.handleModelCallPayloadRecord(record);
        },
      }
      : null;

    setModelCallObserver(observer);
    if (sink) setModelCallPayloadSink(sink);

    const generation: Generation = {
      id,
      handle,
      activeCallIds,
      observer,
      sink,
      retire() {
        if (retired) return retirement;
        retired = true;

        // 先从全局入口摘下旧代。已有 recorder/capture 持有 wrapper 引用，仍能排水。
        if (getModelCallObserver() === observer) setModelCallObserver(installedObserver ?? priorObserver);
        if (sink && getModelCallPayloadSink() === sink) setModelCallPayloadSink(installedSink);

        // persistence 自己保存了安装前注册对象。先让它完成幂等卸载，再把第三方后来
        // 注册的 externalizer 放回，避免当前旧实现覆盖后继者。
        const externalizerAtRetirement = getModelCallBlobExternalizer();
        handle.uninstall();
        if (installedExternalizer) {
          setModelCallBlobExternalizer(
            externalizerAtRetirement === installedExternalizer
              ? priorExternalizer
              : externalizerAtRetirement,
          );
        }

        retirementTimer = setTimeout(() => { void finalizeClose(); }, boundedDrainTimeoutMs);
        retirementTimer.unref?.();
        scheduleClose();
        return retirement;
      },
    };
    return { generation, handle };
  }

  function trackRetirement(generation: Generation): void {
    const retirement = generation.retire();
    retirementPromises.add(retirement);
    void retirement.finally(() => retirementPromises.delete(retirement));
  }

  const manager: ModelObservabilityGenerationManager = {
    get current() {
      return active?.handle ?? null;
    },
    reconfigure(policy) {
      if (closing) return null;
      const previous = active;
      active = null;
      if (previous) trackRetirement(previous);
      if (!policy?.enabled) return null;
      const installed = createGeneration(policy);
      active = installed.generation;
      return installed.handle;
    },
    getState() {
      return {
        activeGeneration: active?.id ?? null,
        activeCalls: active?.activeCallIds.size ?? 0,
        retiringGenerations: retirementPromises.size,
      };
    },
    async waitForRetired() {
      while (retirementPromises.size > 0) {
        await Promise.all([...retirementPromises]);
      }
    },
    async close() {
      if (!closing) {
        closing = true;
        const previous = active;
        active = null;
        if (previous) trackRetirement(previous);
      }
      await manager.waitForRetired();
    },
  };
  return manager;
}
