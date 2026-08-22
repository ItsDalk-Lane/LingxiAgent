/**
 * ObservabilityGroups.tsx — Aggregate 分组可视化（Phase 9 §三十四～三十九）。
 *
 * 统一消费 ModelObservabilityGroupBucket[]（§三十四），按主维度族选择渲染器：
 *   - date（单维）           → 时间柱（纯 SVG 属性，无图表库，§三十九）
 *   - model/category/provider/status（单维）→ 排名横条
 *   - session/task/agent（单维）          → 排名列表（高基数，不画图）
 *   - 多维 groupBy（2~3 维）              → 排名列表（标签 = 各维值组合）
 *
 * 排序在客户端（§三十八）：date 升序，其余按 callCount 降序。
 * 动态宽度一律经 ref el.style.setProperty 设 CSS var（desktop 风格立法：
 * tabs/ 下零新增内联样式字面量）。
 */
import React, { useCallback } from 'react';
import type {
  ModelObservabilityGroupBucket,
  ModelObservabilityGroupByDimension,
  ModelObservabilityGroupValues,
} from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import styles from '../../Settings.module.css';
import { formatCompactNumber, formatNumber, shortId } from './model-observability-format';
import {
  groupByDimensionLabel,
  inputShapeLabel,
  provenancePrecisionLabel,
  terminalStatusLabel,
} from './model-observability-labels';

/* ── bucket 标签 ──────────────────────────────────────────────────────── */

function dimensionValueLabel(dimension: ModelObservabilityGroupByDimension, values: ModelObservabilityGroupValues): string {
  const raw = (values as Record<string, string | null | undefined>)[dimension];
  if (dimension === 'model') {
    const provider = values.provider ?? null;
    const modelId = values.modelId ?? null;
    if (modelId === null) return t('settings.observability.groups.unknownValue');
    return provider ? `${provider}/${modelId}` : modelId;
  }
  if (raw === null || raw === undefined || raw === '') return t('settings.observability.groups.unknownValue');
  if (dimension === 'status') return terminalStatusLabel(raw);
  if (dimension === 'inputShape') return inputShapeLabel(raw);
  if (dimension === 'provenancePrecision') return provenancePrecisionLabel(raw);
  if (dimension === 'session' || dimension === 'conversation' || dimension === 'task' || dimension === 'agent') {
    return shortId(raw);
  }
  return raw;
}

function bucketLabel(groupBy: readonly ModelObservabilityGroupByDimension[], bucket: ModelObservabilityGroupBucket): string {
  return groupBy.map((dimension) => dimensionValueLabel(dimension, bucket.values)).join(' · ');
}

function bucketTitle(groupBy: readonly ModelObservabilityGroupByDimension[], bucket: ModelObservabilityGroupBucket): string {
  return groupBy.map((dimension) => {
    const raw = (bucket.values as Record<string, string | null | undefined>)[dimension];
    return `${groupByDimensionLabel(dimension)}: ${raw ?? '—'}`;
  }).join('\n');
}

/* ── 动态宽度条（CSS var，不经 inline style 字面量）────────────────────── */

function BarTrack({ share }: { share: number }) {
  const ref = useCallback((node: HTMLDivElement | null) => {
    node?.style.setProperty('--observability-bar-share', String(Math.max(0, Math.min(1, share))));
  }, [share]);
  return (
    <div className={styles['observability-bar-track']} ref={ref}>
      <div className={styles['observability-bar-fill']} />
    </div>
  );
}

/* ── 渲染器：date → 时间柱（SVG）──────────────────────────────────────── */

const CHART_W = 100;
const CHART_H = 36;

function DateBars({ buckets }: { buckets: ModelObservabilityGroupBucket[] }) {
  const sorted = [...buckets].sort((a, b) => String(a.values.date ?? '').localeCompare(String(b.values.date ?? '')));
  const max = Math.max(1, ...sorted.map((bucket) => bucket.metrics.callCount));
  const peak = sorted.reduce((best, bucket) => (bucket.metrics.callCount > (best?.metrics.callCount ?? -1) ? bucket : best), sorted[0] ?? null);
  const barWidth = sorted.length > 0 ? CHART_W / sorted.length : CHART_W;
  return (
    <div>
      <svg
        className={styles['observability-date-chart']}
        viewBox={`0 0 ${CHART_W} ${CHART_H}`}
        preserveAspectRatio="none"
        role="img"
        aria-label={t('settings.observability.groups.dateChartAria')}
      >
        {sorted.map((bucket, index) => {
          const height = Math.max(1, (bucket.metrics.callCount / max) * (CHART_H - 2));
          return (
            <rect
              key={bucket.key}
              x={index * barWidth + barWidth * 0.12}
              y={CHART_H - height}
              width={Math.max(0.5, barWidth * 0.76)}
              height={height}
              rx={0.6}
              className={styles['observability-date-bar']}
            >
              <title>{`${bucket.values.date ?? bucket.key}: ${formatNumber(bucket.metrics.callCount)}`}</title>
            </rect>
          );
        })}
      </svg>
      {/* §一百六十二：图表必须配文字摘要（不只靠图形/颜色） */}
      <div className={styles['observability-groups-summary']}>
        {peak && t('settings.observability.groups.dateSummary', {
          total: formatCompactNumber(sorted.reduce((sum, bucket) => sum + bucket.metrics.callCount, 0)),
          peakDate: String(peak.values.date ?? peak.key),
          peakCount: formatCompactNumber(peak.metrics.callCount),
        })}
      </div>
    </div>
  );
}

