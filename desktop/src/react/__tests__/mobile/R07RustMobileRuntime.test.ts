// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';
import { createLocalServerConnection } from '../../services/server-connection';
import { chatFetch } from '../../services/chat-client';
import { initializeMobileRuntime, invalidateMobileRuntime, loadMobileSessions, projectMobileRustSessions } from '../../mobile/mobile-init';
import { installMobilePlatform } from '../../mobile/mobile-platform';

const mocks = vi.hoisted(() => ({ fetch: vi.fn(), history: vi.fn(), desk: vi.fn(), connect: vi.fn(), getWebSocket: vi.fn(() => null) }));
vi.mock('../../hooks/use-hana-fetch', () => ({ lingxiFetch: mocks.fetch, lingxiUrl: (path: string) => path }));
vi.mock('../../stores/desk-actions', () => ({ activateWorkspaceDesk: mocks.desk }));
vi.mock('../../stores/agent-actions', () => ({ applyAgentIdentity: vi.fn(), loadAvatars: vi.fn() }));
vi.mock('../../stores/session-actions', () => ({ loadMessages: mocks.history, pendingNewSessionIdentityPatch: () => ({ currentSessionId: null, pendingNewSession: true }) }));
vi.mock('../../services/websocket', () => ({ connectWebSocket: mocks.connect, getWebSocket: mocks.getWebSocket }));
vi.mock('../../services/app-event-actions', () => ({ configureAppEventActions: vi.fn() }));
vi.mock('../../services/ws-message-handler', () => ({ configureWsMessageHandler: vi.fn() }));

const row = { sessionId: 'sess_one', agentId: 'alpha', ownerUserId: 'owner', title: '真实会话', runCount: 1, lastRuns: [{ runId: 'run_one', principalId: 'owner', startedAtUnixMs: 1000 }] };
const agent = { id: 'alpha', name: 'Alpha', yuan: 'lingxi', isPrimary: true };
const principal = { userId: 'owner', studioId: 'studio', scopes: ['chat', 'resources.read', 'files.read', 'files.write'] };
const response = (body: unknown) => ({ json: async () => body }) as Response;
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(done => { resolve = done; }); return { resolve, promise }; }
function connection(token = 'synthetic') {
  const value = createLocalServerConnection({ serverPort: '3001', serverToken: token, serverNodeKind: 'lingxi-service' });
  if (!value) throw new Error('createLocalServerConnection returned null');
  useStore.setState({ activeServerConnection: value, activeServerConnectionId: value.connectionId, serverConnections: { [value.connectionId]: value } });
}

beforeEach(() => {
  vi.clearAllMocks(); invalidateMobileRuntime(); connection();
  useStore.setState({ currentSessionId: null, currentSessionPath: null, currentAgentId: 'alpha', selectedAgentId: null, pendingNewSession: false, sessions: [], agents: [agent], chatSessions: {}, sessionLocatorsById: {}, models: [], currentModel: null });
  mocks.desk.mockResolvedValue(undefined); mocks.history.mockResolvedValue(undefined);
});

