/**
 * StreamBufferManager — per-session 流式事件节流缓冲
 *
 * WS 事件到达时写入 buffer（纯 JS 对象，不触发 React），
 * 普通增量按画面合并并受最高发布频率约束，语义边界立即发布。
 *
 * 设计为 singleton，不依赖 React 组件生命周期。
 * app-ws-shim 直接调用 streamBufferManager.handle(msg)。
 */

import type { ChatMessage, ContentBlock } from '../stores/chat-types';
import { useStore } from '../stores';
import { sessionScopedKey, sessionScopedValue } from '../stores/session-slice';
import { cleanMoodText } from '../utils/message-parser';
import { findOpenToolIndex, toolCallFromStartEvent, toolCallIdFromEvent } from '../utils/tool-call-identity';
import {
  registerStreamBufferInvalidator,
  registerStreamBufferSnapshot,
  type StreamBufferSnapshot,
} from '../stores/stream-invalidator';
import { bumpMessageLiveVersion } from '../stores/message-live-version';
import {
  clearLiveAssistantMessage,
  publishLiveAssistantMessage,
  type LiveAssistantSegment,
  type LiveAssistantSegmentPhase,
} from '../stores/live-turn-store';
import { recordChatPerformance } from '../utils/chat-performance';
import {
  normalizeContentBlocks,
} from '../utils/content-semantics';
import { projectAssistantTurn } from '../utils/turn-projector';
import { KNOWLEDGE_RESEARCH_TOOL_NAMES } from '../utils/tool-label';

/* eslint-disable @typescript-eslint/no-explicit-any -- 流式消息 handle(msg) 接收动态 JSON */

const STREAM_FLUSH_FPS = 30;
const FLUSH_INTERVAL = Math.round(1000 / STREAM_FLUSH_FPS);
let streamMessageSeq = 0;
type InterludeContentBlock = Extract<ContentBlock, { type: 'interlude' }>;

function nextStreamMessageId(): string {
  streamMessageSeq = (streamMessageSeq + 1) % Number.MAX_SAFE_INTEGER;
  return `stream-${Date.now()}-${streamMessageSeq}`;
}

interface Buffer {
  sessionPath: string;
  blocks: ContentBlock[];
  segmentsById: Map<string, LiveAssistantSegment>;
  segmentOrder: string[];
  /** 当前用户轮次的完整可见正文，供断线快照使用。 */
  textAcc: string;
  /** 当前工具/富内容边界之后的正文段，只更新它自己对应的 text block。 */
  textSegmentAcc: string;
  textSegmentOrdinal: number | null;
  thinkingAcc: string;
  moodAcc: string;
  moodYuan: string;
  /** 已有已封存 mood 段时，下一段首个非空 mood_text 到达前暂挂的分隔符（\n\n）。
   *  只有新段真正产生可见内容才落地，避免空段制造多余空白。 */
  moodPendingSeparator: boolean;
  inThinking: boolean;
  hasThinkingBlock: boolean;
  inMood: boolean;
  inCard: boolean;
  cardAttrs: { type: string; plugin: string; route: string; title?: string } | null;
  cardDescAcc: string;
  lastFlushTime: number;
  flushTimer: ReturnType<typeof setTimeout> | null;
  flushFrame: number | null;
  /** 自上次发布后，缓冲区里是否出现了新的可见变化。 */
  publishPending: boolean;
  /** 当前 Assistant Run 绑定的 assistant message id */
  messageId: string | null;
  /** assistant_run_end/中止收口时为 true，确保所有仍在流式的内容统一封口。 */
  runEnding: boolean;
  /** 是否有活动 Assistant Run。只能由 assistant_run_start 开启、assistant_run_end
   *  （或管理性 finishRun/clear）关闭；status（Session Busy）与 Pi Model Turn（
   *  model_turn_start/model_turn_end）都不得改变它。 */
  runActive: boolean;
  /** 活动 Run 的身份（优先 runId，缺失时 streamId）；null 表示身份未知的隐式 Run。 */
  activeRunKey: string | null;
  /** 最近一次已完成 finalization 的 Run 身份，用于重复 assistant_run_end 的 exactly-once 去重。 */
  lastFinalizedRunKey: string | null;
  /** 本 Run 内已出现 canonical assistant_segment_*：legacy text/thinking 兼容事件
   *  不得再产生 UI block（只能累积断线恢复快照）。canonicalLocked 是 Run 级，跨 Model Turn 保留。 */
  canonicalLocked: boolean;
  /** 过程区到达序号计数器：思考段/工具组/卡片首次物化时各取一个单调递增的戳，
   *  投影层据此把两条"车道"交错回真实时间线。 */
  nextProcessOrder: number;
  /** 防重放叠加：每个 canonical 段已应用的最大事件 seq；≤ 它的 delta 直接丢弃。
   *  resume 增量重放在流元数据失配时会把已应用事件原样重发，没有这层防御
   *  会把同一段正文拼接两遍（尾重叠）或让同一工具调用生成第二张卡。 */
  canonicalAppliedSeqBySegment: Map<string, number>;
  /** 防重放叠加：本 Run 内已应用的 tool_start 事件 seq 集合。 */
  appliedToolStartSeqs: Set<number>;
}

function createBuffer(sessionPath: string): Buffer {
  return {
    sessionPath,
    blocks: [],
    segmentsById: new Map(),
    segmentOrder: [],
    textAcc: '',
    textSegmentAcc: '',
    textSegmentOrdinal: null,
    thinkingAcc: '',
    moodAcc: '',
    moodYuan: 'lingxi',
    moodPendingSeparator: false,
    inThinking: false,
    hasThinkingBlock: false,
    inMood: false,
    inCard: false,
    cardAttrs: null,
    cardDescAcc: '',
    lastFlushTime: 0,
    flushTimer: null,
    flushFrame: null,
    publishPending: false,
    messageId: null,
    runEnding: false,
    runActive: false,
    activeRunKey: null,
    lastFinalizedRunKey: null,
    canonicalLocked: false,
    nextProcessOrder: 0,
    canonicalAppliedSeqBySegment: new Map(),
    appliedToolStartSeqs: new Set(),
  };
}

