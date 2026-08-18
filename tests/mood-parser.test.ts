import { describe, expect, it } from "vitest";
import { MoodParser } from "../core/events.ts";

function collect(chunks: string[]) {
  const parser = new MoodParser();
  const events: Array<{ type: string; data?: string }> = [];
  for (const chunk of chunks) parser.feed(chunk, (event) => events.push(event));
  parser.flush((event) => events.push(event));
  return events;
}

function visibleText(events: Array<{ type: string; data?: string }>) {
  return events.filter((event) => event.type === "text").map((event) => event.data || "").join("");
}

describe("MoodParser", () => {
  it.each(["mood", "pulse", "reflect"])(
    "parses a leading <%s> block with the existing stream event contract",
    (tag) => {
      expect(collect([`<${tag}>inside</${tag}>\nafter`])).toEqual([
        { type: "mood_start" },
        { type: "mood_text", data: "inside" },
        { type: "mood_end" },
        { type: "text", data: "after" },
      ]);
    },
  );

  it("recognizes a leading block across chunks after optional BOM and whitespace", () => {
    expect(collect(["﻿ \n<pul", "se>inside</pu", "lse>\nafter"])).toEqual([
      { type: "text", data: "﻿ \n" },
      { type: "mood_start" },
      { type: "mood_text", data: "inside" },
      { type: "mood_end" },
      { type: "text", data: "after" },
    ]);
  });

  it("parses reserved tags anywhere in a generation, not just at the leading position", () => {
    // 新契约：保留标签出现在正文中间同样是协议，一轮里允许多个 mood 块。
    expect(collect(["<mood>A</mood>一些过程<mood>B</mood>最终回答"])).toEqual([
      { type: "mood_start" },
      { type: "mood_text", data: "A" },
      { type: "mood_end" },
      { type: "text", data: "一些过程" },
      { type: "mood_start" },
      { type: "mood_text", data: "B" },
      { type: "mood_end" },
      { type: "text", data: "最终回答" },
    ]);
  });

  it("parses a prose-internal tag across chunks", () => {
    const events = collect(["先做了一点事 <mo", "od>中途</mood> 继续"]);
    expect(events).toEqual([
      { type: "text", data: "先做了一点事 " },
      { type: "mood_start" },
      { type: "mood_text", data: "中途" },
      { type: "mood_end" },
      { type: "text", data: " 继续" },
    ]);
  });

  it("parses a second tag after a valid leading block within the same segment", () => {
    expect(collect(["<mood>inside</mood>\nafter <pulse>literal</pulse>"])).toEqual([
      { type: "mood_start" },
      { type: "mood_text", data: "inside" },
      { type: "mood_end" },
      { type: "text", data: "after " },
      { type: "mood_start" },
      { type: "mood_text", data: "literal" },
      { type: "mood_end" },
    ]);
  });

  it("keeps parsing after flush within a segment (tags are reserved anywhere)", () => {
    const parser = new MoodParser();
    const events: Array<{ type: string; data?: string }> = [];
    parser.feed("<mood>A</mood>正文", (e) => events.push(e));
    parser.feed("<reflect>literal</reflect>", (e) => events.push(e));
    parser.flush((e) => events.push(e));
    expect(events.filter((e) => e.type === "mood_start")).toHaveLength(2);
    expect(events.filter((e) => e.type === "mood_end")).toHaveLength(2);
    const visible = events.filter((e) => e.type === "text").map((e) => e.data || "").join("");
    expect(visible).toBe("正文");
  });

  it.each(["mood", "pulse", "reflect"])(
    "keeps an inline-code <%s> tag and all following prose visible",
    (tag) => {
      // 行内代码是受保护位置：标签在反引号内一律按字面文本透传。
      const input = `\`<${tag}>\` suffix survives`;
      const events = collect([input]);
      expect(events.map((event) => event.type)).toEqual(["text"]);
      expect(visibleText(events)).toBe(input);
    },
  );

  it("keeps code-protected tags literal across chunks", () => {
    const input = "prefix `<mood>` suffix";
    const events = collect(["prefix `<mo", "od>` suffix"]);
    expect(events.every((event) => event.type === "text")).toBe(true);
    expect(visibleText(events)).toBe(input);
  });

  it("keeps fenced Markdown tags visible", () => {
    const input = "```xml\n<mood>literal</mood>\n```\nafter";
    const events = collect([input]);
    expect(events.every((event) => event.type === "text")).toBe(true);
    expect(visibleText(events)).toBe(input);
  });

  it("parses tags again after a fence closes", () => {
    const events = collect(["```\n<mood>literal</mood>\n```\n<mood>real</mood>"]);
    expect(events).toEqual([
      { type: "text", data: "```\n<mood>literal</mood>\n```\n" },
      { type: "mood_start" },
      { type: "mood_text", data: "real" },
      { type: "mood_end" },
    ]);
  });

  it("treats an escaped opener as literal text", () => {
    const events = collect(["\\<mood>A</mood>"]);
    expect(events.every((event) => event.type === "text")).toBe(true);
    // 反斜杠被消费，标签按字面量保留
    expect(visibleText(events)).toBe("<mood>A</mood>");
  });

  it("treats tag content as opaque: the first matching closer ends the block", () => {
    const events = collect(["<mood>A\\</mood>B</mood>after"]);
    // 内容不透明：第一个同名闭标签即关闭；之后孤立的闭标签按字面文本处理
    expect(events).toEqual([
      { type: "mood_start" },
      { type: "mood_text", data: "A\\" },
      { type: "mood_end" },
      { type: "text", data: "B</mood>after" },
    ]);
  });

  it("handles an escape split across chunks", () => {
    const events = collect(["文字\\", "<mood>A</mood>"]);
    expect(events.every((event) => event.type === "text")).toBe(true);
    expect(visibleText(events)).toBe("文字<mood>A</mood>");
  });

  it("re-arms leading mood eligibility on an explicit new assistant segment", () => {
    // 工具循环：segment1 <reflect>A</reflect> → 工具 → segment2 <reflect>B</reflect>。
    const parser = new MoodParser();
    const events: Array<{ type: string; data?: string }> = [];
    parser.feed("<reflect>A</reflect>我先查一下", (e) => events.push(e));
    // 工具执行结束、下一段模型生成开始的显式边界：
    parser.beginAssistantSegment();
    parser.feed("<reflect>B</reflect>最终答案", (e) => events.push(e));
    parser.flush((e) => events.push(e));
    expect(events).toEqual([
      { type: "mood_start" },
      { type: "mood_text", data: "A" },
      { type: "mood_end" },
      { type: "text", data: "我先查一下" },
      { type: "mood_start" },
      { type: "mood_text", data: "B" },
      { type: "mood_end" },
      { type: "text", data: "最终答案" },
    ]);
  });

  it("re-arm is tag-agnostic across mood / pulse / reflect", () => {
    for (const [first, second] of [["mood", "reflect"], ["pulse", "mood"], ["reflect", "pulse"]] as const) {
      const parser = new MoodParser();
      const events: Array<{ type: string; data?: string }> = [];
      parser.feed(`<${first}>1</${first}>t1`, (e) => events.push(e));
      parser.beginAssistantSegment();
      parser.feed(`<${second}>2</${second}>t2`, (e) => events.push(e));
      parser.flush((e) => events.push(e));
      const moodTexts = events.filter((e) => e.type === "mood_text").map((e) => e.data || "");
      expect(moodTexts).toEqual(["1", "2"]);
      const visible = events.filter((e) => e.type === "text").map((e) => e.data || "").join("");
      expect(visible).toBe("t1t2");
    }
  });

  it("requires the closer to match the opener", () => {
    expect(collect(["<mood>inside</pulse>still mood</mood>after"])).toEqual([
      { type: "mood_start" },
      { type: "mood_text", data: "inside</pulse>still mood" },
      { type: "mood_end" },
      { type: "text", data: "after" },
    ]);
  });

  it("preserves flush behavior for an unclosed block", () => {
    expect(collect(["<mood>unfinished"])).toEqual([
      { type: "mood_start" },
      { type: "mood_text", data: "unfinished" },
      { type: "mood_end" },
    ]);
  });
});
