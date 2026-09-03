import { createHash, randomBytes } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import archiver from "archiver";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  LocalModelInstaller,
  LocalModelRegistry,
  parseLocalModelsManifest,
  ResumableDownloader,
  type LocalModelInstallEvent,
  type LocalModelsManifest,
} from "../lib/local-models/index.ts";

const roots: string[] = [];

afterEach(() => {
  for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
});

function tempRoot(): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-local-model-installer-"));
  roots.push(root);
  return root;
}

function sha256(value: Buffer): string {
  return createHash("sha256").update(value).digest("hex");
}

async function buildZip(entries: Array<{ name: string; content: Buffer | string }>): Promise<Buffer> {
  const root = tempRoot();
  const zipPath = path.join(root, "asset.zip");
  await new Promise<void>((resolve, reject) => {
    const output = fs.createWriteStream(zipPath);
    const archive = archiver("zip", { zlib: { level: 6 } });
    output.once("close", resolve);
    output.once("error", reject);
    archive.once("error", reject);
    archive.pipe(output);
    for (const entry of entries) archive.append(entry.content, { name: entry.name });
    void archive.finalize();
  });
  return fs.readFileSync(zipPath);
}

function manifestFor(runtimeZip: Buffer, modelZip: Buffer, secondModelZip?: Buffer): LocalModelsManifest {
  return parseLocalModelsManifest({
    schemaVersion: 1,
    manifestVersion: "models-test-v1",
    updatedAt: "2026-09-02T00:00:00Z",
    runtimes: [{
      id: "fixture-runtime",
      version: "1",
      platforms: {
        "darwin-arm64": {
          format: "zip",
          uri: "https://assets.example.invalid/runtime.zip",
          bytes: runtimeZip.length,
          sha256: sha256(runtimeZip),
          entries: ["bin/runtime"],
        },
      },
    }],
    models: [{
      id: "sensevoice-small",
      category: "stt",
      tier: "small",
      runtime: "fixture-runtime",
      runtimeVersion: "1",
      license: "Apache-2.0",
      licenseFile: "LICENSE",
      variants: [{
        quant: "int8",
        tier: "small",
        estimatedPeakRssMb: 800,
        runtimeArgs: [],
        packages: [{
          platform: "*",
          format: "zip",
          uri: "https://assets.example.invalid/model.zip",
          bytes: modelZip.length,
          sha256: sha256(modelZip),
          entries: ["model.onnx", "LICENSE"],
        }],
      }],
    }, ...(secondModelZip ? [{
      id: "kokoro-82m",
      category: "tts",
      tier: "small",
      runtime: "fixture-runtime",
      runtimeVersion: "1",
      license: "Apache-2.0",
      licenseFile: "LICENSE",
      variants: [{
        quant: "fp32",
        tier: "small",
        estimatedPeakRssMb: 600,
        runtimeArgs: [],
        packages: [{
          platform: "*",
          format: "zip",
          uri: "https://assets.example.invalid/second-model.zip",
          bytes: secondModelZip.length,
          sha256: sha256(secondModelZip),
          entries: ["model.onnx", "LICENSE"],
        }],
      }],
    }] : [])],
  });
}

function rangeResponse(payload: Buffer, init: RequestInit): Response {
  const range = new Headers(init.headers).get("range");
  if (!range) return new Response(Uint8Array.from(payload), { status: 200 });
  const match = /^bytes=(\d+)-(\d+)$/.exec(range);
  if (!match) return new Response(null, { status: 416 });
  return new Response(Uint8Array.from(payload.subarray(Number(match[1]), Number(match[2]) + 1)), { status: 206 });
}

function createHarness(assets: Record<string, Buffer>, events: LocalModelInstallEvent[] = []) {
  const root = tempRoot();
  const fetchImpl = vi.fn(async (url: string, init: RequestInit) => {
    const payload = assets[url];
    if (!payload) return new Response(null, { status: 404 });
    if (init.method === "HEAD") {
      return new Response(null, {
        status: 200,
        headers: { "accept-ranges": "bytes", "content-length": String(payload.length) },
      });
    }
    return rangeResponse(payload, init);
  });
  const registry = new LocalModelRegistry(path.join(root, "models"));
  const downloader = new ResumableDownloader({
    rootDir: path.join(root, "downloads"),
    getFreeBytes: () => 1024 * 1024 * 1024,
    dispatcherForUrl: () => ({ dispatcher: null }),
    fetchImpl,
  });
  const installer = new LocalModelInstaller({
    registry,
    downloader,
    runtimeRoot: path.join(root, "runtime", "local-inference"),
    workRoot: path.join(root, "install-work"),
    platform: "darwin-arm64",
    onEvent: (event) => events.push(event),
  });
  return { root, fetchImpl, registry, downloader, installer };
}