function renderBufferedBlocks(currentBlocks: ContentBlock[], buf: Buffer): ContentBlock[] {
  const blocks = [...currentBlocks];

  if (buf.thinkingAcc || buf.hasThinkingBlock || buf.inThinking) {
    const idx = blocks.findIndex(b => b.type === 'thinking');
    const existing = idx >= 0 ? blocks[idx] : null;
    const thinkingBlock: ContentBlock = {
      type: 'thinking',
      content: buf.thinkingAcc,
      sealed: !buf.inThinking,
      processOrder: existing?.processOrder ?? buf.nextProcessOrder++,
    };
    if (idx >= 0) blocks[idx] = thinkingBlock;
    else blocks.unshift(thinkingBlock);
  }

  if (buf.moodAcc || buf.inMood) {
    const idx = blocks.findIndex(b => b.type === 'mood');
    const existing = idx >= 0 ? blocks[idx] : null;
    const moodBlock: ContentBlock = {
      type: 'mood',
      yuan: buf.moodYuan,
      text: buf.inMood ? buf.moodAcc : cleanMoodText(buf.moodAcc),
      processOrder: existing?.processOrder ?? buf.nextProcessOrder++,
    };
    if (idx >= 0) blocks[idx] = moodBlock;
    else {
      const insertAt = blocks.findIndex(b => b.type !== 'thinking');
      blocks.splice(insertAt >= 0 ? insertAt : blocks.length, 0, moodBlock);
    }
  }

  if (buf.textSegmentAcc) {
    const displayText = buf.textSegmentAcc.replace(/<tool_code>[\s\S]*?<\/tool_code>\s*/g, '');
    const textIndexes = blocks.reduce<number[]>((indexes, block, index) => {
      if (block.type === 'text') indexes.push(index);
      return indexes;
    }, []);
    const idx = buf.textSegmentOrdinal === null ? undefined : textIndexes[buf.textSegmentOrdinal];
    const textBlock: ContentBlock = { type: 'text', source: displayText };
    if (idx !== undefined) {
      blocks[idx] = textBlock;
    } else {
      buf.textSegmentOrdinal = textIndexes.length;
      blocks.push(textBlock);
    }
  }

  return blocks;
}

function normalizeSessionId(value: unknown): string | null {
  return typeof value === 'string' && value.trim() ? value.trim() : null;
}

/** Run 身份：优先 runId，缺失时退回 streamId；都没有则身份未知（null）。 */
function runKeyFrom(identity: { runId?: unknown; streamId?: unknown } | null | undefined): string | null {
  return normalizeSessionId(identity?.runId) || normalizeSessionId(identity?.streamId) || null;
}

/** Pi Model Turn 身份（仅 diagnostics 用）：优先 streamId，缺失时退回 turnId。 */
function turnKeyFrom(identity: { streamId?: unknown; turnId?: unknown } | null | undefined): string | null {
  return normalizeSessionId(identity?.streamId) || normalizeSessionId(identity?.turnId) || null;
}

function normalizeLiveSegmentPhase(value: unknown): LiveAssistantSegmentPhase {
  return value === 'reasoning'
    || value === 'commentary'
    || value === 'final_answer'
    || value === 'unresolved'
    ? value
    : 'unresolved';
}

function bufferKeyForSession(sessionPath: string, sessionId: string | null = null): string {
  const explicitSessionId = normalizeSessionId(sessionId);
  if (explicitSessionId) return explicitSessionId;
  const state = useStore.getState();
  return sessionScopedKey(state, sessionPath) || sessionPath;
}

function resolveSessionYuan(sessionPath: string): string {
  const state = useStore.getState();
  const sessionAgentId = state.sessions.find((session: any) => session.path === sessionPath)?.agentId ?? null;
  if (!sessionAgentId) return 'lingxi';
  return state.agents.find((agent: any) => agent.id === sessionAgentId)?.yuan || 'lingxi';
}

class StreamBufferManager {
  private buffers = new Map<string, Buffer>();
  private bufferKeysByPath = new Map<string, string>();

  private adoptBufferKey(fromKey: string, toKey: string, buf: Buffer): void {
    if (fromKey === toKey) return;
    this.buffers.delete(fromKey);
    this.buffers.set(toKey, buf);
    for (const [pathKey, bufferKey] of this.bufferKeysByPath) {
      if (bufferKey === fromKey) this.bufferKeysByPath.set(pathKey, toKey);
    }
  }

  private deleteBufferKey(key: string): void {
    this.buffers.delete(key);
    for (const [pathKey, bufferKey] of [...this.bufferKeysByPath]) {
      if (bufferKey === key) this.bufferKeysByPath.delete(pathKey);
    }
  }

  private lookupBuffer(sessionPath: string, sessionId: string | null = null): Buffer | null {
    const key = bufferKeyForSession(sessionPath, sessionId);
    let buf = this.buffers.get(key) || null;
    if (buf) return buf;

    const aliasKey = this.bufferKeysByPath.get(sessionPath) || null;
    if (aliasKey) {
      buf = this.buffers.get(aliasKey) || null;
      if (buf) {
        this.adoptBufferKey(aliasKey, key, buf);
        this.bufferKeysByPath.set(sessionPath, key);
        return buf;
      }
    }

    if (key !== sessionPath) {
      buf = this.buffers.get(sessionPath) || null;
      if (buf) {
        this.adoptBufferKey(sessionPath, key, buf);
        this.bufferKeysByPath.set(sessionPath, key);
        return buf;
      }
    }

    return null;
  }

