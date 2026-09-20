// @vitest-environment jsdom

/**
 * 插话（steer）触发的 Run 切分：前端实时渲染顺序回归。
 *
 * 场景（2026-09-19 真实事故）：Run 进行中用户插入新消息，旧实现里实时回合不切分，
 * 后续输出继续流进位于用户消息之前的旧气泡（顺序颠倒），且最终收口把两个逻辑回合
 * 并成一个。服务端现在在插话落盘时发 assistant_run_end(completed) + assistant_run_start，
 * 本测试锁定前端对该事件对的处理：
 *   1. 旧回合气泡原地收口，无答案时显示 missing_final_answer（与历史重投影一致）；
 *   2. 新回合气泡创建在插话用户消息之后（顺序正确）；
 *   3. 新回合的答案正常渲染。
 */

import { describe, it, expect, beforeEach } from 'vitest';
import { streamBufferManager } from '../../hooks/use-stream-buffer';
import { useStore } from '../../stores';
import type { ChatListItem } from '../../stores/chat-types';

const PATH = '/test/steer-split.jsonl';

function userItem(id: string, text: string): ChatListItem {
  return { type: 'message', data: { id, role: 'user', text } };
}

function getItems(): ChatListItem[] {
  return useStore.getState().chatSessions[PATH]?.items ?? [];
}

function messageRoles(): string[] {
  return getItems().map((item) => (item.type === 'message' ? item.data.role : item.type));
}

function assistantBlocksAt(index: number) {
  const item = getItems()[index];
  if (item?.type !== 'message' || item.data.role !== 'assistant') return [];
  return item.data.blocks || [];
}

describe('插话触发的 Run 切分（前端顺序）', () => {
  beforeEach(() => {
    streamBufferManager.clearAll();
    useStore.setState({
      currentSessionId: null,
      currentSessionPath: null,
      sessions: [],
      sessionLocatorsById: {},
    } as never);
    useStore.getState().clearSession(PATH);
    useStore.getState().initSession(PATH, [userItem('u1', '帮我改一下浏览器行为')], false);
  });

  it('run_end+run_start 事件对把新回合气泡放到插话消息之后', () => {
    // Run 1 开始并流出过程内容
    streamBufferManager.handle({ type: 'assistant_run_start', sessionPath: PATH, runId: 'run-1' });
    streamBufferManager.handle({ type: 'tool_start', sessionPath: PATH, id: 'call-1', name: 'grep', args: {} });
    streamBufferManager.handle({
      type: 'tool_end', sessionPath: PATH, id: 'call-1', name: 'grep',
      result: 'ok', isError: false,
    });

    // Run 1 的气泡已创建在 u1 之后
    expect(messageRoles()).toEqual(['user', 'assistant']);

    // 用户插话：乐观气泡追加到末尾
    useStore.getState().appendOptimisticUserMessage(PATH, {
      id: 'u2', role: 'user', text: '有一点需要纠正', clientMessageId: 'cm-2',
    } as never);
    expect(messageRoles()).toEqual(['user', 'assistant', 'user']);

    // 服务端切分事件对：旧 Run 收口（completed）+ 新 Run 开始
    streamBufferManager.handle({
      type: 'assistant_run_end', sessionPath: PATH, runId: 'run-1', status: 'completed',
      turnInputEntryId: 'entry-u1', assistantEntryId: 'entry-a1',
    });
    streamBufferManager.handle({ type: 'assistant_run_start', sessionPath: PATH, runId: 'run-2' });

    // 新 Run 的内容必须进入插话消息之后的新气泡
    streamBufferManager.handle({
      type: 'assistant_segment_start', sessionPath: PATH,
      segmentId: 'run2:text:0', kind: 'text', semanticPhase: 'final_answer',
    });
    streamBufferManager.handle({
      type: 'assistant_segment_delta', sessionPath: PATH,
      segmentId: 'run2:text:0', delta: '改完了，验证全部通过。',
    });
    streamBufferManager.handle({
      type: 'assistant_segment_end', sessionPath: PATH,
      segmentId: 'run2:text:0', semanticPhase: 'final_answer',
    });
    streamBufferManager.handle({
      type: 'assistant_run_end', sessionPath: PATH, runId: 'run-2', status: 'completed',
      turnInputEntryId: 'entry-u2', assistantEntryId: 'entry-a2',
    });

    // 顺序：u1 → 回合1气泡 → u2（插话）→ 回合2气泡
    expect(messageRoles()).toEqual(['user', 'assistant', 'user', 'assistant']);

    // 回合 1：只有过程没有答案 → missing_final_answer（与历史重投影逐字一致）
    const turn1Blocks = assistantBlocksAt(1);
    expect(turn1Blocks.some((b) => b.type === 'turn_status' && b.status === 'missing_final_answer')).toBe(true);

    // 回合 2：答案正常渲染，不丢尾部
    const turn2Blocks = assistantBlocksAt(3);
    const answer = turn2Blocks.find((b) => b.type === 'text' && b.surfaceRole === 'answer');
    expect(answer && 'source' in answer ? answer.source : '').toContain('改完了，验证全部通过。');
    expect(turn2Blocks.some((b) => b.type === 'turn_status')).toBe(false);

    // 归属绑定：两个回合各自认领自己的输入
    const items = getItems();
    const turn1 = items[1];
    const turn2 = items[3];
    expect(turn1.type === 'message' && turn1.data.turnInputEntryId).toBe('entry-u1');
    expect(turn2.type === 'message' && turn2.data.turnInputEntryId).toBe('entry-u2');
  });
});
