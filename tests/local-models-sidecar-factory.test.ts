import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  DEFAULT_LOCAL_MODELS_CONFIG,
  expandRuntimeArgs,
  SidecarInstanceFactory,
  SidecarManager,
  type LocalModelDescriptor,
  type LocalModelRegistryEntry,
  type SidecarManagerOptions,
  type SidecarReadyMessage,
} from "../lib/local-models/index.ts";

const roots: string[] = [];

afterEach(() => {
  for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
});

function tempRoot(): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-sidecar-factory-"));
  roots.push(root);
  return root;
}

function sha256(value: Buffer): string {
  return createHash("sha256").update(value).digest("hex");
}

describe("旁路进程内存真值", () => {
  it("缺少可采样进程时返回未知，不把缺测写成零", async () => {
    const root = tempRoot();
    const factory = new SidecarInstanceFactory({ runtimeRoot: root, logRoot: root, config: () => ({ ...DEFAULT_LOCAL_MODELS_CONFIG }) });
    expect(await factory.rssMb({ backend: "cpu", protocolId: "test" })).toBeNaN();
  });
});

function fixture(): {
  root: string;
  descriptor: LocalModelDescriptor;
  installed: LocalModelRegistryEntry;
} {
  const root = tempRoot();
  const runtimeDir = path.join(root, "runtime", "fixture-runtime", "1", "darwin-arm64");
  const modelDir = path.join(root, "models", "embedding", "qwen3-embedding-0.6b@fp8");
  fs.mkdirSync(path.join(runtimeDir, "bin"), { recursive: true });
  fs.mkdirSync(modelDir, { recursive: true });
  const executable = Buffer.from("fixture executable");
  fs.writeFileSync(path.join(runtimeDir, "bin", "runtime"), executable, { mode: 0o755 });
  fs.writeFileSync(path.join(runtimeDir, "runtime.json"), `${JSON.stringify({
    schemaVersion: 1,
    id: "fixture-runtime",
    version: "1",
    platform: "darwin-arm64",
    kind: "sidecar",
    entrypoint: "bin/runtime",
    packageSha256: "a".repeat(64),
    installedAt: "2026-09-02T00:00:00.000Z",
    files: [{ path: "bin/runtime", bytes: executable.length, sha256: sha256(executable) }],
  }, null, 2)}\n`);
  const descriptor: LocalModelDescriptor = {
    id: "qwen3-embedding-0.6b",
    quant: "fp8",
    manifestVersion: "models-v1",
    category: "embedding",
    tier: "small",
    runtimeId: "fixture-runtime",
    runtimeVersion: "1",
    estimatedPeakRssMb: 256,
  };
  const installed: LocalModelRegistryEntry = {
    id: descriptor.id,
    quant: descriptor.quant,
    version: descriptor.manifestVersion,
    category: descriptor.category,
    tier: descriptor.tier,
    runtimeId: descriptor.runtimeId,
    runtimeVersion: descriptor.runtimeVersion,
    runtimeKind: "sidecar",
    estimatedPeakRssMb: descriptor.estimatedPeakRssMb,
    runtimeArgs: ["--model", "{modelDir}", "--backend", "{backend}", "--threads", "{threads}"],
    capabilities: { dimensions: 2 },
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

class FakeSidecarManager extends SidecarManager {
  readonly optionsSeen: SidecarManagerOptions;
  readonly stopMock = vi.fn(async () => {});

  constructor(options: SidecarManagerOptions) {
    super(options);
    this.optionsSeen = options;
  }

  override async start(): Promise<SidecarReadyMessage> {
    return {
      type: "ready",
      protocol: 1,
      token: "fixture",
      runtimeId: "fixture-runtime",
      runtimeVersion: "1",
      backend: this.optionsSeen.environment?.LINGXI_LOCAL_MODEL_BACKEND ?? "cpu",
      pid: 123,
    };
  }

  override async request<T>(method: string): Promise<T> {
    if (method === "embed") {
      return { vectors: [[0.1, 0.2]], dimensions: 2, modelKey: "local:qwen3-embedding-0.6b@fp8@models-v1" } as T;
    }
    throw new Error(`unexpected method ${method}`);
  }

  override async stop(): Promise<void> {
    await this.stopMock();
  }
}

describe("SidecarInstanceFactory", () => {
  it.each(["stt", "tts"] as const)("%s 显式启用退出确认，重载后进程编号实时更新", async (category) => {
    const { root, descriptor, installed } = fixture();
    const managers: FakeSidecarManager[] = [];
    const factory = new SidecarInstanceFactory({
      runtimeRoot: path.join(root, "runtime"), logRoot: path.join(root, "logs"), platform: "darwin", arch: "arm64",
      config: () => ({ ...DEFAULT_LOCAL_MODELS_CONFIG, backend: "cpu" }),
      createManager: (options) => { const manager = new FakeSidecarManager(options); managers.push(manager); return manager; },
    });
    const instance = await factory.load({ ...descriptor, category }, { ...installed, category }, new AbortController().signal);
    const manager = managers.at(-1)!;
    expect(manager.optionsSeen.terminateOnRequestFailure).toBe(true);
    expect(manager.optionsSeen.maxResponseBytes).toBe(category === "tts" ? 64 * 1024 * 1024 : undefined);
    const ready = await manager.start();
    const diagnostics = vi.spyOn(manager, "diagnostics").mockReturnValue(ready);
    expect(instance.diagnostics?.pid).toBe(123);
    diagnostics.mockReturnValue({ ...ready, pid: 456 });
    expect(instance.diagnostics?.pid).toBe(456);
    diagnostics.mockReturnValue(null);
    expect(instance.diagnostics?.pid).toBeUndefined();
    await factory.unload(instance, { ...descriptor, category }, new AbortController().signal);
  });

  it("verifies the runtime, probes a real backend handshake, expands argv without a shell, and invokes inference", async () => {
    const { root, descriptor, installed } = fixture();
    const managers: FakeSidecarManager[] = [];
    const factory = new SidecarInstanceFactory({
      runtimeRoot: path.join(root, "runtime"),
      logRoot: path.join(root, "logs"),
      platform: "darwin",
      arch: "arm64",
      config: () => DEFAULT_LOCAL_MODELS_CONFIG,
      createManager: (options) => {
        const manager = new FakeSidecarManager(options);
        managers.push(manager);
        return manager;
      },
    });

    const instance = await factory.load(descriptor, installed, new AbortController().signal);
    expect(instance).toMatchObject({ backend: "metal", protocolId: "local-sidecar-embedding" });
    expect(managers).toHaveLength(2);
    expect(managers[0].optionsSeen.args).toEqual([
      "--model", installed.directory, "--backend", "metal", "--threads", "auto",
    ]);
    const output = await instance.embed!({
      model: { id: descriptor.id, quant: descriptor.quant, manifestVersion: descriptor.manifestVersion },
      texts: ["hello"],
      signal: new AbortController().signal,
    });
    expect(output).toMatchObject({ dimensions: 2, vectors: [[0.1, 0.2]] });
    await factory.unload(instance, descriptor, new AbortController().signal);
    expect(managers[1].stopMock).toHaveBeenCalledTimes(1);
  });

  it("rejects a tampered runtime before starting any process", async () => {
    const { root, descriptor, installed } = fixture();
    fs.writeFileSync(path.join(root, "runtime", "fixture-runtime", "1", "darwin-arm64", "bin", "runtime"), "tampered");
    const createManager = vi.fn((options: SidecarManagerOptions) => new FakeSidecarManager(options));
    const factory = new SidecarInstanceFactory({
      runtimeRoot: path.join(root, "runtime"),
      logRoot: path.join(root, "logs"),
      platform: "darwin",
      arch: "arm64",
      config: () => DEFAULT_LOCAL_MODELS_CONFIG,
      createManager,
    });

    await expect(factory.load(descriptor, installed, new AbortController().signal))
      .rejects.toMatchObject({ code: "LOCAL_MODEL_RUNTIME_MISSING" });
    expect(createManager).not.toHaveBeenCalled();
  });

  it("rejects unknown argument placeholders instead of passing them to a process", () => {
    expect(() => expandRuntimeArgs(["--unsafe", "{secret}"], {
      modelDir: "/model",
      backend: "cpu",
      threads: "auto",
      mmap: "true",
      mlock: "false",
    })).toThrowError(expect.objectContaining({ code: "LOCAL_MODEL_MANIFEST_INVALID" }));
  });
});
