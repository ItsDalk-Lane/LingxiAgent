/**
 * history-protocol-client.ts — E03 客户端条件校验与失效（TASKBOOK E03.1–E03.5）
 *
 * COMPAT（E07）：受支持的旧服务器行为 = 不返回 Lingxi-History-Protocol/ETag 头
 * （→ 本模块不存记录，请求保持无条件 50，见 loadMessages 旧服务器测试与
 *   tests/history-protocol-conditional.test.ts 的头断言）；受支持的旧客户端行为 =
 * * 不发 If-None-Match（服务端照常 200，见 server/routes/sessions.ts 条件求值的
 *   无条件分支）。退役条件：所有连接均确认协议 v1 能力且无旧实例混布后，
 *   无能力回退分支才可移除——本任务不设自动删除日期，不改版本/发布坐标。
 *
 * 职责边界：
 *  - 校验记录（内存，不持久化）：requestKey/etag/连接 epoch/已应用版本/原始记录
 *    边界覆盖证明/协议能力与推荐值。每会话 ≤32 条、总元数据 ≤256KiB，超限淘汰
 *    记录（不淘汰消息）；不新增 raw-page/serialized-response 缓存。
 *  - 条件请求包装：仅「重校验当前已加载表示」语义使用（E03.4 触发点调用）；
 *    强制恢复/流式修复/严格对账/新页首次加载由调用方无条件请求，不经本模块。
 *  - 失效不靠本模块轮询：live 版本在 304 到达后新鲜重查（E03.3）；状态清理
 *    生命周期经 invalidateSessionCache 钩子丢弃记录；认证/换 server 由
 *    connection epoch 变化使记录不可达（自然淘汰），不记录凭据原文。
 *
 * 304 分支精确行为见 protocol/client-state-machine.md。
 */
import { useStore } from './index';
import { lingxiFetch } from '../hooks/use-hana-fetch';
import { requireServerConnection } from '../services/server-connection';
import { readMessageLiveVersion } from './message-live-version';

export const HISTORY_PROTOCOL_VERSION = 1;
export const HISTORY_PROTOCOL_TAG_SCOPE = 'hrp1';
/** E04 合同同源：单会话校验记录上限（超限淘汰记录，不淘汰消息）。 */
export const HISTORY_VALIDATION_MAX_RECORDS_PER_SESSION = 32;
/** 全局校验元数据预算（256KiB，估算值：key/etag/覆盖字段字节合计）。 */
export const HISTORY_VALIDATION_METADATA_BUDGET_BYTES = 256 * 1024;

export interface HistoryValidationRecord {
  requestKey: string;
  etag: string;
  /** 连接/认证 epoch（connectionId+authState+token 指纹）：变更即不可复用。 */
  connectionEpoch: string;
  sessionId: string | null;
  sessionPath: string;
  /** 发送条件请求时绑定的已应用消息版本（_loadMessagesVersion）。 */
  appliedMessageVersion: number;
  /** 已应用的 todos live 版本（todos 状态更新即失配）。 */
  appliedTodosVersion: number;
  /** 页面覆盖的原始记录边界（服务端原始 id=display 序号，不是归并显示项 id）。 */
  coverage: {
    firstRecordId: string;
    lastRecordId: string;
    recordCount: number;
    hasMore: boolean;
    nextBefore: string | null;
    revision: string;
  };
  /** 协议能力与推荐值（响应头回读；作用域=connection epoch）。 */
  protocolVersion: number;
  recommendedLimit: number;
  savedAt: number;
}

interface ValidationStoreShape {
  chatSessions: Record<string, any> | undefined;
  streamingSessions?: string[] | undefined;
  pendingNewSession?: boolean;
  pendingSessionSwitchPath?: string | null;
  currentSessionPath?: string | null;
  todosLiveVersionBySession?: Record<string, any>;
  _loadMessagesVersion?: Record<string, any>;
}

const records = new Map<string, HistoryValidationRecord>();
const recordsBySession = new Map<string, Set<string>>();
const conditionalDisabledEpochs = new Set<string>();
let totalMetadataBytes = 0;

