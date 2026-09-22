/**
 * stream-resume.ts — 流恢复逻辑（从 app-ws-shim.ts 迁移）
 *
 * 管理 per-session 流元数据、断线重连后的 stream resume 请求和事件重放。
 * 不依赖 ctx 注入，通过 Zustand store 访问状态。
 */

/* eslint-disable @typescript-eslint/no-explicit-any -- WS 消息协议为动态 JSON，类型无法静态收窄 */

import { StreamAdmission } from './stream-admission';
import { streamBufferManager } from '../hooks/use-stream-buffer';
import { useStore } from '../stores';
import { sessionIdForPathFromLocatorState, sessionScopedKey } from '../stores/session-slice';
import { clearChat } from '../stores/agent-actions';
import { loadMessages } from '../stores/session-actions';
import { registerStreamResumeMetaInvalidator } from '../stores/stream-invalidator';

// 延迟导入，打破循环依赖
let _handleServerMessage: ((msg: any) => boolean | void) | null = null;
let _applyStreamingStatus: ((
  isStreaming: boolean,
  sessionPath: string | null,
  identity?: { streamId?: string | null; turnId?: string | null },
  options?: { force?: boolean },
) => boolean | void) | null = null;
let _getWebSocket: (() => WebSocket | null) | null = null;

export function injectHandlers(
  handleServerMessage: (msg: any) => boolean | void,
  applyStreamingStatus: (
    isStreaming: boolean,
    sessionPath: string | null,
    identity?: { streamId?: string | null; turnId?: string | null },
    options?: { force?: boolean },
  ) => boolean | void,
): void {
  _handleServerMessage = handleServerMessage;
  _applyStreamingStatus = applyStreamingStatus;
}

export function injectWebSocketGetter(getWebSocket: () => WebSocket | null): void {
  _getWebSocket = getWebSocket;
}

// ── 流恢复版本计数 ──
const _streamResumeRebuildVersions: Record<string, number> = {};
let _streamResumeRebuildingFor: string | null = null;

// ── Session 流元数据（module-level，不走 Zustand） ──
const MAX_CONSUMED_SEQS = 10_000;

type SessionStreamMeta = {
  admission: StreamAdmission;
  streamId: string | null;
  lastSeq: number;
  consumedSeqs: Set<number>;
  /**
   * 恢复代次：每次真实接纳提交（updateSessionStreamMeta 返回 true 或恢复重建
   * 重置水位）+1。rebuild 在 await 历史读取前后对比它，等待期间发生过任何
   * 接纳提交（例如新流经真实 WS 被接纳）就整体放弃旧恢复，防止旧快照覆盖新流。
   */
  admissionEpoch: number;
  /** 当前流权威是何时确立的（admissionEpoch 值）：迟到的旧 resume 响应据此判定过期。 */
  authorityEpoch: number;
};

const _sessionStreams: Record<string, SessionStreamMeta> = {};

/** 按 session 记录最近一次 resume 请求的恢复代次（请求-响应关联）。 */
type ResumeRequestGeneration = {
  token: string;
  epochAtRequest: number;
  authorityAtRequest: string | null;
};
const _resumeRequestGenerations: Record<string, ResumeRequestGeneration> = {};
let _resumeTokenCounter = 0;
function nextResumeToken(): string {
  _resumeTokenCounter += 1;
  return `rrg-${Date.now().toString(36)}-${_resumeTokenCounter}-${Math.random().toString(36).slice(2, 8)}`;
}

type StreamSessionInput = string | {
  sessionId?: unknown;
  sessionPath?: unknown;
  path?: unknown;
  session?: {
    sessionId?: unknown;
    path?: unknown;
  } | null;
} | null | undefined;

type ResolvedStreamSession = {
  sessionId: string | null;
  sessionPath: string | null;
  key: string | null;
  isCurrent: boolean;
};

function normalizeStreamString(value: unknown): string | null {
  return typeof value === 'string' && value.trim() ? value.trim() : null;
}

