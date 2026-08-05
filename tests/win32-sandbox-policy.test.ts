import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, describe, expect, it } from "vitest";
import { deriveSandboxPolicy } from "../lib/sandbox/policy.ts";
import {
  buildWin32SandboxGrants,
  externalReadPathsFromSessionFiles,
} from "../lib/sandbox/win32-policy.ts";
import { canonicalFilesystemPathSync } from "../shared/link-aware-fs.ts";

describe("Windows sandbox policy projection", () => {
  let tempRoot;

  afterEach(() => {
    if (tempRoot) fs.rmSync(tempRoot, { recursive: true, force: true });
    tempRoot = null;
  });

  function makeTree() {
    tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "hana-win32-sandbox-"));
    const lingxiHome = path.join(tempRoot, "hana-home");
    const agentDir = path.join(lingxiHome, "agents", "hana");
    const workspace = path.join(tempRoot, "workspace");
    const externalDir = path.join(tempRoot, "external");
    for (const dir of [
      lingxiHome,
      agentDir,
      workspace,
      path.join(workspace, ".git"),
      externalDir,
      path.join(agentDir, "memory"),
      path.join(agentDir, "sessions"),
      path.join(lingxiHome, "user"),
      path.join(lingxiHome, "skills"),
      path.join(lingxiHome, "session-files"),
      path.join(lingxiHome, ".ephemeral"),
    ]) {
      fs.mkdirSync(dir, { recursive: true });
    }
    fs.writeFileSync(path.join(agentDir, "config.yaml"), "agent:\n  name: Hana\n");
    fs.writeFileSync(path.join(agentDir, "pinned.md"), "pinned");
    fs.writeFileSync(path.join(lingxiHome, "auth.json"), "{}");
    fs.writeFileSync(path.join(externalDir, "reference.md"), "outside");
    return { lingxiHome, agentDir, workspace, externalDir };
  }

  // 期望值必须与生产代码同一条规范化路径（native realpath 会展开 Windows 8.3
  // 短名，JS 版 fs.realpathSync 不会；CI runner 的 TEMP 恰好是短名形式）。
  const real = (p) => canonicalFilesystemPathSync(p);

  it("projects restricted-token write roots without external read grants", () => {
    const { lingxiHome, agentDir, workspace, externalDir } = makeTree();
    const externalFile = path.join(externalDir, "reference.md");
    const policy = deriveSandboxPolicy({
      agentDir,
      workspace,
      workspaceFolders: [],
      lingxiHome,
      mode: "standard",
    });

    const grants = buildWin32SandboxGrants({
      policy,
      cwd: workspace,
      externalReadPaths: [externalFile],
      systemReadRoots: [externalDir],
    } as any);

    expect(grants.writePaths).toEqual([real(workspace)]);
    expect(grants.optionalWritePaths).toEqual(expect.arrayContaining([
      real(path.join(agentDir, "memory")),
      real(path.join(agentDir, "sessions")),
    ]));
    expect(grants.readPaths).toEqual([]);
    expect(grants.optionalReadPaths).toEqual([]);
    expect(grants.denyReadPaths).toEqual([]);
    expect(grants.writePaths).not.toContain(real(externalFile));
    expect(grants.optionalWritePaths).toContain(real(path.join(lingxiHome, ".ephemeral")));
    expect(grants.denyWritePaths).not.toContain(real(path.join(workspace, ".git")));
    expect(grants.denyWritePaths).not.toContain(real(path.join(lingxiHome, "session-files")));
  });

  it("keeps the Windows write model functionality-first for Git worktrees", () => {
    const { lingxiHome, agentDir, workspace } = makeTree();
    const policy = deriveSandboxPolicy({
      agentDir,
      workspace,
      workspaceFolders: [],
      lingxiHome,
      mode: "standard",
    });

    const grants = buildWin32SandboxGrants({
      policy,
      cwd: workspace,
    });

    expect(grants.writePaths).toContain(real(workspace));
    expect(grants.optionalWritePaths).toContain(real(path.join(lingxiHome, ".ephemeral")));
    expect(grants.denyWritePaths).not.toContain(real(path.join(workspace, ".git")));
    expect(grants.denyReadPaths).toEqual([]);
  });

  it("does not turn a per-command working directory into a writable root", () => {
    const { lingxiHome, agentDir, workspace, externalDir } = makeTree();
    const policy = deriveSandboxPolicy({
      agentDir,
      workspace,
      workspaceFolders: [],
      lingxiHome,
      mode: "standard",
    });

    const grants = buildWin32SandboxGrants({
      policy,
      cwd: externalDir,
    });

    expect(grants.writePaths).toEqual([real(workspace)]);
    expect(grants.optionalWritePaths).not.toContain(real(externalDir));
    expect([
      ...grants.writePaths,
      ...grants.optionalWritePaths,
    ]).not.toContain(real(externalDir));
  });

  it("does not project ordinary system-readable roots into ACL work", () => {
    const { lingxiHome, agentDir, workspace, externalDir } = makeTree();
    const policy = deriveSandboxPolicy({
      agentDir,
      workspace,
      workspaceFolders: [],
      lingxiHome,
      mode: "standard",
    });

    const grants = buildWin32SandboxGrants({
      policy,
      cwd: workspace,
      systemReadRoots: [externalDir],
    } as any);

    expect(grants.readPaths).toEqual([]);
    expect(grants.optionalReadPaths).toEqual([]);
    expect(grants.writePaths).not.toContain(real(externalDir));
    expect(grants.optionalWritePaths).not.toContain(real(externalDir));
    expect(grants.denyReadPaths).toEqual([]);
  });

  it("keeps non-Git protected paths inside write roots as deny-write grants", () => {
    const { lingxiHome, agentDir, workspace } = makeTree();
    const protectedBuildCache = path.join(workspace, "protected-cache");
    fs.mkdirSync(protectedBuildCache, { recursive: true });
    const policy = deriveSandboxPolicy({
      agentDir,
      workspace,
      workspaceFolders: [],
      lingxiHome,
      mode: "standard",
    });
    policy.protectedPaths.push(protectedBuildCache);

    const grants = buildWin32SandboxGrants({
      policy,
      cwd: workspace,
    });

    expect(grants.writePaths).toContain(real(workspace));
    expect(grants.denyWritePaths).toContain(real(protectedBuildCache));
  });

  it("projects explicit runtime writable roots for language caches and bundled runtimes", () => {
    const { lingxiHome, agentDir, workspace } = makeTree();
    const runtimeRoot = path.join(lingxiHome, ".ephemeral", "runtime-cache");
    fs.mkdirSync(runtimeRoot, { recursive: true });
    const policy = deriveSandboxPolicy({
      agentDir,
      workspace,
      workspaceFolders: [],
      lingxiHome,
      runtimeWritablePaths: [runtimeRoot],
      mode: "standard",
    });

    const grants = buildWin32SandboxGrants({
      policy,
      cwd: workspace,
    });

    expect(grants.optionalWritePaths).toContain(real(runtimeRoot));
  });

  it("keeps read-only Hana prompt files out of Windows ACL projection", () => {
    const { lingxiHome, agentDir, workspace, externalDir } = makeTree();
    const externalFile = path.join(externalDir, "reference.md");
    const optionalPrompt = path.join(agentDir, "config.yaml");
    const missingLegacyPrompt = path.join(agentDir, "yuan.md");
    const policy = deriveSandboxPolicy({
      agentDir,
      workspace,
      workspaceFolders: [],
      lingxiHome,
      mode: "standard",
    });

    const grants = buildWin32SandboxGrants({
      policy,
      cwd: workspace,
      externalReadPaths: [externalFile],
    } as any);

    expect(grants.readPaths).toEqual([]);
    expect(grants.readPaths).not.toContain(real(optionalPrompt));
    expect(grants.readPaths).not.toContain(path.resolve(missingLegacyPrompt));
    expect(grants.optionalReadPaths).toEqual([]);
    expect(grants.optionalReadPaths).not.toContain(path.resolve(missingLegacyPrompt));
  });

  it("derives read-only external grants from active session files without re-granting workspace or managed-cache files", () => {
    const { lingxiHome, workspace, externalDir } = makeTree();
    const externalFile = path.join(externalDir, "reference.md");
    const workspaceFile = path.join(workspace, "owned.md");
    const managedFile = path.join(lingxiHome, "session-files", "cache", "image.png");
    fs.mkdirSync(path.dirname(managedFile), { recursive: true });
    fs.writeFileSync(workspaceFile, "workspace");
    fs.writeFileSync(managedFile, "cache");

    const grants = externalReadPathsFromSessionFiles([
      { filePath: externalFile, realPath: externalFile, storageKind: "external", status: "available" },
      { filePath: workspaceFile, realPath: workspaceFile, storageKind: "external", status: "available" },
      { filePath: managedFile, realPath: managedFile, storageKind: "managed_cache", status: "available" },
      { filePath: path.join(externalDir, "missing.md"), storageKind: "external", status: "missing" },
    ], {
      workspaceRoots: [workspace],
      lingxiHome,
    });

    expect(grants).toEqual([real(externalFile)]);
  });
});
