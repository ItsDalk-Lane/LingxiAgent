/** @vitest-environment jsdom */
import React, { useState } from 'react';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { spawn } from 'node:child_process';
import { createServer, type IncomingMessage } from 'node:http';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, readFile, realpath, rm, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it, vi } from 'vitest';
import { OAuthCredentials } from '../../settings/tabs/providers/OAuthCredentials';
import { useSettingsStore, type ProviderSummary } from '../../settings/store';
import { lingxiFetchJson } from '../../settings/api';
import type { ServerConnection } from '../../services/server-connection';

const binary = process.env.LINGXI_R07_SERVICE_BIN;
const evidenceDir = process.env.LINGXI_R07_OAUTH_EVIDENCE_DIR;
const sha = (bytes: Uint8Array | string) => createHash('sha256').update(bytes).digest('hex');
async function body(request: IncomingMessage): Promise<string> {
  const parts: Buffer[] = []; for await (const part of request) parts.push(Buffer.from(part));
  return Buffer.concat(parts).toString('utf8');
}

// 只替代外部授权和模型端点。生产 Rust HTTP、凭证、配置、存储、网关均不替换。
describe('T08 OAuth 页面连接真实 Rust 子进程与自有授权响应端', () => {
  it.skipIf(!binary)('实际按钮完成 PKCE 手输码、device 轮询、摘要刷新、私密落盘和登出；无二进制明确跳过', async () => {
    const scratch = await realpath(await mkdtemp(join(tmpdir(), 'lingxi-r07-oauth-ui-')));
    const home = join(scratch, 'home'); const workspace = join(scratch, 'workspace');
    const previous = useSettingsStore.getState(); const translate = window.t; const platform = window.platform;
    let child: ReturnType<typeof spawn> | undefined; let exited: Promise<number | null> | undefined;
    let stdout = ''; let stderr = ''; let serviceExit: number | null = null; let success = false;
    const opened: string[] = []; const tokenRequests: Array<Record<string, unknown>> = [];
    const modelRequests: Array<Record<string, unknown>> = []; const refreshed: Array<Record<string, unknown>> = [];
    const checks: string[] = []; let devicePolls = 0; let externalBase = '';
    const external = createServer(async (request, response) => {
      try {
        const url = new URL(request.url!, externalBase); const raw = await body(request);
        const json = (status: number, value: unknown) => { response.writeHead(status, { 'content-type': 'application/json' }); response.end(JSON.stringify(value)); };
        if (url.pathname === '/authorize' || url.pathname === '/verify') {
          response.writeHead(200, { 'content-type': 'text/plain' }); response.end('本轮本地合成授权页面，不连接真实账户'); return;
        }
        if (url.pathname === '/device') {
          json(200, { device_code: 'synthetic-local-device', user_code: 'LOCAL-UI-CODE', verification_uri: `${externalBase}/verify`, expires_in: 300, interval: 1 }); return;
        }
        if (url.pathname === '/token') {
          const form = new URLSearchParams(raw); const grant = form.get('grant_type');
          tokenRequests.push({ grant, code: form.get('code'), hasVerifier: !!form.get('code_verifier'), redirectUri: form.get('redirect_uri'), deviceCodeMatches: form.get('device_code') === 'synthetic-local-device' });
          if (grant === 'urn:ietf:params:oauth:grant-type:device_code' && ++devicePolls === 1) { json(400, { error: 'authorization_pending' }); return; }
          json(200, { access_token: grant === 'authorization_code' ? 'synthetic-pkce-access' : 'synthetic-device-access', refresh_token: 'synthetic-local-refresh', expires_in: 3600, token_type: 'Bearer' }); return;
        }
        if (url.pathname === '/v1/chat/completions') {
          const data = JSON.parse(raw); modelRequests.push({ model: data.model, authorizationMatches: request.headers.authorization === 'Bearer synthetic-pkce-access' || request.headers.authorization === 'Bearer synthetic-device-access' });
          response.writeHead(200, { 'content-type': 'text/event-stream' });
          response.end(`data: ${JSON.stringify({ id: 'local-oauth-ui', choices: [{ index: 0, delta: { role: 'assistant', content: 'OK' }, finish_reason: 'stop' }], usage: { prompt_tokens: 2, completion_tokens: 1, total_tokens: 3 } })}\n\ndata: [DONE]\n\n`); return;
        }
        json(404, { error: 'unknown synthetic endpoint' });
      } catch (error) { response.writeHead(500); response.end(String(error)); }
    });
    try {
      await Promise.all([mkdir(home), mkdir(workspace)]);
      await new Promise<void>(resolve => external.listen(0, '127.0.0.1', resolve));
      const address = external.address(); if (!address || typeof address === 'string') throw new Error('合成授权端未绑定');
      externalBase = `http://127.0.0.1:${address.port}`;
      const oauth = (pkce: boolean) => ({ kind: 'oauth', flow: pkce ? 'authorizationCodePkce' : 'deviceCode', clientId: 'synthetic-ui-client', tokenEndpoint: `${externalBase}/token`, ...(pkce ? { authorizeEndpoint: `${externalBase}/authorize` } : { deviceAuthorizationEndpoint: `${externalBase}/device` }) });
      const config = join(scratch, 'config.json');
      const document = { home, workspace, providers: {
        'openai-codex': { protocol: 'openai-completions', endpoint: `${externalBase}/v1`, auth: oauth(true), models: [{ id: 'pkce-model', capabilities: { tools: true } }] },
        'device-local': { protocol: 'openai-completions', endpoint: `${externalBase}/v1`, auth: oauth(false), models: [{ id: 'device-model', capabilities: { tools: true } }] },
      }, models: { chat: { provider: 'openai-codex', model: 'pkce-model', capabilities: { tools: true } } } };
      const configBytes = JSON.stringify(document); await writeFile(config, configBytes);
      const env = { ...process.env }; delete env.LINGXI_HOME;
      child = spawn(binary!, ['--config', config, '--bind', '127.0.0.1:0', '--shutdown-timeout-ms', '5000'], { env, stdio: ['ignore', 'pipe', 'pipe'] });
      exited = new Promise(resolve => child!.once('exit', resolve));
      child.stderr!.on('data', data => { stderr += String(data); });
      const serviceAddress = await new Promise<string>((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error(`真实服务启动超时：${stderr.slice(-4000)}`)), 30_000);
        child!.once('error', error => { clearTimeout(timer); reject(error); });
        child!.once('exit', code => { clearTimeout(timer); reject(new Error(`真实服务提前退出 ${code}：${stderr.slice(-4000)}`)); });
        child!.stdout!.on('data', data => { stdout += String(data); const found = stdout.match(/LINGXI_SERVICE_READY addr=(\S+)/); if (found) { clearTimeout(timer); resolve(found[1]); } });
      });
      const { token } = JSON.parse(await readFile(join(home, 'lingxi-service/local-token.json'), 'utf8'));
      const connection: ServerConnection = { connectionId: 'r07-oauth-real', kind: 'local', serverId: 'local', serverNodeKind: 'lingxi-service', studioId: 'local', label: '本地合成授权联验', baseUrl: `http://${serviceAddress}`, wsUrl: `ws://${serviceAddress}`, token, authState: 'paired', trustState: 'local', credentialKind: 'loopback_token', capabilities: [] };
      const toast = vi.fn(); window.t = ((key: string) => key) as typeof window.t;
      // 只记录并真实 GET 自有回环授权网页；不把此代理说成原生浏览器已启动。
      Object.defineProperty(window, 'platform', { configurable: true, value: { openExternal: async (url: string) => { expect(new URL(url).origin).toBe(externalBase); opened.push(url); expect((await fetch(url)).status).toBe(200); } } });
      useSettingsStore.setState({ serverConnections: { [connection.connectionId]: connection }, activeServerConnection: connection, activeServerConnectionId: connection.connectionId, showToast: toast });
      const summaries = () => lingxiFetchJson<{ providers: Record<string, ProviderSummary> }>('/api/providers/summary');
      const credentialFile = join(home, 'lingxi-service/credentials.json');
      const credentials = async () => JSON.parse(await readFile(credentialFile, 'utf8'));
      const initial = await summaries();
      function Page({ providerId }: { providerId: string }) {
        const [summary, setSummary] = useState(initial.providers[providerId]);
        return <OAuthCredentials providerId={providerId} summary={summary} onRefresh={async () => {
          const actual = (await summaries()).providers[providerId];
          refreshed.push({ provider: providerId, loggedIn: actual.logged_in }); setSummary(actual);
        }} />;
      }
      for (const flow of [{ requested: 'openai-codex-oauth', canonical: 'openai-codex', model: 'pkce-model', manual: true }, { requested: 'device-local', canonical: 'device-local', model: 'device-model', manual: false }]) {
        expect(initial.providers[flow.requested].logged_in).toBe(false);
        const view = render(<Page providerId={flow.requested} />);
        fireEvent.click(screen.getByRole('button', { name: 'settings.oauth.login' }));
        if (flow.manual) {
          const input = await screen.findByPlaceholderText('settings.oauth.codePlaceholder', {}, { timeout: 10_000 });
          expect(tokenRequests).toHaveLength(0);
          const authorize = new URL(opened.at(-1)!); expect(authorize.searchParams.get('state')).toBeTruthy(); expect(authorize.searchParams.get('code_challenge')).toBeTruthy();
          fireEvent.change(input, { target: { value: 'synthetic-ui-manual-code' } });
          fireEvent.click(screen.getByRole('button', { name: 'settings.oauth.submit' }));
        } else await screen.findByText('LOCAL-UI-CODE', {}, { timeout: 10_000 });
        await screen.findByRole('button', { name: 'settings.oauth.logout' }, { timeout: 15_000 });
        expect(refreshed.at(-1)).toEqual({ provider: flow.requested, loggedIn: true });
        const disk = await credentials();
        if (process.platform !== 'win32') expect((await stat(credentialFile)).mode & 0o077).toBe(0);
        expect(disk.providers[flow.canonical].tokens.accessToken).toBe(flow.manual ? 'synthetic-pkce-access' : 'synthetic-device-access');
        if (flow.requested !== flow.canonical) expect(disk.providers[flow.requested]).toBeUndefined();
        expect(JSON.stringify(await summaries())).not.toContain(flow.manual ? 'synthetic-pkce-access' : 'synthetic-device-access');
        const health = await lingxiFetchJson<{ ok: boolean }>('/api/models/health', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ provider: flow.requested, model: flow.model }) });
        expect(health.ok).toBe(true); expect(modelRequests.at(-1)).toEqual({ model: flow.model, authorizationMatches: true });
        checks.push(`${flow.manual ? 'pkce-manual-state' : 'device-pending-poll'}-buttons-summary-private-store-real-gateway`);
        fireEvent.click(screen.getByRole('button', { name: 'settings.oauth.logout' }));
        await screen.findByRole('button', { name: 'settings.oauth.login' }, { timeout: 10_000 });
        expect(refreshed.at(-1)).toEqual({ provider: flow.requested, loggedIn: false });
        expect((await credentials()).providers[flow.canonical]?.tokens ?? null).toBeNull();
        expect((await summaries()).providers[flow.requested].logged_in).toBe(false);
        checks.push(`${flow.canonical}-logout-clears-real-credential-and-summary`); view.unmount();
      }
      expect(tokenRequests.find(request => request.grant === 'authorization_code')).toMatchObject({ code: 'synthetic-ui-manual-code', hasVerifier: true });
      expect(devicePolls).toBe(2); expect(tokenRequests.filter(request => request.grant === 'urn:ietf:params:oauth:grant-type:device_code').every(request => request.deviceCodeMatches)).toBe(true);
      expect(await readFile(config, 'utf8')).toBe(configBytes); checks.push('single-embedded-source-unchanged');
      cleanup(); child.kill('SIGTERM');
      serviceExit = await Promise.race([exited, new Promise<never>((_, reject) => { const timer = setTimeout(() => reject(new Error('真实OAuth服务未回收')), 12_000); exited!.finally(() => clearTimeout(timer)); })]);
      expect(serviceExit).toBe(0); checks.push('real-service-exit-zero'); success = true;
    } finally {
      cleanup(); if (child?.pid && child.exitCode === null && child.signalCode === null) { child.kill('SIGKILL'); await exited; }
      if (external.listening) { external.closeAllConnections(); await new Promise<void>(resolve => external.close(() => resolve())); }
      Object.defineProperty(window, 'platform', { configurable: true, value: platform }); window.t = translate; useSettingsStore.setState(previous);
      if (evidenceDir) {
        await mkdir(evidenceDir, { recursive: true });
        await writeFile(join(evidenceDir, 'receipt.json'), JSON.stringify({ success, scratch, binary, binarySha256: binary ? sha(await readFile(binary)) : null, checks, serviceExit, finalChildExitCode: child?.exitCode, finalChildSignal: child?.signalCode, externalClosed: !external.listening, openedUrlCount: opened.length, tokenRequests, modelRequests, refreshed, notes: ['Rust HTTP/存储/凭证/网关保持真实，仅替外部回环OAuth和模型响应。', 'openExternal以记录URL+真实GET自有授权页面代理，不证明原生浏览器启动。', '本检查取消/迟到/连接切换边界由独立13项单测验证，不能据此称全部原生OAuth通过。'] }, null, 2));
        await writeFile(join(evidenceDir, 'service.stdout.log'), stdout); await writeFile(join(evidenceDir, 'service.stderr.log'), stderr);
      }
      if (success) await rm(scratch, { recursive: true, force: true });
      else console.error('真实OAuth联验失败，合成现场保留：', scratch);
    }
  }, 70_000);
});
