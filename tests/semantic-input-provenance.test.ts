// Phase 5 Semantic Input Provenance — 契约层单元测试（Step 3/4）。
// 覆盖：renderer 字节级等价（中文/emoji/代理对/空段）、span UTF-16 语义、
// provenance 无正文（毒丸）、sanitize fail closed（含绝对路径/URL 拒绝）、
// precision rollup、observer summary、chat context 构造（前缀证明/turn 判定/
// toolResult 分类/native summarizer）、callText remap（system merge 同步）、
// direct summary 构造。
import { describe, expect, it } from "vitest";

import {
  SEMANTIC_INPUT_PROVENANCE_SCHEMA_VERSION,
  buildCallTextFallbackProvenance,
  buildChatContextProvenance,
  buildDirectSummaryProvenance,
  createSemanticInputProvenance,
  renderProvenancedText,
  remapCallTextProvenance,
  rollupProvenancePrecision,
  sanitizeSemanticInputProvenance,
  summarizeSemanticInputProvenance,
  type ProvenancedTextSegment,
  type SemanticInputProvenanceSection,
  type SessionPromptProvenancePayload,
} from "../lib/llm/semantic-input-provenance.ts";
import { createModelCallRecorder } from "../lib/llm/model-call-recorder.ts";
import { createTestModelCallObserver } from "../lib/llm/model-call-observer-testing.ts";

const POISON_MARKERS = [
  "TOP_SECRET_PERSONA",
  "TOP_SECRET_MEMORY",
  "TOP_SECRET_USER",
  "TOP_SECRET_TOOL_RESULT",
];

function section(
  category: any,
  locator: any,
  precision: any = "exact",
): SemanticInputProvenanceSection {
  return { category, precision, locator, role: null, source: null };
}

describe("renderProvenancedText", () => {
  it("renders byte-identical to join() with UTF-16 spans covering each segment", () => {
    const segments: ProvenancedTextSegment[] = [
      { text: "平台说明 TOP_SECRET_PERSONA", category: "platform_instruction" },
      { text: "中文段落🎉𝐀 Surrogate", category: "persona" },
      { text: "## Memory\n\n- TOP_SECRET_MEMORY", category: "memory_context" },
    ];
    const rendered = renderProvenancedText(segments, "\n\n", { root: "systemPrompt" });
    expect(rendered.text).toBe(segments.map((s) => s.text).join("\n\n"));
    expect(rendered.sections).toHaveLength(3);
    for (let i = 0; i < segments.length; i++) {
      const span = rendered.sections[i].locator.span!;
      expect(rendered.text.slice(span.start, span.end)).toBe(segments[i].text);
      expect(rendered.sections[i].category).toBe(segments[i].category);
      expect(rendered.sections[i].precision).toBe("exact");
    }
  });

  it("keeps empty segments in the join (no filter, no trim)", () => {
    const texts = ["a", "", "b"];
    const rendered = renderProvenancedText(
      texts.map((text) => ({ text, category: "task_input" as const })),
      "\n\n",
    );
    expect(rendered.text).toBe(texts.join("\n\n")); // "a\n\n\n\nb"
    // 多段中的空段不产生 section，但分隔符语义原样保留。
    expect(rendered.sections).toHaveLength(2);
    expect(rendered.text.slice(
      rendered.sections[0].locator.span!.start,
      rendered.sections[0].locator.span!.end,
    )).toBe("a");
    expect(rendered.text.slice(
      rendered.sections[1].locator.span!.start,
      rendered.sections[1].locator.span!.end,
    )).toBe("b");
  });

  it("treats offsets as UTF-16 code units (surrogate pairs occupy two)", () => {
    const emoji = "🎉"; // U+1F389, surrogate pair → length 2
    const mathBold = "𝐀"; // U+1D400, surrogate pair → length 2
    const flag = "🇨🇳"; // two surrogate pairs → length 4
    const zwj = "👨‍💻"; // ZWJ sequence → length 5
    const segments: ProvenancedTextSegment[] = [
      { text: `前${emoji}`, category: "task_instruction" },
      { text: `${mathBold}${flag}${zwj}后`, category: "task_input" },
    ];
    const rendered = renderProvenancedText(segments, "|");
    expect(rendered.text).toBe(`前${emoji}|${mathBold}${flag}${zwj}后`);
    const second = rendered.sections[1].locator.span!;
    expect(rendered.text.slice(second.start, second.end))
      .toBe(`${mathBold}${flag}${zwj}后`);
    expect(second.start).toBe(1 + emoji.length + 1); // 前(1) + emoji(2) + separator(1)
  });

  it("supports message-root locators with path", () => {
    const rendered = renderProvenancedText(
      [{ text: "hello TOP_SECRET_USER", category: "current_user_input" }],
      "\n",
      { root: "messages", path: [3] },
    );
    expect(rendered.sections[0].locator.root).toBe("messages");
    expect(rendered.sections[0].locator.path).toEqual([3]);
  });
});

