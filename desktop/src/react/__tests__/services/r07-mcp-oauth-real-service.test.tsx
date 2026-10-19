/** @vitest-environment jsdom */
import React from 'react';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { expect, it } from 'vitest';
import { useSettingsStore } from '../../settings/store';

const base = process.env.LINGXI_R07_MCP_UI_BASE;
const token = process.env.LINGXI_R07_MCP_UI_TOKEN;
const external = process.env.LINGXI_R07_MCP_UI_EXTERNAL;

it.skipIf(!base || !token || !external)('真实设置导航经MCP登录按钮到官方SDK，启动、退出及取消都有实际回执', async () => {
  const original = { fetch: globalThis.fetch, platform: window.platform, t: window.t, i18n: window.i18n, store: useSettingsStore.getState() };
  const requests: Array<{ path: string; method: string; status: number }> = [];
  const opened: string[] = [];
  let completeInBrowser = true;
  let pending = 0;
  let actualConnectorStatus: string | undefined;
  try {
    globalThis.fetch = async (input, init) => {
      pending++;
      try {
        const response = await original.fetch(input, init);
        const url = new URL(String(input));
        requests.push({ path: url.pathname, method: init?.method || 'GET', status: response.status });
        if (url.origin === base && url.pathname === '/lingxi/v1/mcp/state' && response.ok) {
          const wire = await response.clone().json();
          actualConnectorStatus = wire.config.connectors.find((row: { id: string }) => row.id === 'owned')?.status;
        }
        return response;
      } finally { pending--; }
    };
    window.t = ((key: string) => key) as typeof window.t;
    window.i18n = { locale: 'en', defaultName: 'Lingxi', _data: {}, _agentOverrides: {}, load: async () => {}, setAgentOverrides: () => {}, t: window.t };
    window.platform = {
      getServerConnectionInfo: async () => ({ port: Number(new URL(base!).port), token, serverNodeKind: 'lingxi-service', serverNodeTransport: 'http' }),
      getPlatform: async () => 'darwin', onSettingsChanged: () => () => {}, onServerRestarted: () => () => {},
      // 只替代系统打开浏览器；实际GET自有授权端并跟随回调，不伪造服务结果。
      openExternal: async (url: string) => {
        expect(new URL(url).origin).toBe(external);
        opened.push(url);
        if (completeInBrowser) expect((await globalThis.fetch(url)).status).toBe(200);
        return { ok: true };
      },
    } as unknown as typeof window.platform;
    localStorage.clear();
    useSettingsStore.setState({ ...useSettingsStore.getInitialState(), activeTab: 'access' }, true);
    // 平台桥先就位，和现役窗口启动顺序相同；不能导入后再替换模块捕获的桥。
    const { SettingsContent } = await import('../../settings/SettingsContent');
    render(<SettingsContent variant="modal" />);
    await waitFor(() => {
      expect(useSettingsStore.getState().ready).toBe(true);
      expect(useSettingsStore.getState().rustSettingsUnavailable).toBe(false);
      expect(useSettingsStore.getState().settingsIdentityOwner).toBeTruthy();
    }, { timeout: 15_000 });
    fireEvent.click(screen.getByRole('button', { name: 'settings.tabs.mcp' }));
    fireEvent.click(await screen.findByRole('button', { name: 'settings.mcp.manage' }, { timeout: 10_000 }));
    fireEvent.click(screen.getByRole('button', { name: 'settings.mcp.oauthConnect' }));
    await screen.findByRole('button', { name: 'settings.oauth.logout' }, { timeout: 15_000 });
    expect(opened).toHaveLength(1);
    expect(requests.some(row => row.path.endsWith('/owned/oauth/start') && row.status === 200)).toBe(true);
    expect(requests.some(row => row.path.startsWith('/lingxi/v1/mcp/oauth/poll/') && row.status === 200)).toBe(true);
    fireEvent.click(screen.getByRole('button', { name: 'settings.mcp.start' }));
    await waitFor(() => {
      expect(actualConnectorStatus).toBe('running');
      expect((screen.getByRole('button', { name: 'settings.mcp.refresh' }) as HTMLButtonElement).disabled).toBe(false);
    }, { timeout: 10_000 });
    expect(requests.some(row => row.path === '/lingxi/v1/mcp/settings-action' && row.status === 200)).toBe(true);
    fireEvent.click(screen.getByRole('button', { name: 'settings.oauth.logout' }));
    await screen.findByRole('button', { name: 'settings.mcp.oauthConnect' }, { timeout: 10_000 });
    expect(requests.some(row => row.path.endsWith('/owned/oauth/logout') && row.status === 200)).toBe(true);
    completeInBrowser = false;
    fireEvent.click(screen.getByRole('button', { name: 'settings.mcp.oauthConnect' }));
    await waitFor(() => expect(opened).toHaveLength(2));
    fireEvent.click(within(screen.getByTestId('mcp-oauth-waiting')).getByRole('button', { name: 'common.cancel' }));
    await waitFor(() => expect(requests.some(row => row.path.endsWith('/owned/oauth/cancel') && row.status === 200)).toBe(true));
    expect(screen.queryByTestId('mcp-oauth-waiting')).toBeNull();
    expect(requests.some(row => row.path.startsWith('/api/'))).toBe(false);
    expect(requests.some(row => row.status >= 400)).toBe(false);
    console.info('MCP实际设置页回执', JSON.stringify({ opens: opened.length, requests }));
  } finally {
    cleanup();
    try { await waitFor(() => expect(pending).toBe(0), { timeout: 10_000 }); }
    finally {
      globalThis.fetch = original.fetch;
      window.platform = original.platform; window.t = original.t; window.i18n = original.i18n;
      useSettingsStore.setState(original.store, true);
    }
  }
}, 60_000);
