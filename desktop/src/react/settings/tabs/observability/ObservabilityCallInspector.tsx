import React, { useEffect, useMemo, useRef, useState } from 'react';
import type {
  ModelObservabilityCallDetail,
  ModelObservabilityPayloadRecordDetail,
} from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import { Overlay } from '../../../ui';
import styles from '../../Settings.module.css';
import {
  isObservabilityAbortError,
  loadObservabilityCallDetail,
  loadObservabilityPayloadRecord,
  ModelObservabilityRequestError,
} from './model-observability-actions';
import {
  formatDurationMs,
  formatLocalFullDateTime,
  formatNumber,
} from './model-observability-format';
import {
  payloadKindLabel,
  sourceIdentityKindLabel,
  sourceIdentityTitle,
  terminalStatusLabel,
} from './model-observability-labels';
import { payloadToReadableText } from './trace-detail/payload-plain-text';
import { PayloadPlainView } from './trace-detail/TrajectoryTable';

type PayloadState =
  | { status: 'loading' }
  | { status: 'loaded'; detail: ModelObservabilityPayloadRecordDetail }
  | { status: 'error'; message: string };

const PAYLOAD_ORDER: Record<string, number> = {
  semantic_request: 0,
  provider_request: 1,
  provider_response: 2,
  semantic_response: 3,
};

function Metric({ label, children }: { label: string; children: React.ReactNode }) {
  return <div className={styles['observability-summary-row']}><span className={styles['observability-summary-label']}>{label}</span><span>{children}</span></div>;
}

function usageText(detail: ModelObservabilityCallDetail): string {
  const usage = detail.call.usage.summary;
  if (!usage) return '—';
  const parts = [
    [t('settings.observability.inspector.usage.input'), usage.inputTokens],
    [t('settings.observability.inspector.usage.output'), usage.outputTokens],
    [t('settings.observability.inspector.usage.cacheRead'), usage.cacheReadTokens],
    [t('settings.observability.inspector.usage.cacheWrite'), usage.cacheWriteTokens],
    [t('settings.observability.inspector.usage.reasoning'), usage.reasoningTokens],
  ].filter((entry): entry is [string, number] => typeof entry[1] === 'number')
    .map(([label, value]) => `${label} ${formatNumber(value)}`);
  if (typeof usage.costTotal === 'number') parts.push(`${t('settings.observability.inspector.usage.cost')} $${usage.costTotal.toFixed(6)}`);
  return parts.join(' · ') || '—';
}

