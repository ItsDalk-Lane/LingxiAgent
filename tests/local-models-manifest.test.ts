import { describe, expect, it } from "vitest";
import {
  BUILTIN_LOCAL_MODEL_CATALOG,
  DEFAULT_LOCAL_MODELS_CONFIG,
  LocalModelError,
  localModelKey,
  normalizeLocalModelsConfig,
  parseLocalModelsManifest,
} from "../lib/local-models/index.ts";

const SHA = "a".repeat(64);

function validManifest() {
  return {
    schemaVersion: 1,
    manifestVersion: "models-2026-09-02.1",
    updatedAt: "2026-09-02T00:00:00Z",
    runtimes: [{
      id: "sherpa-onnx",
      version: "1.12.20",
      platforms: {
        "darwin-arm64": {
          uri: "https://github.com/ItsDalk-Lane/LingxiAgent/releases/download/models-manifest-v1/sherpa.zip",
          bytes: 1024,
          sha256: SHA,
          entries: ["bin/sherpa.node"],
        },
      },
    }],
    models: [{
      id: "sensevoice-small",
      category: "stt",
      tier: "small",
      runtime: "sherpa-onnx",
      runtimeVersion: "1.12.20",
      license: "Apache-2.0",
      licenseFile: "LICENSE",
      variants: [{
        quant: "int8",
        estimatedPeakRssMb: 800,
        runtimeArgs: ["--provider", "{backend}"],
        capabilities: { languages: ["zh", "en"] },
        packages: [{
          platform: "*",
          uri: "https://github.com/ItsDalk-Lane/LingxiAgent/releases/download/models-manifest-v1/sensevoice.zip",
          bytes: 2048,
          sha256: SHA,
          entries: ["model.int8.onnx", "tokens.txt", "LICENSE"],
        }],
      }],
    }],
  };
}

describe("local model manifest", () => {
  it("parses the complete runtime, platform and variant identity chain", () => {
    const parsed = parseLocalModelsManifest(validManifest());
    expect(parsed.manifestVersion).toBe("models-2026-09-02.1");
    expect(parsed.runtimes[0].platforms["darwin-arm64"].bytes).toBe(1024);
    expect(parsed.models[0]).toMatchObject({
      id: "sensevoice-small",
      category: "stt",
      tier: "small",
      runtime: "sherpa-onnx",
      runtimeVersion: "1.12.20",
      licenseFile: "LICENSE",
    });
    expect(localModelKey({
      id: parsed.models[0].id,
      quant: parsed.models[0].variants[0].quant,
      manifestVersion: parsed.manifestVersion,
    })).toBe("local:sensevoice-small@int8@models-2026-09-02.1");
  });

  it.each([
    ["http asset", (manifest: any) => { manifest.models[0].variants[0].packages[0].uri = "http://example.test/model.zip"; }],
    ["zero bytes", (manifest: any) => { manifest.models[0].variants[0].packages[0].bytes = 0; }],
    ["empty hash", (manifest: any) => { manifest.models[0].variants[0].packages[0].sha256 = ""; }],
    ["path traversal", (manifest: any) => { manifest.models[0].licenseFile = "../LICENSE"; }],
    ["missing runtime", (manifest: any) => { manifest.models[0].runtimeVersion = "missing"; }],
  ])("rejects %s", (_name, mutate) => {
    const manifest = validManifest();
    mutate(manifest);
    expect(() => parseLocalModelsManifest(manifest)).toThrow(LocalModelError);
  });

  it("keeps the built-in catalog honest when no verified asset manifest exists", () => {
    expect(BUILTIN_LOCAL_MODEL_CATALOG.map((entry) => entry.id)).toEqual([
      "qwen3-embedding-8b",
      "glm-ocr",
      "qwen3-asr-1.7b",
      "indextts-2.5",
    ]);
    expect(BUILTIN_LOCAL_MODEL_CATALOG.every((entry) => entry.distributionStatus === "catalog-only")).toBe(true);
    expect(JSON.stringify(BUILTIN_LOCAL_MODEL_CATALOG)).not.toContain("https://");
  });
});

describe("local model config", () => {
  it("uses the requested defaults", () => {
    expect(DEFAULT_LOCAL_MODELS_CONFIG).toMatchObject({
      backend: "auto",
      threads: "auto",
      embedding: { batchSize: 32 },
      stt: { vadEnabled: true, chunkMs: 30_000 },
      tts: { streaming: true },
      useMmap: true,
      mlock: false,
      quantPreference: "smallest",
      idleUnloadMs: { small: 300_000, large: 120_000 },
      memoryBudgetSmallMb: 1536,
      preloadSmall: false,
      download: { concurrency: 4, mirrorBaseUrl: "" },
    });
  });

  it("normalizes unsafe or out-of-range values without mutating defaults", () => {
    const normalized = normalizeLocalModelsConfig({
      backend: "invented",
      threads: 0,
      idleUnloadMs: { small: -1, large: 50 },
      memoryBudgetSmallMb: 64,
      download: { concurrency: 99, mirrorBaseUrl: "  https://mirror.example  " },
    });
    expect(normalized).toMatchObject({
      backend: "auto",
      threads: "auto",
      idleUnloadMs: { small: 300_000, large: 50 },
      memoryBudgetSmallMb: 1536,
      download: { concurrency: 4, mirrorBaseUrl: "https://mirror.example" },
    });
    expect(DEFAULT_LOCAL_MODELS_CONFIG.download.mirrorBaseUrl).toBe("");
  });
});

