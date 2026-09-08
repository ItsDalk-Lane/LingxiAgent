// @vitest-environment jsdom

/**
 * F8/P6：保留通用形状规则，停止误删正常标记（T01–T16）。
 *
 * 三个分开的概念：已知内部协议块 / 正常未知标记的开启栈 / 代码与转义保护。
 * 未知孤儿闭标签只在最终原始 assistant 文本边界清理；配对未知标记必须原样保留。
 * 测试跑完整 Think→Mood 链与历史入口，不只测孤立 scanner。
 */

import { describe, expect, it, vi } from "vitest";
import { MoodParser, ThinkTagParser } from "../core/events.ts";
import {
  ReservedTagScanner,
  splitReservedTagSegments,
} from "../shared/reserved-tag-stream.ts";
import { sanitizePersistedSegmentSource } from "../desktop/src/react/utils/history-segment-sanitizer.ts";
import { createChatRoute } from "../server/routes/chat.ts";
import { streamBufferManager } from "../desktop/src/react/hooks/use-stream-buffer.ts";
import { useStore } from "../desktop/src/react/stores/index.ts";
import { readLiveAssistantMessage } from "../desktop/src/react/stores/live-turn-store.ts";
import { buildItemsFromHistory } from "../desktop/src/react/utils/history-builder.ts";
import { extractPersistedAssistantSemanticSegments } from "../shared/assistant-semantic-segments.ts";

type ChainEvent = { type: string; data?: string };

/** 完整解析链：raw → ThinkTagParser（中间层）→ MoodParser（链尾）→ 事件。与 chat.ts 组合一致。 */
function runChain(raw: string, chunks: string[] = [raw]): ChainEvent[] {
  const think = new ThinkTagParser();
  const mood = new MoodParser();
  const events: ChainEvent[] = [];
  const feedMood = (evt: { type: string; data?: string }) => {
    events.push(evt.type === "text" ? { type: "text", data: evt.data } : evt);
  };
  const feedThink = (evt: { type: string; data?: string }) => {
    if (evt.type === "text") mood.feed(evt.data ?? "", feedMood);
    else events.push(evt);
  };
  for (const chunk of chunks) think.feed(chunk, feedThink);
  think.flush(feedThink);
  mood.flush(feedMood);
  return events;
}

function chainText(events: ChainEvent[]): string {
  return events.filter((e) => e.type === "text").map((e) => e.data ?? "").join("");
}

/** 事件序列归一：合并相邻 text 事件（流式分块天然改变 token 边界，语义不变）。 */
function normalizeEvents(events: ChainEvent[]): ChainEvent[] {
  const out: ChainEvent[] = [];
  for (const evt of events) {
    const prev = out[out.length - 1];
    if (evt.type === "text" && prev?.type === "text") prev.data = (prev.data ?? "") + (evt.data ?? "");
    else out.push({ ...evt });
  }
  return out;
}

function scannerText(input: string, chunks: string[] = [input]): string {
  const scanner = new ReservedTagScanner(["think", "thinking"], { dropUnknownOrphanClosers: true });
  let text = "";
  const collect = (tokens: Array<{ type: string; text?: string }>) => {
    for (const token of tokens) if (token.type === "text") text += token.text;
  };
  for (const chunk of chunks) collect(scanner.feed(chunk));
  collect(scanner.flush());
  return text;
}

const MIXED_SOURCE = [
  '<details><summary>说明</summary>正文段落</details>',
  '好<x:item a="1>0">内容</x:item>',
  '代码示例：',
  '```html',
  '</div>与 <custom/> 标记',
  '```',
  '行内 `</mm:think>` 也不删',
  '真正的孤儿：',
].join('\n');

