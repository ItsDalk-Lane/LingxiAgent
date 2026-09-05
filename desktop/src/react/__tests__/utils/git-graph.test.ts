// @vitest-environment jsdom

import { describe, expect, it } from 'vitest';
import { computeGraphRows, graphLaneCount } from '../../utils/git-graph';

describe('computeGraphRows', () => {
  it('linear history stays on a single lane', () => {
    // 新→旧：C ← B ← A
    const rows = computeGraphRows([
      { hash: 'C', parents: ['B'] },
      { hash: 'B', parents: ['A'] },
      { hash: 'A', parents: [] },
    ]);
    expect(rows).toEqual([
      { nodeLane: 0, activeLanes: [0], mergeLanes: [], laneCount: 1 },
      { nodeLane: 0, activeLanes: [0], mergeLanes: [], laneCount: 1 },
      { nodeLane: 0, activeLanes: [], mergeLanes: [], laneCount: 1 },
    ]);
    expect(graphLaneCount(rows)).toBe(1);
  });

  it('merge commit opens a second lane with a curve, convergence releases it', () => {
    // 新→旧：M(合并 B+C) ← B ← C ← A
    const rows = computeGraphRows([
      { hash: 'M', parents: ['B', 'C'] },
      { hash: 'B', parents: ['A'] },
      { hash: 'C', parents: ['A'] },
      { hash: 'A', parents: [] },
    ]);
    expect(rows[0]).toEqual({ nodeLane: 0, activeLanes: [0, 1], mergeLanes: [1], laneCount: 2 });
    expect(rows[1]).toEqual({ nodeLane: 0, activeLanes: [0, 1], mergeLanes: [], laneCount: 2 });
    // C 在泳道 1：第一父 A 已被泳道 0 跟踪 → 会合曲线，泳道 1 释放
    expect(rows[2]).toEqual({ nodeLane: 1, activeLanes: [0], mergeLanes: [0], laneCount: 2 });
    expect(rows[3]).toEqual({ nodeLane: 0, activeLanes: [], mergeLanes: [], laneCount: 2 });
    expect(graphLaneCount(rows)).toBe(2);
  });

  it('second parent already tracked by another lane reuses it without a new lane', () => {
    // X ← M(合并 B+A)，A 先于 M 已在泳道上（限流截断场景的对偶）
    const rows = computeGraphRows([
      { hash: 'X', parents: ['B', 'A'] },
      { hash: 'A', parents: [] },
    ]);
    // X: 泳道0=B；第二父 A 不在泳道 → 新泳道1
    expect(rows[0]).toEqual({ nodeLane: 0, activeLanes: [0, 1], mergeLanes: [1], laneCount: 2 });
    // A: 节点在泳道1，无父 → 泳道1 结束；泳道0 仍跟踪 B（图底截断）
    expect(rows[1]).toEqual({ nodeLane: 1, activeLanes: [0], mergeLanes: [], laneCount: 2 });
  });

  it('handles empty input', () => {
    expect(computeGraphRows([])).toEqual([]);
    expect(graphLaneCount([])).toBe(0);
  });
});
