import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';
import { captureSessionConnection } from '../../services/session-connection-owner';
import { loadDeskFiles, loadDeskTreeFiles, loadJianContent, deskTrashTreeItems } from '../../stores/desk-actions';
import type { ServerConnection } from '../../services/server-connection';

const connection: ServerConnection = {
  connectionId: 'r07-desk', kind: 'local', serverId: 'local', serverNodeKind: 'lingxi-service', studioId: 'local',
  label: '本机', baseUrl: 'http://127.0.0.1:3210', wsUrl: 'ws://127.0.0.1:3210', token: 'r07-desk-token',
  authState: 'paired', trustState: 'local', credentialKind: 'loopback_token', capabilities: [],
};
const json = (value: unknown) => ({ ok: true, status: 200, json: async () => value }) as Response;
beforeEach(() => {
  useStore.setState({ serverConnections: { [connection.connectionId]: connection }, activeServerConnectionId: connection.connectionId, activeServerConnection: connection });
  captureSessionConnection();
  useStore.setState({ currentSessionId: null, currentSessionPath: null, deskBasePath: '/tmp/approved-r07-desk', deskWorkspaceMountId: null,
    deskTreeFilesByPath: {}, deskFiles: [], deskJianContent: null, selectedAgentId: null, currentAgentId: null });
});
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); });

describe('Rust 工作台实际页面操作链', () => {
  it('欢迎页无需先建立聊天，即可读取批准目录、展开树和显示便笺', async () => {
    const fetchImpl = vi.fn(async (input: RequestInfo | URL) => {
      const url = new URL(String(input));
      if (url.pathname.endsWith('/desk/jian')) return json({ content: '# 当前便笺' });
      return json({ basePath: '/tmp/approved-r07-desk', subdir: url.searchParams.get('subdir') || '', files: [{ name: 'note.md', isDir: false, size: 7 }] });
    });
    vi.stubGlobal('fetch', fetchImpl);
    await loadDeskFiles();
    await vi.waitFor(() => expect(useStore.getState().deskJianContent).toBe('# 当前便笺'));
    expect(useStore.getState().deskFiles[0].name).toBe('note.md');
    expect(await loadDeskTreeFiles('notes', { force: true })).toBe(true);
    expect(fetchImpl.mock.calls.some(([url]) => String(url).includes('/lingxi/v1/desk/files?dir='))).toBe(true);
    expect(fetchImpl.mock.calls.some(([url]) => String(url).includes('subdir=notes'))).toBe(true);
    expect(fetchImpl.mock.calls.some(([url]) => String(url).includes('/workspace-files'))).toBe(false);
  });

  it('选中的挂载工作台可以读取文件和便笺，返回错误归属则保留原树', async () => {
    useStore.setState({ deskBasePath: 'studio:mount-a', deskWorkspaceMountId: 'mount-a' });
    let mountId = 'mount-a';
    const fetchImpl = vi.fn(async (input: RequestInfo | URL) => String(input).includes('/workbench/content')
      ? ({ ok: true, status: 200, text: async () => '挂载便笺' } as Response)
      : json({ mountId, mount: { nativeRootPath: '/tmp/mount-a', label: '资料' }, files: [{ name: 'mounted.md', isDir: false }] }));
    vi.stubGlobal('fetch', fetchImpl);
    await loadDeskFiles();
    await vi.waitFor(() => expect(useStore.getState().deskJianContent).toBe('挂载便笺'));
    expect(useStore.getState().deskFiles[0].name).toBe('mounted.md');
    mountId = 'foreign'; const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(await loadDeskTreeFiles('notes', { force: true })).toBe(false);
    expect(error).toHaveBeenCalled();
    expect(useStore.getState().deskFiles[0].name).toBe('mounted.md');
  });

  it('换工作区后迟到的旧便笺不覆盖新页面', async () => {
    let deliver!: (value: unknown) => void;
    vi.stubGlobal('fetch', vi.fn(async () => ({ ok: true, json: () => new Promise(resolve => { deliver = resolve; }) })));
    const pending = loadJianContent();
    await vi.waitFor(() => expect(deliver).toBeTypeOf('function'));
    useStore.setState({ deskBasePath: '/tmp/new-desk', deskJianContent: '新便笺' });
    deliver({ content: '旧便笺' }); await pending;
    expect(useStore.getState().deskJianContent).toBe('新便笺');
  });

  it('本机Rust批准目录删除仍走服务，不能误删默认挂载或绕到桌面壳', async () => {
    const trashItem = vi.fn();
    vi.stubGlobal('window', { t: (key: string) => key, platform: { trashItem } });
    const fetchImpl = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) => json({ ok: true, files: [] }));
    vi.stubGlobal('fetch', fetchImpl);
    expect(await deskTrashTreeItems([{ name: 'note.md', sourceSubdir: 'notes', isDirectory: false }])).toBe(true);
    const [url, options] = fetchImpl.mock.calls[0];
    expect(String(url)).toContain('/lingxi/v1/desk/files');
    expect(JSON.parse(String(options?.body))).toMatchObject({ action: 'remove', dir: '/tmp/approved-r07-desk', subdir: 'notes', name: 'note.md' });
    expect(trashItem).not.toHaveBeenCalled();
  });
});
