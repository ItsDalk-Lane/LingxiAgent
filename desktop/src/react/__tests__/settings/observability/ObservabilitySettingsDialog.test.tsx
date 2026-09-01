/**
 * @vitest-environment jsdom
 *
 * 模型观测设置对话框测试：采集能力固定全开，界面只保留真实运行状态、
 * 静态加密事实和三类保留天数。
 */
import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';

const mocks = vi.hoisted(() => ({
  updateObservabilitySettings: vi.fn(),
}));

vi.mock('../../../settings/tabs/observability/model-observability-actions', async (importOriginal) => {
  const original = await importOriginal<typeof import('../../../settings/tabs/observability/model-observability-actions')>();
  return {
    ...original,
    updateObservabilitySettings: (...args: unknown[]) => mocks.updateObservabilitySettings(...args),
  };
});

import { ObservabilitySettingsDialog } from '../../../settings/tabs/observability/ObservabilitySettingsDialog';
import type {
  ModelObservabilityHealthResponse,
  ModelObservabilitySettingsResponse,
} from '../../../../../../shared/model-observability-api-contract.ts';

const SETTINGS_ACTIVE: ModelObservabilitySettingsResponse = {
  desired: {
    enabled: true,
    persistTraceMetadata: true,
    persistPayloads: true,
    persistBlobs: true,
    retention: { traceDays: 180, payloadDays: 30, blobDays: 30 },
  },
  effective: {
    recordingStatus: 'active',
    storeDisabledReasonCode: null,
    persistTraceMetadata: true,
    persistPayloads: true,
    persistBlobs: true,
    schemaVersion: 4,
  },
  cryptographicallyEncryptedAtRest: false,
};

const SETTINGS_MISMATCH: ModelObservabilitySettingsResponse = {
  ...SETTINGS_ACTIVE,
  effective: {
    ...SETTINGS_ACTIVE.effective,
    recordingStatus: 'disabled',
    storeDisabledReasonCode: 'disabled_by_policy',
    persistTraceMetadata: false,
  },
};

function makeHealth(): ModelObservabilityHealthResponse {
  return {
    recordingStatus: 'active',
    storeDisabledReasonCode: null,
    persistTraceMetadata: true,
    persistPayloads: true,
    persistBlobs: true,
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
      queryStatus: 'ready',
      queryStatusReason: null,
      schemaVersion: 4,
      accountingProjectionAvailable: true,
      oldestCallAt: null,
      newestCallAt: null,
      callCount: 0,
      traceCount: 0,
      payloadRecordCount: 0,
      usageProjectionCount: 0,
      dataCompleteness: {
        status: 'known',
        droppedTraceEvents: 0, droppedPayloadRecords: 0, droppedBlobs: 0, interruptedByRestartCalls: 0,
      },
    },
  };
}

