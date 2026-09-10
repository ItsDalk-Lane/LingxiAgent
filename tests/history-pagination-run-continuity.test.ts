// @vitest-environment jsdom

/**
 * 长单轮会话历史分页 × Run 连续性 × 结局唯一裁决（F1–F3 故障链）回归。
 *
 * 全链路真实代码：真实 JSONL → 真实 /api/sessions/messages 路由（Hono）→
 * 真实 loadMessages/loadMoreMessages（仅 lingxiFetch 打桩到 Hono app）→
 * 真实 buildItemsFromHistory / chat-slice 投影与合并。
 *
 * 核心不变量（任务书 §三）：
 *  - 分页不改变语义：任意切页恢复 ≡ 全量恢复（内容/归属/顺序/结局）。
 *  - 终态有权威来源：Run 未权威终结的页片段不得派生 missing_final_answer。
 *  - 游标独立于显示身份：分页边界来自服务端原始页面范围，不来自归并显示项。
 *  - 合并幂等：同版本同事实重复到达不增加消息/工具/告警数量。
 */

import { Hono } from "hono";
import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import { createHash } from "node:crypto";

// ── lingxiFetch 打桩：路由到真实 Hono app（vi.hoisted 保证 mock 工厂可见）──
const harness = vi.hoisted(() => ({
  app: null as any,
  bodies: [] as any[],
}));
vi.mock("../desktop/src/react/hooks/use-hana-fetch.ts", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return {
    ...original,
    lingxiFetch: async (url: string, opts?: any) => {
      const res = await harness.app.request(url, opts);
      harness.bodies.push({ url, body: await res.clone().json() });
      return res;
    },
  };
});

const { loadMessages, loadMoreMessages } = await import(
  "../desktop/src/react/stores/session-actions.ts"
);
const { useStore } = await import("../desktop/src/react/stores/index.ts");
const { sessionScopedValue } = await import("../desktop/src/react/stores/session-slice.ts");
const { buildItemsFromHistory } = await import("../desktop/src/react/utils/history-builder.ts");
const { SessionManager } = await import("../lib/pi-sdk/index.ts");
const { SessionManifestStore } = await import("../core/session-manifest/store.ts");
const { readCurrentSessionBranch } = await import("../lib/session-jsonl.ts");

// ── fixture ──

const RUN_SIZE = 140;

// SDK 文件头 UUID（type:"session" 行的 id）与业务 sessionId 是两套身份：
// 文件头是 SDK 的 UUID，业务 sessionId 由 SessionManifestStore 按生产格式生成（sess_…）。
const SESSION_FILE_HEADER_ID = "7e5f1a3c-9d24-4b8e-a6c1-2f4b8d9e0a31";
const BUSINESS_AGENT_ID = "hana";
const PAGE_SIZE = 50;

function jsonlLine(id: string, parentId: string | null, message: unknown): string {
  return JSON.stringify({
    type: "message",
    id,
    parentId,
    timestamp: "2026-09-10T10:00:00Z",
    message,
  });
}

/**
 * 长单轮 fixture（A03 修正后的合法父子链）：
 *   1 个文件头 + 1 条 user + N 条 assistant + (N-1) 条 toolResult，
 *   物理行数 = 2N+1，display 总数 = N+1（工具结果不占 display 序号）。
 * 链形状：u1(parent=null) → a1(parent=u1) → r1(parent=a1, toolCallId=tu-1)
 *        → a2(parent=r1) → r2(parent=a2) → … → aN(parent=rN-1)。
 * 每条 toolResult 使用独立 id（r<i>），parentId 指向对应 assistant；后续 assistant
 * 沿前一条实际物理记录（toolResult）延续父链。规模 N=1000 时 21 页、N=10000 时 201 页
 * （页大小 50，页数 = ceil(display/50)）。
 */
