/**
 * ObservabilityCategoryPieChart.tsx — 「类别构成」（近 30 天）。
 *
 * 「按日 / 累计」切换总量口径：按日 = 顶部选一天看当天的类别构成；
 * 累计 = 窗口内所有日期合计的类别构成（日期下拉隐藏）。
 * 「调用次数 / Token 用量」可单选可同显（同显 = 并排双环）。
 * 环心标当前口径合计；右侧明细列 + 当前口径总计。
 * 悬停扇区显示该类别的量（当前所选指标）与当前口径总计。
 */
import React, { useMemo, useRef, useState } from 'react';
import type { ModelObservabilityGroupByDimension } from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import styles from '../../Settings.module.css';
import {
  bucketValue,
  ChartDot,
  ChartHead,
  ChartNotice,
  ChartTipLayer,
  DEFAULT_CHART_METRIC,
  formatAxisDate,
  MetricToggle,
  seriesColor,
  useObservabilityChartBuckets,
  type ChartMetricSelection,
  type ChartTip,
} from './observability-charts';
import { formatCompactNumber, formatNumber } from './model-observability-format';
import { subsystemLabel } from './model-observability-labels';

const GROUP_BY: readonly ModelObservabilityGroupByDimension[] = ['date', 'category'];
const WINDOW_DAYS = 29;

const VIEW_W = 520;
const VIEW_H = 260;

type DayEntry = { key: string; byCategory: Map<string, { calls: number; tokens: number }> };
type PieViewMode = 'daily' | 'all';

