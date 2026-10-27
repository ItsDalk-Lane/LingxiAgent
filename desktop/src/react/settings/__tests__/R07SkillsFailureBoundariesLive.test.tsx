// @vitest-environment jsdom
import React from 'react';
import fs from 'node:fs';
import { afterEach, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { SkillsTab } from '../tabs/SkillsTab';
import { SkillsPanel } from '../../components/SkillsPanel';
import { useSettingsStore } from '../store';
import { useStore } from '../../stores';
import { createDeviceServerConnection, createLocalServerConnection, type ServerIdentity } from '../../services/server-connection';
import { installWindowTestT } from '../../__tests__/helpers/i18n-test-strings';
import { captureSettingsConnection } from '../connection-owner';

type Surface = 'settings' | 'panel';
type Bundle = { id: string; name: string; skillNames: string[] };
type RequestFact = { path: string; method: string; status: number; restrictedInput: boolean };
const skillPath = '/lingxi/v1/skills';
const bundlePath = '/lingxi/v1/skills/bundles';
const subscriptions: (() => void)[] = [];
const waitOptions = { timeout: 15_000 };
const loadFailed = 'Could not load skills. Please try again.';

afterEach(() => {
  cleanup();
  subscriptions.splice(0).forEach(stop => stop());
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function setup(surface: Surface) {
  const skillName = `${surface}-boundary-skill`;
  const base = process.env.R07_SKILLS_URL!;
  const token = process.env.R07_SKILLS_TOKEN!;
  const restricted = process.env.R07_SKILLS_CHAT_ONLY!;
  expect(base).toMatch(/^http:\/\/127\.0\.0\.1:/);
  expect(restricted).toBeTruthy();
  const originalFetch = globalThis.fetch;
  const requests: RequestFact[] = [];
  let failPath: string | null = null;
  // 只更换本次请求的真实凭证输入；不替换响应，不更改页面连接所代表的身份。
  vi.stubGlobal('fetch', async (input: RequestInfo | URL, init?: RequestInit) => {
    const path = new URL(String(input)).pathname;
    const method = init?.method || 'GET';
    const restrictedInput = method === 'GET' && path === failPath;
    const headers = new Headers(init?.headers);
    if (restrictedInput) headers.set('Authorization', `Bearer ${restricted}`);
    const response = await originalFetch(input, { ...init, headers });
    requests.push({ path, method, status: response.status, restrictedInput });
    return response;
  });
  installWindowTestT({ 'settings.skills.loadFailed': loadFailed });
  window.i18n = { locale: 'en' } as typeof window.i18n;
  window.platform = { selectSkill: async () => null, selectFolder: async () => null } as never;
  const connection = createLocalServerConnection({ serverPort: new URL(base).port, serverToken: token, serverNodeKind: 'lingxi-service', serverNodeTransport: 'http' })!;
  const agents = [{ id: 'agentA', name: '助手A', yuan: 'lingxi', isPrimary: true }];
  useSettingsStore.setState({ serverConnections: { local: connection }, activeServerConnectionId: 'local', activeServerConnection: connection });
  useSettingsStore.setState({ agents, currentAgentId: 'agentA', settingsAgentId: 'agentA', settingsAgentsOwner: captureSettingsConnection(), settingsDataOwner: captureSettingsConnection(), settingsDataAgentId: 'agentA', settingsConfigStatus: 'ready', settingsConfig: { capabilities: { learn_skills: { enabled: true, safety_review: true } } }, ready: true });
  useStore.setState({ serverConnections: { local: connection }, activeServerConnectionId: 'local', activeServerConnection: connection, activePanel: 'skills', currentAgentId: 'agentA', agents, agentName: '助手A', agentYuan: 'lingxi', toasts: [] } as never);
  const notices: { message: string; type?: string }[] = [];
  // 只读观察真实通知；不替换Zustand里的方法，避免跨用例复制spy改变执行链。
  subscriptions.push(useSettingsStore.subscribe((state, previous) => {
    if (state.toastMessage !== previous.toastMessage || state.toastType !== previous.toastType) {
      notices.push({ message: state.toastMessage, type: state.toastType });
    }
  }));
  subscriptions.push(useStore.subscribe((state, previous) => {
    for (const toast of state.toasts) {
      if (!previous.toasts.some(row => row.id === toast.id)) notices.push({ message: toast.text, type: toast.type });
    }
  }));
  const ownerRequest = async (method: string, path: string, body?: unknown) => {
    const response = await originalFetch(base + path, { method, headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
    expect(response.ok, `${method} ${path}: ${response.status}`).toBe(true);
    return response.json();
  };
  const mount = () => render(surface === 'settings' ? <SkillsTab /> : <SkillsPanel />);
  const reload = async () => {
    if (surface === 'settings') act(() => window.dispatchEvent(new CustomEvent('hana-skills-changed')));
    else {
      act(() => useStore.setState({ activePanel: null }));
      act(() => useStore.setState({ activePanel: 'skills' }));
    }
  };
  const saveReceipt = (name: string, facts: unknown) => fs.appendFileSync(process.env.R07_SKILLS_RECEIPT!, JSON.stringify({ name, surface, facts, requests }) + '\n');
  const restrictedConnection = async () => {
    const response = await originalFetch(base + '/lingxi/v1/server/identity', { headers: { Authorization: `Bearer ${restricted}` } });
    expect(response.ok).toBe(true);
    const identity = await response.json() as ServerIdentity;
    // 设备连接按手动连接的设备类型展示；TCP仍是本夹具loopback，不冒称LAN实测。
    return createDeviceServerConnection({ baseUrl: base, credential: restricted, identity: { ...identity, connectionKind: 'lan' } });
  };
  return { skillName, connection, requests, notices, ownerRequest, mount, reload, saveReceipt, restrictedConnection, setFailure: (path: string | null) => { failPath = path; } };
}

for (const surface of ['settings', 'panel'] as const) {
  for (const [kind, path] of [['skills', skillPath], ['bundles', bundlePath]] as const) {
    it(`${surface}_first_${kind}_get_real_scope_denial_is_visible_not_empty_success`, async () => {
      const h = setup(surface);
      const skillName = h.skillName;
      h.setFailure(path);
      h.mount();
      const alert = await screen.findByRole('alert', {}, waitOptions);
      expect(alert.textContent).toBe(loadFailed);
      const area = surface === 'settings' ? alert.closest('section')! : alert.parentElement!;
      expect(within(area).queryByText('settings.skills.noUser')).toBeNull();
      if (surface === 'settings') expect(within(screen.getByRole('region', { name: 'settings.skills.userSkillsTitle' })).queryByText('settings.skills.noUser')).toBeNull();
      expect(screen.queryByText(skillName)).toBeNull();
      await waitFor(() => expect(h.requests.some(r => r.path === path && r.restrictedInput && r.status === 403)).toBe(true), waitOptions);
      expect(h.requests.filter(r => r.method !== 'GET')).toEqual([]);
      expect(h.notices.some(n => n.type === 'success')).toBe(false);
      h.saveReceipt(`${surface}-initial-${kind}-403`, { visibleError: true, noFalseEmpty: true, zeroWrites: true });
    });
  }

  it(`${surface}_same_owner_get_failures_keep_old_lists_and_two_bundle_writes_preserve_real_partial_result`, async () => {
    const h = setup(surface);
    const skillName = h.skillName;
    const area = () => surface === 'settings' ? screen.getByRole('region', { name: 'settings.skills.manageTitle' }) : document.getElementById('skillsPanel')!;
    const bundleHeader = (id: string) => within(area()).getByTestId(`skill-bundle-header-${id}`);
    const a: Bundle = (await h.ownerRequest('POST', `${bundlePath}?agentId=agentA`, { name: `${surface}-source`, skillNames: [skillName] })).bundle;
    const b: Bundle = (await h.ownerRequest('POST', `${bundlePath}?agentId=agentA`, { name: `${surface}-target`, skillNames: [] })).bundle;
    h.mount();
    await waitFor(() => expect(bundleHeader(a.id)).toBeTruthy(), waitOptions);
    fireEvent.click(within(bundleHeader(a.id)).getByRole('button', { name: 'settings.skills.expandBundleAriaLabel' }));
    await waitFor(() => expect(screen.getAllByText(skillName).length).toBeGreaterThan(0), waitOptions);
    for (const path of [skillPath, bundlePath]) {
      const before = h.requests.length;
      h.setFailure(path);
      await h.reload();
      await screen.findByRole('alert', {}, waitOptions);
      await waitFor(() => expect(h.requests.slice(before).some(r => r.path === path && r.status === 403 && r.restrictedInput)).toBe(true), waitOptions);
      expect(bundleHeader(a.id)).toBeTruthy();
      expect(bundleHeader(b.id)).toBeTruthy();
      expect(screen.getAllByText(skillName).length).toBeGreaterThan(0);
      expect(useSettingsStore.getState().activeServerConnection === h.connection).toBe(true);
      expect(useStore.getState().activeServerConnection === h.connection).toBe(true);
      h.setFailure(null);
      await h.reload();
      await waitFor(() => expect(screen.queryByRole('alert')).toBeNull(), waitOptions);
    }
    expect(h.requests.filter(r => r.method !== 'GET')).toEqual([]);
    // 外部真实删除目标，页面保留旧视图；两笔PUT分别落盘成功和返回不存在。
    await h.ownerRequest('DELETE', `${bundlePath}/${b.id}`);
    expect(bundleHeader(b.id)).toBeTruthy();
    const beforeMove = h.requests.length;
    h.notices.length = 0;
    fireEvent.drop(bundleHeader(b.id), { dataTransfer: { getData: (type: string) => type === 'application/x-lingxi-skill-name' ? skillName : '', types: ['application/x-lingxi-skill-name'] } });
    await waitFor(() => expect(h.requests.slice(beforeMove).filter(r => r.method === 'PUT')).toHaveLength(2), waitOptions);
    const writes = h.requests.slice(beforeMove).filter(r => r.method === 'PUT');
    expect(writes.find(r => r.path === `${bundlePath}/${a.id}`)?.status).toBe(200);
    expect(writes.find(r => r.path === `${bundlePath}/${b.id}`)?.status).toBe(404);
    await waitFor(() => expect(h.notices.some(n => n.type === 'error' && n.message.includes(surface === 'settings' ? 'skill bundle not found' : '404'))).toBe(true), waitOptions);
    expect(h.notices.some(n => n.type === 'success')).toBe(false);
    const readback = (await h.ownerRequest('GET', `${bundlePath}?agentId=agentA`)).bundles as Bundle[];
    expect(readback.find(row => row.id === a.id)?.skillNames).toEqual([]);
    expect(readback.some(row => row.id === b.id)).toBe(false);
    const disk = JSON.parse(fs.readFileSync(process.env.R07_SKILLS_BUNDLE_FILE!, 'utf8')) as { bundles: Bundle[] };
    expect(disk.bundles.find(row => row.id === a.id)?.skillNames).toEqual([]);
    expect(disk.bundles.some(row => row.id === b.id)).toBe(false);
    // 确实换成另一授权连接后，旧列表不再属于当前身份，不能沿用“失败保留”规则。
    const other = await h.restrictedConnection();
    act(() => {
      if (surface === 'settings') useSettingsStore.setState({ serverConnections: { local: other }, activeServerConnection: other });
      else useStore.setState({ serverConnections: { local: other }, activeServerConnection: other });
    });
    await screen.findByRole('alert', {}, waitOptions);
    expect(within(area()).queryByTestId(`skill-bundle-header-${a.id}`)).toBeNull();
    expect(screen.queryByText(skillName)).toBeNull();
    h.saveReceipt(`${surface}-old-lists-and-partial-write`, { a: a.id, b: b.id, sourceCommitted: true, deletedTargetAbsent: true, writes, diskVerified: true, errorVisible: true, changedConnectionHidesOldData: true });
  });
}
