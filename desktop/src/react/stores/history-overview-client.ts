/**
 * history-overview-client.ts — E05 概览客户端接线（非阻塞建议数据）。
 *
 * 契约：TASKBOOK E04/E05 + protocol-contract.md。概览永不阻塞首屏：
 * 触发点在「首个消息请求成功且页面已显示」之后（ChatMessageBadge effect），
 * 失败/慢响应不影响消息渲染；同一连接同一会话的在途请求合并；不轮询不预取。
 *
 * 能力/失败隔离（E05）：404/405/未知协议/结构不兼容 → 当前连接+认证 epoch 记为
 * 不支持（回退既有「翻页探底」）；401/403 → 清理失效统计、保留鉴权流程，不标为
 * 能力缺席；一般网络错误 → 保留旧状态（不弹错）。epoch 变化（换 server/认证）
 * 后可重新探测，不永久写全局配置。
 *
 * stale 丢弃（E05）：请求时捕获 generation（currentSessionPath/_loadMessagesVersion/
 * messageLiveVersion/revision），响应到达时已切换/分支重置/版本过期 → 丢弃，
 * 更新的消息状态绝不被更旧概览反向覆盖；概览不 stamp 消息缓存、不判定对账 complete。
 */
import { useStore } from './index';
import { lingxiFetch } from '../hooks/use-hana-fetch';
import { readMessageLiveVersion } from './message-live-version';

export type HistoryTaskCategoryName = 'subagent' | 'workflow' | 'media' | 'other';

export interface HistoryOverviewSnapshot {
  status: 'idle' | 'loading' | 'available' | 'unavailable' | 'unsupported' | 'error';
  /** available 时为安全化后的只读概览数据；其余为 null。 */
  data: {
    displayRecords: number;
    sourceRecords: number;
    runsWithAssistant: number;
    referencedTasks: number;
    runSizeDistribution: { oneTo50: number; from51To200: number; over200: number };
    taskDistribution: { subagent: number; workflow: number; media: number; other: number };
    revision: string;
    recommendedLimit: number;
    estimatedPagesAtRecommendedLimit: number;
  } | null;
  /** available:false 时的原因（revision_unknown/directory_unavailable/unsupported_history）。 */
  unavailableReason: string | null;
}

const snapshots = new Map<string, HistoryOverviewSnapshot>();
/** 稳定的 idle 快照（useSyncExternalStore 要求 getSnapshot 引用稳定，避免无限重渲）。 */
const IDLE_SNAPSHOT: HistoryOverviewSnapshot = Object.freeze({
  status: 'idle' as const,
  data: null,
  unavailableReason: null,
});
const inFlight = new Map<string, Promise<void>>();
const unsupportedEpochs = new Set<string>();
const listeners = new Set<() => void>();

function snapshotFor(sessionPath: string): HistoryOverviewSnapshot {
  return snapshots.get(sessionPath) ?? IDLE_SNAPSHOT;
}

function setSnapshot(sessionPath: string, next: HistoryOverviewSnapshot): void {
  snapshots.set(sessionPath, next);
  for (const notify of listeners) notify();
}

/** 测试辅助：清空全部概览状态。 */
export function __clearHistoryOverviewStateForTest(): void {
  snapshots.clear();
  inFlight.clear();
  unsupportedEpochs.clear();
  listeners.clear();
}

/** React useSyncExternalStore 订阅接口。 */
export function subscribeHistoryOverview(notify: () => void): () => void {
  listeners.add(notify);
  return () => listeners.delete(notify);
}

export function getHistoryOverviewSnapshot(sessionPath: string): HistoryOverviewSnapshot {
  return snapshotFor(sessionPath);
}

// ── schema/约束校验（E05：坏数据拒绝，不静默截断） ────────────────────────

function isCount(v: unknown): v is number {
  return typeof v === 'number' && Number.isFinite(v) && Number.isSafeInteger(v) && v >= 0;
}

