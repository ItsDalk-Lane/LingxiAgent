import { describe, expect, it, vi } from "vitest";
import { createChatRoute } from "../server/routes/chat.ts";

/**
 * Agent 工具循环中 <reflect>/<mood>/<pulse> 内部块的生命周期回归测试。
 *
 * 实证事件序（见调查）：一次 user turn 内，pi SDK 把每段模型生成（含工具调用后的
 * 第二轮）都包在独立的 turn_start/turn_end 里，且每段开头有一次 message_start(assistant)。
 * 服务端必须保证：保留协议标签（<mood>/<pulse>/<reflect>）无论出现在一段生成的什么
 * 位置都被结构化为 mood_*，绝不作为 text_delta 泄漏；一段里允许多个 mood 块。
 * 需要字面量时由模型用 \<tag> 转义或行内代码 / 围栏代码块表达。
 */

function makeHarness(sessionPath = "/tmp/mood-segment.jsonl") {
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
  return { subscriber, sessionPath, payloads, handlers, ws };
}

function emitAssistantText(subscriber, sessionPath, message, text) {
  subscriber?.({ type: "message_start", message }, sessionPath);
  subscriber?.({
    type: "message_update",
    message,
    assistantMessageEvent: { type: "text_delta", delta: text },
  }, sessionPath);
}

function emitTool(subscriber, sessionPath, toolCallId = "t1", name = "read") {
  subscriber?.({ type: "tool_execution_start", toolCallId, toolName: name, args: {} }, sessionPath);
  subscriber?.({
    type: "tool_execution_end",
    toolCallId,
    toolName: name,
    result: { content: [{ type: "text", text: "ok" }] },
    isError: false,
  }, sessionPath);
}

