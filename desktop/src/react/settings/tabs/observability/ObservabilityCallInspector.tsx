/**
 * ObservabilityCallInspector.tsx — Call Inspector 右侧抽屉（Phase 9
 * §五十一～七十一）。
 *
 *   - 抽屉宽度 52%/min(720px)，窄窗口全宽 overlay（CSS 处理；§五十一）。
 *   - header：Call ID / Trace ID / Model / Category / Start / Duration /
 *     Status / Usage / PayloadAvailability + 复制按钮（§五十二）。
 *   - 分节 Overview / Attempts / Input-Output Pipeline / Trace（§五十四：
 *     是 drawer 内分节，不是页面级 tab）。
 *   - Attempt 表展示真实 model_attempts 行；Attempt ≠ Provider Request
 *     明说（§五十八：Codex Image 401 是 2 attempts 2 provider requests；
 *     Pi logical_boundary 不得伪造成重试计数）。
 *   - Pipeline viewer：Semantic Request → Attempt N（Provider Request /
 *     Response）→ Semantic Response（§五十九）。
 *   - Payload 正文 per-card 懒加载（§六十二）；复制按钮文案
 *     「Copy captured payload」（§一百五十六：不是 Copy raw request）。
 */
import React, { useCallback, useEffect, useRef, useState } from 'react';
import type {
  ModelObservabilityAttemptSummary,
  ModelObservabilityCallDetail,
  ModelObservabilityPayloadRecordMetadata,
} from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import { Overlay, Tooltip } from '../../../ui';
import styles from '../../Settings.module.css';
import {
  isObservabilityAbortError,
  loadObservabilityCallDetail,
  loadObservabilityPayloadRecord,
  ModelObservabilityRequestError,
} from './model-observability-actions';
import {
  ObservabilityPayloadCard,
  payloadBodyErrorState,
  type PayloadBodyState,
} from './ObservabilityPayloadCard';
import {
  formatDurationMs,
  formatLocalFullDateTime,
  formatNumber,
  isoTooltip,
  shortId,
} from './model-observability-format';
import {
  attributionKindLabel,
  inputShapeLabel,
  operationLabel,
  originLabel,
  payloadAvailabilityLabel,
  persistenceCompletenessLabel,
  provenancePrecisionLabel,
  subsystemLabel,
  terminalStatusLabel,
  usageAvailabilityLabel,
  usageStatusLabel,
} from './model-observability-labels';

/* ── 复制按钮（§五十二/§一百五十六）────────────────────────────────────── */

function CopyButton({ value, ariaLabel }: { value: string; ariaLabel: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      className={styles['observability-copy-btn']}
      aria-label={ariaLabel}
      onClick={() => {
        void navigator.clipboard?.writeText(value).then(() => {
          setCopied(true);
          setTimeout(() => setCopied(false), 1500);
        });
      }}
    >
      {copied ? '✓' : '⧉'}
    </button>
  );
}

/* ── Attempts 表（§五十七）─────────────────────────────────────────────── */

function AttemptsTable({ attempts, providerRequestCount }: {
  attempts: ModelObservabilityAttemptSummary[];
  providerRequestCount: number;
}) {
  return (
    <div>
      <table className={styles['observability-attempts-table']}>
        <thead>
          <tr>
            <th>{t('settings.observability.attempts.col.attemptId')}</th>
            <th>{t('settings.observability.attempts.col.started')}</th>
            <th>{t('settings.observability.attempts.col.responded')}</th>
            <th>{t('settings.observability.attempts.col.httpStatus')}</th>
            <th>{t('settings.observability.attempts.col.error')}</th>
          </tr>
        </thead>
        <tbody>
          {attempts.map((attempt) => (
            <tr key={attempt.attemptId}>
              <td title={attempt.attemptId}>{shortId(attempt.attemptId)}</td>
              <td title={isoTooltip(attempt.startedAt)}>{formatLocalFullDateTime(attempt.startedAt)}</td>
              <td title={isoTooltip(attempt.responseReceivedAt)}>{formatLocalFullDateTime(attempt.responseReceivedAt)}</td>
              <td>{attempt.httpStatus ?? '—'}</td>
              <td>
                {attempt.errorName || attempt.errorCode
                  ? [attempt.errorName, attempt.errorCode].filter(Boolean).join(' / ')
                  : '—'}
              </td>
            </tr>
          ))}
          {attempts.length === 0 && (
            <tr><td colSpan={5}>{t('settings.observability.attempts.empty')}</td></tr>
          )}
        </tbody>
      </table>
      {/* §五十八：Attempt ≠ Provider Request——明说，不伪造 */}
      <div className={styles['observability-attempts-note']}>
        {t('settings.observability.attempts.notEqualNote', {
          attempts: formatNumber(attempts.length),
          providerRequests: formatNumber(providerRequestCount),
        })}
      </div>
    </div>
  );
}

