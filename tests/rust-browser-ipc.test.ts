import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { parse } from 'acorn';
import { describe, expect, it, vi } from 'vitest';

/** 执行 main.cjs 的正式 IPC 处理器，仅替换窗口与联网出口。 */
function harness() {
  const source = readFileSync(resolve('desktop/main.cjs'), 'utf8');
  const ast = parse(source, { ecmaVersion: 'latest', sourceType: 'script' });
  const wanted = new Set(['set-browser-connection', 'get-browser-viewer-context', 'rust-browser-request', 'open-browser-viewer', 'browser-go-back', 'browser-emergency-stop']);
  const snippets = ast.body.filter((node: any) => node.type === 'ExpressionStatement' && ['wrapIpcHandler', 'wrapIpcBestEffortHandler'].includes(node.expression.callee?.name) && wanted.has(node.expression.arguments[0]?.value)).map(node => source.slice(node.start, node.end));
  const handlers = new Map<string, (...args: any[]) => any>();
  const windows: any[] = [];
  class FakeWindow {
    webContents = {}; destroyed = false; callbacks = new Map<string, () => void>();
    constructor(public options: unknown) { windows.push(this); }
    on(name: string, callback: () => void) { this.callbacks.set(name, callback); }
    isDestroyed() { return this.destroyed; }
    destroy() { this.destroyed = true; this.callbacks.get('closed')?.(); }
  }
  const connection = { connectionId: 'remote', serverNodeKind: 'lingxi-service', serverId: 'server-a', baseUrl: 'https://service.example', token: 'private-token', label: '远程服务' };
  const fetch = vi.fn(async () => ({ ok: true, json: async () => ({ workspaces: [] }) }));
  const { createRustBrowserControl } = createRequire(import.meta.url)('../desktop/src/shared/rust-browser-control.cjs');
  const mainWindow = { webContents: {} };
  const forbidden = vi.fn(() => { throw new Error('local Electron path used'); });
  const context = vm.createContext({ mainWindow, serverNodeKind: 'lingxi-service', rustBrowserViewerWindow: null, rustBrowserViewerTarget: null,
    BrowserWindow: FakeWindow, path: { join: (...args: string[]) => args.join('/') }, __dirname: '/desktop', titleBarOpts: () => ({}), getThemeBackgroundColor: () => '#fff', themeRegistry: { DEFAULT_THEME: 'light' }, applyWindowThemeColors: vi.fn(), attachRendererArtifactCrashSentinel: vi.fn(), loadWindowURL: vi.fn(),
    createBrowserViewerWindow: forbidden, _resolveBrowserIpcSessionPath: forbidden, closeBrowserSessionViaServer: forbidden,
    wrapIpcHandler: (name: string, handler: (...args: any[]) => any) => handlers.set(name, handler), wrapIpcBestEffortHandler: (name: string, handler: (...args: any[]) => any) => handlers.set(name, handler),
  });
  context.rustBrowserControl = createRustBrowserControl({ fetch, onChange: () => { context.rustBrowserViewerTarget = null; const win = context.rustBrowserViewerWindow; if (win && !win.isDestroyed()) win.destroy(); context.rustBrowserViewerWindow = null; } });
  vm.runInContext(snippets.join('\n'), context);
  return { invoke: (name: string, sender: unknown, ...args: unknown[]) => handlers.get(name)!({ sender }, ...args), main: mainWindow.webContents, windows, forbidden, connection, fetch };
}

describe('production Rust browser IPC boundary', () => {
  it('creates a distinct status window and never constructs the legacy web view', async () => {
    const h = harness();
    const connectionKey = h.invoke('set-browser-connection', h.main, h.connection);
    const target = { connectionKey, sessionPath: 'native:s' };
    await h.invoke('open-browser-viewer', h.main, 'light', target);
    expect(h.windows).toHaveLength(1);
    expect(h.forbidden).not.toHaveBeenCalled();
    const context = h.invoke('get-browser-viewer-context', h.windows[0].webContents);
    expect(context).toMatchObject({ ...target, executionNode: 'server-a' });
    expect(JSON.stringify(context)).not.toContain('private-token');
    expect(h.invoke('get-browser-viewer-context', {})).toBeNull();
  });
  it('reopens another session without the old window clearing the new scope', async () => {
    const h = harness();
    const connectionKey = h.invoke('set-browser-connection', h.main, h.connection);
    await h.invoke('open-browser-viewer', h.main, 'light', { connectionKey, sessionPath: 'native:first' });
    await h.invoke('open-browser-viewer', h.main, 'light', { connectionKey, sessionPath: 'native:second' });
    expect(h.windows[0].destroyed).toBe(true);
    expect(h.invoke('get-browser-viewer-context', h.windows[1].webContents)).toMatchObject({ connectionKey, sessionPath: 'native:second' });
  });
  it('denies viewer credential replacement and cross-session commands', async () => {
    const h = harness();
    const connectionKey = h.invoke('set-browser-connection', h.main, h.connection);
    const target = { connectionKey, sessionPath: 'native:s' };
    await h.invoke('open-browser-viewer', h.main, 'light', target);
    const viewer = h.windows[0].webContents;
    expect(() => h.invoke('set-browser-connection', viewer, h.connection)).toThrow('browser_connection_sender_denied');
    expect(() => h.invoke('rust-browser-request', viewer, { ...target, sessionPath: 'native:other' }, 'show')).toThrow('browser_session_changed');
    await h.invoke('rust-browser-request', viewer, target, 'new_tab');
    expect(h.fetch).toHaveBeenCalledTimes(1);
  });
  it('closes the old window on identity change and rejects every legacy Rust fallback', async () => {
    const h = harness();
    const target = { connectionKey: h.invoke('set-browser-connection', h.main, h.connection), sessionPath: 'native:s' };
    await h.invoke('open-browser-viewer', h.main, 'light', target);
    h.invoke('set-browser-connection', h.main, { ...h.connection, token: 'new-token' });
    expect(h.windows[0].destroyed).toBe(true);
    await expect(h.invoke('open-browser-viewer', h.main, 'light', target)).rejects.toThrow('browser_connection_changed');
    await expect(h.invoke('open-browser-viewer', h.main, 'light', { sessionPath: 'native:s' })).rejects.toThrow('browser_connection_required');
    expect(() => h.invoke('browser-go-back', h.main, 'native:s')).toThrow('browser_connection_required');
    expect(() => h.invoke('browser-emergency-stop', h.main, 'native:s')).toThrow('browser_connection_required');
    expect(h.forbidden).not.toHaveBeenCalled();
  });
});
