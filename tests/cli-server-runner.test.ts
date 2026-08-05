import { afterEach, describe, expect, it, vi } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import { createRequire } from "module";
import {
  buildServeSpawnEnv,
  guardAgainstForeignServer,
  resolveRendererDistPointer,
  resolveServerSpawnSpec,
  spawnServerForeground,
} from "../cli/server-runner.ts";

const require = createRequire(import.meta.url);
const pointerStore = require("../shared/artifact-core/pointer-store.cjs");
const { rendererPointerChannel } = require("../shared/artifact-core/pointer-channels.cjs");

function makeTmpDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "hana-cli-runner-"));
}

/**
 * Writes a `current` renderer pointer for `channel` under `lingxiHome`. When
 * `withReceipt` is true the versioned directory + matching `.verified`
 * receipt are also written, so `activation.resolveBoot` judges it valid;
 * when false the pointer file exists but nothing backs it (simulating an
 * externally deleted/corrupted version directory).
 */
async function writeRendererPointer(lingxiHome, channel, { version = "9.9.9", withReceipt }: { version?: string; withReceipt: boolean }) {
  const rendererChannel = rendererPointerChannel(channel);
  const versionDir = path.join(lingxiHome, "artifacts", "renderer", version);
  const sha256 = "0".repeat(64);
  if (withReceipt) {
    fs.mkdirSync(versionDir, { recursive: true });
    fs.writeFileSync(path.join(versionDir, "mobile.html"), "<!doctype html>", "utf-8");
    fs.writeFileSync(
      path.join(versionDir, ".verified"),
      JSON.stringify({ sha256, train: 1, version, activatedAt: new Date().toISOString() }),
      "utf-8",
    );
  }
  await pointerStore.writePointer(lingxiHome, rendererChannel, "current", {
    train: 1,
    channel: rendererChannel,
    kind: "renderer",
    version,
    platformArch: null,
    versionDir,
    sha256,
    activatedAt: new Date().toISOString(),
  });
  return versionDir;
}

describe("CLI server runner", () => {
  let tmpDir = null;
  let lingxiHome = null;

  afterEach(() => {
    if (tmpDir) fs.rmSync(tmpDir, { recursive: true, force: true });
    if (lingxiHome) fs.rmSync(lingxiHome, { recursive: true, force: true });
    tmpDir = null;
    lingxiHome = null;
  });

  it("runs the source server entry in development", async () => {
    tmpDir = makeTmpDir();
    lingxiHome = makeTmpDir(); // isolated LINGXI_HOME — never touch the real user home's pointers
    const spec = await resolveServerSpawnSpec({
      projectRoot: tmpDir,
      env: { LINGXI_HOME: lingxiHome },
      extraArgs: ["--chat"],
    });

    expect(spec).toMatchObject({
      mode: "source",
      command: process.execPath,
    });
    expect(spec.args).toEqual([path.join(tmpDir, "server", "main-full.ts"), "--chat"]);
    expect(spec.env.LINGXI_RENDERER_DIST).toBeUndefined();
  });

  it("runs the packaged bootstrap entry when LINGXI_ROOT is available", async () => {
    tmpDir = makeTmpDir();
    lingxiHome = makeTmpDir();
    fs.mkdirSync(path.join(tmpDir, "bundle"), { recursive: true });
    fs.writeFileSync(path.join(tmpDir, "bootstrap.js"), "", "utf-8");
    fs.writeFileSync(path.join(tmpDir, "bundle", "index.js"), "", "utf-8");

    const spec = await resolveServerSpawnSpec({
      projectRoot: "/source/project",
      env: { LINGXI_ROOT: tmpDir, LINGXI_HOME: lingxiHome },
      extraArgs: [],
    });

    expect(spec.mode).toBe("packaged");
    expect(spec.args).toEqual([path.join(tmpDir, "bootstrap.js")]);
    expect(spec.env.LINGXI_ROOT).toBe(tmpDir);
    expect(spec.env.LINGXI_SERVER_ENTRY).toBe(path.join(tmpDir, "bundle", "index.js"));
  });

  it("injects LINGXI_RENDERER_DIST into the spawn env when a valid renderer pointer exists", async () => {
    tmpDir = makeTmpDir();
    lingxiHome = makeTmpDir();
    const versionDir = await writeRendererPointer(lingxiHome, "stable", { withReceipt: true });

    const spec = await resolveServerSpawnSpec({
      projectRoot: tmpDir,
      env: { LINGXI_HOME: lingxiHome },
      extraArgs: [],
    });

    expect(spec.env.LINGXI_RENDERER_DIST).toBe(versionDir);
    expect(spec.rendererDist).toEqual({ distDir: versionDir, version: "9.9.9", valid: true });
  });
});

