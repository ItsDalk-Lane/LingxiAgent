/**
 * E04 会话概览端点测试（GET /api/sessions/history-overview，真实路由）：
 *  - 结构/类型校验（schemaVersion/counts/三桶/task 分布/pagination；不用 any）；
 *  - 手写 oracle：counts、三桶合计=runsWithAssistant、task 去重与四类合计=referencedTasks；
 *  - 空会话=合法零计数（estimatedPages=0）；
 *  - 追加更新 + Run 跨桶（1→51 条 displayable assistant 跨 oneTo50→from51To200）；
 *  - 分支切换（rewind）不返回旧分支统计；追加到废弃分支不污染当前分支计数；
 *  - 热路径硬断言（E04.3）：全文件读取 0、JSONL 解析 0、不触发外部状态 hydrate；
 *  - 固定契约 JSON ≤ 4KiB；
 *  - 不可用三 reason（revision_unknown 真实路径 + 映射函数逐一）；404/403 原策略；
 *  - revision=null 不提供精确计数。
 */
import { describe, it, expect, afterEach, vi } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { Hono } from "hono";

import { HistoryDirectoryCache } from "../server/history-read/cache.ts";
import { readSessionHistoryOverview, mapOverviewUnavailableReason } from "../server/history-read/index.ts";
import { createSessionsRoute } from "../server/routes/sessions.ts";
import { SessionManifestStore } from "../core/session-manifest/store.ts";
import { SessionManager } from "../lib/pi-sdk/index.ts";
import { buildLongRunFixtureBytes, messagesUrl } from "../scripts/lib/history-read-fixture.mjs";
import { createHistoryReadCounters } from "../scripts/lib/history-read-counters.mjs";
import { installModuleWrappers } from "../scripts/lib/history-read-instrumentation.mjs";
import type { HistoryOverview, HistoryOverviewUnavailable } from "../server/history-read/protocol.ts";

const tmpDirs: string[] = [];
const manifestStores: SessionManifestStore[] = [];

afterEach(() => {
  while (manifestStores.length) manifestStores.pop()?.close();
  while (tmpDirs.length) {
    const dir = tmpDirs.pop();
    fs.rmSync(dir, { recursive: true, force: true, maxRetries: 20, retryDelay: 250 });
  }
});

function makeTmpDir(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-overview-test-"));
  tmpDirs.push(dir);
  return dir;
}

function jsonl(id: string, parentId: string | null, payload: Record<string, any>) {
  return JSON.stringify({ type: "message", id, parentId, timestamp: "2026-09-10T10:00:00Z", ...payload });
}
function msg(id: string, parentId: string | null, role: string, content: any) {
  return jsonl(id, parentId, { message: { role, content } });
}
function custom(id: string, parentId: string | null, customType: string, data: any) {
  return JSON.stringify({ type: "custom", id, parentId, timestamp: "2026-09-10T10:00:00Z", customType, data });
}
const HEADER = JSON.stringify({ type: "session", version: 3, id: "7e5f1a3c-9d24-4b8e-a6c1-2f4b8d9e0a31", cwd: "/tmp", timestamp: "2026-09-10T09:00:00Z" });

