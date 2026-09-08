import { describe, expect, it } from "vitest";
import { buildFaithfulPasteContent, serializeEditor } from "../desktop/src/react/utils/editor-serializer.ts";
import type { JSONContent } from "@tiptap/core";

/**
 * F10/P7.2/P7.3：粘贴到发送的文本保真（P01–P04、P06 的序列化层断言）。
 *
 * N(text) = 既有剪贴板换行编码规则（CRLF／CR → LF）。除这一归一外，
 * 非空正文必须原样保留首尾空格、tab、首尾换行、空行、连续空格与全角空格。
 * 空白判空是独立谓词（调用方用 trim 副本判断），不是序列化器的行为。
 * 比较 N(剪贴板输入) 与最终序列化文本；不许把双方同时 trim 后比较冒充保真。
 */
function roundTrip(text: string): string {
  const doc: JSONContent = { type: "doc", content: [buildFaithfulPasteContent(text)] };
  return serializeEditor(doc).text;
}

describe("buildFaithfulPasteContent 粘贴保真", () => {
  it("P01: 围栏代码块 + 空行 + 缩进（首行四个空格开头）逐字符还原", () => {
    const source = [
      "前言。",
      "",
      "```bash",
      "for sub in .config .cache; do",
      "    scan_dir \"文件\" \"$H/$sub\";",
      "done",
      "",
      "if [[ \"$OS\" == \"Darwin\" ]]; then",
      "  echo '== 保留 =='",
      "fi",
      "```",
      "后记。",
    ].join("\n");
    expect(roundTrip(source)).toBe(source);
  });

  it("P02: tab 开头、首尾空行、尾部多个空格完整保留", () => {
    const source = "\n\n\t缩进行与  双空格\n尾行   \n\n";
    expect(roundTrip(source)).toBe(source);
  });

  it("P03: Python／Shell 围栏、多空行、中文、emoji 的 N(text) 往返一致", () => {
    const source = [
      "```python",
      "def f():",
      "    return '灵犀 🌸'",
      "```",
      "",
      "",
      "中文正文……",
      "```sh",
      "echo 'a  b'",
      "```",
    ].join("\n");
    expect(roundTrip(source)).toBe(source);
  });

  it("P04: CRLF 与 CR 输入只做既定 LF 归一，无其他变换", () => {
    expect(roundTrip("第一行\r\n\r\n```python\r\nprint('hi')\r\n```\r\n尾行")).toBe(
      "第一行\n\n```python\nprint('hi')\n```\n尾行",
    );
    expect(roundTrip("旧式 Mac\r行")).toBe("旧式 Mac\n行");
  });

  it("P02b: 全角空格与行内连续空格逐字符保留", () => {
    const source = "全角　空格 和   连续半角";
    expect(roundTrip(source)).toBe(source);
  });

  it("P06: 空 paragraph 与连续 hardBreak 序列化为可见空行，不被过滤", () => {
    const doc: JSONContent = {
      type: "doc",
      content: [
        { type: "paragraph", content: [{ type: "text", text: "第一段" }] },
        { type: "paragraph" },
        { type: "paragraph" },
        {
          type: "paragraph",
          content: [
            { type: "text", text: "行一" },
            { type: "hardBreak" },
            { type: "hardBreak" },
            { type: "text", text: "行三" },
          ],
        },
        { type: "paragraph", content: [{ type: "text", text: "   " }] },
      ],
    };
    expect(serializeEditor(doc).text).toBe(
      "第一段\n\n\n行一\n\n行三\n   ",
    );
  });

  it("首尾空行不再被发送侧 trim 收口（F10/P7.3 修正旧断言）", () => {
    // 旧断言把「首尾空行被丢弃」当正确行为；契约升级后正文原样保留，
    // 空白判空由调用方独立谓词负责（见 InputArea composerPayloadIsEmpty）。
    expect(roundTrip("\n\n中间\n\n")).toBe("\n\n中间\n\n");
  });

  it("技能徽章序列化不受粘贴保真影响（徽章仍被提取而非当文本）", () => {
    const doc: JSONContent = {
      type: "doc",
      content: [
        { type: "paragraph", content: [{ type: "skillBadge", attrs: { name: "翻译" } }] },
        { type: "paragraph", content: [{ type: "text", text: "正文" }] },
      ],
    };
    const { text, skills } = serializeEditor(doc);
    expect(text).toBe("正文");
    expect(skills).toEqual(["翻译"]);
  });

  it("多段落文档：段落间换行数与空段一一对应（无 trim 兜底）", () => {
    const doc: JSONContent = {
      type: "doc",
      content: [
        { type: "paragraph", content: [{ type: "text", text: "  前导空格" }] },
        { type: "paragraph", content: [{ type: "text", text: "第二段" }] },
      ],
    };
    expect(serializeEditor(doc).text).toBe("  前导空格\n第二段");
  });
});
