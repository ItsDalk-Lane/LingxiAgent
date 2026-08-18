import type {
  AssistantTurnStatus,
  ChatListItem,
  ChatMessage,
  ContentBlock,
  ToolCall,
} from '../../stores/chat-types';
import { isToolCallHiddenFromProcessUi } from '../../utils/tool-call-visibility';
import { recordChatPerformance } from '../../utils/chat-performance';
import { resolveContentSurface } from '../../utils/content-semantics';

export interface ProcessFoldStats {
  toolCount: number;
  thinkingCount: number;
  unsuccessfulCount: number;
}

export interface SourceTranscriptRenderItem {
  type: 'source';
  item: ChatListItem;
  originalIndex: number;
  continuesAssistantTurn?: boolean;
}

/**
 * Process Fold 只持有引用（任务书 §17）：不复制 AssistantMessage，不重新
 * 分配 id。blocks 直接引用源消息的 block 数组；外层折叠只是展示方式。
 */
export interface ProcessFoldBlockRef {
  sourceMessageId: string;
  originalIndex: number;
  blocks: ContentBlock[];
  /** 该源消息是否还承载非 process 内容（answer/result）分屏渲染。 */
  registerSourceMessageElement: boolean;
  /** 完成时间锚点：源消息 timestamp（completion footer 使用）。 */
  timestamp?: number;
}

export interface ProcessFoldNavigationAnchors {
  terminal: string[];
  subagent: string[];
}

export interface ProcessFoldRenderItem {
  type: 'process_fold';
  id: string;
  turnId: string;
  blockIds: string[];
  refs: ProcessFoldBlockRef[];
  originalIndex: number;
  stats: ProcessFoldStats;
  status: Exclude<AssistantTurnStatus, 'streaming'>;
  defaultCollapsed: boolean;
  ownsTurnCompletion: boolean;
  navigationAnchors: ProcessFoldNavigationAnchors;
}

export type TranscriptRenderItem = SourceTranscriptRenderItem | ProcessFoldRenderItem;

export type ProcessFoldTranslator = (key: string, vars?: Record<string, string | number>) => string;

function isAssistantMessage(item: ChatListItem): item is Extract<ChatListItem, { type: 'message' }> {
  return item.type === 'message' && item.data.role === 'assistant';
}

function hasVisibleToolCallsShape(block: ContentBlock): block is Extract<ContentBlock, { type: 'tool_group' }> {
  return block.type === 'tool_group' && Array.isArray(block.tools);
}

function visibleToolCalls(block: Extract<ContentBlock, { type: 'tool_group' }>): ToolCall[] {
  return block.tools.filter((tool) => !isToolCallHiddenFromProcessUi(tool));
}

function visibleBlocks(message: ChatMessage): ContentBlock[] {
  return (message.blocks || []).filter((block) => (
    block.type !== 'session_confirmation' || block.surface !== 'input'
  ));
}

function blockSurface(message: ChatMessage, block: ContentBlock): ContentBlock['surfaceRole'] {
  if (block.id && message.turnProjection) {
    if (message.turnProjection.processBlockIds.includes(block.id)) return 'process';
    if (message.turnProjection.answerBlockIds.includes(block.id)) return 'answer';
    if (message.turnProjection.resultBlockIds.includes(block.id)) return 'result';
    if (message.turnProjection.controlBlockIds.includes(block.id)) return 'control';
  }
  return resolveContentSurface(block);
}

function isFoldableProcessBlock(message: ChatMessage, block: ContentBlock): boolean {
  if (blockSurface(message, block) !== 'process') return false;
  return block.type !== 'tool_group' || hasVisibleToolCallsShape(block);
}

function isMalformedLegacyBlock(block: ContentBlock): boolean {
  if (block.type === 'tool_group') return !Array.isArray(block.tools);
  if (block.type === 'text') {
    return typeof block.html !== 'string' && typeof block.source !== 'string';
  }
  return false;
}

export function isProcessOnlyAssistantMessage(message: ChatMessage): boolean {
  if (message.role !== 'assistant') return false;
  const blocks = visibleBlocks(message);
  return blocks.length > 0 && blocks.every((block) => isFoldableProcessBlock(message, block));
}

function turnInputEntryId(message: ChatMessage): string | null {
  return typeof message.turnInputEntryId === 'string' && message.turnInputEntryId.trim()
    ? message.turnInputEntryId.trim()
    : null;
}