describe('Rust 手机使用正式聊天上下文', () => {
  it('按明确助手和会话读取聊天模型，不访问管理模型目录', async () => {
    useStore.setState({ currentSessionId: 'sess_one', currentSessionPath: 'native:sess_one' });
    mocks.fetch.mockResolvedValue(response({ models: [] }));
    await (await chatFetch('/api/models')).json();
    expect(mocks.fetch).toHaveBeenCalledWith('/lingxi/v1/chat/models?agentId=alpha&sessionId=sess_one', {});
  });
  it('新会话按选中的助手读取模型，不借旧会话', async () => {
    useStore.setState({ currentSessionId: 'old', pendingNewSession: true, selectedAgentId: 'beta' });
    mocks.fetch.mockResolvedValue(response({ models: [] }));
    await chatFetch('/api/models');
    expect(mocks.fetch).toHaveBeenCalledWith('/lingxi/v1/chat/models?agentId=beta', {});
  });
  it.each(['agent', 'connection', 'selection'])('拒绝%s切换后的迟到模型正文', async kind => {
    const body = deferred<unknown>(); mocks.fetch.mockResolvedValue({ json: () => body.promise });
    const res = await chatFetch('/api/models');
    const reading = res.json();
    if (kind === 'agent') useStore.setState({ currentAgentId: 'beta' });
    if (kind === 'connection') connection('rotated');
    if (kind === 'selection') useStore.setState({ currentSessionId: 'other' });
    body.resolve({ models: [] });
    await expect(reading).rejects.toThrow('已丢弃');
  });
  it('会话摘要只使用真实身份，缺修订点保持未知', () => {
    expect(projectMobileRustSessions({ sessions: [row] }, [agent])[0]).toMatchObject({ path: 'native:sess_one', sessionId: 'sess_one', agentName: 'Alpha', revision: null, modified: '1970-01-01T00:00:01.000Z' });
    expect(() => projectMobileRustSessions({ sessions: [{ ...row, sessionId: null }] }, [agent])).toThrow('incomplete');
    expect(() => projectMobileRustSessions([row], [agent])).toThrow('incomplete');
  });
  it('手机列表用chat可读入口，前台刷新读历史而不重投prompt', async () => {
    useStore.setState({ currentSessionId: 'sess_one', currentSessionPath: 'native:sess_one', chatSessions: { sess_one: {} } as never });
    mocks.fetch.mockResolvedValue(response({ sessions: [row] }));
    await loadMobileSessions();
    expect(mocks.fetch).toHaveBeenCalledWith('/lingxi/v1/sessions');
    expect(mocks.history).toHaveBeenCalledWith('native:sess_one');
    expect(mocks.connect).not.toHaveBeenCalled();
    expect(mocks.fetch.mock.calls.every(([, options]) => !options?.body)).toBe(true);
  });
  it('列表迟到不覆盖切换后的会话', async () => {
    const body = deferred<unknown>(); mocks.fetch.mockResolvedValue({ json: () => body.promise });
    const loading = loadMobileSessions();
    useStore.setState({ currentSessionId: 'other', currentSessionPath: 'native:other', sessions: [{ path: 'native:other' }] as never });
    body.resolve({ sessions: [row] });
    await expect(loading).rejects.toThrow('已忽略旧结果');
    expect(useStore.getState().sessions[0].path).toBe('native:other');
  });
  it('初始化读取正式bootstrap与四scope会话入口，不读取settings权限面', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ serverNodeKind: 'lingxi-service', serverId: 'server', studioId: 'studio', userId: 'owner', connectionKind: 'local', trustState: 'local' }), { headers: { 'Content-Type': 'application/json' } })));
    const bootstrap = { agents: [agent], currentAgentId: 'alpha', permissionMode: 'ask', models: [], activeModel: null, modelState: 'unconfigured', modelGeneration: '9007199254740993' };
    mocks.fetch.mockImplementation(async (path: string) => {
      if (path === '/lingxi/v1/mobile/bootstrap') return response(bootstrap);
      if (path.startsWith('/lingxi/v1/chat/models')) return response({ models: [], activeModel: null });
      if (path === '/lingxi/v1/sessions') return response({ sessions: [row] });
      throw new Error(`未预期请求 ${path}`);
    });
    try {
      await initializeMobileRuntime(principal, 'rust');
      expect(mocks.connect.mock.calls.length).toBe(1);
      expect(useStore.getState().currentAgentId).toBe('alpha');
      expect(useStore.getState().pendingNewSessionPermissionMode).toBe('ask');
      // 模型面真实状态与代次入只读镜像；空目录保持 unconfigured，不伪装 ready。
      expect(useStore.getState().modelState).toBe('unconfigured');
      expect(useStore.getState().modelGeneration).toBe('9007199254740993');
      expect(mocks.fetch.mock.calls.some(([path]) => path.includes('/api/') || path.includes('studio-list') || path.includes('permission-default'))).toBe(false);
    } finally { vi.unstubAllGlobals(); }
  });
  it('初始化按生成协议投影助手与权限默认值，未知档位不落库', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ serverNodeKind: 'lingxi-service', serverId: 'server', studioId: 'studio', userId: 'owner', connectionKind: 'local', trustState: 'local' }), { headers: { 'Content-Type': 'application/json' } })));
    const bootstrap = {
      agents: [{ ...agent, hasAvatar: true, homeFolder: '/home/alpha', effectiveHomeFolder: '/home/alpha', memoryMasterEnabled: false }],
      currentAgentId: 'alpha',
      permissionMode: 'not-a-mode',
      models: [], activeModel: null, modelState: 'ready', modelGeneration: '7',
    };
    mocks.fetch.mockImplementation(async (path: string) => {
      if (path === '/lingxi/v1/mobile/bootstrap') return response(bootstrap);
      if (path.startsWith('/lingxi/v1/chat/models')) return response({ models: [], activeModel: null });
      if (path === '/lingxi/v1/sessions') return response({ sessions: [] });
      throw new Error(`未预期请求 ${path}`);
    });
    try {
      await initializeMobileRuntime(principal, 'rust');
      const stored = useStore.getState().agents[0];
      expect(stored).toMatchObject({ id: 'alpha', hasAvatar: true, homeFolder: '/home/alpha', effectiveHomeFolder: '/home/alpha', memoryMasterEnabled: false });
      expect(useStore.getState().pendingNewSessionPermissionMode).not.toBe('not-a-mode');
      expect(useStore.getState().modelState).toBe('ready');
    } finally { vi.unstubAllGlobals(); }
  });
  it('初始化作废时真实关闭仍打开的连接，旧主体不得继续收事件', () => {
    const close = vi.fn();
    mocks.getWebSocket.mockReturnValue({ readyState: 1, close } as never);
    invalidateMobileRuntime();
    expect(close.mock.calls.length).toBe(1);
  });
  it('正式资源URL可打开，本机路径和file地址不能落回浏览器', () => {
    installMobilePlatform();
    const resource = '/lingxi/v1/resources/resource_owned/content?ticket=synthetic';
    expect(window.platform.getFileUrl?.(resource)).toBe(resource);
    expect(window.platform.getFileUrl?.('/Users/private/file.txt')).toBe('');
    expect(window.platform.getFileUrl?.('file:///Users/private/file.txt')).toBe('');
    expect(window.platform.getFileUrl?.('/lingxi/v1/settings/export')).toBe('');
  });
});
