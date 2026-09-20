/**
 * settleStreamingProjection（换模型清账的第四本账）单测：
 * 投影残留 streaming 的助手消息按历史重载同一条投影路径结算为 completed。
 */
import { describe, it, expect, beforeEach } from 'vitest';
import { createChatSlice, type ChatSlice } from '../../stores/chat-slice';
import type { AssistantTurnProjection, ChatListItem, ChatMessage, ContentBlock } from '../../stores/chat-types';
import { registerStreamBufferInvalidator, registerStreamResumeMetaInvalidator } from '../../stores/stream-invalidator';

function makeSlice(initial: Record<string, unknown> = {}): ChatSlice {
  let state: ChatSlice & Record<string, unknown>;
  const set = (partial: Partial<ChatSlice> | ((s: ChatSlice) => Partial<ChatSlice>)) => {
    const patch = typeof partial === 'function' ? partial(state) : partial;
    state = { ...state, ...patch };
  };
  const get = () => state;
  state = { ...createChatSlice(set as never, get), ...initial };
  return new Proxy({} as ChatSlice, {
    get: (_, key: string) => (state as unknown as Record<string, unknown>)[key],
  });
}

registerStreamBufferInvalidator(() => {});
registerStreamResumeMetaInvalidator(() => {});

function streamingProjection(messageId: string, overrides: Partial<AssistantTurnProjection> = {}): AssistantTurnProjection {
  return {
    id: `${messageId}:turn`,
    inputMessageId: null,
    assistantMessageIds: [messageId],
    processBlockIds: [],
    answerBlockIds: [],
    resultBlockIds: [],
    controlBlockIds: [],
    status: 'streaming',
    ...overrides,
  };
}

function assistantItem(message: ChatMessage): ChatListItem {
  return { type: 'message', data: message };
}

describe('settleStreamingProjection', () => {
  let slice: ChatSlice;
  const sessionPath = '/sessions/switched.jsonl';

  beforeEach(() => {
    slice = makeSlice();
  });

  it('会话不存在时返回 false', () => {
    expect(slice.settleStreamingProjection(sessionPath)).toBe(false);
  });

  it('没有 streaming 残留时返回 false 且不写入', () => {
    slice.initSession(sessionPath, [
      assistantItem({
        id: 'a1',
        role: 'assistant',
        blocks: [{ type: 'text', source: '已结束的回复' }],
        turnProjection: streamingProjection('a1', { status: 'completed' }),
      }),
    ], false);

    expect(slice.settleStreamingProjection(sessionPath)).toBe(false);
    const item = slice.chatSessions[sessionPath].items[0];
    expect(item.type === 'message' && item.data.turnProjection?.status).toBe('completed');
  });

  it('把 streaming 残留结算为 completed：思考进过程区、答案进答案区、块统一封口', () => {
    slice.initSession(sessionPath, [
      assistantItem({
        id: 'res-1',
        role: 'assistant',
        timestamp: 111,
        blocks: [
          { type: 'thinking', content: '先查一下资料', sealed: true },
          { type: 'text', source: '这是最终答案。' },
        ],
        turnProjection: streamingProjection('res-1', { provisionalBlockIds: [] }),
      }),
    ], false);

    expect(slice.settleStreamingProjection(sessionPath)).toBe(true);

    const item = slice.chatSessions[sessionPath].items[0];
    if (item.type !== 'message') throw new Error('expected message item');
    const blocks = item.data.blocks ?? [];
    const { turnProjection } = item.data;
    expect(turnProjection?.status).toBe('completed');
    expect(turnProjection?.outcome).toBe('completed_with_answer');
    expect(turnProjection?.completedAt).toEqual(expect.any(Number));

    const thinking = blocks.find((block) => block.type === 'thinking');
    const answer = blocks.find((block) => block.type === 'text');
    expect(thinking?.surfaceRole).toBe('process');
    expect(thinking?.lifecycle).toBe('sealed');
    expect(answer?.surfaceRole).toBe('answer');
    expect(turnProjection?.processBlockIds).toContain(thinking?.id);
    expect(turnProjection?.answerBlockIds).toContain(answer?.id);
  });

  it('只有过程块、无答案的残留按权威裁决补 missing_final_answer 终态', () => {
    slice.initSession(sessionPath, [
      assistantItem({
        id: 'res-2',
        role: 'assistant',
        blocks: [{ type: 'thinking', content: '想了一下', sealed: true }],
        turnProjection: streamingProjection('res-2'),
      }),
    ], false);

    expect(slice.settleStreamingProjection(sessionPath)).toBe(true);

    const item = slice.chatSessions[sessionPath].items[0];
    if (item.type !== 'message') throw new Error('expected message item');
    expect(item.data.turnProjection?.status).toBe('completed');
    expect(item.data.turnProjection?.outcome).toBe('completed_without_user_output');
    expect(item.data.blocks?.some((block) => (
      block.type === 'turn_status' && block.status === 'missing_final_answer'
    ))).toBe(true);
  });

  it('没有投影的助手消息保持不动（渲染层 fallback 归类可正常折叠）', () => {
    const untouched: ChatMessage = {
      id: 'plain-1',
      role: 'assistant',
      blocks: [{ type: 'thinking', content: '旧数据', sealed: true } satisfies ContentBlock],
    };
    slice.initSession(sessionPath, [
      assistantItem({ ...untouched }),
      assistantItem({
        id: 'res-3',
        role: 'assistant',
        blocks: [{ type: 'text', source: '答案' }],
        turnProjection: streamingProjection('res-3'),
      }),
    ], false);

    expect(slice.settleStreamingProjection(sessionPath)).toBe(true);

    const items = slice.chatSessions[sessionPath].items;
    const first = items[0];
    if (first.type !== 'message') throw new Error('expected message item');
    expect(first.data.turnProjection).toBeUndefined();
    expect(first.data.id).toBe('plain-1');
    const second = items[1];
    if (second.type !== 'message') throw new Error('expected message item');
    expect(second.data.turnProjection?.status).toBe('completed');
  });
});
