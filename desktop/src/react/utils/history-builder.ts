/**
 * history-builder.ts — 将 /api/sessions/messages 的 API 响应转换为 ChatListItem[]
 *
 * 替代 app-messages-shim.ts loadMessages() 中的 DOM 构建循环。
 */

import type {
  ChatMessage,
  ChatListItem,
  ContentBlock,
  HistoryRunFacts,
  HistoryRunRecordFact,
  SessionRegistryFile,
  UserAttachment,
} from '../stores/chat-types';
import type { TodoItem } from '../types';
import { parseUserAttachments } from './message-parser';
import { renderMarkdown } from './markdown';
import { extOfName } from './file-kind';
import { buildAssistantBlocksFromContent } from './assistant-block-builder';
import { recordChatPerformance } from './chat-performance';
import { normalizeContentBlocks } from './content-semantics';
import { projectAssistantTurn } from './turn-projector';
import { sanitizePersistedSegments } from './history-segment-sanitizer';
import type { LiveAssistantSegment } from '../stores/live-turn-store';

/* eslint-disable @typescript-eslint/no-explicit-any -- API 历史消息 JSON 结构动态，难以静态收窄 */

const LEGACY_STEER_PREFIX_RE = /^(?:（插话，无需 MOOD）|\(Interjection, no MOOD needed\))\n?/;
const MEDIA_ONLY_PLACEHOLDER_TEXT = new Set([
  '(看图)',
  '（看图）',
  '(view image)',
  '（看圖）',
  '（画像を見る）',
  '(이미지 보기)',
  '(看视频)',
  '（看视频）',
  '(view video)',
  '（看影片）',
  '（動画を見る）',
  '(비디오 보기)',
  '(听音频)',
  '（听音频）',
  '(listen to audio)',
  '（聽音訊）',
  '（音声を聞く）',
  '(오디오 듣기)',
]);

// 历史里可能残留 provider/model 边界注入的图片尺寸提示。
// 这类 <file name="image-N"> 行只服务于模型坐标换算，不属于用户输入的可见正文。
const LEGACY_IMAGE_DIMENSION_NOTE_LINE_RE =
  /^<file name="image-\d+">\[Image: original \d+x\d+, displayed at \d+x\d+\. Multiply coordinates by \d+(?:\.\d+)? to map to original image\.\]<\/file>$/;

// ── API 响应类型 ──

/** 历史消息聚合时"消息内 content 索引 → 全 Turn 序号"的跨消息偏移步长。 */
const HISTORY_MESSAGE_ORDER_STRIDE = 1000;

export interface HistoryApiResponse {
  messages: Array<{
    id?: string;
    entryId?: string;
    sourceEntryId?: string;
    clientMessageId?: string;
    snapshotVersion?: number;
    role: string;
    content: string;
    assistantSegments?: LiveAssistantSegment[];
    turnStatus?: 'completed' | 'failed' | 'aborted';
    thinking?: string;
    toolCalls?: Array<{
      id?: string;
      toolCallId?: string;
      name: string;
      args?: Record<string, unknown>;
      status?: 'succeeded' | 'failed' | 'unknown';
      success?: boolean;
      error?: string;
      details?: Record<string, unknown>;
      processOrder?: number;
    }>;
    images?: Array<{ data?: string; mimeType: string; deferred?: import('../stores/chat-types').DeferredHistoryContent }>;
    timestamp?: number | string | null;
    sourceIndex?: number;
    turnInputEntryId?: string;
    turnInputVisible?: boolean;
    /** Run 边界（display 序号）：该记录所属 Run 的首/尾 displayable 记录序号。 */
    turnStartIndex?: number;
    turnEndIndex?: number;
    agentReview?: import('../stores/chat-types').AgentReviewContext;
    agentReviewRequest?: import('../stores/chat-types').AgentReviewRequestContext;
    sessionRefs?: Array<{ sessionId: string; label: string }>;
    agentMentions?: Array<{ agentId: string; label: string }>;
    knowledgeRefs?: {
      notebookIds: string[];
      mode: import('../stores/chat-types').KnowledgeReferenceModeDisplay;
      notebooks?: Array<{ id: string; name?: string }>;
    };
    knowledgeRetrieval?: import('../../../../shared/knowledge-refs.ts').KnowledgeRetrievalStats;
    displayText?: string;
    modelCallRef?: { modelCallId: string; traceId: string | null; parentCallId: string | null };
  }>;
  sessionFiles?: SessionRegistryFile[];
  blocks?: Array<any>;
  // COMPAT(v0.98/v0.127, remove no earlier than v0.133):
  // 以下三个老字段在新服务端不再返回；其中 artifacts 仅保留为旧 session 恢复协议。
  fileOutputs?: Array<{
    afterIndex: number;
    files: Array<{ fileId?: string; filePath: string; label: string; ext: string; mime?: string; kind?: string; storageKind?: string; presentation?: string; listed?: boolean; status?: string; missingAt?: number | null }>;
  }>;
  artifacts?: Array<{
    afterIndex: number;
    artifactId: string;
    artifactType: string;
    title: string;
    content: string;
    language?: string;
    fileId?: string;
    filePath?: string;
    label?: string;
    ext?: string;
    mime?: string;
    kind?: string;
    storageKind?: string;
    presentation?: string;
    listed?: boolean;
    status?: string;
    missingAt?: number | null;
  }>;
  todos?: TodoItem[];
  hasMore?: boolean;
  /** 服务端下发的下一页游标（display 序号字符串；null=没有更早记录）。 */
  nextBefore?: string | null;
}

