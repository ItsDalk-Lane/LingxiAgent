/**
 * @vitest-environment jsdom
 */

import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { useSettingsStore, type ProviderSummary } from '../../settings/store';

const mocks = vi.hoisted(() => ({
  lingxiFetch: vi.fn(),
  loadSettingsConfig: vi.fn(async () => {}),
}));

vi.mock('../../settings/api', () => ({
  lingxiFetch: (...args: unknown[]) => mocks.lingxiFetch(...args),
  lingxiFetchJson: async (...args: unknown[]) => {
    const response = await mocks.lingxiFetch(...args);
    const data = await response.json();
    if (data?.error) throw new Error(data.error);
    return data;
  },
}));

vi.mock('../../settings/actions', () => ({
  loadSettingsConfig: () => mocks.loadSettingsConfig(),
  updateSettingsSnapshot: vi.fn(),
}));

vi.mock('../../hooks/use-config', () => ({
  invalidateConfigCache: vi.fn(),
}));

vi.mock('../../settings/helpers', () => ({
  t: (key: string, params?: Record<string, unknown>) => (
    params?.name ? `${key}:${params.name}` : key
  ),
  PROVIDER_PRESETS: [
    { value: 'deepseek', label: 'DeepSeek', url: 'https://api.deepseek.com', api: 'openai-completions' },
    { value: 'groq', label: 'Groq', url: 'https://api.groq.com/openai/v1', api: 'openai-completions' },
  ],
  API_FORMAT_OPTIONS: [
    { value: 'openai-completions', label: 'OpenAI Compatible' },
  ],
}));

vi.mock('../../settings/tabs/providers/SearchProviderSection', () => ({
  SearchProviderSection: () => <div data-testid="search-provider-section" />,
}));

vi.mock('../../settings/tabs/providers/SearchApiKeyConfig', () => ({
  SearchApiKeyConfig: () => <div data-testid="search-api-key-config" />,
}));

vi.mock('../../settings/tabs/providers/ProviderModelList', () => ({
  ProviderModelList: () => <div data-testid="provider-model-list" />,
}));

import { ProvidersTab } from '../../settings/tabs/ProvidersTab';

function jsonResponse(body: unknown): Response {
  return { json: async () => body } as Response;
}

function providerSummary(overrides: Partial<ProviderSummary>): ProviderSummary {
  return {
    type: 'api-key',
    auth_type: 'api-key',
    display_name: '',
    base_url: '',
    api: 'openai-completions',
    api_key: '',
    models: [],
    custom_models: [],
    has_credentials: false,
    supports_oauth: false,
    is_configured: true,
    can_delete: false,
    ...overrides,
  };
}

