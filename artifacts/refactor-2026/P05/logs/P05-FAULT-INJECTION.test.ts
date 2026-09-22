// P05-T07 负向证明（故障注入反例）——证明既有等价/连续性断言真的会挡错。
// 归属：本文件匹配 vitest 默认 include（npm test 的 --exclude 不含 artifacts/），
//   已并入默认测试集并在全量套件真实执行（P05-T08-full-suite：2/2 绿）；
//   开发期曾在隔离临时根（/tmp/p05x-fault-injection）迭代，终态为仓库内默认集成员。
// 可移植性：vi.mock/import 一律为相对本文件的仓库相对路径（../../../../ = 仓库根），
//   无机器绝对路径，提交后 CI/其他机器可直接解析。
//
// FI-1（T07-4a）：历史侧段重导出被篡改——commentary 一律当 final_answer。
//   断言 live(真实链) ≠ history(被篡改链) 的语义 profile ——即 live-history-reserved-tag-parity
//   的等价断言在此故障下必然失败（能捕获"把 commentary 当 final"的同类不一致）。
// FI-2（T07-4b）：分页游标被误用为"源数组下标/UI 列表 ID"（忽略 displayable 过滤），
//   而不是 display 序号读取位置。断言翻页拼接 ≠ 全量恢复 ——即 history-pagination-run-continuity
//   T01 的"分页串联与全量恢复语义等价"断言在此故障下必然失败。
// @vitest-environment jsdom

import { describe, expect, it, vi } from "vitest";

// ── FI-1 ────────────────────────────────────────────────────────────────────
// 篡改历史段语义：commentary → final_answer（模拟"来源错误地把 commentary 当 final"）。
// vi.mock 需要字面量路径（hoisting 先于变量初始化）。
vi.mock("../../../../shared/assistant-semantic-segments.ts", async (importOriginal) => {
  const orig = await importOriginal<typeof import("../../../../shared/assistant-semantic-segments.ts")>();
  return {
    ...orig,
    extractPersistedAssistantSemanticSegments: (content: unknown, ordinal = 1) =>
      orig.extractPersistedAssistantSemanticSegments(content, ordinal).map((segment) => ({
        ...segment,
        semanticPhase: "final_answer" as const,
      })),
  };
});

// FI-1 依赖（在 mock 生效后导入）
import { createChatRoute } from "../../../../server/routes/chat.ts";
import { streamBufferManager } from "../../../../desktop/src/react/hooks/use-stream-buffer.ts";
import { useStore } from "../../../../desktop/src/react/stores/index.ts";
import { readLiveAssistantMessage } from "../../../../desktop/src/react/stores/live-turn-store.ts";
import { buildItemsFromHistory } from "../../../../desktop/src/react/utils/history-builder.ts";
import { extractPersistedAssistantSemanticSegments } from "../../../../shared/assistant-semantic-segments.ts";

// ── FI-2（独立 mock：篡改页边界解析——beforeId 当作源数组下标而非 display 边界）──
vi.mock("../../../../server/history-read/page.ts", async (importOriginal) => {
  const orig = await importOriginal<typeof import("../../../../server/history-read/page.ts")>();
  return {
    ...orig,
    resolveHistoryPageBounds: (
      sourceMessages: Array<Record<string, unknown>>,
      opts: { beforeId: number | null; limit: number; forceAll: boolean },
    ) => {
      if (opts.forceAll) {
        let totalAll = 0;
        for (const m of sourceMessages) totalAll += 1;
        return { total: totalAll, startIdx: 0, endIdx: totalAll, hasMore: false };
      }
      // 故障：把 beforeId 解释为“全部源记录数组的下标”（UI 列表式 ID），
      // 忽略 isDisplayableHistoryMessage 过滤——正确语义是 display 序号边界。
      const total = sourceMessages.length;
      const endIdx = opts.beforeId != null && Number.isFinite(opts.beforeId) && opts.beforeId >= 0
        ? Math.min(opts.beforeId, total)
        : total;
      const startIdx = Math.max(0, endIdx - opts.limit);
      return { total, startIdx, endIdx, hasMore: startIdx > 0 };
    },
  };
});

import { projectFullHistoryPage } from "../../../../server/history-read/index.ts";

// ── FI-1 器件（复刻 live-history-reserved-tag-parity 的真实链路） ─────────────
const SESSION_PATH = "/tmp/p05-fi-parity.jsonl";

