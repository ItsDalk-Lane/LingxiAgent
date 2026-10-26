// @vitest-environment jsdom

/**
 * R08 设置容器初始化（MCP 设置页 / 安全设置页两条叶共用的第一条原断言）。
 *
 * 真实渲染生产 `SettingsContent`（含真实 store/actions/初始化链路），只有网络传输层
 * （全局 fetch）替换成合成 lingxi-service 回执。覆盖：冷启动 loading mask →
 * ready 后移除并渲染目标页；已有缓存直接显示旧数据并后台刷新；供应商摘要失败
 * 不阻塞 ready 也不冒充空列表；初始化异常记日志后仍 ready；15 秒超时强制 ready。
 */

import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, render, screen, waitFor } from '@testing-library/react';

const hostMocks = vi.hoisted(() => {
  const api = {
    getPlatform: vi.fn(async () => 'darwin'),
  };
  Object.defineProperty(window, 'platform', { configurable: true, value: api });
  return api;
});

import { SettingsContent } from '../../settings/SettingsContent';
import { useSettingsStore } from '../../settings/store';
import { captureSettingsConnection } from '../../settings/connection-owner';
import { createLocalServerConnection } from '../../services/server-connection';

interface WireCall { path: string; search: string; method: string; body: unknown }

const snapshotConfig = {
  sandbox: false,
  sandbox_network: true,
  file_backup: { retention_days: 1 },
  network_proxy: {
    mode: 'system',
    httpProxy: '',
    httpsProxy: '',
    wsProxy: '',
    wssProxy: '',
    noProxy: 'localhost, 127.0.0.1, ::1',
  },
};

const service = {
  calls: [] as WireCall[],
  identity: { serverNodeKind: 'lingxi-service', serverNodeTransport: 'http', serverId: 'srv-r08', studioId: 'studio-r08', authState: 'user', capabilities: [] as string[] },
  identityGate: null as null | Promise<void>,
  providersSummaryError: null as null | string,
  config: { ...snapshotConfig } as Record<string, unknown>,
};

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
}

function snapshot() {
  return {
    agentId: 'lingxi',
    config: { ...service.config },
    identity: '',
    agents: '',
    publicAgents: '',
    userProfile: '',
    experience: '',
    globalModels: {},
    preferences: {},
  };
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
    case '/lingxi/v1/settings/config':
      return jsonResponse({ locale: 'zh-CN' });
    case '/lingxi/v1/server/identity':
      if (service.identityGate) await service.identityGate;
      return jsonResponse(service.identity);
    case '/lingxi/v1/agents':
      return jsonResponse({ agents: [{ id: 'lingxi', name: 'Hanako', yuan: 'lingxi', isPrimary: true }] });
    case '/lingxi/v1/settings/snapshot':
      return jsonResponse(snapshot());
    case '/lingxi/v1/settings/avatar-health':
      return jsonResponse({ avatars: {} });
    case '/lingxi/v1/models':
      return jsonResponse({ models: [] });
    case '/lingxi/v1/providers/summary':
      if (service.providersSummaryError) return jsonResponse({ error: service.providersSummaryError }, 500);
      return jsonResponse({ providers: {} });
    case '/lingxi/v1/mcp/state':
      return jsonResponse({
        agentId: url.searchParams.get('agentId'),
        config: { enabled: true, deferEnabled: true, deferThreshold: 12, builtinDeferEnabled: false, connectors: [] },
        agentConfig: { connectors: {} },
      });
    default:
      return jsonResponse({ ok: true });
  }
});

const initialStore = useSettingsStore.getState();
let portSeed = 43610;

function presetConnection(port: number) {
  const connection = createLocalServerConnection({
    serverPort: port,
    serverToken: `owned-${port}`,
    serverNodeKind: 'lingxi-service',
    serverNodeTransport: 'http',
  })!;
  useSettingsStore.setState({
    activeServerConnectionId: 'local',
    serverConnections: { local: connection },
    activeServerConnection: connection,
  });
}