function streamRefFromInput(input: StreamSessionInput, opts: any = {}): { sessionId: string | null; sessionPath: string | null } {
  if (typeof input === 'string') {
    return {
      sessionId: normalizeStreamString(opts?.sessionId),
      sessionPath: normalizeStreamString(input) || normalizeStreamString(opts?.sessionPath),
    };
  }
  const session = input && typeof input === 'object' && input.session && typeof input.session === 'object'
    ? input.session
    : null;
  return {
    sessionId: normalizeStreamString(input && typeof input === 'object' ? input.sessionId : null)
      || normalizeStreamString(session?.sessionId)
      || normalizeStreamString(opts?.sessionId),
    sessionPath: normalizeStreamString(input && typeof input === 'object' ? input.sessionPath : null)
      || normalizeStreamString(input && typeof input === 'object' ? input.path : null)
      || normalizeStreamString(session?.path)
      || normalizeStreamString(opts?.sessionPath),
  };
}

function resolveStreamSession(
  input?: StreamSessionInput,
  opts: { fallbackToCurrent?: boolean; requestOptions?: any } = {},
): ResolvedStreamSession {
  const state = useStore.getState();
  const ref = streamRefFromInput(input, opts.requestOptions);
  const currentSessionId = normalizeStreamString(state.currentSessionId);
  const currentSessionPath = normalizeStreamString(state.currentSessionPath);
  const explicitPath = ref.sessionPath || (opts.fallbackToCurrent !== false ? currentSessionPath : null);
  const sessionId = ref.sessionId
    || (explicitPath ? sessionIdForPathFromLocatorState(state, explicitPath) : null)
    || (explicitPath && explicitPath === currentSessionPath ? currentSessionId : null);
  const currentPathForSessionId = sessionId && sessionId === currentSessionId ? currentSessionPath : null;
  const locatorPath = sessionId
    ? normalizeStreamString(state.sessionLocatorsById?.[sessionId]?.path)
    : null;
  const sessionPath = currentPathForSessionId || locatorPath || explicitPath;
  const key = sessionId || (sessionPath ? sessionScopedKey(state, sessionPath) || sessionPath : null);
  const isCurrent = (!!sessionId && sessionId === currentSessionId)
    || (!!sessionPath && sessionPath === currentSessionPath);
  return { sessionId, sessionPath, key, isCurrent };
}

function isStillCurrentStreamSession(target: ResolvedStreamSession): boolean {
  const state = useStore.getState();
  if (target.sessionId) return state.currentSessionId === target.sessionId;
  return !!target.sessionPath && state.currentSessionPath === target.sessionPath;
}

function streamIdentityKey(input?: StreamSessionInput): string | null {
  return resolveStreamSession(input).key;
}

export function invalidateSessionStreamMeta(sessionRef?: StreamSessionInput): void {
  if (sessionRef == null) {
    for (const key of Object.keys(_sessionStreams)) delete _sessionStreams[key];
    for (const key of Object.keys(_resumeRequestGenerations)) delete _resumeRequestGenerations[key];
    return;
  }
  const target = resolveStreamSession(sessionRef, { fallbackToCurrent: false });
  const key = target.key || target.sessionPath;
  if (key) {
    delete _sessionStreams[key];
    delete _resumeRequestGenerations[key];
  }
  if (target.sessionPath && target.sessionPath !== key) {
    delete _sessionStreams[target.sessionPath];
    delete _resumeRequestGenerations[target.sessionPath];
  }
}

export function getSessionStreamMeta(sessionRef?: StreamSessionInput): SessionStreamMeta | null {
  const target = resolveStreamSession(sessionRef);
  const path = target.sessionPath;
  const key = target.key || path;
  if (!key) return null;
  if (!_sessionStreams[key]) {
    _sessionStreams[key] = (path ? _sessionStreams[path] : null) || {
      admission: new StreamAdmission(),
      streamId: null,
      lastSeq: 0,
      consumedSeqs: new Set(),
      admissionEpoch: 0,
      authorityEpoch: 0,
    };
    if (path && key !== path) delete _sessionStreams[path];
  }
  return _sessionStreams[key];
}

