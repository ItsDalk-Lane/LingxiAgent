// @vitest-environment jsdom

import { beforeEach, describe, expect, it, vi } from "vitest";
import { createChatRoute } from "../server/routes/chat.ts";
import { streamBufferManager } from "../desktop/src/react/hooks/use-stream-buffer.ts";
import { useStore } from "../desktop/src/react/stores/index.ts";
import { readLiveAssistantMessage } from "../desktop/src/react/stores/live-turn-store.ts";
import { buildItemsFromHistory } from "../desktop/src/react/utils/history-builder.ts";
import { extractPersistedAssistantSemanticSegments } from "../shared/assistant-semantic-segments.ts";

/**
 * Live = History 语义等价测试（任务书 §27 / 不变量 5）。
 *
 * 同一语义输入走两条真实路径：
 *   Live：服务端 createChatRoute 真实事件链 → WS payloads → streamBufferManager 实时投影
 *   History：对应的持久化数据 → extractPersistedAssistantSemanticSegments（与 history API
 *             同源）→ buildItemsFromHistory 历史投影
 * 最终语义块必须等价：mood 内容、thinking 内容、answer 文本、tool 名称/状态、
 * 块序列（非 mood 块）、turn status 全部一致；answer 通道不得出现保留标签。
 *
 * 说明：实时路径把一个 turn 的多段 mood 聚合为单个 mood block（\n\n 分隔），历史路径
 * 按 assistant message 各产一个 mood block——这是既有产品聚合差异（§27 允许），
 * 因此 mood 比较内容总序列而非块数。
 */

const PATH = "/tmp/live-history-parity.jsonl";
const RESERVED_TAG_PATTERN = /<\/?(?:mood|pulse|reflect|think|thinking)>/;

/** 实时路径消费的流事件白名单（与 ws-message-handler 路由到 streamBufferManager 的一致）。 */
const LIVE_EVENT_TYPES = new Set([
  "mood_start", "mood_text", "mood_end",
  "thinking_start", "thinking_delta", "thinking_end",
  "assistant_segment_start", "assistant_segment_delta", "assistant_segment_end",
  "text_delta", "card_start", "card_text", "card_end",
  "tool_start", "tool_end", "content_block", "deferred_result",
  "model_turn_start", "model_turn_end", "assistant_run_end",
]);

function makeServerHarness(sessionPath: string) {
  let createHandlers;
  let subscriber;
  const upgradeWebSocket = vi.fn((factory) => {
    createHandlers = factory;
    return () => new Response(null);
  });
  const hub = {
    subscribe: vi.fn((fn) => { subscriber = fn; }),
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
  const payloads = () => ws.send.mock.calls.map(([raw]) => JSON.parse(raw));
  return { subscriber, sessionPath, payloads };
}

function assistantMessage(raw: string) {
  return { role: "assistant", api: "anthropic-messages", content: [{ type: "text", text: raw }] };
}

/** 场景一（§27）：mood-only → generate-image → mood + final answer */
function emitImageToolScenario(subscriber, sessionPath) {
  const msg1 = assistantMessage("<mood>准备生成图片</mood>");
  subscriber?.({ type: "agent_start" }, sessionPath);
  subscriber?.({ type: "turn_start" }, sessionPath);
  subscriber?.({ type: "message_start", message: msg1 }, sessionPath);
  subscriber?.({
    type: "message_update", message: msg1,
    assistantMessageEvent: { type: "text_delta", contentIndex: 0, delta: "<mood>准备生成图片</mood>", partial: msg1 },
  }, sessionPath);
  subscriber?.({
    type: "message_update", message: msg1,
    assistantMessageEvent: { type: "text_end", contentIndex: 0, content: "<mood>准备生成图片</mood>", partial: msg1 },
  }, sessionPath);
  subscriber?.({ type: "message_end", message: msg1 }, sessionPath);
  subscriber?.({ type: "tool_execution_start", toolCallId: "img-1", toolName: "generate-image", args: {} }, sessionPath);
  subscriber?.({
    type: "tool_execution_end", toolCallId: "img-1", toolName: "generate-image",
    result: { content: [{ type: "text", text: "ok" }] }, isError: false,
  }, sessionPath);
  subscriber?.({ type: "message_start", message: { role: "toolResult" } }, sessionPath);
  subscriber?.({ type: "message_end", message: { role: "toolResult" } }, sessionPath);
  subscriber?.({ type: "turn_end", message: msg1, toolResults: [] }, sessionPath);

  const msg2 = assistantMessage("<mood>图片生成请求已提交</mood>完成。");
  subscriber?.({ type: "turn_start" }, sessionPath);
  subscriber?.({ type: "message_start", message: msg2 }, sessionPath);
  subscriber?.({
    type: "message_update", message: msg2,
    assistantMessageEvent: { type: "text_delta", contentIndex: 0, delta: "<mood>图片生成请求已提交</mood>完成。", partial: msg2 },
  }, sessionPath);
  subscriber?.({
    type: "message_update", message: msg2,
    assistantMessageEvent: { type: "text_end", contentIndex: 0, content: "<mood>图片生成请求已提交</mood>完成。", partial: msg2 },
  }, sessionPath);
  subscriber?.({ type: "message_end", message: msg2 }, sessionPath);
  subscriber?.({ type: "turn_end", message: msg2, toolResults: [] }, sessionPath);
  subscriber?.({ type: "agent_settled" }, sessionPath);
}

/** 场景二（§20）：mood-only，无工具、无 final answer */
function emitMoodOnlyScenario(subscriber, sessionPath) {
  const msg = assistantMessage("<mood>A</mood>");
  subscriber?.({ type: "agent_start" }, sessionPath);
  subscriber?.({ type: "turn_start" }, sessionPath);
  subscriber?.({ type: "message_start", message: msg }, sessionPath);
  subscriber?.({
    type: "message_update", message: msg,
    assistantMessageEvent: { type: "text_delta", contentIndex: 0, delta: "<mood>A</mood>", partial: msg },
  }, sessionPath);
  subscriber?.({
    type: "message_update", message: msg,
    assistantMessageEvent: { type: "text_end", contentIndex: 0, content: "<mood>A</mood>", partial: msg },
  }, sessionPath);
  subscriber?.({ type: "message_end", message: msg }, sessionPath);
  subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);
  subscriber?.({ type: "agent_settled" }, sessionPath);
}

