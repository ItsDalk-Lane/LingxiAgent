/**
 * composer-send-coordinator.ts — 输入发送的会话级租约与队列调度（F1/F2）。
 *
 * 模块级单例（不随 InputArea 卸载/重挂载丢失）：
 * - 每个会话同一时刻只有一个在途发送租约（preparing → awaiting_ack）；
 *   租约在第一个 await 之前同步占用，记录 leaseId/会话身份/连接代次/快照版本。
 * - 回执（session_user_message 的 clientMessageId）到达 → accepted 并释放
 *   传输互斥；断线/回执超时 → delivery_unknown，禁止自动重发。
 * - 回执之后的 run 串行由既有 running 态门禁（streamingSessions/
 *   turnPendingSessions，assistant run 事件驱动）负责，两道门禁互相独立。
 * - 队列项是快照持有者：prepare 前不删除；失败原位标 blocked/failed；
 *   编辑/删除会取消在途准备，迟到的结果不得发送。
 *
 * 它不是新的服务端消息系统：传输仍走既有 ws 信封，回执仍走既有
 * session_user_message 关联。
 */

import { useStore } from '../stores';
import { sessionScopedKey, sessionScopedListIncludes, sessionScopedValue } from '../stores/session-slice';
import { isSessionCompacting } from '../stores/context-slice';
import type { ComposerSendBundle, QueuedTurnInput } from '../stores/chat-types';
import {
  commitPreparedComposerSend,
  createClientUserMessageId,
  prepareComposerSend,
  type ComposerDispatchResult,
  type ComposerPrepareOutcome,
  type ComposerPreSubmitResult,
  type ComposerSendOptions,
  type PreparedComposerSend,
} from '../components/input/composer-send';

// ── 类型 ──

export type ComposerSessionIdentity =
  | { kind: 'session'; sessionId: string; sessionPath: string; agentId: string | null }
  | { kind: 'pending_draft'; draftId: string };

export type ComposerSendPhase =
  | 'preparing'
  | 'awaiting_ack'
  | 'accepted'
  | 'blocked'
  | 'failed_before_submit'
  | 'delivery_unknown';

export interface ComposerSendRecord {
  leaseId: string;
  clientMessageId: string;
  queueItemId: string | null;
  identity: ComposerSessionIdentity;
  /** 取得租约时的连接代次；代次不匹配的准备任务禁止向新连接发送旧载荷。 */
  connectionGeneration: number;
  snapshotVersion: number;
  composerRevisionAtClick: number | null;
  bundle: ComposerSendBundle;
  /** 立即插入的目标 run 身份（streamId/turnId），commit 时复核。 */
  targetRun: { streamId: string | null; turnId: string | null } | null;
  phase: ComposerSendPhase;
  code: string | null;
  retryable: boolean;
  cancelled: boolean;
  /**
   * 身份刚由 ensureSession 创建（transferLeaseIdentity 之后）：sessions[] 投影
   * 可能尚未回流，revalidate 不得把「投影缺席」误判为会话已删除。
   */
  identityFreshlyCreated?: boolean;
  ackTimer: ReturnType<typeof setTimeout> | null;
}

export interface AcquireSendLeaseInput {
  identity: ComposerSessionIdentity;
  bundle: ComposerSendBundle;
  queueItemId?: string | null;
  snapshotVersion?: number;
  composerRevisionAtClick?: number | null;
  targetRun?: { streamId: string | null; turnId: string | null } | null;
}

export type AcquireSendLeaseResult =
  | { ok: true; leaseId: string }
  | { ok: false; reason: 'transport_busy' };

export interface ComposerSendFlowDeps extends ComposerSendOptions {
  /** 复核通过后、乐观消息创建前的同步钩子（InputArea 的输入清理）。 */
  onCommit?: (() => void) | null;
}

export interface QueueFlushDeps extends ComposerSendOptions {
  /** 返回 true 时跳过该项（例如正在编辑中的队列项不得被自动取走）。 */
  shouldSkipItem?: (item: QueuedTurnInput) => boolean;
}

export type QueuedInsertNowAction = 'interject' | 'prompt' | 'wait' | 'review_forbidden';

// ── 模块状态 ──

