import { describe, expect, it, vi } from "vitest";
import { createChatRoute } from "../server/routes/chat.ts";

/**
 * 实时流式 <mood>/<think> 内部协议经 text_end raw fallback 二次回流的回归测试。
 *
 * 根因：同一段 raw assistant text 先经 text_delta 进入 ReservedTagPipeline（全部识别为
 * mood/think，clean visible text 为空），随后 text_end 又携带同一 raw content。旧代码只在
 * 「未处理过」时才清空 raw content，已处理过反而把 raw 原样交给 AssistantEventNormalizer；
 * normalizer 在没有 text segment 时新建 segment 并把 raw endText 当成 final answer 输出。
 *
 * 不变量：任何已经被 ReservedTagPipeline 解释过的 raw assistant text，从此永久失去作为
 * 正文 fallback 的资格——无论 normalizer 从 event.content、event.partial.content[].text
 * 还是 fallbackMessage.content[].text 读取，都拿不到 raw source。
 */

const RESERVED_TAG_PATTERN = /<\/?(?:mood|pulse|reflect|think|thinking)>/;

function makeHarness(sessionPath = "/tmp/reserved-raw-fallback.jsonl") {
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

function assistantMessage(textBlocks: string[], extra: Record<string, unknown> = {}) {
  return {
    role: "assistant",
    api: "anthropic-messages",
    content: textBlocks.map((text) => ({ type: "text", text })),
    ...extra,
  };
}

function emit(subscriber, sessionPath, message, assistantMessageEvent) {
  subscriber?.({ type: "message_update", message, assistantMessageEvent }, sessionPath);
}

function emitDelta(subscriber, sessionPath, message, delta, contentIndex = 0) {
  emit(subscriber, sessionPath, message, {
    type: "text_delta",
    contentIndex,
    delta,
    partial: message,
  });
}

function emitTextEnd(subscriber, sessionPath, message, content, contentIndex = 0, partial = message) {
  emit(subscriber, sessionPath, message, {
    type: "text_end",
    contentIndex,
    content,
    partial,
  });
}

function emitTool(subscriber, sessionPath, toolCallId, name) {
  subscriber?.({ type: "tool_execution_start", toolCallId, toolName: name, args: {} }, sessionPath);
  subscriber?.({
    type: "tool_execution_end",
    toolCallId,
    toolName: name,
    result: { content: [{ type: "text", text: "ok" }] },
    isError: false,
  }, sessionPath);
}

function visibleText(payloads) {
  return payloads.filter((p) => p.type === "text_delta").map((p) => p.delta);
}

function moodTexts(payloads) {
  return payloads.filter((p) => p.type === "mood_text").map((p) => p.delta);
}

function thinkingTexts(payloads) {
  return payloads.filter((p) => p.type === "thinking_delta").map((p) => p.delta);
}

function canonicalTextDeltas(payloads) {
  return payloads
    .filter((p) => p.type === "assistant_segment_delta" && p.semanticPhase !== "reasoning")
    .map((p) => p.delta);
}

function textSegmentStarts(payloads) {
  return payloads.filter((p) => p.type === "assistant_segment_start" && p.kind === "text");
}

/** §28 全局不变量：最终 answer 通道（legacy text_delta + canonical 文本段 delta）不得含保留标签。 */
function expectNoReservedTagInAnswerChannel(payloads) {
  for (const text of [...visibleText(payloads), ...canonicalTextDeltas(payloads)]) {
    expect(text).not.toMatch(RESERVED_TAG_PATTERN);
  }
}

describe("chat route reserved raw fallback (text_end 二次回流)", () => {
  it("Test 1: mood-only delta + text_end 重复同一 raw content，不得产生正文", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();
    const message = assistantMessage(["<mood>A</mood>"]);
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message }, sessionPath);
    emitDelta(subscriber, sessionPath, message, "<mood>A</mood>");
    emitTextEnd(subscriber, sessionPath, message, "<mood>A</mood>");
    subscriber?.({ type: "message_end", message }, sessionPath);
    subscriber?.({ type: "turn_end", message, toolResults: [] }, sessionPath);

    expect(moodTexts(payloads())).toEqual(["A"]);
    // 核心断言：mood 内容不得以任何形式进入正文通道
    expect(visibleText(payloads())).toEqual([]);
    expect(canonicalTextDeltas(payloads())).toEqual([]);
    expectNoReservedTagInAnswerChannel(payloads());
    // 不变量 4：没有 visible text 就不得制造 text segment
    expect(textSegmentStarts(payloads())).toEqual([]);
  });

  it("Test 2/3: reflect-only 与 pulse-only 同样不得经 text_end 回流", () => {
    for (const tag of ["reflect", "pulse"]) {
      const { subscriber, sessionPath, payloads } = makeHarness(`/tmp/raw-fallback-${tag}.jsonl`);
      const raw = `<${tag}>A</${tag}>`;
      const message = assistantMessage([raw]);
      subscriber?.({ type: "turn_start" }, sessionPath);
      subscriber?.({ type: "message_start", message }, sessionPath);
      emitDelta(subscriber, sessionPath, message, raw);
      emitTextEnd(subscriber, sessionPath, message, raw);
      subscriber?.({ type: "turn_end", message, toolResults: [] }, sessionPath);

      expect(moodTexts(payloads())).toEqual(["A"]);
      expect(visibleText(payloads())).toEqual([]);
      expect(canonicalTextDeltas(payloads())).toEqual([]);
      expectNoReservedTagInAnswerChannel(payloads());
      expect(textSegmentStarts(payloads())).toEqual([]);
    }
  });

  it("Test 4/5: think/thinking-only 进入 reasoning，不得经 text_end 回流成正文", () => {
    for (const tag of ["think", "thinking"]) {
      const { subscriber, sessionPath, payloads } = makeHarness(`/tmp/raw-fallback-${tag}.jsonl`);
      const raw = `<${tag}>A</${tag}>`;
      const message = assistantMessage([raw]);
      subscriber?.({ type: "turn_start" }, sessionPath);
      subscriber?.({ type: "message_start", message }, sessionPath);
      emitDelta(subscriber, sessionPath, message, raw);
      emitTextEnd(subscriber, sessionPath, message, raw);
      subscriber?.({ type: "turn_end", message, toolResults: [] }, sessionPath);

      expect(thinkingTexts(payloads())).toEqual(["A"]);
      expect(visibleText(payloads())).toEqual([]);
      expect(canonicalTextDeltas(payloads())).toEqual([]);
      expectNoReservedTagInAnswerChannel(payloads());
      expect(textSegmentStarts(payloads())).toEqual([]);
      // reasoning segment 允许存在（kind=reasoning），但不得携带标签字面量
      const reasoningDeltas = payloads()
        .filter((p) => p.type === "assistant_segment_delta" && p.semanticPhase === "reasoning")
        .map((p) => p.delta);
      expect(reasoningDeltas.join("")).toBe("A");
    }
  });

  it("Test 21: mood + 正文，text_end 重复全文时正文不得翻倍", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();
    const raw = "<mood>A</mood>正文";
    const message = assistantMessage([raw]);
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message }, sessionPath);
    emitDelta(subscriber, sessionPath, message, raw);
    emitTextEnd(subscriber, sessionPath, message, raw);
    subscriber?.({ type: "turn_end", message, toolResults: [] }, sessionPath);

    expect(moodTexts(payloads())).toEqual(["A"]);
    expect(visibleText(payloads()).join("")).toBe("正文");
    expect(canonicalTextDeltas(payloads()).join("")).toBe("正文");
    expectNoReservedTagInAnswerChannel(payloads());
  });

  it("Test 22: 无 delta 的 Provider（仅 text_end 给完整内容）仍正确解析 mood 与正文", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();
    const raw = "<mood>A</mood>正文";
    const message = assistantMessage([raw]);
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message }, sessionPath);
    emitTextEnd(subscriber, sessionPath, message, raw);
    subscriber?.({ type: "message_end", message }, sessionPath);
    subscriber?.({ type: "turn_end", message, toolResults: [] }, sessionPath);

    expect(moodTexts(payloads())).toEqual(["A"]);
    expect(visibleText(payloads()).join("")).toBe("正文");
    expect(canonicalTextDeltas(payloads()).join("")).toBe("正文");
    expectNoReservedTagInAnswerChannel(payloads());
  });

  it("Test 22b: 无 delta 且 text_end 仅含 mood 时不制造正文段", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();
    const raw = "<mood>A</mood>";
    const message = assistantMessage([raw]);
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message }, sessionPath);
    emitTextEnd(subscriber, sessionPath, message, raw);
    subscriber?.({ type: "turn_end", message, toolResults: [] }, sessionPath);

    expect(moodTexts(payloads())).toEqual(["A"]);
    expect(visibleText(payloads())).toEqual([]);
    expect(textSegmentStarts(payloads())).toEqual([]);
  });

  it("Test 30: text_end content 为空但 partial/message 仍带 raw text 时，fallback 同步关闭", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();
    const message = assistantMessage(["<mood>A</mood>"]);
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message }, sessionPath);
    emitDelta(subscriber, sessionPath, message, "<mood>A</mood>");
    // raw source 已经由 delta 消费；这个 text_end 的 content 为空，
    // raw text 只存留在 partial.content[0].text / message.content[0].text
    emitTextEnd(subscriber, sessionPath, message, "");
    subscriber?.({ type: "turn_end", message, toolResults: [] }, sessionPath);

    expect(moodTexts(payloads())).toEqual(["A"]);
    expect(visibleText(payloads())).toEqual([]);
    expect(canonicalTextDeltas(payloads())).toEqual([]);
    expect(textSegmentStarts(payloads())).toEqual([]);
  });

  it("Test 23: 多 contentIndex 四种 delta/end 组合都不丢正文、不回流标签", () => {
    const cases = {
      // 情形 A：两个 index 都有 delta
      A: { deltaIndices: [0, 1], endIndices: [0, 1] },
      // 情形 B：index 0 有 delta，index 1 只有 text_end
      B: { deltaIndices: [0], endIndices: [0, 1] },
      // 情形 C：index 0 只有 text_end，index 1 有 delta
      C: { deltaIndices: [1], endIndices: [0, 1] },
      // 情形 D：两个 index 都只有 text_end
      D: { deltaIndices: [], endIndices: [0, 1] },
    };
    for (const [name, { deltaIndices, endIndices }] of Object.entries(cases)) {
      const { subscriber, sessionPath, payloads } = makeHarness(`/tmp/raw-fallback-multi-${name}.jsonl`);
      const message = assistantMessage(["<mood>A</mood>", "正文"]);
      subscriber?.({ type: "turn_start" }, sessionPath);
      subscriber?.({ type: "message_start", message }, sessionPath);
      for (const index of deltaIndices) {
        emitDelta(subscriber, sessionPath, message, index === 0 ? "<mood>A</mood>" : "正文", index);
      }
      for (const index of endIndices) {
        emitTextEnd(subscriber, sessionPath, message, index === 0 ? "<mood>A</mood>" : "正文", index);
      }
      subscriber?.({ type: "message_end", message }, sessionPath);
      subscriber?.({ type: "turn_end", message, toolResults: [] }, sessionPath);

      expect(moodTexts(payloads()), `case ${name} mood`).toEqual(["A"]);
      expect(visibleText(payloads()).join(""), `case ${name} visible`).toBe("正文");
      expect(canonicalTextDeltas(payloads()).join(""), `case ${name} canonical`).toBe("正文");
      expectNoReservedTagInAnswerChannel(payloads());
    }
  });

  it("Test 24: 多个内部块交替，text_end 重复全文时全部只结构化一次", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();
    const raw = "<mood>A</mood>\n正文1\n<reflect>B</reflect>\n正文2\n<pulse>C</pulse>";
    const message = assistantMessage([raw]);
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message }, sessionPath);
    emitDelta(subscriber, sessionPath, message, raw);
    emitTextEnd(subscriber, sessionPath, message, raw);
    subscriber?.({ type: "turn_end", message, toolResults: [] }, sessionPath);

    expect(moodTexts(payloads())).toEqual(["A", "B", "C"]);
    const visible = visibleText(payloads()).join("");
    expect(visible).toContain("正文1");
    expect(visible).toContain("正文2");
    expect(visible).not.toContain("A");
    expectNoReservedTagInAnswerChannel(payloads());
  });

  it("Test 25: 跨 delta 半截标签 + text_end 重复全文仍只结构化一次", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();
    const message = assistantMessage(["<mood>A</mood>"]);
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message }, sessionPath);
    for (const chunk of ["<mo", "od>", "A", "</mo", "od>"]) {
      emitDelta(subscriber, sessionPath, message, chunk);
    }
    emitTextEnd(subscriber, sessionPath, message, "<mood>A</mood>");
    subscriber?.({ type: "turn_end", message, toolResults: [] }, sessionPath);

    expect(moodTexts(payloads())).toEqual(["A"]);
    expect(visibleText(payloads())).toEqual([]);
    expect(canonicalTextDeltas(payloads())).toEqual([]);
    expect(textSegmentStarts(payloads())).toEqual([]);
  });

  it("Test 26: 转义 / 行内代码 / 围栏代码中的字面标签仍按正文透传", () => {
    const scenarios = [
      { name: "escaped", raw: "\\<mood>A</mood>", expected: "<mood>A</mood>" },
      { name: "inline-code", raw: "`<mood>A</mood>`", expected: "`<mood>A</mood>`" },
      { name: "fenced", raw: "```text\n<mood>A</mood>\n```", expected: "```text\n<mood>A</mood>\n```" },
    ];
    for (const { name, raw, expected } of scenarios) {
      const { subscriber, sessionPath, payloads } = makeHarness(`/tmp/raw-fallback-literal-${name}.jsonl`);
      const message = assistantMessage([raw]);
      subscriber?.({ type: "turn_start" }, sessionPath);
      subscriber?.({ type: "message_start", message }, sessionPath);
      emitDelta(subscriber, sessionPath, message, raw);
      emitTextEnd(subscriber, sessionPath, message, raw);
      subscriber?.({ type: "turn_end", message, toolResults: [] }, sessionPath);

      expect(moodTexts(payloads()), name).toEqual([]);
      expect(visibleText(payloads()).join(""), name).toBe(expected);
      // 字面标签只能出现一次（text_end 不得二次追加）
      expect(canonicalTextDeltas(payloads()).join(""), name).toBe(expected);
    }
  });

  it("Test 18: mood-only → generate-image → 第二段 mood + final answer 全链路", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();
    const msg1 = assistantMessage(["<mood>准备生成图片</mood>"]);
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message: msg1 }, sessionPath);
    emitDelta(subscriber, sessionPath, msg1, "<mood>准备生成图片</mood>");
    emitTextEnd(subscriber, sessionPath, msg1, "<mood>准备生成图片</mood>");
    subscriber?.({ type: "message_end", message: msg1 }, sessionPath);
    emitTool(subscriber, sessionPath, "img-1", "generate-image");
    subscriber?.({ type: "message_start", message: { role: "toolResult" } }, sessionPath);
    subscriber?.({ type: "message_end", message: { role: "toolResult" } }, sessionPath);
    subscriber?.({ type: "turn_end", message: msg1, toolResults: [] }, sessionPath);

    const msg2 = assistantMessage(["<mood>图片生成请求已提交</mood>完成。"]);
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message: msg2 }, sessionPath);
    emitDelta(subscriber, sessionPath, msg2, "<mood>图片生成请求已提交</mood>完成。");
    emitTextEnd(subscriber, sessionPath, msg2, "<mood>图片生成请求已提交</mood>完成。");
    subscriber?.({ type: "message_end", message: msg2 }, sessionPath);
    subscriber?.({ type: "turn_end", message: msg2, toolResults: [] }, sessionPath);

    expect(moodTexts(payloads())).toEqual(["准备生成图片", "图片生成请求已提交"]);
    expect(visibleText(payloads()).join("")).toBe("完成。");
    expect(canonicalTextDeltas(payloads()).join("")).toBe("完成。");
    expect(payloads().filter((p) => p.type === "tool_start").map((p) => p.name)).toEqual(["generate-image"]);
    expectNoReservedTagInAnswerChannel(payloads());
  });

  it("Test 19: mood-only → generate-video → 第二段 mood + final answer 全链路", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();
    const msg1 = assistantMessage(["<mood>准备生成视频</mood>"]);
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message: msg1 }, sessionPath);
    emitDelta(subscriber, sessionPath, msg1, "<mood>准备生成视频</mood>");
    emitTextEnd(subscriber, sessionPath, msg1, "<mood>准备生成视频</mood>");
    subscriber?.({ type: "message_end", message: msg1 }, sessionPath);
    emitTool(subscriber, sessionPath, "vid-1", "generate-video");
    subscriber?.({ type: "message_end", message: { role: "toolResult" } }, sessionPath);
    subscriber?.({ type: "turn_end", message: msg1, toolResults: [] }, sessionPath);

    const msg2 = assistantMessage(["<mood>视频生成请求已提交</mood>完成。"]);
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message: msg2 }, sessionPath);
    emitDelta(subscriber, sessionPath, msg2, "<mood>视频生成请求已提交</mood>完成。");
    emitTextEnd(subscriber, sessionPath, msg2, "<mood>视频生成请求已提交</mood>完成。");
    subscriber?.({ type: "turn_end", message: msg2, toolResults: [] }, sessionPath);

    expect(moodTexts(payloads())).toEqual(["准备生成视频", "视频生成请求已提交"]);
    expect(visibleText(payloads()).join("")).toBe("完成。");
    expect(payloads().filter((p) => p.type === "tool_start").map((p) => p.name)).toEqual(["generate-video"]);
    expectNoReservedTagInAnswerChannel(payloads());
  });

  it("Test 20: mood-only + 工具、无 final answer 时不制造假正文段", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();
    const msg = assistantMessage(["<mood>A</mood>"]);
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message: msg }, sessionPath);
    emitDelta(subscriber, sessionPath, msg, "<mood>A</mood>");
    emitTextEnd(subscriber, sessionPath, msg, "<mood>A</mood>");
    subscriber?.({ type: "message_end", message: msg }, sessionPath);
    emitTool(subscriber, sessionPath, "t1", "read");
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);

    expect(moodTexts(payloads())).toEqual(["A"]);
    expect(visibleText(payloads())).toEqual([]);
    expect(canonicalTextDeltas(payloads())).toEqual([]);
    // 不得出现空的 final_answer text segment（否则前端会错误豁免 missing_final_answer）
    expect(textSegmentStarts(payloads())).toEqual([]);
    expect(payloads().filter((p) => p.type === "tool_end")).toHaveLength(1);
  });

  it("Test G: phase-at-end Provider 的 textSignature 相位判定不受 raw blanking 影响", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();
    const textSignature = JSON.stringify({ v: 1, id: "final-1", phase: "final_answer" });
    const raw = "<mood>A</mood>正文";
    const partial = {
      role: "assistant",
      api: "openai-responses",
      content: [{ type: "text", text: raw }],
    };
    const ended = {
      ...partial,
      content: [{ type: "text", text: raw, textSignature }],
    };
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message: partial }, sessionPath);
    emit(subscriber, sessionPath, partial, {
      type: "text_delta",
      contentIndex: 0,
      delta: raw,
      partial,
    });
    emit(subscriber, sessionPath, ended, {
      type: "text_end",
      contentIndex: 0,
      content: raw,
      partial: ended,
    });
    subscriber?.({ type: "turn_end", message: ended, toolResults: [] }, sessionPath);

    expect(moodTexts(payloads())).toEqual(["A"]);
    // 流式阶段 unresolved → 不发布可见文本；text_end 解析出 final_answer 后一次性发布
    expect(visibleText(payloads())).toEqual(["正文"]);
    const segmentEnds = payloads().filter((p) => p.type === "assistant_segment_end");
    expect(segmentEnds).toHaveLength(1);
    expect(segmentEnds[0].semanticPhase).toBe("final_answer");
    expectNoReservedTagInAnswerChannel(payloads());
  });
});