/** 只读查看：不惰性创建条目（rebuild 的代次守卫用它避免为检查而产生副作用）。 */
function peekSessionStreamMeta(sessionRef?: StreamSessionInput): SessionStreamMeta | null {
  const target = resolveStreamSession(sessionRef);
  const key = target.key || target.sessionPath;
  if (!key) return null;
  return _sessionStreams[key] || null;
}

export function isStreamScopedMessage(msg: any): boolean {
  const ref = streamRefFromInput(msg, {});
  return !!(msg && (ref.sessionId || ref.sessionPath) && (msg.streamId || Number.isFinite(msg.seq)));
}

export function updateSessionStreamMeta(meta: any = {}): boolean {
  const target = resolveStreamSession(meta);
  if (!target.key && !target.sessionPath) return true;
  const entry = getSessionStreamMeta(target);
  if (!entry) return true;

  // ① 身份校验先行且无副作用：被拒帧不得改变退休集合或当前身份
  //（例如 run_end 因缺口被拒时先退休了 runId，补发后会被当成旧 Run 永远拒收）。
  if (!entry.admission.wouldAccept(meta)) {
    if (entry.admission.recoveryRequired && !entry.admission.recoveryRequested) {
      entry.admission.recoveryRequested = true;
      requestStreamResume(target, { fromStart: true, streamId: null });
    }
    return false;
  }

  const stream = normalizeStreamString(meta.streamId);
  // 换流会把序号基线重置为 0：序号校验按切换后的前瞻视图进行，但提交必须等
  // 全部校验通过后一次性完成，被拒帧不得留下半套切换状态。
  const switchesStream = !!stream && !!entry.streamId && entry.streamId !== stream;

  if (Number.isFinite(meta.seq)) {
    const seq = Math.max(0, Math.floor(meta.seq));
    // 幂等重复：不重投影；水位证据必须来自先前真实消费，不为重复帧新增证据。
    if (!switchesStream && (entry.consumedSeqs.has(seq) || seq <= entry.lastSeq)) return false;
    // ② 实际 WS 帧发现缺口时不提前提交（含身份切换），向已有恢复通道补取连续前缀。
    const baseline = switchesStream ? 0 : entry.lastSeq;
    if (typeof meta.type === 'string' && seq > baseline + 1 && !meta.__fromReplay) {
      requestStreamResume(
        target,
        switchesStream ? { fromStart: true, streamId: stream } : { sinceSeq: entry.lastSeq },
      );
      return false;
    }
  }

  // ③ 全部校验通过后一次性提交身份与水位。
  const authorityBefore = entry.admission.streamId;
  entry.admission.commit(meta);
  if (switchesStream) {
    entry.lastSeq = 0;
    entry.consumedSeqs.clear();
  }
  if (stream) entry.streamId = stream;
  if (Number.isFinite(meta.seq)) markConsumedSeq(entry, Math.max(0, Math.floor(meta.seq)));
  entry.admissionEpoch += 1;
  if (entry.admission.streamId !== authorityBefore) entry.authorityEpoch = entry.admissionEpoch;
  return true;
}

export function isStreamResumeRebuilding(): string | null {
  return _streamResumeRebuildingFor;
}

export function requestStreamResume(sessionRef?: StreamSessionInput, opts: any = {}): void {
  const target = resolveStreamSession(sessionRef, { requestOptions: opts });
  const path = target.sessionPath;
  const ws = _getWebSocket?.() || null;
  if (!path || !ws || ws.readyState !== WebSocket.OPEN) return;
  const sessionId = target.sessionId || sessionIdForPathFromLocatorState(useStore.getState(), path);
  const existing = peekSessionStreamMeta(target);
  const fromStart = !!opts.fromStart;
  const streamId = opts.streamId !== undefined ? opts.streamId : (existing?.streamId || null);
  const sinceSeq = Number.isFinite(opts.sinceSeq)
    ? Math.max(0, Math.floor(opts.sinceSeq))
    : (fromStart ? 0 : (existing?.lastSeq || 0));
  // 恢复代次：每次请求登记“发出时刻的接纳代次与流权威”，响应携带同一 token 回来时
  // 用于丢弃迟到的旧响应（等待期间本地已接纳新流）。
  const generationKey = target.key || path;
  const generation: ResumeRequestGeneration = {
    token: nextResumeToken(),
    epochAtRequest: existing?.admissionEpoch ?? 0,
    authorityAtRequest: existing?.admission.streamId ?? null,
  };
  _resumeRequestGenerations[generationKey] = generation;
  ws.send(JSON.stringify({
    type: 'resume_stream',
    sessionPath: path,
    ...(sessionId ? { sessionId } : {}),
    streamId,
    sinceSeq,
    resumeToken: generation.token,
  }));
}

