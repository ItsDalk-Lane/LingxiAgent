import { ThinkTagParser, MoodParser, CardParser } from '../core/events.ts';
import { getAssistantTextPhase } from '../shared/text-signature.ts';

export type CanonicalAssistantPhase = 'reasoning' | 'commentary' | 'final_answer' | 'unresolved';

export type CanonicalAssistantSegmentEvent =
  | {
      type: 'assistant_segment_start';
      segmentId: string;
      kind: 'text' | 'reasoning';
      semanticPhase: CanonicalAssistantPhase;
    }
  | {
      type: 'assistant_segment_delta';
      segmentId: string;
      delta: string;
      semanticPhase: CanonicalAssistantPhase;
    }
  | {
      type: 'assistant_segment_end';
      segmentId: string;
      semanticPhase: Exclude<CanonicalAssistantPhase, 'unresolved'>;
    };

/**
 * 内部协议解析副产物（任务书 §7）：parser 只回答"这是什么内容"，
 * 不决定 provider phase。moodOrdinal / thinkingOrdinal 在首次识别时
 * 分配，之后绝不改变（稳定内容身份）。
 */
export type CanonicalInternalProtocolEvent =
  | { type: 'mood_start'; moodOrdinal: number }
  | { type: 'mood_text'; moodOrdinal: number; delta: string }
  | { type: 'mood_end'; moodOrdinal: number }
  | { type: 'assistant_thinking_start' }
  | { type: 'assistant_thinking_delta'; delta: string }
  | { type: 'assistant_thinking_end' }
  | { type: 'card_start'; attrs: { type: string; plugin: string; route: string; title?: string } }
  | { type: 'card_text'; delta: string }
  | { type: 'card_end' };

export interface AssistantEventNormalizerDiagnostic {
  code: 'unresolved_phase_fallback';
  segmentId: string;
  fallbackPhase: 'final_answer';
}

export interface NormalizedAssistantEventBatch {
  canonicalEvents: CanonicalAssistantSegmentEvent[];
  /** 内部协议结构化副产物（mood/thinking/card），供 WS 层直接下发。 */
  internalProtocolEvents: CanonicalInternalProtocolEvent[];
  /**
   * 兼容期输出：剥净内部标签后的可见文本增量，供旧前端正文链使用。
   * 前端迁完后可删除。
   */
  visibleTextDeltas: string[];
  diagnostics: AssistantEventNormalizerDiagnostic[];
}

interface OpenTextSegment {
  key: string;
  segmentId: string;
  contentIndex: number | null;
  semanticPhase: Exclude<CanonicalAssistantPhase, 'reasoning'>;
  bufferedText: string;
  visiblePublished: boolean;
}

const PHASE_AT_END_APIS = new Set([
  'openai-codex-responses',
  'openai-responses',
  'azure-openai-responses',
]);

function emptyBatch(): NormalizedAssistantEventBatch {
  return { canonicalEvents: [], internalProtocolEvents: [], visibleTextDeltas: [], diagnostics: [] };
}