function crossesTurnInputBoundary(current: string | null, next: string | null): boolean {
  return current !== next && (current !== null || next !== null);
}

function lastAssistantIndex(items: ChatListItem[]): number {
  for (let index = items.length - 1; index >= 0; index -= 1) {
    if (isAssistantMessage(items[index])) return index;
  }
  return -1;
}

function sourceItem(
  item: ChatListItem,
  originalIndex: number,
  continuesAssistantTurn = false,
): SourceTranscriptRenderItem {
  return {
    type: 'source',
    item,
    originalIndex,
    ...(continuesAssistantTurn ? { continuesAssistantTurn: true } : {}),
  };
}

function collectStats(refs: ProcessFoldBlockRef[]): ProcessFoldStats {
  let toolCount = 0;
  let thinkingCount = 0;
  let unsuccessfulCount = 0;

  for (const ref of refs) {
    for (const block of ref.blocks) {
      if (block.type === 'thinking') {
        thinkingCount += 1;
        continue;
      }
      if (!hasVisibleToolCallsShape(block)) continue;
      const tools = visibleToolCalls(block);
      toolCount += tools.length;
      unsuccessfulCount += tools.filter((tool) => tool.done && !tool.success).length;
    }
  }

  return { toolCount, thinkingCount, unsuccessfulCount };
}

