import { describe, expect, it } from 'vitest';
import { AssistantEventNormalizer } from '../server/assistant-event-normalizer.ts';

function signature(id: string, phase: 'commentary' | 'final_answer'): string {
  return JSON.stringify({ v: 1, id, phase });
}

describe('AssistantEventNormalizer', () => {
  it('把结构化推理增量归一为 reasoning 分段', () => {
    const normalizer = new AssistantEventNormalizer();
    normalizer.beginAssistantMessage();

    const first = normalizer.handleReasoningDelta('先核对事实。');
    const second = normalizer.handleReasoningDelta('再得出结论。');
    const end = normalizer.finishReasoning();

    expect(first.canonicalEvents).toEqual([
      {
        type: 'assistant_segment_start',
        segmentId: 'assistant:1:reasoning:default',
        kind: 'reasoning',
        semanticPhase: 'reasoning',
      },
      {
        type: 'assistant_segment_delta',
        segmentId: 'assistant:1:reasoning:default',
        delta: '先核对事实。',
        semanticPhase: 'reasoning',
      },
    ]);
    expect(second.canonicalEvents).toEqual([
      expect.objectContaining({
        type: 'assistant_segment_delta',
        delta: '再得出结论。',
      }),
    ]);
    expect(end.canonicalEvents).toEqual([
      {
        type: 'assistant_segment_end',
        segmentId: 'assistant:1:reasoning:default',
        semanticPhase: 'reasoning',
      },
    ]);
  });

  it('把没有阶段能力的普通文字直接定义为最终答复，不猜自然语言', () => {
    const normalizer = new AssistantEventNormalizer();
    normalizer.beginAssistantMessage();

    const delta = normalizer.handleTextEvent({
      type: 'text_delta',
      delta: '让我想想，再确认一下。',
      partial: { role: 'assistant', api: 'anthropic-messages' },
    });
    const end = normalizer.handleTextEvent({
      type: 'text_end',
      content: '让我想想，再确认一下。',
      partial: { role: 'assistant', api: 'anthropic-messages' },
    });

    expect(delta.canonicalEvents).toEqual([
      expect.objectContaining({
        type: 'assistant_segment_start',
        segmentId: 'assistant:1:text:default',
        kind: 'text',
        semanticPhase: 'final_answer',
      }),
      expect.objectContaining({
        type: 'assistant_segment_delta',
        segmentId: 'assistant:1:text:default',
        delta: '让我想想，再确认一下。',
      }),
    ]);
    expect(delta.visibleTextDeltas).toEqual(['让我想想，再确认一下。']);
    expect(end.canonicalEvents).toEqual([
      expect.objectContaining({
        type: 'assistant_segment_end',
        segmentId: 'assistant:1:text:default',
        semanticPhase: 'final_answer',
      }),
    ]);
  });

  it('阶段只能在块结束时确定时，先发未决过程段，commentary 结束后不进入正文', () => {
    const normalizer = new AssistantEventNormalizer();
    normalizer.beginAssistantMessage();
    const partial = {
      role: 'assistant',
      api: 'openai-codex-responses',
      provider: 'openai-codex',
      content: [{ type: 'text', text: '正在核对内部状态。' }],
    };

    const delta = normalizer.handleTextEvent({
      type: 'text_delta',
      contentIndex: 0,
      delta: '正在核对内部状态。',
      partial,
    });
    const end = normalizer.handleTextEvent({
      type: 'text_end',
      contentIndex: 0,
      content: '正在核对内部状态。',
      partial: {
        ...partial,
        content: [{
          type: 'text',
          text: '正在核对内部状态。',
          textSignature: signature('commentary-1', 'commentary'),
        }],
      },
    });

    expect(delta.canonicalEvents).toEqual([
      expect.objectContaining({ semanticPhase: 'unresolved' }),
      expect.objectContaining({ type: 'assistant_segment_delta', delta: '正在核对内部状态。' }),
    ]);
    expect(delta.visibleTextDeltas).toEqual([]);
    expect(end.canonicalEvents).toEqual([
      expect.objectContaining({
        type: 'assistant_segment_end',
        semanticPhase: 'commentary',
      }),
    ]);
    expect(end.visibleTextDeltas).toEqual([]);
  });

  it('阶段只能在块结束时确定时，把 final_answer 缓冲一次性送入旧正文兼容链', () => {
    const normalizer = new AssistantEventNormalizer();
    normalizer.beginAssistantMessage();
    const partial = {
      role: 'assistant',
      api: 'openai-responses',
      content: [{ type: 'text', text: '已经完成。' }],
    };

    normalizer.handleTextEvent({
      type: 'text_delta',
      contentIndex: 0,
      delta: '已经',
      partial,
    });
    normalizer.handleTextEvent({
      type: 'text_delta',
      contentIndex: 0,
      delta: '完成。',
      partial,
    });
    const end = normalizer.handleTextEvent({
      type: 'text_end',
      contentIndex: 0,
      partial: {
        ...partial,
        content: [{
          type: 'text',
          text: '已经完成。',
          textSignature: signature('final-1', 'final_answer'),
        }],
      },
    });

    expect(end.canonicalEvents).toEqual([
      expect.objectContaining({
        type: 'assistant_segment_end',
        segmentId: 'assistant:1:text:0',
        semanticPhase: 'final_answer',
      }),
    ]);
    expect(end.visibleTextDeltas).toEqual(['已经完成。']);
  });

  it('供应商直接给出阶段时按该阶段映射，不等待也不改写', () => {
    const normalizer = new AssistantEventNormalizer();
    normalizer.beginAssistantMessage();

    const commentary = normalizer.handleTextEvent({
      type: 'text_delta',
      semanticPhase: 'commentary',
      delta: '结构化过程说明',
      partial: { role: 'assistant', api: 'provider-with-explicit-channels' },
    });

    expect(commentary.canonicalEvents).toEqual([
      expect.objectContaining({
        type: 'assistant_segment_start',
        semanticPhase: 'commentary',
      }),
      expect.objectContaining({
        type: 'assistant_segment_delta',
        semanticPhase: 'commentary',
        delta: '结构化过程说明',
      }),
    ]);
    expect(commentary.visibleTextDeltas).toEqual([]);
  });

  it('回合结束仍未拿到阶段时使用供应商约定的最终答复回退，并留下诊断', () => {
    const normalizer = new AssistantEventNormalizer();
    normalizer.beginAssistantMessage();
    normalizer.handleTextEvent({
      type: 'text_delta',
      contentIndex: 2,
      delta: '连接提前结束前的文字',
      partial: { role: 'assistant', api: 'azure-openai-responses' },
    });

    const result = normalizer.finishTurn();

    expect(result.canonicalEvents).toEqual([
      expect.objectContaining({
        type: 'assistant_segment_end',
        segmentId: 'assistant:1:text:2',
        semanticPhase: 'final_answer',
      }),
    ]);
    expect(result.visibleTextDeltas).toEqual(['连接提前结束前的文字']);
    expect(result.diagnostics).toEqual([
      expect.objectContaining({
        code: 'unresolved_phase_fallback',
        segmentId: 'assistant:1:text:2',
        fallbackPhase: 'final_answer',
      }),
    ]);
  });
});
