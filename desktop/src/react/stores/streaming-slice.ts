import { sessionScopedKey } from './session-slice';
import type { PresentedError } from '../errors/error-presenter';

/**
 * 一条 inline error：text 是给用户看的人话，detail 与 code 是排障用的原始信息，
 * 由错误条的展开区呈现。形状跟 presentError() 的产物一致，两端不各自定义。
 */
export type InlineErrorEntry = PresentedError;

export interface ActiveSessionStream {
  streamId: string | null;
  turnId: string | null;
}

export interface StreamingStatusIdentity {
  streamId?: string | null;
  turnId?: string | null;
}

/**
 * 循环任务的实时状态（按 sessionId 存）。来自 loop_status WS 推送 + 会话列表冷启动注入，
 * 驱动会话列表循环徽章与 interlude 气泡上的控制按钮态。
 */
export type LoopStatus = {
  status: 'running' | 'paused' | 'stopped' | 'completed';
  turnCount: number;
  maxTurns: number | null;
  pausedReason: string | null;
  prompt: string | null;
};

/** 知识注入过程行（服务器只发元数据：查询词/命中数/方向名，无模型输出）。 */
export interface KnowledgeTraceEntry {
  id: string;
  kind: 'think' | 'search' | 'read' | 'note';
  phase: 'start' | 'done' | 'failed';
  query?: string;
  hits?: number;
  current?: number;
  total?: number;
  queries?: string[];
  detail?: string | null;
}

export interface StreamingSlice {
  /** 所有正在 streaming 的 session identity key 集合（legacy path 只做兼容 locator） */
  streamingSessions: string[];
  /** 正在 streaming 的 session 身份。用于忽略旧 turn/status 迟到事件。 */
  activeSessionStreams: Record<string, ActiveSessionStream>;
  addStreamingSession: (path: string, identity?: StreamingStatusIdentity) => void;
  removeStreamingSession: (path: string, identity?: StreamingStatusIdentity) => boolean;
  forceRemoveStreamingSession: (path: string) => boolean;
  /**
   * 知识库检索进行中的 session（ws knowledge_retrieval_started → 后续任意事件清除）。
   * 纯瞬态：不落盘、不进 stream_resume，只驱动「知识库检索中」胶囊。
   */
  knowledgeRetrievingSessions: string[];
  beginKnowledgeRetrieval: (path: string) => void;
  endKnowledgeRetrieval: (path: string) => void;
  /**
   * 知识蒸馏进行中的 session（ws knowledge_distill_progress 逐批更新 →
   * 该 session 任意后续事件清除）。纯瞬态：驱动「蒸馏中 · N 批」胶囊。
   */
  /** 滚动注入进度（2026-08-31 取代蒸馏胶囊）：current/total 为「正在阅读第 X/N 部分」。 */
  knowledgeRollupBySession: Record<string, { current: number; total: number }>;
  updateKnowledgeRollup: (path: string, progress: { current: number; total: number }) => void;
  endKnowledgeRollup: (path: string) => void;
  /** 补充检索过程（滚动循环内模型自主发起）：queries 逐轮覆盖展示。 */
  knowledgeSupplementBySession: Record<string, { queries: string[]; round: number }>;
  updateKnowledgeSupplement: (path: string, progress: { queries: string[]; round: number }) => void;
  endKnowledgeSupplement: (path: string) => void;
  /**
   * 知识注入过程行堆（2026-08-31 二轮，对齐编程 Agent 的工具调用过程卡）：
   * knowledge_trace / knowledge_rollup_progress / knowledge_supplement_search
   * 事件按 id 原位更新成有序行（思考/检索/阅读/补充检索）。纯瞬态：不落盘、
   * 不进 stream_resume；该 session 首个非知识过程事件（session_user_message 等）
   * 保守清除——真实轮消息到达即整堆收起。
   */
  knowledgeTraceBySession: Record<string, KnowledgeTraceEntry[]>;
  upsertKnowledgeTrace: (path: string, entry: KnowledgeTraceEntry) => void;
  resetKnowledgeTrace: (path: string) => void;
  /**
   * 发送后本地进入「等待助手」态的 session（ws.send 成功 → 该 session 首个
   * 后续事件清除）。服务器在知识检索/排队期间不置 isStreaming，靠它保证
   * 发送瞬间就有 typing 指示器，不依赖任何服务器信号（旧 server 也生效）。
   * 纯瞬态：不落盘、不进 stream_resume、不做发送互斥（互斥仍归 streamingSessions）。
   */
  turnPendingSessions: string[];
  beginTurnPending: (path: string) => void;
  endTurnPending: (path: string) => void;
  /** ws 断连时全清 pending：没有后续事件会来清它，不清会挂出永久指示器。 */
  clearAllTurnPending: () => void;
  /** 后台 session 已完成新输出，但用户尚未切回查看。 */
  unreadOutputSessionPaths: string[];
  markSessionOutputUnread: (path: string) => void;
  clearSessionOutputUnread: (path: string) => void;
  /** 按 session path 存储的内联错误（权威源）。null 表示无 error。 */
  inlineErrors: Record<string, InlineErrorEntry | null>;
  /**
   * 写入某个 session 的 inline error；ttl>0 时 ttl 毫秒后自动清除（默认 5s）。
   * 新 error 覆盖旧 error 会取消旧定时器。传字符串表示这句话已经是给用户看的成品，
   * 没有额外详情；带 detail/code 的错误传 InlineErrorEntry。
   */
  setInlineError: (path: string, error: string | InlineErrorEntry, ttlMs?: number) => void;
  /** 清除某个 session 的 inline error（同时取消其定时器）。 */
  clearInlineError: (path: string) => void;
  /** 按 sessionId 存的循环任务状态（loop_status WS + 冷启动注入），驱动会话列表徽章与 interlude 按钮态。 */
  loopStatusBySession: Record<string, LoopStatus>;
  /** 写入/清除某会话的循环状态；传 null 或 stopped/completed 后续会清除，避免徽章残留。 */
  setLoopStatus: (sessionId: string, status: LoopStatus | null) => void;
  clearLoopStatus: (sessionId: string) => void;
  /** 模型切换进行中（阻止发送） */
  modelSwitching: boolean;
  setModelSwitching: (v: boolean) => void;
}

