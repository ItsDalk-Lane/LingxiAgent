/**
 * ModelObservabilitySection.tsx — Model Observatory 页面编排（Phase 9）。
 *
 * 页面分三个子标签页（Token 用量 / 调用台账 / 调用轨迹），子标签行是页首行
 * （页内大标题与录制状态条已移除），右端挂当前子页的刷新/导出与全局记录设置。
 * 每页一套独立筛选状态——三份 useObservabilityQueryState 实例提升持有在本壳
 * 内，面板随子标签卸载但筛选不丢。本壳保留：health/settings bootstrap、
 * 子标签行、Inspector 抽屉（浮层，不属于任何子页）、记录设置弹窗、导出弹窗、
 * 台账→轨迹的跨页跳转。
 *
 * 纪律：
 *   - 刷新 = health + 当前子页数据（§五十；绝不触发 writer flush）。
 *   - recording disabled 但 query ready → 历史照常浏览（§九十七）。
 *   - 分层 loading：health/aggregate/ledger 各自独立，不做整页白闪（§一百三十七）。
 *   - local-only 功能（export/settings PUT/payload 正文/blob）用
 *     isLocalOwnerConnection 灰化，route security 仍是最终裁决（§一百三十二）。
 */
import React, { useCallback, useEffect, useState } from 'react';
import type {
  ModelObservabilityHealthResponse,
  ModelObservabilitySettingsResponse,
} from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import { useSettingsStore } from '../../store';
import { Button, Tooltip } from '../../../ui';
import { isLocalOwnerConnection } from '../../../services/server-connection';
import styles from '../../Settings.module.css';
import {
  isObservabilityAbortError,
  loadObservabilityHealth,
  loadObservabilitySettings,
  ModelObservabilityRequestError,
} from './model-observability-actions';
import { useObservabilityQueryState } from './use-observability-query-state';
import { ObservabilityUsagePanel } from './ObservabilityUsagePanel';
import { ObservabilityLedgerPanel } from './ObservabilityLedgerPanel';
import { ObservabilityTracesPanel } from './ObservabilityTracesPanel';
import { ObservabilityCallInspector } from './ObservabilityCallInspector';
import { ObservabilitySettingsPanel } from './ObservabilitySettingsPanel';
import { ObservabilityExportDialog } from './ObservabilityExportDialog';

type BootstrapError = { kind: 'forbidden' | 'network'; message: string };

type ObservabilitySubTab = 'usage' | 'ledger' | 'traces' | 'settings';

const OBSERVABILITY_SUB_TABS: { key: ObservabilitySubTab; labelKey: string }[] = [
  { key: 'usage', labelKey: 'settings.observability.subtab.usage' },
  { key: 'ledger', labelKey: 'settings.observability.ledger.title' },
  { key: 'traces', labelKey: 'settings.observability.trace.title' },
  { key: 'settings', labelKey: 'settings.observability.subtab.settings' },
];

/** 有自己筛选状态的内容子页（「设置」页没有筛选条，也不参与导出）。 */
type ContentSubTab = Exclude<ObservabilitySubTab, 'settings'>;

function toBootstrapError(error: unknown): BootstrapError {
  if (error instanceof ModelObservabilityRequestError
    && (error.kind === 'local_only_route' || error.kind === 'studio_owner_required' || error.kind === 'forbidden')) {
    return { kind: 'forbidden', message: error.message };
  }
  return { kind: 'network', message: error instanceof Error ? error.message : String(error) };
}