/* ── Pipeline viewer（§五十九）────────────────────────────────────────── */

type PipelineGroups = {
  semanticRequest: ModelObservabilityPayloadRecordMetadata[];
  semanticResponse: ModelObservabilityPayloadRecordMetadata[];
  byAttempt: Array<{
    attemptKey: string;
    requests: ModelObservabilityPayloadRecordMetadata[];
    responses: ModelObservabilityPayloadRecordMetadata[];
  }>;
};

function groupPipeline(records: ModelObservabilityPayloadRecordMetadata[]): PipelineGroups {
  const semanticRequest = records.filter((record) => record.kind === 'semantic_request');
  const semanticResponse = records.filter((record) => record.kind === 'semantic_response');
  const attemptMap = new Map<string, { requests: ModelObservabilityPayloadRecordMetadata[]; responses: ModelObservabilityPayloadRecordMetadata[] }>();
  for (const record of records) {
    if (record.kind !== 'provider_request' && record.kind !== 'provider_response') continue;
    const key = record.attemptId ?? 'unknown';
    const bucket = attemptMap.get(key) ?? { requests: [], responses: [] };
    if (record.kind === 'provider_request') bucket.requests.push(record);
    else bucket.responses.push(record);
    attemptMap.set(key, bucket);
  }
  const byAttempt = [...attemptMap.entries()].map(([attemptKey, bucket]) => ({
    attemptKey,
    requests: [...bucket.requests].sort((a, b) => (a.providerRequestOrdinal ?? 0) - (b.providerRequestOrdinal ?? 0)),
    responses: [...bucket.responses].sort((a, b) => (a.providerRequestOrdinal ?? 0) - (b.providerRequestOrdinal ?? 0)),
  }));
  return { semanticRequest, semanticResponse, byAttempt };
}

/* ── 主组件 ───────────────────────────────────────────────────────────── */