function writeLongRunSession(
  sessionPath: string,
  runSize = RUN_SIZE,
  opts: { trailingNewline?: boolean } = {},
): void {
  const trailingNewline = opts.trailingNewline ?? false;
  const lines: string[] = [
    JSON.stringify({
      type: "session",
      version: 3,
      id: SESSION_FILE_HEADER_ID,
      cwd: "/tmp",
      timestamp: "2026-09-10T09:00:00Z",
    }),
    jsonlLine("u1", null, { role: "user", content: "请完成长任务并给出最终报告" }),
  ];
  let parent = "u1";
  for (let i = 1; i <= runSize; i += 1) {
    const isFinal = i === runSize;
    const content = isFinal
      ? [
          { type: "thinking", thinking: `最终整理 ${i}` },
          { type: "text", text: `这是最终报告：任务已完成（第 ${i} 次调用后）。` },
        ]
      : [
          { type: "thinking", thinking: `思考片段 ${i}` },
          { type: "tool_use", id: `tu-${i}`, name: "read_file", input: { path: `f${i}` } },
        ];
    lines.push(jsonlLine(`a${i}`, parent, { role: "assistant", content }));
    if (!isFinal) {
      lines.push(
        jsonlLine(`r${i}`, `a${i}`, {
          role: "toolResult",
          toolCallId: `tu-${i}`,
          toolName: "read_file",
          content: `文件内容 ${i}`,
        }),
      );
      parent = `r${i}`;
    }
  }
  fs.writeFileSync(sessionPath, lines.join("\n") + (trailingNewline ? "\n" : ""), "utf8");
}

/**
 * 与服务端 display 计数同源：只有 user/assistant 且可显示的消息推进 display 序号
 * （server/routes/sessions.ts isDisplayableHistoryMessage / Run 边界预扫描）。
 * 本夹具族全部 user/assistant 均可显示（user 有文本、assistant 有 thinking/tool_use/text），
 * toolResult 不占序号。
 */
function countDisplayRecords(sessionPath: string): number {
  let count = 0;
  for (const line of fs.readFileSync(sessionPath, "utf8").split("\n")) {
    if (!line.trim()) continue;
    let entry: any;
    try {
      entry = JSON.parse(line);
    } catch {
      continue;
    }
    if (entry?.type === "message" && (entry.message?.role === "user" || entry.message?.role === "assistant")) {
      count += 1;
    }
  }
  return count;
}

function pagesForDisplay(displayCount: number, pageSize = PAGE_SIZE): number {
  return Math.ceil(displayCount / pageSize);
}

/** 分支入口跟踪：证明读取走 engine.openSessionManagerAtCurrentBranch → 真实 SessionManager 分支读取，且分支读取零异常（→ 兼容 raw-read fallback 不可达）。 */
interface BranchTracker {
  openCalls: number;
  openSessionPathCalls: number;
  openThrows: number;
  getBranchCalls: number;
  getBranchThrows: number;
}

function createBranchTracker(): BranchTracker {
  return { openCalls: 0, openSessionPathCalls: 0, openThrows: 0, getBranchCalls: 0, getBranchThrows: 0 };
}

async function buildApp(
  agentsDir: string,
  sessionPath: string,
  sessionManifest: any,
  branchTracker: BranchTracker,
) {
  const { createSessionsRoute } = await import("../server/routes/sessions.ts");
  const app = new Hono();
  const engine = {
    agentsDir,
    currentSessionPath: null,
    isSessionStreaming: () => false,
    agentIdFromSessionPath: () => BUSINESS_AGENT_ID,
    getAgent: () => ({ agentName: "Hana" }),
    getSessionWorkspaceMount: () => null,
    // 业务 sessionId ↔ 文件路径的生产映射来自真实 SessionManifestStore（SQLite）。
    getSessionManifest: (id: string) => (id === sessionManifest.sessionId ? sessionManifest : null),
    getSessionIdForPath: (p: string) => (p === sessionPath ? sessionManifest.sessionId : null),
    // 现行分支入口（生产 engine 同名方法）：真实 SessionManager.open + 真实 getBranch，
    // 仅加计数/异常跟踪包装，不改变行为。分支读取抛错才会触发 loadSessionHistoryMessages
    // 的兼容 raw-read fallback——getBranchThrows/openThrows 为 0 即证明 fallback 次数为 0。
    openSessionManagerAtCurrentBranch: (p: string, dir: string) => {
      branchTracker.openCalls += 1;
      if (p === sessionPath) branchTracker.openSessionPathCalls += 1;
      let manager: any;
      try {
        manager = SessionManager.open(p, dir);
      } catch (error) {
        branchTracker.openThrows += 1;
        throw error;
      }
      const rawGetBranch = manager.getBranch.bind(manager);
      manager.getBranch = (...args: any[]) => {
        branchTracker.getBranchCalls += 1;
        try {
          return rawGetBranch(...args);
        } catch (error) {
          branchTracker.getBranchThrows += 1;
          throw error;
        }
      };
      return manager;
    },
  };
  app.route("/api", createSessionsRoute(engine));
  return app;
}