// ── 兼容层 ──

/**
 * COMPAT(v0.98/v0.127, remove no earlier than v0.133):
 * 旧历史消息兼容层，可在确认老 session 已完成迁移后整个删除。
 *
 * 将老格式（fileOutputs/artifacts/cards）转为新 blocks[] 格式。
 * 新服务端返回 blocks[]，此函数只在升级过渡期（老服务端 → 新前端）命中。
 * 如果没有 data.blocks，还需从 toolCalls 重建 cron/settings 确认卡片，
 * 因为老 session 的 toolResult.details 没有 jobData/settingKey 字段。
 */
function normalizeBlocks(data: HistoryApiResponse): Array<any> {
  if (data.blocks) return data.blocks.map(normalizeHistoryBlock).filter((block): block is Record<string, any> => !!block);
  const blocks: Array<any> = [];
  for (const fo of (data.fileOutputs || [])) {
    for (const f of fo.files) {
      blocks.push({ type: 'file', afterIndex: fo.afterIndex, ...f });
    }
  }
  for (const ar of (data.artifacts || [])) {
    const { afterIndex, ...artifact } = ar;
    blocks.push({ type: 'artifact', afterIndex, ...artifact });
  }

  // COMPAT: 从 toolCalls 重建 cron/settings 确认卡片（仅老 session 无 blocks[] 时）
  for (let i = 0; i < (data.messages || []).length; i++) {
    const m = data.messages[i];
    if (m.role !== 'assistant' || !m.toolCalls) continue;
    for (const tc of m.toolCalls) {
      if (tc.name === 'update_settings' && tc.args) {
        const a = tc.args as Record<string, string>;
        if (a.action === 'apply' || (!a.action && a.key && a.value)) {
          blocks.push({
            type: 'settings_confirm',
            afterIndex: i,
            confirmId: '',
            settingKey: a.key || '',
            cardType: (a.key === 'sandbox' || a.key === 'memory.enabled') ? 'toggle' : 'list',
            currentValue: '',
            proposedValue: a.value || '',
            label: a.key || '',
            status: 'confirmed',
          });
        }
      }
      if (tc.name === 'cron' && tc.args) {
        const a = tc.args as Record<string, any>;
        if (a.action === 'add') {
          blocks.push({
            type: 'cron_confirm',
            afterIndex: i,
            confirmId: '',
            jobData: { type: a.type, schedule: a.schedule, prompt: a.prompt, label: a.label },
            status: 'approved',
          });
        }
      }
    }
  }

  return blocks;
}

function isRecord(value: unknown): value is Record<string, any> {
  return !!value && typeof value === 'object' && !Array.isArray(value);
}

function nonEmptyString(value: unknown): string | null {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  return trimmed ? value : null;
}

function normalizeBlockAfterIndex(value: unknown): number | null {
  return Number.isInteger(value) && (value as number) >= 0 ? value as number : null;
}

function basenamePortable(value: string): string {
  const normalized = value.replace(/\\/g, '/');
  const slash = normalized.lastIndexOf('/');
  return slash >= 0 ? normalized.slice(slash + 1) : normalized;
}

function normalizePathKey(value: unknown): string | null {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  if (!trimmed) return null;
  return trimmed.replace(/\\/g, '/');
}

function sessionFileIdLookupKey(fileId: unknown): string | null {
  if (typeof fileId !== 'string') return null;
  const trimmed = fileId.trim();
  return trimmed ? `session-file:${trimmed}` : null;
}

