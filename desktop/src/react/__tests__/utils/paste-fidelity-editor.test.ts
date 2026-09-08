// @vitest-environment jsdom

/**
 * F10/P7.3：真实 TipTap 编辑器上的粘贴插入与选区处理（P05/P09/P11），
 * 以及 wire → 历史/复制边界的正文保真（P12）。
 * 不只构造 JSON 调 serializeEditor——这里用真实 Editor 与真实 schema。
 */

import { describe, expect, it } from 'vitest';
import { Editor } from '@tiptap/core';
import { createInputEditorExtensions } from '../../components/input/input-editor-extensions';
import {
  insertFaithfulPasteAtSelection,
  serializeEditor,
} from '../../utils/editor-serializer';
import { extractPlainUrlPaste } from '../../utils/plain-url-paste';
import { buildItemsFromHistory } from '../../utils/history-builder';

function makeEditor(html?: string): Editor {
  const editor = new Editor({
    extensions: createInputEditorExtensions(''),
    ...(html ? { content: html } : {}),
  });
  return editor;
}

function placeCaret(editor: Editor, afterText: string): void {
  let match = -1;
  editor.state.doc.descendants((node, pos) => {
    if (match >= 0 || !node.isText || !node.text?.includes(afterText)) return match < 0;
    match = pos + node.text.indexOf(afterText) + afterText.length;
    return false;
  });
  if (match < 0) throw new Error(`text not found: ${afterText}`);
  editor.commands.setTextSelection(match);
}

function selectText(editor: Editor, text: string): void {
  let from = -1;
  let to = -1;
  editor.state.doc.descendants((node, pos) => {
    if (from >= 0 || !node.isText || !node.text?.includes(text)) return from < 0;
    from = pos + node.text.indexOf(text);
    to = from + text.length;
    return false;
  });
  if (from < 0) throw new Error(`text not found: ${text}`);
  editor.commands.setTextSelection({ from, to });
}

describe('F10/P7.3 真实编辑器粘贴插入（P05）', () => {
  it('段落中间粘贴：只插入目标字符，不把当前段落拆成多段', () => {
    const editor = makeEditor('<p>前abc后</p>');
    placeCaret(editor, 'a');
    insertFaithfulPasteAtSelection(editor, '粘\n贴');
    expect(serializeEditor(editor.getJSON()).text).toBe('前a粘\n贴bc后');
    // 单段：插入不引入额外段落边界
    expect(editor.getJSON().content).toHaveLength(1);
    editor.destroy();
  });

  it('选区替换：替换选中的字符，首尾不多生换行', () => {
    const editor = makeEditor('<p>前abc后</p>');
    selectText(editor, 'bc');
    insertFaithfulPasteAtSelection(editor, 'X\nY');
    expect(serializeEditor(editor.getJSON()).text).toBe('前aX\nY后');
    expect(editor.getJSON().content).toHaveLength(1);
    editor.destroy();
  });

  it('空编辑器：粘贴多行内容原样落地（首尾空行保留）', () => {
    const editor = makeEditor();
    insertFaithfulPasteAtSelection(editor, '\nA\n\nB\n');
    expect(serializeEditor(editor.getJSON()).text).toBe('\nA\n\nB\n');
    editor.destroy();
  });

  it('列表环境：粘贴进列表项不拆出列表，续行留在同一项', () => {
    const editor = makeEditor('<ul><li><p>项目</p></li></ul>');
    placeCaret(editor, '项目');
    insertFaithfulPasteAtSelection(editor, '，二\n行');
    const text = serializeEditor(editor.getJSON()).text;
    expect(text).toBe('- 项目，二\n  行');
    // 没有内容被拆出到顶层段落（TipTap 的 schema 垫尾空段除外）
    const json = editor.getJSON();
    expect((json.content || []).filter((block: any) => block.type === 'paragraph' && block.content?.length)).toHaveLength(0);
    expect(json.content?.[0]?.type).toBe('bulletList');
    editor.destroy();
  });

  it('多行粘贴含空行：空行是连续 hardBreak，序列化往返零丢失', () => {
    const editor = makeEditor('<p>x</p>');
    placeCaret(editor, 'x');
    insertFaithfulPasteAtSelection(editor, '一\n\n二');
    expect(serializeEditor(editor.getJSON()).text).toBe('x一\n\n二');
    editor.destroy();
  });
});