describe("provenance content safety", () => {
  it("carries no prompt content: JSON.stringify misses all poison markers", () => {
    const rendered = renderProvenancedText([
      { text: `persona:${POISON_MARKERS[0]}`, category: "persona" },
      { text: `memory:${POISON_MARKERS[1]}`, category: "memory_context" },
      { text: `user:${POISON_MARKERS[2]}`, category: "current_user_input" },
      { text: `tool:${POISON_MARKERS[3]}`, category: "tool_result" },
    ], "\n\n");
    const provenance = createSemanticInputProvenance("chat_context", rendered.sections);
    expect(provenance).not.toBeNull();
    const serialized = JSON.stringify(provenance);
    for (const marker of POISON_MARKERS) {
      expect(serialized.includes(marker)).toBe(false);
    }
  });

  it("sanitizer drops invalid sections fail-closed", () => {
    expect(sanitizeSemanticInputProvenance({
      schemaVersion: SEMANTIC_INPUT_PROVENANCE_SCHEMA_VERSION,
      inputShape: "chat_context",
      sections: [
        section("not_a_category", { root: "systemPrompt", span: { start: 0, end: 1 } }),
        section("persona", { root: "bad_root", span: { start: 0, end: 1 } }),
        section("persona", { root: "systemPrompt", span: { start: 5, end: 3 } }), // 倒置
        { ...section("persona", { root: "systemPrompt", span: null }), precision: "exact" }, // exact 无 span
        section("persona", { root: "systemPrompt", span: { start: 0, end: 1 } }), // 合法
      ],
    })?.sections).toEqual([
      { category: "persona", precision: "exact", role: null, source: null,
        locator: { root: "systemPrompt", span: { start: 0, end: 1 } } },
    ]);
    expect(sanitizeSemanticInputProvenance({ schemaVersion: 2, inputShape: "chat_context", sections: [] })).toBeNull();
    expect(sanitizeSemanticInputProvenance(null)).toBeNull();
  });

  it("rejects absolute paths and URLs in source ids", () => {
    expect(sanitizeSemanticInputProvenance({
      schemaVersion: SEMANTIC_INPUT_PROVENANCE_SCHEMA_VERSION,
      inputShape: "calltext",
      sections: [
        {
          ...section("persona", { root: "systemPrompt", span: { start: 0, end: 1 } }),
          source: { type: "runtime", id: "/Users/alice/memory.md" },
        },
        {
          ...section("persona", { root: "systemPrompt", span: { start: 0, end: 1 } }),
          source: { type: "runtime", id: "C:\\Users\\bob\\persona.md" },
        },
        {
          ...section("media_reference", { root: "parameters", path: ["image", 0] }),
          source: { type: "runtime", id: "https://secret.example/ref.png" },
        },
        {
          ...section("persona", { root: "systemPrompt", span: { start: 0, end: 1 } }),
          source: { type: "runtime", id: "memory.today" }, // 安全逻辑 id 放行
        },
      ],
    })?.sections).toHaveLength(1);
  });
});