function buildSessionFileLookup(sessionFiles: unknown): Map<string, SessionRegistryFile> {
  const lookup = new Map<string, SessionRegistryFile>();
  if (!Array.isArray(sessionFiles)) return lookup;
  for (const file of sessionFiles) {
    if (!isRecord(file)) continue;
    const record = file as SessionRegistryFile;
    const keys = [
      sessionFileIdLookupKey(record.fileId),
      sessionFileIdLookupKey(record.id),
      ...(record.legacyFileIds || []).map(sessionFileIdLookupKey),
      normalizePathKey(record.filePath),
      normalizePathKey(record.realPath),
      ...(record.legacyFilePaths || []).map(normalizePathKey),
      normalizePathKey(record.resource?.links?.content),
      normalizePathKey(record.resource?.links?.self),
    ].filter((key): key is string => !!key);
    for (const key of keys) {
      if (!lookup.has(key)) lookup.set(key, record);
    }
  }
  return lookup;
}

// 历史 marker 只表达“这条消息引用了哪个文件”。展示名、mime、生命周期状态
// 必须来自 SessionFile 账本；账本不存在时才退回 marker path 的 basename。
function displayNameForSessionFile(file: SessionRegistryFile | null | undefined, fallbackPath: string): string {
  if (file?.displayName) return file.displayName;
  if (file?.label) return file.label;
  if (file?.filename) return file.filename;
  return basenamePortable(fallbackPath);
}

function presentationForSessionFile(file: SessionRegistryFile | null | undefined, fallback?: Partial<UserAttachment>): string {
  if (file?.presentation === 'voice-input' || fallback?.presentation === 'voice-input') return 'voice-input';
  return 'attachment';
}

function listedForSessionFile(file: SessionRegistryFile | null | undefined, fallback?: Partial<UserAttachment>): boolean {
  if (typeof file?.listed === 'boolean') return file.listed;
  if (typeof fallback?.listed === 'boolean') return fallback.listed;
  return presentationForSessionFile(file, fallback) !== 'voice-input';
}

function attachmentFromRef(
  ref: { path: string; name: string; isDirectory?: boolean; fileId?: string },
  sessionFileLookup: Map<string, SessionRegistryFile>,
  fallback?: Partial<UserAttachment>,
): UserAttachment {
  const sessionFile = (ref.fileId ? sessionFileLookup.get(sessionFileIdLookupKey(ref.fileId) || '') : null)
    ?? (fallback?.fileId ? sessionFileLookup.get(sessionFileIdLookupKey(fallback.fileId) || '') : null)
    ?? sessionFileLookup.get(normalizePathKey(ref.path) || '');
  const fileId = sessionFile?.fileId || sessionFile?.id || ref.fileId || fallback?.fileId;
  const filePath = sessionFile?.filePath || sessionFile?.realPath || fallback?.path || ref.path;
  const mimeType = sessionFile?.mime || fallback?.mimeType;
  const status = sessionFile?.status || fallback?.status;
  const hasMissingAt = !!sessionFile && Object.prototype.hasOwnProperty.call(sessionFile, 'missingAt')
    ? true
    : !!fallback && Object.prototype.hasOwnProperty.call(fallback, 'missingAt');
  const missingAt = !!sessionFile && Object.prototype.hasOwnProperty.call(sessionFile, 'missingAt')
    ? sessionFile.missingAt
    : fallback?.missingAt;
  const presentation = presentationForSessionFile(sessionFile, fallback);
  const listed = listedForSessionFile(sessionFile, fallback);
  const transcription = sessionFile?.transcription || fallback?.transcription;
  const waveform = sessionFile?.waveform || fallback?.waveform;
  return {
    ...(fileId ? { fileId } : {}),
    path: filePath,
    name: displayNameForSessionFile(sessionFile, fallback?.name || ref.path || ref.name),
    isDir: sessionFile?.isDirectory ?? ref.isDirectory ?? false,
    ...(mimeType ? { mimeType } : {}),
    ...(presentation !== 'attachment' ? { presentation } : {}),
    ...(listed === false ? { listed } : {}),
    ...(status ? { status } : {}),
    ...(hasMissingAt ? { missingAt } : {}),
    ...(transcription ? { transcription } : {}),
    ...(waveform ? { waveform } : {}),
  };
}

type SessionFileMarkerRef = { fileId: string; sessionPath?: string; label: string; kind: string };

function markerMatchesAttachment(marker: SessionFileMarkerRef, ref: { path: string; name: string; isDirectory?: boolean }): boolean {
  if (marker.kind === 'directory' && !ref.isDirectory) return false;
  if (marker.kind !== 'directory' && ref.isDirectory) return false;
  return marker.label === ref.path || marker.label === ref.name;
}

function consumeSessionFileMarkerForAttachment(
  markers: SessionFileMarkerRef[],
  consumed: Set<number>,
  ref: { path: string; name: string; isDirectory?: boolean },
): SessionFileMarkerRef | null {
  for (let index = 0; index < markers.length; index += 1) {
    if (consumed.has(index)) continue;
    const marker = markers[index];
    if (!markerMatchesAttachment(marker, ref)) continue;
    consumed.add(index);
    return marker;
  }
  return null;
}

