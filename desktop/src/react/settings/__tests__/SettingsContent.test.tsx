// @vitest-environment jsdom

import { cleanup, render, screen, waitFor } from '@testing-library/react';
import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { SettingsContent, normalizeSettingsTab } from '../SettingsContent';
import { useSettingsStore } from '../store';

const actionMocks = vi.hoisted(() => ({
  loadAgents: vi.fn(async () => {}),
  loadAvatars: vi.fn(async () => {}),
  loadSettingsSnapshot: vi.fn(async () => {}),
  loadSettingsModels: vi.fn(async () => {}),
  loadProvidersSummary: vi.fn(async () => {}),
}));

vi.mock('../actions', () => ({
  loadAgents: actionMocks.loadAgents,
  loadAvatars: actionMocks.loadAvatars,
  loadSettingsConfig: vi.fn(async () => {}),
  loadSettingsSnapshot: actionMocks.loadSettingsSnapshot,
  loadSettingsModels: actionMocks.loadSettingsModels,
  loadProvidersSummary: actionMocks.loadProvidersSummary,
  loadPluginSettings: vi.fn(async () => {}),
  updateSettingsSnapshot: vi.fn(),
}));

vi.mock('../api', () => ({
  lingxiFetch: vi.fn(async (url: string) => {
    if (url === '/api/config') {
      return new Response(JSON.stringify({ locale: 'zh-CN' }));
    }
    return new Response(JSON.stringify({ experiments: [] }));
  }),
}));

