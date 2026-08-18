/**
 * 内部保留协议标签表。<mood>/<pulse>/<reflect> 是保留协议，不是普通文本：
 * 流式解析见 core/events.ts（ReservedTagScanner），历史全文切分见
 * shared/reserved-tag-stream.ts 的 splitReservedTagSegments。
 */
export const INTERNAL_MOOD_TAGS = Object.freeze([
  "mood",
  "pulse",
  "reflect",
] as const);

export type InternalMoodTag = (typeof INTERNAL_MOOD_TAGS)[number];
