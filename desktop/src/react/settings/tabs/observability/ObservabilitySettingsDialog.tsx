import React, { useCallback, useEffect, useState } from 'react';
import type { ModelObservabilityHealthResponse, ModelObservabilitySettingsResponse } from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import { Button, Overlay } from '../../../ui';
import styles from '../../Settings.module.css';
import { isObservabilityErrorKind, updateObservabilitySettings } from './model-observability-actions';
import { formatLocalFullDateTime } from './model-observability-format';
import { queryStatusLabel, recordingStatusLabel } from './model-observability-labels';

const MIN_RETENTION_DAYS = 1;
const MAX_RETENTION_DAYS = 3650;
type DraftState = { traceDays: string; payloadDays: string; blobDays: string };

function parseDays(raw: string): number | null {
  const value = Number(raw);
  return Number.isInteger(value) && value >= MIN_RETENTION_DAYS && value <= MAX_RETENTION_DAYS ? value : null;
}

export function ObservabilitySettingsDialog({ open, isLocalOwner, settings, health, onClose, onApplied }: {
  open: boolean;
  isLocalOwner: boolean;
  settings: ModelObservabilitySettingsResponse | null;
  health: ModelObservabilityHealthResponse | null;
  onClose: () => void;
  onApplied: () => void;
}) {
  const [draft, setDraft] = useState<DraftState | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    if (open && settings) {
      setDraft({
        traceDays: String(settings.desired.retention.traceDays),
        payloadDays: String(settings.desired.retention.payloadDays),
        blobDays: String(settings.desired.retention.blobDays),
      });
      setSaveError(null);
    } else if (!open) {
      setDraft(null);
    }
  }, [open, settings]);

  const patchDraft = useCallback((patch: Partial<DraftState>) => {
    setDraft(current => current ? { ...current, ...patch } : current);
  }, []);
  const traceDays = draft ? parseDays(draft.traceDays) : null;
  const payloadDays = draft ? parseDays(draft.payloadDays) : null;
  const blobDays = draft ? parseDays(draft.blobDays) : null;
  const daysValid = traceDays !== null && payloadDays !== null && blobDays !== null;
  const effectiveInactive = settings && ['disabled', 'closed'].includes(settings.effective.recordingStatus);
  const effectiveDegraded = settings?.effective.recordingStatus === 'degraded';

  const save = useCallback(() => {
    if (!daysValid) return;
    setSaving(true);
    setSaveError(null);
    updateObservabilitySettings({ retention: { traceDays: traceDays!, payloadDays: payloadDays!, blobDays: blobDays! } })
      .then(() => { setSaving(false); onApplied(); })
      .catch((error: unknown) => {
        setSaving(false);
        setSaveError(
          isObservabilityErrorKind(error, 'local_only_route') || isObservabilityErrorKind(error, 'studio_owner_required')
            ? t('settings.observability.recording.localOnlyHint')
            : error instanceof Error ? error.message : String(error),
        );
      });
  }, [daysValid, traceDays, payloadDays, blobDays, onApplied]);

  return (
    <Overlay open={open} scope="inline" onClose={onClose} closeOnEsc closeOnBackdrop trapFocus
      contentProps={{ role: 'dialog', 'aria-label': t('settings.observability.recording.dialogAria') }}>
      <div className={styles['observability-settings-dialog']}>
        <h3 className={styles['observability-panel-title']}>{t('settings.observability.recording.title')}</h3>
        {!isLocalOwner && <div className={styles['observability-provenance-note']}>{t('settings.observability.recording.localOnlyHint')}</div>}
        {settings && (
          <div className={styles['observability-settings-effective']} data-mismatch={effectiveInactive || effectiveDegraded || undefined}>
            <div>{t('settings.observability.recording.effectiveStatus')}{': '}<strong>{recordingStatusLabel(settings.effective.recordingStatus)}</strong></div>
            {effectiveInactive && <div className={styles['observability-recording-reason']} role="status">
              {t('settings.observability.recording.configuredButInactive')}
              {settings.effective.storeDisabledReasonCode ? ` — ${t('settings.observability.recording.reason', { code: settings.effective.storeDisabledReasonCode })}` : ''}
            </div>}
            {effectiveDegraded && settings.effective.storeDisabledReasonCode && <div className={styles['observability-recording-reason']} role="status">
              {t('settings.observability.recording.reason', { code: settings.effective.storeDisabledReasonCode })}
            </div>}
            <div className={styles['observability-ledger-muted']}>
              {t('settings.observability.recording.queryStatus')}{': '}{health ? queryStatusLabel(health.query.queryStatus) : '—'}
              {settings.effective.schemaVersion !== null ? ` · schema v${settings.effective.schemaVersion}` : ''}
            </div>
            <div className={styles['observability-ledger-muted']}>{t('settings.observability.recording.persistPayloadsHint')}</div>
            <div className={styles['observability-ledger-muted']}>
              {t('settings.observability.recording.atRestEncryption')}{': '}
              {settings.cryptographicallyEncryptedAtRest ? t('settings.observability.tri.yes') : t('settings.observability.recording.atRestEncryptionNo')}
            </div>
            {health?.lastSuccessfulFlushAt && <div className={styles['observability-ledger-muted']}>
              {t('settings.observability.recording.lastFlush')}{': '}{formatLocalFullDateTime(health.lastSuccessfulFlushAt)}
            </div>}
          </div>
        )}
        {draft && <div className={styles['observability-settings-form']}>
          <div className={styles['observability-settings-retention']}>
            <div className={styles['observability-advanced-title']}>{t('settings.observability.recording.retentionTitle')}</div>
            <div className={styles['observability-settings-retention-fields']}>
              {([
                ['traceDays', 'settings.observability.recording.retentionTrace', traceDays],
                ['payloadDays', 'settings.observability.recording.retentionPayload', payloadDays],
                ['blobDays', 'settings.observability.recording.retentionBlob', blobDays],
              ] as const).map(([field, labelKey, parsed]) => <div className={styles['observability-advanced-field']} key={field}>
                <label className={styles['observability-advanced-label']}>{t(labelKey)}</label>
                <input className={styles['settings-input']} type="number" min={MIN_RETENTION_DAYS} max={MAX_RETENTION_DAYS}
                  value={draft[field]} disabled={!isLocalOwner} data-invalid={parsed === null || undefined}
                  onChange={event => patchDraft({ [field]: event.target.value })} />
                {parsed === null && <span className={styles['observability-settings-invalid']} role="alert">
                  {t('settings.observability.recording.retentionInvalid', { min: MIN_RETENTION_DAYS, max: MAX_RETENTION_DAYS })}
                </span>}
              </div>)}
            </div>
          </div>
          {saveError && <div className={styles['observability-error-detail']} role="alert">{saveError}</div>}
          <div className={styles['observability-advanced-footer']}>
            <Button variant="secondary" size="sm" onClick={onClose}>{t('settings.observability.actions.cancel')}</Button>
            <Button variant="primary" size="sm" loading={saving} disabled={!isLocalOwner || !daysValid} onClick={save}>{t('settings.observability.actions.save')}</Button>
          </div>
        </div>}
      </div>
    </Overlay>
  );
}
