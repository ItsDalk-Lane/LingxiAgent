/**
 * @vitest-environment jsdom
 *
 * 模型观测子标签页测试：三标签渲染与默认落点、Group By 只在 Token 用量页、
 * 各子页筛选状态互相独立（且切换子标签后面板卸载但筛选不丢）。
 */
import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { useSettingsStore } from '../../../settings/store';

const mocks = vi.hoisted(() => ({
  loadObservabilityHealth: vi.fn(),
  loadObservabilitySettings: vi.fn(),
  queryObservabilityAggregate: vi.fn(),
  queryObservabilityCalls: vi.fn(),
  queryObservabilityTraces: vi.fn(),
  updateObservabilitySettings: vi.fn(),
}));

vi.mock('../../../settings/tabs/observability/model-observability-actions', async (importOriginal) => {
  const original = await importOriginal<typeof import('../../../settings/tabs/observability/model-observability-actions')>();
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

import { ModelObservabilitySection } from '../../../settings/tabs/observability/ModelObservabilitySection';

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

const ALWAYS_ON_SETTINGS = {
  desired: {
    enabled: true,
    persistTraceMetadata: true,
    persistPayloads: true,
    persistBlobs: true,
    retention: { traceDays: 180, payloadDays: 30, blobDays: 30 },
  },
  effective: {
    recordingStatus: 'disabled',
    storeDisabledReasonCode: 'not_installed',
    persistTraceMetadata: false,
    persistPayloads: false,
    persistBlobs: false,
    schemaVersion: null,
  },
  cryptographicallyEncryptedAtRest: false,
};

const SUBTAB_USAGE = 'settings.observability.subtab.usage';
const SUBTAB_LEDGER = 'settings.observability.ledger.title';
const SUBTAB_TRACES = 'settings.observability.trace.title';

describe('ModelObservabilitySection sub-tabs', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.loadObservabilityHealth.mockResolvedValue(ABSENT_HEALTH);
    mocks.loadObservabilitySettings.mockResolvedValue(ALWAYS_ON_SETTINGS);
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
      activeSubTabs: {},
      platformName: 'darwin',
      ready: true,
    } as never);
  });

  afterEach(() => {
    cleanup();
  });

  it('renders three sub-tabs and lands on Token usage by default', async () => {
    render(<ModelObservabilitySection />);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: SUBTAB_USAGE })).toBeInTheDocument();
    });
    expect(screen.getByRole('tab', { name: SUBTAB_LEDGER })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: SUBTAB_TRACES })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: SUBTAB_USAGE })).toHaveAttribute('aria-selected', 'true');
    expect(screen.getByRole('tab', { name: SUBTAB_LEDGER })).toHaveAttribute('aria-selected', 'false');

    // Token 用量页默认挂载：指标卡 + 四张常驻图表各自发起聚合查询；
    // 台账/轨迹未挂载（不发起自己的查询）。
    expect(mocks.queryObservabilityAggregate).toHaveBeenCalled();
    expect(mocks.queryObservabilityCalls).not.toHaveBeenCalled();
    expect(mocks.queryObservabilityTraces).not.toHaveBeenCalled();
  });

  it('switches panels via the settings store; ledger/traces queries fire on demand', async () => {
    render(<ModelObservabilitySection />);
    await waitFor(() => {
      expect(screen.getByRole('tab', { name: SUBTAB_USAGE })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('tab', { name: SUBTAB_LEDGER }));
    expect(useSettingsStore.getState().activeSubTabs.usage).toBe('ledger');
    await waitFor(() => {
      expect(mocks.queryObservabilityCalls).toHaveBeenCalled();
    });
    const aggregateCallsOnLedger = mocks.queryObservabilityAggregate.mock.calls.length;

    fireEvent.click(screen.getByRole('tab', { name: SUBTAB_TRACES }));
    expect(useSettingsStore.getState().activeSubTabs.usage).toBe('traces');
    await waitFor(() => {
      expect(mocks.queryObservabilityTraces).toHaveBeenCalled();
    });
    expect(mocks.queryObservabilityAggregate.mock.calls.length).toBe(aggregateCallsOnLedger);

    fireEvent.click(screen.getByRole('tab', { name: SUBTAB_USAGE }));
    await waitFor(() => {
      expect(screen.getByText('settings.observability.charts.heat.title')).toBeInTheDocument();
    });
  });

  it('keeps each sub-tab filter independent and surviving tab switches', async () => {
    render(<ModelObservabilitySection />);
    await waitFor(() => {
      expect(screen.getByRole('tab', { name: SUBTAB_USAGE })).toBeInTheDocument();
    });

    // 进入台账页，把时间范围从默认「全部」改成 24h。
    fireEvent.click(screen.getByRole('tab', { name: SUBTAB_LEDGER }));
    await waitFor(() => {
      expect(mocks.queryObservabilityCalls).toHaveBeenCalled();
    });
    fireEvent.click(screen.getByRole('button', { name: /observability\.datePreset\.all/ }));
    fireEvent.click(screen.getByLabelText('settings.observability.datePreset.24h'));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /observability\.datePreset\.24h/ })).toBeInTheDocument();
    });

    // 切回 Token 用量页：仍是默认「全部」，未被台账页污染。
    fireEvent.click(screen.getByRole('tab', { name: SUBTAB_USAGE }));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /observability\.datePreset\.all/ })).toBeInTheDocument();
    });
    expect(screen.queryByRole('button', { name: /observability\.datePreset\.24h/ })).toBeNull();

    // 再切回台账页：24h 还在（面板卸载后筛选状态不丢）。
    fireEvent.click(screen.getByRole('tab', { name: SUBTAB_LEDGER }));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /observability\.datePreset\.24h/ })).toBeInTheDocument();
    });
  });
});
