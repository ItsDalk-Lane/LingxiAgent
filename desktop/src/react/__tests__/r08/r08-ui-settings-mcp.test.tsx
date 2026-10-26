// @vitest-environment jsdom

/**
 * R08 UI 叶 F-D03-UI-UI-SETTINGS-MCP-B4BB24 客户端份额逐断言证据。
 *
 * 真实渲染生产 `McpTab` + 真实 settings store/actions/mcp-api；只有网络传输层
 * （全局 fetch）被替换为合成 Rust 服务回执，回执形状对齐 lingxi-service 的
 * `mcp/settings-action`、`mcp/state`、`mcp/oauth/*` 真实投影（见
 * R07McpRustAPI.test.ts 与 mcp-api.ts 的严格校验）。断言页面实际投影、请求
 * 副作用（action/助手/载荷）、空态与错误态，不做快照。
 */

import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';

const hostMocks = vi.hoisted(() => {
  const api = {
    openExternal: vi.fn(async (_url: string) => undefined),
  };
  Object.defineProperty(window, 'platform', { configurable: true, value: api });
  return api;
});

import { McpTab } from '../../settings/tabs/McpTab';
import { useSettingsStore } from '../../settings/store';
import { createLocalServerConnection } from '../../services/server-connection';
import type { McpConnector } from '../../settings/tabs/mcp/types';

interface WireCall {
  path: string;
  search: string;
  method: string;
  body: unknown;
}

type StateProjection = (agentId: string | null) => unknown;

const DEFAULT_CONFIG = {
  enabled: false,
  deferEnabled: true,
  deferThreshold: 12,
  builtinDeferEnabled: false,
  connectors: [] as McpConnector[],
};

function stateFor(agentId: string | null, config = DEFAULT_CONFIG, agentConfig: unknown = { connectors: {} }) {
  return { agentId, config, agentConfig, settingsResults: [] };
}

const applied = (action: string, extra: Record<string, unknown> = {}) => ({
  settingsUpdate: { action, status: 'applied', saved: true, runtimeApplied: true },
  ...extra,
});

function connector(overrides: Partial<McpConnector> & { id: string }): McpConnector {
  return {
    name: overrides.id,
    transport: 'streamable-http',
    url: 'http://127.0.0.1:9/mcp',
    status: 'running',
    tools: [],
    ...overrides,
  } as McpConnector;
}

const service = {
  calls: [] as WireCall[],
  state: ((agentId: string | null) => stateFor(agentId)) as StateProjection,
  action: ((body: { action: string }) => applied(body.action)) as (body: { action: string }) => unknown,
};

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
}

const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
  const url = new URL(String(input));
  const call: WireCall = {
    path: url.pathname,
    search: url.search,
    method: (init?.method || 'GET').toUpperCase(),
    body: init?.body ? JSON.parse(String(init.body)) : undefined,
  };
  service.calls.push(call);
  switch (call.path) {
    case '/lingxi/v1/mcp/state':
      return jsonResponse(service.state(url.searchParams.get('agentId')));
    case '/lingxi/v1/mcp/settings-action':
      return jsonResponse(service.action(call.body as { action: string }));
    case '/lingxi/v1/mcp/connectors/oauth-flow/oauth/start':
      return jsonResponse({ sessionId: 'oauth-flow-1', url: 'http://127.0.0.1:9/authorize' });
    case '/lingxi/v1/mcp/oauth/poll/oauth-flow-1':
      return jsonResponse({ status: 'done' });
    case '/lingxi/v1/mcp/connectors/oauth-flow/oauth/cancel':
      return jsonResponse({ cancelled: 1 });
    case '/lingxi/v1/mcp/connectors/oauth-flow/oauth/logout':
      return jsonResponse({ connector: { id: 'oauth-flow' }, state: stateFor('lingxi') });
    default:
      throw new Error(`unexpected fetch ${call.method} ${call.path}${call.search}`);
  }
});

const initialStore = useSettingsStore.getState();
let portSeed = 43410;

function connect(port: number) {
  const connection = createLocalServerConnection({
    serverPort: port,
    serverToken: `owned-${port}`,
    serverNodeKind: 'lingxi-service',
  })!;
  useSettingsStore.setState({
    activeServerConnectionId: 'local',
    serverConnections: { local: connection },
    activeServerConnection: connection,
  });
}

function withAgent(agentId: string | null) {
  useSettingsStore.setState({
    currentAgentId: agentId,
    agents: agentId
      ? [{ id: agentId, name: 'Hanako', yuan: 'lingxi', isPrimary: true } as never]
      : [],
  });
}

beforeEach(() => {
  service.calls = [];
  service.state = (agentId: string | null) => stateFor(agentId);
  service.action = (body: { action: string }) => applied(body.action);
  hostMocks.openExternal.mockReset();
  hostMocks.openExternal.mockResolvedValue(undefined);
  window.t = ((key: string) => key) as typeof window.t;
  portSeed += 1;
  connect(portSeed);
  withAgent('lingxi');
  vi.stubGlobal('fetch', fetchMock);
  fetchMock.mockClear();
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  useSettingsStore.setState(initialStore, true);
  useSettingsStore.setState({ currentAgentId: null, agents: [], toastMessage: '', toastType: '', toastVisible: false });
});