function setPlatform(connectionInfo: () => Promise<unknown>) {
  Object.assign(window.platform, {
    getServerConnectionInfo: vi.fn(connectionInfo),
  });
}

beforeEach(() => {
  service.calls = [];
  service.identity = { serverNodeKind: 'lingxi-service', serverNodeTransport: 'http', serverId: 'srv-r08', studioId: 'studio-r08', authState: 'user', capabilities: [] };
  service.identityGate = null;
  service.providersSummaryError = null;
  service.config = { ...snapshotConfig };
  hostMocks.getPlatform.mockReset();
  hostMocks.getPlatform.mockResolvedValue('darwin');
  portSeed += 1;
  const port = portSeed;
  setPlatform(async () => ({ port, token: `owned-${port}`, serverNodeKind: 'lingxi-service', serverNodeTransport: 'http' }));
  window.t = ((key: string) => key) as typeof window.t;
  window.i18n = { load: vi.fn(async () => {}), t: (key: string) => key, locale: 'zh-CN' } as unknown as typeof window.i18n;
  vi.stubGlobal('fetch', fetchMock);
  fetchMock.mockClear();
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  service.identityGate = null;
  useSettingsStore.setState(initialStore, true);
  useSettingsStore.setState({
    currentAgentId: null, agents: [], settingsConfig: null, settingsConfigKey: null,
    ready: false, rustSettingsUnavailable: false, providersSummary: {},
    providersSummaryError: null, activeTab: 'agent', toastMessage: '', toastType: '', toastVisible: false,
  });
});

