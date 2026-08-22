/**
 * ModelObservabilitySection.tsx — Model Observatory 页面编排（Phase 9）。
 *
 * 职责：health/settings bootstrap、录制状态条（低噪声，§九十六）、
 * onboarding 空态（§九十八）、aggregate 唯一查询点（Metrics/Groups 共享，
 * §十四）、Inspector/TraceExplorer/Settings/Export 的挂载与导航联动。
 *
 * 纪律：
 *   - 刷新 = health + aggregate + 首页 ledger（§五十；绝不触发 writer flush）。
 *   - recording disabled 但 query ready → 历史照常浏览（§九十七）。
 *   - 分层 loading：health/aggregate/ledger 各自独立，不做整页白闪（§一百三十七）。
 *   - local-only 功能（export/settings PUT/payload 正文/blob）用
 *     isLocalOwnerConnection 灰化，route security 仍是最终裁决（§一百三十二）。
 */
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type {
  ModelObservabilityAggregateResult,
  ModelObservabilityHealthResponse,
  ModelObservabilitySettingsResponse,
} from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import { useSettingsStore } from '../../store';
import { isLocalOwnerConnection } from '../../../services/server-connection';
import styles from '../../Settings.module.css';
import {
  isObservabilityAbortError,
  loadObservabilityHealth,
  loadObservabilitySettings,
  ModelObservabilityRequestError,
  queryObservabilityAggregate,
  updateObservabilitySettings,
} from './model-observability-actions';
import {
  buildCallFilterInput,
  dateBucketForGroupBy,
} from './model-observability-filter';
import { useObservabilityQueryState } from './use-observability-query-state';
import { ObservabilityFilterBar } from './ObservabilityFilterBar';
import { ObservabilityMetrics } from './ObservabilityMetrics';
import { ObservabilityGroups } from './ObservabilityGroups';
import { ObservabilityCallLedger } from './ObservabilityCallLedger';
import { ObservabilityCallInspector } from './ObservabilityCallInspector';
import { ObservabilityTraceExplorer } from './ObservabilityTraceExplorer';
import { ObservabilitySettingsDialog } from './ObservabilitySettingsDialog';
import { ObservabilityExportDialog } from './ObservabilityExportDialog';
import { recordingStatusLabel } from './model-observability-labels';

type BootstrapError = { kind: 'forbidden' | 'network'; message: string };

function toBootstrapError(error: unknown): BootstrapError {
  if (error instanceof ModelObservabilityRequestError
    && (error.kind === 'local_only_route' || error.kind === 'studio_owner_required' || error.kind === 'forbidden')) {
    return { kind: 'forbidden', message: error.message };
  }
  return { kind: 'network', message: error instanceof Error ? error.message : String(error) };
}