/** 回执看门狗：transport_submitted 后等待 session_user_message 关联回执。 */
const ACK_TIMEOUT_MS = 15_000;
/** 队列自动续发的视觉缓冲；计时结束后仍重新验证状态并同步取得租约。 */
const QUEUE_FLUSH_DEBOUNCE_MS = 400;
const MAX_RETAINED_RECORDS = 200;

const recordsByLease = new Map<string, ComposerSendRecord>();
const activeLeaseByKey = new Map<string, string>();
const pendingFlushBySession = new Map<string, { timer: ReturnType<typeof setTimeout>; deps: QueueFlushDeps }>();
let connectionGeneration = 0;
let leaseSeq = 0;

function identityKey(identity: ComposerSessionIdentity): string {
  return identity.kind === 'session' ? `session:${identity.sessionPath}` : `draft:${identity.draftId}`;
}

function pruneRecords(): void {
  if (recordsByLease.size <= MAX_RETAINED_RECORDS) return;
  const activeLeaseIds = new Set(activeLeaseByKey.values());
  for (const [leaseId, record] of recordsByLease) {
    if (recordsByLease.size <= MAX_RETAINED_RECORDS) break;
    if (activeLeaseIds.has(leaseId)) continue;
    if (record.phase === 'preparing' || record.phase === 'awaiting_ack') continue;
    if (record.ackTimer) clearTimeout(record.ackTimer);
    recordsByLease.delete(leaseId);
  }
}

function releaseSendLease(leaseId: string): void {
  // 只允许持有相同 leaseId 的回调释放自己的租约；旧回调不能释放新任务的锁。
  for (const [key, active] of activeLeaseByKey) {
    if (active === leaseId) activeLeaseByKey.delete(key);
  }
}

function settleRecord(
  record: ComposerSendRecord,
  phase: 'blocked' | 'failed_before_submit',
  code: string,
  retryable: boolean,
): void {
  // 幂等：已被取消/结算的记录不被迟到结果覆盖。
  if (record.phase !== 'preparing' && record.phase !== 'awaiting_ack') return;
  record.phase = phase;
  record.code = code;
  record.retryable = retryable;
  releaseSendLease(record.leaseId);
}

function markAwaitingAck(record: ComposerSendRecord): void {
  if (record.phase !== 'preparing') return;
  record.phase = 'awaiting_ack';
  record.ackTimer = setTimeout(() => {
    record.ackTimer = null;
    if (record.phase !== 'awaiting_ack') return;
    transitionToDeliveryUnknown(record, 'ack_timeout');
  }, ACK_TIMEOUT_MS);
  // DOM 版 setTimeout 返回 number，unref 仅 Node 存在；两端都安全。
  (record.ackTimer as unknown as { unref?: () => void }).unref?.();
}

function transitionToDeliveryUnknown(record: ComposerSendRecord, code: string): void {
  record.phase = 'delivery_unknown';
  record.code = code;
  record.retryable = false;
  releaseSendLease(record.leaseId);
  if (record.identity.kind === 'session') {
    // 显性标记：服务端是否收到不可证，提示核对，不做盲目重试。
    useStore.getState().markOptimisticUserMessageFailed(
      record.identity.sessionPath,
      record.clientMessageId,
      'delivery_unknown',
    );
  }
}

// ── 租约生命周期 ──

export function tryAcquireSendLease(input: AcquireSendLeaseInput): AcquireSendLeaseResult {
  const key = identityKey(input.identity);
  if (activeLeaseByKey.has(key)) return { ok: false, reason: 'transport_busy' };
  const leaseId = `composer-lease-${++leaseSeq}`;
  // 同一队列项的重试/类型切换复用原 clientMessageId：一条逻辑消息一个身份。
  let clientMessageId: string | null = null;
  if (input.queueItemId) {
    for (const record of recordsByLease.values()) {
      if (record.queueItemId === input.queueItemId) {
        clientMessageId = record.clientMessageId;
        break;
      }
    }
  }
  const record: ComposerSendRecord = {
    leaseId,
    clientMessageId: clientMessageId ?? createClientUserMessageId(),
    queueItemId: input.queueItemId ?? null,
    identity: input.identity,
    connectionGeneration,
    snapshotVersion: input.snapshotVersion ?? 1,
    composerRevisionAtClick: input.composerRevisionAtClick ?? null,
    bundle: input.bundle,
    targetRun: input.targetRun ?? null,
    phase: 'preparing',
    code: null,
    retryable: false,
    cancelled: false,
    ackTimer: null,
  };
  recordsByLease.set(leaseId, record);
  activeLeaseByKey.set(key, leaseId);
  pruneRecords();
  return { ok: true, leaseId };
}

