/**
 * E02.4 服务端条件 GET 验证（真实路由 /api/sessions/messages）：
 *  - 首次 200 带协议头/ETag/私有策略；
 *  - 同表示 If-None-Match → 304：无正文、无 Content-Length、带 ETag/私有策略；
 *    **每个 304 用同请求的无条件 200 作为 oracle 比较实际当前 JSON**（不只比 hash）；
 *  - 页参数不同 → 200（标签跨页/跨参数失配）；
 *  - 追加后 → 200 新内容；文件不变但外部状态（deferred 任务）变化 → 200；
 *  - stat 失败不签发（无 ETag/协议头）；路径安全拒绝 403 不泄露标签；
 *  - all=1 / reconciliation=1 不进入条件快路径（带条件头仍 200 完整响应）；
 *  - 弱比较（W/ 前缀忽略）、`*`、非法语法 → 200。
 */
import { describe, it, expect, afterEach } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { Hono } from "hono";

import { HistoryDirectoryCache } from "../server/history-read/cache.ts";
import { createSessionsRoute } from "../server/routes/sessions.ts";
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
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-protocol-test-"));
  tmpDirs.push(dir);
  return dir;
}

function jsonlLine(id: string, parentId: string | null, message: any) {
  return JSON.stringify({ type: "message", id, parentId, timestamp: "2026-09-10T10:00:00Z", message });
}

function createHarness() {
  const root = makeTmpDir();
  const agentsDir = path.join(root, "agents");
  const sessionPath = path.join(agentsDir, "hana", "sessions", "longrun.jsonl");
  fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
  fs.writeFileSync(sessionPath, buildLongRunFixtureBytes(10));
  const manifestStore = new SessionManifestStore({ dbPath: path.join(root, "manifest.db") });
  const manifest = manifestStore.createForPath({ sessionPath, ownerAgentId: "hana", domain: "desktop", kind: "chat" });
  const cache = new HistoryDirectoryCache();
  const deferredTasks: any[] = [];
  const engine: any = {
    agentsDir,
    currentSessionPath: sessionPath,
    isSessionStreaming: () => false,
    agentIdFromSessionPath: () => "hana",
    getAgent: () => ({ agentName: "Hana" }),
    historyReadCache: cache,
    activityHub: null,
    deferredResults: { listBySession: (p: string) => (p === sessionPath ? deferredTasks : []) },
    subagentRuns: null,
    getSessionManifest: (id: string) => (id === manifest.sessionId ? manifest : null),
    getSessionIdForPath: (p: string) => (p === sessionPath ? manifest.sessionId : null),
    getSessionBranchHead: (id: string) => manifestStore.getBranchHead(id) || null,
    _sessionManifestStore: manifestStore,
    openSessionManagerAtCurrentBranch: (p: string, d: string) => SessionManager.open(p, d),
    syncBranchHead: (leafId: string | null, observedTailLeafId: string | null, reason: string) =>
      manifestStore.setBranchHead(manifest.sessionId, { leafId, observedTailLeafId, reason }),
  };
  const app = new Hono();
  app.route("/api", createSessionsRoute(engine));
  return { root, agentsDir, sessionPath, sessionId: manifest.sessionId, manifestStore, engine, cache, app, deferredTasks };
}

function appendLines(sessionPath: string, lines: string[]) {
  const existing = fs.readFileSync(sessionPath);
  const needsSep = existing.length > 0 && existing[existing.length - 1] !== 0x0a;
  const payload = (needsSep ? "\n" : "") + lines.join("\n") + "\n";
  fs.appendFileSync(sessionPath, payload);
}