describe("F8/P6 通用形状规则与正常标记保护（T01–T12：scanner 与完整链）", () => {
  it("T01: 正常配对的未知标记全部保留", () => {
    const source = '<details><summary>说明</summary>正文</details>';
    expect(chainText(runChain(source))).toBe(source);
    expect(scannerText(source)).toBe(source);
  });

  it("T02: 属性内带引号的 > 不误截断，配对闭标签保留", () => {
    const source = '<x:item a="1>0">内容</x:item>';
    expect(chainText(runChain(source))).toBe(source);
    expect(scannerText(source)).toBe(source);
  });

  it("T03: 未知自闭合标记与正常嵌套不产生伪孤儿", () => {
    const source = '<custom/>和<a><b>x</b></a>正文';
    expect(chainText(runChain(source))).toBe(source);
    expect(scannerText(source)).toBe(source);
  });

  it("T04: 未知孤儿闭标签在最终原始 assistant 边界清理", () => {
    expect(chainText(runChain('正文前</vendor:tail>正文后'))).toBe('正文前正文后');
    expect(scannerText('正文前</vendor:tail>正文后')).toBe('正文前正文后');
  });

  it("T05: 已知 think/mood/mm:think 成对结构化通道不退化（完整链）", () => {
    const events = runChain('<think>内心</think><mood>开心</mood><mm:think>方言</mm:think>答案');
    expect(events).toEqual([
      { type: "think_start" },
      { type: "think_text", data: "内心" },
      { type: "think_end" },
      { type: "mood_start" },
      { type: "mood_text", data: "开心" },
      { type: "mood_end" },
      { type: "think_start" },
      { type: "think_text", data: "方言" },
      { type: "think_end" },
      { type: "text", data: "答案" },
    ]);
  });

  it("T06: code fence／inline code 内闭标签字符不变", () => {
    const source = '示例：\n```html\n</div>\n```\n行内 `</x:y>` 结束';
    expect(chainText(runChain(source))).toBe(source);
    expect(scannerText(source)).toBe(source);
  });

  it("T07: 已转义已知／未知闭标签经完整链保留 canonical 字面量", () => {
    const raw = '教学：\\</think> 与 \\</vendor:tail> 两种闭标签';
    const text = chainText(runChain(raw));
    // 链上保护：反斜杠随字面量保留，任何层都不得吞掉标签本身
    expect(text).toBe('教学：\\</think> 与 \\</vendor:tail> 两种闭标签');
    // 转义开标签同样保护（不会被误结构化为思考块）
    const openEvents = runChain('\\<think>不是思考块');
    expect(chainText(openEvents)).toBe('\\<think>不是思考块');
  });

  it("T08: 每个字符位置二分喂入与一次性喂入语义一致", () => {
    const once = runChain(MIXED_SOURCE);
    for (let i = 1; i < MIXED_SOURCE.length; i += 1) {
      const split = runChain(MIXED_SOURCE, [MIXED_SOURCE.slice(0, i), MIXED_SOURCE.slice(i)]);
      expect(normalizeEvents(split)).toEqual(normalizeEvents(once));
    }
  });

  it("T09: 固定种子多段分块与一次性喂入等价（事件序列归一）", () => {
    let seed = 42;
    const rand = () => {
      seed = (seed * 1103515245 + 12345) % 2147483648;
      return seed / 2147483648;
    };
    for (let round = 0; round < 12; round += 1) {
      const chunks: string[] = [];
      let rest = MIXED_SOURCE;
      while (rest.length) {
        const size = 1 + Math.floor(rand() * 7);
        chunks.push(rest.slice(0, size));
        rest = rest.slice(size);
      }
      expect(normalizeEvents(runChain(MIXED_SOURCE, chunks))).toEqual(normalizeEvents(runChain(MIXED_SOURCE)));
    }
  });

  it("T10: 半截标记后 flush 按字面定界，不跨 assistant segment 污染", () => {
    const think = new ThinkTagParser();
    const mood = new MoodParser();
    const texts: string[] = [];
    const feedMood = (evt: { type: string; data?: string }) => {
      if (evt.type === "text") texts.push(evt.data ?? "");
    };
    const feedThink = (evt: { type: string; data?: string }) => {
      if (evt.type === "text") mood.feed(evt.data ?? "", feedMood);
    };
    think.feed('正文<ven', feedThink);
    think.flush(feedThink);
    mood.flush(feedMood);
    expect(texts.join('')).toBe('正文<ven');
    // 新 assistant segment：上一段的半截状态不得泄漏
    think.beginAssistantSegment();
    mood.beginAssistantSegment();
    think.feed('>后续正文', feedThink);
    think.flush(feedThink);
    mood.flush(feedMood);
    expect(texts.join('')).toBe('正文<ven>后续正文');
  });

  it("T11: a<b、比较式、注释、CDATA 保守保留非协议内容", () => {
    const source = 'a<b 且 5 < 6 <!-- 注释 <think> 不解释 --> 结束';
    expect(chainText(runChain(source))).toBe(source);
    const cdata = '<![CDATA[<think>与</think>都在字面里]]>尾巴';
    expect(chainText(runChain(cdata))).toBe(cdata);
  });

  it("T12: 错配嵌套与超深/超长输入不崩溃、不成段丢正文", () => {
    const mismatch = '<x><y>内容</x></y>结尾';
    expect(chainText(runChain(mismatch))).toBe(mismatch);
    // 超深嵌套：全部原样保留（上限后保守透传，不得丢正文）
    const deep = '<a>'.repeat(500) + '正文' + '</a>'.repeat(500);
    expect(chainText(runChain(deep))).toBe(deep);
    // 超长未结束标记：flush 按字面吐出，不无限缓冲
    const longUnfinished = '前文<' + 'a'.repeat(200_000);
    const start = Date.now();
    expect(chainText(runChain(longUnfinished))).toBe(longUnfinished);
    expect(Date.now() - start).toBeLessThan(5_000);
  });
});