function writeServerInfoFile(lingxiHome, info) {
  fs.mkdirSync(lingxiHome, { recursive: true });
  fs.writeFileSync(path.join(lingxiHome, "server-info.json"), JSON.stringify(info), "utf-8");
}

describe("guardAgainstForeignServer (CLI pre-spawn 同宅互斥预判)", () => {
  let lingxiHome = null;

  afterEach(() => {
    if (lingxiHome) fs.rmSync(lingxiHome, { recursive: true, force: true });
    lingxiHome = null;
  });

  it("does not block when no server-info.json exists", async () => {
    lingxiHome = makeTmpDir();
    const result = await guardAgainstForeignServer({ lingxiHome });
    expect(result).toEqual({ blocked: false, message: null });
  });

  it("blocks when the probe reports alive-same-home, with a message naming ownerKind/version/pid", async () => {
    lingxiHome = makeTmpDir();
    writeServerInfoFile(lingxiHome, { port: 12345, token: "tok", ownerKind: "desktop", version: "0.393.0", pid: 555 });
    const probeImpl = async () => ({ status: "alive-same-home" as const });

    const result = await guardAgainstForeignServer({ lingxiHome, probeImpl });

    expect(result.blocked).toBe(true);
    expect(result.message).toContain("desktop");
    expect(result.message).toContain("0.393.0");
    expect(result.message).toContain("555");
  });

  it("blocks when the probe reports alive-unauthorized", async () => {
    lingxiHome = makeTmpDir();
    writeServerInfoFile(lingxiHome, { port: 12345, token: "tok", ownerKind: "standalone", pid: 1 });
    const probeImpl = async () => ({ status: "alive-unauthorized" as const });

    const result = await guardAgainstForeignServer({ lingxiHome, probeImpl });
    expect(result.blocked).toBe(true);
  });

  it("does not block when the probe reports not-hana or dead — self-cleaning cases", async () => {
    lingxiHome = makeTmpDir();
    writeServerInfoFile(lingxiHome, { port: 12345, token: "tok" });

    const deadResult = await guardAgainstForeignServer({ lingxiHome, probeImpl: async () => ({ status: "dead" as const }) });
    expect(deadResult).toEqual({ blocked: false, message: null });

    const notLingxiResult = await guardAgainstForeignServer({
      lingxiHome,
      probeImpl: async () => ({ status: "not-hana" as const, detail: "whatever" }),
    });
    expect(notLingxiResult).toEqual({ blocked: false, message: null });
  });

  it("ignores whether the recorded pid looks alive — the probe result is the sole source of truth", async () => {
    lingxiHome = makeTmpDir();
    // pid 999999999 is virtually guaranteed not to be alive on this machine,
    // yet the probe (not the pid check) still decides the outcome here.
    writeServerInfoFile(lingxiHome, { port: 12345, token: "tok", ownerKind: "standalone", pid: 999999999 });
    const probeImpl = async () => ({ status: "alive-same-home" as const });

    const result = await guardAgainstForeignServer({ lingxiHome, probeImpl });
    expect(result.blocked).toBe(true);
  });
});