  /** 获取或创建 session buffer */
  private getBuffer(sessionPath: string, sessionId: string | null = null): Buffer {
    const key = bufferKeyForSession(sessionPath, sessionId);
    let buf = this.lookupBuffer(sessionPath, sessionId);
    if (!buf) {
      buf = createBuffer(sessionPath);
      this.buffers.set(key, buf);
    }
    buf.sessionPath = sessionPath;
    this.bufferKeysByPath.set(sessionPath, key);
    return buf;
  }

  private hasRunState(buf: Buffer): boolean {
    return !!(
      buf.messageId ||
      buf.blocks.length > 0 ||
      buf.segmentOrder.length > 0 ||
      buf.textAcc ||
      buf.thinkingAcc ||
      buf.hasThinkingBlock ||
      buf.moodAcc ||
      buf.inThinking ||
      buf.inMood ||
      buf.inCard ||
      buf.cardAttrs ||
      buf.cardDescAcc
    );
  }

  private resetRunState(buf: Buffer): void {
    this.cancelScheduledFlush(buf);
    buf.textAcc = '';
    buf.blocks = [];
    buf.segmentsById.clear();
    buf.canonicalAppliedSeqBySegment.clear();
    buf.appliedToolStartSeqs.clear();
    buf.segmentOrder = [];
    buf.textSegmentAcc = '';
    buf.textSegmentOrdinal = null;
    buf.thinkingAcc = '';
    buf.hasThinkingBlock = false;
    buf.moodAcc = '';
    buf.moodPendingSeparator = false;
    buf.inThinking = false;
    buf.inMood = false;
    buf.inCard = false;
    buf.cardAttrs = null;
    buf.cardDescAcc = '';
    buf.messageId = null;
    buf.runEnding = false;
    buf.publishPending = false;
    buf.runActive = false;
    buf.activeRunKey = null;
    buf.canonicalLocked = false;
    buf.nextProcessOrder = 0;
    // lastFinalizedRunKey 刻意保留：跨 reset 支撑重复 assistant_run_end 的 exactly-once。
  }

  private finalizeRun(buf: Buffer, persistedEntries: {
    turnInputEntryId?: string | null;
    userEntryId?: string | null;
    assistantEntryId?: string | null;
    assistantEntryIds?: string[];
    status?: 'completed' | 'failed' | 'aborted';
  } = {}): void {
    if (this.hasRunState(buf)) {
      buf.runEnding = true;
      this.flush(buf);
      this.commitLiveRun(buf, persistedEntries);
    } else {
      this.cancelScheduledFlush(buf);
    }
    this.resetRunState(buf);
  }

  /** 确保 store 中已存在当前 Run 绑定的 assistant message */
  private ensureMessage(buf: Buffer): void {
    const store = useStore.getState();
    const session = sessionScopedValue(store, store.chatSessions, buf.sessionPath);
    if (!session) return; // session 未初始化（loadMessages 尚未完成）

    const targetId = buf.messageId;
    const existing = targetId
      ? session.items.find((item) =>
        item.type === 'message' &&
        item.data.id === targetId &&
        item.data.role === 'assistant',
      )
      : null;
    if (existing) {
      buf.messageId = targetId;
      return;
    }

    const id = targetId || nextStreamMessageId();
    const msg: ChatMessage = { id, role: 'assistant', blocks: [], timestamp: Date.now() };
    store.appendItem(buf.sessionPath, { type: 'message', data: msg });
    bumpMessageLiveVersion(buf.sessionPath);
    buf.messageId = id;
  }

  private updateTargetMessage(buf: Buffer, updater: (msg: ChatMessage) => ChatMessage): boolean {
    this.ensureMessage(buf);
    if (!buf.messageId) return false;
    const store = useStore.getState();
    const session = sessionScopedValue(store, store.chatSessions, buf.sessionPath);
    const item = session?.items.find((entry) => (
      entry.type === 'message'
      && entry.data.role === 'assistant'
      && entry.data.id === buf.messageId
    ));
    if (!item || item.type !== 'message') {
      console.warn('[stream] target assistant message missing after ensureMessage:', buf.sessionPath, buf.messageId);
      return false;
    }
    const next = updater({ ...item.data, blocks: buf.blocks });
    const blocks = normalizeContentBlocks(next.blocks || [], {
      idPrefix: item.data.sourceEntryId || item.data.id,
      turnLifecycle: buf.runEnding ? 'sealed' : 'streaming',
    });
    buf.blocks = blocks;
    this.publishLiveRun(buf);
    bumpMessageLiveVersion(buf.sessionPath);
    return true;
  }

  private publishLiveRun(buf: Buffer): void {
    if (!buf.messageId) return;
    const store = useStore.getState();
    const session = sessionScopedValue(store, store.chatSessions, buf.sessionPath);
    const item = session?.items.find((entry) => (
      entry.type === 'message'
      && entry.data.role === 'assistant'
      && entry.data.id === buf.messageId
    ));
    const message = item?.type === 'message' ? item.data : null;
    const idPrefix = message?.sourceEntryId || message?.id || buf.messageId;
    const segments = buf.segmentOrder
      .map((segmentId) => buf.segmentsById.get(segmentId))
      .filter((segment): segment is LiveAssistantSegment => !!segment);
    const projected = projectAssistantTurn({
      idPrefix,
      inputMessageId: message?.turnInputEntryId || null,
      assistantMessageIds: [message?.sourceEntryId || message?.id || buf.messageId],
      segments,
      legacyBlocks: buf.blocks,
      status: 'streaming',
    });
    publishLiveAssistantMessage(buf.sessionPath, buf.messageId, projected.blocks, {
      segmentsById: Object.fromEntries(buf.segmentsById),
      segmentOrder: [...buf.segmentOrder],
      status: buf.runEnding ? 'sealed' : 'streaming',
      turnProjection: projected.projection,
    });
  }

