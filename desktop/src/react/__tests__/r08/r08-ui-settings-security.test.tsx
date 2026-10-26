// @vitest-environment jsdom

/**
 * R08 UI 叶 F-D04-UI-UI-SETTINGS-SECURITY-DA5AD5 客户端份额逐断言证据。
 *
 * 真实渲染生产 `SecurityTab` + 真实 settings store/actions/helpers；只有网络传输层
 * （全局 fetch）替换为合成 lingxi-service 回执。断言页面投影、PUT 请求体
 * （每助手 config 接口承载全局 preferences 的现役契约）、重读副作用、空态与错误态。
 */

import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';

const hostMocks = vi.hoisted(() => {
  const api = {
    settingsChanged: vi.fn(),
  };
  Object.defineProperty(window, 'platform', { configurable: true, value: api });
  return api;
});

import { SecurityTab } from '../../settings/tabs/SecurityTab';
import { useSettingsStore } from '../../settings/store';
import { createLocalServerConnection } from '../../services/server-connection';

interface WireCall { path: string; search: string; method: string; body: unknown }

const baseConfig = {
  sandbox: true,
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
  config: { ...baseConfig } as Record<string, unknown>,
  checkpoints: [] as Array<{ id: string; ts: number; tool: string; path: string; size: number }>,
  failPut: null as null | string,
  failSnapshot: false,
  failCheckpoints: null as null | string,
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
  if (call.path === '/lingxi/v1/agents/lingxi/config') {
    if (call.method === 'PUT') {
      if (service.failPut) return jsonResponse({ error: service.failPut }, 500);
      service.config = { ...service.config, ...(call.body as Record<string, unknown>) };
      return jsonResponse({ ok: true });
    }
    if (service.failSnapshot) return jsonResponse({ error: '真实快照读取失败' }, 500);
    return jsonResponse({ ...service.config });
  }
  if (call.path === '/lingxi/v1/agents/lingxi/identity') return jsonResponse({ content: '' });
  if (call.path === '/lingxi/v1/agents/lingxi/agents-md') return jsonResponse({ content: '' });
  if (call.path === '/lingxi/v1/agents/lingxi/public-agents-md') return jsonResponse({ content: '' });
  if (call.path === '/lingxi/v1/agents/lingxi/experience') return jsonResponse({ content: '' });
  if (call.path === '/lingxi/v1/user-profile') return jsonResponse({ content: '' });
  if (call.path === '/lingxi/v1/preferences/models') return jsonResponse({});
  if (call.path === '/lingxi/v1/checkpoints') {
    if (service.failCheckpoints) return jsonResponse({ error: service.failCheckpoints }, 500);
    return jsonResponse({ checkpoints: service.checkpoints });
  }
  throw new Error(`unexpected fetch ${call.method} ${call.path}`);
});

const initialStore = useSettingsStore.getState();
let portSeed = 43510;

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

function setConfig(config: Record<string, unknown> | null) {
  useSettingsStore.setState({
    currentAgentId: 'lingxi',
    settingsAgentId: null,
    agents: [{ id: 'lingxi', name: 'Hanako', yuan: 'lingxi', isPrimary: true } as never],
    platformName: 'darwin',
    settingsConfig: config as never,
  });
}

function switches(): HTMLButtonElement[] {
  return screen.getAllByRole('switch') as HTMLButtonElement[];
}

function selectTriggers(): HTMLButtonElement[] {
  return Array.from(document.querySelectorAll('button[aria-haspopup="listbox"]')) as HTMLButtonElement[];
}

beforeEach(() => {
  service.calls = [];
  service.config = { ...baseConfig };
  service.checkpoints = [];
  service.failPut = null;
  service.failSnapshot = false;
  service.failCheckpoints = null;
  hostMocks.settingsChanged.mockReset();
  window.t = ((key: string) => key) as typeof window.t;
  portSeed += 1;
  connect(portSeed);
  setConfig(service.config);
  vi.stubGlobal('fetch', fetchMock);
  fetchMock.mockClear();
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  useSettingsStore.setState(initialStore, true);
  useSettingsStore.setState({ currentAgentId: null, agents: [], settingsConfig: null, toastMessage: '', toastType: '', toastVisible: false });
});