export function ObservabilityCategoryPieChart({ refreshToken }: { refreshToken: number }) {
  const { buckets, loading, error } = useObservabilityChartBuckets({ days: WINDOW_DAYS, groupBy: GROUP_BY, refreshToken });
  const [metric, setMetric] = useState<ChartMetricSelection>(DEFAULT_CHART_METRIC);
  const [selectedDate, setSelectedDate] = useState<string>('');
  const [viewMode, setViewMode] = useState<PieViewMode>('daily');
  const cardRef = useRef<HTMLElement | null>(null);
  const tipRef = useRef<HTMLDivElement | null>(null);
  const [tip, setTip] = useState<ChartTip | null>(null);

  const model = useMemo(() => {
    const days = new Map<string, DayEntry>();
    const totals = new Map<string, { calls: number; tokens: number }>();
    for (const bucket of buckets ?? []) {
      const key = String(bucket.values.date ?? '');
      const category = String(bucket.values.category ?? '—');
      if (!key) continue;
      const day = days.get(key) ?? { key, byCategory: new Map() };
      const acc = day.byCategory.get(category) ?? { calls: 0, tokens: 0 };
      const value = bucketValue(bucket);
      acc.calls += value.calls;
      acc.tokens += value.tokens;
      day.byCategory.set(category, acc);
      days.set(key, day);
      const total = totals.get(category) ?? { calls: 0, tokens: 0 };
      total.calls += value.calls;
      total.tokens += value.tokens;
      totals.set(category, total);
    }
    // 日期下拉按时间倒序（最新在前）；类别系列按 30 天总量降序拿稳定配色。
    const dayOptions = [...days.keys()].sort().reverse();
    const categories = [...totals.entries()]
      .sort((a, b) => b[1].calls - a[1].calls || b[1].tokens - a[1].tokens)
      .map(([name], index) => ({ name, label: subsystemLabel(name), color: seriesColor(index) }));
    return { days, dayOptions, categories, totals };
  }, [buckets]);

  // 默认停在最新有数据的一天；数据刷新后所选日期不在列表则回退到最新。
  const effectiveDate = model.dayOptions.includes(selectedDate) ? selectedDate : model.dayOptions[0] ?? '';
  const day = effectiveDate ? model.days.get(effectiveDate) : undefined;

  const dayTotals = useMemo(() => {
    let calls = 0;
    let tokens = 0;
    if (day) for (const value of day.byCategory.values()) { calls += value.calls; tokens += value.tokens; }
    return { calls, tokens };
  }, [day]);

  // 累计口径：窗口内所有日期合计（按类别累计再求总）。
  const allTotals = useMemo(() => {
    let calls = 0;
    let tokens = 0;
    for (const value of model.totals.values()) { calls += value.calls; tokens += value.tokens; }
    return { calls, tokens };
  }, [model]);

  // 当前口径的总计与类别取数入口（按日 = 所选当天；累计 = 窗口合计）。
  const grand = viewMode === 'all' ? allTotals : dayTotals;
  const categoryOf = (name: string) =>
    viewMode === 'all'
      ? model.totals.get(name) ?? { calls: 0, tokens: 0 }
      : day?.byCategory.get(name) ?? { calls: 0, tokens: 0 };
  const totalLabel = viewMode === 'all'
    ? t('settings.observability.charts.pie.allTotal')
    : t('settings.observability.charts.dailyTotal');

  const keys = (['calls', 'tokens'] as const).filter((key) => metric[key]);

  const onSliceHover = (categoryLabel: string, color: string, evt: React.MouseEvent) => {
    const name = model.categories.find((item) => item.label === categoryLabel)?.name ?? categoryLabel;
    const entry = categoryOf(name);
    // 饼图扇区无固定日期列，保持贴鼠标出现（右侧优先，右侧放不下放左侧）。
    const card = cardRef.current;
    const anchor = card
      ? {
          cx: evt.clientX - card.getBoundingClientRect().left,
          cy: evt.clientY - card.getBoundingClientRect().top,
          placement: 'horizontal' as const,
        }
      : undefined;
    setTip({
      title: `${categoryLabel} · ${viewMode === 'all'
        ? t('settings.observability.charts.pie.viewAll')
        : effectiveDate ? formatAxisDate(effectiveDate) : ''}`,
      rows: keys.map((key) => ({
        color,
        name: t(`settings.observability.charts.metric.${key}`),
        value: key === 'calls' ? `${formatNumber(entry.calls)}` : formatCompactNumber(entry.tokens),
      })),
      total: {
        label: totalLabel,
        value: keys.map((key) => key === 'calls' ? `${formatNumber(grand.calls)}` : formatCompactNumber(grand.tokens)).join(' · '),
      },
      anchor,
    });
  };

  return (
    <section className={styles['observability-panel']} ref={cardRef} data-chart="category-pie">
      <ChartHead
        title={t('settings.observability.charts.pie.title')}
        right={(
          <>
            <MetricToggle value={metric} onChange={setMetric} />
            <span className={styles['observability-chart-seg']} role="group" data-chart-view={viewMode}>
              <button
                type="button"
                data-active={viewMode === 'daily' ? 'true' : undefined}
                onClick={() => setViewMode('daily')}
              >
                {t('settings.observability.charts.pie.viewDaily')}
              </button>
              <button
                type="button"
                data-active={viewMode === 'all' ? 'true' : undefined}
                onClick={() => setViewMode('all')}
              >
                {t('settings.observability.charts.pie.viewAll')}
              </button>
            </span>
            {viewMode === 'daily' && (
              <span className={styles['observability-chart-select']}>
                <label>
                  {t('settings.observability.charts.pie.dateLabel')}
                  <select value={effectiveDate} onChange={(event) => setSelectedDate(event.target.value)} disabled={!effectiveDate}>
                    {model.dayOptions.map((option) => (
                      <option key={option} value={option}>{formatAxisDate(option)}</option>
                    ))}
                  </select>
                </label>
              </span>
            )}
          </>
        )}
      />
      {error ? <ChartNotice kind="error" /> : loading ? <ChartNotice kind="loading" />
        : viewMode === 'all'
          ? (grand.calls === 0 && grand.tokens === 0) ? <ChartNotice kind="empty" /> : renderBody()
          : !day ? <ChartNotice kind="empty" /> : renderBody()}
      <ChartTipLayer tip={tip} tipRef={tipRef} />
    </section>
  );

  function renderBody() {
    return (
      <div className={styles['observability-pie-wrap']}>
        <svg
          className={styles['observability-pie-svg']}
          viewBox={`0 0 ${VIEW_W} ${VIEW_H}`}
          role="img"
          aria-label={t('settings.observability.charts.pie.title')}
        >
          {keys.map((key, ringIndex) => {
            const cx = keys.length === 1 ? VIEW_W / 2 : VIEW_W * (ringIndex === 0 ? 0.27 : 0.73);
            const cy = VIEW_H / 2 + 4;
            const r = keys.length === 1 ? 92 : 78;
            const total = grand[key];
            let angle = -Math.PI / 2;
            const arcs = model.categories.map((item) => {
              const value = categoryOf(item.name)[key];
              const sweep = total > 0 ? (value / total) * Math.PI * 2 : 0;
              const start = angle;
              angle += sweep;
              return { item, value, start, end: angle };
            }).filter((arc) => arc.value > 0 && arc.end - arc.start > 0.004);
            return (
              <g key={key}>
                {arcs.map((arc) => {
                  const large = arc.end - arc.start > Math.PI ? 1 : 0;
                  const x0 = cx + r * Math.cos(arc.start);
                  const y0 = cy + r * Math.sin(arc.start);
                  const x1 = cx + r * Math.cos(arc.end);
                  const y1 = cy + r * Math.sin(arc.end);
                  return (
                    <path
                      key={arc.item.name}
                      d={`M ${cx} ${cy} L ${x0.toFixed(1)} ${y0.toFixed(1)} A ${r} ${r} 0 ${large} 1 ${x1.toFixed(1)} ${y1.toFixed(1)} Z`}
                      fill={arc.item.color}
                      className={styles['observability-chart-pie-slice']}
                      onMouseMove={(evt) => onSliceHover(arc.item.label, arc.item.color, evt)}
                      onMouseLeave={() => setTip(null)}
                    />
                  );
                })}
                <circle cx={cx} cy={cy} r={r * 0.55} className={styles['observability-chart-pie-hole']} />
                <text x={cx} y={cy - 2} fontSize="17" textAnchor="middle" className={styles['observability-chart-pie-total']}>
                  {key === 'calls' ? formatCompactNumber(grand.calls) : formatCompactNumber(grand.tokens)}
                </text>
                <text x={cx} y={cy + 16} fontSize="10" textAnchor="middle" className={styles['observability-chart-axis-text']}>
                  {t(`settings.observability.charts.metric.${key}`)}
                </text>
              </g>
            );
          })}
        </svg>
        <div className={styles['observability-pie-side']}>
          {model.categories.flatMap((item) => {
            const value = categoryOf(item.name);
            // 当前口径下对所选指标全为 0 的类别不占行（无数据成员不显示）。
            if (!keys.some((key) => value[key] > 0)) return [];
            const parts: string[] = [];
            if (metric.calls) parts.push(t('settings.observability.charts.callsUnit', { count: formatNumber(value.calls) }));
            if (metric.tokens) parts.push(t('settings.observability.charts.tokensUnit', { count: formatCompactNumber(value.tokens) }));
            return [(
              <div key={item.name} className={styles['observability-pie-row']}>
                <ChartDot color={item.color} size={10} />
                <span>{item.label}</span>
                <span className={styles['observability-pie-row-val']}>{parts.join(' · ') || '—'}</span>
              </div>
            )];
          })}
          <div className={styles['observability-pie-total']}>
            <span>{totalLabel}</span>
            <span>
              {[
                metric.calls ? t('settings.observability.charts.callsUnit', { count: formatNumber(grand.calls) }) : '',
                metric.tokens ? t('settings.observability.charts.tokensUnit', { count: formatCompactNumber(grand.tokens) }) : '',
              ].filter(Boolean).join(' · ') || '—'}
            </span>
          </div>
        </div>
      </div>
    );
  }
}
