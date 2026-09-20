/**
 * 编辑重发（retry）后读档混页回归：
 * 「编辑已发送消息 → 触发重新生成」在会话文件上等价于：
 *   1. 分支回退（head 行写 {leafId: 回退点, observedTailLeafId: 旧物理尾}，reason=replay_rewind）；
 *   2. 追加分支重置标记（parent=回退点）；
 *   3. 追加新用户消息与新助手输出（parent 链挂在新分支上）；
 *   4. run 收尾后 head 行同步为新尾（reason=prompt_finally / append_sync）。
 *
 * 若历史读取目录缓存在第 1 步之前已完成构建（会话打开时的首屏 hydrate），
 * 生成结束瞬间客户端 reconcile 会立刻重读历史。此时增量路径把「旧视图尾段」
 * 与「新追加链」直接拼接（appendedChain.includes(selectedLeafId) 判定为延长
 * 当前分支），旧的被丢弃条目（旧用户消息/旧回答）残留在视图里 —— 界面表现为
 * 「生成完成的瞬间跳回旧内容 / 新旧混排」，且目录分支指纹与 head 行一致，
 * 后续 probe 恒 valid，直到进程重启才恢复（对应桌面端实测：仅重启软件可复原）。
 *
 * 本文件钉死三个时间窗的正确投影（entryId 序列）：
 *   - 完成窗（head 已同步新尾）：[u0, a0, u2, a2]（旧 u1/a1 必须消失）；
 *   - 生成中窗（head 仍是 rewind 行，仅标记+新用户消息落盘）：[u0, a0, u2]；
 *   - 复读稳定性：同一状态二次读取结果一致（不因缓存状态漂移）。
 */
import { describe, it, expect, afterEach } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { readSessionHistoryPage } from "../server/history-read/index.ts";
import { HistoryDirectoryCache } from "../server/history-read/cache.ts";
import { resetSessionFileMutationEpochsForTest } from "../core/session-file-mutation-epoch.ts";
import { SessionManifestStore } from "../core/session-manifest/store.ts";

const tmpDirs: string[] = [];
const manifestStores: SessionManifestStore[] = [];

afterEach(() => {
  resetSessionFileMutationEpochsForTest();
  while (manifestStores.length) manifestStores.pop()?.close();
  while (tmpDirs.length) {
    const dir = tmpDirs.pop();
    fs.rmSync(dir, { recursive: true, force: true, maxRetries: 20, retryDelay: 250 });
  }
});

function makeTmpDir(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-retry-rewind-test-"));
  tmpDirs.push(dir);
  return dir;
}

function messageLine(id: string, parentId: string | null, role: "user" | "assistant", text: string) {
  return JSON.stringify({
    type: "message",
    id,
    parentId,
    timestamp: "2026-09-10T10:00:00Z",
    message: { role, content: [{ type: "text", text }] },
  });
}

/** 分支重置标记（与 core/session-turn-actions.ts 的 appendCustomEntry 同形） */
function branchResetLine(id: string, parentId: string) {
  return JSON.stringify({
    type: "custom",
    id,
    parentId,
    customType: "hana-session-branch-reset",
    data: { sourceEntryId: "u1", timestamp: 1 },
    timestamp: "2026-09-10T10:00:01Z",
  });
}

function appendLines(sessionPath: string, lines: string[]): void {
  const existing = fs.readFileSync(sessionPath);
  const needsSep = existing.length > 0 && existing[existing.length - 1] !== 0x0a;
  fs.appendFileSync(sessionPath, (needsSep ? "\n" : "") + lines.join("\n") + "\n");
}

function createHarness() {
  const root = makeTmpDir();
  const sessionPath = path.join(root, "agents", "hana", "sessions", "retry.jsonl");
  fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
  // 旧分支：u0→a0（保留段）→ u1→a1（将被编辑重发丢弃的轮次）
  fs.writeFileSync(sessionPath, [
    messageLine("u0", null, "user", "第一问"),
    messageLine("a0", "u0", "assistant", "第一答"),
    messageLine("u1", "a0", "user", "旧版问题"),
    messageLine("a1", "u1", "assistant", "旧版回答"),
  ].join("\n") + "\n");
  const manifestStore = new SessionManifestStore({ dbPath: path.join(root, "manifest.db") });
  manifestStores.push(manifestStore);
  const manifest = manifestStore.createForPath({ sessionPath, ownerAgentId: "hana", domain: "desktop", kind: "chat" });
  const cache = new HistoryDirectoryCache();
  const engine: any = {
    agentsDir: path.join(root, "agents"),
    isSessionStreaming: () => false,
    historyReadCache: cache,
    deferredResults: null,
    subagentRuns: null,
    getSessionManifest: (id: string) => (id === manifest.sessionId ? manifest : null),
    getSessionIdForPath: (p: string) => (p === sessionPath ? manifest.sessionId : null),
    getSessionBranchHead: (id: string) => manifestStore.getBranchHead(id) || null,
    syncBranchHead: (leafId: string | null, observedTailLeafId: string | null, reason: string) =>
      manifestStore.setBranchHead(manifest.sessionId, { leafId, observedTailLeafId, reason }),
  };
  return { sessionPath, engine, cache };
}

