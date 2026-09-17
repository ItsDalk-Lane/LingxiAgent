/**
 * ObservabilityProviderBarChart.tsx — 「供应商调用分布」（固定 · 供应商 × 日期，近 30 天）。
 *
 * 每天一根柱、柱内按供应商分段堆叠；「调用次数 / Token 用量」可单选可
 * 同显（同显 = 上下两层）。悬停显示当日每个供应商的量 + 当日总量。
 */
import React, { useMemo, useRef, useState } from 'react';
import type { ModelObservabilityGroupByDimension } from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import styles from '../../Settings.module.css';
import {
  bucketValue,
  chartMetricValue,
  ChartHead,
  ChartLegend,
  ChartNotice,
  ChartTipLayer,
  DEFAULT_CHART_METRIC,
  formatAxisDate,
  localDateKey,
  MetricToggle,
  placeChartTip,
  seriesColor,
  useObservabilityChartBuckets,
  type ChartMetricSelection,
  type ChartTip,
} from './observability-charts';
import { formatCompactNumber } from './model-observability-format';

const GROUP_BY: readonly ModelObservabilityGroupByDimension[] = ['date', 'provider'];
const WINDOW_DAYS = 29;

const VIEW_W = 1120;
const VIEW_H = 300;
const VIEW_L = 70;
const VIEW_R = 16;
const VIEW_T = 16;
const VIEW_B = 30;

type Day = { key: string; byProvider: Map<string, { calls: number; tokens: number }> };