describe('R08 安全设置页：沙箱与沙箱网络（断言 1）', () => {
  it('两个沙箱开关经每助手配置接口写入并重读当前助手合成配置视图', async () => {
    render(<SecurityTab />);
    const [sandbox, network] = switches();
    expect(sandbox.getAttribute('aria-checked')).toBe('true');
    expect(network.getAttribute('aria-checked')).toBe('true');

    // 先写联网沙箱（沙箱关闭后该开关按语义禁用），再写沙箱本身。
    fireEvent.click(network);
    await waitFor(() => expect(service.calls.some(call => call.path === '/lingxi/v1/agents/lingxi/config' && call.method === 'PUT')).toBe(true));
    expect(service.calls.filter(call => call.path === '/lingxi/v1/agents/lingxi/config' && call.method === 'PUT').at(-1)!.body)
      .toEqual({ sandbox_network: false });
    await waitFor(() => expect(service.calls.filter(call => call.path === '/lingxi/v1/agents/lingxi/config' && call.method === 'GET').length).toBeGreaterThanOrEqual(1));

    fireEvent.click(screen.getAllByRole('switch')[0]);
    await waitFor(() => expect(service.calls.filter(call => call.path === '/lingxi/v1/agents/lingxi/config' && call.method === 'PUT').length).toBeGreaterThanOrEqual(2));
    // 保存成功后立即重读当前助手 config 快照。
    expect(service.calls.filter(call => call.path === '/lingxi/v1/agents/lingxi/config' && call.method === 'PUT').at(-1)!.body)
      .toEqual({ sandbox: false });
    await waitFor(() => expect(service.config.sandbox).toBe(false));
    await waitFor(() => expect(screen.getAllByRole('switch')[0].getAttribute('aria-checked')).toBe('false'));
    expect(service.calls.filter(call => call.path === '/lingxi/v1/agents/lingxi/config' && call.method === 'GET').length)
      .toBeGreaterThanOrEqual(2);
  });

  it('状态未初始化时开关为 loading/禁用，不把未加载当关闭，也不发保存请求', async () => {
    setConfig(null);
    render(<SecurityTab />);
    const items = switches();
    expect(items).toHaveLength(2);
    for (const item of items) {
      expect(item.getAttribute('aria-checked')).toBe('mixed');
      expect(item.getAttribute('aria-busy')).toBe('true');
      expect(item.disabled).toBe(true);
      fireEvent.click(item);
    }
    expect(service.calls.some(call => call.method === 'PUT')).toBe(false);
  });

  it('保存失败恢复开关并提示错误；PUT 已成功但快照读取失败时如实报保存失败', async () => {
    service.failPut = '真实写入被拒绝';
    render(<SecurityTab />);
    fireEvent.click(switches()[0]);
    await waitFor(() => expect(useSettingsStore.getState().toastType).toBe('error'));
    expect(useSettingsStore.getState().toastMessage).toContain('真实写入被拒绝');
    expect(useSettingsStore.getState().settingsConfig?.sandbox).toBe(true);

    cleanup();
    service.failPut = null;
    service.failSnapshot = true;
    setConfig(service.config);
    render(<SecurityTab />);
    fireEvent.click(switches()[0]);
    await waitFor(() => expect(useSettingsStore.getState().toastType).toBe('error'));
    // PUT 实际已生效，但快照 GET 失败时 helper 仍报保存失败——不能据 toast 断言未持久化。
    expect(useSettingsStore.getState().toastMessage).toContain('settings.saveFailed');
    expect(service.calls.some(call => call.path === '/lingxi/v1/agents/lingxi/config' && call.method === 'PUT')).toBe(true);
  });
});