export function ModelObservabilitySection() {
  // 三个子页各自独立的筛选状态（§十四的「单一事实源」收窄为「每个子页一份」）。
  const usageState = useObservabilityQueryState();
  const ledgerState = useObservabilityQueryState();
  const tracesState = useObservabilityQueryState();
  const stateByTab: Record<ContentSubTab, typeof usageState> = {
    usage: usageState,
    ledger: ledgerState,
    traces: tracesState,
  };

  const isLocalOwner = isLocalOwnerConnection(useSettingsStore((s) => s.activeServerConnection));
  const storedSubTab = useSettingsStore((s) => s.activeSubTabs.usage);
  const subTab: ObservabilitySubTab = storedSubTab === 'ledger' || storedSubTab === 'traces' || storedSubTab === 'settings'
    ? storedSubTab
    : 'usage';

  const [health, setHealth] = useState<ModelObservabilityHealthResponse | null>(null);
  const [settings, setSettings] = useState<ModelObservabilitySettingsResponse | null>(null);
  const [bootstrapError, setBootstrapError] = useState<BootstrapError | null>(null);
  const [refreshTokens, setRefreshTokens] = useState<Record<ContentSubTab | 'settings', number>>({
    usage: 0,
    ledger: 0,
    traces: 0,
    settings: 0,
  });
  const [refreshing, setRefreshing] = useState(false);
  const [exportOpen, setExportOpen] = useState(false);
  // Inspector 是浮层抽屉，从台账或轨迹都能打开，不属于任何子页。
  const [selectedCallId, setSelectedCallId] = useState<string | null>(null);

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

  const refresh = useCallback((target: ContentSubTab | 'settings') => {
    setRefreshing(true);
    setRefreshTokens((prev) => ({ ...prev, [target]: prev[target] + 1 }));
    reloadControlPlane()
      .catch((error: unknown) => setBootstrapError(toBootstrapError(error)))
      .finally(() => setRefreshing(false));
  }, [reloadControlPlane]);

  const navigateSubTab = useCallback((target: ObservabilitySubTab) => {
    // 切页时收起导出弹窗：它跟随当前子页的筛选，不能跨页滞留。
    setExportOpen(false);
    useSettingsStore.getState().navigateSettings({ tabId: 'usage', subTabId: target });
  }, []);

  const handleSelectCall = useCallback((callId: string) => {
    setSelectedCallId(callId);
  }, []);

  /* ── 跨页跳转：调用详情 → 轨迹（先收抽屉，切页，再选中轨迹）─────────── */
  const handleOpenTraceFromCall = useCallback((traceId: string) => {
    setSelectedCallId(null);
    tracesState.selectTrace(traceId);
    navigateSubTab('traces');
  }, [tracesState, navigateSubTab]);

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

  const exportButton = (
    <Button
      variant="secondary"
      size="sm"
      disabled={!isLocalOwner}
      onClick={() => setExportOpen(true)}
      aria-label={t('settings.observability.export.open')}
    >
      {t('settings.observability.export.open')}
    </Button>
  );

  return (
    <div className={styles['observability-root']}>
      <div className={styles['observability-sub-tabs']}>
        <div className={styles['observability-sub-tab-group']} role="tablist">
          {OBSERVABILITY_SUB_TABS.map((tab) => (
            <button
              key={tab.key}
              type="button"
              role="tab"
              aria-selected={subTab === tab.key}
              className={`${styles['observability-sub-tab']}${subTab === tab.key ? ` ${styles.active}` : ''}`}
              onClick={() => navigateSubTab(tab.key)}
            >
              {t(tab.labelKey)}
            </button>
          ))}
        </div>
        <div className={styles['observability-sub-tab-actions']}>
          <Button
            variant="secondary"
            size="sm"
            loading={refreshing}
            onClick={() => refresh(subTab)}
            aria-label={t('settings.observability.actions.refresh')}
          >
            {t('settings.observability.actions.refresh')}
          </Button>
          {subTab !== 'settings' && (isLocalOwner ? exportButton : (
            <Tooltip content={t('settings.observability.export.localOnlyHint')}>
              <span>{exportButton}</span>
            </Tooltip>
          ))}
        </div>
      </div>

      {subTab === 'usage' && (
        <ObservabilityUsagePanel state={usageState} refreshToken={refreshTokens.usage} />
      )}
      {subTab === 'settings' && (
        <ObservabilitySettingsPanel
          health={health}
          settings={settings}
          refreshToken={refreshTokens.settings}
          onSaved={() => {
            void reloadControlPlane().then(() => setRefreshTokens((prev) => ({
              usage: prev.usage + 1,
              ledger: prev.ledger + 1,
              traces: prev.traces + 1,
              settings: prev.settings + 1,
            })));
          }}
        />
      )}
      {subTab === 'ledger' && (
        <ObservabilityLedgerPanel
          state={ledgerState}
          refreshToken={refreshTokens.ledger}
          selectedCallId={selectedCallId}
          onSelectCall={handleSelectCall}
        />
      )}
      {subTab === 'traces' && (
        <ObservabilityTracesPanel
          state={tracesState}
          refreshToken={refreshTokens.traces}
          onSelectCall={handleSelectCall}
        />
      )}

      <ObservabilityCallInspector
        callId={selectedCallId}
        isLocalOwner={isLocalOwner}
        onClose={() => setSelectedCallId(null)}
        onOpenTrace={handleOpenTraceFromCall}
      />

      <ObservabilityExportDialog
        open={exportOpen}
        appliedFilter={subTab === 'settings' ? usageState.appliedFilter : stateByTab[subTab].appliedFilter}
        onClose={() => setExportOpen(false)}
      />
    </div>
  );
}
