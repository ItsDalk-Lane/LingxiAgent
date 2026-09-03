import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LocalModelsManifestClient } from "../lib/local-models/index.ts";

const roots: string[] = [];
const SHA = "a".repeat(64);

afterEach(() => {
  for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
});

function tempRoot(): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-local-model-manifest-"));
  roots.push(root);
  return root;
}

function manifestBody(): string {
  return JSON.stringify({
    schemaVersion: 1,
    manifestVersion: "models-2026-09-02.1",
    updatedAt: "2026-09-02T00:00:00Z",
    runtimes: [{
      id: "sherpa-onnx",
      version: "1.12.20",
      platforms: {
        "darwin-arm64": {
          uri: "https://example.invalid/runtime.zip",
          bytes: 1024,
          sha256: SHA,
          entries: ["bin/runtime"],
        },
      },
    }],
    models: [{
      id: "qwen3-asr-1.7b",
      category: "stt",
      tier: "small",
      runtime: "sherpa-onnx",
      runtimeVersion: "1.12.20",
      license: "Apache-2.0",
      licenseFile: "LICENSE",
      variants: [{
        quant: "int8",
        estimatedPeakRssMb: 800,
        runtimeArgs: [],
        packages: [{
          platform: "*",
          uri: "https://example.invalid/model.zip",
          bytes: 2048,
          sha256: SHA,
          entries: ["model.onnx", "LICENSE"],
        }],
      }],
    }],
  });
}

describe("LocalModelsManifestClient", () => {
  it("fetches through the project dispatcher, stores ETag, and serves a 304 from cache", async () => {
    const body = manifestBody();
    const dispatcher = { proxy: true };
    let calls = 0;
    const fetchImpl = vi.fn(async (_url: string, init: RequestInit & { dispatcher?: unknown }) => {
      expect(init.dispatcher).toBe(dispatcher);
      calls += 1;
      if (calls === 1) {
        return new Response(body, { status: 200, headers: { etag: '"manifest-v1"' } });
      }
      expect(new Headers(init.headers).get("if-none-match")).toBe('"manifest-v1"');
      return new Response(null, { status: 304 });
    });
    const client = new LocalModelsManifestClient({
      url: "https://example.invalid/manifest.json",
      cacheDir: tempRoot(),
      fetchImpl,
      dispatcherForUrl: () => ({ dispatcher }),
    });

    const remote = await client.refresh({ signal: new AbortController().signal });
    expect(remote).toMatchObject({ source: "remote", etag: '"manifest-v1"', warning: null });
    expect(remote.catalog.find((entry) => entry.id === "qwen3-asr-1.7b")?.distributionStatus)
      .toBe("manifest-available");

    const cached = await client.refresh({ signal: new AbortController().signal });
    expect(cached).toMatchObject({ source: "cache", etag: '"manifest-v1"', warning: null });
    expect(cached.manifest?.manifestVersion).toBe("models-2026-09-02.1");
  });

  it("uses the last valid cache when the network or new payload fails", async () => {
    let response: "valid" | "network" | "invalid" = "valid";
    const client = new LocalModelsManifestClient({
      url: "https://example.invalid/manifest.json",
      cacheDir: tempRoot(),
      dispatcherForUrl: () => ({ dispatcher: null }),
      fetchImpl: async () => {
        if (response === "network") throw new Error("offline");
        return new Response(response === "valid" ? manifestBody() : "{broken", { status: 200 });
      },
    });
    await client.refresh({ signal: new AbortController().signal });
    response = "network";
    const offline = await client.refresh({ signal: new AbortController().signal });
    expect(offline).toMatchObject({ source: "cache", warning: "offline" });
    response = "invalid";
    const broken = await client.refresh({ signal: new AbortController().signal });
    expect(broken.source).toBe("cache");
    expect(broken.warning).toContain("JSON");
  });

  it("falls back to the honest built-in catalog when no valid cache exists", async () => {
    const client = new LocalModelsManifestClient({
      url: "https://example.invalid/manifest.json",
      cacheDir: tempRoot(),
      dispatcherForUrl: () => ({ dispatcher: null }),
      fetchImpl: async () => { throw new Error("offline"); },
    });

    const result = await client.refresh({ signal: new AbortController().signal });
    expect(result).toMatchObject({ source: "builtin", manifest: null, warning: "offline" });
    expect(result.catalog).toHaveLength(4);
    expect(result.catalog.every((entry) => entry.distributionStatus === "catalog-only")).toBe(true);
  });

  it("rejects oversized responses before parsing and does not cache them", async () => {
    const cacheDir = tempRoot();
    const client = new LocalModelsManifestClient({
      url: "https://example.invalid/manifest.json",
      cacheDir,
      maxBytes: 1024,
      dispatcherForUrl: () => ({ dispatcher: null }),
      fetchImpl: async () => new Response("x".repeat(2048), {
        status: 200,
        headers: { "content-length": "2048" },
      }),
    });
    const result = await client.refresh({ signal: new AbortController().signal });
    expect(result.source).toBe("builtin");
    expect(result.warning).toContain("size limit");
    expect(fs.existsSync(path.join(cacheDir, "manifest-cache.json"))).toBe(false);
  });

  it("requires an HTTPS manifest URL before any network access", () => {
    expect(() => new LocalModelsManifestClient({
      url: "http://example.invalid/manifest.json",
      cacheDir: tempRoot(),
    })).toThrowError(expect.objectContaining({ code: "LOCAL_MODEL_MANIFEST_INVALID" }));
  });
});
