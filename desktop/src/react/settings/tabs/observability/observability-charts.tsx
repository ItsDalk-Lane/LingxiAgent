/**
 * observability-charts.tsx — Token 用量页四张常驻图表的共享基建。
 *
 * 设计契约（2026-09-17 用户定稿）：
 *   - 四张图各自按固定维度取数（热力=日期 12 个月、折线=模型×日期、
 *     柱状=供应商×日期、饼图=类别×日期，后三者为近 30 天），**不随首行
 *     筛选联动**——图表自身的维度已限定数据范围；首行筛选只作用于指标卡。
 *   - 折线/柱状/饼图右上角可切「调用次数 / Token 用量」，可单选可同显。
 *   - 悬停明细的「总量」口径 = 当日（热力图的每周/累计档除外）。
 *
 * 纪律：tabs/ 下零新增 `style={{` 字面量（settings-primitives-contract
 * ratchet）——动态定位一律经 el.style.setProperty 写 CSS var，系列色经
 * SVG fill 属性而非 style。
 */
import React, { useEffect, useRef, useState } from 'react';
import type {
  ModelObservabilityGroupBucket,
  ModelObservabilityGroupByDimension,
} from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import styles from '../../Settings.module.css';
import {
  isObservabilityAbortError,
  ModelObservabilityRequestError,
  queryObservabilityAggregate,
} from './model-observability-actions';
import { dateBucketForGroupBy } from './model-observability-filter';
import { formatCompactNumber, formatNumber } from './model-observability-format';

/* ── 固定取数：不受首行筛选影响 ──────────────────────────────────────── */

/**
 * 按固定维度 + 滚动天数窗取聚合桶。days/groupBy 须为模块级常量
 * （引用稳定，effect 才不会每次渲染重查）；refreshToken 驱动手动刷新。
 */
export function useObservabilityChartBuckets({ days, groupBy, refreshToken }: {
  days: number;
  groupBy: readonly ModelObservabilityGroupByDimension[];
  refreshToken: number;
}): { buckets: ModelObservabilityGroupBucket[] | null; loading: boolean; error: string | null } {
  const [buckets, setBuckets] = useState<ModelObservabilityGroupBucket[] | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const generationRef = useRef(0);

  useEffect(() => {
    const generation = ++generationRef.current;
    const controller = new AbortController();
    setLoading(true);
    setError(null);
    const since = new Date(Date.now() - days * 86_400_000).toISOString();
    queryObservabilityAggregate(
      {
        filter: { since },
        groupBy: [...groupBy],
        dateBucket: groupBy.includes('date') ? dateBucketForGroupBy(['date']) : undefined,
      },
      { signal: controller.signal },
    ).then((result) => {
      if (generationRef.current !== generation) return;
      setBuckets(result.groups);
      setLoading(false);
    }).catch((err: unknown) => {
      if (generationRef.current !== generation || isObservabilityAbortError(err)) return;
      if (err instanceof ModelObservabilityRequestError && err.kind === 'not_initialized') {
        setBuckets([]);
        setLoading(false);
      } else {
        setError(err instanceof Error ? err.message : String(err));
        setLoading(false);
      }
    });
    return () => controller.abort();
  }, [days, groupBy, refreshToken]);

  return { buckets, loading, error };
}

/* ── 系列与指标 ─────────────────────────────────────────────────────── */

export const CHART_SERIES_COLORS = ['#0e7490', '#b45309', '#6d28d9', '#be185d', '#15803d', '#9a3412'] as const;

export function seriesColor(index: number): string {
  return CHART_SERIES_COLORS[index % CHART_SERIES_COLORS.length];
}

export type ChartMetricKey = 'calls' | 'tokens';
export type ChartMetricSelection = Record<ChartMetricKey, boolean>;
export const DEFAULT_CHART_METRIC: ChartMetricSelection = { calls: true, tokens: false };

export type ChartMemberValue = { calls: number; tokens: number };

/** 按勾选的指标拼展示值：只选一个 → 单项；两个都选 → "N 次 · X Token"。 */
export function chartMetricValue(value: ChartMemberValue, selection: ChartMetricSelection): string {
  const parts: string[] = [];
  if (selection.calls) {
    parts.push(t('settings.observability.charts.callsUnit', { count: formatNumber(value.calls) }));
  }
  if (selection.tokens) {
    parts.push(t('settings.observability.charts.tokensUnit', { count: formatCompactNumber(value.tokens) }));
  }
  return parts.join(' · ') || '—';
}

/** 桶指标 → 图表数值（totalTokens 无事实按 0 展示；callCount 恒在）。 */
export function bucketValue(bucket: ModelObservabilityGroupBucket): ChartMemberValue {
  return { calls: bucket.metrics.callCount, tokens: bucket.metrics.totalTokens ?? 0 };
}

/* ── 悬停提示 ───────────────────────────────────────────────────────── */

export type ChartTipRow = { color: string; name: string; value: string };
export type ChartTip = { title: string; rows: ChartTipRow[]; total?: { label: string; value: string } };