describe("E02.4 条件 GET：真实路由", () => {
  it("首次 200 带协议头与 ETag；同表示 → 304（oracle=无条件 200 的实际 JSON；无正文/无 Content-Length）", async () => {
    const h = createHarness();
    const url = messagesUrl(h.sessionPath, { limit: 50 });
    const first = await h.app.request(url);
    expect(first.status).toBe(200);
    expect(first.headers.get("lingxi-history-protocol")).toBe("1");
    expect(first.headers.get("lingxi-history-page-limit")).toBe("100"); // E06.2 选定 K=100
    expect(first.headers.get("cache-control")).toBe("private, no-store");
    const etag = first.headers.get("etag");
    expect(etag).toMatch(/^W\/"hrp1-[0-9a-f]{64}"$/);
    const firstBody = await first.text();
    expect(firstBody.length).toBeGreaterThan(0);

    // oracle：同请求的无条件 200（实际当前 JSON），304 的"未变"结论必须与它逐字节一致
    const oracle = await h.app.request(url);
    expect(oracle.status).toBe(200);
    const oracleBody = await oracle.text();
    expect(oracleBody).toBe(firstBody);

    const notMod = await h.app.request(url, { headers: { "if-none-match": etag! } });
    expect(notMod.status).toBe(304);
    expect(await notMod.text()).toBe(""); // 无正文（不是 {notModified:true} 也不是空 messages[]）
    expect(notMod.headers.get("content-length")).toBeNull(); // 默认不设置 304 Content-Length
    expect(notMod.headers.get("etag")).toBe(etag);
    expect(notMod.headers.get("cache-control")).toBe("private, no-store");
    expect(notMod.headers.get("lingxi-history-protocol")).toBe("1");
  });

  it("页参数不同 → 200（跨参数标签失配，ETag 不同）；翻页游标同理", async () => {
    const h = createHarness();
    const url50 = messagesUrl(h.sessionPath, { limit: 50 });
    const first = await h.app.request(url50);
    const etag50 = first.headers.get("etag")!;
    const other = await h.app.request(messagesUrl(h.sessionPath, { limit: 10 }));
    expect(other.status).toBe(200);
    expect(other.headers.get("etag")).not.toBe(etag50);
    const page2 = await h.app.request(messagesUrl(h.sessionPath, { before: 10, limit: 50 }), {
      headers: { "if-none-match": etag50 },
    });
    expect(page2.status).toBe(200); // 跨页标签不命中
  });

  it("追加后 → 200 新表示（旧标签失配）；oracle 与新内容一致", async () => {
    const h = createHarness();
    const url = messagesUrl(h.sessionPath, { limit: 50 });
    const first = await h.app.request(url);
    const etag = first.headers.get("etag")!;
    appendLines(h.sessionPath, [jsonlLine("a11", "a10", { role: "assistant", content: [{ type: "text", text: "协议追加" }] })]);
    h.engine.syncBranchHead("a11", "a11", "append_sync");
    const second = await h.app.request(url, { headers: { "if-none-match": etag } });
    expect(second.status).toBe(200);
    const secondBody = await second.text();
    expect(secondBody).not.toBe(await first.clone().text());
    expect(secondBody).toContain("协议追加");
    expect(second.headers.get("etag")).not.toBe(etag);
    // oracle：同请求无条件 200 与条件 200 一致
    const oracle = await h.app.request(url);
    expect(await oracle.text()).toBe(secondBody);
  });

  it("文件不变但外部状态变化（deferred 任务）→ 200（外部字段进入表示）", async () => {
    const h = createHarness();
    const url = messagesUrl(h.sessionPath, { limit: 50 });
    const first = await h.app.request(url);
    const firstBody = await first.text();
    const etag = first.headers.get("etag")!;
    h.deferredTasks.push({ taskId: "media-e2e-1", status: "resolved", meta: { type: "image-generation" }, result: { url: "s", format: "png" } });
    const second = await h.app.request(url, { headers: { "if-none-match": etag } });
    expect(second.status).toBe(200); // 外部状态改变 → 不允许 304
    expect(await second.text()).not.toBe(firstBody);
  });

  it("stat 失败（文件缺失）→ 不签发标签；路径安全拒绝 403 不泄露标签", async () => {
    const h = createHarness();
    fs.unlinkSync(h.sessionPath);
    const res = await h.app.request(messagesUrl(h.sessionPath, { limit: 50 }));
    expect(res.headers.get("etag")).toBeNull();
    expect(res.headers.get("lingxi-history-protocol")).toBeNull();
    const outside = path.join(makeTmpDir(), "outside.jsonl");
    fs.writeFileSync(outside, "{}\n");
    const denied = await h.app.request(messagesUrl(outside, { limit: 50 }));
    expect(denied.status).toBe(403);
    expect(denied.headers.get("etag")).toBeNull();
    expect(denied.headers.get("lingxi-history-protocol")).toBeNull();
  });

  it("all=1 / reconciliation=1 不进入条件快路径（带条件头仍 200 完整响应）", async () => {
    const h = createHarness();
    const url = messagesUrl(h.sessionPath, { limit: 50 });
    const first = await h.app.request(url);
    const etag = first.headers.get("etag")!;
    const all = await h.app.request(messagesUrl(h.sessionPath, { limit: 50, all: true }), {
      headers: { "if-none-match": etag },
    });
    expect(all.status).toBe(200);
    expect((await all.json()).messages.length).toBeGreaterThanOrEqual(11); // 全量而非 304
    const reconciling = await h.app.request(messagesUrl(h.sessionPath, { limit: 50 }) + "&reconciliation=1", {
      headers: { "if-none-match": etag },
    });
    expect(reconciling.status).toBe(200);
    expect(reconciling.headers.get("etag")).toBeNull(); // 严格证据模式不签发/不走快路径
  });

  it("弱比较（W/ 前缀忽略）与 `*`；非法语法忽略 → 200", async () => {
    const h = createHarness();
    const url = messagesUrl(h.sessionPath, { limit: 50 });
    const first = await h.app.request(url);
    const etag = first.headers.get("etag")!; // W/"hrp1-<hex>"
    const opaque = etag!.slice(2);
    const weak = await h.app.request(url, { headers: { "if-none-match": opaque } }); // 强形式同不透明值 → 弱比较命中
    expect(weak.status).toBe(304);
    const star = await h.app.request(url, { headers: { "if-none-match": "*" } });
    expect(star.status).toBe(304);
    const invalid = await h.app.request(url, { headers: { "if-none-match": 'abc, "unterminated' } });
    expect(invalid.status).toBe(200); // 无任何合法元素 → 忽略条件头
    const oracle = await h.app.request(url);
    expect(await oracle.text()).toBe(await invalid.clone().text());
  });
});

describe("E06.3：默认 limit 与声明同源", () => {
  it("省略 limit 的旧请求 → 服务端默认 50 且声明头一致；概览 recommendedLimit 同源", async () => {
    const h = createHarness();
    const res = await h.app.request(messagesUrl(h.sessionPath, {}));
    expect(res.status).toBe(200);
    expect(res.headers.get("lingxi-history-page-limit")).toBe("100"); // E06.2 选定 K=100
    const body = (await res.json()) as { messages: unknown[] };
    expect(body.messages.length).toBeLessThanOrEqual(50);
    const overview = await h.app.request("/api/sessions/history-overview?path=" + encodeURIComponent(h.sessionPath));
    const ovBody = (await overview.json()) as { pagination: { recommendedLimit: number; legacyDefaultLimit: number; maxLimit: number } };
    expect(ovBody.pagination.recommendedLimit).toBe(Number(res.headers.get("lingxi-history-page-limit")));
    expect(ovBody.pagination.legacyDefaultLimit).toBe(50);
    expect(ovBody.pagination.maxLimit).toBe(200);
  });
});
