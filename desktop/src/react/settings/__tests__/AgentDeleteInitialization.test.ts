// @vitest-environment jsdom
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, expect, it, vi } from 'vitest';
import { useSettingsStore } from '../store';
import { observeAgentDeleteInitialization } from './agent-delete-initialization';

const initialState = useSettingsStore.getState();
const cleanups: Array<() => void> = [];
afterEach(() => {
  for (const cleanup of cleanups.splice(0).reverse()) cleanup();
  useSettingsStore.setState(initialState, true);
  vi.restoreAllMocks();
});

function observer() {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'agent-delete-initialization-'));
  fs.mkdirSync(path.join(home, 'lingxi-service'));
  fs.writeFileSync(path.join(home, 'lingxi-service/management.json'), JSON.stringify({ serverId: 'expected-server', studioId: 'expected-studio' }));
  // 这些负例仅验证测试屏障会拒绝假就绪，不充当真实服务、鉴权或删除成功证据。
  const actualFetch: typeof fetch = vi.fn(async () => new Response(JSON.stringify({
    serverNodeKind: 'lingxi-service', serverId: 'another-server', serverNodeId: 'another-server',
    studioId: 'expected-studio', userId: 'user_local', connectionKind: 'local',
    credentialKind: 'loopback_token', trustState: 'local', authState: 'user', capabilities: ['studio.owner'],
  }), { status: 200, headers: { 'Content-Type': 'application/json' } }));
  vi.spyOn(console, 'warn').mockImplementation(() => {});
  vi.spyOn(console, 'error').mockImplementation(() => {});
  const result = observeAgentDeleteInitialization({ actualFetch, base: 'http://127.0.0.1:1', token: 'fixture-only', home, testDeadline: performance.now() + 60_000 });
  cleanups.push(() => fs.rmSync(home, { recursive: true, force: true }), () => result.dispose());
  return result;
}

it('ready=true仍拒绝真实资源的错误状态', async () => {
  const initialization = observer();
  useSettingsStore.setState({ ready: true, settingsConfigStatus: 'error', settingsConfigError: 'fixture read failed' });
  await expect(initialization.waitForReady()).rejects.toThrow('读取错误');
});

it('15秒兜底的ready=true不能放行删除', async () => {
  const initialization = observer();
  useSettingsStore.setState({ ready: true });
  console.warn('[settings] init timeout (15s), forcing ready');
  await expect(initialization.waitForReady()).rejects.toThrow('15秒兜底');
});

it('身份HTTP成功但属于另一个服务时立即拒绝', async () => {
  const initialization = observer();
  await initialization.fetch('http://127.0.0.1:1/lingxi/v1/server/identity');
  await expect(initialization.waitForReady()).rejects.toThrow('身份或本机拥有者权限');
});
