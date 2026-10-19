/** @vitest-environment jsdom */
import React from 'react';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { spawn } from 'node:child_process';
import { mkdtemp, mkdir, readFile, realpath, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { expect, it, vi } from 'vitest';
import { SettingsContent } from '../../settings/SettingsContent';
import { useSettingsStore } from '../../settings/store';
import { ownsSettingsConnection } from '../../settings/connection-owner';
import { lingxiFetch } from '../../settings/api';

const binary = process.env.LINGXI_R07_SERVICE_BIN;

it.skipIf(!binary).each(['configured', 'without-model', 'without-agent', 'restart', 'rejected-restart', 'invalid-resource-credential', 'provider-refresh-failure', 'all-pages'] as const)('真实设置容器的导航、恢复及失败处理（%s）', async scenario => {
  const scratch = await realpath(await mkdtemp(join(tmpdir(), 'lingxi-r07-settings-entry-')));
  const home = join(scratch, 'home');
  const workspace = join(scratch, 'workspace');
  const previous = useSettingsStore.getState();
  const previousPlatform = window.platform;
  const previousI18n = window.i18n;
  const previousT = window.t;
  const actualFetch = globalThis.fetch;
  const requests: Array<{ path: string; method: string; status: number }> = [];
  let pendingRequests = 0;
  let damageAfterProviderSave = false;
  let preservedAgentConfig: Buffer | undefined;
  let delayNextInvalidModel = false;
  let lateResponseGate: Promise<void> | undefined;
  let releaseLate401: (() => void) | undefined;
  let pendingLate401: Promise<unknown> | undefined;
  let exported: Blob | undefined;
  const originalCreateUrl = URL.createObjectURL;
  const originalRevokeUrl = URL.revokeObjectURL;
  const anchorClick = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => {});
  let child: ReturnType<typeof spawn> | undefined;
  let exited: Promise<number | null> | undefined;
  let restarted: ((data: { port: number; token: string; serverNodeKind: string; serverNodeTransport: string }) => void) | undefined;
  try {
    await Promise.all([mkdir(home), mkdir(workspace)]);
    const config = join(scratch, 'config.json');
    await writeFile(config, JSON.stringify({ home, workspace, ...(scenario === 'without-model' ? {} : { providers: {
      'entry-fixture': { protocol: 'openai-completions', endpoint: 'http://127.0.0.1:9/v1', auth: { kind: 'none' }, models: [{ id: 'entry-model', capabilities: { tools: true } }] },
    }, models: { chat: { provider: 'entry-fixture', model: 'entry-model', capabilities: { tools: true } } } }) }));
    // 正式服务必须从它自己的全新资料启动，不能继承工作电脑的用户目录。
    const env = { ...process.env }; delete env.LINGXI_HOME;
    const boot = async () => {
      child = spawn(binary!, ['--config', config, '--bind', '127.0.0.1:0'], { env, stdio: ['ignore', 'pipe', 'pipe'] });
      exited = new Promise(resolve => child!.once('exit', resolve));
      return await new Promise<string>((resolve, reject) => {
      let output = '';
      const timer = setTimeout(() => reject(new Error('隔离服务未在30秒内启动')), 30_000);
      child!.once('error', error => { clearTimeout(timer); reject(error); });
      child!.once('exit', code => { clearTimeout(timer); reject(new Error(`隔离服务提前退出：${code}`)); });
      child!.stderr!.on('data', () => {});
      child!.stdout!.on('data', data => {
        output += String(data);
        const found = output.match(/LINGXI_SERVICE_READY addr=(\S+)/);
        if (found) { clearTimeout(timer); resolve(found[1]); }
      });
      });
    };
    const address = await boot();
    const { token } = JSON.parse(await readFile(join(home, 'lingxi-service/local-token.json'), 'utf8'));
    const base = `http://${address}`;
    if (scenario === 'configured') {
      const preferences = join(home, 'user/preferences.json');
      await mkdir(join(home, 'user'), { recursive: true });
      const original = await readFile(preferences, 'utf8').then(JSON.parse, error => {
        if (error.code === 'ENOENT') return {};
        throw error;
      });
      await writeFile(preferences, JSON.stringify({ ...original, future: { keep: 37 },
        search_provider: 'tavily', search_api_keys: { tavily: 'synthetic-entry-secret' } }));
    }
    if (scenario !== 'without-agent') {
      const created = await actualFetch(`${base}/lingxi/v1/agents`, {
        method: 'POST', headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
        body: JSON.stringify({ id: 'entry-agent', name: '入口助手' }),
      });
      expect(created.status).toBe(200);
      expect(await created.json()).toMatchObject({ id: 'entry-agent', name: '入口助手' });
    }
    // 只观察真实请求的地址与状态；不代填初始化资料、不替换服务响应。
    globalThis.fetch = async (input, init) => {
      pendingRequests++;
      try {
        const delayedResponse = delayNextInvalidModel && new URL(String(input)).pathname === '/lingxi/v1/models';
        if (delayedResponse) delayNextInvalidModel = false;
        if ((scenario === 'invalid-resource-credential' || delayedResponse) && new URL(String(input)).pathname === '/lingxi/v1/models') {
          // 损坏本次请求的凭据，服务自行返回401；不伪造响应，也不修改连接坐标。
          const headers = new Headers(init?.headers); headers.set('Authorization', 'Bearer synthetic-invalid-entry-credential');
          init = { ...init, headers };
        }
        const response = await actualFetch(input, init);
        const path = new URL(String(input)).pathname;
        requests.push({ path, method: init?.method || 'GET', status: response.status });
        if (delayedResponse) await lateResponseGate;
        if (damageAfterProviderSave && path === '/lingxi/v1/settings/config' && init?.method === 'PUT' && response.status === 200) {
          damageAfterProviderSave = false;
          const agentConfig = join(home, 'agents/entry-agent/config.yaml');
          preservedAgentConfig = await readFile(agentConfig);
          // 保存实际成功后损坏隔离助手资料，下一次真实读取必须失败。
          await writeFile(agentConfig, 'memory: [invalid');
        }
        return response;
      } finally { pendingRequests--; }
    };
    window.t = ((key: string) => key) as typeof window.t;
    window.i18n = { locale: 'en', defaultName: 'Lingxi', _data: {}, _agentOverrides: {}, load: async () => {}, setAgentOverrides: () => {}, t: window.t };
    window.platform = {
      getServerConnectionInfo: async () => ({ port: Number(new URL(base).port), token, serverNodeKind: 'lingxi-service', serverNodeTransport: 'http' }),
      getPlatform: async () => 'darwin', onSettingsChanged: () => () => {},
      onServerRestarted: (callback: typeof restarted) => { restarted = callback; return () => { restarted = undefined; }; },
    } as unknown as typeof window.platform;
    localStorage.clear();
    useSettingsStore.setState({ ...useSettingsStore.getInitialState(), activeTab: 'access' }, true);
    render(<SettingsContent variant="modal" />);
    if (scenario === 'invalid-resource-credential') {
      await waitFor(() => {
        expect(requests.some(request => request.path === '/lingxi/v1/models' && request.status === 401)).toBe(true);
        expect(useSettingsStore.getState().ready).toBe(true);
        expect(useSettingsStore.getState().settingsIdentityOwner).toBeNull();
        expect(useSettingsStore.getState().rustSettingsUnavailable).toBe(true);
      }, { timeout: 10_000 });
      fireEvent.click(screen.getByRole('button', { name: 'settings.tabs.general' }));
      expect(document.querySelector('div[data-tab="general"]:not([data-settings-page])')).toBeNull();
      expect(screen.getAllByRole('alert').some(alert => alert.textContent?.includes('settings.rustCoreUnavailable'))).toBe(true);
      return;
    }
    await waitFor(() => {
      const state = useSettingsStore.getState();
      expect(state.ready).toBe(true);
      expect(ownsSettingsConnection(state.settingsIdentityOwner)).toBe(true);
      expect(state.rustSettingsUnavailable).toBe(false);
      if (scenario !== 'without-agent') {
        expect(state.settingsConfigStatus).toBe('ready');
        expect(state.settingsSnapshot.status).toBe('ready');
        expect(state.settingsDataAgentId).toBe('entry-agent');
        expect(ownsSettingsConnection(state.settingsDataOwner)).toBe(true);
        expect(state.agentsError || state.settingsConfigError).toBeNull();
        if (scenario !== 'without-model') expect(state.runtimeModelsError).toBeNull();
        else expect(state.runtimeModelsError).toBeTruthy();
      } else expect(state.agents).toEqual([]);
    }, { timeout: 14_500 });
    expect(requests.some(request => request.path === '/lingxi/v1/server/identity' && request.status === 200)).toBe(true);
    const blocked: string[] = [];
    const tabs = scenario === 'all-pages'
      ? ['agent', 'access', 'me', 'interface', 'keybindings', 'general', 'browser', 'work', 'skills', 'mcp', 'bridge', 'providers', 'models', 'sharing', 'experiments', 'security', 'about']
      : ['general', 'providers', 'skills'];
    for (const tab of tabs) {
      fireEvent.click(screen.getByRole('button', { name: `settings.tabs.${tab}` }));
      if (screen.queryAllByRole('alert').some(alert => alert.textContent?.includes('settings.rustCoreUnavailable'))) blocked.push(tab);
      await waitFor(() => {
        if (tab === 'experiments') {
          // 实验页使用Fragment，核对现役页体内容，不能把外层空包装算页面已打开。
          const page = document.querySelector('[data-settings-page][data-tab="experiments"]');
          expect(page?.children.length, tab).toBeGreaterThan(0);
          expect(page?.textContent?.trim().length, tab).toBeGreaterThan(0);
        } else expect(document.querySelector(`div[data-tab="${tab}"]:not([data-settings-page])`), tab).not.toBeNull();
      }, { timeout: 10_000 });
      if (tab === 'general') expect(screen.getByRole('button', { name: 'settings.transfer.export' })).toBeTruthy();
      if (scenario === 'without-model') expect(screen.getAllByRole('alert').some(alert => alert.textContent?.includes(useSettingsStore.getState().runtimeModelsError!))).toBe(true);
      await waitFor(() => expect(pendingRequests).toBe(0), { timeout: 10_000 });
    }
    console.error('R07_SETTINGS_ENTRY_RESULT', JSON.stringify({ blocked, requests }));
    expect(blocked).toEqual([]);
    if (scenario === 'all-pages') {
      expect(requests.filter(request => request.path.startsWith('/api/') || [404, 405].includes(request.status))).toEqual([]);
    }
    if (scenario === 'provider-refresh-failure') {
      fireEvent.click(screen.getByRole('button', { name: 'settings.tabs.providers' }));
      await waitFor(() => expect(useSettingsStore.getState().providersSummary['entry-fixture']).toBeTruthy());
      fireEvent.click(screen.getByRole('button', { name: /^entry-fixture/ }));
      const input = await screen.findByPlaceholderText('https://api.example.com/v1');
      fireEvent.change(input, { target: { value: 'http://127.0.0.1:9/entry-saved/v1' } });
      fireEvent.blur(input);
      // 此服务从显式配置文件启动，模型修改应写回同一来源。
      const source = config;
      await waitFor(async () => {
        expect(JSON.parse(await readFile(source, 'utf8')).providers['entry-fixture'].endpoint).toBe('http://127.0.0.1:9/entry-saved/v1');
        expect(useSettingsStore.getState().toastMessage).toBe('settings.saved');
        expect(useSettingsStore.getState().settingsConfigStatus).toBe('ready');
      }, { timeout: 10_000 });
      await waitFor(() => expect(pendingRequests).toBe(0));
      const beforeFailure = requests.length;
      damageAfterProviderSave = true;
      fireEvent.change(input, { target: { value: 'http://127.0.0.1:9/entry-kept/v1' } });
      fireEvent.blur(input);
      await waitFor(() => {
        expect(useSettingsStore.getState().toastMessage).toContain('settings.refreshFailed');
        expect(useSettingsStore.getState().toastType).toBe('error');
        expect(useSettingsStore.getState().settingsConfigStatus).toBe('error');
      }, { timeout: 10_000 });
      await waitFor(() => expect(pendingRequests).toBe(0));
      expect(JSON.parse(await readFile(source, 'utf8')).providers['entry-fixture'].endpoint).toBe('http://127.0.0.1:9/entry-kept/v1');
      const failureRequests = requests.slice(beforeFailure);
      expect(failureRequests.some(request => request.path === '/lingxi/v1/settings/config' && request.method === 'PUT' && request.status === 200)).toBe(true);
      expect(failureRequests.some(request => request.path === '/lingxi/v1/agents/entry-agent/config' && request.status === 500)).toBe(true);
      expect(failureRequests.filter(request => request.path === '/lingxi/v1/providers/summary' || request.path.startsWith('/lingxi/v1/media/'))).toEqual([]);
    }
    if (scenario === 'configured') {
      fireEvent.click(screen.getByRole('button', { name: 'settings.tabs.general' }));
      URL.createObjectURL = vi.fn(blob => { exported = blob as Blob; return 'blob:settings-entry-owned'; });
      URL.revokeObjectURL = vi.fn();
      fireEvent.click(screen.getByRole('button', { name: 'settings.transfer.export' }));
      await waitFor(() => expect(exported).toBeDefined());
      expect(anchorClick).toHaveBeenCalledOnce();
      const text = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader(); reader.onload = () => resolve(String(reader.result));
        reader.onerror = () => reject(reader.error); reader.readAsText(exported!);
      });
      expect(text).not.toContain('synthetic-entry-secret');
      const archive = JSON.parse(text);
      expect(archive.format).toBe('lingxi-settings');
      archive.config.locale = 'ko';
      const bytes = JSON.stringify(archive);
      const file = new File([bytes], 'entry-settings.json', { type: 'application/json' });
      // jsdom 仅补齐本地文件读取；保存和随后读取都使用实际服务。
      Object.defineProperty(file, 'text', { value: async () => bytes });
      await waitFor(() => expect((screen.getByRole('button', { name: 'settings.transfer.import' }) as HTMLButtonElement).disabled).toBe(false));
      fireEvent.change(screen.getByLabelText('settings.transfer.import'), { target: { files: [file] } });
      await waitFor(async () => {
        const saved = JSON.parse(await readFile(join(home, 'user/preferences.json'), 'utf8'));
        expect(saved.locale).toBe('ko'); expect(saved.future.keep).toBe(37);
        expect(saved.search_api_keys.tavily).toBe('synthetic-entry-secret');
        expect(useSettingsStore.getState().settingsConfig?.locale).toBe('ko');
      }, { timeout: 10_000 });
      expect(requests.some(request => request.path === '/lingxi/v1/settings/export' && request.status === 200)).toBe(true);
      expect(requests.some(request => request.path === '/lingxi/v1/settings/import' && request.method === 'POST' && request.status === 200)).toBe(true);
      fireEvent.click(screen.getByRole('button', { name: 'settings.tabs.skills' }));
      expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:settings-entry-owned');
    }
    if (scenario === 'restart' || scenario === 'rejected-restart') {
      const oldOwner = useSettingsStore.getState().settingsIdentityOwner;
      if (scenario === 'restart') {
        lateResponseGate = new Promise(resolve => { releaseLate401 = resolve; });
        delayNextInvalidModel = true;
        pendingLate401 = lingxiFetch('/api/models').then(() => { throw new Error('旧凭据请求不应成功'); }, error => error);
        // 旧服务已真正返回401，只延迟交给旧请求的消费者，直到新连接恢复。
        await waitFor(() => expect(requests.some(request => request.path === '/lingxi/v1/models' && request.status === 401)).toBe(true));
      }
      const oldExit = exited;
      child!.kill('SIGTERM');
      const stopTimer = setTimeout(() => child!.kill('SIGKILL'), 10_000);
      try { expect(await oldExit).toBe(0); } finally { clearTimeout(stopTimer); }
      const nextAddress = await boot();
      const nextToken = JSON.parse(await readFile(join(home, 'lingxi-service/local-token.json'), 'utf8')).token;
      expect(nextToken === token).toBe(false);
      expect(restarted).toBeTypeOf('function');
      act(() => restarted!({ port: Number(new URL(`http://${nextAddress}`).port),
        token: scenario === 'rejected-restart' ? token : nextToken,
        serverNodeKind: 'lingxi-service', serverNodeTransport: 'http' }));
      expect(ownsSettingsConnection(oldOwner)).toBe(false);
      if (scenario === 'rejected-restart') {
        expect(useSettingsStore.getState().settingsIdentityOwner).toBeNull();
        expect(useSettingsStore.getState().rustSettingsUnavailable).toBe(true);
        fireEvent.click(screen.getByRole('button', { name: 'settings.tabs.general' }));
        expect(document.querySelector('div[data-tab="general"]:not([data-settings-page])')).toBeNull();
        expect(screen.getAllByRole('alert').some(alert => alert.textContent?.includes('settings.rustCoreUnavailable'))).toBe(true);
      } else {
        await waitFor(() => {
          const state = useSettingsStore.getState();
          expect(ownsSettingsConnection(state.settingsIdentityOwner)).toBe(true);
          expect(state.rustSettingsUnavailable).toBe(false);
          expect(state.activeServerConnection?.baseUrl).toBe(`http://${nextAddress}`);
          expect(state.settingsDataAgentId).toBe('entry-agent');
          expect(ownsSettingsConnection(state.settingsDataOwner)).toBe(true);
        }, { timeout: 14_500 });
        fireEvent.click(screen.getByRole('button', { name: 'settings.tabs.general' }));
        expect(screen.getByRole('button', { name: 'settings.transfer.export' })).toBeTruthy();
        const newOwner = useSettingsStore.getState().settingsIdentityOwner;
        releaseLate401!();
        expect(await pendingLate401).toMatchObject({ code: 'unauthorized' });
        expect(useSettingsStore.getState().settingsIdentityOwner).toEqual(newOwner);
        expect(ownsSettingsConnection(newOwner)).toBe(true);
        expect(useSettingsStore.getState().rustSettingsUnavailable).toBe(false);
        expect(document.querySelector('div[data-tab="general"]:not([data-settings-page])')).not.toBeNull();
      }
    }
  } finally {
    releaseLate401?.();
    try {
      cleanup();
      await waitFor(() => expect(pendingRequests).toBe(0), { timeout: 10_000 });
      await pendingLate401;
    } finally {
      globalThis.fetch = actualFetch;
      URL.createObjectURL = originalCreateUrl; URL.revokeObjectURL = originalRevokeUrl; anchorClick.mockRestore();
      window.platform = previousPlatform; window.i18n = previousI18n; window.t = previousT;
      useSettingsStore.setState(previous, true);
      try {
        try {
          if (preservedAgentConfig) await writeFile(join(home, 'agents/entry-agent/config.yaml'), preservedAgentConfig);
        } finally {
          if (child?.pid) {
            if (child.exitCode === null && child.signalCode === null) child.kill('SIGTERM');
            const timer = setTimeout(() => child!.kill('SIGKILL'), 10_000);
            try { expect(await exited).toBe(0); } finally { clearTimeout(timer); }
          }
        }
      } finally { await rm(scratch, { recursive: true, force: true }); }
    }
  }
}, 60_000);