export function ObservabilityCallInspector({ callId, isLocalOwner, onClose, onOpenTrace }: {
  callId: string | null;
  isLocalOwner: boolean;
  onClose: () => void;
  onOpenTrace: (traceId: string) => void;
}) {
  const [detail, setDetail] = useState<ModelObservabilityCallDetail | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<{ kind: string; message: string } | null>(null);
  const [bodies, setBodies] = useState<Record<number, PayloadBodyState>>({});
  const [highlightOrdinal, setHighlightOrdinal] = useState<number | null>(null);
  const generationRef = useRef(0);

  useEffect(() => {
    if (!callId) {
      setDetail(null);
      setBodies({});
      setHighlightOrdinal(null);
      return;
    }
    const generation = ++generationRef.current;
    const controller = new AbortController();
    setLoading(true);
    setError(null);
    setDetail(null);
    setBodies({});
    setHighlightOrdinal(null);
    loadObservabilityCallDetail(callId, { signal: controller.signal })
      .then((value) => {
        if (generationRef.current !== generation) return;
        setDetail(value);
        setLoading(false);
      })
      .catch((err: unknown) => {
        if (generationRef.current !== generation || isObservabilityAbortError(err)) return;
        if (err instanceof ModelObservabilityRequestError && err.kind === 'not_found') {
          setError({ kind: 'not_found', message: err.message });
        } else {
          setError({ kind: 'query_failed', message: err instanceof Error ? err.message : String(err) });
        }
        setLoading(false);
      });
    return () => controller.abort();
  }, [callId]);

  const loadBody = useCallback((recordId: number) => {
    setBodies((prev) => ({ ...prev, [recordId]: { status: 'loading' } }));
    loadObservabilityPayloadRecord(recordId)
      .then((record) => {
        setBodies((prev) => ({ ...prev, [recordId]: { status: 'loaded', detail: record } }));
      })
      .catch((err: unknown) => {
        setBodies((prev) => ({ ...prev, [recordId]: payloadBodyErrorState(err) }));
      });
  }, []);

  /** §八十四 交叉跳转：provider mapping → semantic section（必要时先载正文）。 */
  const jumpToSection = useCallback((ordinal: number) => {
    setHighlightOrdinal(ordinal);
    if (!detail) return;
    const semanticRecord = detail.payloadRecords.find(
      (record) => record.kind === 'semantic_request' && record.hasSemanticProvenance,
    );
    if (semanticRecord && !bodies[semanticRecord.id]) {
      loadBody(semanticRecord.id);
    }
  }, [detail, bodies, loadBody]);

  const pipeline = detail ? groupPipeline(detail.payloadRecords) : null;

  if (!callId) return null;

  return (
    <Overlay
      open
      scope="inline"
      onClose={onClose}
      closeOnEsc
      closeOnBackdrop
      trapFocus
      className={styles['observability-inspector-backdrop']}
      backdrop="none"
      contentProps={{ role: 'dialog', 'aria-label': t('settings.observability.inspector.ariaLabel') }}
    >
      <div className={styles['observability-inspector']}>
        <div className={styles['observability-inspector-head']}>
          <div className={styles['observability-inspector-title']}>
            {t('settings.observability.inspector.title')}
          </div>
          <button
            type="button"
            className={styles['observability-inspector-close']}
            aria-label={t('settings.observability.inspector.closeAria')}
            onClick={onClose}
          >
            ×
          </button>
        </div>

        {loading && (
          <div className={styles['observability-loading']} aria-busy>
            {t('settings.observability.loading.detail')}
          </div>
        )}
        {error && (
          <div className={styles['observability-error']} role="alert" data-kind={error.kind}>
            <div className={styles['observability-error-title']}>
              {t(`settings.observability.error.${error.kind === 'not_found' ? 'not_found' : 'query_failed'}`)}
            </div>
            <div className={styles['observability-error-detail']}>{error.message}</div>
          </div>
        )}

        {detail && (
          <div className={styles['observability-inspector-body']}>
            {/* ── Header 摘要（§五十二）── */}
            <div className={styles['observability-inspector-summary']}>
              <div className={styles['observability-summary-row']}>
                <span className={styles['observability-summary-label']}>Call ID</span>
                <code title={detail.call.callId}>{shortId(detail.call.callId)}</code>
                <CopyButton value={detail.call.callId} ariaLabel={t('settings.observability.copy.callIdAria')} />
              </div>
              <div className={styles['observability-summary-row']}>
                <span className={styles['observability-summary-label']}>Trace ID</span>
                {detail.call.traceId ? (
                  <>
                    <code title={detail.call.traceId}>{shortId(detail.call.traceId)}</code>
                    <CopyButton value={detail.call.traceId} ariaLabel={t('settings.observability.copy.traceIdAria')} />
                    <button
                      type="button"
                      className={styles['observability-link-btn']}
                      onClick={() => onOpenTrace(detail.call.traceId!)}
                    >
                      {t('settings.observability.inspector.openTrace')}
                    </button>
                  </>
                ) : '—'}
              </div>
              <div className={styles['observability-summary-row']}>
                <span className={styles['observability-summary-label']}>
                  {t('settings.observability.inspector.model')}
                </span>
                <span>
                  {detail.call.model.modelId ?? '—'}
                  {detail.call.model.provider && ` (${detail.call.model.provider})`}
                  {detail.call.model.api && ` · ${detail.call.model.api}`}
                </span>
              </div>
              <div className={styles['observability-summary-row']}>
                <span className={styles['observability-summary-label']}>
                  {t('settings.observability.ledger.col.category')}
                </span>
                <span title={[detail.call.source.subsystem, detail.call.source.operation].filter(Boolean).join(' / ') || undefined}>
                  {subsystemLabel(detail.call.source.subsystem)} / {operationLabel(detail.call.source.operation)}
                </span>
              </div>
              <div className={styles['observability-summary-row']}>
                <span className={styles['observability-summary-label']}>
                  {t('settings.observability.inspector.start')}
                </span>
                <span title={isoTooltip(detail.call.startedAt)}>{formatLocalFullDateTime(detail.call.startedAt)}</span>
                <span className={styles['observability-ledger-muted']}>
                  {' · '}{formatDurationMs(detail.call.durationMs)}
                </span>
              </div>
              <div className={styles['observability-summary-row']}>
                <span className={styles['observability-badge']} data-status={detail.call.terminalStatus ?? 'unknown'}>
                  {terminalStatusLabel(detail.call.terminalStatus)}
                </span>
                {detail.call.interruptedByRestart && (
                  <Tooltip content={t('settings.observability.ledger.interruptedTooltip')}>
                    <span className={styles['observability-ledger-flag']}>↻</span>
                  </Tooltip>
                )}
                <span className={styles['observability-badge']} data-kind="usage">
                  {usageAvailabilityLabel(detail.call.usage.availability)}
                  {detail.call.usage.status ? ` / ${usageStatusLabel(detail.call.usage.status)}` : ''}
                </span>
                <span className={styles['observability-badge']} data-kind="payload">
                  {payloadAvailabilityLabel(detail.call.payloadAvailability)}
                </span>
              </div>
            </div>

            {/* ── Overview（§五十六）── */}
            <section>
              <h4 className={styles['observability-panel-subtitle']}>{t('settings.observability.inspector.overview')}</h4>
              <dl className={styles['observability-provenance-meta']}>
                <dt>{t('settings.observability.inspector.field.callPurpose')}</dt>
                <dd>{detail.call.callPurpose ?? '—'}</dd>
                <dt>{t('settings.observability.inspector.field.inputShape')}</dt>
                <dd>{inputShapeLabel(detail.call.inputShape)}</dd>
                <dt>{t('settings.observability.inspector.field.provenancePrecision')}</dt>
                <dd>{provenancePrecisionLabel(detail.call.provenancePrecision)}</dd>
                <dt>{t('settings.observability.inspector.field.surfaceTrigger')}</dt>
                <dd>{[detail.call.source.surface, detail.call.source.trigger].filter(Boolean).join(' / ') || '—'}</dd>
                <dt>{t('settings.observability.inspector.field.attribution')}</dt>
                <dd>
                  {[
                    attributionKindLabel(detail.call.attribution.kind),
                    detail.call.attribution.agentId,
                    detail.call.attribution.sessionId,
                    detail.call.attribution.conversationId,
                    detail.call.attribution.taskId,
                  ].filter(Boolean).join(' · ') || '—'}
                </dd>
                <dt>{t('settings.observability.inspector.field.persistence')}</dt>
                <dd>{persistenceCompletenessLabel(detail.call.persistenceCompleteness)}</dd>
                <dt>{t('settings.observability.inspector.field.interruptedByRestart')}</dt>
                <dd>{detail.call.interruptedByRestart
                  ? t('settings.observability.tri.yes')
                  : t('settings.observability.tri.no')}</dd>
                <dt>{t('settings.observability.inspector.field.attempts')}</dt>
                <dd>{formatNumber(detail.call.attemptCount)}</dd>
                <dt>{t('settings.observability.inspector.field.payloadRecords')}</dt>
                <dd>{formatNumber(detail.call.payloadRecordCount)}</dd>
                <dt>{t('settings.observability.inspector.field.usage')}</dt>
                <dd>
                  {detail.call.usage.summary
                    ? [
                      `in ${formatNumber(detail.call.usage.summary.inputTokens)}`,
                      `out ${formatNumber(detail.call.usage.summary.outputTokens)}`,
                      `cache ${formatNumber(detail.call.usage.summary.cacheReadTokens)}`,
                    ].join(' · ')
                    : usageAvailabilityLabel(detail.call.usage.availability)}
                </dd>
              </dl>
              {(detail.parentCall || detail.childCalls.length > 0) && (
                <div className={styles['observability-inspector-family']}>
                  {detail.parentCall && (
                    <button
                      type="button"
                      className={styles['observability-link-btn']}
                      onClick={() => onOpenTrace(detail.call.traceId ?? '')}
                      title={detail.parentCall.callId}
                    >
                      {t('settings.observability.inspector.parentCall', { id: shortId(detail.parentCall.callId) })}
                    </button>
                  )}
                  {detail.childCalls.length > 0 && (
                    <span>
                      {t('settings.observability.inspector.childCalls', { count: detail.childCalls.length })}
                    </span>
                  )}
                </div>
              )}
            </section>

            {/* ── Attempts（§五十七）── */}
            <section>
              <h4 className={styles['observability-panel-subtitle']}>{t('settings.observability.inspector.attempts')}</h4>
              <AttemptsTable attempts={detail.attempts} providerRequestCount={detail.call.providerRequestCount} />
            </section>

            {/* ── Input-Output Pipeline（§五十九）── */}
            <section>
              <h4 className={styles['observability-panel-subtitle']}>{t('settings.observability.inspector.pipeline')}</h4>
              {detail.payloadRecords.length === 0 && (
                <div className={styles['observability-provenance-note']}>
                  {t(`settings.observability.payload.availabilityEmpty.${detail.call.payloadAvailability}`)}
                </div>
              )}
              {pipeline && (
                <div className={styles['observability-pipeline']}>
                  {pipeline.semanticRequest.map((record) => (
                    <ObservabilityPayloadCard
                      key={record.id}
                      metadata={record}
                      body={bodies[record.id] ?? { status: 'idle' }}
                      isLocalOwner={isLocalOwner}
                      onLoadBody={loadBody}
                      highlightOrdinal={highlightOrdinal}
                    />
                  ))}
                  {pipeline.byAttempt.map((group) => (
                    <div key={group.attemptKey} className={styles['observability-pipeline-attempt']}>
                      <div className={styles['observability-pipeline-attempt-label']}>
                        {t('settings.observability.pipeline.attemptLabel', { id: shortId(group.attemptKey) })}
                      </div>
                      {[...group.requests, ...group.responses].map((record) => (
                        <ObservabilityPayloadCard
                          key={record.id}
                          metadata={record}
                          body={bodies[record.id] ?? { status: 'idle' }}
                          isLocalOwner={isLocalOwner}
                          onLoadBody={loadBody}
                          onJumpToSection={jumpToSection}
                        />
                      ))}
                    </div>
                  ))}
                  {pipeline.semanticResponse.map((record) => (
                    <ObservabilityPayloadCard
                      key={record.id}
                      metadata={record}
                      body={bodies[record.id] ?? { status: 'idle' }}
                      isLocalOwner={isLocalOwner}
                      onLoadBody={loadBody}
                    />
                  ))}
                </div>
              )}
            </section>

            {/* ── Trace（§九十 联动入口）── */}
            {detail.trace && (
              <section>
                <h4 className={styles['observability-panel-subtitle']}>{t('settings.observability.inspector.traceSection')}</h4>
                <div className={styles['observability-summary-row']}>
                  <code title={detail.trace.traceId}>{shortId(detail.trace.traceId)}</code>
                  <span className={styles['observability-ledger-muted']}>
                    {originLabel(detail.trace.origin)} · {formatLocalFullDateTime(detail.trace.firstSeenAt)}
                    {' → '}{formatLocalFullDateTime(detail.trace.lastSeenAt)}
                  </span>
                  <button
                    type="button"
                    className={styles['observability-link-btn']}
                    onClick={() => onOpenTrace(detail.trace!.traceId)}
                  >
                    {t('settings.observability.inspector.openTrace')}
                  </button>
                </div>
              </section>
            )}
          </div>
        )}
      </div>
    </Overlay>
  );
}
