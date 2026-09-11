/**
 * B06 路由接线与快照/回退测试（X05 / P06 / P11）：
 *  - stat 失败（文件缺失）→ 不读不写缓存，legacy 全量承接（P06/I08）；
 *  - 目录读取失败（受控 readFile 注入 short_read）→ 尝试 1 → invalidate+重建
 *    （尝试 2）→ legacy 全量，重建次数恰为 1，输出与全量逐字段一致（受控调度不 sleep）；
 *  - 真实故障 → mode:"error" → 路由既有 500 路径（不返回旧页、不伪造空历史，P11）；
 *  - reason 11 项枚举齐全；
 *  - rebroadcast 恰一次：首屏成功后一次、翻页零次（B07）。
 */
import { describe, it, expect, afterEach, vi } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { Hono } from "hono";

import { readSessionHistoryPage, projectFullHistoryPage } from "../server/history-read/index.ts";
import { HistoryDirectoryCache } from "../server/history-read/cache.ts";
import { INVALIDATION_REASONS } from "../server/history-read/types.ts";
import { createSessionsRoute } from "../server/routes/sessions.ts";
import { SessionManifestStore } from "../core/session-manifest/store.ts";
import { SessionManager } from "../lib/pi-sdk/index.ts";
import { buildLongRunFixtureBytes, messagesUrl } from "../scripts/lib/history-read-fixture.mjs";

const tmpDirs: string[] = [];

afterEach(() => {
  while (tmpDirs.length) {
    const dir = tmpDirs.pop();
    fs.rmSync(dir, { recursive: true, force: true, maxRetries: 20, retryDelay: 250 });
  }
});

function makeTmpDir(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-fallback-test-"));
  tmpDirs.push(dir);
  return dir;
}

function createHarness({ readFile = undefined as HistoryReadHookLike | undefined, cache = new HistoryDirectoryCache(), activityHub = null as any } = {}) {
  void readFile;
  const root = makeTmpDir();
  const agentsDir = path.join(root, "agents");
  const sessionPath = path.join(agentsDir, "hana", "sessions", "longrun.jsonl");
  fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
  fs.writeFileSync(sessionPath, buildLongRunFixtureBytes(10));
  const manifestStore = new SessionManifestStore({ dbPath: path.join(root, "manifest.db") });
  const manifest = manifestStore.createForPath({ sessionPath, ownerAgentId: "hana", domain: "desktop", kind: "chat" });
  const engine: any = {
    agentsDir,
    currentSessionPath: sessionPath,
    isSessionStreaming: () => false,
    agentIdFromSessionPath: () => "hana",
    getAgent: () => ({ agentName: "Hana" }),
    historyReadCache: cache,
    activityHub,
    deferredResults: null,
    subagentRuns: null,
    getSessionManifest: (id: string) => (id === manifest.sessionId ? manifest : null),
    getSessionIdForPath: (p: string) => (p === sessionPath ? manifest.sessionId : null),
    getSessionBranchHead: (id: string) => manifestStore.getBranchHead(id) || null,
    _sessionManifestStore: manifestStore,
    openSessionManagerAtCurrentBranch: (p: string, d: string) => {
      const manager = SessionManager.open(p, d);
      return manager;
    },
  };
  return { root, agentsDir, sessionPath, sessionId: manifest.sessionId, manifestStore, engine, cache };
}

type HistoryReadHookLike = (buffer: Buffer, offset: number, length: number, position: number) => Promise<number>;

function baseInput(engine: any, cache: HistoryDirectoryCache, sessionPath: string, overrides: Record<string, any> = {}) {
  return {
    engine,
    cache,
    sessionPath,
    sessionId: engine.getSessionIdForPath(sessionPath),
    beforeId: null as number | null,
    limit: 50,
    forceAll: false,
    sanitizeVisibleContent: (value: string) => value,
    ...overrides,
  };
}

