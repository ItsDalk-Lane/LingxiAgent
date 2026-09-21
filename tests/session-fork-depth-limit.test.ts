import { Hono } from "hono";
import fs from "fs";
import os from "os";
import path from "path";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { retrySessionTurnMock, replayLatestUserTurnMock, resolveSessionNodeTargetMock } = vi.hoisted(() => ({
  retrySessionTurnMock: vi.fn(async () => ({ text: null, toolMedia: [] })),
  replayLatestUserTurnMock: vi.fn(async () => ({ text: null, toolMedia: [] })),
  resolveSessionNodeTargetMock: vi.fn((_branch: any, target: any) => ({
    turnInputEntry: { id: target?.turnInputEntryId || target?.entryId || null },
  })),
}));

vi.mock("../lib/browser/browser-manager.js", () => ({
  BrowserManager: {
    instance: () => ({
      isRunning: () => false,
      currentUrl: () => null,
      suspendForSession: vi.fn(),
      resumeForSession: vi.fn(),
      resumeForSessionIfAvailable: vi.fn(async () => ({ status: "skipped", canResume: false })),
      notifyViewerSession: vi.fn(),
      closeBrowserForSession: vi.fn(),
      getBrowserSessions: vi.fn(() => ({})),
      getBrowserSessionStates: vi.fn(() => ({})),
    }),
  },
}));

vi.mock("../core/message-utils.js", async (importOriginal) => ({
  ...(await importOriginal()),
  isValidSessionPath: vi.fn(() => true),
  isActiveSessionPath: vi.fn(() => true),
  isActiveDesktopSessionPath: vi.fn(() => true),
  isArchivedDesktopSessionPath: vi.fn(() => true),
}));

vi.mock("../core/session-turn-actions.js", () => ({
  replayLatestUserTurn: replayLatestUserTurnMock,
  retrySessionTurn: retrySessionTurnMock,
  resolveSessionNodeTarget: resolveSessionNodeTargetMock,
}));

