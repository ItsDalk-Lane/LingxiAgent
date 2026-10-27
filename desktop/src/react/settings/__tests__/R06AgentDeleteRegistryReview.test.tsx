// @vitest-environment jsdom
import React from 'react';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, expect, it } from 'vitest';
import fs from 'node:fs';
import zh from '../../../locales/zh.json';
import { SettingsContent } from '../SettingsContent';
import { useSettingsStore } from '../store';
import { observeAgentDeleteInitialization } from './agent-delete-initialization';

const enabled = !!process.env.R06_AGENT_DELETE_UI_URL;
afterEach(cleanup);

it.skipIf(!enabled)('rust_agent_delete_registry_only_counterexample', async () => {
  const testDeadline = performance.now() + 60_000;
  const base = process.env.R06_AGENT_DELETE_UI_URL!;
  const token = process.env.R06_AGENT_DELETE_UI_TOKEN!;
  const home = process.env.R06_AGENT_DELETE_UI_HOME!;
  const scenario = process.env.R06_AGENT_DELETE_UI_CASE!;
  const actualFetch = globalThis.fetch;
  const initialization = observeAgentDeleteInitialization({ actualFetch, base, token, home, testDeadline });
  const receipts: Array<{ url: string; path: string; method: string; status: number; data: any }> = [];
  // 只观察真实HTTP响应，不替换服务、鉴权、路由或业务结果。
  globalThis.fetch = async (input, init) => {
    const response = await initialization.fetch(input, init);
    receipts.push({ url: String(input), path: new URL(String(input)).pathname, method: init?.method || 'GET', status: response.status, data: await response.clone().json().catch(() => null) });
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
    await initialization.waitForReady();
    fireEvent.click(screen.getByRole('button', { name: '助手' }));
    await waitFor(() => expect(screen.getByRole('combobox', { name: '助手' })).toBeEnabled());
    fireEvent.change(screen.getByRole('combobox', { name: '助手' }), { target: { value: 'victim' } });
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
    const previousConnection = useSettingsStore.getState().activeServerConnection!;
    const nextConnection = { ...previousConnection, baseUrl: process.env.R06_AGENT_DELETE_UI_OTHER_URL!, token: process.env.R06_AGENT_DELETE_UI_OTHER_TOKEN! };
    useSettingsStore.setState({ serverConnections: { ...useSettingsStore.getState().serverConnections, [previousConnection.connectionId]: nextConnection } });
    await new Promise(resolve => setTimeout(resolve, 30));
    fireEvent.click(screen.getByRole('button', { name: '永久删除' }));
    await new Promise(resolve => setTimeout(resolve, 300));
    console.error('INDEPENDENT_DELETE_RECEIPTS', JSON.stringify(receipts.filter(item => item.method === 'DELETE')));
    expect(receipts.filter(item => item.method === 'DELETE'), JSON.stringify(receipts.filter(item => item.method === 'DELETE'))).toEqual([]);
    expect(screen.getAllByRole('alert').some(alert => alert.textContent?.includes('连接已改变'))).toBe(true);
    return;
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
      expect(receipt.data.skillsDeleted).toEqual(scenario === 'complete' ? ['demo-owned'] : []);
      expect(receipt.data.skillsSkipped).toEqual(scenario === 'complete' ? [] : [{ name: 'demo-owned', reason: scenario === 'changed' ? 'not_orphan' : 'removal_failed' }]);
      await waitFor(() => expect(useSettingsStore.getState().toastType).toBe('success'));
      expect(useSettingsStore.getState().toastMessage).toBe(scenario === 'complete' ? '待删除助手 已删除，并删除 1 个技能' : '待删除助手 已删除');
      await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent(scenario === 'complete' ? '待删除助手 已删除，并删除 1 个技能' : '待删除助手 已删除'));
      expect(screen.queryByText(/not_orphan|removal_failed/)).not.toBeInTheDocument();
      await waitFor(() => expect(useSettingsStore.getState().agents.map(agent => agent.id)).not.toContain('victim'));
    }
    // T08技能页须沿真实服务加载；连接守卫的提前返回仍保留原负例，不借此段领通过。
    fireEvent.click(screen.getByRole('button', { name: '技能' }));
    await waitFor(() => expect(document.querySelector('div[data-tab="skills"]:not([data-settings-page])')).not.toBeNull());
    expect(screen.queryAllByRole('alert').some(alert => alert.textContent?.includes(translate('settings.rustCoreUnavailable')))).toBe(false);
  } finally {
    if (restoreSkillMode !== undefined) fs.chmodSync(`${home}/skills/demo-owned`, restoreSkillMode);
    globalThis.fetch = actualFetch;
    initialization.dispose();
  }
});
