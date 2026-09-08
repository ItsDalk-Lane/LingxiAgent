/**
 * composer-send-coordinator.ts — 输入发送的会话级租约与队列调度（F1/F2）。
 *
 * 模块级单例（不随 InputArea 卸载/重挂载丢失）：
 * - 每个会话同一时刻只有一个在途发送租约（preparing → awaiting_ack）；
 *   租约在第一个 await 之前同步占用，记录 leaseId/会话身份/连接代次/快照版本。
 * - 回执（session_user_message 的 clientMessageId）到达 → accepted 并释放
 *   传输互斥；断线/回执超时 → delivery_unknown，禁止自动重发。
 * - 未决输入独立阻挡后续普通发送；canonical接收和运行终态分别验证，
 *   断线后通过捕获连接的有界历史查询核对，不自动重发。
 * - 队列项是快照持有者：prepare 前不删除；失败原位标 blocked/failed；
 *   编辑/删除会取消在途准备，迟到的结果不得发送。
 *
 * 它不是新的服务端消息系统：传输仍走既有 ws 信封，回执仍走既有
 * session_user_message 关联。
 */

import { useStore } from '../stores';
import { resolveServerConnection, type ServerConnection } from './server-connection';
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
  originConnectionKey: string;
  snapshotVersion: number;
  composerRevisionAtClick: number | null;
  bundle: ComposerSendBundle;
  /** 立即插入的目标 run 身份（streamId/turnId），commit 时复核。 */
  targetRun: { streamId: string | null; turnId: string | null } | null;
  phase: ComposerSendPhase;
  code: string | null;
  retryable: boolean;
  cancelled: boolean;
  /** 同步提交窗口已进入，UI 不再允许伪装为未发送编辑。 */
  commitStarted?: boolean;
  /**
   * 身份刚由 ensureSession 创建（transferLeaseIdentity 之后）：sessions[] 投影
   * 可能尚未回流，revalidate 不得把「投影缺席」误判为会话已删除。
   */
  identityFreshlyCreated?: boolean;
  acceptance: 'unproven' | 'accepted';
  runStatus: 'run_unknown' | 'running' | 'terminal' | 'reconciled_idle';
  sourceEntryId?: string;
  observedRunId?: string;
  observedRunSeq?: number;
  reconciliationStatus?: 'checking' | 'failed';
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
  | { ok: false; reason: 'transport_busy' | 'unresolved_limit' };

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
const MAX_UNRESOLVED_RECORDS = 512;

const recordsByLease = new Map<string, ComposerSendRecord>();
const activeLeaseByKey = new Map<string, string>();
export interface QueueFlushScope {
  sessionId: string;
  sessionPath: string;
  originConnectionKey: string;
  connectionGeneration: number;
}
interface QueueFlushIntent {
  intentId: number;
  scope: QueueFlushScope;
  ownerToken: object;
  timer: ReturnType<typeof setTimeout>;
  latestDeps: QueueFlushDeps;
}
const pendingFlushBySession = new Map<string, QueueFlushIntent>();
const foregroundQueueOwners = new Map<string, {scope: QueueFlushScope; ownerToken: object; latestDeps: QueueFlushDeps}>();
const defaultFlushOwner = {};
let intentSeq = 0;