function normalizeUserVisibleText(text: string, hasMediaAttachment: boolean): string {
  if (!hasMediaAttachment) return text;
  const withoutLegacyImageNotes = text
    .split(/\r?\n/)
    .filter(line => !LEGACY_IMAGE_DIMENSION_NOTE_LINE_RE.test(line.trim()))
    .join('\n')
    .trim();
  const trimmed = withoutLegacyImageNotes.trim();
  if (!trimmed) return '';
  return MEDIA_ONLY_PLACEHOLDER_TEXT.has(trimmed) ? '' : withoutLegacyImageNotes;
}

function normalizeHistoryBlock(raw: unknown): Record<string, any> | null {
  if (!isRecord(raw)) return null;
  const type = nonEmptyString(raw.type);
  const afterIndex = normalizeBlockAfterIndex(raw.afterIndex);
  if (!type || afterIndex === null) return null;

  if (type === 'file') {
    const filePath = nonEmptyString(raw.filePath);
    if (!filePath) return null;
    const label = nonEmptyString(raw.label) || basenamePortable(filePath);
    const ext = nonEmptyString(raw.ext) || extOfName(label) || extOfName(filePath) || '';
    return { ...raw, type, afterIndex, filePath, label, ext };
  }


  if (type === 'cron_confirm') {
    if (!isRecord(raw.jobData)) return null;
    return {
      ...raw,
      type,
      afterIndex,
      jobData: raw.jobData,
      status: nonEmptyString(raw.status) || 'approved',
    };
  }

  if (type === 'suggestion_card') {
    const kind = nonEmptyString(raw.kind);
    const title = nonEmptyString(raw.title);
    if (!kind || !title) return null;
    return {
      ...raw,
      type,
      afterIndex,
      kind,
      title,
      status: nonEmptyString(raw.status) || 'pending',
    };
  }

  if (type === 'screenshot') {
    if (!nonEmptyString(raw.base64) || !nonEmptyString(raw.mimeType)) return null;
  } else if (type === 'settings_update') {
    if (!isRecord(raw.update)) return null;
  } else if (type === 'skill') {
    if (!nonEmptyString(raw.skillName)) return null;
  } else if (type === 'interlude') {
    if (!nonEmptyString(raw.text)) return null;
  }

  return { ...raw, type, afterIndex };
}

// ── 构建 ──

type InterludeContentBlock = Extract<ContentBlock, { type: 'interlude' }>;

function isInterludeHistoryBlock(block: Record<string, any>): block is InterludeContentBlock & { afterIndex: number; sourceIndex?: number } {
  return block.type === 'interlude';
}

function interludeContentBlock(block: Record<string, any>, fallbackId: string): InterludeContentBlock {
  const { afterIndex: _afterIndex, ...content } = block;
  return {
    ...content,
    type: 'interlude',
    id: nonEmptyString(content.id) || fallbackId,
    variant: nonEmptyString(content.variant) || 'deferred_result',
    text: nonEmptyString(content.text) || '',
  } as InterludeContentBlock;
}

function isMediaInterludeAnchor(block: Record<string, any>, taskId: string): boolean {
  return (
    (block.type === 'media_generation' && block.taskId === taskId) ||
    (block.type === 'file' && block.replacesTaskId === taskId)
  );
}

function shouldPlaceInterludeBeforeMessage(interlude: Record<string, any>, inlineBlocks: Record<string, any>[]): boolean {
  if (interlude.timelinePlacement === 'after_anchor_message') return false;
  const taskId = nonEmptyString(interlude.taskId);
  if (!taskId) return false;
  return inlineBlocks.some((block) => isMediaInterludeAnchor(block, taskId));
}

function normalizeHistoryTimestamp(value: number | string | null | undefined): number | undefined {
  if (typeof value === 'number' && Number.isFinite(value)) return value;
  if (typeof value === 'string') {
    const parsed = Date.parse(value);
    return Number.isNaN(parsed) ? undefined : parsed;
  }
  return undefined;
}

function normalizeSourceIndex(value: unknown): number | null {
  return typeof value === 'number' && Number.isInteger(value) && value >= 0 ? value : null;
}

