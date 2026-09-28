import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { afterEach, describe, expect, it } from "vitest";

import {
  REQUIRED_STANDALONE_SERVER_FILES,
  STANDALONE_LAYOUT_ROOT,
  buildWindowsStandaloneArtifact,
  standaloneArtifactNames,
  standaloneWrapperContents,
} from "../scripts/build-standalone-server-artifact.mjs";
import {
  standaloneExecCommandSmokeSpec,
  standaloneRestrictedTokenSmokeSpec,
  verifyWindowsStandaloneArtifact,
} from "../scripts/verify-standalone-server-artifact.mjs";
import { resolveWindowsRustReader } from "../cli/rust-service.ts";
import { rustSourceDigest } from "../scripts/build-rust-desktop-service.mjs";

const require = createRequire(import.meta.url);
const ustar = require("../shared/artifact-core/ustar.cjs");
const { rustServiceContentDigest } = require("../desktop/src/shared/rust-local-service.cjs");
const tempRoots: string[] = [];

function makeTempRoot() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-standalone-test-"));
  tempRoots.push(root);
  fs.writeFileSync(path.join(root, "package.json"), `${JSON.stringify({ version: "1.2.3" })}\n`);
  return root;
}

function writeFile(root: string, relative: string, content = relative) {
  const target = path.join(root, ...relative.split("/"));
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(target, content);
}

function createInputs(root: string) {
  const serverDir = path.join(root, "dist-server", "win-x64");
  for (const relative of REQUIRED_STANDALONE_SERVER_FILES) writeFile(serverDir, relative);
  fs.writeFileSync(path.join(serverDir, "node_modules/usearch/package.json"), JSON.stringify({ version: "2.26.0" }));
  writeFile(serverDir, "package.json", '{"version":"1.2.3"}\n');
  writeFile(serverDir, "lib/runtime.json", "server source must remain unchanged\n");

  const gitDir = path.join(root, "vendor", "mingit");
  writeFile(gitDir, "cmd/git.exe", "git cmd");
  writeFile(gitDir, "mingw64/bin/git.exe", "git mingw");
  writeFile(gitDir, "usr/bin/sh.exe", "posix shell");
  writeFile(gitDir, "usr/bin/grep.exe", "coreutils");

  const helperPath = path.join(root, "dist-sandbox", "win-x64", "lingxi-win-sandbox.exe");
  writeFile(path.dirname(helperPath), path.basename(helperPath), "sandbox helper");

  const rustDir = path.join(serverDir, "rust-service");
  const executable = Buffer.alloc(2048);
  executable.write("MZ", 0);
  executable.writeUInt32LE(128, 0x3c);
  executable.write("PE\0\0", 128);
  executable.writeUInt16LE(0x8664, 132);
  executable.writeUInt16LE(1, 134);
  executable.writeUInt16LE(240, 148);
  executable.writeUInt16LE(0x20b, 152);
  executable.writeUInt32LE(1536, 408);
  executable.writeUInt32LE(512, 412);
  fs.mkdirSync(rustDir, { recursive: true });
  fs.writeFileSync(path.join(rustDir, "lingxi-service.exe"), executable);
  const sha256 = createHash("sha256").update(executable).digest("hex");
  fs.writeFileSync(path.join(rustDir, "build.json"), JSON.stringify({
    schemaVersion: 1, platform: "win", arch: "x64", target: "x86_64-pc-windows-msvc",
    toolchain: "1.98.1", appVersion: "1.2.3", binary: "lingxi-service.exe",
    sha256, contentSha256: rustServiceContentDigest(executable, "win32"), sourceSha256: "a".repeat(64),
  }));

  return {
    serverDir,
    gitDir,
    helperPath,
    rustDir,
  };
}