// 定时器按 session identity key 存在模块闭包里，不污染 store 的可见状态。
// 生命周期规则：
//   - setInlineError 覆盖写入时，先 clear 旧 timer 再起新的，避免"旧 timer 误清新 text"竞态
//   - clearInlineError 清状态时同步 clear timer，防 timer 在 null 写入后继续 fire
//   - timer 回调内部用 get() 取最新条目：若已被新 error 覆盖，引用不等于本次写入的条目，不动它。
//     用引用而非文本比较，同一句错误连续发生两次时旧定时器才不会把新条目清掉
const inlineErrorTimers = new Map<string, ReturnType<typeof setTimeout>>();

/** 把字符串或部分条目归一化成完整条目。总是新建对象，让每次写入拥有独一无二的引用。 */
function toInlineErrorEntry(error: string | InlineErrorEntry): InlineErrorEntry {
  if (typeof error === 'string') return { text: error, detail: null, code: null };
  return {
    text: error.text,
    detail: error.detail ?? null,
    code: error.code ?? null,
  };
}

function cancelTimer(path: string): void {
  const t = inlineErrorTimers.get(path);
  if (t) {
    clearTimeout(t);
    inlineErrorTimers.delete(path);
  }
}

function identityKeyForPath(get: (() => any) | undefined, path: string): string {
  return sessionScopedKey(get?.() || {}, path) || path;
}

function filterLegacyAndIdentity(list: readonly string[], path: string, key: string): string[] {
  return list.filter((item) => item !== key && item !== path);
}

function putIdentityMapValue<T>(map: Record<string, T>, path: string, key: string, value: T): Record<string, T> {
  const next = { ...map, [key]: value };
  if (key !== path) delete next[path];
  return next;
}

function deleteIdentityMapValue<T>(map: Record<string, T>, path: string, key: string): Record<string, T> {
  const next = { ...map };
  delete next[key];
  if (key !== path) delete next[path];
  return next;
}

