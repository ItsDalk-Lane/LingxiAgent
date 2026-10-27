// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { useSettingsStore } from '../../settings/store';
import { captureSettingsConnection } from '../../settings/connection-owner';
import { createLocalServerConnection } from '../../services/server-connection';
import { lingxiFetch } from '../../settings/api';
import {
  addMcpConnectorsBulk, loadMcpState, runMcpConnectorAction, setAgentMcpTools,
  setMcpDeferSettings, setMcpEnabled, startMcpOAuth, pollMcpOAuth, cancelMcpOAuth, logoutMcpOAuth,
} from '../../settings/tabs/mcp/mcp-api';

// 本文件验证页面传输合同和旧响应隔离；真正服务/磁盘/SDK效果另由Rust实链验证。
const initial = useSettingsStore.getState();
const fetchMock = vi.fn();
const response = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } });
function connect(port = 3210) {
  const local = createLocalServerConnection({ serverPort: port, serverToken: `fixture-${port}`, serverNodeKind: 'lingxi-service' })!;
  useSettingsStore.setState({ activeServerConnectionId: 'local', serverConnections: { local }, activeServerConnection: local });
  useSettingsStore.setState({ settingsIdentityOwner: captureSettingsConnection(), rustSettingsUnavailable: false });
}
const state = () => ({ config: { enabled: true, deferEnabled: true, deferThreshold: 4, builtinDeferEnabled: false,
  connectors: [{ id: 'local', name: 'Local', transport: 'stdio', command: 'node', args: ['fixture.cjs'], env: { KEY: '********' }, status: 'running', tools: [{ name: 'echo' }] }] },
  agentId: 'a', agentConfig: { connectors: { local: { enabled: false, tools: { echo: true } } } }, settingsResults: [] });
const applied = (action: string) => ({ settingsUpdate: { action, status: 'applied', saved: true, runtimeApplied: true } });

beforeEach(() => { fetchMock.mockReset(); vi.stubGlobal('fetch', fetchMock); connect(); });
afterEach(() => { useSettingsStore.setState(initial, true); vi.unstubAllGlobals(); });

it('Rust状态按正式嵌套配置和明确助手权限读取，坏结构不伪装为空', async () => {
  fetchMock.mockResolvedValueOnce(response(state()));
  const actual = await loadMcpState('a');
  expect(actual.connectors[0].command).toBe('node');
  expect(actual.agentConfig.connectors?.local).toEqual({ enabled: false, tools: { echo: true } });
  expect(String(fetchMock.mock.calls[0][0])).toContain('/lingxi/v1/mcp/state?agentId=a');
  fetchMock.mockResolvedValueOnce(response({ enabled: true, connectors: [] }));
  await expect(loadMcpState('a')).rejects.toThrow('公开配置');
  fetchMock.mockResolvedValueOnce(response({ ...state(), agentId: 'b' }));
  await expect(loadMcpState('a')).rejects.toThrow('目标助手');
});

it('空助手读取真实全局状态而不扩大连接凭据投影', async () => {
  const global = { ...state(), agentId: null, agentConfig: { connectors: {} },
    config: { ...state().config, connectors: [{ id: 'local', name: 'Local', transport: 'stdio', status: 'stopped', tools: [] }] } };
  fetchMock.mockResolvedValueOnce(response(global));
  const actual = await loadMcpState(null);
  expect(new URL(String(fetchMock.mock.calls[0][0])).pathname).toBe('/lingxi/v1/mcp/state');
  expect(new URL(String(fetchMock.mock.calls[0][0])).search).toBe('');
  expect(actual.enabled).toBe(true);
  expect(actual.connectors[0].command).toBeUndefined();
  expect(actual.connectors[0].env).toBeUndefined();
  expect(actual.agentConfig.connectors).toEqual({});
});

it('Rust页面只向同一管理者提交正式动作和明确工具批次，不调用旧URL', async () => {
  fetchMock.mockImplementation(async (_url, init: RequestInit) => response(applied(JSON.parse(String(init.body)).action)));
  await setMcpEnabled(false);
  await setMcpDeferSettings({ builtinDeferEnabled: true });
  await runMcpConnectorAction('local', 'refresh-tools');
  await setAgentMcpTools('a', 'local', { echo: false, second: true });
  expect(fetchMock.mock.calls.map(([url]) => new URL(String(url)).pathname)).toEqual(Array(4).fill('/lingxi/v1/mcp/settings-action'));
  expect(JSON.parse(String(fetchMock.mock.calls[3][1].body))).toEqual({ action: 'mcp.agent.tool.enable', agentId: 'a', payload: { connectorId: 'local', tools: { echo: false, second: true } } });
});

