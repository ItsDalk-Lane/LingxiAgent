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

export interface AssistantEventNormalizerDiagnostic {
  code: 'unresolved_phase_fallback';
  segmentId: string;
  fallbackPhase: 'final_answer';
}

export interface NormalizedAssistantEventBatch {
  canonicalEvents: CanonicalAssistantSegmentEvent[];
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
  return { canonicalEvents: [], visibleTextDeltas: [], diagnostics: [] };
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
 * 把各供应商的文字增量收敛为统一分段事件。
 * 兼容期同时返回旧正文链需要的文字；未来前端迁完后可删除该兼容输出。
 */
export class AssistantEventNormalizer {
  private messageOrdinal = 0;
  private readonly openTextSegments = new Map<string, OpenTextSegment>();
  private reasoningSegmentId: string | null = null;

  beginAssistantMessage(): void {
    this.messageOrdinal += 1;
  }

  reset(): void {
    this.messageOrdinal = 0;
    this.openTextSegments.clear();
    this.reasoningSegmentId = null;
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
      // 没有可见文字就没有 text segment：text_end 不得凭空制造空 final_answer 段
      // （全部内容属于 mood/think 的消息不因此产生假正文段，也不豁免 missing_final_answer）。
      if (event.type === 'text_end' && !textFromEndEvent(event, fallbackMessage)) {
        return result;
      }
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
      segment.bufferedText += delta;
      result.canonicalEvents.push({
        type: 'assistant_segment_delta',
        segmentId: segment.segmentId,
        delta,
        semanticPhase: segment.semanticPhase,
      });
      if (segment.semanticPhase === 'final_answer') {
        segment.visiblePublished = true;
        result.visibleTextDeltas.push(delta);
      }
      return result;
    }

    const endText = textFromEndEvent(event, fallbackMessage);
    if (!segment.bufferedText && endText) {
      segment.bufferedText = endText;
      result.canonicalEvents.push({
        type: 'assistant_segment_delta',
        segmentId: segment.segmentId,
        delta: endText,
        semanticPhase: segment.semanticPhase,
      });
    }
    const resolvedPhase = explicitTextPhase(event, fallbackMessage)
      || (segment.semanticPhase === 'commentary' ? 'commentary' : 'final_answer');
    if (segment.semanticPhase === 'unresolved' && !explicitTextPhase(event, fallbackMessage)) {
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
    const result = this.finishReasoning();
    for (const segment of this.openTextSegments.values()) {
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
