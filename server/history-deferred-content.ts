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
/**
 * 实时工具结果引用的坐标版本。
 *
 * 实时 `tool_execution_end` 触发时工具结果还没落到 session JSONL（消息在随后的
 * `message_end` 才 append），此刻拿不到条目 id 与数组下标，而 v1 locator 两者都要。
 * 所以实时引用只记「哪个会话、哪次调用」，展开时再按保存记录现场解析——解析读的是
 * 已经保存下来的那一条记录，不重跑工具、不读当前磁盘文件。
 */
const LIVE_TOOL_CONTENT_ID_VERSION = 2;

export type HistoryDeferredContentKind =
  | 'assistant_segment'
  | 'tool_output'
  | 'tool_search'
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

function encodeLocator(locator: HistoryContentLocator | LiveToolContentLocator): string {
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
        'tool_search',
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

/**
 * 实时引用允许承载的内容类型。
 *
 * 只登记实时链路真的会签发的两种：正文与搜索结构。补丁 / 写入正文 / 技能正文在
 * 实时路径上都还有别的首包字段承载，不在这里多开定位分支——可定位的类型越少，
 * 伪造引用能触碰的面就越小。
 */
type LiveToolContentKind = 'tool_output' | 'tool_search';

interface LiveToolContentLocator {
  version: number;
  sessionPath: string;
  toolCallId: string;
  kind: LiveToolContentKind;
}

function decodeLiveLocator(id: string): LiveToolContentLocator | null {
  try {
    const parsed = JSON.parse(Buffer.from(id, 'base64url').toString('utf8')) as Partial<LiveToolContentLocator>;
    if (
      parsed.version !== LIVE_TOOL_CONTENT_ID_VERSION
      || typeof parsed.sessionPath !== 'string'
      || !parsed.sessionPath
      || typeof parsed.toolCallId !== 'string'
      || !parsed.toolCallId
      || !['tool_output', 'tool_search'].includes(String(parsed.kind))
    ) return null;
    return parsed as LiveToolContentLocator;
  } catch {
    return null;
  }
}

/**
 * 实时大结果的可加载引用。
 *
 * 只登记「哪个会话、哪次调用、要哪种内容」，不携带正文：引用进首包/WS 的体积恒定，
 * 正文留在已经保存的记录里按需解析。与会话内容数组无关，实时路径不必持有它。
 */
export function createLiveToolContentDescriptor<K extends LiveToolContentKind>(
  sessionPath: string,
  toolCallId: string,
  kind: K,
  size: number,
): Extract<HistoryDeferredContentDescriptor, { kind: K }> | null {
  if (!sessionPath || !toolCallId) return null;
  return {
    id: encodeLocator({ version: LIVE_TOOL_CONTENT_ID_VERSION, sessionPath, toolCallId, kind }),
    kind,
    size,
    available: true,
  } as Extract<HistoryDeferredContentDescriptor, { kind: K }>;
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

/**
 * 按调用 id 定位工具结果记录。
 * 实时引用只有 toolCallId 可用，所以两个版本都要按 id 找；v1 的 sourceIndex 只是
 * 快路径，真伪仍由 entryId 校验（见 resolveHistoryDeferredContent）。
 */
function toolResultByCallId(
  sourceMessages: unknown[],
  toolCallId: string,
): { message: Record<string, unknown>; sourceIndex: number } | null {
  for (let index = sourceMessages.length - 1; index >= 0; index -= 1) {
    const message = recordOf(sourceMessages[index]);
    if (message?.role === 'toolResult' && message.toolCallId === toolCallId) {
      return { message, sourceIndex: index };
    }
  }
  return null;
}

/** 单条工具结果的展示投影；实时引用与历史 locator 共用，保证两条链路同语义。 */
function projectToolResultDetails(
  sourceMessages: unknown[],
  message: Record<string, unknown>,
): ReturnType<typeof projectToolPresentationDetails> {
  return projectToolPresentationDetails({
    ...message,
    isError: message.isError === true || isKnownLegacyLingxiToolFailure(message),
  }, toolContextForResult(sourceMessages, message), Infinity);
}

function contentFromToolResultDetails(
  details: ReturnType<typeof projectToolPresentationDetails>,
  kind: HistoryDeferredContentKind,
): string | undefined {
  // 搜索的 path / line / context / 统计是本次执行已经知道的结构化事实，
  // 首包删掉 files 只是为了体积；恢复时必须原样取回，不能降级成文本重猜。
  if (kind === 'tool_output') return details?.output;
  if (kind === 'tool_search') return details?.search ? JSON.stringify(details.search) : undefined;
  if (kind === 'tool_patch') return details?.fileChange?.patch;
  if (kind === 'tool_file_content') return details?.fileChange?.content;
  return undefined;
}

function resolveLiveToolContent(
  sourceMessages: unknown[],
  locator: LiveToolContentLocator,
  requestedSessionPath: string | null,
): { id: string; kind: HistoryDeferredContentKind; content: string } | null {
  // 引用里写的会话必须就是本次读取授权的会话：locator 不是授权凭证。
  if (!requestedSessionPath || requestedSessionPath !== locator.sessionPath) return null;
  const found = toolResultByCallId(sourceMessages, locator.toolCallId);
  if (!found) return null;
  const details = projectToolResultDetails(sourceMessages, found.message);
  const content = contentFromToolResultDetails(details, locator.kind);
  return typeof content === 'string' ? { id: '', kind: locator.kind, content } : null;
}

export function resolveHistoryDeferredContent(
  sourceMessages: unknown[],
  id: string,
  requestedSessionPath: string | null = null,
): {
  id: string;
  kind: HistoryDeferredContentKind;
  content: string;
  mimeType?: string;
} | null {
  const live = decodeLiveLocator(id);
  if (live) {
    const resolved = resolveLiveToolContent(sourceMessages, live, requestedSessionPath);
    return resolved ? { ...resolved, id } : null;
  }
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

  if (['tool_output', 'tool_search', 'tool_patch', 'tool_file_content'].includes(locator.kind)) {
    if (message.role !== 'toolResult') return null;
    const content = contentFromToolResultDetails(projectToolResultDetails(sourceMessages, message), locator.kind);
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