// ── 流恢复 / 重建 ──

function nextResumeRebuildVersion(target: ResolvedStreamSession): number {
  const key = target.key || target.sessionPath;
  if (!key) return 0;
  const next = (_streamResumeRebuildVersions[key] ?? 0) + 1;
  _streamResumeRebuildVersions[key] = next;
  if (target.sessionPath && key !== target.sessionPath) delete _streamResumeRebuildVersions[target.sessionPath];
  return next;
}

function isLatestResumeRebuild(target: ResolvedStreamSession, version: number): boolean {
  const key = target.key || target.sessionPath;
  return !!key && _streamResumeRebuildVersions[key] === version;
}

function shouldHydrateCompletedEmptyResume(msg: any): boolean {
  if (msg.isStreaming) return false;
  if (!msg.streamId) return false;
  if (Array.isArray(msg.events) && msg.events.length > 0) return false;
  return Number.isFinite(msg.nextSeq) && msg.nextSeq > 1;
}

function resolveRuntimeStreaming(msg: any): boolean {
  return typeof msg.runtimeIsStreaming === 'boolean'
    ? msg.runtimeIsStreaming
    : !!msg.isStreaming;
}

function shouldForceApplyRuntimeStreamingStatus(msg: any): boolean {
  return msg?.runtimeIsStreaming === false;
}

function prepareStreamMeta(sessionRef: StreamSessionInput, streamId: string | null, opts: { resetConsumed?: boolean } = {}): SessionStreamMeta | null {
  const meta = getSessionStreamMeta(sessionRef);
  if (!meta) return null;
  const authorityBefore = meta.admission.streamId;
  if (streamId) {
    if (meta.streamId && meta.streamId !== streamId) {
      meta.lastSeq = 0;
      meta.consumedSeqs.clear();
      meta.admissionEpoch += 1;
    }
    meta.streamId = streamId;
  }
  if (opts.resetConsumed) {
    meta.admission.restore(streamId);
    meta.lastSeq = 0;
    meta.consumedSeqs.clear();
    meta.admissionEpoch += 1;
  }
  if (meta.admission.streamId !== authorityBefore) meta.authorityEpoch = meta.admissionEpoch;
  return meta;
}

function markConsumedSeq(meta: SessionStreamMeta, seq: unknown): void {
  const value = Number(seq);
  if (!Number.isFinite(value)) return;
  const normalized = Math.max(0, Math.floor(value));
  meta.lastSeq = Math.max(meta.lastSeq || 0, normalized);
  meta.consumedSeqs.add(normalized);
  pruneConsumedSeqs(meta);
}

function pruneConsumedSeqs(meta: SessionStreamMeta): void {
  if (meta.consumedSeqs.size <= MAX_CONSUMED_SEQS) return;
  const sorted = [...meta.consumedSeqs].sort((a, b) => a - b);
  const removeCount = meta.consumedSeqs.size - MAX_CONSUMED_SEQS;
  for (let i = 0; i < removeCount; i += 1) {
    meta.consumedSeqs.delete(sorted[i]);
  }
}

function dispatchReplayEvent(sessionPath: string, streamId: string | null, entry: any, meta: SessionStreamMeta | null): void {
  const seq = Number.isFinite(entry?.seq) ? Math.max(0, Math.floor(Number(entry.seq))) : null;
  if (seq !== null && meta?.consumedSeqs.has(seq)) return;

  const consumed = _handleServerMessage?.({
    ...entry.event,
    sessionPath,
    streamId,
    seq: entry.seq,
    __fromReplay: true,
  });

  // 只有真实接纳/消费（或非水位管辖帧）才推进水位；分发层返回 false 表示该帧被
  // 接纳门禁拒绝，不得把“函数已经调用”当成“帧已消费”而虚增 consumedSeqs。
  if (seq !== null && meta && consumed !== false) {
    markConsumedSeq(meta, seq);
  }
}