/** 测试/登出辅助：清空全部状态。 */
export function __clearHistoryProtocolStateForTest(): void {
  records.clear();
  recordsBySession.clear();
  conditionalDisabledEpochs.clear();
  totalMetadataBytes = 0;
  capabilityByEpoch.clear();
  overviewRecommendedByPath.clear();
}

// ── 请求键与连接 epoch（E03.1：不只 sessionPath/file revision，不记录凭据原文） ──

function tokenFingerprint(token: string | null | undefined): string {
  if (!token) return 'anon';
  // 非可逆指纹：仅用于检测"凭证是否变化"，不编码原文。
  let h = 5381;
  for (let i = 0; i < token.length; i += 1) h = ((h << 5) + h + token.charCodeAt(i)) | 0;
  return `fp${(h >>> 0).toString(16)}:${token.length}`;
}

/** 连接/认证 epoch：变更（换 server/重连换凭证/鉴权状态变化）即旧标签不可复用。 */
export function connectionEpoch(connection: {
  connectionId?: string;
  studioId?: string;
  serverNodeId?: string;
  authState?: unknown;
  token?: string | null;
} | null | undefined): string {
  if (!connection) return 'no-connection';
  return [
    connection.connectionId ?? 'unknown-connection',
    String(connection.authState ?? ''),
    tokenFingerprint(connection.token ?? null),
  ].join('|');
}

export function buildHistoryRequestKey(input: {
  epoch: string;
  sessionId: string | null;
  sessionPath: string;
  before: string | null;
  limit: number;
  mode?: string;
  language?: string;
  projectionVersion?: number;
}): string {
  // 语言/投影版本为表示维度（当前恒定 v1/空），进入键以保证投影规则变化即失配。
  return [
    input.epoch,
    input.sessionId ?? 'no-session-id',
    input.sessionPath,
    input.mode ?? 'normal',
    input.before == null ? 'latest' : String(input.before),
    String(input.limit),
    `lang=${input.language ?? ''}`,
    `proj=${input.projectionVersion ?? 1}`,
  ].join('|');
}

/** E03.4：该普通页是否已有可复验记录（无记录 → 调用方保持既有无 HTTP 行为）。 */
export function hasMessagesValidationRecord(
  sessionPath: string,
  sessionId: string | null,
  limit = 50,
): boolean {
  console.error('[has-dbg] called sessionPath=', sessionPath, 'sessionId=', sessionId);
  console.error('[has-dbg] recordsSize=', records.size, 'limit=', limit);
  if (records.size === 0) return false;
  const state = useStore.getState() as Record<string, any>;
  let connection: any;
  try {
    connection = requireServerConnection(state, 'history protocol record lookup: server connection not ready');
  } catch {
    return false;
  }
  const epoch = connectionEpoch(connection);
  const requestKey = buildHistoryRequestKey({
    epoch,
    sessionId,
    sessionPath,
    before: null,
    limit,
  });
  return records.has(requestKey);
}

// ── 记录存取（LRU：每会话 32 条 + 全局 256KiB 元数据预算） ──────────────

function recordBytes(record: HistoryValidationRecord): number {
  return (
    record.requestKey.length +
    record.etag.length +
    record.connectionEpoch.length +
    record.sessionPath.length +
    (record.sessionId?.length ?? 0) +
    record.coverage.firstRecordId.length +
    record.coverage.lastRecordId.length +
    (record.coverage.nextBefore?.length ?? 0) +
    160
  );
}

function evictForBudget(sessionPath: string): void {
  const sessionKeys = recordsBySession.get(sessionPath);
  if (sessionKeys && sessionKeys.size > HISTORY_VALIDATION_MAX_RECORDS_PER_SESSION) {
    const overflow = sessionKeys.size - HISTORY_VALIDATION_MAX_RECORDS_PER_SESSION;
    let dropped = 0;
    for (const key of sessionKeys) {
      if (dropped >= overflow) break;
      const record = records.get(key);
      if (record) totalMetadataBytes -= recordBytes(record);
      records.delete(key);
      sessionKeys.delete(key);
      dropped += 1;
    }
  }
  while (totalMetadataBytes > HISTORY_VALIDATION_METADATA_BUDGET_BYTES) {
    const oldest = records.keys().next();
    if (oldest.done) break;
    dropRecord(oldest.value);
  }
}