function userItem(id: string, text: string) {
  return { type: "message" as const, data: { id, role: "user" as const, text } };
}

function resetLiveSession() {
  streamBufferManager.clearAll();
  useStore.setState({
    currentSessionId: null,
    currentSessionPath: null,
    sessions: [],
    sessionLocatorsById: {},
    streamingSessions: [],
  } as never);
  useStore.getState().clearSession(PATH);
  useStore.getState().initSession(PATH, [userItem("u1", "生成一张图")], false);
}

/** 把服务端 WS payloads 喂给真实实时投影管线，返回提交后的 assistant blocks。 */
function projectLive(payloads) {
  resetLiveSession();
  for (const payload of payloads) {
    if (LIVE_EVENT_TYPES.has(payload.type)) streamBufferManager.handle(payload);
  }
  const state = useStore.getState() as unknown as {
    chatSessions: Record<string, { items?: Array<{ type: string; data: { id: string; role: string; blocks?: unknown[] } }> }>;
  };
  const items = state.chatSessions[PATH]?.items ?? [];
  const assistant = items.find((entry) => entry.type === "message" && entry.data.role === "assistant");
  if (!assistant) return [];
  const live = readLiveAssistantMessage(PATH, assistant.data.id);
  return live ? [...live.blocks] : (assistant.data.blocks || []);
}

interface SemanticProfile {
  sequence: string[];
  moods: string[];
  answers: Array<{ surfaceRole: unknown; semanticPhase: unknown; source: unknown }>;
  tools: Array<{ name: unknown }>;
  turnStatus: unknown[];
}

/** 语义归一化：mood 聚合差异按 §27 豁免（比内容总序列），非语义字段（id/timestamp）剥离。 */
function semanticProfile(blocks): SemanticProfile {
  const significant = blocks.filter((block) => (
    ["mood", "thinking", "text", "tool_group", "turn_status"].includes(block.type)
  ));
  return {
    sequence: significant
      .filter((block) => block.type !== "mood")
      .map((block) => (block.type === "text"
        ? `text:${block.surfaceRole ?? "none"}`
        : block.type)),
    moods: significant
      .filter((block) => block.type === "mood")
      .flatMap((block) => String(block.text || "").split("\n\n").map((part) => part.trim()).filter(Boolean)),
    answers: significant
      .filter((block) => block.type === "text" && block.surfaceRole === "answer")
      .map((block) => ({
        surfaceRole: block.surfaceRole,
        semanticPhase: block.semanticPhase ?? null,
        source: block.source ?? null,
      })),
    tools: significant
      .filter((block) => block.type === "tool_group")
      .flatMap((block) => (block.tools || []).map((tool) => ({ name: tool.name }))),
    turnStatus: significant
      .filter((block) => block.type === "turn_status")
      .map((block) => block.status),
  };
}