/** 稳定服务器身份不含凭证；连接代次另存，不能把重连当成换服务器。 */
export function composerOriginConnectionKey(connection = resolveServerConnection(useStore.getState())): string {
  return JSON.stringify(connection
    ? [connection.kind, connection.connectionId, connection.serverId, connection.studioId,
      connection.serverNodeId ?? null, connection.executionBoundary?.boundaryId ?? null,
      // 尚无握手身份的旧本地配置只能使用配置命名空间，不能只凭端口跨服务器恢复。
      connection.serverNodeId || connection.executionBoundary ? null : connection.baseUrl]
    : ['unconfigured']);
}
export function captureQueueFlushScope(sessionPath: string): QueueFlushScope | null {
  const state = useStore.getState();
  const sessionId = state.sessions.find(session => session.path === sessionPath)?.sessionId
    ?? (state.currentSessionPath === sessionPath ? state.currentSessionId : null);
  return sessionId ? { sessionId, sessionPath, originConnectionKey: composerOriginConnectionKey(), connectionGeneration } : null;
}
function flushScopeKey(scope: QueueFlushScope): string {
  return JSON.stringify([scope.originConnectionKey, scope.sessionId]);
}
function currentQueueItem(sessionPath: string, id: string): QueuedTurnInput | undefined {
  const state = useStore.getState();
  return (sessionScopedValue(state, state.queuedTurnInputsByPath, sessionPath) || []).find(item => item.id === id);
}
function setQueueEditing(sessionPath: string, id: string, editing: boolean): void {
  useStore.setState(state => {
    const key = sessionScopedKey(state, sessionPath) || sessionPath;
    const items = state.queuedTurnInputsByPath[key];
    if (!items) return {};
    return { queuedTurnInputsByPath: { ...state.queuedTurnInputsByPath,
      [key]: items.map(item => item.id === id ? { ...item, editing } : item) } };
  });
}
export type QueueEditResult = 'ok' | 'missing' | 'too_late' | 'invalid';
/** 编辑点击先同步锁住队列项，再取消本项准备；状态与原快照保持不变。 */
export function beginQueuedItemEdit(sessionPath: string, id: string): QueueEditResult {
  for (const record of recordsByLease.values()) {
    if (record.queueItemId === id && record.identity.kind === 'session' && record.identity.sessionPath === sessionPath
      && record.originConnectionKey === composerOriginConnectionKey()
      && (record.commitStarted || ['awaiting_ack', 'accepted', 'delivery_unknown'].includes(record.phase))) return 'too_late';
  }
  const item = currentQueueItem(sessionPath, id);
  if (!item) return 'missing';
  setQueueEditing(sessionPath, id, true);
  cancelQueueItemSend(id, 'queue_edit_started', { sessionPath, snapshotVersion: item.snapshotVersion ?? 1 });
  return 'ok';
}
export function cancelQueuedItemEdit(sessionPath: string, id: string): void {
  setQueueEditing(sessionPath, id, false);
}
export function saveQueuedItemEdit(sessionPath: string, id: string, text: string): QueueEditResult {
  const item = currentQueueItem(sessionPath, id);
  if (!item) return 'missing';
  if (!item.editing || !text.trim()) return 'invalid';
  useStore.getState().updateQueuedTurnInputText(sessionPath, id, text);
  setQueueEditing(sessionPath, id, false);
  return 'ok';
}
let connectionGeneration = 0;
let leaseSeq = 0;

function identityKey(identity: ComposerSessionIdentity, origin = composerOriginConnectionKey()): string {
  return JSON.stringify([origin, identity.kind, identity.kind === 'session' ? identity.sessionId : identity.draftId]);
}