  private updateCanonicalSegment(buf: Buffer, msg: any): void {
    this.ensureMessage(buf);
    if (!buf.messageId || typeof msg.segmentId !== 'string' || !msg.segmentId) return;
    const existing = buf.segmentsById.get(msg.segmentId);
    // 首次见到该 segment 时盖到达序号；delta/end 只更新内容，不抢新戳。
    const processOrder = existing?.processOrder ?? buf.nextProcessOrder++;
    if (msg.type === 'assistant_segment_start') {
      const semanticPhase = normalizeLiveSegmentPhase(msg.semanticPhase);
      const segment: LiveAssistantSegment = {
        id: msg.segmentId,
        kind: msg.kind === 'reasoning' ? 'reasoning' : 'text',
        semanticPhase,
        source: existing?.source || '',
        lifecycle: 'streaming',
        processOrder,
      };
      if (!existing) buf.segmentOrder.push(msg.segmentId);
      buf.segmentsById.set(msg.segmentId, segment);
    } else if (msg.type === 'assistant_segment_delta') {
      // 幂等防御：resume 增量重放会原样重发 delta，≤ 已应用 seq 的重复禁止再拼接，
      // 否则同一段正文出现尾部重叠重复。无 seq 的旧协议保持原行为。
      const incomingSeq = Number.isFinite(msg.seq) ? Math.max(0, Math.floor(Number(msg.seq))) : null;
      if (incomingSeq !== null) {
        const appliedSeq = buf.canonicalAppliedSeqBySegment.get(msg.segmentId);
        if (appliedSeq !== undefined && incomingSeq <= appliedSeq) return;
        buf.canonicalAppliedSeqBySegment.set(msg.segmentId, incomingSeq);
      }
      const semanticPhase = normalizeLiveSegmentPhase(msg.semanticPhase || existing?.semanticPhase);
      const segment: LiveAssistantSegment = existing || {
        id: msg.segmentId,
        kind: semanticPhase === 'reasoning' ? 'reasoning' : 'text',
        semanticPhase,
        source: '',
        lifecycle: 'streaming',
        processOrder,
      };
      if (!existing) buf.segmentOrder.push(msg.segmentId);
      buf.segmentsById.set(msg.segmentId, {
        ...segment,
        semanticPhase,
        source: `${segment.source}${typeof msg.delta === 'string' ? msg.delta : ''}`,
      });
    } else if (msg.type === 'assistant_segment_end') {
      const semanticPhase = normalizeLiveSegmentPhase(msg.semanticPhase || existing?.semanticPhase);
      const segment: LiveAssistantSegment = existing || {
        id: msg.segmentId,
        kind: semanticPhase === 'reasoning' ? 'reasoning' : 'text',
        semanticPhase,
        source: '',
        lifecycle: 'streaming',
        processOrder,
      };
      if (!existing) buf.segmentOrder.push(msg.segmentId);
      buf.segmentsById.set(msg.segmentId, {
        ...segment,
        semanticPhase,
        lifecycle: 'sealed',
      });
    }
    if (msg.type === 'assistant_segment_delta') this.scheduleFlush(buf);
    else this.publishBoundary(buf);
  }

  private commitLiveRun(buf: Buffer, persistedEntries: {
    turnInputEntryId?: string | null;
    userEntryId?: string | null;
    assistantEntryId?: string | null;
    assistantEntryIds?: string[];
    status?: 'completed' | 'failed' | 'aborted';
  }): void {
    if (!buf.messageId) return;
    const store = useStore.getState();
    const session = sessionScopedValue(store, store.chatSessions, buf.sessionPath);
    const item = session?.items.find((entry) => (
      entry.type === 'message'
      && entry.data.role === 'assistant'
      && entry.data.id === buf.messageId
    ));
    const message = item?.type === 'message' ? item.data : null;
    const assistantId = persistedEntries.assistantEntryId || message?.sourceEntryId || buf.messageId;
    const segments = buf.segmentOrder
      .map((segmentId) => buf.segmentsById.get(segmentId))
      .filter((segment): segment is LiveAssistantSegment => !!segment);
    // block.id 从创建那一刻起永久不变（任务书 §二十一/§二十二）：persisted assistant
    // entry 绑定只增加 sourceEntryId / assistantEntryIds，绝不改写 block.id。
    const legacyBlocks = buf.blocks;
    const projected = projectAssistantTurn({
      // idPrefix 用稳定 Run 身份（buf.messageId，跨流式 → 提交不变），而不是持久化 assistant
      // entry id：保证 block.id / segment.id 从创建起永久不变（任务书 §二十一/§二十二）。
      idPrefix: buf.messageId,
      inputMessageId: persistedEntries.turnInputEntryId || message?.turnInputEntryId || null,
      assistantMessageIds: persistedEntries.assistantEntryIds?.length
        ? persistedEntries.assistantEntryIds
        : [assistantId],
      segments,
      legacyBlocks,
      status: persistedEntries.status || 'completed',
    });
    for (const diagnostic of projected.diagnostics) {
      console.warn('[stream] unresolved assistant segment finalized with fallback:', diagnostic.segmentId);
    }
    buf.blocks = projected.blocks;
    const committed = useStore.getState().bindPersistedTurnEntries(buf.sessionPath, {
      ...persistedEntries,
      assistantMessageId: buf.messageId,
      assistantBlocks: projected.blocks,
      assistantProjection: projected.projection,
    });
    clearLiveAssistantMessage(buf.sessionPath, buf.messageId);
    if (!committed) {
      console.warn('[stream] failed to commit live assistant run:', buf.sessionPath, buf.messageId);
    }
  }

  private appendInterlude(buf: Buffer, block: InterludeContentBlock): boolean {
    const consumed = useStore.getState().appendInterludeItem(buf.sessionPath, block);
    if (consumed) bumpMessageLiveVersion(buf.sessionPath);
    return consumed;
  }

