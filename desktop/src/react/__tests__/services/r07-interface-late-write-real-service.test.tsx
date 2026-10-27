/** @vitest-environment jsdom */
import React from 'react';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { spawn } from 'node:child_process';
import { mkdtemp, mkdir, readFile, realpath, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { expect, it, vi } from 'vitest';
import { SettingsContent } from '../../settings/SettingsContent';
import { useSettingsStore } from '../../settings/store';
import { ownsSettingsConnection } from '../../settings/connection-owner';

const binary = process.env.LINGXI_R07_SERVICE_BIN;
const waitOptions = { timeout: 15_000 };
function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>(done => { resolve = done; });
  return { promise, resolve };
}

it.skipIf(!binary).each(['key-save', 'key-reset', 'sidebar-save', 'archive-import-aba'] as const)('actual A save, delayed response and actual B connection isolate snapshot, events and OS reload (%s)', async scenario => {
  const scratch = await realpath(await mkdtemp(join(tmpdir(), 'lingxi-r07-interface-late-write-')));
  const previous = useSettingsStore.getState();
  const previousPlatform = window.platform; const previousHana = window.hana;
  const previousI18n = window.i18n; const previousT = window.t;
  const actualFetch = globalThis.fetch;
  const release = deferred(); const arrived = deferred(); const bodyConsumed = deferred();
  let restarted: ((data: { port: number; token: string; serverNodeKind: string; serverNodeTransport: string }) => void) | undefined;
  const settingsChanged = vi.fn(); const reload = vi.fn(async () => ({ ok: true }));
  const windowChanges: unknown[] = [];
  const onSettings = (event: Event) => windowChanges.push((event as CustomEvent).detail);
  const services: { home: string; base: string; token: string; child: ReturnType<typeof spawn>; exited: Promise<number | null> }[] = [];
  const requests: { server: string; path: string; method: string; status: number; delayed: boolean }[] = [];
  let delayArmed = false; let passed = false; let pendingRequests = 0;
  const startedRequests: { server: string; path: string; method: string }[] = [];
  const selectedPath = scenario === 'sidebar-save' ? '/lingxi/v1/preferences/sidebar-ui' : '/lingxi/v1/preferences/keybindings';
  const boot = async (name: string) => {
    const home = join(scratch, name, 'home'); const workspace = join(scratch, name, 'workspace');
    await Promise.all([mkdir(home, { recursive: true }), mkdir(workspace, { recursive: true })]);
    const config = join(scratch, name, 'config.json');
    await writeFile(config, JSON.stringify({ home, workspace, providers: { fixture: { protocol: 'openai-completions', endpoint: 'http://127.0.0.1:9/v1', auth: { kind: 'none' }, models: [{ id: 'fixture-chat', capabilities: { tools: true } }] } }, models: { chat: { provider: 'fixture', model: 'fixture-chat', capabilities: { tools: true } } } }));
    const env = { ...process.env }; delete env.LINGXI_HOME;
    const child = spawn(binary!, ['--config', config, '--bind', '127.0.0.1:0'], { env, stdio: ['ignore', 'pipe', 'pipe'] });
    const exited = new Promise<number | null>(resolve => child.once('exit', resolve));
    const address = await new Promise<string>((resolve, reject) => {
      let output = ''; const timer = setTimeout(() => reject(new Error('隔离服务启动超时')), 30_000);
      child.once('error', error => { clearTimeout(timer); reject(error); });
      child.once('exit', code => { clearTimeout(timer); reject(new Error(`隔离服务提前退出：${code}`)); });
      child.stderr!.on('data', () => {});
      child.stdout!.on('data', data => { output += String(data); const found = output.match(/LINGXI_SERVICE_READY addr=(\S+)/); if (found) { clearTimeout(timer); resolve(found[1]); } });
    });
    const token = JSON.parse(await readFile(join(home, 'lingxi-service/local-token.json'), 'utf8')).token;
    const service = { home, base: `http://${address}`, token, child, exited }; services.push(service);
    const response = await actualFetch(service.base + '/lingxi/v1/agents', { method: 'POST', headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' }, body: JSON.stringify({ id: name, name }) });
    expect(response.status).toBe(200); await response.json();
    await mkdir(join(home, 'user'), { recursive: true });
    const prefs = await readFile(join(home, 'user/preferences.json'), 'utf8').then(JSON.parse, error => { if (error.code === 'ENOENT') return {}; throw error; });
    // 两端资料在各自实际服务的隔离目录预置，B快照之后必须由容器真实读取。
    await writeFile(join(home, 'user/preferences.json'), JSON.stringify({ ...prefs, keybindings: { 'quick-chat.toggle': [name === 'server-a' ? 'F9' : 'F8'] }, sidebar_ui: { sessionList: { rowMode: 'two-line' } }, future: { owner: name } }));
    return service;
  };
  const readPrefs = async (index: number) => JSON.parse(await readFile(join(services[index].home, 'user/preferences.json'), 'utf8'));
  const readyFor = async (index: number) => waitFor(() => {
    const state = useSettingsStore.getState();
    expect(state.activeServerConnection?.baseUrl).toBe(services[index].base);
    expect(state.settingsDataAgentId).toBe(index === 0 ? 'server-a' : 'server-b');
    expect(ownsSettingsConnection(state.settingsIdentityOwner)).toBe(true);
    expect(ownsSettingsConnection(state.settingsDataOwner)).toBe(true);
    expect(state.settingsSnapshot.status).toBe('ready'); expect(state.settingsConfigStatus).toBe('ready');
    expect(state.rustSettingsUnavailable).toBe(false);
  }, waitOptions);
  const navigate = (tab: string) => fireEvent.click(screen.getByRole('button', { name: `settings.tabs.${tab}` }));
  const keyRow = () => document.querySelector<HTMLElement>('[data-command="quick-chat.toggle"]')!;
  const density = () => within(screen.getByText('settings.interface.sessionListSingleLine').parentElement!.parentElement!).getByRole('switch');
  try {
    const a = await boot('server-a'); const b = await boot('server-b');
    globalThis.fetch = async (input, init) => {
      const url = new URL(String(input)); const method = init?.method || 'GET';
      const delayed = delayArmed && url.origin === a.base && url.pathname === selectedPath && method === 'PUT';
      if (delayed) delayArmed = false;
      startedRequests.push({ server: url.origin === a.base ? 'A' : url.origin === b.base ? 'B' : 'other', path: url.pathname, method });
      pendingRequests++;
      try {
      const response = await actualFetch(input, init);
      requests.push({ server: url.origin === a.base ? 'A' : url.origin === b.base ? 'B' : 'other', path: url.pathname, method, status: response.status, delayed });
      if (delayed) {
        expect(response.status).toBe(200);
        // 先核实际正文已到达，只延迟交付原Response；不要求过期consumer继续读取。
        await response.clone().json();
        arrived.resolve(); await release.promise; bodyConsumed.resolve();
      }
      return response;
      } finally { pendingRequests--; }
    };
    window.t = ((key: string) => key) as typeof window.t;
    window.i18n = { locale: 'en', defaultName: 'Lingxi', _data: {}, _agentOverrides: {}, load: async () => {}, setAgentOverrides: () => {}, t: window.t };
    window.platform = { getServerConnectionInfo: async () => ({ port: Number(new URL(a.base).port), token: a.token, serverNodeKind: 'lingxi-service', serverNodeTransport: 'http' }), getPlatform: async () => 'darwin', onSettingsChanged: () => () => {}, settingsChanged,
      onServerRestarted: (callback: typeof restarted) => { restarted = callback; return () => { restarted = undefined; }; },
    } as unknown as typeof window.platform;
    // 只观察外部桌面重载边界；它不能代表系统级注册已验证。
    window.hana = { settingsChanged, keybindingsReloadGlobal: reload } as unknown as typeof window.hana;
    window.addEventListener('hana-settings', onSettings);
    localStorage.clear(); useSettingsStore.setState({ ...useSettingsStore.getInitialState(), activeTab: 'access' }, true);
    render(<SettingsContent variant="modal" />); await readyFor(0);
    if (scenario === 'archive-import-aba') {
      navigate('general');
      const exported = await actualFetch(a.base + '/lingxi/v1/settings/export', { headers: { Authorization: `Bearer ${a.token}` } });
      expect(exported.status).toBe(200); const archive = await exported.json(); archive.config.locale = 'ko';
      const aBefore = await readPrefs(0); const bBefore = await readPrefs(1);
      const file = new File([JSON.stringify(archive)], 'slow-owned-settings.json', { type: 'application/json' });
      // 仅替代本地文件读取速度；内容来自真实导出，慢读取跨越A→B→A。
      Object.defineProperty(file, 'text', { value: async () => { arrived.resolve(); await release.promise; bodyConsumed.resolve(); return JSON.stringify(archive); } });
      fireEvent.change(screen.getByLabelText('settings.transfer.import'), { target: { files: [file] } });
      await arrived.promise;
      await act(async () => restarted!({ port: Number(new URL(b.base).port), token: b.token, serverNodeKind: 'lingxi-service', serverNodeTransport: 'http' }));
      await readyFor(1);
      expect((screen.getByRole('button', { name: 'settings.transfer.import' }) as HTMLButtonElement).disabled).toBe(false);
      await act(async () => restarted!({ port: Number(new URL(a.base).port), token: a.token, serverNodeKind: 'lingxi-service', serverNodeTransport: 'http' }));
      await readyFor(0);
      expect((screen.getByRole('button', { name: 'settings.transfer.import' }) as HTMLButtonElement).disabled).toBe(false);
      const before = { snapshot: JSON.stringify(useSettingsStore.getState().settingsSnapshot), toast: useSettingsStore.getState().toastMessage };
      await act(async () => { release.resolve(); await bodyConsumed.promise; await Promise.resolve(); await Promise.resolve(); });
      await waitFor(() => expect(pendingRequests).toBe(0), waitOptions);
      expect(startedRequests.filter(request => request.path === '/lingxi/v1/settings/import'), '旧File.text跨越连接revision后不得发起导入').toEqual([]);
      expect(JSON.stringify(useSettingsStore.getState().settingsSnapshot)).toBe(before.snapshot);
      expect(useSettingsStore.getState().toastMessage).toBe(before.toast);
      expect(await readPrefs(0)).toEqual(aBefore); expect(await readPrefs(1)).toEqual(bBefore);
      if (process.env.LINGXI_R07_LATE_WRITE_RECEIPT_DIR) await writeFile(join(process.env.LINGXI_R07_LATE_WRITE_RECEIPT_DIR, `${scenario}.json`), JSON.stringify({ scenario, realAtoBtoAInitialization: true, noStaleImportPost: true, snapshotsAndToastUnchanged: true, bothPreferencesUnchanged: true, startedRequests, requests }, null, 2));
      passed = true; return;
    }
    navigate(scenario === 'sidebar-save' ? 'interface' : 'keybindings');
    if (scenario === 'sidebar-save') {
      await waitFor(() => { expect(density().hasAttribute('disabled')).toBe(false); expect(density().getAttribute('aria-checked')).toBe('false'); }, waitOptions);
      delayArmed = true; fireEvent.click(density());
    } else {
      await waitFor(() => expect(keyRow()?.textContent).toContain('F9'), waitOptions);
      delayArmed = true;
      if (scenario === 'key-reset') fireEvent.click(screen.getByText('settings.keybindings.resetAll'));
      else { fireEvent.click(within(keyRow()).getByRole('button', { name: 'settings.keybindings.editBinding' })); fireEvent.keyDown(window, { key: 'F10' }); }
    }
    await arrived.promise;
    const aSaved = await readPrefs(0);
    expect(scenario === 'sidebar-save' ? aSaved.sidebar_ui.sessionList.rowMode : aSaved.keybindings).toEqual(scenario === 'sidebar-save' ? 'single-line' : scenario === 'key-reset' ? {} : { 'quick-chat.toggle': ['F10'] });
    // 通过现役宿主连接事件切到真实B，让共享容器自行验证身份并读取所有资料。
    await act(async () => restarted!({ port: Number(new URL(b.base).port), token: b.token, serverNodeKind: 'lingxi-service', serverNodeTransport: 'http' }));
    await readyFor(1);
    if (scenario === 'sidebar-save') await waitFor(() => expect(density().getAttribute('aria-checked')).toBe('false'), waitOptions);
    else await waitFor(() => { expect(keyRow()?.textContent).toContain('F8'); expect((within(keyRow()).getByRole('button', { name: 'settings.keybindings.editBinding' }) as HTMLButtonElement).disabled).toBe(false); }, waitOptions);
    const before = { snapshot: JSON.stringify(useSettingsStore.getState().settingsSnapshot), toast: useSettingsStore.getState().toastMessage, events: settingsChanged.mock.calls.length, reloads: reload.mock.calls.length, windowEvents: windowChanges.length };
    await act(async () => { release.resolve(); await bodyConsumed.promise; await Promise.resolve(); });
    const after = { snapshot: JSON.stringify(useSettingsStore.getState().settingsSnapshot), toast: useSettingsStore.getState().toastMessage, events: settingsChanged.mock.calls.length, reloads: reload.mock.calls.length, windowEvents: windowChanges.length };
    const violations = Object.keys(before).filter(key => before[key as keyof typeof before] !== after[key as keyof typeof after]);
    expect(violations, 'A已真实保存；迟到回复不得写B快照/提示或触发当前桌面及窗口事件').toEqual([]);
    if (scenario === 'sidebar-save') expect(density().getAttribute('aria-checked')).toBe('false');
    else expect(keyRow().textContent).toContain('F8');
    expect((await readPrefs(1)).keybindings).toEqual({ 'quick-chat.toggle': ['F8'] });
    expect((await readPrefs(1)).sidebar_ui.sessionList.rowMode).toBe('two-line');
    expect(await readPrefs(0)).toEqual(aSaved);
    if (process.env.LINGXI_R07_LATE_WRITE_RECEIPT_DIR) await writeFile(join(process.env.LINGXI_R07_LATE_WRITE_RECEIPT_DIR, `${scenario}.json`), JSON.stringify({ scenario, aSaved: true, bSnapshotUnchanged: before.snapshot === after.snapshot, bToastUnchanged: before.toast === after.toast, noHostOrWindowEvents: before.events === after.events && before.windowEvents === after.windowEvents, noOsReload: before.reloads === after.reloads, aPersistenceUnchangedAfterLateResponse: true, requests, desktopBoundary: 'owned reload spy; no OS registration' }, null, 2));
    passed = true;
  } finally {
    release.resolve(); cleanup(); window.removeEventListener('hana-settings', onSettings);
    try {
      for (const service of services.reverse()) {
        if (service.child.exitCode === null && service.child.signalCode === null) service.child.kill('SIGTERM');
        const timer = setTimeout(() => service.child.kill('SIGKILL'), 10_000);
        try { expect(await service.exited).toBe(0); } finally { clearTimeout(timer); }
      }
    } finally {
      globalThis.fetch = actualFetch; window.platform = previousPlatform; window.hana = previousHana; window.i18n = previousI18n; window.t = previousT; useSettingsStore.setState(previous, true);
      if (passed) await rm(scratch, { recursive: true, force: true });
      else console.error('R07_INTERFACE_LATE_WRITE_FAILURE_HOME', scratch);
    }
  }
}, 60_000);