describe("快照/回退链（X05/P06/P11）", () => {
  it("reason 枚举 11 项齐全且不重复", () => {
    expect(INVALIDATION_REASONS).toHaveLength(11);
    expect(new Set(INVALIDATION_REASONS).size).toBe(11);
    for (const reason of ["revision_unknown", "file_identity_changed", "branch_changed", "locator_changed", "untrusted_mutation", "directory_invalid", "short_read", "tail_incomplete", "budget_exceeded", "legacy_fallback", "snapshot_changed"]) {
      expect(INVALIDATION_REASONS).toContain(reason);
    }
  });

  it("P06：stat 失败（文件缺失）→ revision_unknown → legacy 全量承接，缓存零读写", async () => {
    const { engine, cache } = createHarness();
    const missing = path.join(makeTmpDir(), "missing.jsonl");
    const outcome = await readSessionHistoryPage(baseInput(engine, cache, missing));
    expect(outcome.mode).toBe("full");
    expect((outcome as any).fallbackReason).toBe("revision_unknown");
    expect(cache.stats().builds).toBe(0);
    expect(cache.stats().sessions).toBe(0);
  });

  it("X05：目录读取 short_read → 尝试 1 + 重建 1 次（builds=2）→ legacy 全量，输出与全量等价", async () => {
    const { engine, cache, sessionPath } = createHarness();
    const raw = fs.readFileSync(sessionPath);
    // 受控计数 hook：仅第 1 次读取（尝试 1 的整文件扫描）真实供数，其后恒 EOF——
    // 尝试 1 定点读取 short_read → 重建；尝试 2 的扫描也 EOF → 目录拒绝 → legacy。
    let readCalls = 0;
    const failAfterFirst = async (buffer: Buffer, offset: number, length: number, position: number) => {
      readCalls += 1;
      if (readCalls === 1 && position < raw.length) {
        return raw.subarray(position, Math.min(position + length, raw.length)).copy(buffer, offset);
      }
      return 0;
    };
    const outcome = await readSessionHistoryPage(baseInput(engine, cache, sessionPath, { readFile: failAfterFirst }));
    expect(outcome.mode).toBe("full");
    expect((outcome as any).fallbackReason).toBe("short_read");
    expect((outcome as any).rebuilds).toBe(1);
    expect(cache.stats().builds).toBe(2); // 尝试 1 + 重建 1 次
    expect(cache.stats().sessions).toBe(0); // 失败后不驻留

    // 输出与无注入的全量逐字段一致（legacy 承接不降级语义）
    const baseline = await readSessionHistoryPage(baseInput(engine, cache, sessionPath, { disableCache: true }));
    expect(baseline.mode).toBe("full");
    expect(JSON.stringify((outcome as any).result)).toBe(JSON.stringify((baseline as any).result));
  });

  it("P11：hydrate 真实故障 → mode:error → 路由既有 500 路径（不伪造空历史）", async () => {
    const { engine, cache, sessionPath } = createHarness();
    engine.listSessionFiles = () => {
      throw new Error("registry disk failure");
    };
    engine._sessionFiles = undefined;
    const outcome = await readSessionHistoryPage(baseInput(engine, cache, sessionPath, { disableCache: true }));
    expect(outcome.mode).toBe("error");
    expect((outcome as any).error.message).toBe("registry disk failure");

    // 路由层：既有 500 catch 承接
    const { app } = await buildRoute(engine);
    const res = await app.request(messagesUrl(sessionPath, { limit: 5 }));
    expect(res.status).toBe(500);
    expect((await res.json()).error).toBe("registry disk failure");
  });

  async function buildRoute(engine: any) {
    const app = new Hono();
    app.route("/api", createSessionsRoute(engine));
    return { app };
  }

  it("rebroadcast 恰一次：首屏一次、翻页零次、失败不广播", async () => {
    const rebroadcastSession = vi.fn();
    const { engine, sessionPath } = createHarness({ activityHub: { rebroadcastSession } });
    const { app } = await buildRoute(engine);
    expect(await app.request(messagesUrl(sessionPath, { limit: 5 }))).toBeTruthy();
    expect(rebroadcastSession).toHaveBeenCalledTimes(1);
    // 翻页：不广播
    await app.request(messagesUrl(sessionPath, { before: 5, limit: 5 }));
    expect(rebroadcastSession).toHaveBeenCalledTimes(1);
    // 第二次首屏：再恰一次
    await app.request(messagesUrl(sessionPath, { limit: 5 }));
    expect(rebroadcastSession).toHaveBeenCalledTimes(2);
    // 故障请求：不广播
    engine.listSessionFiles = () => {
      throw new Error("boom");
    };
    engine._sessionFiles = undefined;
    await app.request(messagesUrl(sessionPath, { limit: 5 }));
    expect(rebroadcastSession).toHaveBeenCalledTimes(2);
  });
});

describe("projectFullHistoryPage 语义（reconciliation 共用入口）", () => {
  it("before/limit 边界与 all=1 同源", async () => {
    const { engine, sessionPath } = createHarness();
    const sourceMessages = await loadMessages(engine, sessionPath);
    const page = await projectFullHistoryPage(engine, {
      sessionPath, sourceMessages, beforeId: 3, limit: 2, forceAll: false, sanitizeVisibleContent: (v) => v,
    });
    expect(page.messages.map((m: any) => m.id)).toEqual(["1", "2"]);
    expect(page.hasMore).toBe(true);
    expect(page.nextBefore).toBe("1");
    const all = await projectFullHistoryPage(engine, {
      sessionPath, sourceMessages, beforeId: null, limit: 50, forceAll: true, sanitizeVisibleContent: (v) => v,
    });
    expect(all.messages.length).toBe(11);
    expect(all.hasMore).toBe(false);
    expect(all.nextBefore).toBeNull();
  });

  async function loadMessages(engine: any, sessionPath: string) {
    const msgUtils = await import("../core/message-utils.ts");
    return await msgUtils.loadSessionHistoryMessages(engine, sessionPath);
  }
});
