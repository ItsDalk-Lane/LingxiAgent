/**
 * 缓存身份与授权测试（X18 / P10）：
 *  - 授权先于缓存（I01）：拒绝的请求返回 403 且缓存零读写、零失效（热缓存后拒绝仍 403）；
 *  - 跨 sessionId/path、studio/runtime 无数据、索引与分支状态污染（P10）：
 *    键含 runtime\0studio\0会话标识，双 studio 双 runtime 共享同一 cache 实例
 *    仍各自建槽、各自命中、输出互不串扰。
 */
import { describe, it, expect, afterEach } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { Hono } from "hono";

import { createSessionsRoute } from "../server/routes/sessions.ts";
import { HistoryDirectoryCache, historyDirectoryCacheKey } from "../server/history-read/cache.ts";
import { readSessionHistoryPage } from "../server/history-read/index.ts";
import { SessionManifestStore } from "../core/session-manifest/store.ts";
import { SessionManager } from "../lib/pi-sdk/index.ts";
import { buildLongRunFixtureBytes, messagesUrl } from "../scripts/lib/history-read-fixture.mjs";

const tmpDirs: string[] = [];

afterEach(() => {
  while (tmpDirs.length) {
    const dir = tmpDirs.pop();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

function makeTmpDir(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-auth-test-"));
  tmpDirs.push(dir);
  return dir;
}

function createHarness({ runtimeId = null, studioId = null }: { runtimeId?: string | null; studioId?: string | null } = {}) {
  const root = makeTmpDir();
  const agentsDir = path.join(root, "agents");
  const sessionPath = path.join(agentsDir, "hana", "sessions", "longrun.jsonl");
  fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
  fs.writeFileSync(sessionPath, buildLongRunFixtureBytes(8));
  const manifestStore = new SessionManifestStore({ dbPath: path.join(root, "manifest.db") });
  const manifest = manifestStore.createForPath({ sessionPath, ownerAgentId: "hana", domain: "desktop", kind: "chat" });
  const cache = new HistoryDirectoryCache();
  const engine: any = {
    agentsDir,
    currentSessionPath: sessionPath,
    isSessionStreaming: () => false,
    agentIdFromSessionPath: () => "hana",
    getAgent: () => ({ agentName: "Hana" }),
    historyReadCache: cache,
    deferredResults: null,
    subagentRuns: null,
    getRuntimeContext: runtimeId || studioId ? () => ({ runtimeId, studioId }) : undefined,
    getSessionManifest: (id: string) => (id === manifest.sessionId ? manifest : null),
    getSessionIdForPath: (p: string) => (p === sessionPath ? manifest.sessionId : null),
    getSessionBranchHead: (id: string) => manifestStore.getBranchHead(id) || null,
    _sessionManifestStore: manifestStore,
    openSessionManagerAtCurrentBranch: (p: string, d: string) => SessionManager.open(p, d),
  };
  const app = new Hono();
  app.route("/api", createSessionsRoute(engine));
  return { root, agentsDir, sessionPath, sessionId: manifest.sessionId, engine, app, cache };
}

describe("X18：授权先于缓存", () => {
  it("热缓存后拒绝授权仍 403，缓存零读写", async () => {
    const harness = createHarness(); // 无 runtimeContext → legacy 放行
    const url = messagesUrl(harness.sessionPath, { limit: 5 });
    const first = await harness.app.request(url);
    expect(first.status).toBe(200);
    const statsAfterWarm = harness.cache.stats();
    expect(statsAfterWarm.builds).toBeGreaterThanOrEqual(1); // 目录已热

    // 同一 cache 实例、拒绝授权的引擎：缓存热也不得绕过授权
    const deniedEngine = {
      ...harness.engine,
      historyReadCache: harness.cache,
      getRuntimeContext: () => ({ runtimeId: "rt-1", studioId: "st-1" }),
    };
    const deniedApp = new Hono();
    deniedApp.route("/api", createSessionsRoute(deniedEngine));
    const res = await deniedApp.request(messagesUrl(harness.sessionPath, { limit: 5 }));
    expect(res.status).toBe(403);
    expect(await res.json()).toMatchObject({ error: "insufficient_scope" });
    // 缓存零变化：拒绝的请求不读、不写、不失效任何槽位
    expect(harness.cache.stats()).toEqual(statsAfterWarm);
  });
});

describe("P10：跨会话/studio/runtime 隔离", () => {
  it("双 studio / 双 runtime 共享同一 cache：各自建槽、互不污染", async () => {
    const root = makeTmpDir();
    const agentsDir = path.join(root, "agents");
    const sessionPath = path.join(agentsDir, "hana", "sessions", "shared.jsonl");
    fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
    fs.writeFileSync(sessionPath, buildLongRunFixtureBytes(8));
    const manifestStore = new SessionManifestStore({ dbPath: path.join(root, "manifest.db") });
    const manifest = manifestStore.createForPath({ sessionPath, ownerAgentId: "hana", domain: "desktop", kind: "chat" });
    const cache = new HistoryDirectoryCache();

    const buildEngine = (runtimeId: string, studioId: string) => ({
      agentsDir,
      currentSessionPath: sessionPath,
      isSessionStreaming: () => false,
      agentIdFromSessionPath: () => "hana",
      getAgent: () => ({ agentName: "Hana" }),
      historyReadCache: cache,
      deferredResults: null,
      subagentRuns: null,
      getRuntimeContext: () => ({ runtimeId, studioId }),
      getSessionManifest: (id: string) => (id === manifest.sessionId ? manifest : null),
      getSessionIdForPath: (p: string) => (p === sessionPath ? manifest.sessionId : null),
      getSessionBranchHead: (id: string) => manifestStore.getBranchHead(id) || null,
      _sessionManifestStore: manifestStore,
      openSessionManagerAtCurrentBranch: (p: string, d: string) => SessionManager.open(p, d),
    });

    const studioA = buildEngine("rt-a", "st-a");
    const studioB = buildEngine("rt-b", "st-b");
    const input = (engine: any) => ({
      engine,
      cache,
      sessionPath,
      sessionId: manifest.sessionId,
      studioId: engine.getRuntimeContext().studioId,
      beforeId: null,
      limit: 5,
      forceAll: false,
      sanitizeVisibleContent: (value: string) => value,
    });

    const outA = await readSessionHistoryPage(input(studioA)) as any;
    const outB = await readSessionHistoryPage(input(studioB)) as any;
    expect(outA.mode).toBe("directory");
    expect(outB.mode).toBe("directory");
    // 键隔离：双 studio 各建一槽（同一物理会话）
    expect(cache.stats().sessions).toBe(2);
    expect(outA.result.messages).toEqual(outB.result.messages);
    expect(cache.stats().sessions).toBe(2); // 双槽并存，A 侧失效后重建不波及 B 侧

    // 变异 A 侧目录（输出基线变化）不影响 B 侧命中
    const keyA = historyDirectoryCacheKey({ runtimeId: "rt-a", studioId: "st-a", sessionId: manifest.sessionId, sessionPath });
    cache.invalidate(keyA, "file_identity_changed");
    const outA2 = await readSessionHistoryPage(input(studioA)) as any;
    expect(outA2.mode).toBe("directory");
    expect(outA2.result.messages).toEqual(outA.result.messages);
    const outB2 = await readSessionHistoryPage(input(studioB)) as any;
    expect(outB2.result.messages).toEqual(outB.result.messages);
  });

  it("跨 sessionId/path：不同会话各自目录与输出，无串扰", async () => {
    const root = makeTmpDir();
    const agentsDir = path.join(root, "agents");
    const pathA = path.join(agentsDir, "hana", "sessions", "a.jsonl");
    const pathB = path.join(agentsDir, "hana", "sessions", "b.jsonl");
    fs.mkdirSync(path.dirname(pathA), { recursive: true });
    fs.writeFileSync(pathA, buildLongRunFixtureBytes(6));
    fs.writeFileSync(pathB, buildLongRunFixtureBytes(20));
    const manifestStore = new SessionManifestStore({ dbPath: path.join(root, "manifest.db") });
    const manifestA = manifestStore.createForPath({ sessionPath: pathA, ownerAgentId: "hana", domain: "desktop", kind: "chat" });
    const manifestB = manifestStore.createForPath({ sessionPath: pathB, ownerAgentId: "hana", domain: "desktop", kind: "chat" });
    const cache = new HistoryDirectoryCache();
    const byPath = new Map([[pathA, manifestA.sessionId], [pathB, manifestB.sessionId]]);
    const byId = new Map([[manifestA.sessionId, manifestA], [manifestB.sessionId, manifestB]]);
    const engine: any = {
      agentsDir,
      currentSessionPath: pathA,
      isSessionStreaming: () => false,
      getAgent: () => ({ agentName: "Hana" }),
      historyReadCache: cache,
      deferredResults: null,
      subagentRuns: null,
      getSessionManifest: (id: string) => byId.get(id) ?? null,
      getSessionIdForPath: (p: string) => byPath.get(p) ?? null,
      getSessionBranchHead: (id: string) => manifestStore.getBranchHead(id) || null,
      _sessionManifestStore: manifestStore,
      openSessionManagerAtCurrentBranch: (p: string, d: string) => SessionManager.open(p, d),
    };
    const outcomeA = await readSessionHistoryPage({
      engine, cache, sessionPath: pathA, sessionId: manifestA.sessionId,
      beforeId: null, limit: 5, forceAll: false, sanitizeVisibleContent: (v) => v,
    });
    const outcomeB = await readSessionHistoryPage({
      engine, cache, sessionPath: pathB, sessionId: manifestB.sessionId,
      beforeId: null, limit: 5, forceAll: false, sanitizeVisibleContent: (v) => v,
    });
    expect(outcomeA.mode).toBe("directory");
    expect(outcomeB.mode).toBe("directory");
    const idsA = (outcomeA as any).result.messages.map((m: any) => m.entryId);
    const idsB = (outcomeB as any).result.messages.map((m: any) => m.entryId);
    // A 夹具（6 assistant）与 B 夹具（20 assistant）各自正确，无串页
    expect(idsA).not.toEqual(idsB);
    expect((outcomeA as any).result.messages.every((m: any) => Number(m.id) >= 2)).toBe(true);
    expect((outcomeB as any).result.messages.every((m: any) => Number(m.id) >= 16)).toBe(true);
    expect(cache.stats().sessions).toBe(2);
  });
});