/** 首页 pending 草稿 → 真实会话的原子身份转移（ensureSession 完成后调用）。 */
export function transferLeaseIdentity(leaseId: string, next: ComposerSessionIdentity): boolean {
  const record = recordsByLease.get(leaseId);
  if (!record || record.phase !== 'preparing' || record.cancelled) return false;
  const nextKey = identityKey(next);
  const occupant = activeLeaseByKey.get(nextKey);
  if (occupant && occupant !== leaseId) return false;
  activeLeaseByKey.delete(identityKey(record.identity));
  activeLeaseByKey.set(nextKey, leaseId);
  record.identity = next;
  record.identityFreshlyCreated = true;
  return true;
}

export function updateLeaseBundle(leaseId: string, bundle: ComposerSendBundle): boolean {
  const record = recordsByLease.get(leaseId);
  if (!record || record.phase !== 'preparing') return false;
  record.bundle = bundle;
  return true;
}

/** 取消在途准备：lease 立即失效，迟到的读取结果不得发送已删除/过期的内容。 */
export function cancelSendLease(leaseId: string, code = 'send_cancelled'): void {
  const record = recordsByLease.get(leaseId);
  if (!record) return;
  record.cancelled = true;
  if (record.ackTimer) {
    clearTimeout(record.ackTimer);
    record.ackTimer = null;
  }
  if (record.phase === 'preparing') {
    record.phase = 'failed_before_submit';
    record.code = code;
    record.retryable = true;
    releaseSendLease(leaseId);
  }
}

export function cancelQueueItemSend(queueItemId: string, code = 'send_cancelled'): void {
  for (const record of recordsByLease.values()) {
    if (record.queueItemId === queueItemId && record.phase === 'preparing') {
      cancelSendLease(record.leaseId, code);
    }
  }
}

export function hasInFlightSend(identity: ComposerSessionIdentity): boolean {
  return activeLeaseByKey.has(identityKey(identity));
}

export function getSendRecord(leaseId: string): ComposerSendRecord | null {
  return recordsByLease.get(leaseId) ?? null;
}

export function findSendRecordByClientMessageId(clientMessageId: string): ComposerSendRecord | null {
  // 同一 clientMessageId 可能横跨多条记录（队列项取消/编辑后重试复用身份）：
  // 优先返回仍在等待回执/投递未知的活跃记录，其次才是最新一条历史记录。
  let latest: ComposerSendRecord | null = null;
  for (const record of recordsByLease.values()) {
    if (record.clientMessageId !== clientMessageId) continue;
    if (record.phase === 'awaiting_ack' || record.phase === 'delivery_unknown') return record;
    latest = record;
  }
  return latest;
}

// ── 连接代次与回执关联 ──

export function getComposerConnectionGeneration(): number {
  return connectionGeneration;
}

export function noteComposerConnectionOpened(): void {
  connectionGeneration += 1;
}

export function noteComposerConnectionClosed(): void {
  connectionGeneration += 1;
  // 断线后再不会有回执：在途 awaiting_ack 一律转 delivery_unknown，禁止自动重发。
  for (const record of recordsByLease.values()) {
    if (record.phase !== 'awaiting_ack') continue;
    if (record.ackTimer) {
      clearTimeout(record.ackTimer);
      record.ackTimer = null;
    }
    transitionToDeliveryUnknown(record, 'connection_closed');
  }
}

/** 既有 session_user_message 回执（含重连后恢复/历史回放）关联发送记录。 */
export function noteComposerServerAck(clientMessageId: string | null | undefined): void {
  if (!clientMessageId) return;
  const record = findSendRecordByClientMessageId(clientMessageId);
  if (!record) return;
  if (record.phase !== 'awaiting_ack' && record.phase !== 'delivery_unknown') return;
  if (record.ackTimer) {
    clearTimeout(record.ackTimer);
    record.ackTimer = null;
  }
  record.phase = 'accepted';
  record.code = null;
  releaseSendLease(record.leaseId);
}

