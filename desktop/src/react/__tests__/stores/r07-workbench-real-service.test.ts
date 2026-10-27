/** @vitest-environment jsdom */
import React from 'react';
import { cleanup, fireEvent, render, waitFor, screen, within } from '@testing-library/react';
import { parse as parseYaml } from 'yaml';
import { spawn } from 'node:child_process';
import { mkdtemp, mkdir, readFile, realpath, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';
import { captureSessionConnection } from '../../services/session-connection-owner';
import { lingxiFetch } from '../../hooks/use-hana-fetch';
import { retainResourceWatch } from '../../services/resource-events';
import { startRustResourceEvents } from '../../services/rust-workbench';
import { createLocalStudioWorkspaceFromFolder, deskCreateFileInSubdir, loadDeskFiles, loadJianContent, saveJianContent } from '../../stores/desk-actions';
import type { ServerConnection } from '../../services/server-connection';
import { useSettingsStore } from '../../settings/store';
import { WorkTab } from '../../settings/tabs/WorkTab';

const serviceBinary = process.env.LINGXI_R07_SERVICE_BIN;

describe('T07 页面操作连接真实 Rust 子进程', () => {
  it.skipIf(!serviceBinary)('隔离目录中新建文件、便笺、挂载与监听实际生效，撤权和退出回收资源（无 LINGXI_R07_SERVICE_BIN 明确跳过）', async () => {
    const scratch = await realpath(await mkdtemp(join(tmpdir(), 'lingxi-r07-ui-real-')));
    const home = join(scratch, 'home'); const workspace = join(scratch, 'workspace'); const mounted = join(scratch, 'mounted');
    const previous = useStore.getState();
    const previousSettings = useSettingsStore.getState();
    const previousTranslate = window.t;
    let releaseWatch: (() => void) | undefined; let stopEvents: (() => void) | undefined;
    let child: ReturnType<typeof spawn> | undefined; let exited: Promise<number | null> | undefined;
    try {
      await Promise.all([mkdir(home), mkdir(workspace), mkdir(mounted)]);
      const config = join(scratch, 'service-config.json');
      await writeFile(config, JSON.stringify({ home, workspace }));
      const environment = { ...process.env }; delete environment.LINGXI_HOME;
      child = spawn(serviceBinary!, ['--config', config, '--bind', '127.0.0.1:0', '--shutdown-timeout-ms', '5000'], {
        env: environment, stdio: ['ignore', 'pipe', 'pipe'],
      });
      exited = new Promise(resolve => child!.once('exit', code => resolve(code)));
      let stdout = ''; let stderr = '';
      child.stderr!.on('data', chunk => { stderr = (stderr + String(chunk)).slice(-4000); });
      const address = await new Promise<string>((resolve, reject) => {
        const timeout = setTimeout(() => reject(new Error(`真实服务启动超时：${stderr}`)), 30_000);
        child!.once('error', error => { clearTimeout(timeout); reject(error); });
        child!.once('exit', code => { clearTimeout(timeout); reject(new Error(`真实服务在就绪前退出 (${code})：${stderr}`)); });
        child!.stdout!.on('data', chunk => {
          stdout += String(chunk);
          const match = stdout.match(/LINGXI_SERVICE_READY addr=(\S+)/);
          if (match) { clearTimeout(timeout); resolve(match[1]); }
        });
      });
      const { token } = JSON.parse(await readFile(join(home, 'lingxi-service/local-token.json'), 'utf8'));
      expect(typeof token).toBe('string');
      const source: ServerConnection = { connectionId: 'r07-real-subprocess', kind: 'local', serverId: 'local', serverNodeKind: 'lingxi-service',
        studioId: 'local', label: '隔离真实服务', baseUrl: `http://${address}`, wsUrl: `ws://${address}`, token,
        authState: 'paired', trustState: 'local', credentialKind: 'loopback_token', capabilities: [] };
      window.t = ((key: string) => key) as typeof window.t;
      useStore.setState({ serverConnections: { [source.connectionId]: source }, activeServerConnectionId: source.connectionId, activeServerConnection: source });
      captureSessionConnection();
      // 空目录先通过正式启动安全闸，再经正式接口创建供设置页浏览的助手。
      await lingxiFetch('/lingxi/v1/agents', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ id: 'a', name: 'a' }) });
      useStore.setState({ currentSessionId: null, currentSessionPath: null, currentAgentId: 'a', selectedAgentId: null,
        deskBasePath: workspace, deskWorkspaceMountId: null, deskWorkspaceNativeRoot: null, deskFiles: [], deskTreeFilesByPath: {}, deskJianContent: null });

      // 使用真实页面动作，既不替换 fetch，也不绕过认证、服务端或存储。
      expect(await deskCreateFileInSubdir('', 'created-by-ui.md', '# 真实服务文件')).toBe(true);
      expect(await readFile(join(workspace, 'created-by-ui.md'), 'utf8')).toBe('# 真实服务文件');
      await loadDeskFiles();
      expect(useStore.getState().deskFiles.some(file => file.name === 'created-by-ui.md')).toBe(true);
      await saveJianContent('真实便笺');
      expect(await readFile(join(workspace, 'jian.md'), 'utf8')).toBe('真实便笺');
      await loadJianContent(); expect(useStore.getState().deskJianContent).toBe('真实便笺');

      // 工作设置使用真正组件与正式保存入口，页面状态来自真实服务读回。
      useSettingsStore.setState({ serverConnections: { [source.connectionId]: source }, activeServerConnectionId: source.connectionId, activeServerConnection: source });
      const agentConfig = await (await lingxiFetch('/api/agents/a/config')).json();
      useSettingsStore.setState({ currentAgentId: 'a', settingsAgentId: 'a',
        settingsConfigStatus: 'ready', settingsConfig: agentConfig,
        agents: [{ id: 'a', name: 'a', yuan: 'lingxi', isPrimary: true }] });
      const settingsView = render(React.createElement(WorkTab));
      const heartbeatRow = screen.getByText('settings.work.heartbeatMaster').parentElement!.parentElement!;
      const heartbeatToggle = within(heartbeatRow).getByRole('switch');
      const nextHeartbeat = heartbeatToggle.getAttribute('aria-checked') !== 'true';
      fireEvent.click(heartbeatToggle);
      await waitFor(async () => {
        const preferences = JSON.parse(await readFile(join(home, 'user/preferences.json'), 'utf8'));
        expect(preferences.heartbeat_master).toBe(nextHeartbeat);
        const actual = await (await lingxiFetch('/api/agents/a/config')).json();
        expect(actual.desk.heartbeat_master).toBe(nextHeartbeat);
      }, { timeout: 5_000 });
      const injectionToggle = await screen.findByRole('switch', { name: 'settings.work.injectAgentsMd' });
      const nextInjection = injectionToggle.getAttribute('aria-checked') !== 'true';
      fireEvent.click(injectionToggle);
      await waitFor(async () => {
        const stored = parseYaml(await readFile(join(home, 'agents/a/config.yaml'), 'utf8'));
        expect(stored.workspace_context.inject_agents_md).toBe(nextInjection);
        expect(stored.agent.name).toBe('a');
        const actual = await (await lingxiFetch('/api/agents/a/config')).json();
        expect(actual.workspace_context.inject_agents_md).toBe(nextInjection);
      }, { timeout: 5_000 });
      settingsView.unmount();

      const mount = await createLocalStudioWorkspaceFromFolder(mounted);
      expect(mount?.capabilities).toContain('watch'); expect(mount?.mountId).toMatch(/^local_fs_/);
      useStore.setState({ deskBasePath: `studio:${mount!.mountId}`, deskWorkspaceMountId: mount!.mountId, deskWorkspaceNativeRoot: mounted });
      expect(await deskCreateFileInSubdir('', 'mounted-by-ui.txt', '挂载真实文件')).toBe(true);
      expect(await readFile(join(mounted, 'mounted-by-ui.txt'), 'utf8')).toBe('挂载真实文件');
      const events: Record<string, unknown>[] = [];
      stopEvents = startRustResourceEvents(source, { applyEvent: event => events.push(event) });
      releaseWatch = retainResourceWatch({ kind: 'mount', mountId: mount!.mountId, path: '' });
      await vi.waitFor(async () => {
        const result = await (await lingxiFetch('/api/resource-io/watch-diagnostics')).json();
        expect(result.diagnostics.subscriptions).toBe(1);
      }, { timeout: 5_000, interval: 100 });
      await writeFile(join(mounted, 'outside-change.txt'), '真实外部文件变化');
      await vi.waitFor(() => expect(events.some(event => JSON.stringify(event).includes('outside-change.txt'))).toBe(true), { timeout: 7_000, interval: 100 });
      await lingxiFetch(`/api/studio/workspaces/${encodeURIComponent(mount!.mountId)}`, { method: 'DELETE' });
      const diagnostics = await (await lingxiFetch('/api/resource-io/watch-diagnostics')).json();
      expect(diagnostics.diagnostics.subscriptions).toBe(0); expect(diagnostics.diagnostics.watches).toEqual([]);
      expect(await readFile(join(mounted, 'mounted-by-ui.txt'), 'utf8')).toBe('挂载真实文件');
      const cursor = (await (await lingxiFetch('/api/resource-io/events?since=0')).json()).latestSequence;
      await writeFile(join(mounted, 'outside-change.txt'), '撤权后变化');
      await new Promise(resolve => setTimeout(resolve, 350));
      expect((await (await lingxiFetch(`/api/resource-io/events?since=${cursor}`)).json()).events).toEqual([]);
      stopEvents(); stopEvents = undefined; releaseWatch(); releaseWatch = undefined;
      child.kill('SIGTERM');
      let exitDeadline: ReturnType<typeof setTimeout> | undefined;
      const exitCode = await Promise.race([exited, new Promise<never>((_, reject) => {
        exitDeadline = setTimeout(() => reject(new Error('真实服务未按时退出')), 8_000);
      })]).finally(() => { if (exitDeadline) clearTimeout(exitDeadline); });
      expect(exitCode).toBe(0);
    } finally {
      cleanup();
      stopEvents?.(); releaseWatch?.();
      if (child?.pid && child.exitCode === null && child.signalCode === null) {
        child.kill('SIGKILL'); await exited;
      }
      useStore.setState({ serverConnections: previous.serverConnections, activeServerConnectionId: previous.activeServerConnectionId, activeServerConnection: previous.activeServerConnection });
      useSettingsStore.setState(previousSettings);
      window.t = previousTranslate;
      vi.unstubAllGlobals();
      await rm(scratch, { recursive: true, force: true });
    }
  }, 50_000);
});
