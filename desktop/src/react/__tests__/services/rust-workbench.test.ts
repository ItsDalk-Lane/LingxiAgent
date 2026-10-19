import { afterEach, describe, expect, it, vi } from 'vitest';
import { rustWorkbenchRoute, startRustResourceEvents } from '../../services/rust-workbench';
import type { ServerConnection } from '../../services/server-connection';
import { useStore } from '../../stores';
import { pauseRustResourceWatches, resubscribeActiveWatches, retainResourceWatch } from '../../services/resource-events';

const source: ServerConnection = {
  connectionId: 'r07-workbench', kind: 'custom_remote', serverId: 'server-a', serverNodeKind: 'lingxi-service',
  studioId: 'studio-a', label: '工作台服务', baseUrl: 'http://127.0.0.1:3210', wsUrl: 'ws://127.0.0.1:3210',
  token: 'workbench-test-token', authState: 'paired', trustState: 'tunnel', credentialKind: 'device_credential', capabilities: [],
};
afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

describe('Rust 工作台正式入口', () => {
  it.each([
    '/api/git/status?dir=%2Ftmp%2Frepo', '/api/git/log-stats', '/api/git/stashes?dir=x', '/api/git/ai-commit-message',
    '/api/workbench/content?mountId=mount-a&name=a.md', '/api/workbench/actions', '/api/workbench/upload',
    '/api/studio/workspaces', '/api/studio/workspaces/mount-a', '/api/studio/workspaces/mount-a/files',
    '/api/desk/files?dir=x', '/api/desk/search-files?q=note', '/api/desk/jian', '/api/desk/path', '/api/desk/logs',
    '/api/diary/write', '/api/diary/list', '/api/resource-io/subscribe', '/api/resource-io/subscriptions/sub-a',
    '/api/preferences/goal',
    '/api/resource-io/watch/watch-a', '/api/resource-io/events?since=24',
  ])('将 %s 交给当前 Rust 服务，旧服务保持旧地址', path => {
    expect(rustWorkbenchRoute(source, path)).toBe(`/lingxi/v1${path.slice(4)}`);
    expect(rustWorkbenchRoute({}, path)).toBe(path);
  });

  it('手机别名使用同一工作台处理器，未知路径不能误映射', () => {
    expect(rustWorkbenchRoute(source, '/api/mobile/workbench/files?mountId=default')).toBe('/lingxi/v1/workbench/files?mountId=default');
    for (const path of ['/api/git/status-extra', '/api/desk/unknown', '/api/resource-io/unimplemented', '/api/workbench/content/extra']) {
      expect(rustWorkbenchRoute(source, path)).toBe(path);
    }
  });
});

