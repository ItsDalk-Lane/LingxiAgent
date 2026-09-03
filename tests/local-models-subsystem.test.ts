import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LocalModelsSubsystem } from "../lib/local-models/index.ts";

const roots: string[] = [];

afterEach(() => {
  for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
});

function tempRoot(): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-local-model-subsystem-"));
  roots.push(root);
  return root;
}

function sha256(value: Buffer | string): string {
  return createHash("sha256").update(value).digest("hex");
}

function manualBundle(): string {
  const root = tempRoot();
  const model = Buffer.from("manual local model");
  fs.writeFileSync(path.join(root, "model.onnx"), model);
  const files = [{ path: "model.onnx", bytes: model.length, sha256: sha256(model) }];
  fs.writeFileSync(path.join(root, "model.json"), `${JSON.stringify({
    schemaVersion: 1,
    id: "sensevoice-small",
    category: "stt",
    quant: "int8",
    tier: "small",
    version: "manual-v1",
    runtimeId: "sherpa-onnx",
    runtimeVersion: "1",
    runtimeKind: "in-process",
    estimatedPeakRssMb: 800,
    runtimeArgs: [],
    capabilities: { languages: ["zh"] },
    source: "manual",
    installedAt: "2026-09-02T00:00:00.000Z",
    integrity: "unknown",
    bytes: model.length,
    sha256Manifest: sha256(JSON.stringify(files)),
    files,
  }, null, 2)}\n`);
  return root;
}

describe("LocalModelsSubsystem", () => {
  it("initializes without assets, persists normalized settings, imports, reports sanitized state, and removes", async () => {
    const home = tempRoot();
    let preferences: Record<string, unknown> = { locale: "zh" };
    const savePreferences = vi.fn(async (next: Record<string, unknown>) => { preferences = next; });
    const events: Record<string, unknown>[] = [];
    const subsystem = new LocalModelsSubsystem({
      lingxiHome: home,
      getPreferences: () => preferences,
      savePreferences,
      emitEvent: (event) => events.push(event),
    });
    await subsystem.initialize({ signal: new AbortController().signal });

    const empty = await subsystem.state();
    expect(empty.manifest).toMatchObject({ source: "builtin", configured: false });
    expect(empty.catalog).toHaveLength(4);
    expect(empty.installed).toEqual([]);

    const config = await subsystem.setConfig({
      backend: "cpu",
      threads: 8,
      memoryBudgetSmallMb: 2048,
      download: { concurrency: 2, mirrorBaseUrl: "https://mirror.example" },
    });
    expect(config).toMatchObject({
      backend: "cpu",
      threads: 8,
      memoryBudgetSmallMb: 2048,
      download: { concurrency: 2, mirrorBaseUrl: "https://mirror.example" },
    });
    expect(savePreferences).toHaveBeenCalledTimes(1);
    expect(preferences).toMatchObject({ locale: "zh", localModels: config });
    expect((await subsystem.state()).resources).toMatchObject({ memoryBudgetSmallMb: 2048 });

    const source = manualBundle();
    await subsystem.importDirectory(source, { signal: new AbortController().signal });
    const installed = await subsystem.state();
    expect(installed.installed).toEqual([expect.objectContaining({
      id: "sensevoice-small",
      quant: "int8",
      source: "manual",
      integrity: "unknown",
      runtimeKind: "in-process",
    })]);
    expect(JSON.stringify(installed)).not.toContain(source);
    expect(JSON.stringify(installed)).not.toContain("model.onnx");

    expect(await subsystem.remove("stt", "sensevoice-small", "int8", {
      signal: new AbortController().signal,
    })).toBe(true);
    expect((await subsystem.state()).installed).toEqual([]);
    expect(events.some((event) => event.kind === "model_imported")).toBe(true);
    expect(events.some((event) => event.kind === "model_removed")).toBe(true);
    expect(JSON.stringify(events)).not.toContain(source);
    await subsystem.dispose();
  });

  it("runs enabled small-model preload as a bounded background task", async () => {
    const home = tempRoot();
    let preferences: Record<string, unknown> = {};
    const events: Record<string, unknown>[] = [];
    const subsystem = new LocalModelsSubsystem({
      lingxiHome: home,
      getPreferences: () => preferences,
      savePreferences: (next) => { preferences = next; },
      emitEvent: (event) => events.push(event),
    });
    await subsystem.initialize({ signal: new AbortController().signal });
    await subsystem.importDirectory(manualBundle(), { signal: new AbortController().signal });
    await subsystem.setConfig({ preloadSmall: true });

    await vi.waitFor(() => expect(events.some((event) => event.kind === "preload_finished")).toBe(true));
    expect(events.find((event) => event.kind === "preload_finished")).toMatchObject({
      loadedCount: 0,
      failedCount: 1,
    });
    await subsystem.dispose();
  });

  it("does not start a download when no verified remote manifest is available", async () => {
    const subsystem = new LocalModelsSubsystem({
      lingxiHome: tempRoot(),
      getPreferences: () => ({}),
      savePreferences: () => {},
    });
    await subsystem.initialize({ signal: new AbortController().signal });
    await expect(subsystem.install("sensevoice-small", "int8", {
      signal: new AbortController().signal,
    })).rejects.toMatchObject({ code: "LOCAL_MODEL_MANIFEST_INVALID" });
    expect((await subsystem.state()).downloads).toEqual([]);
    await subsystem.dispose();
  });
});
