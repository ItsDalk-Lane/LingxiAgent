import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createRequire } from "module";
import fs from "fs";
import os from "os";
import path from "path";

const require = createRequire(import.meta.url);
const { maybeHealWin32InstallAcl, buildInstallAclHealDiagnostics } = require(
  "../desktop/src/shared/win32-install-acl-heal.cjs",
);

let root: string | null;

function makeHome() {
  root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-acl-heal-"));
  return root;
}

function readJson(p: string) {
  return JSON.parse(fs.readFileSync(p, "utf-8"));
}

function healStatePath(lingxiHome: string) {
  return path.join(lingxiHome, "user", "win32-install-acl-heal.json");
}

function gpuStatePath(lingxiHome: string) {
  return path.join(lingxiHome, "user", "gpu-startup.json");
}

function writeGpuState(lingxiHome: string, state: object) {
  fs.mkdirSync(path.dirname(gpuStatePath(lingxiHome)), { recursive: true });
  fs.writeFileSync(gpuStatePath(lingxiHome), JSON.stringify(state, null, 2) + "\n", "utf-8");
}

const INSTALL_DIR = "C:\\Users\\demo\\AppData\\Local\\Programs\\LingxiAgent";
const ICACLS = "C:\\Windows\\System32\\icacls.exe";

function baseInput(lingxiHome: string, overrides: object = {}) {
  return {
    lingxiHome,
    platform: "win32",
    isPackaged: true,
    installDir: INSTALL_DIR,
    appVersion: "0.442.0",
    env: { SystemRoot: "C:\\Windows" },
    execFileSync: vi.fn(),
    ...overrides,
  };
}

