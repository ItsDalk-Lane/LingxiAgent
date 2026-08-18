/**
 * 历史迁移边界的 canonical segment 净化（任务书 §22/§23）。
 *
 * 只存在于 history adapter：流式 canonical 事件链已经保证文本不含内部
 * 协议标签；但旧版本落盘的 assistantSegments.source 可能残留 <mood>/
 * <think> 原始标签（旧服务端 normalizer 不剥标签）。这里做一次性去重：
 * 剥离已被结构化 mood/thinking block 承载的 leading 内部块，避免
 * canonical text 与 mood block 双重表示（不变量 1）。
 * 禁止把这套逻辑放回主 renderer。
 */

import { INTERNAL_MOOD_TAGS } from '../../../../shared/internal-mood-block.ts';
import { splitReservedTagSegments } from '../../../../shared/reserved-tag-stream.ts';
import type { LiveAssistantSegment } from '../stores/live-turn-store';

/** 全部保留协议标签：mood 家族 + think 家族。 */
const RESERVED_TAGS: readonly string[] = [...INTERNAL_MOOD_TAGS, 'think', 'thinking'];

/** 剥离 segment 开头的内部协议块；只处理 leading 位置，正文内部的标签按普通文本保留。 */
export function sanitizePersistedSegmentSource(
  source: string,
  options: { hasStructuredMood: boolean; hasStructuredThinking: boolean },
): string {
  const segments = splitReservedTagSegments(source, RESERVED_TAGS);
  let start = 0;
  let changed = false;
  // leading 位置可能有 mood + think 交错；空白片段不阻断 leading 判定
  while (start < segments.length) {
    const segment = segments[start];
    if (segment.type === 'text') {
      if (segment.text.trim()) break;
      start += 1;
      continue;
    }
    const isMood = (INTERNAL_MOOD_TAGS as readonly string[]).includes(segment.tag);
    if (isMood ? options.hasStructuredMood : options.hasStructuredThinking) {
      changed = true;
      start += 1;
      continue;
    }
    break;
  }
  if (!changed) return source;
  // 非剥离部分的标签块按原字面量重建，保证正文内部标签一字不动
  const rest = segments.slice(start).map((segment) => (
    segment.type === 'text' ? segment.text : `<${segment.tag}>${segment.content}</${segment.tag}>`
  )).join('');
  return rest.replace(/^[ \t]*\n+/, '');
}

/** 对整组 segments 应用净化；结构化 block 存在性由调用方（history-builder）判定。 */
export function sanitizePersistedSegments(
  segments: readonly LiveAssistantSegment[],
  options: { hasStructuredMood: boolean; hasStructuredThinking: boolean },
): LiveAssistantSegment[] {
  return segments.map((segment) => (
    segment.kind === 'text'
      ? { ...segment, source: sanitizePersistedSegmentSource(segment.source, options) }
      : segment
  ));
}
