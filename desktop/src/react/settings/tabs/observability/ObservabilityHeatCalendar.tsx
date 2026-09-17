/**
 * ObservabilityHeatCalendar.tsx — 「调用热力日历」（固定 · 日期，滚动 12 个月）。
 *
 * 规范日历热力图：列 = 周（时间横向从左往右），行 = 星期（一…日 竖向），
 * 月份标签横排在底沿（从当前月往前数 12 个月，标在当月 1 号所在列下方）。
 * 三档视图共享同一面日历格子：
 *   - 每日：每格 = 当天调用量；
 *   - 每周：格子位置不变，同一周的格子按当周合计统一着色（纵向成条）；
 *   - 累计：每格 = 截至当天的累计调用量（越近越饱和）。
 * 悬停显示对应口径的调用次数 + Token 总量。
 */
import React, { useMemo, useRef, useState } from 'react';
import type { ModelObservabilityGroupByDimension } from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import styles from '../../Settings.module.css';
import {
  bucketValue,
  ChartHead,
  ChartNotice,
  ChartTipLayer,
  formatAxisDate,
  formatFullLocalDate,
  localDateKey,
  mondayIndex,
  placeChartTip,
  useObservabilityChartBuckets,
  weekStart,
  type ChartTip,
} from './observability-charts';
import { formatCompactNumber, formatNumber } from './model-observability-format';

const GROUP_BY: readonly ModelObservabilityGroupByDimension[] = ['date'];
/** 365 天 + 首列周内偏移，凑满 53 周。 */
const WINDOW_DAYS = 364;

type HeatMode = 'daily' | 'weekly' | 'cumulative';
const MODES: HeatMode[] = ['daily', 'weekly', 'cumulative'];

const VIEW_W = 1120;
const VIEW_L = 44;
const VIEW_R = 16;
const VIEW_T = 6;
const CELL_H = 24;
const CELL_GAP = 5;