function dropRecord(requestKey: string): void {
  const record = records.get(requestKey);
  if (!record) return;
  totalMetadataBytes -= recordBytes(record);
  records.delete(requestKey);
  const sessionKeys = recordsBySession.get(record.sessionPath);
  sessionKeys?.delete(requestKey);
  if (sessionKeys && sessionKeys.size === 0) recordsBySession.delete(record.sessionPath);
}

function saveRecord(record: HistoryValidationRecord): void {
  const existing = records.get(record.requestKey);
  if (existing) {
    totalMetadataBytes -= recordBytes(existing);
    records.delete(record.requestKey);
  }
  records.set(record.requestKey, record); // Map 插入序 = LRU 序
  totalMetadataBytes += recordBytes(record);
  const sessionKeys = recordsBySession.get(record.sessionPath) ?? new Set<string>();
  sessionKeys.add(record.requestKey);
  recordsBySession.set(record.sessionPath, sessionKeys);
  evictForBudget(record.sessionPath);
}

/** 测试/观测：按会话读取记录数与元数据估算。 */
export function historyValidationStatsForSession(sessionPath: string): { records: number; metadataBytes: number } {
  const sessionKeys = recordsBySession.get(sessionPath);
  let bytes = 0;
  let count = 0;
  for (const key of sessionKeys ?? []) {
    const record = records.get(key);
    if (record) {
      count += 1;
      bytes += recordBytes(record);
    }
  }
  return { records: count, metadataBytes: bytes };
}

/**
 * E03.4 失效钩子：状态清理生命周期丢弃记录（在既有 invalidateSessionCache /
 * 淘汰 / 登出路径调用）。sessionPath==null 清空全部。
 */
export function noteHistoryValidationInvalidated(sessionPath?: string): void {
  if (sessionPath == null) {
    for (const key of [...records.keys()]) dropRecord(key);
    return;
  }
  for (const key of [...(recordsBySession.get(sessionPath) ?? [])]) dropRecord(key);
}

/** E03.5：该连接 epoch 已知不支持条件请求（本 epoch 内不再尝试）。 */
function isEpochConditionalDisabled(epoch: string): boolean {
  return conditionalDisabledEpochs.has(epoch);
}

// ── 新鲜状态守卫（E03.3：304 到达后重查，不用发请求前快照） ────────────────

function freshStateGuards(
  sessionPath: string,
  record: HistoryValidationRecord,
  requestVersion: number,
  appliedLiveVersion: number,
): { outcome: 'valid' | 'stale' | 'superseded' } {
  const state = useStore.getState() as ValidationStoreShape;
  // 会话被淘汰/清空 → 覆盖证明失效
  const store = useStore.getState() as Record<string, any>;
  const chatKey = (store.sessionScopedKey?.(store, sessionPath) as string | undefined) ?? sessionPath;
  const chatCached = (store.chatSessions as Record<string, any> | undefined)?.[chatKey] ?? null;
  if (!chatCached) return { outcome: 'superseded' }; // 会话被淘汰/清空 → 丢弃，不补取
  if (state.pendingNewSession || state.pendingSessionSwitchPath) return { outcome: 'superseded' };
  if (state.currentSessionPath != null && state.currentSessionPath !== sessionPath) return { outcome: 'superseded' };
  if ((state.streamingSessions ?? []).includes(sessionPath)) return { outcome: 'superseded' }; // 流式进行中保守退出
  // 请求版本：已有更新的 load 在途 → 本次结果作废（superseded，不补取）
  const currentLoadVersion = (state._loadMessagesVersion as Record<string, any> | undefined)?.[sessionPath];
  if (currentLoadVersion != null && currentLoadVersion !== requestVersion) return { outcome: 'superseded' };
  if (readMessageLiveVersion(sessionPath) !== appliedLiveVersion) return { outcome: 'stale' }; // WS live 更新 → 需补取
  // todos live 版本变化 → 表示可能已变化 → 需补取
  const todosVersion = (state.todosLiveVersionBySession as Record<string, any> | undefined)?.[sessionPath] ?? 0;
  if (todosVersion !== record.appliedTodosVersion) return { outcome: 'stale' };
  // 覆盖证明仍驻留：原始边界 id + 游标 + revision 与已应用状态一致
  if (chatCached.revision != null && chatCached.revision !== record.coverage.revision) return { outcome: 'stale' };
  if (record.coverage.nextBefore != null && chatCached.nextBefore !== record.coverage.nextBefore) return { outcome: 'stale' };
  if (chatCached.hasMore !== undefined && chatCached.hasMore !== record.coverage.hasMore) return { outcome: 'stale' };
  return { outcome: 'valid' };
}