// 谱系深度限制（session_fork_depth_limit）：fork 与 detached 建会话共用同一道闸。
// 链条：root(0 层) → child(1) → grand(2) → great(3，旧数据遗留形态)。
// 规则：来源在第 2 层（及以上）时再派生会被拒绝，且拒绝必须发生在建会话之前。
describe("sessions route fork depth limit", () => {
  let tmpDir: string;

  beforeEach(() => {
    vi.resetModules();
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-fork-depth-"));
  });

  // 返回 any：各用例按需注入 forkSessionAtNode / createDetachedSession 等桩，
  // 路由对 engine 的消费是鸭子类型，类型上不逐字段声明。
  function lineageEngine(overrides: Record<string, unknown> = {}): any {
    // fork 路由在建分支前会做 pathExists 落盘校验，测试会话文件须真实存在。
    const sessionDir = path.join(tmpDir, "agents", "a", "sessions");
    fs.mkdirSync(sessionDir, { recursive: true });
    const makePath = (name: string) => path.join(sessionDir, name);
    for (const name of ["root.jsonl", "child.jsonl", "grand.jsonl", "great.jsonl"]) {
      fs.writeFileSync(path.join(sessionDir, name), "");
    }
    const sessions = [
      { path: makePath("root.jsonl"), sessionId: "sess_root", forkedFrom: null },
      { path: makePath("child.jsonl"), sessionId: "sess_child", forkedFrom: { sessionId: "sess_root" } },
      { path: makePath("grand.jsonl"), sessionId: "sess_grand", forkedFrom: { sessionId: "sess_child" } },
      { path: makePath("great.jsonl"), sessionId: "sess_great", forkedFrom: { sessionId: "sess_grand" } },
    ];
    const byPath = new Map(sessions.map((s) => [s.path, s]));
    return {
      agentsDir: path.join(tmpDir, "agents"),
      cwd: "/tmp/workspace",
      memoryEnabled: true,
      planMode: false,
      memoryModelUnavailableReason: null,
      currentSessionPath: makePath("root.jsonl"),
      listSessions: vi.fn(async () => sessions),
      getSessionIdForPath: vi.fn((sessionPath: string) => byPath.get(sessionPath)?.sessionId || null),
      isSessionStreaming: vi.fn(() => false),
      getAgent: vi.fn(() => ({ agentName: "Hana" })),
      ...overrides,
    };
  }

  it("rejects forking a second-level conversation with session_fork_depth_limit before any fork work", async () => {
    const { createSessionsRoute } = await import("../server/routes/sessions.ts");
    const app = new Hono();
    const engine = lineageEngine({
      forkSessionAtNode: vi.fn(async () => ({ sessionPath: path.join(tmpDir, "agents/a/sessions/new.jsonl") })),
    });

    app.route("/api", createSessionsRoute(engine));
    const grandPath = path.join(tmpDir, "agents/a/sessions/grand.jsonl");
    const res = await app.request("/api/sessions/fork", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ path: grandPath }),
    });

    expect(res.status).toBe(409);
    expect(await res.json()).toMatchObject({ code: "session_fork_depth_limit" });
    expect(engine.forkSessionAtNode).not.toHaveBeenCalled();
  });

  it("still allows forking a first-level conversation", async () => {
    const { createSessionsRoute } = await import("../server/routes/sessions.ts");
    const app = new Hono();
    const engine = lineageEngine({
      forkSessionAtNode: vi.fn(async () => ({
        sessionPath: path.join(tmpDir, "agents/a/sessions/newfork.jsonl"),
        sessionId: "sess_newfork",
        permissionMode: "ask",
      })),
    });

    app.route("/api", createSessionsRoute(engine));
    const childPath = path.join(tmpDir, "agents/a/sessions/child.jsonl");
    const res = await app.request("/api/sessions/fork", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ path: childPath }),
    });

    expect(res.status).toBe(200);
    expect(await res.json()).toMatchObject({ ok: true, sessionId: "sess_newfork" });
    expect(engine.forkSessionAtNode).toHaveBeenCalledWith(
      expect.objectContaining({ sessionId: "sess_child" }),
    );
  });

  it("rejects a detached session whose lineage source sits at the depth limit, before creating it", async () => {
    const { createSessionsRoute } = await import("../server/routes/sessions.ts");
    const app = new Hono();
    const engine = lineageEngine({
      createDetachedSession: vi.fn(async () => ({
        sessionPath: "/tmp/agents/a/sessions/quick.jsonl",
        sessionId: "sess_quick",
        agentId: "a",
      })),
      persistSessionMeta: vi.fn(),
      setSessionForkedFrom: vi.fn(async () => ({ forkedFrom: { sessionId: "sess_grand" } })),
    });

    app.route("/api", createSessionsRoute(engine));
    const res = await app.request("/api/sessions/new-detached", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ cwd: tmpDir, forkedFromSessionId: "sess_grand" }),
    });

    expect(res.status).toBe(409);
    expect(await res.json()).toMatchObject({ code: "session_fork_depth_limit" });
    expect(engine.createDetachedSession).not.toHaveBeenCalled();
    expect(engine.setSessionForkedFrom).not.toHaveBeenCalled();
  });

  it("keeps detached lineage writes working for a first-level source", async () => {
    const { createSessionsRoute } = await import("../server/routes/sessions.ts");
    const app = new Hono();
    const engine = lineageEngine({
      createDetachedSession: vi.fn(async () => ({
        sessionPath: "/tmp/agents/a/sessions/quick.jsonl",
        sessionId: "sess_quick",
        agentId: "a",
      })),
      persistSessionMeta: vi.fn(),
      setSessionForkedFrom: vi.fn(async () => ({ forkedFrom: { sessionId: "sess_child" } })),
    });

    app.route("/api", createSessionsRoute(engine));
    const res = await app.request("/api/sessions/new-detached", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ cwd: tmpDir, forkedFromSessionId: "sess_child" }),
    });

    expect(res.status).toBe(200);
    expect(engine.createDetachedSession).toHaveBeenCalled();
    expect(engine.setSessionForkedFrom).toHaveBeenCalledWith(
      { sessionPath: "/tmp/agents/a/sessions/quick.jsonl" },
      { sessionId: "sess_child" },
    );
    expect(await res.json()).toMatchObject({
      ok: true,
      sessionId: "sess_quick",
      forkedFrom: { sessionId: "sess_child" },
    });
  });
});
