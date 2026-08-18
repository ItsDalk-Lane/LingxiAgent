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

import { parseLeadingInternalMoodBlock } from '../../../../shared/internal-mood-block.ts';
import type { LiveAssistantSegment } from '../stores/live-turn-store';

const THINK_LEADING_RE = /^[ \t]*(?:<think>|<thinking>)/;

/** 剥离 segment 开头的内部协议块；只处理 leading 位置，正文内部的标签按普通文本保留。 */
export function sanitizePersistedSegmentSource(
  source: string,
  options: { hasStructuredMood: boolean; hasStructuredThinking: boolean },
): string {
  let text = source;
  let changed = false;
  // 有限次循环：leading 位置可能有 mood + think 交错
  for (let round = 0; round < 4; round += 1) {
    const mood = parseLeadingInternalMoodBlock(text);
    if (mood && options.hasStructuredMood) {
      text = (mood.prefix || '') + mood.rest;
      changed = true;
      continue;
    }
    if (options.hasStructuredThinking && THINK_LEADING_RE.test(text)) {
      const closeMatch = text.match(/<\/(?:think|thinking)>/);
      if (closeMatch && closeMatch.index !== undefined) {
        text = text.slice(closeMatch.index + closeMatch[0].length);
        changed = true;
        continue;
      }
    }
    break;
  }
  if (changed) text = text.replace(/^[ \t]*\n+/, '');
  return text;
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
