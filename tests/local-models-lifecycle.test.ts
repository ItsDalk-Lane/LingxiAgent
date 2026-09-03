import { describe, expect, it, vi } from "vitest";
import {
  BackendProbe,
  LargeSlot,
  LocalModelError,
  MemoryGovernor,
  backendCandidates,
} from "../lib/local-models/index.ts";

describe("LargeSlot", () => {
  it("keeps mixed N=20 requests on exactly one large model at a time", async () => {
    const states: Array<{ activeKey: string | null; queueLength: number }> = [];
    const slot = new LargeSlot((state) => states.push({ activeKey: state.activeKey, queueLength: state.queue.length }));
    const controllers = Array.from({ length: 20 }, () => new AbortController());
    const firstWave = controllers.slice(0, 10).map((controller) =>
      slot.request({ key: "large-asr", signal: controller.signal }));
    const secondWave = controllers.slice(10).map((controller) =>
      slot.request({ key: "large-tts", signal: controller.signal }));

    await Promise.all(firstWave);
    expect(slot.snapshot().activeKey).toBe("large-asr");
    expect(slot.snapshot().queue).toHaveLength(10);
    expect(states.every((state) => state.activeKey === null || ["large-asr", "large-tts"].includes(state.activeKey))).toBe(true);

    let secondResolved = false;
    void Promise.all(secondWave).then(() => { secondResolved = true; });
    await Promise.resolve();
    expect(secondResolved).toBe(false);
    slot.release("large-asr");
    await Promise.all(secondWave);
    expect(slot.snapshot().activeKey).toBe("large-tts");
    slot.release("large-tts");
    expect(slot.snapshot()).toEqual({ activeKey: null, activeKeys: [], capacity: 1, queue: [] });
  });

  it("allows interactive requests to overtake batch work without breaking FIFO inside a priority", async () => {
    const slot = new LargeSlot();
    const signals = Array.from({ length: 5 }, () => new AbortController().signal);
    await slot.request({ key: "active", signal: signals[0] });
    const order: string[] = [];
    const batch = slot.request({ key: "batch", priority: "batch", signal: signals[1] }).then(() => order.push("batch"));
    const normalA = slot.request({ key: "normal-a", priority: "normal", signal: signals[2] }).then(() => order.push("normal-a"));
    const normalB = slot.request({ key: "normal-b", priority: "normal", signal: signals[3] }).then(() => order.push("normal-b"));
    const interactive = slot.request({ key: "interactive", priority: "interactive", signal: signals[4] }).then(() => order.push("interactive"));

    slot.release("active");
    await interactive;
    expect(order).toEqual(["interactive"]);
    slot.release("interactive");
    await normalA;
    expect(order).toEqual(["interactive", "normal-a"]);
    slot.release("normal-a");
    await normalB;
    slot.release("normal-b");
    await batch;
    slot.release("batch");
    expect(order).toEqual(["interactive", "normal-a", "normal-b", "batch"]);
  });

  it("removes an aborted queued request without loading it", async () => {
    const slot = new LargeSlot();
    const active = new AbortController();
    const queued = new AbortController();
    await slot.request({ key: "active", signal: active.signal });
    const promise = slot.request({ key: "cancelled", signal: queued.signal });
    queued.abort();
    await expect(promise).rejects.toMatchObject({ code: "LOCAL_MODEL_ABORTED" });
    expect(slot.snapshot()).toEqual({ activeKey: "active", activeKeys: ["active"], capacity: 1, queue: [] });
    slot.release("active");
  });
});