describe('ObservabilitySettingsDialog', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.t = ((key: string, params?: Record<string, unknown>) => {
      if (params && Object.keys(params).length > 0) return `${key}:${JSON.stringify(params)}`;
      return key;
    }) as typeof window.t;
  });

  afterEach(() => cleanup());

  it('hides the dialog content when closed', () => {
    render(
      <ObservabilitySettingsDialog
        open={false} isLocalOwner settings={SETTINGS_ACTIVE} health={makeHealth()}
        onClose={() => {}} onApplied={() => {}}
      />,
    );
    expect(screen.queryByRole('switch')).toBeNull();
  });

  it('desired enabled + effective disabled surfaces configuredButInactive with the reason code (§一百)', async () => {
    render(
      <ObservabilitySettingsDialog
        open isLocalOwner settings={SETTINGS_MISMATCH} health={makeHealth()}
        onClose={() => {}} onApplied={() => {}}
      />,
    );
    const mismatch = await screen.findByRole('status');
    expect(mismatch.textContent).toContain('settings.observability.recording.configuredButInactive');
    expect(mismatch.textContent).toContain('disabled_by_policy');
  });

  it('degraded 是仍在记录的告警态，不伪装成未生效', async () => {
    const degraded: ModelObservabilitySettingsResponse = {
      ...SETTINGS_ACTIVE,
      effective: {
        ...SETTINGS_ACTIVE.effective,
        recordingStatus: 'degraded',
        storeDisabledReasonCode: 'write_failed_pending_receipt',
      },
    };
    render(
      <ObservabilitySettingsDialog
        open isLocalOwner settings={degraded} health={{
          ...makeHealth(),
          recordingStatus: 'degraded',
          storeDisabledReasonCode: 'write_failed_pending_receipt',
        }}
        onClose={() => {}} onApplied={() => {}}
      />,
    );
    const effective = document.querySelector('[class*="observability-settings-effective"]');
    expect(effective?.textContent).toContain('degraded');
    expect(effective?.textContent).not.toContain('settings.observability.recording.configuredButInactive');
  });

  it('states the honest at-rest encryption fact (never claims encrypted storage, §一百零三)', async () => {
    render(
      <ObservabilitySettingsDialog
        open isLocalOwner settings={SETTINGS_ACTIVE} health={makeHealth()}
        onClose={() => {}} onApplied={() => {}}
      />,
    );
    await screen.findByRole('dialog', { name: 'settings.observability.recording.dialogAria' });
    const effective = document.querySelector('[class*="observability-settings-effective"]');
    expect(effective!.textContent).toContain('settings.observability.recording.atRestEncryptionNo');
    expect(effective!.textContent).not.toMatch(/加密存储|encrypted storage/i);
  });

  it('removes all recording switches and only exposes three retention fields', async () => {
    render(
      <ObservabilitySettingsDialog
        open isLocalOwner settings={SETTINGS_ACTIVE} health={makeHealth()}
        onClose={() => {}} onApplied={() => {}}
      />,
    );
    await screen.findByRole('dialog', { name: 'settings.observability.recording.dialogAria' });
    expect(screen.queryByRole('switch')).toBeNull();
    expect(screen.getAllByRole('spinbutton')).toHaveLength(3);
    expect(document.body.textContent).toContain('settings.observability.recording.persistPayloadsHint');
  });

  it('does not restore legacy opt-in controls when an old response contains false switches', async () => {
    const off: ModelObservabilitySettingsResponse = {
      ...SETTINGS_ACTIVE,
      desired: { ...SETTINGS_ACTIVE.desired, persistPayloads: false, persistBlobs: false },
      effective: { ...SETTINGS_ACTIVE.effective, persistPayloads: false, persistBlobs: false },
    };
    render(
      <ObservabilitySettingsDialog
        open isLocalOwner settings={off} health={makeHealth()}
        onClose={() => {}} onApplied={() => {}}
      />,
    );
    await screen.findByRole('dialog', { name: 'settings.observability.recording.dialogAria' });
    expect(screen.queryByRole('switch')).toBeNull();
    expect(screen.queryByText('settings.observability.recording.confirmPayloadsTitle')).toBeNull();
  });

  it('invalid retention blocks save and announces the allowed range (§一百零六)', async () => {
    render(
      <ObservabilitySettingsDialog
        open isLocalOwner settings={SETTINGS_ACTIVE} health={makeHealth()}
        onClose={() => {}} onApplied={() => {}}
      />,
    );
    await screen.findByRole('dialog', { name: 'settings.observability.recording.dialogAria' });

    const traceInput = document.querySelector('input[class*="settings-input"]') as HTMLInputElement;
    fireEvent.change(traceInput, { target: { value: '0' } });

    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('settings.observability.recording.retentionInvalid');
    const saveButton = screen.getByRole('button', { name: 'settings.observability.actions.save' });
    expect(saveButton).toBeDisabled();
  });

  it('save submits retention only', async () => {
    mocks.updateObservabilitySettings.mockResolvedValue({});
    render(
      <ObservabilitySettingsDialog
        open isLocalOwner settings={SETTINGS_ACTIVE} health={makeHealth()}
        onClose={() => {}} onApplied={() => {}}
      />,
    );
    await screen.findByRole('dialog', { name: 'settings.observability.recording.dialogAria' });

    fireEvent.click(screen.getByRole('button', { name: 'settings.observability.actions.save' }));

    await waitFor(() => {
      expect(mocks.updateObservabilitySettings).toHaveBeenCalledWith({
        retention: { traceDays: 180, payloadDays: 30, blobDays: 30 },
      });
    });
  });
});
