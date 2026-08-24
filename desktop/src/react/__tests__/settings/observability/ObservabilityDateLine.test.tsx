/**
 * @vitest-environment jsdom
 *
 * 日期分组折线图几何回归锁定：
 *   1. 点的 X 按真实日期比例（隔 8 天的相邻点距离 ≈ 隔 1 天的 8 倍）；
 *   2. X 轴标签的位置与其对应数据点严格一致（百分比注入同一坐标系）；
 *   3. 标签按水平位置贪心抽稀（密集区跳过、首尾必显），任何点附近有日期参照。
 */
import React from 'react';
import { afterEach, describe, expect, it, beforeAll } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import type {
  ModelObservabilityGroupBucket,
  ModelObservabilityGroupMetrics,
} from '../../../../../../shared/model-observability-api-contract.ts';
import { ObservabilityGroups } from '../../../settings/tabs/observability/ObservabilityGroups';

beforeAll(() => {
  window.t = ((key: string, params?: Record<string, unknown>) => {
    if (params && Object.keys(params).length > 0) return `${key}:${JSON.stringify(params)}`;
    return key;
  }) as typeof window.t;
});

const METRICS: ModelObservabilityGroupMetrics = {
  callCount: 1, traceCount: 1, okCount: 1, errorCount: 0, abortedCount: 0, incompleteCount: 0,
  attemptCount: 1, durationObservedCount: 0, durationAverageMs: null, durationMinMs: null, durationMaxMs: null,
  usagePresentCount: 0, usageMissingCount: 0, usageCorruptCount: 0, usageProjectionUnavailableCount: 0,
  usageUnknownCount: 1, inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0,
  totalTokens: 0, costTotal: null, cacheHitCount: 0, cacheObservedCount: 0,
} as unknown as ModelObservabilityGroupMetrics;

function dateBucket(date: string, callCount: number): ModelObservabilityGroupBucket {
  return { key: `date:${date}`, values: { date }, metrics: { ...METRICS, callCount } };
}

const POINT_X = /M ([\d.]+) [\d.]+ l 0\.001 0/g;

function dotXs(container: HTMLElement): number[] {
  const path = container.querySelector('path[class*="date-point"]')?.getAttribute('d') ?? '';
  return [...path.matchAll(POINT_X)].map((match) => Number(match[1]));
}

function tickPositions(container: HTMLElement): Array<{ label: string; x: number }> {
  const ticks = [...container.querySelectorAll<HTMLElement>('span[class*="date-tick"]')];
  return ticks
    .map((node) => ({
      label: (node.textContent ?? '').trim(),
      x: Number.parseFloat(node.style.getPropertyValue('--observability-date-tick-x')),
    }))
    .filter((tick) => tick.label !== '');
}

describe('date line chart geometry', () => {
  it('dot X follows real date proportions (8-day gap ≈ 8× 1-day gap)', () => {
    const buckets = [
      dateBucket('2026-08-01', 3),
      dateBucket('2026-08-02', 1),
      dateBucket('2026-08-10', 2),
      dateBucket('2026-08-11', 5),
      dateBucket('2026-08-19', 1),
      dateBucket('2026-08-20', 4),
    ];
    const { container } = render(<ObservabilityGroups buckets={buckets} groupBy={['date']} loading={false} />);
    const xs = dotXs(container);
    expect(xs).toHaveLength(6);
    // 窗口 8-01 → 8-20（19 天），绘图宽 88：首点 6，末点 94
    expect(xs[0]).toBeCloseTo(6, 1);
    expect(xs[5]).toBeCloseTo(94, 1);
    // 8-02 距 8-01 一天：6 + 88/19 ≈ 10.63
    expect(xs[1]).toBeCloseTo(6 + (88 / 19), 1);
    // 8-10 距 8-01 九天：6 + 88*9/19 ≈ 47.68
    expect(xs[2]).toBeCloseTo(6 + (88 * 9 / 19), 1);
    // 隔 8 天的间距是隔 1 天的 8 倍（真实时间轴的核心语义）
    expect((xs[2] - xs[1]) / (xs[1] - xs[0])).toBeCloseTo(8, 1);
  });

  it('ticks sit exactly under their dots and spread evenly (greedy min-gap)', () => {
    const buckets = [
      dateBucket('2026-08-01', 3),
      dateBucket('2026-08-02', 1),
      dateBucket('2026-08-10', 2),
      dateBucket('2026-08-11', 5),
      dateBucket('2026-08-19', 1),
      dateBucket('2026-08-20', 4),
    ];
    const { container } = render(<ObservabilityGroups buckets={buckets} groupBy={['date']} loading={false} />);
    const xs = dotXs(container);
    const ticks = tickPositions(container);
    // 首尾必显，密集点（8-02、8-11、8-19 与邻近标签过近）被跳过
    expect(ticks.map((tick) => tick.label)).toEqual(['Aug 1', 'Aug 10', 'Aug 20']);
    for (const tick of ticks) {
      const matchingDot = xs.find((x) => Math.abs(x - tick.x) < 0.01);
      expect(matchingDot, `tick ${tick.label}@${tick.x} must sit under its dot`).toBeDefined();
    }
  });

  it('single day renders one centered dot with one tick', () => {
    const { container } = render(
      <ObservabilityGroups buckets={[dateBucket('2026-08-23', 7)]} groupBy={['date']} loading={false} />,
    );
    expect(dotXs(container)).toEqual([50]);
    const ticks = tickPositions(container);
    expect(ticks).toHaveLength(1);
    expect(ticks[0].x).toBeCloseTo(50, 1);
  });

  it('hovering a date region shows its tooltip card; leaving hides it; hit zones cover their dots', async () => {
    const buckets = [dateBucket('2026-08-22', 12), dateBucket('2026-08-23', 68)];
    const { container, unmount } = render(
      <ObservabilityGroups buckets={buckets} groupBy={['date']} loading={false} />,
    );
    const xs = dotXs(container);
    const hits = [...container.querySelectorAll<SVGRectElement>('rect[class*="date-hit"]')];
    expect(hits).toHaveLength(2);
    // 命中区必须盖住各自数据点的 x（首点 6 在 [0,50]，末点 94 在 [50,100]）
    hits.forEach((rect, index) => {
      const left = Number(rect.getAttribute('x'));
      const width = Number(rect.getAttribute('width'));
      expect(xs[index]).toBeGreaterThanOrEqual(left);
      expect(xs[index]).toBeLessThanOrEqual(left + width);
    });
    // window.t mock 返回 key+params JSON；日期经 formatAxisDate 成本地短格式（en → Aug 23）。
    // React 的 onMouseEnter/onMouseLeave 委托自 mouseover/mouseout，须用 mouseOver/mouseOut 触发。
    fireEvent.mouseOver(hits[1]);
    const tooltip = await screen.findByText(/datePointTooltip/);
    expect(tooltip.textContent).toContain('Aug 23');
    expect(tooltip.textContent).toContain('68');
    fireEvent.mouseOut(hits[1]);
    await waitFor(() => expect(screen.queryByText(/datePointTooltip/)).toBeNull());
    unmount();
    cleanup();
  });
});

afterEach(() => {
  cleanup();
});
