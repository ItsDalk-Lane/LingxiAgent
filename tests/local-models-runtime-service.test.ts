import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  LocalModelRuntimeService,
  LocalModelRegistry,
  type LocalModelCallObservation,
  type LocalModelCategory,
  type LocalModelInstanceFactory,
  type LocalModelRef,
} from "../lib/local-models/index.ts";

const roots: string[] = [];

afterEach(() => {
  vi.useRealTimers();
  for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
});

function tempRoot(): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-local-model-runtime-"));
  roots.push(root);
  return root;
}

function sha256(value: Buffer): string {
  return createHash("sha256").update(value).digest("hex");
}

async function installFixture(
  registry: LocalModelRegistry,
  input: { id: string; quant: string; category: LocalModelCategory; tier?: "small" | "large" },
): Promise<LocalModelRef> {
  const bundle = tempRoot();
  const content = Buffer.from(`model:${input.id}:${input.quant}`);
  fs.writeFileSync(path.join(bundle, "model.bin"), content);
  const files = [{ path: "model.bin", bytes: content.length, sha256: sha256(content) }];
  fs.writeFileSync(path.join(bundle, "model.json"), `${JSON.stringify({
    schemaVersion: 1,
    id: input.id,
    category: input.category,
    quant: input.quant,
    tier: input.tier ?? "small",
    version: "fixture-v1",
    runtimeId: "fixture-runtime",
    runtimeVersion: "1",
    estimatedPeakRssMb: input.tier === "large" ? 512 : 64,
    source: "manual",
    installedAt: "2026-09-02T00:00:00.000Z",
    integrity: "verified",
    bytes: content.length,
    sha256Manifest: sha256(Buffer.from(JSON.stringify(files))),
    files,
  }, null, 2)}\n`);
  await registry.importDirectory(bundle, { signal: new AbortController().signal });
  return { id: input.id, quant: input.quant, manifestVersion: "fixture-v1" };
}

