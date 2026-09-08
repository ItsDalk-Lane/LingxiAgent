// @vitest-environment jsdom

import { Editor, type JSONContent } from '@tiptap/core';
import { describe, expect, it } from 'vitest';
import { createInputEditorExtensions } from '../../components/input/input-editor-extensions';
import { serializeEditor } from '../../utils/editor-serializer';

function makeEditor(content?: string | JSONContent): Editor {
  return new Editor({
    extensions: createInputEditorExtensions(''),
    ...(content === undefined ? {} : { content }),
  });
}

describe('R06 末尾空段内容所有权', () => {
  it('R06-01：普通文档的两个用户尾空段逐个序列化', () => {
    const result = serializeEditor({
      type: 'doc',
      content: [
        { type: 'paragraph', content: [{ type: 'text', text: 'hello' }] },
        { type: 'paragraph' },
        { type: 'paragraph' },
      ],
    });
    expect(result.text).toBe('hello\n\n');
  });

  it('R06-02：首中尾空段、tab、全角空格和连续尾空段全部保留', () => {
    const result = serializeEditor({
      type: 'doc',
      content: [
        { type: 'paragraph' },
        { type: 'paragraph', content: [{ type: 'text', text: '\t中　间' }] },
        { type: 'paragraph' },
        { type: 'paragraph', content: [{ type: 'text', text: '尾　' }] },
        { type: 'paragraph' },
        { type: 'paragraph' },
      ],
    });
    expect(result.text).toBe('\n\t中　间\n\n尾　\n\n');
  });

  it('R06-03：尾部 hardBreak 与尾部 paragraph 表达的换行都保留', () => {
    const result = serializeEditor({
      type: 'doc',
      content: [
        {
          type: 'paragraph',
          content: [{ type: 'text', text: 'hello' }, { type: 'hardBreak' }],
        },
        { type: 'paragraph' },
      ],
    });
    expect(result.text).toBe('hello\n\n');
  });

  it('R06-04：真实 TipTap splitBlock 创建的用户尾空段进入实际 source', () => {
    const editor = makeEditor('<p>hello</p>');
    editor.commands.focus('end');
    expect(editor.commands.splitBlock()).toBe(true);
    expect(editor.getJSON().content).toHaveLength(2);
    expect(editor.getJSON().content?.[1]).toMatchObject({ type: 'paragraph' });
    expect(serializeEditor(editor.getJSON()).text).toBe('hello\n');
    editor.destroy();
  });

  it('R06-05：composer 明确关闭无来源标记的 StarterKit trailingNode', () => {
    const editor = makeEditor('<ul><li><p>项目</p></li></ul>');
    expect(editor.extensionManager.extensions.some(extension => extension.name === 'trailingNode')).toBe(false);
    expect(editor.getJSON().content?.map(node => node.type)).toEqual(['bulletList']);
    editor.destroy();
  });

  it('R06-06：列表后由用户创建的空段仍按用户内容保留', () => {
    const editor = makeEditor({
      type: 'doc',
      content: [
        {
          type: 'bulletList',
          content: [{
            type: 'listItem',
            content: [{ type: 'paragraph', content: [{ type: 'text', text: '项目' }] }],
          }],
        },
        { type: 'paragraph' },
      ],
    });
    expect(serializeEditor(editor.getJSON()).text).toBe('- 项目\n');
    editor.commands.focus('end');
    editor.commands.insertContent('用户续写');
    expect(serializeEditor(editor.getJSON()).text).toBe('- 项目\n用户续写');
    editor.destroy();
  });

  it('R06-07：无 synthetic 标记的旧草稿 hydration 与再次保存不猜删尾行', () => {
    const oldDraft: JSONContent = {
      type: 'doc',
      content: [
        { type: 'paragraph', content: [{ type: 'text', text: '旧草稿' }] },
        { type: 'paragraph' },
      ],
    };
    const editor = makeEditor(oldDraft);
    const hydrated = editor.getJSON();
    expect(serializeEditor(hydrated).text).toBe('旧草稿\n');
    editor.commands.setContent(hydrated);
    expect(serializeEditor(editor.getJSON()).text).toBe('旧草稿\n');
    editor.destroy();
  });

  it('R06-09：纯空白正文由 trim 副本判空，附件身份仍独立提取', () => {
    const result = serializeEditor({
      type: 'doc',
      content: [
        { type: 'paragraph', content: [{ type: 'text', text: '\t　' }] },
        {
          type: 'paragraph',
          content: [{
            type: 'fileBadge',
            attrs: { path: '/synthetic/a.txt', name: 'a.txt', isDirectory: false },
          }],
        },
      ],
    });
    expect(result.text).toBe('\t　');
    expect(result.text.trim()).toBe('');
    expect(result.fileRefs).toEqual([{ path: '/synthetic/a.txt', name: 'a.txt', isDirectory: false }]);
  });
});
