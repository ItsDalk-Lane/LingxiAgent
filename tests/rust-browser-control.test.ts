import { createRequire } from 'node:module';
import { describe, expect, it, vi } from 'vitest';
const { createRustBrowserControl } = createRequire(import.meta.url)('../desktop/src/shared/rust-browser-control.cjs');
const connection = { connectionId: 'remote', serverNodeKind: 'lingxi-service', serverId: 'server-a', baseUrl: 'https://service.example', token: 'secret', label: '远程服务' };
function response(data: unknown) { return { ok: true, status: 200, json: async () => data }; }

describe('Rust browser trusted IPC controller', () => {
  it('keeps credentials private and binds commands to an explicit native session', async () => {
    const fetch = vi.fn(async (..._args: any[]) => response({ state: {} }));
    const control = createRustBrowserControl({ fetch });
    const key = control.setConnection(connection);
    const target = { connectionKey: key, sessionPath: 'native:session-a' };
    expect(control.context(target)).toEqual({ ...target, executionNode: 'server-a', serviceLabel: '远程服务', serviceUrl: connection.baseUrl });
    await control.call(target, 'new_tab');
    expect(fetch).toHaveBeenCalledWith('https://service.example/lingxi/v1/browser/command', expect.objectContaining({ redirect: 'error', headers: expect.objectContaining({ Authorization: 'Bearer secret' }), body: JSON.stringify({ sessionPath: target.sessionPath, action: 'new_tab' }) }));
    await expect(control.call({ ...target, sessionPath: '/tmp/legacy.jsonl' }, 'reload')).rejects.toThrow('browser_native_session_required');
    await expect(control.call(target, 'evaluate')).rejects.toThrow('browser_action_invalid');
    expect(fetch).toHaveBeenCalledTimes(1);
  });
  it('aborts old requests and rejects late responses after switching away and back', async () => {
    let resolve!: (value: unknown) => void;
    const fetch = vi.fn((..._args: any[]) => new Promise(done => { resolve = done; }));
    const control = createRustBrowserControl({ fetch });
    const old = { connectionKey: control.setConnection(connection), sessionPath: 'native:s' };
    const pending = control.call(old, 'show');
    control.setConnection({ ...connection, token: 'replacement' });
    control.setConnection(connection);
    expect(fetch.mock.calls[0][1].signal.aborted).toBe(true);
    resolve(response({ state: {} }));
    await expect(pending).rejects.toThrow('browser_connection_changed');
    await expect(control.call(old, 'new_tab')).rejects.toThrow('browser_connection_changed');
    expect(fetch).toHaveBeenCalledTimes(1);
  });
  it('routes stop through real service revocation and never accepts a command endpoint', async () => {
    const fetch = vi.fn(async (..._args: any[]) => response({ ok: true }));
    const control = createRustBrowserControl({ fetch });
    const key = control.setConnection(connection);
    await control.call({ connectionKey: key, sessionPath: 'native:s', baseUrl: 'https://attacker.example' }, 'stop');
    expect(fetch.mock.calls[0][0]).toBe('https://service.example/lingxi/v1/browser/close-session');
    expect(JSON.parse(fetch.mock.calls[0][1].body)).toEqual({ sessionPath: 'native:s', revoke: true });
    expect(() => control.setConnection({ ...connection, baseUrl: 'https://user:password@service.example' })).toThrow('browser_connection_invalid');
  });
});