describe('R08 安全设置页：检查点保留策略与备份列表（断言 2）', () => {
  it('保留天数写入每助手配置并展开读取真实检查点列表', async () => {
    service.checkpoints = [
      { id: 'cp-1', ts: 1_700_000_000_000, tool: 'edit', path: '/Users/testuser/work/owned.txt', size: 2048 },
    ];
    render(<SecurityTab />);
    // 第一个下拉是保留策略。
    fireEvent.click(selectTriggers()[0]);
    const option = await screen.findByRole('option', { name: 'settings.security.retention3d' });
    fireEvent.click(option);
    await waitFor(() => expect(service.calls.some(call => call.path === '/lingxi/v1/agents/lingxi/config' && call.method === 'PUT')).toBe(true));
    expect(service.calls.filter(call => call.path === '/lingxi/v1/agents/lingxi/config' && call.method === 'PUT').at(-1)!.body)
      .toEqual({ file_backup: { retention_days: 3 } });

    fireEvent.click(screen.getByRole('button', { name: /settings.security.viewBackups/ }));
    await waitFor(() => expect(service.calls.some(call => call.path === '/lingxi/v1/checkpoints')).toBe(true));
    await screen.findByText(/owned\.txt/);
    expect(screen.getByText('2.0 KB')).toBeTruthy();
  });

  it('无检查点显示空列表；读取失败显示显式错误并保留重试，不冒充空列表', async () => {
    render(<SecurityTab />);
    fireEvent.click(screen.getByRole('button', { name: /settings.security.viewBackups/ }));
    await screen.findByText('settings.security.noBackups');
    expect(screen.queryByRole('alert')).toBeNull();

    cleanup();
    service.failCheckpoints = '真实检查点读取失败';
    render(<SecurityTab />);
    fireEvent.click(screen.getByRole('button', { name: /settings.security.viewBackups/ }));
    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('settings.security.backupsLoadFailed');
    expect(screen.queryByText('settings.security.noBackups')).toBeNull();
    service.failCheckpoints = null;
    fireEvent.click(screen.getByRole('button', { name: 'action.retry' }));
    await screen.findByText('settings.security.noBackups');
  });
});