describe("LocalModelRuntimeService", () => {
  it("原生协作取消的普通错误归一为取消，等待真正收尾后再释放引用", async () => {
    const registry = new LocalModelRegistry(path.join(tempRoot(), "models"));
    const model = await installFixture(registry, { id: "kokoro-82m", quant: "fp32", category: "tts" });
    const controller = new AbortController(), observations: LocalModelCallObservation[] = [];
    let finish: (() => void) | undefined;
    const factory: LocalModelInstanceFactory = {
      load: async () => ({ backend: "cpu", protocolId: "fixture", synthesize: async () => {
        controller.abort();
        await new Promise<void>((resolve) => { finish = resolve; });
        throw new Error("原生工作已收尾");
      } }),
      unload: vi.fn(async () => {}),
    };
    const runtime = new LocalModelRuntimeService({ registry, factory, onObservation: event => observations.push(event) });
    const result = runtime.synthesize({ model, text: "你好", signal: controller.signal });
    const rejected = expect(result).rejects.toMatchObject({ code: "LOCAL_MODEL_ABORTED" });
    await vi.waitFor(() => expect(finish).toBeTypeOf("function"));
    expect(runtime.snapshot()[0].refs).toBe(1);
    finish!(); await rejected;
    expect(runtime.snapshot()[0].refs).toBe(0);
    expect(observations.at(-1)).toMatchObject({ phase: "failure", errorCode: "LOCAL_MODEL_ABORTED" });
    await runtime.dispose();
  });

  it("routes all four abilities through one lifecycle and emits content-free observations", async () => {
    vi.useFakeTimers();
    const root = tempRoot();
    const registry = new LocalModelRegistry(path.join(root, "models"));
    const embedding = await installFixture(registry, {
      id: "qwen3-embedding-0.6b", quant: "fp8", category: "embedding",
    });
    const ocr = await installFixture(registry, { id: "glm-ocr", quant: "q4", category: "ocr", tier: "large" });
    const stt = await installFixture(registry, { id: "sensevoice-small", quant: "int8", category: "stt" });
    const tts = await installFixture(registry, { id: "kokoro-82m", quant: "fp32", category: "tts" });
    const audioPath = path.join(root, "speech.wav");
    fs.writeFileSync(audioPath, Buffer.alloc(123));
    const unload = vi.fn(async () => {});
    const factory: LocalModelInstanceFactory = {
      load: vi.fn(async (descriptor) => ({
        backend: "cpu" as const,
        protocolId: `fixture-${descriptor.category}`,
        diagnostics: { peakRssMb: 42 },
        embed: async (request) => ({
          vectors: request.texts.map(() => [0.1, 0.2]),
          dimensions: 2,
          modelKey: `local:${descriptor.id}@${descriptor.quant}@${descriptor.manifestVersion}`,
        }),
        ocr: async () => ({ markdown: "识别结果", text: "识别结果", format: "ocr" as const, warnings: [] }),
        transcribe: async () => ({ text: "转写结果", language: "zh" }),
        synthesize: async () => ({ sampleRate: 24000 as const, format: "wav" as const, audio: Uint8Array.of(1, 2, 3) }),
      })),
      unload,
      rssMb: () => 42,
    };
    const observations: LocalModelCallObservation[] = [];
    const runtime = new LocalModelRuntimeService({
      registry,
      factory,
      idleUnloadMs: { small: 0, large: 0 },
      onObservation: (event) => observations.push(event),
    });
    await runtime.initialize({ signal: new AbortController().signal });
    const signal = new AbortController().signal;

    const embedded = await runtime.embed({ model: embedding, texts: ["甲", "乙"], signal });
    const recognized = await runtime.ocr({ model: ocr, image: Uint8Array.of(1, 2), mime: "image/png", signal });
    const transcribed = await runtime.transcribe({ model: stt, filePath: audioPath, mime: "audio/wav", signal });
    const synthesized = await runtime.synthesize({ model: tts, text: "你好", signal });

    expect(embedded.output).toMatchObject({
      dimensions: 2,
      modelKey: "local:qwen3-embedding-0.6b@fp8@fixture-v1",
    });
    expect(recognized.output.text).toBe("识别结果");
    expect(transcribed).toMatchObject({ inputBytes: 123, output: { text: "转写结果" } });
    expect(synthesized.output.audio).toEqual(Uint8Array.of(1, 2, 3));
    expect(observations.filter((event) => event.phase === "success")).toHaveLength(4);
    expect(JSON.stringify(observations)).not.toContain("识别结果");
    expect(JSON.stringify(observations)).not.toContain("转写结果");
    expect(observations.find((event) => event.operation === "transcribe")?.inputBytes).toBe(123);

    await vi.runAllTimersAsync();
    await vi.waitFor(() => expect(unload).toHaveBeenCalledTimes(4));
    expect(runtime.snapshot()).toEqual([]);
  });

  it("rejects missing and wrong-category models without invoking a backend", async () => {
    const root = tempRoot();
    const registry = new LocalModelRegistry(path.join(root, "models"));
    const stt = await installFixture(registry, { id: "sensevoice-small", quant: "int8", category: "stt" });
    const factory: LocalModelInstanceFactory = {
      load: vi.fn(async () => ({ backend: "cpu" as const, protocolId: "fixture" })),
      unload: vi.fn(async () => {}),
    };
    const runtime = new LocalModelRuntimeService({ registry, factory });
    await runtime.initialize({ signal: new AbortController().signal });

    await expect(runtime.embed({
      model: { id: "missing", quant: "q4", manifestVersion: "fixture-v1" },
      texts: ["x"],
      signal: new AbortController().signal,
    })).rejects.toMatchObject({ code: "LOCAL_MODEL_NOT_INSTALLED" });
    await expect(runtime.embed({
      model: stt,
      texts: ["x"],
      signal: new AbortController().signal,
    })).rejects.toMatchObject({ code: "LOCAL_MODEL_UNSUPPORTED" });
    expect(factory.load).not.toHaveBeenCalled();
  });

  it("releases the lease and records a structured failure when inference fails", async () => {
    vi.useFakeTimers();
    const root = tempRoot();
    const registry = new LocalModelRegistry(path.join(root, "models"));
    const embedding = await installFixture(registry, {
      id: "qwen3-embedding-0.6b", quant: "fp8", category: "embedding",
    });
    const unload = vi.fn(async () => {});
    const observations: LocalModelCallObservation[] = [];
    const runtime = new LocalModelRuntimeService({
      registry,
      idleUnloadMs: { small: 0 },
      onObservation: (event) => observations.push(event),
      factory: {
        load: async () => ({
          backend: "cpu",
          protocolId: "fixture-embedding",
          embed: async () => { throw new Error("fixture failure"); },
        }),
        unload,
      },
    });
    await runtime.initialize({ signal: new AbortController().signal });

    await expect(runtime.embed({ model: embedding, texts: ["secret payload"], signal: new AbortController().signal }))
      .rejects.toThrow("fixture failure");
    await vi.runAllTimersAsync();
    await vi.waitFor(() => expect(unload).toHaveBeenCalledTimes(1));
    expect(observations.at(-1)).toMatchObject({
      phase: "failure",
      operation: "embed",
      errorCode: "LOCAL_MODEL_RUNTIME_ERROR",
    });
    expect(JSON.stringify(observations)).not.toContain("secret payload");
  });
});