function isCountObject(v: unknown, keys: string[]): v is Record<string, number> {
  if (!v || typeof v !== 'object') return false;
  for (const k of keys) if (!isCount((v as Record<string, unknown>)[k])) return false;
  return true;
}

const COUNT_KEYS = ['oneTo50', 'from51To200', 'over200'] as const;
const TASK_KEYS = ['subagent', 'workflow', 'media', 'other'] as const;

/** 严格校验概览响应；结构/约束不符返回 null（调用方按不支持处理）。 */
export function validateOverviewPayload(data: unknown): HistoryOverviewSnapshot['data'] | 'unavailable' | null {
  if (!data || typeof data !== 'object') return null;
  const d = data as Record<string, any>;
  if (d.schemaVersion !== 1) return null;
  if (d.available === false) {
    const reasons = ['revision_unknown', 'directory_unavailable', 'unsupported_history'];
    return reasons.includes(d.reason) ? 'unavailable' : null;
  }
  if (d.available !== true) return null;
  if (!isCountObject(d.counts ?? {}, ['displayRecords', 'sourceRecords', 'runsWithAssistant', 'referencedTasks'])) return null;
  if (!isCountObject(d.runSizeDistribution ?? {}, [...COUNT_KEYS])) return null;
  if (!isCountObject(d.taskDistribution ?? {}, [...TASK_KEYS])) return null;
  if (typeof d.revision !== 'string' || !d.revision) return null;
  const pagination = d.pagination ?? {};
  if (pagination.legacyDefaultLimit !== 50 || pagination.maxLimit !== 200) return null;
  if (!isCount(pagination.recommendedLimit) || !isCount(pagination.estimatedPagesAtRecommendedLimit)) return null;
  const counts = d.counts as Record<string, number>;
  const dist = d.runSizeDistribution as Record<string, number>;
  const tasks = d.taskDistribution as Record<string, number>;
  const bucketSum = dist.oneTo50 + dist.from51To200 + dist.over200;
  if (bucketSum !== counts.runsWithAssistant) return null; // 三桶合计必须等于轮次数
  const taskSum = tasks.subagent + tasks.workflow + tasks.media + tasks.other;
  if (taskSum !== counts.referencedTasks) return null; // 分类合计必须等于关联任务数
  return {
    displayRecords: counts.displayRecords,
    sourceRecords: counts.sourceRecords,
    runsWithAssistant: counts.runsWithAssistant,
    referencedTasks: counts.referencedTasks,
    runSizeDistribution: { oneTo50: dist.oneTo50, from51To200: dist.from51To200, over200: dist.over200 },
    taskDistribution: { subagent: tasks.subagent, workflow: tasks.workflow, media: tasks.media, other: tasks.other },
    revision: d.revision,
    recommendedLimit: pagination.recommendedLimit,
    estimatedPagesAtRecommendedLimit: pagination.estimatedPagesAtRecommendedLimit,
  };
}

// ── 触发与合并（E05：非阻塞；同连接同会话在途合并；能力按 epoch 隔离） ────────

/**
 * 非阻塞请求一次概览（fire-and-forget）。合并同连接同会话在途请求；
 * 404/405/结构不兼容 → 当前 epoch 记为不支持（回退翻页探底）；
 * 401/403 → 清理失效统计、不标记；available:false → 显示未知（不伪造计数）。
 */
