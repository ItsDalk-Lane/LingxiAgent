import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createFileProbeCache } from "../lib/local-models/backend-probe.ts";
import { resolveLargeResidentCapacity } from "../lib/local-models/config.ts";
import { LargeSlot } from "../lib/local-models/large-slot.ts";

const GiB = 1024 * 1024 * 1024;
const signal = new AbortController().signal;
const roots: string[] = [];

afterEach(() => {
  for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
});

describe("resolveLargeResidentCapacity", () => {
  it("auto mode requires at least 32GiB of physical memory for two resident large models", () => {
    expect(resolveLargeResidentCapacity("auto", 96 * GiB)).toBe(2);
    expect(resolveLargeResidentCapacity("auto", 32 * GiB)).toBe(2);
    expect(resolveLargeResidentCapacity("auto", 32 * GiB - 1)).toBe(1);
    expect(resolveLargeResidentCapacity("auto", 16 * GiB)).toBe(1);
  });

  it("explicit config overrides the device check in both directions", () => {
    expect(resolveLargeResidentCapacity(1, 96 * GiB)).toBe(1);
    expect(resolveLargeResidentCapacity(2, 8 * GiB)).toBe(2);
  });

  it("rejects non-finite memory readings", () => {
    expect(resolveLargeResidentCapacity("auto", Number.NaN)).toBe(1);
  });
});

describe("LargeSlot capacity", () => {
  it("grants up to capacity concurrently and queues the rest", async () => {
    const slot = new LargeSlot(undefined, { capacity: 2 });
    await slot.request({ key: "a", signal });
    await slot.request({ key: "b", signal });
    expect(slot.snapshot().activeKeys).toEqual(["a", "b"]);
    let granted = false;
    const pending = slot.request({ key: "c", signal }).then(() => { granted = true; });
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(granted).toBe(false);
    expect(slot.snapshot().queue.map((entry) => entry.key)).toEqual(["c"]);
    slot.release("a");
    await pending;
    expect(slot.snapshot().activeKeys).toEqual(["b", "c"]);
    slot.release("b");
    slot.release("c");
    expect(slot.snapshot().activeKeys).toEqual([]);
  });

  it("serves a queued higher-priority model before an older batch request", async () => {
    const slot = new LargeSlot(undefined, { capacity: 1 });
    await slot.request({ key: "resident", priority: "batch", signal });
    const batchPending = slot.request({ key: "batch-old", priority: "batch", signal });
    const interactivePending = slot.request({ key: "interactive-new", priority: "interactive", signal });
    slot.release("resident");
    await interactivePending;
    expect(slot.snapshot().activeKey).toBe("interactive-new");
    slot.release("interactive-new");
    await batchPending;
    expect(slot.snapshot().activeKey).toBe("batch-old");
    slot.release("batch-old");
  });

  it("keeps same-key re-request cheap and rejects out-of-band releases", async () => {
    const slot = new LargeSlot(undefined, { capacity: 2 });
    await slot.request({ key: "a", signal });
    await slot.request({ key: "a", signal });
    expect(slot.snapshot().activeKeys).toEqual(["a"]);
    expect(() => slot.release("not-active")).toThrow(/release mismatch/);
    slot.release("a");
    expect(slot.snapshot().activeKey).toBeNull();
  });

  it("validates capacity bounds", () => {
    expect(() => new LargeSlot(undefined, { capacity: 0 })).toThrow(/capacity/);
    expect(() => new LargeSlot(undefined, { capacity: 9 })).toThrow(/capacity/);
    expect(() => new LargeSlot(undefined, { capacity: 1.5 })).toThrow(/capacity/);
  });
});

describe("createFileProbeCache", () => {
  it("round-trips entries and merges concurrent writers", async () => {
    const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-probe-cache-")), "probe-cache.json");
    roots.push(path.dirname(file));
    const cache = createFileProbeCache(file);
    await cache.write("llama.cpp@1|darwin-arm64|false|auto", "metal");
    await cache.write("audio.cpp@2|darwin-arm64|false|auto", "metal");
    expect(await cache.read("llama.cpp@1|darwin-arm64|false|auto")).toBe("metal");
    expect(await cache.read("audio.cpp@2|darwin-arm64|false|auto")).toBe("metal");
    expect(await cache.read("missing-key")).toBeNull();
  });

  it("treats missing or corrupt cache files as a miss", async () => {
    const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-probe-cache-")), "probe-cache.json");
    roots.push(path.dirname(file));
    const cache = createFileProbeCache(file);
    expect(await cache.read("any")).toBeNull();
    await fs.writeFileSync(file, "{not json");
    expect(await cache.read("any")).toBeNull();
    await fs.writeFileSync(file, JSON.stringify({ schemaVersion: 1, entries: { key: "teleport" } }));
    expect(await cache.read("key")).toBeNull();
  });
});