  private cancelScheduledFlush(buf: Buffer): void {
    if (buf.flushTimer !== null) {
      clearTimeout(buf.flushTimer);
      buf.flushTimer = null;
    }
    if (buf.flushFrame !== null) {
      if (typeof globalThis.cancelAnimationFrame === 'function') {
        globalThis.cancelAnimationFrame(buf.flushFrame);
      }
      buf.flushFrame = null;
    }
  }

  private requestFlushFrame(buf: Buffer): void {
    if (buf.flushFrame !== null) return;
    if (typeof globalThis.requestAnimationFrame !== 'function') {
      this.flush(buf);
      return;
    }
    buf.flushFrame = globalThis.requestAnimationFrame(() => {
      buf.flushFrame = null;
      this.flush(buf);
    });
  }

  /** 普通增量先合并到下一画面，再受最高发布频率约束。 */
  private scheduleFlush(buf: Buffer): void {
    buf.publishPending = true;
    if (buf.flushTimer !== null || buf.flushFrame !== null) return;
    const now = Date.now();
    if (now - buf.lastFlushTime >= FLUSH_INTERVAL) {
      this.requestFlushFrame(buf);
    } else {
      buf.flushTimer = setTimeout(() => {
        buf.flushTimer = null;
        this.requestFlushFrame(buf);
      }, FLUSH_INTERVAL - (now - buf.lastFlushTime));
    }
  }

  private publishBufferedState(
    buf: Buffer,
    updater: (msg: ChatMessage) => ChatMessage,
    force: boolean,
  ): void {
    if (!force && !buf.publishPending) {
      this.cancelScheduledFlush(buf);
      return;
    }
    this.cancelScheduledFlush(buf);
    const published = this.updateTargetMessage(buf, (msg) => updater({
      ...msg,
      blocks: renderBufferedBlocks(msg.blocks || [], buf),
    }));
    if (!published) return;

    recordChatPerformance('stream_flush', {
      sessionPath: buf.sessionPath,
      messageId: buf.messageId || undefined,
      sourceLength: buf.textSegmentAcc.length,
      blockCount: buf.blocks.length,
    });
    buf.lastFlushTime = Date.now();
    buf.publishPending = false;
  }

  /** 把普通增量一次性发布到当前回合。 */
  private flush(buf: Buffer): void {
    this.publishBufferedState(buf, (msg) => msg, false);
  }

  /** 语义边界立即发布，并把尚未发布的普通增量合并进同一次更新。 */
  private publishBoundary(
    buf: Buffer,
    updater: (msg: ChatMessage) => ChatMessage = (msg) => msg,
  ): void {
    this.publishBufferedState(buf, updater, true);
  }

  // ── 公开事件处理器 ──

  isRunActive(sessionPath: string): boolean {
    return this.lookupBuffer(sessionPath)?.runActive === true;
  }

  /** 研究卡可以跨过并行旧回答的结束点，始终按原身份更新，不增建消息或普通工具。 */
  updateKnowledgeResearchToolProgress(sessionPath: string, id: string, args: Record<string, unknown>, resultNote?: string,
    status?: 'running' | 'succeeded' | 'failed' | 'unknown'): void {
    const buf = this.lookupBuffer(sessionPath);
    const matches = (blocks: readonly ContentBlock[]) => blocks.some(block => block.type === 'tool_group'
      && block.tools.some(tool => tool.id === id && KNOWLEDGE_RESEARCH_TOOL_NAMES.has(tool.name)));
    const update = (message: ChatMessage): ChatMessage => ({
      ...message,
      blocks: (message.blocks || []).map(block => {
        if (block.type !== 'tool_group' || !block.tools.some(tool => tool.id === id && KNOWLEDGE_RESEARCH_TOOL_NAMES.has(tool.name))) return block;
        const tools = block.tools.map(tool => tool.id !== id || !KNOWLEDGE_RESEARCH_TOOL_NAMES.has(tool.name) ? tool : {
          ...tool, args, ...(resultNote !== undefined ? { resultNote } : {}),
          ...(status ? { status, done: status !== 'running', success: status === 'succeeded' } : {}),
        });
        return { ...block, tools, ...(status ? { collapsed: tools.length > 1 && tools.every(tool => tool.done) } : {}) };
      }),
    });
    if (buf?.messageId && matches(buf.blocks)) {
      this.publishBoundary(buf, update);
      return;
    }
    // 旧主回答已经提交时，原卡在会话消息中；不可对空缓冲发送结束事件。
    const store = useStore.getState();
    const session = sessionScopedValue(store, store.chatSessions, sessionPath);
    const item = session?.items.find(item => item.type === 'message' && item.data.role === 'assistant' && matches(item.data.blocks || []));
    if (item?.type === 'message' && store.updateMessageById(sessionPath, item.data.id, update)) bumpMessageLiveVersion(sessionPath);
  }