function sourceOrderedItems(
  items: ChatListItem[],
  data: HistoryApiResponse,
  allBlocks: Array<any>,
): ChatListItem[] {
  if (!items.length) return items;
  const orderById = new Map<string, number>();
  for (let i = 0; i < data.messages.length; i += 1) {
    const sourceIndex = normalizeSourceIndex(data.messages[i].sourceIndex);
    if (sourceIndex === null) continue;
    orderById.set(data.messages[i].id || `hist-${i}`, sourceIndex);
  }
  for (const block of allBlocks) {
    if (!isInterludeHistoryBlock(block)) continue;
    const sourceIndex = normalizeSourceIndex(block.sourceIndex);
    const id = nonEmptyString(block.id);
    if (sourceIndex === null || !id) continue;
    orderById.set(id, sourceIndex);
  }
  if (orderById.size === 0) return items;

  const entries = items.map((item, index) => {
    const id = item.type === 'message' ? item.data.id : item.id;
    return { item, index, sourceIndex: orderById.get(id) ?? null };
  });
  if (entries.some((entry) => entry.sourceIndex === null)) return items;

  return entries
    .sort((a, b) => (
      (a.sourceIndex as number) - (b.sourceIndex as number) ||
      a.index - b.index
    ))
    .map((entry) => entry.item);
}

// ── Run 投影（页内归并与跨页缝合共用核心）──

/** 计算单条记录在 Run 内的到达序偏移：与工具块按真实时间线交错的坐标系。 */
function runRecordStrideOffset(offset: number): number {
  return offset * HISTORY_MESSAGE_ORDER_STRIDE;
}

/**
 * 把一个 Run 的原始记录事实投影为 ChatMessage。
 * 页内归并（buildItemsFromHistory）与跨页缝合（prependItems 后重投影）共用，
 * 保证「任意切页恢复 ≡ 全量恢复」：同 idPrefix、同 stride 坐标系、同裁决入口。
 *
 * - 终态（turnStatus）取组内最后一条记录：中间尝试失败后恢复成功的 Run 不被放大为失败；
 * - runTerminal=false（Run 尾部不在本组，或该 Run 仍在流式进行）时绝不派生任何
 *   Run 终态块/结局——「未生成最终回复」只能来自 Run 尾部的权威裁决。
 */
function projectHistoryRunRecords(
  records: HistoryRunRecordFact[],
  meta: {
    turnInputEntryId?: string;
    turnInputVisible?: boolean;
    firstRecordTimestamp?: number;
  },
  runTerminal: boolean,
): ChatMessage & { blocks: ContentBlock[] } {
  const finalRecord = records[records.length - 1];
  const finalId = finalRecord.displayId;
  const idPrefix = finalRecord.entryId || finalId;

  const legacyBlocks: ContentBlock[] = [];
  for (let offset = 0; offset < records.length; offset += 1) {
    const record = records[offset];
    legacyBlocks.push(...buildAssistantBlocksFromContent({
      content: record.content,
      thinking: record.thinking ?? null,
      toolCalls: (record.toolCalls ?? null) as never,
      extraBlocks: (record.inlineBlocks ?? null) as never,
    }).map((block) => (
      block.processOrder !== undefined
        ? { ...block, processOrder: block.processOrder + runRecordStrideOffset(offset) }
        : block
    )));
  }

  // 兼容分叉：整组没有任何持久化语义分段的旧数据（老服务端/纯工具记录）保持
  // 旧投影路径——数组位置即展示顺序（技能卡跟在正文后等），不进四区投影、
  // 不派生 Run 终态。带 segments 的组（现行服务端对一切有文字/思考内容的
  // 记录都会下发）走统一裁决路径。
  const hasSemanticSegments = records.some((record) => (record.segments || []).length > 0);
  if (!hasSemanticSegments) {
    const blocks = normalizeContentBlocks(legacyBlocks, {
      idPrefix,
      turnLifecycle: 'sealed',
    });
    return {
      id: finalId,
      sourceEntryId: finalRecord.entryId,
      role: 'assistant',
      blocks,
      ...(meta.turnInputEntryId ? { turnInputEntryId: meta.turnInputEntryId } : {}),
      ...(typeof meta.turnInputVisible === 'boolean' ? { turnInputVisible: meta.turnInputVisible } : {}),
      ...(meta.firstRecordTimestamp !== undefined ? { timestamp: meta.firstRecordTimestamp } : {}),
    };
  }

  const segments = records.flatMap((record, offset) => (
    (record.segments || []).map((segment) => (
      segment.processOrder !== undefined
        ? { ...segment, processOrder: segment.processOrder + runRecordStrideOffset(offset) }
        : segment
    ))
  ));
  const projectionResult = projectAssistantTurn({
    idPrefix,
    inputMessageId: meta.turnInputEntryId || null,
    assistantMessageIds: records.map((record) => record.entryId || record.displayId),
    // 迁移边界一次性净化：旧落盘 segments 可能残留 leading 内部标签，
    // 与结构化 mood/thinking block 双重表示时剥离（任务书 §23）。
    segments: sanitizePersistedSegments(segments as never, {
      hasStructuredMood: legacyBlocks.some((block) => block.type === 'mood'),
      hasStructuredThinking: legacyBlocks.some((block) => block.type === 'thinking'),
    }),
    legacyBlocks,
    status: finalRecord.turnStatus === 'failed'
      ? 'failed'
      : finalRecord.turnStatus === 'aborted'
        ? 'aborted'
        : 'completed',
    runTerminal,
  });

  const msg: ChatMessage & { blocks: ContentBlock[] } = {
    id: finalId,
    sourceEntryId: finalRecord.entryId,
    role: 'assistant',
    blocks: projectionResult.blocks,
    turnProjection: projectionResult.projection,
    ...(meta.turnInputEntryId ? { turnInputEntryId: meta.turnInputEntryId } : {}),
    ...(typeof meta.turnInputVisible === 'boolean' ? { turnInputVisible: meta.turnInputVisible } : {}),
    ...(meta.firstRecordTimestamp !== undefined ? { timestamp: meta.firstRecordTimestamp } : {}),
  };
  return msg;
}