describe("win32 install ACL heal", () => {
  beforeEach(() => {
    root = null;
  });

  afterEach(() => {
    if (root) fs.rmSync(root, { recursive: true, force: true });
  });

  it("skips on non-win32 without touching state or exec", () => {
    const lingxiHome = makeHome();
    const input = baseInput(lingxiHome, { platform: "darwin" });
    const result = maybeHealWin32InstallAcl(input);
    expect(result).toEqual({ status: "skipped", reason: "platform" });
    expect(input.execFileSync).not.toHaveBeenCalled();
    expect(fs.existsSync(healStatePath(lingxiHome))).toBe(false);
  });

  it("skips when not packaged unless LINGXI_FORCE_INSTALL_ACL_HEAL is set", () => {
    const lingxiHome = makeHome();
    const input = baseInput(lingxiHome, { isPackaged: false });
    expect(maybeHealWin32InstallAcl(input)).toEqual({ status: "skipped", reason: "not-packaged" });

    const forced = baseInput(lingxiHome, {
      isPackaged: false,
      env: { SystemRoot: "C:\\Windows", LINGXI_FORCE_INSTALL_ACL_HEAL: "1" },
    });
    const result = maybeHealWin32InstallAcl(forced);
    expect(result.status).toBe("healed");
    expect(forced.execFileSync).toHaveBeenCalledTimes(1);
  });

  it("skips when LINGXI_DISABLE_INSTALL_ACL_HEAL is set", () => {
    const lingxiHome = makeHome();
    const input = baseInput(lingxiHome, {
      env: { SystemRoot: "C:\\Windows", LINGXI_DISABLE_INSTALL_ACL_HEAL: "1" },
    });
    expect(maybeHealWin32InstallAcl(input)).toEqual({ status: "skipped", reason: "disabled" });
    expect(input.execFileSync).not.toHaveBeenCalled();
  });

  it("grants the ACE with the exact icacls invocation and probes recovery on first run", () => {
    const lingxiHome = makeHome();
    writeGpuState(lingxiHome, {
      version: 2,
      autoGpuMode: { mode: "software-safe", reason: "gpu-child-process-gone", updatedAt: "2026-08-01T00:00:00.000Z" },
      lastGpuCrash: { type: "GPU", reason: "crashed", at: "2026-08-01T00:00:00.000Z", platform: "win32" },
      startup: { status: "pending", startupId: "1-1", phase: "splash-ready", platform: "win32", startedAt: "2026-08-01T00:00:00.000Z", updatedAt: "2026-08-01T00:00:00.000Z" },
    });

    const input = baseInput(lingxiHome, { now: "2026-08-03T00:00:00.000Z" });
    const result = maybeHealWin32InstallAcl(input);

    expect(input.execFileSync).toHaveBeenCalledWith(
      ICACLS,
      [INSTALL_DIR, "/grant", "*S-1-15-2-2:(OI)(CI)(RX)"],
      expect.objectContaining({ timeout: 15000, windowsHide: true }),
    );
    expect(result).toMatchObject({ status: "healed", probed: true, clearedMode: "software-safe" });

    const heal = readJson(healStatePath(lingxiHome));
    expect(heal.heal).toMatchObject({
      installDir: INSTALL_DIR,
      appVersion: "0.442.0",
      status: "ok",
      failureCount: 0,
      healedAt: "2026-08-03T00:00:00.000Z",
      probedAt: "2026-08-03T00:00:00.000Z",
      clearedMode: "software-safe",
    });

    const gpu = readJson(gpuStatePath(lingxiHome));
    expect(gpu.autoGpuMode).toBeUndefined();
    expect(gpu.startup).toBeUndefined();
    expect(gpu.lastGpuRecovery).toMatchObject({ reason: "win32-install-acl-heal", clearedMode: "software-safe" });
  });

  it("does not re-run icacls for the same install identity when nothing crashed since the probe", () => {
    const lingxiHome = makeHome();
    const first = baseInput(lingxiHome);
    maybeHealWin32InstallAcl(first);

    const second = baseInput(lingxiHome);
    const result = maybeHealWin32InstallAcl(second);
    expect(result).toEqual({ status: "skipped", reason: "already-healed" });
    expect(second.execFileSync).not.toHaveBeenCalled();
  });

  it("re-grants when the app version changes", () => {
    const lingxiHome = makeHome();
    maybeHealWin32InstallAcl(baseInput(lingxiHome));

    const next = baseInput(lingxiHome, { appVersion: "0.443.0" });
    const result = maybeHealWin32InstallAcl(next);
    expect(next.execFileSync).toHaveBeenCalledTimes(1);
    expect(result.status).toBe("healed");
  });

  it("records grant failures and stops after three consecutive failures", () => {
    const lingxiHome = makeHome();
    const failingExec = vi.fn(() => {
      const err: any = new Error("Access is denied.");
      err.status = 5;
      throw err;
    });

    for (let i = 1; i <= 3; i += 1) {
      const input = baseInput(lingxiHome, { execFileSync: failingExec });
      const result = maybeHealWin32InstallAcl(input);
      expect(result).toMatchObject({ status: "grant-failed" });
      expect(readJson(healStatePath(lingxiHome)).heal).toMatchObject({ status: "failed", failureCount: i });
    }

    const capped = baseInput(lingxiHome, { execFileSync: failingExec });
    expect(maybeHealWin32InstallAcl(capped)).toEqual({ status: "skipped", reason: "failure-cap" });
    expect(failingExec).toHaveBeenCalledTimes(3);
  });

  it("marks the heal ineffective and restores the deeper pre-probe mode when a crash lands after the probe", () => {
    const lingxiHome = makeHome();
    writeGpuState(lingxiHome, {
      version: 2,
      autoGpuMode: { mode: "deep-compat", reason: "gpu-child-process-gone", updatedAt: "2026-08-01T00:00:00.000Z" },
      lastGpuCrash: { type: "GPU", reason: "crashed", at: "2026-08-01T00:00:00.000Z", platform: "win32" },
    });
    maybeHealWin32InstallAcl(baseInput(lingxiHome, { now: "2026-08-03T00:00:00.000Z" }));

    writeGpuState(lingxiHome, {
      version: 2,
      autoGpuMode: { mode: "gpu-sandbox-compat", reason: "gpu-child-process-gone", updatedAt: "2026-08-03T01:00:00.000Z" },
      lastGpuCrash: { type: "GPU", reason: "crashed", at: "2026-08-03T01:00:00.000Z", platform: "win32" },
      startup: { status: "pending", startupId: "2-2", phase: "splash-ready", platform: "win32", startedAt: "2026-08-03T01:00:00.000Z", updatedAt: "2026-08-03T01:00:00.000Z", policy: { mode: "hardware" } },
    });

    const input = baseInput(lingxiHome, { now: "2026-08-03T02:00:00.000Z" });
    const result = maybeHealWin32InstallAcl(input);
    expect(result).toMatchObject({ status: "ineffective" });
    expect(input.execFileSync).not.toHaveBeenCalled();

    const heal = readJson(healStatePath(lingxiHome));
    expect(heal.heal.status).toBe("ineffective");
    expect(heal.ineffectiveCount).toBe(1);

    const gpu = readJson(gpuStatePath(lingxiHome));
    expect(gpu.autoGpuMode.mode).toBe("deep-compat");
    expect(gpu.startup).toBeUndefined();
  });

  it("keeps the current mode when it is already deeper than the pre-probe mode", () => {
    const lingxiHome = makeHome();
    writeGpuState(lingxiHome, {
      version: 2,
      autoGpuMode: { mode: "gpu-sandbox-compat", reason: "gpu-child-process-gone", updatedAt: "2026-08-01T00:00:00.000Z" },
      lastGpuCrash: { type: "GPU", reason: "crashed", at: "2026-08-01T00:00:00.000Z", platform: "win32" },
    });
    maybeHealWin32InstallAcl(baseInput(lingxiHome, { now: "2026-08-03T00:00:00.000Z" }));

    writeGpuState(lingxiHome, {
      version: 2,
      autoGpuMode: { mode: "deep-compat", reason: "gpu-child-process-gone", updatedAt: "2026-08-03T01:00:00.000Z" },
      lastGpuCrash: { type: "GPU", reason: "crashed", at: "2026-08-03T01:00:00.000Z", platform: "win32" },
    });

    const result = maybeHealWin32InstallAcl(baseInput(lingxiHome, { now: "2026-08-03T02:00:00.000Z" }));
    expect(result).toMatchObject({ status: "ineffective" });
    expect(readJson(gpuStatePath(lingxiHome)).autoGpuMode.mode).toBe("deep-compat");
  });

  it("stops probing (grant-only) after two ineffective verdicts across versions", () => {
    const lingxiHome = makeHome();
    fs.mkdirSync(path.dirname(healStatePath(lingxiHome)), { recursive: true });
    fs.writeFileSync(
      healStatePath(lingxiHome),
      JSON.stringify({ version: 1, ineffectiveCount: 2, heal: { installDir: INSTALL_DIR, appVersion: "0.441.0", status: "ineffective", failureCount: 0, healedAt: "2026-08-01T00:00:00.000Z", probedAt: "2026-08-01T00:00:00.000Z", clearedMode: "software-safe", lastError: null, updatedAt: "2026-08-01T00:00:00.000Z" } }, null, 2),
      "utf-8",
    );
    writeGpuState(lingxiHome, {
      version: 2,
      autoGpuMode: { mode: "software-safe", reason: "gpu-child-process-gone", updatedAt: "2026-08-02T00:00:00.000Z" },
    });

    const input = baseInput(lingxiHome, { appVersion: "0.443.0", now: "2026-08-03T00:00:00.000Z" });
    const result = maybeHealWin32InstallAcl(input);
    expect(input.execFileSync).toHaveBeenCalledTimes(1);
    expect(result).toMatchObject({ status: "healed", probed: false });
    expect(readJson(gpuStatePath(lingxiHome)).autoGpuMode.mode).toBe("software-safe");
  });

  it("resets ineffectiveCount after a completed hardware startup follows the probe", () => {
    const lingxiHome = makeHome();
    fs.mkdirSync(path.dirname(healStatePath(lingxiHome)), { recursive: true });
    fs.writeFileSync(
      healStatePath(lingxiHome),
      JSON.stringify({ version: 1, ineffectiveCount: 1, heal: { installDir: INSTALL_DIR, appVersion: "0.442.0", status: "ok", failureCount: 0, healedAt: "2026-08-03T00:00:00.000Z", probedAt: "2026-08-03T00:00:00.000Z", clearedMode: "software-safe", lastError: null, updatedAt: "2026-08-03T00:00:00.000Z" } }, null, 2),
      "utf-8",
    );
    writeGpuState(lingxiHome, {
      version: 2,
      startup: { status: "ready", startupId: "3-3", phase: "app-ready", platform: "win32", startedAt: "2026-08-03T01:00:00.000Z", readyAt: "2026-08-03T01:00:30.000Z", updatedAt: "2026-08-03T01:00:30.000Z", policy: { mode: "hardware" } },
    });

    const result = maybeHealWin32InstallAcl(baseInput(lingxiHome));
    expect(result).toEqual({ status: "stable" });
    expect(readJson(healStatePath(lingxiHome)).ineffectiveCount).toBe(0);
  });

  it("resumes the recovery probe when a previous launch stopped between grant and probe", () => {
    const lingxiHome = makeHome();
    fs.mkdirSync(path.dirname(healStatePath(lingxiHome)), { recursive: true });
    fs.writeFileSync(
      healStatePath(lingxiHome),
      JSON.stringify({ version: 1, ineffectiveCount: 0, heal: { installDir: INSTALL_DIR, appVersion: "0.442.0", status: "ok", failureCount: 0, healedAt: "2026-08-03T00:00:00.000Z", probedAt: null, clearedMode: null, lastError: null, updatedAt: "2026-08-03T00:00:00.000Z" } }, null, 2),
      "utf-8",
    );
    writeGpuState(lingxiHome, {
      version: 2,
      autoGpuMode: { mode: "software-safe", reason: "gpu-child-process-gone", updatedAt: "2026-08-01T00:00:00.000Z" },
    });

    const input = baseInput(lingxiHome, { now: "2026-08-03T01:00:00.000Z" });
    const result = maybeHealWin32InstallAcl(input);
    expect(input.execFileSync).not.toHaveBeenCalled();
    expect(result).toMatchObject({ status: "healed", probed: true, clearedMode: "software-safe" });
    expect(readJson(healStatePath(lingxiHome)).heal.probedAt).toBe("2026-08-03T01:00:00.000Z");
    expect(readJson(gpuStatePath(lingxiHome)).autoGpuMode).toBeUndefined();
  });

  it("treats a corrupt heal state file as empty and re-grants", () => {
    const lingxiHome = makeHome();
    fs.mkdirSync(path.dirname(healStatePath(lingxiHome)), { recursive: true });
    fs.writeFileSync(healStatePath(lingxiHome), "{not json", "utf-8");

    const input = baseInput(lingxiHome);
    const result = maybeHealWin32InstallAcl(input);
    expect(result.status).toBe("healed");
    expect(input.execFileSync).toHaveBeenCalledTimes(1);
  });

  it("compares install identity case-insensitively", () => {
    const lingxiHome = makeHome();
    maybeHealWin32InstallAcl(baseInput(lingxiHome));

    const lower = baseInput(lingxiHome, { installDir: INSTALL_DIR.toLowerCase() });
    expect(maybeHealWin32InstallAcl(lower)).toEqual({ status: "skipped", reason: "already-healed" });
    expect(lower.execFileSync).not.toHaveBeenCalled();
  });

  it("buildInstallAclHealDiagnostics reports state and the manual command", () => {
    const lingxiHome = makeHome();
    maybeHealWin32InstallAcl(baseInput(lingxiHome));
    const text = buildInstallAclHealDiagnostics({ lingxiHome });
    expect(text).toContain("--- Install ACL Heal ---");
    expect(text).toContain("S-1-15-2-2:(OI)(CI)(RX)");
    expect(text).toContain("LINGXI_DISABLE_INSTALL_ACL_HEAL");
  });
});