function objectValue(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function stringValue(value: unknown): string {
  return typeof value === 'string' ? value : '';
}

function contentIndexFromEvent(event: Record<string, unknown>): number | null {
  return Number.isInteger(event.contentIndex) ? event.contentIndex as number : null;
}

function messageFromEvent(
  event: Record<string, unknown>,
  fallbackMessage?: unknown,
): Record<string, unknown> {
  return objectValue(event.partial) || objectValue(fallbackMessage) || {};
}

function textBlockFromEvent(
  event: Record<string, unknown>,
  fallbackMessage?: unknown,
): unknown {
  const message = messageFromEvent(event, fallbackMessage);
  const content = Array.isArray(message.content) ? message.content : null;
  if (!content) return null;
  const contentIndex = contentIndexFromEvent(event);
  if (contentIndex !== null) return content[contentIndex] || null;
  return content.find((block) => objectValue(block)?.type === 'text') || null;
}

function explicitTextPhase(
  event: Record<string, unknown>,
  fallbackMessage?: unknown,
): 'commentary' | 'final_answer' | null {
  for (const value of [event.semanticPhase, event.phase]) {
    if (value === 'commentary' || value === 'final_answer') return value;
  }
  return getAssistantTextPhase(textBlockFromEvent(event, fallbackMessage));
}

function phaseKnownAtEnd(
  event: Record<string, unknown>,
  fallbackMessage?: unknown,
): boolean {
  const message = messageFromEvent(event, fallbackMessage);
  const api = stringValue(message.api).toLowerCase();
  const provider = stringValue(message.provider).toLowerCase();
  return provider === 'openai-codex' || PHASE_AT_END_APIS.has(api);
}

function textFromEndEvent(event: Record<string, unknown>, fallbackMessage?: unknown): string {
  const direct = stringValue(event.content);
  if (direct) return direct;
  const block = objectValue(textBlockFromEvent(event, fallbackMessage));
  return stringValue(block?.text);
}

/**
 * 把各供应商事件收敛为统一 canonical 语义分段。
 *
 * 数据流（任务书 §7）：raw provider text -> Think/Mood/Card 内部协议
 * 解析链（复用 core/events.ts 跨 chunk 状态机）-> 纯净文本 + 结构化
 * 副产物 -> canonical segment 事件。canonical 文本绝不包含已成功解析
 * 的内部协议标签（不变量 2）；provider phase 仍只由 textSignature /
 * semanticPhase / phase-at-end 决定（不变量 8）。
 */
export class AssistantEventNormalizer {
  private messageOrdinal = 0;
  private readonly openTextSegments = new Map<string, OpenTextSegment>();
  private reasoningSegmentId: string | null = null;

  // ── 内部协议解析链（每回合一条：Think -> Mood -> Card）──
  private readonly thinkTagParser = new ThinkTagParser();
  private readonly moodParser = new MoodParser();
  private readonly cardParser = new CardParser();
  private moodOrdinal = -1;
  private inMood = false;
  private inThink = false;

  beginAssistantMessage(): void {
    this.messageOrdinal += 1;
  }

  reset(): void {
    this.messageOrdinal = 0;
    this.openTextSegments.clear();
    this.reasoningSegmentId = null;
    this.thinkTagParser.reset();
    this.moodParser.reset();
    this.cardParser.reset();
    this.moodOrdinal = -1;
    this.inMood = false;
    this.inThink = false;
  }

  /** 一次新的模型生成开始：重新武装 leading opener 资格并冲空缓冲。 */
  beginAssistantSegment(): void {
    this.thinkTagParser.beginAssistantSegment();
    this.moodParser.beginAssistantSegment();
  }

  /**
   * 冲空解析链残留（turn/segment 边界调用）。返回解析出的剩余事件。
   */
  flushProtocolParsers(): NormalizedAssistantEventBatch {
    const result = emptyBatch();
    this.thinkTagParser.flush((evt) => this.consumeThinkEvent(evt, result));
    this.moodParser.flush((evt) => this.consumeMoodEvent(evt, result));
    this.cardParser.flush((evt) => this.consumeCardEvent(evt, result));
    return result;
  }

  private consumeThinkEvent(
    evt: { type: string; data?: string },
    result: NormalizedAssistantEventBatch,
  ): void {
    switch (evt.type) {
      case 'think_start':
        if (!this.inThink) {
          this.inThink = true;
          result.internalProtocolEvents.push({ type: 'assistant_thinking_start' });
        }
        break;
      case 'think_text':
        if (evt.data) {
          if (!this.inThink) {
            this.inThink = true;
            result.internalProtocolEvents.push({ type: 'assistant_thinking_start' });
          }
          result.internalProtocolEvents.push({ type: 'assistant_thinking_delta', delta: evt.data });
        }
        break;
      case 'think_end':
        if (this.inThink) {
          this.inThink = false;
          result.internalProtocolEvents.push({ type: 'assistant_thinking_end' });
        }
        break;
      case 'text':
        if (evt.data) this.feedMoodParser(evt.data, result);
        break;
    }
  }

  private feedMoodParser(text: string, result: NormalizedAssistantEventBatch): void {
    this.moodParser.feed(text, (evt) => this.consumeMoodEvent(evt, result));
  }

  private consumeMoodEvent(
    evt: { type: string; data?: string },
    result: NormalizedAssistantEventBatch,
  ): void {
    switch (evt.type) {
      case 'mood_start': {
        this.moodOrdinal += 1;
        this.inMood = true;
        result.internalProtocolEvents.push({ type: 'mood_start', moodOrdinal: this.moodOrdinal });
        break;
      }
      case 'mood_text':
        if (evt.data) {
          result.internalProtocolEvents.push({
            type: 'mood_text',
            moodOrdinal: this.inMood ? this.moodOrdinal : Math.max(this.moodOrdinal, 0),
            delta: evt.data,
          });
        }
        break;
      case 'mood_end':
        this.inMood = false;
        result.internalProtocolEvents.push({ type: 'mood_end', moodOrdinal: this.moodOrdinal });
        break;
      case 'text':
        if (evt.data) this.feedCardParser(evt.data, result);
        break;
    }
  }

  private feedCardParser(text: string, result: NormalizedAssistantEventBatch): void {
    this.cardParser.feed(text, (evt) => this.consumeCardEvent(evt, result));
  }

  private consumeCardEvent(
    evt: { type: string; data?: string; attrs?: unknown },
    result: NormalizedAssistantEventBatch,
  ): void {
    switch (evt.type) {
      case 'card_start': {
        const attrs = objectValue(evt.attrs) || {};
        result.internalProtocolEvents.push({
          type: 'card_start',
          attrs: {
            type: stringValue(attrs.type) || 'iframe',
            plugin: stringValue(attrs.plugin),
            route: stringValue(attrs.route),
            ...(stringValue(attrs.title) ? { title: stringValue(attrs.title) } : {}),
          },
        });
        break;
      }
      case 'card_text':
        if (evt.data) result.internalProtocolEvents.push({ type: 'card_text', delta: evt.data });
        break;
      case 'card_end':
        result.internalProtocolEvents.push({ type: 'card_end' });
        break;
      case 'text':
        // 纯净可见文本：同时进 canonical segment 与旧正文兼容链
        if (evt.data) {
          this.emitCleanText(evt.data, result);
        }
        break;
    }
  }

  /** 纯净文本落地：写 canonical segment delta + 兼容 visibleTextDeltas。 */
  private emitCleanText(text: string, result: NormalizedAssistantEventBatch): void {
    for (const segment of this.openTextSegments.values()) {
      // OpenTextSegment.semanticPhase 类型上不含 reasoning；运行时防御保留。
      segment.bufferedText += text;
      result.canonicalEvents.push({
        type: 'assistant_segment_delta',
        segmentId: segment.segmentId,
        delta: text,
        semanticPhase: segment.semanticPhase,
      });
      if (segment.semanticPhase === 'final_answer') {
        segment.visiblePublished = true;
        result.visibleTextDeltas.push(text);
      }
      return;
    }
    // 没有打开的 text segment（reasoning 阶段的 <think> 后直接出现正文等罕见路径）：
    // 只发兼容输出，canonical segment 由 handleTextEvent 的常规路径补建。
    result.visibleTextDeltas.push(text);
  }

  handleReasoningDelta(deltaValue: unknown): NormalizedAssistantEventBatch {
    const delta = stringValue(deltaValue);
    if (!delta) return emptyBatch();
    if (this.messageOrdinal === 0) this.beginAssistantMessage();
    const result = emptyBatch();
    if (!this.reasoningSegmentId) {
      this.reasoningSegmentId = `assistant:${this.messageOrdinal}:reasoning:default`;
      result.canonicalEvents.push({
        type: 'assistant_segment_start',
        segmentId: this.reasoningSegmentId,
        kind: 'reasoning',
        semanticPhase: 'reasoning',
      });
    }
    result.canonicalEvents.push({
      type: 'assistant_segment_delta',
      segmentId: this.reasoningSegmentId,
      delta,
      semanticPhase: 'reasoning',
    });
    return result;
  }

  finishReasoning(): NormalizedAssistantEventBatch {
    if (!this.reasoningSegmentId) return emptyBatch();
    const result = emptyBatch();
    result.canonicalEvents.push({
      type: 'assistant_segment_end',
      segmentId: this.reasoningSegmentId,
      semanticPhase: 'reasoning',
    });
    this.reasoningSegmentId = null;
    return result;
  }

  handleTextEvent(rawEvent: unknown, fallbackMessage?: unknown): NormalizedAssistantEventBatch {
    const event = objectValue(rawEvent);
    if (!event || (event.type !== 'text_delta' && event.type !== 'text_end')) return emptyBatch();
    if (this.messageOrdinal === 0) this.beginAssistantMessage();

    const contentIndex = contentIndexFromEvent(event);
    const indexPart = contentIndex === null ? 'default' : String(contentIndex);
    const key = `${this.messageOrdinal}:${indexPart}`;
    let segment = this.openTextSegments.get(key);
    const result = emptyBatch();

    if (!segment) {
      const explicitPhase = explicitTextPhase(event, fallbackMessage);
      const semanticPhase = event.type === 'text_delta' && phaseKnownAtEnd(event, fallbackMessage)
        ? 'unresolved'
        : explicitPhase || (phaseKnownAtEnd(event, fallbackMessage) ? 'unresolved' : 'final_answer');
      segment = {
        key,
        segmentId: `assistant:${this.messageOrdinal}:text:${indexPart}`,
        contentIndex,
        semanticPhase,
        bufferedText: '',
        visiblePublished: false,
      };
      this.openTextSegments.set(key, segment);
      result.canonicalEvents.push({
        type: 'assistant_segment_start',
        segmentId: segment.segmentId,
        kind: 'text',
        semanticPhase,
      });
    }

    if (event.type === 'text_delta') {
      const delta = stringValue(event.delta);
      if (!delta) return result;
      // 原始增量先过内部协议解析链；canonical segment delta 由解析链的
      // 纯净 text 输出驱动，保证 canonical 文本不含内部标签。
      this.thinkTagParser.feed(delta, (evt) => this.consumeThinkEvent(evt, result));
      return result;
    }

    const endText = textFromEndEvent(event, fallbackMessage);
    if (!segment.bufferedText && endText) {
      // text_end 直接给出全文（start/delta 从未到达）：解析整段再收口
      this.thinkTagParser.feed(endText, (evt) => this.consumeThinkEvent(evt, result));
    }
    const flushed = this.flushProtocolParsers();
    result.canonicalEvents.push(...flushed.canonicalEvents);
    result.internalProtocolEvents.push(...flushed.internalProtocolEvents);
    result.visibleTextDeltas.push(...flushed.visibleTextDeltas);

    const explicitPhase = explicitTextPhase(event, fallbackMessage);
    const resolvedPhase = explicitPhase
      || (segment.semanticPhase === 'commentary' ? 'commentary' : 'final_answer');
    if (segment.semanticPhase === 'unresolved' && !explicitPhase) {
      result.diagnostics.push({
        code: 'unresolved_phase_fallback',
        segmentId: segment.segmentId,
        fallbackPhase: 'final_answer',
      });
    }
    if (resolvedPhase === 'final_answer' && !segment.visiblePublished && segment.bufferedText) {
      result.visibleTextDeltas.push(segment.bufferedText);
    }
    result.canonicalEvents.push({
      type: 'assistant_segment_end',
      segmentId: segment.segmentId,
      semanticPhase: resolvedPhase,
    });
    this.openTextSegments.delete(key);
    return result;
  }

  finishMessage(message?: unknown): NormalizedAssistantEventBatch {
    return this.finishOpenSegments(message);
  }

  finishTurn(message?: unknown): NormalizedAssistantEventBatch {
    return this.finishOpenSegments(message);
  }

  private finishOpenSegments(message?: unknown): NormalizedAssistantEventBatch {
    const result = emptyBatch();
    // 先冲空解析链：残留的半个标签/未闭合 mood 按 text/mood 落地
    const flushed = this.flushProtocolParsers();
    result.canonicalEvents.push(...flushed.canonicalEvents);
    result.internalProtocolEvents.push(...flushed.internalProtocolEvents);
    result.visibleTextDeltas.push(...flushed.visibleTextDeltas);

    for (const segment of [...this.openTextSegments.values()]) {
      const phaseEvent: Record<string, unknown> = {
        contentIndex: segment.contentIndex,
        partial: message,
      };
      const explicitPhase = explicitTextPhase(phaseEvent, message);
      const resolvedPhase = explicitPhase
        || (segment.semanticPhase === 'commentary' ? 'commentary' : 'final_answer');
      if (segment.semanticPhase === 'unresolved' && !explicitPhase) {
        result.diagnostics.push({
          code: 'unresolved_phase_fallback',
          segmentId: segment.segmentId,
          fallbackPhase: 'final_answer',
        });
      }
      if (resolvedPhase === 'final_answer' && !segment.visiblePublished && segment.bufferedText) {
        result.visibleTextDeltas.push(segment.bufferedText);
      }
      result.canonicalEvents.push({
        type: 'assistant_segment_end',
        segmentId: segment.segmentId,
        semanticPhase: resolvedPhase,
      });
    }
    this.openTextSegments.clear();
    return result;
  }
}
