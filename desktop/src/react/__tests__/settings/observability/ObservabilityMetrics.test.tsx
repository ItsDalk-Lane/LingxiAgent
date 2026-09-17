/**
 * @vitest-environment jsdom
 *
 * Phase 9 Metrics 仪表盘测试 — 8 卡布局、null≠0（cost/duration null → —，
 * §三十三）、cache 命中率只在有观测样本时出现。
 */
import React from 'react';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { ObservabilityMetrics } from '../../../settings/tabs/observability/ObservabilityMetrics';
import type { ModelObservabilityGroupMetrics } from '../../../../../../shared/model-observability-api-contract.ts';

function makeMetrics(overrides: Partial<ModelObservabilityGroupMetrics> = {}): ModelObservabilityGroupMetrics {
  return {
    callCount: 100,
    traceCount: 40,
    okCount: 90,
    errorCount: 8,
    abortedCount: 2,
    incompleteCount: 0,
    attemptCount: 110,
    durationObservedCount: 100,
    durationTotalMs: 850_000,
    durationAverageMs: 8500,
    usageAggregateAvailability: 'complete',
    usageCoveredCalls: 95,
    usageCorruptCalls: 0,
    usageNotCorrelatedCalls: 5,
    usageUnknownCalls: 0,
    usageMissingCalls: 5,
    inputTokens: 12_000,
    outputTokens: 3_400,
    reasoningTokens: 0,
    cacheReadTokens: 1_000,
    cacheWriteTokens: 0,
    totalTokens: 15_400,
    costTotal: 1.234,
    cacheHitCount: 4,
    cacheObservedCount: 5,
    ...overrides,
  };
}