describe('Rust 资源监听补拉与回收', () => {
  it('断连暂停与重连申请不会遗留在途订阅，也不会让旧收据覆盖新收据', async () => {
    useStore.setState({ serverConnections: { [source.connectionId]: source }, activeServerConnectionId: source.connectionId, activeServerConnection: source });
    let deliverOld!: (response: Response) => void; let subscribeCount = 0;
    const fetchImpl = vi.fn(async (input: RequestInfo | URL, _init?: RequestInit) => {
      if (String(input).endsWith('/subscribe')) {
        if (++subscribeCount === 1) return new Promise<Response>(resolve => { deliverOld = resolve; });
        return { ok: true, json: async () => ({ subscriptionId: 'resumed-sub' }) } as Response;
      }
      return { ok: true, json: async () => ({ ok: true }) } as Response;
    });
    vi.stubGlobal('fetch', fetchImpl);
    const release = retainResourceWatch({ kind: 'local-file', path: '/tmp/r07-reconnect.md' });
    pauseRustResourceWatches(source);
    await resubscribeActiveWatches();
    expect(subscribeCount).toBe(2);
    deliverOld({ ok: true, json: async () => ({ subscriptionId: 'late-old-sub' }) } as Response);
    await vi.waitFor(() => expect(fetchImpl.mock.calls.some(([url]) => String(url).endsWith('/subscriptions/late-old-sub'))).toBe(true));
    expect(fetchImpl.mock.calls.some(([url]) => String(url).endsWith('/subscriptions/resumed-sub'))).toBe(false);
    release();
    await vi.waitFor(() => expect(fetchImpl.mock.calls.some(([url]) => String(url).endsWith('/subscriptions/resumed-sub'))).toBe(true));
  });
  it('旧连接迟到的订阅收据仍交给旧服务回收，旧释放函数不能删除新连接订阅', async () => {
    const install = (connection: ServerConnection) => useStore.setState({
      serverConnections: { [connection.connectionId]: connection }, activeServerConnectionId: connection.connectionId,
      activeServerConnection: connection,
    });
    let finishOld!: (response: Response) => void;
    const fetchImpl = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      if (url.endsWith('/subscribe') && url.includes(':3210/')) return new Promise<Response>(resolve => { finishOld = resolve; });
      return { ok: true, json: async () => url.endsWith('/subscribe') ? { subscriptionId: 'new-sub' } : { ok: true } } as Response;
    });
    vi.stubGlobal('fetch', fetchImpl);
    install(source);
    const releaseOld = retainResourceWatch({ kind: 'local-file', path: '/tmp/r07-owned-note.md' });
    const replacement = { ...source, baseUrl: 'http://127.0.0.1:3211', token: 'new-token' };
    install(replacement);
    const releaseNew = retainResourceWatch({ kind: 'local-file', path: '/tmp/r07-owned-note.md' });
    finishOld({ ok: true, json: async () => ({ subscriptionId: 'old-sub' }) } as Response);
    await vi.waitFor(() => expect(fetchImpl.mock.calls.some(([url]) => String(url).includes('/subscriptions/old-sub'))).toBe(true));
    const oldDelete = fetchImpl.mock.calls.find(([url]) => String(url).includes('/subscriptions/old-sub'))!;
    expect(String(oldDelete[0])).toContain(':3210/');
    expect(oldDelete[1]?.headers).toMatchObject({ Authorization: 'Bearer workbench-test-token' });
    releaseOld();
    expect(fetchImpl.mock.calls.some(([url]) => String(url).includes('/subscriptions/new-sub'))).toBe(false);
    releaseNew();
    await vi.waitFor(() => expect(fetchImpl.mock.calls.some(([url]) => String(url).includes('/subscriptions/new-sub'))).toBe(true));
    const newDelete = fetchImpl.mock.calls.find(([url]) => String(url).includes('/subscriptions/new-sub'))!;
    expect(String(newDelete[0])).toContain(':3211/');
    expect(newDelete[1]?.headers).toMatchObject({ Authorization: 'Bearer new-token' });
  });
  it('按真实游标顺序拉取，切换连接立即停止且不应用迟到事件', async () => {
    vi.useFakeTimers();
    let current: ServerConnection | null = source;
    let listener: (() => void) | undefined;
    let finish!: (response: Response) => void;
    const apply = vi.fn(); const unsubscribe = vi.fn();
    const fetchImpl = vi.fn<(...args: Parameters<typeof fetch>) => ReturnType<typeof fetch>>()
      .mockResolvedValueOnce({ ok: true, json: async () => ({ stale: false, latestSequence: 3, events: [{ type: 'resource.changed', sequence: 3, resourceKey: 'local_fs:/tmp/note.md' }] }) } as Response)
      .mockImplementationOnce(() => new Promise<Response>(resolve => { finish = resolve; }));
    const stop = startRustResourceEvents(source, {
      currentConnection: () => current, subscribeConnection: callback => { listener = callback; return unsubscribe; },
      fetch: fetchImpl, applyEvent: apply, reportError: vi.fn(),
    });
    await vi.advanceTimersByTimeAsync(0);
    expect(apply).toHaveBeenCalledOnce();
    expect(fetchImpl.mock.calls[0][0]).toBe('http://127.0.0.1:3210/lingxi/v1/resource-io/events?since=0');
    expect(fetchImpl.mock.calls[0][1]!.headers).toMatchObject({ Authorization: 'Bearer workbench-test-token' });
    await vi.advanceTimersByTimeAsync(1_000);
    expect(fetchImpl.mock.calls[1][0]).toContain('since=3');
    const pendingSignal = fetchImpl.mock.calls[1][1]!.signal!;
    current = { ...source, token: 'replacement-token' }; listener?.();
    expect(pendingSignal.aborted).toBe(true);
    finish({ ok: true, json: async () => ({ stale: false, latestSequence: 4, events: [{ type: 'resource.changed', sequence: 4 }] }) } as Response);
    await vi.advanceTimersByTimeAsync(5_000);
    expect(apply).toHaveBeenCalledOnce(); expect(fetchImpl).toHaveBeenCalledTimes(2);
    expect(unsubscribe).toHaveBeenCalled(); stop();
  });

  it('退出停止在途请求和后续定时器', async () => {
    vi.useFakeTimers();
    const fetchImpl = vi.fn((..._args: Parameters<typeof fetch>) => new Promise<Response>(() => {}));
    const stop = startRustResourceEvents(source, { currentConnection: () => source, subscribeConnection: () => () => {}, fetch: fetchImpl, applyEvent: vi.fn() });
    expect(fetchImpl).toHaveBeenCalledOnce();
    stop();
    expect(fetchImpl.mock.calls[0][1]!.signal!.aborted).toBe(true);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(fetchImpl).toHaveBeenCalledOnce();
  });
});
