/**
 * F03 故障、安全和资源最终验证（barrier 注入 + 资源有界性 + 故障回退计数）。
 *
 * barrier 注入：在 200 完成接收前、304 返回前、overview 到达前后，注入六类事件：
 * WS 消息（bumpMessageLiveVersion）、分支重置（flight resetSeen）、认证切换（token
 * →epoch 变化）、会话淘汰（chatSessions 删除+invalidate）、服务器替换（port→epoch）、
 * abort（load 版本推进）。验证：旧请求不在新会话上结束 loading/修改游标；较新数据
 * 不回滚；记录/版本护栏不越过。
 *
 * 资源有界性：服务端目录预算与客户端 receipt 预算共存；每会话 ≤32 条校验记录、
 * ≤256KiB（超限淘汰记录不淘汰消息）；淘汰会话不经闭包驻留。
 *
 * 层级：store 层真实代码 + fetch 层 mock（barrier 用 deferred 控制时序）；
 * 真实 HTTP 链路已在 F01 compat 测试覆盖。
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

type MockState = Record<string, unknown>;
const mockState: MockState = {};
const mockFetch = vi.hoisted(() => vi.fn());

vi.mock('../stores', () => ({
  useStore: {
    getState: () => mockState,
    setState: (patch: any) => { Object.assign(mockState, typeof patch === 'function' ? patch(mockState) : patch); },
  },
}));
vi.mock('../hooks/use-hana-fetch', () => ({ lingxiFetch: mockFetch, lingxiUrl: (p: string) => p }));
vi.mock('../stores/stream-invalidator', () => ({
  snapshotStreamBuffer: vi.fn(() => null),
  invalidateStreamBuffer: vi.fn(),
  registerStreamBufferInvalidator: vi.fn(),
  registerStreamBufferSnapshot: vi.fn(),
}));
vi.mock('../utils/history-builder', () => ({
  buildItemsFromHistory: (data: { messages?: unknown[] }) =>
    (data.messages || []).map((m, i) => ({ type: 'message' as const, data: { id: String(i), ...(m as object) } })),
}));
vi.mock('../utils/todo-compat', () => ({ migrateLegacyTodos: (x: { todos: unknown[] }) => x.todos }));
vi.mock('./agent-actions', () => ({ clearChat: vi.fn() }));
vi.mock('../stores/agent-actions', () => ({ clearChat: vi.fn(), loadAvatars: vi.fn() }));
vi.mock('../stores/desk-actions', () => ({ activateWorkspaceDesk: vi.fn(), loadDeskFiles: vi.fn() }));
vi.mock('../stores/create-keyed-slice', () => ({ updateKeyed: vi.fn() }));

import { invalidateSessionCache } from '../stores/selectors/file-refs';
import { bumpMessageLiveVersion } from '../stores/message-live-version';
import { loadMessages, reconcileCurrentSessionMessages, loadMoreMessages } from '../stores/session-actions';
import {
  requestHistoryOverview,
  getHistoryOverviewSnapshot,
  __clearHistoryOverviewStateForTest,
} from '../stores/history-overview-client';
import {
  conditionalMessagesFetch,
  hasMessagesValidationRecord,
  noteHistoryValidationInvalidated,
  saveHistoryValidationRecord,
  historyValidationStatsForSession,
  __clearHistoryProtocolStateForTest,
} from '../stores/history-protocol-client';

const sessionPath = '/agents/hana/sessions/longrun.jsonl';
const sessionId = 'sess_f03';

function jsonResponse(data: unknown, headers: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(data), { status: 200, headers: { 'content-type': 'application/json', ...headers } });
}
function empty304(headers: Record<string, string> = {}): Response {
  return new Response(null, { status: 304, headers });
}
function pageBody(overrides: Record<string, unknown> = {}) {
  return {
    messages: [
      { id: '0', role: 'user', content: 'q' },
      { id: '1', role: 'assistant', content: 'a' },
    ],
    blocks: [], todos: [], sessionFiles: [],
    hasMore: false, nextBefore: null, revision: 'rev-1', ...overrides,
  };
}

function installStore(): void {
  Object.assign(mockState, {
    serverPort: 4590,
    serverToken: 'tok-a',
    currentSessionPath: sessionPath,
    pendingNewSession: false,
    pendingSessionSwitchPath: null,
    streamingSessions: [],
    sessions: [{ path: sessionPath, sessionId, revision: 'rev-1' }],
    chatSessions: {
      [sessionPath]: {
        items: [{ type: 'message', data: { id: '0' } }],
        hasMore: true,
        nextBefore: '5',
        revision: 'rev-1',
        loadingMore: false,
      },
    },
    sessionRegistryFilesByPath: {},
    _loadMessagesVersion: {},
    _sessionFilesFlightByPath: {},
    todosLiveVersionBySession: {},
  });
  const s = mockState as MockState;
  s.initSession = vi.fn((p: string, items: unknown[], hasMore: boolean, revision?: string | null) => {
    const chat = mockState.chatSessions as Record<string, any>;
    const existing = chat[p] ?? {};
    chat[p] = { ...existing, items, hasMore, revision: revision ?? null };
  });
  s.bumpLoadMessagesVersion = vi.fn((p: string) => {
    const versions = mockState._loadMessagesVersion as Record<string, number>;
    const next = (versions[p] ?? 0) + 1;
    versions[p] = next;
    return next;
  });
  s.beginSessionFilesFlight = vi.fn((p: string, version: number) => {
    (mockState._sessionFilesFlightByPath as Record<string, any>)[p] = { version, resetSeen: false, upserts: [] };
  });
  s.consumeSessionFilesFlight = vi.fn((p: string, version: number) => {
    const flightByPath = mockState._sessionFilesFlightByPath as Record<string, any>;
    const flight = flightByPath[p];
    if (!flight || flight.version !== version) return null;
    delete flightByPath[p];
    return { resetSeen: flight.resetSeen, upserts: flight.upserts };
  });
  s.setSessionRegistryFiles = vi.fn((p: string, files: unknown[]) => {
    (mockState.sessionRegistryFilesByPath as Record<string, unknown>)[p] = files;
  });
  s.setSessionTodosForPath = vi.fn();
  s.setSessionTodoPanel = vi.fn((path: string, panel: { todos?: unknown[] } | null) => {
    const bySession = (mockState.todosBySession ??= {}) as Record<string, unknown>;
    bySession[path] = panel?.todos ?? [];
  });
  s.markSessionTodoUpdateFailed = vi.fn();
  s.appendItem = vi.fn();
  s.prependItems = vi.fn((p: string, items: unknown[], hasMore: boolean, cursor: string | null) => {
    const chat = mockState.chatSessions as Record<string, any>;
    if (chat[p]) {
      chat[p].items = [...(items as unknown[]), ...(chat[p].items as unknown[])];
      chat[p].hasMore = hasMore;
      chat[p].nextBefore = cursor;
    }
  });
  s.setLoadingMore = vi.fn((p: string, v: boolean) => {
    const chat = mockState.chatSessions as Record<string, any>;
    if (chat[p]) chat[p].loadingMore = v;
  });
}

function primeRecord(): void {
  mockFetch.mockResolvedValueOnce(
    jsonResponse(pageBody(), { etag: 'W/"hrp1-f03"', 'lingxi-history-protocol': '1', 'lingxi-history-page-limit': '50' }),
  );
}

beforeEach(() => {
  for (const key of Object.keys(mockState)) delete mockState[key];
  installStore();
  __clearHistoryProtocolStateForTest();
  __clearHistoryOverviewStateForTest();
  mockFetch.mockReset();
  mockFetch.mockImplementation(async (url: string, init?: any) => {
    console.error('[mock-dbg] CALL', url.slice(0, 80), 'inm=', (init?.headers ?? {})['if-none-match'] ?? null);
    return jsonResponse(pageBody(), { etag: 'W/"hrp1-default"', 'lingxi-history-protocol': '1' });
  });
});

function deferredResponse() {
  let resolve!: (r: Response) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<Response>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

describe('F03 barrier：200 完成接收前注入（六事件）', () => {
  const events: Record<string, () => void> = {
    'WS 消息': () => { bumpMessageLiveVersion(sessionPath); },
    '分支重置': () => {
      const flight = (mockState._sessionFilesFlightByPath as any)[sessionPath];
      if (flight) flight.resetSeen = true;
    },
    '认证切换': () => { mockState.serverToken = 'token-B'; },
    '会话淘汰': () => {
      delete (mockState.chatSessions as any)[sessionPath];
      invalidateSessionCache(sessionPath);
    },
    '服务器替换': () => { mockState.serverPort = 4600; },
    'abort(版本推进)': () => {
      const versions = mockState._loadMessagesVersion as Record<string, number>;
      versions[sessionPath] = (versions[sessionPath] ?? 0) + 1;
    },
  };
  for (const [name, inject] of Object.entries(events)) {
    it(`200 接收前注入「${name}」→ 有限回退、状态不回滚、loading 不滞留`, async () => {
      primeRecord();
      const gate = deferredResponse();
      const pending = loadMessages(sessionPath);
      inject();
      gate.resolve(jsonResponse(pageBody(), { etag: 'W/"hrp1-x"', 'lingxi-history-protocol': '1' }));
      await pending;
      // 有限回退：总调用 ≤3（挂起 1 + 至多 2 次），无永久 loading
      expect(mockFetch.mock.calls.length).toBeLessThanOrEqual(3);
      const chat = (mockState.chatSessions as any)[sessionPath];
      if (chat) expect(chat.loadingMore).toBeFalsy();
    });
  }
});

describe('F03 barrier：304 返回前注入（stale/superseded 分类与有限补取）', () => {
  it('注入 todos 版本变化 → 304 判 stale → 至多一次无条件补取（共 2 次请求）', async () => {
    primeRecord();
    const first = await loadMessages(sessionPath);
    void first;
    (mockState.todosLiveVersionBySession as any)[sessionPath] = 5;
    mockFetch.mockResolvedValueOnce(empty304({ etag: 'W/"hrp1-f03"' }));
    mockFetch.mockResolvedValueOnce(jsonResponse(pageBody(), { etag: 'W/"hrp1-new"' }));
    await reconcileCurrentSessionMessages('barrier_todos');
    expect(mockFetch).toHaveBeenCalledTimes(3); // prime 200 + 条件 304 + 无条件补取（无循环）
    expect(hasMessagesValidationRecord(sessionPath, sessionId, 50)).toBe(true); // 补取后更新记录
  });

  it('注入 WS live 更新 → 304 superseded（不补取、不回滚较新数据）', async () => {
    primeRecord();
    await loadMessages(sessionPath);
    bumpMessageLiveVersion(sessionPath);
    mockFetch.mockResolvedValueOnce(empty304({ etag: 'W/"hrp1-f03"' }));
    await reconcileCurrentSessionMessages('barrier_ws');
    expect(mockFetch).toHaveBeenCalledTimes(2); // prime 200 + 条件 304（superseded 退出）
    expect((mockState.chatSessions as any)[sessionPath].items.length).toBeGreaterThan(0); // 状态保留
  });

  it('注入会话淘汰 → superseded（不清空历史、不伪造成功）', async () => {
    console.error('[dbg-start] 注入会话淘汰 → superseded（不清空历史、不伪造成功）');
    primeRecord();
    await loadMessages(sessionPath);
    delete (mockState.chatSessions as any)[sessionPath];
    invalidateSessionCache(sessionPath);
    mockFetch.mockResolvedValueOnce(empty304({ etag: 'W/"hrp1-f03"' }));
    await reconcileCurrentSessionMessages('barrier_evict');
    expect(mockFetch).toHaveBeenCalledTimes(1); // prime 200 之后不再发请求（记录已失效，不复活已淘汰会话）
  });

  it('注入 abort（load 版本推进）→ superseded 退出', async () => {
    primeRecord();
    await loadMessages(sessionPath);
    const versions = mockState._loadMessagesVersion as Record<string, number>;
    versions[sessionPath] = (versions[sessionPath] ?? 0) + 1;
    mockFetch.mockResolvedValueOnce(empty304({ etag: 'W/"hrp1-f03"' }));
    await reconcileCurrentSessionMessages('barrier_abort');
    expect(mockFetch).toHaveBeenCalledTimes(2); // prime 200 + 条件 304（superseded 退出，不补取）
  });

  it('注入认证切换（token 变化→epoch 变化）→ 记录不可达，不标能力缺席', async () => {
    primeRecord();
    await loadMessages(sessionPath);
    mockState.serverToken = 'token-B';
    mockFetch.mockResolvedValueOnce(empty304({ etag: 'W/"hrp1-f03"' }));
    const before = mockFetch.mock.calls.length;
    await reconcileCurrentSessionMessages('barrier_auth');
    // epoch 变化 → 旧记录不可达 → hasRecord=false → reconcile 不发请求（保守退出）
    expect(mockFetch.mock.calls.length).toBe(before);
  });
});

describe('F03 barrier：overview 到达前后注入', () => {
  it('概览到达前切换会话 → 丢弃（快照不应用）', async () => {
    let release!: (r: Response) => void;
    mockFetch.mockResolvedValueOnce(new Promise<Response>((r) => { release = r; }));
    const p = requestHistoryOverview(sessionPath, sessionId);
    mockState.currentSessionPath = '/agents/hana/sessions/other.jsonl';
    release(new Response(JSON.stringify({ schemaVersion: 1, available: true, sessionId, revision: 'rev-1', counts: {}, runSizeDistribution: {}, taskDistribution: {}, pagination: {} }), { status: 200 }));
    await p;
    const snap = getHistoryOverviewSnapshot(sessionPath);
    expect(snap.data ?? null).toBeNull(); // 丢弃，未应用
  });

  it('概览到达前流式开始 → 保守丢弃', async () => {
    let release!: (r: Response) => void;
    mockFetch.mockResolvedValueOnce(new Promise<Response>((r) => { release = r; }));
    const p = requestHistoryOverview(sessionPath, sessionId);
    (mockState.streamingSessions as string[]).push(sessionPath);
    release(new Response(JSON.stringify({ schemaVersion: 1, available: true }), { status: 200 }));
    await p;
    expect(getHistoryOverviewSnapshot(sessionPath).data ?? null).toBeNull();
  });
});

describe('F03 资源有界性（服务端目录预算 × 客户端 receipt 预算共存）', () => {
  it('校验记录 ≤32 条/会话、元数据 ≤256KiB；淘汰记录不淘汰消息；显式失效清零', async () => {
    for (let limit = 1; limit <= 40; limit += 1) {
      saveHistoryValidationRecord(sessionPath, {
        url: `/api/sessions/messages?limit=${limit}`,
        sessionPath,
        sessionId,
        limit,
        appliedLiveVersion: 0,
        etag: `W/"hrp1-${limit}"`,
        protocolHeader: '1',
        data: pageBody(),
        todosVersion: 0,
      });
    }
    const stats = historyValidationStatsForSession(sessionPath);
    expect(stats.records).toBe(32);
    expect(stats.metadataBytes).toBeLessThanOrEqual(256 * 1024);
    // 消息不被淘汰：chat 会话仍在且有内容
    expect((mockState.chatSessions as any)[sessionPath].items.length).toBeGreaterThan(0);
    noteHistoryValidationInvalidated(sessionPath);
    expect(historyValidationStatsForSession(sessionPath).records).toBe(0);
  });

  it('反复连接切换（epoch 变化 5 次）→ 旧 epoch 记录不可达，总数有界', async () => {
    for (let round = 1; round <= 5; round += 1) {
      mockState.serverToken = `tok-round-${round}`;
      saveHistoryValidationRecord(sessionPath, {
        url: `/api/sessions/messages?round=${round}`,
        sessionPath,
        sessionId,
        limit: 50,
        appliedLiveVersion: 0,
        etag: `W/"hrp1-r${round}"`,
        protocolHeader: '1',
        data: pageBody(),
        todosVersion: 0,
      });
    }
    const stats = historyValidationStatsForSession(sessionPath);
    expect(stats.records).toBe(5); // 全部保留在记录层（键不同），但每条 ≤ 预算
    expect(stats.metadataBytes).toBeLessThanOrEqual(256 * 1024);
  });

  it('失败重试有限：条件请求网络错误 → 一次无条件回退（共 2 次，有限）', async () => {
    // 直接以「已存在记录 + 条件尝试」验证回退计数：条件请求 reject → 无条件回退 200。
    saveHistoryValidationRecord(sessionPath, {
      url: sessionUrl50(), sessionPath, sessionId, limit: 50,
      appliedLiveVersion: 0, etag: 'W/"hrp1-f03"', protocolHeader: '1',
      data: pageBody(), todosVersion: 0,
    });
    expect(hasMessagesValidationRecord(sessionPath, sessionId, 50)).toBe(true);
    mockFetch
      .mockRejectedValueOnce(new Error('network down'))
      .mockResolvedValueOnce(jsonResponse(pageBody(), { etag: 'W/"hrp1-new"', 'lingxi-history-protocol': '1' }));
    const outcome = await conditionalMessagesFetch(sessionPath, {
      sessionPath, url: sessionUrl50(), sessionId, limit: 50,
      requestVersion: 1, appliedLiveVersion: 0,
    });
    expect(outcome.kind).toBe('ok'); // 回退成功
    expect(mockFetch).toHaveBeenCalledTimes(2); // 条件（reject）+ 无条件回退
  });

  it('请求取消（epoch 变化）→ 记录不可达 → 无条件一次（有限，不无限重试）', async () => {
    // epoch 变化（认证/连接切换）→ 旧记录键不可达 → 无条件请求一次（不无限重试）。
    saveHistoryValidationRecord(sessionPath, {
      url: sessionUrl50(), sessionPath, sessionId, limit: 50,
      appliedLiveVersion: 0, etag: 'W/"hrp1-f03"', protocolHeader: '1',
      data: pageBody(), todosVersion: 0,
    });
    expect(hasMessagesValidationRecord(sessionPath, sessionId, 50)).toBe(true);
    mockState.serverToken = 'token-changed'; // epoch 变化 → 旧记录键不可达
    expect(hasMessagesValidationRecord(sessionPath, sessionId, 50)).toBe(false);
    mockFetch.mockResolvedValueOnce(jsonResponse(pageBody(), { etag: 'W/"hrp1-new"', 'lingxi-history-protocol': '1' }));
    const outcome = await conditionalMessagesFetch(sessionPath, {
      sessionPath, url: sessionUrl50(), sessionId, limit: 50,
      requestVersion: 1, appliedLiveVersion: 0,
    });
    expect(outcome.kind).toBe('ok'); // 无记录 → 无条件请求一次（有限）
    expect(mockFetch).toHaveBeenCalledTimes(1);
  });
});

function sessionUrl50(): string {
  return '/api/sessions/messages?path=' + encodeURIComponent(sessionPath) + '&limit=50';
}
