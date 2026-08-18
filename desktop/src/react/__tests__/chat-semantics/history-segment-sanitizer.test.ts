import { describe, expect, it } from 'vitest';
import {
  sanitizePersistedSegments,
  sanitizePersistedSegmentSource,
} from '../../utils/history-segment-sanitizer';
import type { LiveAssistantSegment } from '../../stores/live-turn-store';

function textSegment(id: string, source: string): LiveAssistantSegment {
  return { id, kind: 'text', semanticPhase: 'final_answer', source, lifecycle: 'sealed' };
}

describe('history-segment-sanitizer（迁移边界一次性净化）', () => {
  it('结构化 mood 存在时剥离 leading <mood> 标签，正文保留', () => {
    const source = '<mood>Vibe: 专注</mood>\n\n任务已完成。';
    expect(sanitizePersistedSegmentSource(source, { hasStructuredMood: true, hasStructuredThinking: false }))
      .toBe('任务已完成。');
  });

  it('没有结构化 mood 时保留原文（避免内容凭空消失）', () => {
    const source = '<mood>Vibe: 专注</mood>\n\n任务已完成。';
    expect(sanitizePersistedSegmentSource(source, { hasStructuredMood: false, hasStructuredThinking: false }))
      .toBe(source);
  });

  it('结构化 thinking 存在时剥离 leading <think> 块', () => {
    const source = '<think>先想想</think>结论如下';
    expect(sanitizePersistedSegmentSource(source, { hasStructuredMood: false, hasStructuredThinking: true }))
      .toBe('结论如下');
  });

  it('正文内部的标签保留（模型在讲解标签时是内容）', () => {
    const source = '正文解释 <mood>literal</mood> 的用法';
    expect(sanitizePersistedSegmentSource(source, { hasStructuredMood: true, hasStructuredThinking: false }))
      .toBe(source);
  });

  it('多段 mood 交错时循环剥离，reasoning segment 不动', () => {
    const segments = [
      textSegment('assistant:1:text:0', '<mood>A</mood><think>x</think>过程'),
      { id: 'assistant:1:reasoning:default', kind: 'reasoning', semanticPhase: 'reasoning', source: '<mood>不应被动</mood>', lifecycle: 'sealed' } as LiveAssistantSegment,
    ];
    const result = sanitizePersistedSegments(segments, { hasStructuredMood: true, hasStructuredThinking: true });
    expect(result[0]?.source).toBe('过程');
    expect(result[1]?.source).toBe('<mood>不应被动</mood>');
  });
});
