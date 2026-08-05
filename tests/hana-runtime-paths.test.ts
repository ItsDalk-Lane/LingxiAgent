import { describe, expect, it } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";

import {
  resolveLingxiHome,
  migrateLegacyHanakoHome,
  resolveLingxiPiSdkManagedBinDir,
  resolveLingxiPiSdkResourceLoaderAgentDir,
  resolveLingxiPiSdkResourceLoaderCwd,
  resolveLingxiPiSdkRuntimeRoot,
  resolveLegacyPiSdkManagedBinDir,
} from "../shared/hana-runtime-paths.ts";

describe("Hana runtime path contracts", () => {
  it("derives Hana-owned Pi SDK runtime paths from LINGXI_HOME", () => {
    const lingxiHome = path.join(os.tmpdir(), "hana-runtime-paths", ".lingxi-dev");
    const runtimeRoot = path.join(lingxiHome, "runtime", "pi-sdk");

    expect(resolveLingxiPiSdkRuntimeRoot(lingxiHome)).toBe(runtimeRoot);
    expect(resolveLingxiPiSdkManagedBinDir(lingxiHome)).toBe(path.join(runtimeRoot, "bin"));
    expect(resolveLingxiPiSdkResourceLoaderCwd(lingxiHome)).toBe(path.join(runtimeRoot, "resource-loader", "project"));
    expect(resolveLingxiPiSdkResourceLoaderAgentDir(lingxiHome)).toBe(path.join(runtimeRoot, "resource-loader", "agent"));
  });

  it("normalizes LINGXI_HOME before deriving Pi SDK paths", () => {
    const homeDir = path.join(os.tmpdir(), "hana-runtime-home");

    expect(resolveLingxiHome("~/.lingxi-dev", homeDir)).toBe(path.join(homeDir, ".lingxi-dev"));
  });

  it("keeps legacy Pi binary lookup explicit without creating either tree", () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-runtime-dirs-"));
    const lingxiHome = path.join(root, ".lingxi");

    expect(resolveLegacyPiSdkManagedBinDir(lingxiHome)).toBe(
      path.join(lingxiHome, ".pi", "agent", "bin"),
    );
    expect(resolveLingxiPiSdkManagedBinDir(lingxiHome)).toBe(
      path.join(lingxiHome, "runtime", "pi-sdk", "bin"),
    );

    expect(fs.existsSync(lingxiHome)).toBe(false);
    fs.rmSync(root, { recursive: true, force: true });
  });

  it("migrates a legacy ~/.hanako home to ~/.lingxi when the new home is absent", () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-legacy-migrate-"));
    const legacyHome = path.join(root, ".hanako");
    const lingxiHome = path.join(root, ".lingxi");
    fs.mkdirSync(path.join(legacyHome, "user"), { recursive: true });
    fs.writeFileSync(path.join(legacyHome, "user", "preferences.json"), "{}");

    expect(migrateLegacyHanakoHome(root)).toBe(lingxiHome);
    expect(fs.existsSync(legacyHome)).toBe(false);
    expect(fs.readFileSync(path.join(lingxiHome, "user", "preferences.json"), "utf-8")).toBe("{}");
    fs.rmSync(root, { recursive: true, force: true });
  });

  it("never overwrites an existing ~/.lingxi during legacy migration", () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-legacy-keep-"));
    fs.mkdirSync(path.join(root, ".hanako"), { recursive: true });
    fs.mkdirSync(path.join(root, ".lingxi"), { recursive: true });

    expect(migrateLegacyHanakoHome(root)).toBe(null);
    expect(fs.existsSync(path.join(root, ".hanako"))).toBe(true);
    fs.rmSync(root, { recursive: true, force: true });
  });
});