describe("F8/P6 历史与实时入口（T13–T16）", () => {
  const PATH = "/tmp/reserved-tag-preservation-parity.jsonl";

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

  function userItem(id: string, text: string) {
    return { type: "message" as const, data: { id, role: "user" as const, text } };
  }

  function emitAnswerTurn(subscriber, sessionPath, raw) {
    const msg = assistantMessage(raw);
    subscriber?.({ type: "agent_start" }, sessionPath);
    subscriber?.({ type: "turn_start" }, sessionPath);
    subscriber?.({ type: "message_start", message: msg }, sessionPath);
    subscriber?.({
      type: "message_update", message: msg,
      assistantMessageEvent: { type: "text_delta", contentIndex: 0, delta: raw, partial: msg },
    }, sessionPath);
    subscriber?.({
      type: "message_update", message: msg,
      assistantMessageEvent: { type: "text_end", contentIndex: 0, content: raw, partial: msg },
    }, sessionPath);
    subscriber?.({ type: "message_end", message: msg }, sessionPath);
    subscriber?.({ type: "turn_end", message: msg, toolResults: [] }, sessionPath);
    subscriber?.({ type: "agent_settled" }, sessionPath);
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
    useStore.getState().initSession(PATH, [userItem("u1", "提问")], false);
  }

  function liveAnswerText(payloadList: Array<{ type: string }>): string {
    resetLiveSession();
    for (const payload of payloadList) {
      if (LIVE_EVENT_TYPES.has(payload.type)) streamBufferManager.handle(payload);
    }
    const state = useStore.getState() as unknown as {
      chatSessions: Record<string, { items?: Array<{ type: string; data: { id: string; role: string; blocks?: unknown[] } }> }>;
    };
    const items = state.chatSessions[PATH]?.items ?? [];
    const assistant = items.find((entry) => entry.type === "message" && entry.data.role === "assistant");
    if (!assistant) return "";
    const live = readLiveAssistantMessage(PATH, assistant.data.id);
    const blocks = live ? [...live.blocks] : (assistant.data.blocks || []);
    return blocks
      .filter((block: any) => block.type === "text" && block.surfaceRole !== "thinking")
      .map((block: any) => block.source ?? block.text ?? "")
      .join("");
  }

  function historyAnswerText(raw: string): string {
    const historyItems = buildItemsFromHistory({
      messages: [
        { id: "0", entryId: "entry-user-1", role: "user", content: "提问" },
        {
          id: "1",
          entryId: "entry-assistant-1",
          role: "assistant",
          content: raw,
          turnInputEntryId: "entry-user-1",
          assistantSegments: extractPersistedAssistantSemanticSegments(
            [{ type: "text", text: raw }],
            1,
          ),
        },
      ],
    } as never);
    const texts: string[] = [];
    const collect = (item: any) => {
      const blocks = item?.type === "message" && item?.data?.role === "assistant"
        ? (item.data.blocks || [])
        : [];
      for (const block of blocks) {
        if (block.type === "text") texts.push(String(block.source ?? block.text ?? ""));
      }
    };
    for (const item of historyItems) collect(item);
    return texts.join("");
  }

  function historyUserText(content: string): string {
    const historyItems = buildItemsFromHistory({
      messages: [
        { id: "0", entryId: "entry-user-1", role: "user", content },
      ],
    } as never);
    const texts: string[] = [];
    for (const item of historyItems as any[]) {
      if (item?.type === "message" && item.data?.role === "user") {
        if (typeof item.data.text === "string") texts.push(item.data.text);
      }
    }
    return texts.join("");
  }

  it("T13: 同源 raw 实时与历史路径的 answer 文本一致（未知配对保留 + 孤儿清理 + 代码保护）", () => {
    const raw = [
      '<details><summary>说明</summary>看这里</details>',
      '换模型了</vendor:tail>正文继续',
      '```html',
      '</div>',
      '```',
      '收尾',
    ].join('\n');
    const harness = makeServerHarness(PATH);
    emitAnswerTurn(harness.subscriber, PATH, raw);
    const liveText = liveAnswerText(harness.payloads());
    const historyText = historyAnswerText(raw);

    expect(liveText).toBe(historyText);
    const expected = [
      '<details><summary>说明</summary>看这里</details>',
      '换模型了正文继续',
      '```html',
      '</div>',
      '```',
      '收尾',
    ].join('\n');
    expect(liveText).toBe(expected);
  });

  it("T14: 已规范化 canonical 文本再渲染不再次清理字面内容（转义字面量稳定）", () => {
    const canonical = '教学示例 \\</think> 与 <details>配对</details> 保留';
    const once = historyAnswerText(canonical);
    const twice = historyAnswerText(once);
    expect(once).toBe(canonical);
    expect(twice).toBe(canonical);
  });

  it("T15: 用户输入含 HTML/XML 不进入 assistant 残渣清理", () => {
    const content = '我写了 <div>x</div></span> 和 </mm:think> 给你看';
    const joined = historyUserText(content);
    expect(joined).toContain('<div>x</div>');
    expect(joined).toContain('</span>');
    expect(joined).toContain('</mm:think>');
  });

  it("T16: 历史仅孤儿残渣、无 leading block 可删时不被 return 原 source 抵消清理", () => {
    expect(sanitizePersistedSegmentSource('答案正文</mm:think>', {
      hasStructuredMood: false,
      hasStructuredThinking: true,
    })).toBe('答案正文');
    expect(sanitizePersistedSegmentSource('答案正文', {
      hasStructuredMood: false,
      hasStructuredThinking: true,
    })).toBe('答案正文');
  });
});

describe("F8/P6 splitReservedTagSegments 与显示投影", () => {
  it("孤儿清理与配对保留在全文切分同时成立", () => {
    const segments = splitReservedTagSegments(
      '<think>秘密</think>露出的</vendor:tail><b>加粗</b>尾巴',
      ["think", "thinking"],
    );
    expect(segments).toEqual([
      { type: "block", tag: "think", content: "秘密" },
      { type: "text", text: "露出的<b>加粗</b>尾巴" },
    ]);
  });

  it("canonical source 保留标签转义与普通反斜杠，多次经过 scanner 不漂移", () => {
    const tagged = '\\</think>\\<b>x\\</b>与\\<custom/>';
    expect(scannerText(tagged)).toBe(tagged);
    const ordinary = 'C:\\path 与 1 \\< 2';
    expect(scannerText(ordinary)).toBe(ordinary);
    const once = scannerText('\\</vendor:tail>');
    expect(scannerText(once)).toBe(once);
  });
});