describe('R08 设置容器初始化（MCP 设置页叶 & 安全设置页叶共用断言 1）', () => {
  it('MCP 设置页冷启动：显示 loading mask，初始化完成后移除并真实渲染 MCP 页', async () => {
    useSettingsStore.setState({ activeTab: 'mcp' });
    render(<SettingsContent variant="window" />);
    // 冷启动（无缓存）先有全屏 mask，不能把未完成初始化装成 ready 页面。
    expect(document.querySelector('#settingsLoadingMask')).not.toBeNull();

    await waitFor(() => expect(useSettingsStore.getState().ready).toBe(true), { timeout: 10000 });
    await waitFor(() => expect(document.querySelector('#settingsLoadingMask')).toBeNull());
    await screen.findByText('settings.mcp.masterName');
    const state = useSettingsStore.getState();
    expect(state.rustSettingsUnavailable).toBe(false);
    expect(state.settingsConfigStatus).toBe('ready');
    expect(state.settingsDataAgentId).toBe('lingxi');
    // 真实初始化链路：先身份核验再读连接器/助手资料。
    const paths = service.calls.map(call => call.path);
    expect(paths).toContain('/lingxi/v1/server/identity');
    expect(paths).toContain('/lingxi/v1/agents');
    expect(paths).toContain('/lingxi/v1/settings/snapshot');
  }, 20000);

  it('安全设置页冷启动：初始化完成后移除 mask 并渲染沙箱/备份/代理配置', async () => {
    useSettingsStore.setState({ activeTab: 'security' });
    render(<SettingsContent variant="window" />);
    expect(document.querySelector('#settingsLoadingMask')).not.toBeNull();
    await waitFor(() => expect(useSettingsStore.getState().ready).toBe(true), { timeout: 10000 });
    await waitFor(() => expect(document.querySelector('#settingsLoadingMask')).toBeNull());
    await waitFor(() => expect(screen.getAllByText('settings.security.sandbox').length).toBeGreaterThan(0));
    expect(screen.getByText('settings.security.fileBackup')).toBeTruthy();
    expect(screen.getByText('settings.security.networkProxy')).toBeTruthy();
  }, 20000);

  it('已有缓存直接显示旧数据并在后台刷新，不阻塞 ready、不显示全屏 mask', async () => {
    let releaseIdentity = () => {};
    service.identityGate = new Promise<void>(resolve => { releaseIdentity = resolve; });
    presetConnection(portSeed);
    useSettingsStore.setState({
      activeTab: 'security',
      ready: true,
      settingsIdentityOwner: captureSettingsConnection(),
      settingsConfig: { ...snapshotConfig, sandbox: false } as never,
    });
    render(<SettingsContent variant="window" />);
    // 身份核验仍在进行：旧数据直接可见，没有 mask。
    await waitFor(() => expect(service.calls.some(call => call.path === '/lingxi/v1/server/identity')).toBe(true));
    expect(document.querySelector('#settingsLoadingMask')).toBeNull();
    expect((screen.getAllByRole('switch')[0] as HTMLButtonElement).getAttribute('aria-checked')).toBe('false');

    // 后台刷新落地后按真实快照更新（snapshot 里 sandbox=true）。
    service.config = { ...snapshotConfig, sandbox: true };
    await act(async () => { releaseIdentity(); await Promise.resolve(); });
    await waitFor(() => expect((screen.getAllByRole('switch')[0] as HTMLButtonElement).getAttribute('aria-checked')).toBe('true'));
    expect(document.querySelector('#settingsLoadingMask')).toBeNull();
  }, 20000);

  it('供应商摘要失败不阻塞 ready，也不冒充空列表成功', async () => {
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      service.providersSummaryError = '真实供应商摘要读取失败';
      useSettingsStore.setState({ activeTab: 'mcp' });
      render(<SettingsContent variant="window" />);
      await waitFor(() => expect(useSettingsStore.getState().ready).toBe(true), { timeout: 10000 });
      await waitFor(() => expect(useSettingsStore.getState().providersSummaryError).toContain('真实供应商摘要读取失败'));
      expect(document.querySelector('#settingsLoadingMask')).toBeNull();
      // 摘要失败只记录错误，不向页面伪装成成功空列表。
      expect(useSettingsStore.getState().providersSummary).toEqual({});
      expect(quiet).toHaveBeenCalled();
    } finally {
      quiet.mockRestore();
    }
  }, 20000);

  it('初始化异常记录日志后仍 ready，并保留可操作的恢复入口（不装成成功空数据）', async () => {
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      service.identity = { serverNodeKind: 'other-node', serverNodeTransport: 'http', serverId: 'x', studioId: 'y', authState: 'user', capabilities: [] };
      useSettingsStore.setState({ activeTab: 'security' });
      render(<SettingsContent variant="window" />);
      await waitFor(() => expect(useSettingsStore.getState().ready).toBe(true), { timeout: 10000 });
      await waitFor(() => expect(document.querySelector('#settingsLoadingMask')).toBeNull());
      const state = useSettingsStore.getState();
      expect(state.rustSettingsUnavailable).toBe(true);
      expect(state.settingsIdentityOwner).toBeNull();
      expect(quiet).toHaveBeenCalled();
      expect(useSettingsStore.getState().toastType).toBe('error');
    } finally {
      quiet.mockRestore();
    }
  }, 20000);

  it('15 秒无响应强制 ready 并移除 loading mask，杜绝无限白屏', async () => {
    const quiet = vi.spyOn(console, 'warn').mockImplementation(() => {});
    try {
      service.identityGate = new Promise<void>(() => {});
      useSettingsStore.setState({ activeTab: 'security' });
      vi.useFakeTimers();
      try {
        render(<SettingsContent variant="window" />);
        await act(async () => {
          for (let i = 0; i < 8; i += 1) await Promise.resolve();
        });
        expect(useSettingsStore.getState().ready).toBe(false);
        await act(async () => {
          await vi.advanceTimersByTimeAsync(15_000);
          for (let i = 0; i < 8; i += 1) await Promise.resolve();
        });
        expect(useSettingsStore.getState().ready).toBe(true);
        expect(document.querySelector('#settingsLoadingMask')).toBeNull();
      } finally {
        vi.useRealTimers();
      }
    } finally {
      quiet.mockRestore();
    }
  }, 20000);
});
