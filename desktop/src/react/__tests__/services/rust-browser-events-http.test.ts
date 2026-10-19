/** @vitest-environment jsdom */
import { createServer } from 'node:http';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';
import { browserStateForPath, setBrowserStateForPath, setBrowserCardCollapsed, type BrowserSessionState } from '../../stores/browser-slice';
import { handleAppEvent } from '../../services/app-event-actions';
import { startRustAutomationNotifications } from '../../services/rust-automation';
import type { ServerConnection } from '../../services/server-connection';
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(done => { resolve = done; }); return { promise, resolve }; }
afterEach(() => vi.useRealTimers());

describe('Rust browser events: HTTP cursor → shared production handler → actual store', () => {
  it('binds browser updates to their service, preserves manual collapse, and clears actual stopped state', async () => {
    const ready = deferred<void>();
    const paths: string[] = [];
    let phase = 0;
    const thumbnail = 'ZmFrZS1qcGVn';
    const event = (seq: number, type: string, payload: Record<string, unknown>) => ({ seq, type, payload, source: 'server', atUnixMs: 1_800_000_000_000 + seq });
    const batches = [
      { stale: false, latestSequence: 0, events: [] },
      { stale: false, latestSequence: 2, events: [
        event(1, 'browser_status', { sessionPath: 'native:s', executionNode: 'other-service', running: true, url: 'https://wrong.example' }),
        event(2, 'browser_status', { sessionPath: 'native:s', executionNode: 'server-a', running: true, url: 'https://right.example', thumbnail, thumbnailCapturedAt: 1_800_000_000_000, thumbnailUrl: 'https://right.example' }),
      ] },
      { stale: false, latestSequence: 4, events: [
        event(3, 'browser_update', { sessionPath: 'native:s', executionNode: 'server-a', running: true, url: 'https://right.example/next' }),
        event(4, 'browser_status', { sessionPath: 'native:s', executionNode: 'server-a', running: false, url: null }),
      ] },
    ];
    const server = createServer((request, response) => {
      paths.push(request.url || ''); response.setHeader('Content-Type', 'application/json');
      if (request.headers.authorization !== 'Bearer event-token') { response.statusCode = 401; response.end(JSON.stringify({ error: 'authorization required' })); return; }
      const path = new URL(request.url!, 'http://127.0.0.1').pathname;
      if (path === '/lingxi/v1/app-events') response.end(JSON.stringify(batches[phase]));
      else if (path === '/lingxi/v1/browser/workspaces') response.end(JSON.stringify({ workspaces: [] }));
      else if (path === '/lingxi/v1/desk/activities') response.end(JSON.stringify({ activities: [] }));
      else { response.statusCode = 404; response.end(JSON.stringify({ error: 'unexpected endpoint' })); }
    });
    await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
    const address = server.address(); if (!address || typeof address === 'string') throw new Error('HTTP fixture unavailable');
    const source: ServerConnection = { connectionId: 'browser-http', kind: 'custom_remote', serverId: 'server-a', serverNodeKind: 'lingxi-service', studioId: 'studio', label: '服务A', baseUrl: `http://127.0.0.1:${address.port}`, wsUrl: `ws://127.0.0.1:${address.port}`, token: 'event-token', authState: 'paired', trustState: 'tunnel', credentialKind: 'device_credential', capabilities: [] };
    useStore.setState({ serverConnections: { [source.connectionId]: source }, activeServerConnectionId: source.connectionId, activeServerConnection: source, currentSessionPath: 'native:s', currentSessionId: 's', sessions: [], sessionLocatorsById: {}, browserBySession: {} } as never);
    const seen: BrowserSessionState[] = []; const rawSizes: number[] = []; const errors: string[] = [];
    vi.useFakeTimers();
    const stop = startRustAutomationNotifications(source, { replaceActivities: () => ready.resolve(), reportError: error => errors.push(error), deliverEvent: async (type, payload, origin, isCurrent) => {
      await handleAppEvent(type, payload, { source: origin, isCurrent });
      seen.push(structuredClone(browserStateForPath(useStore.getState(), 'native:s')));
      rawSizes.push(Object.keys(useStore.getState().browserBySession).length);
    } });
    try {
      await ready.promise; phase = 1;
      await vi.advanceTimersByTimeAsync(5_000);
      await vi.waitFor(() => expect(seen).toHaveLength(2));
      expect(seen[0].running).toBe(false); expect(rawSizes[0]).toBe(0);
      expect(seen[1]).toMatchObject({ executionNode: 'server-a', running: true, thumbnail, thumbnailFresh: true, url: 'https://right.example', collapsed: false });
      setBrowserCardCollapsed('native:s', true);
      phase = 2;
      await vi.advanceTimersByTimeAsync(0); await vi.advanceTimersByTimeAsync(5_000);
      await vi.waitFor(() => expect(seen).toHaveLength(4));
      expect(seen[2]).toMatchObject({ running: true, collapsed: true, thumbnail, thumbnailFresh: false, url: 'https://right.example/next' });
      expect(seen[3]).toMatchObject({ running: false, collapsed: true, thumbnail: null, thumbnailCapturedAt: null, thumbnailUrl: null, thumbnailFresh: false });
      expect(paths).toContain('/lingxi/v1/app-events?since=2');
      expect(paths.some(path => path.includes('/notifications'))).toBe(false);
      expect(errors).toEqual([]);
    } finally { stop(); vi.useRealTimers(); server.closeAllConnections(); await new Promise<void>(resolve => server.close(() => resolve())); }
  }, 10_000);
});


