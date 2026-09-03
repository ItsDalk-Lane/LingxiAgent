import { afterEach, describe, expect, it, vi } from "vitest";
import {
  InstanceManager,
  LargeSlot,
  MemoryGovernor,
  localModelKey,
  type LocalModelDescriptor,
  type ManagedInstanceLoader,
} from "../lib/local-models/index.ts";

interface FakeInstance { key: string }

const SMALL: LocalModelDescriptor = {
  id: "sensevoice-small",
  quant: "int8",
  manifestVersion: "test-v1",
  category: "stt",
  tier: "small",
  runtimeId: "fake",
  runtimeVersion: "1",
  estimatedPeakRssMb: 100,
};

const LARGE_ASR: LocalModelDescriptor = {
  ...SMALL,
  id: "qwen3-asr-1.7b",
  quant: "q4_k_m",
  category: "stt",
  tier: "large",
  estimatedPeakRssMb: 1000,
};

const LARGE_TTS: LocalModelDescriptor = {
  ...LARGE_ASR,
  id: "indextts-2.5",
  quant: "q4",
  category: "tts",
};

afterEach(() => {
  vi.useRealTimers();
});

function createManager(
  loader: ManagedInstanceLoader<FakeInstance>,
  idleUnloadMs: { small?: number; large?: number } = { small: 0, large: 0 },
) {
  return new InstanceManager({
    loader,
    largeSlot: new LargeSlot(),
    memoryGovernor: new MemoryGovernor({ getAvailableMemoryMb: () => 16_384 }),
    idleUnloadMs,
  });
}

