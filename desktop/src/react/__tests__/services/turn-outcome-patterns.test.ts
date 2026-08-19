/**
 * Turn Outcome 模式回归（计划阶段8，任务书 §四十二 重写）
 *
 * 以"服务端现在真实会发的事件序列"驱动 streamBufferManager：assistant_run_start /
 * model_turn_start / model_turn_end / assistant_run_end。锁定五种链路模式的终态结构：
 *   A. 常规供应商：thinking + mood + 工具 + 最终答复，无 turn_status
 *   B. 多段生成（工具循环，多个 Model Turn 一个 Assistant Run）：3 个 mood、2 个工具、
 *      1 个答案、1 个 Process Fold、0 个 missing
 *   C. status 高频抖动：内容生命周期与无抖动的运行逐字段相等
 *   D. phase-at-end 供应商：流式期 unresolved → provisional，终结后成为答案
 *   E. 真·过程轮：只有过程和工具，Run 真正 terminal 后恰好一个 missing_final_answer
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

// Assistant Run 生命周期（一个用户 Run 一个）。
function runStart(runId = 'run-1') {
  streamBufferManager.handle({ type: 'assistant_run_start', sessionPath: PATH, runId, streamId: 'stream-' + runId });
}
function runEnd(turnInputEntryId: string, assistantEntryId: string) {
  streamBufferManager.handle({
    type: 'assistant_run_end',
    sessionPath: PATH,
    runId: 'run-1',
    turnInputEntryId,
    userEntryId: turnInputEntryId,
    assistantEntryId,
    assistantEntryIds: [assistantEntryId],
  });
}

// Pi Model Turn 边界（一个 Assistant Run 内可有多个）。
function turnStart(turnId: string) {
  streamBufferManager.handle({ type: 'model_turn_start', sessionPath: PATH, turnId, streamId: 'stream-run-1' });
}
function turnEnd() {
  streamBufferManager.handle({ type: 'model_turn_end', sessionPath: PATH });
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
    runStart();
    turnStart('turn-a');
    canonicalSegment('assistant:1:reasoning:0', 'reasoning', 'reasoning', '先想一下', 'reasoning');
    moodCycle('有点小兴奋');
    toolCycle('call-1');
    canonicalSegment('assistant:2:text:0', 'text', 'final_answer', '这是最终答复', 'final_answer');
    turnEnd();
    runEnd('entry-u1', 'entry-a1');

    const blocks = allBlocks();
    expect(blocks.map((block) => block.type)).toEqual(['thinking', 'mood', 'tool_group', 'text']);
    expect(blocks.filter((block) => block.type === 'turn_status')).toHaveLength(0);
    const turn = assistantMessages()[0];
    expect(turn.turnProjection).toMatchObject({ status: 'completed' });
    expect(turn.turnProjection?.answerBlockIds).toHaveLength(1);
  });

  it('模式 B：多段生成 —— 3 mood / 2 工具 / 1 答案 / 1 个 Process Fold / 0 个 missing', () => {
    runStart();
    // 第一段生成：mood A → 工具 → mood B（中段标签同样是协议）→ 过程文字
    turnStart('turn-b1');
    moodCycle('A');
    toolCycle('call-1');
    moodCycle('B');
    canonicalSegment('assistant:1:text:0', 'text', 'commentary', '先查一下', 'commentary');
    turnEnd();
    // 第二段生成：mood C → 工具 → 最终答复
    turnStart('turn-b2');
    moodCycle('C');
    toolCycle('call-2');
    canonicalSegment('assistant:2:text:0', 'text', 'final_answer', '最终答复', 'final_answer');
    turnEnd();
    runEnd('entry-u1', 'entry-a2');

    // 整个 Assistant Run 只有一个 assistant 消息（跨 Model Turn 持续累计）。
    const turns = assistantMessages();
    expect(turns).toHaveLength(1);
    // 3 个 mood 段聚合到同一消息：A\n\nB\n\nC。
    const moodBlocks = allBlocks().filter((block) => block.type === 'mood');
    expect(moodBlocks.map((block) => block.type === 'mood' && block.text)).toEqual(['A\n\nB\n\nC']);
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
      runStart();
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
      turnEnd();
      runEnd('entry-u1', 'entry-a1');
      if (withStatusFlap) {
        streamBufferManager.handle({ type: 'status', sessionPath: PATH, isStreaming: false });
      }
      // 逐字段比较语义内容；block.id / turnProjection.id 因 stream message id 非
      // 确定性（每次 resetStore 生成新 id）而不同，属于测试夹具噪声，不是内容差异。
      return assistantMessages().map((message) => {
        const stripId = (block: any) => { const { id, ...rest } = block; return rest; };
        return {
          blocks: (message.blocks ?? []).map(stripId),
          turnProjection: message.turnProjection ? {
            status: message.turnProjection.status,
            inputMessageId: message.turnProjection.inputMessageId,
            assistantMessageIds: message.turnProjection.assistantMessageIds,
          } : undefined,
        };
      });
    };

    expect(runCanonicalTurn(true)).toEqual(runCanonicalTurn(false));
  });

  it('模式 D：phase-at-end 供应商 —— 流式期 provisional，终结后成为答案', () => {
    runStart();
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
    turnEnd();
    runEnd('entry-u1', 'entry-a1');

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
    runStart();
    // 第一段带工具：Run 仍 active，missing 是误报，绝不产生。
    turnStart('turn-e1');
    canonicalSegment('assistant:1:text:0', 'text', 'commentary', '先查一下', 'commentary');
    toolCycle('call-1');
    turnEnd();
    // 终结段只有过程文字、没有工具。
    turnStart('turn-e2');
    canonicalSegment('assistant:2:text:0', 'text', 'commentary', '只做了内部检查', 'commentary');
    turnEnd();
    // 只有 assistant_run_end（真正 terminal）后才产生 missing。
    runEnd('entry-u1', 'entry-a2');

    // 整个 Run 一个 assistant 消息。
    const turns = assistantMessages();
    expect(turns).toHaveLength(1);
    const statusBlocks = turns[0].blocks?.filter((block) => block.type === 'turn_status') ?? [];
    expect(statusBlocks).toHaveLength(1);
    expect(statusBlocks[0]).toMatchObject({
      status: 'missing_final_answer',
      surfaceRole: 'result',
    });
    // 过程文字不被晋升成答案
    expect(turns[0].turnProjection?.answerBlockIds).toEqual([]);
  });
});