describe('ProvidersTab provider-scoped form state', () => {
  const providersSummary = {
    deepseek: providerSummary({
      display_name: 'DeepSeek',
      base_url: 'https://api.deepseek.com',
      api_key: 'saved-deepseek-key',
      has_credentials: true,
    }),
    groq: providerSummary({
      display_name: 'Groq',
      base_url: 'https://api.groq.com/openai/v1',
      api_key: '',
    }),
  };

  beforeEach(() => {
    vi.clearAllMocks();
    mocks.loadSettingsConfig.mockReset();
    mocks.loadSettingsConfig.mockResolvedValue(undefined);
    mocks.lingxiFetch.mockImplementation((path: string) => {
      if (path === '/api/providers/summary') {
        return Promise.resolve(jsonResponse({ providers: providersSummary }));
      }
      return Promise.resolve(jsonResponse({ ok: true }));
    });
    useSettingsStore.setState({
      providersSummary,
      selectedProviderId: 'deepseek',
      settingsConfig: {
        providers: {
          deepseek: { api_key: 'saved-deepseek-key' },
          groq: {},
        },
      },
    });
  });

  afterEach(() => {
    cleanup();
  });

  it('does not carry an unsaved api key draft when switching providers', async () => {
    const { container } = render(<ProvidersTab />);

    // 持久供应商由配置目录恢复，当前选择只决定右栏编辑对象。
    const deepseekInput = await screen.findByDisplayValue('saved-deepseek-key');
    fireEvent.change(deepseekInput, { target: { value: 'unsaved-deepseek-draft' } });
    expect(screen.getByDisplayValue('unsaved-deepseek-draft')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /Groq/ }));

    await waitFor(() => {
      expect(useSettingsStore.getState().selectedProviderId).toBe('groq');
    });
    expect(screen.queryByDisplayValue('unsaved-deepseek-draft')).not.toBeInTheDocument();
    const groqKeyInput = container.querySelector('input[type="password"]');
    expect(groqKeyInput).toHaveValue('');
  });

  it('treats registry-only preset providers as setup entries after deletion', async () => {
    const registryOnlySummary = {
      deepseek: providerSummary({
        display_name: 'DeepSeek',
        base_url: 'https://api.deepseek.com',
        models: [],
        has_credentials: false,
        can_delete: false,
        config_status: 'needs_setup',
        is_configured: false,
      }),
    };

    mocks.lingxiFetch.mockImplementation((path: string) => {
      if (path === '/api/providers/summary') {
        return Promise.resolve(jsonResponse({ providers: registryOnlySummary }));
      }
      return Promise.resolve(jsonResponse({ ok: true }));
    });
    useSettingsStore.setState({
      providersSummary: registryOnlySummary,
      selectedProviderId: null,
      settingsConfig: { providers: {} },
    });

    render(<ProvidersTab />);

    // 未注册预设只出现在「添加服务商」选择弹层中
    fireEvent.click(await screen.findByRole('button', { name: /settings.providers.addService/ }));
    const deepseekButton = await screen.findByRole('button', { name: /DeepSeek/ });
    fireEvent.click(deepseekButton);

    await waitFor(() => {
      expect(useSettingsStore.getState().selectedProviderId).toBe('deepseek');
    });
    // 这是当前页面的未保存草稿，必须提供本地移除入口，但不调用后端删除。
    expect(screen.getByRole('button', { name: 'settings.providers.delete' })).toBeInTheDocument();
  });

  it('keeps registry-only non-preset providers visible as setup entries', async () => {
    const registryOnlySummary = {
      baichuan: providerSummary({
        display_name: 'Baichuan',
        base_url: 'https://api.baichuan-ai.com/v1',
        models: [],
        has_credentials: false,
        can_delete: false,
        config_status: 'needs_setup',
        is_configured: false,
      }),
    };

    mocks.lingxiFetch.mockImplementation((path: string) => {
      if (path === '/api/providers/summary') {
        return Promise.resolve(jsonResponse({ providers: registryOnlySummary }));
      }
      return Promise.resolve(jsonResponse({ ok: true }));
    });
    useSettingsStore.setState({
      providersSummary: registryOnlySummary,
      selectedProviderId: null,
      settingsConfig: { providers: {} },
    });

    render(<ProvidersTab />);

    // registry-only 服务商需通过「添加服务商」选择界面加入
    fireEvent.click(screen.getByRole('button', { name: /settings.providers.addService/ }));
    const baichuanButton = await screen.findByRole('button', { name: /Baichuan/ });
    fireEvent.click(baichuanButton);

    await waitFor(() => {
      expect(useSettingsStore.getState().selectedProviderId).toBe('baichuan');
    });
    expect(screen.getByDisplayValue('https://api.baichuan-ai.com/v1')).toBeInTheDocument();
  });

  it('saves registry-only non-preset providers through the initial setup payload', async () => {
    const registryOnlySummary = {
      agnes: providerSummary({
        display_name: 'Agnes',
        base_url: 'https://apihub.agnes-ai.com/v1',
        api: 'openai-completions',
        models: [],
        has_credentials: false,
        can_delete: false,
        config_status: 'needs_setup',
        is_configured: false,
      }),
    };

    mocks.lingxiFetch.mockImplementation((path: string) => {
      if (path === '/api/providers/summary') {
        return Promise.resolve(jsonResponse({ providers: registryOnlySummary }));
      }
      return Promise.resolve(jsonResponse({ ok: true }));
    });
    useSettingsStore.setState({
      providersSummary: registryOnlySummary,
      selectedProviderId: null,
      settingsConfig: { providers: {} },
    });

    const { container } = render(<ProvidersTab />);

    fireEvent.click(screen.getByRole('button', { name: /settings.providers.addService/ }));
    fireEvent.click(await screen.findByRole('button', { name: /Agnes/ }));

    const input = await waitFor(() => container.querySelector('input[type="password"]') as HTMLInputElement);
    fireEvent.change(input, { target: { value: 'agnes-key' } });
    const saveButton = container.querySelector('button[title="settings.providers.verifyConnection"]') as HTMLButtonElement;
    fireEvent.click(saveButton);

    await waitFor(() => expect(mocks.lingxiFetch).toHaveBeenCalledWith(
      '/api/config',
      expect.objectContaining({ method: 'PUT' }),
    ));
    const configCall = mocks.lingxiFetch.mock.calls.find(([path]) => path === '/api/config');
    expect(JSON.parse(String((configCall?.[1] as RequestInit).body))).toEqual({
      providers: {
        agnes: {
          base_url: 'https://apihub.agnes-ai.com/v1',
          api_key: 'agnes-key',
          api: 'openai-completions',
          seed_default_models: true,
        },
      },
    });
  });

  it('restores every persisted provider after unmount while selection only controls styling', async () => {
    const mixedSummary = {
      deepseek: providerSummary({
        display_name: 'DeepSeek',
        base_url: 'https://api.deepseek.com',
        has_credentials: true,
        models: ['deepseek-chat'],
      }),
      groq: providerSummary({
        display_name: 'Groq',
        base_url: 'https://api.groq.com/openai/v1',
        has_credentials: true,
        models: ['groq-chat'],
      }),
      baichuan: providerSummary({
        display_name: 'Baichuan',
        base_url: 'https://api.baichuan-ai.com/v1',
        is_configured: false,
      }),
      'my-proxy': providerSummary({
        display_name: 'My Proxy',
        base_url: 'https://proxy.example.com/v1',
        has_credentials: true,
        models: ['proxy-chat'],
        can_delete: true,
      }),
    };

    mocks.lingxiFetch.mockImplementation((path: string) => {
      if (path === '/api/providers/summary') {
        return Promise.resolve(jsonResponse({ providers: mixedSummary }));
      }
      return Promise.resolve(jsonResponse({ ok: true }));
    });
    useSettingsStore.setState({
      providersSummary: mixedSummary,
      selectedProviderId: 'my-proxy',
      settingsConfig: {
        providers: {
          deepseek: { api_key: 'deepseek-key' },
          groq: { api_key: 'groq-key' },
          'my-proxy': { api_key: 'proxy-key' },
        },
      },
    });

    const first = render(<ProvidersTab />);

    expect(screen.getByRole('button', { name: /DeepSeek/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Groq/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /My Proxy/ })).toBeInTheDocument();
    expect(useSettingsStore.getState().selectedProviderId).toBe('my-proxy');

    first.unmount();
    render(<ProvidersTab />);

    expect(screen.getByRole('button', { name: /DeepSeek/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Groq/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /My Proxy/ })).toBeInTheDocument();
    expect(useSettingsStore.getState().selectedProviderId).toBe('my-proxy');
  });

  it('keeps persisted providers when a draft is added, and drops only the draft after remount', async () => {
    const mixedSummary = {
      deepseek: providerSummary({ display_name: 'DeepSeek', has_credentials: true }),
      groq: providerSummary({ display_name: 'Groq', has_credentials: true }),
      baichuan: providerSummary({
        display_name: 'Baichuan',
        base_url: 'https://api.baichuan-ai.com/v1',
        is_configured: false,
      }),
    };
    mocks.lingxiFetch.mockImplementation((path: string) => Promise.resolve(jsonResponse(
      path === '/api/providers/summary' ? { providers: mixedSummary } : { ok: true },
    )));
    useSettingsStore.setState({
      providersSummary: mixedSummary,
      selectedProviderId: 'deepseek',
      settingsConfig: { providers: { deepseek: {}, groq: {} } },
    });

    const first = render(<ProvidersTab />);
    fireEvent.click(screen.getByRole('button', { name: /settings.providers.addService/ }));
    fireEvent.click(await screen.findByRole('button', { name: /Baichuan/ }));

    expect(screen.getByRole('button', { name: /DeepSeek/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Groq/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Baichuan/ })).toBeInTheDocument();

    first.unmount();
    useSettingsStore.setState({ selectedProviderId: 'deepseek' });
    render(<ProvidersTab />);

    expect(screen.getByRole('button', { name: /DeepSeek/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Groq/ })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Baichuan/ })).not.toBeInTheDocument();
  });

  it('removes an unsaved draft without touching persisted providers', async () => {
    const mixedSummary = {
      deepseek: providerSummary({ display_name: 'DeepSeek', has_credentials: true }),
      groq: providerSummary({ display_name: 'Groq', has_credentials: true }),
      baichuan: providerSummary({ display_name: 'Baichuan', is_configured: false }),
    };
    mocks.lingxiFetch.mockImplementation((path: string) => Promise.resolve(jsonResponse(
      path === '/api/providers/summary' ? { providers: mixedSummary } : { ok: true },
    )));
    useSettingsStore.setState({
      providersSummary: mixedSummary,
      selectedProviderId: 'deepseek',
      settingsConfig: { providers: { deepseek: {}, groq: {} } },
    });

    render(<ProvidersTab />);
    fireEvent.click(screen.getByRole('button', { name: /settings.providers.addService/ }));
    fireEvent.click(await screen.findByRole('button', { name: /Baichuan/ }));
    fireEvent.click(screen.getByRole('button', { name: 'settings.providers.delete' }));

    expect(screen.queryByRole('button', { name: /Baichuan/ })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: /DeepSeek/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Groq/ })).toBeInTheDocument();
    expect(mocks.lingxiFetch).not.toHaveBeenCalledWith('/api/config', expect.anything());
  });

  it('removes a deleted configured provider from the list and keeps a built-in provider in the picker', async () => {
    let currentSummary: Record<string, ProviderSummary> = {
      deepseek: providerSummary({
        display_name: 'DeepSeek',
        has_credentials: true,
        can_delete: true,
      }),
      groq: providerSummary({ display_name: 'Groq', has_credentials: true }),
    };
    mocks.lingxiFetch.mockImplementation(async (path: string, options?: RequestInit) => {
      if (path === '/api/config' && options?.method === 'PUT') {
        currentSummary = {
          ...currentSummary,
          deepseek: providerSummary({ display_name: 'DeepSeek', is_configured: false }),
        };
        return jsonResponse({ ok: true });
      }
      if (path === '/api/providers/summary') return jsonResponse({ providers: currentSummary });
      return jsonResponse({ ok: true });
    });
    mocks.loadSettingsConfig.mockImplementation(async () => {
      useSettingsStore.setState({ settingsConfig: { providers: { groq: {} } } });
    });
    useSettingsStore.setState({
      providersSummary: currentSummary,
      selectedProviderId: 'deepseek',
      settingsConfig: { providers: { deepseek: {}, groq: {} } },
    });

    render(<ProvidersTab />);
    fireEvent.click(screen.getByRole('button', { name: 'settings.providers.delete' }));
    fireEvent.click(screen.getAllByRole('button', { name: 'settings.providers.delete' }).at(-1) as HTMLButtonElement);

    await waitFor(() => expect(screen.queryByRole('button', { name: /DeepSeek/ })).not.toBeInTheDocument());
    expect(screen.getByRole('button', { name: /Groq/ })).toBeInTheDocument();
    expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/config', expect.objectContaining({
      method: 'PUT',
      body: JSON.stringify({ providers: { deepseek: null } }),
    }));

    fireEvent.click(screen.getByRole('button', { name: /settings.providers.addService/ }));
    expect(await screen.findByRole('button', { name: /DeepSeek/ })).toBeInTheDocument();
  });

  it('keeps a configured provider and skips authoritative reload when deletion returns an error', async () => {
    const configuredSummary = {
      deepseek: providerSummary({
        display_name: 'DeepSeek',
        has_credentials: true,
        can_delete: true,
      }),
    };
    mocks.lingxiFetch.mockImplementation(async (path: string, options?: RequestInit) => {
      if (path === '/api/config' && options?.method === 'PUT') {
        return jsonResponse({ error: 'delete sync failed' });
      }
      if (path === '/api/providers/summary') return jsonResponse({ providers: configuredSummary });
      return jsonResponse({ ok: true });
    });
    useSettingsStore.setState({
      providersSummary: configuredSummary,
      selectedProviderId: 'deepseek',
      settingsConfig: { providers: { deepseek: {} } },
      toastMessage: '',
      toastType: '',
      toastVisible: false,
    });

    render(<ProvidersTab />);
    fireEvent.click(screen.getByRole('button', { name: 'settings.providers.delete' }));
    fireEvent.click(screen.getAllByRole('button', { name: 'settings.providers.delete' }).at(-1) as HTMLButtonElement);

    await waitFor(() => expect(useSettingsStore.getState().toastMessage).toContain('delete sync failed'));
    expect(screen.getByRole('button', { name: /DeepSeek/ })).toBeInTheDocument();
    expect(useSettingsStore.getState().selectedProviderId).toBe('deepseek');
    expect(mocks.loadSettingsConfig).not.toHaveBeenCalled();
  });

  it('offers api and search sub tabs instead of usage/models', () => {
    render(<ProvidersTab />);

    expect(screen.queryByRole('tab', { name: 'settings.providers.subtab.usage' })).not.toBeInTheDocument();
    expect(screen.queryByRole('tab', { name: 'settings.providers.subtab.models' })).not.toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'settings.providers.subtab.api' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'settings.providers.subtab.search' })).toBeInTheDocument();
  });

  it('switches to the search sub tab through navigateSettings and renders search config', async () => {
    render(<ProvidersTab />);

    fireEvent.click(screen.getByRole('tab', { name: 'settings.providers.subtab.search' }));

    await waitFor(() => {
      expect(useSettingsStore.getState().activeSubTabs.providers).toBe('search');
    });
    expect(screen.getByTestId('search-provider-section')).toBeInTheDocument();
    expect(screen.getByTestId('search-api-key-config')).toBeInTheDocument();
  });
});
