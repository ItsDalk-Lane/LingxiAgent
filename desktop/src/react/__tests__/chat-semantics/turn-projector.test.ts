import { describe, expect, it } from 'vitest';
import type { ContentBlock } from '../../stores/chat-types';
import type { LiveAssistantSegment } from '../../stores/live-turn-store';
import { projectAssistantTurn } from '../../utils/turn-projector';

function segment(
  id: string,
  semanticPhase: LiveAssistantSegment['semanticPhase'],
  source: string,
  kind: LiveAssistantSegment['kind'] = 'text',
): LiveAssistantSegment {
  return { id, kind, semanticPhase, source, lifecycle: 'sealed' };
}

describe('turn projector', () => {
  it('未决文字在流式期是 provisional（不算过程也不算答案），结束后按供应商回退成为答案且不换标识', () => {
    const unresolved = {
      ...segment('assistant:1:text:0', 'unresolved', '连接提前结束前的文字'),
      lifecycle: 'streaming' as const,
    };
    const live = projectAssistantTurn({
      idPrefix: 'entry-assistant-1',
      inputMessageId: 'entry-user-1',
      assistantMessageIds: ['entry-assistant-1'],
      segments: [unresolved],
      legacyBlocks: [],
      status: 'streaming',
    });
    const finalized = projectAssistantTurn({
      idPrefix: 'entry-assistant-1',
      inputMessageId: 'entry-user-1',
      assistantMessageIds: ['entry-assistant-1'],
      segments: [unresolved],
      legacyBlocks: [],
      status: 'completed',
    });

    // 流式期：unresolved ≠ commentary。它是"还没判明身份"的临时文字，
    // 不进过程折叠，也不冒称答案。
    expect(live.blocks[0]).toMatchObject({
      id: 'entry-assistant-1:segment:assistant:1:text:0',
      type: 'text',
      semanticPhase: 'unresolved',
      surfaceRole: 'provisional',
      lifecycle: 'streaming',
    });
    expect(live.projection.provisionalBlockIds).toEqual([
      'entry-assistant-1:segment:assistant:1:text:0',
    ]);
    expect(live.projection.processBlockIds).toEqual([]);
    expect(live.projection.answerBlockIds).toEqual([]);
    expect(finalized.blocks[0]).toMatchObject({
      id: 'entry-assistant-1:segment:assistant:1:text:0',
      type: 'text',
      semanticPhase: 'final_answer',
      surfaceRole: 'answer',
      lifecycle: 'sealed',
    });
    expect(finalized.projection.provisionalBlockIds ?? []).toEqual([]);
    expect(finalized.diagnostics).toEqual([
      expect.objectContaining({ code: 'unresolved_phase_fallback' }),
    ]);
  });

  it('只按显式语义分类过程、答案、结果和待操作项', () => {
    const legacyBlocks: ContentBlock[] = [
      {
        id: 'tool-group-1',
        type: 'tool_group',
        tools: [{ id: 'call-1', name: 'read', done: true, success: true }],
        collapsed: false,
      },
      { id: 'file-1', type: 'file', fileId: 'file-1', filePath: '/tmp/a.md', label: 'a.md', ext: 'md' },
      {
        id: 'confirm-1',
        type: 'session_confirmation',
        confirmId: 'confirm-1',
        kind: 'approval',
        surface: 'message',
        status: 'pending',
        title: '需要确认',
      },
    ];
    const result = projectAssistantTurn({
      idPrefix: 'entry-assistant-1',
      inputMessageId: 'entry-user-1',
      assistantMessageIds: ['entry-assistant-1'],
      segments: [
        segment('assistant:1:reasoning:default', 'reasoning', '推理过程', 'reasoning'),
        segment('assistant:1:text:0', 'commentary', '过程'.repeat(2_000)),
        segment('assistant:1:text:1', 'final_answer', '完成。'),
      ],
      legacyBlocks,
      status: 'completed',
    });

    expect(result.projection).toMatchObject({
      id: 'entry-assistant-1:turn',
      inputMessageId: 'entry-user-1',
      assistantMessageIds: ['entry-assistant-1'],
      status: 'completed',
    });
    expect(result.projection.processBlockIds).toHaveLength(3);
    expect(result.projection.answerBlockIds).toHaveLength(1);
    expect(result.projection.resultBlockIds).toEqual(['file-1']);
    expect(result.projection.controlBlockIds).toEqual(['confirm-1']);
    expect(result.blocks.map((block) => block.surfaceRole)).toEqual([
      'process',
      'process',
      'process',
      'answer',
      'result',
      'control',
    ]);
  });

  it('流式期（非终结态）即使只有过程文字也绝不产生 turn_status 块', () => {
    // 不变式 H：missing_final_answer 只允许在 Turn 终结后出现
    const result = projectAssistantTurn({
      idPrefix: 'entry-assistant-1',
      inputMessageId: 'entry-user-1',
      assistantMessageIds: ['entry-assistant-1'],
      segments: [{
        ...segment('assistant:1:text:0', 'commentary', '还在检查中'),
        lifecycle: 'streaming' as const,
      }],
      legacyBlocks: [],
      status: 'streaming',
    });

    expect(result.blocks.every((block) => block.type !== 'turn_status')).toBe(true);
    expect(result.projection.resultBlockIds).toEqual([]);
  });

  it('正常结束但没有答案和结果时，明确生成未回复状态而不晋升最后一段过程', () => {
    const result = projectAssistantTurn({
      idPrefix: 'entry-assistant-1',
      inputMessageId: 'entry-user-1',
      assistantMessageIds: ['entry-assistant-1'],
      segments: [segment('assistant:1:text:0', 'commentary', '只完成了内部检查')],
      legacyBlocks: [],
      status: 'completed',
    });

    expect(result.blocks).toEqual([
      expect.objectContaining({ semanticPhase: 'commentary', surfaceRole: 'process' }),
      expect.objectContaining({
        type: 'turn_status',
        status: 'missing_final_answer',
        surfaceRole: 'result',
      }),
    ]);
    expect(result.projection.answerBlockIds).toEqual([]);
    expect(result.projection.resultBlockIds).toEqual([
      'entry-assistant-1:missing-final-answer',
    ]);
  });

  it.each([
    ['failed', 'failed'],
    ['aborted', 'aborted'],
  ] as const)('%s 回合没有答案时生成对应状态结果', (turnStatus, blockStatus) => {
    const result = projectAssistantTurn({
      idPrefix: `entry-assistant-${turnStatus}`,
      inputMessageId: 'entry-user-1',
      assistantMessageIds: [`entry-assistant-${turnStatus}`],
      segments: [],
      legacyBlocks: [],
      status: turnStatus,
    });

    expect(result.blocks).toEqual([
      expect.objectContaining({ type: 'turn_status', status: blockStatus, surfaceRole: 'result' }),
    ]);
    expect(result.projection.status).toBe(turnStatus);
  });

  it('同一输入的实时收口与历史重载得到深度相等的块和投影', () => {
    const input = {
      idPrefix: 'entry-assistant-1',
      inputMessageId: 'entry-user-1',
      assistantMessageIds: ['entry-assistant-1'],
      segments: [
        segment('assistant:1:text:0', 'commentary', '内部检查'),
        segment('assistant:1:text:1', 'final_answer', '最终答复'),
      ],
      legacyBlocks: [{
        id: 'tool-group-1',
        type: 'tool_group' as const,
        tools: [{ id: 'call-1', name: 'read', done: true, success: true }],
        collapsed: false,
      }],
      status: 'completed' as const,
    };

    const liveFinalized = projectAssistantTurn(input);
    const reloaded = projectAssistantTurn({
      ...input,
      segments: input.segments.map((item) => ({ ...item })),
      legacyBlocks: input.legacyBlocks.map((item) => ({ ...item })),
    });

    expect(reloaded).toEqual(liveFinalized);
  });
});
