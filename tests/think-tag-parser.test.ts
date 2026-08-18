import { describe, expect, it } from "vitest";
import { ThinkTagParser } from "../core/events.ts";

function collect(input, chunks = [input]) {
  const parser = new ThinkTagParser();
  const events = [];
  for (const chunk of chunks) parser.feed(chunk, (event) => events.push(event));
  parser.flush((event) => events.push(event));
  return events;
}

describe("ThinkTagParser", () => {
  it("parses provider-emitted leading think tags as thinking", () => {
    expect(collect("<think>internal</think>\nvisible")).toEqual([
      { type: "think_start" },
      { type: "think_text", data: "internal" },
      { type: "think_end" },
      { type: "text", data: "visible" },
    ]);
  });

  it("parses think tags anywhere in the text (reserved protocol, not leading-only)", () => {
    // 新契约：<think> 出现在正文中间同样是协议，一律结构化为 thinking。
    expect(collect("正文里提到 <think> 标签时，后续内容不能被吞。")).toEqual([
      { type: "text", data: "正文里提到 " },
      { type: "think_start" },
      { type: "think_text", data: " 标签时，后续内容不能被吞。" },
      { type: "think_end" },
    ]);
  });

  it("parses a mid-text think block with a closer", () => {
    expect(collect("先说<think>想一下</think>再说")).toEqual([
      { type: "text", data: "先说" },
      { type: "think_start" },
      { type: "think_text", data: "想一下" },
      { type: "think_end" },
      { type: "text", data: "再说" },
    ]);
  });

  it("keeps inline-code think tags visible as normal text", () => {
    // 需要字面量时由代码保护（或转义）表达，而不是靠位置侥幸。
    expect(collect("正文里提到 `<think>` 标签时，后续内容不能被吞。")).toEqual([
      { type: "text", data: "正文里提到 `<think>` 标签时，后续内容不能被吞。" },
    ]);
  });

  it("treats an escaped think tag as literal text", () => {
    expect(collect("正文里提到 \\<think> 标签")).toEqual([
      { type: "text", data: "正文里提到 <think> 标签" },
    ]);
  });

  it("keeps ordinary reasoning prose visible unless it is in a structured channel", () => {
    expect(collect("思考过程：先比较方案。最终答案：选择 A。")).toEqual([
      { type: "text", data: "思考过程：先比较方案。最终答案：选择 A。" },
    ]);
  });

  it("holds a trailing partial tag across chunks and parses it when completed", () => {
    const chunks = ["正文里提到 <thi", "nk> 标签"];
    expect(collect(chunks.join(""), chunks)).toEqual([
      { type: "text", data: "正文里提到 " },
      { type: "think_start" },
      { type: "think_text", data: " 标签" },
      { type: "think_end" },
    ]);
  });
});
