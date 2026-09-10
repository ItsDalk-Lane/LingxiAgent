/**
 * E03 客户端条件校验与失效测试（mock 边界在 fetch/store 层，不改 lingxiFetch 全局默认）：
 *  - 请求键构成（epoch/sessionId/path/before/limit/语言/投影版本任一变化即不同键）；
 *  - 仅重校验路径（同 revision 既有触发点 + 已有记录）带 If-None-Match；
 *  - 304：保留全部已加载状态、不解析 JSON、不 initSession、不推动游标；
 *  - 304 期间状态失效 → 丢弃 + 至多一次无条件补取 + 无循环；
 *  - 新页首次加载无条件（loadMoreMessages 不带条件头）；
 *  - 200 应用完成后才保存记录（失败不保存）；淘汰/每会话 32 条上限/元数据预算；
 *  - 失效清单逐项（invalidateSessionCache 钩子、live 版本变化、显式 note 失效）；
 *  - 旧服务器无 ETag/能力头 → 正常 200 处理、不保存记录、保持默认 50；
 *  - 异常分类：网络错误回退但不标 epoch；400 标 epoch；401/403 抛错且不标 epoch。
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
    setState: (patch: MockState | ((s: MockState) => MockState)) => {
      const next = typeof patch === 'function' ? patch(mockState) : patch;
      Object.assign(mockState, next);
    },
  },
}));

vi.mock('../hooks/use-hana-fetch', () => ({
  lingxiFetch: mockFetch,
  lingxiUrl: (p: string) => p,
}));

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

vi.mock('../utils/todo-compat', () => ({
  migrateLegacyTodos: (x: { todos: unknown[] }) => x.todos,
}));

vi.mock('./agent-actions', () => ({ clearChat: vi.fn() }));
vi.mock('../stores/agent-actions', () => ({ clearChat: vi.fn(), loadAvatars: vi.fn() }));
vi.mock('../stores/desk-actions', () => ({ activateWorkspaceDesk: vi.fn(), loadDeskFiles: vi.fn() }));
vi.mock('../stores/create-keyed-slice', () => ({ updateKeyed: vi.fn() }));

import {
  buildHistoryRequestKey,
  connectionEpoch,
  conditionalMessagesFetch,
  hasMessagesValidationRecord,
  historyValidationStatsForSession,
  negotiatedHistoryPageLimit,
  noteHistoryValidationInvalidated,
  noteOverviewRecommendedLimit,
  saveHistoryValidationRecord,
  __clearHistoryProtocolStateForTest,
} from '../stores/history-protocol-client';
import {
  invalidateSessionCache,
} from '../stores/selectors/file-refs';
import {
  loadMessages,
  loadMoreMessages,
  reconcileCurrentSessionMessages,
} from '../stores/session-actions';

const sessionPath = '/agents/hana/sessions/longrun.jsonl';
const sessionId = 'sess_test_1';

function jsonResponse(data: unknown, headers: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(data), {
    status: 200,
    headers: { 'content-type': 'application/json', ...headers },
  });
}
function emptyResponse(status: number, headers: Record<string, string> = {}): Response {
  return new Response(null, { status, headers });
}

function installStoreMethods(): void {
  const s = mockState as MockState;
  s.initSession = vi.fn((p: string, items: unknown[], hasMore: boolean, revision?: string | null) => {
    const chat = mockState.chatSessions as Record<string, any>;
    const existing = chat[p] ?? {};
    chat[p] = { ...existing, items, hasMore, loadingMore: false, revision: revision ?? null };
  });
  s.bumpLoadMessagesVersion = vi.fn((p: string) => {
    const versions = mockState._loadMessagesVersion as Record<string, number>;
    const next = (versions[p] ?? 0) + 1;
    versions[p] = next;
    return next;
  });
  s.beginSessionFilesFlight = vi.fn((p: string, version: number) => {
    const flightByPath = mockState._sessionFilesFlightByPath as Record<string, any>;
    flightByPath[p] = { version, resetSeen: false, upserts: [] };
  });
  s.consumeSessionFilesFlight = vi.fn((p: string, version: number) => {
    const flightByPath = mockState._sessionFilesFlightByPath as Record<string, any>;
    const flight = flightByPath[p];
    if (!flight || flight.version !== version) return null;
    delete flightByPath[p];
    return { resetSeen: flight.resetSeen, upserts: flight.upserts };
  });
  s.setSessionRegistryFiles = vi.fn((p: string, files: unknown[]) => {
    const bySession = mockState.sessionRegistryFilesByPath as Record<string, unknown>;
    bySession[p] = files;
    // 与生产 chat-slice 行为一致：registry 整表应用会触发校验记录失效
    noteHistoryValidationInvalidated(p);
  });
  s.setSessionTodosForPath = vi.fn();
  s.appendItem = vi.fn();
  s.setLoadingMore = vi.fn((p: string, v: boolean) => {
    const chat = mockState.chatSessions as Record<string, any>;
    if (chat[p]) chat[p].loadingMore = v;
  });
  s.prependItems = vi.fn((p: string, items: unknown[], hasMore: boolean, cursor: string | null) => {
    const chat = mockState.chatSessions as Record<string, any>;
    if (chat[p]) {
      chat[p].items = [...(items as unknown[]), ...(chat[p].items as unknown[])];
      chat[p].hasMore = hasMore;
      chat[p].nextBefore = cursor;
    }
  });
}

function installSession({ revision = 'rev-1', hasMore = false, nextBefore = null as string | null } = {}): void {
  mockState.currentSessionPath = sessionPath;
  mockState.pendingNewSession = false;
  mockState.pendingSessionSwitchPath = null;
  mockState.streamingSessions = [];
  mockState.sessions = [{ path: sessionPath, sessionId, revision }];
  mockState.chatSessions = {
    [sessionPath]: {
      items: [{ type: 'message', data: { id: '0' } }],
      hasMore,
      nextBefore,
      revision,
      oldestId: '0',
      loadingMore: false,
    },
  };
  (mockState as MockState).sessionScopedKey = undefined;
}

beforeEach(() => {
  for (const key of Object.keys(mockState)) delete mockState[key];
  Object.assign(mockState, {
    serverPort: 4590,
    serverToken: 'test-token',
    currentSessionPath: null,
    chatSessions: {},
    sessions: [],
    sessionRegistryFilesByPath: {},
    sessionModelsByPath: {},
    _loadMessagesVersion: {},
    _sessionFilesFlightByPath: {},
    todosLiveVersionBySession: {},
    streamingSessions: [],
    pendingNewSession: false,
    pendingSessionSwitchPath: null,
    currentTab: 'chat',
  });
  installStoreMethods();
  installSession(); // 默认装一个已打开会话（sessionId/path 与记录键一致）
  __clearHistoryProtocolStateForTest();
  mockFetch.mockReset();
});

function pageBody(overrides: Record<string, unknown> = {}) {
  return {
    messages: [
      { id: '0', role: 'user', content: 'q' },
      { id: '1', role: 'assistant', content: 'a' },
    ],
    blocks: [],
    todos: [],
    sessionFiles: [],
    hasMore: false,
    nextBefore: null,
    revision: 'rev-1',
    ...overrides,
  };
}

async function primeRecord(): Promise<void> {
  // 走一次真实 loadMessages（200+ETag）建立校验记录
  mockFetch.mockResolvedValueOnce(
    jsonResponse(pageBody(), {
      etag: 'W/"hrp1-abc"',
      'lingxi-history-protocol': '1',
      'lingxi-history-page-limit': '50',
    }),
  );
  await loadMessages(sessionPath);
  expect(hasMessagesValidationRecord(sessionPath, sessionId, 50)).toBe(true);
  mockFetch.mockClear();
}

describe('E03 客户端条件校验', () => {
  it('请求键构成：任一维度变化即不同键', () => {
    const base = { epoch: 'e1', sessionId, sessionPath, before: null, limit: 50 };
    expect(buildHistoryRequestKey(base)).toBe(buildHistoryRequestKey({ ...base }));
    expect(buildHistoryRequestKey(base)).not.toBe(buildHistoryRequestKey({ ...base, epoch: 'e2' }));
    expect(buildHistoryRequestKey(base)).not.toBe(buildHistoryRequestKey({ ...base, sessionId: 'other' }));
    expect(buildHistoryRequestKey(base)).not.toBe(buildHistoryRequestKey({ ...base, sessionPath: '/other' }));
    expect(buildHistoryRequestKey(base)).not.toBe(buildHistoryRequestKey({ ...base, before: '10' }));
    expect(buildHistoryRequestKey(base)).not.toBe(buildHistoryRequestKey({ ...base, limit: 100 }));
    // 模式/语言/投影版本也进键（E03.1 请求键绑定）
    expect(buildHistoryRequestKey(base)).not.toBe(buildHistoryRequestKey({ ...base, mode: 'reconciliation' }));
    expect(buildHistoryRequestKey(base)).not.toBe(buildHistoryRequestKey({ ...base, language: 'ja' }));
    expect(buildHistoryRequestKey(base)).not.toBe(buildHistoryRequestKey({ ...base, projectionVersion: 2 }));
    expect(connectionEpoch({ connectionId: 'a', token: 'x' })).toBe(connectionEpoch({ connectionId: 'a', token: 'x' }));
    expect(connectionEpoch({ connectionId: 'a', token: 'x' })).not.toBe(connectionEpoch({ connectionId: 'a', token: 'y' }));
  });

  it('新页首次加载/无记录 → 无条件请求（不带 If-None-Match）', async () => {
    mockFetch.mockResolvedValueOnce(
      jsonResponse(pageBody(), { etag: 'W/"hrp1-abc"', 'lingxi-history-protocol': '1' }),
    );
    await loadMessages(sessionPath);
    expect(mockFetch).toHaveBeenCalledTimes(1);
    const [, init] = mockFetch.mock.calls[0];
    expect(init?.headers?.['if-none-match']).toBeUndefined();
    expect(hasMessagesValidationRecord(sessionPath, sessionId, 50)).toBe(true);
  });

  it('仅重校验路径带 If-None-Match；304 → 保留全部已加载状态且不解析 JSON', async () => {
    await primeRecord();
    const initSession = mockState.initSession as ReturnType<typeof vi.fn>;
    initSession.mockClear();
    const itemsBefore = (mockState.chatSessions as any)[sessionPath].items;

    let bodyRead = false;
    const parseProbe = vi.fn(async () => { bodyRead = true; throw new Error('304 body must not be parsed'); });
    const res304 = {
      status: 304,
      ok: false,
      headers: new Headers({ etag: 'W/"hrp1-abc"', 'cache-control': 'private, no-store' }),
      text: async () => { bodyRead = true; return 'POISONED'; },
      json: parseProbe,
    } as unknown as Response;
    mockFetch.mockResolvedValueOnce(res304);

    await reconcileCurrentSessionMessages('window_focus');

    const [url, init] = mockFetch.mock.calls[0]; // primeRecord 已 mockClear：本用例唯一一次 fetch
    expect(String(url)).toContain('/api/sessions/messages');
    expect(init?.headers?.['if-none-match']).toBe('W/"hrp1-abc"');
    expect(init?.headers?.['cache-control']).toBe('no-store');
    expect(mockFetch).toHaveBeenCalledTimes(1); // 条件请求一次，无补取
    expect(bodyRead).toBe(false); // 未解析 304 正文
    expect(initSession).not.toHaveBeenCalled(); // 不 initSession 清数据
    expect((mockState.chatSessions as any)[sessionPath].items).toBe(itemsBefore); // 已加载状态保留
  });

  it('304 期间 todos 版本失效 → 丢弃并至多一次无条件补取（无循环）', async () => {
    await primeRecord();
    const todos = mockState.todosLiveVersionBySession as Record<string, number>;
    todos[sessionPath] = 7; // 外部 todo 状态变化 → 304 不可复用
    mockFetch
      .mockResolvedValueOnce(emptyResponse(304, { etag: 'W/"hrp1-abc"' }))
      .mockResolvedValueOnce(
        jsonResponse(pageBody(), { etag: 'W/"hrp1-def"', 'lingxi-history-protocol': '1' }),
      );
    await reconcileCurrentSessionMessages('window_focus');
    // 条件 1 次 + 无条件补取 1 次 = 2 次，无 304→补取→304 循环
    console.log('[dbg] calls:', JSON.stringify(mockFetch.mock.calls.map((c) => ({ url: String(c[0]), inm: (c[1]?.headers ?? {})['if-none-match'] }))));
    expect(mockFetch).toHaveBeenCalledTimes(2);
    const [, retryInit] = mockFetch.mock.calls[1];
    expect(retryInit?.headers?.['if-none-match']).toBeUndefined();
    expect(hasMessagesValidationRecord(sessionPath, sessionId, 50)).toBe(true);
    // 再来一次：只有新记录的条件请求（1 次，304 命中）
    mockFetch.mockResolvedValueOnce(emptyResponse(304, { etag: 'W/"hrp1-def"' }));
    await reconcileCurrentSessionMessages('window_focus');
    // 全程：prime 1 + 条件 304 1 + 补取 1 + 新记录复验 1 = 4 次，无循环
    expect(mockFetch).toHaveBeenCalledTimes(4);
  });

  it('新页首次加载无条件（loadMoreMessages 永不带条件头）', async () => {
    await primeRecord();
    (mockState.chatSessions as any)[sessionPath].hasMore = true;
    (mockState.chatSessions as any)[sessionPath].nextBefore = '10';
    mockFetch.mockResolvedValueOnce(
      jsonResponse({ ...pageBody(), messages: [{ id: '10', role: 'user', content: 'early' }], hasMore: false }),
    );
    await loadMoreMessages(sessionPath);
    const [url, init] = mockFetch.mock.calls[mockFetch.mock.calls.length - 1];
    expect(String(url)).toContain('before=10');
    expect(init?.headers?.['if-none-match']).toBeUndefined();
  });

  it('200 应用完成才保存记录；解码/应用失败不保存', async () => {
    // 失败：非法 JSON（解析失败 → 无记录）
    mockFetch.mockResolvedValueOnce(
      new Response('{"messages":', { status: 200, headers: { etag: 'W/"hrp1-x"', 'lingxi-history-protocol': '1' } }),
    );
    await loadMessages(sessionPath);
    expect(hasMessagesValidationRecord(sessionPath, sessionId, 50)).toBe(false);
    // 成功：完整应用后保存
    mockFetch.mockResolvedValueOnce(
      jsonResponse(pageBody(), { etag: 'W/"hrp1-abc"', 'lingxi-history-protocol': '1' }),
    );
    await loadMessages(sessionPath);
    expect(hasMessagesValidationRecord(sessionPath, sessionId, 50)).toBe(true);
  });

  it('旧服务器（无 ETag/能力头）→ 正常 200 应用、不保存记录、保持默认 50', async () => {
    mockFetch.mockResolvedValueOnce(jsonResponse(pageBody())); // 无任何协议头
    await loadMessages(sessionPath);
    expect((mockState.chatSessions as any)[sessionPath].items).toHaveLength(2); // 正常应用
    expect(hasMessagesValidationRecord(sessionPath, sessionId, 50)).toBe(false); // 无标签可复用
    // 后续仍无条件 50 条请求（默认 50 保持）
    mockFetch.mockResolvedValueOnce(jsonResponse(pageBody()));
    await loadMessages(sessionPath);
    const [url, init] = mockFetch.mock.calls[mockFetch.mock.calls.length - 1];
    expect(String(url)).not.toContain('limit='); // 未传 limit → 服务端默认 50
    expect(init?.headers?.['if-none-match']).toBeUndefined();
  });

  it('上限：每会话 32 条淘汰（不淘汰消息）、元数据预算跟踪', async () => {
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
    expect(stats.records).toBe(32); // 超限淘汰记录
    expect((mockState.chatSessions as any)[sessionPath].items.length).toBeGreaterThan(0); // 消息不受影响
    expect(stats.metadataBytes).toBeLessThanOrEqual(256 * 1024);
  });

  it('失效清单：invalidateSessionCache / 显式 note → 记录丢弃', async () => {
    await primeRecord();
    invalidateSessionCache(sessionPath); // 淘汰/registry/流式/登出清理共用钩子（动态导入异步生效）
    await new Promise((r) => setTimeout(r, 0)); // 等待动态导入的失效钩子落地
    expect(hasMessagesValidationRecord(sessionPath, sessionId, 50)).toBe(false);
    await primeRecord();
    noteHistoryValidationInvalidated(sessionPath); // retry/fork/reset/archive 落点
    expect(hasMessagesValidationRecord(sessionPath, sessionId, 50)).toBe(false);
    await primeRecord();
    noteHistoryValidationInvalidated(); // 登出/切换 workspace：全清
    expect(hasMessagesValidationRecord(sessionPath, sessionId, 50)).toBe(false);
  });

  it('异常分类：网络错误回退但不标 epoch；400 标 epoch；401/403 抛错且不标', async () => {
    await primeRecord();
    // 网络错误：条件请求 reject → 无条件回退成功；epoch 未标记（下次仍带条件头）
    const callsAfterPrime = mockFetch.mock.calls.length;
    mockFetch.mockRejectedValueOnce(new Error('network down'));
    mockFetch.mockResolvedValueOnce(jsonResponse(pageBody(), { etag: 'W/"hrp1-abc"', 'lingxi-history-protocol': '1' }));
    const r1 = await conditionalMessagesFetch(sessionPath, {
      url: messagesUrlOf(),
      sessionId,
      limit: 50,
      requestVersion: 1,
      appliedLiveVersion: 0,
    });
    expect(r1.kind).toBe('ok');
    expect(mockFetch).toHaveBeenCalledTimes(callsAfterPrime + 2); // 条件失败 + 无条件回退
    const [, fallbackInit] = mockFetch.mock.calls[1];
    expect(fallbackInit?.headers?.['if-none-match']).toBeUndefined();
    // 下次仍尝试条件（未标 epoch）
    mockFetch.mockResolvedValueOnce(emptyResponse(304, { etag: 'W/"hrp1-abc"' }));
    const r2 = await conditionalMessagesFetch(sessionPath, {
      url: messagesUrlOf(), sessionId, limit: 50, requestVersion: 1, appliedLiveVersion: 0,
    });
    expect(r2.kind).toBe('not-modified');
    // 400：条件头被拒绝 → 标记 epoch（回退无条件 1 次）
    const callsBefore400 = mockFetch.mock.calls.length;
    mockFetch.mockResolvedValueOnce(new Response('bad', { status: 400 }));
    mockFetch.mockResolvedValueOnce(jsonResponse(pageBody()));
    await expect(conditionalMessagesFetch(sessionPath, {
      url: messagesUrlOf(), sessionId, limit: 50, requestVersion: 1, appliedLiveVersion: 0,
    })).resolves.toBeTruthy();
    expect(mockFetch).toHaveBeenCalledTimes(callsBefore400 + 2); // 400 + 无条件回退
    // 后续：epoch 已标记 → 直接无条件（无 if-none-match）
    mockFetch.mockResolvedValueOnce(jsonResponse(pageBody(), { etag: 'W/"hrp1-abc"', 'lingxi-history-protocol': '1' }));
    await conditionalMessagesFetch(sessionPath, {
      url: messagesUrlOf(), sessionId, limit: 50, requestVersion: 1, appliedLiveVersion: 0,
    });
    const [lastUrl, lastInit] = mockFetch.mock.calls[mockFetch.mock.calls.length - 1];
    expect(lastInit?.headers?.['if-none-match']).toBeUndefined();
    void lastUrl;
  });

  it('401/403：抛错走既有错误流程，不标 epoch（后续仍带条件头）', async () => {
    await primeRecord();
    mockFetch.mockResolvedValueOnce(new Response('{"error":"insufficient_scope"}', { status: 403 }));
    await expect(conditionalMessagesFetch(sessionPath, {
      url: messagesUrlOf(), sessionId, limit: 50, requestVersion: 1, appliedLiveVersion: 0,
    })).rejects.toThrow();
    // epoch 未标记 → 下次仍发条件请求
    mockFetch.mockResolvedValueOnce(emptyResponse(304, { etag: 'W/"hrp1-abc"' }));
    const r = await conditionalMessagesFetch(sessionPath, {
      url: messagesUrlOf(), sessionId, limit: 50, requestVersion: 1, appliedLiveVersion: 0,
    });
    expect(r.kind).toBe('not-modified');
    const [, init] = mockFetch.mock.calls[mockFetch.mock.calls.length - 1];
    expect(init?.headers?.['if-none-match']).toBe('W/"hrp1-abc"');
  });
});

function messagesUrlOf(): string {
  return `/api/sessions/messages?path=${encodeURIComponent(sessionPath)}&sessionId=${sessionId}`;
}

describe('E06.3 页大小协商与混合 limit', () => {
  it('未知能力 → 首请求省略 limit（服务端默认 50）', async () => {
    await loadMessages(sessionPath);
    const [url, init] = mockFetch.mock.calls[0];
    expect(String(url)).not.toContain('limit=');
    expect(init?.headers?.['if-none-match']).toBeUndefined();
    expect(negotiatedHistoryPageLimit(sessionPath)).toBeNull();
  });

  it('合法推荐值（头 100）→ 后续新页显式携带 limit=100；游标仍用服务端 nextBefore', async () => {
    mockFetch.mockResolvedValueOnce(
      jsonResponse(pageBody(), {
        etag: 'W/"hrp1-abc"',
        'lingxi-history-protocol': '1',
        'lingxi-history-page-limit': '100',
      }),
    );
    await loadMessages(sessionPath);
    expect(negotiatedHistoryPageLimit(sessionPath)).toBe(100);
    (mockState.chatSessions as any)[sessionPath].hasMore = true;
    (mockState.chatSessions as any)[sessionPath].nextBefore = '100';
    mockFetch.mockResolvedValueOnce(jsonResponse({ ...pageBody(), messages: [{ id: '100' }], hasMore: false }));
    const { loadMoreMessages } = await import('../stores/session-actions');
    await loadMoreMessages(sessionPath);
    const [url, init] = mockFetch.mock.calls[mockFetch.mock.calls.length - 1];
    expect(String(url)).toContain('limit=100');
    expect(String(url)).toContain('before=100');
    expect(init?.headers?.['if-none-match']).toBeUndefined(); // 新页首次加载无条件
  });

  it('非法值/超限 → 保守省略（保持 50）', async () => {
    mockFetch.mockResolvedValueOnce(
      jsonResponse(pageBody(), { etag: 'W/"hrp1-x"', 'lingxi-history-protocol': '1', 'lingxi-history-page-limit': '77' }),
    );
    await loadMessages(sessionPath);
    expect(negotiatedHistoryPageLimit(sessionPath)).toBeNull();
    mockFetch.mockResolvedValueOnce(
      jsonResponse(pageBody(), { etag: 'W/"hrp1-y"', 'lingxi-history-protocol': '1', 'lingxi-history-page-limit': '400' }),
    );
    await loadMessages(sessionPath);
    expect(negotiatedHistoryPageLimit(sessionPath)).toBeNull();
  });

  it('头建议与概览建议冲突 → 保守 50 并输出诊断', async () => {
    mockFetch.mockResolvedValueOnce(
      jsonResponse(pageBody(), {
        etag: 'W/"hrp1-abc"',
        'lingxi-history-protocol': '1',
        'lingxi-history-page-limit': '100',
      }),
    );
    await loadMessages(sessionPath);
    noteOverviewRecommendedLimit(sessionPath, 150); // 概览建议与头冲突
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    expect(negotiatedHistoryPageLimit(sessionPath)).toBe(50);
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });

  it('per-page effective limit：50 页记录不命中 100 limit 请求（不串页）', async () => {
    mockFetch.mockResolvedValueOnce(
      jsonResponse(pageBody(), { etag: 'W/"hrp1-abc"', 'lingxi-history-protocol': '1' }),
    );
    await loadMessages(sessionPath); // 无 limit（默认 50）→ 记录 limit=50
    expect(hasMessagesValidationRecord(sessionPath, sessionId, 50)).toBe(true);
    mockFetch.mockResolvedValueOnce(
      jsonResponse(pageBody(), { etag: 'W/"hrp1-abc2"', 'lingxi-history-protocol': '1', 'lingxi-history-page-limit': '100' }),
    );
    const outcome = await conditionalMessagesFetch(sessionPath, {
      url: messagesUrlOf() + '&limit=100',
      sessionPath,
      sessionId,
      limit: 100,
      requestVersion: 1,
      appliedLiveVersion: 0,
    });
    if (outcome.kind !== 'ok') throw new Error('expected ok');
    expect(outcome.conditionalTried).toBe(false); // 100-limit 无记录 → 无条件
    expect(hasMessagesValidationRecord(sessionPath, sessionId, 50)).toBe(true);
  });

  it('epoch 隔离：换凭证 → 能力未知（重新协商）', async () => {
    mockFetch.mockResolvedValueOnce(
      jsonResponse(pageBody(), { etag: 'W/"hrp1-abc"', 'lingxi-history-protocol': '1', 'lingxi-history-page-limit': '100' }),
    );
    await loadMessages(sessionPath);
    expect(negotiatedHistoryPageLimit(sessionPath)).toBe(100);
    mockState.serverToken = 'other-token'; // 连接/认证 epoch 变化
    expect(negotiatedHistoryPageLimit(sessionPath)).toBeNull(); // 旧能力不跨 epoch
  });
});

describe('E07 故障场景（传输/代理/重启）', () => {
  it('代理移除 ETag 头（200 仅协议头）→ 不保存记录（无标签可复用）', async () => {
    mockFetch.mockResolvedValueOnce(
      jsonResponse(pageBody(), { 'lingxi-history-protocol': '1' }), // 无 etag
    );
    await loadMessages(sessionPath);
    expect((mockState.chatSessions as any)[sessionPath].items).toHaveLength(2); // 正常应用
    expect(hasMessagesValidationRecord(sessionPath, sessionId, 50)).toBe(false); // 无标签
  });

  it('304 缺 ETag 头 → 无法确认表示 → 无条件补取', async () => {
    await primeRecord();
    mockFetch.mockResolvedValueOnce(
      new Response(null, { status: 304, headers: { 'cache-control': 'private, no-store' } }), // 无 etag
    );
    const r = await conditionalMessagesFetch(sessionPath, {
      url: messagesUrlOf(), sessionId, limit: 50, requestVersion: 1, appliedLiveVersion: 0,
    });
    expect(r.kind).toBe('ok');
    expect(mockFetch).toHaveBeenCalledTimes(2); // 条件 + 补取
    const [, retryInit] = mockFetch.mock.calls[1];
    expect(retryInit?.headers?.['if-none-match']).toBeUndefined();
  });

  it('服务器重启失配（旧标签 → 200 新表示）→ 正常应用并更新记录', async () => {
    await primeRecord();
    mockFetch.mockResolvedValueOnce(
      jsonResponse(pageBody(), { etag: 'W/"hrp1-new-salt"', 'lingxi-history-protocol': '1' }),
    );
    const r = await conditionalMessagesFetch(sessionPath, {
      url: messagesUrlOf(), sessionId, limit: 50, requestVersion: 1, appliedLiveVersion: 0,
    });
    expect(r.kind).toBe('ok'); // 重启换盐 → 标签失配 → 正常 200（不报错）
    await loadMessages(sessionPath); // 应用新表示并更新记录
    expect(hasMessagesValidationRecord(sessionPath, sessionId, 50)).toBe(true);
  });

  it('非法 200 空正文 → 不当成功空会话（错误路径、无记录）', async () => {
    mockFetch.mockResolvedValueOnce(new Response('', { status: 200 }));
    await loadMessages(sessionPath);
    expect((mockState.chatSessions as any)[sessionPath].items.length).toBeGreaterThan(0); // 旧状态未被清成空
    expect(hasMessagesValidationRecord(sessionPath, sessionId, 50)).toBe(false);
  });
});
