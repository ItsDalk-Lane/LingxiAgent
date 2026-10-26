// @vitest-environment jsdom

/**
 * R08 安全设置页叶共用法守卫分句补证（断言 2/3/4 的「无 agentId/resourceKey 时
 * loadSettingsConfig 不请求」「经验未启用时不 GET experience」）。
 *
 * 直接驱动生产 `settings/actions.loadSettingsConfig` + 真实 settings store，
 * 只 mock 全局 fetch 传输层；不渲染整页、不伪造请求结果。
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { loadSettingsConfig } from '../../settings/actions';
import { useSettingsStore } from '../../settings/store';
import { createLocalServerConnection } from '../../services/server-connection';

interface WireCall { path: string; search: string; method: string }

const service = {
  calls: [] as WireCall[],
  config: { sandbox: true } as Record<string, unknown>,
  experienceContent: 'owned experience',
};

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
}

const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
  const url = new URL(String(input));
  service.calls.push({ path: url.pathname, search: url.search, method: (init?.method || 'GET').toUpperCase() });
  switch (url.pathname) {
    case '/lingxi/v1/agents/lingxi/config': return jsonResponse(service.config);
    case '/lingxi/v1/agents/lingxi/identity': return jsonResponse({ content: '' });
    case '/lingxi/v1/agents/lingxi/agents-md': return jsonResponse({ content: '' });
    case '/lingxi/v1/agents/lingxi/public-agents-md': return jsonResponse({ content: '' });
    case '/lingxi/v1/agents/lingxi/experience': return jsonResponse({ content: service.experienceContent });
    case '/lingxi/v1/user-profile': return jsonResponse({ content: '' });
    case '/lingxi/v1/preferences/models': return jsonResponse({});
    default: throw new Error(`unexpected fetch ${url.pathname}`);
  }
});

const initialStore = useSettingsStore.getState();
let portSeed = 43810;

function connect(port: number) {
  const connection = createLocalServerConnection({
    serverPort: port,
    serverToken: `owned-${port}`,
    serverNodeKind: 'lingxi-service',
  })!;
  useSettingsStore.setState({
    activeServerConnectionId: 'local',
    serverConnections: { local: connection },
    activeServerConnection: connection,
  });
}

function seedAgent(agentId: string | null) {
  useSettingsStore.setState({
    agents: agentId ? [{ id: agentId, name: 'Hanako', yuan: 'lingxi', isPrimary: true } as never] : [],
    currentAgentId: agentId,
    settingsAgentId: null,
  });
}

function agentReadPaths(): string[] {
  return service.calls
    .map(call => call.path)
    .filter(path => path.startsWith('/lingxi/v1/agents/lingxi/') || path === '/lingxi/v1/user-profile' || path === '/lingxi/v1/preferences/models');
}

beforeEach(() => {
  service.calls = [];
  service.config = { sandbox: true };
  window.t = ((key: string) => key) as typeof window.t;
  portSeed += 1;
  connect(portSeed);
  seedAgent('lingxi');
  vi.stubGlobal('fetch', fetchMock);
  fetchMock.mockClear();
  useSettingsStore.setState({ settingsConfig: null, settingsConfigKey: null, settingsConfigStatus: 'idle', settingsConfigError: null });
});

afterEach(() => {
  vi.unstubAllGlobals();
  useSettingsStore.setState(initialStore, true);
  useSettingsStore.setState({ currentAgentId: null, agents: [], settingsConfig: null, settingsConfigKey: null, settingsConfigStatus: 'idle', settingsConfigError: null });
});

describe('R08 安全设置页共用法守卫：loadSettingsConfig 请求边界', () => {
  it('无 agentId 时 loadSettingsConfig 不发任何请求，并显式报 No settings agent selected', async () => {
    seedAgent(null);
    await loadSettingsConfig();
    expect(service.calls).toHaveLength(0);
    expect(fetchMock).not.toHaveBeenCalled();
    const state = useSettingsStore.getState();
    expect(state.settingsConfigStatus).toBe('error');
    expect(state.settingsConfigError).toContain('No settings agent selected');
  });

  it('经验未启用时不 GET experience；显式启用后才发起 experience 读取', async () => {
    await loadSettingsConfig();
    expect(agentReadPaths()).toContain('/lingxi/v1/agents/lingxi/config');
    expect(service.calls.some(call => call.path === '/lingxi/v1/agents/lingxi/experience')).toBe(false);
    expect(useSettingsStore.getState().settingsConfigStatus).toBe('ready');

    // 服务端配置显式 experience.enabled=true 后才允许拉取经验内容。
    service.calls = [];
    service.config = { sandbox: true, experience: { enabled: true } };
    useSettingsStore.setState({ settingsConfig: null, settingsConfigStatus: 'idle' });
    await loadSettingsConfig();
    expect(service.calls.filter(call => call.path === '/lingxi/v1/agents/lingxi/experience')).toHaveLength(1);
    expect(useSettingsStore.getState().settingsConfig?._experience).toBe(service.experienceContent);
  });

  it('无可用连接（resourceKey 无法定位真实服务）时 loadSettingsConfig 不发出网络请求', async () => {
    useSettingsStore.setState({ activeServerConnectionId: null, serverConnections: {}, activeServerConnection: null });
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      await loadSettingsConfig();
      expect(fetchMock).not.toHaveBeenCalled();
      const state = useSettingsStore.getState();
      expect(state.settingsConfigStatus).toBe('error');
      expect(state.settingsConfigError).toBeTruthy();
    } finally {
      quiet.mockRestore();
    }
  });
});
