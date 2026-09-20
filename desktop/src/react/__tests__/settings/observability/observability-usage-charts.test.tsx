/**
 * @vitest-environment jsdom
 *
 * Token 用量页常驻四图表测试：
 *   - 四张卡渲染（无固定维度角标、无提示行）；
 *   - 热力日历 每日/每周/累计 切换 + 悬停提示语；
 *   - 调用次数 / Token 用量 切换（可同显、最后一路不可关）；
 *   - 饼图日期下拉取自数据 + 按日/累计口径切换；
 *   - 四张图不随首行筛选联动（改筛选只重发指标卡查询，图表查询不重发）。
 */
import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import type { ModelObservabilityGroupMetrics } from '../../../../../../shared/model-observability-api-contract.ts';

const mocks = vi.hoisted(() => ({
  loadObservabilityHealth: vi.fn(),
  loadObservabilitySettings: vi.fn(),
  queryObservabilityAggregate: vi.fn(),
  queryObservabilityCalls: vi.fn(),
  queryObservabilityTraces: vi.fn(),
  updateObservabilitySettings: vi.fn(),
}));

vi.mock('../../../settings/tabs/observability/model-observability-actions', async (importOriginal) => {
  const original = await importOriginal<typeof import('../../../settings/tabs/observability/model-observability-actions')>();
  return {
    ...original,
    loadObservabilityHealth: (...args: unknown[]) => mocks.loadObservabilityHealth(...args),
    loadObservabilitySettings: (...args: unknown[]) => mocks.loadObservabilitySettings(...args),
    queryObservabilityAggregate: (...args: unknown[]) => mocks.queryObservabilityAggregate(...args),
    queryObservabilityCalls: (...args: unknown[]) => mocks.queryObservabilityCalls(...args),
    queryObservabilityTraces: (...args: unknown[]) => mocks.queryObservabilityTraces(...args),
    updateObservabilitySettings: (...args: unknown[]) => mocks.updateObservabilitySettings(...args),
  };
});

import { ObservabilityUsagePanel } from '../../../settings/tabs/observability/ObservabilityUsagePanel';
import { useObservabilityQueryState } from '../../../settings/tabs/observability/use-observability-query-state';

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

function bucket(values: Record<string, string | null>, metrics: Partial<ModelObservabilityGroupMetrics>) {
  return { key: Object.values(values).join('|'), values, metrics: makeMetrics(metrics) };
}

/* 近两天的玩具数据：热力/折线/柱状/饼图共用。 */
const D1 = new Date();
D1.setHours(0, 0, 0, 0);
const D2 = new Date(D1);
D2.setDate(D2.getDate() - 1);
const pad = (n: number) => String(n).padStart(2, '0');
const keyOf = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const DAY_KEYS = [keyOf(D2), keyOf(D1)];

function installAggregateMock() {
  mocks.queryObservabilityAggregate.mockImplementation((query: { groupBy: string[] }) => {
    const groupBy = query.groupBy;
    if (groupBy.length === 0) {
      return Promise.resolve({ overall: makeMetrics({ callCount: 42 }), groups: [] });
    }
    if (groupBy.length === 1 && groupBy[0] === 'date') {
      return Promise.resolve({
        overall: makeMetrics(),
        groups: DAY_KEYS.map((date) => bucket({ date }, { callCount: 20, totalTokens: 1000 })),
      });
    }
    if (groupBy.includes('model')) {
      return Promise.resolve({
        overall: makeMetrics(),
        groups: DAY_KEYS.flatMap((date) => [
          bucket({ date, provider: 'p1', modelId: 'model-a' }, { callCount: 12, totalTokens: 800 }),
          bucket({ date, provider: 'p2', modelId: 'model-b' }, { callCount: 8, totalTokens: 200 }),
        ]),
      });
    }
    if (groupBy.includes('provider')) {
      return Promise.resolve({
        overall: makeMetrics(),
        groups: DAY_KEYS.flatMap((date) => [
          bucket({ date, provider: 'p1' }, { callCount: 12, totalTokens: 800 }),
          bucket({ date, provider: 'p2' }, { callCount: 8, totalTokens: 200 }),
        ]),
      });
    }
    return Promise.resolve({
      overall: makeMetrics(),
      groups: DAY_KEYS.flatMap((date) => [
        bucket({ date, category: 'llm' }, { callCount: 18, totalTokens: 900 }),
        bucket({ date, category: 'utility' }, { callCount: 2, totalTokens: 100 }),
      ]),
    });
  });
}