it('批次输入拒绝保留逐行结果，保存后启动失败仍保留保存事实', async () => {
  const rejected = { settingsUpdate: { action: 'mcp.connector.add_bulk', status: 'failed', saved: false, runtimeApplied: false, error: { code: 'invalid_mcp_settings', message: '整批未保存' } },
    results: [{ ok: false, saved: false, runtimeApplied: false, error: 'bad command' }] };
  fetchMock.mockResolvedValueOnce(response(rejected, 400));
  await expect(addMcpConnectorsBulk([{ transport: 'stdio', command: '' }])).rejects.toMatchObject({ results: rejected.results });
  fetchMock.mockResolvedValueOnce(response({ settingsUpdate: { action: 'mcp.connector.start', status: 'failed', saved: true, runtimeApplied: false, error: { code: 'mcp_connect_failed', message: '启动失败' } } }, 400));
  await expect(runMcpConnectorAction('local', 'start')).rejects.toThrow('设置已保存');
});

it('旧连接迟到状态不能覆盖新连接，旧401不能撤销新身份，403保留当前身份', async () => {
  let finish!: (value: Response) => void;
  fetchMock.mockImplementationOnce(() => new Promise<Response>(resolve => { finish = resolve; }));
  const pending = loadMcpState('a'); connect(3211); finish(response(state()));
  await expect(pending).rejects.toThrow('连接已改变');
  fetchMock.mockImplementationOnce(() => new Promise<Response>(resolve => { finish = resolve; }));
  const old401 = lingxiFetch('/lingxi/v1/mcp/state'); connect(3212); finish(response({ error: 'invalid_credential' }, 401));
  await expect(old401).rejects.toThrow();
  expect(useSettingsStore.getState().settingsIdentityOwner).not.toBeNull();
  fetchMock.mockResolvedValueOnce(response({ error: 'forbidden' }, 403));
  await expect(lingxiFetch('/lingxi/v1/mcp/state')).rejects.toThrow();
  expect(useSettingsStore.getState().settingsIdentityOwner).not.toBeNull();
  fetchMock.mockResolvedValueOnce(response({ error: 'invalid_credential' }, 401));
  await expect(lingxiFetch('/lingxi/v1/mcp/state')).rejects.toThrow();
  expect(useSettingsStore.getState().settingsIdentityOwner).toBeNull();
  expect(useSettingsStore.getState().rustSettingsUnavailable).toBe(true);
});

it('Rust连接器独立登录四动作使用正式回执，不借模型OAuth或旧Node入口', async () => {
  fetchMock.mockResolvedValueOnce(response({ sessionId: 'opaque-state', url: 'http://127.0.0.1:3215/authorize' }));
  await expect(startMcpOAuth('local')).resolves.toEqual({ sessionId: 'opaque-state', url: 'http://127.0.0.1:3215/authorize' });
  fetchMock.mockResolvedValueOnce(response({ status: 'done', result: { connectorId: 'local' } }));
  await expect(pollMcpOAuth('opaque-state')).resolves.toMatchObject({ status: 'done' });
  fetchMock.mockResolvedValueOnce(response({ cancelled: 1 })); await cancelMcpOAuth('local');
  fetchMock.mockResolvedValueOnce(response({ connector: { id: 'local' }, state: state() })); await logoutMcpOAuth('local');
  expect(fetchMock.mock.calls.map(([url]) => new URL(String(url)).pathname)).toEqual([
    '/lingxi/v1/mcp/connectors/local/oauth/start', '/lingxi/v1/mcp/oauth/poll/opaque-state',
    '/lingxi/v1/mcp/connectors/local/oauth/cancel', '/lingxi/v1/mcp/connectors/local/oauth/logout',
  ]);
});

it('登录回执拒绝无效外链和跨连接迟到结果，权限错误不伪装成功', async () => {
  fetchMock.mockResolvedValueOnce(response({ sessionId: 'opaque', url: 'javascript:alert(1)' }));
  await expect(startMcpOAuth('local')).rejects.toThrow('登录地址');
  let finish!: (value: Response) => void;
  fetchMock.mockImplementationOnce(() => new Promise<Response>(resolve => { finish = resolve; }));
  const pending = startMcpOAuth('local'); connect(3211); finish(response({ sessionId: 'old', url: 'http://127.0.0.1/authorize' }));
  await expect(pending).rejects.toThrow('连接已改变');
  fetchMock.mockResolvedValueOnce(response({ error: 'forbidden' }, 403));
  await expect(cancelMcpOAuth('local')).rejects.toThrow();
});

it('已注册的设置和媒体请求精确切换到Rust，保留查询参数', async () => {
  fetchMock.mockImplementation(async () => response({ ok: true }));
  const paths = ['/api/preferences/quick-chat', '/api/preferences/notifications', '/api/preferences/keybindings',
    '/api/preferences/sidebar-ui', '/api/checkpoints', '/api/sessions/workspace-rollback',
    '/api/media/image/providers?owned=1', '/api/media/video/config', '/api/media/speech/providers',
    '/api/speech-recognition/providers/system-speech/models'];
  for (const path of paths) await lingxiFetch(path);
  expect(fetchMock.mock.calls.map(([url]) => { const parsed = new URL(String(url)); return parsed.pathname + parsed.search; }))
    .toEqual(paths.map(path => `/lingxi/v1${path.slice('/api'.length)}`));
});