describe('R08 MCP 设置页：连接器状态与工具权限（断言 2）', () => {
  it('读取真实连接器列表并逐条显示状态、工具数与助手权限；无连接器时显示添加入口', async () => {
    const running = connector({
      id: 'gh',
      name: 'GitHub MCP',
      status: 'running',
      tools: [{ name: 'create_issue' }, { name: 'list_issues' }],
      toolPermissions: { create_issue: 'allow' },
    });
    const stopped = connector({ id: 'local-tool', name: 'Local Tool', transport: 'stdio', status: 'stopped', tools: [] });
    service.state = (agentId) => stateFor(agentId, {
      ...DEFAULT_CONFIG,
      enabled: true,
      connectors: [running, stopped],
    }, { connectors: { gh: { enabled: true, tools: { list_issues: true } } } });

    const first = render(<McpTab />);
    await screen.findByText('GitHub MCP');

    expect(screen.getByText('Local Tool')).toBeTruthy();
    expect(screen.getByText('settings.mcp.statusRunning')).toBeTruthy();
    expect(screen.getByText('settings.mcp.statusStopped')).toBeTruthy();
    expect(screen.getByText(/2 .*settings\.mcp\.toolsCount/)).toBeTruthy();
    expect(screen.getByText(/1 .*settings\.mcp\.enabledAgentsCount/)).toBeTruthy();
    // 有 agentId 时状态读取携带该助手，不跨助手偷读。
    expect(service.calls.filter(call => call.path === '/lingxi/v1/mcp/state').map(call => call.search))
      .toContain('?agentId=lingxi');
    first.unmount();

    service.state = (agentId) => stateFor(agentId, { ...DEFAULT_CONFIG, enabled: false, connectors: [] });
    render(<McpTab />);
    await screen.findByText('settings.mcp.noConnectors');
    expect((screen.getByText('settings.mcp.addConnector') as HTMLButtonElement).disabled).toBe(false);
    expect((screen.getByText('settings.mcp.importJson') as HTMLButtonElement).disabled).toBe(false);
  });

  it('连接器读取失败显示显式错误并保留旧状态，不把失败冒充成空列表', async () => {
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      service.state = (agentId) => stateFor(agentId, {
        ...DEFAULT_CONFIG,
        enabled: true,
        connectors: [connector({ id: 'gh', name: 'GitHub MCP', status: 'connecting' })],
      });
      render(<McpTab />);
      await screen.findByText('GitHub MCP');

      let stateReads = 0;
      service.state = () => {
        stateReads += 1;
        if (stateReads >= 1) throw new Error('真实状态读取失败');
        return stateFor('lingxi');
      };
      // 后台连接器停在 connecting 时页面会真实重读；这里让重读失败。
      await waitFor(() => expect(service.calls.filter(call => call.path === '/lingxi/v1/mcp/state').length).toBeGreaterThanOrEqual(2), { timeout: 4000 });
      const alert = await screen.findByRole('alert');
      expect(alert.textContent).toContain('真实状态读取失败');
      // 旧连接器仍显示，未被空数组覆盖。
      expect(screen.getByText('GitHub MCP')).toBeTruthy();
      expect(screen.queryByText('settings.mcp.noConnectors')).toBeNull();
      expect(useSettingsStore.getState().toastVisible).toBe(false);
    } finally {
      quiet.mockRestore();
    }
  });
});