describe("chat route mood segment lifecycle", () => {
  it("projects model skill reads as bounded tool_end details", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({
      type: "tool_execution_start",
      toolCallId: "skill-1",
      toolName: "read",
      args: { path: "/skills/leader/SKILL.md" },
    }, sessionPath);
    subscriber?.({
      type: "tool_execution_end",
      toolCallId: "skill-1",
      toolName: "read",
      result: { content: [{ type: "text", text: "# Skill: leader\n\nLead the work." }] },
      isError: false,
    }, sessionPath);

    expect(payloads().find((payload) => payload.type === "tool_end" && payload.id === "skill-1")).toMatchObject({
      name: "read",
      success: true,
      details: {
        skillInvocation: {
          content: "# Skill: leader\n\nLead the work.",
        },
      },
    });
  });

  it("Case A: single leading reflect → mood_start/mood_text/mood_end, answer as text", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();
    subscriber?.({ type: "turn_start" }, sessionPath);
    emitAssistantText(subscriber, sessionPath, { role: "assistant" }, "<reflect>AAA</reflect>最终答案");
    subscriber?.({ type: "turn_end", message: { role: "assistant" }, toolResults: [] }, sessionPath);

    const textDeltas = payloads().filter((p) => p.type === "text_delta").map((p) => p.delta);
    const moodTexts = payloads().filter((p) => p.type === "mood_text").map((p) => p.delta);
    expect(moodTexts).toEqual(["AAA"]);
    expect(textDeltas).toEqual(["最终答案"]);
    // 正文里绝不出现裸标签
    expect(textDeltas.join("")).not.toMatch(/<\/?reflect>/);
  });

  it("Case B: tool loop second segment reflect stays in mood, never leaks to text", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();
    const msg = { role: "assistant" };
    subscriber?.({ type: "turn_start" }, sessionPath);
    emitAssistantText(subscriber, sessionPath, msg, "<reflect>AAA</reflect>我先查一下");
    subscriber?.({ type: "message_end", message: msg }, sessionPath);
    emitTool(subscriber, sessionPath);
    subscriber?.({ type: "message_start", message: { role: "toolResult" } }, sessionPath);
    subscriber?.({ type: "message_end", message: { role: "toolResult" } }, sessionPath);
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);
    // 第二段
    subscriber?.({ type: "turn_start" }, sessionPath);
    emitAssistantText(subscriber, sessionPath, msg, "<reflect>BBB</reflect>最终答案");
    subscriber?.({ type: "message_end", message: msg }, sessionPath);
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);

    const textDeltas = payloads().filter((p) => p.type === "text_delta").map((p) => p.delta);
    const moodTexts = payloads().filter((p) => p.type === "mood_text").map((p) => p.delta);
    expect(moodTexts).toEqual(["AAA", "BBB"]);
    expect(textDeltas).toEqual(["我先查一下", "最终答案"]);
    // 硬性要求：text_delta 通道里永远不能出现 <reflect> 字面量
    expect(textDeltas.join("")).not.toMatch(/<\/?reflect>/);
  });

  it("Case B for <mood> and <pulse>: both tags survive a tool loop into mood", () => {
    for (const tag of ["mood", "pulse"]) {
      const { subscriber, sessionPath, payloads } = makeHarness();
      const msg = { role: "assistant" };
      subscriber?.({ type: "turn_start" }, sessionPath);
      emitAssistantText(subscriber, sessionPath, msg, `<${tag}>A</${tag}>t1`);
      subscriber?.({ type: "message_end", message: msg }, sessionPath);
      emitTool(subscriber, sessionPath);
      subscriber?.({ type: "message_end", message: { role: "toolResult" } }, sessionPath);
      subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);
      subscriber?.({ type: "turn_start" }, sessionPath);
      emitAssistantText(subscriber, sessionPath, msg, `<${tag}>B</${tag}>t2`);
      subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);

      const textDeltas = payloads().filter((p) => p.type === "text_delta").map((p) => p.delta);
      expect(textDeltas.join("")).not.toMatch(new RegExp(`</?${tag}>`));
      const moodTexts = payloads().filter((p) => p.type === "mood_text").map((p) => p.delta);
      expect(moodTexts).toEqual(["A", "B"]);
    }
  });

  it("Case C: consecutive tools before segment 2 emit exactly one mood cycle", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();
    const msg = { role: "assistant" };
    subscriber?.({ type: "turn_start" }, sessionPath);
    emitAssistantText(subscriber, sessionPath, msg, "<reflect>A</reflect>");
    for (const id of ["t1", "t2", "t3"]) emitTool(subscriber, sessionPath, id, "read");
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);
    subscriber?.({ type: "turn_start" }, sessionPath);
    emitAssistantText(subscriber, sessionPath, msg, "<reflect>B</reflect>answer");
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);

    const moodStarts = payloads().filter((p) => p.type === "mood_start");
    const moodEnds = payloads().filter((p) => p.type === "mood_end");
    expect(moodStarts).toHaveLength(2);
    expect(moodEnds).toHaveLength(2);
  });

  it("Case E: prose-internal reserved tag is structured too (tags are protocol, not text)", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();
    const msg = { role: "assistant" };
    subscriber?.({ type: "turn_start" }, sessionPath);
    emitAssistantText(subscriber, sessionPath, msg, "<reflect>A</reflect>");
    emitTool(subscriber, sessionPath);
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);
    subscriber?.({ type: "turn_start" }, sessionPath);
    // 新契约：保留标签出现在正文中间同样是协议，一律结构化为 mood，不进正文。
    emitAssistantText(subscriber, sessionPath, msg, "正文解释 <reflect>literal</reflect>");
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);

    const textDeltas = payloads().filter((p) => p.type === "text_delta").map((p) => p.delta);
    expect(textDeltas.join("")).toBe("正文解释 ");
    expect(textDeltas.join("")).not.toMatch(/<\/?reflect>/);
    const moodTexts = payloads().filter((p) => p.type === "mood_text").map((p) => p.delta);
    expect(moodTexts).toEqual(["A", "literal"]);
  });

  it("Case E2: a literal tag in prose is expressed with escape or code, and survives", () => {
    // 模型要"讲解标签"时的正确表达：\<reflect> 转义 / 行内代码。两者都必须按字面量进正文。
    const { subscriber, sessionPath, payloads } = makeHarness();
    const msg = { role: "assistant" };
    subscriber?.({ type: "turn_start" }, sessionPath);
    emitAssistantText(subscriber, sessionPath, msg, "写法是 \\<reflect> 或 `<reflect>` 两种");
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);

    const textDeltas = payloads().filter((p) => p.type === "text_delta").map((p) => p.delta);
    expect(textDeltas.join("")).toBe("写法是 <reflect> 或 `<reflect>` 两种");
    expect(payloads().filter((p) => p.type === "mood_start")).toHaveLength(0);
  });

  it("Case F: same-segment second tag is structured as a second mood block", () => {
    // 新契约：同一段生成里允许多个 mood 块（mood:0/1/2 互不覆盖）。
    const { subscriber, sessionPath, payloads } = makeHarness();
    const msg = { role: "assistant" };
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message: msg }, sessionPath);
    subscriber?.({
      type: "message_update",
      message: msg,
      assistantMessageEvent: { type: "text_delta", delta: "<reflect>A</reflect>普通正文<reflect>B</reflect>" },
    }, sessionPath);
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);

    const textDeltas = payloads().filter((p) => p.type === "text_delta").map((p) => p.delta);
    const moodTexts = payloads().filter((p) => p.type === "mood_text").map((p) => p.delta);
    expect(moodTexts).toEqual(["A", "B"]);
    expect(textDeltas.join("")).toBe("普通正文");
  });

  it("Case G: tool failure then new segment → second reflect still parses", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();
    const msg = { role: "assistant" };
    subscriber?.({ type: "turn_start" }, sessionPath);
    emitAssistantText(subscriber, sessionPath, msg, "<reflect>A</reflect>");
    subscriber?.({ type: "tool_execution_start", toolCallId: "t1", toolName: "read", args: {} }, sessionPath);
    subscriber?.({
      type: "tool_execution_end", toolCallId: "t1", toolName: "read",
      result: { content: [{ type: "text", text: "boom" }] }, isError: true,
    }, sessionPath);
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);
    subscriber?.({ type: "turn_start" }, sessionPath);
    emitAssistantText(subscriber, sessionPath, msg, "<reflect>B</reflect>恢复");
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);

    const moodTexts = payloads().filter((p) => p.type === "mood_text").map((p) => p.delta);
    expect(moodTexts).toEqual(["A", "B"]);
  });

  it("Case H: chunked second opener still parses across deltas", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();
    const msg = { role: "assistant" };
    subscriber?.({ type: "turn_start" }, sessionPath);
    emitAssistantText(subscriber, sessionPath, msg, "<reflect>A</reflect>");
    emitTool(subscriber, sessionPath);
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message: msg }, sessionPath);
    // 把第二段 opener + 内容 + closer 拆成跨 chunk 到达
    for (const chunk of ["<re", "flect>", "B", "</re", "flect>", "答案"]) {
      subscriber?.({
        type: "message_update",
        message: msg,
        assistantMessageEvent: { type: "text_delta", delta: chunk },
      }, sessionPath);
    }
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);

    const moodTexts = payloads().filter((p) => p.type === "mood_text").map((p) => p.delta);
    expect(moodTexts).toEqual(["A", "B"]);
    const textDeltas = payloads().filter((p) => p.type === "text_delta").map((p) => p.delta);
    expect(textDeltas.join("")).not.toMatch(/<\/?reflect>/);
    expect(textDeltas.join("")).toContain("答案");
  });

  it("Case I: native thinking and mood are independent within a segment", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();
    const msg = { role: "assistant" };
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message: msg }, sessionPath);
    subscriber?.({
      type: "message_update",
      message: msg,
      assistantMessageEvent: { type: "thinking_delta", delta: "原生推理" },
    }, sessionPath);
    emitAssistantText(subscriber, sessionPath, msg, "<reflect>AAA</reflect>正文");
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);

    const thinking = payloads().filter((p) => p.type === "thinking_delta").map((p) => p.delta).join("");
    const moodTexts = payloads().filter((p) => p.type === "mood_text").map((p) => p.delta);
    expect(thinking).toBe("原生推理");
    expect(moodTexts).toEqual(["AAA"]);
  });

  it("turn reset never cross-contaminates a fresh user turn", () => {
    const { subscriber, sessionPath, payloads } = makeHarness();
    const msg = { role: "assistant" };
    // turn 1
    subscriber?.({ type: "turn_start" }, sessionPath);
    emitAssistantText(subscriber, sessionPath, msg, "<reflect>A</reflect>t1");
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);
    // turn 2（新 user turn）：不应串入 A
    subscriber?.({ type: "turn_start" }, sessionPath);
    emitAssistantText(subscriber, sessionPath, msg, "<reflect>B</reflect>t2");
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);

    const moodTexts = payloads().filter((p) => p.type === "mood_text").map((p) => p.delta);
    expect(moodTexts).toEqual(["A", "B"]);
  });

  it("message_start re-arm does not silently drop a partial tag tail from the previous segment", () => {
    // SDK handleRunFailure 风格路径：上一段流被截断，只流出半个 opener "<re"
    // （leading-only 下 MoodParser 把它挂起等待后续 chunk），运行异常使下一个
    // message_start(assistant) 先于 turn_end 到达。re-arm 前必须先把挂起尾巴按
    // 可见文本冲出，不能随缓冲清空被静默丢弃。
    const { subscriber, sessionPath, payloads } = makeHarness();
    const msg = { role: "assistant" };
    subscriber?.({ type: "turn_start" }, sessionPath);
    emitAssistantText(subscriber, sessionPath, msg, "<re");
    // 无 turn_end：直接出现下一段 assistant message（运行失败 / 重试路径）
    emitAssistantText(subscriber, sessionPath, msg, "<reflect>B</reflect>恢复");
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);

    const textDeltas = payloads().filter((p) => p.type === "text_delta").map((p) => p.delta);
    // 上一段的挂起尾巴必须作为可见文本保留，不能被 re-arm 清掉
    expect(textDeltas.join("")).toContain("<re");
    // 新 segment 的 leading 内部块仍正常解析
    const moodTexts = payloads().filter((p) => p.type === "mood_text").map((p) => p.delta);
    expect(moodTexts).toEqual(["B"]);
  });

  it("message_start(assistant) re-arms leading eligibility without a turn boundary", () => {
    // 防护性契约：即便 turn_start/turn_end 没有干净分隔两段生成（重试 / 重连 /
    // 自定义工具在同 turn 内再次驱动的极端路径），只要 SDK 给出了新的
    // message_start(role=assistant)，第二段的 leading 内部块仍应被识别。
    const { subscriber, sessionPath, payloads } = makeHarness();
    const msg = { role: "assistant" };
    subscriber?.({ type: "turn_start" }, sessionPath);
    emitAssistantText(subscriber, sessionPath, msg, "<reflect>A</reflect>t1");
    // 故意不发 turn_end / turn_start，直接进入下一段 assistant message
    emitAssistantText(subscriber, sessionPath, msg, "<reflect>B</reflect>t2");
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);

    const moodTexts = payloads().filter((p) => p.type === "mood_text").map((p) => p.delta);
    expect(moodTexts).toEqual(["A", "B"]);
    const textDeltas = payloads().filter((p) => p.type === "text_delta").map((p) => p.delta);
    expect(textDeltas.join("")).not.toMatch(/<\/?reflect>/);
  });
});
