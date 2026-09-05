/**
 * 工作台处置 + 孤儿清扫路由（v0.1.34 需求：移除工作台时对话二选一处置；
 * 绕过移除流程的孤儿会话静默自动归档）。
 *
 * 复用 sessions-archived-route.test.ts 的 harness 形态（真实 tmp 目录 + engine mock，
 * 文件搬移走真实 fs）。
 */
import { Hono } from "hono";
import { beforeEach, describe, expect, it, vi } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import { RcStateStore } from "../core/slash-commands/rc-state.ts";

const browserMock = vi.hoisted(() => ({
  isRunning: vi.fn(() => false),
  currentUrl: vi.fn(() => null),
  suspendForSession: vi.fn(),
  resumeForSession: vi.fn(),
  closeBrowserForSession: vi.fn(),
  getBrowserSessions: vi.fn(() => ({})),
  getBrowserSessionStates: vi.fn(() => ({})),
  get hasAnyRunning() { return false; },
}));

vi.mock("../lib/browser/browser-manager.js", () => ({
  BrowserManager: {
    instance: () => browserMock,
  },
}));

vi.mock("../core/message-utils.js", async () => {
  const actual = await vi.importActual("../core/message-utils.js");
  return {
    ...actual,
    extractTextContent: vi.fn(() => ({ text: "", images: [], thinking: "", toolUses: [] })),
    loadSessionHistoryMessages: vi.fn(async () => []),
  };
});

function makeEngine(tmpDir, overrides: any = {}) {
  const engine: any = {
    agentsDir: path.join(tmpDir, "agents"),
    lingxiHome: path.join(tmpDir, "home"),
    defaultDeskCwd: null,
    closeSession: vi.fn(async () => {}),
    setSessionPinned: vi.fn(async () => null),
    agentIdFromSessionPath: (p) => {
      const rel = path.relative(path.join(tmpDir, "agents"), p);
      return rel.split(path.sep)[0] || null;
    },
    getSessionIdForPath: vi.fn(() => null),
    getSessionManifest: vi.fn(() => null),
    getAgent: () => ({ agentName: "Hana" }),
    clearSessionTitle: vi.fn(async () => {}),
    deleteSessionInputDrafts: vi.fn(),
    listArchivedSessions: vi.fn(async () => []),
    listSessions: vi.fn(async () => []),
    moveSessionLifecycle: vi.fn(async ({ toPath, lifecycle }) => ({
      sessionId: "sess_disposal",
      lifecycle,
      currentLocator: { path: path.resolve(toPath) },
    })),
    emitEvent: vi.fn(),
    rcState: new RcStateStore(),
    discardSessionRuntime: vi.fn(async () => false),
    resolveSessionOwnership: vi.fn(() => ({ agentId: "a", source: "path", agentDeleted: false })),
    isSessionStreaming: vi.fn(() => false),
    switchSession: vi.fn(async () => {}),
    getSessionByPath: vi.fn(() => ({ messages: [] })),
    currentSessionPath: null,
    currentAgentId: "a",
    activeSessionModel: null,
    currentModel: null,
    planMode: false,
    permissionMode: "operate",
    accessMode: "operate",
    getSessionWorkspaceFolders: vi.fn(() => []),
    getSessionThinkingLevel: vi.fn(() => "medium"),
  };
  Object.assign(engine, overrides);
  return engine;
}

function writeSession(tmpDir, name, content = "{}\n"): string {
  const sessDir = path.join(tmpDir, "agents", "a", "sessions");
  fs.mkdirSync(sessDir, { recursive: true });
  const sess = path.join(sessDir, name);
  fs.writeFileSync(sess, content);
  return sess;
}