describe('R08 MCP 设置页：全局启停与延迟（断言 3）', () => {
  it('总开关保存后按助手重读状态并显示真实成功提示', async () => {
    service.action = body => applied(body.action);
    service.state = (agentId) => stateFor(agentId, { ...DEFAULT_CONFIG, enabled: service.calls.some(call => call.path === '/lingxi/v1/mcp/settings-action') });

    render(<McpTab />);
    await screen.findByRole('switch', { name: 'common.off' });
    fireEvent.click(screen.getByText('settings.mcp.masterName'));

    await waitFor(() => expect(service.calls.some(call => call.path === '/lingxi/v1/mcp/settings-action')).toBe(true));
    const action = service.calls.find(call => call.path === '/lingxi/v1/mcp/settings-action')!;
    expect(action.body).toEqual({ action: 'mcp.global.enabled', agentId: null, payload: { enabled: true } });
    // 保存后经统一 run 重读连接器状态与工具权限。
    await waitFor(() => expect(service.calls.filter(call => call.path === '/lingxi/v1/mcp/state').length).toBeGreaterThanOrEqual(2));
    expect(service.calls.filter(call => call.path === '/lingxi/v1/mcp/state').at(-1)!.search).toBe('?agentId=lingxi');
    await waitFor(() => expect(useSettingsStore.getState().toastType).toBe('success'));
    expect(screen.getByRole('switch', { name: 'common.on' })).toBeTruthy();
  });

  it('总开关保存失败恢复旧配置并显示错误，不写成功状态', async () => {
    service.state = (agentId) => stateFor(agentId, { ...DEFAULT_CONFIG, enabled: true, connectors: [] });
    service.action = body => ({
      settingsUpdate: {
        action: body.action,
        status: 'failed',
        saved: false,
        runtimeApplied: false,
        error: { code: 'mcp_write_failed', message: '真实写入被拒绝' },
      },
    });

    render(<McpTab />);
    await screen.findByRole('switch', { name: 'common.on' });
    fireEvent.click(screen.getByText('settings.mcp.masterName'));

    await waitFor(() => expect(useSettingsStore.getState().toastType).toBe('error'));
    expect(useSettingsStore.getState().toastMessage).toContain('真实写入被拒绝');
    // 失败后重读真实状态，界面仍为服务端旧配置（开）。
    await waitFor(() => expect(service.calls.filter(call => call.path === '/lingxi/v1/mcp/state').length).toBeGreaterThanOrEqual(2));
    expect(screen.getByRole('switch', { name: 'common.on' })).toBeTruthy();
  });

  it('延迟加载开关与阈值保存到全局设置，并在保存后重读状态', async () => {
    service.state = (agentId) => stateFor(agentId, { ...DEFAULT_CONFIG, enabled: true, connectors: [connector({ id: 'gh', name: 'GitHub MCP' })] });

    render(<McpTab />);
    await screen.findByText('settings.mcp.deferTitle');
    fireEvent.click(screen.getByText('settings.mcp.deferTitle'));
    fireEvent.click(screen.getByRole('switch', { name: 'settings.mcp.deferEnabled' }));

    await waitFor(() => expect(service.calls.some(call => call.path === '/lingxi/v1/mcp/settings-action')).toBe(true));
    expect(service.calls.find(call => call.path === '/lingxi/v1/mcp/settings-action')!.body)
      .toEqual({ action: 'mcp.global.defer', agentId: null, payload: { deferEnabled: false } });
    await waitFor(() => expect(service.calls.filter(call => call.path === '/lingxi/v1/mcp/state').length).toBeGreaterThanOrEqual(2));

    const threshold = screen.getByRole('spinbutton') as HTMLInputElement;
    fireEvent.change(threshold, { target: { value: '24' } });
    await waitFor(() => expect(service.calls.filter(call => call.path === '/lingxi/v1/mcp/settings-action').length).toBeGreaterThanOrEqual(2));
    expect(service.calls.filter(call => call.path === '/lingxi/v1/mcp/settings-action').at(-1)!.body)
      .toEqual({ action: 'mcp.global.defer', agentId: null, payload: { deferThreshold: 24 } });
  });

  it('无 agentId 时读取全局状态（不带 agentId 查询），不伪造助手权限', async () => {
    withAgent(null);
    service.state = (agentId) => stateFor(agentId, { ...DEFAULT_CONFIG, enabled: false, connectors: [] });

    render(<McpTab />);
    await waitFor(() => expect(service.calls.some(call => call.path === '/lingxi/v1/mcp/state')).toBe(true));
    const stateCalls = service.calls.filter(call => call.path === '/lingxi/v1/mcp/state');
    expect(stateCalls.every(call => call.search === '')).toBe(true);
    // 全局总开关仍可用。
    await screen.findByRole('switch', { name: 'common.off' });
    fireEvent.click(screen.getByText('settings.mcp.masterName'));
    await waitFor(() => expect(service.calls.some(call => call.path === '/lingxi/v1/mcp/settings-action')).toBe(true));
    expect((service.calls.find(call => call.path === '/lingxi/v1/mcp/settings-action')!.body as { agentId: unknown }).agentId).toBeNull();
  });
});