// ── 驱动器：镜像用户连续上滑加载更早历史的完整行为 ──

interface DriveStats {
  messageRequests: number;
  returnedRecords: number;
}

async function driveLoadUntilExhausted(
  sessionPath: string,
  pageSize = PAGE_SIZE,
  maxPagesOverride?: number,
): Promise<DriveStats> {
  const displayCount = countDisplayRecords(sessionPath);
  // 页数上限 = 实际 display 总数的理论页数 + 少量诊断余量（A03：不再硬编码 maxPages=200）。
  // 既保留死循环保护，也不会把 10k 夹具合法的第 201 页（ceil(10001/50)=201）误报成失控。
  const maxPages = maxPagesOverride ?? pagesForDisplay(displayCount, pageSize) + 2;
  const before = harness.bodies.length;
  await loadMessages(sessionPath);
  let pages = 1;
  for (;;) {
    const state: any = useStore.getState();
    const session: any = sessionScopedValue(state, state.chatSessions, sessionPath);
    if (!session || !session.hasMore || session.loadingMore) break;
    if (pages >= maxPages) {
      throw new Error(
        `分页失控：display 总数 ${displayCount} 理论 ${pagesForDisplay(displayCount, pageSize)} 页，` +
        `超过上限 ${maxPages} 页仍未耗尽 hasMore`,
      );
    }
    await loadMoreMessages(sessionPath);
    pages += 1;
  }
  const fetched = harness.bodies.slice(before).filter((entry) => entry.url.includes("/api/sessions/messages"));
  let returnedRecords = 0;
  for (const entry of fetched) returnedRecords += entry.body?.messages?.length ?? 0;
  return { messageRequests: fetched.length, returnedRecords };
}

function currentItems(sessionPath: string) {
  const state: any = useStore.getState();
  const session: any = sessionScopedValue(state, state.chatSessions, sessionPath);
  return session?.items ?? [];
}

function assistantMessages(sessionPath: string): any[] {
  return currentItems(sessionPath).filter(
    (item: any) => item.type === "message" && item.data.role === "assistant",
  );
}

function countMissingFinalAnswer(sessionPath: string): number {
  let count = 0;
  for (const item of currentItems(sessionPath)) {
    if (item.type !== "message" || item.data.role !== "assistant") continue;
    for (const block of item.data.blocks || []) {
      if (block.type === "turn_status" && block.status === "missing_final_answer") count += 1;
    }
  }
  return count;
}

