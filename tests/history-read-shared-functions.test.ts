/**
 * X20：共用函数回归——all/reconciliation/find 不被目录快路径改变语义。
 *  - all=1 与分页拼接等价（经真实路由，热/冷路径皆走新管线）；
 *  - reconciliation=1 的 complete/runRevision 不被目录触碰（严格证据等级 I12）；
 *  - find 序号契约与 messages 序号一致。
 */
import { describe, it, expect, afterEach } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { Hono } from "hono";

import { createSessionsRoute } from "../server/routes/sessions.ts";
import { HistoryDirectoryCache } from "../server/history-read/cache.ts";
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
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-shared-test-"));
  tmpDirs.push(dir);
  return dir;
}

function createHarness() {
  const root = makeTmpDir();
  const agentsDir = path.join(root, "agents");
  const sessionPath = path.join(agentsDir, "hana", "sessions", "longrun.jsonl");
  fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
  fs.writeFileSync(sessionPath, buildLongRunFixtureBytes(14));
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
    activityHub: { rebroadcastSession: () => {} },
    deferredResults: null,
    subagentRuns: null,
    getSessionManifest: (id: string) => (id === manifest.sessionId ? manifest : null),
    getSessionIdForPath: (p: string) => (p === sessionPath ? manifest.sessionId : null),
    getSessionBranchHead: (id: string) => manifestStore.getBranchHead(id) || null,
    _sessionManifestStore: manifestStore,
    openSessionManagerAtCurrentBranch: (p: string, d: string) => SessionManager.open(p, d),
  };
  const app = new Hono();
  app.route("/api", createSessionsRoute(engine));
  return { app, engine, cache, sessionPath, sessionId: manifest.sessionId };
}

describe("all=1 与分页拼接等价（X20）", () => {
  it("沿 nextBefore 翻至终点：反转摊平后与 all=1 逐条一致；blocks 覆盖一致", async () => {
    const harness = createHarness();
    const pages: any[][] = [];
    const walkedBlocks: any[] = [];
    let cursor: string | null = null;
    let guard = 0;
    for (;;) {
      guard += 1;
      expect(guard).toBeLessThan(10);
      const url = cursor == null
        ? messagesUrl(harness.sessionPath, { limit: 5 })
        : messagesUrl(harness.sessionPath, { before: Number(cursor), limit: 5 });
      const res = await harness.app.request(url);
      expect(res.status).toBe(200);
      const body = await res.json();
      pages.push(body.messages ?? []);
      walkedBlocks.push(...(body.blocks ?? []));
      if (!body.hasMore || body.nextBefore == null) break;
      cursor = body.nextBefore;
    }

    const allRes = await harness.app.request(messagesUrl(harness.sessionPath, { all: true }));
    const all = await allRes.json();
    const allMessages = all.messages ?? [];
    const walkedReversed = [...pages].reverse().flat();
    expect(walkedReversed.map((m: any) => m.id)).toEqual(allMessages.map((m: any) => m.id));
    expect(walkedReversed).toEqual(allMessages);

    // 块覆盖一致：分页块（重映射回原始 afterIndex）与 all=1 块集合一致
    const remapped = walkedBlocks.map((b: any) => b);
    expect(remapped.length).toBe((all.blocks ?? []).length);
    const sig = (b: any) => JSON.stringify([b.type, b.afterIndex, b.sourceIndex]);
    expect(new Set(remapped.map(sig))).toEqual(new Set((all.blocks ?? []).map(sig)));

    // 热命中后重复对比（目录路径与全量输出稳定一致）
    const again = await harness.app.request(messagesUrl(harness.sessionPath, { limit: 5 }));
    const againBody = await again.json();
    expect(againBody.messages).toEqual(pages[0]);
  });
});

describe("reconciliation 不进目录（I12/X20）", () => {
  it("reconciliation=1 返回严格证据字段，目录零构建零命中", async () => {
    const harness = createHarness();
    // 严格证据路径要求持久化 head 行存在（缺失 → complete=false，与旧语义一致）。
    // 这里按生产语义 seed 当前 head（物理尾 = a14）后再走 reconciliation。
    harness.engine._sessionManifestStore.setBranchHead(harness.sessionId, {
      leafId: "a14",
      observedTailLeafId: "a14",
      reason: "test_seed",
    });
    const res = await harness.app.request(
      `/api/sessions/messages?path=${encodeURIComponent(harness.sessionPath)}&reconciliation=1&limit=5`,
    );
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.reconciliation).toBeDefined();
    expect(body.reconciliation.sessionId).toBe(harness.sessionId);
    expect(body.reconciliation.complete).toBe(true);
    expect(["string", "number"]).toContain(typeof body.reconciliation.runRevision);
    expect(typeof body.reconciliation.snapshotId).toBe("string");
    expect(["running", "reconciled_idle", "unknown"]).toContain(body.reconciliation.runStatus);
    // 目录完全未被触碰
    const stats = harness.cache.stats();
    expect(stats.builds).toBe(0);
    expect(stats.sessions).toBe(0);
    expect(stats.hits).toBe(0);
  });
});

describe("find 序号契约（X20）", () => {
  it("find 命中序号与 messages 序号一致", async () => {
    const harness = createHarness();
    const messagesRes = await harness.app.request(messagesUrl(harness.sessionPath, { all: true }));
    const all = await messagesRes.json();
    const target = (all.messages ?? []).find((m: any) => m.role === "assistant" && (m.content || "").includes("最终报告"));
    expect(target).toBeDefined();

    const findRes = await harness.app.request(
      `/api/sessions/find?path=${encodeURIComponent(harness.sessionPath)}&q=${encodeURIComponent("最终报告")}`,
    );
    expect(findRes.status).toBe(200);
    const find = await findRes.json();
    expect(find.matches.length).toBeGreaterThan(0);
    const indexes = find.matches.map((m: any) => m.index);
    expect(indexes).toContain(Number(target.id));
  });
});