function normalizeIdentityPart(value: unknown): string | null {
  return typeof value === 'string' && value.trim() ? value : null;
}

function hasExplicitIdentity(identity: StreamingStatusIdentity | undefined): boolean {
  return !!identity
    && (Object.prototype.hasOwnProperty.call(identity, 'streamId')
      || Object.prototype.hasOwnProperty.call(identity, 'turnId'));
}

function hasKnownIdentityPart(identity: ActiveSessionStream | StreamingStatusIdentity | undefined): boolean {
  return !!normalizeIdentityPart(identity?.streamId) || !!normalizeIdentityPart(identity?.turnId);
}

function identitiesMatch(
  current: ActiveSessionStream | undefined,
  incoming: StreamingStatusIdentity | undefined,
): boolean {
  if (!current || !hasKnownIdentityPart(current)) return true;
  const currentStreamId = normalizeIdentityPart(current.streamId);
  const incomingStreamId = normalizeIdentityPart(incoming?.streamId);
  let matchedKnownPart = false;
  if (currentStreamId && incomingStreamId) {
    if (currentStreamId !== incomingStreamId) return false;
    matchedKnownPart = true;
  }

  const currentTurnId = normalizeIdentityPart(current.turnId);
  const incomingTurnId = normalizeIdentityPart(incoming?.turnId);
  if (currentTurnId && incomingTurnId) {
    if (currentTurnId !== incomingTurnId) return false;
    matchedKnownPart = true;
  }

  return matchedKnownPart;
}