describe('SettingsContent tab heading', () => {
  let settingsChangedHandler: ((type: string, data: unknown) => void) | undefined;

  beforeEach(() => {
    settingsChangedHandler = undefined;
    actionMocks.loadSettingsModels.mockClear();
    // 个别用例会给这几个 mock 装定时实现（如延迟 resolve 的 loadAgents），
    // clearAllMocks 不清实现，这里统一回到默认值防止泄漏到后续用例。
    actionMocks.loadAgents.mockReset().mockResolvedValue(undefined);
    actionMocks.loadAvatars.mockReset().mockResolvedValue(undefined);
    actionMocks.loadSettingsSnapshot.mockReset().mockResolvedValue(undefined);
    actionMocks.loadProvidersSummary.mockReset();
    actionMocks.loadProvidersSummary.mockResolvedValue(undefined);
    window.t = ((key: string) => key) as typeof window.t;
    window.i18n = {
      locale: 'zh-CN',
      defaultName: 'Hana',
      _data: {},
      _agentOverrides: {},
      load: vi.fn(async () => {}),
      setAgentOverrides: vi.fn(),
      t: ((key: string) => key) as typeof window.t,
    };
    window.platform = {
      getServerPort: vi.fn(async () => 3000),
      getServerToken: vi.fn(async () => null),
      getPlatform: vi.fn(async () => 'darwin'),
      onSwitchTab: vi.fn(),
      onSettingsChanged: vi.fn((handler: (type: string, data: unknown) => void) => {
        settingsChangedHandler = handler;
        return vi.fn();
      }),
      onServerRestarted: vi.fn(),
    } as unknown as typeof window.platform;
    useSettingsStore.setState({
      activeTab: 'experiments',
      platformName: 'darwin',
      ready: true,
    } as never);
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('renders experiment copy as a tab-level description', async () => {
    render(React.createElement(SettingsContent, { variant: 'window' }));

    const description = screen.getByText('settings.experiments.description');
    expect(description.tagName).toBe('P');

    await waitFor(() => {
      expect(screen.getByText('settings.experiments.empty')).toBeTruthy();
    });
  });

  it('renders the built-in browser settings tab', async () => {
    useSettingsStore.setState({
      activeTab: 'browser',
      platformName: 'darwin',
      ready: true,
      settingsSnapshot: {
        status: 'ready',
        key: 'snapshot:browser',
        requestId: 1,
        data: {
          agentId: 'hana',
          config: {},
          identity: '',
          agents: '',
          publicAgents: '',
          userProfile: '',
          experience: '',
          pinned: { pins: [] },
          globalModels: {},
          preferences: {
            quickChat: {},
            notifications: {},
            bridge: { permissionMode: 'auto', readOnly: false, receiptEnabled: true },
            speechRecognition: {},
            experiments: [],
            browser: { acceptCookies: true, agentOpenBehavior: 'smart' },
          },
          plugins: {
            allowFullAccess: false,
            devToolsEnabled: false,
            userDir: '',
            settingsTabs: [],
          },
        },
      },
    } as never);

    render(React.createElement(SettingsContent, { variant: 'window' }));

    expect(screen.getAllByText('settings.tabs.browser').length).toBeGreaterThan(0);
    expect(screen.getByText('settings.browser.acceptCookies')).toBeTruthy();
    expect(screen.getByText('settings.browser.clearCookies')).toBeTruthy();
    expect(screen.getByText('settings.browser.agentOpenBehavior')).toBeTruthy();
  });

  it('lists Connectors as a static tab with no plugin-contributed tabs present', () => {
    // MCP is a core module now: its tab must appear from the static table, not
    // from a plugin settings-tab contribution.
    useSettingsStore.setState({
      activeTab: 'agent',
      platformName: 'darwin',
      ready: true,
    } as never);

    render(React.createElement(SettingsContent, { variant: 'window' }));

    const mcpNavButton = document.querySelector('button[data-tab="mcp"]');
    expect(mcpNavButton).toBeTruthy();
    expect(mcpNavButton?.textContent).toContain('settings.tabs.mcp');
  });

  it('reloads the shared settings model catalog when models-changed arrives', async () => {
    render(React.createElement(SettingsContent, { variant: 'window' }));

    await waitFor(() => expect(settingsChangedHandler).toBeTypeOf('function'));
    actionMocks.loadSettingsModels.mockClear();
    settingsChangedHandler?.('models-changed', { reason: 'provider' });

    expect(actionMocks.loadSettingsModels).toHaveBeenCalledTimes(1);
  });

  it('reloads the shared settings model catalog from a same-window models event', async () => {
    render(React.createElement(SettingsContent, { variant: 'modal' }));

    await waitFor(() => expect(actionMocks.loadSettingsModels).toHaveBeenCalled());
    actionMocks.loadSettingsModels.mockClear();
    window.dispatchEvent(new CustomEvent('hana-models-changed', {
      detail: { reason: 'provider' },
    }));

    await waitFor(() => expect(actionMocks.loadSettingsModels).toHaveBeenCalledTimes(1));
  });

  it('normalizes legacy media and computer tab ids to their replacements', () => {
    expect(normalizeSettingsTab('media')).toBe('models');
    expect(normalizeSettingsTab('computer')).toBe('experiments');
    expect(normalizeSettingsTab('providers')).toBe('providers');
    expect(normalizeSettingsTab('models')).toBe('models');
  });

  it('shows a models nav item and hides the legacy media nav item', async () => {
    useSettingsStore.setState({
      activeTab: 'agent',
      platformName: 'darwin',
      ready: true,
    } as never);

    render(React.createElement(SettingsContent, { variant: 'window' }));

    const modelsNavButton = document.querySelector('button[data-tab="models"]');
    const mediaNavButton = document.querySelector('button[data-tab="media"]');
    expect(modelsNavButton).toBeTruthy();
    expect(modelsNavButton?.textContent).toContain('settings.tabs.models');
    expect(mediaNavButton).toBeFalsy();
  });

  it('loads the provider summary inside init, only after the server connection is ready', async () => {
    // 回归：供应商摘要曾由 ProvidersTab 挂载时自行拉取——子组件 effect 先于
    // initSettings 执行，连接未就绪时 fetch 必败且静默后无重试，供应商页打开即空白。
    useSettingsStore.setState({
      activeTab: 'providers',
      serverPort: null,
      serverToken: null,
      activeServerConnection: null,
      activeServerConnectionId: null,
      serverConnections: {},
      ready: false,
    } as never);
    actionMocks.loadProvidersSummary.mockImplementation(async () => {
      // 被调用时连接三要素必须已经写入 store（init 先并行解析 port/token/platform）
      expect(useSettingsStore.getState().serverPort).toBe(3000);
    });

    render(React.createElement(SettingsContent, { variant: 'window' }));

    await waitFor(() => expect(actionMocks.loadProvidersSummary).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(useSettingsStore.getState().ready).toBe(true));
  });

  it('loads the settings snapshot only after agents have resolved the agent id', async () => {
    // 回归（v0.1.30）：initSettings 曾把 loadAgents 与 loadSettingsSnapshot 放进
    // 同一个 Promise.all 并行。快照在被调用的瞬间同步读 getSettingsAgentId()，
    // 此时 agents 的 fetch 尚未返回，agentId 必为 null → 快照以「No settings
    // agent selected」必败且无任何重试，settingsConfig 恒为 null——关于页
    // 「自动检查更新 / 接收测试版更新」两个 Toggle 永久卡 loading 脉冲且点不动。
    let releaseAgents: (() => void) | undefined;
    actionMocks.loadAgents.mockImplementation(async () => {
      await new Promise<void>((resolve) => { releaseAgents = resolve; });
      useSettingsStore.setState({ currentAgentId: 'lingxi' } as never);
    });
    const agentIdAtSnapshotCall: Array<string | null> = [];
    actionMocks.loadSettingsSnapshot.mockImplementation(async () => {
      agentIdAtSnapshotCall.push(useSettingsStore.getState().getSettingsAgentId());
    });

    useSettingsStore.setState({
      activeTab: 'about',
      currentAgentId: null,
      settingsAgentId: null,
      serverPort: null,
      serverToken: null,
      activeServerConnection: null,
      activeServerConnectionId: null,
      serverConnections: {},
      ready: false,
    } as never);

    render(React.createElement(SettingsContent, { variant: 'window' }));

    // init 已推进到等待 agents：快照绝不许抢跑，mask 也不许提前放行
    await waitFor(() => expect(releaseAgents).toBeTypeOf('function'));
    expect(actionMocks.loadSettingsSnapshot).not.toHaveBeenCalled();
    expect(useSettingsStore.getState().ready).toBe(false);

    releaseAgents?.();
    await waitFor(() => expect(actionMocks.loadSettingsSnapshot).toHaveBeenCalledTimes(1));
    expect(agentIdAtSnapshotCall[0]).toBe('lingxi');
    await waitFor(() => expect(useSettingsStore.getState().ready).toBe(true));
  });

  it('reopens without the loading mask when cached config exists (silent background refresh)', async () => {
    // 回归：store 是模块级 singleton，重开设置时上次数据还在。旧行为每次打开都
    // 强制 ready:false 用全屏 mask 盖住已有内容（用户感知的「空白一段」）。
    useSettingsStore.setState({
      activeTab: 'providers',
      ready: true,
      settingsConfig: { providers: { deepseek: { api_key: 'k' } } },
      providersSummary: {},
      platformName: 'darwin',
    } as never);

    render(React.createElement(SettingsContent, { variant: 'window' }));

    // 有缓存数据：绝不显示全屏 mask，但 init 仍在后台静默刷新
    expect(document.querySelector('.settings-loading-mask')).toBeNull();
    await waitFor(() => expect(actionMocks.loadProvidersSummary).toHaveBeenCalledTimes(1));
    expect(useSettingsStore.getState().ready).toBe(true);
  });
});