describe('Rust browser initial/recovery snapshots use the same real HTTP scope', () => {
  it('loads an already-running workspace, preserves cached presentation, rejects bad snapshots, and clears only disappeared workspaces in this connection', async () => {
    const ready = deferred<void>(); const paths: string[] = []; const errors: string[] = [];
    let phase = 0;
    const workspace = { sessionId: 's', sessionPath: 'native:s', executionNode: 'server-a', running: true, resumable: true, url: 'https://page.example/first', title: '已在运行', activeTabId: null, tabs: [], suspended: false, userTakeover: false, revoked: false, visibleCapable: true, unavailableReason: null };
    const snapshots = [[workspace], [{ ...workspace, url: 'https://page.example/second' }], [{ ...workspace, executionNode: 'wrong-service' }], []];
    const events = [
      { stale: false, latestSequence: 9, events: [] },
      { stale: true, latestSequence: 99, events: [] },
      { stale: true, latestSequence: 100, events: [] },
      { stale: false, latestSequence: 0, events: [] },
    ];
    const server = createServer((request, response) => {
      paths.push(request.url || ''); response.setHeader('Content-Type', 'application/json');
      if (request.headers.authorization !== 'Bearer snapshot-token') { response.statusCode = 401; response.end(JSON.stringify({ error: 'authorization required' })); return; }
      const path = new URL(request.url!, 'http://127.0.0.1').pathname;
      if (path === '/lingxi/v1/app-events') response.end(JSON.stringify(events[phase]));
      else if (path === '/lingxi/v1/desk/activities') response.end(JSON.stringify({ activities: [] }));
      else if (path === '/lingxi/v1/browser/workspaces') response.end(JSON.stringify({ workspaces: snapshots[phase] }));
      else { response.statusCode = 404; response.end(JSON.stringify({ error: 'unexpected endpoint' })); }
    });
    await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
    const address = server.address(); if (!address || typeof address === 'string') throw new Error('HTTP fixture unavailable');
    const source: ServerConnection = { connectionId: 'snapshot-http', kind: 'custom_remote', serverId: 'server-a', serverNodeKind: 'lingxi-service', studioId: 'studio', label: '服务A', baseUrl: `http://127.0.0.1:${address.port}`, wsUrl: `ws://127.0.0.1:${address.port}`, token: 'snapshot-token', authState: 'paired', trustState: 'tunnel', credentialKind: 'device_credential', capabilities: [] };
    useStore.setState({ serverConnections: { [source.connectionId]: source }, activeServerConnectionId: source.connectionId, activeServerConnection: source, currentSessionPath: 'native:s', currentSessionId: 's', sessions: [], sessionLocatorsById: {}, browserBySession: {} } as never);
    setBrowserStateForPath('native:gone', { executionNode: 'server-a', running: true, url: 'https://gone.example', thumbnail: 'old' });
    const outsideNode = { executionNode: 'server-b', connectionScope: 'other-node', sessionPath: 'native:foreign-node', running: true, url: 'https://other.example', thumbnail: 'foreign' };
    const outsideConnection = { executionNode: 'server-a', connectionScope: 'another-connection', sessionPath: 'native:foreign-connection', running: true, url: 'https://other.example', thumbnail: 'foreign' };
    useStore.setState(state => ({ browserBySession: { ...state.browserBySession, 'foreign-node': outsideNode, 'foreign-connection': outsideConnection } }));
    vi.useFakeTimers();
    const stop = startRustAutomationNotifications(source, { replaceActivities: () => ready.resolve(), reportError: error => errors.push(error), deliverEvent: (type, payload, origin, isCurrent) => handleAppEvent(type, payload, { source: origin, isCurrent }) });
    try {
      await ready.promise;
      await vi.waitFor(() => {
        expect(browserStateForPath(useStore.getState(), 'native:s')).toMatchObject({ running: true, url: 'https://page.example/first' });
        expect(browserStateForPath(useStore.getState(), 'native:gone').running).toBe(false);
      });
      setBrowserStateForPath('native:s', { ...browserStateForPath(useStore.getState(), 'native:s'), thumbnail: 'cached-thumbnail', thumbnailCapturedAt: 123, thumbnailUrl: 'https://page.example/first' });
      setBrowserCardCollapsed('native:s', true);
      phase = 1;
      await vi.advanceTimersByTimeAsync(0); await vi.advanceTimersByTimeAsync(5_000);
      await vi.waitFor(() => expect(browserStateForPath(useStore.getState(), 'native:s')).toMatchObject({ running: true, url: 'https://page.example/second', collapsed: true, thumbnail: 'cached-thumbnail', thumbnailCapturedAt: 123 }));
      phase = 2;
      await vi.advanceTimersByTimeAsync(0); await vi.advanceTimersByTimeAsync(5_000);
      await vi.waitFor(() => expect(errors).toHaveLength(1));
      expect(browserStateForPath(useStore.getState(), 'native:s')).toMatchObject({ running: true, url: 'https://page.example/second', collapsed: true, thumbnail: 'cached-thumbnail' });
      phase = 3;
      await vi.advanceTimersByTimeAsync(0); await vi.advanceTimersByTimeAsync(5_000);
      await vi.waitFor(() => expect(browserStateForPath(useStore.getState(), 'native:s')).toMatchObject({ running: false, thumbnail: null }));
      expect(useStore.getState().browserBySession['foreign-node']).toEqual(outsideNode);
      expect(useStore.getState().browserBySession['foreign-connection']).toEqual(outsideConnection);
      expect(paths.filter(path => path === '/lingxi/v1/browser/workspaces')).toHaveLength(4);
      expect(paths.filter(path => path === '/lingxi/v1/app-events?since=99')).toHaveLength(2);
      expect(paths.some(path => path.includes('/notifications'))).toBe(false);
    } finally { stop(); vi.useRealTimers(); server.closeAllConnections(); await new Promise<void>(resolve => server.close(() => resolve())); }
  }, 10_000);
});