  handle(msg: any): void {
    const sessionPath = msg.sessionPath;
    if (!sessionPath) {
      console.warn('[ws] stream event missing sessionPath:', msg.type);
      return;
    }
    const sessionId = normalizeSessionId(msg.sessionId);
    const buf = this.getBuffer(sessionPath, sessionId);

    switch (msg.type) {
      case 'assistant_run_start':
        // Assistant Run 开始（任务书 §十七）：唯一能 beginRun 的事件。
        this.beginRun(sessionPath, sessionId, { runId: msg.runId, streamId: msg.streamId });
        break;

      case 'model_turn_start':
      case 'turn_start':
        // Pi Model Turn 开始：仅 diagnostics，绝不得 beginRun / finishRun / reset blocks。
        break;

      case 'assistant_segment_start':
      case 'assistant_segment_delta':
      case 'assistant_segment_end': {
        // canonical 锁定：本轮此后 legacy text/thinking 兼容事件不再产生 UI block。
        buf.canonicalLocked = true;
        // 诊断：Run 已终结、且没有新 assistant_run_start 的情况下又收到 canonical 事件
        if (!buf.runActive && buf.lastFinalizedRunKey) {
          console.warn('[stream] canonical_after_terminal: segment event after run finalized:', {
            type: msg.type,
            segmentId: msg.segmentId,
            lastFinalizedRunKey: buf.lastFinalizedRunKey,
            sessionPath,
          });
        }
        // 诊断：delta/end 落在从未 start 过的 segment 上，说明中间丢了事件
        if (msg.type !== 'assistant_segment_start'
          && typeof msg.segmentId === 'string'
          && !buf.segmentsById.has(msg.segmentId)) {
          console.warn('[stream] canonical_segment_gap: segment event without start:', {
            type: msg.type,
            segmentId: msg.segmentId,
            sessionPath,
          });
        }
        this.updateCanonicalSegment(buf, msg);
        break;
      }

      case 'text_delta':
        // 断线恢复快照始终累积完整正文。
        buf.textAcc += msg.delta || '';
        // canonical 模式：正文唯一真相源是 assistant_segment_*，legacy text_delta
        // 不得再创建 text block / segment（只允许累积快照与诊断）。
        if (buf.canonicalLocked) break;
        this.ensureMessage(buf);
        buf.textSegmentAcc += msg.delta || '';
        this.scheduleFlush(buf);
        break;

      case 'thinking_start':
        // canonical 模式：思考唯一真相源是 reasoning segment，legacy thinking_*
        // 不再产生第二个 thinking block（thinkingAcc 仍累积供快照）。
        if (buf.canonicalLocked) break;
        this.ensureMessage(buf);
        buf.inThinking = true;
        buf.hasThinkingBlock = true;
        buf.thinkingAcc = '';
        this.publishBoundary(buf);
        break;

      case 'thinking_delta':
        buf.hasThinkingBlock = true;
        buf.thinkingAcc += msg.delta || '';
        // 与 text/mood 共用时间节流，避免思考流只能在结束后显示。
        if (!buf.canonicalLocked) this.scheduleFlush(buf);
        break;

      case 'thinking_end':
        buf.hasThinkingBlock = true;
        if (buf.canonicalLocked) break;
        buf.inThinking = false;
        this.publishBoundary(buf);
        break;

      case 'mood_start':
        this.ensureMessage(buf);
        buf.inMood = true;
        // 同一 buffer 内若已有已封存的 mood 段（例如一个 Assistant Run 内多段模型生成
        // 聚合到同一条消息），新的 mood 段不能清掉前段：暂挂分隔符，等首个非空
        // mood_text 到达再落地；首段则从空开始。
        if (buf.moodAcc.trim().length > 0) {
          buf.moodPendingSeparator = true;
        } else {
          buf.moodAcc = '';
          buf.moodPendingSeparator = false;
        }
        buf.moodYuan = resolveSessionYuan(sessionPath);
        this.publishBoundary(buf);
        break;

      case 'mood_text':
        // pending separator 只在收到非空内容时落地，空段不制造多余空白
        if (buf.moodPendingSeparator && (msg.delta || '').trim()) {
          buf.moodAcc += '\n\n';
          buf.moodPendingSeparator = false;
        }
        buf.moodAcc += msg.delta || '';
        this.scheduleFlush(buf);
        break;

      case 'mood_end':
        buf.inMood = false;
        this.publishBoundary(buf);
        break;

      case 'card_start':
        this.ensureMessage(buf);
        buf.inCard = true;
        buf.cardAttrs = msg.attrs || null;
        buf.cardDescAcc = '';
        break;

      case 'card_text':
        buf.cardDescAcc += msg.delta || '';
        break;

      case 'card_end': {
        buf.inCard = false;
        if (buf.cardAttrs) {
          const card = {
            type: buf.cardAttrs.type || 'iframe',
            pluginId: buf.cardAttrs.plugin || '',
            route: buf.cardAttrs.route || '',
            title: buf.cardAttrs.title,
            description: buf.cardDescAcc,
          };
          this.publishBoundary(buf, (m) => ({
            ...m,
            blocks: [...(m.blocks || []), { type: 'plugin_card' as const, card }],
          }));
        }
        buf.cardAttrs = null;
        buf.cardDescAcc = '';
        break;
      }

      case 'tool_start': {
        // 幂等防御（第一层，按事件 seq）：resume 增量重放会原样重发 tool_start，
        // 同一 seq 只允许成卡一次；无 seq 的旧协议保持原行为。
        const startSeq = Number.isFinite(msg.seq) ? Math.max(0, Math.floor(Number(msg.seq))) : null;
        if (startSeq !== null) {
          if (buf.appliedToolStartSeqs.has(startSeq)) break;
          buf.appliedToolStartSeqs.add(startSeq);
        }
        this.ensureMessage(buf);
        this.publishBoundary(buf, (m) => {
          const blocks = [...(m.blocks || [])];
          // 幂等防御（第二层，按工具身份）：即使 seq 缺失，同一 toolCallId 也
          // 绝不生成第二张卡。tool_end 侧的 findOpenToolIndex 已天然幂等。
          const callId = typeof msg.id === 'string' ? msg.id : '';
          if (callId && blocks.some((block) => (
            block.type === 'tool_group' && block.tools.some((tool) => tool.id === callId)
          ))) {
            return m;
          }
          // 找最后一个 tool_group 或创建新的
          let lastTg = blocks.length - 1;
          while (lastTg >= 0 && blocks[lastTg].type !== 'tool_group') lastTg--;
          if (lastTg >= 0 && blocks[lastTg].type === 'tool_group') {
            const tg = blocks[lastTg] as Extract<ContentBlock, { type: 'tool_group' }>;
            const isResearchCard = KNOWLEDGE_RESEARCH_TOOL_NAMES.has(msg.name);
            // 只合并连续到达的同类在途工具，不能跨过思考段，也不能混入研究状态卡。
            if (lastTg === blocks.length - 1
              && tg.processOrder === buf.nextProcessOrder - 1
              && tg.tools.every(tool => KNOWLEDGE_RESEARCH_TOOL_NAMES.has(tool.name) === isResearchCard)
              && tg.tools.some(t => !t.done)) {
              blocks[lastTg] = {
                ...tg,
                tools: [...tg.tools, toolCallFromStartEvent(msg)],
              };
              return { ...m, blocks };
            }
          }
          // 新建 tool_group（盖到达序号，与思考段按真实时间线交错）
          blocks.push({
            type: 'tool_group',
            tools: [toolCallFromStartEvent(msg)],
            collapsed: false,
            processOrder: buf.nextProcessOrder++,
          });
          return { ...m, blocks };
        });
        // 工具之后的新正文必须形成新的文本块，不能继续覆盖工具之前的正文。
        buf.textSegmentAcc = '';
        buf.textSegmentOrdinal = null;
        break;
      }

      case 'tool_end':
        this.publishBoundary(buf, (m) => {
          const blocks = [...(m.blocks || [])];
          // 从后往前找含该 tool 名且未 done 的
          for (let i = blocks.length - 1; i >= 0; i--) {
            if (blocks[i].type !== 'tool_group') continue;
            const tg = blocks[i] as Extract<ContentBlock, { type: 'tool_group' }>;
            const toolIdx = findOpenToolIndex(tg.tools, msg);
            if (toolIdx >= 0) {
              const tools = [...tg.tools];
              const id = toolCallIdFromEvent(msg);
              tools[toolIdx] = {
                ...tools[toolIdx],
                ...(id ? { id } : {}),
                done: true,
                success: !!msg.success,
                status: msg.status || (msg.success ? 'succeeded' : 'failed'),
                ...(typeof msg.error === 'string' && msg.error ? { error: msg.error } : {}),
                ...(msg.details !== undefined ? { details: msg.details } : {}),
                ...(typeof msg.resultNote === 'string' && msg.resultNote ? { resultNote: msg.resultNote } : {}),
              };
              const allDone = tools.every(t => t.done);
              blocks[i] = { ...tg, tools, collapsed: allDone && tools.length > 1 };
              return { ...m, blocks };
            }
          }
          return m;
        });
        break;

      case 'content_block': {
        let block = msg.block;
        // Apply cached patches (block_update 可能先于 content_block 到达)
        if (block.taskId) {
          const pending = (useStore.getState() as any)._pendingBlockPatches;
          const cached = pending?.[block.taskId];
          if (cached) {
            block = { ...block, ...cached };
            delete pending[block.taskId];
          }
        }

        if (isInterludeBlock(block)) {
          if (this.hasRunState(buf)) this.flush(buf);
          this.appendInterlude(buf, block);
          break;
        }

        const taskId = replacementTaskId(block);
        if (taskId) {
          if (this.hasRunState(buf)) this.flush(buf);
          const consumed = useStore.getState().resolveBlockByTaskId(buf.sessionPath, taskId, block);
          if (consumed) {
            bumpMessageLiveVersion(buf.sessionPath);
            break;
          }
        }

        this.ensureMessage(buf);
        this.publishBoundary(buf, (m) => ({
          ...m,
          blocks: mergeContentBlock([...(m.blocks || [])], block, () => buf.nextProcessOrder++),
        }));
        break;
      }

      case 'compaction_start':
        break;

      case 'compaction_end':
        break;

      case 'assistant_run_end': {
        // Assistant Run 结束（任务书 §十七/§二十二）：唯一能 finalize 的事件。
        // 严禁在 Pi Model Turn 边界执行 commit / reset / rebase block ID。
        const runKey = runKeyFrom(msg);
        // 不变量：同一个 Run 的 finalization 最多执行一次。
        if (!buf.runActive && runKey && runKey === buf.lastFinalizedRunKey) {
          console.debug('[stream] duplicate assistant_run_end ignored (exactly-once):', runKey);
          break;
        }
        if (buf.runActive && runKey && buf.activeRunKey && runKey !== buf.activeRunKey) {
          // 身份不匹配：Run 仍必须终结（服务端是 Run 结束的权威），但记录诊断。
          console.warn('[stream] assistant_run_end identity mismatch; finalizing active run:', {
            activeRunKey: buf.activeRunKey,
            runEndKey: runKey,
          });
        }
        if ((msg.aborted || msg.failed) && !this.hasRunState(buf)) {
          this.ensureMessage(buf);
        }
        const activeKeyBeforeFinish = buf.activeRunKey;
        this.finalizeRun(buf, {
          turnInputEntryId: msg.turnInputEntryId,
          userEntryId: msg.userEntryId,
          assistantEntryId: msg.assistantEntryId,
          assistantEntryIds: Array.isArray(msg.assistantEntryIds) ? msg.assistantEntryIds : undefined,
          status: msg.aborted ? 'aborted' : msg.failed ? 'failed' : 'completed',
        });
        buf.lastFinalizedRunKey = runKey || activeKeyBeforeFinish || buf.lastFinalizedRunKey;
        break;
      }

      case 'model_turn_end':
      case 'turn_end':
        // Pi Model Turn 结束（任务书 §十一）：仅 diagnostics，绝不 finalize / commit /
        // reset / rebase block ID / 产生 missing_final_answer。
        break;

    }
  }