describe("precision rollup + observer summary", () => {
  it("rolls up exact/partial/opaque per contract", () => {
    expect(rollupProvenancePrecision(null)).toBeNull();
    const allExact = createSemanticInputProvenance("chat_context", [
      section("persona", { root: "systemPrompt", span: { start: 0, end: 1 } }),
      section("tool_definition", { root: "tools", path: [0] }),
    ]);
    expect(rollupProvenancePrecision(allExact)).toBe("exact");
    const mixed = createSemanticInputProvenance("chat_context", [
      section("persona", { root: "systemPrompt", span: { start: 0, end: 1 } }),
      section("sdk_internal", { root: "systemPrompt", span: { start: 1, end: 2 } }, "structural"),
    ]);
    expect(rollupProvenancePrecision(mixed)).toBe("partial");
    const allOpaque = createSemanticInputProvenance("chat_context", [
      section("unknown", { root: "messages", path: [0] }, "opaque"),
    ]);
    expect(rollupProvenancePrecision(allOpaque)).toBe("opaque");
  });

  it("summary carries unique categories and opaque count only", () => {
    const provenance = createSemanticInputProvenance("chat_context", [
      section("conversation_history", { root: "messages", path: [0] }),
      section("conversation_history", { root: "messages", path: [1] }),
      section("conversation_history", { root: "messages", path: [2] }),
      section("unknown", { root: "messages", path: [3] }, "opaque"),
    ]);
    const summary = summarizeSemanticInputProvenance(provenance);
    expect(summary).toMatchObject({
      inputShape: "chat_context",
      provenancePrecision: "partial",
      inputSectionCount: 4,
      inputCategories: ["conversation_history", "unknown"],
      opaqueSectionCount: 1,
    });
    expect(summarizeSemanticInputProvenance(null)).toBeNull();
  });
});

describe("buildChatContextProvenance (MC-01/03)", () => {
  const snapshotSections: SemanticInputProvenanceSection[] = [
    section("platform_instruction", { root: "systemPrompt", span: { start: 0, end: 10 } }),
    section("persona", { root: "systemPrompt", span: { start: 12, end: 30 } }),
  ];
  const snapshot: SessionPromptProvenancePayload = {
    customPrompt: "x".repeat(30),
    sections: snapshotSections,
    appendSystemPrompt: ["extra"],
    skillNames: ["pdf-tools", "web-search"],
    agentsFileNames: ["AGENTS.md"],
  };

  it("uses snapshot sections when runtime prefix verification passes, plus SDK tail", () => {
    const context = { systemPrompt: snapshot.customPrompt + "\n\nskills tail", messages: [], tools: [] };
    const provenance = buildChatContextProvenance(context, { promptSnapshot: snapshot });
    const categories = provenance!.sections.map((s) => s.category);
    expect(categories.slice(0, 2)).toEqual(["platform_instruction", "persona"]);
    expect(categories).toContain("sdk_internal");
    expect(categories).toContain("skill_instruction");
    expect(categories).toContain("agents_file");
    expect(categories).toContain("session_instruction"); // append identity-only
    const tail = provenance!.sections.find((s) => s.category === "sdk_internal")!;
    expect(tail.locator.span).toEqual({ start: 30, end: context.systemPrompt.length });
    expect(tail.precision).toBe("structural");
  });

  it("degrades to structural session_instruction when prefix verification fails", () => {
    const provenance = buildChatContextProvenance(
      { systemPrompt: "completely different", messages: [], tools: [] },
      { promptSnapshot: snapshot },
    );
    expect(provenance!.sections).toHaveLength(1);
    expect(provenance!.sections[0]).toMatchObject({
      category: "session_instruction",
      precision: "structural",
    });
  });

  it("degrades honestly without snapshot (legacy session)", () => {
    const provenance = buildChatContextProvenance(
      { systemPrompt: "frozen legacy prompt", messages: [], tools: [] },
      {},
    );
    expect(provenance!.sections[0]).toMatchObject({
      category: "session_instruction",
      precision: "structural",
    });
  });

  it("classifies messages: history / current input (turn proof) / tool results", () => {
    const messages = [
      { role: "user", content: "早前问题" },
      { role: "assistant", content: "回答" },
      { role: "toolResult", toolCallId: "tc1", toolName: "read", content: [] },
      { role: "assistant", content: "继续" },
      { role: "user", content: "TOP_SECRET_USER 当前输入" },
    ];
    const inTurn = buildChatContextProvenance(
      { systemPrompt: "s", messages, tools: [{ name: "read" }, { name: "grep" }] },
      { promptTurn: true },
    )!;
    expect(inTurn.sections.filter((s) => s.locator.root === "messages").map((s) => s.category))
      .toEqual([
        "conversation_history",
        "conversation_history",
        "tool_result",
        "conversation_history",
        "current_user_input",
      ]);
    const toolResultSection = inTurn.sections.find((s) => s.category === "tool_result")!;
    expect(toolResultSection.source).toEqual({ type: "tool", id: "read" });
    expect(toolResultSection.role).toBe("tool");

    // 无 turn 标记（agent.continue / 无 ingress）：最后 user 消息不猜 current input。
    const noTurn = buildChatContextProvenance(
      { systemPrompt: "s", messages, tools: [] },
      { promptTurn: false },
    )!;
    expect(noTurn.sections.filter((s) => s.locator.root === "messages").map((s) => s.category))
      .toEqual([
        "conversation_history",
        "conversation_history",
        "tool_result",
        "conversation_history",
        "conversation_history",
      ]);
  });

  it("marks native summarization inputs structural (no fake exact)", () => {
    const provenance = buildChatContextProvenance(
      {
        systemPrompt: "You are a context summarization assistant…",
        messages: [{ role: "user", content: [{ type: "text", text: "<conversation>…" }] }],
        tools: [],
      },
      { nativeSummarization: true },
    )!;
    expect(provenance.sections.map((s) => `${s.category}:${s.precision}`)).toEqual([
      "task_instruction:structural",
      "task_input:structural",
    ]);
    expect(provenance.sections[0].source).toEqual({ type: "sdk", id: "pi.native-summarizer.system" });
    expect(rollupProvenancePrecision(provenance)).toBe("partial");
  });
});