describe('R08 安全设置页：网络代理模式与字段保存（断言 3）', () => {
  it('手工代理保存 HTTP/HTTPS/WS/WSS/noProxy 并广播 network-proxy-changed、重读配置', async () => {
    render(<SecurityTab />);
    fireEvent.click(selectTriggers()[1]);
    fireEvent.click(await screen.findByRole('option', { name: 'settings.security.networkProxyManual' }));

    fireEvent.change(screen.getByPlaceholderText('settings.security.networkProxyHttp'), { target: { value: 'http://127.0.0.1:7890' } });
    fireEvent.change(screen.getByPlaceholderText('settings.security.networkProxyHttps'), { target: { value: 'http://127.0.0.1:7891' } });
    fireEvent.change(screen.getByPlaceholderText('settings.security.networkProxyWs'), { target: { value: 'ws://127.0.0.1:7892' } });
    fireEvent.change(screen.getByPlaceholderText('settings.security.networkProxyWss'), { target: { value: 'wss://127.0.0.1:7893' } });
    fireEvent.change(screen.getByPlaceholderText('settings.security.networkProxyNoProxy'), { target: { value: 'localhost, 127.0.0.1' } });

    fireEvent.click(screen.getByRole('button', { name: 'settings.security.networkProxySave' }));
    await waitFor(() => expect(service.calls.some(call => call.path === '/lingxi/v1/agents/lingxi/config' && call.method === 'PUT')).toBe(true));
    expect(service.calls.filter(call => call.path === '/lingxi/v1/agents/lingxi/config' && call.method === 'PUT').at(-1)!.body).toEqual({
      network_proxy: {
        mode: 'manual',
        httpProxy: 'http://127.0.0.1:7890',
        httpsProxy: 'http://127.0.0.1:7891',
        wsProxy: 'ws://127.0.0.1:7892',
        wssProxy: 'wss://127.0.0.1:7893',
        noProxy: 'localhost, 127.0.0.1',
      },
    });
    expect(hostMocks.settingsChanged).toHaveBeenCalledWith('network-proxy-changed', {
      network_proxy: {
        mode: 'manual',
        httpProxy: 'http://127.0.0.1:7890',
        httpsProxy: 'http://127.0.0.1:7891',
        wsProxy: 'ws://127.0.0.1:7892',
        wssProxy: 'wss://127.0.0.1:7893',
        noProxy: 'localhost, 127.0.0.1',
      },
    });
    await waitFor(() => expect(useSettingsStore.getState().toastType).toBe('success'));
    await waitFor(() => expect(service.calls.filter(call => call.path === '/lingxi/v1/agents/lingxi/config' && call.method === 'GET').length).toBeGreaterThanOrEqual(1));
  });

  it('系统代理模式不渲染手工字段（保存仅模式）；保存失败提示并保留草稿，不广播', async () => {
    render(<SecurityTab />);
    fireEvent.click(selectTriggers()[1]);
    fireEvent.click(await screen.findByRole('option', { name: 'settings.security.networkProxyManual' }));
    expect(screen.getByPlaceholderText('settings.security.networkProxyHttp')).toBeTruthy();

    fireEvent.click(selectTriggers()[1]);
    fireEvent.click(await screen.findByRole('option', { name: 'settings.security.networkProxySystem' }));
    expect(screen.queryByPlaceholderText('settings.security.networkProxyHttp')).toBeNull();

    service.failPut = '代理写入失败';
    fireEvent.click(screen.getByRole('button', { name: 'settings.security.networkProxySave' }));
    await waitFor(() => expect(useSettingsStore.getState().toastType).toBe('error'));
    expect(useSettingsStore.getState().toastMessage).toContain('代理写入失败');
    expect(hostMocks.settingsChanged).not.toHaveBeenCalled();
    // 草稿仍保留（模式选择仍显示 system）。
    expect(selectTriggers()[1]?.title).toContain('settings.security.networkProxySystem');
  });
});

describe('R08 安全设置页：代理通知失败不得冒充完成（补充断言 3 未覆盖子句）', () => {
  it('network-proxy-changed 通知抛错时仅记录未处理异常，不弹保存成功、不完成后续重读', async () => {
    const rejections: unknown[] = [];
    const onUnhandled = (reason: unknown) => { rejections.push(reason); };
    process.on('unhandledRejection', onUnhandled);
    try {
      render(<SecurityTab />);
      fireEvent.click(selectTriggers()[1]);
      fireEvent.click(await screen.findByRole('option', { name: 'settings.security.networkProxyManual' }));
      fireEvent.change(screen.getByPlaceholderText('settings.security.networkProxyHttp'), { target: { value: 'http://127.0.0.1:7890' } });
      hostMocks.settingsChanged.mockImplementationOnce(() => { throw new Error('IPC 不可用'); });

      const identityReadsBefore = service.calls.filter(call => call.path === '/lingxi/v1/agents/lingxi/identity').length;
      fireEvent.click(screen.getByRole('button', { name: 'settings.security.networkProxySave' }));

      // PUT 已发出；通知抛错发生在成功 toast 与重读之前。
      await waitFor(() => expect(service.calls.some(call => call.path === '/lingxi/v1/agents/lingxi/config' && call.method === 'PUT')).toBe(true));
      await waitFor(() => expect(rejections.length).toBeGreaterThan(0));
      expect(String((rejections[0] as Error)?.message ?? rejections[0])).toContain('IPC 不可用');
      expect(useSettingsStore.getState().toastType).not.toBe('success');
      expect(service.calls.filter(call => call.path === '/lingxi/v1/agents/lingxi/identity').length).toBe(identityReadsBefore);
    } finally {
      process.off('unhandledRejection', onUnhandled);
    }
  });
});
