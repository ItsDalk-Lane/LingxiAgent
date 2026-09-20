/**
 * ObservabilityModelTrendChart.tsx — 「模型调用趋势」（近 30 天）。
 *
 * 每个模型一条折线；右上角「调用次数 / Token 用量」可单选可同显
 * （同显 = 上下两层，各自独立刻度）。悬停显示当日每个模型的量 + 当日总量。
 * X 轴铺满 30 个日历日（无调用的日子是真实零，点落到 0）。
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
  seriesColor,
  svgAnchor,
  useObservabilityChartBuckets,
  type ChartMetricSelection,
  type ChartTip,
} from './observability-charts';
import { formatCompactNumber } from './model-observability-format';

const GROUP_BY: readonly ModelObservabilityGroupByDimension[] = ['date', 'model'];
const WINDOW_DAYS = 29; // 含今天共 30 天

const VIEW_W = 1120;
const VIEW_H = 300;
const VIEW_L = 70;
const VIEW_R = 16;
const VIEW_T = 16;
const VIEW_B = 30;

type Day = { key: string; byModel: Map<string, { calls: number; tokens: number }> };

export function ObservabilityModelTrendChart({ refreshToken }: { refreshToken: number }) {
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
      days.unshift({ key: localDateKey(cursor), byModel: new Map() });
    }
    const totals = new Map<string, { calls: number; tokens: number }>();
    for (const bucket of buckets ?? []) {
      const day = days.find((item) => item.key === String(bucket.values.date ?? ''));
      const name = bucket.values.modelId ?? '—';
      if (!day) continue;
      const value = bucketValue(bucket);
      const acc = day.byModel.get(name) ?? { calls: 0, tokens: 0 };
      acc.calls += value.calls;
      acc.tokens += value.tokens;
      day.byModel.set(name, acc);
      const total = totals.get(name) ?? { calls: 0, tokens: 0 };
      total.calls += value.calls;
      total.tokens += value.tokens;
      totals.set(name, total);
    }
    // 系列按总量降序稳定排列（最大者拿第一色）。
    const series = [...totals.entries()]
      .sort((a, b) => b[1].calls - a[1].calls || b[1].tokens - a[1].tokens)
      .map(([name, totalsOf], index) => ({ name, color: seriesColor(index), totals: totalsOf }));
    const dayTotals = days.map((day) => {
      let calls = 0;
      let tokens = 0;
      for (const value of day.byModel.values()) { calls += value.calls; tokens += value.tokens; }
      return { calls, tokens };
    });
    return { days, series, dayTotals };
  }, [buckets]);

  const keys = (['calls', 'tokens'] as const).filter((key) => metric[key]);
  // 当前所选指标下窗口总量全为 0 的模型不进图（无数据成员不显示）。
  const activeSeries = model.series.filter((item) => keys.some((key) => item.totals[key] > 0));
  const n = model.days.length;
  const xOf = (index: number) => VIEW_L + (index / Math.max(1, n - 1)) * (VIEW_W - VIEW_L - VIEW_R);
  const panelHeight = (VIEW_H - VIEW_T - VIEW_B) / keys.length;

  const axisIndices = useMemo(() => {
    const step = Math.max(1, Math.floor(n / 6));
    return Array.from({ length: n }, (_, index) => index).filter((index) => index % step === 0 || index === n - 1);
  }, [n]);

  const onMove = (evt: React.MouseEvent) => {
    const svg = svgRef.current;
    const card = cardRef.current;
    if (!svg || !card) return;
    const rect = svg.getBoundingClientRect();
    const viewX = ((evt.clientX - rect.left) / rect.width) * VIEW_W;
    const index = Math.max(0, Math.min(n - 1, Math.round(((viewX - VIEW_L) / (VIEW_W - VIEW_L - VIEW_R)) * (n - 1))));
    setHoverIndex(index);
    const day = model.days[index];
    const dayTotal = model.dayTotals[index];
    // 当天为 0 的成员不进悬停明细（无数据不显示）。
    const rows = activeSeries
      .map((item) => ({ item, value: day.byModel.get(item.name) ?? { calls: 0, tokens: 0 } }))
      .filter(({ value }) => keys.some((key) => value[key] > 0))
      .sort((a, b) => b.value.calls - a.value.calls || b.value.tokens - a.value.tokens)
      .map(({ item, value }) => ({ color: item.color, name: item.name, value: chartMetricValue(value, metric) }));
    // 锚定当日数据点：提示框放点左右，所指日期列始终可见。
    const point = svgAnchor(svg, card, VIEW_W, xOf(index), VIEW_H / 2);
    const mouseY = evt.clientY - card.getBoundingClientRect().top;
    setTip({
      title: `${formatAxisDate(day.key)} · ${t('settings.observability.charts.dailyTotal')}`,
      rows,
      total: { label: t('settings.observability.charts.dailyTotal'), value: chartMetricValue(dayTotal, metric) },
      anchor: {
        cx: point.cx,
        cy: mouseY,
        w: 9 * point.scale,
        placement: 'horizontal',
        fy: mouseY,
      },
    });
  };

  return (
    <section className={styles['observability-panel']} ref={cardRef} data-chart="model-trend">
      <ChartHead
        title={t('settings.observability.charts.line.title')}
        right={<MetricToggle value={metric} onChange={setMetric} />}
      />
      {activeSeries.length > 0 && <ChartLegend items={activeSeries.map((item) => ({ color: item.color, label: item.name }))} />}
      {error ? <ChartNotice kind="error" /> : loading ? <ChartNotice kind="loading" /> : (
        <svg
          ref={svgRef}
          className={styles['observability-chart-svg']}
          viewBox={`0 0 ${VIEW_W} ${VIEW_H}`}
          role="img"
          aria-label={t('settings.observability.charts.line.title')}
          onMouseMove={onMove}
          onMouseLeave={() => { setTip(null); setHoverIndex(null); }}
        >
          {keys.map((key, panelIndex) => {
            const gap = keys.length > 1 ? 16 : 0;
            const top = VIEW_T + panelIndex * panelHeight + (panelIndex ? gap : 0);
            const height = panelHeight - (panelIndex ? gap : 0);
            const maxValue = Math.max(1, ...model.days.flatMap((day) => [...day.byModel.values()].map((value) => value[key])));
            const yOf = (value: number) => top + (1 - value / (maxValue * 1.12)) * height;
            return (
              <g key={key}>
                {[0, 0.5, 1].map((ratio) => {
                  const gy = top + ratio * height;
                  return (
                    <g key={ratio}>
                      <line x1={VIEW_L} y1={gy} x2={VIEW_W - VIEW_R} y2={gy} className={styles['observability-chart-grid']} />
                      <text x={VIEW_L - 8} y={gy + 4} fontSize="10" textAnchor="end" className={styles['observability-chart-axis-text']}>
                        {formatCompactNumber(maxValue * 1.12 * (1 - ratio))}
                      </text>
                    </g>
                  );
                })}
                <text x={VIEW_L + 6} y={top + 13} fontSize="11" className={styles['observability-chart-panel-label']}>
                  {t(`settings.observability.charts.metric.${key}`)}
                </text>
                {activeSeries.map((item) => {
                  // 0 值日不画（点与线段都断开），避免无数据模型贴地出零线。
                  const segments: string[] = [];
                  let current: string[] = [];
                  model.days.forEach((day, index) => {
                    const value = day.byModel.get(item.name)?.[key] ?? 0;
                    if (value > 0) {
                      current.push(`${xOf(index).toFixed(1)},${yOf(value).toFixed(1)}`);
                    } else if (current.length > 0) {
                      segments.push(current.join(' '));
                      current = [];
                    }
                  });
                  if (current.length > 0) segments.push(current.join(' '));
                  return (
                    <g key={item.name}>
                      {segments.map((points) => (
                        <polyline
                          key={points.slice(0, 24)}
                          points={points}
                          fill="none"
                          stroke={item.color}
                          strokeWidth="2.2"
                          strokeLinejoin="round"
                          className={styles['observability-chart-line']}
                        />
                      ))}
                      {model.days.map((day, index) => {
                        const value = day.byModel.get(item.name)?.[key] ?? 0;
                        if (value <= 0) return null;
                        return <circle key={day.key} cx={xOf(index)} cy={yOf(value).toFixed(1)} r="2.4" fill={item.color} />;
                      })}
                    </g>
                  );
                })}
                {panelIndex === keys.length - 1 && axisIndices.map((index) => (
                  <text key={index} x={xOf(index)} y={VIEW_H - 8} fontSize="11" textAnchor="middle" className={styles['observability-chart-axis-text']}>
                    {formatAxisDate(model.days[index].key)}
                  </text>
                ))}
              </g>
            );
          })}
          {hoverIndex !== null && (
            <line
              x1={xOf(hoverIndex)}
              x2={xOf(hoverIndex)}
              y1={VIEW_T}
              y2={VIEW_H - VIEW_B}
              className={styles['observability-chart-guide']}
            />
          )}
        </svg>
      )}
      <ChartTipLayer tip={tip} tipRef={tipRef} />
    </section>
  );
}