describe("POST /api/sessions/workspace-disposal", () => {
  let tmpDir, engine, app, workspaceDir;

  beforeEach(async () => {
    vi.resetModules();
    vi.clearAllMocks();
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-ws-disposal-"));
    workspaceDir = path.join(tmpDir, "ws-main");
    fs.mkdirSync(workspaceDir, { recursive: true });
    engine = makeEngine(tmpDir);
    const { createSessionsRoute } = await import("../server/routes/sessions.ts");
    app = new Hono();
    app.route("/api", createSessionsRoute(engine));
  });

  it("rejects requests without a workspace identity", async () => {
    const res = await app.request("/api/sessions/workspace-disposal", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ action: "archive" }),
    });
    expect(res.status).toBe(400);
  });

  it("archives mount-form and legacy cwd-form sessions of the workspace (dual identity)", async () => {
    const mountSess = writeSession(tmpDir, "m1.jsonl");
    const cwdSess = writeSession(tmpDir, "c1.jsonl");
    const otherSess = writeSession(tmpDir, "o1.jsonl");
    engine.listSessions = vi.fn(async () => [
      { path: mountSess, sessionId: "s-m1", workspaceMountId: "mount_main", cwd: workspaceDir },
      { path: cwdSess, sessionId: "s-c1", workspaceMountId: null, cwd: workspaceDir },
      { path: otherSess, sessionId: "s-o1", workspaceMountId: "mount_other", cwd: path.join(tmpDir, "other") },
    ]);

    const res = await app.request("/api/sessions/workspace-disposal", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ cwd: workspaceDir, action: "archive" }),
    });
    const data = await res.json();

    expect(res.status).toBe(200);
    expect(data.matched).toBe(2);
    expect(data.disposed).toBe(2);
    expect(fs.existsSync(path.join(tmpDir, "agents", "a", "sessions", "archived", "m1.jsonl"))).toBe(true);
    expect(fs.existsSync(path.join(tmpDir, "agents", "a", "sessions", "archived", "c1.jsonl"))).toBe(true);
    // 其他工作台的会话不受影响
    expect(fs.existsSync(otherSess)).toBe(true);
  });

  it("delete action archives then permanently removes the session files", async () => {
    const sess = writeSession(tmpDir, "d1.jsonl");
    engine.listSessions = vi.fn(async () => [
      { path: sess, sessionId: "s-d1", workspaceMountId: null, cwd: workspaceDir },
    ]);

    const res = await app.request("/api/sessions/workspace-disposal", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ cwd: workspaceDir, action: "delete" }),
    });
    const data = await res.json();

    expect(res.status).toBe(200);
    expect(data.disposed).toBe(1);
    expect(fs.existsSync(sess)).toBe(false);
    expect(fs.existsSync(path.join(tmpDir, "agents", "a", "sessions", "archived", "d1.jsonl"))).toBe(false);
    expect(engine.clearSessionTitle).toHaveBeenCalled();
  });

  it("skips streaming sessions instead of archiving them", async () => {
    const sess = writeSession(tmpDir, "busy.jsonl");
    engine.listSessions = vi.fn(async () => [
      { path: sess, sessionId: "s-busy", workspaceMountId: null, cwd: workspaceDir },
    ]);
    engine.isSessionStreaming = vi.fn(() => true);

    const res = await app.request("/api/sessions/workspace-disposal", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ cwd: workspaceDir, action: "archive" }),
    });
    const data = await res.json();

    expect(res.status).toBe(200);
    expect(data.disposed).toBe(0);
    expect(data.skippedStreaming).toBe(1);
    expect(fs.existsSync(sess)).toBe(true);
  });

  it("resolves the default mount root for mount-form disposal identity", async () => {
    // default mount 的解析根 = engine.defaultDeskCwd；mountId 形态处置用它做 cwd 双形态匹配
    const mountSess = writeSession(tmpDir, "dm.jsonl");
    engine.defaultDeskCwd = workspaceDir;
    engine.listSessions = vi.fn(async () => [
      { path: mountSess, sessionId: "s-dm", workspaceMountId: "default", cwd: null },
    ]);

    const res = await app.request("/api/sessions/workspace-disposal", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ workspaceMountId: "default", action: "archive" }),
    });
    const data = await res.json();

    expect(res.status).toBe(200);
    expect(data.disposed).toBe(1);
    expect(fs.existsSync(path.join(tmpDir, "agents", "a", "sessions", "archived", "dm.jsonl"))).toBe(true);
  });
});

describe("POST /api/sessions/sweep-orphaned-workspaces", () => {
  let tmpDir, engine, app;

  beforeEach(async () => {
    vi.resetModules();
    vi.clearAllMocks();
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-ws-sweep-"));
    engine = makeEngine(tmpDir);
    const { createSessionsRoute } = await import("../server/routes/sessions.ts");
    app = new Hono();
    app.route("/api", createSessionsRoute(engine));
  });

  it("silently auto-archives sessions whose mount is gone or whose directory was deleted from disk", async () => {
    const existingDir = path.join(tmpDir, "still-here");
    fs.mkdirSync(existingDir, { recursive: true });
    const goneMountSess = writeSession(tmpDir, "gm.jsonl");
    const goneDirSess = writeSession(tmpDir, "gd.jsonl");
    const keepMountSess = writeSession(tmpDir, "km.jsonl");
    const keepDirSess = writeSession(tmpDir, "kd.jsonl");
    engine.listSessions = vi.fn(async () => [
      // mount 失效（local_fs_gone 不在任何活跃 mount 列表）→ 孤儿
      { path: goneMountSess, sessionId: "s-gm", workspaceMountId: "local_fs_gone", cwd: null },
      // cwd 目录已从磁盘删除 → 孤儿
      { path: goneDirSess, sessionId: "s-gd", workspaceMountId: null, cwd: path.join(tmpDir, "deleted-dir") },
      // default mount 永远存在 → 保留
      { path: keepMountSess, sessionId: "s-km", workspaceMountId: "default", cwd: null },
      // 目录仍在 → 保留
      { path: keepDirSess, sessionId: "s-kd", workspaceMountId: null, cwd: existingDir },
    ]);

    const res = await app.request("/api/sessions/sweep-orphaned-workspaces", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({}),
    });
    const data = await res.json();

    expect(res.status).toBe(200);
    expect(data.archived).toBe(2);
    expect(fs.existsSync(path.join(tmpDir, "agents", "a", "sessions", "archived", "gm.jsonl"))).toBe(true);
    expect(fs.existsSync(path.join(tmpDir, "agents", "a", "sessions", "archived", "gd.jsonl"))).toBe(true);
    expect(fs.existsSync(keepMountSess)).toBe(true);
    expect(fs.existsSync(keepDirSess)).toBe(true);
  });

  it("reports nothing to archive when every session resolves to a live workspace", async () => {
    const existingDir = path.join(tmpDir, "alive");
    fs.mkdirSync(existingDir, { recursive: true });
    const sess = writeSession(tmpDir, "ok.jsonl");
    engine.listSessions = vi.fn(async () => [
      { path: sess, sessionId: "s-ok", workspaceMountId: "default", cwd: existingDir },
    ]);

    const res = await app.request("/api/sessions/sweep-orphaned-workspaces", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({}),
    });
    const data = await res.json();

    expect(res.status).toBe(200);
    expect(data.archived).toBe(0);
    expect(fs.existsSync(sess)).toBe(true);
  });
});
