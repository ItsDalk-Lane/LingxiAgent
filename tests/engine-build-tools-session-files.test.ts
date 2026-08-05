import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SessionFileRegistry } from "../lib/session-files/session-file-registry.ts";
// 期望值必须与生产代码同一条规范化路径（native realpath 会展开 Windows 8.3
// 短名，JS 版 fs.realpathSync 不会；CI runner 的 TEMP 恰好是短名形式）。
import { canonicalFilesystemPathSync } from "../shared/link-aware-fs.ts";

const createSandboxedTools = vi.fn(() => ({ tools: [], customTools: [] }));

vi.mock("../lib/sandbox/index.js", () => ({
  createSandboxedTools,
}));

const { LingxiEngine } = await import("../core/engine.ts");

describe("LingxiEngine.buildTools session external sandbox grants", () => {
  let tempRoot;

  afterEach(() => {
    if (tempRoot) fs.rmSync(tempRoot, { recursive: true, force: true });
    tempRoot = null;
    vi.clearAllMocks();
  });

  function writeSession(sessionPath, entries) {
    fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
    fs.writeFileSync(sessionPath, [
      JSON.stringify({
        type: "session",
        version: 3,
        id: path.basename(sessionPath, ".jsonl"),
        timestamp: "2026-07-19T00:00:00.000Z",
        cwd: path.dirname(sessionPath),
      }),
      ...entries.map((entry) => JSON.stringify(entry)),
      "",
    ].join("\n"));
  }

  it("projects and resolves SessionFiles from an unloaded session's active branch", () => {
    tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "hana-engine-unloaded-session-files-"));
    const sessionPath = path.join(tempRoot, "agents", "hana", "subagent-sessions", "child.jsonl");
    const filePath = path.join(tempRoot, "outside", "child-note.md");
    fs.mkdirSync(path.dirname(filePath), { recursive: true });
    fs.writeFileSync(filePath, "child note");

    const registry = new SessionFileRegistry({ now: () => 1234 });
    const sessionFile = registry.registerFile({
      sessionPath,
      filePath,
      origin: "stage_files",
      storageKind: "external",
    });
    writeSession(sessionPath, [{
      type: "message",
      id: "user-child",
      parentId: null,
      timestamp: "2026-07-19T00:00:01.000Z",
      message: {
        role: "user",
        content: `[SessionFile] ${JSON.stringify({ fileId: sessionFile.id })}`,
      },
    }]);

    const engine = Object.create(LingxiEngine.prototype);
    engine._sessionFiles = registry;
    engine.getSessionByPath = vi.fn(() => null);

    expect(engine.listActiveSessionFiles(sessionPath)).toEqual([sessionFile]);
    expect(engine.resolveActiveSessionFile({
      fileId: sessionFile.id,
      sessionPath,
    })).toEqual(sessionFile);
  });

  it("grants and resolves only active-branch SessionFiles while retaining the hidden sidecar superset", async () => {
    tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "hana-engine-sandbox-files-"));
    const hanakoHome = path.join(tempRoot, "hana-home");
    const agentDir = path.join(hanakoHome, "agents", "hana");
    const workspace = path.join(tempRoot, "workspace");
    const externalFile = path.join(tempRoot, "outside", "brief.md");
    const hiddenExternalFile = path.join(tempRoot, "outside", "abandoned.md");
    const workspaceFile = path.join(workspace, "owned.md");
    const managedFile = path.join(hanakoHome, "session-files", "cache", "shot.png");
    for (const file of [externalFile, hiddenExternalFile, workspaceFile, managedFile]) {
      fs.mkdirSync(path.dirname(file), { recursive: true });
      fs.writeFileSync(file, "x");
    }
    fs.mkdirSync(agentDir, { recursive: true });
    const sessionPath = path.join(agentDir, "sessions", "one.jsonl");

    const engine = Object.create(LingxiEngine.prototype);
    engine.hanakoHome = hanakoHome;
    engine.getAgent = vi.fn(() => ({ id: "hana", agentDir, tools: [] }));
    engine._pluginManager = null;
    engine._prefs = { getFileBackup: () => ({ enabled: false }) };
    engine._readPreferences = () => ({ sandbox: true });
    engine._confirmStore = null;
    engine._emitEvent = vi.fn();
    engine.getSessionPermissionMode = vi.fn(() => "operate");
    engine._agentMgr = { agent: { id: "hana", agentDir, tools: [] } };
    const activeFile = {
      id: "sf-active",
      sessionPath,
      filePath: externalFile,
      realPath: canonicalFilesystemPathSync(externalFile),
      storageKind: "external",
      status: "available",
    };
    const hiddenFile = {
      id: "sf-hidden",
      sessionPath,
      filePath: hiddenExternalFile,
      realPath: canonicalFilesystemPathSync(hiddenExternalFile),
      storageKind: "external",
      status: "available",
    };
    const activeProjection = [
      activeFile,
      { id: "sf-workspace", sessionPath, filePath: workspaceFile, realPath: canonicalFilesystemPathSync(workspaceFile), storageKind: "external", status: "available" },
      { id: "sf-managed", sessionPath, filePath: managedFile, realPath: canonicalFilesystemPathSync(managedFile), storageKind: "managed_cache", status: "available" },
    ];
    engine.listSessionFiles = vi.fn(() => [...activeProjection, hiddenFile]);
    engine.listActiveSessionFiles = vi.fn(() => activeProjection);
    engine.getSessionFile = vi.fn((fileId) => [...activeProjection, hiddenFile]
      .find((file) => file.id === fileId) || null);

    engine.buildTools(workspace, [], {
      agentDir,
      workspace,
      getSessionPath: () => sessionPath,
    });

    const sandboxOpts = (createSandboxedTools.mock.calls as any)[0][2];
    expect(sandboxOpts!.getExternalReadPaths()).toEqual([canonicalFilesystemPathSync(externalFile)]);
    expect(sandboxOpts!.resolveSessionFile("sf-active", { sessionPath })).toBe(activeFile);
    expect(sandboxOpts!.resolveSessionFile("sf-hidden", { sessionPath })).toBeNull();
    await expect(sandboxOpts!.resourceIO.read({
      kind: "session-file",
      fileId: "sf-hidden",
      sessionPath,
    })).rejects.toThrow("session file not found");
    expect(engine.listSessionFiles()).toContain(hiddenFile);
  });

  it("passes the sandbox network preference as a dynamic sandbox option", () => {
    const originalPlatform = process.platform;
    Object.defineProperty(process, "platform", { value: "linux", configurable: true });
    try {
      tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "hana-engine-sandbox-network-"));
      const hanakoHome = path.join(tempRoot, "hana-home");
      const agentDir = path.join(hanakoHome, "agents", "hana");
      const workspace = path.join(tempRoot, "workspace");
      fs.mkdirSync(agentDir, { recursive: true });
      fs.mkdirSync(workspace, { recursive: true });

      const engine = Object.create(LingxiEngine.prototype);
      engine.hanakoHome = hanakoHome;
      engine.getAgent = vi.fn(() => ({ id: "hana", agentDir, tools: [] }));
      engine._pluginManager = null;
      engine._prefs = { getFileBackup: () => ({ enabled: false }) };
      let prefs = { sandbox: true, sandbox_network: true };
      engine._readPreferences = () => prefs;
      engine._confirmStore = null;
      engine._emitEvent = vi.fn();
      engine.getSessionPermissionMode = vi.fn(() => "operate");
      engine._agentMgr = { agent: { id: "hana", agentDir, tools: [] } };
      engine.listSessionFiles = vi.fn(() => []);

      engine.buildTools(workspace, [], {
        agentDir,
        workspace,
        getSessionPath: () => path.join(agentDir, "sessions", "one.jsonl"),
      });

      const sandboxOpts = (createSandboxedTools.mock.calls as any)[0][2];
      expect(sandboxOpts!.getSandboxNetworkEnabled()).toBe(true);
      prefs = { sandbox: true, sandbox_network: false };
      expect(sandboxOpts!.getSandboxNetworkEnabled()).toBe(false);
    } finally {
      Object.defineProperty(process, "platform", { value: originalPlatform, configurable: true });
    }
  });

  it("keeps Windows restricted-token command sandbox networking enabled at the tool boundary", () => {
    tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "hana-engine-win32-sandbox-network-"));
    const originalPlatform = process.platform;
    Object.defineProperty(process, "platform", { value: "win32", configurable: true });

    try {
      const hanakoHome = path.join(tempRoot, "hana-home");
      const agentDir = path.join(hanakoHome, "agents", "hana");
      const workspace = path.join(tempRoot, "workspace");
      fs.mkdirSync(agentDir, { recursive: true });
      fs.mkdirSync(workspace, { recursive: true });

      const engine = Object.create(LingxiEngine.prototype);
      engine.hanakoHome = hanakoHome;
      engine.getAgent = vi.fn(() => ({ id: "hana", agentDir, tools: [] }));
      engine._pluginManager = null;
      engine._prefs = { getFileBackup: () => ({ enabled: false }) };
      engine._readPreferences = () => ({ sandbox: true, sandbox_network: false });
      engine._confirmStore = null;
      engine._emitEvent = vi.fn();
      engine.getSessionPermissionMode = vi.fn(() => "operate");
      engine._agentMgr = { agent: { id: "hana", agentDir, tools: [] } };
      engine.listSessionFiles = vi.fn(() => []);

      engine.buildTools(workspace, [], {
        agentDir,
        workspace,
        getSessionPath: () => path.join(agentDir, "sessions", "one.jsonl"),
      });

      const sandboxOpts = (createSandboxedTools.mock.calls as any)[0][2];
      expect(sandboxOpts!.getSandboxNetworkEnabled()).toBe(true);
    } finally {
      Object.defineProperty(process, "platform", { value: originalPlatform, configurable: true });
    }
  });

  it("rejects disabling sandbox networking through LingxiEngine on Windows", () => {
    const originalPlatform = process.platform;
    Object.defineProperty(process, "platform", { value: "win32", configurable: true });

    try {
      const engine = Object.create(LingxiEngine.prototype);
      engine._prefs = {
        getSandboxNetwork: vi.fn(() => false),
        setSandboxNetwork: vi.fn(),
      };

      expect(engine.getSandboxNetwork()).toBe(true);
      expect(() => engine.setSandboxNetwork(false)).toThrow("does not support network isolation");
      expect(engine._prefs.setSandboxNetwork).not.toHaveBeenCalled();
      engine.setSandboxNetwork(true);
      expect(engine._prefs.setSandboxNetwork).toHaveBeenCalledWith(true);
    } finally {
      Object.defineProperty(process, "platform", { value: originalPlatform, configurable: true });
    }
  });

  it("defaults sandbox networking to enabled when the preference has not been written yet", () => {
    tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "hana-engine-sandbox-network-default-"));
    const hanakoHome = path.join(tempRoot, "hana-home");
    const agentDir = path.join(hanakoHome, "agents", "hana");
    const workspace = path.join(tempRoot, "workspace");
    fs.mkdirSync(agentDir, { recursive: true });
    fs.mkdirSync(workspace, { recursive: true });

    const engine = Object.create(LingxiEngine.prototype);
    engine.hanakoHome = hanakoHome;
    engine.getAgent = vi.fn(() => ({ id: "hana", agentDir, tools: [] }));
    engine._pluginManager = null;
    engine._prefs = { getFileBackup: () => ({ enabled: false }) };
    engine._readPreferences = () => ({ sandbox: true });
    engine._confirmStore = null;
    engine._emitEvent = vi.fn();
    engine.getSessionPermissionMode = vi.fn(() => "operate");
    engine._agentMgr = { agent: { id: "hana", agentDir, tools: [] } };
    engine.listSessionFiles = vi.fn(() => []);

    engine.buildTools(workspace, [], {
      agentDir,
      workspace,
      getSessionPath: () => path.join(agentDir, "sessions", "one.jsonl"),
    });

    const sandboxOpts = (createSandboxedTools.mock.calls as any)[0][2];
    expect(sandboxOpts!.getSandboxNetworkEnabled()).toBe(true);
  });

  it("builds file tools through ResourceIO without a runtime switch", () => {
    tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "hana-engine-resource-io-tools-"));
    const hanakoHome = path.join(tempRoot, "hana-home");
    const agentDir = path.join(hanakoHome, "agents", "hana");
    const workspace = path.join(tempRoot, "workspace");
    fs.mkdirSync(agentDir, { recursive: true });
    fs.mkdirSync(workspace, { recursive: true });
    const sessionPath = path.join(agentDir, "sessions", "one.jsonl");

    const engine = Object.create(LingxiEngine.prototype);
    engine.hanakoHome = hanakoHome;
    engine.getAgent = vi.fn(() => ({ id: "hana", agentDir, tools: [] }));
    engine._pluginManager = null;
    engine._prefs = {
      getFileBackup: () => ({ enabled: false }),
    };
    engine._readPreferences = () => ({ sandbox: true });
    engine._confirmStore = null;
    engine._emitEvent = vi.fn();
    engine.getSessionPermissionMode = vi.fn(() => "operate");
    engine._agentMgr = { agent: { id: "hana", agentDir, tools: [] } };
    engine.listSessionFiles = vi.fn(() => []);
    engine.getSessionIdForPath = vi.fn(() => "session-one");
    engine.getSessionFile = vi.fn();
    engine.recordSessionFileOperation = vi.fn();
    engine.getVisionBridge = vi.fn(() => null);
    engine.isVisionAuxiliaryEnabled = vi.fn(() => false);

    engine.buildTools(workspace, [], {
      agentDir,
      workspace,
      getSessionPath: () => sessionPath,
    });

    const sandboxOpts = (createSandboxedTools.mock.calls as any)[0][2];
    expect(Object.prototype.hasOwnProperty.call(sandboxOpts!, "useResourceIoTools")).toBe(false);
    expect(sandboxOpts!.resourceIO).toBeTruthy();
  });

  it("resolves a forked legacy file id against the active child before its source locator", () => {
    tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "hana-engine-fork-file-resolve-"));
    const hanakoHome = path.join(tempRoot, "hana-home");
    const agentDir = path.join(hanakoHome, "agents", "hana");
    const workspace = path.join(tempRoot, "workspace");
    const childSessionPath = path.join(agentDir, "sessions", "child.jsonl");
    const sourceSessionPath = path.join(agentDir, "sessions", "source.jsonl");
    const sourceFilePath = path.join(hanakoHome, "session-files", "source", "brief.md");
    fs.mkdirSync(agentDir, { recursive: true });
    fs.mkdirSync(workspace, { recursive: true });

    const childFile = {
      id: "child-file",
      sessionPath: childSessionPath,
      filePath: path.join(hanakoHome, "session-files", "child", "brief.md"),
      legacyFileIds: ["source-file"],
      legacyFilePaths: [sourceFilePath],
    };
    const engine = Object.create(LingxiEngine.prototype);
    engine.hanakoHome = hanakoHome;
    engine.getAgent = vi.fn(() => ({ id: "hana", agentDir, tools: [] }));
    engine._pluginManager = null;
    engine._prefs = { getFileBackup: () => ({ enabled: false }) };
    engine._readPreferences = () => ({ sandbox: true });
    engine._confirmStore = null;
    engine._emitEvent = vi.fn();
    engine.getSessionPermissionMode = vi.fn(() => "operate");
    engine._agentMgr = { agent: { id: "hana", agentDir, tools: [] } };
    engine.listSessionFiles = vi.fn(() => []);
    engine.listActiveSessionFiles = vi.fn(() => [childFile]);
    engine.getSessionIdForPath = vi.fn(() => "sess-child");
    engine.getSessionFile = vi.fn((fileId, options) => (
      fileId === "source-file" && options?.sessionPath === childSessionPath ? childFile : null
    ));
    engine.getSessionFileByPath = vi.fn((filePath, options) => (
      filePath === sourceFilePath && options?.sessionPath === childSessionPath ? childFile : null
    ));
    engine.recordSessionFileOperation = vi.fn();
    engine.getVisionBridge = vi.fn(() => null);
    engine.isVisionAuxiliaryEnabled = vi.fn(() => false);

    engine.buildTools(workspace, [], {
      agentDir,
      workspace,
      getSessionPath: () => childSessionPath,
    });

    const sandboxOpts = (createSandboxedTools.mock.calls as any)[0][2];
    expect(sandboxOpts.resolveSessionFile("source-file", { sessionPath: sourceSessionPath })).toBe(childFile);
    expect(engine.resolveActiveSessionFile({
      filePath: sourceFilePath,
      sessionPath: childSessionPath,
    })).toBe(childFile);
    expect(engine.listActiveSessionFiles).toHaveBeenCalledWith(childSessionPath, []);
    expect(engine.getSessionFile).toHaveBeenCalledWith("source-file", { sessionId: null, sessionPath: childSessionPath });
  });

  it("includes inherited parent session files in read-only sandbox inputs", () => {
    tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "hana-engine-sandbox-parent-files-"));
    const hanakoHome = path.join(tempRoot, "hana-home");
    const agentDir = path.join(hanakoHome, "agents", "hana");
    const workspace = path.join(tempRoot, "workspace");
    const childExternal = path.join(tempRoot, "outside", "child.md");
    const parentExternal = path.join(tempRoot, "outside", "parent.md");
    const parentWorkspaceFile = path.join(workspace, "owned-by-workspace.md");
    for (const file of [childExternal, parentExternal, parentWorkspaceFile]) {
      fs.mkdirSync(path.dirname(file), { recursive: true });
      fs.writeFileSync(file, "x");
    }
    fs.mkdirSync(agentDir, { recursive: true });
    const childSessionPath = path.join(agentDir, "subagent-sessions", "child.jsonl");
    const parentSessionPath = path.join(agentDir, "sessions", "parent.jsonl");

    const engine = Object.create(LingxiEngine.prototype);
    engine.hanakoHome = hanakoHome;
    engine.getAgent = vi.fn(() => ({ id: "hana", agentDir, tools: [] }));
    engine._pluginManager = null;
    engine._prefs = { getFileBackup: () => ({ enabled: false }) };
    engine._readPreferences = () => ({ sandbox: true });
    engine._confirmStore = null;
    engine._emitEvent = vi.fn();
    engine.getSessionPermissionMode = vi.fn(() => "operate");
    engine._agentMgr = { agent: { id: "hana", agentDir, tools: [] } };
    engine.listActiveSessionFiles = vi.fn((sessionPath) => {
      if (sessionPath === childSessionPath) {
        return [
          { filePath: childExternal, realPath: canonicalFilesystemPathSync(childExternal), storageKind: "external", status: "available" },
        ];
      }
      if (sessionPath === parentSessionPath) {
        return [
          { filePath: parentExternal, realPath: canonicalFilesystemPathSync(parentExternal), storageKind: "external", status: "available" },
          { filePath: parentWorkspaceFile, realPath: canonicalFilesystemPathSync(parentWorkspaceFile), storageKind: "external", status: "available" },
        ];
      }
      return [];
    });

    engine.buildTools(workspace, [], {
      agentDir,
      workspace,
      getSessionPath: () => childSessionPath,
      fileReadSessionPaths: [parentSessionPath],
    });

    const sandboxOpts = (createSandboxedTools.mock.calls as any)[0][2];
    expect(sandboxOpts!.getExternalReadPaths()).toEqual([
      canonicalFilesystemPathSync(childExternal),
      canonicalFilesystemPathSync(parentExternal),
    ]);
  });

  it("resolves and grants an unloaded explicit parent session file", () => {
    tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "hana-engine-unloaded-parent-files-"));
    const hanakoHome = path.join(tempRoot, "hana-home");
    const agentDir = path.join(hanakoHome, "agents", "hana");
    const workspace = path.join(tempRoot, "workspace");
    const childSessionPath = path.join(agentDir, "subagent-sessions", "child.jsonl");
    const parentSessionPath = path.join(agentDir, "sessions", "parent.jsonl");
    const parentExternal = path.join(tempRoot, "outside", "parent.md");
    fs.mkdirSync(workspace, { recursive: true });
    fs.mkdirSync(path.dirname(parentExternal), { recursive: true });
    fs.writeFileSync(parentExternal, "parent note");

    const registry = new SessionFileRegistry({ now: () => 1234 });
    const parentFile = registry.registerFile({
      sessionPath: parentSessionPath,
      filePath: parentExternal,
      origin: "stage_files",
      storageKind: "external",
    });
    writeSession(parentSessionPath, [{
      type: "message",
      id: "parent-user",
      parentId: null,
      timestamp: "2026-07-19T00:00:01.000Z",
      message: {
        role: "user",
        content: `[SessionFile] ${JSON.stringify({ fileId: parentFile.id })}`,
      },
    }]);
    writeSession(childSessionPath, [{
      type: "message",
      id: "child-user",
      parentId: null,
      timestamp: "2026-07-19T00:00:02.000Z",
      message: { role: "user", content: "continue" },
    }]);

    const engine = Object.create(LingxiEngine.prototype);
    engine.hanakoHome = hanakoHome;
    engine._sessionFiles = registry;
    engine.getSessionByPath = vi.fn(() => null);
    engine.getAgent = vi.fn(() => ({ id: "hana", agentDir, tools: [] }));
    engine._pluginManager = null;
    engine._prefs = { getFileBackup: () => ({ enabled: false }) };
    engine._readPreferences = () => ({ sandbox: true });
    engine._confirmStore = null;
    engine._emitEvent = vi.fn();
    engine.getSessionPermissionMode = vi.fn(() => "operate");
    engine._agentMgr = { agent: { id: "hana", agentDir, tools: [] } };
    engine.getSessionIdForPath = vi.fn((sessionPath) => path.basename(sessionPath, ".jsonl"));
    engine.recordSessionFileOperation = vi.fn();
    engine.getVisionBridge = vi.fn(() => null);
    engine.isVisionAuxiliaryEnabled = vi.fn(() => false);

    engine.buildTools(workspace, [], {
      agentDir,
      workspace,
      getSessionPath: () => childSessionPath,
      fileReadSessionPaths: [parentSessionPath],
    });

    const sandboxOpts = (createSandboxedTools.mock.calls as any)[0][2];
    expect(sandboxOpts!.getExternalReadPaths()).toEqual([canonicalFilesystemPathSync(parentExternal)]);
    expect(sandboxOpts!.resolveSessionFile(parentFile.id, {
      sessionPath: parentSessionPath,
    })).toEqual(parentFile);
  });
});
