/** @vitest-environment jsdom */
import React from 'react';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
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
it.skipIf(!binary)('actual navigation saves keybindings and sidebar, keeps saved reload failures, and reads global archive metadata across restart', async () => {
  const scratch = await realpath(await mkdtemp(join(tmpdir(), 'lingxi-r07-interface-entry-')));
  const home = join(scratch, 'home');
  const workspace = join(scratch, 'workspace');
  const previous = useSettingsStore.getState();
  const previousPlatform = window.platform;
  const previousHana = window.hana;
  const previousI18n = window.i18n;
  const previousT = window.t;
  const actualFetch = globalThis.fetch;
  const requests: { path: string; method: string; status: number }[] = [];
  const checks: string[] = [];
  let pending = 0;
  let child: ReturnType<typeof spawn> | undefined;
  let exited: Promise<number | null> | undefined;
  let token = '';
  let base = '';
  let passed = false;
  const reloadGlobal = vi.fn(async () => ({ ok: false }));
  const settingsChanged = vi.fn();
  const boot = async () => {
    const env = { ...process.env }; delete env.LINGXI_HOME;
    child = spawn(binary!, ['--config', join(scratch, 'config.json'), '--bind', '127.0.0.1:0'], { env, stdio: ['ignore', 'pipe', 'pipe'] });
    exited = new Promise(resolve => child!.once('exit', resolve));
    const address = await new Promise<string>((resolve, reject) => {
      let output = '';
      const timer = setTimeout(() => reject(new Error('隔离服务未在30秒内启动')), 30_000);
      child!.once('error', error => { clearTimeout(timer); reject(error); });
      child!.once('exit', code => { clearTimeout(timer); reject(new Error(`隔离服务提前退出：${code}`)); });
      child!.stderr!.on('data', () => {});
      child!.stdout!.on('data', data => { output += String(data); const found = output.match(/LINGXI_SERVICE_READY addr=(\S+)/); if (found) { clearTimeout(timer); resolve(found[1]); } });
    });
    base = `http://${address}`;
    token = JSON.parse(await readFile(join(home, 'lingxi-service/local-token.json'), 'utf8')).token;
  };
  const stop = async () => {
    if (!child || !exited) return;
    if (child.exitCode === null && child.signalCode === null) child.kill('SIGTERM');
    const timer = setTimeout(() => child!.kill('SIGKILL'), 10_000);
    try { expect(await exited).toBe(0); } finally { clearTimeout(timer); }
  };
  const request = async (method: string, path: string, body?: unknown) => {
    const response = await actualFetch(base + path, { method, headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
    expect(response.ok, `${method} ${path}: ${response.status}`).toBe(true);
    return response.json();
  };
  const mount = async () => {
    useSettingsStore.setState({ ...useSettingsStore.getInitialState(), activeTab: 'access' }, true);
    render(<SettingsContent variant="modal" />);
    await waitFor(() => {
      const state = useSettingsStore.getState();
      expect(state.ready).toBe(true); expect(state.rustSettingsUnavailable).toBe(false);
      expect(ownsSettingsConnection(state.settingsIdentityOwner)).toBe(true);
      expect(state.settingsSnapshot.status).toBe('ready'); expect(state.settingsConfigStatus).toBe('ready');
      expect(ownsSettingsConnection(state.settingsDataOwner)).toBe(true);
    }, { timeout: 14_500 });
  };
  const navigate = (tab: string) => fireEvent.click(screen.getByRole('button', { name: `settings.tabs.${tab}` }));
  const prefs = async () => JSON.parse(await readFile(join(home, 'user/preferences.json'), 'utf8'));
  try {
    await Promise.all([mkdir(home), mkdir(workspace)]);
    await writeFile(join(scratch, 'config.json'), JSON.stringify({ home, workspace, providers: { 'entry-fixture': { protocol: 'openai-completions', endpoint: 'http://127.0.0.1:9/v1', auth: { kind: 'none' }, models: [{ id: 'entry-model', capabilities: { tools: true } }] } }, models: { chat: { provider: 'entry-fixture', model: 'entry-model', capabilities: { tools: true } } } }));
    await boot();
    await request('POST', '/lingxi/v1/agents', { id: 'entry-agent', name: '入口助手' });
    await mkdir(join(home, 'user'), { recursive: true });
    const original = await readFile(join(home, 'user/preferences.json'), 'utf8').then(JSON.parse, error => { if (error.code === 'ENOENT') return {}; throw error; });
    delete original.keybindings;
    await writeFile(join(home, 'user/preferences.json'), JSON.stringify({ ...original, future: { keep: 37 } }));
    const edited = join(home, 'owned-editor.txt'); await writeFile(edited, 'ARCHIVE_BODY_NOT_FOR_UI');
    const created = await request('POST', '/lingxi/v1/checkpoints/user-edit', { filePath: edited, reason: 'edit-start' });
    const archive = join(home, 'checkpoints', `${created.checkpoint.id}.json`);
    const archiveBytes = await readFile(archive);
    globalThis.fetch = async (input, init) => {
      pending++;
      try { const response = await actualFetch(input, init); requests.push({ path: new URL(String(input)).pathname, method: init?.method || 'GET', status: response.status }); return response; }
      finally { pending--; }
    };
    window.t = ((key: string) => key) as typeof window.t;
    window.i18n = { locale: 'en', defaultName: 'Lingxi', _data: {}, _agentOverrides: {}, load: async () => {}, setAgentOverrides: () => {}, t: window.t };
    window.platform = { getServerConnectionInfo: async () => ({ port: Number(new URL(base).port), token, serverNodeKind: 'lingxi-service', serverNodeTransport: 'http' }), getPlatform: async () => 'darwin', onSettingsChanged: () => () => {}, settingsChanged } as unknown as typeof window.platform;
    // 只替代桌面系统快捷键重载这个外部边界；服务、状态与保存结果都是真实链。
    window.hana = { keybindingsReloadGlobal: reloadGlobal, settingsChanged } as unknown as typeof window.hana;
    localStorage.clear(); await mount();
    navigate('keybindings');
    const keyRow = () => document.querySelector<HTMLElement>('[data-command="quick-chat.toggle"]')!;
    await waitFor(() => expect(keyRow()).toBeTruthy(), waitOptions);
    await waitFor(() => expect(requests.some(r => r.path === '/lingxi/v1/preferences/keybindings' && r.status === 200)).toBe(true), waitOptions);
    const editKey = () => within(keyRow()).getByRole('button', { name: 'settings.keybindings.editBinding' }) as HTMLButtonElement;
    await waitFor(() => expect(editKey().disabled).toBe(false), waitOptions);
    fireEvent.click(editKey()); fireEvent.keyDown(window, { key: 'F9' });
    await waitFor(async () => expect((await prefs()).keybindings).toEqual({ 'quick-chat.toggle': ['F9'] }), waitOptions);
    await waitFor(() => expect(useSettingsStore.getState().toastMessage).toContain('settings.keybindings.savedReloadFailed'), waitOptions);
    expect(keyRow().textContent).toContain('F9');
    expect((await request('GET', '/lingxi/v1/preferences/keybindings')).keybindings).toEqual({ 'quick-chat.toggle': ['F9'] });
    checks.push('actual-navigation-keybinding-put-readback-keeps-saved-value-after-owned-desktop-reload-failure');
    fireEvent.click(screen.getByText('settings.keybindings.resetAll'));
    await waitFor(async () => expect((await prefs()).keybindings).toEqual({}), waitOptions);
    expect(keyRow().textContent).not.toContain('F9');
    expect(useSettingsStore.getState().toastType).toBe('error');
    checks.push('actual-reset-persists-defaults-and-reports-reload-ok-false');
    navigate('interface');
    const density = () => within(screen.getByText('settings.interface.sessionListSingleLine').parentElement!.parentElement!).getByRole('switch');
    await waitFor(() => expect(density().getAttribute('aria-checked')).toBe('false'), waitOptions);
    fireEvent.click(density());
    await waitFor(async () => expect((await prefs()).sidebar_ui.sessionList.rowMode).toBe('single-line'), waitOptions);
    await waitFor(() => expect(density().getAttribute('aria-checked')).toBe('true'), waitOptions);
    expect((await request('GET', '/lingxi/v1/preferences/sidebar-ui')).sidebarUi.sessionList.rowMode).toBe('single-line');
    expect((await prefs()).future.keep).toBe(37);
    checks.push('actual-navigation-sidebar-partial-put-render-readback-preserves-other-preferences');
    navigate('security');
    const archivesToggle = () => screen.getByRole('button', { name: /settings.security.viewBackups/ });
    fireEvent.click(archivesToggle()); await screen.findByText(/owned-editor.txt/, {}, waitOptions);
    expect(document.body.textContent).not.toContain('ARCHIVE_BODY_NOT_FOR_UI');
    await writeFile(archive, '{broken');
    fireEvent.click(archivesToggle()); fireEvent.click(archivesToggle());
    await screen.findByText('settings.security.backupsLoadFailed', {}, waitOptions);
    expect(screen.getByText(/owned-editor.txt/)).toBeTruthy(); expect(screen.queryByText('settings.security.noBackups')).toBeNull();
    await writeFile(archive, archiveBytes); fireEvent.click(screen.getByRole('button', { name: 'action.retry' }));
    await waitFor(() => expect(screen.queryByText('settings.security.backupsLoadFailed')).toBeNull(), waitOptions);
    expect((await request('GET', '/lingxi/v1/checkpoints')).checkpoints[0].id).toBe(created.checkpoint.id);
    checks.push('actual-navigation-global-archive-metadata-and-real-corrupt-file-error-keeps-last-list-and-retry');
    cleanup(); await waitFor(() => expect(pending).toBe(0)); await stop(); await boot(); await mount();
    navigate('interface'); await waitFor(() => expect(density().getAttribute('aria-checked')).toBe('true'));
    navigate('keybindings'); await waitFor(() => expect(keyRow()).toBeTruthy()); expect(keyRow().textContent).not.toContain('F9');
    navigate('security'); fireEvent.click(archivesToggle()); await screen.findByText(/owned-editor.txt/, {}, waitOptions);
    checks.push('same-home-real-binary-restart-reinitializes-actual-pages-with-persisted-sidebar-default-bindings-and-archives');
    for (const path of ['/lingxi/v1/preferences/keybindings', '/lingxi/v1/preferences/sidebar-ui']) expect(requests.some(r => r.path === path && r.method === 'PUT' && r.status === 200)).toBe(true);
    expect(requests.some(r => r.path === '/lingxi/v1/checkpoints' && r.status === 500)).toBe(true);
    if (process.env.LINGXI_R07_INTERFACE_UI_RECEIPT) await writeFile(process.env.LINGXI_R07_INTERFACE_UI_RECEIPT, JSON.stringify({ checks, requests, realBinaryRestart: true, desktopReloadBoundary: 'owned bridge returning ok:false; no real OS shortcut registration', noArchiveBodyInUi: true }, null, 2));
    passed = true;
  } finally {
    cleanup();
    try { await stop(); }
    finally {
      globalThis.fetch = actualFetch; window.platform = previousPlatform; window.hana = previousHana; window.i18n = previousI18n; window.t = previousT; useSettingsStore.setState(previous, true);
      if (passed) await rm(scratch, { recursive: true, force: true });
      else console.error('R07_INTERFACE_UI_FAILURE_HOME', scratch);
    }
  }
}, 60_000);