export function ObservabilityHeatCalendar({ refreshToken }: { refreshToken: number }) {
  const { buckets, loading, error } = useObservabilityChartBuckets({ days: WINDOW_DAYS, groupBy: GROUP_BY, refreshToken });
  const [mode, setMode] = useState<HeatMode>('daily');
  const cardRef = useRef<HTMLElement | null>(null);
  const tipRef = useRef<HTMLDivElement | null>(null);
  const [tip, setTip] = useState<ChartTip | null>(null);

  const model = useMemo(() => {
    // 以「今天」为右端点铺满 365 格；无数据的日期按 0（真实零）。
    const end = new Date();
    end.setHours(0, 0, 0, 0);
    const start = new Date(end);
    start.setDate(start.getDate() - WINDOW_DAYS);
    const firstOffset = mondayIndex(start);
    const days: { date: Date; key: string; calls: number; tokens: number }[] = [];
    for (let cursor = new Date(start); cursor <= end; cursor.setDate(cursor.getDate() + 1)) {
      days.push({ date: new Date(cursor), key: localDateKey(cursor), calls: 0, tokens: 0 });
    }
    const byKey = new Map(days.map((day) => [day.key, day]));
    for (const bucket of buckets ?? []) {
      const day = byKey.get(String(bucket.values.date ?? ''));
      if (!day) continue;
      const value = bucketValue(bucket);
      day.calls += value.calls;
      day.tokens += value.tokens;
    }
    // 每周合计（以周一为键）与逐日累计。
    const weekTotal = new Map<string, { calls: number; tokens: number }>();
    const weekStartDate = new Map<string, Date>();
    for (const day of days) {
      const monday = weekStart(day.date);
      const key = localDateKey(monday);
      const acc = weekTotal.get(key) ?? { calls: 0, tokens: 0 };
      acc.calls += day.calls;
      acc.tokens += day.tokens;
      weekTotal.set(key, acc);
      weekStartDate.set(key, monday);
    }
    const cumulative: { calls: number; tokens: number }[] = [];
    let run = { calls: 0, tokens: 0 };
    for (const day of days) {
      run = { calls: run.calls + day.calls, tokens: run.tokens + day.tokens };
      cumulative.push(run);
    }
    const columns = Math.ceil((days.length + firstOffset) / 7);
    return { days, firstOffset, columns, weekTotal, weekStartDate, cumulative, total: run };
  }, [buckets]);

  const cellSide = Math.max(10, Math.min(
    Math.floor((VIEW_W - VIEW_L - VIEW_R) / model.columns) - CELL_GAP,
    CELL_H - CELL_GAP,
  ));
  const colWidth = (VIEW_W - VIEW_L - VIEW_R) / model.columns;
  const viewH = VIEW_T + 7 * CELL_H + 34;

  const scaleMax = useMemo(() => {
    if (mode === 'weekly') {
      let max = 0;
      for (const acc of model.weekTotal.values()) max = Math.max(max, acc.calls);
      return max;
    }
    if (mode === 'cumulative') return model.total.calls;
    return Math.max(1, ...model.days.map((day) => day.calls));
  }, [mode, model]);

  const valueOf = (index: number) => {
    const day = model.days[index];
    if (mode === 'weekly') return model.weekTotal.get(localDateKey(weekStart(day.date)))?.calls ?? 0;
    if (mode === 'cumulative') return model.cumulative[index].calls;
    return day.calls;
  };

  const onHover = (index: number, evt: React.MouseEvent) => {
    const day = model.days[index];
    let title: string;
    let calls: number;
    let tokens: number;
    if (mode === 'weekly') {
      const monday = weekStart(day.date);
      const sunday = new Date(monday);
      sunday.setDate(monday.getDate() + 6);
      title = t('settings.observability.charts.heat.weekRange', {
        since: formatAxisDate(localDateKey(monday)),
        until: formatAxisDate(localDateKey(sunday)),
      });
      const acc = model.weekTotal.get(localDateKey(monday)) ?? { calls: 0, tokens: 0 };
      calls = acc.calls;
      tokens = acc.tokens;
    } else if (mode === 'cumulative') {
      title = t('settings.observability.charts.heat.cumulativeUntil', { date: formatAxisDate(day.key) });
      calls = model.cumulative[index].calls;
      tokens = model.cumulative[index].tokens;
    } else {
      title = formatFullLocalDate(day.date);
      calls = day.calls;
      tokens = day.tokens;
    }
    setTip({
      title,
      rows: [
        { color: '#9ce0d4', name: t('settings.observability.charts.heat.tooltipCalls'), value: `${formatNumber(calls)}` },
        { color: '#f2cf8d', name: t('settings.observability.charts.heat.tooltipTokens'), value: formatCompactNumber(tokens) },
      ],
    });
    placeChartTip(tipRef.current, cardRef.current, evt);
  };

  const monthLabels = useMemo(() => {
    const labels: { x: number; text: string }[] = [];
    const end = model.days[model.days.length - 1].date;
    const formatter = new Intl.DateTimeFormat(chartLocaleSafe(), { month: 'short' });
    for (let back = 0; back < 12; back += 1) {
      const firstOfMonth = new Date(end.getFullYear(), end.getMonth() - back, 1);
      const index = model.days.findIndex((day) => day.key === localDateKey(firstOfMonth));
      if (index < 0) continue;
      const column = Math.floor((index + model.firstOffset) / 7);
      labels.push({ x: VIEW_L + column * colWidth + colWidth / 2, text: formatter.format(firstOfMonth) });
    }
    return labels;
  }, [model, colWidth]);

  const weekdayLabels = useMemo(() => {
    const monday = weekStart(model.days[0].date);
    const formatter = new Intl.DateTimeFormat(chartLocaleSafe(), { weekday: 'narrow' });
    return Array.from({ length: 7 }, (_, row) => {
      const probe = new Date(monday);
      probe.setDate(monday.getDate() + row);
      return { row, text: formatter.format(probe) };
    });
  }, [model]);

  return (
    <section className={styles['observability-panel']} ref={cardRef} data-chart="heat-calendar">
      <ChartHead
        title={t('settings.observability.charts.heat.title')}
        badge={t('settings.observability.charts.fixedBadgeDate')}
        right={(
          <span className={styles['observability-chart-seg']} role="group" data-chart-mode={mode}>
            {MODES.map((item) => (
              <button
                key={item}
                type="button"
                data-active={mode === item ? 'true' : undefined}
                onClick={() => setMode(item)}
              >
                {t(`settings.observability.charts.heat.${item}`)}
              </button>
            ))}
          </span>
        )}
      />
      <div className={styles['observability-chart-hint']}>{t(`settings.observability.charts.heat.hint.${mode}`)}</div>
      {error ? <ChartNotice kind="error" /> : loading ? <ChartNotice kind="loading" /> : (
        <svg
          className={styles['observability-chart-svg']}
          viewBox={`0 0 ${VIEW_W} ${viewH}`}
          role="img"
          aria-label={t('settings.observability.charts.heat.title')}
        >
          {model.days.map((day, index) => {
            const column = Math.floor((index + model.firstOffset) / 7);
            const row = mondayIndex(day.date);
            const ratio = scaleMax > 0 ? Math.min(1, valueOf(index) / scaleMax) : 0;
            const opacity = 0.07 + Math.pow(ratio, 0.6) * 0.9;
            const cellX = VIEW_L + column * colWidth + (colWidth - cellSide) / 2;
            const cellY = VIEW_T + row * CELL_H + (CELL_H - cellSide) / 2;
            return (
              <rect
                key={day.key}
                x={cellX}
                y={cellY}
                width={cellSide}
                height={cellSide}
                rx="4"
                className={mode === 'weekly'
                  ? styles['observability-heat-cell-weekly']
                  : styles['observability-heat-cell-daily']}
                opacity={opacity.toFixed(2)}
                onMouseMove={(evt) => onHover(index, evt)}
                onMouseLeave={() => setTip(null)}
              />
            );
          })}
          {weekdayLabels.map((label) => (
            <text
              key={label.row}
              x={VIEW_L - 10}
              y={VIEW_T + label.row * CELL_H + CELL_H / 2 + 4}
              fontSize="11"
              textAnchor="end"
              className={styles['observability-chart-axis-text']}
            >
              {label.text}
            </text>
          ))}
          {monthLabels.map((label) => (
            <text
              key={label.text + label.x.toFixed(0)}
              x={label.x}
              y={VIEW_T + 7 * CELL_H + 22}
              fontSize="11"
              textAnchor="middle"
              className={styles['observability-chart-axis-text']}
            >
              {label.text}
            </text>
          ))}
        </svg>
      )}
      <ChartTipLayer tip={tip} tipRef={tipRef} />
    </section>
  );
}

function chartLocaleSafe(): string {
  const locale = (window as unknown as { i18n?: { locale?: string } }).i18n?.locale;
  return locale || 'zh-CN';
}
