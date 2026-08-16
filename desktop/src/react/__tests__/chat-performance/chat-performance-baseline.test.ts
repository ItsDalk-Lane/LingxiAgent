import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { streamBufferManager } from '../../hooks/use-stream-buffer';
import { useStore } from '../../stores';
import type { ChatListItem } from '../../stores/chat-types';
import {
  observeChatPerformance,
  type ChatPerformanceEvent,
  type ChatPerformanceEventName,
} from '../../utils/chat-performance';
import { buildItemsFromHistory, type HistoryApiResponse } from '../../utils/history-builder';

const PATH = '/benchmark/chat-rendering.jsonl';

function count(events: ChatPerformanceEvent[], name: ChatPerformanceEventName): number {
  return events.filter((event) => event.name === name).length;
}

function currentItems(): ChatListItem[] {
  return useStore.getState().chatSessions[PATH]?.items ?? [];
}

function currentTextSource(): string {
  const assistant = currentItems().find((item) => item.type === 'message' && item.data.role === 'assistant');
  if (!assistant || assistant.type !== 'message') return '';
  const text = assistant.data.blocks?.find((block) => block.type === 'text');
  return text?.type === 'text' ? text.source || '' : '';
}

describe('chat rendering performance baseline', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-08-16T00:00:00.000Z'));
    streamBufferManager.clearAll();
    useStore.setState({
      currentSessionId: null,
      currentSessionPath: null,
      sessions: [],
      sessionLocatorsById: {},
    } as never);
    useStore.getState().clearSession(PATH);
    useStore.getState().initSession(PATH, [{
      type: 'message',
      data: { id: 'user-1', role: 'user', text: '请生成压力测试正文' },
    }], false);
  });

  afterEach(() => {
    streamBufferManager.clearAll();
    vi.useRealTimers();
  });

  for (const deltaCount of [10_000, 50_000, 100_000]) {
    it(`${deltaCount.toLocaleString('en-US')} 个 text_delta 在状态层不预解析 Markdown`, () => {
      const events: ChatPerformanceEvent[] = [];
      const stop = observeChatPerformance((event) => events.push(event));
      const initialItems = currentItems();

      for (let index = 0; index < deltaCount; index += 1) {
        streamBufferManager.handle({ type: 'text_delta', sessionPath: PATH, delta: '字' });
      }
      vi.advanceTimersByTime(34);
      streamBufferManager.finishTurn(PATH);
      stop();

      expect(currentTextSource()).toHaveLength(deltaCount);
      expect(currentItems()).not.toBe(initialItems);
      expect(count(events, 'stream_flush')).toBe(3);
      expect(count(events, 'markdown_parse')).toBe(0);
      expect(count(events, 'structural_message_update')).toBe(1);
      expect(events
        .filter((event) => event.name === 'markdown_parse')
        .map((event) => event.sourceLength)).toEqual([]);
    });
  }

  it('记录 100,000 个 thinking_delta 的现状成本', () => {
    const events: ChatPerformanceEvent[] = [];
    const stop = observeChatPerformance((event) => events.push(event));

    streamBufferManager.handle({ type: 'thinking_start', sessionPath: PATH });
    for (let index = 0; index < 100_000; index += 1) {
      streamBufferManager.handle({ type: 'thinking_delta', sessionPath: PATH, delta: '想' });
    }
    vi.advanceTimersByTime(34);
    streamBufferManager.handle({ type: 'thinking_end', sessionPath: PATH });
    stop();

    expect(count(events, 'stream_flush')).toBe(3);
    expect(count(events, 'structural_message_update')).toBe(0);
    expect(count(events, 'markdown_parse')).toBe(0);
  });

  for (const messageCount of [500, 2_000]) {
    it(`${messageCount.toLocaleString('en-US')} 条历史消息不预解析助手正文`, () => {
      const data: HistoryApiResponse = {
        messages: Array.from({ length: messageCount }, (_, index) => ({
          id: `history-${index}`,
          role: index % 2 === 0 ? 'user' : 'assistant',
          content: `第 ${index + 1} 条历史消息`,
        })),
      };
      const events: ChatPerformanceEvent[] = [];
      const stop = observeChatPerformance((event) => events.push(event));

      const items = buildItemsFromHistory(data);
      stop();

      expect(items).toHaveLength(messageCount);
      expect(events.filter((event) => event.name === 'history_projection')).toEqual([
        expect.objectContaining({ itemCount: messageCount }),
      ]);
      expect(count(events, 'markdown_parse')).toBe(messageCount / 2);
      const assistantBlocks = items
        .filter((item) => item.type === 'message' && item.data.role === 'assistant')
        .flatMap((item) => item.type === 'message' ? item.data.blocks || [] : [])
        .filter((block) => block.type === 'text');
      expect(assistantBlocks).toHaveLength(messageCount / 2);
      expect(assistantBlocks.every((block) => typeof block.source === 'string' && !('html' in block))).toBe(true);
    });
  }
});
