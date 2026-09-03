import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  BackendProbe,
  DEFAULT_LOCAL_MODELS_CONFIG,
  InProcessInstanceFactory,
  type InProcessRuntimeCreateOptions,
  type InProcessRuntimeModule,
  type LocalModelDescriptor,
  type LocalModelRegistryEntry,
} from "../lib/local-models/index.ts";

const roots: string[] = [];

afterEach(() => {
  for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
});

function tempRoot(): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-in-process-factory-"));
  roots.push(root);
  return root;
}

function sha256(value: Buffer): string {
  return createHash("sha256").update(value).digest("hex");
}

function fixture(category: "stt" | "tts" = "stt") {
  const root = tempRoot();
  const runtimeDir = path.join(root, "runtime", "sherpa-onnx", "1", "darwin-arm64");
  const modelDir = path.join(root, "models", category, `fixture-${category}@int8`);
  fs.mkdirSync(runtimeDir, { recursive: true });
  fs.mkdirSync(modelDir, { recursive: true });
  const moduleBytes = Buffer.from("exported runtime wrapper");
  fs.writeFileSync(path.join(runtimeDir, "runtime.mjs"), moduleBytes);
  fs.writeFileSync(path.join(runtimeDir, "runtime.json"), `${JSON.stringify({
    schemaVersion: 1,
    id: "sherpa-onnx",
    version: "1",
    platform: "darwin-arm64",
    kind: "in-process",
    entrypoint: "runtime.mjs",
    packageSha256: "a".repeat(64),
    installedAt: "2026-09-02T00:00:00.000Z",
    files: [{ path: "runtime.mjs", bytes: moduleBytes.length, sha256: sha256(moduleBytes) }],
  }, null, 2)}\n`);
  const descriptor: LocalModelDescriptor = {
    id: `fixture-${category}`,
    quant: "int8",
    manifestVersion: "models-v1",
    category,
    tier: "small",
    runtimeId: "sherpa-onnx",
    runtimeVersion: "1",
    estimatedPeakRssMb: 128,
  };
  const installed: LocalModelRegistryEntry = {
    id: descriptor.id,
    quant: descriptor.quant,
    version: descriptor.manifestVersion,
    category: descriptor.category,
    tier: descriptor.tier,
    runtimeId: descriptor.runtimeId,
    runtimeVersion: descriptor.runtimeVersion,
    runtimeKind: "in-process",
    estimatedPeakRssMb: descriptor.estimatedPeakRssMb,
    runtimeArgs: [],
    capabilities: { languages: ["zh", "en"] },
    source: "remote",
    installedAt: "2026-09-02T00:00:00.000Z",
    bytes: 1,
    sha256Manifest: "b".repeat(64),
    integrity: "verified",
    directory: modelDir,
    files: [],
  };
  return { root, descriptor, installed };
}

describe("InProcessInstanceFactory", () => {
  it("falls through unavailable Metal to CoreML, creates a session, and disposes it", async () => {
    const { root, descriptor, installed } = fixture("stt");
    const probes: string[] = [];
    const creates: InProcessRuntimeCreateOptions[] = [];
    const dispose = vi.fn(async () => {});
    const module: InProcessRuntimeModule = {
      probeBackend: async (options) => {
        probes.push(options.backend);
        return options.backend === "coreml";
      },
      createLocalModelRuntime: async (options) => {
        creates.push(options);
        return {
          backend: options.backend,
          protocolId: "local-sherpa-stt",
          transcribe: async () => ({ text: "本地转写", language: "zh" }),
          dispose,
          rssMb: () => 123,
        };
      },
    };
    const factory = new InProcessInstanceFactory({
      runtimeRoot: path.join(root, "runtime"),
      platform: "darwin",
      arch: "arm64",
      config: () => DEFAULT_LOCAL_MODELS_CONFIG,
      importModule: async () => module,
    });

    const instance = await factory.load(descriptor, installed, new AbortController().signal);
    expect(probes).toEqual(["metal", "coreml"]);
    expect(creates).toHaveLength(1);
    expect(creates[0]).toMatchObject({
      modelDirectory: installed.directory,
      backend: "coreml",
      threads: "auto",
      capabilities: { languages: ["zh", "en"] },
    });
    expect(await instance.transcribe!({
      model: { id: descriptor.id, quant: descriptor.quant, manifestVersion: descriptor.manifestVersion },
      filePath: "/tmp/fixture.wav",
      mime: "audio/wav",
      signal: new AbortController().signal,
    })).toEqual({ text: "本地转写", language: "zh" });
    expect(await factory.rssMb(instance)).toBe(123);
    await factory.unload(instance, descriptor, new AbortController().signal);
    expect(dispose).toHaveBeenCalledTimes(1);
  });

  it("rejects non-speech categories before importing a runtime module", async () => {
    const { root, descriptor, installed } = fixture("stt");
    const importModule = vi.fn(async () => ({}));
    const factory = new InProcessInstanceFactory({
      runtimeRoot: path.join(root, "runtime"),
      platform: "darwin",
      arch: "arm64",
      config: () => DEFAULT_LOCAL_MODELS_CONFIG,
      importModule,
    });

    await expect(factory.load(
      { ...descriptor, category: "embedding" },
      { ...installed, category: "embedding" },
      new AbortController().signal,
    )).rejects.toMatchObject({ code: "LOCAL_MODEL_UNSUPPORTED" });
    expect(importModule).not.toHaveBeenCalled();
  });

  it("keeps backend probe caches isolated by runtime scope", async () => {
    const probe = new BackendProbe();
    const firstValidate = vi.fn(async () => ({ available: true }));
    const secondValidate = vi.fn(async () => ({ available: true }));
    const signal = new AbortController().signal;
    await probe.probe({
      platform: "darwin", arch: "arm64", signal, cacheScope: "runtime-a", validate: firstValidate,
    });
    await probe.probe({
      platform: "darwin", arch: "arm64", signal, cacheScope: "runtime-b", validate: secondValidate,
    });
    expect(firstValidate).toHaveBeenCalledTimes(1);
    expect(secondValidate).toHaveBeenCalledTimes(1);
  });
});