/** Run 缝合用：跨页重投影已合并的 facts（头部到达后由调用方决定是否丢弃 facts）。 */
export function projectHistoryRunFromFacts(facts: HistoryRunFacts): ChatMessage {
  const records = [...facts.records].sort((a, b) => Number(a.displayId) - Number(b.displayId));
  const ownsRunTail = records.length > 0
    && Number(records[records.length - 1].displayId) === facts.turnEndIndex;
  return projectHistoryRunRecords(records, {
    turnInputEntryId: facts.turnInputEntryId,
    turnInputVisible: facts.turnInputVisible,
    firstRecordTimestamp: facts.firstRecordTimestamp,
  }, ownsRunTail);
}

function historyRunBounds(message: HistoryApiResponse['messages'][number]): { turnStartIndex: number; turnEndIndex: number } | null {
  if (!Number.isInteger(message.turnStartIndex) || !Number.isInteger(message.turnEndIndex)) return null;
  return { turnStartIndex: message.turnStartIndex as number, turnEndIndex: message.turnEndIndex as number };
}

function sameHistoryRun(
  next: HistoryApiResponse['messages'][number],
  current: HistoryApiResponse['messages'][number],
): boolean {
  const nextBounds = historyRunBounds(next);
  const currentBounds = historyRunBounds(current);
  if (nextBounds && currentBounds) {
    return nextBounds.turnStartIndex === currentBounds.turnStartIndex
      && nextBounds.turnEndIndex === currentBounds.turnEndIndex;
  }
  // 兼容旧服务端（无 Run 边界）：沿用 turnInputEntryId 相等 + 双方带 segments 的旧规则。
  return !!(next.assistantSegments && next.turnInputEntryId
    && current.assistantSegments && next.turnInputEntryId === current.turnInputEntryId);
}

export interface BuildItemsFromHistoryOptions {
  /**
   * 当前会话仍有活跃流（本端 stream buffer 有内容）时为 true：最新 Run 在磁盘上的
   * 记录必然不完整，其页内投影不得携带终态（T06：运行中冷加载不误终结）。
   * Run 真正结束后由实时收口（commitLiveRun）按权威状态落定。
   */
  openTailRun?: boolean;
}