describe("长单轮会话历史分页 × Run 连续性", () => {
  let agentsDir: string;
  let sessionPath: string;
  let manifestStore: any;
  let sessionManifest: any;
  let branchTracker: BranchTracker;

  beforeEach(async () => {
    agentsDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-history-page-"));
    sessionPath = path.join(agentsDir, BUSINESS_AGENT_ID, "sessions", "page-target.jsonl");
    fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
    writeLongRunSession(sessionPath);
    // 真实 manifest 存储：业务 sessionId 由生产 SessionManifestStore 生成（sess_… 格式），
    // 与 SDK 文件头 UUID 是两套身份。
    manifestStore = new SessionManifestStore({ dbPath: path.join(agentsDir, "session-manifest.db") });
    sessionManifest = manifestStore.createForPath({
      sessionPath,
      ownerAgentId: BUSINESS_AGENT_ID,
      domain: "desktop",
      kind: "chat",
    });
    branchTracker = createBranchTracker();
    harness.app = await buildApp(agentsDir, sessionPath, sessionManifest, branchTracker);
    harness.bodies = [];
    useStore.setState({ chatSessions: {} } as never);
  });

  afterEach(() => {
    manifestStore?.close();
    manifestStore = null;
    fs.rmSync(agentsDir, { recursive: true, force: true });
  });

  it("T01：固定快照下分页不重叠、单逻辑 Run、无伪无回复状态，且与全量恢复语义等价", async () => {
    const stats = await driveLoadUntilExhausted(sessionPath);

    // 游标必须来自服务端原始页面范围：display 总数 1+N=141 / 每页 50 → 3 次请求、141 条返回、零重叠。
    expect(stats.messageRequests).toBe(3);
    expect(stats.returnedRecords).toBe(1 + RUN_SIZE);

    // 分支身份：业务 sessionId（真实 manifest 存储生成）与 SDK 文件头 UUID 两套身份、各自合法格式。
    expect(sessionManifest.sessionId).toMatch(/^sess_[0-9a-z]+_[0-9a-f]{20}$/);
    expect(SESSION_FILE_HEADER_ID).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    );
    expect(sessionManifest.sessionId).not.toBe(SESSION_FILE_HEADER_ID);
    expect(sessionManifest.currentLocator.path).toBe(sessionPath);

    // 执行路径证明（B06 起）：3 次分页请求全部走目录快路径——不再为热页打开
    // 全量 SessionManager（openSessionPathCalls=0）。legacy 回退必然打开管理器，
    // 计数为 0 即证明零回退（兼容 raw-read fallback 不可达）。此断言点之后才会
    // 发出 all=1 等价性对照请求（强制全量仍走 legacy 分支入口，计数 +1）。
    expect(branchTracker.openSessionPathCalls).toBe(0);
    expect(branchTracker.getBranchCalls).toBe(0);
    expect(branchTracker.openThrows).toBe(0);
    expect(branchTracker.getBranchThrows).toBe(0);

    // 当前分支 parent 链（生产严格校验读取器）：u1 → a1 → r1 → a2 → … → aN，
    // 物理记录 2N 条（1 user + N assistant + N-1 toolResult），ID 全局唯一。
    const branchProjection = readCurrentSessionBranch(sessionPath);
    const lineage = branchProjection.lineage;
    expect(lineage).toHaveLength(2 * RUN_SIZE);
    expect(new Set(lineage.map((entry: any) => entry.id)).size).toBe(lineage.length);
    lineage.forEach((entry: any, index: number) => {
      expect(entry.parentId).toBe(index === 0 ? null : lineage[index - 1].id);
      expect(entry.id).not.toBe(entry.parentId);
    });
    expect(lineage[0].id).toBe("u1");
    expect(branchProjection.selectedLeafId).toBe(`a${RUN_SIZE}`);

    // 同一 Run 只允许一个显示项（含用户消息共 2 个 message item）。
    const assistants = assistantMessages(sessionPath);
    expect(assistants.length).toBe(1);
    const users = currentItems(sessionPath).filter(
      (item: any) => item.type === "message" && item.data.role === "user",
    );
    expect(users.length).toBe(1);

    // 最终答案存在 → 不允许任何 missing_final_answer。
    expect(countMissingFinalAnswer(sessionPath)).toBe(0);

    // 与全量恢复（all=1）的语义等价：块序列（id/type 逐项）与结局一致。
    const allRes = await harness.app.request(
      `/api/sessions/messages?path=${encodeURIComponent(sessionPath)}&all=1`,
    );
    const allData = await allRes.json();
    const allItems = buildItemsFromHistory(allData);
    const pagedRun = assistants[0].data;
    const fullRun = (allItems.find(
      (item: any) => item.type === "message" && item.data.role === "assistant",
    ) as any)?.data;
    expect(fullRun).toBeTruthy();
    expect(pagedRun.id).toBe(fullRun!.id);
    expect(pagedRun.blocks.map((b: any) => [b.id, b.type])).toEqual(
      fullRun!.blocks.map((b: any) => [b.id, b.type]),
    );
    expect(pagedRun.turnProjection?.status).toBe(fullRun!.turnProjection?.status);
    // Run 完整加载后不应残留跨页缝合用的原始事实。
    expect((pagedRun as any).runFacts).toBeUndefined();
  });

  it("T04：重复到达的同一页幂等，不增加显示项或过程块", async () => {
    await driveLoadUntilExhausted(sessionPath);
    const before = currentItems(sessionPath).length;
    const blocksBefore = assistantMessages(sessionPath)[0].data.blocks.length;

    // 直接以 slice 层重放「第 2 页」：从抓到的 bodies 里取非首页重复投影。
    const secondPage = harness.bodies
      .filter((entry: any) => entry.url.includes("/api/sessions/messages") && !entry.url.includes("all=1"))
      .map((entry: any) => entry.body)[1];
    expect(secondPage).toBeTruthy();
    const replayItems = buildItemsFromHistory(secondPage);
    useStore.getState().prependItems(sessionPath, replayItems, false);

    const after = currentItems(sessionPath);
    expect(after.length).toBe(before);
    expect(assistantMessages(sessionPath).length).toBe(1);
    expect(assistantMessages(sessionPath)[0].data.blocks.length).toBe(blocksBefore);
    expect(countMissingFinalAnswer(sessionPath)).toBe(0);
  });

  it("T11：before=0 返回空页并终结分页，不回退到最新页；翻页响应显式携带下一页游标", async () => {
    const zeroRes = await harness.app.request(
      `/api/sessions/messages?path=${encodeURIComponent(sessionPath)}&before=0`,
    );
    expect(zeroRes.status).toBe(200);
    const zero = await zeroRes.json();
    expect(zero.messages).toEqual([]);
    expect(zero.hasMore).toBe(false);
    expect(zero.nextBefore).toBeNull();

    const firstRes = await harness.app.request(
      `/api/sessions/messages?path=${encodeURIComponent(sessionPath)}`,
    );
    const first = await firstRes.json();
    expect(first.hasMore).toBe(true);
    // 游标由服务端原始页面范围决定（字符串化的 display index），而不是归并显示项 id。
    expect(first.nextBefore).toBe("91");

    // 沿 nextBefore 链路走完：页数、记录唯一性与 all=1 一致（页序从新到旧）。
    const seen: string[] = [...(first.messages || []).map((m: any) => m.id)];
    let cursor: string | null = first.nextBefore;
    let pages = 1;
    while (cursor != null) {
      const res = await harness.app.request(
        `/api/sessions/messages?path=${encodeURIComponent(sessionPath)}&before=${cursor}`,
      );
      const page = await res.json();
      seen.push(...(page.messages || []).map((m: any) => m.id));
      cursor = page.nextBefore ?? null;
      pages += 1;
    }
    expect(pages).toBe(3);
    expect(seen.length).toBe(1 + RUN_SIZE);
    expect(new Set(seen).size).toBe(seen.length);
  });

  it("T11b：整页都被前端过滤（隐藏后台通知）时，分页进度仍推进、更早真实历史可达", async () => {
    // 在长 Run 之后追加 55 条隐藏后台通知 user 消息（前端逐条过滤，但占 display 序号）。
    const lines = fs.readFileSync(sessionPath, "utf8").split("\n").filter(Boolean);
    let parent = "a" + RUN_SIZE;
    for (let i = 1; i <= 55; i += 1) {
      const id = `bg${i}`;
      lines.push(
        jsonlLine(id, parent, {
          role: "user",
          content: `<hana-background-result task="t${i}">后台结果 ${i}</hana-background-result>`,
        }),
      );
      parent = id;
    }
    fs.writeFileSync(sessionPath, lines.join("\n"), "utf8");

    const stats = await driveLoadUntilExhausted(sessionPath);

    // 首页（最新 50 条）整页被过滤 → 不允许被静默截断：hasMore 必须继续推进，
    // 最终真实历史（用户输入 + 最终报告）必须可达。
    const users = currentItems(sessionPath).filter(
      (item: any) => item.type === "message" && item.data.role === "user",
    );
    expect(users.length).toBe(1);
    expect(users[0].data.text).toContain("最终报告");
    const assistants = assistantMessages(sessionPath);
    expect(assistants.length).toBe(1);
    expect(countMissingFinalAnswer(sessionPath)).toBe(0);
    // 全部 displayable 记录 = 141 + 55 = 196 → 4 页（50×3 + 46）。
    expect(stats.messageRequests).toBe(4);
  });

  it("T03：任意页大小切页，串联结果与全量恢复同序同集合", async () => {
    const allRes = await harness.app.request(
      `/api/sessions/messages?path=${encodeURIComponent(sessionPath)}&all=1`,
    );
    const all = await allRes.json();
    const allIds = all.messages.map((m: any) => m.id);

    const pagesOfIds: string[][] = [];
    let cursor: string | null = null;
    let guard = 0;
    for (;;) {
      guard += 1;
      if (guard > 100) throw new Error("分页失控");
      const url = `/api/sessions/messages?path=${encodeURIComponent(sessionPath)}&limit=7${
        cursor != null ? `&before=${cursor}` : ""
      }`;
      const res = await harness.app.request(url);
      const page = await res.json();
      pagesOfIds.push((page.messages || []).map((m: any) => m.id));
      if (!page.hasMore || page.nextBefore == null) break;
      cursor = page.nextBefore;
    }
    // 页间从新到旧、页内从旧到新：页序反转后摊平 = 全量恢复的从旧到新序列。
    expect([...pagesOfIds].reverse().flat()).toEqual(allIds);
  });

  it("T12：夹具规模定义——1 文件头 + 1 user + N assistant + (N-1) toolResult，页大小 50 时 1k=21 页、10k=201 页", () => {
    for (const n of [1, 2, 3, RUN_SIZE, 1000, 10000]) {
      const scalePath = path.join(agentsDir, BUSINESS_AGENT_ID, "sessions", `scale-${n}.jsonl`);
      writeLongRunSession(scalePath, n);
      const physicalLines = fs.readFileSync(scalePath, "utf8").split("\n").filter(Boolean);
      // 物理行数 2N+1；display 总数 N+1；页数 = ceil(display/50)。
      expect(physicalLines.length).toBe(2 * n + 1);
      expect(countDisplayRecords(scalePath)).toBe(n + 1);
      expect(pagesForDisplay(n + 1)).toBe(Math.ceil((n + 1) / PAGE_SIZE));
      const header = JSON.parse(physicalLines[0]);
      expect(header.type).toBe("session");
      expect(header.version).toBe(3);
      expect(header.id).toBe(SESSION_FILE_HEADER_ID);
      // 工具结果数量 = N-1，独立 id、parentId 指向对应 assistant。
      const toolResults = physicalLines
        .map((line) => JSON.parse(line))
        .filter((entry: any) => entry.message?.role === "toolResult");
      expect(toolResults.length).toBe(n - 1);
      expect(new Set(toolResults.map((entry: any) => entry.id)).size).toBe(toolResults.length);
      fs.rmSync(scalePath, { force: true });
    }
    // 固定锚点：1k → 21 页，10k → 201 页（驱动器上限 = 理论页数 + 2 不会误报失控）。
    expect(pagesForDisplay(1001)).toBe(21);
    expect(pagesForDisplay(10001)).toBe(201);
  });

  it("T12c：死循环保护保留——页数超限（人为压低上限）时显式报错而非无限翻页", async () => {
    await expect(driveLoadUntilExhausted(sessionPath, PAGE_SIZE, 1)).rejects.toThrow(/分页失控/);
  });

  it.each([
    { trailingNewline: false, label: "不带尾换行" },
    { trailingNewline: true, label: "带尾换行" },
  ])(
    "T12b：合法夹具变体——$label，经真实分支入口分页语义一致",
    async ({ trailingNewline }) => {
      const variantRunSize = 60; // display 61 → 2 页（50+11），快速覆盖两种文件尾形态
      const variantPath = path.join(
        agentsDir,
        BUSINESS_AGENT_ID,
        "sessions",
        `page-variant-${trailingNewline ? "nl" : "nonl"}.jsonl`,
      );
      writeLongRunSession(variantPath, variantRunSize, { trailingNewline });
      expect(fs.readFileSync(variantPath, "utf8").endsWith("\n")).toBe(trailingNewline);
      // 两种变体的物理行数断言一致（尾换行不产生第 2N+2 条记录）。
      expect(fs.readFileSync(variantPath, "utf8").split("\n").filter(Boolean).length).toBe(
        2 * variantRunSize + 1,
      );

      const variantManifest = manifestStore.createForPath({
        sessionPath: variantPath,
        ownerAgentId: BUSINESS_AGENT_ID,
        domain: "desktop",
        kind: "chat",
      });
      const variantTracker = createBranchTracker();
      const variantApp = await buildApp(agentsDir, variantPath, variantManifest, variantTracker);
      const savedApp = harness.app;
      harness.app = variantApp;
      try {
        const stats = await driveLoadUntilExhausted(variantPath);
        expect(stats.messageRequests).toBe(2);
        expect(stats.returnedRecords).toBe(variantRunSize + 1);

        // 语义断言镜像 T01：单逻辑 Run、最终报告可达、无伪无回复状态。
        expect(assistantMessages(variantPath).length).toBe(1);
        const users = currentItems(variantPath).filter(
          (item: any) => item.type === "message" && item.data.role === "user",
        );
        expect(users.length).toBe(1);
        expect(users[0].data.text).toContain("最终报告");
        expect(countMissingFinalAnswer(variantPath)).toBe(0);

        // 分支入口证明（B06 起）+ fallback 次数 0：分页走目录快路径，零管理器打开
        // （legacy 回退必然打开 → 0 即零回退）。
        expect(variantTracker.openSessionPathCalls).toBe(0);
        expect(variantTracker.getBranchThrows).toBe(0);
        expect(variantTracker.openThrows).toBe(0);

        // 生产严格读取器：链形状与 ID 唯一性对两种尾换行形态一致。
        const projection = readCurrentSessionBranch(variantPath);
        expect(projection.lineage).toHaveLength(2 * variantRunSize);
        expect(new Set(projection.lineage.map((entry: any) => entry.id)).size).toBe(
          2 * variantRunSize,
        );
        projection.lineage.forEach((entry: any, index: number) => {
          expect(entry.parentId).toBe(index === 0 ? null : projection.lineage[index - 1].id);
        });
        expect(projection.selectedLeafId).toBe(`a${variantRunSize}`);
      } finally {
        harness.app = savedApp;
      }
    },
  );
});

