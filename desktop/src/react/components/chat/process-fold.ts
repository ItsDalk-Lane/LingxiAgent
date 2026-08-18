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

export interface ProcessFoldMessage {
  item: Extract<ChatListItem, { type: 'message' }>;
  originalIndex: number;
  sourceMessageId: string;
  registerSourceMessageElement: boolean;
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
  items: ProcessFoldMessage[];
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
    if (message.turnProjection.provisionalBlockIds?.includes(block.id)) return 'provisional';
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

function clonedAssistantMessage(
  entry: Extract<ChatListItem, { type: 'message' }>,
  blocks: ContentBlock[],
  id: string,
): Extract<ChatListItem, { type: 'message' }> {
  return {
    type: 'message',
    data: {
      ...entry.data,
      id,
      blocks,
    },
  };
}

function collectStats(messages: ProcessFoldMessage[]): ProcessFoldStats {
  let toolCount = 0;
  let thinkingCount = 0;
  let unsuccessfulCount = 0;

  for (const entry of messages) {
    for (const block of entry.item.data.blocks || []) {
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

function collectNavigationAnchors(messages: ProcessFoldMessage[]): ProcessFoldNavigationAnchors {
  const terminal = new Set<string>();
  const subagent = new Set<string>();

  for (const entry of messages) {
    for (const block of entry.item.data.blocks || []) {
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

function processFoldId(messages: ProcessFoldMessage[], turnId: string, hasProjection: boolean): string {
  if (hasProjection) return `${turnId}:process`;
  const first = messages[0]?.sourceMessageId || 'start';
  const last = messages[messages.length - 1]?.sourceMessageId || first;
  return `process-fold-${first}-${last}`;
}

function projectedTurnItems(
  segment: Array<{ entry: Extract<ChatListItem, { type: 'message' }>; originalIndex: number }>,
): TranscriptRenderItem[] {
  if (segment.some(({ entry }) => visibleBlocks(entry.data).some(isMalformedLegacyBlock))) {
    return segment.map(({ entry, originalIndex }) => sourceItem(entry, originalIndex));
  }
  const processMessages: ProcessFoldMessage[] = [];
  const sourceMessages: SourceTranscriptRenderItem[] = [];
  const blockIds: string[] = [];

  for (const { entry, originalIndex } of segment) {
    const blocks = visibleBlocks(entry.data);
    const processBlocks = blocks.filter((block) => isFoldableProcessBlock(entry.data, block));
    const nonProcessBlocks = blocks.filter((block) => !isFoldableProcessBlock(entry.data, block));

    if (processBlocks.length > 0) {
      const continuesAfterFold = nonProcessBlocks.length > 0;
      const processMessageId = continuesAfterFold ? `${entry.data.id}:process` : entry.data.id;
      processMessages.push({
        item: clonedAssistantMessage(entry, processBlocks, processMessageId),
        originalIndex,
        sourceMessageId: entry.data.id,
        registerSourceMessageElement: !continuesAfterFold,
      });
      blockIds.push(...processBlocks.map((block, index) => (
        block.id || `${entry.data.id}:${block.type}:${index}`
      )));
    }

    if (nonProcessBlocks.length > 0) {
      sourceMessages.push(sourceItem(
        clonedAssistantMessage(entry, nonProcessBlocks, entry.data.id),
        originalIndex,
        processBlocks.length > 0,
      ));
    } else if (processBlocks.length === 0) {
      sourceMessages.push(sourceItem(entry, originalIndex));
    }
  }

  if (processMessages.length === 0) return sourceMessages;

  const projection = [...segment]
    .reverse()
    .map(({ entry }) => entry.data.turnProjection)
    .find((value) => !!value);
  const firstMessageId = processMessages[0].sourceMessageId;
  const lastMessageId = processMessages[processMessages.length - 1].sourceMessageId;
  const turnId = projection?.id || `legacy-turn-${firstMessageId}-${lastMessageId}`;
  const status = completedStatus(segment.map(({ entry }) => entry));
  const fold: ProcessFoldRenderItem = {
    type: 'process_fold',
    id: processFoldId(processMessages, turnId, !!projection),
    turnId,
    blockIds,
    items: processMessages,
    originalIndex: processMessages[0].originalIndex,
    stats: collectStats(processMessages),
    status,
    defaultCollapsed: status === 'completed',
    ownsTurnCompletion: sourceMessages.length === 0,
    navigationAnchors: collectNavigationAnchors(processMessages),
  };
  return [fold, ...sourceMessages];
}

export function buildTranscriptRenderItems(
  items: ChatListItem[],
  options: { isStreaming: boolean; liveTurnStatus?: AssistantTurnStatus | null },
): TranscriptRenderItem[] {
  recordChatPerformance('transcript_projection', { itemCount: items.length });
  const rendered: TranscriptRenderItem[] = [];
  const latestAssistantIndex = lastAssistantIndex(items);
  const liveTurnStatus = options.liveTurnStatus ?? null;

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

    // Process Fold 只依赖 Turn 生命周期，不依赖 Session Busy：
    //   1) 有 turnProjection 的现代消息：唯一依据是 turnProjection.status；
    //   2) 正在流式的 live 消息（投影只存在于 live-turn-store）：依据 liveTurnStatus；
    //   3) 只有两者都没有的 legacy 历史消息，才允许回退到 Session Busy。
    // 因此 status（isStreaming）抖动绝不会让进行中的 Turn 折出 Process Fold。
    const hasProjection = segment.some(({ entry }) => !!entry.data.turnProjection);
    const touchesLatest = segment.some(({ originalIndex }) => originalIndex === latestAssistantIndex);
    const segmentStreaming = hasProjection
      ? segment.some(({ entry }) => entry.data.turnProjection?.status === 'streaming')
      : touchesLatest && liveTurnStatus !== null
        ? liveTurnStatus === 'streaming'
        : options.isStreaming && touchesLatest;
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