export function buildItemsFromHistory(data: HistoryApiResponse, options: BuildItemsFromHistoryOptions = {}): ChatListItem[] {
  recordChatPerformance('history_projection', { itemCount: data.messages.length });
  const items: ChatListItem[] = [];
  const sessionFileLookup = buildSessionFileLookup(data.sessionFiles);

  // 按 afterIndex 分组统一 blocks
  const allBlocks = normalizeBlocks(data);
  const blockMap: Record<number, Array<any>> = {};
  for (const b of allBlocks) {
    (blockMap[b.afterIndex] ??= []).push(b);
  }

  // 页内最新的 assistant 记录 id（display 序号）：openTailRun 只作用于包含它的组。
  let newestAssistantId: string | null = null;
  for (let i = data.messages.length - 1; i >= 0; i -= 1) {
    if (data.messages[i].role === 'assistant') {
      newestAssistantId = data.messages[i].id || null;
      break;
    }
  }

  for (let i = 0; i < data.messages.length; i++) {
    const m = data.messages[i];
    const id = m.id || `hist-${i}`;
    const timestamp = normalizeHistoryTimestamp(m.timestamp);

    if (m.role === 'user') {
      // 跨 session 协作：非用户本人发出的消息带 origin，此时以 displayText（干净正文，
      // 不含模型侧身份前缀）为准；带知识库引用 / 检索统计的消息同理（content 含
      // [KnowledgeContext] 注入块，displayText 才是用户敲的原文）；老消息没有这些
      // 字段，走既有 content 管道，行为不变。
      const origin = (m as any).origin;
      const originDisplayText = (origin || m.agentReview || m.agentReviewRequest || m.sessionRefs?.length
        || m.knowledgeRefs?.notebookIds?.length || m.knowledgeRetrieval)
        && typeof m.displayText === 'string' ? m.displayText : null;

      // strip steer 前缀（内部标记，不应展示给用户）
      const rawContent = (originDisplayText ?? (m.content || ''))
        .replace(LEGACY_STEER_PREFIX_RE, '')
        .replace(/^<t>[^<]*<\/t>\s*/, '');

      // 过滤系统注入的后台任务通知（steer 消息）与中断标记，不展示给用户
      // （模型可见；中断标记旧版明文前缀兼容开发期已写入的历史）
      if (/<hana-background-result\s/.test(rawContent) || /<hana-deferred-tasks>/.test(rawContent)
        || /<hana-turn-interrupted(?:\s|>)/.test(rawContent)
        || rawContent.startsWith('[Turn interrupted by user]')) {
        continue;
      }

      const { text, skills, files, attachedImages, attachedVideos, attachedAudios, sessionFileRefs, deskContext, quotedText } = parseUserAttachments(rawContent);
      const hasMarkerMedia = attachedImages.length > 0 || attachedVideos.length > 0 || attachedAudios.length > 0;
      const visibleText = normalizeUserVisibleText(text, hasMarkerMedia);
      const consumedSessionFileMarkers = new Set<number>();
      const fileAtts = files.map((f) => {
        const marker = consumeSessionFileMarkerForAttachment(sessionFileRefs, consumedSessionFileMarkers, f);
        return attachmentFromRef({
          path: f.path,
          name: f.name,
          isDirectory: f.isDirectory,
          ...(marker?.fileId ? { fileId: marker.fileId } : {}),
        }, sessionFileLookup, marker ? {
          fileId: marker.fileId,
          name: marker.label,
        } : undefined);
      });
      const imageBlocks = m.images || [];
      const markerImageAtts = attachedImages.map((ref, idx) => {
        const img = imageBlocks[idx];
        return attachmentFromRef(ref, sessionFileLookup, {
          ...(img?.mimeType ? { mimeType: img.mimeType } : {}),
        });
      });
      const imageAtts = imageBlocks.slice(attachedImages.length).map((img, idx) => ({
        path: `image-${idx}`,
        name: `image-${idx}.${(img.mimeType || 'image/png').split('/')[1] || 'png'}`,
        isDir: false,
        ...(img.data ? { base64Data: img.data } : {}),
        mimeType: img.mimeType,
        ...(img.deferred ? { deferred: img.deferred } : {}),
      }));
      const markerVideoAtts = attachedVideos.map((ref) => attachmentFromRef(ref, sessionFileLookup));
      const markerAudioAtts = attachedAudios.map((ref) => attachmentFromRef(ref, sessionFileLookup));
      const allAtts = [...fileAtts, ...markerImageAtts, ...markerVideoAtts, ...markerAudioAtts, ...imageAtts];
      const msg: ChatMessage = {
        id,
        sourceEntryId: m.sourceEntryId || m.entryId,
        clientMessageId: m.clientMessageId,
        role: 'user',
        text: visibleText,
        textHtml: visibleText ? renderMarkdown(visibleText) : undefined,
        attachments: allAtts.length ? allAtts : undefined,
        deskContext: deskContext || undefined,
        quotedText: quotedText || undefined,
        timestamp,
        // 手动技能调用落盘是 [Use skill: x] 前缀，这里解析回胶囊字段，
        // 与 live 提交路径的 message.skills 形状一致（否则重进会话退化为纯文本）。
        ...(skills.length ? { skills } : {}),
        ...(origin ? { origin } : {}),
        ...(m.agentReview ? { agentReview: m.agentReview } : {}),
        ...(m.agentReviewRequest ? { agentReviewRequest: m.agentReviewRequest } : {}),
        ...(m.sessionRefs?.length ? { sessionRefs: m.sessionRefs } : {}),
        ...(m.agentMentions?.length ? { agentMentions: m.agentMentions } : {}),
        ...(m.knowledgeRefs?.notebookIds?.length ? { knowledgeRefs: m.knowledgeRefs } : {}),
        ...(m.knowledgeRetrieval ? { knowledgeRetrieval: m.knowledgeRetrieval } : {}),
      };
      items.push({ type: 'message', data: msg });
    } else if (m.role === 'assistant') {
      let groupEnd = i;
      while (
        groupEnd + 1 < data.messages.length
        && data.messages[groupEnd + 1].role === 'assistant'
        && sameHistoryRun(data.messages[groupEnd + 1], m)
      ) {
        groupEnd += 1;
      }
      const groupMessages = data.messages.slice(i, groupEnd + 1);
      const runBounds = historyRunBounds(m);
      const firstRecordIndex = Number(m.id);
      const lastRecordIndex = Number(groupMessages[groupMessages.length - 1].id);
      // Run 头部是否已在本组内（未被分页截断）；无边界元数据时保守视为完整。
      const coversRunHead = runBounds === null
        || (!Number.isInteger(firstRecordIndex) || firstRecordIndex <= runBounds.turnStartIndex);
      // Run 尾部是否在本组内：尾部持有整轮结局的裁决权。无元数据时保守视为持有
      // （兼容旧服务端的既有行为；新服务端下被截断的头部片段 runTerminal=false）。
      let ownsRunTail = true;
      if (runBounds !== null && Number.isInteger(lastRecordIndex)) {
        ownsRunTail = lastRecordIndex >= runBounds.turnEndIndex;
      }
      // 活跃流的最新 Run：磁盘记录不完整，不派生终态（T06）。
      // 「最新组」以组末条记录判定（runTerminal 由尾部持有）。
      const openTailRun = !!options.openTailRun
        && ownsRunTail
        && newestAssistantId != null
        && newestAssistantId === groupMessages[groupMessages.length - 1].id;
      const beforeInterludes: Array<Record<string, any>> = [];
      const afterInterludes: Array<Record<string, any>> = [];
      const records: HistoryRunRecordFact[] = [];

      for (let offset = 0; offset < groupMessages.length; offset += 1) {
        const assistantMessage = groupMessages[offset];
        const messageBlocks = blockMap[i + offset] || [];
        const inlineBlocks = messageBlocks.filter((block) => !isInterludeHistoryBlock(block));
        const interludeBlocks = messageBlocks.filter(isInterludeHistoryBlock);
        beforeInterludes.push(...interludeBlocks.filter((block) => (
          shouldPlaceInterludeBeforeMessage(block, inlineBlocks)
        )));
        afterInterludes.push(...interludeBlocks.filter((block) => (
          !shouldPlaceInterludeBeforeMessage(block, inlineBlocks)
        )));
        records.push({
          displayId: assistantMessage.id || `hist-${i + offset}`,
          ...(assistantMessage.entryId ? { entryId: assistantMessage.entryId } : {}),
          content: assistantMessage.content,
          thinking: assistantMessage.thinking ?? null,
          toolCalls: (assistantMessage.toolCalls ?? null) as HistoryRunRecordFact['toolCalls'],
          segments: (assistantMessage.assistantSegments || []) as HistoryRunRecordFact['segments'],
          ...(assistantMessage.turnStatus && assistantMessage.turnStatus !== 'completed'
            ? { turnStatus: assistantMessage.turnStatus }
            : {}),
          inlineBlocks,
        });
      }

      const projectionMeta = {
        turnInputEntryId: m.turnInputEntryId,
        turnInputVisible: m.turnInputVisible,
        firstRecordTimestamp: timestamp,
      };
      const runTerminal = ownsRunTail && !openTailRun;
      const msg = projectHistoryRunRecords(records, projectionMeta, runTerminal);
      const blocks = msg.blocks;

      // Run 被分页截断（头部未加载，或防御性场景尾部缺失）时携带缝合事实；
      // 更早页片段到达后由 chat-slice 按 runKey 缝合并重投影，头部加载完成后丢弃。
      if (runBounds !== null && (!coversRunHead || !ownsRunTail)) {
        msg.runFacts = {
          runKey: `${runBounds.turnStartIndex}:${runBounds.turnEndIndex}`,
          turnStartIndex: runBounds.turnStartIndex,
          turnEndIndex: runBounds.turnEndIndex,
          ...(m.turnInputEntryId ? { turnInputEntryId: m.turnInputEntryId } : {}),
          ...(typeof m.turnInputVisible === 'boolean' ? { turnInputVisible: m.turnInputVisible } : {}),
          ...(timestamp !== undefined ? { firstRecordTimestamp: timestamp } : {}),
          records,
        };
      }

      for (let j = 0; j < beforeInterludes.length; j += 1) {
        const data = interludeContentBlock(beforeInterludes[j], `interlude:${i}:before:${j}`);
        items.push({
          type: 'interlude',
          id: data.id,
          data,
        });
      }

      if (blocks.length > 0) {
        items.push({ type: 'message', data: msg });
      }

      for (let j = 0; j < afterInterludes.length; j += 1) {
        const data = interludeContentBlock(afterInterludes[j], `interlude:${i}:after:${j}`);
        items.push({
          type: 'interlude',
          id: data.id,
          data,
        });
      }
      i = groupEnd;
    }
  }

  return sourceOrderedItems(items, data, allBlocks);
}
