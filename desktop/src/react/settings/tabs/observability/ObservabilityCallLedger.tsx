/**
 * ObservabilityCallLedger.tsx — Call Ledger（Phase 9 §四十～五十）。
 *
 * 以 callId 为键的调用台账（§四十：不是 usage requestId）。keyset cursor
 * 分页「Load More」（50/页，保留已加载页，不做滚动到底自动加载）；
 * appliedFilter 引用变化 → cursor 主动作废、从第一页重查（§四十五）。
 *
 * 请求竞态（§十二/§一百六十七）：每次取数带 generation id + AbortController，
 * 过期响应一律丢弃（stale 响应绝不写入 state）。
 *
 * 状态正交性（§三）：terminalStatus ≠ usage.status；usage_missing 只是小警告，
 * 绝不把 call 画成 error（§四十三）；interruptedByRestart 独立表达（§四十二）；
 * payloadAvailability 5 态各自 tooltip，unknown ≠ not_captured（§四十四）。
 */
import React, { useCallback, useEffect, useRef, useState } from 'react';
import type {
  ModelObservabilityCallListItem,
  ModelObservabilityCallPage,
  ModelObservabilityDataCompleteness,
} from '../../../../../../shared/model-observability-api-contract.ts';
import {
  MODEL_OBSERVABILITY_PAGE_DEFAULT_LIMIT,
  MODEL_OBSERVABILITY_PAYLOAD_AVAILABILITIES,
} from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import { Button, Tooltip } from '../../../ui';
import styles from '../../Settings.module.css';
import {
  isObservabilityAbortError,
  ModelObservabilityRequestError,
  queryObservabilityCalls,
} from './model-observability-actions';
import { buildCallFilterInput, type ObservabilityFilterState } from './model-observability-filter';
import {
  formatCompactNumber,
  formatCost,
  formatDurationMs,
  formatLocalDateTime,
  formatNumber,
  isoTooltip,
  shortId,
} from './model-observability-format';
import { payloadAvailabilityLabel, terminalStatusLabel } from './model-observability-labels';

type LedgerError = {
  kind: 'invalid_cursor' | 'not_initialized' | 'query_failed' | 'forbidden' | 'network';
  message: string;
  matchedCalls?: number | null;
  maxCalls?: number | null;
};

function toLedgerError(error: unknown): LedgerError {
  if (error instanceof ModelObservabilityRequestError) {
    if (error.kind === 'invalid_cursor') return { kind: 'invalid_cursor', message: error.message };
    if (error.kind === 'not_initialized') return { kind: 'not_initialized', message: error.message };
    if (error.kind === 'local_only_route' || error.kind === 'studio_owner_required' || error.kind === 'forbidden') {
      return { kind: 'forbidden', message: error.message };
    }
    return { kind: 'query_failed', message: error.message };
  }
  return { kind: 'network', message: error instanceof Error ? error.message : String(error) };
}

/* ── 单元格小组件 ─────────────────────────────────────────────────────── */

function StatusCell({ call }: { call: ModelObservabilityCallListItem }) {
  const status = call.terminalStatus;
  return (
    <span className={styles['observability-ledger-status']} data-status={status ?? 'unknown'}>
      <span>{terminalStatusLabel(status)}</span>
      {/* §四十二：interruptedByRestart 与 terminalStatus 分开表达 */}
      {call.interruptedByRestart && (
        <Tooltip content={t('settings.observability.ledger.interruptedTooltip')}>
          <span className={styles['observability-ledger-flag']} aria-label={t('settings.observability.ledger.interruptedAria')}>↻</span>
        </Tooltip>
      )}
      {/* §四十三：usage_missing 是小警告，绝不把 call 画成 error */}
      {call.usage.availability !== 'present' && (
        <Tooltip content={t(`settings.observability.ledger.usageMissing.${call.usage.availability}`)}>
          <span className={styles['observability-ledger-usage-warn']} aria-label={t('settings.observability.ledger.usageMissingAria')}>!</span>
        </Tooltip>
      )}
    </span>
  );
}

function PayloadCell({ call }: { call: ModelObservabilityCallListItem }) {
  const availability = MODEL_OBSERVABILITY_PAYLOAD_AVAILABILITIES.includes(
    call.payloadAvailability as typeof MODEL_OBSERVABILITY_PAYLOAD_AVAILABILITIES[number],
  ) ? call.payloadAvailability : 'unknown';
  return (
    <Tooltip content={t(`settings.observability.payloadAvailabilityHint.${availability}`)}>
      <span className={styles['observability-ledger-payload']} data-availability={availability}>
        {payloadAvailabilityLabel(availability)}
        {call.payloadRecordCount > 0 ? ` (${call.payloadRecordCount})` : ''}
      </span>
    </Tooltip>
  );
}