describe('R08 MCP 设置页：添加与批量导入（断言 4）', () => {
  it('添加连接器提交真实动作与启动意图，成功后面板出现新连接器', async () => {
    let added = false;
    service.state = (agentId) => stateFor(agentId, {
      ...DEFAULT_CONFIG,
      enabled: false,
      connectors: added ? [connector({ id: 'added-1', name: 'Added Remote', status: 'stopped' })] : [],
    });
    service.action = body => {
      if (body.action === 'mcp.connector.add') added = true;
      return applied(body.action);
    };

    render(<McpTab />);
    await screen.findByText('settings.mcp.noConnectors');
    fireEvent.click(screen.getByText('settings.mcp.addConnector'));
    const form = await screen.findByPlaceholderText('https://mcp.example.com/mcp');
    fireEvent.change(screen.getByPlaceholderText('GitHub'), { target: { value: 'Added Remote' } });
    fireEvent.change(form, { target: { value: 'https://mcp.example.com/mcp' } });
    const submitButtons = screen.getAllByRole('button', { name: 'settings.mcp.addConnector' });
    const submit = submitButtons.at(-1) as HTMLButtonElement;
    expect(submitButtons.length).toBeGreaterThanOrEqual(2);
    expect(submit.disabled).toBe(false);
    fireEvent.click(submit);

    await waitFor(() => expect(service.calls.some(call => call.path === '/lingxi/v1/mcp/settings-action')).toBe(true));
    const addBody = service.calls.find(call => call.path === '/lingxi/v1/mcp/settings-action')!.body as {
      action: string; agentId: string; payload: { connector: Record<string, unknown>; startIfEnabled: boolean };
    };
    expect(addBody.action).toBe('mcp.connector.add');
    expect(addBody.agentId).toBeNull();
    expect(addBody.payload.connector).toMatchObject({ name: 'Added Remote', transport: 'remote', url: 'https://mcp.example.com/mcp' });
    expect(addBody.payload.startIfEnabled).toBe(true);
    await screen.findByText('Added Remote');
  });

  it('空表单不可提交；批量导入逐项反馈部分成功，不冒充全成功', async () => {
    service.state = () => stateFor('lingxi', { ...DEFAULT_CONFIG, enabled: false, connectors: [] });
    let imported = false;
    service.state = (agentId) => stateFor(agentId, {
      ...DEFAULT_CONFIG,
      enabled: false,
      connectors: imported ? [connector({ id: 'ok-row', name: 'ok-row', status: 'stopped' })] : [],
    });
    service.action = body => {
      if (body.action === 'mcp.connector.add_bulk') {
        imported = true;
        return {
          settingsUpdate: { action: body.action, status: 'failed', saved: false, runtimeApplied: false, error: { code: 'invalid_mcp_settings', message: '整批未保存' } },
          results: [
            { ok: true, id: 'good', saved: true, runtimeApplied: true },
            { ok: false, saved: false, runtimeApplied: false, error: 'bad command' },
          ],
        };
      }
      return applied(body.action);
    };

    render(<McpTab />);
    await screen.findByText('settings.mcp.noConnectors');
    fireEvent.click(screen.getByText('settings.mcp.addConnector'));
    const addForm = await screen.findByPlaceholderText('https://mcp.example.com/mcp');
    const formSubmit = screen.getAllByRole('button', { name: 'settings.mcp.addConnector' }).at(-1) as HTMLButtonElement;
    expect(formSubmit.disabled).toBe(true);
    expect(addForm).toBeTruthy();
    fireEvent.click(screen.getAllByRole('button', { name: 'settings.mcp.addConnector' })[0]);

    fireEvent.click(screen.getByText('settings.mcp.importJson'));
    const textarea = await screen.findByPlaceholderText(/mcpServers/);
    fireEvent.change(textarea, {
      target: {
        value: JSON.stringify({
          mcpServers: {
            good: { url: 'https://mcp.example.com/mcp' },
            second: { url: 'https://second.example.com/mcp' },
          },
        }),
      },
    });
    const preview = screen.getAllByRole('button').find(button => button.textContent === 'settings.mcp.importParse') as HTMLButtonElement;
    fireEvent.click(preview);
    await screen.findByTestId('mcp-import-preview');

    const submit = screen.getAllByRole('button').find(button => button.textContent === 'settings.mcp.importConfirm') as HTMLButtonElement;
    expect(submit.disabled).toBe(false);
    fireEvent.click(submit);
    await waitFor(() => expect(service.calls.some(call => call.path === '/lingxi/v1/mcp/settings-action')).toBe(true));
    const bulkBody = service.calls.find(call => call.path === '/lingxi/v1/mcp/settings-action')!.body as {
      action: string; payload: { connectors: Array<Record<string, unknown>> };
    };
    expect(bulkBody.action).toBe('mcp.connector.add_bulk');
    expect(bulkBody.payload.connectors).toHaveLength(2);
    // 服务端整批拒绝但逐行回执：页面显示失败行原因，不弹全成功。
    await screen.findByText(/bad command/);
    expect(useSettingsStore.getState().toastVisible).toBe(false);
  });
});

