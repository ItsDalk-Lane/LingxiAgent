import { describe, expect, it } from 'vitest';
import { buildItemsFromHistory } from '../../utils/history-builder';

/**
 * 历史恢复路径的 mood 生命周期回归（任务 §13/§14/§18）。
 *
 * 服务端 sessions.ts 历史接口把每个 JSONL assistant entry 投影成独立 message；
 * buildItemsFromHistory 对每条 assistant message 各跑一次 leading-only mood 解析。
 * 因此一个 user turn 内多段模型生成（工具循环）在重载后应表现为多条 assistant 消息，
 * 每条各自带 mood block，正文里绝不残留裸 <reflect>/<mood>/<pulse> 标签。
 * 且语义必须与实时流一致。
 */

function assistantMsg(id: string, content: string, toolCalls: any[] = []) {
  return { id, role: 'assistant' as const, content, toolCalls: toolCalls.length ? toolCalls : undefined };
}

function getMoodText(item: any): string | undefined {
  if (item?.type !== 'message') return undefined;
  return item.data.blocks?.find((b: any) => b.type === 'mood')?.text;
}

function getTextSource(item: any): string | undefined {
  if (item?.type !== 'message') return undefined;
  return item.data.blocks?.find((b: any) => b.type === 'text')?.source;
}

describe('buildItemsFromHistory mood segment lifecycle', () => {
  it('每条 assistant message 各自抽出 leading mood，互不吞标签', () => {
    const items = buildItemsFromHistory({
      messages: [
        { id: 'u1', role: 'user', content: '帮我查' },
        assistantMsg('a1', '<reflect>AAA</reflect>我先查一下', [{ id: 't1', name: 'read', args: {} }]),
        assistantMsg('a2', '<reflect>BBB</reflect>最终答案'),
      ],
    });
    const assistants = items.filter((i) => i.type === 'message' && i.data.role === 'assistant');
    expect(assistants).toHaveLength(2);
    expect(getMoodText(assistants[0])).toBe('AAA');
    expect(getMoodText(assistants[1])).toBe('BBB');
    expect(getTextSource(assistants[0])).toContain('我先查一下');
    expect(getTextSource(assistants[1])).toContain('最终答案');
    // 被抽离的内部标签不能残留在权威原文中。
    expect(getTextSource(assistants[0])).not.toMatch(/<\/?reflect>/);
    expect(getTextSource(assistants[1])).not.toMatch(/<\/?reflect>/);
  });

  it('mood / pulse / reflect 三种标签在历史里都正确抽离', () => {
    for (const tag of ['mood', 'pulse', 'reflect']) {
      const items = buildItemsFromHistory({
        messages: [
          { id: 'u1', role: 'user', content: 'q' },
          assistantMsg('a1', `<${tag}>内部</${tag}>正文`),
        ],
      });
      const asst = items.find((i) => i.type === 'message' && i.data.role === 'assistant');
      expect(getMoodText(asst)).toBe('内部');
      expect(getTextSource(asst)).toContain('正文');
    }
  });

  it('同一条 assistant message 内正文后的标签保持可见（leading-only）', () => {
    // 历史单条消息里，正文开始后的 <reflect> 必须保留为可见文本（模型在讲解标签），
    // 不能被全局贪婪正则抽走。
    const items = buildItemsFromHistory({
      messages: [
        { id: 'u1', role: 'user', content: 'q' },
        assistantMsg('a1', '<reflect>AAA</reflect>正文里讲标签'),
      ],
    });
    const asst = items.find((i) => i.type === 'message' && i.data.role === 'assistant');
    expect(getMoodText(asst)).toBe('AAA');
    expect(getTextSource(asst)).toContain('正文里讲标签');
  });

  it('inline-code 与 fenced code 里的标签保持普通正文（不被抽成 mood）', () => {
    const items = buildItemsFromHistory({
      messages: [
        { id: 'u1', role: 'user', content: 'q' },
        assistantMsg('a1', '`<reflect>` 示例\n```xml\n<reflect>literal</reflect>\n```'),
      ],
    });
    const asst = items.find((i) => i.type === 'message' && i.data.role === 'assistant');
    // 没有 leading mood 块：整段都保留在权威原文中。
    expect(getMoodText(asst)).toBeUndefined();
    expect(getTextSource(asst)).toContain('reflect');
  });
});