function makeServerHarness(sessionPath: string) {
  let createHandlers: any;
  let subscriber: any;
  const upgradeWebSocket = vi.fn((factory: any) => {
    createHandlers = factory;
    return () => new Response(null);
  });
  const hub = {
    subscribe: vi.fn((fn: any) => { subscriber = fn; }),
    send: vi.fn(async () => {}),
    eventBus: { emit: vi.fn() },
  };
  const engine = {
    agentName: "Ming",
    abortAllStreaming: vi.fn(async () => {}),
    getSessionByPath: vi.fn(() => ({ entries: [] })),
    isSessionStreaming: vi.fn(() => false),
    isSessionSwitching: vi.fn(() => false),
    steerSession: vi.fn(() => false),
    slashDispatcher: null,
  };
  createChatRoute(engine, hub, { upgradeWebSocket });
  const handlers = createHandlers({});
  const ws = { readyState: 1, send: vi.fn() };
  handlers.onOpen({}, ws);
  return {
    subscriber,
    payloads: () => ws.send.mock.calls.map(([raw]: [string]) => JSON.parse(raw)),
    close: () => handlers.onClose({}, ws),
  };
}

const LIVE_EVENT_TYPES = new Set([
  "mood_start", "mood_text", "mood_end",
  "thinking_start", "thinking_delta", "thinking_end",
  "assistant_segment_start", "assistant_segment_delta", "assistant_segment_end",
  "text_delta", "tool_start", "tool_end", "content_block",
  "model_turn_start", "model_turn_end", "assistant_run_end",
]);

/** OpenAI Responses 双相位场景：content[0]=commentary、content[1]=final。 */
function emitCommentaryThenFinal(subscriber: any, sessionPath: string) {
  const commentarySignature = JSON.stringify({ v: 1, id: "msg_commentary", phase: "commentary" });
  const finalSignature = JSON.stringify({ v: 1, id: "msg_final", phase: "final_answer" });
  const partial = {
    role: "assistant",
    api: "openai-codex-responses",
    provider: "openai-codex",
    content: [{ type: "text", text: "I need to inspect the current state." }],
  };
  const finalContent = [
    { type: "text", text: "I need to inspect the current state.", textSignature: commentarySignature },
    { type: "text", text: "已经查到状态。", textSignature: finalSignature },
  ];
  subscriber?.({ type: "agent_start" }, sessionPath);
  subscriber?.({ type: "turn_start" }, sessionPath);
  subscriber?.({ type: "message_start", message: partial }, sessionPath);
  subscriber?.({
    type: "message_update", message: partial,
    assistantMessageEvent: { type: "text_delta", contentIndex: 0, delta: "I need to inspect the current state.", partial },
  }, sessionPath);
  subscriber?.({
    type: "message_update", message: { ...partial, content: [finalContent[0]] },
    assistantMessageEvent: { type: "text_end", contentIndex: 0, content: "I need to inspect the current state.", partial: { ...partial, content: [finalContent[0]] } },
  }, sessionPath);
  subscriber?.({
    type: "message_update", message: { ...partial, content: finalContent },
    assistantMessageEvent: { type: "text_delta", contentIndex: 1, delta: "已经查到状态。", partial: { ...partial, content: finalContent } },
  }, sessionPath);
  subscriber?.({
    type: "message_update", message: { ...partial, content: finalContent },
    assistantMessageEvent: { type: "text_end", contentIndex: 1, content: "已经查到状态。", partial: { ...partial, content: finalContent } },
  }, sessionPath);
  subscriber?.({ type: "message_end", message: { ...partial, content: finalContent } }, sessionPath);
  subscriber?.({ type: "turn_end", message: { ...partial, content: finalContent }, toolResults: [] }, sessionPath);
  subscriber?.({ type: "agent_settled" }, sessionPath);
}

function textProfiles(blocks: any[]) {
  return blocks
    .filter((block) => block && block.type === "text")
    .map((block) => ({ surfaceRole: block.surfaceRole, source: block.source }));
}