describe("LocalModelInstaller", () => {
  it("installs and verifies the runtime before atomically registering the model", async () => {
    const runtimeZip = await buildZip([{ name: "bin/runtime", content: randomBytes(96 * 1024) }]);
    const modelBytes = randomBytes(128 * 1024);
    const modelZip = await buildZip([
      { name: "model.onnx", content: modelBytes },
      { name: "LICENSE", content: "Apache-2.0" },
    ]);
    const manifest = manifestFor(runtimeZip, modelZip);
    const events: LocalModelInstallEvent[] = [];
    const harness = createHarness({
      "https://assets.example.invalid/runtime.zip": runtimeZip,
      "https://assets.example.invalid/model.zip": modelZip,
    }, events);

    const installed = await harness.installer.install(manifest, "sensevoice-small", "int8", {
      signal: new AbortController().signal,
    });

    expect(installed).toMatchObject({
      id: "sensevoice-small",
      quant: "int8",
      version: "models-test-v1",
      source: "remote",
      integrity: "verified",
    });
    expect(fs.readFileSync(path.join(installed.directory, "model.onnx"))).toEqual(modelBytes);
    const runtimeDir = harness.installer.runtimeDirectory("fixture-runtime", "1");
    expect(JSON.parse(fs.readFileSync(path.join(runtimeDir, "runtime.json"), "utf8"))).toMatchObject({
      id: "fixture-runtime",
      version: "1",
      platform: "darwin-arm64",
      packageSha256: sha256(runtimeZip),
    });
    expect(events.map((event) => event.kind)).toEqual([
      "runtime_download", "runtime_extract", "runtime_ready",
      "model_download", "model_extract", "model_ready",
    ]);
    expect((await harness.downloader.listTasks())).toEqual([]);

    const calls = harness.fetchImpl.mock.calls.length;
    const reused = await harness.installer.install(manifest, "sensevoice-small", "int8", {
      signal: new AbortController().signal,
    });
    expect(reused.directory).toBe(installed.directory);
    expect(harness.fetchImpl).toHaveBeenCalledTimes(calls);
  });

  it("deduplicates concurrent installs of the same model variant", async () => {
    const runtimeZip = await buildZip([{ name: "bin/runtime", content: randomBytes(80 * 1024) }]);
    const modelZip = await buildZip([
      { name: "model.onnx", content: randomBytes(80 * 1024) },
      { name: "LICENSE", content: "Apache-2.0" },
    ]);
    const manifest = manifestFor(runtimeZip, modelZip);
    const harness = createHarness({
      "https://assets.example.invalid/runtime.zip": runtimeZip,
      "https://assets.example.invalid/model.zip": modelZip,
    });
    const signal = new AbortController().signal;
    const results = await Promise.all(Array.from({ length: 10 }, () =>
      harness.installer.install(manifest, "sensevoice-small", "int8", { signal })));
    expect(new Set(results.map((entry) => entry.directory)).size).toBe(1);
    expect(harness.fetchImpl.mock.calls.filter((call) => call[1]?.method === "GET")).toHaveLength(2);
  });

  it("refuses to use a tampered shared runtime for another model", async () => {
    const runtimeZip = await buildZip([{ name: "bin/runtime", content: randomBytes(80 * 1024) }]);
    const modelZip = await buildZip([
      { name: "model.onnx", content: randomBytes(80 * 1024) },
      { name: "LICENSE", content: "Apache-2.0" },
    ]);
    const secondZip = await buildZip([
      { name: "model.onnx", content: randomBytes(80 * 1024) },
      { name: "LICENSE", content: "Apache-2.0" },
    ]);
    const manifest = manifestFor(runtimeZip, modelZip, secondZip);
    const harness = createHarness({
      "https://assets.example.invalid/runtime.zip": runtimeZip,
      "https://assets.example.invalid/model.zip": modelZip,
      "https://assets.example.invalid/second-model.zip": secondZip,
    });
    await harness.installer.install(manifest, "sensevoice-small", "int8", {
      signal: new AbortController().signal,
    });
    fs.writeFileSync(path.join(harness.installer.runtimeDirectory("fixture-runtime", "1"), "bin", "runtime"), "tampered");

    await expect(harness.installer.install(manifest, "kokoro-82m", "fp32", {
      signal: new AbortController().signal,
    })).rejects.toMatchObject({ code: "LOCAL_MODEL_RUNTIME_MISSING" });
    expect(harness.registry.snapshot().models.map((entry) => entry.id)).toEqual(["sensevoice-small"]);
  });

  it("fails before downloading when the requested platform is absent", async () => {
    const runtimeZip = await buildZip([{ name: "bin/runtime", content: randomBytes(80 * 1024) }]);
    const modelZip = await buildZip([
      { name: "model.onnx", content: randomBytes(80 * 1024) },
      { name: "LICENSE", content: "Apache-2.0" },
    ]);
    const manifest = manifestFor(runtimeZip, modelZip);
    const harness = createHarness({});
    delete manifest.runtimes[0].platforms["darwin-arm64"];

    await expect(harness.installer.install(manifest, "sensevoice-small", "int8", {
      signal: new AbortController().signal,
    })).rejects.toMatchObject({ code: "LOCAL_MODEL_RUNTIME_MISSING" });
    expect(harness.fetchImpl).not.toHaveBeenCalled();
  });
});
