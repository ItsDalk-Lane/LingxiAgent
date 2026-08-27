/**
 * 流装配器防重放幂等回归测试。
 *
 * 背景：resume 增量重放在流元数据失配时会把已应用事件原样重发到未清空的
 * buffer 上；此前 tool_start 无按 id 去重、canonical delta 无条件追加，
 * 造成「一次技能调用出现多张已运行技能卡 + 正文尾部重叠重复」。
 * 防御层：Buffer.appliedToolStartSeqs（按 seq）+ toolCallId 存在性检查 +
 * Buffer.canonicalAppliedSeqBySegment（按段记录最大已应用 seq）。
 */

// @vitest-environment jsdom

import { beforeEach, describe, expect, it } from 'vitest';
import { streamBufferManager } from '../../hooks/use-stream-buffer';
import { useStore } from '../../stores';
import { readLiveAssistantMessage } from '../../stores/live-turn-store';
import type { ChatListItem, ChatMessage, ContentBlock } from '../../stores/chat-types';

const PATH = '/test/stream-dedupe.jsonl';

function userItem(id: string, text: string): ChatListItem {
  return { type: 'message', data: { id, role: 'user', text } };
}

function getItems(): ChatListItem[] {
  return useStore.getState().chatSessions[PATH]?.items ?? [];
}

function getAssistantMessage(): ChatMessage | null {
  const item = getItems().find((entry) => entry.type === 'message' && entry.data.role === 'assistant');
  return item?.type === 'message' ? item.data : null;
}

function assistantBlocks(): ContentBlock[] {
  const message = getAssistantMessage();
  if (!message) return [];
  const live = readLiveAssistantMessage(PATH, message.id);
  return live ? [...live.blocks] : (message.blocks || []);
}

function allTools(): Array<{ id?: string; name: string; done?: boolean }> {
  return assistantBlocks()
    .flatMap((block) => (block.type === 'tool_group' ? block.tools : []));
}

function canonicalTurn(script: Array<Record<string, unknown>>): void {
  for (const event of script) streamBufferManager.handle({ sessionPath: PATH, ...event });
}

describe('streamBufferManager 重放幂等', () => {
  beforeEach(() => {
    streamBufferManager.clearAll();
    useStore.setState({
      currentSessionId: null,
      currentSessionPath: null,
      sessions: [],
      sessionLocatorsById: {},
      streamingSessions: [],
      selectedIdsBySession: {},
      terminalsBySession: {},
      agents: [],
      agentName: '小花',
      agentYuan: 'lingxi',
    } as never);
    useStore.getState().clearSession(PATH);
    useStore.getState().initSession(PATH, [userItem('u1', 'hi')], false);
  });

  it('同一 toolCallId 重复投递（含缺 seq 的重放）只生成一张卡', () => {
    const skillTool = { id: 'call-skill', name: 'read', args: { path: '/skills/x/SKILL.md' } };
    canonicalTurn([
      { type: 'assistant_run_start', runId: 'run-dedupe-1' },
      // 正常到达
      { type: 'tool_start', ...skillTool, seq: 4 },
      // resume 重放：完全相同的 seq
      { type: 'tool_start', ...skillTool, seq: 4 },
      // 元数据失配下的另一形态：没有 seq 但 id 相同
      { type: 'tool_start', ...skillTool },
      { type: 'tool_end', id: 'call-skill', name: 'read', success: true, status: 'succeeded' },
      { type: 'assistant_segment_start', segmentId: 'assistant:1:text:0', kind: 'text', semanticPhase: 'final_answer' },
      { type: 'assistant_segment_delta', segmentId: 'assistant:1:text:0', delta: '完成', semanticPhase: 'final_answer', seq: 8 },
      { type: 'assistant_segment_end', segmentId: 'assistant:1:text:0', semanticPhase: 'final_answer', seq: 9 },
      { type: 'assistant_run_end', assistantEntryId: 'entry-a-1', turnInputEntryId: 'u1' },
    ]);

    const skillTools = allTools().filter((tool) => tool.id === 'call-skill');
    expect(skillTools).toHaveLength(1);
    expect(skillTools[0]?.done).toBe(true);
  });

  it('不同工具的正常投递不受幂等防御影响', () => {
    canonicalTurn([
      { type: 'assistant_run_start', runId: 'run-dedupe-2' },
      { type: 'tool_start', id: 'call-a', name: 'grep', args: {}, seq: 20 },
      { type: 'tool_end', id: 'call-a', name: 'grep', success: true, status: 'succeeded', seq: 21 },
      { type: 'tool_start', id: 'call-b', name: 'ls', args: {}, seq: 22 },
      { type: 'tool_end', id: 'call-b', name: 'ls', success: true, status: 'succeeded', seq: 23 },
      { type: 'assistant_segment_start', segmentId: 'assistant:1:text:0', kind: 'text', semanticPhase: 'final_answer' },
      { type: 'assistant_segment_delta', segmentId: 'assistant:1:text:0', delta: 'ok', semanticPhase: 'final_answer', seq: 24 },
      { type: 'assistant_segment_end', segmentId: 'assistant:1:text:0', semanticPhase: 'final_answer', seq: 25 },
      { type: 'assistant_run_end', assistantEntryId: 'entry-a-2', turnInputEntryId: 'u1' },
    ]);

    expect(allTools()).toHaveLength(2);
  });

  it('canonical delta 重放不再叠加：正文不出现尾部重叠重复', () => {
    canonicalTurn([
      { type: 'assistant_run_start', runId: 'run-dedupe-3' },
      { type: 'assistant_segment_start', segmentId: 'seg-text', kind: 'text', semanticPhase: 'final_answer', seq: 30 },
      { type: 'assistant_segment_delta', segmentId: 'seg-text', delta: 'AB', semanticPhase: 'final_answer', seq: 31 },
      { type: 'assistant_segment_delta', segmentId: 'seg-text', delta: 'CD', semanticPhase: 'final_answer', seq: 32 },
      // resume 重放区间与已在渲染的内容部分重叠
      { type: 'assistant_segment_delta', segmentId: 'seg-text', delta: 'CD', semanticPhase: 'final_answer', seq: 32 },
      { type: 'assistant_segment_delta', segmentId: 'seg-text', delta: 'EF', semanticPhase: 'final_answer', seq: 33 },
      // 更旧的重放也必须被丢掉
      { type: 'assistant_segment_delta', segmentId: 'seg-text', delta: 'AB', semanticPhase: 'final_answer', seq: 31 },
      { type: 'assistant_segment_end', segmentId: 'seg-text', semanticPhase: 'final_answer', seq: 34 },
      { type: 'assistant_run_end', assistantEntryId: 'entry-a-3', turnInputEntryId: 'u1' },
    ]);

    const text = assistantBlocks()
      .filter((block): block is Extract<ContentBlock, { type: 'text' }> => block.type === 'text')
      .map((block) => block.source || '');
    expect(text.join('')).toBe('ABCDEF');
  });
});
