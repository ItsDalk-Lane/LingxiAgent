import { extractBlocks } from './block-extractors.ts';
import { extractTextContent, filterUnreferencedInlineImages } from '../core/message-utils.ts';
import { extractPersistedAssistantSemanticSegments } from '../shared/assistant-semantic-segments.ts';

// 默认一页最多 50 条消息；8 KiB 门槛把单页重文本的首包量级压在约 400 KiB，
// 同时让普通回复和短工具结果继续一次返回，避免为小内容增加一次网络往返。
export const HISTORY_INLINE_CONTENT_LIMIT = 8 * 1024;
const HISTORY_CONTENT_PREVIEW_LIMIT = 240;
const HISTORY_CONTENT_ID_VERSION = 1;

export type HistoryDeferredContentKind =
  | 'assistant_segment'
  | 'tool_output'
  | 'skill_content'
  | 'screenshot'
  | 'artifact'
  | 'inline_image';

export interface HistoryDeferredContentDescriptor {
  id: string;
  kind: HistoryDeferredContentKind;
  size: number;
  preview?: string;
  available: true;
}

interface HistoryContentLocator {
  version: number;
  sourceIndex: number;
  entryId: string | null;
  kind: HistoryDeferredContentKind;
  ordinal: number;
}

function recordOf(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function rawTextResult(message: Record<string, unknown>): string | null {
  if (!Array.isArray(message.content) || message.content.length !== 1) return null;
  const block = recordOf(message.content[0]);
  return block?.type === 'text' && typeof block.text === 'string' ? block.text : null;
}

function encodeLocator(locator: HistoryContentLocator): string {
  return Buffer.from(JSON.stringify(locator), 'utf8').toString('base64url');
}

function decodeLocator(id: string): HistoryContentLocator | null {
  try {
    const parsed = JSON.parse(Buffer.from(id, 'base64url').toString('utf8')) as Partial<HistoryContentLocator>;
    if (
      parsed.version !== HISTORY_CONTENT_ID_VERSION
      || !Number.isInteger(parsed.sourceIndex)
      || (parsed.sourceIndex as number) < 0
      || !Number.isInteger(parsed.ordinal)
      || (parsed.ordinal as number) < 0
      || ![
        'assistant_segment',
        'tool_output',
        'skill_content',
        'screenshot',
        'artifact',
        'inline_image',
      ].includes(String(parsed.kind))
      || (parsed.entryId !== null && typeof parsed.entryId !== 'string')
    ) return null;
    return parsed as HistoryContentLocator;
  } catch {
    return null;
  }
}

export function shouldDeferHistoryContent(value: unknown): value is string {
  return typeof value === 'string' && value.length > HISTORY_INLINE_CONTENT_LIMIT;
}

export function createHistoryDeferredContent(
  sourceMessages: unknown[],
  sourceIndex: number,
  kind: HistoryDeferredContentKind,
  ordinal: number,
  content: string,
  { preview = true }: { preview?: boolean } = {},
): HistoryDeferredContentDescriptor {
  const message = recordOf(sourceMessages[sourceIndex]);
  const entryId = typeof message?.id === 'string' && message.id.trim() ? message.id.trim() : null;
  return {
    id: encodeLocator({
      version: HISTORY_CONTENT_ID_VERSION,
      sourceIndex,
      entryId,
      kind,
      ordinal,
    }),
    kind,
    size: content.length,
    ...(preview ? { preview: content.slice(0, HISTORY_CONTENT_PREVIEW_LIMIT) } : {}),
    available: true,
  };
}

export function resolveHistoryDeferredContent(sourceMessages: unknown[], id: string): {
  id: string;
  kind: HistoryDeferredContentKind;
  content: string;
  mimeType?: string;
} | null {
  const locator = decodeLocator(id);
  if (!locator) return null;
  const message = recordOf(sourceMessages[locator.sourceIndex]);
  if (!message) return null;
  const currentEntryId = typeof message.id === 'string' && message.id.trim() ? message.id.trim() : null;
  if (currentEntryId !== locator.entryId) return null;

  if (locator.kind === 'assistant_segment') {
    const segment = extractPersistedAssistantSemanticSegments(message.content)[locator.ordinal];
    return typeof segment?.source === 'string'
      ? { id, kind: locator.kind, content: segment.source }
      : null;
  }

  if (locator.kind === 'tool_output' || locator.kind === 'skill_content') {
    const content = rawTextResult(message);
    return content == null ? null : { id, kind: locator.kind, content };
  }

  if (locator.kind === 'inline_image') {
    const { text, images } = extractTextContent(message.content);
    const image = filterUnreferencedInlineImages(text, images)[locator.ordinal];
    return typeof image?.data === 'string'
      ? { id, kind: locator.kind, content: image.data, mimeType: image.mimeType }
      : null;
  }

  const block = extractBlocks(message.toolName || message.customType, message.details, message)?.[locator.ordinal];
  if (locator.kind === 'screenshot' && block?.type === 'screenshot' && typeof block.base64 === 'string') {
    return { id, kind: locator.kind, content: block.base64, mimeType: block.mimeType };
  }
  if (locator.kind === 'artifact' && block?.type === 'artifact' && typeof block.content === 'string') {
    return { id, kind: locator.kind, content: block.content };
  }
  return null;
}