// ── 条件请求（仅重校验语义；调用方为 E03.4 既有触发点） ────────────────────

export type ConditionalMessagesOutcome =
  | { kind: 'not-modified'; requestKey: string }
  | { kind: 'superseded' }
  | { kind: 'ok'; response: Response; etag: string | null; requestKey: string; conditionalTried: boolean };

/**
 * 普通消息页的条件请求（E03.1/E03.3/E03.5）。
 * - url 由调用方构造（sessionMessagesUrl 同源），本模块不做任何读取/状态应用。
 * - 无记录/预检失败 → 无条件请求；304 → 新鲜状态重查 → 失效则至多一次无条件补取；
 * - 条件请求传输失败 → 同目标一次无条件回退并标记该 epoch 暂不条件（401/403 不算）。
 */
export function headerGet(res: any, name: string): string | null {
  const headers = res?.headers;
  if (!headers) return null;
  if (typeof headers.get === 'function') {
    const v = headers.get(name) ?? headers.get(name.toLowerCase());
    return v == null ? null : String(v);
  }
  const direct = headers[name] ?? headers[name.toLowerCase()];
  return direct == null ? null : String(direct);
}

function statusOf(res: any): number {
  return typeof res?.status === 'number' ? res.status : res?.ok ? 200 : 0;
}

// ── E06.3 页大小协商（能力缓存绑连接/认证 epoch；未知→省略 limit=服务端默认 50） ──

const LEGAL_PAGE_LIMITS = [50, 100, 150, 200];
const capabilityByEpoch = new Map<string, { recommendedLimit: number }>();
const overviewRecommendedByPath = new Map<string, number>();

function noteCapabilityFromResponse(res: any, epoch: string): void {
  if (headerGet(res, 'lingxi-history-protocol') !== String(HISTORY_PROTOCOL_VERSION)) return;
  const raw = headerGet(res, 'lingxi-history-page-limit');
  if (raw == null) return;
  const v = Number(raw);
  if (!LEGAL_PAGE_LIMITS.includes(v)) return; // 非法/超限 → 不采纳
  capabilityByEpoch.set(epoch, { recommendedLimit: v });
}

/** E04 概览校验成功后回填概览建议（与头建议冲突→保守 50，见 negotiatedHistoryPageLimit）。 */
export function noteOverviewRecommendedLimit(sessionPath: string, limit: number): void {
  if (LEGAL_PAGE_LIMITS.includes(limit)) overviewRecommendedByPath.set(sessionPath, limit);
}

/**
 * 当前会话的协商页大小：未知能力 → null（请求省略 limit=服务端默认 50）；
 * 头建议与概览建议冲突 → 保守 50 并输出诊断。
 */
export function negotiatedHistoryPageLimit(sessionPath: string): number | null {
  const state = useStore.getState() as Record<string, any>;
  let epoch: string;
  try {
    epoch = connectionEpoch(requireServerConnection(state, 'negotiated page limit: no connection'));
  } catch {
    return null;
  }
  const cap = capabilityByEpoch.get(epoch)?.recommendedLimit ?? null;
  const ov = overviewRecommendedByPath.get(sessionPath) ?? null;
  if (cap != null && ov != null && cap !== ov) {
    console.warn(`[history-protocol] 页大小建议冲突：头=${cap} 概览=${ov} → 保守 50（需修复服务端同源配置）`);
    return 50;
  }
  return cap ?? ov ?? null;
}

