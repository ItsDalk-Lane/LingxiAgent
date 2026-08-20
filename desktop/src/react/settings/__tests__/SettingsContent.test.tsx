// @vitest-environment jsdom

import { cleanup, render, screen, waitFor } from '@testing-library/react';
import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { SettingsContent, normalizeSettingsTab } from '../SettingsContent';
import { useSettingsStore } from '../store';

const actionMocks = vi.hoisted(() => ({
  loadSettingsModels: vi.fn(async () => {}),
}));

vi.mock('../actions', () => ({
  loadAgents: vi.fn(async () => {}),
  loadAvatars: vi.fn(async () => {}),
  loadSettingsConfig: vi.fn(async () => {}),
  loadSettingsSnapshot: vi.fn(async () => {}),
  loadSettingsModels: actionMocks.loadSettingsModels,
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
});