// ── 提交复核 ──

function currentModelKeyForSession(sessionPath: string | null): string | null {
  const state = useStore.getState();
  const sessionModel = sessionPath
    ? sessionScopedValue(state as never, state.sessionModelsByPath, sessionPath)
    : undefined;
  const models = state.models;
  const globalModelInfo = models.find(m => m.isCurrent);
  const sessionModelInfo = (() => {
    if (!sessionModel) return undefined;
    const full = models.find(m => m.id === sessionModel.id && m.provider === sessionModel.provider);
    return full ? { ...full, ...sessionModel } : sessionModel;
  })();
  const info = sessionModelInfo || globalModelInfo;
  return info ? `${info.provider}:${info.id}` : null;
}

function revalidateLeaseCommit(
  record: ComposerSendRecord,
  prepared: PreparedComposerSend,
): ComposerPreSubmitResult | null {
  if (record.cancelled || record.phase !== 'preparing') {
    return { kind: 'failed_before_submit', code: record.code ?? 'send_cancelled', retryable: true };
  }
  if (activeLeaseByKey.get(identityKey(record.identity)) !== record.leaseId) {
    return { kind: 'failed_before_submit', code: 'lease_lost', retryable: true };
  }
  // 连接代次不匹配：禁止向新连接发送旧载荷。
  if (record.connectionGeneration !== connectionGeneration) {
    return { kind: 'failed_before_submit', code: 'connection_changed', retryable: true };
  }
  if (record.identity.kind === 'session') {
    const { sessionId, sessionPath } = record.identity;
    const state = useStore.getState();
    const locatorPath = state.sessionLocatorsById?.[sessionId]?.path;
    if (locatorPath && locatorPath !== sessionPath) {
      return { kind: 'blocked', code: 'session_identity_mismatch', retryable: false };
    }
    const projection = state.sessions.find(session => session?.path === sessionPath);
    if (projection && projection.sessionId !== sessionId) {
      return { kind: 'blocked', code: 'session_identity_mismatch', retryable: false };
    }
    const knownAsCurrent = state.currentSessionPath === sessionPath && state.currentSessionId === sessionId;
    // 会话已删除的判定需要证据：sessions[] 非空时投影缺席才算删除。
    // 空列表只说明投影尚未加载；ensureSession 刚创建的身份同样豁免（投影未回流）。
    if (!record.identityFreshlyCreated && state.sessions.length > 0 && !projection && !locatorPath && !knownAsCurrent) {
      return { kind: 'blocked', code: 'session_deleted', retryable: false };
    }
  }
  // 模型在准备期间变更：载荷按旧模型预检，拒绝；重试会按新模型重新预检。
  const sessionPath = record.identity.kind === 'session' ? record.identity.sessionPath : prepared.sessionPathForSend;
  if (prepared.modelKey !== currentModelKeyForSession(sessionPath)) {
    return { kind: 'failed_before_submit', code: 'model_changed', retryable: true };
  }
  // 立即插入：准备结束到实际发送之间复核目标 run 身份。
  if (record.bundle.type === 'interject' && record.identity.kind === 'session') {
    const state = useStore.getState();
    const path = record.identity.sessionPath;
    const streaming = sessionScopedListIncludes(state as never, state.streamingSessions, path);
    if (!streaming) {
      return { kind: 'blocked', code: 'run_ended', retryable: false };
    }
    const key = sessionScopedKey(state as never, path) || path;
    const active = state.activeSessionStreams?.[key] ?? state.activeSessionStreams?.[path];
    const target = record.targetRun;
    if (target?.streamId && active?.streamId && target.streamId !== active.streamId) {
      return { kind: 'blocked', code: 'run_changed', retryable: false };
    }
    if (target?.turnId && active?.turnId && target.turnId !== active.turnId) {
      return { kind: 'blocked', code: 'run_changed', retryable: false };
    }
  }
  return null;
}

// ── 队列项状态回写 ──