export async function conditionalMessagesFetch(
  sessionPath: string,
  input: {
    sessionPath?: string;
    url: string;
    sessionId: string | null;
    limit: number;
    /** 发送时捕获的 load 请求版本（bumpLoadMessagesVersion；用于 supersede 判定）。 */
    requestVersion: number;
    /** 已应用的 messages live 版本（readMessageLiveVersion；304 新鲜度判定）。 */
    appliedLiveVersion: number;
  },
): Promise<ConditionalMessagesOutcome> {
  console.error('[cmf-entry] path=', sessionPath.slice(-24), 'records.size=', records.size);
  const state = useStore.getState() as Record<string, any>;

  console.error('[cmf-dbg] called size=', records.size, 'url=', input.url.slice(0, 60));
  // 快路径：从未建立过任何校验记录（首载/旧服务端）→ 纯无条件请求，
  // 不触碰连接解析（既有无连接环境/测试行为完全不变）。
  if (records.size === 0) {
    const res = await lingxiFetch(input.url);
    // 无记录也捕获能力（供后续新页协商 K）；无连接环境跳过（try 吞掉）
    try {
      noteCapabilityFromResponse(res, connectionEpoch(requireServerConnection(useStore.getState() as Record<string, any>, 'x')));
    } catch { /* 无连接：无法构造作用域 → 跳过能力捕获 */ }
    return { kind: 'ok', response: res, etag: headerGet(res, 'etag'), requestKey: '', conditionalTried: false };
  }

  let epoch = 'no-connection';
  let requestKey = buildHistoryRequestKey({ epoch, sessionId: input.sessionId, sessionPath, before: null, limit: input.limit });
  let record: HistoryValidationRecord | undefined;
  try {
    const connection = requireServerConnection(state, 'history protocol conditional fetch: server connection not ready');
    epoch = connectionEpoch(connection);
    requestKey = buildHistoryRequestKey({
      epoch,
      sessionId: input.sessionId,
      sessionPath,
      before: null,
      limit: input.limit,
    });
    record = records.get(requestKey);
  } catch {
    record = undefined; // 连接不可用 → 无条件请求（语义与旧行为一致）
  }
  const todosVersion = ((state.todosLiveVersionBySession as Record<string, any> | undefined)?.[sessionPath] ?? 0);
  void todosVersion;

  console.error('[cmf-dbg] size=', records.size, 'hasRecord=', !!record, 'disabled=', isEpochConditionalDisabled(epoch));
  // 无记录（或该 epoch 已知不支持条件请求）→ 无条件请求（错误语义与 lingxiFetch
  // 默认一致：非 2xx 抛错，由调用方既有 catch 承接）。
  if (!record || isEpochConditionalDisabled(epoch)) {
    const res = await lingxiFetch(input.url);
    return { kind: 'ok', response: res, etag: res.headers.get('etag'), requestKey: buildHistoryRequestKey({ epoch, sessionId: input.sessionId, sessionPath, before: null, limit: input.limit }), conditionalTried: false };
  }

  let res: any;
  try {
    res = await lingxiFetch(input.url, {
      headers: { 'if-none-match': record.etag, 'cache-control': 'no-store' },
      throwOnHttpError: false,
    });
  } catch (err) {
    // 传输失败（网络错误等）：同目标一次无条件回退；不标记 epoch（一般网络错误
    // 不是「不支持」证据，E03.5）。回退仍失败 → 异常按既有错误流程冒出。
    const retry = await lingxiFetch(input.url);
    return { kind: 'ok', response: retry, etag: headerGet(retry, 'etag'), requestKey, conditionalTried: true };
  }
  if (statusOf(res) === 400) {
    // 条件头被服务端/旧代理拒绝（E03.5）：标记该 epoch 暂不条件 + 一次无条件回退。
    conditionalDisabledEpochs.add(epoch);
    const retry = await lingxiFetch(input.url);
    noteCapabilityFromResponse(retry, epoch);
    return { kind: 'ok', response: retry, etag: headerGet(retry, 'etag'), requestKey, conditionalTried: true };
  }
  if (statusOf(res) === 304) {
    // E07 故障场景：304 缺 ETag 头 → 无法确认对应表示 → 按 stale 无条件补取。
    if (!headerGet(res, 'etag')) {
      const retry = await lingxiFetch(input.url);
      return { kind: 'ok', response: retry, etag: headerGet(retry, 'etag'), requestKey, conditionalTried: true };
    }
    // E03.3：新鲜状态重查（连接/身份/请求版本/已应用版本，非发请求前快照）。
    const guard = freshStateGuards(sessionPath, record, input.requestVersion, input.appliedLiveVersion);
    if (guard.outcome === 'valid') {
      record.savedAt = Date.now();
      return { kind: 'not-modified', requestKey };
    }
    if (guard.outcome === 'superseded') return { kind: 'superseded' };
    // 守卫失效（stale：WS/todo/覆盖漂移）→ 至多一次无条件补取
    //（补取不带条件头，禁止 304→补取→304 循环）。
    const retry = await lingxiFetch(input.url);
    return { kind: 'ok', response: retry, etag: headerGet(retry, 'etag'), requestKey, conditionalTried: true };
  }
  if (typeof res.ok === 'boolean' && !res.ok) {
    // 401/403/404/5xx：按现有错误流程抛出（不归类为「不支持」，不标 epoch）。
    throw new Error(`lingxiFetch ${input.url}: ${statusOf(res)}`);
  }
  noteCapabilityFromResponse(res, epoch);
  return { kind: 'ok', response: res, etag: headerGet(res, 'etag'), requestKey, conditionalTried: true };
}

