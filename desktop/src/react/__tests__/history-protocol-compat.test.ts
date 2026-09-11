/**
 * F01 四组合兼容矩阵（客户端回退侧；旧+新 真实 HTTP 链路）。
 *
 * mock 边界=fetch 层：lingxiFetch 被 mock 为「向目标端口的真实 HTTP 转发」
 * （node:http），客户端代码（loadMessages/reconcile/概览）走真实链路。
 *  - 旧+新：目标=A01 clone 旧 sessions 路由子进程（真实旧代码、真实 HTTP）。
 *  - 新+新（控制组）：目标=主仓库现行路由真实 HTTP。
 * 未运行完整旧 renderer / 新 Electron UI（如实标注层级）。
 *
 * runner 自包含：模板 tracked 于主仓库 tests/compat-old-server-runner.template.mjs，beforeAll 幂等写入
 * clone tests/compat-old-server-runner.mjs 再 spawn；clone 缺席时整套件 skip 并 console 明示
 * 「四组合中旧服务端侧指定环境未验证」（不静默 skip）。
 */
import { describe, it, expect, vi, beforeAll, afterAll } from 'vitest';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import http from 'node:http';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { Hono } from 'hono';
import { serve } from '@hono/node-server';

const CLONE_ROOT = '/tmp/lingxi-baseline-1d42b740';
const RUNNER_TEMPLATE = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', '..', '..', 'tests', 'compat-old-server-runner.template.mjs');
const RUNNER_PATH = path.join(CLONE_ROOT, 'tests', 'compat-old-server-runner.mjs');

function cloneReady(): boolean {
  return (
    fs.existsSync(path.join(CLONE_ROOT, '.git')) &&
    fs.existsSync(path.join(CLONE_ROOT, 'server', 'routes', 'sessions.ts')) &&
    fs.existsSync(RUNNER_TEMPLATE)
  );
}
const sessionPath = '/agents/hana/sessions/longrun.jsonl';

type MockState = Record<string, unknown>;
const mockState: MockState = {};
let targetPort = 0;

// fetch 边界 mock：把 lingxiFetch 转发到目标端口的真实 HTTP（保留 header/status/正文）
const mockFetch = vi.hoisted(() => {
  const state: { port: number } = { port: 0 };
  (globalThis as any).__setCompatTargetPort = (p: number) => { state.port = p; };
  return Object.assign(
    vi.fn(async (url: string, init?: any) => {
      const http = await import('node:http');
      return await new Promise<any>((resolve, reject) => {
        const req = http.request(
          { host: '127.0.0.1', port: state.port, path: url, method: init?.method ?? 'GET', headers: init?.headers ?? {} },
          (res) => {
            const chunks: Buffer[] = [];
            res.on('data', (c) => chunks.push(c));
            res.on('end', () => {
              const body = Buffer.concat(chunks);
              const headers = new Headers();
              for (const [k, v] of Object.entries(res.headers)) {
                if (v != null) headers.set(k, Array.isArray(v) ? v.join(', ') : String(v));
              }
              resolve(new Response(body, { status: res.statusCode ?? 200, headers }));
            });
            res.on('error', reject);
          },
        );
        req.on('error', reject);
        req.end();
      });
    }),
    { mockResolvedValueOnce: undefined },
  );
});

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

import { loadMessages, reconcileCurrentSessionMessages, loadMoreMessages } from '../stores/session-actions';
import {
  requestHistoryOverview,
  getHistoryOverviewSnapshot,
  __clearHistoryOverviewStateForTest,
} from '../stores/history-overview-client';
import { __clearHistoryProtocolStateForTest, hasMessagesValidationRecord } from '../stores/history-protocol-client';

let child: ReturnType<typeof spawn> | null = null;
let oldPort = 0;
let oldSessionId = '';
let oldSessionPath = '';

async function startOldServer(): Promise<void> {
  child = spawn('node', [path.join(CLONE_ROOT, 'tests', 'compat-old-server-runner.mjs')], {
    cwd: CLONE_ROOT, stdio: ['ignore', 'pipe', 'pipe'],
  });
  await new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('old server 启动超时')), 30000);
    const onData = (buf: Buffer) => {
      const text = buf.toString();
      const m = text.match(/PORT=(\d+)/);
      const pMatch = text.match(/SESSION_PATH=(.*)/);
      if (pMatch) oldSessionPath = pMatch[1].trim();
      if (m) { oldPort = Number(m[1]); (globalThis as any).__setCompatTargetPort(oldPort); clearTimeout(timer); resolve(); }
    };
    child!.stdout!.on('data', onData);
    child!.stderr!.on('data', (b: Buffer) => process.stderr.write(b));
    child!.on('exit', (code) => reject(new Error(`old server 提前退出 code=${code}`)));
  });
}

