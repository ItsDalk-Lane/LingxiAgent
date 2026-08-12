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
    expect(collect(["\uFEFF \n<pul", "se>inside</pu", "lse>\nafter"])).toEqual([
      { type: "text", data: "\uFEFF \n" },
      { type: "mood_start" },
      { type: "mood_text", data: "inside" },
      { type: "mood_end" },
      { type: "text", data: "after" },
    ]);
  });

  it.each(["mood", "pulse", "reflect"])(
    "keeps an inline-code <%s> tag and all following prose visible",
    (tag) => {
      const input = `\`<${tag}>\` suffix survives`;
      const events = collect([input]);
      expect(events.map((event) => event.type)).toEqual(["text"]);
      expect(visibleText(events)).toBe(input);
    },
  );

  it("permanently treats later tags as text once visible prose begins, including across chunks", () => {
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

  it("does not reopen mood parsing after a valid leading block", () => {
    expect(collect(["<mood>inside</mood>\nafter <pulse>literal</pulse>"])).toEqual([
      { type: "mood_start" },
      { type: "mood_text", data: "inside" },
      { type: "mood_end" },
      { type: "text", data: "after <pulse>literal</pulse>" },
    ]);
  });

  it("does not reopen within a segment even after flush (leading-only safety holds)", () => {
    // 同一段可见正文内，flush / feed 之间再次出现的标签仍必须保持普通正文。
    const parser = new MoodParser();
    const events: Array<{ type: string; data?: string }> = [];
    parser.feed("<mood>A</mood>正文", (e) => events.push(e));
    parser.feed("<reflect>literal</reflect>", (e) => events.push(e));
    parser.flush((e) => events.push(e));
    // 只允许一次 mood cycle（首段的 A），第二段标签不能再触发 mood_start
    expect(events.filter((e) => e.type === "mood_start")).toHaveLength(1);
    expect(events.filter((e) => e.type === "mood_end")).toHaveLength(1);
    const visible = events.filter((e) => e.type === "text").map((e) => e.data || "").join("");
    expect(visible).toBe("正文<reflect>literal</reflect>");
  });

  it("re-arms leading mood eligibility on an explicit new assistant segment", () => {
    // 契约2：显式的下一段 assistant 生成可以重新识别 leading 内部块。
    // 对应工具循环：segment1 <reflect>A</reflect> → 工具 → segment2 <reflect>B</reflect>。
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
    // 三种协议都必须在新 segment 重新工作（任务要求不能只特判 reflect）。
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

  it("requires the closer to match the leading opener", () => {
    expect(collect(["<mood>inside</pulse>still mood</mood>after"])).toEqual([
      { type: "mood_start" },
      { type: "mood_text", data: "inside</pulse>still mood" },
      { type: "mood_end" },
      { type: "text", data: "after" },
    ]);
  });

  it("preserves flush behavior for an unclosed valid leading block", () => {
    expect(collect(["<mood>unfinished"])).toEqual([
      { type: "mood_start" },
      { type: "mood_text", data: "unfinished" },
      { type: "mood_end" },
    ]);
  });
});
