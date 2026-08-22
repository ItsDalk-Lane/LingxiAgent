/**
 * ObservabilitySettingsDialog.tsx — 录制设置（Phase 9 §九十六～一百零八）。
 *
 * 纪律：
 *   - desired（用户配置）≠ effective（运行态）：schema_newer 等场景显示
 *     「已配置但运行时无法启动 + 原因」，绝不伪装 Active（§一百）。
 *   - persistPayloads false→true：显式确认框，列出将持久化的内容（脱敏后的
 *     prompt/response/tool result/reasoning/transcription）并明说**磁盘上
 *     没有密码学加密**（§一百零三：绝不写「数据安全加密存储」——事实不成立）。
 *   - persistBlobs ⊆ persistPayloads 不变量（§一百零五）：payload 关闭时
 *     blob 开关禁用；关 payload 自动关 blob。
 *   - blob 确认框只说真话：只存 Phase 7 eligible 的 ingestion 二进制（§一百零四）。
 *   - retention 1..3650 天客户端校验，server 终裁（§一百零六）。
 *   - 关闭录制解释「停止新记录，不删历史」（§一百零七）；无 Delete All
 *     （§一百零八）。
 */
import React, { useCallback, useEffect, useState } from 'react';
import type {
  ModelObservabilityHealthResponse,
  ModelObservabilitySettingsResponse,
} from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import { Button, ConfirmDialog, Overlay, Toggle } from '../../../ui';
import styles from '../../Settings.module.css';
import {
  isObservabilityErrorKind,
  ModelObservabilityRequestError,
  updateObservabilitySettings,
} from './model-observability-actions';
import { formatLocalFullDateTime } from './model-observability-format';
import { queryStatusLabel, recordingStatusLabel } from './model-observability-labels';

const MIN_RETENTION_DAYS = 1;
const MAX_RETENTION_DAYS = 3650;

type DraftState = {
  enabled: boolean;
  persistTraceMetadata: boolean;
  persistPayloads: boolean;
  persistBlobs: boolean;
  traceDays: string;
  payloadDays: string;
  blobDays: string;
};

function draftFromSettings(settings: ModelObservabilitySettingsResponse): DraftState {
  return {
    enabled: settings.desired.enabled,
    persistTraceMetadata: settings.desired.persistTraceMetadata,
    persistPayloads: settings.desired.persistPayloads,
    persistBlobs: settings.desired.persistBlobs,
    traceDays: String(settings.desired.retention.traceDays),
    payloadDays: String(settings.desired.retention.payloadDays),
    blobDays: String(settings.desired.retention.blobDays),
  };
}