describe("P05 FI-1：历史侧 commentary→final 篡改会被等价断言捕获", () => {
  it("被篡改的历史投影与真实 live 投影语义 profile 不相等", () => {
    const h = makeServerHarness(SESSION_PATH);
    emitCommentaryThenFinal(h.subscriber, SESSION_PATH);

    // live：真实 chat 路由 → WS → streamBufferManager（复刻 parity 测试的会话复位）
    streamBufferManager.clearAll();
    useStore.setState({
      currentSessionId: null,
      currentSessionPath: null,
      sessions: [],
      sessionLocatorsById: {},
      streamingSessions: [],
    } as never);
    useStore.getState().clearSession(SESSION_PATH);
    useStore.getState().initSession(SESSION_PATH, [
      { type: "message", data: { id: "u1", role: "user", text: "查状态" } },
    ], false);
    for (const payload of h.payloads()) {
      if (LIVE_EVENT_TYPES.has(payload.type)) streamBufferManager.handle(payload);
    }
    const state: any = useStore.getState();
    const items = state.chatSessions[SESSION_PATH]?.items ?? [];
    const assistant = items.find((entry: any) => entry.type === "message" && entry.data.role === "assistant");
    const live = readLiveAssistantMessage(SESSION_PATH, assistant.data.id);
    const liveTexts = textProfiles(live ? live.blocks : (assistant.data.blocks || []));

    // history：持久化形状正确（textSignature 已标注），但段重导出被 FI 篡改
    const commentarySignature = JSON.stringify({ v: 1, id: "msg_commentary", phase: "commentary" });
    const finalSignature = JSON.stringify({ v: 1, id: "msg_final", phase: "final_answer" });
    const content = [
      { type: "text", text: "I need to inspect the current state.", textSignature: commentarySignature },
      { type: "text", text: "已经查到状态。", textSignature: finalSignature },
    ];
    const historyItems = buildItemsFromHistory({
      messages: [
        { id: "0", entryId: "entry-user-1", role: "user", content: "查状态" },
        {
          id: "1", entryId: "entry-assistant-1", role: "assistant", content,
          turnInputEntryId: "entry-user-1",
          assistantSegments: extractPersistedAssistantSemanticSegments(content, 1),
        },
      ],
    } as never);
    const historyAssistant = historyItems.find(
      (entry: any) => entry.type === "message" && entry.data.role === "assistant",
    );
    const historyTexts = historyAssistant?.type === "message" ? textProfiles(historyAssistant.data.blocks || []) : [];

    // 未篡改基线（live 侧真值）：commentary 是 process、final 是 answer
    expect(liveTexts).toEqual([
      { surfaceRole: "process", source: "I need to inspect the current state." },
      { surfaceRole: "answer", source: "已经查到状态。" },
    ]);
    // 故障生效证明：被篡改的历史投影把 commentary 升为 answer
    expect(historyTexts.find((t: any) => t.source.includes("inspect"))?.surfaceRole).toBe("answer");
    // 等价断言捕获故障：live ≠ history（parity 测试的 toEqual 在此必然红）
    expect(historyTexts).not.toEqual(liveTexts);

    h.close();
  });
});

// ── FI-2 器件：带非 displayable 记录的夹具，翻页拼接 vs 全量恢复 ───────────────
describe("P05 FI-2：游标被误用为源数组下标时连续性断言捕获", () => {
  it("故障页边界使翻页拼接 ≠ 全量恢复（正确实现下两者必须相等）", async () => {
    // 25 条 displayable + 5 条非 displayable（custom 展示隐藏）交织；
    // display 序号空间 ≠ 源数组下标空间，正是"UI 列表 ID ≠ 读取位置"的分歧点。
    const sourceMessages: any[] = [];
    for (let i = 0; i < 30; i += 1) {
      const hidden = i % 6 === 5;
      if (i % 2 === 0) {
        sourceMessages.push({ id: `u${i}`, role: "user", content: `用户消息 ${i}` });
      } else if (hidden) {
        sourceMessages.push({ id: `h${i}`, role: "custom", customType: "hidden_note", display: false, message: { role: "custom" } });
      } else {
        sourceMessages.push({ id: `a${i}`, role: "assistant", content: [{ type: "text", text: `回复 ${i}` }] });
      }
    }

    const sanitize = (value: string) => value;
    const full = await projectFullHistoryPage({} as any, {
      sessionPath: null, sourceMessages, beforeId: null, limit: 50, forceAll: true,
      sanitizeVisibleContent: sanitize,
    });
    const displayTotal = full.messages.length; // 25（15 user + 10 assistant）
    const PAGE_SIZE = 5;
    const expectedPages = Math.ceil(displayTotal / PAGE_SIZE); // 正确语义：5 页

    // 故障边界下逐页拼接（计数每页 display 记录数与总页数）
    const pagedIds: string[] = [];
    const perPageCounts: number[] = [];
    let beforeId: number | null = null;
    let guard = 0;
    let pages = 0;
    for (;;) {
      guard += 1;
      if (guard > 50) throw new Error("翻页失控");
      const page = await projectFullHistoryPage({} as any, {
        sessionPath: null, sourceMessages, beforeId, limit: PAGE_SIZE, forceAll: false,
        sanitizeVisibleContent: sanitize,
      });
      pages += 1;
      perPageCounts.push(page.messages.length);
      for (const message of page.messages) pagedIds.push(message.id);
      if (!page.hasMore || page.nextBefore == null) break;
      beforeId = Number(page.nextBefore);
    }

    // 故障生效证明 1：首页（最新页）按 display 语义必须满页（5 条），
    // 源数组下标语义在含隐藏记录的会话里窗口跨错记录 → 页欠填。
    expect(perPageCounts[0]).toBeLessThan(PAGE_SIZE);
    // 故障生效证明 2：总页数偏离 display 页数学（T12 页数学断言口径）。
    expect(pages).not.toBe(expectedPages);
  });
});
