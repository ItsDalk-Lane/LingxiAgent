/**
 * ObservabilityMetrics.tsx — Metrics Dashboard（Phase 9 §二十八～三十三）。
 *
 * 数据只来自 POST /query/aggregate 的 overall（§二十八：绝不把 50 行
 * call page 加总当指标）。纯展示组件；null 与 0 严格区分（§三十三：
 * costTotal=null → "—"，不是 $0.00）。
 *
 *   - 错误率 = (error + aborted) / callCount；incomplete 不自动算错误（§三十一）。
 *   - cache 命中率只在 cacheObservedCount > 0 时显示（§三十二）。
 *   - usage 覆盖（usageCoveredCalls / usageMissingCalls）常显（§三十）。
 */
import React from 'react';
import type { ModelObservabilityGroupMetrics } from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import styles from '../../Settings.module.css';
import {
  formatCompactNumber,
  formatCost,
  formatDurationMs,
  formatNumber,
  formatPercent,
} from './model-observability-format';

function MetricCard({ label, value, title, hint }: {
  label: string;
  value: string;
  title?: string;
  hint?: string;
}) {
  return (
    <div className={styles['observability-metric-card']}>
      <div className={styles['observability-metric-label']}>{label}</div>
      <div className={styles['observability-metric-value']} title={title}>{value}</div>
      {hint && <div className={styles['observability-metric-hint']}>{hint}</div>}
    </div>
  );
}

export function ObservabilityMetrics({ overall, loading }: {
  overall: ModelObservabilityGroupMetrics | null;
  loading: boolean;
}) {
  if (!overall) {
    return (
      <div className={styles['observability-metrics']} data-loading={loading || undefined}>
        {Array.from({ length: 8 }, (_, index) => (
          <MetricCard
            key={index}
            label={t(`settings.observability.metrics.${['calls', 'totalTokens', 'inputTokens', 'outputTokens', 'cacheRead', 'errors', 'avgDuration', 'cost'][index]}`)}
            value="—"
          />
        ))}
      </div>
    );
  }

  const errorRate = overall.callCount > 0
    ? (overall.errorCount + overall.abortedCount) / overall.callCount
    : null;
  const cacheHitRate = overall.cacheObservedCount > 0 && overall.cacheHitCount !== null
    ? overall.cacheHitCount / overall.cacheObservedCount
    : null;
  const incompleteHint = overall.incompleteCount > 0
    ? t('settings.observability.metrics.incompleteHint', { count: formatNumber(overall.incompleteCount) })
    : undefined;

  return (
    <div>
      <div className={styles['observability-metrics']} data-loading={loading || undefined}>
        <MetricCard
          label={t('settings.observability.metrics.calls')}
          value={formatCompactNumber(overall.callCount)}
          title={formatNumber(overall.callCount)}
          hint={overall.traceCount > 0
            ? t('settings.observability.metrics.tracesHint', { count: formatCompactNumber(overall.traceCount) })
            : undefined}
        />
        <MetricCard
          label={t('settings.observability.metrics.totalTokens')}
          value={formatCompactNumber(overall.totalTokens)}
          title={formatNumber(overall.totalTokens)}
        />
        <MetricCard
          label={t('settings.observability.metrics.inputTokens')}
          value={formatCompactNumber(overall.inputTokens)}
          title={formatNumber(overall.inputTokens)}
        />
        <MetricCard
          label={t('settings.observability.metrics.outputTokens')}
          value={formatCompactNumber(overall.outputTokens)}
          title={formatNumber(overall.outputTokens)}
        />
        <MetricCard
          label={t('settings.observability.metrics.cacheRead')}
          value={formatCompactNumber(overall.cacheReadTokens)}
          title={formatNumber(overall.cacheReadTokens)}
          hint={cacheHitRate !== null
            ? t('settings.observability.metrics.cacheHitRate', { rate: formatPercent(cacheHitRate) })
            : undefined}
        />
        <MetricCard
          label={t('settings.observability.metrics.errors')}
          value={formatCompactNumber(overall.errorCount + overall.abortedCount)}
          title={`${formatNumber(overall.errorCount)} + ${formatNumber(overall.abortedCount)}`}
          hint={[
            errorRate !== null ? formatPercent(errorRate) : null,
            incompleteHint,
          ].filter(Boolean).join(' · ') || undefined}
        />
        <MetricCard
          label={t('settings.observability.metrics.avgDuration')}
          value={formatDurationMs(overall.durationAverageMs)}
          title={overall.durationObservedCount > 0
            ? t('settings.observability.metrics.durationObserved', { count: formatNumber(overall.durationObservedCount) })
            : undefined}
        />
        {/* §三十三：costTotal=null → "—"（未知），绝不显示 $0.00 伪装已知 */}
        <MetricCard
          label={t('settings.observability.metrics.cost')}
          value={formatCost(overall.costTotal)}
          title={overall.costTotal !== null ? `$${overall.costTotal}` : undefined}
        />
      </div>
      <div
        className={styles['observability-metrics-coverage']}
        data-usage-availability={overall.usageAggregateAvailability}
        role={overall.usageAggregateAvailability === 'complete' ? undefined : 'status'}
      >
        {t(`settings.observability.metrics.${
          overall.usageAggregateAvailability === 'complete'
            ? 'usageCoverageComplete'
            : overall.usageAggregateAvailability === 'partial'
              ? 'usageCoveragePartial'
              : overall.usageAggregateAvailability === 'corrupt'
                ? 'usageCoverageCorrupt'
              : overall.usageAggregateAvailability === 'projection_unavailable'
                ? 'usageCoverageProjectionUnavailable'
                : 'usageCoverageUnknown'
        }`, {
          covered: formatNumber(overall.usageCoveredCalls),
          corrupt: formatNumber(overall.usageCorruptCalls),
          notCorrelated: formatNumber(overall.usageNotCorrelatedCalls),
          unknown: formatNumber(overall.usageUnknownCalls),
          total: formatNumber(overall.callCount),
          missing: formatNumber(overall.usageMissingCalls),
        })}
      </div>
    </div>
  );
}