async function rebuildSessionFromResume(msg: any, opts: { finishTurnBeforeHydrate?: boolean } = {}): Promise<void> {
  const target = resolveStreamSession(msg);
  const sessionPath = target.sessionPath;
  if (!sessionPath) return;

  const snapshot = getSessionStreamMeta(target);
  const previousAdmission = snapshot?.admission ?? null;
  const isCurrentSession = target.isCurrent;
  const myVersion = nextResumeRebuildVersion(target);
  if (isCurrentSession) _streamResumeRebuildingFor = sessionPath;
  try {
    if (opts.finishTurnBeforeHydrate) {
      streamBufferManager.finishRun(sessionPath);
    } else {
      // 清掉旧 buffer 防止脏写
      streamBufferManager.clear(sessionPath);
    }

    if (isCurrentSession) {
      clearChat();
    } else {
      useStore.getState().clearSession?.(sessionPath);
    }
    // clearSession 会使水位失效，但不能遗忘已确认的旧流身份；并发恢复共享此屏障。
    if (previousAdmission) {
      const pending = getSessionStreamMeta(target);
      if (pending) pending.admission = previousAdmission;
    }
    // 恢复代次：以“进入 await 时存活的元数据对象”为基准。等待期间同一对象上
    // 发生过真实接纳提交（后台会话经真实 WS 接纳的新 run/stream、增量重放等）
    // 时，本次旧恢复整体放弃——不写身份、消息或终态，新流保持权威；
    // restore(oldStream) 不得把新流退休。对象被失效重建属于 hydrate/LRU 生命
    // 周期，由上方 previousAdmission 屏障按既有语义恢复身份连续性。
    const entryAtAwait = getSessionStreamMeta(target);
    const epochAtAwait = entryAtAwait?.admissionEpoch ?? 0;
    await loadMessages(sessionPath);

    if (!isLatestResumeRebuild(target, myVersion)) return;
    if (isCurrentSession && !isStillCurrentStreamSession(target)) return;
    const current = peekSessionStreamMeta(target);
    if (current && current === entryAtAwait && current.admissionEpoch !== epochAtAwait) return;

    const streamId = msg.streamId || null;
    if (previousAdmission) {
      const existing = getSessionStreamMeta(target);
      if (existing) existing.admission = previousAdmission;
    }
    const meta = prepareStreamMeta(target, streamId, { resetConsumed: true });

    for (const entry of msg.events || []) {
      dispatchReplayEvent(sessionPath, streamId, entry, meta);
    }

    if (meta && Number.isFinite(msg.nextSeq)) {
      meta.lastSeq = Math.max(meta.lastSeq || 0, Math.max(0, msg.nextSeq - 1));
    }

    _applyStreamingStatus?.(resolveRuntimeStreaming(msg), sessionPath, {
      streamId: msg.streamId || null,
    }, { force: shouldForceApplyRuntimeStreamingStatus(msg) });

    const ws = _getWebSocket?.() || null;
    if (isCurrentSession && isStillCurrentStreamSession(target) && ws?.readyState === WebSocket.OPEN && msg.isStreaming) {
      requestStreamResume(target);
    }
  } finally {
    if (isLatestResumeRebuild(target, myVersion) && _streamResumeRebuildingFor === sessionPath) {
      _streamResumeRebuildingFor = null;
    }
  }
}

/**
 * 增量重放信任门槛：只有当响应 streamId 与本地元数据一致、且断点 sinceSeq 是
 * 本地真实消费过的 seq 时，才允许把重放事件叠加到现有渲染状态上。
 * 例外：sinceSeq===0 的全量补发只要求本地处于空白状态（lastSeq===0），因为它
 * 覆盖流从头到尾的完整窗口。元数据一旦被 invalidate / 劈叉（半路刷新、LRU
 * 淘汰、分支重置、streamId 变更），consumedSeqs 与已渲染 buffer 不再互证——
 * 此时任何增量叠加都可能重复工具卡或把正文拼接两遍，必须整段重建。
 */
