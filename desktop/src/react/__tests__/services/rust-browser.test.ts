/** @vitest-environment jsdom */
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';
import { openRustBrowserViewer, rustBrowserTarget, startRustBrowserConnectionObserver } from '../../services/rust-browser';
import type { ServerConnection } from '../../services/server-connection';
const connection = (id: string): ServerConnection => ({ connectionId: id, kind: 'custom_remote', serverId: `server-${id}`, serverNodeKind: 'lingxi-service', studioId: 'studio', label: id, baseUrl: `https://${id}.example`, wsUrl: `wss://${id}.example`, token: `${id}-token`, authState: 'paired', trustState: 'tunnel', credentialKind: 'device_credential', capabilities: [] });
function select(id: string) { const conn = connection(id); useStore.setState({ serverConnections: { [id]: conn }, activeServerConnectionId: id, activeServerConnection: conn }); }
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(done => { resolve = done; }); return { promise, resolve }; }
const register = vi.fn(); const request = vi.fn(); const open = vi.fn();
beforeEach(() => {
  register.mockReset().mockImplementation(async (conn: ServerConnection | null) => conn ? `handle:${conn.connectionId}` : null);
  request.mockReset().mockResolvedValue({ ok: true }); open.mockReset().mockResolvedValue({ opened: true });
  Object.defineProperty(window, 'platform', { configurable: true, value: { setBrowserConnection: register, rustBrowserRequest: request, openBrowserViewer: open } });
  select('a'); startRustBrowserConnectionObserver();
});
describe('Rust browser connection and session ownership', () => {
  it('uses the trusted handle and explicit native session for restore and open', async () => {
    await openRustBrowserViewer('native:s', true);
    expect(register).toHaveBeenCalledWith(connection('a'));
    expect(request).toHaveBeenCalledWith({ sessionPath: 'native:s', connectionKey: 'handle:a' }, 'open');
    expect(open).toHaveBeenCalledWith({ sessionPath: 'native:s', connectionKey: 'handle:a' });
    expect(JSON.stringify(open.mock.calls)).not.toContain('a-token');
  });
  it('never falls back to the local Electron viewer when restore fails', async () => {
    request.mockRejectedValueOnce(new Error('restore denied'));
    await expect(openRustBrowserViewer('native:s', true)).rejects.toThrow('restore denied');
    expect(open).not.toHaveBeenCalled();
    await expect(rustBrowserTarget('/tmp/legacy.jsonl')).rejects.toThrow('browser_native_session_required');
  });
  it('opens a supplied URL as a new tab in the explicit service session', async () => {
    await openRustBrowserViewer('native:link-owner', false, 'https://example.org/link');
    expect(request).toHaveBeenCalledWith({ sessionPath: 'native:link-owner', connectionKey: 'handle:a' }, 'new_tab', { url: 'https://example.org/link' });
    expect(open).toHaveBeenCalledWith({ sessionPath: 'native:link-owner', connectionKey: 'handle:a' });
    expect(request.mock.invocationCallOrder[0]).toBeLessThan(open.mock.invocationCallOrder[0]);
    await expect(openRustBrowserViewer('', false, 'https://example.org/link')).rejects.toThrow('browser_native_session_required');
    await expect(openRustBrowserViewer('native:link-owner', false, 'file:///private/file')).rejects.toThrow('browser_url_invalid');
    expect(request).toHaveBeenCalledTimes(1);
  });
  it('never opens a stale link window or falls back when creating the remote tab fails', async () => {
    request.mockRejectedValueOnce(new Error('new tab denied'));
    await expect(openRustBrowserViewer('native:s', false, 'https://example.org/link')).rejects.toThrow('new tab denied');
    expect(open).not.toHaveBeenCalled();
    const pending = deferred<unknown>(); request.mockReturnValueOnce(pending.promise);
    const opening = openRustBrowserViewer('native:s', false, 'https://example.org/late');
    await vi.waitFor(() => expect(request).toHaveBeenCalledTimes(2));
    select('b'); pending.resolve({ ok: true });
    await expect(opening).rejects.toThrow('browser_connection_changed');
    expect(open).not.toHaveBeenCalled();
  });
  it('discards a delayed handle after switching away and back', async () => {
    const pending = deferred<string>(); register.mockImplementation((conn: ServerConnection | null) => conn?.connectionId === 'delayed' ? pending.promise : Promise.resolve('new'));
    select('delayed');
    const target = rustBrowserTarget('native:s');
    select('b'); select('delayed');
    pending.resolve('old-handle');
    await expect(target).rejects.toThrow('browser_connection_changed');
    expect(open).not.toHaveBeenCalled();
  });
  it('does not open a late restored workspace after connection change', async () => {
    const pending = deferred<unknown>(); request.mockReturnValueOnce(pending.promise);
    const opening = openRustBrowserViewer('native:s', true);
    await vi.waitFor(() => expect(request).toHaveBeenCalledTimes(1));
    select('b'); pending.resolve({ ok: true });
    await expect(opening).rejects.toThrow('browser_connection_changed');
    expect(open).not.toHaveBeenCalled();
  });
  it('does not open a restored workspace after switching assistants', async () => {
    useStore.setState({ currentAgentId: 'agent-a' });
    const pending = deferred<unknown>(); request.mockReturnValueOnce(pending.promise);
    const opening = openRustBrowserViewer('native:s', true);
    await vi.waitFor(() => expect(request).toHaveBeenCalledTimes(1));
    useStore.setState({ currentAgentId: 'agent-b' }); pending.resolve({ ok: true });
    await expect(opening).rejects.toThrow('browser_connection_changed');
    expect(open).not.toHaveBeenCalled();
  });
  it('removes previous service browser state synchronously on identity change', () => {
    useStore.setState({ browserBySession: { old: { sessionPath: 'native:s', executionNode: 'server-a', running: true, url: null, thumbnail: null } } });
    select('b');
    expect(useStore.getState().browserBySession).toEqual({});
  });
});