function expectNoReservedTagInAnswers(blocks) {
  for (const block of blocks) {
    if (block.type === "text" && block.surfaceRole === "answer") {
      expect(String(block.source ?? "")).not.toMatch(RESERVED_TAG_PATTERN);
    }
  }
}

describe("live = history 语义等价（保留协议不泄漏）", () => {
  beforeEach(() => {
    streamBufferManager.clearAll();
  });

  it("场景一：mood-only → generate-image → mood + final answer，两条路径语义一致", () => {
    const { subscriber, sessionPath, payloads } = makeServerHarness(PATH);
    emitImageToolScenario(subscriber, sessionPath);

    const liveBlocks = projectLive(payloads());
    const historyItems = buildItemsFromHistory({
      messages: [
        { id: "0", entryId: "entry-user-1", role: "user", content: "生成一张图" },
        {
          id: "1",
          entryId: "entry-assistant-1",
          role: "assistant",
          content: "<mood>准备生成图片</mood>",
          turnInputEntryId: "entry-user-1",
          assistantSegments: extractPersistedAssistantSemanticSegments(
            [{ type: "text", text: "<mood>准备生成图片</mood>" }],
            1,
          ),
          toolCalls: [{ id: "img-1", name: "generate-image", status: "succeeded" }],
        },
        {
          id: "2",
          entryId: "entry-assistant-2",
          role: "assistant",
          content: "<mood>图片生成请求已提交</mood>完成。",
          turnInputEntryId: "entry-user-1",
          assistantSegments: extractPersistedAssistantSemanticSegments(
            [{ type: "text", text: "<mood>图片生成请求已提交</mood>完成。" }],
            2,
          ),
        },
      ],
    } as never);
    const historyAssistant = historyItems.find(
      (entry) => entry.type === "message" && entry.data.role === "assistant",
    );
    const historyBlocks = historyAssistant?.type === "message" ? (historyAssistant.data.blocks || []) : [];

    const liveProfile = semanticProfile(liveBlocks);
    const historyProfile = semanticProfile(historyBlocks);

    expect(liveProfile.moods).toEqual(["准备生成图片", "图片生成请求已提交"]);
    expect(liveProfile.answers).toEqual([
      { surfaceRole: "answer", semanticPhase: "final_answer", source: "完成。" },
    ]);
    expect(liveProfile.tools).toEqual([{ name: "generate-image" }]);
    // Live = History：归一化语义 profile 完全一致
    expect(liveProfile).toEqual(historyProfile);
    expectNoReservedTagInAnswers(liveBlocks);
    expectNoReservedTagInAnswers(historyBlocks);
  });

  it("场景二：mood-only 无 final answer，两条路径都得到 missing_final_answer 且无假正文段", () => {
    const { subscriber, sessionPath, payloads } = makeServerHarness(PATH);
    emitMoodOnlyScenario(subscriber, sessionPath);

    const liveBlocks = projectLive(payloads());
    const historyItems = buildItemsFromHistory({
      messages: [
        { id: "0", entryId: "entry-user-1", role: "user", content: "在吗" },
        {
          id: "1",
          entryId: "entry-assistant-1",
          role: "assistant",
          content: "<mood>A</mood>",
          turnInputEntryId: "entry-user-1",
          assistantSegments: extractPersistedAssistantSemanticSegments(
            [{ type: "text", text: "<mood>A</mood>" }],
            1,
          ),
        },
      ],
    } as never);
    const historyAssistant = historyItems.find(
      (entry) => entry.type === "message" && entry.data.role === "assistant",
    );
    const historyBlocks = historyAssistant?.type === "message" ? (historyAssistant.data.blocks || []) : [];

    const liveProfile = semanticProfile(liveBlocks);
    const historyProfile = semanticProfile(historyBlocks);

    expect(liveProfile.moods).toEqual(["A"]);
    expect(liveProfile.answers).toEqual([]);
    // 空 text segment 不得豁免 missing_final_answer（§20.4），两条路径一致
    expect(liveProfile.turnStatus).toEqual(["missing_final_answer"]);
    expect(liveProfile).toEqual(historyProfile);
    expectNoReservedTagInAnswers(liveBlocks);
    expectNoReservedTagInAnswers(historyBlocks);
  });
});
