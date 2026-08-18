import { describe, expect, it } from 'vitest';
import { AssistantEventNormalizer } from '../server/assistant-event-normalizer.ts';

function signature(id: string, phase: 'commentary' | 'final_answer'): string {
  return JSON.stringify({ v: 1, id, phase });
}

/** 收集一个 text 块全部事件：正常流是 start/delta.../end。 */
function feedTextBlock(
  normalizer: AssistantEventNormalizer,
  deltas: string[],
  partial: Record<string, unknown>,
  endPartial?: Record<string, unknown>,
) {
  const events: any[] = [];
  const collect = (batch: any) => {
    events.push(...batch.canonicalEvents);
    events.push(...batch.internalProtocolEvents);
  };
  for (const delta of deltas) {
    collect(normalizer.handleTextEvent({ type: 'text_delta', delta, partial }));
  }
  collect(normalizer.handleTextEvent({
    type: 'text_end',
    ...(endPartial ? { partial: endPartial } : {}),
  }));
  return events;
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

describe('AssistantEventNormalizer 内部协议前移（canonical 不泄漏内部标签）', () => {
  const partial = { role: 'assistant', api: 'anthropic-messages' };

  it('测试A：单 mood 不重复表示，canonical 文本不含 <mood> 标签且产生结构化 mood 事件', () => {
    const normalizer = new AssistantEventNormalizer();
    normalizer.beginAssistantMessage();
    const events = feedTextBlock(
      normalizer,
      ['<mood>Vibe: 专注\nWill: 继续</mood>', '\n\n', '最终答复。'],
      partial,
    );

    const textDeltas = events
      .filter((event) => event.type === 'assistant_segment_delta')
      .map((event) => event.delta)
      .join('');
    expect(textDeltas).not.toContain('<mood>');
    expect(textDeltas).not.toContain('</mood>');
    expect(textDeltas).toBe('最终答复。');

    const moodStarts = events.filter((event) => event.type === 'mood_start');
    expect(moodStarts).toHaveLength(1);
    const moodText = events
      .filter((event) => event.type === 'mood_text')
      .map((event) => event.delta)
      .join('');
    expect(moodText).toBe('Vibe: 专注\nWill: 继续');
    expect(events.filter((event) => event.type === 'mood_end')).toHaveLength(1);
  });

  it('测试A流式变体：mood 标签跨多个 delta 到达时不泄漏', () => {
    const normalizer = new AssistantEventNormalizer();
    normalizer.beginAssistantMessage();
    const events: any[] = [];
    const collect = (batch: any) => {
      events.push(...batch.canonicalEvents);
      events.push(...batch.internalProtocolEvents);
    };
    for (const chunk of ['<mo', 'od>Vibe: 好</m', 'ood>', '正文开始']) {
      collect(normalizer.handleTextEvent({ type: 'text_delta', delta: chunk, partial }));
    }
    collect(normalizer.handleTextEvent({ type: 'text_end' }));

    const textDeltas = events
      .filter((event) => event.type === 'assistant_segment_delta')
      .map((event) => event.delta)
      .join('');
    expect(textDeltas).toBe('正文开始');
    expect(events.filter((event) => event.type === 'mood_start')).toHaveLength(1);
    expect(events.filter((event) => event.type === 'mood_end')).toHaveLength(1);
  });

  it('测试B：多个 mood 各解析一次，正文只保留最终回答', () => {
    const normalizer = new AssistantEventNormalizer();
    normalizer.beginAssistantMessage();
    const events = feedTextBlock(
      normalizer,
      ['<mood>A</mood>', '一些过程', '\n\n<mood>B</mood>', '最终回答'],
      partial,
    );

    const textDeltas = events
      .filter((event) => event.type === 'assistant_segment_delta')
      .map((event) => event.delta)
      .join('');
    // leading-only 解析安全：第一个 mood 是 leading 块被剥离；正文开始后
    // 第二个标签按普通正文保留（模型在讲解标签，吞掉才是错误）。
    expect(textDeltas).toBe('一些过程\n\n<mood>B</mood>最终回答');
    expect(events.filter((event) => event.type === 'mood_start')).toHaveLength(1);
    expect(events.filter((event) => event.type === 'mood_end')).toHaveLength(1);
  });

  it('正文内部的 mood 标签（leading-only 之后）按解析安全规则保留为普通文本', () => {
    // 与 chat-route-mood-segment Case E/F 的 leading-only 契约一致：正文开始后
    // 模型在讲解标签时，标签是内容的一部分，不能被吞。canonical 流里该标签
    // 会保留在文本中；这是解析层安全设计，不是泄漏。
    const normalizer = new AssistantEventNormalizer();
    normalizer.beginAssistantMessage();
    const events = feedTextBlock(normalizer, ['<mood>A</mood>正文解释 <mood>literal</mood>'], partial);

    const textDeltas = events
      .filter((event) => event.type === 'assistant_segment_delta')
      .map((event) => event.delta)
      .join('');
    expect(textDeltas).toBe('正文解释 <mood>literal</mood>');
    expect(events.filter((event) => event.type === 'mood_start')).toHaveLength(1);
  });

  it('think 标签解析为 reasoning 事件，不进入 canonical 可见文本', () => {
    const normalizer = new AssistantEventNormalizer();
    normalizer.beginAssistantMessage();
    const events = feedTextBlock(normalizer, ['<think>先想一下</think>', '结论如下'], partial);

    const textDeltas = events
      .filter((event) => event.type === 'assistant_segment_delta')
      .map((event) => event.delta)
      .join('');
    expect(textDeltas).not.toContain('<think>');
    expect(textDeltas).toBe('结论如下');
    const thinkingStarts = events.filter((event) => event.type === 'assistant_thinking_start');
    expect(thinkingStarts).toHaveLength(1);
  });

  it('phase-at-end API 的分段在解析层不改变阶段语义', () => {
    const normalizer = new AssistantEventNormalizer();
    normalizer.beginAssistantMessage();
    const codexPartial = {
      role: 'assistant',
      api: 'openai-codex-responses',
      provider: 'openai-codex',
    };
    const events: any[] = [];
    const firstBatch = normalizer.handleTextEvent({
      type: 'text_delta', contentIndex: 0, delta: '<mood>A</mood>核对中', partial: codexPartial,
    });
    events.push(...firstBatch.canonicalEvents, ...firstBatch.internalProtocolEvents);
    const end = normalizer.handleTextEvent({
      type: 'text_end',
      contentIndex: 0,
      partial: {
        ...codexPartial,
        content: [{
          type: 'text',
          text: '核对中',
          textSignature: signature('c1', 'commentary'),
        }],
      },
    });
    events.push(...end.canonicalEvents, ...end.internalProtocolEvents);

    expect(events.filter((event) => event.type === 'mood_start')).toHaveLength(1);
    expect(events.find((event) => event.type === 'assistant_segment_end'))
      .toMatchObject({ semanticPhase: 'commentary' });
  });
});