export const createStreamingSlice = (
  set: (partial: Partial<StreamingSlice> | ((s: StreamingSlice) => Partial<StreamingSlice>)) => void,
  get?: () => StreamingSlice,
): StreamingSlice => ({
  streamingSessions: [],
  activeSessionStreams: {},
  knowledgeRetrievingSessions: [],
  beginKnowledgeRetrieval: (path) => set((s) => {
    const key = identityKeyForPath(get, path);
    if (s.knowledgeRetrievingSessions.includes(key)
      || (key !== path && s.knowledgeRetrievingSessions.includes(path))) {
      return {};
    }
    return {
      knowledgeRetrievingSessions: [
        ...filterLegacyAndIdentity(s.knowledgeRetrievingSessions, path, key),
        key,
      ],
    };
  }),
  endKnowledgeRetrieval: (path) => {
    // 高频事件路径也调用这里：列表不含该 session 时直接返回，不产生 setState。
    const key = identityKeyForPath(get, path);
    const current = get?.().knowledgeRetrievingSessions ?? [];
    if (!current.includes(key) && !(key !== path && current.includes(path))) return;
    set((s) => ({
      knowledgeRetrievingSessions: filterLegacyAndIdentity(s.knowledgeRetrievingSessions, path, key),
    }));
  },
  knowledgeRollupBySession: {},
  updateKnowledgeRollup: (path, progress) => {
    const key = identityKeyForPath(get, path);
    const current = get?.();
    const existing = current?.knowledgeRollupBySession?.[key];
    if (existing && existing.current === progress.current && existing.total === progress.total) return;
    const base = filterLegacyAndIdentity(
      Object.keys(current?.knowledgeRollupBySession ?? {}),
      path,
      key,
    ).reduce<Record<string, { current: number; total: number }>>((acc, k) => {
      acc[k] = current!.knowledgeRollupBySession[k];
      return acc;
    }, {});
    base[key] = progress;
    set({ knowledgeRollupBySession: base });
  },
  endKnowledgeRollup: (path) => {
    const key = identityKeyForPath(get, path);
    const current = get?.();
    if (!current?.knowledgeRollupBySession?.[key] && !current?.knowledgeRollupBySession?.[path]) return;
    const kept = filterLegacyAndIdentity(
      Object.keys(current?.knowledgeRollupBySession ?? {}),
      path,
      key,
    ).reduce<Record<string, { current: number; total: number }>>((acc, k) => {
      acc[k] = current!.knowledgeRollupBySession[k];
      return acc;
    }, {});
    set({ knowledgeRollupBySession: kept });
  },
  knowledgeSupplementBySession: {},
  updateKnowledgeSupplement: (path, progress) => {
    const key = identityKeyForPath(get, path);
    const current = get?.();
    const existing = current?.knowledgeSupplementBySession?.[key];
    if (existing && existing.round === progress.round
      && existing.queries.join('\n') === progress.queries.join('\n')) return;
    const base = filterLegacyAndIdentity(
      Object.keys(current?.knowledgeSupplementBySession ?? {}),
      path,
      key,
    ).reduce<Record<string, { queries: string[]; round: number }>>((acc, k) => {
      acc[k] = current!.knowledgeSupplementBySession[k];
      return acc;
    }, {});
    base[key] = progress;
    set({ knowledgeSupplementBySession: base });
  },
  endKnowledgeSupplement: (path) => {
    const key = identityKeyForPath(get, path);
    const current = get?.();
    if (!current?.knowledgeSupplementBySession?.[key] && !current?.knowledgeSupplementBySession?.[path]) return;
    const kept = filterLegacyAndIdentity(
      Object.keys(current?.knowledgeSupplementBySession ?? {}),
      path,
      key,
    ).reduce<Record<string, { queries: string[]; round: number }>>((acc, k) => {
      acc[k] = current!.knowledgeSupplementBySession[k];
      return acc;
    }, {});
    set({ knowledgeSupplementBySession: kept });
  },
  knowledgeTraceBySession: {},
  upsertKnowledgeTrace: (path, entry) => {
    const key = identityKeyForPath(get, path);
    const current = get?.().knowledgeTraceBySession ?? {};
    const list = current[key] ?? [];
    const index = list.findIndex(item => item.id === entry.id);
    const next = index >= 0
      ? list.map((item, i) => (i === index ? { ...item, ...entry } : item))
      : [...list, entry];
    set({ knowledgeTraceBySession: { ...current, [key]: next } });
  },
  resetKnowledgeTrace: (path) => {
    const key = identityKeyForPath(get, path);
    const current = get?.().knowledgeTraceBySession ?? {};
    if (!current[key] || current[key].length === 0) return;
    const kept = { ...current };
    delete kept[key];
    set({ knowledgeTraceBySession: kept });
  },
  turnPendingSessions: [],
  beginTurnPending: (path) => set((s) => {
    const key = identityKeyForPath(get, path);
    if (s.turnPendingSessions.includes(key)
      || (key !== path && s.turnPendingSessions.includes(path))) {
      return {};
    }
    return {
      turnPendingSessions: [
        ...filterLegacyAndIdentity(s.turnPendingSessions, path, key),
        key,
      ],
    };
  }),
  endTurnPending: (path) => {
    // 与 endKnowledgeRetrieval 同一高频清除点调用：未命中直接返回，零 setState。
    const key = identityKeyForPath(get, path);
    const current = get?.().turnPendingSessions ?? [];
    if (!current.includes(key) && !(key !== path && current.includes(path))) return;
    set((s) => ({
      turnPendingSessions: filterLegacyAndIdentity(s.turnPendingSessions, path, key),
    }));
  },
  clearAllTurnPending: () => set((s) => (
    s.turnPendingSessions.length === 0 ? {} : { turnPendingSessions: [] }
  )),
  addStreamingSession: (path, identity) => set((s) => {
    const key = identityKeyForPath(get, path);
    const active = s.activeSessionStreams || {};
    const current = active[key] || active[path];
    const currentStreamId = normalizeIdentityPart(current?.streamId);
    const currentTurnId = normalizeIdentityPart(current?.turnId);
    const incomingStreamId = normalizeIdentityPart(identity?.streamId);
    const incomingTurnId = normalizeIdentityPart(identity?.turnId);
    const explicitIdentity = hasExplicitIdentity(identity);
    const streamChanged = !!incomingStreamId && incomingStreamId !== currentStreamId;
    const streamingSessions = filterLegacyAndIdentity(s.streamingSessions, path, key);
    return {
      streamingSessions: [...streamingSessions, key],
      activeSessionStreams: putIdentityMapValue(active, path, key, {
          streamId: explicitIdentity
            ? (incomingStreamId ?? currentStreamId ?? null)
            : (currentStreamId ?? null),
          turnId: explicitIdentity
            ? (incomingTurnId ?? (streamChanged ? null : currentTurnId) ?? null)
            : (currentTurnId ?? null),
      }),
    };
  }),
  removeStreamingSession: (path, identity) => {
    let applied = true;
    set((s) => {
      const key = identityKeyForPath(get, path);
      const active = s.activeSessionStreams || {};
      if (!identitiesMatch(active[key] || active[path], identity)) {
        applied = false;
        return {};
      }
      return {
        streamingSessions: filterLegacyAndIdentity(s.streamingSessions, path, key),
        activeSessionStreams: deleteIdentityMapValue(active, path, key),
      };
    });
    return applied;
  },
  forceRemoveStreamingSession: (path) => {
    let applied = false;
    set((s) => {
      const key = identityKeyForPath(get, path);
      const active = s.activeSessionStreams || {};
      const hadSession = s.streamingSessions.includes(key) || (key !== path && s.streamingSessions.includes(path));
      const hadIdentity = Object.prototype.hasOwnProperty.call(active, key)
        || (key !== path && Object.prototype.hasOwnProperty.call(active, path));
      if (!hadSession && !hadIdentity) return {};
      applied = hadSession || hadIdentity;
      return {
        streamingSessions: filterLegacyAndIdentity(s.streamingSessions, path, key),
        activeSessionStreams: deleteIdentityMapValue(active, path, key),
      };
    });
    return applied;
  },
  unreadOutputSessionPaths: [],
  markSessionOutputUnread: (path) => set((s) => {
    const key = identityKeyForPath(get, path);
    const unread = filterLegacyAndIdentity(s.unreadOutputSessionPaths, path, key);
    return { unreadOutputSessionPaths: [...unread, key] };
  }),
  clearSessionOutputUnread: (path) => set((s) => {
    const key = identityKeyForPath(get, path);
    return { unreadOutputSessionPaths: filterLegacyAndIdentity(s.unreadOutputSessionPaths, path, key) };
  }),
  inlineErrors: {},
  setInlineError: (path, error, ttlMs = 5000) => {
    const key = identityKeyForPath(get, path);
    const entry = toInlineErrorEntry(error);
    cancelTimer(key);
    if (key !== path) cancelTimer(path);
    set((s) => ({ inlineErrors: putIdentityMapValue(s.inlineErrors, path, key, entry) }));
    if (ttlMs > 0) {
      const timer = setTimeout(() => {
        inlineErrorTimers.delete(key);
        const current = get?.().inlineErrors[key];
        if (current !== entry) return;
        set((s) => ({ inlineErrors: putIdentityMapValue(s.inlineErrors, path, key, null) }));
      }, ttlMs);
      inlineErrorTimers.set(key, timer);
    }
  },
  clearInlineError: (path) => {
    const key = identityKeyForPath(get, path);
    cancelTimer(key);
    if (key !== path) cancelTimer(path);
    set((s) => ({ inlineErrors: putIdentityMapValue(s.inlineErrors, path, key, null) }));
  },
  loopStatusBySession: {},
  setLoopStatus: (sessionId, status) => set((s) => {
    if (!sessionId) return {};
    const next = { ...s.loopStatusBySession };
    // running/paused 才常驻；stopped/completed/null 一律清除，让徽章与按钮立即消失。
    if (status && (status.status === 'running' || status.status === 'paused')) {
      next[sessionId] = status;
    } else {
      delete next[sessionId];
    }
    return { loopStatusBySession: next };
  }),
  clearLoopStatus: (sessionId) => set((s) => {
    if (!sessionId || !Object.prototype.hasOwnProperty.call(s.loopStatusBySession, sessionId)) return {};
    const next = { ...s.loopStatusBySession };
    delete next[sessionId];
    return { loopStatusBySession: next };
  }),
  modelSwitching: false,
  setModelSwitching: (v) => set({ modelSwitching: v }),
});
