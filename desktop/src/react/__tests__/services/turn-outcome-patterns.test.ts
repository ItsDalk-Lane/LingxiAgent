/**
 * Turn Outcome 模式回归（计划阶段8）
 *
 * 以"服务端现在真实会发的事件序列"驱动 streamBufferManager，锁定五种链路模式的
 * 终态结构：
 *   A. 常规供应商：thinking + mood + 工具 + 最终答复，无 turn_status
 *   B. 多段生成（工具循环）：3 个 mood、2 个工具、1 个答案、1 个 Process Fold、0 个 missing
 *   C. status 高频抖动：内容生命周期与无抖动的运行逐字段相等
 *   D. phase-at-end 供应商：流式期 unresolved → provisional，终结后成为答案
 *   E. 真·过程轮：只有过程和工具，终结后恰好一个 missing_final_answer
 */

import { describe, it, expect, beforeEach } from 'vitest';
import { streamBufferManager } from '../../hooks/use-stream-buffer';
import { useStore } from '../../stores';
import type { ChatListItem, ChatMessage } from '../../stores/chat-types';
import { readLiveAssistantMessage } from '../../stores/live-turn-store';
import { buildTranscriptRenderItems } from '../../components/chat/process-fold';

const PATH = '/test/turn-outcome.jsonl';

function resetStore() {
  streamBufferManager.clearAll();
  useStore.setState({
    currentSessionId: null,
    currentSessionPath: null,
    sessions: [{
      path: PATH,
      agentId: 'owner',
      title: null,
      firstMessage: '',
      modified: '',
      messageCount: 0,
    }],
    agents: [{ id: 'owner', yuan: 'ming' }],
    currentAgentId: 'focus',
    agentYuan: 'lingxi',
    sessionLocatorsById: {},
  } as never);
  useStore.getState().clearSession(PATH);
  useStore.getState().initSession(PATH, [
    { type: 'message', data: { id: 'u1', role: 'user', text: 'hi' } },
  ], false);
}

function assistantMessages(): ChatMessage[] {
  const items = useStore.getState().chatSessions[PATH]?.items ?? [];
  return items
    .filter((item): item is Extract<ChatListItem, { type: 'message' }> => (
      item.type === 'message' && item.data.role === 'assistant'
    ))
    .map((item) => item.data);
}

function allBlocks(): NonNullable<ChatMessage['blocks']> {
  return assistantMessages().flatMap((message) => message.blocks ?? []);
}

function turnStart(turnId: string) {
  streamBufferManager.handle({ type: 'turn_start', sessionPath: PATH, turnId, streamId: `stream-${turnId}` });
}

function turnEnd(turnInputEntryId: string, assistantEntryId: string) {
  streamBufferManager.handle({
    type: 'turn_end',
    sessionPath: PATH,
    turnInputEntryId,
    userEntryId: turnInputEntryId,
    assistantEntryId,
    assistantEntryIds: [assistantEntryId],
  });
}

function moodCycle(text: string) {
  streamBufferManager.handle({ type: 'mood_start', sessionPath: PATH });
  streamBufferManager.handle({ type: 'mood_text', sessionPath: PATH, delta: text });
  streamBufferManager.handle({ type: 'mood_end', sessionPath: PATH });
}

function toolCycle(id: string, name = 'read') {
  streamBufferManager.handle({ type: 'tool_start', sessionPath: PATH, id, name, args: { path: '/tmp/a.md' } });
  streamBufferManager.handle({ type: 'tool_end', sessionPath: PATH, id, name, success: true, status: 'succeeded' });
}

function canonicalSegment(segmentId: string, kind: string, phaseAtStart: string, delta: string, phaseAtEnd: string) {
  streamBufferManager.handle({
    type: 'assistant_segment_start', sessionPath: PATH,
    segmentId, kind, semanticPhase: phaseAtStart,
  });
  streamBufferManager.handle({
    type: 'assistant_segment_delta', sessionPath: PATH,
    segmentId, delta, semanticPhase: phaseAtStart,
  });
  streamBufferManager.handle({
    type: 'assistant_segment_end', sessionPath: PATH,
    segmentId, semanticPhase: phaseAtEnd,
  });
}

