/**
 * @vitest-environment jsdom
 *
 * Phase 9 Recording Settings 对话框测试 — desired/effective 拆分展示
 * （§一百：mismatch 绝不伪装 Active）、persistBlobs ⊆ persistPayloads
 * （§一百零五）、payload opt-in 确认（§一百零三：无加密事实文案）、
 * retention 校验（§一百零六）。
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
    schemaVersion: 2,
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
      schemaVersion: 2,
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

function switchByLabel(label: string): HTMLElement {
  return screen.getByRole('switch', { name: label });
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
    await screen.findByRole('switch', { name: 'settings.observability.recording.enabled' });
    const effective = document.querySelector('[class*="observability-settings-effective"]');
    expect(effective!.textContent).toContain('settings.observability.recording.atRestEncryptionNo');
    expect(effective!.textContent).not.toMatch(/加密存储|encrypted storage/i);
  });

  it('turning payloads off also turns blobs off (§一百零五 persistBlobs ⊆ persistPayloads)', async () => {
    render(
      <ObservabilitySettingsDialog
        open isLocalOwner settings={SETTINGS_ACTIVE} health={makeHealth()}
        onClose={() => {}} onApplied={() => {}}
      />,
    );
    const payloads = await screen.findByRole('switch', { name: 'settings.observability.recording.persistPayloads' });
    const blobs = screen.getByRole('switch', { name: 'settings.observability.recording.persistBlobs' });
    expect(payloads).toHaveAttribute('aria-checked', 'true');
    expect(blobs).toHaveAttribute('aria-checked', 'true');

    fireEvent.click(payloads);

    expect(switchByLabel('settings.observability.recording.persistPayloads'))
      .toHaveAttribute('aria-checked', 'false');
    expect(switchByLabel('settings.observability.recording.persistBlobs'))
      .toHaveAttribute('aria-checked', 'false');
  });

  it('payload opt-in requires an explicit confirmation listing persisted content kinds (§一百零三)', async () => {
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
    const payloads = await screen.findByRole('switch', { name: 'settings.observability.recording.persistPayloads' });
    fireEvent.click(payloads);

    // 确认对话框列出 5 类内容 + 无加密事实，confirm 后 draft 才翻 true。
    expect(screen.getByText('settings.observability.recording.confirmPayloadsTitle')).toBeInTheDocument();
    expect(screen.getByText('settings.observability.recording.confirmPayloadsItemReasoning')).toBeInTheDocument();
    expect(screen.getByText('settings.observability.recording.confirmPayloadsEncryptionFact')).toBeInTheDocument();
    expect(switchByLabel('settings.observability.recording.persistPayloads'))
      .toHaveAttribute('aria-checked', 'false');

    fireEvent.click(screen.getByRole('button', { name: 'settings.observability.recording.confirmPayloadsConfirm' }));
    expect(switchByLabel('settings.observability.recording.persistPayloads'))
      .toHaveAttribute('aria-checked', 'true');
  });

  it('invalid retention blocks save and announces the allowed range (§一百零六)', async () => {
    render(
      <ObservabilitySettingsDialog
        open isLocalOwner settings={SETTINGS_ACTIVE} health={makeHealth()}
        onClose={() => {}} onApplied={() => {}}
      />,
    );
    await screen.findByRole('switch', { name: 'settings.observability.recording.enabled' });

    const traceInput = document.querySelector('input[class*="settings-input"]') as HTMLInputElement;
    fireEvent.change(traceInput, { target: { value: '0' } });

    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('settings.observability.recording.retentionInvalid');
    const saveButton = screen.getByRole('button', { name: 'settings.observability.actions.save' });
    expect(saveButton).toBeDisabled();
  });

  it('save submits the full desired shape via PUT payload', async () => {
    mocks.updateObservabilitySettings.mockResolvedValue({});
    render(
      <ObservabilitySettingsDialog
        open isLocalOwner settings={SETTINGS_ACTIVE} health={makeHealth()}
        onClose={() => {}} onApplied={() => {}}
      />,
    );
    await screen.findByRole('switch', { name: 'settings.observability.recording.enabled' });

    fireEvent.click(screen.getByRole('button', { name: 'settings.observability.actions.save' }));

    await waitFor(() => {
      expect(mocks.updateObservabilitySettings).toHaveBeenCalledWith({
        enabled: true,
        persistTraceMetadata: true,
        persistPayloads: true,
        persistBlobs: true,
        retention: { traceDays: 180, payloadDays: 30, blobDays: 30 },
      });
    });
  });
});
