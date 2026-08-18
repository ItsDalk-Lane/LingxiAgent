/**
 * StreamBufferManager - per-session 流式事件节流缓冲（canonical 单通道收口版）
 *
 * 数据流（任务书 §9/§10）：WS 事件按轮锁定 inputMode——
 *   canonical：assistant_segment_* / mood_*(moodOrdinal) / tool_* / content_block
 *              统一进入 blocksById + blockOrder（唯一 block 集合），投影走
 *              projectAssistantTurn；legacy text/thinking/mood 事件只维护
 *              断线快照 textAcc，不再进 blocks（禁止双源合并）。
 *   legacy：旧服务器（无 canonical 事件）走原 text/thinking/mood 聚合管线，
 *           在 commit 时由 projector 统一收口成 canonical 投影。
 *
 * Block 身份（不变量 4/5）：canonical 模式下 idPrefix 在首事件时就用
 * `turn:<streamId|messageId>` 稳定前缀；turn_end 绑定持久化 entry 只写
 * sourceEntryId / turnInputEntryId metadata，绝不 rebase 已有 block.id。
 *
 * WS 事件到达时写入 buffer（纯 JS 对象，不触发 React），
 * 普通增量按画面合并并受最高发布频率约束，语义边界立即发布。
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
import { normalizeContentBlocks } from '../utils/content-semantics';
import { projectAssistantTurn } from '../utils/turn-projector';

/* eslint-disable @typescript-eslint/no-explicit-any -- 流式消息 handle(msg) 接收动态 JSON */

const STREAM_FLUSH_FPS = 30;
const FLUSH_INTERVAL = Math.round(1000 / STREAM_FLUSH_FPS);
let streamMessageSeq = 0;
type InterludeContentBlock = Extract<ContentBlock, { type: 'interlude' }>;

function nextStreamMessageId(): string {
  streamMessageSeq = (streamMessageSeq + 1) % Number.MAX_SAFE_INTEGER;
  return `stream-${Date.now()}-${streamMessageSeq}`;
}

type TurnInputMode = 'canonical' | 'legacy';

interface Buffer {
  sessionPath: string;
  /** 本轮输入模式：首个 canonical 事件到达即锁定（任务书 §10），turn 结束释放。 */
  inputMode: TurnInputMode;
  /** canonical 模式的稳定 block id 前缀（首次事件分配，绝不随 turn_end 改变）。 */
  canonicalIdPrefix: string | null;
  /** 统一 block 集合（任务书 §9）：id -> block；blockOrder 记录到达顺序。 */
  blocksById: Map<string, ContentBlock>;
  blockOrder: string[];
  segmentsById: Map<string, LiveAssistantSegment>;
  segmentOrder: string[];
  /** legacy 管线（旧服务器兼容）：普通文本/思考/mood 聚合。 */
  blocks: ContentBlock[];
  segmentsLegacyTextAcc: string;
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
  /** 当前 turn 绑定的 assistant message id */
  messageId: string | null;
  /** turn_end/中止收口时为 true，确保所有仍在流式的内容统一封口。 */
  turnEnding: boolean;
}

function createBuffer(sessionPath: string): Buffer {
  return {
    sessionPath,
    inputMode: 'legacy',
    canonicalIdPrefix: null,
    blocksById: new Map(),
    blockOrder: [],
    segmentsById: new Map(),
    segmentOrder: [],
    blocks: [],
    segmentsLegacyTextAcc: '',
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
    turnEnding: false,
  };
}

function pushBlock(buf: Buffer, id: string, block: ContentBlock): void {
  if (!buf.blocksById.has(id)) buf.blockOrder.push(id);
  buf.blocksById.set(id, block);
}

function canonicalMoodBlockId(buf: Buffer, moodOrdinal: number): string {
  return `${buf.canonicalIdPrefix}:mood:${moodOrdinal}`;
}