describe('R08 MCP 设置页：编辑/删除/启停/刷新（断言 5）', () => {
  it('删除需确认，确认后提交真实移除动作；失败时保留连接器并显示错误', async () => {
    let removed = false;
    let failRemove = true;
    service.state = (agentId) => stateFor(agentId, {
      ...DEFAULT_CONFIG,
      enabled: true,
      connectors: removed ? [] : [connector({ id: 'gh', name: 'GitHub MCP' })],
    });
    service.action = body => {
      if (body.action === 'mcp.connector.remove') {
        if (failRemove) {
          failRemove = false;
          return { settingsUpdate: { action: body.action, status: 'failed', saved: false, runtimeApplied: false, error: { code: 'busy', message: '连接器正在运行' } } };
        }
        removed = true;
      }
      return applied(body.action);
    };

    render(<McpTab />);
    await screen.findByText('GitHub MCP');
    fireEvent.click(screen.getByRole('button', { name: 'common.remove' }));
    const dialog = await screen.findByRole('dialog', { name: 'settings.mcp.removeTitle' });
    fireEvent.click(within(dialog).getByRole('button', { name: 'common.remove' }));

    await waitFor(() => expect(useSettingsStore.getState().toastType).toBe('error'));
    expect(useSettingsStore.getState().toastMessage).toContain('连接器正在运行');
    expect(screen.getByText('GitHub MCP')).toBeTruthy();
    expect(removed).toBe(false);
    // 失败后确认框仍留在页面上，可直接重试而不是要求重新打开。
    expect(screen.getByRole('dialog', { name: 'settings.mcp.removeTitle' })).toBeTruthy();
    fireEvent.click(within(screen.getByRole('dialog', { name: 'settings.mcp.removeTitle' })).getByRole('button', { name: 'common.remove' }));
    await waitFor(() => expect(removed).toBe(true));
    await waitFor(() => expect(screen.queryByText('GitHub MCP')).toBeNull());
    expect((service.calls.filter(call => call.path === '/lingxi/v1/mcp/settings-action').at(-1)!.body as {
      action: string; payload: { connectorId: string };
    })).toMatchObject({ action: 'mcp.connector.remove', payload: { connectorId: 'gh' } });
  });

  it('启动、停止与刷新工具提交真实动作；停用不清凭据，成功后重读连接状态', async () => {
    const oauthConnector = connector({
      id: 'owned',
      name: 'Owned MCP',
      status: 'running',
      authType: 'oauth',
      authStatus: 'connected',
      tools: [{ name: 'echo' }],
    });
    service.state = (agentId) => stateFor(agentId, {
      ...DEFAULT_CONFIG,
      enabled: true,
      connectors: [{ ...oauthConnector, status: stoppedAfterCall() }],
    });
    function stoppedAfterCall() {
      return service.calls.some(call => call.path === '/lingxi/v1/mcp/settings-action') ? 'stopped' : 'running';
    }

    render(<McpTab />);
    await screen.findByText('Owned MCP');
    fireEvent.click(screen.getByTestId('mcp-connector-row-owned'));

    // 真实运行态下刷新工具可用，动作走正式 refresh_tools。
    const refresh = await screen.findByRole('button', { name: 'settings.mcp.refresh' });
    expect((refresh as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(refresh);
    await waitFor(() => expect(service.calls.some(call => call.path === '/lingxi/v1/mcp/settings-action')).toBe(true));
    expect((service.calls.find(call => call.path === '/lingxi/v1/mcp/settings-action')!.body as { action: string; payload: unknown }))
      .toEqual({ action: 'mcp.connector.refresh_tools', agentId: null, payload: { connectorId: 'owned' } });

    // 停止后状态重读为 stopped，刷新按钮按运行态禁用。
    const stop = await screen.findByRole('button', { name: 'settings.mcp.stop' });
    fireEvent.click(stop);
    await waitFor(() => expect(service.calls.filter(call => call.path === '/lingxi/v1/mcp/settings-action').length).toBeGreaterThanOrEqual(2));
    expect((service.calls.filter(call => call.path === '/lingxi/v1/mcp/settings-action').at(-1)!.body as { action: string; payload: unknown }))
      .toEqual({ action: 'mcp.connector.stop', agentId: null, payload: { connectorId: 'owned' } });
    // 停用动作不携带/清除 OAuth 凭据；连接器上的登录态仍是 connected。
    expect(JSON.stringify(service.calls.filter(call => call.path === '/lingxi/v1/mcp/settings-action').at(-1)!.body)).not.toContain('oauth');
    await waitFor(() => expect((screen.getByRole('button', { name: 'settings.mcp.refresh' }) as HTMLButtonElement).disabled).toBe(true));

    // 停止后仍可从同一详情页重新启动（start 动作同样走正式协议）。
    fireEvent.click(screen.getByRole('button', { name: 'settings.mcp.start' }));
    await waitFor(() => expect(service.calls.filter(call => call.path === '/lingxi/v1/mcp/settings-action').length).toBeGreaterThanOrEqual(3));
    expect((service.calls.filter(call => call.path === '/lingxi/v1/mcp/settings-action').at(-1)!.body as { action: string; payload: unknown }))
      .toEqual({ action: 'mcp.connector.start', agentId: null, payload: { connectorId: 'owned' } });
  });
});

describe('R08 MCP 设置页：按助手权限/工具/置顶（断言 6）', () => {
  it('按助手启停连接器并保存工具权限与置顶，动作携带当前助手身份', async () => {
    const toolConnector = connector({
      id: 'gh',
      name: 'GitHub MCP',
      status: 'running',
      permissionMode: 'allowlist',
      tools: [
        { name: 'create_issue', title: 'Create issue' },
        { name: 'destructive_thing', annotations: { destructiveHint: true } },
      ],
    });
    service.state = (agentId) => stateFor(agentId, {
      ...DEFAULT_CONFIG,
      enabled: true,
      connectors: [toolConnector],
    }, { connectors: { gh: { enabled: false, tools: {} } } });

    render(<McpTab />);
    await screen.findByText('GitHub MCP');
    fireEvent.click(screen.getByTestId('mcp-connector-row-gh'));
    await screen.findByTestId('mcp-connector-detail-gh');

    fireEvent.click(screen.getByRole('switch', { name: 'settings.mcp.agentConnectorEnabled' }));
    await waitFor(() => expect(service.calls.some(call => call.path === '/lingxi/v1/mcp/settings-action')).toBe(true));
    const agentToggleBody = service.calls.find(call => call.path === '/lingxi/v1/mcp/settings-action')!.body as {
      action: string; agentId: string | null; payload: unknown;
    };
    expect(agentToggleBody.action).toBe('mcp.agent.connector.enable');
    expect(agentToggleBody.agentId).toBe('lingxi');
    expect(agentToggleBody.payload).toEqual({ connectorId: 'gh', enabled: true });

    fireEvent.click(screen.getByRole('switch', { name: 'create_issue settings.mcp.permissionAllow' }));
    await waitFor(() => expect(service.calls.filter(call => call.path === '/lingxi/v1/mcp/settings-action').length).toBeGreaterThanOrEqual(2));
    expect(service.calls.filter(call => call.path === '/lingxi/v1/mcp/settings-action').at(-1)!.body)
      .toEqual({ action: 'mcp.connector.update', agentId: null, payload: { connectorId: 'gh', patch: { toolPermissions: { create_issue: 'allow' } } } });

    // 服务端声明 destructive 的工具不提供可用的静默放行控件（控件在但禁用）。
    const destructiveAllow = screen.getByRole('switch', { name: 'destructive_thing settings.mcp.permissionAllow' }) as HTMLButtonElement;
    expect(destructiveAllow.disabled).toBe(true);
    fireEvent.click(screen.getByRole('switch', { name: 'create_issue settings.mcp.pinTool' }));
    await waitFor(() => expect(service.calls.filter(call => call.path === '/lingxi/v1/mcp/settings-action').length).toBeGreaterThanOrEqual(3));
    expect(service.calls.filter(call => call.path === '/lingxi/v1/mcp/settings-action').at(-1)!.body)
      .toEqual({ action: 'mcp.connector.update', agentId: null, payload: { connectorId: 'gh', patch: { pinnedTools: { create_issue: true } } } });
  });

  it('无工具显示空目录；无助手时不伪造授权，打开详情被明确拒绝', async () => {
    withAgent(null);
    service.state = (agentId) => stateFor(agentId, {
      ...DEFAULT_CONFIG,
      enabled: true,
      connectors: [connector({ id: 'empty', name: 'Empty Tools', status: 'running', tools: [] })],
    });

    render(<McpTab />);
    await screen.findByText('Empty Tools');
    fireEvent.click(screen.getByTestId('mcp-connector-row-empty'));
    // 无助手时不允许进入详情（详情里的权限写操作必须有明确助手所有者）。
    await waitFor(() => expect(useSettingsStore.getState().toastType).toBe('error'));
    expect(useSettingsStore.getState().toastMessage).toContain('selectAgentForDetails');
    expect(service.calls.some(call => call.path === '/lingxi/v1/mcp/settings-action')).toBe(false);
  });
});

describe('R08 MCP 设置页：OAuth 登录/取消/退出（断言 7）', () => {
  it('登录打开外链并轮询到 done 后刷新连接器并显示保存成功', async () => {
    const oauth = connector({ id: 'oauth-flow', name: 'OAuth MCP', status: 'stopped', authType: 'oauth', authStatus: 'disconnected' });
    service.state = (agentId) => stateFor(agentId, { ...DEFAULT_CONFIG, enabled: true, connectors: [oauth] });

    render(<McpTab />);
    await screen.findByText('OAuth MCP');
    fireEvent.click(screen.getByTestId('mcp-connector-row-oauth-flow'));
    await screen.findByTestId('mcp-connector-detail-oauth-flow');
    vi.useFakeTimers();
    try {
      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: 'settings.mcp.oauthConnect' }));
      });
      expect(hostMocks.openExternal).toHaveBeenCalledWith('http://127.0.0.1:9/authorize');
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000);
        for (let i = 0; i < 8; i += 1) await Promise.resolve();
      });
      expect(service.calls.some(call => call.path === '/lingxi/v1/mcp/oauth/poll/oauth-flow-1')).toBe(true);
      expect(service.calls.some(call => call.path === '/lingxi/v1/mcp/connectors/oauth-flow/oauth/start')).toBe(true);
      expect(useSettingsStore.getState().toastType).toBe('success');
      expect(screen.queryByTestId('mcp-oauth-waiting')).toBeNull();
      const stateReads = service.calls.filter(call => call.path === '/lingxi/v1/mcp/state');
      expect(stateReads.length).toBeGreaterThanOrEqual(2);
      expect(stateReads.at(-1)!.search).toBe('?agentId=lingxi');
    } finally {
      vi.useRealTimers();
    }
  }, 15000);

  it('取消只提交取消动作并关闭等待卡，不因取消本身重读状态', async () => {
    const oauth = connector({ id: 'oauth-flow', name: 'OAuth MCP', status: 'stopped', authType: 'oauth', authStatus: 'disconnected' });
    service.state = (agentId) => stateFor(agentId, { ...DEFAULT_CONFIG, enabled: true, connectors: [oauth] });
    const handled: string[] = [];
    // 轮询回执保持 pending，取消后的在途轮询返回 cancelled 时登录回调安静结束。
    const originalFetch = fetchMock.getMockImplementation()!;
    fetchMock.mockImplementation(async (input, init) => {
      const url = new URL(String(input));
      if (url.pathname === '/lingxi/v1/mcp/oauth/poll/oauth-flow-1') {
        handled.push(url.pathname);
        return jsonResponse({ status: 'cancelled' });
      }
      return originalFetch(input, init);
    });

    render(<McpTab />);
    await screen.findByText('OAuth MCP');
    fireEvent.click(screen.getByTestId('mcp-connector-row-oauth-flow'));
    await screen.findByTestId('mcp-connector-detail-oauth-flow');
    vi.useFakeTimers();
    try {
      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: 'settings.mcp.oauthConnect' }));
        for (let i = 0; i < 6; i += 1) await Promise.resolve();
      });
      expect(screen.getByTestId('mcp-oauth-waiting')).toBeTruthy();
      const readsBeforeCancel = service.calls.filter(call => call.path === '/lingxi/v1/mcp/state').length;
      await act(async () => {
        fireEvent.click(within(screen.getByTestId('mcp-oauth-waiting')).getByRole('button', { name: 'common.cancel' }));
      });
      expect(service.calls.some(call => call.path === '/lingxi/v1/mcp/connectors/oauth-flow/oauth/cancel')).toBe(true);
      expect(screen.queryByTestId('mcp-oauth-waiting')).toBeNull();
      expect(service.calls.filter(call => call.path === '/lingxi/v1/mcp/state').length).toBe(readsBeforeCancel);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000);
        for (let i = 0; i < 8; i += 1) await Promise.resolve();
      });
      // 在途轮询返回 cancelled 后登录回调正常结束，不被取消动作误标成保存成功。
      expect(service.calls.filter(call => call.path === '/lingxi/v1/mcp/state').length).toBe(readsBeforeCancel);
      expect(useSettingsStore.getState().toastType).not.toBe('success');
      // 取消时启动链可能尚未进入等待循环：轮询最多 1 次（cancelled 回执），绝不重读状态或报成功。
      expect(handled.length).toBeLessThanOrEqual(1);
    } finally {
      vi.useRealTimers();
    }
  }, 15000);

  it('退出登录走统一动作并经真实状态刷新显示成功；启动失败不打开外链', async () => {
    let loggedOut = false;
    service.state = (agentId) => stateFor(agentId, {
      ...DEFAULT_CONFIG,
      enabled: true,
      connectors: [connector({
        id: 'oauth-flow',
        name: 'OAuth MCP',
        status: 'stopped',
        authType: 'oauth',
        authStatus: loggedOut ? 'disconnected' : 'connected',
      })],
    });
    service.action = body => applied(body.action);

    render(<McpTab />);
    await screen.findByText('OAuth MCP');
    fireEvent.click(screen.getByTestId('mcp-connector-row-oauth-flow'));
    await screen.findByTestId('mcp-connector-detail-oauth-flow');
    loggedOut = true;
    fireEvent.click(screen.getByRole('button', { name: 'settings.oauth.logout' }));

    await waitFor(() => expect(service.calls.some(call => call.path === '/lingxi/v1/mcp/connectors/oauth-flow/oauth/logout')).toBe(true));
    await waitFor(() => expect(useSettingsStore.getState().toastType).toBe('success'));
    expect(hostMocks.openExternal).not.toHaveBeenCalled();
    await waitFor(() => expect(service.calls.filter(call => call.path === '/lingxi/v1/mcp/state').length).toBeGreaterThanOrEqual(2));
  });

  it('登录启动失败显示错误且不打开外链、不进入轮询', async () => {
    service.state = (agentId) => stateFor(agentId, {
      ...DEFAULT_CONFIG,
      enabled: true,
      connectors: [connector({ id: 'oauth-flow', name: 'OAuth MCP', status: 'stopped', authType: 'oauth', authStatus: 'disconnected' })],
    });
    const originalFetch = fetchMock.getMockImplementation()!;
    fetchMock.mockImplementation(async (input, init) => {
      const url = new URL(String(input));
      if (url.pathname === '/lingxi/v1/mcp/connectors/oauth-flow/oauth/start') {
        return jsonResponse({ error: 'authorize_failed', message: '授权服务器不可达' }, 500);
      }
      return originalFetch(input, init);
    });

    render(<McpTab />);
    await screen.findByText('OAuth MCP');
    fireEvent.click(screen.getByTestId('mcp-connector-row-oauth-flow'));
    await screen.findByTestId('mcp-connector-detail-oauth-flow');
    fireEvent.click(screen.getByRole('button', { name: 'settings.mcp.oauthConnect' }));

    await waitFor(() => expect(useSettingsStore.getState().toastType).toBe('error'));
    expect(useSettingsStore.getState().toastMessage).toContain('authorize_failed');
    expect(hostMocks.openExternal).not.toHaveBeenCalled();
    expect(service.calls.some(call => call.path.startsWith('/lingxi/v1/mcp/oauth/poll/'))).toBe(false);
    expect(screen.queryByTestId('mcp-oauth-waiting')).toBeNull();
  }, 15000);
});