describe('F10/P7.3 富 URL 分支与纯文本保真分支（P09）', () => {
  function clipboard(fields: Record<string, string>): ClipboardEvent['clipboardData'] {
    return {
      getData: (type: string) => fields[type] ?? '',
    } as ClipboardEvent['clipboardData'];
  }

  it('复制超链接（text/html 带 href）→ 走富 URL 分支取 href', () => {
    const data = clipboard({
      'text/html': '<a href="https://example.com/article">文章标题</a>',
      'text/plain': '文章标题',
    });
    expect(extractPlainUrlPaste(data)).toBe('https://example.com/article');
  });

  it('普通多行含 URL 文本（无 text/html）→ 不进富 URL 分支，整段按纯文本保真', () => {
    const data = clipboard({
      'text/plain': '见 https://example.com/a\n第二行\n\n第三行',
    });
    expect(extractPlainUrlPaste(data)).toBeNull();
    const editor = makeEditor();
    insertFaithfulPasteAtSelection(editor, '见 https://example.com/a\n第二行\n\n第三行');
    expect(serializeEditor(editor.getJSON()).text).toBe('见 https://example.com/a\n第二行\n\n第三行');
    editor.destroy();
  });
});

describe('F10/P7.3 转写式插入不解释 HTML（P11）', () => {
  it('含 <...> 的文本按纯文本节点插入，不产生对应节点/标记', () => {
    const editor = makeEditor();
    insertFaithfulPasteAtSelection(editor, '第一行 <b>加粗</b>\n<think>不成块');
    const json = editor.getJSON();
    expect(serializeEditor(json).text).toBe('第一行 <b>加粗</b>\n<think>不成块');
    // 没有 bold mark、没有 think 节点：标签只是文本
    const marks = new Set<string>();
    const types = new Set<string>();
    const walk = (node: unknown) => {
      const n = node as { type?: string; marks?: Array<{ type: string }>; content?: unknown[]; text?: string };
      if (n.type) types.add(n.type);
      for (const mark of n.marks || []) marks.add(mark.type);
      for (const child of n.content || []) walk(child);
    };
    walk(json);
    expect(types.has('bold')).toBe(false);
    expect(types.has('hardBreak')).toBe(true);
    expect(marks.size).toBe(0);
    editor.destroy();
  });
});

describe('F10/P7.4 wire → 落盘/历史/复制边界（P12）', () => {
  it('用户正文在历史投影中原样保留，系统追加内容独立成块', () => {
    // 服务端落盘形态：技能调用前缀是协议追加块，其后是用户原文（含首尾空白）
    const persisted = '[Use skill: 翻译]\n  请保留我的空格  \n和空行\n';
    const items = buildItemsFromHistory({
      messages: [
        { id: '0', entryId: 'e0', role: 'user', content: persisted },
      ],
    } as never);
    const userItem = items.find((item: any) => item.type === 'message' && item.data.role === 'user') as any;
    expect(userItem).toBeTruthy();
    // 系统追加块被解析为技能胶囊（独立维度），剩余正文逐字符保留
    expect(userItem.data.skills).toEqual(['翻译']);
    expect(userItem.data.text).toBe('  请保留我的空格  \n和空行\n');
  });

  it('无系统追加块的纯正文：首尾空格、tab 与空行在历史投影逐字符保留', () => {
    const persisted = '\n  开头两空格\n\ttab 行\n\n尾行  ';
    const items = buildItemsFromHistory({
      messages: [
        { id: '0', entryId: 'e0', role: 'user', content: persisted },
      ],
    } as never);
    const userItem = items.find((item: any) => item.type === 'message' && item.data.role === 'user') as any;
    expect(userItem.data.text).toBe(persisted);
  });
});
