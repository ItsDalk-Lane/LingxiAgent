import { extractBlocks } from './block-extractors.ts';
import { extractTextContent, filterUnreferencedInlineImages } from '../core/message-utils.ts';
import { extractPersistedAssistantSemanticSegments } from '../shared/assistant-semantic-segments.ts';
import { isKnownLegacyLingxiToolFailure, skillInvocationName } from '../shared/tool-outcome.ts';
import { projectToolPresentationDetails, safeToolInput, toolResultText } from '../shared/tool-presentation.ts';

// 默认一页最多 50 条消息；8 KiB 门槛把单页重文本的首包量级压在约 400 KiB，
// 同时让普通回复和短工具结果继续一次返回，避免为小内容增加一次网络往返。
export const HISTORY_INLINE_CONTENT_LIMIT = 8 * 1024;
const HISTORY_CONTENT_PREVIEW_LIMIT = 240;
const HISTORY_CONTENT_ID_VERSION = 1;

export type HistoryDeferredContentKind =
  | 'assistant_segment'
  | 'tool_output'
  | 'tool_input'
  | 'tool_patch'
  | 'tool_file_content'
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
  return toolResultText(message);
}

function toolCallAt(message: Record<string, unknown>, ordinal: number): Record<string, unknown> | null {
  if (message.role !== 'assistant' || !Array.isArray(message.content)) return null;
  const block = recordOf(message.content[ordinal]);
  return block && (block.type === 'toolCall' || block.type === 'tool_use') && typeof block.name === 'string'
    ? block : null;
}

function toolContextForResult(messages: unknown[], result: Record<string, unknown>) {
  for (const entry of messages) {
    const message = recordOf(entry);
    if (message?.role !== 'assistant' || !Array.isArray(message.content)) continue;
    const call = message.content.map(recordOf).find(block => block
      && (block.type === 'toolCall' || block.type === 'tool_use')
      && typeof result.toolCallId === 'string' && block.id === result.toolCallId);
    if (call) return { toolName: call.name, args: call.input ?? call.arguments ?? call.args };
  }
  return { toolName: result.toolName };
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
        'tool_input',
        'tool_patch',
        'tool_file_content',
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

export function createHistoryDeferredContentFor(
  record: unknown,
  sourceIndex: number,
  kind: HistoryDeferredContentKind,
  ordinal: number,
  content: string,
  { preview = true }: { preview?: boolean } = {},
): HistoryDeferredContentDescriptor {
  const message = recordOf(record);
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

/**
 * 既有入口（route / projector 调用面不变）：取出单条记录后委托 For 版本。
 * B05 定点读取只持有一条记录时直接使用 createHistoryDeferredContentFor，
 * locator 编码（version/sourceIndex/entryId/kind/ordinal）与这里完全一致。
 */
export function createHistoryDeferredContent(
  sourceMessages: unknown[],
  sourceIndex: number,
  kind: HistoryDeferredContentKind,
  ordinal: number,
  content: string,
  opts: { preview?: boolean } = {},
): HistoryDeferredContentDescriptor {
  return createHistoryDeferredContentFor(sourceMessages[sourceIndex], sourceIndex, kind, ordinal, content, opts);
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
    if (message.role !== 'assistant') return null;
    const segment = extractPersistedAssistantSemanticSegments(message.content)[locator.ordinal];
    return typeof segment?.source === 'string'
      ? { id, kind: locator.kind, content: segment.source }
      : null;
  }

  if (locator.kind === 'tool_input') {
    const call = toolCallAt(message, locator.ordinal);
    if (!call) return null;
    const content = safeToolInput(call.name, call.input ?? call.arguments ?? call.args, Infinity)?.input;
    return typeof content === 'string' ? { id, kind: locator.kind, content } : null;
  }

  if (locator.kind === 'tool_file_content' && message.role === 'assistant') {
    const call = toolCallAt(message, locator.ordinal);
    if (call?.name !== 'write') return null;
    const args = recordOf(call.input ?? call.arguments ?? call.args);
    return typeof args?.content === 'string' ? { id, kind: locator.kind, content: args.content } : null;
  }

  if (['tool_output', 'tool_patch', 'tool_file_content'].includes(locator.kind)) {
    if (message.role !== 'toolResult') return null;
    const details = projectToolPresentationDetails({
      ...message,
      isError: message.isError === true || isKnownLegacyLingxiToolFailure(message),
    }, toolContextForResult(sourceMessages, message), Infinity);
    const content = locator.kind === 'tool_output' ? details?.output
      : locator.kind === 'tool_patch' ? details?.fileChange?.patch
        : details?.fileChange?.content;
    return typeof content === 'string' ? { id, kind: locator.kind, content } : null;
  }

  if (locator.kind === 'skill_content') {
    // locator 不是授权凭证；不能改 kind 绕过普通工具的字段遮盖与合成卡保护。
    if (message.role !== 'toolResult' || message.isError === true || isKnownLegacyLingxiToolFailure(message)
      || !skillInvocationName(toolContextForResult(sourceMessages, message))) return null;
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