export function requestHistoryOverview(sessionPath: string, sessionId: string | null = null): Promise<void> {
  const existing = inFlight.get(sessionPath);
  if (existing) return existing;
  const task = (async () => {
    let state: any;
    try {
      state = useStore.getState() as Record<string, any>;
      if (state.pendingNewSession || state.pendingSessionSwitchPath) return;
      if (state.currentSessionPath != null && state.currentSessionPath !== sessionPath) return;
      if ((state.streamingSessions ?? []).includes(sessionPath)) return; // 流式中保守不发
    } catch {
      return;
    }
    const epochFingerprint = `${state.serverPort ?? ''}:${state.serverToken ?? ''}:${state.activeServerConnectionId ?? ''}`;
    if (unsupportedEpochs.has(epochFingerprint)) return;

    setSnapshot(sessionPath, { status: 'loading', data: null, unavailableReason: null });
    const capturedLoadVersion = (state._loadMessagesVersion as Record<string, any> | undefined)?.[sessionPath] ?? 0;
    const capturedLiveVersion = readMessageLiveVersion(sessionPath);
    const cached = ((state.chatSessions as Record<string, any> | undefined)?.[sessionPath] ?? null) as any;
    const capturedRevision = typeof cached?.revision === 'string' ? cached.revision : null;

    try {
      const params = new URLSearchParams({ path: sessionPath });
      const sid = sessionId ?? (state.sessions as any[] | undefined)?.find?.((s) => s?.path === sessionPath)?.sessionId ?? null;
      if (sid) params.set('sessionId', String(sid));
      const res = await lingxiFetch(`/api/sessions/history-overview?${params.toString()}`, {
        cache: 'no-store',
        throwOnHttpError: false,
      });
      const status = typeof res?.status === 'number' ? res.status : 0;
      if (status === 404 || status === 405) {
        unsupportedEpochs.add(epochFingerprint); // 回退既有翻页探底，不弹持续错误
        setSnapshot(sessionPath, { status: 'unsupported', data: null, unavailableReason: null });
        return;
      }
      if (status === 401 || status === 403) {
        // 保留鉴权流程并清理失效统计，不标为普通能力缺席
        snapshots.delete(sessionPath);
        for (const notify of listeners) notify();
        return;
      }
      if (status === 0) return; // 网络/传输失败：保留旧状态，不弹错
      if (status !== 200) {
        unsupportedEpochs.add(epochFingerprint);
        setSnapshot(sessionPath, { status: 'unsupported', data: null, unavailableReason: null });
        return;
      }

      // stale 丢弃：到达时已切换/流式/版本过期 → 丢弃（不更新快照）
      const nowState = useStore.getState() as Record<string, any>;
      if (nowState.currentSessionPath != null && nowState.currentSessionPath !== sessionPath) return;
      if ((nowState.streamingSessions ?? []).includes(sessionPath)) return;
      const nowLoadVersion = (nowState._loadMessagesVersion as Record<string, any> | undefined)?.[sessionPath] ?? 0;
      if (nowLoadVersion !== capturedLoadVersion) return;
      if (readMessageLiveVersion(sessionPath) !== capturedLiveVersion) return;
      const nowCached = ((nowState.chatSessions as Record<string, any> | undefined)?.[sessionPath] ?? null) as any;
      if (!nowCached) return;
      // 更新的消息 revision 已被观察到 → 旧 generation 概览不得反向覆盖显示
      if (capturedRevision != null && typeof nowCached.revision === 'string' && nowCached.revision !== capturedRevision) return;

      const raw = await res.json();
      const validated = validateOverviewPayload(raw);
      if (validated === null) {
        unsupportedEpochs.add(epochFingerprint); // 结构不兼容
        setSnapshot(sessionPath, { status: 'unsupported', data: null, unavailableReason: null });
        return;
      }
      if (validated === 'unavailable') {
        setSnapshot(sessionPath, {
          status: 'unavailable',
          data: null,
          unavailableReason: (raw as any).reason ?? 'directory_unavailable',
        });
        return;
      }
      // 应用安全化数据
      setSnapshot(sessionPath, { status: 'available', data: validated, unavailableReason: null });
    } catch {
      // 概览失败不影响消息功能：保留旧状态（若首请求失败回到 idle，可再次触发）
      if (!snapshots.has(sessionPath)) setSnapshot(sessionPath, { status: 'idle', data: null, unavailableReason: null });
    } finally {
      inFlight.delete(sessionPath);
    }
  })();
  inFlight.set(sessionPath, task);
  return task;
}