describe('R08 MCP 设置页：批量成功/失败混合回执与权限保存失败（补充断言 4/6 未覆盖子句）', () => {
  it('批量导入 HTTP 200 混合回执逐行呈现成功/runtimeApplied=false 事实，不弹全成功 toast', async () => {
    service.state = (agentId) => stateFor(agentId, { ...DEFAULT_CONFIG, enabled: false, connectors: [] });
    service.action = body => {
      if (body.action === 'mcp.connector.add_bulk') {
        // Rust 正式回执：整批 applied，但逐行 runtimeApplied 可失败；启动失败不能抹去保存事实。
        return {
          settingsUpdate: { action: body.action, status: 'applied', saved: true, runtimeApplied: true },
          results: [
            { ok: true, id: 'good', saved: true, runtimeApplied: true },
            { ok: false, id: 'slow', saved: true, runtimeApplied: false, error: '连接器启动失败' },
          ],
        };
      }
      return applied(body.action);
    };

    render(<McpTab />);
    await screen.findByText('settings.mcp.noConnectors');
    fireEvent.click(screen.getByText('settings.mcp.importJson'));
    fireEvent.change(await screen.findByPlaceholderText(/mcpServers/), {
      target: {
        value: JSON.stringify({
          mcpServers: {
            good: { url: 'https://good.example.com/mcp' },
            slow: { url: 'https://slow.example.com/mcp' },
          },
        }),
      },
    });
    fireEvent.click(screen.getAllByRole('button').find(button => button.textContent === 'settings.mcp.importParse') as HTMLButtonElement);
    await screen.findByTestId('mcp-import-preview');
    fireEvent.click(screen.getAllByRole('button').find(button => button.textContent === 'settings.mcp.importConfirm') as HTMLButtonElement);

    await screen.findByTestId('mcp-import-result');
    // 逐行反馈：成功行、saved-but-not-runtime-applied 行、失败原因行都在。
    expect(screen.getByText('settings.mcp.importItemOk')).toBeTruthy();
    expect(screen.getByText(/settings\.transfer\.savedNotApplied .*连接器启动失败/)).toBeTruthy();
    // 混合回执不得弹「全成功」toast。
    expect(useSettingsStore.getState().toastVisible).toBe(false);
    const bulkBody = service.calls.find(call => call.path === '/lingxi/v1/mcp/settings-action')!.body as { action: string };
    expect(bulkBody.action).toBe('mcp.connector.add_bulk');
  });

  it('工具权限与置顶保存失败显示错误且不伪装授权已更新', async () => {
    const toolConnector = connector({
      id: 'gh',
      name: 'GitHub MCP',
      status: 'running',
      permissionMode: 'allowlist',
      tools: [{ name: 'create_issue', title: 'Create issue' }],
    });
    service.state = (agentId) => stateFor(agentId, {
      ...DEFAULT_CONFIG,
      enabled: true,
      connectors: [toolConnector],
    }, { connectors: { gh: { enabled: true, tools: {} } } });
    service.action = body => ({
      settingsUpdate: {
        action: body.action,
        status: 'failed',
        saved: false,
        runtimeApplied: false,
        error: { code: 'policy_write_failed', message: '策略写入被拒绝' },
      },
    });

    render(<McpTab />);
    await screen.findByText('GitHub MCP');
    fireEvent.click(screen.getByTestId('mcp-connector-row-gh'));
    await screen.findByTestId('mcp-connector-detail-gh');

    const allow = screen.getByRole('switch', { name: 'create_issue settings.mcp.permissionAllow' }) as HTMLButtonElement;
    expect(allow.getAttribute('aria-checked')).toBe('false');
    fireEvent.click(allow);
    await waitFor(() => expect(useSettingsStore.getState().toastType).toBe('error'));
    expect(useSettingsStore.getState().toastMessage).toContain('策略写入被拒绝');
    // 失败后重读真实状态：权限开关仍为未授权，页面不伪装已更新。
    await waitFor(() => expect(service.calls.filter(call => call.path === '/lingxi/v1/mcp/settings-action').length).toBeGreaterThanOrEqual(1));
    await waitFor(() => expect(service.calls.filter(call => call.path === '/lingxi/v1/mcp/state').length).toBeGreaterThanOrEqual(2));
    expect((screen.getByRole('switch', { name: 'create_issue settings.mcp.permissionAllow' }) as HTMLButtonElement).getAttribute('aria-checked')).toBe('false');
    expect(screen.queryByText(/settings\.autoSaved/)).toBeNull();
  });
});
