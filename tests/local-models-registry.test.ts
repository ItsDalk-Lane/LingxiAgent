import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  LocalModelRegistry,
  type LocalModelInstallMetadata,
} from "../lib/local-models/index.ts";

const roots: string[] = [];

afterEach(() => {
  for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
});

function tempRoot(prefix: string): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  roots.push(root);
  return root;
}

function sha256(value: Buffer | string): string {
  return createHash("sha256").update(value).digest("hex");
}

function createBundle(options: {
  root?: string;
  id?: string;
  quant?: string;
  category?: "embedding" | "ocr" | "stt" | "tts";
  content?: Buffer;
} = {}): { directory: string; metadata: LocalModelInstallMetadata; content: Buffer } {
  const directory = options.root ?? tempRoot("lingxi-local-model-bundle-");
  const id = options.id ?? "sensevoice-small";
  const quant = options.quant ?? "int8";
  const category = options.category ?? "stt";
  const content = options.content ?? Buffer.from("verified model bytes");
  fs.mkdirSync(path.join(directory, "weights"), { recursive: true });
  fs.writeFileSync(path.join(directory, "weights", "model.onnx"), content);
  const metadata: LocalModelInstallMetadata = {
    schemaVersion: 1,
    id,
    category,
    quant,
    tier: "small",
    version: "2026-09-02",
    runtimeId: "sherpa-onnx-node",
    runtimeVersion: "1.12.23",
    runtimeKind: "in-process",
    estimatedPeakRssMb: 512,
    runtimeArgs: [],
    capabilities: {},
    source: "remote",
    installedAt: "2026-09-02T00:00:00.000Z",
    integrity: "verified",
    bytes: content.length,
    sha256Manifest: sha256(JSON.stringify([{ path: "weights/model.onnx", bytes: content.length, sha256: sha256(content) }])),
    files: [{ path: "weights/model.onnx", bytes: content.length, sha256: sha256(content) }],
  };
  fs.writeFileSync(path.join(directory, "model.json"), `${JSON.stringify(metadata, null, 2)}\n`);
  return { directory, metadata, content };
}