function pruneRecords(): void {
  if (recordsByLease.size <= MAX_RETAINED_RECORDS) return;
  const activeLeaseIds = new Set(activeLeaseByKey.values());
  for (const [leaseId, record] of recordsByLease) {
    if (recordsByLease.size <= MAX_RETAINED_RECORDS) break;
    if (activeLeaseIds.has(leaseId)) continue;
    if (record.phase === 'preparing' || isUnresolved(record)) continue;
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
    if (record.identity.kind === 'session') void reconcileComposerSession(record.identity.sessionPath);
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
  if (activeLeaseByKey.has(key) || unresolvedForKey(key).some(record => input.bundle.type === 'prompt' || record.acceptance !== 'accepted' || record.runStatus === 'run_unknown')) return { ok: false, reason: 'transport_busy' };
  if ([...recordsByLease.values()].filter(isUnresolved).length >= MAX_UNRESOLVED_RECORDS) {
    return { ok:false, reason:'unresolved_limit' };
  }
  const leaseId = `composer-lease-${++leaseSeq}`;
  // 同一队列项同版本的重试/类型切换复用身份；编辑改变版本后创建新身份。
  let clientMessageId: string | null = null;
  if (input.queueItemId) {
    for (const record of recordsByLease.values()) {
      if (record.queueItemId === input.queueItemId && record.snapshotVersion === (input.snapshotVersion ?? 1)
        && identityKey(record.identity, record.originConnectionKey) === key) {
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
    originConnectionKey: composerOriginConnectionKey(),
    snapshotVersion: input.snapshotVersion ?? 1,
    composerRevisionAtClick: input.composerRevisionAtClick ?? null,
    bundle: input.bundle,
    targetRun: input.targetRun ?? null,
    phase: 'preparing',
    acceptance: 'unproven',
    runStatus: 'run_unknown',
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
  activeLeaseByKey.delete(identityKey(record.identity, record.originConnectionKey));
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

export function cancelQueueItemSend(queueItemId: string, code = 'send_cancelled', scope?: { sessionPath: string; snapshotVersion: number }): void {
  for (const record of recordsByLease.values()) {
    if (record.queueItemId === queueItemId && record.phase === 'preparing'
      && record.originConnectionKey === composerOriginConnectionKey()
      && (!scope || record.identity.kind === 'session' && record.identity.sessionPath === scope.sessionPath && record.snapshotVersion === scope.snapshotVersion)) {
      cancelSendLease(record.leaseId, code);
    }
  }
}

export function hasInFlightSend(identity: ComposerSessionIdentity): boolean {
  return activeLeaseByKey.has(identityKey(identity)) || unresolvedForKey(identityKey(identity)).length > 0;
}

export function getSendRecord(leaseId: string): ComposerSendRecord | null {
  return recordsByLease.get(leaseId) ?? null;
}

export function findSendRecordByClientMessageId(clientMessageId: string): ComposerSendRecord | null {
  // 同一 clientMessageId 可能横跨同版本重试记录；编辑后的新版本使用新身份：
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
  cancelReconciliationChains();
}

export function noteComposerConnectionClosed(): void {
  connectionGeneration += 1;
  cancelReconciliationChains();
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

/** 仅凭明确 canonical entry 的关联结算接收；ACK 不证明 run 结束。 */
export function noteComposerServerAck(clientMessageId: string | null | undefined, evidence?: {
  originConnectionKey: string; sessionId: string; sessionPath: string;
  snapshotVersion: number; sourceEntryId: string;
}): boolean {
  if (!clientMessageId || !evidence || !evidence.sourceEntryId) return false;
  const records = [...recordsByLease.values()].filter(record => record.clientMessageId === clientMessageId
    && record.originConnectionKey === evidence.originConnectionKey && record.identity.kind === 'session'
    && record.identity.sessionId === evidence.sessionId && record.identity.sessionPath === evidence.sessionPath
    && record.snapshotVersion === evidence.snapshotVersion && isUnresolved(record));
  if (records.length !== 1) return false;
  const record = records[0];
  if (record.sourceEntryId && record.sourceEntryId !== evidence.sourceEntryId) {
    record.code = 'association_ambiguous'; return false;
  }
  if (record.ackTimer) clearTimeout(record.ackTimer);
  record.ackTimer = null;
  record.phase = 'accepted';
  record.acceptance = 'accepted';
  record.sourceEntryId = evidence.sourceEntryId;
  record.code = null;
  releaseSendLease(record.leaseId);
  return true;
}

function isUnresolved(record: ComposerSendRecord): boolean {
  return record.phase === 'awaiting_ack' || record.phase === 'delivery_unknown'
    || record.phase === 'accepted' && !['terminal', 'reconciled_idle'].includes(record.runStatus);
}
function unresolvedForKey(key: string): ComposerSendRecord[] {
  return [...recordsByLease.values()].filter(record => isUnresolved(record)
    && identityKey(record.identity, record.originConnectionKey) === key);
}
const eventRevisions = new Map<string, number>();
const reconciliationChains = new Map<string, { controller: AbortController; promise: Promise<void> }>();
function cancelReconciliationChains(): void {
  for (const chain of reconciliationChains.values()) chain.controller.abort();
  reconciliationChains.clear();
}
export function unresolvedComposerSessionPaths(origin = composerOriginConnectionKey()): string[] {
  return [...new Set([...recordsByLease.values()].flatMap(record => record.originConnectionKey === origin
    && record.identity.kind === 'session' && isUnresolved(record) ? [record.identity.sessionPath] : []))];
}
/** 只在已有协议身份校验通过之后消费；普通 status false 不解除未决输入。 */
export function noteComposerRunEvent(message: { type: string; sessionId?: string; sessionPath?: string;
  streamId?: string; turnId?: string; runId?: string; seq?: number; turnInputEntryId?: string }, origin = composerOriginConnectionKey()): void {
  if (!message.sessionId || !message.sessionPath || origin !== composerOriginConnectionKey()) return;
  const key = identityKey({kind:'session',sessionId:message.sessionId,sessionPath:message.sessionPath,agentId:null},origin);
  eventRevisions.set(key, (eventRevisions.get(key) ?? 0) + 1);
  const runId = message.runId || message.streamId || message.turnId;
  for (const record of unresolvedForKey(key)) {
    if (message.type === 'assistant_run_start' && runId) {
      // 不接受第二个不同run覆盖未终结身份；随后以有界历史查询核实。
      if (!record.observedRunId || record.observedRunId === runId) {
        record.observedRunId = runId; record.observedRunSeq = message.seq;
      }
      record.runStatus = 'running';
      if (record.acceptance === 'accepted') clearReconciliationDisplay(record);
    } else if (message.type === 'assistant_run_end' && runId && record.observedRunId === runId
      && record.sourceEntryId && message.turnInputEntryId === record.sourceEntryId
      && Number.isSafeInteger(message.seq) && Number.isSafeInteger(record.observedRunSeq) && message.seq! > record.observedRunSeq!) {
      record.runStatus = 'terminal';
      resumeForegroundQueue(message.sessionPath);
    }
  }
  if (message.type === 'assistant_run_end' && unresolvedForKey(key).length) void reconcileComposerSession(message.sessionPath);
}
function updateReconciliationDisplay(record: ComposerSendRecord, checking: boolean): void {
  record.reconciliationStatus = checking ? 'checking' : 'failed';
  if (record.identity.kind === 'session') useStore.getState().markOptimisticUserMessageFailed(
    record.identity.sessionPath, record.clientMessageId, checking ? 'delivery_unknown_checking' : 'delivery_unknown_failed');
}

function clearReconciliationDisplay(record: ComposerSendRecord): void {
  if (record.identity.kind !== 'session') return;
  const state = useStore.getState();
  const session = sessionScopedValue(state, state.chatSessions, record.identity.sessionPath);
  const item = session?.items.find(candidate => candidate.type === 'message'
    && (candidate.data.id === record.clientMessageId || candidate.data.clientMessageId === record.clientMessageId));
  if (item?.type === 'message') {
    state.confirmOptimisticUserMessage(record.identity.sessionPath, record.clientMessageId, item.data);
  }
}
/** 捕获服务器认证与会话身份，每会话单飞；最多20页/10秒，无自动轮询。 */
export function reconcileComposerSession(sessionPath: string): Promise<void> {
  const connection = resolveServerConnection(useStore.getState());
  const scope = captureQueueFlushScope(sessionPath);
  if (!connection || !scope) return Promise.resolve();
  const key = identityKey({kind:'session',sessionId:scope.sessionId,sessionPath,agentId:null}, scope.originConnectionKey);
  const existing = reconciliationChains.get(key);
  if (existing) return existing.promise;
  const records = unresolvedForKey(key);
  if (!records.length) return Promise.resolve();
  const captured: ServerConnection = { ...connection };
  const controller = new AbortController();
  const generation = connectionGeneration;
  const revision = eventRevisions.get(key) ?? 0;
  const timer = setTimeout(() => controller.abort(), 10_000);
  records.forEach(record => updateReconciliationDisplay(record, true));
  const chain = { controller, promise: Promise.resolve() };
  reconciliationChains.set(key, chain);
  const valid = () => !controller.signal.aborted && generation === connectionGeneration
    && composerOriginConnectionKey() === scope.originConnectionKey && reconciliationChains.get(key) === chain;
  chain.promise = Promise.resolve().then(async () => {
    const { fetchSessionHistoryPage } = await import('../stores/session-actions');
    let before: string | undefined;
    const seen = new Set<string>();
    for (let pageIndex = 0; pageIndex < 20 && valid(); pageIndex++) {
      let abortRead!: () => void;
      const deadline = new Promise<never>((_resolve, reject) => {
        abortRead = () => reject(new Error('reconciliation_timeout'));
        if (controller.signal.aborted) abortRead();
        else controller.signal.addEventListener('abort', abortRead, {once:true});
      });
      // 即使读取替身/旧实现未响应abort，本模块也必须释放单飞与checking状态。
      const page = await Promise.race([
        fetchSessionHistoryPage(captured, { sessionId: scope.sessionId, sessionPath }, { before, signal:controller.signal }), deadline,
      ]).finally(() => controller.signal.removeEventListener('abort', abortRead));
      if (!valid()) return;
      const snapshot = page.reconciliation;
      if (!snapshot || snapshot.sessionId !== scope.sessionId || snapshot.sessionPath !== sessionPath
        || snapshot.complete !== true || !snapshot.snapshotId || !Number.isSafeInteger(snapshot.runRevision)) throw new Error('history_identity_or_evidence_unavailable');
      for (const record of records) {
        const matches = page.messages.filter(message => message.role === 'user' && message.clientMessageId === record.clientMessageId
          && message.snapshotVersion === record.snapshotVersion && typeof message.sourceEntryId === 'string');
        if (matches.length !== 1) continue;
        const message = matches[0];
        if (!noteComposerServerAck(record.clientMessageId, {...scope, sourceEntryId:message.sourceEntryId!,snapshotVersion:record.snapshotVersion})) continue;
        if ((eventRevisions.get(key) ?? 0) === revision) {
          record.runStatus = snapshot.runStatus === 'reconciled_idle' ? 'reconciled_idle'
            : snapshot.runStatus === 'running' ? 'running' : 'run_unknown';
          if (record.runStatus === 'reconciled_idle') {
            useStore.getState().forceRemoveStreamingSession(sessionPath);
            useStore.getState().endTurnPending?.(sessionPath);
            resumeForegroundQueue(sessionPath);
          }
        }
        const state = useStore.getState();
        const session = sessionScopedValue(state, state.chatSessions, sessionPath);
        const item = session?.items.find(item => item.type === 'message' && (item.data.id === record.clientMessageId || item.data.clientMessageId === record.clientMessageId));
        if (item?.type === 'message') state.confirmOptimisticUserMessage(sessionPath, record.clientMessageId, {
          ...item.data, sourceEntryId:message.sourceEntryId,
        });
        if (record.runStatus === 'run_unknown') {
          state.markOptimisticUserMessageFailed(sessionPath, record.clientMessageId, 'delivery_run_unknown');
        }
        record.reconciliationStatus = undefined;
      }
      if (records.every(record => record.acceptance === 'accepted')) return;
      if (!page.hasMore) break;
      const oldest = page.oldestId || page.messages[0]?.id;
      if (!oldest || seen.has(oldest)) throw new Error('history_pagination_incomplete');
      seen.add(oldest); before = oldest;
    }
    for (const record of records) if (record.acceptance !== 'accepted') {
      record.code = 'history_incomplete_or_unproven'; updateReconciliationDisplay(record, false);
    }
  }).catch(error => {
    if (!valid() && !controller.signal.aborted) return;
    if (generation !== connectionGeneration || composerOriginConnectionKey() !== scope.originConnectionKey) return;
    for (const record of records) {
      if (!isUnresolved(record) || (eventRevisions.get(key) ?? 0) !== revision) continue;
      record.code = controller.signal.aborted ? 'reconciliation_timeout' : String(error?.message || 'reconciliation_failed');
      updateReconciliationDisplay(record, false);
    }
  }).finally(() => {
    clearTimeout(timer);
    if (reconciliationChains.get(key) === chain) reconciliationChains.delete(key);
  });
  return chain.promise;
}
export function reconcilePendingComposerSessions(): void {
  for (const path of unresolvedComposerSessionPaths()) void reconcileComposerSession(path);
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
  if (activeLeaseByKey.get(identityKey(record.identity, record.originConnectionKey)) !== record.leaseId) {
    return { kind: 'failed_before_submit', code: 'lease_lost', retryable: true };
  }
  // 连接代次不匹配：禁止向新连接发送旧载荷。
  if (record.connectionGeneration !== connectionGeneration || record.originConnectionKey !== composerOriginConnectionKey()) {
    return { kind: 'failed_before_submit', code: 'connection_changed', retryable: true };
  }
  if (unresolvedForKey(identityKey(record.identity, record.originConnectionKey)).some(other => other !== record && (record.bundle.type === 'prompt' || other.acceptance !== 'accepted' || other.runStatus === 'run_unknown'))) {
    return { kind:'blocked', code:'unresolved_input', retryable:false };
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
  if (record.queueItemId && record.identity.kind === 'session') {
    const item = currentQueueItem(record.identity.sessionPath, record.queueItemId);
    if (!item || item.editing || (item.snapshotVersion ?? 1) !== record.snapshotVersion) {
      return { kind: 'failed_before_submit', code: 'queue_snapshot_changed', retryable: true };
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
  if (!current || current.editing) return; // 删除或编辑期间，不覆盖其原状态。
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
    prep = await prepareComposerSend(record.bundle, { ...deps, clientMessageId: record.clientMessageId, snapshotVersion: record.snapshotVersion });
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
    onCommit: () => { record.commitStarted = true; deps.onCommit?.(); },
    revalidate: () => revalidateLeaseCommit(record, prep.prepared),
  });
  if (result.kind === 'transport_submitted') {
    markAwaitingAck(record);
  } else {
    record.commitStarted = false;
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
  const key = identityKey(record.identity, record.originConnectionKey);
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
  const current = currentQueueItem(ref.sessionPath, item.id);
  if (!current || current.editing || (current.snapshotVersion ?? 1) !== (item.snapshotVersion ?? 1)) {
    return { kind: 'blocked', code: 'queue_snapshot_changed', retryable: false };
  }
  if (dispatch.type === 'interject' && item.bundle.agentMentions.length > 0) {
    return { kind: 'blocked', code: 'agent_review_interject_forbidden', retryable: false };
  }
  const acq = tryAcquireSendLease({
    identity: { kind: 'session', sessionId: ref.sessionId, sessionPath: ref.sessionPath, agentId: ref.agentId },
    bundle: { ...item.bundle, type: dispatch.type },
    queueItemId: item.id,
    snapshotVersion: item.snapshotVersion ?? 1,
    targetRun: dispatch.targetRun ?? null,
  });
  if (acq.ok === false) return { kind: 'blocked', code: acq.reason, retryable: acq.reason !== 'unresolved_limit' };
  return sendWithLease(acq.leaseId, deps);
}

// ── 队列自动续发 ──

/** 队首派发：实时校验全部门禁并同步取得租约；不满足即放弃（等下一次状态变化）。 */
export async function flushQueuedHeadNow(sessionPath: string, deps: QueueFlushDeps): Promise<void> {
  const store = useStore.getState();
  if (!store.connected) return;
  if (store.currentTab !== 'chat' || store.currentSessionPath !== sessionPath) return;
  if (store.modelSwitching) return;
  if (store.pendingSessionSwitchPath) return;
  if (sessionScopedListIncludes(store as never, store.streamingSessions, sessionPath)) return;
  if (sessionScopedListIncludes(store as never, store.turnPendingSessions, sessionPath)) return;
  if (sessionScopedListIncludes(store as never, store.capabilityRefreshingSessions, sessionPath)) return;
  if (isSessionCompacting(store as never, sessionPath)) return;
  const item = (sessionScopedValue(store as never, store.queuedTurnInputsByPath, sessionPath) || [])[0];
  if (!item || item.editing || item.bundle.sessionRef.sessionId !== store.currentSessionId) return;
  // 失败/阻塞的队首原位保留：不无限循环，也不跳过队首导致后续乱序。
  if ((item.status ?? 'ready') !== 'ready') return;
  if (deps.shouldSkipItem?.(item)) return;
  await dispatchQueuedItem(item, { type: 'prompt' }, deps);
}

/**
 * 组件只发出调度意图；互斥状态由本模块持有。400ms 仅作视觉缓冲，
 * 计时结束后重新验证状态并同步取得租约（不把时长当正确性条件）。
 */
export function requestQueueFlush(sessionPath: string, deps: QueueFlushDeps, ownerToken: object = defaultFlushOwner): QueueFlushScope | null {
  const scope = captureQueueFlushScope(sessionPath);
  if (!scope) return null;
  const key = flushScopeKey(scope);
  foregroundQueueOwners.set(key, {scope,ownerToken,latestDeps:deps});
  const existing = pendingFlushBySession.get(key);
  if (existing && existing.scope.connectionGeneration === scope.connectionGeneration) {
    existing.latestDeps = deps;
    existing.ownerToken = ownerToken;
    return scope;
  }
  if (existing) clearTimeout(existing.timer);
  const intentId = ++intentSeq;
  const timer = setTimeout(() => {
    const intent = pendingFlushBySession.get(key);
    if (!intent || intent.intentId !== intentId) return;
    pendingFlushBySession.delete(key);
    if (intent.scope.connectionGeneration !== connectionGeneration
      || intent.scope.originConnectionKey !== composerOriginConnectionKey()) return;
    if (useStore.getState().currentSessionId !== intent.scope.sessionId) return;
    void flushQueuedHeadNow(intent.scope.sessionPath, intent.latestDeps);
  }, QUEUE_FLUSH_DEBOUNCE_MS);
  (timer as unknown as { unref?: () => void }).unref?.();
  pendingFlushBySession.set(key, { intentId, scope, ownerToken, timer, latestDeps: deps });
  return scope;
}

function resumeForegroundQueue(sessionPath: string): void {
  const scope = captureQueueFlushScope(sessionPath);
  if (!scope) return;
  const owner = foregroundQueueOwners.get(flushScopeKey(scope));
  if (!owner || owner.scope.connectionGeneration !== connectionGeneration) return;
  requestQueueFlush(sessionPath, owner.latestDeps, owner.ownerToken);
}

/** 只撤销尚未开始的意图；已取得的 A 租约可以完成，不影响 B 的草稿。 */
export function cancelQueueFlushIntent(scope: QueueFlushScope, ownerToken: object): void {
  const key = flushScopeKey(scope);
  const owner = foregroundQueueOwners.get(key);
  if (owner?.ownerToken === ownerToken && owner.scope.connectionGeneration === scope.connectionGeneration) foregroundQueueOwners.delete(key);
  const intent = pendingFlushBySession.get(key);
  if (!intent || intent.ownerToken !== ownerToken || intent.scope.connectionGeneration !== scope.connectionGeneration) return;
  clearTimeout(intent.timer);
  pendingFlushBySession.delete(key);
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
  cancelReconciliationChains();
  eventRevisions.clear();
  recordsByLease.clear();
  activeLeaseByKey.clear();
  for (const pending of pendingFlushBySession.values()) {
    clearTimeout(pending.timer);
  }
  pendingFlushBySession.clear();
  foregroundQueueOwners.clear();
  connectionGeneration = 0;
  leaseSeq = 0;
  intentSeq = 0;
}