function immediateLoader() {
  return {
    load: vi.fn(async (spec: LocalModelDescriptor) => ({ key: localModelKey(spec) })),
    unload: vi.fn(async () => {}),
  } satisfies ManagedInstanceLoader<FakeInstance>;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("InstanceManager", () => {
  it("shares one cold load across concurrent acquires and unloads once after the last release", async () => {
    vi.useFakeTimers();
    const loader = immediateLoader();
    const manager = createManager(loader);
    const signal = new AbortController().signal;
    const leases = await Promise.all(Array.from({ length: 20 }, () => manager.acquire(SMALL, { signal })));

    expect(loader.load).toHaveBeenCalledTimes(1);
    expect(manager.snapshot()[0]).toMatchObject({ refs: 20, phase: "ready" });
    await Promise.all(leases.map((lease) => lease.release()));
    await vi.runAllTimersAsync();
    expect(loader.unload).toHaveBeenCalledTimes(1);
    expect(manager.snapshot()).toEqual([]);
  });

  it("cancels the underlying load when the final waiter aborts and unloads a late result", async () => {
    const loadResult = deferred<FakeInstance>();
    const loader = {
      load: vi.fn(() => loadResult.promise),
      unload: vi.fn(async () => {}),
    } satisfies ManagedInstanceLoader<FakeInstance>;
    const manager = createManager(loader);
    const controller = new AbortController();
    const acquiring = manager.acquire(SMALL, { signal: controller.signal });
    await vi.waitFor(() => expect(loader.load).toHaveBeenCalledTimes(1));
    controller.abort();
    await expect(acquiring).rejects.toMatchObject({ code: "LOCAL_MODEL_ABORTED" });
    loadResult.resolve({ key: localModelKey(SMALL) });
    await vi.waitFor(() => expect(loader.unload).toHaveBeenCalledTimes(1));
    expect(manager.snapshot()).toEqual([]);
  });

  it("does not cancel a shared load when another waiter still needs it", async () => {
    const loadResult = deferred<FakeInstance>();
    const loader = {
      load: vi.fn(() => loadResult.promise),
      unload: vi.fn(async () => {}),
    } satisfies ManagedInstanceLoader<FakeInstance>;
    const manager = createManager(loader, { small: 60_000 });
    const firstController = new AbortController();
    const secondController = new AbortController();
    const first = manager.acquire(SMALL, { signal: firstController.signal });
    const second = manager.acquire(SMALL, { signal: secondController.signal });
    await vi.waitFor(() => expect(loader.load).toHaveBeenCalledTimes(1));
    firstController.abort();
    await expect(first).rejects.toMatchObject({ code: "LOCAL_MODEL_ABORTED" });
    loadResult.resolve({ key: localModelKey(SMALL) });
    const lease = await second;
    expect(lease.instance.key).toBe(localModelKey(SMALL));
    expect(manager.snapshot()[0]).toMatchObject({ refs: 1, waiters: 0, phase: "ready" });
    await lease.release();
    await manager.dispose();
    expect(loader.unload).toHaveBeenCalledTimes(1);
  });

  it("cancels an abort-aware unload and reuses the instance when a new request arrives", async () => {
    vi.useFakeTimers();
    let unloadCalls = 0;
    const loader = {
      load: vi.fn(async () => ({ key: localModelKey(SMALL) })),
      unload: vi.fn((_instance: FakeInstance, _spec: LocalModelDescriptor, signal: AbortSignal) => {
        unloadCalls += 1;
        if (unloadCalls > 1) return Promise.resolve();
        return new Promise<void>((_resolve, reject) => {
          signal.addEventListener("abort", () => reject(new Error("unload cancelled")), { once: true });
        });
      }),
    } satisfies ManagedInstanceLoader<FakeInstance>;
    const manager = createManager(loader);
    const signal = new AbortController().signal;
    const first = await manager.acquire(SMALL, { signal });
    await first.release();
    await vi.runOnlyPendingTimersAsync();
    await vi.waitFor(() => expect(loader.unload).toHaveBeenCalledTimes(1));

    const second = await manager.acquire(SMALL, { signal });
    expect(loader.load).toHaveBeenCalledTimes(1);
    expect(second.instance).toBe(first.instance);
    await second.release();
    await manager.unloadNow(localModelKey(SMALL));
    expect(loader.unload).toHaveBeenCalledTimes(2);
  });

  it("does not double-unload when dispose races with an idle unload", async () => {
    vi.useFakeTimers();
    const unloadResult = deferred<void>();
    const loader = {
      load: vi.fn(async () => ({ key: localModelKey(SMALL) })),
      unload: vi.fn(() => unloadResult.promise),
    } satisfies ManagedInstanceLoader<FakeInstance>;
    const manager = createManager(loader);
    const lease = await manager.acquire(SMALL, { signal: new AbortController().signal });
    await lease.release();
    await vi.runOnlyPendingTimersAsync();
    await vi.waitFor(() => expect(loader.unload).toHaveBeenCalledTimes(1));
    const disposing = manager.dispose();
    unloadResult.resolve();
    await disposing;
    expect(loader.unload).toHaveBeenCalledTimes(1);
  });

  it("reschedules an idle instance when the unload timeout changes", async () => {
    vi.useFakeTimers();
    const loader = immediateLoader();
    const manager = createManager(loader, { small: 60_000 });
    const lease = await manager.acquire(SMALL, { signal: new AbortController().signal });
    await lease.release();

    manager.reconfigure({ idleUnloadMs: { small: 0 } });
    await vi.runAllTimersAsync();

    expect(loader.unload).toHaveBeenCalledTimes(1);
    expect(manager.snapshot()).toEqual([]);
  });

  it("applies a changed memory budget to later loads", async () => {
    const loader = immediateLoader();
    const manager = new InstanceManager({
      loader,
      largeSlot: new LargeSlot(),
      memoryGovernor: new MemoryGovernor({ smallBudgetMb: 1_536, getAvailableMemoryMb: () => 16_384 }),
      idleUnloadMs: { small: 60_000 },
    });
    manager.reconfigure({ smallBudgetMb: 100 });

    await expect(manager.acquire(SMALL, { signal: new AbortController().signal }))
      .rejects.toMatchObject({ code: "LOCAL_MODEL_MEMORY_INSUFFICIENT" });
    expect(loader.load).not.toHaveBeenCalled();
  });

  it("fully unloads one large instance before loading a different large model", async () => {
    vi.useFakeTimers();
    let activeLarge = 0;
    let maxActiveLarge = 0;
    const loadOrder: string[] = [];
    const loader = {
      load: vi.fn(async (spec: LocalModelDescriptor) => {
        activeLarge += 1;
        maxActiveLarge = Math.max(maxActiveLarge, activeLarge);
        loadOrder.push(`load:${spec.id}`);
        return { key: localModelKey(spec) };
      }),
      unload: vi.fn(async (_instance: FakeInstance, spec: LocalModelDescriptor) => {
        loadOrder.push(`unload:${spec.id}`);
        activeLarge -= 1;
      }),
    } satisfies ManagedInstanceLoader<FakeInstance>;
    const manager = createManager(loader);
    const signal = new AbortController().signal;
    const asr = await manager.acquire(LARGE_ASR, { signal });
    const queuedTts = manager.acquire(LARGE_TTS, { signal, priority: "interactive" });
    await Promise.resolve();
    expect(manager.snapshot().find((entry) => entry.key === localModelKey(LARGE_TTS))).toMatchObject({ phase: "queued" });
    await asr.release();
    await vi.runAllTimersAsync();
    const tts = await queuedTts;
    expect(maxActiveLarge).toBe(1);
    expect(loadOrder).toEqual([
      "load:qwen3-asr-1.7b",
      "unload:qwen3-asr-1.7b",
      "load:indextts-2.5",
    ]);
    await tts.release();
    await vi.runAllTimersAsync();
    expect(activeLarge).toBe(0);
  });
});