function Harness({ refreshToken = 0 }: { refreshToken?: number }) {
  const state = useObservabilityQueryState();
  return <ObservabilityUsagePanel state={state} refreshToken={refreshToken} />;
}

describe('ObservabilityUsagePanel resident charts', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    installAggregateMock();
    window.t = ((key: string) => key) as typeof window.t;
    window.i18n = {
      locale: 'zh-CN',
      defaultName: 'Hana',
      _data: {},
      _agentOverrides: {},
      load: vi.fn(async () => {}),
      setAgentOverrides: vi.fn(),
      t: ((key: string) => key) as typeof window.t,
    };
    window.platform = {
      getServerPort: vi.fn(async () => 3000),
      getServerToken: vi.fn(async () => null),
      getPlatform: vi.fn(async () => 'darwin'),
      onSwitchTab: vi.fn(),
      onSettingsChanged: vi.fn(() => vi.fn()),
      onServerRestarted: vi.fn(),
    } as unknown as typeof window.platform;
  });

  afterEach(() => cleanup());

  it('renders the four resident chart cards (no fixed-dimension badges, no hints)', async () => {
    render(<Harness />);
    await waitFor(() => {
      expect(screen.getByText('settings.observability.charts.heat.title')).toBeInTheDocument();
    });
    expect(screen.getByText('settings.observability.charts.line.title')).toBeInTheDocument();
    expect(screen.getByText('settings.observability.charts.bar.title')).toBeInTheDocument();
    expect(screen.getByText('settings.observability.charts.pie.title')).toBeInTheDocument();
    // 固定维度角标已从四张卡移除。
    expect(screen.queryByText('settings.observability.charts.fixedBadgeDate')).not.toBeInTheDocument();
    expect(screen.queryByText('settings.observability.charts.fixedBadgeModelDate')).not.toBeInTheDocument();
    expect(screen.queryByText('settings.observability.charts.fixedBadgeProviderDate')).not.toBeInTheDocument();
    expect(screen.queryByText('settings.observability.charts.fixedBadgeCategoryDate')).not.toBeInTheDocument();
    // 每张图独立取数：挂载 = 4 次图表聚合 + overall + 洞察按日/按模型 = 7 次。
    expect(mocks.queryObservabilityAggregate).toHaveBeenCalledTimes(7);
    // 洞察卡：两天窗口 → 活跃天数 2（D1/D2 相邻 → 最长连续 2）；最高模型 model-a。
    await waitFor(() => {
      expect(screen.getByText('settings.observability.metrics.activeDays')).toBeInTheDocument();
    });
    const insightValues = [...document.querySelectorAll('[class*="observability-metric-card"]')]
      .find((card) => card.textContent!.includes('settings.observability.metrics.activeDays'))!
      .querySelector('[class*="observability-metric-value"]');
    expect(insightValues!.textContent).toBe('2');
  });

  it('heat calendar switches 每日/每周/累计 and re-renders the same grid', async () => {
    render(<Harness />);
    await waitFor(() => {
      expect(screen.getByText('settings.observability.charts.heat.daily')).toBeInTheDocument();
    });
    const seg = document.querySelector('[data-chart="heat-calendar"] [data-chart-mode]') as HTMLElement;
    expect(seg.getAttribute('data-chart-mode')).toBe('daily');

    fireEvent.click(within(seg).getByText('settings.observability.charts.heat.weekly'));
    expect(seg.getAttribute('data-chart-mode')).toBe('weekly');
    // 日历格子不因切换模式增减（365 格恒在）。
    const cells = document.querySelectorAll('[data-chart="heat-calendar"] rect').length;
    expect(cells).toBeGreaterThan(300);

    fireEvent.click(within(seg).getByText('settings.observability.charts.heat.cumulative'));
    expect(seg.getAttribute('data-chart-mode')).toBe('cumulative');
    expect(document.querySelectorAll('[data-chart="heat-calendar"] rect').length).toBe(cells);
  });

  it('metric toggle supports both-on and refuses turning off the last one', async () => {
    render(<Harness />);
    await waitFor(() => {
      expect(screen.getByText('settings.observability.charts.line.title')).toBeInTheDocument();
    });
    const card = document.querySelector('[data-chart="model-trend"]') as HTMLElement;
    const toggle = within(card).getAllByRole('button').filter((button) =>
      button.textContent === 'settings.observability.charts.metric.calls'
      || button.textContent === 'settings.observability.charts.metric.tokens');
    const callsButton = toggle.find((button) => button.textContent === 'settings.observability.charts.metric.calls')!;
    const tokensButton = toggle.find((button) => button.textContent === 'settings.observability.charts.metric.tokens')!;

    // 默认只开调用次数：单层标签；唯一的开启项不可关闭。
    expect(callsButton.getAttribute('aria-pressed')).toBe('true');
    expect((callsButton as HTMLButtonElement).disabled).toBe(true);

    // 打开 Token 用量 → 双层标签（同显）。
    fireEvent.click(tokensButton);
    expect(tokensButton.getAttribute('aria-pressed')).toBe('true');
    expect(card.querySelectorAll('[class*="observability-chart-panel-label"]')).toHaveLength(2);

    // 关掉调用次数 → 只剩 Token 单层。
    expect((callsButton as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(callsButton);
    expect(card.querySelectorAll('[class*="observability-chart-panel-label"]')).toHaveLength(1);
  });

  it('heat calendar hover shows the daily calls-and-tokens hint title', async () => {
    render(<Harness />);
    await waitFor(() => {
      expect(screen.getByText('settings.observability.charts.heat.daily')).toBeInTheDocument();
    });
    const cells = document.querySelectorAll('[data-chart="heat-calendar"] rect');
    expect(cells.length).toBeGreaterThan(300);
    fireEvent.mouseMove(cells[0]);
    const tipTitle = document.querySelector('[data-chart="heat-calendar"] [class*="observability-chart-tip-title"]');
    expect(tipTitle?.textContent).toContain('settings.observability.charts.heat.tooltipHint');
  });

  it('pie chart lists data days in the date select, latest by default', async () => {
    render(<Harness />);
    await waitFor(() => {
      expect(screen.getByText('settings.observability.charts.pie.title')).toBeInTheDocument();
    });
    const card = document.querySelector('[data-chart="category-pie"]') as HTMLElement;
    const select = within(card).getByRole('combobox') as HTMLSelectElement;
    const options = Array.from(select.options).map((option) => option.value);
    expect(options).toEqual(DAY_KEYS.slice().reverse()); // 最新在前
    expect(select.value).toBe(keyOf(D1)); // 默认停最新一天
  });

  it('pie chart can switch to the all-days cumulative view', async () => {
    render(<Harness />);
    await waitFor(() => {
      expect(screen.getByText('settings.observability.charts.pie.title')).toBeInTheDocument();
    });
    const card = document.querySelector('[data-chart="category-pie"]') as HTMLElement;
    // 默认按日：日期下拉在。
    expect(within(card).getByRole('combobox')).toBeInTheDocument();
    // 切到累计：日期下拉隐藏，总计行改为累计总量口径。
    fireEvent.click(within(card).getByText('settings.observability.charts.pie.viewAll'));
    expect(within(card).queryByRole('combobox')).not.toBeInTheDocument();
    const totalRow = card.querySelector('[class*="observability-pie-total"]') as HTMLElement;
    expect(totalRow.textContent).toContain('settings.observability.charts.pie.allTotal');
    expect(totalRow.textContent).toContain('settings.observability.charts.callsUnit');
    // 切回按日：下拉恢复，总计行回到当日口径。
    fireEvent.click(within(card).getByText('settings.observability.charts.pie.viewDaily'));
    expect(within(card).getByRole('combobox')).toBeInTheDocument();
    expect((card.querySelector('[class*="observability-pie-total"]') as HTMLElement).textContent)
      .toContain('settings.observability.charts.dailyTotal');
  });

  it('hides members with no data for the selected metrics (line chart)', async () => {
    // model-zero：两指标全 0；model-c：有调用但 Token 为 0（Token 视角下隐藏）。
    mocks.queryObservabilityAggregate.mockImplementation((query: { groupBy: string[] }) => {
      const groupBy = query.groupBy;
      if (groupBy.length === 0) return Promise.resolve({ overall: makeMetrics(), groups: [] });
      if (groupBy.length === 1 && groupBy[0] === 'date') {
        return Promise.resolve({ overall: makeMetrics(), groups: DAY_KEYS.map((date) => bucket({ date }, { callCount: 20, totalTokens: 1000 })) });
      }
      if (groupBy.includes('model')) {
        return Promise.resolve({
          overall: makeMetrics(),
          groups: DAY_KEYS.flatMap((date) => [
            bucket({ date, provider: 'p1', modelId: 'model-a' }, { callCount: 12, totalTokens: 800 }),
            bucket({ date, provider: 'p2', modelId: 'model-b' }, { callCount: 8, totalTokens: 200 }),
            bucket({ date, provider: 'p3', modelId: 'model-c' }, { callCount: 5, totalTokens: 0 }),
            bucket({ date, provider: 'p4', modelId: 'model-zero' }, { callCount: 0, totalTokens: 0 }),
          ]),
        });
      }
      if (groupBy.includes('provider')) {
        return Promise.resolve({
          overall: makeMetrics(),
          groups: DAY_KEYS.map((date) => bucket({ date, provider: 'p1' }, { callCount: 25, totalTokens: 1000 })),
        });
      }
      return Promise.resolve({
        overall: makeMetrics(),
        groups: DAY_KEYS.map((date) => bucket({ date, category: 'llm' }, { callCount: 20, totalTokens: 1000 })),
      });
    });
    render(<Harness />);
    await waitFor(() => {
      expect(screen.getByText('settings.observability.charts.line.title')).toBeInTheDocument();
    });
    const card = document.querySelector('[data-chart="model-trend"]') as HTMLElement;
    const legend = card.querySelector('[class*="observability-chart-legend"]') as HTMLElement;
    const legendText = () => legend.textContent ?? '';

    // 默认（调用次数）：model-zero 隐藏，model-c 可见。
    expect(legendText()).toContain('model-a');
    expect(legendText()).toContain('model-c');
    expect(legendText()).not.toContain('model-zero');

    // 切到只看 Token：model-c（有调用但 0 Token）与 model-zero 都隐藏。
    const toggleButtons = within(card).getAllByRole('button').filter((button) =>
      button.textContent === 'settings.observability.charts.metric.calls'
      || button.textContent === 'settings.observability.charts.metric.tokens');
    const tokensButton = toggleButtons.find((b) => b.textContent === 'settings.observability.charts.metric.tokens')!;
    const callsButton = toggleButtons.find((b) => b.textContent === 'settings.observability.charts.metric.calls')!;
    fireEvent.click(tokensButton); // 双开
    fireEvent.click(callsButton); // 只剩 tokens
    await waitFor(() => {
      expect(legend.textContent ?? '').not.toContain('model-c');
    });
    expect(legend.textContent ?? '').not.toContain('model-zero');
    expect(legend.textContent ?? '').toContain('model-a');
  });

  it('pie side list hides categories that are zero on the selected day', async () => {
    render(<Harness />);
    await waitFor(() => {
      expect(screen.getByText('settings.observability.charts.pie.title')).toBeInTheDocument();
    });
    const card = document.querySelector('[data-chart="category-pie"]') as HTMLElement;
    const side = card.querySelector('[class*="observability-pie-side"]') as HTMLElement;
    // 默认最新一天（D1）：llm 与 utility 都有数据 → 两行（外加总计行）。
    const rows = Array.from(side.children).filter((el) =>
      el.className.includes('observability-pie-row') && !el.className.includes('observability-pie-total'));
    expect(rows.length).toBe(2);
  });

  it('charts do not refetch when the first-row filter changes (metrics do)', async () => {
    render(<Harness />);
    await waitFor(() => {
      expect(mocks.queryObservabilityAggregate).toHaveBeenCalledTimes(7);
    });
    // 图表侧签名：折线 date,model / 柱状 date,provider / 饼图 date,category。
    const chartSig = (call: unknown[]) => ((call[0] as { groupBy: string[] }).groupBy || []).join(',');
    const isChartCall = (call: unknown[]) =>
      ['date,model', 'date,provider', 'date,category'].includes(chartSig(call));
    const chartCallsBefore = mocks.queryObservabilityAggregate.mock.calls.filter(isChartCall).length;
    const metricsCallsBefore = mocks.queryObservabilityAggregate.mock.calls
      .filter((call) => (call[0] as { groupBy: string[] }).groupBy.length === 0).length;

    // 首行筛选改时间范围（默认「全部」→ 24h）。
    fireEvent.click(screen.getByRole('button', { name: /observability\.datePreset\.all/ }));
    fireEvent.click(screen.getByLabelText('settings.observability.datePreset.24h'));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /observability\.datePreset\.24h/ })).toBeInTheDocument();
    });

    const chartCallsAfter = mocks.queryObservabilityAggregate.mock.calls.filter(isChartCall).length;
    const metricsCallsAfter = mocks.queryObservabilityAggregate.mock.calls
      .filter((call) => (call[0] as { groupBy: string[] }).groupBy.length === 0).length;
    expect(metricsCallsAfter).toBeGreaterThan(metricsCallsBefore); // 指标卡随筛选重查
    expect(chartCallsAfter).toBe(chartCallsBefore); // 四张图不随首行筛选联动
  });
});