function objectValue(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function collectNavigationAnchors(refs: ProcessFoldBlockRef[]): ProcessFoldNavigationAnchors {
  const terminal = new Set<string>();
  const subagent = new Set<string>();

  for (const ref of refs) {
    for (const block of ref.blocks) {
      if (block.type === 'subagent') {
        if (block.taskId) subagent.add(block.taskId);
        if (block.streamKey) subagent.add(block.streamKey);
        continue;
      }
      if (!hasVisibleToolCallsShape(block)) continue;
      for (const tool of block.tools) {
        if (tool.name !== 'exec_command') continue;
        if (tool.id) terminal.add(tool.id);
        const execDetails = objectValue(tool.details?.execCommand);
        for (const value of [execDetails?.terminalId, execDetails?.processId]) {
          if (typeof value === 'string' && value.trim()) terminal.add(value);
        }
      }
    }
  }

  return { terminal: [...terminal], subagent: [...subagent] };
}

function completedStatus(
  messages: Extract<ChatListItem, { type: 'message' }>[],
): Exclude<AssistantTurnStatus, 'streaming'> {
  if (messages.some((entry) => entry.data.turnProjection?.status === 'failed')) return 'failed';
  if (messages.some((entry) => entry.data.turnProjection?.status === 'aborted')) return 'aborted';
  return 'completed';
}

function processFoldId(refs: ProcessFoldBlockRef[], turnId: string, hasProjection: boolean): string {
  if (hasProjection) return `${turnId}:process`;
  const first = refs[0]?.sourceMessageId || 'start';
  const last = refs[refs.length - 1]?.sourceMessageId || first;
  return `process-fold-${first}-${last}`;
}

function projectedTurnItems(
  segment: Array<{ entry: Extract<ChatListItem, { type: 'message' }>; originalIndex: number }>,
): TranscriptRenderItem[] {
  if (segment.some(({ entry }) => visibleBlocks(entry.data).some(isMalformedLegacyBlock))) {
    return segment.map(({ entry, originalIndex }) => sourceItem(entry, originalIndex));
  }
  const blockRefs: ProcessFoldBlockRef[] = [];
  const sourceMessages: SourceTranscriptRenderItem[] = [];
  const blockIds: string[] = [];

  for (const { entry, originalIndex } of segment) {
    const blocks = visibleBlocks(entry.data);
    const processBlocks = blocks.filter((block) => isFoldableProcessBlock(entry.data, block));
    const nonProcessBlocks = blocks.filter((block) => !isFoldableProcessBlock(entry.data, block));

    if (processBlocks.length > 0) {
      blockRefs.push({
        sourceMessageId: entry.data.id,
        originalIndex,
        blocks: processBlocks,
        registerSourceMessageElement: nonProcessBlocks.length === 0,
        ...(entry.data.timestamp !== undefined ? { timestamp: entry.data.timestamp } : {}),
      });
      blockIds.push(...processBlocks.map((block, index) => (
        block.id || `${entry.data.id}:${block.type}:${index}`
      )));
    }

    if (nonProcessBlocks.length > 0) {
      // 非 process 内容留在源消息位置；process 块已由 fold 持有引用，
      // 源消息渲染时按投影排除，不再克隆消息（不变量 6）。
      sourceMessages.push(sourceItem(
        { type: 'message', data: { ...entry.data, blocks: nonProcessBlocks } },
        originalIndex,
        processBlocks.length > 0,
      ));
    } else if (processBlocks.length === 0) {
      sourceMessages.push(sourceItem(entry, originalIndex));
    }
  }

  if (blockRefs.length === 0) return sourceMessages;

  const projection = [...segment]
    .reverse()
    .map(({ entry }) => entry.data.turnProjection)
    .find((value) => !!value);
  const firstMessageId = blockRefs[0].sourceMessageId;
  const lastMessageId = blockRefs[blockRefs.length - 1].sourceMessageId;
  const turnId = projection?.id || `legacy-turn-${firstMessageId}-${lastMessageId}`;
  const status = completedStatus(segment.map(({ entry }) => entry));
  const fold: ProcessFoldRenderItem = {
    type: 'process_fold',
    id: processFoldId(blockRefs, turnId, !!projection),
    turnId,
    blockIds,
    refs: blockRefs,
    originalIndex: blockRefs[0].originalIndex,
    stats: collectStats(blockRefs),
    status,
    defaultCollapsed: status === 'completed',
    ownsTurnCompletion: sourceMessages.length === 0,
    navigationAnchors: collectNavigationAnchors(blockRefs),
  };
  return [fold, ...sourceMessages];
}

export function buildTranscriptRenderItems(
  items: ChatListItem[],
  options: { isStreaming: boolean },
): TranscriptRenderItem[] {
  recordChatPerformance('transcript_projection', { itemCount: items.length });
  const rendered: TranscriptRenderItem[] = [];
  const latestAssistantIndex = lastAssistantIndex(items);

  for (let index = 0; index < items.length;) {
    const item = items[index];
    if (!isAssistantMessage(item)) {
      rendered.push(sourceItem(item, index));
      index += 1;
      continue;
    }

    const segment: Array<{
      entry: Extract<ChatListItem, { type: 'message' }>;
      originalIndex: number;
    }> = [];
    let cursor = index;
    let segmentTurnInputEntryId = turnInputEntryId(item.data);
    while (cursor < items.length && isAssistantMessage(items[cursor])) {
      const candidate = items[cursor] as Extract<ChatListItem, { type: 'message' }>;
      const candidateTurnInputEntryId = turnInputEntryId(candidate.data);
      if (
        segment.length > 0
        && crossesTurnInputBoundary(segmentTurnInputEntryId, candidateTurnInputEntryId)
      ) break;
      segment.push({ entry: candidate, originalIndex: cursor });
      segmentTurnInputEntryId = candidateTurnInputEntryId;
      cursor += 1;
    }

    const segmentStreaming = segment.some(({ entry }) => entry.data.turnProjection?.status === 'streaming')
      || (options.isStreaming && segment.some(({ originalIndex }) => originalIndex === latestAssistantIndex));
    if (segmentStreaming) {
      rendered.push(...segment.map(({ entry, originalIndex }) => sourceItem(entry, originalIndex)));
    } else {
      rendered.push(...projectedTurnItems(segment));
    }
    index = cursor;
  }

  return rendered;
}

function fallbackTranslate(key: string, vars?: Record<string, string | number>): string {
  const table: Record<string, string> = {
    'processFold.summary': '✨ {name}忙活了一阵子',
    'processFold.tools': '{n} 个工具',
    'processFold.thinking': '{n} 次思考',
    'processFold.unsuccessful': '{n} 次尝试未成功',
  };
  return interpolate(table[key] || key, vars);
}

function interpolate(template: string, vars?: Record<string, string | number>): string {
  return template.replace(/\{(\w+)\}/g, (_match, name) => String(vars?.[name] ?? ''));
}

export function buildProcessFoldSummary(
  stats: ProcessFoldStats,
  agentName: string,
  translate: ProcessFoldTranslator = fallbackTranslate,
): string {
  const parts = [translate('processFold.summary', { name: agentName })];
  if (stats.toolCount > 0) parts.push(translate('processFold.tools', { n: stats.toolCount }));
  if (stats.thinkingCount > 0) parts.push(translate('processFold.thinking', { n: stats.thinkingCount }));
  if (stats.unsuccessfulCount > 0) parts.push(translate('processFold.unsuccessful', { n: stats.unsuccessfulCount }));
  return parts.join(' · ');
}
