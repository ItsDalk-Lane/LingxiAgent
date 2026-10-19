/** @vitest-environment jsdom */
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { spawn } from 'node:child_process';
import { mkdtemp, mkdir, readFile, realpath, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it, vi } from 'vitest';
import { SettingsTransferSection } from '../../settings/components/SettingsTransferSection';
import { useSettingsStore } from '../../settings/store';
import { lingxiFetchJson } from '../../settings/api';
import type { ServerConnection } from '../../services/server-connection';

const binary = process.env.LINGXI_R07_SERVICE_BIN;
describe('设置页按钮连接真实 Rust 子进程', () => {
  it.skipIf(!binary)('实际导出隐藏密钥，修改导出文件后从按钮导入并保留其他设置（无服务二进制时明确跳过）', async () => {
    const scratch = await realpath(await mkdtemp(join(tmpdir(), 'lingxi-r07-settings-ui-')));
    const home = join(scratch, 'home'); const workspace = join(scratch, 'workspace');
    const previous = useSettingsStore.getState(); const translate = window.t;
    let child: ReturnType<typeof spawn> | undefined;
    let exited: Promise<number | null> | undefined;
    let exported: Blob | undefined;
    const oldCreate = URL.createObjectURL; const oldRevoke = URL.revokeObjectURL;
    const toast = vi.fn();
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => {});
    try {
      // 真实生产入口先初始化全新资料，不能伪装接受未迁移的旧资料。
      await Promise.all([mkdir(home), mkdir(workspace)]);
      const config = join(scratch, 'config.json'); await writeFile(config, JSON.stringify({ home, workspace }));
      const env = { ...process.env }; delete env.LINGXI_HOME;
      child = spawn(binary!, ['--config', config, '--bind', '127.0.0.1:0'], { env, stdio: ['ignore', 'pipe', 'pipe'] });
      exited = new Promise(resolve => child!.once('exit', resolve));
      let stdout = ''; let stderr = '';
      child.stderr!.on('data', data => { stderr = (stderr + String(data)).slice(-4000); });
      const address = await new Promise<string>((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error(`服务启动超时：${stderr}`)), 30_000);
        child!.once('error', error => { clearTimeout(timer); reject(error); });
        child!.once('exit', code => { clearTimeout(timer); reject(new Error(`服务提前退出 ${code}：${stderr}`)); });
        child!.stdout!.on('data', data => { stdout += String(data); const found = stdout.match(/LINGXI_SERVICE_READY addr=(\S+)/); if (found) { clearTimeout(timer); resolve(found[1]); } });
      });
      const { token } = JSON.parse(await readFile(join(home, 'lingxi-service/local-token.json'), 'utf8'));
      const preferences = join(home, 'user/preferences.json');
      await mkdir(join(home, 'user'), { recursive: true });
      const original = await readFile(preferences, 'utf8').then(JSON.parse, error => { if (error.code === 'ENOENT') return {}; throw error; });
      await writeFile(preferences, JSON.stringify({ ...original, future: { keep: 37 }, search_provider: 'tavily', search_api_keys: { tavily: 'synthetic-ui-secret' } }));
      const source: ServerConnection = { connectionId: 'r07-settings-actual', kind: 'local', serverId: 'local', serverNodeKind: 'lingxi-service', studioId: 'local', label: '独立设置测试', baseUrl: `http://${address}`, wsUrl: `ws://${address}`, token, authState: 'paired', trustState: 'local', credentialKind: 'loopback_token', capabilities: [] };
      window.t = ((key: string) => key) as typeof window.t;
      useSettingsStore.setState({ serverConnections: { [source.connectionId]: source }, activeServerConnectionId: source.connectionId, activeServerConnection: source, showToast: toast });
      await lingxiFetchJson('/api/agents', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ id: 'a', name: 'a' }) });
      useSettingsStore.setState({ currentAgentId: 'a', settingsAgentId: 'a', agents: [{ id: 'a', name: 'a', yuan: 'lingxi', isPrimary: true }] });
      URL.createObjectURL = vi.fn(blob => { exported = blob as Blob; return 'blob:settings-owned'; });
      URL.revokeObjectURL = vi.fn();
      render(<SettingsTransferSection />);
      fireEvent.click(screen.getByRole('button', { name: 'settings.transfer.export' }));
      await waitFor(() => expect(exported).toBeDefined()); expect(click).toHaveBeenCalledOnce();
      const text = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader(); reader.onload = () => resolve(String(reader.result)); reader.onerror = () => reject(reader.error); reader.readAsText(exported!);
      });
      expect(text).not.toContain('synthetic-ui-secret');
      const archive = JSON.parse(text); expect(archive.format).toBe('lingxi-settings'); archive.config.locale = 'ko';
      const bytes = JSON.stringify(archive); const file = new File([bytes], 'lingxi-settings.json', { type: 'application/json' });
      // jsdom缺少File.text，补齐本地文件读取接口；服务请求始终是真HTTP。
      Object.defineProperty(file, 'text', { value: async () => bytes });
      await waitFor(() => expect((screen.getByRole('button', { name: 'settings.transfer.import' }) as HTMLButtonElement).disabled).toBe(false));
      fireEvent.change(screen.getByLabelText('settings.transfer.import'), { target: { files: [file] } });
      await waitFor(async () => { const saved = JSON.parse(await readFile(join(home, 'user/preferences.json'), 'utf8')); expect(saved.locale).toBe('ko'); expect(saved.future.keep).toBe(37); expect(saved.search_api_keys.tavily).toBe('synthetic-ui-secret'); }, { timeout: 10_000 });
      expect((await lingxiFetchJson<{ locale: string }>('/api/config')).locale).toBe('ko');
      await expect(lingxiFetchJson('/api/config', { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ locale: 'ja', providers: { invalid: { api: 'not-a-supported-protocol', base_url: 'http://127.0.0.1:1', api_key: 'synthetic-invalid-key', models: ['x'] } } }) })).rejects.toThrow('部分设置已保存');
      expect((await lingxiFetchJson<{ locale: string }>('/api/config')).locale).toBe('ja');
      cleanup(); expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:settings-owned');
      child.kill('SIGTERM');
      const code = await Promise.race([exited, new Promise<never>((_, reject) => { const timer = setTimeout(() => reject(new Error('服务未回收')), 10_000); exited!.finally(() => clearTimeout(timer)); })]); expect(code).toBe(0);
    } finally {
      cleanup(); if (child?.pid && child.exitCode === null && child.signalCode === null) { child.kill('SIGKILL'); await exited; }
      URL.createObjectURL = oldCreate; URL.revokeObjectURL = oldRevoke; click.mockRestore(); window.t = translate;
      useSettingsStore.setState(previous); await rm(scratch, { recursive: true, force: true });
    }
  }, 55_000);
});