describe("callText provenance normalization (MC-04)", () => {
  const systemTextOf = (message: { content?: unknown }) =>
    typeof message.content === "string" ? message.content : "";

  it("remaps system-message sections into merged systemPrompt spans and reindexes messages", () => {
    const systemPrompt = "BASE"; // len 4
    const messages = [
      { role: "system", content: "SYS_ONE_TOP_SECRET" }, // merged at [5, 5+17)
      { role: "user", content: "hello" },
      { role: "system", content: "SYS_TWO" }, // merged after "\n" at [23, 30)
      { role: "user", content: "world" },
    ];
    const input = buildCallTextFallbackProvenance({ systemPrompt, messages: [] as any });
    const explicit = createSemanticInputProvenance("calltext", [
      // system 消息段（span 相对该消息文本内）
      section("task_instruction", { root: "messages", path: [0], span: { start: 0, end: 8 } }, "structural"),
      section("task_input", { root: "messages", path: [1] }, "structural"),
      section("task_instruction", { root: "messages", path: [2] }, "structural"),
      section("task_input", { root: "messages", path: [3] }, "structural"),
    ])!;
    expect(input).not.toBeNull();
    const remapped = remapCallTextProvenance(explicit, { systemPrompt, messages, systemTextOf })!;
    // merged = "BASE" + "\n" + "SYS_ONE_TOP_SECRET"(18) + "\n" + "SYS_TWO"(7)
    const merged = systemPrompt + "\n" + "SYS_ONE_TOP_SECRET" + "\n" + "SYS_TWO";
    const sysSections = remapped.sections.filter((s) => s.locator.root === "systemPrompt");
    // 段内 span 平移：[0,8) → merged 内 [5,13)
    expect(merged.slice(5, 13)).toBe("SYS_ONE_");
    expect(sysSections.find((s) => s.locator.span?.start === 5)?.locator.span).toEqual({ start: 5, end: 13 });
    // 消息级（无段内 span）→ 整条消息文本 span：SYS_TWO 在 [24,31)
    expect(merged.slice(24, 31)).toBe("SYS_TWO");
    expect(sysSections.find((s) => s.locator.span?.start === 24)?.locator.span).toEqual({ start: 24, end: 31 });
    const msgSections = remapped.sections.filter((s) => s.locator.root === "messages");
    expect(msgSections.map((s) => s.locator.path)).toEqual([[0], [1]]);
  });

  it("fallback provenance is always structural and covers system + messages", () => {
    const provenance = buildCallTextFallbackProvenance({
      systemPrompt: "S",
      messages: [
        { role: "system", content: "SYS" },
        { role: "user", content: "U" },
      ],
    })!;
    expect(provenance.sections.map((s) => `${s.category}/${s.precision}`)).toEqual([
      "task_instruction/structural",
      "task_instruction/structural",
      "task_input/structural",
    ]);
    expect(rollupProvenancePrecision(provenance)).toBe("partial");
  });
});