describe('Turn Outcome 模式回归', () => {
  beforeEach(resetStore);

  it('模式 A：常规供应商一轮 —— thinking + mood + 工具 + 答案，无 turn_status', () => {
    turnStart('turn-a');
    canonicalSegment('assistant:1:reasoning:0', 'reasoning', 'reasoning', '先想一下', 'reasoning');
    moodCycle('有点小兴奋');
    toolCycle('call-1');
    canonicalSegment('assistant:2:text:0', 'text', 'final_answer', '这是最终答复', 'final_answer');
    turnEnd('entry-u1', 'entry-a1');

    const blocks = allBlocks();
    expect(blocks.map((block) => block.type)).toEqual(['thinking', 'mood', 'tool_group', 'text']);
    expect(blocks.filter((block) => block.type === 'turn_status')).toHaveLength(0);
    const turn = assistantMessages()[0];
    expect(turn.turnProjection).toMatchObject({ status: 'completed' });
    expect(turn.turnProjection?.answerBlockIds).toHaveLength(1);
  });

  it('模式 B：多段生成 —— 3 mood / 2 工具 / 1 答案 / 1 个 Process Fold / 0 个 missing', () => {
    // 第一段生成：mood A → 工具 → mood B（中段标签同样是协议）→ 过程文字
    turnStart('turn-b1');
    moodCycle('A');
    toolCycle('call-1');
    moodCycle('B');
    canonicalSegment('assistant:1:text:0', 'text', 'commentary', '先查一下', 'commentary');
    turnEnd('entry-u1', 'entry-a1');
    // 第二段生成：mood C → 工具 → 最终答复
    turnStart('turn-b2');
    moodCycle('C');
    toolCycle('call-2');
    canonicalSegment('assistant:2:text:0', 'text', 'final_answer', '最终答复', 'final_answer');
    turnEnd('entry-u1', 'entry-a2');

    const turns = assistantMessages();
    expect(turns).toHaveLength(2);
    // 3 个 mood 段：第一段消息聚合 A+B，第二段消息是 C
    const moodBlocks = allBlocks().filter((block) => block.type === 'mood');
    expect(moodBlocks.map((block) => block.type === 'mood' && block.text)).toEqual(['A\n\nB', 'C']);
    // 0 个 missing / turn_status
    expect(allBlocks().filter((block) => block.type === 'turn_status')).toHaveLength(0);
    // 恰好 1 个答案块
    const answerIds = turns.flatMap((turn) => turn.turnProjection?.answerBlockIds ?? []);
    expect(answerIds).toHaveLength(1);

    // 缝合展示：同一个用户输入下的两段生成折成恰好 1 个 Process Fold，含 2 个工具
    const items = useStore.getState().chatSessions[PATH]?.items ?? [];
    const rendered = buildTranscriptRenderItems(items, { isStreaming: false, liveTurnStatus: null });
    const folds = rendered.filter((item) => item.type === 'process_fold');
    expect(folds).toHaveLength(1);
    if (folds[0].type !== 'process_fold') throw new Error('expected fold');
    expect(folds[0].stats.toolCount).toBe(2);
    // 答案留在折叠外面
    const visibleTexts = rendered
      .filter((item) => item.type === 'source')
      .flatMap((item) => (item.type === 'source' && item.item.type === 'message' ? item.item.data.blocks ?? [] : []))
      .filter((block) => block.type === 'text')
      .map((block) => (block.type === 'text' ? block.source : ''));
    expect(visibleTexts).toContain('最终答复');
    expect(visibleTexts).not.toContain('先查一下');
  });

  it('模式 C：status 高频抖动与无抖动的运行产生逐字段相等的内容', () => {
    const runCanonicalTurn = (withStatusFlap: boolean) => {
      resetStore();
      turnStart('turn-c');
      if (withStatusFlap) {
        streamBufferManager.handle({ type: 'status', sessionPath: PATH, isStreaming: false });
        streamBufferManager.handle({ type: 'status', sessionPath: PATH, isStreaming: true });
      }
      canonicalSegment('assistant:1:text:0', 'text', 'commentary', '过程', 'commentary');
      if (withStatusFlap) {
        streamBufferManager.handle({ type: 'status', sessionPath: PATH, isStreaming: false });
        streamBufferManager.handle({ type: 'status', sessionPath: PATH, isStreaming: true });
        streamBufferManager.handle({ type: 'status', sessionPath: PATH, isStreaming: false });
      }
      canonicalSegment('assistant:2:text:0', 'text', 'final_answer', '答案', 'final_answer');
      turnEnd('entry-u1', 'entry-a1');
      if (withStatusFlap) {
        streamBufferManager.handle({ type: 'status', sessionPath: PATH, isStreaming: false });
      }
      return assistantMessages().map((message) => ({
        blocks: message.blocks,
        turnProjection: message.turnProjection,
      }));
    };

    expect(runCanonicalTurn(true)).toEqual(runCanonicalTurn(false));
  });

  it('模式 D：phase-at-end 供应商 —— 流式期 provisional，终结后成为答案', () => {
    turnStart('turn-d');
    streamBufferManager.handle({
      type: 'assistant_segment_start', sessionPath: PATH,
      segmentId: 'assistant:1:text:0', kind: 'text', semanticPhase: 'unresolved',
    });
    streamBufferManager.handle({
      type: 'assistant_segment_delta', sessionPath: PATH,
      segmentId: 'assistant:1:text:0', delta: '身份未判明的文字', semanticPhase: 'unresolved',
    });

    // 流式期：unresolved → provisional，不算过程也不算答案
    const liveSeat = assistantMessages()[0];
    const live = liveSeat ? readLiveAssistantMessage(PATH, liveSeat.id) : null;
    expect(live?.turnProjection?.status).toBe('streaming');
    expect(live?.turnProjection?.provisionalBlockIds).toHaveLength(1);
    expect(live?.turnProjection?.processBlockIds).toHaveLength(0);
    expect(live?.turnProjection?.answerBlockIds).toHaveLength(0);
    expect(live?.blocks.find((block) => block.type === 'text')).toMatchObject({
      semanticPhase: 'unresolved',
      surfaceRole: 'provisional',
    });

    // text_end 时刻供应商才给出身份：final_answer
    streamBufferManager.handle({
      type: 'assistant_segment_end', sessionPath: PATH,
      segmentId: 'assistant:1:text:0', semanticPhase: 'final_answer',
    });
    turnEnd('entry-u1', 'entry-a1');

    const turn = assistantMessages()[0];
    expect(turn.turnProjection?.answerBlockIds).toHaveLength(1);
    expect(turn.turnProjection?.provisionalBlockIds ?? []).toHaveLength(0);
    expect(allBlocks().filter((block) => block.type === 'turn_status')).toHaveLength(0);
    expect(allBlocks().find((block) => block.type === 'text')).toMatchObject({
      semanticPhase: 'final_answer',
      surfaceRole: 'answer',
      lifecycle: 'sealed',
    });
  });

  it('模式 E：真·过程轮 —— 循环停下来仍没给答复，整个用户轮恰好一个 missing_final_answer', () => {
    // 第一段带工具：循环继续，missing 是误报，必须豁免
    turnStart('turn-e1');
    canonicalSegment('assistant:1:text:0', 'text', 'commentary', '先查一下', 'commentary');
    toolCycle('call-1');
    turnEnd('entry-u1', 'entry-a1');
    // 终结段只有过程文字、没有工具：循环真的停了却没给答复 → 恰好一个 missing
    turnStart('turn-e2');
    canonicalSegment('assistant:2:text:0', 'text', 'commentary', '只做了内部检查', 'commentary');
    turnEnd('entry-u1', 'entry-a2');

    const turns = assistantMessages();
    expect(turns).toHaveLength(2);
    expect(turns[0].blocks?.filter((block) => block.type === 'turn_status')).toHaveLength(0);
    const statusBlocks = turns[1].blocks?.filter((block) => block.type === 'turn_status') ?? [];
    expect(statusBlocks).toHaveLength(1);
    expect(statusBlocks[0]).toMatchObject({
      id: 'entry-a2:missing-final-answer',
      status: 'missing_final_answer',
      surfaceRole: 'result',
    });
    // 过程文字不被晋升成答案
    expect(turns[1].turnProjection?.answerBlockIds).toEqual([]);
  });
});