function syncQueueItemAfterSend(record: ComposerSendRecord, result: ComposerDispatchResult): void {
  if (!record.queueItemId || record.identity.kind !== 'session') return;
  const sessionPath = record.identity.sessionPath;
  const store = useStore.getState();
  const items = sessionScopedValue(store as never, store.queuedTurnInputsByPath, sessionPath) || [];
  const current = items.find(item => item.id === record.queueItemId);
  if (!current) return; // 发送途中被删除：无需回写。
  // 发送途中被编辑（快照版本递增）：迟到结果不得覆盖新快照的状态。
  if ((current.snapshotVersion ?? 1) !== record.snapshotVersion) return;
  if (result.kind === 'transport_submitted') {
    // 快照已移交乐观消息与发送记录，队列项才可以移除。
    store.removeQueuedTurnInput(sessionPath, current.id);
    return;
  }
  if (result.kind === 'blocked' || result.kind === 'failed_before_submit') {
    store.setQueuedTurnInputStatus(
      sessionPath,
      current.id,
      result.kind === 'blocked' ? 'blocked' : 'failed',
      result.code,
      result.retryable,
    );
  }
}

// ── 发送执行 ──

function cancelledResult(record: ComposerSendRecord): ComposerDispatchResult {
  return {
    kind: 'failed_before_submit',
    code: record.code ?? 'send_cancelled',
    // 取消发生在提交前：内容从未上线，按原快照重试总是安全的。
    retryable: true,
  };
}

/** 持租约执行 prepare → commit；阻塞/失败/提交都会结算租约并回写队列项。 */
export async function sendWithLease(
  leaseId: string,
  deps: ComposerSendFlowDeps,
): Promise<ComposerDispatchResult> {
  const record = recordsByLease.get(leaseId);
  if (!record) return { kind: 'failed_before_submit', code: 'lease_missing', retryable: false };
  if (record.phase !== 'preparing') return cancelledResult(record);

  let prep: ComposerPrepareOutcome;
  try {
    prep = await prepareComposerSend(record.bundle, { ...deps, clientMessageId: record.clientMessageId });
  } catch (err) {
    // 程序异常归入明确阶段并保留诊断，不冒充网络错误。
    console.error('[composer-send] prepare threw', err);
    settleRecord(record, 'failed_before_submit', 'prepare_threw', true);
    const thrown: ComposerDispatchResult = { kind: 'failed_before_submit', code: 'prepare_threw', retryable: true };
    syncQueueItemAfterSend(record, thrown);
    return thrown;
  }

  if (record.cancelled || record.phase !== 'preparing') {
    // 准备期间被取消（队列项删除/编辑）：不发送。
    return cancelledResult(record);
  }
  if (prep.ok === false) {
    settleRecord(
      record,
      prep.result.kind === 'blocked' ? 'blocked' : 'failed_before_submit',
      prep.result.code,
      prep.result.retryable,
    );
    syncQueueItemAfterSend(record, prep.result);
    return prep.result;
  }

  const result = await commitPreparedComposerSend(prep.prepared, {
    t: deps.t,
    onCommit: deps.onCommit ?? undefined,
    revalidate: () => revalidateLeaseCommit(record, prep.prepared),
  });
  if (result.kind === 'transport_submitted') {
    markAwaitingAck(record);
  } else {
    settleRecord(record, result.kind === 'blocked' ? 'blocked' : 'failed_before_submit', result.code, result.retryable);
  }
  syncQueueItemAfterSend(record, result);
  return result;
}

/** 显式重试：仅允许「明确未发送」的记录（blocked/failed_before_submit 且可重试）。 */
export async function retrySendRecord(
  leaseId: string,
  deps: ComposerSendFlowDeps,
): Promise<ComposerDispatchResult | null> {
  const record = recordsByLease.get(leaseId);
  if (!record) return null;
  if ((record.phase !== 'failed_before_submit' && record.phase !== 'blocked') || !record.retryable) {
    return null;
  }
  const key = identityKey(record.identity);
  if (activeLeaseByKey.has(key)) return null;
  activeLeaseByKey.set(key, leaseId);
  record.phase = 'preparing';
  record.code = null;
  record.retryable = false;
  record.cancelled = false;
  record.connectionGeneration = connectionGeneration;
  return sendWithLease(leaseId, deps);
}