export function ObservabilityCallInspector({ callId, isLocalOwner, onClose, onOpenTrace }: {
  callId: string | null;
  isLocalOwner: boolean;
  onClose: () => void;
  onOpenTrace: (traceId: string) => void;
}) {
  const [detail, setDetail] = useState<ModelObservabilityCallDetail | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<{ kind: string; message: string } | null>(null);
  const [payloads, setPayloads] = useState<Record<number, PayloadState>>({});
  const generationRef = useRef(0);

  useEffect(() => {
    if (!callId) { setDetail(null); setPayloads({}); return; }
    const generation = ++generationRef.current;
    const controller = new AbortController();
    setLoading(true); setError(null); setDetail(null); setPayloads({});
    loadObservabilityCallDetail(callId, { signal: controller.signal })
      .then(value => {
        if (generationRef.current !== generation) return;
        setDetail(value); setLoading(false);
        const initial = Object.fromEntries(value.payloadRecords.map(record => [record.id, { status: 'loading' }])) as Record<number, PayloadState>;
        setPayloads(initial);
        // 打开详情即并行加载全部载荷；单条失败只标记该条，不影响其他正文。
        for (const record of value.payloadRecords) {
          loadObservabilityPayloadRecord(record.id, { signal: controller.signal })
            .then(payload => {
              if (generationRef.current === generation) setPayloads(current => ({ ...current, [record.id]: { status: 'loaded', detail: payload } }));
            })
            .catch((reason: unknown) => {
              if (isObservabilityAbortError(reason) || generationRef.current !== generation) return;
              setPayloads(current => ({ ...current, [record.id]: { status: 'error', message: reason instanceof Error ? reason.message : String(reason) } }));
            });
        }
      })
      .catch((reason: unknown) => {
        if (generationRef.current !== generation || isObservabilityAbortError(reason)) return;
        setError({
          kind: reason instanceof ModelObservabilityRequestError && reason.kind === 'not_found' ? 'not_found' : 'query_failed',
          message: reason instanceof Error ? reason.message : String(reason),
        });
        setLoading(false);
      });
    return () => controller.abort();
  }, [callId]);

  const orderedRecords = useMemo(() => {
    const sorted = [...(detail?.payloadRecords ?? [])].sort((left, right) =>
      (PAYLOAD_ORDER[left.kind] ?? 99) - (PAYLOAD_ORDER[right.kind] ?? 99)
      || (left.providerRequestOrdinal ?? 0) - (right.providerRequestOrdinal ?? 0)
      || left.id - right.id);
    // 同 kind 多条（重试/回放的多次供应商请求）时，折叠标题附序号消歧。
    const totals = new Map<string, number>();
    for (const record of sorted) totals.set(record.kind, (totals.get(record.kind) ?? 0) + 1);
    const withinByKind = new Map<string, number>();
    return sorted.map(record => {
      const within = (withinByKind.get(record.kind) ?? 0) + 1;
      withinByKind.set(record.kind, within);
      return { record, within, repeated: (totals.get(record.kind) ?? 0) > 1 };
    });
  }, [detail]);
  if (!callId) return null;

  const showRequests = detail !== null && (
    detail.call.terminalStatus !== 'ok'
    || detail.attempts.length > 1
    || detail.call.providerRequestCount > 1
  );
  const attemptErrors = detail?.attempts.filter(attempt => attempt.errorName || attempt.errorCode || attempt.httpStatus && attempt.httpStatus >= 400) ?? [];

  return <Overlay open scope="inline" onClose={onClose} closeOnEsc closeOnBackdrop trapFocus
    className={styles['observability-inspector-layer']} backdrop="none"
    contentProps={{ role: 'dialog', 'aria-label': t('settings.observability.inspector.ariaLabel') }}>
    <div className={styles['observability-inspector-backdrop']} onMouseDown={event => { if (event.target === event.currentTarget) onClose(); }}>
      <div className={styles['observability-inspector']} data-layout="detail-page">
        <div className={styles['observability-inspector-head']}>
          <div className={styles['observability-inspector-title']}>{t('settings.observability.inspector.title')}</div>
          <button type="button" className={styles['observability-inspector-close']} aria-label={t('settings.observability.inspector.closeAria')} onClick={onClose}>×</button>
        </div>
        {loading && <div className={styles['observability-loading']} aria-busy>{t('settings.observability.loading.detail')}</div>}
        {error && <div className={styles['observability-error']} role="alert" data-kind={error.kind}><div className={styles['observability-error-detail']}>{error.message}</div></div>}
        {detail && <div className={styles['observability-inspector-body']}>
          <section className={styles['observability-inspector-section']}>
            <h4>{sourceIdentityTitle(detail.call.sourceIdentity)} <small>· {sourceIdentityKindLabel(detail.call.sourceIdentity?.kind ?? 'unknown')}</small></h4>
            <div className={styles['observability-inspector-summary']}>
              <Metric label={t('settings.observability.inspector.field.callPurpose')}>{detail.call.callPurpose || detail.call.source.operation || '—'}</Metric>
              <Metric label={t('settings.observability.inspector.model')}>{detail.call.model.modelId || '—'}{detail.call.model.provider ? ` · ${detail.call.model.provider}` : ''}</Metric>
              <Metric label={t('settings.observability.ledger.col.status')}>{terminalStatusLabel(detail.call.terminalStatus)}</Metric>
              <Metric label={t('settings.observability.inspector.start')}>{formatLocalFullDateTime(detail.call.startedAt)}</Metric>
              <Metric label={t('settings.observability.ledger.col.duration')}>{formatDurationMs(detail.call.durationMs)}</Metric>
              <Metric label={t('settings.observability.inspector.field.usage')}>{usageText(detail)}</Metric>
            </div>
            {attemptErrors.length > 0 && <div className={styles['observability-error-detail']} role="status">
              {attemptErrors.map(attempt => [attempt.errorName, attempt.errorCode, attempt.httpStatus].filter(Boolean).join(' · ')).join('\n')}
            </div>}
          </section>

          {showRequests && <section className={styles['observability-inspector-section']}>
            <h4>{t('settings.observability.inspector.attempts')}</h4>
            <div className={styles['observability-ledger-muted']}>
              {detail.attempts.map((attempt, index) => `${index + 1}. ${formatLocalFullDateTime(attempt.startedAt)} · HTTP ${attempt.httpStatus ?? '—'}${attempt.errorName || attempt.errorCode ? ` · ${[attempt.errorName, attempt.errorCode].filter(Boolean).join(' / ')}` : ''}`).join('\n') || '—'}
            </div>
          </section>}

          <section className={styles['observability-inspector-section']}>
            <h4>{t('settings.observability.inspector.pipeline')}</h4>
            {!isLocalOwner && <div className={styles['observability-provenance-note']}>{t('settings.observability.recording.localOnlyHint')}</div>}
            {orderedRecords.length === 0 && <div className={styles['observability-empty']}>{t('settings.observability.payload.empty')}</div>}
            {orderedRecords.map(({ record, within, repeated }) => {
              const state = payloads[record.id];
              const ordinalSuffix = repeated
                ? ` · ${record.providerRequestOrdinal !== null ? `req #${record.providerRequestOrdinal}` : `#${within}`}`
                : '';
              return (
                <details key={record.id} className={styles['observability-fold']}>
                  <summary className={styles['observability-fold-head']}>
                    <span className={styles['observability-fold-title']}>{payloadKindLabel(record.kind)}{ordinalSuffix}</span>
                  </summary>
                  <div className={styles['observability-fold-body']}>
                    {!state || state.status === 'loading' ? (
                      <div className={styles['observability-loading']}>{t('settings.observability.loading.payload')}</div>
                    ) : state.status === 'error' ? (
                      <div className={styles['observability-error-detail']} role="alert">{state.message}</div>
                    ) : (
                      <PayloadPlainView record={record} text={payloadToReadableText(state.detail.payload)} hideKindLabel />
                    )}
                  </div>
                </details>
              );
            })}
          </section>

          <details className={styles['observability-fold']}>
            <summary className={styles['observability-fold-head']}>
              <span className={styles['observability-fold-title']}>{t('settings.observability.inspector.technical')}</span>
            </summary>
            <div className={styles['observability-fold-body']}>
              <div className={styles['observability-inspector-summary']}>
                <Metric label="Call ID"><code>{detail.call.callId}</code></Metric>
                <Metric label="Trace ID"><code>{detail.call.traceId || '—'}</code></Metric>
                <Metric label={t('settings.observability.inspector.origin')}>{detail.trace?.origin || '—'}</Metric>
                <Metric label={t('settings.observability.inspector.field.attribution')}>{detail.call.attribution.kind || '—'} · {detail.call.sourceIdentity?.resolution || 'unknown'}</Metric>
                <Metric label={t('settings.observability.inspector.field.inputShape')}>{detail.call.inputShape || '—'}</Metric>
                <Metric label={t('settings.observability.inspector.field.persistence')}>{detail.call.persistenceCompleteness}</Metric>
                <Metric label={t('settings.observability.inspector.attempts')}>{detail.call.attemptCount} / {detail.call.providerRequestCount}</Metric>
              </div>
              {detail.call.traceId && <button type="button" className={styles['observability-link-btn']} onClick={() => onOpenTrace(detail.call.traceId!)}>{t('settings.observability.inspector.openTrace')}</button>}
            </div>
          </details>
        </div>}
      </div>
    </div>
  </Overlay>;
}
