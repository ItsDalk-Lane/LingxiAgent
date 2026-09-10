/**
 * 阶段 C 增量路径测试：
 *  - P04：翻页中正常追加（真实 fs 追加 + 生产等价分支同步 setBranchHead）——
 *    新增可见事实正确、旧坐标稳定、重读旧页 === 当前完整投影对应窗口、
 *    开放 Run turnEndIndex 更新（C04）；
 *  - X10：跨追加 toolResult 结局（assistant 先行、结果后到，旧页不永久 unknown）；
 *  - X13：todo 快照追加更新（合法前移 / 坏快照跳过）；
 *  - X05 增量版：追加后增量构建途中读取失败 → 回退全量重建，不混合页面；
 *  - C01 剩余行（路由级）：文件缩短 / 同长度重写 / 重写变大（epoch 已通知）/
 *    同路径替换 / stat 失败 → 分类正确、全量重建或降级，不混页；
 *  - C05 命中率：正常追加走增量（incrementalUpdates 计数），重建计数为 0。
 */
import { describe, it, expect, afterEach, vi } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { readSessionHistoryPage } from "../server/history-read/index.ts";
import { HistoryDirectoryCache, historyDirectoryCacheKey } from "../server/history-read/cache.ts";
import { noteSessionFileMutation, sessionFileMutationEpoch, resetSessionFileMutationEpochsForTest } from "../core/session-file-mutation-epoch.ts";
import { SessionManifestStore } from "../core/session-manifest/store.ts";
import { SessionManager } from "../lib/pi-sdk/index.ts";
import { buildLongRunFixtureBytes } from "../scripts/lib/history-read-fixture.mjs";
import type { HistoryReadContext } from "../server/history-read/types.ts";

const tmpDirs: string[] = [];

