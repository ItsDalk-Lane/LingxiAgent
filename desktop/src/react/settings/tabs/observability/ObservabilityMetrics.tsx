/**
 * ObservabilityMetrics.tsx — Metrics Dashboard（Phase 9 §二十八～三十三）。
 *
 * 数据只来自 POST /query/aggregate 的 overall（§二十八：绝不把 50 行
 * call page 加总当指标）。纯展示组件；null 与 0 严格区分（§三十三）。
 *
 *   - 2026-09-17 用户调整：成本卡移除（从未投入使用）；新增两张洞察卡
 *     （活跃天数·含最长连续天数、用量最高的模型·含调用与 Token），
 *     数据由 UsagePanel 随首行筛选查询（insights prop），与 overall 同生命周期。
 *   - 错误率 = (error + aborted) / callCount；incomplete 不自动算错误（§三十一）。
 *   - cache 命中率只在 cacheObservedCount > 0 时显示（§三十二）。
 *   - usage 覆盖（usageCoveredCalls / usageMissingCalls）常显（§三十）。
 */
import React from 'react';
import type { ModelObservabilityGroupMetrics } from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import styles from '../../Settings.module.css';
import {
  formatAxisDate,
  formatCompactNumber,
  formatDurationMs,
  formatNumber,
  formatPercent,
} from './model-observability-format';

export type ObservabilityMetricInsights = {
  /** 选定范围内有调用的天数（null = 尚无事实）。 */
  activeDays: number | null;
  /** 最长连续活跃天数。 */
  longestStreak: number | null;
  /** 用量最高的模型（按调用次数，平手比 Token）。 */
  topModel: { name: string; calls: number; tokens: number } | null;
  /** 高峰日（按 Token 总量最高，平手比调用）。 */
  peakDay: { date: string; calls: number; tokens: number } | null;
};

const INSIGHTS_PLACEHOLDER: ObservabilityMetricInsights = {
  activeDays: null,
  longestStreak: null,
  topModel: null,
  peakDay: null,
};

const METRIC_LABEL_KEYS = [
  'totalTokens', 'calls', 'inputTokens', 'outputTokens',
  'cacheRead', 'topModel', 'peakDay', 'activeDays', 'avgDuration', 'errors',
];

function MetricCard({ label, value, title, hints }: {
  label: string;
  value: string;
  title?: string;
  /** 副行（上下排列，每项一行）。 */
  hints?: string[];
}) {
  return (
    <div className={styles['observability-metric-card']}>
      <div className={styles['observability-metric-label']}>{label}</div>
      <div className={styles['observability-metric-value']} title={title}>{value}</div>
      {hints?.map((line) => (
        <div key={line} className={styles['observability-metric-hint']}>{line}</div>
      ))}
    </div>
  );
}

export function ObservabilityMetrics({ overall, loading, insights }: {
  overall: ModelObservabilityGroupMetrics | null;
  loading: boolean;
  insights?: ObservabilityMetricInsights | null;
}) {
  const view = insights ?? INSIGHTS_PLACEHOLDER;
  if (!overall) {
    return (
      <div className={styles['observability-metrics']} data-loading={loading || undefined}>
        {Array.from({ length: METRIC_LABEL_KEYS.length }, (_, index) => (
          <MetricCard
            key={index}
            label={t(`settings.observability.metrics.${METRIC_LABEL_KEYS[index]}`)}
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
          label={t('settings.observability.metrics.totalTokens')}
          value={formatCompactNumber(overall.totalTokens)}
          title={formatNumber(overall.totalTokens)}
        />
        <MetricCard
          label={t('settings.observability.metrics.calls')}
          value={formatCompactNumber(overall.callCount)}
          title={formatNumber(overall.callCount)}
          hints={overall.traceCount > 0
            ? [t('settings.observability.metrics.tracesHint', { count: formatCompactNumber(overall.traceCount) })]
            : undefined}
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
          hints={cacheHitRate !== null
            ? [t('settings.observability.metrics.cacheHitRate', { rate: formatPercent(cacheHitRate) })]
            : undefined}
        />
        <MetricCard
          label={t('settings.observability.metrics.topModel')}
          value={view.topModel ? view.topModel.name : '—'}
          title={view.topModel?.name}
          hints={view.topModel
            ? [
              t('settings.observability.charts.callsUnit', { count: formatNumber(view.topModel.calls) }),
              t('settings.observability.charts.tokensUnit', { count: formatCompactNumber(view.topModel.tokens) }),
            ]
            : undefined}
        />
        <MetricCard
          label={t('settings.observability.metrics.peakDay')}
          value={view.peakDay ? formatAxisDate(view.peakDay.date) : '—'}
          title={view.peakDay?.date}
          hints={view.peakDay
            ? [
              t('settings.observability.charts.callsUnit', { count: formatNumber(view.peakDay.calls) }),
              t('settings.observability.charts.tokensUnit', { count: formatCompactNumber(view.peakDay.tokens) }),
            ]
            : undefined}
        />
        <MetricCard
          label={t('settings.observability.metrics.activeDays')}
          value={view.activeDays !== null ? formatNumber(view.activeDays) : '—'}
          hints={view.longestStreak !== null && view.longestStreak > 0
            ? [t('settings.observability.metrics.longestStreak', { count: formatNumber(view.longestStreak) })]
            : undefined}
        />
        <MetricCard
          label={t('settings.observability.metrics.avgDuration')}
          value={formatDurationMs(overall.durationAverageMs)}
          title={overall.durationObservedCount > 0
            ? t('settings.observability.metrics.durationObserved', { count: formatNumber(overall.durationObservedCount) })
            : undefined}
        />
        <MetricCard
          label={t('settings.observability.metrics.errors')}
          value={formatCompactNumber(overall.errorCount + overall.abortedCount)}
          title={`${formatNumber(overall.errorCount)} + ${formatNumber(overall.abortedCount)}`}
          hints={[
            errorRate !== null ? formatPercent(errorRate) : null,
            incompleteHint,
          ].filter(Boolean).map((line) => line as string)}
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