export interface SaveHistoryValidationRecordInput {
  url: string;
  sessionPath: string;
  sessionId: string | null;
  limit: number;
  /** 应用完成时刻的 messages live 版本（304 新鲜度判定基准）。 */
  appliedLiveVersion: number;
  etag: string | null;
  protocolHeader: string | null;
  /** 200 的完整业务 JSON（messages 为原始记录数组）。 */
  data: {
    messages?: Array<{ id?: string }> | null;
    hasMore?: boolean;
    nextBefore?: string | null;
    revision?: string | null;
  };
  todosVersion: number;
}

/**
 * E03.2：仅在调用方完成 JSON 校验 + stale 判定 + build/merge + todos/files 更新后
 * 原子保存。etag 或协议能力头缺失/未知版本 → 不保存（清除不可信能力）。
 */
export function saveHistoryValidationRecord(
  sessionPath: string,
  input: SaveHistoryValidationRecordInput,
): void {
  console.error('[save-dbg] etag=', input.etag, 'proto=', input.protocolHeader, 'limit=', input.limit);
  if (!input.etag) return;
  let connection: any;
  try {
    connection = requireServerConnection(useStore.getState() as Record<string, any>, 'history protocol save: server connection not ready');
  } catch {
    return; // 连接不可用 → 无法构造作用域键 → 不保存（重校验走无条件路径）
  }
  if (input.protocolHeader !== String(HISTORY_PROTOCOL_VERSION)) return;
  const messages = Array.isArray(input.data.messages) ? input.data.messages : [];
  if (messages.length === 0) return; // 空页不构成覆盖证明
  const epoch = connectionEpoch(connection);
  const requestKey = buildHistoryRequestKey({
    epoch,
    sessionId: input.sessionId,
    sessionPath: input.sessionPath,
    before: null,
    limit: input.limit,
  });
  const first = messages[0]?.id;
  const last = messages[messages.length - 1]?.id;
  if (typeof first !== 'string' || typeof last !== 'string') return;
  saveRecord({
    requestKey,
    etag: input.etag,
    connectionEpoch: epoch,
    sessionId: input.sessionId,
    sessionPath: input.sessionPath,
    appliedMessageVersion: input.appliedLiveVersion,
    appliedTodosVersion: input.todosVersion,
    coverage: {
      firstRecordId: first,
      lastRecordId: last,
      recordCount: messages.length,
      hasMore: input.data.hasMore ?? false,
      nextBefore: typeof input.data.nextBefore === 'string' ? input.data.nextBefore : null,
      revision: typeof input.data.revision === 'string' ? input.data.revision : '',
    },
    protocolVersion: HISTORY_PROTOCOL_VERSION,
    recommendedLimit: HISTORY_VALIDATION_RECOMMENDED_LIMIT,
    savedAt: Date.now(),
  });
}

export const HISTORY_VALIDATION_RECOMMENDED_LIMIT = 50;
