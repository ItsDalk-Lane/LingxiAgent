/**
 * ObservabilityUsagePanel.tsx — 「Token 用量」子标签页。
 *
 * 布局（2026-09-17 用户定稿）：首行筛选条（只作用于指标卡）+ 指标卡 +
 * 四张独占一行的常驻图表（热力日历 / 模型折线 / 供应商柱状 / 类别饼图）。
 * 四张图按各自固定维度自行取数，不随首行筛选联动。
 *
 * 指标卡查询（随首行筛选）：
 *   - aggregate groupBy:[] → overall 八卡；
 *   - aggregate groupBy:['date'] → 活跃天数 + 最长连续天数；
 *   - aggregate groupBy:['model'] → 用量最高的模型。
 */
import React, { useEffect, useRef, useState } from 'react';
import type { ModelObservabilityAggregateResult } from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import styles from '../../Settings.module.css';
import {
  isObservabilityAbortError,
  ModelObservabilityRequestError,
  queryObservabilityAggregate,
} from './model-observability-actions';
import { buildCallFilterInput, dateBucketForGroupBy } from './model-observability-filter';
import type { ObservabilityQueryStateApi } from './use-observability-query-state';
import type { ObservabilityMetricInsights } from './ObservabilityMetrics';
import { ObservabilityFilterBar } from './ObservabilityFilterBar';
import { ObservabilityMetrics } from './ObservabilityMetrics';
import { ObservabilityHeatCalendar } from './ObservabilityHeatCalendar';
import { ObservabilityModelTrendChart } from './ObservabilityModelTrendChart';
import { ObservabilityProviderBarChart } from './ObservabilityProviderBarChart';
import { ObservabilityCategoryPieChart } from './ObservabilityCategoryPieChart';

type Props = {
  state: ObservabilityQueryStateApi;
  refreshToken: number;
};

function computeInsights(
  dateGroups: { values: { date?: unknown }; metrics: { callCount: number; totalTokens: number | null } }[],
  modelGroups: { values: { modelId?: unknown }; metrics: { callCount: number; totalTokens: number | null } }[],
): ObservabilityMetricInsights {
  const dates = new Set<string>();
  for (const bucket of dateGroups) {
    const date = typeof bucket.values.date === 'string' ? bucket.values.date : '';
    if (date) dates.add(date);
  }
  const perModel = new Map<string, { calls: number; tokens: number }>();
  for (const bucket of modelGroups) {
    const name = typeof bucket.values.modelId === 'string' && bucket.values.modelId ? bucket.values.modelId : '—';
    const acc = perModel.get(name) ?? { calls: 0, tokens: 0 };
    acc.calls += bucket.metrics.callCount;
    acc.tokens += bucket.metrics.totalTokens ?? 0;
    perModel.set(name, acc);
  }
  // 最长连续活跃天数：按本地日历日逐日递增比对（跨 DST 不用毫秒差）。
  const sorted = [...dates].sort();
  let longest = 0;
  let run = 0;
  let prev: Date | null = null;
  for (const key of sorted) {
    const [y, m, d] = key.split('-').map(Number);
    const current = new Date(y, m - 1, d);
    if (prev) {
      const next = new Date(prev);
      next.setDate(prev.getDate() + 1);
      run = next.getTime() === current.getTime() ? run + 1 : 1;
    } else {
      run = 1;
    }
    longest = Math.max(longest, run);
    prev = current;
  }
  // 用户定稿：用量最高的模型按 Token 消耗排名（平手比调用次数）。
  const top = [...perModel.entries()].sort((a, b) => b[1].tokens - a[1].tokens || b[1].calls - a[1].calls)[0];
  // 高峰日：按 Token 总量最高（平手比调用次数）。
  let peak: { date: string; calls: number; tokens: number } | null = null;
  for (const bucket of dateGroups) {
    const date = typeof bucket.values.date === 'string' ? bucket.values.date : '';
    if (!date) continue;
    const calls = bucket.metrics.callCount;
    const tokens = bucket.metrics.totalTokens ?? 0;
    if (!peak || tokens > peak.tokens || (tokens === peak.tokens && calls > peak.calls)) {
      peak = { date, calls, tokens };
    }
  }
  return {
    activeDays: sorted.length,
    longestStreak: longest,
    topModel: top ? { name: top[0], calls: top[1].calls, tokens: top[1].tokens } : null,
    peakDay: peak,
  };
}