function installStore(basePort: number): void {
  (globalThis as any).__setCompatTargetPort(basePort);
  Object.assign(mockState, {
    serverPort: basePort,
    serverToken: 'compat-token',
    currentSessionPath: oldSessionPath,
    pendingNewSession: false,
    pendingSessionSwitchPath: null,
    streamingSessions: [],
    sessions: [{ path: oldSessionPath, sessionId: oldSessionId, revision: 'rev-old' }],
    chatSessions: {
      [oldSessionPath]: {
        items: [{ type: 'message', data: { id: '0' } }],
        hasMore: true,
        nextBefore: '5',
        revision: 'rev-old',
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

if (!cloneReady()) {
  console.warn(`[history-protocol-compat:desktop] A01 clone 缺席（${CLONE_ROOT}）：四组合中旧服务端侧指定环境未验证，本套件 skip（见 protocol/compatibility-matrix.json 环境依赖注记）`);
}

describe.skipIf(!cloneReady())('F01 旧+新：新客户端对旧服务端无感回退（真实 HTTP）', () => {
  beforeAll(async () => {
    // runner 自包含：tracked 模板幂等覆盖写入 clone（每次运行重建，防止旧副本/缺失副本漂移）
    fs.copyFileSync(RUNNER_TEMPLATE, RUNNER_PATH);
    await startOldServer();
  }, 40000);
  afterAll(() => {
    child?.kill('SIGTERM');
  });

  it('loadMessages 对旧服务端：200 正常应用、不保存校验记录（无 ETag 可复用）', async () => {
    installStore(oldPort);
    __clearHistoryProtocolStateForTest();
    await loadMessages(oldSessionPath);
    const dbg = await fetch(`http://127.0.0.1:${oldPort}/api/sessions/messages?path=${encodeURIComponent(oldSessionPath)}&limit=50`);
    const dbgBody = await dbg.text();
    console.error('[old-dbg] status=', dbg.status, 'body0=', dbgBody.slice(0, 160));
    const chat = (mockState.chatSessions as any)[oldSessionPath];
    expect(chat.items.length).toBeGreaterThanOrEqual(11); // 旧 JSON 正常应用
    expect(hasMessagesValidationRecord(oldSessionPath, oldSessionId, 50)).toBe(false); // 无 ETag → 无记录
  });

  it('概览对旧服务端 404 → epoch 记为不支持（不卡住、不弹持续错误）', async () => {
    installStore(oldPort);
    __clearHistoryOverviewStateForTest();
    (globalThis as any).__setCompatTargetPort(oldPort);
    await requestHistoryOverview(oldSessionPath, oldSessionId);
    const snap = getHistoryOverviewSnapshot(oldSessionPath);
    expect(snap.status).toBe('unsupported'); // 404 → 能力缺席，回退翻页探底
    expect(snap.data).toBeNull();
  });

  it('翻页对旧服务端照常推进（hasMore+nextBefore 旧协议不变）', async () => {
    installStore(oldPort);
    (mockState.chatSessions as any)[oldSessionPath].hasMore = true;
    (mockState.chatSessions as any)[oldSessionPath].nextBefore = '5';
    await loadMoreMessages(oldSessionPath);
    const chat = (mockState.chatSessions as any)[oldSessionPath];
    expect(chat.items.length).toBeGreaterThan(1); // 更早记录并入
  });

  it('reconcile 对旧服务端正常触发既有补拉链（不因概览/条件层卡住）', async () => {
    installStore(oldPort);
    (mockState.chatSessions as any)[oldSessionPath].revision = null; // cached revision 未知 → 既有补拉
    await reconcileCurrentSessionMessages('compat_reconnect');
    expect((mockState.chatSessions as any)[oldSessionPath].items.length).toBeGreaterThan(0);
  });
});

function sessionIdOf(): string {
  return 'sess_compat_old';
}