/** 提示层跟随鼠标：位置经 CSS var 注入（零 style 字面量）。 */
export function placeChartTip(
  tip: HTMLDivElement | null,
  host: HTMLElement | null,
  evt: { clientX: number; clientY: number },
) {
  if (!tip || !host) return;
  const rect = host.getBoundingClientRect();
  const x = Math.max(4, Math.min(evt.clientX - rect.left + 16, rect.width - tip.offsetWidth - 8));
  const y = Math.max(4, Math.min(evt.clientY - rect.top + 12, rect.height - tip.offsetHeight - 8));
  tip.style.setProperty('--chart-tip-x', `${Math.round(x)}px`);
  tip.style.setProperty('--chart-tip-y', `${Math.round(y)}px`);
}

/** 常驻提示层（无内容时隐藏；ref 恒在，避免首帧定位闪烁）。 */
export function ChartTipLayer({ tip, tipRef }: {
  tip: ChartTip | null;
  tipRef: React.RefObject<HTMLDivElement | null>;
}) {
  return (
    <div
      ref={tipRef}
      className={styles['observability-chart-tip']}
      data-open={tip ? 'true' : undefined}
      role="status"
    >
      {tip && (
        <>
          <div className={styles['observability-chart-tip-title']}>{tip.title}</div>
          {tip.rows.map((row) => (
            <div key={`${row.name}:${row.value}`} className={styles['observability-chart-tip-row']}>
              <ChartDot color={row.color} />
              <span>{row.name}</span>
              <span className={styles['observability-chart-tip-val']}>{row.value}</span>
            </div>
          ))}
          {tip.total && (
            <div className={styles['observability-chart-tip-total']}>
              <span>{tip.total.label}</span>
              <span>{tip.total.value}</span>
            </div>
          )}
        </>
      )}
    </div>
  );
}

/* ── 卡片骨架件 ─────────────────────────────────────────────────────── */

export function ChartDot({ color, size = 8 }: { color: string; size?: number }) {
  const half = size / 2;
  return (
    <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} aria-hidden="true" className={styles['observability-chart-dot']}>
      <circle cx={half} cy={half} r={half} fill={color} />
    </svg>
  );
}

export function ChartHead({ title, badge, right }: {
  title: string;
  badge: string;
  right?: React.ReactNode;
}) {
  return (
    <div className={styles['observability-chart-head']}>
      <span className={styles['observability-chart-title']}>{title}</span>
      <span className={styles['observability-chart-badge']}>{badge}</span>
      {right && <span className={styles['observability-chart-head-right']}>{right}</span>}
    </div>
  );
}

/** 调用次数 / Token 用量 切换：可单选可同显；最后一路不可关闭。 */
export function MetricToggle({ value, onChange }: {
  value: ChartMetricSelection;
  onChange: (next: ChartMetricSelection) => void;
}) {
  const keys: ChartMetricKey[] = ['calls', 'tokens'];
  return (
    <span className={styles['observability-metric-toggle']} role="group">
      {keys.map((key) => {
        const other = key === 'calls' ? 'tokens' : 'calls';
        return (
          <button
            key={key}
            type="button"
            data-on={value[key] ? 'true' : undefined}
            disabled={value[key] && !value[other]}
            aria-pressed={value[key]}
            onClick={() => onChange({ ...value, [key]: !value[key] })}
          >
            {t(`settings.observability.charts.metric.${key}`)}
          </button>
        );
      })}
    </span>
  );
}

export function ChartLegend({ items }: { items: { color: string; label: string }[] }) {
  return (
    <div className={styles['observability-chart-legend']}>
      {items.map((item) => (
        <span key={item.label} className={styles['observability-chart-legend-item']}>
          <ChartDot color={item.color} />
          {item.label}
        </span>
      ))}
    </div>
  );
}

/** 图表空态 / 失败态（区别于 loading；空数据不是错误）。 */
export function ChartNotice({ kind }: { kind: 'empty' | 'error' | 'loading' }) {
  return (
    <div className={styles['observability-chart-notice']} data-kind={kind} aria-busy={kind === 'loading' || undefined}>
      {t(`settings.observability.charts.${kind}`)}
    </div>
  );
}

/* ── 日期工具（轴标签 / 提示标题）──────────────────────────────────── */

/** 本地「9月12日 / Sep 12」— 复用 DateLine 的轴格式。 */
export { formatAxisDate } from './model-observability-format';

export function chartLocale(): string {
  const locale = (window as unknown as { i18n?: { locale?: string } }).i18n?.locale;
  return locale || 'zh-CN';
}

/** 完整本地日期（热力图提示标题）：2026年9月12日。 */
export function formatFullLocalDate(date: Date): string {
  return new Intl.DateTimeFormat(chartLocale(), { year: 'numeric', month: 'long', day: 'numeric' }).format(date);
}

/** 本地 YYYY-MM-DD（日历格 key，与 date 分组值同构）。 */
export function localDateKey(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/** 周一为 0 的星期序。 */
export function mondayIndex(date: Date): number {
  return (date.getDay() + 6) % 7;
}

/** 该日期所在周的周一。 */
export function weekStart(date: Date): Date {
  const m = new Date(date);
  m.setHours(0, 0, 0, 0);
  m.setDate(m.getDate() - mondayIndex(m));
  return m;
}
