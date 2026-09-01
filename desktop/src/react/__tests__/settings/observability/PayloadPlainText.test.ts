/**
 * payloadToReadableText 测试 — TXT 阅读式纯文本投影规则。
 */
import { describe, expect, it } from 'vitest';
import { payloadToReadableText } from '../../../settings/tabs/observability/trace-detail/payload-plain-text';

describe('payloadToReadableText', () => {
  it('字符串直出、保留换行；短值行内、长值成块', () => {
    const text = payloadToReadableText({
      systemPrompt: '第一行\n第二行',
      model: 'gpt-test',
      messages: [
        { role: 'user', content: '你好' },
      ],
    });
    expect(text).toContain('systemPrompt:');
    expect(text).toContain('  第一行\n  第二行');
    expect(text).toContain('model: gpt-test');
    expect(text).toContain('messages:');
    expect(text).toContain('[1]');
    expect(text).toContain('role: user');
    // 不出现 JSON 语法。
    expect(text).not.toContain('{');
    expect(text).not.toContain('"');
  });

  it('数组元素块间空行分隔；空对象/空数组给中文占位', () => {
    const text = payloadToReadableText({
      items: [{ a: 1 }, { b: 2 }],
      emptyList: [],
      emptyObject: {},
    });
    expect(text).toContain('[1]');
    expect(text).toContain('[2]');
    expect(text).toContain('a: 1');
    expect(text).toContain('emptyList: （空）');
    expect(text).toContain('emptyObject: （空）');
  });

  it('null / 顶层字符串 / 顶层数组照实处理', () => {
    expect(payloadToReadableText(null)).toBe('（无内容）');
    expect(payloadToReadableText('纯文本')).toBe('纯文本');
    expect(payloadToReadableText(42)).toBe('42');
    expect(payloadToReadableText([{ x: 'y' }])).toContain('[1]');
  });
});
