import { describe, expect, it } from "vitest";
import { ReservedTagScanner, splitReservedTagSegments } from "../shared/reserved-tag-stream.ts";
import { MoodParser, ThinkTagParser } from "../core/events.ts";

/**
 * 词表外孤儿闭标签形状规则（方言无关兜底）：
 * 没有打开标签时遇到的收尾标签，若名字不在扫描器词表内，是模型思考方言的
 * 协议残渣，一律吞掉。背景 bug：MiniMax M3 的 </mm:think> 闭标签不在词表里，
 * 旧实现按字面文本放行，正文和折叠区出现泄漏（Windows 实测）。
 *
 * 分层契约：解析链中间层（ThinkTagParser）必须关闭此规则——`</mood>` 对
 * think 层就是「词表外闭标签」，吞掉会破坏下游 MoodParser；规则只在链尾
 * （MoodParser）与最终文本边界（splitReservedTagSegments）开启。
 */

const THINK_VOCAB = ["think", "thinking"];

function scannerText(input: string, chunks = [input]): string {
  const scanner = new ReservedTagScanner(THINK_VOCAB, { dropUnknownOrphanClosers: true });
  let text = "";
  for (const chunk of chunks) {
    for (const token of scanner.feed(chunk)) {
      if (token.type === "text") text += token.text;
    }
  }
  for (const token of scanner.flush()) {
    if (token.type === "text") text += token.text;
  }
  return text;
}

describe("ReservedTagScanner 词表外孤儿闭标签形状规则", () => {
  it("词表外的孤儿闭标签按残渣吞掉（mm:think 方言，单次喂入）", () => {
    expect(scannerText("</mm:think>已提交，图片生成完会自动出来。")).toBe(
      "已提交，图片生成完会自动出来。",
    );
  });

  it("连续两个词表外孤儿闭标签全部吞掉（模型连发收尾标签的实测形态）", () => {
    expect(scannerText("</mm:think></mm:think>好的，这就把它做成视频。")).toBe(
      "好的，这就把它做成视频。",
    );
  });

  it("跨 delta 撕裂的词表外闭标签也能识别并吞掉", () => {
    expect(scannerText("正文</mm", ["正文", "</mm", ":think>尾巴"])).toBe("正文尾巴");
  });

  it("词表内的孤儿闭标签保持字面透传（转义/教学契约，残渣由链尾兜底）", () => {
    expect(scannerText("</think>好的")).toBe("</think>好的");
  });

  it("链尾 MoodParser 吞掉词表外孤儿闭标签（mm:think 实测泄漏形态）", () => {
    const parser = new MoodParser();
    const texts: string[] = [];
    const moods: string[] = [];
    parser.feed("工具干完了</mm:think>", (event) => {
      const evt = event as { type: string; data?: string };
      if (evt.type === "text") texts.push(evt.data ?? "");
      if (evt.type === "mood_text") moods.push(evt.data ?? "");
    });
    parser.flush((event) => {
      const evt = event as { type: string; data?: string };
      if (evt.type === "text") texts.push(evt.data ?? "");
      if (evt.type === "mood_text") moods.push(evt.data ?? "");
    });
    expect(texts.join("")).toBe("工具干完了");
    expect(moods.join("")).toBe("");
  });

  it("链头 ThinkTagParser 透传下游结构标签（</mood> 不得被误吞）", () => {
    const think = new ThinkTagParser();
    const downstream: string[] = [];
    think.feed("<mood>A</mood>正文", (event) => {
      const evt = event as { type: string; data?: string };
      if (evt.type === "text") downstream.push(evt.data ?? "");
    });
    think.flush(() => {});
    expect(downstream.join("")).toBe("<mood>A</mood>正文");
  });

  it("成对的 mm:think 方言标签结构化，内容进 think 通道", () => {
    const parser = new ThinkTagParser();
    const events: Array<{ type: string; data?: string }> = [];
    parser.feed("<mm:think>内心戏</mm:think>正文", (event) => events.push(event as any));
    parser.flush((event) => events.push(event as any));
    expect(events).toEqual([
      { type: "think_start" },
      { type: "think_text", data: "内心戏" },
      { type: "think_end" },
      { type: "text", data: "正文" },
    ]);
  });

  it("代码保护区内的闭标签仍按字面保留（教学场景不受影响）", () => {
    expect(scannerText("闭标签写作 `</mm:think>`")).toBe("闭标签写作 `</mm:think>`");
    expect(scannerText("示例：\n```html\n</div>\n```\n完")).toBe("示例：\n```html\n</div>\n```\n完");
  });

  it("普通小于号与数学比较不受影响", () => {
    expect(scannerText("5 < 6 且 a<b")).toBe("5 < 6 且 a<b");
  });

  it("转义的字面闭标签（词表内）仍按契约输出字面量", () => {
    // 转义反斜杠在 canonical source 中保留，显示层交给 Markdown 词法处理。
    expect(scannerText("\\</think>")).toBe("\\</think>");
  });
});

describe("splitReservedTagSegments 词表外孤儿闭标签清理（历史渲染兜底）", () => {
  it("旧落盘正文里的孤儿闭标签在全文切分时被清掉", () => {
    const segments = splitReservedTagSegments("前文</mm:think>后文</mm:think>尾", THINK_VOCAB);
    const text = segments.map(s => (s.type === "text" ? s.text : `[${s.tag}]`)).join("");
    expect(text).toBe("前文后文尾");
  });

  it("词表内的成对标签在全文切分时成为结构化块", () => {
    const segments = splitReservedTagSegments("<think>秘密</think>露出的部分", THINK_VOCAB);
    expect(segments).toEqual([
      { type: "block", tag: "think", content: "秘密" },
      { type: "text", text: "露出的部分" },
    ]);
  });
});
