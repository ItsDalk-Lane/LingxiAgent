import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { streamBufferManager } from '../../hooks/use-stream-buffer';
import { useStore } from '../../stores';
import type { ChatListItem } from '../../stores/chat-types';
import { observeChatPerformance } from '../../utils/chat-performance';
import { readLiveAssistantMessage } from '../../stores/live-turn-store';

const PATH = '/benchmark/live-turn-isolation.jsonl';

function currentItems(): ChatListItem[] {
  return useStore.getState().chatSessions[PATH]?.items ?? [];
}

function assistantSource(): string {
  const item = currentItems().find((entry) => entry.type === 'message' && entry.data.role === 'assistant');
  if (!item || item.type !== 'message') return '';
  const text = item.data.blocks?.find((block) => block.type === 'text');
  return text?.type === 'text' ? text.source || '' : '';
}

describe('live turn structural isolation', () => {
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
      data: { id: 'user-1', role: 'user', text: '请生成内容' },
    }], false);
  });

  afterEach(() => {
    streamBufferManager.clearAll();
    vi.useRealTimers();
  });

  it('一千个普通增量只更新当前回合，结构列表到结束前保持同一引用', () => {
    streamBufferManager.handle({ type: 'text_delta', sessionPath: PATH, delta: '首' });
    const itemsAfterSeat = currentItems();
    const assistantSeat = itemsAfterSeat[1];
    expect(assistantSeat?.type).toBe('message');

    const events: string[] = [];
    const stop = observeChatPerformance((event) => events.push(event.name));
    for (let index = 0; index < 1_000; index += 1) {
      streamBufferManager.handle({ type: 'text_delta', sessionPath: PATH, delta: '字' });
    }
    vi.advanceTimersByTime(34);

    expect(currentItems()).toBe(itemsAfterSeat);
    expect(assistantSeat?.type === 'message' ? assistantSeat.data.blocks : null).toEqual([]);
    expect(events.filter((name) => name === 'structural_message_update')).toHaveLength(0);

    let structuralCommits = 0;
    const unsubscribe = useStore.subscribe((state, previous) => {
      if (state.chatSessions !== previous.chatSessions) structuralCommits += 1;
    });
    streamBufferManager.handle({
      type: 'assistant_run_end',
      sessionPath: PATH,
      turnInputEntryId: 'entry-user-1',
      userEntryId: 'entry-user-1',
      assistantEntryId: 'entry-assistant-1',
    });
    unsubscribe();
    stop();

    expect(currentItems()).not.toBe(itemsAfterSeat);
    expect(structuralCommits).toBe(1);
    expect(assistantSource()).toHaveLength(1_001);
    expect(events.filter((name) => name === 'structural_message_update')).toHaveLength(1);
  });

  it('一千个统一分段增量只合并发布当前分段，不逐次改结构', () => {
    streamBufferManager.handle({
      type: 'assistant_segment_start',
      sessionPath: PATH,
      segmentId: 'assistant:1:text:0',
      kind: 'text',
      semanticPhase: 'unresolved',
    });
    const itemsAfterSeat = currentItems();
    const assistantSeat = itemsAfterSeat[1];
    if (assistantSeat?.type !== 'message') throw new Error('expected assistant seat');

    const events: string[] = [];
    const stop = observeChatPerformance((event) => events.push(event.name));
    for (let index = 0; index < 1_000; index += 1) {
      streamBufferManager.handle({
        type: 'assistant_segment_delta',
        sessionPath: PATH,
        segmentId: 'assistant:1:text:0',
        delta: '字',
        semanticPhase: 'unresolved',
      });
    }
    vi.advanceTimersByTime(34);
    stop();

    expect(currentItems()).toBe(itemsAfterSeat);
    expect(events.filter((name) => name === 'structural_message_update')).toHaveLength(0);
    expect(events.filter((name) => name === 'stream_flush')).toHaveLength(1);
    expect(readLiveAssistantMessage(PATH, assistantSeat.data.id)?.segmentsById[
      'assistant:1:text:0'
    ]?.source).toHaveLength(1_000);
  });
});