function parseDays(raw: string): number | null {
  const n = Number(raw);
  if (!Number.isInteger(n) || n < MIN_RETENTION_DAYS || n > MAX_RETENTION_DAYS) return null;
  return n;
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
  const [confirming, setConfirming] = useState<'payloads' | 'blobs' | null>(null);

  // 打开时从 settings 播种 draft；关闭时清掉（下次打开重新读最新值）。
  useEffect(() => {
    if (open && settings) {
      setDraft(draftFromSettings(settings));
      setSaveError(null);
    } else if (!open) {
      setDraft(null);
      setConfirming(null);
    }
  }, [open, settings]);

  const patchDraft = useCallback((patch: Partial<DraftState>) => {
    setDraft((prev) => (prev ? { ...prev, ...patch } : prev));
  }, []);

  /** §一百零五 不变量：persistBlobs ⊆ persistPayloads。 */
  const setPersistPayloads = useCallback((on: boolean) => {
    if (on) {
      setConfirming('payloads');
      return;
    }
    setDraft((prev) => (prev ? { ...prev, persistPayloads: false, persistBlobs: false } : prev));
  }, []);

  const setPersistBlobs = useCallback((on: boolean) => {
    if (on) {
      setConfirming('blobs');
      return;
    }
    patchDraft({ persistBlobs: false });
  }, [patchDraft]);

  const traceDays = draft ? parseDays(draft.traceDays) : null;
  const payloadDays = draft ? parseDays(draft.payloadDays) : null;
  const blobDays = draft ? parseDays(draft.blobDays) : null;
  const daysValid = traceDays !== null && payloadDays !== null && blobDays !== null;

  const save = useCallback(() => {
    if (!draft || !daysValid) return;
    setSaving(true);
    setSaveError(null);
    updateObservabilitySettings({
      enabled: draft.enabled,
      persistTraceMetadata: draft.persistTraceMetadata,
      persistPayloads: draft.persistPayloads,
      persistBlobs: draft.persistBlobs,
      retention: {
        traceDays: traceDays!,
        payloadDays: payloadDays!,
        blobDays: blobDays!,
      },
    }).then(() => {
      setSaving(false);
      onApplied();
    }).catch((error: unknown) => {
      setSaving(false);
      // §一百零七：应用失败明说，绝不伪装 Active。
      if (isObservabilityErrorKind(error, 'local_only_route') || isObservabilityErrorKind(error, 'studio_owner_required')) {
        setSaveError(t('settings.observability.recording.localOnlyHint'));
      } else {
        setSaveError(error instanceof Error ? error.message : String(error));
      }
    });
  }, [draft, daysValid, traceDays, payloadDays, blobDays, onApplied]);

  const effectiveMismatch = settings
    && settings.desired.enabled
    && (settings.effective.recordingStatus === 'disabled'
      || settings.effective.recordingStatus === 'closed');

  return (
    <>
      <Overlay
        open={open}
        scope="inline"
        onClose={onClose}
        closeOnEsc
        closeOnBackdrop
        trapFocus
        contentProps={{ role: 'dialog', 'aria-label': t('settings.observability.recording.dialogAria') }}
      >
        <div className={styles['observability-settings-dialog']}>
          <h3 className={styles['observability-panel-title']}>
            {t('settings.observability.recording.title')}
          </h3>

          {!isLocalOwner && (
            <div className={styles['observability-provenance-note']}>
              {t('settings.observability.recording.localOnlyHint')}
            </div>
          )}

          {/* effective 状态（§一百 desired vs effective） */}
          {settings && (
            <div className={styles['observability-settings-effective']} data-mismatch={effectiveMismatch || undefined}>
              <div>
                {t('settings.observability.recording.effectiveStatus')}
                {': '}
                <strong>{recordingStatusLabel(settings.effective.recordingStatus)}</strong>
              </div>
              {effectiveMismatch && (
                <div className={styles['observability-recording-reason']} role="status">
                  {t('settings.observability.recording.configuredButInactive')}
                  {settings.effective.storeDisabledReasonCode && (
                    <>
                      {' — '}
                      {t('settings.observability.recording.reason', { code: settings.effective.storeDisabledReasonCode })}
                    </>
                  )}
                </div>
              )}
              <div className={styles['observability-ledger-muted']}>
                {t('settings.observability.recording.queryStatus')}
                {': '}
                {health ? queryStatusLabel(health.query.queryStatus) : '—'}
                {settings.effective.schemaVersion !== null && (
                  <>
                    {' · schema v'}{settings.effective.schemaVersion}
                  </>
                )}
              </div>
              {/* §一百零二：at-rest 加密事实诚实展示（contract 恒 false） */}
              <div className={styles['observability-ledger-muted']}>
                {t('settings.observability.recording.atRestEncryption')}
                {': '}
                {settings.cryptographicallyEncryptedAtRest
                  ? t('settings.observability.tri.yes')
                  : t('settings.observability.recording.atRestEncryptionNo')}
              </div>
              {health?.lastSuccessfulFlushAt && (
                <div className={styles['observability-ledger-muted']}>
                  {t('settings.observability.recording.lastFlush')}
                  {': '}
                  {formatLocalFullDateTime(health.lastSuccessfulFlushAt)}
                </div>
              )}
            </div>
          )}

          {draft && (
            <div className={styles['observability-settings-form']}>
              <label className={styles['observability-settings-row']}>
                <span>
                  {t('settings.observability.recording.enabled')}
                  <span className={styles['observability-settings-hint']}>
                    {draft.enabled
                      ? t('settings.observability.recording.enabledHintOn')
                      : t('settings.observability.recording.enabledHintOff')}
                  </span>
                </span>
                <Toggle
                  on={draft.enabled}
                  disabled={!isLocalOwner}
                  onChange={(on) => patchDraft({ enabled: on })}
                  ariaLabel={t('settings.observability.recording.enabled')}
                />
              </label>

              <label className={styles['observability-settings-row']}>
                <span>{t('settings.observability.recording.persistTraceMetadata')}</span>
                <Toggle
                  on={draft.persistTraceMetadata}
                  disabled={!isLocalOwner || !draft.enabled}
                  onChange={(on) => patchDraft({ persistTraceMetadata: on })}
                  ariaLabel={t('settings.observability.recording.persistTraceMetadata')}
                />
              </label>

              <label className={styles['observability-settings-row']}>
                <span>
                  {t('settings.observability.recording.persistPayloads')}
                  <span className={styles['observability-settings-hint']}>
                    {t('settings.observability.recording.persistPayloadsHint')}
                  </span>
                </span>
                <Toggle
                  on={draft.persistPayloads}
                  disabled={!isLocalOwner || !draft.enabled}
                  onChange={setPersistPayloads}
                  ariaLabel={t('settings.observability.recording.persistPayloads')}
                />
              </label>

              <label className={styles['observability-settings-row']} data-disabled={!draft.persistPayloads || undefined}>
                <span>
                  {t('settings.observability.recording.persistBlobs')}
                  <span className={styles['observability-settings-hint']}>
                    {draft.persistPayloads
                      ? t('settings.observability.recording.persistBlobsHint')
                      : t('settings.observability.recording.persistBlobsRequiresPayloads')}
                  </span>
                </span>
                <Toggle
                  on={draft.persistBlobs}
                  disabled={!isLocalOwner || !draft.enabled || !draft.persistPayloads}
                  onChange={setPersistBlobs}
                  ariaLabel={t('settings.observability.recording.persistBlobs')}
                />
              </label>

              <div className={styles['observability-settings-retention']}>
                <div className={styles['observability-advanced-title']}>
                  {t('settings.observability.recording.retentionTitle')}
                </div>
                {([
                  ['traceDays', 'settings.observability.recording.retentionTrace', traceDays],
                  ['payloadDays', 'settings.observability.recording.retentionPayload', payloadDays],
                  ['blobDays', 'settings.observability.recording.retentionBlob', blobDays],
                ] as const).map(([field, labelKey, parsed]) => (
                  <div className={styles['observability-advanced-field']} key={field}>
                    <label className={styles['observability-advanced-label']}>{t(labelKey)}</label>
                    <input
                      className={styles['settings-input']}
                      type="number"
                      min={MIN_RETENTION_DAYS}
                      max={MAX_RETENTION_DAYS}
                      value={draft[field]}
                      disabled={!isLocalOwner}
                      data-invalid={parsed === null || undefined}
                      onChange={(event) => patchDraft({ [field]: event.target.value })}
                    />
                    {parsed === null && (
                      <span className={styles['observability-settings-invalid']} role="alert">
                        {t('settings.observability.recording.retentionInvalid', {
                          min: MIN_RETENTION_DAYS,
                          max: MAX_RETENTION_DAYS,
                        })}
                      </span>
                    )}
                  </div>
                ))}
              </div>

              {saveError && (
                <div className={styles['observability-error-detail']} role="alert">{saveError}</div>
              )}

              <div className={styles['observability-advanced-footer']}>
                <Button variant="secondary" size="sm" onClick={onClose}>
                  {t('settings.observability.actions.cancel')}
                </Button>
                <Button
                  variant="primary"
                  size="sm"
                  loading={saving}
                  disabled={!isLocalOwner || !daysValid}
                  onClick={save}
                >
                  {t('settings.observability.actions.save')}
                </Button>
              </div>
            </div>
          )}
        </div>
      </Overlay>

      {/* §一百零三 payload opt-in 确认：列清将持久化的内容 + 无磁盘加密事实 */}
      <ConfirmDialog
        open={confirming === 'payloads'}
        scope="inline"
        title={t('settings.observability.recording.confirmPayloadsTitle')}
        confirmLabel={t('settings.observability.recording.confirmPayloadsConfirm')}
        cancelLabel={t('settings.observability.actions.cancel')}
        onConfirm={() => {
          patchDraft({ persistPayloads: true });
          setConfirming(null);
        }}
        onCancel={() => setConfirming(null)}
      >
        <div className={styles['observability-confirm-body']}>
          <p>{t('settings.observability.recording.confirmPayloadsBody')}</p>
          <ul>
            <li>{t('settings.observability.recording.confirmPayloadsItemPrompt')}</li>
            <li>{t('settings.observability.recording.confirmPayloadsItemResponse')}</li>
            <li>{t('settings.observability.recording.confirmPayloadsItemToolResult')}</li>
            <li>{t('settings.observability.recording.confirmPayloadsItemReasoning')}</li>
            <li>{t('settings.observability.recording.confirmPayloadsItemTranscription')}</li>
          </ul>
          {/* 事实文案：脱敏 + 文件权限保护，无密码学加密（绝不写「加密存储」） */}
          <p>{t('settings.observability.recording.confirmPayloadsEncryptionFact')}</p>
        </div>
      </ConfirmDialog>

      {/* §一百零四 blob opt-in 确认：只说 Phase 7 eligible ingestion 二进制 */}
      <ConfirmDialog
        open={confirming === 'blobs'}
        scope="inline"
        title={t('settings.observability.recording.confirmBlobsTitle')}
        confirmLabel={t('settings.observability.recording.confirmBlobsConfirm')}
        cancelLabel={t('settings.observability.actions.cancel')}
        onConfirm={() => {
          patchDraft({ persistBlobs: true });
          setConfirming(null);
        }}
        onCancel={() => setConfirming(null)}
      >
        <div className={styles['observability-confirm-body']}>
          <p>{t('settings.observability.recording.confirmBlobsBody')}</p>
        </div>
      </ConfirmDialog>
    </>
  );
}
