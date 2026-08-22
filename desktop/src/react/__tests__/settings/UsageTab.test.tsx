/**
 * @vitest-environment jsdom
 */

import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { useSettingsStore } from '../../settings/store';

const mocks = vi.hoisted(() => ({
  loadObservabilityHealth: vi.fn(),
  loadObservabilitySettings: vi.fn(),
  queryObservabilityAggregate: vi.fn(),
  queryObservabilityCalls: vi.fn(),
  queryObservabilityTraces: vi.fn(),
  updateObservabilitySettings: vi.fn(),
}));

vi.mock('../../settings/tabs/observability/model-observability-actions', async (importOriginal) => {
  const original = await importOriginal<typeof import('../../settings/tabs/observability/model-observability-actions')>();
  return {
    ...original,
    loadObservabilityHealth: (...args: unknown[]) => mocks.loadObservabilityHealth(...args),
    loadObservabilitySettings: (...args: unknown[]) => mocks.loadObservabilitySettings(...args),
    queryObservabilityAggregate: (...args: unknown[]) => mocks.queryObservabilityAggregate(...args),
    queryObservabilityCalls: (...args: unknown[]) => mocks.queryObservabilityCalls(...args),
    queryObservabilityTraces: (...args: unknown[]) => mocks.queryObservabilityTraces(...args),
    updateObservabilitySettings: (...args: unknown[]) => mocks.updateObservabilitySettings(...args),
  };
});

vi.mock('../../settings/actions', () => ({
  loadAgents: vi.fn(async () => {}),
  loadAvatars: vi.fn(async () => {}),
  loadSettingsConfig: vi.fn(async () => {}),
  loadSettingsSnapshot: vi.fn(async () => {}),
  loadSettingsModels: vi.fn(async () => {}),
  loadPluginSettings: vi.fn(async () => {}),
}));

vi.mock('../../settings/api', () => ({
  lingxiFetch: vi.fn(async (url: string) => new Response(JSON.stringify(
    url === '/api/config' ? { locale: 'zh-CN' } : {},
  ))),
  lingxiFetchJson: vi.fn(async () => ({})),
}));

import { SettingsNav } from '../../settings/SettingsNav';
import { SettingsContent } from '../../settings/SettingsContent';
import { UsageTab } from '../../settings/tabs/UsageTab';

const ABSENT_HEALTH = {
  recordingStatus: 'disabled',
  storeDisabledReasonCode: null,
  persistTraceMetadata: false,
  persistPayloads: false,
  persistBlobs: false,
  queuedTraceEvents: 0,
  queuedPayloadRecords: 0,
  queuedBlobs: 0,
  queuedUsageEntries: 0,
  droppedTraceEvents: 0,
  droppedPayloadRecords: 0,
  droppedBlobs: 0,
  droppedUsageEntries: 0,
  writeFailures: 0,
  maintenanceErrors: 0,
  lastSuccessfulFlushAt: null,
  interruptedByRestartCalls: 0,
  atRestEncryption: false,
  query: {
    queryStatus: 'absent',
    queryStatusReason: 'database_absent',
    schemaVersion: null,
    accountingProjectionAvailable: false,
    oldestCallAt: null,
    newestCallAt: null,
    callCount: 0,
    traceCount: 0,
    payloadRecordCount: 0,
    usageProjectionCount: 0,
    dataCompleteness: {
      droppedTraceEvents: 0,
      droppedPayloadRecords: 0,
      droppedBlobs: 0,
      interruptedByRestartCalls: 0,
    },
  },
};

const DISABLED_SETTINGS = {
  desired: {
    enabled: false,
    persistTraceMetadata: true,
    persistPayloads: false,
    persistBlobs: false,
    retention: { traceDays: 180, payloadDays: 30, blobDays: 30 },
  },
  effective: {
    recordingStatus: 'disabled',
    storeDisabledReasonCode: 'disabled_by_policy',
    persistTraceMetadata: false,
    persistPayloads: false,
    persistBlobs: false,
    schemaVersion: null,
  },
  cryptographicallyEncryptedAtRest: false,
};

describe('UsageTab settings page registration (Phase 9: Model Observatory)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.loadObservabilityHealth.mockResolvedValue(ABSENT_HEALTH);
    mocks.loadObservabilitySettings.mockResolvedValue(DISABLED_SETTINGS);
    mocks.queryObservabilityAggregate.mockRejectedValue(new Error('not initialized'));
    mocks.queryObservabilityCalls.mockRejectedValue(new Error('not initialized'));
    mocks.queryObservabilityTraces.mockRejectedValue(new Error('not initialized'));
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
      onSettingsChanged: vi.fn(() => vi.fn()),
      onServerRestarted: vi.fn(),
    } as unknown as typeof window.platform;
    useSettingsStore.setState({
      activeTab: 'usage',
      platformName: 'darwin',
      ready: true,
    } as never);
  });

  afterEach(() => {
    cleanup();
  });

  it('places the usage nav item strictly after providers and models', () => {
    render(<SettingsNav />);

    const tabIds = [...document.querySelectorAll('button[data-tab]')].map(
      el => el.getAttribute('data-tab'),
    );
    const providersIndex = tabIds.indexOf('providers');
    const modelsIndex = tabIds.indexOf('models');
    const usageIndex = tabIds.indexOf('usage');

    expect(providersIndex).toBeGreaterThanOrEqual(0);
    expect(modelsIndex).toBe(providersIndex + 1);
    expect(usageIndex).toBe(modelsIndex + 1);
    expect(tabIds).not.toContain('media');

    const usageButton = document.querySelector('button[data-tab="usage"]') as HTMLButtonElement;
    // 内部 tab id 不变（§五），可见名称走 settings.tabs.usage（值已升级为模型观测）。
    expect(usageButton.textContent).toContain('settings.tabs.usage');
    expect(usageButton.querySelector('svg')).not.toBeNull();
  });

  it('renders the usage tab content through SettingsContent when usage is active', async () => {
    const { container } = render(<SettingsContent variant="window" />);

    await waitFor(() => {
      expect(container.querySelector('div[data-tab="usage"]')).not.toBeNull();
    });
    // 页标题与导航同源（settings.tabs.usage），绝不漂移（§六）。
    expect(screen.getAllByText('settings.tabs.usage').length).toBeGreaterThan(0);
    const tabIds = [...container.querySelectorAll('button[data-tab]')].map(
      el => el.getAttribute('data-tab'),
    );
    expect(tabIds.indexOf('models')).toBe(tabIds.indexOf('providers') + 1);
    expect(tabIds.indexOf('usage')).toBe(tabIds.indexOf('models') + 1);
    expect(tabIds).not.toContain('media');
  });

  it('shows the onboarding empty state when recording is disabled and the store is absent', async () => {
    render(<UsageTab />);
    // §九十八：disabled + store absent → onboarding（「启用模型观测」按钮），
    // 不是报错页。
    await waitFor(() => {
      expect(screen.getByText('settings.observability.onboarding.enable')).toBeInTheDocument();
    });
  });
});
