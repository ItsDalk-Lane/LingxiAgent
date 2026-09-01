/**
 * payload-plain-text.ts — 观测载荷 → TXT 阅读式纯文本投影。
 *
 * 用户要求（2026-09-01）：观测载荷 tab 直出原始内容、不用 JSON 语法、
 * 按「记事本看 TXT」的自然分段排列。规则：
 *   - 字符串：短值行内 `键: 值`；长值/多行值独立成块、保留原始换行；
 *   - 对象：每个字段一行起头、子内容缩进，不输出花括号/引号；
 *   - 数组：每个元素 `[N]` 起头独立成块，块间空行分隔；
 *   - 原始字节（null/数字/布尔）照实打印。
 */

const INLINE_STRING_MAX_CHARS = 80;

function indentOf(depth: number): string {
  return '  '.repeat(depth);
}

function formatStringBlock(value: string, depth: number): string {
  const pad = indentOf(depth);
  return value
    .split('\n')
    .map(line => `${pad}  ${line}`)
    .join('\n');
}

export function formatReadableNode(label: string, value: unknown, depth: number): string {
  const pad = indentOf(depth);
  if (value === null || value === undefined) {
    return `${pad}${label}: ${value === null ? 'null' : '—'}`;
  }
  if (typeof value === 'number' || typeof value === 'boolean') {
    return `${pad}${label}: ${String(value)}`;
  }
  if (typeof value === 'string') {
    const isMultiline = value.includes('\n');
    const collapsed = value.replace(/\s+/g, ' ').trim();
    if (!isMultiline && collapsed.length <= INLINE_STRING_MAX_CHARS) {
      return `${pad}${label}: ${value}`;
    }
    return `${pad}${label}:\n${formatStringBlock(value, depth)}`;
  }
  if (Array.isArray(value)) {
    if (value.length === 0) return `${pad}${label}: （空）`;
    const items = value.map((item, index) => formatReadableNode(`[${index + 1}]`, item, depth + 1));
    return `${pad}${label}:\n${items.join('\n')}`;
  }
  const entries = Object.entries(value as Record<string, unknown>);
  if (entries.length === 0) return `${pad}${label}: （空）`;
  const parts = entries.map(([key, child]) => formatReadableNode(key, child, depth + 1));
  return `${pad}${label}:\n${parts.join('\n')}`;
}

/** 顶层载荷（对象）直接铺开各字段，不带外层包裹标签。 */
export function payloadToReadableText(payload: unknown): string {
  if (payload === null || payload === undefined) return '（无内容）';
  if (typeof payload === 'string') return payload;
  if (typeof payload === 'number' || typeof payload === 'boolean') return String(payload);
  if (Array.isArray(payload)) {
    return payload
      .map((item, index) => formatReadableNode(`[${index + 1}]`, item, 0))
      .join('\n\n');
  }
  const entries = Object.entries(payload as Record<string, unknown>);
  if (entries.length === 0) return '（无内容）';
  return entries
    .map(([key, child]) => formatReadableNode(key, child, 0))
    .join('\n\n');
}
