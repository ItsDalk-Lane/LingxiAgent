/**
 * @vitest-environment jsdom
 *
 * R09-T01 验收：Tauri 宿主（window.__TAURI__ 全局桥）的解析顺序、boot 基线
 * 成员透传、受限能力显式报错（禁静默 no-op）。本文件不要求真实 Tauri：
 * 只注入最小 `__TAURI__` 桩，真实宿主由 A01/A02 的构建产物证据覆盖。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  explainHostCapability,
  getDesktopHost,
  installDesktopHostApi,
  resetDesktopHostForTests,
} from '../../host/desktop-host';
import {
  createTauriDesktopHost,
  resetTauriContextMenuPolicyForTests,
  TAURI_FILE_URL_REASON,
  TAURI_ONBOARDING_REASON,
  type TauriGlobal,
} from '../../host/tauri-desktop-host';
import { HOST_CAPABILITY_UNAVAILABLE, isHostCapabilityError } from '../../host/host-capability-error';
import type { PlatformApi } from '../../types';

type InvokeMock = (command: string, args?: Record<string, unknown>) => Promise<unknown>;
type ListenMock = (
  event: string,
  handler: (message: { event: string; id: number; payload: unknown }) => void,
) => Promise<() => void>;

function tauriBridge(options: {
  invoke?: InvokeMock;
  listen?: ListenMock;
  convertFileSrc?: (path: string) => string;
  webviewWindow?: TauriGlobal['webviewWindow'];
} = {}): TauriGlobal {
  return {
    core: {
      invoke: (options.invoke ?? (async () => null)) as TauriGlobal['core']['invoke'],
      ...(options.convertFileSrc ? { convertFileSrc: options.convertFileSrc } : {}),
    },
    ...(options.webviewWindow ? { webviewWindow: options.webviewWindow } : {}),
    event: {
      listen: (options.listen ?? (async () => () => {})) as TauriGlobal['event']['listen'],
    },
  };
}

function mountTauri(bridge: ReturnType<typeof tauriBridge>): void {
  resetDesktopHostForTests();
  (window as { __TAURI__?: unknown }).__TAURI__ = bridge;
}

describe('R09-T01：Tauri DesktopHost', () => {
  beforeEach(() => {
    resetDesktopHostForTests();
    delete (window as { hana?: unknown }).hana;
    delete (window as { __TAURI__?: unknown }).__TAURI__;
    delete (window as { platform?: unknown }).platform;
  });

  afterEach(() => {
    resetDesktopHostForTests();
    delete (window as { hana?: unknown }).hana;
    delete (window as { __TAURI__?: unknown }).__TAURI__;
    delete (window as { platform?: unknown }).platform;
    resetTauriContextMenuPolicyForTests();
    vi.restoreAllMocks();
  });

  it('解析顺序 hana > __TAURI__ > platform > web，kind 与桥身份缓存正确', () => {
    const tauriHost = createTauriDesktopHost(tauriBridge());
    expect(tauriHost.kind).toBe('tauri');

    // 只有 __TAURI__：解析为 tauri。
    mountTauri(tauriBridge());
    expect(getDesktopHost().kind).toBe('tauri');
    // 桥身份不变时返回同一个缓存宿主。
    expect(getDesktopHost()).toBe(getDesktopHost());

    // Electron preload 桥优先于 Tauri（hana 是本仓库唯一全能力来源）。
    const hana = { getPlatform: async () => 'darwin' } as unknown as PlatformApi;
    (window as { hana?: unknown }).hana = hana;
    expect(getDesktopHost().kind).toBe('electron');
    delete (window as { hana?: unknown }).hana;

    // Tauri 桥消失、只剩 platform 别名：回落到收编/Web 宿主。
    delete (window as { __TAURI__?: unknown }).__TAURI__;
    expect(getDesktopHost().kind).toBe('web');
    expect(getDesktopHost().kind).not.toBe('tauri');
  });

  it('boot 基线成员透传到宿主命令，payload 原样返回', async () => {
    const invoke = vi.fn(async (command: string): Promise<unknown> => {
      if (command === 'get_server_connection_info') {
        return {
          port: 43120,
          token: 'a'.repeat(32),
          serverNodeKind: 'lingxi-service',
          serverNodeTransport: 'http',
          host: null,
        };
      }
      if (command === 'get_server_port') return 43120;
      if (command === 'get_server_token') return 'b'.repeat(32);
      if (command === 'get_platform') return 'darwin';
      if (command === 'get_app_version') return '0.1.43';
      if (command === 'window_is_maximized') return true;
      if (command === 'open_external') return { ok: true };
      return null;
    });
    mountTauri(tauriBridge({ invoke: invoke as unknown as InvokeMock }));
    const platform = getDesktopHost().api;

    await expect(platform.getServerConnectionInfo?.()).resolves.toEqual({
      port: 43120,
      token: 'a'.repeat(32),
      serverNodeKind: 'lingxi-service',
      serverNodeTransport: 'http',
      host: null,
    });
    await expect(platform.getServerPort()).resolves.toBe('43120');
    await expect(platform.getServerToken()).resolves.toBe('b'.repeat(32));
    await expect(platform.getPlatform?.()).resolves.toBe('darwin');
    await expect(platform.getAppVersion?.()).resolves.toBe('0.1.43');
    await expect(platform.windowIsMaximized?.()).resolves.toBe(true);
    await expect(platform.openExternal('https://example.com')).resolves.toEqual({ ok: true });

    platform.appReady();
    platform.syncWindowTheme?.('dark');
    platform.windowMinimize?.();
    platform.windowMaximize?.();
    platform.windowClose?.();
    await Promise.resolve();
    const commands = invoke.mock.calls.map(([command]) => command);
    expect(commands).toContain('app_ready');
    expect(commands).toContain('window_theme_changed');
    expect(commands).toContain('window_minimize');
    expect(commands).toContain('window_maximize');
    expect(commands).toContain('window_close');
  });

  it('onServerRestarted 走 listen("server-restarted")，返回可解除的 unlisten', async () => {
    const unlisten = vi.fn();
    const listen = vi.fn(
      async (
        _event: string,
        handler: (message: { event: string; id: number; payload: unknown }) => void,
      ) => {
        handler({
          event: 'server-restarted',
          id: 1,
          payload: { port: 43121, token: 'c'.repeat(32), serverNodeKind: 'lingxi-service' },
        });
        return unlisten;
      },
    );
    mountTauri(tauriBridge({ listen: listen as unknown as ListenMock }));
    const received: unknown[] = [];
    const dispose = getDesktopHost().api.onServerRestarted?.((data) => received.push(data));
    await Promise.resolve();
    expect(listen).toHaveBeenCalledWith('server-restarted', expect.any(Function));
    expect(received).toEqual([
      { port: 43121, token: 'c'.repeat(32), serverNodeKind: 'lingxi-service' },
    ]);
    dispose?.();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it('未接线能力调用即抛 HostCapabilityError（禁静默 no-op），理由可解释', () => {
    mountTauri(tauriBridge());
    const host = getDesktopHost();
    const platform = host.api as PlatformApi & Record<string, unknown>;

    for (const name of ['browserGoBack', 'autoUpdateCheck', 'browserReload']) {
      expect(typeof platform[name]).toBe('function');
      let thrown: unknown = null;
      try {
        (platform[name] as () => unknown)();
      } catch (error) {
        thrown = error;
      }
      expect(isHostCapabilityError(thrown), name).toBe(true);
      expect((thrown as { code?: string }).code).toBe(HOST_CAPABILITY_UNAVAILABLE);
      expect((thrown as { reason?: string }).reason).toBeTruthy();
      expect(explainHostCapability(name)).toBeTruthy();
    }
    // 已实现能力不附受限理由。
    expect(host.capabilityReason('getServerConnectionInfo')).toBeNull();
    expect(explainHostCapability('getServerConnectionInfo')).toBeNull();
    // 片段覆盖的成员已真实实现：不再抛错，理由为 null。
    expect(host.capabilityReason('selectFolder')).toBeNull();
    expect(host.capabilityReason('onSettingsChanged')).toBeNull();
    const subscribe = platform.onSettingsChanged as (callback: () => void) => (() => void);
    const dispose = subscribe(() => {});
    expect(typeof dispose).toBe('function');
    dispose();
    // v3.1：onShowSkillViewer 已由 Rust show-skill-viewer 事件真实接线（见 v3.1 用例）。
    expect(host.capabilityReason('onShowSkillViewer')).toBeNull();
    // 片段未覆盖但理由表登记过：受限但理由具体（不是泛化 R09-T03 兜底）。
    expect(host.capabilityReason('readFile')).toBeNull();
    expect(host.capabilityReason('watchWorkspace')).toBeNull();
    expect(host.capabilityReason('browserGoBack')).toContain('R09-T05');
    // R10 功能补齐：About 更新链已真实实现，不再是受限理由。
    expect(host.capabilityReason('releaseCheckLatest')).toBeNull();
    expect(host.capabilityReason('getUpdateDigestHistory')).toBeNull();
    expect(host.capabilityReason('autoUpdateCheck')).toContain('OWNER-03');
    expect(host.capabilityReason('getFilePath')).toBeNull();
    expect(platform.getFilePath?.(new File(['x'], 'x.txt'))).toBeNull();
  });

  it('resetDesktopHostForTests 清理桥身份缓存与 platform 别名', () => {
    mountTauri(tauriBridge());
    const first = getDesktopHost();
    expect(first.kind).toBe('tauri');
    expect((window as { platform?: unknown }).platform).toBe(first.api);
    resetDesktopHostForTests();
    expect((window as { platform?: unknown }).platform).toBeUndefined();
    const reinstalled = tauriBridge();
    (window as { __TAURI__?: unknown }).__TAURI__ = reinstalled;
    expect(getDesktopHost().kind).toBe('tauri');
    expect(getDesktopHost()).not.toBe(first);
  });

  it('T08：T03/T04 能力片段已合并——覆盖成员真实调用 invoke/listen，getFileUrl 走 convertFileSrc', async () => {
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
      if (command === 'select_folder') return '/tmp/folder';
      if (command === 'select_files') return ['/tmp/a.txt'];
      if (command === 'get_keep_awake_status') return { enabled: true, active: false };
      if (command === 'keybindings_reload_global') return { ok: true, shortcut: 'Cmd+Shift+Space' };
      return args ?? null;
    });
    const listens: Array<{ event: string; handler: (message: { event: string; id: number; payload: unknown }) => void }> = [];
    const listen = vi.fn(async (event: string, handler: (message: { event: string; id: number; payload: unknown }) => void) => {
      listens.push({ event, handler });
      return () => {};
    });
    const convertFileSrc = vi.fn((path: string) => `asset://localhost/${encodeURIComponent(path)}`);
    mountTauri(tauriBridge({
      invoke: invoke as unknown as InvokeMock,
      listen: listen as unknown as ListenMock,
      convertFileSrc,
    }));
    const host = getDesktopHost();
    const platform = host.api;

    await expect(platform.selectFolder?.()).resolves.toBe('/tmp/folder');
    await expect(platform.selectFiles?.({ multiple: false })).resolves.toEqual(['/tmp/a.txt']);
    await expect(platform.getKeepAwakeStatus?.()).resolves.toEqual({ enabled: true, active: false });
    await expect(platform.keybindingsReloadGlobal?.()).resolves.toEqual({ ok: true, shortcut: 'Cmd+Shift+Space' });
    expect(invoke.mock.calls.map(([command]) => command)).toContain('select_folder');
    expect(invoke.mock.calls.map(([command]) => command)).toContain('select_files');
    expect(platform.getFileUrl?.('/tmp/x.png')).toBe(`asset://localhost/${encodeURIComponent('/tmp/x.png')}`);
    expect(convertFileSrc).toHaveBeenCalledWith('/tmp/x.png');
    expect(host.capabilityReason('getFileUrl')).toBeNull();
    expect(host.capabilityReason('selectFiles')).toBeNull();

    const received: boolean[] = [];
    platform.onMaximizeChange?.((maximized) => received.push(maximized));
    await Promise.resolve();
    const events = listens.map((entry) => entry.event);
    expect(events).toContain('window-maximized');
    expect(events).toContain('window-unmaximized');
    listens.find((entry) => entry.event === 'window-maximized')?.handler({ event: 'window-maximized', id: 1, payload: null });
    await Promise.resolve();
    expect(received).toEqual([true]);

    // 通知失败如实回执（不抛、不伪装成功）。
    invoke.mockRejectedValueOnce(new Error('notification unavailable'));
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
    await expect(platform.showNotification?.('标题', '正文')).resolves.toEqual({ shown: false, reason: 'failed' });
    warnSpy.mockRestore();
  });

  it('T08：getFileUrl 缺 convertFileSrc 时显式 HostCapabilityError+asset scope 理由', () => {
    mountTauri(tauriBridge());
    const host = getDesktopHost();
    expect(host.capabilityReason('getFileUrl')).toBe(TAURI_FILE_URL_REASON);
    let thrown: unknown = null;
    try {
      host.api.getFileUrl?.('/tmp/x.png');
    } catch (error) {
      thrown = error;
    }
    expect(isHostCapabilityError(thrown)).toBe(true);
    expect((thrown as { reason?: string }).reason).toContain('assetProtocol');
  });

  it('T08：默认右键策略与 Electron 等价——非编辑区域拦截，可编辑区域保留系统菜单', () => {
    mountTauri(tauriBridge());
    getDesktopHost();
    const body = document.createElement('div');
    document.body.appendChild(body);
    const bodyMenu = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
    body.dispatchEvent(bodyMenu);
    expect(bodyMenu.defaultPrevented).toBe(true);

    const editable = document.createElement('textarea');
    document.body.appendChild(editable);
    const editableMenu = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
    editable.dispatchEvent(editableMenu);
    expect(editableMenu.defaultPrevented).toBe(false);
  });

  it('T08：onboarding_complete NOT_IMPLEMENTED 登记为显式受限（不静默）', async () => {
    const invoke = vi.fn(async (command: string): Promise<unknown> => {
      if (command === 'onboarding_complete') throw new Error('NOT_IMPLEMENTED: onboarding_complete');
      return null;
    });
    mountTauri(tauriBridge({ invoke: invoke as unknown as InvokeMock }));
    const host = getDesktopHost();
    expect(host.capabilityReason('onboardingComplete')).toBe(TAURI_ONBOARDING_REASON);
    let thrown: unknown = null;
    try {
      await host.api.onboardingComplete?.();
    } catch (error) {
      thrown = error;
    }
    expect(isHostCapabilityError(thrown)).toBe(true);
    expect((thrown as { reason?: string }).reason).toContain('NOT_IMPLEMENTED');
  });

  it('T08/v3：file-text-io 真实命令映射（读写/快照/监听事件）', async () => {
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
      if (command === 'read_file') return 'hello';
      if (command === 'write_file') return true;
      if (command === 'write_file_if_unchanged') return { ok: true, conflict: false, version: null };
      if (command === 'watch_file' || command === 'unwatch_file') return true;
      return args ?? null;
    });
    const listeners: Array<{ event: string; handler: (m: { event: string; id: number; payload: unknown }) => void }> = [];
    const listen = vi.fn(async (event: string, handler: (m: { event: string; id: number; payload: unknown }) => void) => {
      listeners.push({ event, handler });
      return () => {};
    });
    mountTauri(tauriBridge({ invoke: invoke as unknown as InvokeMock, listen: listen as unknown as ListenMock }));
    const platform = getDesktopHost().api;
    await expect(platform.readFile('/tmp/a.txt')).resolves.toBe('hello');
    await expect(platform.writeFile('/tmp/a.txt', 'x')).resolves.toBe(true);
    await expect(platform.writeFileIfUnchanged?.('/tmp/a.txt', 'y', { mtimeMs: 1, size: 1, sha256: 'a' })).resolves.toEqual(
      { ok: true, conflict: false, version: null },
    );
    await expect(platform.watchFile?.('/tmp/a.txt')).resolves.toBe(true);
    await expect(platform.unwatchFile?.('/tmp/a.txt')).resolves.toBe(true);
    const calls = invoke.mock.calls.map(([command]) => command);
    expect(calls).toEqual(expect.arrayContaining(['read_file', 'write_file', 'write_file_if_unchanged', 'watch_file']));
    const watchArgs = invoke.mock.calls.find(([command]) => command === 'watch_file')?.[1];
    expect(watchArgs).toEqual({ path: '/tmp/a.txt' });
    const received: string[] = [];
    platform.onFileChanged?.((filePath) => received.push(filePath));
    await Promise.resolve();
    const changed = listeners.find((entry) => entry.event === 'file-changed');
    changed?.handler({ event: 'file-changed', id: 1, payload: { filePath: '/tmp/a.txt' } });
    expect(received).toEqual(['/tmp/a.txt']);
  });

  it('T08/v3：detached-viewer 真实窗口命令映射（label→windowId、payload 归一、close）', async () => {
    const invoke = vi.fn(async (command: string, _args?: Record<string, unknown>): Promise<unknown> => {
      if (command === 'spawn_viewer') return 'viewer-7';
      if (command === 'viewer_request_load') return { filePath: '/tmp/note.md', title: 'note', readOnly: true };
      if (command === 'viewer_close') return null;
      return null;
    });
    mountTauri(tauriBridge({
      invoke: invoke as unknown as InvokeMock,
      webviewWindow: { getCurrentWebviewWindow: () => ({ label: 'viewer-7' }) },
    }));
    const platform = getDesktopHost().api;
    await expect(platform.spawnViewer?.({ filePath: '/tmp/note.md', title: 'note', type: 'markdown' })).resolves.toBe(7);
    await expect(platform.viewerRequestLoad?.()).resolves.toEqual({
      filePath: '/tmp/note.md', title: 'note', type: 'markdown', language: null, windowId: 7,
    });
    platform.viewerClose?.();
    await Promise.resolve();
    expect(invoke.mock.calls.map(([command]) => command)).toContain('spawn_viewer');
    expect(invoke.mock.calls.map(([command]) => command)).toContain('viewer_request_load');
    expect(invoke.mock.calls.map(([command]) => command)).toContain('viewer_close');
    expect(invoke.mock.calls.find(([command]) => command === 'spawn_viewer')?.[1]).toEqual({ path: '/tmp/note.md' });
  });

  it('T08/v3：skill-viewer 真实命令映射（open/list/read）', async () => {
    const invoke = vi.fn(async (command: string, _args?: Record<string, unknown>): Promise<unknown> => {
      if (command === 'list_skill_files') return [{ name: 'a.md', path: '/skills/a.md', isDir: false, children: [] }];
      if (command === 'read_skill_file') return '# skill';
      return null;
    });
    mountTauri(tauriBridge({ invoke: invoke as unknown as InvokeMock }));
    const platform = getDesktopHost().api;
    platform.openSkillViewer?.({ skillPath: '/skills/demo', name: 'demo', installed: true });
    await expect(platform.listSkillFiles?.('/skills/demo')).resolves.toEqual(
      [{ name: 'a.md', path: '/skills/a.md', isDir: false, children: [] }],
    );
    await expect(platform.readSkillFile?.('/skills/a.md')).resolves.toBe('# skill');
    await Promise.resolve();
    expect(invoke.mock.calls.find(([command]) => command === 'open_skill_viewer')?.[1]).toEqual({
      data: { name: 'demo', baseDir: '/skills/demo', filePath: null, installed: true },
    });
    expect(invoke.mock.calls.find(([command]) => command === 'list_skill_files')?.[1]).toEqual({ baseDir: '/skills/demo' });
    expect(invoke.mock.calls.find(([command]) => command === 'read_skill_file')?.[1]).toEqual({ path: '/skills/a.md' });
  });

  it('T08/v3：browser-viewer 真实命令映射（连接 key/打开/请求/关闭/connection-changed）', async () => {
    const invoke = vi.fn(async (command: string, _args?: Record<string, unknown>): Promise<unknown> => {
      if (command === 'set_browser_connection') return 'key-1';
      if (command === 'get_browser_viewer_context') return { connectionKey: 'key-1', sessionPath: '/s', executionNode: 'n', serviceLabel: 'l', serviceUrl: 'http://127.0.0.1:1' };
      if (command === 'rust_browser_request') return { ok: true };
      return null;
    });
    const listeners: Array<{ event: string; handler: (m: { event: string; id: number; payload: unknown }) => void }> = [];
    const listen = vi.fn(async (event: string, handler: (m: { event: string; id: number; payload: unknown }) => void) => {
      listeners.push({ event, handler });
      return () => {};
    });
    mountTauri(tauriBridge({ invoke: invoke as unknown as InvokeMock, listen: listen as unknown as ListenMock }));
    const platform = getDesktopHost().api;
    const connection = {
      connectionId: 'c1', kind: 'local', serverId: 's1', baseUrl: 'http://127.0.0.1:1',
      token: 't', authState: 'authenticated', trustState: 'trusted', credentialKind: 'token', label: 'local',
    } as unknown as Parameters<NonNullable<typeof platform.setBrowserConnection>>[0];
    await expect(platform.setBrowserConnection?.(connection)).resolves.toBe('key-1');
    await expect(platform.getBrowserViewerContext?.()).resolves.toEqual({
      connectionKey: 'key-1', sessionPath: '/s', executionNode: 'n', serviceLabel: 'l', serviceUrl: 'http://127.0.0.1:1',
    });
    platform.openBrowserViewer?.({ sessionPath: '/s' });
    await expect(platform.rustBrowserRequest?.({ sessionPath: '/s' }, 'back', { tabId: 't1' })).resolves.toEqual({ ok: true });
    platform.closeBrowserViewer?.();
    await Promise.resolve();
    const openArgs = invoke.mock.calls.find(([command]) => command === 'open_browser_viewer')?.[1];
    expect(openArgs).toEqual({ target: { connectionKey: 'key-1', sessionPath: '/s' } });
    const requestArgs = invoke.mock.calls.find(([command]) => command === 'rust_browser_request')?.[1];
    expect(requestArgs).toEqual({ target: { connectionKey: 'key-1', sessionPath: '/s' }, action: 'back', options: { tabId: 't1' } });
    expect(invoke.mock.calls.map(([command]) => command)).toContain('close_browser_viewer');
    const updates: Array<Record<string, unknown>> = [];
    platform.onBrowserUpdate?.((data) => updates.push(data as Record<string, unknown>));
    await Promise.resolve();
    const changed = listeners.find((entry) => entry.event === 'browser:connection-changed');
    expect(changed).toBeTruthy();
    changed?.handler({ event: 'browser:connection-changed', id: 1, payload: { active: false, reason: 'connection-changed' } });
    expect(updates).toEqual([{ running: false, reason: 'connection-changed' }]);
  });

  it('T08/v3：file-drop-paths 通过 Tauri drag-drop 事件映射 getFilePath（顺序消费）', async () => {
    let dropHandler: (event: { payload?: { type?: string; paths?: string[] } }) => void = () => {};
    const webviewWindow = {
      getCurrentWebviewWindow: () => ({
        label: 'main',
        onDragDropEvent: async (handler: (event: { payload?: { type?: string; paths?: string[] } }) => void) => {
          dropHandler = handler;
          return () => {};
        },
      }),
    };
    mountTauri(tauriBridge({ webviewWindow }));
    const platform = getDesktopHost().api;
    await Promise.resolve();
    dropHandler({ payload: { type: 'drop', paths: ['/tmp/first.txt', '/tmp/second.txt'] } });
    expect(platform.getFilePath?.(new File(['a'], 'first.txt'))).toBe('/tmp/first.txt');
    expect(platform.getFilePath?.(new File(['b'], 'second.txt'))).toBe('/tmp/second.txt');
    expect(platform.getFilePath?.(new File(['c'], 'third.txt'))).toBeNull();
  });

  it('T08/v3.1：workspace 监听 / viewer-closed / show-skill-viewer 真实命令与事件', async () => {
    const invoke = vi.fn(async (command: string, _args?: Record<string, unknown>): Promise<unknown> => {
      if (command === 'watch_workspace' || command === 'unwatch_workspace') return true;
      return null;
    });
    const listeners: Array<{ event: string; handler: (m: { event: string; id: number; payload: unknown }) => void }> = [];
    const listen = vi.fn(async (event: string, handler: (m: { event: string; id: number; payload: unknown }) => void) => {
      listeners.push({ event, handler });
      return () => {};
    });
    mountTauri(tauriBridge({ invoke: invoke as unknown as InvokeMock, listen: listen as unknown as ListenMock }));
    const platform = getDesktopHost().api;
    await expect(platform.watchWorkspace?.('/tmp/ws')).resolves.toBe(true);
    await expect(platform.unwatchWorkspace?.('/tmp/ws')).resolves.toBe(true);
    expect(invoke.mock.calls.find(([command]) => command === 'watch_workspace')?.[1]).toEqual({ rootPath: '/tmp/ws' });
    expect(invoke.mock.calls.find(([command]) => command === 'unwatch_workspace')?.[1]).toEqual({ rootPath: '/tmp/ws' });

    const workspacePayloads: unknown[] = [];
    platform.onWorkspaceChanged?.((payload) => workspacePayloads.push(payload));
    const viewerIds: number[] = [];
    platform.onViewerClosed?.((windowId) => viewerIds.push(windowId));
    const skillPayloads: unknown[] = [];
    platform.onShowSkillViewer?.((data) => skillPayloads.push(data));
    await Promise.resolve();
    const changed = listeners.find((entry) => entry.event === 'workspace-changed');
    expect(changed).toBeTruthy();
    changed?.handler({ event: 'workspace-changed', id: 1, payload: {
      rootPath: '/tmp/ws', changedPath: '/tmp/ws/a.txt', affectedDir: '/tmp/ws', eventType: 'change',
    } });
    expect(workspacePayloads).toEqual([{
      rootPath: '/tmp/ws', changedPath: '/tmp/ws/a.txt', affectedDir: '/tmp/ws', eventType: 'change',
    }]);
    listeners.find((entry) => entry.event === 'viewer-closed')
      ?.handler({ event: 'viewer-closed', id: 2, payload: 5 });
    expect(viewerIds).toEqual([5]);
    listeners.find((entry) => entry.event === 'show-skill-viewer')
      ?.handler({ event: 'show-skill-viewer', id: 3, payload: { name: 'demo', baseDir: '/skills/demo', installed: false } });
    expect(skillPayloads).toEqual([{ name: 'demo', baseDir: '/skills/demo', installed: false }]);
  });

  it('显式安装的宿主仍可覆盖解析（安装顺序语义不变）', () => {
    const host = installDesktopHostApi(
      { getPlatform: async () => 'linux' } as unknown as PlatformApi,
      'tauri',
    );
    expect(getDesktopHost()).toBe(host);
    expect(host.kind).toBe('tauri');
  });
});