describe("buildDirectSummaryProvenance (MC-10)", () => {
  it("separates conversation, custom instructions, and previous summary", () => {
    const provenance = buildDirectSummaryProvenance({
      messages: [
        { role: "user", content: "白天聊了什么" },
        { role: "assistant", content: "聊了 TOP_SECRET_MEMORY 话题" },
      ],
      customInstructions: "临时素材，不写回 session",
      previousSummary: "昨天的摘要",
    })!;
    expect(provenance.inputShape).toBe("pi_direct_summary");
    expect(provenance.sections.map((s) => s.category)).toEqual([
      "conversation_history",
      "conversation_history",
      "task_instruction",
      "previous_summary",
    ]);
    const custom = provenance.sections[2];
    expect(custom.locator).toEqual({ root: "parameters", path: ["customInstructions"] });
    expect(custom.source).toEqual({ type: "runtime", id: "diary.temporary-summary.instructions" });
    expect(rollupProvenancePrecision(provenance)).toBe("exact");
  });
});

describe("recorder provenance sidecar (§四十/§三十九)", () => {
  it("attaches provenance, emits summary in logical_call_start, and carries map via symbol ref only", () => {
    const observer = createTestModelCallObserver();
    const recorder = createModelCallRecorder({ observer });
    const rendered = renderProvenancedText(
      [{ text: "TOP_SECRET_PERSONA", category: "persona" }],
      "\n",
    );
    recorder.attachSemanticInputProvenance(
      createSemanticInputProvenance("chat_context", rendered.sections),
    );
    recorder.beginLogicalCall({ details: { path: "test" } });
    recorder.beginAttempt();
    recorder.endLogicalCall("ok");

    const start = observer.eventsOfType("logical_call_start")[0];
    expect(start.details).toMatchObject({
      path: "test",
      inputShape: "chat_context",
      provenancePrecision: "exact",
      inputSectionCount: 1,
      inputCategories: ["persona"],
      opaqueSectionCount: 0,
    });
    // details 不含 provenance map（毒丸不出现），map 经 symbol 引用可取。
    expect(JSON.stringify(start.details).includes("TOP_SECRET_PERSONA")).toBe(false);
    expect(observer.provenanceForCall(recorder.callId)?.sections[0].category).toBe("persona");
    expect(observer.categoriesForCall(recorder.callId)).toEqual(["persona"]);
    // 事件 JSON 序列化同样不泄漏（symbol 键跳过）。
    observer.assertNoSensitiveContent(["TOP_SECRET_PERSONA"]);
  });

  it("sanitize fail-closed: invalid provenance attaches as null", () => {
    const observer = createTestModelCallObserver();
    const recorder = createModelCallRecorder({ observer });
    recorder.attachSemanticInputProvenance({ bogus: true });
    recorder.beginLogicalCall();
    recorder.endLogicalCall("ok");
    const start = observer.eventsOfType("logical_call_start")[0];
    expect(start.details?.inputShape).toBeUndefined();
    expect(observer.provenanceForCall(recorder.callId)).toBeNull();
  });
});
