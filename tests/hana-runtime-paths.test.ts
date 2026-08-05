import { describe, expect, it } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";

import {
  resolveHanakoHome,
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

    expect(resolveHanakoHome("~/.lingxi-dev", homeDir)).toBe(path.join(homeDir, ".lingxi-dev"));
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
});
