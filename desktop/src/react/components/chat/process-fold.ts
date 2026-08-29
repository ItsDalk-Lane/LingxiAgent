import type {
  AssistantTurnStatus,
  ChatListItem,
  ChatMessage,
  ContentBlock,
  ToolCall,
} from '../../stores/chat-types';
import type { KnowledgeRetrievalStats } from '../../../../../shared/knowledge-refs.ts';
import { isToolCallHiddenFromProcessUi } from '../../utils/tool-call-visibility';
import { recordChatPerformance } from '../../utils/chat-performance';
import { resolveContentSurface } from '../../utils/content-semantics';

export interface ProcessFoldStats {
  toolCount: number;
  thinkingCount: number;
  unsuccessfulCount: number;
  /** 知识检索步骤（非 ContentBlock，由配对 user 消息注入）：0/1，计入摘要步骤数。 */
  knowledgeCount: number;
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
  /**
   * ProcessRegion 展示模式（任务书 §二十四/§二十五/§二十六）：
   *   live    → Assistant Run 进行中：显示过程块，不渲染 summary，不折叠。
   *   settled → assistant_run_end 后：渲染 summary，默认折叠。
   * 同一个 ProcessRegion（相同 key）只切换 mode，不重新创建。
   */
  mode: 'live' | 'settled';
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