describe("spawnServerForeground — blocked path never spawns", () => {
  let lingxiHome = null;

  afterEach(() => {
    if (lingxiHome) fs.rmSync(lingxiHome, { recursive: true, force: true });
    lingxiHome = null;
  });

  it("exits 1 and never resolves resolveServerSpawnSpec/spawn when a foreign server is detected", async () => {
    lingxiHome = makeTmpDir();
    writeServerInfoFile(lingxiHome, { port: 12345, token: "tok", ownerKind: "standalone", version: "0.393.0", pid: 1 });
    const probeImpl = async () => ({ status: "alive-same-home" as const });
    const exitCalls: any[] = [];
    const exit = ((code?: number) => { exitCalls.push(code); return undefined as any; }) as any;
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});

    const result = await spawnServerForeground({
      projectRoot: "/nonexistent/project/root/that/must/never/be/touched",
      env: { LINGXI_HOME: lingxiHome },
      probeImpl,
      exit,
    });

    expect(exitCalls).toEqual([1]);
    expect(result).toBeUndefined();
    expect(errorSpy).toHaveBeenCalled();
    errorSpy.mockRestore();
  });
});

describe("buildServeSpawnEnv", () => {
  it("passes the env through unchanged and does not warn when allowDataDowngrade is false", () => {
    const warn = vi.fn();
    const result = buildServeSpawnEnv({ env: { FOO: "bar" }, allowDataDowngrade: false, warn });
    expect(result).toEqual({ FOO: "bar" });
    expect(result.LINGXI_ALLOW_DATA_DOWNGRADE).toBeUndefined();
    expect(warn).not.toHaveBeenCalled();
  });

  it("sets LINGXI_ALLOW_DATA_DOWNGRADE=1 and warns when allowDataDowngrade is true", () => {
    const warn = vi.fn();
    const result = buildServeSpawnEnv({ env: { FOO: "bar" }, allowDataDowngrade: true, warn });
    expect(result).toEqual({ FOO: "bar", LINGXI_ALLOW_DATA_DOWNGRADE: "1" });
    expect(warn).toHaveBeenCalledTimes(1);
    expect(warn.mock.calls[0][0]).toContain("--allow-data-downgrade");
  });
});

describe("resolveRendererDistPointer (CLI pointer resolution layer)", () => {
  let lingxiHome = null;

  afterEach(() => {
    if (lingxiHome) fs.rmSync(lingxiHome, { recursive: true, force: true });
    lingxiHome = null;
  });

  it("pointer present and validated -> returns the versionDir, valid: true", async () => {
    lingxiHome = makeTmpDir();
    const versionDir = await writeRendererPointer(lingxiHome, "stable", { withReceipt: true });

    const result = await resolveRendererDistPointer({ lingxiHome, channel: "stable" });
    expect(result).toEqual({ distDir: versionDir, version: "9.9.9", valid: true });
  });

  it("no pointer at all (never bundle pull'd) -> returns null, caller must not set the env var", async () => {
    lingxiHome = makeTmpDir();

    const result = await resolveRendererDistPointer({ lingxiHome, channel: "stable" });
    expect(result).toBeNull();
  });

  it("pointer present but its versionDir is missing/corrupted -> still returns the (invalid) versionDir, not null", async () => {
    lingxiHome = makeTmpDir();
    const versionDir = await writeRendererPointer(lingxiHome, "stable", { withReceipt: false });

    const result = await resolveRendererDistPointer({ lingxiHome, channel: "stable" });
    // Damage must stay visible: the caller sets LINGXI_RENDERER_DIST to this
    // broken path anyway so the server lands in its explicit error mode,
    // instead of this function silently reporting "nothing to inject"
    // (which would masquerade as the guide-mode "never installed" case).
    expect(result).toEqual({ distDir: versionDir, version: "9.9.9", valid: false });
  });

  it("channels are namespaced independently (a beta pointer does not satisfy a stable resolve)", async () => {
    lingxiHome = makeTmpDir();
    await writeRendererPointer(lingxiHome, "beta", { withReceipt: true });

    const result = await resolveRendererDistPointer({ lingxiHome, channel: "stable" });
    expect(result).toBeNull();
  });
});