function inputFor(engine: any, cache: HistoryDirectoryCache, sessionPath: string) {
  return {
    engine,
    cache,
    sessionPath,
    sessionId: engine.getSessionIdForPath(sessionPath),
    beforeId: null,
    limit: 50,
    forceAll: false,
    sanitizeVisibleContent: (value: string) => value,
  };
}

async function readEntryIds(engine: any, cache: HistoryDirectoryCache, sessionPath: string): Promise<string[]> {
  const page = await readSessionHistoryPage(inputFor(engine, cache, sessionPath));
  if (page.mode === "error") throw page.error;
  const messages = (page as any).result.messages as Array<{ entryId?: string }>;
  return messages.map((m) => String(m.entryId));
}

describe("编辑重发（rewind + append）后读档不得混拼旧分支尾段", () => {
  it("完成窗：head 已同步新尾 → 视图必须丢弃旧 u1/a1，且复读稳定", async () => {
    const h = createHarness();
    // 首屏 hydrate：缓存以旧分支构建（head 尚未写过 → legacy tail = a1）
    expect(await readEntryIds(h.engine, h.cache, h.sessionPath)).toEqual(["u0", "a0", "u1", "a1"]);

    // 模拟编辑重发：回退到 a0 → 标记 → 新用户消息 → 新回答 → run 收尾同步
    appendLines(h.sessionPath, [
      branchResetLine("m1", "a0"),
      messageLine("u2", "m1", "user", "新版问题"),
      messageLine("a2", "u2", "assistant", "新版回答"),
    ]);
    h.engine.syncBranchHead("a2", "a2", "prompt_finally");

    // 生成完成瞬间的 reconcile 重读：旧 u1/a1 必须消失（修复前为 [u0,a0,u1,a1,u2,a2] 混拼）
    expect(await readEntryIds(h.engine, h.cache, h.sessionPath)).toEqual(["u0", "a0", "u2", "a2"]);
    // 复读稳定：不依赖缓存内部状态漂移
    expect(await readEntryIds(h.engine, h.cache, h.sessionPath)).toEqual(["u0", "a0", "u2", "a2"]);
  });

  it("生成中窗：head 仍是 rewind 行（leaf=a0, observedTail=a1）→ 只见保留段+新输入", async () => {
    const h = createHarness();
    expect(await readEntryIds(h.engine, h.cache, h.sessionPath)).toEqual(["u0", "a0", "u1", "a1"]);

    // 仅标记 + 新用户消息落盘；head 行停在 replay_rewind（observedTail=写行时的物理尾 a1）
    appendLines(h.sessionPath, [
      branchResetLine("m1", "a0"),
      messageLine("u2", "m1", "user", "新版问题"),
    ]);
    h.engine.syncBranchHead("a0", "a1", "replay_rewind");

    // 修复前为 [u0,a0,u1,a1,u2]：旧轮次残留 + 多出一条新输入（界面“多出一条用户消息”）
    expect(await readEntryIds(h.engine, h.cache, h.sessionPath)).toEqual(["u0", "a0", "u2"]);
  });

  it("仅标记窗：head 为 rewind 行且只有标记落盘 → 视图回退到保留段", async () => {
    const h = createHarness();
    expect(await readEntryIds(h.engine, h.cache, h.sessionPath)).toEqual(["u0", "a0", "u1", "a1"]);

    appendLines(h.sessionPath, [branchResetLine("m1", "a0")]);
    h.engine.syncBranchHead("a0", "a1", "replay_rewind");

    // 修复前为 [u0,a0,u1,a1]（旧视图原样残留，界面“整轮退回旧样子”）
    expect(await readEntryIds(h.engine, h.cache, h.sessionPath)).toEqual(["u0", "a0"]);
  });
});
