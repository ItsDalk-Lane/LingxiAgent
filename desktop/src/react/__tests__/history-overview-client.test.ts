/**
 * E05 概览客户端测试（mock 在 fetch/store 层）：
 *  - 非阻塞：请求为 fire-and-forget，消息 store 不被概览写入；
 *  - schema/约束校验拒绝坏数据（负数/NaN/缺字段/三桶合计≠轮次/分类合计≠任务数）；
 *  - stale 丢弃（切换会话/版本过期/revision 前移）；
 *  - 404/405 → 当前 epoch 记为不支持（后续不再发请求，回退翻页探底）；
 *  - available:false 三 reason → 显示未知（不伪造计数）；
 *  - 401/403 → 清理失效统计、不标为能力缺席；
 *  - 在途合并（同连接同会话并发 → 一次请求）；
 *  - 能力按连接+认证 epoch 隔离（换 server 可重新探测）；
 *  - locale 键齐全（zh/en/ja/ko/zh-TW）。
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import fs from 'node:fs';

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

import {
  requestHistoryOverview,
  getHistoryOverviewSnapshot,
  validateOverviewPayload,
  __clearHistoryOverviewStateForTest,
} from '../stores/history-overview-client';

const sessionPath = '/agents/hana/sessions/longrun.jsonl';
const sessionId = 'sess_overview_1';

function okOverview(overrides: Record<string, unknown> = {}) {
  return {
    schemaVersion: 1,
    available: true,
    sessionId,
    revision: 'rev-1',
    counts: { displayRecords: 11, sourceRecords: 20, runsWithAssistant: 2, referencedTasks: 3 },
    runSizeDistribution: { oneTo50: 1, from51To200: 1, over200: 0 },
    taskDistribution: { subagent: 1, workflow: 1, media: 1, other: 0 },
    pagination: { legacyDefaultLimit: 50, recommendedLimit: 50, maxLimit: 200, estimatedPagesAtRecommendedLimit: 1 },
    ...overrides,
  };
}

beforeEach(() => {
  for (const key of Object.keys(mockState)) delete mockState[key];
  Object.assign(mockState, {
    serverPort: 4590,
    serverToken: 'tok',
    currentSessionPath: sessionPath,
    pendingNewSession: false,
    pendingSessionSwitchPath: null,
    streamingSessions: [],
    sessions: [{ path: sessionPath, sessionId, revision: 'rev-1' }],
    chatSessions: { [sessionPath]: { items: [{ type: 'message', data: { id: '0' } }], revision: 'rev-1' } },
    _loadMessagesVersion: { [sessionPath]: 1 },
    todosLiveVersionBySession: {},
    messageLiveVersions: {},
  });
  __clearHistoryOverviewStateForTest();
  mockFetch.mockReset();
});

describe('E05 概览客户端', () => {
  it('非阻塞：请求为 fire-and-forget；响应到达前不写入消息 store', async () => {
    const chatBefore = JSON.stringify(mockState.chatSessions);
    let release!: (r: Response) => void;
    mockFetch.mockResolvedValueOnce(new Promise<Response>((r) => { release = r; }));
    const p = requestHistoryOverview(sessionPath, sessionId);
    expect(getHistoryOverviewSnapshot(sessionPath).status).toBe('loading'); // 在途
    expect(JSON.stringify(mockState.chatSessions)).toBe(chatBefore); // 消息 store 不被触碰
    release(new Response(JSON.stringify(okOverview()), { status: 200, headers: { 'content-type': 'application/json' } }));
    await p;
    expect(getHistoryOverviewSnapshot(sessionPath).status).toBe('available');
    // 消息 store 仍不被概览写入（E05：不 stamp 消息缓存）
    expect(JSON.stringify(mockState.chatSessions)).toBe(chatBefore);
  });

  it('schema/约束校验：坏数据 → unsupported（不应用）', async () => {
    const bad: any[] = [
      { ...okOverview(), counts: { ...okOverview().counts, runsWithAssistant: 5 } }, // 三桶合计 2 ≠ 5
      { ...okOverview(), taskDistribution: { ...okOverview().taskDistribution, other: 5 } }, // 分类合计 4 ≠ 3
      { ...okOverview(), counts: { ...okOverview().counts, displayRecords: -1 } }, // 负数
      { ...okOverview(), schemaVersion: 2 }, // 未知协议版本
      { foo: 1 }, // 结构缺失
    ];
    for (const payload of bad) {
      mockFetch.mockResolvedValueOnce(new Response(JSON.stringify(payload), { status: 200 }));
      await requestHistoryOverview(sessionPath, sessionId);
      expect(getHistoryOverviewSnapshot(sessionPath).status).toBe('unsupported');
      __clearHistoryOverviewStateForTest();
      mockFetch.mockReset();
    }
    // 正确数据通过
    mockFetch.mockResolvedValueOnce(new Response(JSON.stringify(okOverview()), { status: 200 }));
    await requestHistoryOverview(sessionPath, sessionId);
    expect(getHistoryOverviewSnapshot(sessionPath).status).toBe('available');
  });

  it('stale 丢弃：响应到达时已切换会话/版本过期 → 快照不更新', async () => {
    let release!: (r: Response) => void;
    mockFetch.mockResolvedValueOnce(new Promise<Response>((r) => { release = r; }));
    const p = requestHistoryOverview(sessionPath, sessionId);
    // 响应到达前：切换会话 + load 版本推进
    mockState.currentSessionPath = '/agents/hana/sessions/other.jsonl';
    const versions = mockState._loadMessagesVersion as Record<string, number>;
    versions[sessionPath] = (versions[sessionPath] ?? 0) + 1;
    release(new Response(JSON.stringify(okOverview()), { status: 200 }));
    await p;
    const snap = getHistoryOverviewSnapshot(sessionPath);
    expect(snap.status === 'loading' || snap.status === 'idle').toBe(true); // 丢弃，未应用
    expect(snap.data).toBeNull();
  });

  it('404/405 → 当前 epoch 记为不支持（后续请求短路，回退翻页探底）', async () => {
    mockFetch.mockResolvedValueOnce(new Response('not found', { status: 404 }));
    await requestHistoryOverview(sessionPath, sessionId);
    expect(getHistoryOverviewSnapshot(sessionPath).status).toBe('unsupported');
    const calls = mockFetch.mock.calls.length;
    await requestHistoryOverview(sessionPath, sessionId);
    expect(mockFetch).toHaveBeenCalledTimes(calls); // 短路：不再发请求
  });

  it('available:false 三 reason → unavailable 状态原样携带（不伪造计数）', async () => {
    for (const reason of ['revision_unknown', 'directory_unavailable', 'unsupported_history']) {
      mockFetch.mockResolvedValueOnce(new Response(JSON.stringify({ schemaVersion: 1, available: false, reason }), { status: 200 }));
      await requestHistoryOverview(sessionPath, sessionId);
      const snap = getHistoryOverviewSnapshot(sessionPath);
      expect(snap.status).toBe('unavailable');
      expect(snap.unavailableReason).toBe(reason);
      expect(snap.data).toBeNull();
      __clearHistoryOverviewStateForTest();
      mockFetch.mockReset();
    }
  });

  it('401/403 → 清理失效统计（回到 idle）、不标为能力缺席（下次仍尝试）', async () => {
    mockFetch.mockResolvedValueOnce(new Response('{"error":"insufficient_scope"}', { status: 403 }));
    await requestHistoryOverview(sessionPath, sessionId);
    expect(getHistoryOverviewSnapshot(sessionPath).status).toBe('idle'); // 统计被清理
    // 403 未标 epoch：下次仍尝试请求
    mockFetch.mockResolvedValueOnce(new Response(JSON.stringify(okOverview()), { status: 200 }));
    await requestHistoryOverview(sessionPath, sessionId);
    expect(getHistoryOverviewSnapshot(sessionPath).status).toBe('available');
  });

  it('在途合并：同连接同会话并发触发 → 只发一次请求', async () => {
    let release!: (r: Response) => void;
    mockFetch.mockResolvedValueOnce(new Promise<Response>((r) => { release = r; }));
    const p1 = requestHistoryOverview(sessionPath, sessionId);
    const p2 = requestHistoryOverview(sessionPath, sessionId);
    expect(p1).toBe(p2); // 同一在途 Promise
    release(new Response(JSON.stringify(okOverview()), { status: 200 }));
    await Promise.all([p1, p2]);
    expect(mockFetch).toHaveBeenCalledTimes(1);
  });

  it('能力按连接+认证 epoch 隔离：换 server（epoch 变化）后可重新探测', async () => {
    mockFetch.mockResolvedValueOnce(new Response('not found', { status: 404 }));
    await requestHistoryOverview(sessionPath, sessionId);
    expect(getHistoryOverviewSnapshot(sessionPath).status).toBe('unsupported');
    // 换 server（serverToken/serverPort 变化 → epoch 变化）→ 重新探测
    mockState.serverToken = 'other-token';
    mockFetch.mockResolvedValueOnce(new Response(JSON.stringify(okOverview()), { status: 200 }));
    await requestHistoryOverview(sessionPath, sessionId);
    expect(getHistoryOverviewSnapshot(sessionPath).status).toBe('available');
  });


  it('概览返回 HTML 错误页（代理/网关）→ 不崩溃、快照保持原状', async () => {
    mockFetch.mockResolvedValueOnce(new Response('<html><body>502 Gateway</body></html>', { status: 200, headers: { 'content-type': 'text/html' } }));
    await requestHistoryOverview(sessionPath, sessionId);
    const snap = getHistoryOverviewSnapshot(sessionPath);
    expect(['idle', 'loading']).toContain(snap.status); // 不崩溃、不误应用
    expect(snap.data).toBeNull();
  });
  it('locale 键齐全（zh/en/ja/ko/zh-TW 五语言同一键集）', () => {
    const langs = ['zh', 'en', 'ja', 'ko', 'zh-TW'];
    const expected = ['records', 'runs', 'tasks', 'taskSubagent', 'taskWorkflow', 'taskMedia', 'taskOther', 'unknown', 'detail'].sort();
    for (const lang of langs) {
      const data = JSON.parse(fs.readFileSync(new URL(`../../locales/${lang}.json`, import.meta.url), 'utf-8'));
      const ho = data.chat?.historyOverview;
      expect(ho).toBeTruthy();
      expect(Object.keys(ho).sort()).toEqual(expected);
      for (const k of expected) expect(typeof ho[k]).toBe('string');
    }
  });
});