/* ── 渲染器：排名横条 / 排名列表 ───────────────────────────────────────── */

function RankedRows({ buckets, groupBy, withBars, onBucketFilter }: {
  buckets: ModelObservabilityGroupBucket[];
  groupBy: readonly ModelObservabilityGroupByDimension[];
  withBars: boolean;
  onBucketFilter?: (dimension: ModelObservabilityGroupByDimension, value: string) => void;
}) {
  const sorted = [...buckets].sort((a, b) => b.metrics.callCount - a.metrics.callCount);
  const max = Math.max(1, ...sorted.map((bucket) => bucket.metrics.callCount));
  const filterable = groupBy.length === 1 && onBucketFilter
    && ['provider', 'model', 'category', 'status'].includes(groupBy[0]);
  return (
    <div className={styles['observability-group-rows']}>
      {sorted.map((bucket) => {
        const primary = groupBy[0];
        const rawValue = primary === 'model'
          ? (bucket.values.modelId ?? null)
          : ((bucket.values as Record<string, string | null | undefined>)[primary] ?? null);
        const row = (
          <>
            <span className={styles['observability-group-label']} title={bucketTitle(groupBy, bucket)}>
              {bucketLabel(groupBy, bucket)}
            </span>
            {withBars && <BarTrack share={bucket.metrics.callCount / max} />}
            <span className={styles['observability-group-count']} title={formatNumber(bucket.metrics.callCount)}>
              {formatCompactNumber(bucket.metrics.callCount)}
            </span>
            <span className={styles['observability-group-tokens']}>
              {formatCompactNumber(bucket.metrics.totalTokens)}
            </span>
            {bucket.metrics.errorCount + bucket.metrics.abortedCount > 0 && (
              <span className={styles['observability-group-errors']}>
                {formatCompactNumber(bucket.metrics.errorCount + bucket.metrics.abortedCount)}
              </span>
            )}
          </>
        );
        if (filterable && rawValue) {
          return (
            <button
              key={bucket.key}
              type="button"
              className={styles['observability-group-row']}
              title={t('settings.observability.groups.filterByValue', { value: bucketLabel(groupBy, bucket) })}
              onClick={() => onBucketFilter(primary, rawValue)}
            >
              {row}
            </button>
          );
        }
        return (
          <div key={bucket.key} className={styles['observability-group-row']}>
            {row}
          </div>
        );
      })}
    </div>
  );
}

/* ── 主组件 ───────────────────────────────────────────────────────────── */

export function ObservabilityGroups({ buckets, groupBy, loading, onBucketFilter }: {
  buckets: ModelObservabilityGroupBucket[] | null;
  groupBy: readonly ModelObservabilityGroupByDimension[];
  loading: boolean;
  onBucketFilter?: (dimension: ModelObservabilityGroupByDimension, value: string) => void;
}) {
  if (groupBy.length === 0) return null;
  if (!loading && buckets && buckets.length === 0) {
    return <div className={styles['observability-groups-empty']}>{t('settings.observability.groups.empty')}</div>;
  }
  if (!buckets || buckets.length === 0) {
    return <div className={styles['observability-groups-empty']} aria-busy={loading}>{t('settings.observability.groups.loading')}</div>;
  }
  const primary = groupBy[0];
  const single = groupBy.length === 1;
  return (
    <div className={styles['observability-groups']} data-loading={loading || undefined}>
      {single && primary === 'date' ? (
        <DateBars buckets={buckets} />
      ) : single && ['model', 'category', 'provider', 'status'].includes(primary) ? (
        <RankedRows buckets={buckets} groupBy={groupBy} withBars onBucketFilter={onBucketFilter} />
      ) : (
        <RankedRows buckets={buckets} groupBy={groupBy} withBars={false} onBucketFilter={onBucketFilter} />
      )}
    </div>
  );
}
