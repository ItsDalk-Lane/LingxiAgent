import { describe, expect, it } from 'vitest';
import {
  measureChatPerformance,
  observeChatPerformance,
  recordChatPerformance,
  type ChatPerformanceEvent,
} from '../../utils/chat-performance';

describe('chat performance instrumentation', () => {
  it('未启用监听时不改变工作函数的返回值', () => {
    expect(measureChatPerformance('markdown_parse', { sourceLength: 3 }, () => 'ok')).toBe('ok');
    expect(() => recordChatPerformance('stream_flush')).not.toThrow();
  });

  it('显式启用后记录事件，并在释放后停止记录', () => {
    const events: ChatPerformanceEvent[] = [];
    const stop = observeChatPerformance((event) => events.push(event));

    recordChatPerformance('stream_flush', { sessionPath: '/session/a.jsonl' });
    const result = measureChatPerformance(
      'markdown_parse',
      { sourceLength: 12 },
      () => 42,
    );
    stop();
    recordChatPerformance('stream_flush', { sessionPath: '/session/b.jsonl' });

    expect(result).toBe(42);
    expect(events.map((event) => event.name)).toEqual(['stream_flush', 'markdown_parse']);
    expect(events[0]).toMatchObject({ sessionPath: '/session/a.jsonl' });
    expect(events[1]).toMatchObject({ sourceLength: 12 });
    expect(events[1].durationMs).toBeTypeOf('number');
  });
});