describe("LocalModelRegistry", () => {
  it("imports through staging, forces manual provenance, emits refresh, and scans the verified model", async () => {
    const registryRoot = tempRoot("lingxi-local-model-registry-");
    const bundle = createBundle();
    const registry = new LocalModelRegistry(registryRoot);
    const changed = vi.fn();
    registry.on("changed", changed);

    const installed = await registry.importDirectory(bundle.directory, { signal: new AbortController().signal });

    expect(installed).toMatchObject({
      id: "sensevoice-small",
      quant: "int8",
      category: "stt",
      source: "manual",
      integrity: "verified",
      bytes: bundle.content.length,
    });
    expect(fs.readFileSync(path.join(installed.directory, "weights", "model.onnx"))).toEqual(bundle.content);
    expect(JSON.parse(fs.readFileSync(path.join(installed.directory, "model.json"), "utf8"))).toMatchObject({
      source: "manual",
    });
    expect(changed).toHaveBeenCalledTimes(1);
    expect((await registry.scan({ signal: new AbortController().signal })).models).toHaveLength(1);
  });

  it("rejects a hash mismatch and removes its private staging directory", async () => {
    const registryRoot = tempRoot("lingxi-local-model-registry-");
    const bundle = createBundle();
    fs.appendFileSync(path.join(bundle.directory, "weights", "model.onnx"), "tampered");
    const registry = new LocalModelRegistry(registryRoot);

    await expect(registry.importDirectory(bundle.directory, { signal: new AbortController().signal }))
      .rejects.toMatchObject({ code: "LOCAL_MODEL_INSTALL_INVALID" });
    const categoryEntries = fs.readdirSync(path.join(registryRoot, "stt"));
    expect(categoryEntries).toEqual([]);
  });

  it("rejects symbolic links and undeclared files without copying them", async () => {
    const registryRoot = tempRoot("lingxi-local-model-registry-");
    const bundle = createBundle();
    const canary = path.join(tempRoot("lingxi-local-model-canary-"), "canary.bin");
    fs.writeFileSync(canary, "do not copy");
    fs.symlinkSync(canary, path.join(bundle.directory, "escape.bin"));
    const registry = new LocalModelRegistry(registryRoot);

    await expect(registry.importDirectory(bundle.directory, { signal: new AbortController().signal }))
      .rejects.toMatchObject({ code: "LOCAL_MODEL_INSTALL_INVALID" });
    expect(fs.readFileSync(canary, "utf8")).toBe("do not copy");
    expect(fs.readdirSync(path.join(registryRoot, "stt"))).toEqual([]);
  });

  it("reports tampered installed directories as rejected instead of usable", async () => {
    const registryRoot = tempRoot("lingxi-local-model-registry-");
    const bundle = createBundle();
    const registry = new LocalModelRegistry(registryRoot);
    const installed = await registry.importDirectory(bundle.directory, { signal: new AbortController().signal });
    fs.writeFileSync(path.join(installed.directory, "weights", "model.onnx"), "tampered");

    const scan = await registry.scan({ signal: new AbortController().signal });
    expect(scan.models).toEqual([]);
    expect(scan.rejected).toHaveLength(1);
    expect(scan.rejected[0].reason).toContain("size mismatch");
  });

  it("does not overwrite an installed variant and removes only an exact safe target", async () => {
    const registryRoot = tempRoot("lingxi-local-model-registry-");
    const firstBundle = createBundle();
    const secondBundle = createBundle({ content: Buffer.from("replacement") });
    const registry = new LocalModelRegistry(registryRoot);
    const first = await registry.importDirectory(firstBundle.directory, { signal: new AbortController().signal });

    await expect(registry.importDirectory(secondBundle.directory, { signal: new AbortController().signal }))
      .rejects.toMatchObject({ code: "LOCAL_MODEL_ALREADY_INSTALLED" });
    expect(fs.readFileSync(path.join(first.directory, "weights", "model.onnx"))).toEqual(firstBundle.content);

    expect(await registry.remove("stt", "sensevoice-small", "int8", {
      signal: new AbortController().signal,
    })).toBe(true);
    expect(await registry.remove("stt", "sensevoice-small", "int8", {
      signal: new AbortController().signal,
    })).toBe(false);
    expect(registry.snapshot().models).toEqual([]);
  });

  it("recognizes and imports an ONNX directory without model.json using confirmed metadata", async () => {
    const registryRoot = tempRoot("lingxi-local-model-registry-");
    const source = tempRoot("lingxi-local-model-raw-");
    fs.mkdirSync(path.join(source, "weights"));
    fs.writeFileSync(path.join(source, "weights", "model.onnx"), "raw onnx model");
    fs.writeFileSync(path.join(source, "tokens.txt"), "hello 1");
    const registry = new LocalModelRegistry(registryRoot);

    await expect(registry.inspectUnmanagedDirectory(source, { signal: new AbortController().signal }))
      .resolves.toMatchObject({ formatHints: ["onnx"], totalBytes: 21 });
    const installed = await registry.importUnmanagedDirectory(source, {
      id: "sensevoice-small",
      category: "stt",
      quant: "int8",
      tier: "small",
      runtimeId: "sherpa-onnx",
      runtimeVersion: "1.0.0",
      runtimeKind: "in-process",
      estimatedPeakRssMb: 800,
      runtimeArgs: [],
      capabilities: { languages: ["zh"] },
    }, { signal: new AbortController().signal });

    expect(installed).toMatchObject({ source: "manual", integrity: "unknown", version: expect.stringMatching(/^manual-[a-f0-9]{12}$/) });
    expect(JSON.parse(fs.readFileSync(path.join(installed.directory, "model.json"), "utf8"))).toMatchObject({
      id: "sensevoice-small",
      source: "manual",
      integrity: "unknown",
      files: expect.arrayContaining([expect.objectContaining({ path: "weights/model.onnx" })]),
    });
    expect(fs.existsSync(path.join(source, "model.json"))).toBe(false);
  });

  it("rejects unrecognized raw files without generating metadata", async () => {
    const registry = new LocalModelRegistry(tempRoot("lingxi-local-model-registry-"));
    const source = tempRoot("lingxi-local-model-raw-");
    fs.writeFileSync(path.join(source, "unknown.bin"), "unknown");
    await expect(registry.inspectUnmanagedDirectory(source, { signal: new AbortController().signal }))
      .rejects.toMatchObject({ code: "LOCAL_MODEL_INSTALL_INVALID" });
    expect(fs.existsSync(path.join(source, "model.json"))).toBe(false);
  });

  it("keeps a declared license file readable through the bounded registry API", async () => {
    const registryRoot = tempRoot("lingxi-local-model-registry-");
    const bundle = createBundle();
    const license = "Apache License 2.0 test fixture";
    fs.writeFileSync(path.join(bundle.directory, "LICENSE"), license);
    const metadata = JSON.parse(fs.readFileSync(path.join(bundle.directory, "model.json"), "utf8"));
    metadata.licenseFile = "LICENSE";
    metadata.files.push({ path: "LICENSE", bytes: Buffer.byteLength(license), sha256: sha256(license) });
    metadata.bytes += Buffer.byteLength(license);
    metadata.sha256Manifest = sha256(JSON.stringify(metadata.files));
    fs.writeFileSync(path.join(bundle.directory, "model.json"), `${JSON.stringify(metadata, null, 2)}\n`);
    const registry = new LocalModelRegistry(registryRoot);
    await registry.importDirectory(bundle.directory, { signal: new AbortController().signal });

    await expect(registry.readLicense("stt", "sensevoice-small", "int8")).resolves.toBe(license);
    await expect(registry.readLicense("tts", "missing", "q4")).resolves.toBeNull();
  });
});
