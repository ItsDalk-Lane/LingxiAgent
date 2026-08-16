import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { streamBufferManager } from '../../hooks/use-stream-buffer';
import { useStore } from '../../stores';
import { readLiveAssistantMessage } from '../../stores/live-turn-store';
import {
  observeChatPerformance,
  type ChatPerformanceEvent,
} from '../../utils/chat-performance';

const PATH = '/benchmark/semantic-stream-publication.jsonl';

describe('semantic stream publication', () => {
  let nextFrameId = 1;
  let frameCallbacks: Map<number, FrameRequestCallback>;

  function runNextFrame(): void {
    const next = frameCallbacks.entries().next().value as [number, FrameRequestCallback] | undefined;
    if (!next) throw new Error('expected a pending animation frame');
    frameCallbacks.delete(next[0]);
    next[1](performance.now());
  }

  function assistantMessageId(): string {
    const assistant = useStore.getState().chatSessions[PATH]?.items.find((item) => (
      item.type === 'message' && item.data.role === 'assistant'
    ));
    if (!assistant || assistant.type !== 'message') throw new Error('expected assistant seat');
    return assistant.data.id;
  }

  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-08-16T00:00:00.000Z'));
    nextFrameId = 1;
    frameCallbacks = new Map();
    vi.stubGlobal('requestAnimationFrame', vi.fn((callback: FrameRequestCallback) => {
      const id = nextFrameId;
      nextFrameId += 1;
      frameCallbacks.set(id, callback);
      return id;
    }));
    vi.stubGlobal('cancelAnimationFrame', vi.fn((id: number) => {
      frameCallbacks.delete(id);
    }));
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
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it('同一画面内的一百个正文增量只安排并发布一次', () => {
    const events: ChatPerformanceEvent[] = [];
    const stop = observeChatPerformance((event) => events.push(event));

    for (let index = 0; index < 100; index += 1) {
      streamBufferManager.handle({ type: 'text_delta', sessionPath: PATH, delta: '字' });
    }

    const messageId = assistantMessageId();
    expect(requestAnimationFrame).toHaveBeenCalledTimes(1);
    expect(readLiveAssistantMessage(PATH, messageId)).toBeNull();

    runNextFrame();
    stop();

    const live = readLiveAssistantMessage(PATH, messageId);
    const text = live?.blocks.find((block) => block.type === 'text');
    expect(text?.type === 'text' ? text.source : null).toHaveLength(100);
    expect(events.filter((event) => event.name === 'stream_flush')).toHaveLength(1);
  });

  it('语义边界取消待发布画面，并把边界前内容合并为一次立即发布', () => {
    const events: ChatPerformanceEvent[] = [];
    const stop = observeChatPerformance((event) => events.push(event));

    streamBufferManager.handle({ type: 'text_delta', sessionPath: PATH, delta: '边界前正文' });
    streamBufferManager.handle({ type: 'thinking_start', sessionPath: PATH });
    stop();

    const live = readLiveAssistantMessage(PATH, assistantMessageId());
    expect(cancelAnimationFrame).toHaveBeenCalledTimes(1);
    expect(frameCallbacks).toHaveLength(0);
    expect(live?.blocks.map((block) => block.type)).toEqual(['thinking', 'text']);
    expect(events.filter((event) => event.name === 'stream_flush')).toHaveLength(1);
  });

  it('工具开始和结束各立即发布一次，开始时不重复发布边界前正文', () => {
    const events: ChatPerformanceEvent[] = [];
    const stop = observeChatPerformance((event) => events.push(event));

    streamBufferManager.handle({ type: 'text_delta', sessionPath: PATH, delta: '先检查。' });
    streamBufferManager.handle({
      type: 'tool_start',
      sessionPath: PATH,
      id: 'call-1',
      name: 'read',
      args: { path: '/tmp/example' },
    });

    let live = readLiveAssistantMessage(PATH, assistantMessageId());
    expect(live?.blocks.map((block) => block.type)).toEqual(['tool_group', 'text']);
    expect(events.filter((event) => event.name === 'stream_flush')).toHaveLength(1);

    streamBufferManager.handle({
      type: 'tool_end',
      sessionPath: PATH,
      id: 'call-1',
      name: 'read',
      success: true,
    });
    stop();

    live = readLiveAssistantMessage(PATH, assistantMessageId());
    const toolGroup = live?.blocks.find((block) => block.type === 'tool_group');
    expect(toolGroup?.type === 'tool_group' ? toolGroup.tools[0] : null).toMatchObject({
      id: 'call-1',
      done: true,
      success: true,
    });
    expect(events.filter((event) => event.name === 'stream_flush')).toHaveLength(2);
  });

  it('最终回答开始立即可见，回合结束会取消待发布画面并完成结构提交', () => {
    const events: ChatPerformanceEvent[] = [];
    const stop = observeChatPerformance((event) => events.push(event));

    streamBufferManager.handle({
      type: 'assistant_segment_start',
      sessionPath: PATH,
      segmentId: 'assistant:1:text:0',
      kind: 'text',
      semanticPhase: 'final_answer',
    });
    expect(events.filter((event) => event.name === 'stream_flush')).toHaveLength(1);

    streamBufferManager.handle({
      type: 'assistant_segment_delta',
      sessionPath: PATH,
      segmentId: 'assistant:1:text:0',
      delta: '完成。',
      semanticPhase: 'final_answer',
    });
    vi.advanceTimersByTime(33);
    expect(requestAnimationFrame).toHaveBeenCalledTimes(1);

    streamBufferManager.handle({
      type: 'turn_end',
      sessionPath: PATH,
      assistantEntryId: 'entry-assistant-1',
    });
    stop();

    expect(cancelAnimationFrame).toHaveBeenCalledTimes(1);
    expect(frameCallbacks).toHaveLength(0);
    const assistant = useStore.getState().chatSessions[PATH]?.items.find((item) => (
      item.type === 'message' && item.data.role === 'assistant'
    ));
    const answer = assistant?.type === 'message'
      ? assistant.data.blocks?.find((block) => block.surfaceRole === 'answer')
      : null;
    expect(answer?.type === 'text' ? answer.source : null).toBe('完成。');
    expect(events.filter((event) => event.name === 'stream_flush')).toHaveLength(2);
  });

  it('高刷新率画面仍受三十次每秒的发布预算约束', () => {
    const events: ChatPerformanceEvent[] = [];
    const stop = observeChatPerformance((event) => events.push(event));

    streamBufferManager.handle({ type: 'text_delta', sessionPath: PATH, delta: '一' });
    runNextFrame();
    streamBufferManager.handle({ type: 'text_delta', sessionPath: PATH, delta: '二' });

    expect(requestAnimationFrame).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(32);
    expect(requestAnimationFrame).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(1);
    expect(requestAnimationFrame).toHaveBeenCalledTimes(2);
    runNextFrame();
    stop();

    expect(events.filter((event) => event.name === 'stream_flush')).toHaveLength(2);
  });

  it('用量账本事件不创建消息、不安排画面、也不触发发布', () => {
    const events: ChatPerformanceEvent[] = [];
    const stop = observeChatPerformance((event) => events.push(event));
    const itemsBefore = useStore.getState().chatSessions[PATH]?.items;

    streamBufferManager.handle({
      type: 'token_usage',
      sessionPath: PATH,
      usage: { input: 100, output: 20 },
    });
    streamBufferManager.handle({ type: 'compaction_start', sessionPath: PATH });
    streamBufferManager.handle({ type: 'compaction_end', sessionPath: PATH });
    stop();

    expect(useStore.getState().chatSessions[PATH]?.items).toBe(itemsBefore);
    expect(requestAnimationFrame).not.toHaveBeenCalled();
    expect(events).toEqual([]);
  });
});