export function ObservabilityUsagePanel({ state, refreshToken }: Props) {
  const { appliedFilter } = state;
  const [aggregate, setAggregate] = useState<ModelObservabilityAggregateResult | null>(null);
  const [aggregateLoading, setAggregateLoading] = useState(true);
  const [aggregateError, setAggregateError] = useState<string | null>(null);
  const [insights, setInsights] = useState<ObservabilityMetricInsights | null>(null);
  const overallGenerationRef = useRef(0);
  const insightsGenerationRef = useRef(0);

  /* overall：指标卡八卡（groupBy 空，只取汇总）。 */
  useEffect(() => {
    const generation = ++overallGenerationRef.current;
    const controller = new AbortController();
    setAggregateLoading(true);
    setAggregateError(null);
    queryObservabilityAggregate(
      { filter: buildCallFilterInput(appliedFilter), groupBy: [] },
      { signal: controller.signal },
    ).then((result) => {
      if (overallGenerationRef.current !== generation) return;
      setAggregate(result);
      setAggregateLoading(false);
    }).catch((error: unknown) => {
      if (overallGenerationRef.current !== generation || isObservabilityAbortError(error)) return;
      if (error instanceof ModelObservabilityRequestError && error.kind === 'not_initialized') {
        setAggregate(null);
        setAggregateError('not_initialized');
      } else {
        setAggregateError(error instanceof Error ? error.message : String(error));
      }
      setAggregateLoading(false);
    });
    return () => controller.abort();
  }, [appliedFilter, refreshToken]);

  /* 洞察：活跃天数/最长连续（按日）+ 用量最高模型（按模型），随首行筛选。 */
  useEffect(() => {
    const generation = ++insightsGenerationRef.current;
    const controller = new AbortController();
    const filter = buildCallFilterInput(appliedFilter);
    const dateBucket = dateBucketForGroupBy(['date']);
    Promise.all([
      queryObservabilityAggregate({ filter, groupBy: ['date'], dateBucket }, { signal: controller.signal }),
      queryObservabilityAggregate({ filter, groupBy: ['model'] }, { signal: controller.signal }),
    ]).then(([byDate, byModel]) => {
      if (insightsGenerationRef.current !== generation) return;
      setInsights(computeInsights(byDate.groups, byModel.groups));
    }).catch(() => {
      // 洞察取数失败不打断指标卡（overall 已有独立错误位）；保持上次事实。
    });
    return () => controller.abort();
  }, [appliedFilter, refreshToken]);

  return (
    <div className={styles['observability-sub-panel']}>
      <ObservabilityFilterBar state={state} showGroupBy={false} />

      {aggregateError && aggregateError !== 'not_initialized' ? (
        <div className={styles['observability-error']} role="alert" data-kind="query_failed">
          <div className={styles['observability-error-title']}>{t('settings.observability.error.query_failed')}</div>
          <div className={styles['observability-error-detail']}>{aggregateError}</div>
        </div>
      ) : (
        <section className={styles['observability-panel']}>
          <ObservabilityMetrics overall={aggregate?.overall ?? null} loading={aggregateLoading} insights={insights} />
        </section>
      )}

      <ObservabilityHeatCalendar refreshToken={refreshToken} />
      <ObservabilityModelTrendChart refreshToken={refreshToken} />
      <ObservabilityProviderBarChart refreshToken={refreshToken} />
      <ObservabilityCategoryPieChart refreshToken={refreshToken} />
    </div>
  );
}