  /**
   * 权威 Assistant Run 生命周期开始（只能由 WS assistant_run_start 驱动；
   * status / Pi Model Turn 不得调用）。
   *
   * 状态机：
   *   idle + assistant_run_start(A)        → active(A)，初始化本轮状态
   *   active(A) + assistant_run_start(A)   → 幂等 no-op（不得 flush/commit/reset）
   *   active(A) + assistant_run_start(B)   → 协议异常：记录 protocol_interrupted 诊断，
   *                                 把 A 以 aborted 终结（绝不产生 missing_final_answer
   *                                 的误报以外的第二真相），再开启 B
   */
  beginRun(
    sessionPath: string,
    sessionId: string | null = null,
    identity: { runId?: unknown; streamId?: unknown } = {},
  ): void {
    const buf = this.getBuffer(sessionPath, sessionId);
    const key = runKeyFrom(identity);
    if (buf.runActive) {
      if (key === null || buf.activeRunKey === null || key === buf.activeRunKey) {
        // 同一 Run（或一方身份未知）的重复 assistant_run_start：幂等 no-op；
        // 身份未知的一侧借机补上权威身份。
        if (buf.activeRunKey === null && key !== null) buf.activeRunKey = key;
        return;
      }
      console.warn('[stream] protocol_interrupted: assistant_run_start while another run is active:', {
        activeRunKey: buf.activeRunKey,
        incomingRunKey: key,
        sessionPath,
      });
      const interruptedKey = buf.activeRunKey;
      this.finalizeRun(buf, { status: 'aborted' });
      // 被中断 Run 迟到的 assistant_run_end 不得二次 finalize。
      if (interruptedKey) buf.lastFinalizedRunKey = interruptedKey;
    }
    buf.runActive = true;
    buf.activeRunKey = key;
  }