// ── A03 夹具审计契约（fixture-audit.json 证据源）──
//
// 本 describe 固定 A03 夹具契约：sha256/字节数/各规模数量/页数学/分支身份/执行路径计数。
// 默认只断言不落盘；设置 A03_FIXTURE_AUDIT_OUT=<path> 时额外把机器可验证部分写入该路径，
// 供 artifacts/history-read-directory/fixture-audit.json 汇编（普通 npm test 不产生写副作用）。

describe("A03 夹具审计契约", () => {
  it(
    "夹具 sha256/字节数/数量/页数学/分支身份/执行路径计数符合 A03 定义",
    { timeout: 120_000 },
    async () => {
      const auditDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-history-audit-"));
      let auditStore: any = null;
      try {
        const fixtures: Record<string, any> = {};
        for (const n of [RUN_SIZE, 1000, 10000]) {
          const fixturePath = path.join(auditDir, `run-${n}.jsonl`);
          writeLongRunSession(fixturePath, n);
          const raw = fs.readFileSync(fixturePath);
          const physicalLines = raw.toString("utf8").split("\n").filter(Boolean);
          fixtures[`N=${n}`] = {
            n,
            physicalLines: physicalLines.length,
            displayCount: n + 1,
            pagesAtPageSize50: pagesForDisplay(n + 1),
            bytes: raw.length,
            sha256: createHash("sha256").update(raw).digest("hex"),
            trailingNewline: false,
          };
        }
        expect(fixtures["N=1000"].pagesAtPageSize50).toBe(21);
        expect(fixtures["N=10000"].pagesAtPageSize50).toBe(201);
        expect(fixtures["N=140"].physicalLines).toBe(2 * RUN_SIZE + 1);

        // 分支身份：真实 manifest 存储生成业务 sessionId，与 SDK 文件头 UUID 不同且格式合法。
        auditStore = new SessionManifestStore({ dbPath: path.join(auditDir, "session-manifest.db") });
        const identityPath = path.join(auditDir, BUSINESS_AGENT_ID, "sessions", "audit-target.jsonl");
        fs.mkdirSync(path.dirname(identityPath), { recursive: true });
        writeLongRunSession(identityPath);
        const identityManifest = auditStore.createForPath({
          sessionPath: identityPath,
          ownerAgentId: BUSINESS_AGENT_ID,
          domain: "desktop",
          kind: "chat",
        });
        expect(identityManifest.sessionId).toMatch(/^sess_[0-9a-z]+_[0-9a-f]{20}$/);
        expect(identityManifest.sessionId).not.toBe(SESSION_FILE_HEADER_ID);

        // 执行路径计数：真实路由 + 真实分支入口完整驱动 N=140 夹具。
        const tracker = createBranchTracker();
        const auditApp = await buildApp(auditDir, identityPath, identityManifest, tracker);
        const savedApp = harness.app;
        harness.app = auditApp;
        useStore.setState({ chatSessions: {} } as never);
        let stats: DriveStats;
        try {
          stats = await driveLoadUntilExhausted(identityPath);
        } finally {
          harness.app = savedApp;
        }
        expect(stats.messageRequests).toBe(3);
        expect(stats.returnedRecords).toBe(RUN_SIZE + 1);
        // B06 起：分页请求走目录快路径，零 SessionManager 打开——legacy 回退必然
        // 打开管理器，计数为 0 即证明零回退（A03 的分支路径证明由目录路径承接）。
        expect(tracker.openSessionPathCalls).toBe(0);
        expect(tracker.openCalls).toBe(0);
        expect(tracker.getBranchCalls).toBe(0);
        expect(tracker.openThrows).toBe(0);
        expect(tracker.getBranchThrows).toBe(0);

        const audit = {
          task: "A03",
          generatedAt: new Date().toISOString(),
          seed: 20260910,
          determinism:
            "夹具完全由索引派生（内容模板 × 序号），无随机数；seed 为任务书 A05 固定审计种子，仅作标识",
          construction:
            "writeLongRunSession()：1×{type:session,version:3,id:<SDK UUID>} 文件头 + 1×user(u1,parent=null) + N×assistant(a<i>) + (N-1)×toolResult(r<i>,toolCallId=tu-<i>)；链 u1→a1→r1→a2→r2→…→aN(parent=r<N-1>)；每条 toolResult 独立 id、parentId 指向对应 assistant",
          fixtures,
          branchIdentity: {
            businessSessionId: identityManifest.sessionId,
            businessSessionIdFormat: "sess_<base36 ts>_<20 hex>（core/session-manifest/id.ts generateSessionId）",
            sdkHeaderId: SESSION_FILE_HEADER_ID,
            sdkHeaderIdFormat: "UUID v4 形态（lib/pi-sdk SessionManager 文件头身份）",
            distinct: true,
            manifestStore: "core/session-manifest/store.ts SessionManifestStore（真实 SQLite 存储）",
          },
          executionPath: {
            branchEntry:
              "B06 起：普通分页请求走 server/history-read 目录快路径（scan→branch→project），零 SessionManager 打开；all=1 对照请求仍走 legacy 分支入口 engine.openSessionManagerAtCurrentBranch → SessionManager.open(path, dir).getBranch()",
            messageRequests: stats.messageRequests,
            openCalls: tracker.openCalls,
            openSessionPathCalls: tracker.openSessionPathCalls,
            getBranchCalls: tracker.getBranchCalls,
            openThrows: tracker.openThrows,
            getBranchThrows: tracker.getBranchThrows,
            fallbackReads: 0,
            fallbackProof:
              "loadSessionHistoryMessages 仅在分支读取 try 块抛错时进入兼容 raw-read fallback；openThrows=0 且 getBranchThrows=0 证明 legacy 路径 fallback 次数为 0；分页请求 openCalls=0 证明其未落入 legacy（目录路径承接）",
          },
        };
        const outputPath = process.env.A03_FIXTURE_AUDIT_OUT;
        if (outputPath) {
          fs.mkdirSync(path.dirname(path.resolve(outputPath)), { recursive: true });
          fs.writeFileSync(outputPath, JSON.stringify(audit, null, 2) + "\n", "utf8");
        }
      } finally {
        auditStore?.close();
        fs.rmSync(auditDir, { recursive: true, force: true });
      }
    },
  );
});
