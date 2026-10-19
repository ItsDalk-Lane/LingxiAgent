/**
 * @vitest-environment jsdom
 */

import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';

const browserBridge = vi.hoisted(() => {
  const bridge = { openExternal: vi.fn() };
  Object.defineProperty(window, 'platform', { configurable: true, value: bridge });
  return bridge;
});

const apiMocks = vi.hoisted(() => ({
  loadMcpState: vi.fn(),
  setMcpEnabled: vi.fn(),
  addMcpConnector: vi.fn(),
  removeMcpConnector: vi.fn(),
  runMcpConnectorAction: vi.fn(),
  setAgentMcpConnector: vi.fn(),
  setAgentMcpTool: vi.fn(),
  startMcpOAuth: vi.fn(),
  pollMcpOAuth: vi.fn(),
  cancelMcpOAuth: vi.fn(),
  logoutMcpOAuth: vi.fn(),
}));

vi.mock('../../helpers', () => ({
  t: (key: string) => key,
}));

vi.mock('../mcp/mcp-api', () => ({
  EMPTY_MCP_STATE: {
    enabled: false,
    connectors: [],
    agentConfig: { connectors: {} },
  },
  ...apiMocks,
}));

import { McpTab } from '../McpTab';
import { useSettingsStore } from '../../store';
import { WindowSurfaceProvider, type WindowSurface } from '../../../ui/window-surface';
import type { McpConnector } from '../mcp/types';
import { createLocalServerConnection } from '../../../services/server-connection';
const initialStore = useSettingsStore.getState();
function connectFixture(port: number) {
  const local = createLocalServerConnection({ serverPort: port, serverToken: `owned-${port}`, serverNodeKind: 'lingxi-service' })!;
  useSettingsStore.setState({ activeServerConnectionId: 'local', serverConnections: { local }, activeServerConnection: local });
  useSettingsStore.setState({ currentAgentId: 'lingxi', agents: [{ id: 'lingxi', name: 'Hanako', yuan: 'lingxi', isPrimary: true }] });
}
function pending<T>() {
  let resolve!: (value: T) => void; let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function state(enabled: boolean, connectors: McpConnector[] = []) {
  return {
    enabled,
    connectors,
    agentConfig: { connectors: {} },
  };
}

beforeEach(() => {
  Object.defineProperty(browserBridge, 'openExternal', { configurable: true, writable: true, value: vi.fn().mockResolvedValue(undefined) });
});

afterEach(() => {
  cleanup();
  document.body.innerHTML = '';
  Object.values(apiMocks).forEach(mock => mock.mockReset());
  useSettingsStore.setState(initialStore, true);
  useSettingsStore.setState({
    currentAgentId: null,
    agents: [],
    toastMessage: '',
    toastType: '',
    toastVisible: false,
  });
});

describe('McpTab', () => {
  it('旧连接打开浏览器的迟到拒绝不取消新连接的登录等待', async () => {
    const oldOpen = pending<void>(); const newOpen = pending<void>();
    const connector: McpConnector = { id: 'browser-owner', name: 'Owned late browser', transport: 'streamable-http',
      url: 'http://127.0.0.1:33500/mcp', authType: 'oauth', status: 'stopped', tools: [] };
    apiMocks.loadMcpState.mockResolvedValue(state(true, [connector]));
    apiMocks.startMcpOAuth.mockResolvedValue({ sessionId: 'browser-owner-flow', url: 'http://127.0.0.1:33500/authorize' });
    browserBridge.openExternal.mockImplementationOnce(() => oldOpen.promise).mockImplementationOnce(() => newOpen.promise);
    apiMocks.cancelMcpOAuth.mockResolvedValue(undefined);
    connectFixture(33541); render(<McpTab />);
    fireEvent.click(await screen.findByRole('button', { name: 'settings.mcp.manage' }));
    fireEvent.click(screen.getByRole('button', { name: 'settings.mcp.oauthConnect' }));
    await waitFor(() => expect(browserBridge.openExternal).toHaveBeenCalledTimes(1));
    await act(async () => { connectFixture(33542); });
    fireEvent.click(await screen.findByRole('button', { name: 'settings.mcp.manage' }));
    fireEvent.click(screen.getByRole('button', { name: 'settings.mcp.oauthConnect' }));
    await waitFor(() => expect(browserBridge.openExternal).toHaveBeenCalledTimes(2));
    await act(async () => { oldOpen.reject(new Error('旧系统打开失败')); });
    expect(screen.getByTestId('mcp-oauth-waiting')).toBeTruthy();
    expect(apiMocks.cancelMcpOAuth).not.toHaveBeenCalled();
    expect(apiMocks.pollMcpOAuth).not.toHaveBeenCalled();
    expect(useSettingsStore.getState().toastVisible).toBe(false);
    fireEvent.click(within(screen.getByTestId('mcp-oauth-waiting')).getByRole('button', { name: 'common.cancel' }));
    await act(async () => { newOpen.resolve(); });
    expect(apiMocks.cancelMcpOAuth).toHaveBeenCalledExactlyOnceWith('browser-owner');
    expect(apiMocks.pollMcpOAuth).not.toHaveBeenCalled();
  });

  it('浏览器打开失败且服务取消未确认时同时保留两项真实错误', async () => {
    const connector: McpConnector = { id: 'browser-cleanup', name: 'Owned cleanup failure', transport: 'streamable-http',
      url: 'http://127.0.0.1:33500/mcp', authType: 'oauth', status: 'stopped', tools: [] };
    apiMocks.loadMcpState.mockResolvedValue(state(true, [connector]));
    apiMocks.startMcpOAuth.mockResolvedValue({ sessionId: 'browser-cleanup-flow', url: 'http://127.0.0.1:33500/authorize' });
    browserBridge.openExternal.mockResolvedValue({ ok: false, error: '系统打开失败' });
    apiMocks.cancelMcpOAuth.mockRejectedValue(new Error('实际服务取消失败'));
    connectFixture(33543); render(<McpTab />);
    fireEvent.click(await screen.findByRole('button', { name: 'settings.mcp.manage' }));
    fireEvent.click(screen.getByRole('button', { name: 'settings.mcp.oauthConnect' }));
    await waitFor(() => expect(useSettingsStore.getState().toastMessage).toContain('实际服务取消失败'));
    expect(useSettingsStore.getState().toastMessage).toContain('系统打开失败');
    expect(useSettingsStore.getState().toastMessage).toContain('登录等待取消未确认');
    expect(apiMocks.pollMcpOAuth).not.toHaveBeenCalled();
  });

  it.each(['missing', 'rejected', 'receipt'] as const)('打开登录浏览器失败须立即显示原因并停止轮询：%s', async mode => {
    const connector: McpConnector = { id: 'browser-check', name: 'Owned browser failure', transport: 'streamable-http',
      url: 'http://127.0.0.1:33500/mcp', authType: 'oauth', status: 'stopped', tools: [] };
    apiMocks.loadMcpState.mockResolvedValue(state(true, [connector]));
    apiMocks.startMcpOAuth.mockResolvedValue({ sessionId: 'browser-flow', url: 'http://127.0.0.1:33500/authorize' });
    apiMocks.cancelMcpOAuth.mockResolvedValue(undefined);
    const failure = new Error('系统拒绝打开浏览器');
    const rejection = Promise.reject(failure);
    // 首轮旧实现会忽略 Promise；测试保全原错误，同时避免未处理拒绝污染其他场景。
    void rejection.catch(() => {});
    Object.defineProperty(browserBridge, 'openExternal', { configurable: true, writable: true, value: mode === 'missing' ? undefined
      : vi.fn().mockImplementation(() => mode === 'rejected' ? rejection : Promise.resolve({ ok: false, error: failure.message })) });
    connectFixture(33531); render(<McpTab />);
    fireEvent.click(await screen.findByRole('button', { name: 'settings.mcp.manage' }));
    fireEvent.click(screen.getByRole('button', { name: 'settings.mcp.oauthConnect' }));
    await waitFor(() => expect(useSettingsStore.getState().toastMessage).toContain(mode === 'missing' ? 'settings.oauth.browserUnavailable' : failure.message));
    expect(useSettingsStore.getState().toastType).toBe('error');
    expect(screen.queryByTestId('mcp-oauth-waiting')).toBeNull();
    expect(apiMocks.pollMcpOAuth).not.toHaveBeenCalled();
    expect(apiMocks.cancelMcpOAuth).toHaveBeenCalledExactlyOnceWith('browser-check');
    if (mode !== 'missing') expect(browserBridge.openExternal).toHaveBeenCalledOnce();
  });

  it('真实登录done回执之后状态读取失败不能弹出保存成功', async () => {
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
    const connector: McpConnector = { id: 'same', name: 'Owned OAuth', transport: 'streamable-http',
      url: 'http://127.0.0.1:33500/mcp', authType: 'oauth', status: 'stopped', tools: [] };
    apiMocks.loadMcpState.mockResolvedValueOnce(state(true, [connector])).mockRejectedValue(new Error('实际登录状态读取失败'));
    apiMocks.startMcpOAuth.mockResolvedValue({ sessionId: 'fixture-state', url: 'http://127.0.0.1:33500/authorize' });
    apiMocks.pollMcpOAuth.mockResolvedValue({ status: 'done' });
    connectFixture(33521); render(<McpTab />);
    try {
      fireEvent.click(await screen.findByRole('button', { name: 'settings.mcp.manage' }));
      vi.useFakeTimers();
      await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'settings.mcp.oauthConnect' })); });
      await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
      expect(apiMocks.pollMcpOAuth).toHaveBeenCalledWith('fixture-state');
      expect(useSettingsStore.getState().toastType).toBe('error');
      expect(useSettingsStore.getState().toastMessage).toContain('登录可能已保存，但状态刷新未确认');
    } finally { vi.useRealTimers(); quiet.mockRestore(); }
  });

  it('换连接后旧动作的刷新和finally不能清除新连接同键的busy或发旧提示', async () => {
    const delayedRefresh = pending<ReturnType<typeof state>>();
    const newAction = pending<void>();
    apiMocks.loadMcpState.mockResolvedValue(state(false));
    apiMocks.setMcpEnabled.mockResolvedValueOnce(undefined).mockImplementationOnce(() => newAction.promise);
    connectFixture(33501); render(<McpTab />);
    await screen.findByRole('switch', { name: 'common.off' });
    apiMocks.loadMcpState.mockImplementationOnce(() => delayedRefresh.promise);
    fireEvent.click(screen.getByText('settings.mcp.masterName'));
    await waitFor(() => expect(apiMocks.loadMcpState).toHaveBeenCalledTimes(2));
    await act(async () => { connectFixture(33502); });
    await waitFor(() => expect(apiMocks.loadMcpState).toHaveBeenCalledTimes(3));
    fireEvent.click(screen.getByText('settings.mcp.masterName'));
    await waitFor(() => expect(apiMocks.setMcpEnabled).toHaveBeenCalledTimes(2));
    expect((screen.getByRole('switch', { name: 'common.off' }) as HTMLButtonElement).disabled).toBe(true);
    await act(async () => { delayedRefresh.resolve(state(true)); });
    expect((screen.getByRole('switch', { name: 'common.off' }) as HTMLButtonElement).disabled).toBe(true);
    expect(useSettingsStore.getState().toastVisible).toBe(false);
    await act(async () => { newAction.resolve(); });
    await waitFor(() => expect(useSettingsStore.getState().toastType).toBe('success'));
  });

  it('旧登录异常和旧取消异常不能清除新连接同id等待或污染其提示', async () => {
    const oldStart = pending<{ sessionId: string; url: string }>();
    const nextStart = pending<{ sessionId: string; url: string }>();
    const latestStart = pending<{ sessionId: string; url: string }>();
    const oldCancel = pending<void>();
    const connector: McpConnector = { id: 'same', name: 'Owned OAuth', transport: 'streamable-http',
      url: 'http://127.0.0.1:33500/mcp', authType: 'oauth', status: 'stopped', tools: [] };
    apiMocks.loadMcpState.mockResolvedValue(state(true, [connector]));
    apiMocks.startMcpOAuth.mockImplementationOnce(() => oldStart.promise)
      .mockImplementationOnce(() => nextStart.promise).mockImplementationOnce(() => latestStart.promise);
    apiMocks.cancelMcpOAuth.mockImplementationOnce(() => oldCancel.promise);
    connectFixture(33511); render(<McpTab />);
    fireEvent.click(await screen.findByRole('button', { name: 'settings.mcp.manage' }));
    fireEvent.click(screen.getByRole('button', { name: 'settings.mcp.oauthConnect' }));
    expect(screen.getByTestId('mcp-oauth-waiting')).toBeTruthy();
    await act(async () => { connectFixture(33512); });
    fireEvent.click(await screen.findByRole('button', { name: 'settings.mcp.manage' }));
    fireEvent.click(screen.getByRole('button', { name: 'settings.mcp.oauthConnect' }));
    await act(async () => { oldStart.reject(new Error('旧A登录失败')); });
    expect(screen.getByTestId('mcp-oauth-waiting')).toBeTruthy();
    expect(useSettingsStore.getState().toastVisible).toBe(false);
    fireEvent.click(within(screen.getByTestId('mcp-oauth-waiting')).getByRole('button', { name: 'common.cancel' }));
    await act(async () => { connectFixture(33513); });
    fireEvent.click(await screen.findByRole('button', { name: 'settings.mcp.manage' }));
    fireEvent.click(screen.getByRole('button', { name: 'settings.mcp.oauthConnect' }));
    await act(async () => { oldCancel.reject(new Error('旧B取消失败')); nextStart.reject(new Error('旧B登录失败')); });
    expect(screen.getByTestId('mcp-oauth-waiting')).toBeTruthy();
    expect(useSettingsStore.getState().toastVisible).toBe(false);
    await act(async () => { latestStart.reject(new Error('当前C真实失败')); });
    await waitFor(() => expect(useSettingsStore.getState().toastMessage).toContain('当前C真实失败'));
  });

  it('loads actual global MCP settings with no agent and keeps global controls available', async () => {
    apiMocks.loadMcpState.mockResolvedValue(state(false));
    useSettingsStore.setState({ currentAgentId: null, agents: [] });
    render(<McpTab />);
    await waitFor(() => expect(apiMocks.loadMcpState).toHaveBeenCalledWith(null));
    await screen.findByRole('switch', { name: 'common.off' });
    expect((screen.getByText('settings.mcp.addConnector') as HTMLButtonElement).disabled).toBe(false);
    expect((screen.getByText('settings.mcp.importJson') as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(screen.getByText('settings.mcp.masterName'));
    await waitFor(() => expect(apiMocks.setMcpEnabled).toHaveBeenCalledWith(true));
  });

  it('shows a read error instead of a successful empty connector list', async () => {
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      apiMocks.loadMcpState.mockRejectedValue(new Error('fixture state unavailable'));
      useSettingsStore.setState({ currentAgentId: 'lingxi', agents: [{ id: 'lingxi', name: 'Hanako', yuan: 'lingxi', isPrimary: true }] });
      render(<McpTab />);
      expect((await screen.findByRole('alert')).textContent).toContain('fixture state unavailable');
      expect(screen.queryByText('settings.mcp.noConnectors')).toBeNull();
      expect(apiMocks.setMcpEnabled).not.toHaveBeenCalled();
    } finally { quiet.mockRestore(); }
  });

  it('does not claim save success when the actual state refresh fails', async () => {
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      apiMocks.loadMcpState.mockResolvedValueOnce(state(false)).mockRejectedValue(new Error('fixture refresh failed'));
      apiMocks.setMcpEnabled.mockResolvedValue(undefined);
      useSettingsStore.setState({ currentAgentId: 'lingxi', agents: [{ id: 'lingxi', name: 'Hanako', yuan: 'lingxi', isPrimary: true }] });
      render(<McpTab />);
      await screen.findByRole('switch', { name: 'common.off' });
      fireEvent.click(screen.getByText('settings.mcp.masterName'));
      await waitFor(() => expect(useSettingsStore.getState().toastType).toBe('error'));
      expect(useSettingsStore.getState().toastMessage).toContain('状态刷新未确认');
      expect(screen.getByRole('alert').textContent).toContain('fixture refresh failed');
      expect(apiMocks.setMcpEnabled).toHaveBeenCalledTimes(1);
    } finally { quiet.mockRestore(); }
  });

  it('does not show an empty connector list while MCP state is still loading', async () => {
    let resolveState: (value: ReturnType<typeof state>) => void = () => {};
    apiMocks.loadMcpState.mockImplementation(() => new Promise(resolve => {
      resolveState = resolve;
    }));
    useSettingsStore.setState({
      currentAgentId: 'lingxi',
      agents: [{ id: 'lingxi', name: 'Hanako', yuan: 'lingxi', isPrimary: true }],
    });

    render(<McpTab />);

    expect(screen.queryByText('settings.mcp.noConnectors')).toBeNull();
    expect(screen.getAllByText('status.loading').length).toBeGreaterThan(0);

    resolveState(state(false));
    await waitFor(() => expect(screen.getAllByText('settings.mcp.noConnectors').length).toBeGreaterThan(0));
  });

  it('refreshes only while the real manager says a background connector is still connecting', async () => {
    const pending: McpConnector = { id: 'fixture', name: 'Owned SDK', transport: 'stdio',
      command: 'owned-fixture', status: 'connecting', tools: [] };
    apiMocks.loadMcpState.mockResolvedValueOnce(state(true, [pending]))
      .mockResolvedValue(state(true, [{ ...pending, status: 'running' }]));
    useSettingsStore.setState({ currentAgentId: 'lingxi',
      agents: [{ id: 'lingxi', name: 'Hanako', yuan: 'lingxi', isPrimary: true }] });
    const view = render(<McpTab />);
    await waitFor(() => expect(screen.getByText('settings.mcp.statusConnecting')).toBeTruthy());
    await waitFor(() => expect(screen.getByText('settings.mcp.statusRunning')).toBeTruthy());
    expect(apiMocks.loadMcpState).toHaveBeenCalledTimes(2);
    expect(apiMocks.runMcpConnectorAction).not.toHaveBeenCalled();
    view.unmount();
  });

  it('offers the global defer settings on the tab itself, not inside a connector', async () => {
    apiMocks.loadMcpState.mockResolvedValue(state(true));
    useSettingsStore.setState({
      currentAgentId: 'lingxi',
      agents: [{ id: 'lingxi', name: 'Hanako', yuan: 'lingxi', isPrimary: true }],
    });

    render(<McpTab />);

    await waitFor(() => expect(screen.getByText('settings.mcp.deferTitle')).toBeTruthy());
  });

  it('toggles the global connector switch when the master row text is clicked', async () => {
    apiMocks.loadMcpState
      .mockResolvedValueOnce(state(false))
      .mockResolvedValueOnce(state(true));
    apiMocks.setMcpEnabled.mockResolvedValue(undefined);
    useSettingsStore.setState({
      currentAgentId: 'lingxi',
      agents: [{ id: 'lingxi', name: 'Hanako', yuan: 'lingxi', isPrimary: true }],
    });

    render(<McpTab />);

    await waitFor(() => expect(apiMocks.loadMcpState).toHaveBeenCalledWith('lingxi'));
    fireEvent.click(screen.getByText('settings.mcp.masterName'));

    await waitFor(() => expect(apiMocks.setMcpEnabled).toHaveBeenCalledWith(true));
  });

  it('does not send duplicate global toggle requests when the small switch is clicked', async () => {
    apiMocks.loadMcpState
      .mockResolvedValueOnce(state(false))
      .mockResolvedValueOnce(state(true));
    apiMocks.setMcpEnabled.mockResolvedValue(undefined);
    useSettingsStore.setState({
      currentAgentId: 'lingxi',
      agents: [{ id: 'lingxi', name: 'Hanako', yuan: 'lingxi', isPrimary: true }],
    });

    render(<McpTab />);

    await waitFor(() => expect(apiMocks.loadMcpState).toHaveBeenCalledWith('lingxi'));
    fireEvent.click(screen.getByRole('switch', { name: 'common.off' }));

    await waitFor(() => expect(apiMocks.setMcpEnabled).toHaveBeenCalledTimes(1));
    expect(apiMocks.setMcpEnabled).toHaveBeenCalledWith(true);
  });

  it('opens connector removal in the owning settings overlay surface', async () => {
    const connector: McpConnector = {
      id: 'github-mcp',
      name: 'GitHub MCP',
      transport: 'streamable-http',
      url: 'https://mcp.example.com/mcp',
      status: 'stopped',
      tools: [],
    };
    apiMocks.loadMcpState.mockResolvedValue(state(true, [connector]));
    useSettingsStore.setState({
      currentAgentId: 'lingxi',
      agents: [{ id: 'lingxi', name: 'Hanako', yuan: 'lingxi', isPrimary: true }],
    });
    const overlayRoot = document.createElement('div');
    document.body.append(overlayRoot);
    const surface: WindowSurface = {
      id: 'settings-modal:mcp-tab-test',
      window,
      document,
      overlayRoot,
    };

    render(
      <WindowSurfaceProvider surface={surface}>
        <McpTab />
      </WindowSurfaceProvider>,
    );

    await waitFor(() => expect(screen.getByText('GitHub MCP')).toBeTruthy());
    fireEvent.click(screen.getByRole('button', { name: 'common.remove' }));

    expect(within(overlayRoot).getByRole('dialog', { name: 'settings.mcp.removeTitle' })).toBeTruthy();
  });
});