export function ObservabilityProviderBarChart({ refreshToken }: { refreshToken: number }) {
  const { buckets, loading, error } = useObservabilityChartBuckets({ days: WINDOW_DAYS, groupBy: GROUP_BY, refreshToken });
  const [metric, setMetric] = useState<ChartMetricSelection>(DEFAULT_CHART_METRIC);
  const cardRef = useRef<HTMLElement | null>(null);
  const tipRef = useRef<HTMLDivElement | null>(null);
  const [tip, setTip] = useState<ChartTip | null>(null);
  const svgRef = useRef<SVGSVGElement | null>(null);
  const [hoverIndex, setHoverIndex] = useState<number | null>(null);

  const model = useMemo(() => {
    const end = new Date();
    end.setHours(0, 0, 0, 0);
    const days: Day[] = [];
    for (let cursor = new Date(end); days.length <= WINDOW_DAYS; cursor.setDate(cursor.getDate() - 1)) {
      days.unshift({ key: localDateKey(cursor), byProvider: new Map() });
    }
    const totals = new Map<string, { calls: number; tokens: number }>();
    for (const bucket of buckets ?? []) {
      const day = days.find((item) => item.key === String(bucket.values.date ?? ''));
      const name = bucket.values.provider ?? '—';
      if (!day) continue;
      const value = bucketValue(bucket);
      const acc = day.byProvider.get(name) ?? { calls: 0, tokens: 0 };
      acc.calls += value.calls;
      acc.tokens += value.tokens;
      day.byProvider.set(name, acc);
      const total = totals.get(name) ?? { calls: 0, tokens: 0 };
      total.calls += value.calls;
      total.tokens += value.tokens;
      totals.set(name, total);
    }
    const series = [...totals.entries()]
      .sort((a, b) => b[1].calls - a[1].calls || b[1].tokens - a[1].tokens)
      .map(([name, totalsOf], index) => ({ name, color: seriesColor(index), totals: totalsOf }));
    const dayTotals = days.map((day) => {
      let calls = 0;
      let tokens = 0;
      for (const value of day.byProvider.values()) { calls += value.calls; tokens += value.tokens; }
      return { calls, tokens };
    });
    return { days, series, dayTotals };
  }, [buckets]);

  const keys = (['calls', 'tokens'] as const).filter((key) => metric[key]);
  // 当前所选指标下窗口总量全为 0 的供应商不进图（无数据成员不显示）。
  const activeSeries = model.series.filter((item) => keys.some((key) => item.totals[key] > 0));
  const n = model.days.length;
  const slot = (VIEW_W - VIEW_L - VIEW_R) / n;
  const barWidth = Math.min(22, slot * 0.6);
  const xCenter = (index: number) => VIEW_L + index * slot + slot / 2;
  const panelHeight = (VIEW_H - VIEW_T - VIEW_B) / keys.length;

  const axisIndices = useMemo(() => {
    const step = Math.max(1, Math.floor(n / 6));
    return Array.from({ length: n }, (_, index) => index).filter((index) => index % step === 0 || index === n - 1);
  }, [n]);

  const onMove = (evt: React.MouseEvent) => {
    const svg = svgRef.current;
    if (!svg) return;
    const rect = svg.getBoundingClientRect();
    const viewX = ((evt.clientX - rect.left) / rect.width) * VIEW_W;
    const index = Math.max(0, Math.min(n - 1, Math.floor((viewX - VIEW_L) / slot)));
    setHoverIndex(index);
    const day = model.days[index];
    const dayTotal = model.dayTotals[index];
    // 当天为 0 的供应商不进悬停明细（无数据不显示）。
    const rows = activeSeries
      .map((item) => ({ item, value: day.byProvider.get(item.name) ?? { calls: 0, tokens: 0 } }))
      .filter(({ value }) => keys.some((key) => value[key] > 0))
      .map(({ item, value }) => ({
        color: item.color,
        name: item.name,
        value: chartMetricValue(value, metric),
      }));
    setTip({
      title: `${formatAxisDate(day.key)} · ${t('settings.observability.charts.dailyTotal')}`,
      rows,
      total: { label: t('settings.observability.charts.dailyTotal'), value: chartMetricValue(dayTotal, metric) },
    });
    placeChartTip(tipRef.current, cardRef.current, evt);
  };

  return (
    <section className={styles['observability-panel']} ref={cardRef} data-chart="provider-bar">
      <ChartHead
        title={t('settings.observability.charts.bar.title')}
        badge={t('settings.observability.charts.fixedBadgeProviderDate')}
        right={<MetricToggle value={metric} onChange={setMetric} />}
      />
      <div className={styles['observability-chart-hint']}>{t('settings.observability.charts.bar.hint')}</div>
      {activeSeries.length > 0 && <ChartLegend items={activeSeries.map((item) => ({ color: item.color, label: item.name }))} />}
      {error ? <ChartNotice kind="error" /> : loading ? <ChartNotice kind="loading" /> : (
        <svg
          ref={svgRef}
          className={styles['observability-chart-svg']}
          viewBox={`0 0 ${VIEW_W} ${VIEW_H}`}
          role="img"
          aria-label={t('settings.observability.charts.bar.title')}
          onMouseMove={onMove}
          onMouseLeave={() => { setTip(null); setHoverIndex(null); }}
        >
          {keys.map((key, panelIndex) => {
            const gap = keys.length > 1 ? 16 : 0;
            const top = VIEW_T + panelIndex * panelHeight + (panelIndex ? gap : 0);
            const height = panelHeight - (panelIndex ? gap : 0);
            const totals = model.days.map((day) => {
              let sum = 0;
              for (const value of day.byProvider.values()) sum += value[key];
              return sum;
            });
            const maxValue = Math.max(1, ...totals) * 1.12;
            return (
              <g key={key}>
                {[0, 0.5, 1].map((ratio) => {
                  const gy = top + ratio * height;
                  return (
                    <g key={ratio}>
                      <line x1={VIEW_L} y1={gy} x2={VIEW_W - VIEW_R} y2={gy} className={styles['observability-chart-grid']} />
                      <text x={VIEW_L - 8} y={gy + 4} fontSize="10" textAnchor="end" className={styles['observability-chart-axis-text']}>
                        {formatCompactNumber(maxValue * (1 - ratio))}
                      </text>
                    </g>
                  );
                })}
                <text x={VIEW_L + 6} y={top + 13} fontSize="11" className={styles['observability-chart-panel-label']}>
                  {t(`settings.observability.charts.metric.${key}`)}
                </text>
                {model.days.map((day, index) => {
                  let acc = 0;
                  return (
                    <g key={day.key}>
                      {activeSeries.map((item) => {
                        const value = day.byProvider.get(item.name)?.[key] ?? 0;
                        const barHeight = (value / maxValue) * height;
                        const y = top + height - acc - barHeight;
                        acc += barHeight;
                        if (barHeight <= 0) return null;
                        return (
                          <rect
                            key={item.name}
                            x={xCenter(index) - barWidth / 2}
                            y={y}
                            width={barWidth}
                            height={barHeight}
                            fill={item.color}
                            className={styles['observability-chart-bar']}
                          />
                        );
                      })}
                    </g>
                  );
                })}
                {panelIndex === keys.length - 1 && axisIndices.map((index) => (
                  <text key={index} x={xCenter(index)} y={VIEW_H - 8} fontSize="11" textAnchor="middle" className={styles['observability-chart-axis-text']}>
                    {formatAxisDate(model.days[index].key)}
                  </text>
                ))}
              </g>
            );
          })}
          {hoverIndex !== null && (
            <rect
              x={VIEW_L + hoverIndex * slot}
              y={VIEW_T}
              width={slot}
              height={VIEW_H - VIEW_T - VIEW_B}
              rx="6"
              className={styles['observability-chart-hover-band']}
            />
          )}
        </svg>
      )}
      <ChartTipLayer tip={tip} tipRef={tipRef} />
    </section>
  );
}