function snapshotTree(root: string) {
  const snapshot: Record<string, string> = {};
  function walk(dir: string) {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const absolute = path.join(dir, entry.name);
      const relative = path.relative(root, absolute).split(path.sep).join("/");
      if (entry.isDirectory()) {
        snapshot[`${relative}/`] = "directory";
        walk(absolute);
      } else {
        snapshot[relative] = createHash("sha256").update(fs.readFileSync(absolute)).digest("hex");
      }
    }
  }
  walk(root);
  return snapshot;
}

afterEach(() => {
  for (const root of tempRoots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
});

describe("Windows standalone server artifact", () => {
  it("keeps the standalone output outside Electron seed resources and the OTA server namespace", () => {
    const repoRoot = path.resolve(import.meta.dirname, "..");
    const packageJson = JSON.parse(fs.readFileSync(path.join(repoRoot, "package.json"), "utf8"));
    const commonResources = packageJson.build.extraResources as Array<{ from: string; to: string }>;
    const windowsResources = packageJson.build.win.extraResources as Array<{ from: string; to: string }>;

    expect(commonResources.some((resource) => resource.from.includes("dist-standalone"))).toBe(false);
    expect(commonResources).toContainEqual({ from: "dist-server-artifact/${os}-${arch}/", to: "seed/" });
    expect(windowsResources).toContainEqual({ from: "vendor/mingit", to: "git" });
    expect(windowsResources).toContainEqual({ from: "dist-sandbox/win-${arch}/", to: "sandbox/windows/" });
    expect(standaloneArtifactNames("1.2.3").archiveName).not.toMatch(/^server-/);
    expect(fs.readFileSync(path.join(repoRoot, ".gitignore"), "utf8")).toMatch(/^dist-standalone\/$/m);
  });

  it("builds a SHA-256-manifested LingxiCore archive without mutating the thin server tree", async () => {
    const root = makeTempRoot();
    const inputs = createInputs(root);
    const before = snapshotTree(inputs.serverDir);

    const result = await buildWindowsStandaloneArtifact({ rootDir: root, log: () => {} });
    const names = standaloneArtifactNames("1.2.3");

    expect(path.basename(result.archivePath)).toBe("LingxiCore-1.2.3-Windows-x64.tar.gz");
    expect(path.basename(result.archivePath)).toBe(names.archiveName);
    expect(path.basename(result.archivePath)).not.toMatch(/^server-/);
    expect(result.archivePath).toContain(`${path.sep}dist-standalone${path.sep}`);
    expect(fs.existsSync(path.join(root, "dist-server-artifact"))).toBe(false);
    expect(snapshotTree(inputs.serverDir)).toEqual(before);
    expect(fs.existsSync(result.manifestPath)).toBe(true);
    expect(fs.readdirSync(path.join(root, "dist-standalone")).sort()).toEqual([
      names.manifestName,
      names.archiveName,
    ].sort());
    expect(result).not.toHaveProperty("signaturePath");
    expect(standaloneArtifactNames("1.2.3")).not.toHaveProperty("signatureName");

    const manifest = JSON.parse(fs.readFileSync(result.manifestPath, "utf8"));
    expect(manifest).toMatchObject({
      schema: 1,
      kind: "hana-core-standalone",
      version: "1.2.3",
      platform: "win32",
      arch: "x64",
      archive: { path: names.archiveName },
      layout: { root: STANDALONE_LAYOUT_ROOT },
      runtime: { minGitVersion: "2.55.0" },
    });

    const extractDir = path.join(root, "extracted");
    await ustar.extract(result.archivePath, extractDir);
    const layoutRoot = path.join(extractDir, STANDALONE_LAYOUT_ROOT);
    expect(fs.readdirSync(layoutRoot).sort()).toEqual(["git", "hana-server.cmd", "hana.cmd", "sandbox", "server"]);
    expect(fs.readFileSync(path.join(layoutRoot, "server", "rust-service", "build.json"), "utf8"))
      .toBe(fs.readFileSync(path.join(inputs.rustDir, "build.json"), "utf8"));
    expect(fs.readFileSync(path.join(layoutRoot, "server", "lib", "runtime.json"), "utf8"))
      .toBe("server source must remain unchanged\n");
    expect(fs.readFileSync(path.join(layoutRoot, "git", "cmd", "git.exe"), "utf8")).toBe("git cmd");
    expect(fs.readFileSync(path.join(layoutRoot, "sandbox", "windows", "lingxi-win-sandbox.exe"), "utf8"))
      .toBe("sandbox helper");

    const wrappers = standaloneWrapperContents();
    expect(fs.readFileSync(path.join(layoutRoot, "hana.cmd"), "utf8")).toBe(wrappers.hana);
    expect(wrappers.hana).toContain('set "LINGXI_ROOT=%~dp0server"');
    expect(wrappers.hana).toContain('set "LINGXI_SERVER_ENTRY=%~dp0server\\bundle\\index.js"');
    expect(wrappers.hana).toContain(
      'set "LINGXI_WIN32_SANDBOX_HELPER=%~dp0sandbox\\windows\\lingxi-win-sandbox.exe"',
    );
    expect(wrappers.hana).toContain(
      'set "PATH=%~dp0git\\cmd;%~dp0git\\usr\\bin;%~dp0git\\mingw64\\bin;%PATH%"',
    );
    expect(wrappers.server).toContain('"%~dp0server\\hana-server.exe" "%~dp0server\\bootstrap.js" %*');

    await expect(
      verifyWindowsStandaloneArtifact({ rootDir: root, log: () => {} }),
    ).resolves.toMatchObject({ archivePath: result.archivePath });

    const cliFile = path.join(layoutRoot, "server", "bundle", "cli.js");
    const rustBinary = path.join(layoutRoot, "server", "rust-service", "lingxi-service.exe");
    fs.writeFileSync(path.join(extractDir, "Lingxi.exe"), "unrelated adjacent file");
    expect(resolveWindowsRustReader(cliFile, "x64")).toBe(rustBinary);
    const readerManifestPath = path.join(layoutRoot, "server", "rust-service", "build.json");
    const readerManifest = JSON.parse(fs.readFileSync(readerManifestPath, "utf8"));
    fs.writeFileSync(readerManifestPath, JSON.stringify({ ...readerManifest, appVersion: "9.9.9" }));
    expect(() => resolveWindowsRustReader(cliFile, "x64")).toThrow(/manifest does not match/);
    fs.writeFileSync(readerManifestPath, JSON.stringify(readerManifest));
    const changed = fs.readFileSync(rustBinary);
    changed[512] ^= 1;
    fs.writeFileSync(rustBinary, changed);
    expect(() => resolveWindowsRustReader(cliFile, "x64")).toThrow(/digest mismatch/);
  });

  it("fails closed when the packaged server is missing", async () => {
    const root = makeTempRoot();
    await expect(buildWindowsStandaloneArtifact({ rootDir: root, log: () => {} }))
      .rejects.toThrow(/packaged server directory is missing/);
  });

  it("解包复验拒绝与外层发行版本不同的 Rust reader", async () => {
    const root = makeTempRoot();
    createInputs(root);
    const output = await buildWindowsStandaloneArtifact({ rootDir: root, log: () => {} });
    const staging = path.join(root, "mutated-archive");
    await ustar.extract(output.archivePath, staging);
    const rustManifest = path.join(staging, STANDALONE_LAYOUT_ROOT, "server", "rust-service", "build.json");
    const original = JSON.parse(fs.readFileSync(rustManifest, "utf8"));
    fs.writeFileSync(rustManifest, JSON.stringify({ ...original, appVersion: "9.9.9" }));
    await ustar.packTree(staging, output.archivePath);
    const outer = JSON.parse(fs.readFileSync(output.manifestPath, "utf8"));
    outer.archive.sha256 = createHash("sha256").update(fs.readFileSync(output.archivePath)).digest("hex");
    outer.archive.size = fs.statSync(output.archivePath).size;
    fs.writeFileSync(output.manifestPath, JSON.stringify(outer));
    await expect(verifyWindowsStandaloneArtifact({ rootDir: root, log: () => {} }))
      .rejects.toThrow(/does not match/);
  });

  it("解包复验拒绝与外层发行版本不同的 Node 服务", async () => {
    const root = makeTempRoot();
    createInputs(root);
    const output = await buildWindowsStandaloneArtifact({ rootDir: root, log: () => {} });
    const staging = path.join(root, "mutated-server-archive");
    await ustar.extract(output.archivePath, staging);
    fs.writeFileSync(path.join(staging, STANDALONE_LAYOUT_ROOT, "server", "package.json"), '{"version":"1.2.2"}\n');
    await ustar.packTree(staging, output.archivePath);
    const outer = JSON.parse(fs.readFileSync(output.manifestPath, "utf8"));
    outer.archive.sha256 = createHash("sha256").update(fs.readFileSync(output.archivePath)).digest("hex");
    outer.archive.size = fs.statSync(output.archivePath).size;
    fs.writeFileSync(output.manifestPath, JSON.stringify(outer));
    await expect(verifyWindowsStandaloneArtifact({ rootDir: root, log: () => {} }))
      .rejects.toThrow(/packaged server version/);
  });

  it("fails closed when MinGit is incomplete", async () => {
    const root = makeTempRoot();
    const inputs = createInputs(root);
    fs.rmSync(path.join(inputs.gitDir, "usr", "bin", "sh.exe"));
    await expect(buildWindowsStandaloneArtifact({ rootDir: root, log: () => {} }))
      .rejects.toThrow(/MinGit runtime is incomplete/);
  });

  it("fails closed when the sandbox helper is missing", async () => {
    const root = makeTempRoot();
    const inputs = createInputs(root);
    fs.rmSync(inputs.helperPath);
    await expect(buildWindowsStandaloneArtifact({ rootDir: root, log: () => {} }))
      .rejects.toThrow(/Windows sandbox helper is missing/);
  });

  it("拒绝缺失或被改写的随包 Rust 程序", async () => {
    const root = makeTempRoot();
    const inputs = createInputs(root);
    const binary = path.join(inputs.rustDir, "lingxi-service.exe");
    fs.rmSync(binary);
    await expect(buildWindowsStandaloneArtifact({ rootDir: root, log: () => {} }))
      .rejects.toThrow(/ENOENT/);
    const bytes = Buffer.alloc(2048);
    fs.writeFileSync(binary, bytes);
    await expect(buildWindowsStandaloneArtifact({ rootDir: root, log: () => {} }))
      .rejects.toThrow(/matching Windows executable/);
  });

  it("拒绝把旧版本 Node 服务和新版本 Rust 清单拼成独立包", async () => {
    const root = makeTempRoot();
    const inputs = createInputs(root);
    fs.writeFileSync(path.join(inputs.serverDir, "package.json"), '{"version":"1.2.2"}\n');
    await expect(buildWindowsStandaloneArtifact({ rootDir: root, log: () => {} }))
      .rejects.toThrow(/packaged server version/);
  });

  it("激活版和回退版只读取同版本且受指针引用的 Rust 程序", () => {
    const root = makeTempRoot();
    const inputs = createInputs(root);
    const active = path.join(root, "artifacts", "server", "1.2.3-win32-x64");
    fs.cpSync(inputs.serverDir, active, { recursive: true });
    const receipt = { version: "1.2.3", sha256: "b".repeat(64) };
    fs.writeFileSync(path.join(active, ".verified"), JSON.stringify(receipt));
    const pointerDir = path.join(root, "artifacts", "pointers");
    fs.mkdirSync(pointerDir, { recursive: true });
    const pointer = { kind: "server", platformArch: "win32-x64", version: "1.2.3",
      versionDir: active, sha256: receipt.sha256 };
    const previous = path.join(pointerDir, "stable.previous.json");
    fs.writeFileSync(previous, JSON.stringify(pointer));
    const cliFile = path.join(active, "bundle", "cli.js");
    expect(resolveWindowsRustReader(cliFile, "x64"))
      .toBe(path.join(active, "rust-service", "lingxi-service.exe"));
    fs.writeFileSync(previous, JSON.stringify({ ...pointer, sha256: "c".repeat(64) }));
    expect(() => resolveWindowsRustReader(cliFile, "x64")).toThrow(/pointer mismatch/);
    fs.writeFileSync(previous, JSON.stringify(pointer));
    fs.writeFileSync(path.join(active, ".verified"), JSON.stringify({ ...receipt, version: "1.2.2" }));
    expect(() => resolveWindowsRustReader(cliFile, "x64")).toThrow(/receipt mismatch/);
    fs.writeFileSync(path.join(active, ".verified"), JSON.stringify(receipt));
    fs.writeFileSync(path.join(active, "package.json"), JSON.stringify({ version: "1.2.2" }));
    expect(() => resolveWindowsRustReader(cliFile, "x64")).toThrow(/package version mismatch/);
  });

  it("源码 CLI 拒绝旧 Rust stage，也不切到未经核验的 debug 程序", () => {
    const root = makeTempRoot();
    const inputs = createInputs(root);
    const stage = path.join(root, "dist-rust-service", "win-x64");
    fs.cpSync(inputs.rustDir, stage, { recursive: true });
    writeFile(root, "rust-toolchain.toml", '[toolchain]\nchannel = "1.98.1"\n');
    writeFile(root, "rust/Cargo.toml", '[workspace]\n');
    const manifestPath = path.join(stage, "build.json");
    const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
    fs.writeFileSync(manifestPath, JSON.stringify({ ...manifest,
      sourceSha256: rustSourceDigest(path.join(root, "rust")) }));
    const cliFile = path.join(root, "cli", "rust-service.ts");
    expect(resolveWindowsRustReader(cliFile, "x64"))
      .toBe(path.join(stage, "lingxi-service.exe"));
    writeFile(root, "rust/Cargo.toml", '[workspace]\n# newer source\n');
    expect(() => resolveWindowsRustReader(cliFile, "x64")).toThrow(/stale/);
    fs.rmSync(stage, { recursive: true });
    writeFile(root, "rust/target/debug/lingxi-service.exe", "unverified debug");
    expect(() => resolveWindowsRustReader(cliFile, "x64")).toThrow(/ENOENT/);
  });

  it("does not make standalone packaging depend on release signing credentials", async () => {
    const root = makeTempRoot();
    createInputs(root);
    const source = fs.readFileSync(
      path.resolve(import.meta.dirname, "..", "scripts", "build-standalone-server-artifact.mjs"),
      "utf8",
    );

    expect(source).not.toContain("LINGXI_SIGN_KEY");
    expect(source).not.toContain("artifact-sign.mjs");
    await expect(buildWindowsStandaloneArtifact({ rootDir: root, log: () => {} })).resolves.toMatchObject({
      manifest: { kind: "hana-core-standalone" },
    });
  });

  it("refuses the Electron seed directory and unsupported architectures", async () => {
    const root = makeTempRoot();
    createInputs(root);
    await expect(
      buildWindowsStandaloneArtifact({
        rootDir: root,
        artifactOutDir: path.join(root, "dist-server-artifact", "win32-x64"),
        log: () => {},
      }),
    ).rejects.toThrow(/must not enter dist-server-artifact/);
    await expect(
      buildWindowsStandaloneArtifact({
        rootDir: root,
        artifactOutDir: path.join(root, "dist-server"),
        log: () => {},
      }),
    ).rejects.toThrow(/dist-standalone/);
    await expect(buildWindowsStandaloneArtifact({ rootDir: root, arch: "arm64", log: () => {} }))
      .rejects.toThrow(/only x64 is published/);
  });

  it("removes a stale same-version release set before validating a failed rebuild", async () => {
    const root = makeTempRoot();
    const inputs = createInputs(root);
    const first = await buildWindowsStandaloneArtifact({ rootDir: root, log: () => {} });
    const legacySignaturePath = `${first.manifestPath}.sig`;
    fs.writeFileSync(legacySignaturePath, "obsolete standalone signature");
    fs.rmSync(inputs.helperPath);

    await expect(buildWindowsStandaloneArtifact({ rootDir: root, log: () => {} }))
      .rejects.toThrow(/Windows sandbox helper is missing/);
    expect(fs.existsSync(first.archivePath)).toBe(false);
    expect(fs.existsSync(first.manifestPath)).toBe(false);
    expect(fs.existsSync(legacySignaturePath)).toBe(false);
  });

  it("rejects an obsolete standalone signature sidecar", async () => {
    const root = makeTempRoot();
    createInputs(root);
    const result = await buildWindowsStandaloneArtifact({ rootDir: root, log: () => {} });
    fs.writeFileSync(`${result.manifestPath}.sig`, "obsolete standalone signature");

    await expect(verifyWindowsStandaloneArtifact({ rootDir: root, log: () => {} }))
      .rejects.toThrow(/obsolete standalone manifest signature must be removed/);
  });

  it("rejects manifest layout fields that do not describe the archive contract", async () => {
    const root = makeTempRoot();
    createInputs(root);
    const result = await buildWindowsStandaloneArtifact({ rootDir: root, log: () => {} });
    const original = JSON.parse(fs.readFileSync(result.manifestPath, "utf8"));
    const cases = [
      ["root", "WrongRoot"],
      ["server", `${STANDALONE_LAYOUT_ROOT}/wrong-server`],
      ["git", `${STANDALONE_LAYOUT_ROOT}/wrong-git`],
      ["sandboxHelper", `${STANDALONE_LAYOUT_ROOT}/sandbox/windows/wrong-helper.exe`],
    ];

    for (const [field, value] of cases) {
      const changed = structuredClone(original);
      changed.layout[field] = value;
      fs.writeFileSync(result.manifestPath, `${JSON.stringify(changed, null, 2)}\n`);
      await expect(verifyWindowsStandaloneArtifact({ rootDir: root, log: () => {} }))
        .rejects.toThrow(new RegExp(`manifest .*${field === "sandboxHelper" ? "sandbox helper" : field}.* mismatch`, "i"));
    }
  });

  it("builds a restricted-token smoke spec that proves write and deny-write through a native child", () => {
    const workDir = "C:\\Temp\\hana smoke";
    const runtimeEnvRoot = `${workDir}\\.ephemeral\\win32-sandbox-env`;
    const spec = standaloneRestrictedTokenSmokeSpec({
      layoutRoot: "C:\\downloads\\LingxiCore",
      workDir,
      lingxiHome: "C:\\Temp\\hana home",
      env: {
        SystemRoot: "C:\\Windows",
        PATH: "C:\\Program Files\\Git\\cmd;C:\\Windows\\System32",
        Path: "stale-duplicate",
        USERNAME: "runner",
        SystemDrive: "C:",
      },
    });

    expect(spec.helperPath).toBe("C:\\downloads\\LingxiCore\\sandbox\\windows\\lingxi-win-sandbox.exe");
    expect(spec.args).toEqual([
      "--cwd", workDir,
      "--writable-root", workDir,
      "--deny-write", `${workDir}\\blocked`,
      "--timeout-ms", "30000",
      "--",
      "C:\\Windows\\System32\\cmd.exe",
      "/d", "/s", "/c",
      expect.stringContaining("LINGXI_RESTRICTED_TOKEN_OK"),
    ]);
    expect(Object.keys(spec.env).filter((key) => key.toLowerCase() === "path")).toEqual(["Path"]);
    // Native cmd smoke must not put MinGit/MSYS ahead of System32: this step only
    // proves the helper + writable/deny-write contract, and Path must stay native.
    expect(spec.env.Path).toBe("C:\\Windows\\System32");
    expect(spec.env.Path).not.toMatch(/git|usr\\bin|mingw/i);
    // Match production win32-exec: TEMP/LOCALAPPDATA/APPDATA/HOME all live inside
    // the writable root the helper grants, not beside it under lingxiHome.
    expect(spec.env.TEMP).toBe(`${runtimeEnvRoot}\\Temp`);
    expect(spec.env.TMP).toBe(`${runtimeEnvRoot}\\Temp`);
    expect(spec.env.LOCALAPPDATA).toBe(`${runtimeEnvRoot}\\LocalAppData`);
    expect(spec.env.APPDATA).toBe(`${runtimeEnvRoot}\\AppData\\Roaming`);
    expect(spec.env.USERPROFILE).toBe(`${workDir}\\Profile`);
    expect(spec.env.HOME).toBe(`${workDir}\\Profile`);
    expect(spec.runtimeDirs).toEqual([
      `${runtimeEnvRoot}\\Temp`,
      `${runtimeEnvRoot}\\LocalAppData`,
      `${runtimeEnvRoot}\\AppData\\Roaming`,
      `${workDir}\\Profile`,
    ]);
    expect(spec.args.at(-1)).toContain("LINGXI_DENY_WRITE_OK");
    expect(spec.args.at(-1)).toContain("exit 73");
    expect(spec.deniedMarkerPath).toBe(`${workDir}\\blocked\\hana-deny-write-smoke.txt`);
    expect(spec.env).toMatchObject({
      SystemRoot: "C:\\Windows",
      USERNAME: "runner",
      SystemDrive: "C:",
      LINGXI_HOME: "C:\\Temp\\hana home",
      LINGXI_ROOT: "C:\\downloads\\LingxiCore\\server",
      LINGXI_SERVER_ENTRY: "C:\\downloads\\LingxiCore\\server\\bundle\\index.js",
      LINGXI_WIN32_SANDBOX_HELPER: spec.helperPath,
    });
  });

  it("runs the production exec_command chain through the extracted wrapper with a hermetic environment", () => {
    const spec = standaloneExecCommandSmokeSpec({
      layoutRoot: "C:\\downloads\\LingxiCore",
      workDir: "C:\\Temp\\hana smoke",
      lingxiHome: "C:\\Temp\\hana home",
      env: {
        SystemRoot: "C:\\Windows",
        PATH: "C:\\Program Files\\Git\\cmd;C:\\host-tools",
        NODE_OPTIONS: "--require C:\\host\\inject.cjs",
      },
    });

    expect(spec.command).toBe("C:\\Windows\\System32\\cmd.exe");
    expect(spec.args.join(" ")).toContain('call "C:\\downloads\\LingxiCore\\hana-server.cmd"');
    expect(spec.windowsVerbatimArguments).toBe(true);
    expect(spec.env.Path).not.toContain("Program Files\\Git");
    expect(spec.env.LINGXI_ROOT).toBe("Z:\\hana-poison\\server");
    expect(spec.env.LINGXI_STANDALONE_EXPECTED_ROOT).toBe("C:\\downloads\\LingxiCore\\server");
    expect(spec.env.LINGXI_STANDALONE_EXPECTED_HELPER)
      .toBe("C:\\downloads\\LingxiCore\\sandbox\\windows\\lingxi-win-sandbox.exe");
    expect(spec.env.LINGXI_INTERNAL_STANDALONE_RUNTIME_SMOKE).toBe("1");
    expect(spec.env).not.toHaveProperty("NODE_OPTIONS");
    expect(spec.env).not.toHaveProperty("LINGXI_STANDALONE_EXEC_MARKER");
  });

  it("rejects an archive whose bytes no longer match its manifest", async () => {
    const root = makeTempRoot();
    createInputs(root);
    const result = await buildWindowsStandaloneArtifact({ rootDir: root, log: () => {} });
    fs.appendFileSync(result.archivePath, "tampered");
    await expect(verifyWindowsStandaloneArtifact({ rootDir: root, log: () => {} }))
      .rejects.toThrow(/archive sha256 mismatch/);
  });
});