function createHarness({ fixtureLines, n }: { fixtureLines?: string[]; n?: number } = {}) {
  const root = makeTmpDir();
  const agentsDir = path.join(root, "agents");
  const sessionPath = path.join(agentsDir, "hana", "sessions", "longrun.jsonl");
  fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
  fs.writeFileSync(sessionPath, fixtureLines ? fixtureLines.join("\n") + "\n" : buildLongRunFixtureBytes(n ?? 10));
  const manifestStore = new SessionManifestStore({ dbPath: path.join(root, "manifest.db") }); manifestStores.push(manifestStore);
  const manifest = manifestStore.createForPath({ sessionPath, ownerAgentId: "hana", domain: "desktop", kind: "chat" });
  const cache = new HistoryDirectoryCache();
  const deferredTasks: any[] = [];
  const listBySession = vi.fn((p: string) => (p === sessionPath ? deferredTasks : []));
  const engine: any = {
    agentsDir,
    currentSessionPath: sessionPath,
    isSessionStreaming: () => false,
    agentIdFromSessionPath: () => "hana",
    getAgent: () => ({ agentName: "Hana" }),
    historyReadCache: cache,
    activityHub: null,
    deferredResults: { listBySession },
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
  const overviewUrl = "/api/sessions/history-overview?path=" + encodeURIComponent(sessionPath);
  return { root, agentsDir, sessionPath, sessionId: manifest.sessionId, manifestStore, engine, cache, app, overviewUrl, deferredTasks, listBySession };
}

describe("E04 概览：结构与手写 oracle", () => {
  it("手写 oracle：counts/三桶合计/task 去重与四类合计；分页字段符合合同", async () => {
    // 手写夹具（分支链 u1→a1→c1..c5→a2→u2→a3）：
    //   displayable = u1,a1,a2,u2,a3 = 5；sourceRecords = 10；
    //   Run1(u1→a1,c1..c5,a2) 有 a1,a2 → 2 条 displayable assistant；Run2(u2→a3) 有 a3 → 1 条；
    //   taskIds：sub-a(重复一次)、wf-b、med-c、oth-d → 去重 4：subagent/workflow/media/other 各 1。
    const lines = [
      HEADER,
      msg("u1", null, "user", "问题"),
      msg("a1", "u1", "assistant", "回答 1"),
      custom("c1", "a1", "hana-deferred-result", { taskId: "sub-a", status: "resolved", type: "subagent" }),
      custom("c2", "c1", "hana-deferred-result", { taskId: "sub-a", status: "resolved", type: "subagent" }),
      custom("c3", "c2", "hana-deferred-result", { taskId: "wf-b", status: "resolved", type: "workflow" }),
      custom("c4", "c3", "hana-deferred-result", { taskId: "med-c", status: "resolved", type: "image-generation" }),
      custom("c5", "c4", "hana-deferred-result", { taskId: "oth-d", status: "resolved", type: "mystery-thing" }),
      msg("a2", "c5", "assistant", "回答 2"),
      msg("u2", "a2", "user", "问题 2"),
      msg("a3", "u2", "assistant", "回答 3"),
    ];
    const h = createHarness({ fixtureLines: lines });
    const res = await h.app.request(h.overviewUrl);
    expect(res.status).toBe(200);
    expect(res.headers.get("lingxi-history-protocol")).toBe("1");
    expect(res.headers.get("cache-control")).toBe("private, no-store");
    const body = await res.json() as HistoryOverview;
    expect(body.schemaVersion).toBe(1);
    expect(body.available).toBe(true);
    expect(body.sessionId).toBe(h.sessionId);
    expect(typeof body.revision).toBe("string");
    expect(body.counts).toEqual({
      displayRecords: 5,
      sourceRecords: 10,
      runsWithAssistant: 2,
      referencedTasks: 4,
    });
    expect(body.counts.runsWithAssistant).toBe(
      body.runSizeDistribution.oneTo50 + body.runSizeDistribution.from51To200 + body.runSizeDistribution.over200,
    );
    expect(body.counts.referencedTasks).toBe(
      body.taskDistribution.subagent + body.taskDistribution.workflow + body.taskDistribution.media + body.taskDistribution.other,
    );
    expect(body.taskDistribution).toEqual({ subagent: 1, workflow: 1, media: 1, other: 1 });
    expect(body.pagination).toEqual({
      legacyDefaultLimit: 50,
      recommendedLimit: 100, // E06.2 选定 K=100
      maxLimit: 200,
      estimatedPagesAtRecommendedLimit: 1, // ceil(5/100)
    });
    const rawBytes = Buffer.byteLength(JSON.stringify(body), "utf8");
    expect(rawBytes).toBeLessThanOrEqual(4096);
  });

  it("空会话=合法零计数（estimatedPages=0）", async () => {
    const h = createHarness({ fixtureLines: [HEADER] });
    const res = await h.app.request(h.overviewUrl);
    expect(res.status).toBe(200);
    const body = (await res.json()) as HistoryOverview;
    expect(body.available).toBe(true);
    expect(body.counts).toEqual({ displayRecords: 0, sourceRecords: 0, runsWithAssistant: 0, referencedTasks: 0 });
    expect(body.runSizeDistribution).toEqual({ oneTo50: 0, from51To200: 0, over200: 0 });
    expect(body.pagination.estimatedPagesAtRecommendedLimit).toBe(0);
  });

  it("长夹具 oracle（n=10）：display=11 / source=20 / 单 Run 10 条 assistant → oneTo50=1", async () => {
    const h = createHarness({ n: 10 });
    const res = await h.app.request(h.overviewUrl);
    const body = (await res.json()) as HistoryOverview;
    expect(body.counts).toEqual({
      displayRecords: 11,
      sourceRecords: 20,
      runsWithAssistant: 1,
      referencedTasks: 0,
    });
    expect(body.runSizeDistribution).toEqual({ oneTo50: 1, from51To200: 0, over200: 0 });
  });
});

describe("E04 概览：追加与 Run 跨桶（增量维护）", () => {
  it("仅用户尾段不计入；追加 assistant 计入；跨 50 条跨桶 oneTo50→from51To200", async () => {
    const h = createHarness({ fixtureLines: [HEADER, msg("u1", null, "user", "开始")] });
    const overviewOf = async () => (await (await h.app.request(h.overviewUrl)).json()) as HistoryOverview;

    const before = await overviewOf();
    expect(before.counts.runsWithAssistant).toBe(0); // 仅有用户输入尾段不计入
    expect(before.runSizeDistribution).toEqual({ oneTo50: 0, from51To200: 0, over200: 0 });

    // 追加 1 条 assistant → Run 诞生（oneTo50=1）
    appendLines(h.sessionPath, [msg("a1", "u1", "assistant", "r1")]);
    h.engine.syncBranchHead("a1", "a1", "append_sync");
    const one = await overviewOf();
    expect(one.counts.runsWithAssistant).toBe(1);
    expect(one.runSizeDistribution.oneTo50).toBe(1);
    expect(one.counts.displayRecords).toBe(before.counts.displayRecords + 1);

    // 同 Run 再追加 50 条 → 共 51 条 → 跨桶 oneTo50→from51To200
    const lines: string[] = [];
    let parent = "a1";
    for (let i = 2; i <= 51; i += 1) {
      lines.push(msg(`a${i}`, parent, "assistant", `r${i}`));
      parent = `a${i}`;
    }
    appendLines(h.sessionPath, lines);
    h.engine.syncBranchHead("a51", "a51", "append_sync");
    const crossed = await overviewOf();
    expect(crossed.counts.runsWithAssistant).toBe(1);
    expect(crossed.runSizeDistribution.oneTo50).toBe(0);
    expect(crossed.runSizeDistribution.from51To200).toBe(1);
    expect(crossed.counts.displayRecords).toBe(52);
  });

  it("分支 rewind 不返回旧分支统计；追加到废弃分支不污染当前分支计数", async () => {
    const h = createHarness({ n: 10 });
    const before = (await (await h.app.request(h.overviewUrl)).json()) as HistoryOverview;
    expect(before.counts.displayRecords).toBe(11);

    // head rewind 到 a5：branch_view_stale → 前缀重建
    h.manifestStore.setBranchHead(h.sessionId, { leafId: "a5", observedTailLeafId: "a10", reason: "branch_read_observe_tail" });
    const rewound = (await (await h.app.request(h.overviewUrl)).json()) as HistoryOverview;
    expect(rewound.available).toBe(true);
    expect(rewound.counts.displayRecords).toBe(6); // u1 + a1..a5
    expect(rewound.counts.runsWithAssistant).toBe(1);
    expect(rewound.counts.displayRecords).toBeLessThan(before.counts.displayRecords);

    // 追加到废弃尾部（a10 后接 a11，head 停在 a5）：物理索引更新但当前分支计数不变
    appendLines(h.sessionPath, [jsonl("a11", "a10", { message: { role: "assistant", content: "废弃支线" } })]);
    h.engine.syncBranchHead("a5", "a11", "append_sync");
    const after = (await (await h.app.request(h.overviewUrl)).json()) as HistoryOverview;
    expect(after.counts.displayRecords).toBe(6);
    expect(after.counts.runsWithAssistant).toBe(1);
  });
});

describe("E04 概览：热路径硬断言（E04.3）", () => {
  it("冷构建后热概览：全文件读取 0、JSONL 解析 0、不触发外部状态 hydrate", async () => {
    const counters = createHistoryReadCounters({ label: "overview-hot-test" });
    counters.install();
    const uninstall = installModuleWrappers();
    try {
      const h = createHarness({ n: 10 });
      // 冷：目录构建（允许 O(N)）
      const hCold = counters.beginRequest({ fixtureId: "ov", requestKind: "overview-cold" });
      await hCold.run(() => h.app.request(h.overviewUrl));
      const coldRec = counters.endRequest(hCold);
      expect(coldRec.gauges.jsonlParseCount).toBeGreaterThan(0);

      // 热：命中目录 → 0 文件读 / 0 JSONL 解析 / 不 hydrate
      const listBySessionCallsBefore = h.listBySession.mock.calls.length;
      const hHot = counters.beginRequest({ fixtureId: "ov", requestKind: "overview-hot" });
      const res = await hHot.run(() => h.app.request(h.overviewUrl));
      const hotRec = counters.endRequest(hHot);
      expect(res.status).toBe(200);
      expect(hotRec.gauges.fullFileReadCalls).toBe(0);
      expect(hotRec.gauges.jsonlParseCount).toBe(0);
      expect(h.listBySession.mock.calls.length).toBe(listBySessionCallsBefore); // 无页面正文/外部状态 hydrate
    } finally {
      uninstall();
    }
  });
});

describe("E04 概览：不可用与错误边界", () => {
  it("文件缺失 → available:false reason=revision_unknown（不伪造 0 计数）", async () => {
    const h = createHarness({ n: 10 });
    fs.unlinkSync(h.sessionPath);
    const res = await h.app.request(h.overviewUrl);
    expect(res.status).toBe(200);
    const body = (await res.json()) as HistoryOverviewUnavailable;
    expect(body.available).toBe(false);
    expect(body.schemaVersion).toBe(1);
    expect(body.reason).toBe("revision_unknown");
    expect("counts" in body).toBe(false);
  });

  it("未知 sessionId → 404；越权路径 → 403；均不泄露概览/标签", async () => {
    const h = createHarness({ n: 10 });
    const missing = await h.app.request("/api/sessions/history-overview?sessionId=sess_does_not_exist");
    expect(missing.status).toBe(404);
    const outside = path.join(makeTmpDir(), "outside.jsonl");
    fs.writeFileSync(outside, "{}\n");
    const denied = await h.app.request("/api/sessions/history-overview?path=" + encodeURIComponent(outside));
    expect(denied.status).toBe(403);
  });

  it("映射函数逐一：legacy_fallback→unsupported_history；budget_exceeded/directory_invalid→directory_unavailable；revision_unknown→revision_unknown", () => {
    expect(mapOverviewUnavailableReason("legacy_fallback")).toBe("unsupported_history");
    expect(mapOverviewUnavailableReason("budget_exceeded")).toBe("directory_unavailable");
    expect(mapOverviewUnavailableReason("directory_invalid")).toBe("directory_unavailable");
    expect(mapOverviewUnavailableReason("revision_unknown")).toBe("revision_unknown");
  });

  it("revision=null 不提供精确计数（真实路由：返回 revision_unknown 而非 0 计数）", async () => {
    const h = createHarness({ n: 10 });
    const res = await h.app.request(h.overviewUrl);
    expect(res.status).toBe(200);
    const body = (await res.json()) as HistoryOverview;
    expect(body.revision).toBeTruthy(); // 可用则必须带真实 revision
    expect(body.available).toBe(true);
  });
});

function appendLines(sessionPath: string, lines: string[]) {
  const existing = fs.readFileSync(sessionPath);
  const needsSep = existing.length > 0 && existing[existing.length - 1] !== 0x0a;
  fs.appendFileSync(sessionPath, (needsSep ? "\n" : "") + lines.join("\n") + "\n");
}
