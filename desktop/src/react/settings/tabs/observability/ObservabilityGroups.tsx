/**
 * ObservabilityGroups.tsx — Aggregate 分组可视化（Phase 9 §三十四～三十九）。
 *
 * 统一消费 ModelObservabilityGroupBucket[]（§三十四），按主维度族选择渲染器：
 *   - date（单维）           → 时间折线（纯 SVG 路径 + non-scaling stroke，无图表库，§三十九）
 *   - model/category/provider/status（单维）→ 排名横条
 *   - session/task/agent（单维）          → 排名列表（高基数，不画图）
 *   - 多维 groupBy（2~3 维）              → 排名列表（标签 = 各维值组合）
 *
 * 排序在客户端（§三十八）：date 升序，其余按 callCount 降序。
 * 动态宽度一律经 ref el.style.setProperty 设 CSS var（desktop 风格立法：
 * tabs/ 下零新增内联样式字面量）。
 */
import React, { useCallback, useState } from 'react';
import type {
  ModelObservabilityGroupBucket,
  ModelObservabilityGroupByDimension,
  ModelObservabilityGroupValues,
} from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import styles from '../../Settings.module.css';
import { formatAxisDate, formatCompactNumber, formatNumber, shortId } from './model-observability-format';
import {
  attributionKindLabel,
  groupByDimensionLabel,
  inputShapeLabel,
  operationLabel,
  provenancePrecisionLabel,
  subsystemLabel,
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
  if (dimension === 'category') return subsystemLabel(raw);
  if (dimension === 'operation') return operationLabel(raw);
  if (dimension === 'attributionKind') return attributionKindLabel(raw);
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

/* ── 渲染器：date → 时间折线（SVG）────────────────────────────────────── */

const CHART_W = 100;
const CHART_H = 36;
const CHART_PAD_X = 6;
const CHART_TOP = 3;
const CHART_BASELINE = CHART_H - 2;
const AXIS_MAX_LABELS = 7;

type DatePoint = { bucket: ModelObservabilityGroupBucket; x: number; y: number };

/** Catmull-Rom → 三次贝塞尔：过点平滑曲线；控制点夹回绘图区，防止面积越过基线。 */
function smoothLinePath(points: DatePoint[]): string {
  if (points.length === 0) return '';
  const clampX = (value: number) => Math.min(CHART_W, Math.max(0, value));
  const clampY = (value: number) => Math.min(CHART_BASELINE, Math.max(CHART_TOP, value));
  const fx = (value: number) => value.toFixed(2);
  let path = `M ${fx(points[0].x)} ${fx(points[0].y)}`;
  for (let i = 0; i < points.length - 1; i++) {
    const p0 = points[i - 1] ?? points[i];
    const p1 = points[i];
    const p2 = points[i + 1];
    const p3 = points[i + 2] ?? points[i + 1];
    path += ` C ${fx(clampX(p1.x + (p2.x - p0.x) / 6))} ${fx(clampY(p1.y + (p2.y - p0.y) / 6))}`
      + ` ${fx(clampX(p2.x - (p3.x - p1.x) / 6))} ${fx(clampY(p2.y - (p3.y - p1.y) / 6))}`
      + ` ${fx(p2.x)} ${fx(p2.y)}`;
  }
  return path;
}

/** X 轴标签：绝对定位在数据点正下方居中（百分比经 CSS var 注入，遵守 tabs/ 零内联样式立法）。 */
function DateTick({ x, children }: { x: number; children: React.ReactNode }) {
  const ref = useCallback((node: HTMLSpanElement | null) => {
    node?.style.setProperty('--observability-date-tick-x', `${(x / CHART_W) * 100}%`);
  }, [x]);
  return (
    <span ref={ref} className={styles['observability-date-tick']}>
      {children}
    </span>
  );
}

function DateLine({ buckets }: { buckets: ModelObservabilityGroupBucket[] }) {
  const [hoverKey, setHoverKey] = useState<string | null>(null);
  const sorted = [...buckets].sort((a, b) => String(a.values.date ?? '').localeCompare(String(b.values.date ?? '')));
  const max = Math.max(1, ...sorted.map((bucket) => bucket.metrics.callCount));
  const peak = sorted.reduce((best, bucket) => (bucket.metrics.callCount > (best?.metrics.callCount ?? -1) ? bucket : best), sorted[0] ?? null);
  // 首尾点各内收 6%，让居中的 X 轴标签不出界；折线仍尽量满宽；单点居中
  const span = sorted.length > 1 ? (CHART_W - CHART_PAD_X * 2) / (sorted.length - 1) : 0;
  // X 按真实日期比例定位：buckets 只含有调用的日子，若按序号等距排布，
  // 「隔 3 天」和「隔 1 天」的相邻点会画得一样近——点与日期对不上。
  // 解析失败的桶（date 值异常）退回序号位置，绝不猜时间。
  const dayTimeOf = (bucket: ModelObservabilityGroupBucket): number | null => {
    const time = new Date(`${String(bucket.values.date ?? bucket.key)}T00:00:00Z`).getTime();
    return Number.isFinite(time) ? time : null;
  };
  const dayTimes = sorted.map(dayTimeOf);
  const knownTimes = dayTimes.filter((value): value is number => value !== null);
  const minTime = knownTimes.length > 0 ? Math.min(...knownTimes) : 0;
  const timeSpan = knownTimes.length > 0 ? Math.max(...knownTimes) - minTime : 0;
  const points = sorted.map((bucket, index) => {
    const time = dayTimes[index];
    const x = time !== null && timeSpan > 0
      ? CHART_PAD_X + ((time - minTime) / timeSpan) * (CHART_W - CHART_PAD_X * 2)
      : (sorted.length > 1 ? CHART_PAD_X + index * span : CHART_W / 2);
    return {
      bucket,
      x,
      y: CHART_BASELINE - (bucket.metrics.callCount / max) * (CHART_BASELINE - CHART_TOP),
    };
  });
  // hooks 须在早退之前调用（rules-of-hooks）；hovered/tooltipRef 对空 points 同样安全。
  const hovered = hoverKey === null ? null : points.find((point) => point.bucket.key === hoverKey) ?? null;
  const tooltipRef = useCallback((node: HTMLDivElement | null) => {
    if (!node || !hovered) return;
    node.style.setProperty('--observability-date-tooltip-x', `${(hovered.x / CHART_W) * 100}%`);
    node.style.setProperty('--observability-date-tooltip-y', `${(hovered.y / CHART_H) * 100}%`);
  }, [hovered?.x, hovered?.y]);
  if (points.length === 0) return null;
  // 悬停命中区以相邻点中点为界，首尾延到图表边缘
  const hitBounds = points.map((point, index) => {
    const left = index === 0 ? 0 : (points[index - 1].x + point.x) / 2;
    const right = index === points.length - 1 ? CHART_W : (points[index + 1].x + point.x) / 2;
    return { left, width: right - left };
  });
  const fx = (value: number) => value.toFixed(2);
  const linePath = smoothLinePath(points);
  const areaPath = `${linePath} L ${fx(points[points.length - 1].x)} ${CHART_BASELINE} L ${fx(points[0].x)} ${CHART_BASELINE} Z`;
  // 数据点用近零长度子路径 + round linecap 画圆点；non-scaling-stroke 保证不被 viewBox 拉伸变形
  const dotsPath = points.map((point) => `M ${fx(point.x)} ${fx(point.y)} l 0.001 0`).join(' ');
  // X 轴标签稀疏化：按点的真实水平位置贪心抽稀（保持最小间距），保证任何
  // 数据点附近都有可读的日期参照；首尾必显。按索引抽稀在真实时间比例下会把
  // 标签聚到一侧，密集区的点反而无参照（「中间的点对不上日期」）。
  const labeled = new Set<number>();
  if (points.length > 0) {
    labeled.add(0);
    const minGap = (CHART_W / AXIS_MAX_LABELS) * 0.8;
    let lastLabeledX = points[0].x;
    const lastPointX = points[points.length - 1].x;
    for (let i = 1; i < points.length - 1; i += 1) {
      if (points[i].x - lastLabeledX >= minGap && lastPointX - points[i].x >= minGap * 0.5) {
        labeled.add(i);
        lastLabeledX = points[i].x;
      }
    }
    if (points.length > 1) labeled.add(points.length - 1);
  }
  return (
    <div className={styles['observability-date-figure']}>
      <div className={styles['observability-date-plot']}>
        <svg
          className={styles['observability-date-chart']}
          viewBox={`0 0 ${CHART_W} ${CHART_H}`}
          preserveAspectRatio="none"
          role="img"
          aria-label={t('settings.observability.groups.dateChartAria')}
        >
          <defs>
            <linearGradient id="observability-date-area-gradient" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0" className={styles['observability-date-gradient-top']} />
              <stop offset="1" className={styles['observability-date-gradient-bottom']} />
            </linearGradient>
          </defs>
          <path className={styles['observability-date-area']} d={areaPath} />
          <path className={styles['observability-date-line']} d={linePath} />
          <path className={styles['observability-date-point']} d={dotsPath} />
          {hovered && (
            <line
              className={styles['observability-date-guide']}
              x1={fx(hovered.x)}
              y1={CHART_TOP - 1}
              x2={fx(hovered.x)}
              y2={CHART_BASELINE}
            />
          )}
          {points.map(({ bucket }, index) => (
            <rect
              key={bucket.key}
              className={styles['observability-date-hit']}
              x={fx(hitBounds[index].left)}
              y={0}
              width={fx(hitBounds[index].width)}
              height={CHART_H}
              onMouseEnter={() => setHoverKey(bucket.key)}
              onMouseLeave={() => setHoverKey((current) => (current === bucket.key ? null : current))}
            />
          ))}
        </svg>
        {hovered && (
          <div
            ref={tooltipRef}
            className={styles['observability-date-tooltip']}
            data-align={hovered.x < CHART_W * 0.15 ? 'start' : hovered.x > CHART_W * 0.85 ? 'end' : 'center'}
          >
            {t('settings.observability.groups.datePointTooltip', {
              date: formatAxisDate(String(hovered.bucket.values.date ?? hovered.bucket.key)),
              count: formatNumber(hovered.bucket.metrics.callCount),
            })}
          </div>
        )}
      </div>
      <div className={styles['observability-date-axis']} aria-hidden="true">
        {points.map(({ bucket }, index) => (
          <DateTick key={bucket.key} x={points[index].x}>
            {labeled.has(index) ? formatAxisDate(String(bucket.values.date ?? bucket.key)) : ''}
          </DateTick>
        ))}
      </div>
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
        <DateLine buckets={buckets} />
      ) : single && ['model', 'category', 'provider', 'status'].includes(primary) ? (
        <RankedRows buckets={buckets} groupBy={groupBy} withBars onBucketFilter={onBucketFilter} />
      ) : (
        <RankedRows buckets={buckets} groupBy={groupBy} withBars={false} onBucketFilter={onBucketFilter} />
      )}
    </div>
  );
}
