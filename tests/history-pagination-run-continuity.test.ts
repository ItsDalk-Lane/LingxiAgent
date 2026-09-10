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

// ── fixture ──

const RUN_SIZE = 140;

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
 * 长单轮 fixture：1 条可见用户输入 + 同 Run 的 RUN_SIZE 条助手记录。
 * 前 RUN_SIZE-1 条只有 reasoning + tool_use（过程 only），最后一条带最终答案。
 * 每个工具调用后跟一条 toolResult（不占 display 序号，验证 blocks afterIndex 重映射）。
 */
function writeLongRunSession(sessionPath: string, runSize = RUN_SIZE): void {
  const lines: string[] = [
    JSON.stringify({
      type: "session",
      version: 3,
      id: "sess_page",
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
    parent = `a${i}`;
    if (!isFinal) {
      lines.push(
        jsonlLine(parent, parent, {
          role: "toolResult",
          toolCallId: `tu-${i}`,
          toolName: "read_file",
          content: `文件内容 ${i}`,
        }),
      );
    }
  }
  fs.writeFileSync(sessionPath, lines.join("\n"), "utf8");
}

async function buildApp(agentsDir: string, sessionPath: string) {
  const { createSessionsRoute } = await import("../server/routes/sessions.ts");
  const app = new Hono();
  const engine = {
    agentsDir,
    currentSessionPath: null,
    isSessionStreaming: () => false,
    agentIdFromSessionPath: () => "hana",
    getAgent: () => ({ agentName: "Hana" }),
    getSessionWorkspaceMount: () => null,
    getSessionManifest: (id: string) =>
      id === "sess_page" ? { currentLocator: { path: sessionPath } } : null,
  };
  app.route("/api", createSessionsRoute(engine));
  return app;
}

// ── 驱动器：镜像用户连续上滑加载更早历史的完整行为 ──

interface DriveStats {
  messageRequests: number;
  returnedRecords: number;
}

async function driveLoadUntilExhausted(sessionPath: string, maxPages = 200): Promise<DriveStats> {
  const before = harness.bodies.length;
  await loadMessages(sessionPath);
  let pages = 1;
  for (;;) {
    const state: any = useStore.getState();
    const session: any = sessionScopedValue(state, state.chatSessions, sessionPath);
    if (!session || !session.hasMore || session.loadingMore) break;
    if (pages >= maxPages) throw new Error(`分页失控：超过 ${maxPages} 页仍未耗尽 hasMore`);
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

  beforeEach(async () => {
    agentsDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-history-page-"));
    sessionPath = path.join(agentsDir, "hana", "sessions", "page-target.jsonl");
    fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
    writeLongRunSession(sessionPath);
    harness.app = await buildApp(agentsDir, sessionPath);
    harness.bodies = [];
    useStore.setState({ chatSessions: {} } as never);
  });

  afterEach(() => {
    fs.rmSync(agentsDir, { recursive: true, force: true });
  });

  it("T01：固定快照下分页不重叠、单逻辑 Run、无伪无回复状态，且与全量恢复语义等价", async () => {
    const stats = await driveLoadUntilExhausted(sessionPath);

    // 游标必须来自服务端原始页面范围：141 条记录 / 每页 50 → 3 次请求、141 条返回、零重叠。
    expect(stats.messageRequests).toBe(3);
    expect(stats.returnedRecords).toBe(1 + RUN_SIZE);

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
});