function renderBufferedBlocks(currentBlocks: ContentBlock[], buf: Buffer): ContentBlock[] {
  const blocks = [...currentBlocks];

  if (buf.thinkingAcc || buf.hasThinkingBlock || buf.inThinking) {
    const idx = blocks.findIndex(b => b.type === 'thinking');
    const thinkingBlock: ContentBlock = {
      type: 'thinking',
      content: buf.thinkingAcc,
      sealed: !buf.inThinking,
    };
    if (idx >= 0) blocks[idx] = thinkingBlock;
    else blocks.unshift(thinkingBlock);
  }

  if (buf.moodAcc || buf.inMood) {
    const idx = blocks.findIndex(b => b.type === 'mood');
    const moodBlock: ContentBlock = {
      type: 'mood',
      yuan: buf.moodYuan,
      text: buf.inMood ? buf.moodAcc : cleanMoodText(buf.moodAcc),
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

  private hasTurnState(buf: Buffer): boolean {
    return !!(
      buf.messageId ||
      buf.blocks.length > 0 ||
      buf.blocksById.size > 0 ||
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

  private resetTurnState(buf: Buffer): void {
    this.cancelScheduledFlush(buf);
    buf.inputMode = 'legacy';
    buf.canonicalIdPrefix = null;
    buf.blocksById.clear();
    buf.blockOrder = [];
    buf.textAcc = '';
    buf.blocks = [];
    buf.segmentsById.clear();
    buf.segmentOrder = [];
    buf.segmentsLegacyTextAcc = '';
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
    buf.turnEnding = false;
    buf.publishPending = false;
  }

  /**
   * 本轮第一个 canonical 事件到达：锁定 canonical 模式并分配稳定 id 前缀。
   * 前缀一经分配，turn 结束前绝不改变（不变量 4）。
   * legacy 管线已积累的内容（旧服务器先发 text_delta 再发 tool_start 的场景）
   * 冻结为 canonical 起点：原样搬进统一 block 集合，id 首次分配后同样不变。
   */
  private lockCanonicalMode(buf: Buffer): void {
    if (buf.inputMode === 'canonical') return;
    this.ensureMessage(buf);
    buf.inputMode = 'canonical';
    buf.canonicalIdPrefix = `turn:${buf.messageId || nextStreamMessageId()}`;
    // legacy 聚合结果冻结入统一集合（一次性，不做语义改写）。legacy 渲染的
    // thinking/mood 块在这里拿稳定 id；legacy text 块在 canonical 模式下改由
    // synthetic segment 承载（见 ensureSyntheticTextSegment），冻结时跳过，
    // 避免同一正文同时存在于 block 与 segment 两个通道（不变量 1）。
    if (buf.blocks.length > 0) {
      const frozen = buf.blocks.filter((block) => block.type !== 'text');
      buf.blocks = [];
      for (const block of frozen) {
        const id = block.id?.trim()
          || `${buf.canonicalIdPrefix}:${block.type}:${buf.blocksById.size}`;
        if (!buf.blocksById.has(id)) buf.blockOrder.push(id);
        buf.blocksById.set(id, { ...block, id });
      }
    }
  }

  private finishBufferTurn(buf: Buffer, persistedEntries: {
    turnInputEntryId?: string | null;
    userEntryId?: string | null;
    assistantEntryId?: string | null;
    assistantEntryIds?: string[];
    status?: 'completed' | 'failed' | 'aborted';
  } = {}): void {
    if (this.hasTurnState(buf)) {
      buf.turnEnding = true;
      this.flush(buf);
      this.commitLiveTurn(buf, persistedEntries);
    } else {
      this.cancelScheduledFlush(buf);
    }
    this.resetTurnState(buf);
  }

  /** 确保 store 中已存在当前 turn 绑定的 assistant message */
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
    // legacy 模式：进入 store 前补齐语义字段；canonical 模式不经此路径。
    const blocks = buf.inputMode === 'canonical'
      ? next.blocks || []
      : normalizeContentBlocks(next.blocks || [], {
        idPrefix: item.data.sourceEntryId || item.data.id,
        turnLifecycle: buf.turnEnding ? 'sealed' : 'streaming',
      });
    buf.blocks = blocks;
    this.publishLiveTurn(buf);
    bumpMessageLiveVersion(buf.sessionPath);
    return true;
  }

  private publishLiveTurn(buf: Buffer): void {
    if (!buf.messageId) return;
    const store = useStore.getState();
    const session = sessionScopedValue(store, store.chatSessions, buf.sessionPath);
    const item = session?.items.find((entry) => (
      entry.type === 'message'
      && entry.data.role === 'assistant'
      && entry.data.id === buf.messageId
    ));
    const message = item?.type === 'message' ? item.data : null;
    if (buf.inputMode === 'canonical') {
      // canonical 模式：统一 block 集合直接投影，前缀稳定不变。
      const segments = buf.segmentOrder
        .map((segmentId) => buf.segmentsById.get(segmentId))
        .filter((segment): segment is LiveAssistantSegment => !!segment);
      const orderedBlocks = buf.blockOrder
        .map((blockId) => buf.blocksById.get(blockId))
        .filter((block): block is ContentBlock => !!block);
      const projected = projectAssistantTurn({
        idPrefix: buf.canonicalIdPrefix!,
        inputMessageId: message?.turnInputEntryId || null,
        assistantMessageIds: [message?.sourceEntryId || message?.id || buf.messageId],
        segments,
        legacyBlocks: orderedBlocks,
        status: 'streaming',
      });
      publishLiveAssistantMessage(buf.sessionPath, buf.messageId, projected.blocks, {
        segmentsById: Object.fromEntries(buf.segmentsById),
        segmentOrder: [...buf.segmentOrder],
        status: buf.turnEnding ? 'sealed' : 'streaming',
        turnProjection: projected.projection,
      });
      return;
    }
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
      status: buf.turnEnding ? 'sealed' : 'streaming',
      turnProjection: projected.projection,
    });
  }

  private updateCanonicalSegment(buf: Buffer, msg: any): void {
    this.lockCanonicalMode(buf);
    if (!buf.messageId || typeof msg.segmentId !== 'string' || !msg.segmentId) return;
    const existing = buf.segmentsById.get(msg.segmentId);
    if (msg.type === 'assistant_segment_start') {
      const semanticPhase = normalizeLiveSegmentPhase(msg.semanticPhase);
      const segment: LiveAssistantSegment = {
        id: msg.segmentId,
        kind: msg.kind === 'reasoning' ? 'reasoning' : 'text',
        semanticPhase,
        source: existing?.source || '',
        lifecycle: 'streaming',
      };
      if (!existing) buf.segmentOrder.push(msg.segmentId);
      buf.segmentsById.set(msg.segmentId, segment);
    } else if (msg.type === 'assistant_segment_delta') {
      const semanticPhase = normalizeLiveSegmentPhase(msg.semanticPhase || existing?.semanticPhase);
      const segment: LiveAssistantSegment = existing || {
        id: msg.segmentId,
        kind: semanticPhase === 'reasoning' ? 'reasoning' : 'text',
        semanticPhase,
        source: '',
        lifecycle: 'streaming',
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

  /**
   * canonical mood 事件（带 moodOrdinal，服务端 parser 链产出）：
   * 直接进统一 block 集合，一个 mood 一块，id 首次分配后不变。
   */
  private updateCanonicalMood(buf: Buffer, msg: any): void {
    this.lockCanonicalMode(buf);
    const moodOrdinal = Number.isInteger(msg.moodOrdinal) ? msg.moodOrdinal as number : null;
    if (moodOrdinal === null) return;
    const blockId = canonicalMoodBlockId(buf, moodOrdinal);
    const existing = buf.blocksById.get(blockId);
    const yuan = existing?.type === 'mood' ? existing.yuan : resolveSessionYuan(buf.sessionPath);
    if (msg.type === 'mood_start') {
      pushBlock(buf, blockId, {
        id: blockId,
        type: 'mood',
        yuan,
        text: '',
        semanticPhase: 'mood',
        surfaceRole: 'process',
        lifecycle: 'streaming',
      });
      this.publishBoundary(buf);
    } else if (msg.type === 'mood_text') {
      const prev = existing?.type === 'mood' ? existing.text : '';
      pushBlock(buf, blockId, {
        id: blockId,
        type: 'mood',
        yuan,
        text: `${prev}${typeof msg.delta === 'string' ? msg.delta : ''}`,
        semanticPhase: 'mood',
        surfaceRole: 'process',
        lifecycle: 'streaming',
      });
      this.scheduleFlush(buf);
    } else if (msg.type === 'mood_end') {
      const prev = existing?.type === 'mood' ? existing.text : '';
      pushBlock(buf, blockId, {
        id: blockId,
        type: 'mood',
        yuan,
        text: prev,
        semanticPhase: 'mood',
        surfaceRole: 'process',
        lifecycle: 'sealed',
      });
      this.publishBoundary(buf);
    }
  }

  private commitLiveTurn(buf: Buffer, persistedEntries: {
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
    const status = persistedEntries.status || 'completed';
    const segments = buf.segmentOrder
      .map((segmentId) => buf.segmentsById.get(segmentId))
      .filter((segment): segment is LiveAssistantSegment => !!segment);

    // Block ID 在 canonical 模式下绝不 rebase（不变量 5）：持久化 entry 只是
    // metadata。legacy 模式沿用消息 id 前缀收口。
    const legacyBlocks = buf.inputMode === 'canonical'
      ? buf.blockOrder
        .map((blockId) => buf.blocksById.get(blockId))
        .filter((block): block is ContentBlock => !!block)
      : buf.blocks;
    const idPrefix = buf.inputMode === 'canonical'
      ? buf.canonicalIdPrefix!
      : (persistedEntries.assistantEntryId || message?.sourceEntryId || buf.messageId);
    const projected = projectAssistantTurn({
      idPrefix,
      inputMessageId: persistedEntries.turnInputEntryId || message?.turnInputEntryId || null,
      assistantMessageIds: persistedEntries.assistantEntryIds?.length
        ? persistedEntries.assistantEntryIds
        : [persistedEntries.assistantEntryId || message?.sourceEntryId || buf.messageId],
      segments,
      legacyBlocks,
      status,
    });
    for (const diagnostic of projected.diagnostics) {
      console.warn('[stream] unresolved assistant segment finalized with fallback:', diagnostic.segmentId);
    }
    if (buf.inputMode !== 'canonical') buf.blocks = projected.blocks;
    const committed = useStore.getState().bindPersistedTurnEntries(buf.sessionPath, {
      ...persistedEntries,
      assistantMessageId: buf.messageId,
      assistantBlocks: projected.blocks,
      assistantProjection: projected.projection,
    });
    clearLiveAssistantMessage(buf.sessionPath, buf.messageId);
    if (!committed) {
      console.warn('[stream] failed to commit live assistant turn:', buf.sessionPath, buf.messageId);
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
    if (buf.inputMode === 'canonical') {
      // canonical 模式：legacy 聚合管线已退役，直接投影发布。
      this.publishLiveTurn(buf);
      recordChatPerformance('stream_flush', {
        sessionPath: buf.sessionPath,
        messageId: buf.messageId || undefined,
        sourceLength: buf.textSegmentAcc.length,
        blockCount: buf.blocksById.size,
      });
      buf.lastFlushTime = Date.now();
      buf.publishPending = false;
      return;
    }
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

  handle(msg: any): void {
    const sessionPath = msg.sessionPath;
    if (!sessionPath) {
      console.warn('[ws] stream event missing sessionPath:', msg.type);
      return;
    }
    const sessionId = normalizeSessionId(msg.sessionId);
    const buf = this.getBuffer(sessionPath, sessionId);

    switch (msg.type) {
      case 'assistant_segment_start':
      case 'assistant_segment_delta':
      case 'assistant_segment_end':
        this.updateCanonicalSegment(buf, msg);
        break;

      // canonical mood 事件（带 moodOrdinal）：协议解析已前移到服务端
      // AssistantEventNormalizer，这些事件直接进统一 block 集合。
      case 'mood_start':
      case 'mood_text':
      case 'mood_end':
        if (Number.isInteger(msg.moodOrdinal)) {
          this.updateCanonicalMood(buf, msg);
          break;
        }
        // 旧服务器（无 moodOrdinal）走 legacy 聚合管线
        this.handleLegacyMood(buf, msg, sessionPath);
        break;

      case 'text_delta':
        // 断线快照永远记录完整可见正文（含 canonical 模式）。
        buf.textAcc += msg.delta || '';
        if (buf.inputMode === 'canonical') {
          // canonical 模式：正文由 assistant_segment_delta 通道承载。但旧服务器
          // 兼容期可能只发 text_delta（canonical 事件缺席的正文），此时把增量
          // 记入 textSegmentAcc 并合成一个显式 text segment，保证内容不丢。
          buf.textSegmentAcc += msg.delta || '';
          this.ensureSyntheticTextSegment(buf);
          this.scheduleFlush(buf);
          break;
        }
        this.ensureMessage(buf);
        buf.segmentsLegacyTextAcc += msg.delta || '';
        buf.textSegmentAcc += msg.delta || '';
        this.scheduleFlush(buf);
        break;

      case 'thinking_start':
        if (buf.inputMode === 'canonical') break;
        this.ensureMessage(buf);
        buf.inThinking = true;
        buf.hasThinkingBlock = true;
        buf.thinkingAcc = '';
        this.publishBoundary(buf);
        break;

      case 'thinking_delta':
        if (buf.inputMode === 'canonical') break;
        buf.hasThinkingBlock = true;
        buf.thinkingAcc += msg.delta || '';
        // 与 text/mood 共用时间节流，避免思考流只能在结束后显示。
        this.scheduleFlush(buf);
        break;

      case 'thinking_end':
        if (buf.inputMode === 'canonical') break;
        buf.hasThinkingBlock = true;
        buf.inThinking = false;
        this.publishBoundary(buf);
        break;

      case 'card_start':
        if (buf.inputMode === 'canonical') break;
        this.ensureMessage(buf);
        buf.inCard = true;
        buf.cardAttrs = msg.attrs || null;
        buf.cardDescAcc = '';
        break;

      case 'card_text':
        if (buf.inputMode === 'canonical') break;
        buf.cardDescAcc += msg.delta || '';
        break;

      case 'card_end': {
        if (buf.inputMode === 'canonical') break;
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

      case 'tool_start':
        this.lockCanonicalMode(buf);
        this.ensureMessage(buf);
        {
          const blockId = this.nextToolGroupKey(buf, msg);
          const tools = (() => {
            // 找当前打开的 tool_group（canonical：最后一个 tool_group block）
            const openId = this.findOpenToolGroupId(buf);
            if (openId !== null) {
              const group = buf.blocksById.get(openId);
              if (group?.type === 'tool_group') {
                return [...group.tools, toolCallFromStartEvent(msg)];
              }
            }
            return [toolCallFromStartEvent(msg)];
          })();
          pushBlock(buf, blockId, {
            id: blockId,
            type: 'tool_group',
            tools,
            collapsed: false,
            semanticPhase: 'tool',
            surfaceRole: 'process',
            lifecycle: 'streaming',
          });
        }
        // 工具之后的新正文必须形成新的文本块，不能继续覆盖工具之前的正文。
        // 冻结点前先把尚未合成 segment 的兼容正文落盘，工具边界后再开新段。
        if (buf.textSegmentAcc) this.ensureSyntheticTextSegment(buf);
        buf.textSegmentAcc = '';
        buf.textSegmentOrdinal = null;
        this.publishBoundary(buf);
        break;

      case 'tool_end': {
        const openId = this.findOpenToolGroupId(buf);
        if (openId !== null) {
          const group = buf.blocksById.get(openId);
          if (group?.type === 'tool_group') {
            const toolIdx = findOpenToolIndex(group.tools, msg);
            if (toolIdx >= 0) {
              const tools = [...group.tools];
              const id = toolCallIdFromEvent(msg);
              tools[toolIdx] = {
                ...tools[toolIdx],
                ...(id ? { id } : {}),
                done: true,
                success: !!msg.success,
                status: msg.status || (msg.success ? 'succeeded' : 'failed'),
                ...(typeof msg.error === 'string' && msg.error ? { error: msg.error } : {}),
                ...(msg.details !== undefined ? { details: msg.details } : {}),
              };
              const allDone = tools.every(t => t.done);
              pushBlock(buf, openId, {
                ...group,
                tools,
                collapsed: allDone && tools.length > 1,
                lifecycle: allDone ? 'sealed' : 'streaming',
              });
              this.publishBoundary(buf);
              break;
            }
          }
        }
        // legacy 模式：写 buf.blocks（保持旧路径行为）
        if (buf.inputMode !== 'canonical') {
          this.publishBoundary(buf, (m) => {
            const blocks = [...(m.blocks || [])];
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
                };
                const allDone = tools.every(t => t.done);
                blocks[i] = { ...tg, tools, collapsed: allDone && tools.length > 1 };
                return { ...m, blocks };
              }
            }
            return m;
          });
        }
        break;
      }

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
          if (this.hasTurnState(buf)) this.flush(buf);
          this.appendInterlude(buf, block);
          break;
        }

        const taskId = replacementTaskId(block);
        if (taskId) {
          if (this.hasTurnState(buf)) this.flush(buf);
          const consumed = useStore.getState().resolveBlockByTaskId(buf.sessionPath, taskId, block);
          if (consumed) {
            bumpMessageLiveVersion(buf.sessionPath);
            break;
          }
        }

        if (buf.inputMode === 'canonical') {
          this.lockCanonicalMode(buf);
          const id = block.id?.trim()
            || `${buf.canonicalIdPrefix}:${block.type}:${buf.blocksById.size}`;
          const mergeKey = replacementTaskId(block);
          if (mergeKey) {
            const existingId = [...buf.blocksById.entries()]
              .find(([, value]) => value.type === 'media_generation' && value.taskId === mergeKey)?.[0];
            if (existingId) {
              pushBlock(buf, existingId, { ...block, id: existingId });
              this.publishBoundary(buf);
              break;
            }
          }
          pushBlock(buf, id, block);
          this.publishBoundary(buf);
          break;
        }
        this.ensureMessage(buf);
        this.publishBoundary(buf, (m) => ({
          ...m,
          blocks: mergeContentBlock([...(m.blocks || [])], block),
        }));
        break;
      }

      case 'compaction_start':
        break;

      case 'compaction_end':
        break;

      case 'turn_end':
        if ((msg.aborted || msg.failed) && !this.hasTurnState(buf)) {
          this.ensureMessage(buf);
        }
        this.finishBufferTurn(buf, {
          turnInputEntryId: msg.turnInputEntryId,
          userEntryId: msg.userEntryId,
          assistantEntryId: msg.assistantEntryId,
          assistantEntryIds: Array.isArray(msg.assistantEntryIds) ? msg.assistantEntryIds : undefined,
          status: msg.aborted ? 'aborted' : msg.failed ? 'failed' : 'completed',
        });
        break;

    }
  }

  /**
   * canonical 模式下兼容双发的 text_delta：当本段正文没有对应 canonical
   * segment 时（旧服务器只发 text_delta 的路径），把 textSegmentAcc 合成为
   * 一个显式 text segment，保证内容不丢。canonical segment 已承载同段正文
   * 时保持静默（禁止双源，不变量 3）。
   */
  private ensureSyntheticTextSegment(buf: Buffer): void {
    if (!buf.textSegmentAcc) return;
    const segmentId = `legacy-text:${buf.segmentOrder.length}`;
    const existing = buf.segmentsById.get(segmentId);
    const canonicalTextSegments = buf.segmentOrder
      .filter((id) => !id.startsWith('legacy-text:'))
      .map((id) => buf.segmentsById.get(id))
      .filter((segment): segment is LiveAssistantSegment => !!segment && segment.kind === 'text');
    // 完全相同源文本 -> canonical segment 已承载，跳过（双发去重）
    if (canonicalTextSegments.some((segment) => segment.source === buf.textSegmentAcc)) return;
    if (!existing) buf.segmentOrder.push(segmentId);
    buf.segmentsById.set(segmentId, {
      id: segmentId,
      kind: 'text',
      semanticPhase: 'final_answer',
      source: buf.textSegmentAcc,
      lifecycle: 'streaming',
    });
  }

  private handleLegacyMood(buf: Buffer, msg: any, sessionPath: string): void {
    this.ensureMessage(buf);
    if (msg.type === 'mood_start') {
      buf.inMood = true;
      // 同一 buffer 内若已有已封存的 mood 段（例如一个 user turn 内多段模型生成
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
    } else if (msg.type === 'mood_text') {
      // pending separator 只在收到非空内容时落地，空段不制造多余空白
      if (buf.moodPendingSeparator && (msg.delta || '').trim()) {
        buf.moodAcc += '\n\n';
        buf.moodPendingSeparator = false;
      }
      buf.moodAcc += msg.delta || '';
      this.scheduleFlush(buf);
    } else if (msg.type === 'mood_end') {
      buf.inMood = false;
      this.publishBoundary(buf);
    }
  }

  /** canonical 统一集合里最后一个仍有未完成工具的 tool_group key。 */
  private findOpenToolGroupId(buf: Buffer): string | null {
    for (let i = buf.blockOrder.length - 1; i >= 0; i--) {
      const block = buf.blocksById.get(buf.blockOrder[i]);
      if (block?.type === 'tool_group' && block.tools.some(t => !t.done)) return buf.blockOrder[i];
    }
    return null;
  }

  private nextToolGroupKey(buf: Buffer, msg: any): string {
    // 上一个 group 已全部完成 -> 新 group；否则并入打开中的 group（id 不变）。
    const openId = this.findOpenToolGroupId(buf);
    if (openId !== null) return openId;
    const callId = toolCallIdFromEvent(msg as never) || `start-${buf.blocksById.size}`;
    return `${buf.canonicalIdPrefix}:tool_group:tools:${callId}`;
  }

  /** 服务端确认新 turn 开始：释放任何遗留的本地 turn 绑定。 */
  beginTurn(sessionPath: string, sessionId: string | null = null): void {
    const buf = this.getBuffer(sessionPath, sessionId);
    this.finishBufferTurn(buf);
  }

  /** 服务端确认当前 turn 结束或被中止：flush 可见内容，然后释放 turn-local 绑定。 */
  finishTurn(sessionPath: string, sessionId: string | null = null): void {
    const buf = this.lookupBuffer(sessionPath, sessionId);
    if (!buf) return;
    buf.sessionPath = sessionPath;
    this.finishBufferTurn(buf);
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
    if (buf.inputMode === 'canonical') {
      const segments = buf.segmentOrder
        .map((segmentId) => buf.segmentsById.get(segmentId))
        .filter((segment): segment is LiveAssistantSegment => !!segment);
      const blocks = buf.blockOrder
        .map((blockId) => buf.blocksById.get(blockId))
        .filter((block): block is ContentBlock => !!block);
      const segmentTexts = segments
        .filter((segment) => segment.kind === 'text')
        .map((segment) => segment.source);
      const hasContent = !!(blocks.length || segments.length || buf.textAcc);
      if (!hasContent) return null;
      return {
        hasContent: true,
        messageId: buf.messageId,
        blocks: [
          ...blocks,
          ...segments.map((segment) => ({
            type: segment.kind === 'reasoning' ? 'thinking' : 'text',
            ...(segment.kind === 'reasoning'
              ? { content: segment.source, sealed: segment.lifecycle === 'sealed' }
              : { source: segment.source }),
          } as ContentBlock)),
        ],
        text: buf.textAcc || segmentTexts.join(''),
        thinking: segments
          .filter((segment) => segment.kind === 'reasoning')
          .map((segment) => segment.source)
          .join(''),
        mood: buf.blocksById.size
          ? [...buf.blocksById.values()].filter((block): block is Extract<ContentBlock, { type: 'mood' }> => block.type === 'mood')
            .map((block) => block.text).join('\n\n')
          : (buf.inMood ? buf.moodAcc : cleanMoodText(buf.moodAcc)),
        moodYuan: (() => {
          for (const block of buf.blocksById.values()) {
            if (block.type === 'mood') return block.yuan;
          }
          return buf.moodYuan;
        })(),
        inThinking: false,
        inMood: false,
      };
    }
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

function mergeContentBlock(blocks: ContentBlock[], block: ContentBlock): ContentBlock[] {
  if (isInterludeBlock(block)) return blocks;
  if (block.type === 'media_generation' && block.status === 'pending') {
    const resolved = blocks.some((existing) => isResolvedTaskBlock(existing, block.taskId));
    if (resolved) return blocks;
  }
  const taskId = replacementTaskId(block);
  if (!taskId) return [...blocks, block];
  const idx = blocks.findIndex((existing) => (
    existing.type === 'media_generation' &&
    existing.taskId === taskId
  ));
  if (idx < 0) return [...blocks, block];
  const next = [...blocks];
  next[idx] = block;
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
