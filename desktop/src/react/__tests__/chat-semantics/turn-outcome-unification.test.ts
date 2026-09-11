import { describe, expect, it } from 'vitest';
import type { ContentBlock } from '../../stores/chat-types';
import { projectAssistantTurn } from '../../utils/turn-projector';
import { resolveAssistantTurnOutcome } from '../../utils/turn-outcome';
import type { LiveAssistantSegment } from '../../stores/live-turn-store';

function segment(
  id: string,
  semanticPhase: LiveAssistantSegment['semanticPhase'],
  source: string,
  kind: LiveAssistantSegment['kind'] = 'text',
): LiveAssistantSegment {
  return { id, kind, semanticPhase, source, lifecycle: 'sealed' };
}

/**
 * F3：结局裁决必须只有一个入口。
 * projectAssistantTurn 与 resolveAssistantTurnOutcome 对同一事实集合必须给出
 * 一致的结局判定——投影器不得再用 answerBlocks.length/resultBlocks.length/
 * controlBlocks.length 的局部判空自行猜测整轮结局。
 */
describe('turn projector × outcome resolver 唯一裁决', () => {
  const project = (segments: LiveAssistantSegment[], legacyBlocks: ContentBlock[], status: 'completed' | 'failed' | 'aborted' | 'streaming' = 'completed') =>
    projectAssistantTurn({
      idPrefix: 'entry-a1',
      inputMessageId: 'entry-u1',
      assistantMessageIds: ['entry-a1'],
      segments,
      legacyBlocks,
      status,
    });

  it('空白答案块不得冒充答案：两套规则必须同判 completed_without_user_output', () => {
    const segments = [segment('s1', 'final_answer', '   \n\t  ')];
    const projected = project(segments, []);

    const resolverOutcome = resolveAssistantTurnOutcome({
      blocks: projected.blocks.filter((block) => block.type !== 'turn_status'),
      status: 'completed',
    });
    // 权威裁决：空白最终答案 = 没有用户可见输出。
    expect(resolverOutcome.outcome).toBe('completed_without_user_output');
    expect(resolverOutcome.missingFinalAnswerReason).toBe('empty_final_answer');

    // 投影器必须消费同一裁决：生成带原因的 turn_status 块并记录 outcome，
    // 而不是因为「存在答案块」就豁免告警。
    const statusBlocks = projected.blocks.filter((block) => block.type === 'turn_status');
    expect(statusBlocks.length).toBe(1);
    expect(statusBlocks[0]).toMatchObject({ status: 'missing_final_answer' });
    expect(projected.projection.outcome).toBe('completed_without_user_output');
    expect(projected.projection.missingFinalAnswerReason).toBe('empty_final_answer');
  });

  it('pending 控制卡是待处理输出：投影器不得因「存在控制块」豁免，也不得误报无输出', () => {
    // 场景 A：pending 确认卡（待用户处理）→ completed_with_control，不产生无回复告警。
    const pendingControl: ContentBlock[] = [
      {
        id: 'confirm-1',
        type: 'session_confirmation',
        confirmId: 'confirm-1',
        kind: 'approval',
        surface: 'message',
        status: 'pending',
      } as ContentBlock,
    ];
    const withPending = project([segment('s1', 'reasoning', '思考', 'reasoning')], pendingControl);
    const resolverOutcome = resolveAssistantTurnOutcome({
      blocks: withPending.blocks.filter((block) => block.type !== 'turn_status'),
      status: 'completed',
    });
    expect(resolverOutcome.outcome).toBe('completed_with_control');
    expect(withPending.blocks.filter((block) => block.type === 'turn_status')).toEqual([]);
    expect(withPending.projection.outcome).toBe('completed_with_control');

    // 场景 B：已确认（非 pending）控制卡且无答案无结果 → completed_with_result，
    // 同样不产生无回复告警（用户可见交付已存在）。
    const resolvedControl: ContentBlock[] = [
      {
        id: 'confirm-2',
        type: 'session_confirmation',
        confirmId: 'confirm-2',
        kind: 'approval',
        surface: 'message',
        status: 'confirmed',
      } as ContentBlock,
    ];
    const withResolved = project([segment('s1', 'reasoning', '思考', 'reasoning')], resolvedControl);
    expect(
      withResolved.blocks.filter((block) => block.type === 'turn_status' && block.status === 'missing_final_answer'),
    ).toEqual([]);
    expect(withResolved.projection.outcome).toBe('completed_with_result');
  });

  it('只有过程块 → 两者同判 completed_without_user_output（only_process_blocks）', () => {
    const segments = [segment('s1', 'reasoning', '长思考', 'reasoning')];
    const toolBlocks: ContentBlock[] = [
      {
        id: 'tools-1',
        type: 'tool_group',
        tools: [{ id: 'call-1', name: 'read', done: true, success: true }],
        collapsed: false,
      },
    ];
    const projected = project(segments, toolBlocks);
    const outcome = resolveAssistantTurnOutcome({
      blocks: projected.blocks.filter((block) => block.type !== 'turn_status'),
      status: 'completed',
    });
    expect(outcome.outcome).toBe('completed_without_user_output');
    expect(outcome.missingFinalAnswerReason).toBe('only_process_blocks');
    expect(projected.projection.outcome).toBe('completed_without_user_output');
    expect(projected.projection.missingFinalAnswerReason).toBe('only_process_blocks');
  });

  it('派生 turn_status 不得作为「已有结果」的证据参与裁决', () => {
    // 历史遗留：blocks 里可能已含旧投影产生的 turn_status。resolver 必须过滤它，
    // 投影器重新裁决时也不得把它算作 result。
    const stale: ContentBlock[] = [
      {
        id: 'stale-status',
        type: 'turn_status',
        status: 'missing_final_answer',
      } as unknown as ContentBlock,
    ];
    const outcome = resolveAssistantTurnOutcome({ blocks: stale, status: 'completed' });
    expect(outcome.outcome).toBe('completed_without_user_output');

    const projected = project([], stale);
    const statusBlocks = projected.blocks.filter((block) => block.type === 'turn_status');
    // 旧状态块被过滤后重新裁决 → 仍然无输出，但同一 Run 只保留一个状态块（稳定 id）。
    expect(statusBlocks.length).toBe(1);
    expect(statusBlocks[0].id).toBe('entry-a1:missing-final-answer');
  });

  it('失败/中止终态：投影器保持显式状态块，outcome 同步', () => {
    const failed = project([segment('s1', 'final_answer', '部分正文')], [], 'failed');
    expect(failed.blocks.some((block) => block.type === 'turn_status' && (block as any).status === 'failed')).toBe(true);
    expect(failed.projection.outcome).toBe('failed');

    const aborted = project([segment('s1', 'final_answer', '部分正文')], [], 'aborted');
    expect(aborted.blocks.some((block) => block.type === 'turn_status' && (block as any).status === 'aborted')).toBe(true);
    expect(aborted.projection.outcome).toBe('aborted');
  });
});