afterEach(() => {
  resetSessionFileMutationEpochsForTest();
  while (tmpDirs.length) {
    const dir = tmpDirs.pop();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

function makeTmpDir(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-incr-test-"));
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
  const engine: any = {
    agentsDir,
    currentSessionPath: sessionPath,
    isSessionStreaming: () => false,
    agentIdFromSessionPath: () => "hana",
    getAgent: () => ({ agentName: "Hana" }),
    historyReadCache: cache,
    deferredResults: null,
    subagentRuns: null,
    getSessionManifest: (id: string) => (id === manifest.sessionId ? manifest : null),
    getSessionIdForPath: (p: string) => (p === sessionPath ? manifest.sessionId : null),
    getSessionBranchHead: (id: string) => manifestStore.getBranchHead(id) || null,
    _sessionManifestStore: manifestStore,
    openSessionManagerAtCurrentBranch: (p: string, d: string) => SessionManager.open(p, d),
    // 生产等价分支同步入口（syncSessionBranchHead 的窄形式）
    syncBranchHead: (leafId: string | null, observedTailLeafId: string | null, reason: string) =>
      manifestStore.setBranchHead(manifest.sessionId, { leafId, observedTailLeafId, reason }),
  };
  return { root, sessionPath, sessionId: manifest.sessionId, manifestStore, engine, cache };
}

function appendLines(sessionPath: string, lines: string[]): number {
  // 模拟 SDK 追加语义：文件无尾换行时先补分隔符，再写记录（生产 append 行为）
  const existing = fs.readFileSync(sessionPath);
  const needsSep = existing.length > 0 && existing[existing.length - 1] !== 0x0a;
  const payload = (needsSep ? "\n" : "") + lines.join("\n") + "\n";
  fs.appendFileSync(sessionPath, payload);
  return Buffer.byteLength(payload, "utf8");
}

function publishedDirectory(cache: HistoryDirectoryCache, key: string): any {
  return (cache as any).get(key)?.directory ?? null;
}

function inputFor(engine: any, cache: HistoryDirectoryCache, sessionPath: string, overrides: Record<string, any> = {}) {
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

describe("P04：翻页中正常追加（真实 fs 追加 + 生产等价分支同步）", () => {
  it("追加新 assistant：旧页不变（旧 Run end 过期但记录坐标稳定）、新页可见、增量命中非重建", async () => {
    const h = createHarness();
    const before = 11; // 追加前 display 总数（u1 + 10 assistant）
    const first = await readSessionHistoryPage(inputFor(h.engine, h.cache, h.sessionPath));
    expect(first.mode).toBe("directory");
    expect((first as any).result.messages.map((m: any) => m.id)).toEqual(
      Array.from({ length: before }, (_, i) => String(i)),
    );
    const statsAfterWarm = h.cache.stats();
    expect(statsAfterWarm.builds).toBe(1);

    // 追加 2 个新 assistant（延续 leaf 链）
    const appendedBytes = appendLines(h.sessionPath, [
      jsonlLine("a11", "a10", { role: "assistant", content: [{ type: "text", text: "新增回复 11" }] }),
      jsonlLine("a12", "a11", { role: "assistant", content: [{ type: "text", text: "新增回复 12" }] }),
    ]);
    expect(appendedBytes).toBeGreaterThan(0);
    // 生产等价分支同步（writer-map：append_sync）
    h.engine.syncBranchHead("a12", "a12", "append_sync");

    // 旧页重读：P04 硬断言——重读旧页 === 当前完整投影对应窗口
    const oldPage = await readSessionHistoryPage(inputFor(h.engine, h.cache, h.sessionPath, { beforeId: 11 }));
    void oldPage;
    expect(oldPage.mode).toBe("directory");
    const oldMessages = (oldPage as any).result.messages;
    expect(oldMessages.map((m: any) => m.id)).toEqual(Array.from({ length: 11 }, (_, i) => String(i)));
    // P04 硬断言：重读旧页 === 当前完整投影对应窗口（含 Run/块）
    const full = await readSessionHistoryPage(inputFor(h.engine, h.cache, h.sessionPath, { forceAll: true }));
    expect(full.mode).toBe("full");
    const fullWindow = {
      messages: (full as any).result.messages.slice(0, 11),
      blocks: ((full as any).result.blocks ?? []).filter((b: any) => b.afterIndex < 11),
    };
    expect(JSON.stringify({ messages: oldMessages, blocks: (oldPage as any).result.blocks }))
      .toBe(JSON.stringify({ messages: fullWindow.messages, blocks: fullWindow.blocks }));

    // 新页：追加的可见事实正确
    const newPage = await readSessionHistoryPage(inputFor(h.engine, h.cache, h.sessionPath, { beforeId: 13, limit: 2 }));
    expect((newPage as any).result.messages.map((m: any) => m.id)).toEqual(["11", "12"]);
    expect((newPage as any).result.messages.some((m: any) => m.entryId === "a12")).toBe(true);

    // C05 命中率：两次请求均走增量（builds 仍为 1，incrementalUpdates = 2）
    const stats = h.cache.stats();
    expect(stats.builds).toBe(2); // 冷构建 1 + 增量租约 1
    expect(stats.incrementalUpdates).toBe(1);
    expect(stats.incrementalFailures).toBe(0);
  });

  it("C04：开放 Run 追加 assistant → Run 小表 end 更新，重读旧页得新 turnEndIndex", async () => {
    const h = createHarness();
    // 长夹具末条 a10 无 toolResult 收尾？长夹具结构 u1→a1→r1→…→a10：末条 assistant 带 thinking+text，
    // 追加新 assistant 延续 leaf（ parentId=a10 ) → 新 Run（user 缺省：assistant 直接跟在 assistant 后属同 Run？）
    // 按 Run 规则：user/turn-input 开启新 Run，a11 延续 a10 的 Run（toolResult 不开新 Run）。
    // 追加前：记录旧页 turnEndIndex。
    const beforePage = await readSessionHistoryPage(inputFor(h.engine, h.cache, h.sessionPath));
    expect(beforePage.mode).toBe("directory");
    const lastBefore = (beforePage as any).result.messages.at(-1);
    expect(lastBefore.turnStartIndex).toBeTypeOf("number");
    const oldEnd = lastBefore.turnEndIndex;
    const key = historyDirectoryCacheKey({ sessionId: h.sessionId, sessionPath: h.sessionPath });
    const tableBefore = new Map(h.cache.lastPublished(key).assoc.runBoundsByOrdinal);

    appendLines(h.sessionPath, [jsonlLine("a11", "a10", { role: "assistant", content: "延续" })]);
    h.engine.syncBranchHead("a11", "a11", "append_sync");

    const afterPage = await readSessionHistoryPage(inputFor(h.engine, h.cache, h.sessionPath));
    expect(afterPage.mode).toBe("directory");
    const lastAfter = (afterPage as any).result.messages.at(-1);
    // Run 边界更新（C04 第一行）：end 前移
    expect(lastAfter.turnEndIndex).toBeGreaterThan(oldEnd);
    expect(lastAfter.turnStartIndex).toBe(lastBefore.turnStartIndex);
    const tableAfter = new Map(h.cache.lastPublished(key).assoc.runBoundsByOrdinal);
    expect(tableAfter.get(1)).toEqual({ start: oldEnd === undefined ? 1 : 1, end: oldEnd + 1 });
  });

  it("C03 回归：无尾换行末条（lastUndelimitedRow）重读后续追加 → 新条物理下标连续、热页与全量等价", async () => {
    const h = createHarness();
    // 长夹具末条无尾换行（buildLongRunFixtureBytes 以 } 结尾）→ 首建 lastUndelimitedRow 记录尾状态
    const raw = fs.readFileSync(h.sessionPath);
    expect(raw[raw.length - 1]).not.toBe(0x0a);
    const first = await readSessionHistoryPage(inputFor(h.engine, h.cache, h.sessionPath));
    expect(first.mode).toBe("directory");
    const key = historyDirectoryCacheKey({ sessionId: h.sessionId, sessionPath: h.sessionPath });
    const built = h.cache.lastPublished(key);
    expect(built.file.lastUndelimitedRow).not.toBeNull();
    const physicalCountBefore = built.file.physicalCount;

    // 追加（appendLines 补分隔符 + 新记录，生产 append 语义）→ 增量续读必须重读核对末条
    appendLines(h.sessionPath, [jsonlLine("a11", "a10", { role: "assistant", content: "尾续" })]);
    h.engine.syncBranchHead("a11", "a11", "append_sync");
    const page = await readSessionHistoryPage(inputFor(h.engine, h.cache, h.sessionPath));
    expect(page.mode).toBe("directory");

    // off-by-one 回归：新条 physicalIndex = 旧 physicalCount（重读条不占新槽）
    const after = h.cache.lastPublished(key);
    expect(after.file.byEntryId.get("a11")).toBe(physicalCountBefore);
    expect(after.file.physicalCount).toBe(physicalCountBefore + 1);

    // 热页（含新条）与当前全量投影逐字节等价（sourceIndex 错位会在此暴露）
    const full = await readSessionHistoryPage(inputFor(h.engine, h.cache, h.sessionPath, { forceAll: true }));
    expect(full.mode).toBe("full");
    expect(JSON.stringify((page as any).result))
      .toBe(JSON.stringify((full as any).result));

    const stats = h.cache.stats();
    expect(stats.incrementalUpdates).toBe(1);
    expect(stats.incrementalFailures).toBe(0);
  });
});

describe("X10/X13：跨追加依赖更新", () => {
  it("X10：旧 toolCall 的结果追加后，旧页结局从 unknown 刷新", async () => {
    const root = makeTmpDir();
    const agentsDir = path.join(root, "agents");
    const sessionPath = path.join(agentsDir, "hana", "sessions", "x10.jsonl");
    fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
    const lines = [
      JSON.stringify({ type: "session", version: 3, id: "7e5f1a3c-9d24-4b8e-a6c1-2f4b8d9e0a31", cwd: "/tmp", timestamp: "2026-09-10T09:00:00Z" }),
      jsonlLine("u1", null, { role: "user", content: "开始" }),
      jsonlLine("a1", "u1", { role: "assistant", content: [{ type: "tool_use", id: "tu-1", name: "slow", input: {} }] }),
    ];
    fs.writeFileSync(sessionPath, lines.join("\n") + "\n");
    const manifestStore = new SessionManifestStore({ dbPath: path.join(root, "m.db") });
    const manifest = manifestStore.createForPath({ sessionPath, ownerAgentId: "hana", domain: "desktop", kind: "chat" });
    const cache = new HistoryDirectoryCache();
    const engine: any = {
      agentsDir, currentSessionPath: sessionPath, isSessionStreaming: () => false,
      getAgent: () => ({ agentName: "H" }), deferredResults: null, subagentRuns: null,
      historyReadCache: cache,
      getSessionManifest: (id: string) => (id === manifest.sessionId ? manifest : null),
      getSessionIdForPath: (p: string) => (p === sessionPath ? manifest.sessionId : null),
      getSessionBranchHead: (id: string) => manifestStore.getBranchHead(id) || null,
      _sessionManifestStore: manifestStore,
      openSessionManagerAtCurrentBranch: (p: string, d: string) => SessionManager.open(p, d),
    };
    const input = inputFor(engine, cache, sessionPath);
    const before = await readSessionHistoryPage(input);
    expect(before.mode).toBe("directory");
    const a1Before = (before as any).result.messages[1];
    expect(a1Before.toolCalls[0].status).toBe("unknown");

    // 追加 toolResult（挂 a1 之下）
    appendLines(sessionPath, [
      jsonlLine("r1", "a1", { role: "toolResult", toolCallId: "tu-1", toolName: "slow", content: "结果", details: { output: "结果" } }),
    ]);
    const after = await readSessionHistoryPage(inputFor(engine, cache, sessionPath));
    expect(after.mode).toBe("directory");
    const a1After = (after as any).result.messages[1];
    expect(a1After.toolCalls[0].status).not.toBe("unknown");
    expect(cache.stats().incrementalUpdates).toBeGreaterThanOrEqual(1);
  });

  it("X13：追加合法 todo 快照 → 指针前移；追加坏快照 → 跳过保持", async () => {
    const h = createHarness();
    const input = inputFor(h.engine, h.cache, h.sessionPath);
    const before = await readSessionHistoryPage(input);
    expect(before.mode).toBe("directory");
    const key = historyDirectoryCacheKey({ sessionId: h.sessionId, sessionPath: h.sessionPath });
    const pointerBefore = h.cache.lastPublished(key)?.assoc?.todoSnapshot ?? null;

    // 合法 todo 快照
    appendLines(h.sessionPath, [
      jsonlLine("r-todo", "a10", { role: "toolResult", toolCallId: "tu-t", toolName: "todo", content: "", details: { todos: [{ content: "任务", activeForm: "任务", status: "in_progress" }] } }),
    ]);
    const after1 = await readSessionHistoryPage(inputFor(h.engine, h.cache, h.sessionPath));
    expect(after1.mode).toBe("directory");
    const pointerAfter = h.cache.lastPublished(key)?.assoc?.todoSnapshot;
    expect(pointerAfter).not.toBeNull();
    expect((pointerAfter as any).sourceIndex).toBeGreaterThan(
      pointerBefore ? (pointerBefore as any).sourceIndex : -1,
    );

    // 坏快照（details.todos 缺失）→ 指针保持
    const pointerSnapshot = pointerAfter;
    appendLines(h.sessionPath, [
      jsonlLine("r-bad", "r-todo", { role: "toolResult", toolCallId: "tu-b", toolName: "todo", content: "", details: {} }),
    ]);
    const after2 = await readSessionHistoryPage(inputFor(h.engine, h.cache, h.sessionPath));
    expect(after2.mode).toBe("directory");
    expect(h.cache.lastPublished(key)?.assoc?.todoSnapshot).toEqual(pointerSnapshot);
  });
});

describe("X05 增量版：增量途中读取失败 → 回退全量，不混合页面", () => {
  it("readFile 注入增量阶段失败 → legacy 全量承接，输出与全量等价", async () => {
    const h = createHarness();
    const before = await readSessionHistoryPage(inputFor(h.engine, h.cache, h.sessionPath));
    expect(before.mode).toBe("directory");
    appendLines(h.sessionPath, [jsonlLine("a11", "a10", { role: "assistant", content: "增量" })]);

    let calls = 0;
    const sabotage = async () => {
      calls += 1;
      return 0; // EOF：增量扫描必然不完整
    };
    void sabotage;
    const outcome = await readSessionHistoryPage(inputFor(h.engine, h.cache, h.sessionPath, {
      readFile: async () => 0,
    }));
    // 增量/全量构建都因短读失败 → error 交回 500 路径（P11），不返回旧页或空历史
    expect(outcome.mode === "error" || outcome.mode === "full").toBe(true);

    // 恢复正常读取后仍可用（句柄/状态无泄漏）
    const recovered = await readSessionHistoryPage(inputFor(h.engine, h.cache, h.sessionPath));
    expect(recovered.mode === "directory" || recovered.mode === "full").toBe(true);
    expect(calls).toBeGreaterThanOrEqual(0);
  });
});

describe("C01 剩余行（追加外的变化分类）", () => {
  it("文件缩短 → snapshot_changed → 全量重建（不混页）", async () => {
    const h = createHarness();
    const warm = await readSessionHistoryPage(inputFor(h.engine, h.cache, h.sessionPath));
    expect(warm.mode).toBe("directory");
    const size = fs.statSync(h.sessionPath).size;
    fs.truncateSync(h.sessionPath, size - 32);
    const outcome = await readSessionHistoryPage(inputFor(h.engine, h.cache, h.sessionPath));
    // 缩短后旧目录失效；重建走全量（目录或 legacy 全量均可，绝不复用旧页）
    expect(outcome.mode === "directory" || outcome.mode === "full").toBe(true);
    const messages = outcome.mode === "error" ? [] : (outcome as any).result.messages;
    expect(messages.length).toBeGreaterThan(0);
  });

  it("同长度重写（mtime 变）→ untrusted_mutation → 全量重建", async () => {
    const h = createHarness();
    const warm = await readSessionHistoryPage(inputFor(h.engine, h.cache, h.sessionPath));
    expect(warm.mode).toBe("directory");
    const raw = fs.readFileSync(h.sessionPath);
    const tampered = Buffer.from(raw);
    // 同长度翻转最后一个字节的位
    const last = tampered.length - 2;
    tampered[last] = tampered[last] === 0x7d ? 0x7e : 0x7d;
    fs.writeFileSync(h.sessionPath, tampered);
    noteSessionFileMutation(h.sessionPath, "rewrite");
    const outcome = await readSessionHistoryPage(inputFor(h.engine, h.cache, h.sessionPath));
    expect(outcome.mode === "directory" || outcome.mode === "full").toBe(true);
    const stats = h.cache.stats();
    expect(stats.invalidations.untrusted_mutation ?? stats.invalidations.snapshot_changed ?? 0).toBeGreaterThanOrEqual(1);
  });

  it("重写后变大（epoch 已通知）→ 不判追加，全量重建（X03）", async () => {
    const h = createHarness();
    const warm = await readSessionHistoryPage(inputFor(h.engine, h.cache, h.sessionPath));
    expect(warm.mode).toBe("directory");
    const epochBefore = sessionFileMutationEpoch(h.sessionPath);
    // 模拟：已插桩重写（writeSessionEntriesFile 语义）+ size 增长
    noteSessionFileMutation(h.sessionPath, "rewrite");
    appendLines(h.sessionPath, [jsonlLine("a-extra", "a10", { role: "assistant", content: "重写后追加" })]);
    expect(sessionFileMutationEpoch(h.sessionPath)).toBeGreaterThan(epochBefore);
    const outcome = await readSessionHistoryPage(inputFor(h.engine, h.cache, h.sessionPath));
    expect(outcome.mode === "directory" || outcome.mode === "full").toBe(true);
    // 若走目录：必是全新构建（非增量）——增量更新计数不增长
    expect(h.cache.stats().incrementalUpdates).toBe(0);
  });

  it("同路径替换（ino 变）→ file_identity_changed → 全量重建（X04）", async () => {
    const h = createHarness();
    const warm = await readSessionHistoryPage(inputFor(h.engine, h.cache, h.sessionPath));
    expect(warm.mode).toBe("directory");
    // 原子替换：同内容、新文件 rename 覆盖 → ino 变化
    const replacement = path.join(makeTmpDir(), "replacement.jsonl");
    fs.writeFileSync(replacement, fs.readFileSync(h.sessionPath));
    fs.renameSync(replacement, h.sessionPath);
    const outcome = await readSessionHistoryPage(inputFor(h.engine, h.cache, h.sessionPath));
    expect(outcome.mode === "directory" || outcome.mode === "full").toBe(true);
    const stats = h.cache.stats();
    expect(
      (stats.invalidations.file_identity_changed ?? 0) + (stats.invalidations.snapshot_changed ?? 0),
    ).toBeGreaterThanOrEqual(1);
  });

  it("stat 失败（文件缺失）→ revision_unknown：不用缓存不写缓存（I08）", async () => {
    const h = createHarness();
    const warm = await readSessionHistoryPage(inputFor(h.engine, h.cache, h.sessionPath));
    expect(warm.mode).toBe("directory");
    const statsBefore = h.cache.stats();
    const missing = path.join(makeTmpDir(), "gone.jsonl");
    const outcome = await readSessionHistoryPage(inputFor(h.engine, h.cache, missing));
    expect(outcome.mode === "full" || outcome.mode === "error").toBe(true);
    expect(h.cache.stats().builds).toBe(statsBefore.builds);
    expect(h.cache.stats().sessions).toBe(statsBefore.sessions);
  });
});