/** 立即插入/空闲直发队列项：与自动续发共用同一条租约通道。 */
export async function dispatchQueuedItem(
  item: QueuedTurnInput,
  dispatch: { type: 'prompt' | 'interject'; targetRun?: { streamId: string | null; turnId: string | null } | null },
  deps: QueueFlushDeps,
): Promise<ComposerDispatchResult> {
  const ref = item.bundle.sessionRef;
  const acq = tryAcquireSendLease({
    identity: { kind: 'session', sessionId: ref.sessionId, sessionPath: ref.sessionPath, agentId: ref.agentId },
    bundle: { ...item.bundle, type: dispatch.type },
    queueItemId: item.id,
    snapshotVersion: item.snapshotVersion ?? 1,
    targetRun: dispatch.targetRun ?? null,
  });
  if (!acq.ok) return { kind: 'blocked', code: 'transport_busy', retryable: true };
  return sendWithLease(acq.leaseId, deps);
}

// ── 队列自动续发 ──

/** 队首派发：实时校验全部门禁并同步取得租约；不满足即放弃（等下一次状态变化）。 */
export async function flushQueuedHeadNow(sessionPath: string, deps: QueueFlushDeps): Promise<void> {
  const store = useStore.getState();
  if (!store.connected) return;
  if (store.modelSwitching) return;
  if (store.pendingSessionSwitchPath) return;
  if (sessionScopedListIncludes(store as never, store.streamingSessions, sessionPath)) return;
  if (sessionScopedListIncludes(store as never, store.turnPendingSessions, sessionPath)) return;
  if (sessionScopedListIncludes(store as never, store.capabilityRefreshingSessions, sessionPath)) return;
  if (isSessionCompacting(store as never, sessionPath)) return;
  const item = (sessionScopedValue(store as never, store.queuedTurnInputsByPath, sessionPath) || [])[0];
  if (!item) return;
  // 失败/阻塞的队首原位保留：不无限循环，也不跳过队首导致后续乱序。
  if ((item.status ?? 'ready') !== 'ready') return;
  if (deps.shouldSkipItem?.(item)) return;
  await dispatchQueuedItem(item, { type: 'prompt' }, deps);
}

/**
 * 组件只发出调度意图；互斥状态由本模块持有。400ms 仅作视觉缓冲，
 * 计时结束后重新验证状态并同步取得租约（不把时长当正确性条件）。
 */
export function requestQueueFlush(sessionPath: string, deps: QueueFlushDeps): void {
  const existing = pendingFlushBySession.get(sessionPath);
  if (existing) {
    existing.deps = deps;
    return;
  }
  const timer = setTimeout(() => {
    pendingFlushBySession.delete(sessionPath);
    void flushQueuedHeadNow(sessionPath, deps);
  }, QUEUE_FLUSH_DEBOUNCE_MS);
  (timer as unknown as { unref?: () => void }).unref?.();
  pendingFlushBySession.set(sessionPath, { timer, deps });
}

/** 立即插入按钮的行为判定：不凭外观猜测（P3.4）。 */
export function resolveQueuedInsertNowAction(
  item: QueuedTurnInput,
  state: { streaming: boolean; turnPending: boolean },
): QueuedInsertNowAction {
  if (state.streaming) {
    // agent review 请求不能注入进行中的回合（既有禁止语义）。
    return item.bundle.agentMentions.length > 0 ? 'review_forbidden' : 'interject';
  }
  // awaiting_ack / 准备态而无可插入 run：保留输入并明确等待。
  if (state.turnPending) return 'wait';
  return 'prompt';
}

// ── 测试钩子 ──

export function resetComposerSendCoordinatorForTests(): void {
  for (const record of recordsByLease.values()) {
    if (record.ackTimer) clearTimeout(record.ackTimer);
  }
  recordsByLease.clear();
  activeLeaseByKey.clear();
  for (const pending of pendingFlushBySession.values()) {
    clearTimeout(pending.timer);
  }
  pendingFlushBySession.clear();
  connectionGeneration = 0;
  leaseSeq = 0;
}