/* ── 主组件 ───────────────────────────────────────────────────────────── */

type Props = {
  appliedFilter: ObservabilityFilterState;
  selectedCallId: string | null;
  onSelectCall: (callId: string) => void;
  /** 行上「按此值过滤」（§二十六 高基数维度）。 */
  onFilterExact: (field: 'sessionId' | 'conversationId' | 'agentId' | 'taskId', value: string) => void;
  refreshToken: number;
};

export function ObservabilityCallLedger({ appliedFilter, selectedCallId, onSelectCall, onFilterExact, refreshToken }: Props) {
  const [calls, setCalls] = useState<ModelObservabilityCallListItem[]>([]);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [completeness, setCompleteness] = useState<ModelObservabilityDataCompleteness | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<LedgerError | null>(null);
  const generationRef = useRef(0);
  const completenessHasDrops = completeness?.status === 'known' && [
    completeness.droppedTraceEvents,
    completeness.droppedPayloadRecords,
    completeness.droppedBlobs,
    completeness.interruptedByRestartCalls,
  ].some((value) => typeof value === 'number' && value > 0);

  const fetchPage = useCallback(async (cursor: string | null, generation: number, signal: AbortSignal) => {
    return queryObservabilityCalls(
      { filter: buildCallFilterInput(appliedFilter), limit: MODEL_OBSERVABILITY_PAGE_DEFAULT_LIMIT, cursor },
      { signal },
    );
  }, [appliedFilter]);

  // filter / refresh 变化 → cursor 主动作废（§四十五），从第一页重查。
  useEffect(() => {
    const generation = ++generationRef.current;
    const controller = new AbortController();
    setLoading(true);
    setError(null);
    setCalls([]);
    setNextCursor(null);
    fetchPage(null, generation, controller.signal)
      .then((page: ModelObservabilityCallPage) => {
        if (generationRef.current !== generation) return; // stale 丢弃（§十二）
        setCalls(page.calls);
        setNextCursor(page.nextCursor);
        setCompleteness(page.dataCompleteness);
        setLoading(false);
      })
      .catch((err: unknown) => {
        if (generationRef.current !== generation || isObservabilityAbortError(err)) return;
        setError(toLedgerError(err));
        setLoading(false);
      });
    return () => controller.abort();
  }, [fetchPage, refreshToken]);

  const loadMore = useCallback(() => {
    if (!nextCursor || loadingMore) return;
    const generation = generationRef.current;
    const controller = new AbortController();
    setLoadingMore(true);
    fetchPage(nextCursor, generation, controller.signal)
      .then((page) => {
        if (generationRef.current !== generation) return;
        // callId 去重防御：keyset 不重，但写入侧并发时重叠宁可去重不重复渲染。
        setCalls((prev) => {
          const seen = new Set(prev.map((call) => call.callId));
          return [...prev, ...page.calls.filter((call) => !seen.has(call.callId))];
        });
        setNextCursor(page.nextCursor);
        setCompleteness(page.dataCompleteness);
        setLoadingMore(false);
      })
      .catch((err: unknown) => {
        if (generationRef.current !== generation || isObservabilityAbortError(err)) return;
        setError(toLedgerError(err));
        setLoadingMore(false);
      });
  }, [fetchPage, nextCursor, loadingMore]);

  /* ── 分层状态（§一百三十七：distinct error/empty states）────────────── */

  if (error?.kind === 'not_initialized') {
    return (
      <div className={styles['observability-empty']} data-state="not-initialized">
        {t('settings.observability.empty.storeAbsent')}
      </div>
    );
  }
  if (error) {
    return (
      <div className={styles['observability-error']} role="alert" data-kind={error.kind}>
        <div className={styles['observability-error-title']}>
          {t(`settings.observability.error.${error.kind}`)}
        </div>
        <div className={styles['observability-error-detail']}>{error.message}</div>
        {error.kind === 'invalid_cursor' && (
          <div className={styles['observability-error-detail']}>
            {t('settings.observability.error.invalidCursorHint')}
          </div>
        )}
      </div>
    );
  }

  return (
    <div className={styles['observability-ledger']}>
      {/* §四十四：dataCompleteness 是全局累计事实，非阻塞 warning */}
      {completeness && (completeness.status === 'unknown' || completenessHasDrops) && (
        <div className={styles['observability-completeness-note']} data-completeness={completeness.status} role="status">
          {completeness.status === 'unknown'
            ? t('settings.observability.ledger.completenessUnknown')
            : t('settings.observability.ledger.completenessNote', {
              droppedEvents: formatNumber(completeness.droppedTraceEvents),
              droppedPayloads: formatNumber(completeness.droppedPayloadRecords),
              droppedBlobs: formatNumber(completeness.droppedBlobs),
              interrupted: formatNumber(completeness.interruptedByRestartCalls),
            })}
        </div>
      )}
      <div className={styles['observability-ledger-scroll']} data-loading={loading || undefined}>
        <table className={styles['observability-ledger-table']}>
          <thead>
            <tr>
              <th>{t('settings.observability.ledger.col.time')}</th>
              <th>{t('settings.observability.ledger.col.category')}</th>
              <th>{t('settings.observability.ledger.col.operation')}</th>
              <th>{t('settings.observability.ledger.col.model')}</th>
              <th>{t('settings.observability.ledger.col.status')}</th>
              <th>{t('settings.observability.ledger.col.duration')}</th>
              <th>{t('settings.observability.ledger.col.tokens')}</th>
              <th>{t('settings.observability.ledger.col.cache')}</th>
              <th>{t('settings.observability.ledger.col.cost')}</th>
              <th>{t('settings.observability.ledger.col.attempts')}</th>
              <th>{t('settings.observability.ledger.col.payload')}</th>
              <th>{t('settings.observability.ledger.col.context')}</th>
            </tr>
          </thead>
          <tbody>
            {calls.map((call) => (
              <tr
                key={call.callId}
                data-selected={call.callId === selectedCallId || undefined}
                onClick={() => onSelectCall(call.callId)}
                tabIndex={0}
                onKeyDown={(event) => {
                  if (event.key === 'Enter' || event.key === ' ') {
                    event.preventDefault();
                    onSelectCall(call.callId);
                  }
                }}
              >
                <td title={isoTooltip(call.startedAt)}>{formatLocalDateTime(call.startedAt)}</td>
                <td>{call.source.subsystem ?? '—'}</td>
                <td>{call.source.operation ?? '—'}</td>
                <td title={call.model.modelId ?? undefined}>
                  {call.model.modelId ?? '—'}
                  {call.model.provider && <span className={styles['observability-ledger-muted']}> · {call.model.provider}</span>}
                </td>
                <td><StatusCell call={call} /></td>
                <td title={call.durationMs !== null ? `${formatNumber(call.durationMs)}ms` : undefined}>
                  {formatDurationMs(call.durationMs)}
                </td>
                <td title={call.usage.summary ? formatNumber(call.usage.summary.totalTokens) : undefined}>
                  {call.usage.summary ? formatCompactNumber(call.usage.summary.totalTokens) : '—'}
                </td>
                <td title={call.usage.summary ? formatNumber(call.usage.summary.cacheReadTokens) : undefined}>
                  {call.usage.summary ? formatCompactNumber(call.usage.summary.cacheReadTokens) : '—'}
                </td>
                <td>{formatCost(call.usage.summary?.costTotal ?? null)}</td>
                <td title={t('settings.observability.ledger.attemptsTooltip', { providerRequests: call.providerRequestCount })}>
                  {call.attemptCount}
                </td>
                <td><PayloadCell call={call} /></td>
                <td>
                  <span className={styles['observability-ledger-context']}>
                    {call.attribution.sessionId && (
                      <button
                        type="button"
                        className={styles['observability-ledger-context-id']}
                        title={t('settings.observability.ledger.filterBySession', { id: call.attribution.sessionId })}
                        onClick={(event) => {
                          event.stopPropagation();
                          onFilterExact('sessionId', call.attribution.sessionId!);
                        }}
                      >
                        {shortId(call.attribution.sessionId)}
                      </button>
                    )}
                    {call.attribution.taskId && (
                      <button
                        type="button"
                        className={styles['observability-ledger-context-id']}
                        title={t('settings.observability.ledger.filterByTask', { id: call.attribution.taskId })}
                        onClick={(event) => {
                          event.stopPropagation();
                          onFilterExact('taskId', call.attribution.taskId!);
                        }}
                      >
                        {shortId(call.attribution.taskId)}
                      </button>
                    )}
                    {!call.attribution.sessionId && !call.attribution.taskId && '—'}
                  </span>
                </td>
              </tr>
            ))}
            {!loading && calls.length === 0 && (
              <tr>
                <td colSpan={12}>
                  <div className={styles['observability-empty']} data-state="no-results">
                    {t('settings.observability.empty.noResults')}
                  </div>
                </td>
              </tr>
            )}
          </tbody>
        </table>
        {loading && (
          <div className={styles['observability-loading']} aria-busy>
            {t('settings.observability.loading.ledger')}
          </div>
        )}
      </div>
      {nextCursor && !loading && (
        <div className={styles['observability-ledger-more']}>
          <Button variant="secondary" size="sm" loading={loadingMore} onClick={loadMore}>
            {t('settings.observability.ledger.loadMore')}
          </Button>
        </div>
      )}
    </div>
  );
}