export function canApplyIncrementalResume(msg: any, target: StreamSessionInput): boolean {
  const meta = getSessionStreamMeta(target);
  if (!meta) return false;
  const sinceSeq = Number.isFinite(msg?.sinceSeq)
    ? Math.max(0, Math.floor(Number(msg.sinceSeq)))
    : null;
  if (sinceSeq === null) return false;
  if (sinceSeq === 0) return meta.lastSeq === 0;
  const requestedStreamId = normalizeStreamString(msg?.streamId);
  if (!requestedStreamId || meta.streamId !== requestedStreamId) return false;
  return meta.consumedSeqs.has(sinceSeq);
}

export function replayStreamResume(msg: any): void {
  const target = resolveStreamSession(msg);
  const sessionPath = target.sessionPath;
  if (!sessionPath) return;

  // 旧请求的迟到响应也不能清空已切换到新流的投影。
  if (getSessionStreamMeta(target)?.admission.isRetiredStream(msg.streamId)) return;

  // 恢复代次关联：带 token 的响应若描述的流已被“请求发出之后才接纳的新流”取代，
  // 该响应早于新流存在，整体丢弃；本地权威未变或新权威早于请求时照常处理
  //（饱和恢复/普通缺口补发不受影响）。
  const generationKey = target.key || sessionPath;
  const generation = generationKey ? _resumeRequestGenerations[generationKey] : null;
  if (generation && typeof msg.resumeToken === 'string' && msg.resumeToken === generation.token) {
    const meta = peekSessionStreamMeta(target);
    const authorityNow = meta?.admission.streamId ?? null;
    const responseStream = normalizeStreamString(msg.streamId);
    if (authorityNow
      && responseStream !== authorityNow
      && (meta?.authorityEpoch ?? 0) > generation.epochAtRequest) {
      return;
    }
  }

  const completedEmptyResume = shouldHydrateCompletedEmptyResume(msg);
  const replayEvents = Array.isArray(msg.events) ? msg.events : [];

  if (!msg.reset && !msg.truncated && !completedEmptyResume && replayEvents.length === 0) {
    // 空增量：没有可叠加内容，重建只会白付一次 hydrate；只需同步运行态
    // （含 runtimeIsStreaming=false 时的强制清流清理）。
    const streamId = msg.streamId || null;
    const meta = prepareStreamMeta(target, streamId);
    if (meta && Number.isFinite(msg.nextSeq)) {
      meta.lastSeq = Math.max(meta.lastSeq || 0, Math.max(0, msg.nextSeq - 1));
    }
    _applyStreamingStatus?.(resolveRuntimeStreaming(msg), sessionPath, {
      streamId,
    }, { force: shouldForceApplyRuntimeStreamingStatus(msg) });
    return;
  }

  if (msg.reset || msg.truncated || completedEmptyResume || !canApplyIncrementalResume(msg, target)) {
    rebuildSessionFromResume(msg, { finishTurnBeforeHydrate: completedEmptyResume }).catch((err) => {
      console.error('[stream] rebuild failed:', err);
      _streamResumeRebuildingFor = null;
    });
    return;
  }

  const streamId = msg.streamId || null;
  const meta = prepareStreamMeta(target, streamId);

  for (const entry of msg.events || []) {
    dispatchReplayEvent(sessionPath, streamId, entry, meta);
  }

  if (meta && Number.isFinite(msg.nextSeq)) {
    meta.lastSeq = Math.max(meta.lastSeq || 0, Math.max(0, msg.nextSeq - 1));
  }

  _applyStreamingStatus?.(resolveRuntimeStreaming(msg), sessionPath, {
    streamId: msg.streamId || null,
  }, { force: shouldForceApplyRuntimeStreamingStatus(msg) });
}

registerStreamResumeMetaInvalidator((sessionPath) => {
  invalidateSessionStreamMeta(sessionPath);
});
