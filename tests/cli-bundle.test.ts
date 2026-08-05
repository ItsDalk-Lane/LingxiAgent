import fs from "fs";
import os from "os";
import path from "path";
import { createRequire } from "module";
import { afterEach, describe, expect, it, vi } from "vitest";
import { parseCliArgs, helpText } from "../cli/args.ts";
import { runBundlePull, runBundleStatus } from "../cli/bundle.ts";
import { SERVER_PROTOCOL_VERSION } from "../shared/contract-versions.cjs";

const require = createRequire(import.meta.url);
const pointerStore = require("../shared/artifact-core/pointer-store.cjs");
const { rendererPointerChannel } = require("../shared/artifact-core/pointer-channels.cjs");
const otaCore = require("../shared/artifact-core/ota-core.cjs");

const tempDirs: string[] = [];

function makeTempDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-cli-bundle-"));
  tempDirs.push(dir);
  return dir;
}

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
  vi.restoreAllMocks();
});

describe("CLI args: bundle command family", () => {
  it("parses bundle pull with the default channel", () => {
    expect(parseCliArgs(["bundle", "pull"])).toMatchObject({
      command: "bundle",
      subcommand: "pull",
      channel: "stable",
    });
  });

  it("parses bundle status with an explicit channel", () => {
    expect(parseCliArgs(["bundle", "status", "--channel", "beta"])).toMatchObject({
      command: "bundle",
      subcommand: "status",
      channel: "beta",
    });
  });

  it("rejects a missing bundle subcommand", () => {
    expect(parseCliArgs(["bundle"])).toMatchObject({
      command: "help",
      error: expect.stringMatching(/pull or status/i),
    });
  });

  it("rejects an unknown bundle subcommand", () => {
    expect(parseCliArgs(["bundle", "frobnicate"])).toMatchObject({
      command: "help",
      error: expect.stringMatching(/frobnicate/),
    });
  });

  it("rejects an unknown channel value", () => {
    expect(() => parseCliArgs(["bundle", "pull", "--channel", "nightly"])).toThrow(/stable.*beta/);
  });

  it("rejects a missing --channel value", () => {
    expect(() => parseCliArgs(["bundle", "pull", "--channel"])).toThrow("--channel requires a value");
  });

  it("mentions the bundle commands in the help text", () => {
    expect(helpText()).toContain("bundle pull");
    expect(helpText()).toContain("bundle status");
  });
});

describe("CLI bundle pull (mocked core download — the network pipeline is covered in artifact-ota tests)", () => {
  it("passes homeDir, channel, keyset, and serverProtocolVersion through to the core, without any devBypass", async () => {
    const home = makeTempDir();
    const download = vi.fn().mockResolvedValue({ ok: true, train: 2, version: "0.400.0" });
    vi.spyOn(console, "log").mockImplementation(() => {});

    const code = await runBundlePull({ channel: "beta", hanaHome: home, download });

    expect(code).toBe(0);
    expect(download).toHaveBeenCalledTimes(1);
    const opts = download.mock.calls[0][0];
    expect(opts.homeDir).toBe(home);
    expect(opts.channel).toBe("beta");
    expect(opts.serverProtocolVersion).toBe(SERVER_PROTOCOL_VERSION);
    expect(Array.isArray(opts.keyset)).toBe(true);
    expect(opts.keyset.length).toBeGreaterThan(0);
    // The self-hosted CLI has no dev rehearsal switch: the core's
    // NO_DEV_OVERRIDE default must apply, so the key is never even passed.
    expect("devBypass" in opts).toBe(false);
  });

  it("prints the activated version and the restart hint on success", async () => {
    const home = makeTempDir();
    const download = vi.fn().mockResolvedValue({ ok: true, train: 2, version: "0.400.0" });
    const logSpy = vi.spyOn(console, "log").mockImplementation(() => {});

    const code = await runBundlePull({ hanaHome: home, download });

    expect(code).toBe(0);
    const output = logSpy.mock.calls.map((call) => call.join(" ")).join("\n");
    expect(output).toContain("0.400.0");
    expect(output).toMatch(/restart hana serve/i);
  });

  it("prints an up-to-date line for an alreadyCurrent short-circuit", async () => {
    const home = makeTempDir();
    const download = vi.fn().mockResolvedValue({ ok: true, alreadyCurrent: true, version: "0.400.0" });
    const logSpy = vi.spyOn(console, "log").mockImplementation(() => {});

    const code = await runBundlePull({ hanaHome: home, download });

    expect(code).toBe(0);
    const output = logSpy.mock.calls.map((call) => call.join(" ")).join("\n");
    expect(output).toMatch(/already up to date/i);
    expect(output).toContain("0.400.0");
  });

  it("prints the error and exits 1 on failure", async () => {
    const home = makeTempDir();
    const download = vi.fn().mockResolvedValue({ ok: false, error: "train 3 is quarantined" });
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});

    const code = await runBundlePull({ hanaHome: home, download });

    expect(code).toBe(1);
    const output = errorSpy.mock.calls.map((call) => call.join(" ")).join("\n");
    expect(output).toContain("train 3 is quarantined");
  });
});

describe("CLI bundle status", () => {
  it("points at bundle pull when nothing has been pulled yet", async () => {
    const home = makeTempDir();
    const logSpy = vi.spyOn(console, "log").mockImplementation(() => {});

    const code = await runBundleStatus({ hanaHome: home });

    expect(code).toBe(0);
    const output = logSpy.mock.calls.map((call) => call.join(" ")).join("\n");
    expect(output).toMatch(/hana bundle pull/);
  });

  it("prints the activated version, train, last check time, and available version", async () => {
    const home = makeTempDir();
    await pointerStore.writePointer(home, rendererPointerChannel("stable"), "current", {
      train: 3,
      kind: "renderer",
      version: "0.400.0",
      sha256: "b".repeat(64),
    });
    await otaCore.writeOtaChannelState(home, "stable", {
      lastCheckedAt: "2026-07-13T00:00:00.000Z",
      available: { train: 4, version: "0.401.0" },
    });
    const logSpy = vi.spyOn(console, "log").mockImplementation(() => {});

    const code = await runBundleStatus({ hanaHome: home });

    expect(code).toBe(0);
    const output = logSpy.mock.calls.map((call) => call.join(" ")).join("\n");
    expect(output).toContain("0.400.0");
    expect(output).toContain("3");
    expect(output).toContain("2026-07-13T00:00:00.000Z");
    expect(output).toContain("0.401.0");
  });

  it("reads the channel's own renderer pointer namespace", async () => {
    const home = makeTempDir();
    await pointerStore.writePointer(home, rendererPointerChannel("beta"), "current", {
      train: 9,
      kind: "renderer",
      version: "0.500.0",
      sha256: "c".repeat(64),
    });
    const logSpy = vi.spyOn(console, "log").mockImplementation(() => {});

    const code = await runBundleStatus({ channel: "beta", hanaHome: home });

    expect(code).toBe(0);
    const output = logSpy.mock.calls.map((call) => call.join(" ")).join("\n");
    expect(output).toContain("0.500.0");
  });
});
