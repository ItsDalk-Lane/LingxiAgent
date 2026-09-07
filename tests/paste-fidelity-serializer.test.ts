import { describe, expect, it } from "vitest";
import { buildFaithfulPasteContent, serializeEditor } from "../desktop/src/react/utils/editor-serializer.ts";
import type { JSONContent } from "@tiptap/core";

/**
 * 粘贴保真：剪贴板纯文本经「编辑器内容 → 序列化」往返后逐字符还原。
 * 背景 bug：富文本编辑器没有代码块节点，粘贴的 Markdown 被拍平，
 * 代码围栏丢失后渲染端把脚本当普通文本解释（-- 变破折号、== 变高亮）。
 */
function roundTrip(text: string): string {
  const doc: JSONContent = { type: "doc", content: [buildFaithfulPasteContent(text)] };
  return serializeEditor(doc).text;
}

describe("buildFaithfulPasteContent 粘贴保真", () => {
  it("围栏代码块 + 空行 + 缩进逐字符还原", () => {
    const source = [
      "前言一句。",
      "",
      "```bash",
      "for sub in .config .cache; do",
      "  scan_dir \"文件\" \"$H/$sub\";",
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

  it("CRLF 粘贴归一为 LF，行数与空行结构不变", () => {
    expect(roundTrip("第一行\r\n\r\n```python\r\nprint('hi')\r\n```\r\n尾行")).toBe(
      "第一行\n\n```python\nprint('hi')\n```\n尾行",
    );
  });

  it("首尾空行被发送侧 trim 收口（与手打行为一致）", () => {
    expect(roundTrip("\n\n中间\n\n")).toBe("中间");
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
});