  /** 服务端确认当前 Run 结束或被中止：flush 可见内容，然后释放 Run 绑定。 */
  finishRun(sessionPath: string, sessionId: string | null = null): void {
    const buf = this.lookupBuffer(sessionPath, sessionId);
    if (!buf) return;
    buf.sessionPath = sessionPath;
    this.finalizeRun(buf);
  }

  /** 清理指定 session 的 buffer */
  clear(sessionPath: string, sessionId: string | null = null): void {
    const key = bufferKeyForSession(sessionPath, sessionId);
    const aliasKey = this.bufferKeysByPath.get(sessionPath) || null;
    const buf = this.lookupBuffer(sessionPath, sessionId);
    if (buf) this.cancelScheduledFlush(buf);
    if (buf?.messageId) clearLiveAssistantMessage(buf.sessionPath, buf.messageId);
    this.deleteBufferKey(key);
    if (aliasKey && aliasKey !== key) this.deleteBufferKey(aliasKey);
    if (key !== sessionPath) this.deleteBufferKey(sessionPath);
  }

  /** 清理所有 */
  clearAll(): void {
    for (const [, buf] of this.buffers) {
      this.cancelScheduledFlush(buf);
      if (buf.messageId) clearLiveAssistantMessage(buf.sessionPath, buf.messageId);
    }
    this.buffers.clear();
    this.bufferKeysByPath.clear();
  }

  /**
   * 取当前 buffer 的快照。供 loadMessages 在 session 重建后合并 in-flight
   * 内容：jsonl 只在 turn_end 落盘，在 stream 进行中重建 session 时，
   * 这份快照是避免 UI 上"正在流的消息凭空消失"的唯一来源。
   */
  snapshot(sessionPath: string, sessionId: string | null = null): StreamBufferSnapshot | null {
    const buf = this.lookupBuffer(sessionPath, sessionId);
    if (!buf) return null;
    const hasContent = !!(buf.blocks.length || buf.textAcc || buf.thinkingAcc || buf.hasThinkingBlock || buf.moodAcc);
    if (!hasContent) return null;
    return {
      hasContent: true,
      messageId: buf.messageId,
      blocks: renderBufferedBlocks(buf.blocks, buf),
      text: buf.textAcc,
      thinking: buf.thinkingAcc,
      mood: buf.inMood ? buf.moodAcc : cleanMoodText(buf.moodAcc),
      moodYuan: buf.moodYuan,
      inThinking: buf.inThinking,
      inMood: buf.inMood,
    };
  }
}

/** 全局 singleton */
export const streamBufferManager = new StreamBufferManager();

function mergeContentBlock(
  blocks: ContentBlock[],
  block: ContentBlock,
  claimProcessOrder?: () => number,
): ContentBlock[] {
  if (isInterludeBlock(block)) return blocks;
  if (block.type === 'media_generation' && block.status === 'pending') {
    const resolved = blocks.some((existing) => isResolvedTaskBlock(existing, block.taskId));
    if (resolved) return blocks;
  }
  // 追加新块时盖到达序号；任务块被终态替换时保留原戳，位置不漂。
  const stamped = block.processOrder !== undefined
    ? block
    : { ...block, processOrder: claimProcessOrder?.() };
  const taskId = replacementTaskId(block);
  if (!taskId) return [...blocks, stamped];
  const idx = blocks.findIndex((existing) => (
    existing.type === 'media_generation' &&
    existing.taskId === taskId
  ));
  if (idx < 0) return [...blocks, stamped];
  const next = [...blocks];
  next[idx] = blocks[idx].processOrder !== undefined
    ? { ...stamped, processOrder: blocks[idx].processOrder }
    : stamped;
  return next;
}

function replacementTaskId(block: ContentBlock): string | null {
  if (block.type === 'file') return block.replacesTaskId || null;
  if (block.type === 'media_generation' && block.status !== 'pending') return block.taskId;
  return null;
}

function isResolvedTaskBlock(block: ContentBlock, taskId: string): boolean {
  if (block.type === 'file') return block.replacesTaskId === taskId;
  return block.type === 'media_generation' &&
    block.taskId === taskId &&
    block.status !== 'pending';
}

function isInterludeBlock(block: ContentBlock): block is Extract<ContentBlock, { type: 'interlude' }> {
  return block.type === 'interlude';
}


// 让 chat-slice / session-actions 通过桥接模块触达 manager，打破循环依赖。
registerStreamBufferInvalidator((sessionPath) => {
  if (sessionPath == null) streamBufferManager.clearAll();
  else streamBufferManager.clear(sessionPath);
});
registerStreamBufferSnapshot((sessionPath) => streamBufferManager.snapshot(sessionPath));