describe("MemoryGovernor", () => {
  it("applies the 1.25 safety factor and the aggregate small budget", async () => {
    const governor = new MemoryGovernor({
      smallBudgetMb: 1536,
      getAvailableMemoryMb: () => 4096,
      now: () => 10,
    });
    await expect(governor.reserve({ key: "sensevoice", tier: "small", estimatedPeakRssMb: 800 }))
      .resolves.toMatchObject({ reservedMb: 1000 });
    await expect(governor.reserve({ key: "kokoro", tier: "small", estimatedPeakRssMb: 600 }))
      .rejects.toMatchObject({ code: "LOCAL_MODEL_MEMORY_INSUFFICIENT" });
    governor.release("sensevoice");
    await expect(governor.reserve({ key: "kokoro", tier: "small", estimatedPeakRssMb: 600 }))
      .resolves.toMatchObject({ reservedMb: 750 });
  });

  it("rejects a load when system available memory is below the reserved peak", async () => {
    const governor = new MemoryGovernor({ getAvailableMemoryMb: () => 999 });
    await expect(governor.reserve({ key: "large", tier: "large", estimatedPeakRssMb: 800 }))
      .rejects.toBeInstanceOf(LocalModelError);
  });

  it("changes the small-model budget without rewriting existing reservations", async () => {
    const governor = new MemoryGovernor({ smallBudgetMb: 1_536, getAvailableMemoryMb: () => 8_192 });
    await governor.reserve({ key: "first", tier: "small", estimatedPeakRssMb: 800 });
    governor.setSmallBudgetMb(1_100);

    expect(governor.getSmallBudgetMb()).toBe(1_100);
    expect(governor.snapshot()).toEqual([expect.objectContaining({ key: "first", reservedMb: 1_000 })]);
    await expect(governor.reserve({ key: "second", tier: "small", estimatedPeakRssMb: 100 }))
      .rejects.toMatchObject({ code: "LOCAL_MODEL_MEMORY_INSUFFICIENT" });
  });

  it("evicts unused large residents before small residents under pressure", async () => {
    let now = 0;
    const order: string[] = [];
    const governor = new MemoryGovernor({ getAvailableMemoryMb: () => 8192, now: () => now });
    await governor.reserve({ key: "small-old", tier: "small", estimatedPeakRssMb: 100 });
    now = 10;
    await governor.reserve({ key: "large-new", tier: "large", estimatedPeakRssMb: 100 });
    now = 20;
    await governor.reserve({ key: "large-busy", tier: "large", estimatedPeakRssMb: 100 });
    governor.registerResident({ key: "small-old", isInUse: () => false, evict: async () => { order.push("small-old"); } });
    governor.registerResident({ key: "large-new", isInUse: () => false, evict: async () => { order.push("large-new"); } });
    governor.registerResident({ key: "large-busy", isInUse: () => true, evict: async () => { order.push("large-busy"); } });

    expect(await governor.handlePressure("critical")).toEqual(["large-new", "small-old"]);
    expect(order).toEqual(["large-new", "small-old"]);
  });
});

describe("BackendProbe", () => {
  it("uses the required platform-specific order", () => {
    expect(backendCandidates("win32", "x64", true)).toEqual(["cuda", "vulkan", "directml", "cpu"]);
    expect(backendCandidates("win32", "x64", false)).toEqual(["directml", "vulkan", "cpu"]);
    expect(backendCandidates("darwin", "arm64", false)).toEqual(["metal", "coreml", "cpu"]);
    expect(backendCandidates("darwin", "x64", false)).toEqual(["metal", "cpu"]);
    expect(backendCandidates("linux", "x64", false)).toEqual(["cuda", "vulkan", "cpu"]);
  });

  it("validates candidates instead of trusting hardware names and caches success", async () => {
    const validate = vi.fn(async (backend: string) => ({
      available: backend === "directml",
      ...(backend === "vulkan" ? { reason: "initialization failed" } : {}),
    }));
    const probe = new BackendProbe();
    const signal = new AbortController().signal;
    const first = await probe.probe({ platform: "win32", arch: "x64", hasNvidiaGpu: true, signal, validate });
    const second = await probe.probe({ platform: "win32", arch: "x64", hasNvidiaGpu: true, signal, validate });
    expect(first.backend).toBe("directml");
    expect(first.attempts.map((attempt) => attempt.backend)).toEqual(["cuda", "vulkan", "directml"]);
    expect(second).toBe(first);
    expect(validate).toHaveBeenCalledTimes(3);
  });
});