  return { toolCount, thinkingCount, unsuccessfulCount, knowledgeCount: 0 };
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

// ProcessRegion key 必须稳定（任务书 §二十七）：整个 Run 内 key = `${runId}:process`。
// 前端以源消息 id（跨 live → settled 永久不变）作为 runId 的稳定等价物。
function processFoldId(refs: ProcessFoldBlockRef[]): string {
  const first = refs[0]?.sourceMessageId || 'start';
  return `${first}:process`;
}

function projectedTurnItems(
  segment: Array<{ entry: Extract<ChatListItem, { type: 'message' }>; originalIndex: number }>,
  mode: 'live' | 'settled' = 'settled',
  knowledgeRetrievalByIndex?: ReadonlyMap<number, KnowledgeRetrievalStats>,
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

  if (blockRefs.length === 0) {
    // 知识检索是轮内过程步骤（同工具语义）：纯问答轮没有工具/思考块，只要
    // 轮首挂有检索统计也生成 fold，把检索卡收进折叠（摘要「N 次检索」），
    // 回答正文仍在 fold 后的原位渲染。无检索统计则维持原样不折叠。
    const head = segment[0];
    const headRetrieval = knowledgeRetrievalByIndex && head
      ? knowledgeRetrievalByIndex.get(head.originalIndex) ?? null
      : null;
    if (!headRetrieval) return sourceMessages;
    const headProjection = head.entry.data.turnProjection;
    const headStatus = completedStatus(segment.map(({ entry }) => entry));
    const knowledgeFold: ProcessFoldRenderItem = {
      type: 'process_fold',
      id: `${head.entry.data.id}:process`,
      turnId: headProjection?.id || `legacy-turn-${head.entry.data.id}`,
      blockIds: [],
      refs: [],
      originalIndex: head.originalIndex,
      stats: { toolCount: 0, thinkingCount: 0, unsuccessfulCount: 0, knowledgeCount: 1 },
      status: headStatus,
      defaultCollapsed: mode === 'settled' && headStatus === 'completed',
      // 回答正文源消息仍存在，完成时间/操作留在其上。
      ownsTurnCompletion: false,
      navigationAnchors: { terminal: [], subagent: [] },
      mode,
    };
    return [
      knowledgeFold,
      // 正文消息标记「延续本轮」：头像不重复（prevItem 取自身 → 角色相同），
      // 独立渲染路径的检索卡随之抑制（由 fold 面板承载，防双卡双头像）。
      ...sourceMessages.map((source) => (
        source.originalIndex === head.originalIndex
          ? sourceItem(source.item, source.originalIndex, true)
          : source
      )),
    ];
  }

  const projection = [...segment]
    .reverse()
    .map(({ entry }) => entry.data.turnProjection)
    .find((value) => !!value);
  const firstMessageId = blockRefs[0].sourceMessageId;
  const lastMessageId = blockRefs[blockRefs.length - 1].sourceMessageId;
  const turnId = projection?.id || `legacy-turn-${firstMessageId}-${lastMessageId}`;
  const status = completedStatus(segment.map(({ entry }) => entry));
  // 知识检索卡挂在轮首 assistant 消息上（buildTurnState 只在轮首入 map）；
  // 轮首 process 块进 fold 时卡片随 fold 渲染，并把该步骤计入摘要计数。
  const headKnowledgeRetrieval = knowledgeRetrievalByIndex
    ? blockRefs.reduce<KnowledgeRetrievalStats | null>(
      (found, ref) => found ?? knowledgeRetrievalByIndex.get(ref.originalIndex) ?? null,
      null,
    )
    : null;
  const fold: ProcessFoldRenderItem = {
    type: 'process_fold',
    id: processFoldId(blockRefs),
    turnId,
    blockIds,
    refs: blockRefs,
    originalIndex: blockRefs[0].originalIndex,
    stats: {
      ...collectStats(blockRefs),
      knowledgeCount: headKnowledgeRetrieval ? 1 : 0,
    },
    status,
    // live：不折叠、不显示 summary；settled：completed 默认折叠。
    defaultCollapsed: mode === 'settled' && status === 'completed',
    ownsTurnCompletion: sourceMessages.length === 0,
    navigationAnchors: collectNavigationAnchors(blockRefs),
    mode,
  };
  return [fold, ...sourceMessages];
}

export function buildTranscriptRenderItems(
  items: ChatListItem[],
  options: {
    isStreaming: boolean;
    liveTurnStatus?: AssistantTurnStatus | null;
    /** 轮首 assistant → 配对 user 的检索统计（buildTurnState 产出），供 fold 计数。 */
    knowledgeRetrievalByIndex?: ReadonlyMap<number, KnowledgeRetrievalStats>;
  },
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

    // ProcessRegion 只依赖 Assistant Run 生命周期，不依赖 Session Busy：
    //   1) 有 turnProjection 的现代消息：唯一依据是 turnProjection.status；
    //   2) 正在流式的 live 消息（投影只存在于 live-turn-store）：依据 liveTurnStatus；
    //   3) 只有两者都没有的 legacy 历史消息，才允许回退到 Session Busy。
    // live 与 settled 都走 projectedTurnItems，只是 mode 不同：同一个 ProcessRegion
    // 从第一个 process block 就存在，settled 只是切换展示模式（任务书 §二十四/§二十五）。
    const hasProjection = segment.some(({ entry }) => !!entry.data.turnProjection);
    const touchesLatest = segment.some(({ originalIndex }) => originalIndex === latestAssistantIndex);
    const segmentStreaming = hasProjection
      ? segment.some(({ entry }) => entry.data.turnProjection?.status === 'streaming')
      : touchesLatest && liveTurnStatus !== null
        ? liveTurnStatus === 'streaming'
        : options.isStreaming && touchesLatest;
    if (segmentStreaming) {
      rendered.push(...projectedTurnItems(segment, 'live', options.knowledgeRetrievalByIndex));
    } else {
      rendered.push(...projectedTurnItems(segment, 'settled', options.knowledgeRetrievalByIndex));
    }
    index = cursor;
  }

  return rendered;
}

function fallbackTranslate(key: string, vars?: Record<string, string | number>): string {
  const table: Record<string, string> = {
    'processFold.summary': '✨ {name}忙活了一阵子',
    'processFold.tools': '{n} 个工具',
    'processFold.knowledge': '{n} 次检索',
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
  if (stats.toolCount === 0 && stats.thinkingCount === 0 && stats.knowledgeCount > 0) {
    // 纯检索轮（无工具/思考）：步骤只有知识检索，用专属计数而非「N 个工具」。
    parts.push(translate('processFold.knowledge', { n: stats.knowledgeCount }));
  } else {
    // 知识检索是轮内的一步（同工具语义）：摘要计数把它并进步数，+1。
    const stepCount = stats.toolCount + stats.knowledgeCount;
    if (stepCount > 0) parts.push(translate('processFold.tools', { n: stepCount }));
  }
  if (stats.thinkingCount > 0) parts.push(translate('processFold.thinking', { n: stats.thinkingCount }));
  if (stats.unsuccessfulCount > 0) parts.push(translate('processFold.unsuccessful', { n: stats.unsuccessfulCount }));
  return parts.join(' · ');
}