describe('ObservabilityMetrics (§五十三～五十七)', () => {
  beforeAll(() => {
    window.t = ((key: string, params?: Record<string, unknown>) => {
      if (params && Object.keys(params).length > 0) return `${key}:${JSON.stringify(params)}`;
      return key;
    }) as typeof window.t;
  });

  afterEach(() => cleanup());

  it('renders exactly 10 metric cards (cost removed; activeDays/topModel/peakDay added)', () => {
    render(<ObservabilityMetrics overall={makeMetrics()} loading={false} />);
    const metrics = document.querySelector('[class*="observability-metrics"]');
    expect(metrics).not.toBeNull();
    expect(metrics!.querySelectorAll('[class*="observability-metric-card"]')).toHaveLength(10);
  });

  it('insight cards: active days with streak, top model with calls and tokens', () => {
    render(<ObservabilityMetrics
      overall={makeMetrics()}
      loading={false}
      insights={{
        activeDays: 5,
        longestStreak: 3,
        topModel: { name: 'model-a', calls: 24, tokens: 15_400 },
        peakDay: { date: '2026-09-12', calls: 100, tokens: 90_000 },
      }}
    />);
    const cards = [...document.querySelectorAll('[class*="observability-metric-card"]')];
    const activeCard = cards.find((card) => card.textContent!.includes('settings.observability.metrics.activeDays'))!;
    expect(activeCard).toBeDefined();
    expect(activeCard.textContent).toContain('settings.observability.metrics.longestStreak:{"count":"3"}');
    const topCard = cards.find((card) => card.textContent!.includes('settings.observability.metrics.topModel'))!;
    expect(topCard.textContent).toContain('model-a');
    expect(topCard.textContent).toContain('24');
    expect(topCard.textContent).toContain('1.54万');
    // 调用次数与 Token 上下两行（各自独立 hint 行，不拼一行）。
    const topHints = [...topCard.querySelectorAll('[class*="observability-metric-hint"]')].map((el) => el.textContent);
    expect(topHints).toEqual([
      'settings.observability.charts.callsUnit:{"count":"24"}',
      'settings.observability.charts.tokensUnit:{"count":"1.54万"}',
    ]);
    // 高峰日：Token 最高的一天，值 = 日期，副行同样是上下两行。
    const peakCard = cards.find((card) => card.textContent!.includes('settings.observability.metrics.peakDay'))!;
    expect(peakCard.textContent).toContain('settings.observability.charts.callsUnit:{"count":"100"}');
    expect(peakCard.textContent).toContain('9万');
    // 洞察缺事实 → 卡值走 "—"（不冒充 0）。
    const fallback = document.querySelector('[class*="observability-metrics"]');
    expect(fallback).not.toBeNull();
  });

  it('formats real values (avg duration, token totals; cost card removed 2026-09-17)', () => {
    render(<ObservabilityMetrics overall={makeMetrics()} loading={false} />);
    expect(screen.getAllByText('8.5s')).toHaveLength(1);
    expect(screen.getAllByText('1.54万')).toHaveLength(1);
    expect(screen.getAllByText('100')).toHaveLength(1);
    expect(screen.queryByText('$1.23')).toBeNull();
  });

  it('null cost/duration render em dash, never a fake zero (§三十三)', () => {
    render(<ObservabilityMetrics overall={makeMetrics({ costTotal: null, durationAverageMs: null })} loading={false} />);
    const values = [...document.querySelectorAll('[class*="observability-metric-value"]')].map((el) => el.textContent);
    expect(values.filter((v) => v === '—').length).toBeGreaterThanOrEqual(2);
    expect(values).not.toContain('$0.00');
  });

  it('cache hit rate appears only when cacheObservedCount > 0', () => {
    const { unmount } = render(<ObservabilityMetrics overall={makeMetrics()} loading={false} />);
    expect(screen.getAllByText(/cacheHitRate/)).toHaveLength(1);
    unmount();

    render(<ObservabilityMetrics overall={makeMetrics({ cacheObservedCount: 0, cacheHitCount: 0 })} loading={false} />);
    expect(screen.queryByText(/cacheHitRate/)).toBeNull();
  });

  it('overall=null renders 10 placeholder cards with em dashes', () => {
    render(<ObservabilityMetrics overall={null} loading={true} />);
    const cards = document.querySelectorAll('[class*="observability-metric-card"]');
    expect(cards).toHaveLength(10);
    [...cards].forEach((card) => {
      expect(card.querySelector('[class*="observability-metric-value"]')!.textContent).toBe('—');
    });
  });

  it('部分覆盖和投影不可用明确提示，未知 token 保持破折号', () => {
    const { unmount } = render(<ObservabilityMetrics overall={makeMetrics({
      usageAggregateAvailability: 'partial',
      usageCoveredCalls: 5,
      usageNotCorrelatedCalls: 0,
      usageUnknownCalls: 5,
      totalTokens: null,
      inputTokens: null,
      outputTokens: null,
    })} loading={false} />);
    expect(document.querySelector('[data-usage-availability="partial"]')).not.toBeNull();
    expect(screen.getByText(/usageCoveragePartial/)).toBeInTheDocument();
    const values = [...document.querySelectorAll('[class*="observability-metric-value"]')].map((el) => el.textContent);
    expect(values.filter((value) => value === '—').length).toBeGreaterThanOrEqual(3);
    unmount();

    render(<ObservabilityMetrics overall={makeMetrics({
      usageAggregateAvailability: 'projection_unavailable',
      usageCoveredCalls: 0,
      usageUnknownCalls: 100,
      totalTokens: null,
    })} loading={false} />);
    expect(document.querySelector('[data-usage-availability="projection_unavailable"]')).not.toBeNull();
    expect(screen.getByText(/usageCoverageProjectionUnavailable/)).toBeInTheDocument();
  });

  it('损坏的 usage 聚合显示 corrupt 告警，不显示伪造 token', () => {
    render(<ObservabilityMetrics overall={makeMetrics({
      usageAggregateAvailability: 'corrupt',
      usageCoveredCalls: 0,
      usageCorruptCalls: 1,
      usageNotCorrelatedCalls: 0,
      usageUnknownCalls: 0,
      callCount: 1,
      inputTokens: null,
      outputTokens: null,
      totalTokens: null,
      costTotal: null,
    })} loading={false} />);
    expect(document.querySelector('[data-usage-availability="corrupt"]')).not.toBeNull();
    expect(screen.getByText(/usageCoverageCorrupt/)).toBeInTheDocument();
    const values = [...document.querySelectorAll('[class*="observability-metric-value"]')]
      .map((element) => element.textContent);
    expect(values.filter((value) => value === '—').length).toBeGreaterThanOrEqual(4);
  });
});
