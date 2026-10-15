// @vitest-environment jsdom
import React from 'react';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, expect, it } from 'vitest';
import fs from 'node:fs';
import zh from '../../../locales/zh.json';
import { SettingsContent } from '../SettingsContent';
import { useSettingsStore } from '../store';

const enabled = !!process.env.R06_AGENT_DELETE_UI_URL;
afterEach(cleanup);

it.skipIf(!enabled)('rust_agent_delete_current_settings_real_service', async () => {
  const base = process.env.R06_AGENT_DELETE_UI_URL!;
  const token = process.env.R06_AGENT_DELETE_UI_TOKEN!;
  const home = process.env.R06_AGENT_DELETE_UI_HOME!;
  const scenario = process.env.R06_AGENT_DELETE_UI_CASE!;
  const actualFetch = globalThis.fetch;
  const attempts: Array<{ path: string; method: string }> = [];
  const receipts: Array<{ path: string; method: string; status: number; data: any }> = [];
  // 只观察真实HTTP响应，不替换服务、鉴权、路由或业务结果。
  globalThis.fetch = async (input, init) => {
    attempts.push({ path: new URL(String(input)).pathname, method: init?.method || 'GET' });
    const response = await actualFetch(input, init);
    receipts.push({ path: new URL(String(input)).pathname, method: init?.method || 'GET', status: response.status, data: await response.clone().json() });
    return response;
  };
  const translate = (key: string, params?: Record<string, string>) => {
    const value = key.split('.').reduce<any>((value, part) => value?.[part], zh);
    return typeof value === 'string' ? value.replace(/\{([^}]+)\}/g, (_match, name) => params?.[name] || '') : key;
  };
  window.t = translate as typeof window.t;
  window.i18n = { locale: 'zh-CN', defaultName: '灵犀', _data: zh, _agentOverrides: {}, load: async () => {}, setAgentOverrides: () => {}, t: window.t };
  // 合成桌面桥只提供本次真实Rust服务连接，其余业务调用沿当前SettingsContent入口。
  window.platform = {
    getServerConnectionInfo: async () => ({ port: Number(new URL(base).port), token, serverNodeKind: 'lingxi-service', serverNodeTransport: 'http' }),
    getPlatform: async () => 'darwin', onSettingsChanged: () => () => {}, onServerRestarted: () => () => {},
  } as unknown as typeof window.platform;
  localStorage.clear();
  useSettingsStore.setState({ activeTab: 'access', ready: false, rustSettingsUnavailable: false, activeServerConnection: null,
    activeServerConnectionId: null, serverConnections: {}, agents: [], settingsAgentId: null, currentAgentId: null, toastVisible: false });
  const mutation = async (path: string, method: string, body?: unknown) => {
    const response = await actualFetch(`${base}${path}`, { method, headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' }, body: body ? JSON.stringify(body) : undefined });
    expect(response.ok).toBe(true);
  };
  let restoreSkillMode: number | undefined;
  try {
    render(<SettingsContent variant="modal" />);
    await waitFor(() => expect(useSettingsStore.getState().ready).toBe(true));
    fireEvent.click(screen.getByRole('button', { name: '助手' }));
    await waitFor(() => expect(screen.getByRole('combobox', { name: '助手' })).toBeEnabled());
    fireEvent.change(screen.getByRole('combobox', { name: '助手' }), { target: { value: 'victim' } });
    if (scenario === 'guard-registry-fallback') {
      const previous = useSettingsStore.getState().activeServerConnection!;
      // 先让影子对象指向另一服务；预览仍必须沿登记表解析到原服务。
      act(() => useSettingsStore.setState({ activeServerConnection: { ...previous, baseUrl: process.env.R06_AGENT_DELETE_UI_OTHER_URL!, token: process.env.R06_AGENT_DELETE_UI_OTHER_TOKEN! } }));
    }
    fireEvent.click(screen.getByRole('button', { name: '删除助手' }));
    await screen.findByRole('heading', { name: '删除 待删除助手？' });
    fireEvent.click(screen.getByRole('button', { name: '继续' }));
    await screen.findByRole('heading', { name: '同时删除技能？' });
    expect(screen.getByRole('checkbox', { name: /demo-owned/ })).toBeChecked();
    fireEvent.click(screen.getByRole('checkbox', { name: /demo-keep/ }));
    // 预览后改变候选或造成真实文件系统删除故障，实际DELETE必须再核对。
    if (scenario === 'changed') await mutation('/lingxi/v1/agents/survivor/config', 'PUT', { skills: { enabled: ['demo-owned'] } });
    if (scenario === 'failed') {
      restoreSkillMode = fs.statSync(`${home}/skills/demo-owned`).mode;
      fs.chmodSync(`${home}/skills/demo-owned`, 0o555);
    }
    if (scenario === 'last') await mutation('/lingxi/v1/agents/survivor', 'DELETE');
    fireEvent.click(screen.getByRole('button', { name: '删除 1 个技能并继续' }));
    fireEvent.change(screen.getByPlaceholderText('输入助手名字'), { target: { value: '待删除助手' } });
    if (scenario.startsWith('guard-')) {
      const previous = useSettingsStore.getState().activeServerConnection!;
      const next = { ...previous };
      switch (scenario) {
        case 'guard-node-new-id': next.connectionId = 'different-node'; next.serverNodeKind = 'node'; break;
        case 'guard-node-same-id': next.serverNodeKind = 'node'; break;
        case 'guard-rust-endpoint': next.baseUrl = process.env.R06_AGENT_DELETE_UI_OTHER_URL!; next.token = process.env.R06_AGENT_DELETE_UI_OTHER_TOKEN!; break;
        case 'guard-transport': next.serverNodeTransport = 'https'; break;
        case 'guard-instance': next.serverNodeId = 'different-instance'; break;
        case 'guard-kind': next.kind = 'custom_remote'; break;
        case 'guard-token': next.token = 'SYNTHETIC_CHANGED_TOKEN'; break;
        case 'guard-roundtrip': next.connectionId = 'temporarily-other'; break;
        case 'guard-registry-endpoint':
        case 'guard-registry-roundtrip':
        case 'guard-registry-source':
        case 'guard-registry-unrelated':
        case 'guard-registry-fallback':
          next.baseUrl = process.env.R06_AGENT_DELETE_UI_OTHER_URL!; next.token = process.env.R06_AGENT_DELETE_UI_OTHER_TOKEN!; break;
        case 'guard-registry-stable':
        case 'guard-stable': break;
        default: throw new Error('unknown synthetic connection scenario');
      }
      act(() => {
        const source = useSettingsStore.getState();
        if (scenario === 'guard-registry-fallback') {
          const registry = { ...source.serverConnections }; delete registry[source.activeServerConnectionId!];
          useSettingsStore.setState({ serverConnections: registry });
        } else if (scenario === 'guard-registry-source') {
          useSettingsStore.setState({ activeServerConnectionId: 'other-registry-source', serverConnections: { ...source.serverConnections, 'other-registry-source': { ...next, connectionId: 'other-registry-source' } } });
        } else if (scenario === 'guard-registry-unrelated') {
          useSettingsStore.setState({ serverConnections: { ...source.serverConnections, 'unselected-entry': { ...next, connectionId: 'unselected-entry' } } });
        } else if (scenario.startsWith('guard-registry-')) {
          useSettingsStore.setState({ serverConnections: { ...source.serverConnections, [source.activeServerConnectionId!]: next } });
          if (scenario === 'guard-registry-roundtrip') useSettingsStore.setState({ serverConnections: source.serverConnections });
        } else {
          // 更新真实请求采用的登记项，不能只改被登记表遮蔽的影子对象冒称换了端点。
          useSettingsStore.setState({ activeServerConnectionId: next.connectionId, activeServerConnection: next, serverConnections: { ...source.serverConnections, [next.connectionId]: next } });
          if (scenario === 'guard-roundtrip') useSettingsStore.setState({ activeServerConnectionId: previous.connectionId, activeServerConnection: previous, serverConnections: source.serverConnections });
        }
      });
      fireEvent.click(screen.getByRole('button', { name: '永久删除' }));
      if (!['guard-stable', 'guard-registry-stable', 'guard-registry-unrelated'].includes(scenario)) {
        await waitFor(() => expect(screen.getAllByRole('alert').some(alert => alert.textContent?.includes('连接已改变'))).toBe(true));
        expect(attempts.filter(item => item.method === 'DELETE')).toEqual([]);
        expect(receipts.filter(item => item.method === 'DELETE')).toEqual([]);
        expect(useSettingsStore.getState().toastType).toBe('error');
        return;
      }
    } else {
    fireEvent.click(screen.getByRole('button', { name: '永久删除' }));
    }
    const complete = ['complete', 'guard-stable', 'guard-registry-stable', 'guard-registry-unrelated'].includes(scenario);
    await waitFor(() => expect(receipts.some(item => item.path === '/lingxi/v1/agents/victim' && item.method === 'DELETE')).toBe(true));
    const receipt = receipts.find(item => item.path === '/lingxi/v1/agents/victim' && item.method === 'DELETE')!;
    expect(receipts.every(item => !item.path.startsWith('/api/'))).toBe(true);
    if (scenario === 'last') {
      expect(receipt.status).toBe(400);
      await waitFor(() => expect(useSettingsStore.getState().toastType).toBe('error'));
      expect(useSettingsStore.getState().toastMessage).toContain('删除失败');
      expect(screen.getAllByRole('alert').some(alert => alert.textContent?.includes('cannot delete the last agent'))).toBe(true);
    } else {
      expect(receipt.status).toBe(200);
      expect(receipt.data.skillsDeleted).toEqual(complete ? ['demo-owned'] : []);
      expect(receipt.data.skillsSkipped).toEqual(complete ? [] : [{ name: 'demo-owned', reason: scenario === 'changed' ? 'not_orphan' : 'removal_failed' }]);
      await waitFor(() => expect(useSettingsStore.getState().toastType).toBe('success'));
      expect(useSettingsStore.getState().toastMessage).toBe(complete ? '待删除助手 已删除，并删除 1 个技能' : '待删除助手 已删除');
      await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent(complete ? '待删除助手 已删除，并删除 1 个技能' : '待删除助手 已删除'));
      expect(screen.queryByText(/not_orphan|removal_failed/)).not.toBeInTheDocument();
      await waitFor(() => expect(useSettingsStore.getState().agents.map(agent => agent.id)).not.toContain('victim'));
    }
    // 当前桌面入口仅此助手页接通，其他Rust设置仍明确不可用。
    fireEvent.click(screen.getByRole('button', { name: '技能' }));
    expect(screen.getAllByRole('alert').some(alert => alert.textContent?.includes(translate('settings.rustCoreUnavailable')))).toBe(true);
  } finally {
    if (restoreSkillMode !== undefined) fs.chmodSync(`${home}/skills/demo-owned`, restoreSkillMode);
    globalThis.fetch = actualFetch;
  }
});