export function ModelObservabilitySection() {
  const queryState = useObservabilityQueryState();
  const { appliedFilter, groupBy } = queryState;

  const isLocalOwner = isLocalOwnerConnection(useSettingsStore((s) => s.activeServerConnection));

  const [health, setHealth] = useState<ModelObservabilityHealthResponse | null>(null);
  const [settings, setSettings] = useState<ModelObservabilitySettingsResponse | null>(null);
  const [bootstrapError, setBootstrapError] = useState<BootstrapError | null>(null);
  const [aggregate, setAggregate] = useState<ModelObservabilityAggregateResult | null>(null);
  const [aggregateLoading, setAggregateLoading] = useState(true);
  const [aggregateError, setAggregateError] = useState<string | null>(null);
  const [refreshToken, setRefreshToken] = useState(0);
  const [refreshing, setRefreshing] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [exportOpen, setExportOpen] = useState(false);
  const aggregateGenerationRef = useRef(0);

  /* ── bootstrap：health + settings ──────────────────────────────────── */
  const reloadControlPlane = useCallback(async (signal?: AbortSignal) => {
    const [nextHealth, nextSettings] = await Promise.all([
      loadObservabilityHealth({ signal }),
      loadObservabilitySettings({ signal }),
    ]);
    setHealth(nextHealth);
    setSettings(nextSettings);
    setBootstrapError(null);
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    reloadControlPlane(controller.signal).catch((error: unknown) => {
      if (isObservabilityAbortError(error)) return;
      setBootstrapError(toBootstrapError(error));
    });
    return () => controller.abort();
  }, [reloadControlPlane]);

  /* ── aggregate：Metrics/Groups 共享唯一查询（§十四/§二十八）───────────── */
  useEffect(() => {
    const generation = ++aggregateGenerationRef.current;
    const controller = new AbortController();
    setAggregateLoading(true);
    setAggregateError(null);
    queryObservabilityAggregate(
      {
        filter: buildCallFilterInput(appliedFilter),
        groupBy: [...groupBy],
        dateBucket: dateBucketForGroupBy(groupBy),
      },
      { signal: controller.signal },
    ).then((result) => {
      if (aggregateGenerationRef.current !== generation) return;
      setAggregate(result);
      setAggregateLoading(false);
    }).catch((error: unknown) => {
      if (aggregateGenerationRef.current !== generation || isObservabilityAbortError(error)) return;
      if (error instanceof ModelObservabilityRequestError && error.kind === 'not_initialized') {
        setAggregate(null);
        setAggregateError('not_initialized');
      } else {
        setAggregateError(error instanceof Error ? error.message : String(error));
      }
      setAggregateLoading(false);
    });
    return () => controller.abort();
  }, [appliedFilter, groupBy, refreshToken]);

  const refresh = useCallback(() => {
    setRefreshing(true);
    setRefreshToken((token) => token + 1);
    reloadControlPlane()
      .catch((error: unknown) => setBootstrapError(toBootstrapError(error)))
      .finally(() => setRefreshing(false));
  }, [reloadControlPlane]);

  /* ── onboarding：store absent + disabled（§九十八）───────────────────── */
  const [enabling, setEnabling] = useState(false);
  const [enableError, setEnableError] = useState<string | null>(null);
  const enableObservability = useCallback(() => {
    setEnabling(true);
    setEnableError(null);
    // §九十九 安全默认：只开 trace metadata；payload/blob 必须另行显式 opt-in。
    updateObservabilitySettings({
      enabled: true,
      persistTraceMetadata: true,
      persistPayloads: false,
      persistBlobs: false,
    }).then(() => reloadControlPlane())
      .then(() => setRefreshToken((token) => token + 1))
      .catch((error: unknown) => {
        setEnableError(error instanceof Error ? error.message : String(error));
      })
      .finally(() => setEnabling(false));
  }, [reloadControlPlane]);

  const storeAbsent = health?.query.queryStatus === 'absent' || aggregateError === 'not_initialized';
  const recordingEnabled = settings?.desired.enabled === true;

  /* ── 导航联动：Inspector ↔ TraceExplorer ─────────────────────────────── */
  const handleSelectCall = useCallback((callId: string) => {
    queryState.selectCall(callId);
  }, [queryState]);

  const handleSelectTrace = useCallback((traceId: string | null) => {
    queryState.selectTrace(traceId);
  }, [queryState]);

  const handleFilterExact = useCallback((field: 'sessionId' | 'conversationId' | 'agentId' | 'taskId', value: string) => {
    queryState.setDrafts({ [field]: value });
    queryState.patchFilter({ [field]: value });
  }, [queryState]);

  const handleBucketFilter = useCallback((dimension: string, value: string) => {
    if (dimension === 'provider') queryState.patchFilter({ providers: [value] });
    else if (dimension === 'model') queryState.patchFilter({ modelIds: [value] });
    else if (dimension === 'category') queryState.patchFilter({ categories: [value] });
    else if (dimension === 'status') queryState.patchFilter({ terminalStatuses: [value] });
  }, [queryState]);

  const exportUnavailableReason = useMemo(() => {
    if (isLocalOwner) return null;
    return t('settings.observability.export.localOnlyHint');
  }, [isLocalOwner]);

  /* ── render ─────────────────────────────────────────────────────────── */

  if (bootstrapError) {
    return (
      <div className={styles['observability-error']} role="alert" data-kind={bootstrapError.kind}>
        <div className={styles['observability-error-title']}>
          {t(`settings.observability.error.${bootstrapError.kind === 'forbidden' ? 'forbidden' : 'network'}`)}
        </div>
        <div className={styles['observability-error-detail']}>{bootstrapError.message}</div>
      </div>
    );
  }

  if (health && !recordingEnabled && storeAbsent) {
    // §九十八：从未启用 + 无 store → onboarding（不是报错）。
    return (
      <div className={styles['observability-onboarding']}>
        <div className={styles['observability-onboarding-title']}>
          {t('settings.observability.onboarding.title')}
        </div>
        <div className={styles['observability-onboarding-body']}>
          {t('settings.observability.onboarding.body')}
        </div>
        {enableError && (
          <div className={styles['observability-error-detail']} role="alert">{enableError}</div>
        )}
        <button
          type="button"
          className={styles['observability-onboarding-enable']}
          disabled={enabling || !isLocalOwner}
          title={!isLocalOwner ? t('settings.observability.recording.localOnlyHint') : undefined}
          onClick={enableObservability}
        >
          {t('settings.observability.onboarding.enable')}
        </button>
      </div>
    );
  }

  return (
    <div className={styles['observability-root']}>
      {/* 录制状态条（低噪声，§九十六；desired vs effective 分开，§一百） */}
      {health && (
        <div className={styles['observability-recording-strip']} data-status={health.recordingStatus}>
          <span>{recordingStatusLabel(health.recordingStatus)}</span>
          {health.storeDisabledReasonCode && health.recordingStatus !== 'active' && (
            <span className={styles['observability-recording-reason']}>
              {t('settings.observability.recording.reason', { code: health.storeDisabledReasonCode })}
            </span>
          )}
          {settings?.desired.enabled && health.recordingStatus !== 'active' && (
            <span className={styles['observability-recording-reason']}>
              {t('settings.observability.recording.configuredButInactive')}
            </span>
          )}
          {!recordingEnabled && (
            <span className={styles['observability-recording-reason']}>
              {t('settings.observability.recording.disabledBrowsing')}
            </span>
          )}
        </div>
      )}

      <ObservabilityFilterBar
        state={queryState}
        refreshing={refreshing}
        onRefresh={refresh}
        onExport={() => setExportOpen(true)}
        exportAvailable={isLocalOwner}
        exportUnavailableReason={exportUnavailableReason}
        onOpenRecordingSettings={() => setSettingsOpen(true)}
      />

      {aggregateError && aggregateError !== 'not_initialized' ? (
        <div className={styles['observability-error']} role="alert" data-kind="query_failed">
          <div className={styles['observability-error-title']}>{t('settings.observability.error.query_failed')}</div>
          <div className={styles['observability-error-detail']}>{aggregateError}</div>
        </div>
      ) : (
        <>
          <section className={styles['observability-panel']}>
            <ObservabilityMetrics overall={aggregate?.overall ?? null} loading={aggregateLoading} />
          </section>
          {groupBy.length > 0 && (
            <section className={styles['observability-panel']}>
              <ObservabilityGroups
                buckets={aggregate?.groups ?? null}
                groupBy={groupBy}
                loading={aggregateLoading}
                onBucketFilter={handleBucketFilter}
              />
            </section>
          )}
        </>
      )}

      <section className={styles['observability-panel']}>
        <h3 className={styles['observability-panel-title']}>{t('settings.observability.ledger.title')}</h3>
        <ObservabilityCallLedger
          appliedFilter={appliedFilter}
          selectedCallId={queryState.selectedCallId}
          onSelectCall={handleSelectCall}
          onFilterExact={handleFilterExact}
          refreshToken={refreshToken}
        />
      </section>

      <section className={styles['observability-panel']}>
        <h3 className={styles['observability-panel-title']}>{t('settings.observability.trace.title')}</h3>
        <ObservabilityTraceExplorer
          appliedFilter={appliedFilter}
          selectedTraceId={queryState.selectedTraceId}
          onSelectTrace={handleSelectTrace}
          onSelectCall={handleSelectCall}
          refreshToken={refreshToken}
        />
      </section>

      <ObservabilityCallInspector
        callId={queryState.selectedCallId}
        isLocalOwner={isLocalOwner}
        onClose={() => queryState.selectCall(null)}
        onOpenTrace={handleSelectTrace}
      />

      <ObservabilitySettingsDialog
        open={settingsOpen}
        isLocalOwner={isLocalOwner}
        settings={settings}
        health={health}
        onClose={() => setSettingsOpen(false)}
        onApplied={() => {
          setSettingsOpen(false);
          void reloadControlPlane().then(() => setRefreshToken((token) => token + 1));
        }}
      />

      <ObservabilityExportDialog
        open={exportOpen}
        appliedFilter={appliedFilter}
        onClose={() => setExportOpen(false)}
      />
    </div>
  );
}
