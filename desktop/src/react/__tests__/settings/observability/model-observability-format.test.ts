/**
 * Phase 9 展示格式化测试 — null≠0 语义（§三十三）与小成本保精度（§一百五十三）。
 */
import { describe, expect, it } from 'vitest';
import {
  formatCompactNumber,
  formatCost,
  formatDurationMs,
  formatLocalDateTime,
  formatNumber,
  formatPercent,
  isoTooltip,
  shortId,
} from '../../../settings/tabs/observability/model-observability-format';

describe('formatCost (null ≠ 0, §三十三)', () => {
  it('null/undefined → em dash, never a fake $0.00', () => {
    expect(formatCost(null)).toBe('—');
    expect(formatCost(undefined)).toBe('—');
    expect(formatCost(Number.NaN)).toBe('—');
  });

  it('real zero renders $0.00', () => {
    expect(formatCost(0)).toBe('$0.00');
  });

  it('sub-cent keeps 4 decimals ($0.0007, §一百五十三)', () => {
    expect(formatCost(0.0007)).toBe('$0.0007');
    expect(formatCost(0.009)).toBe('$0.0090');
  });

  it('normal amounts use 2 decimals', () => {
    expect(formatCost(1.5)).toBe('$1.50');
    expect(formatCost(123.456)).toBe('$123.46');
  });
});

describe('formatDurationMs', () => {
  it('renders human tiers', () => {
    expect(formatDurationMs(0)).toBe('0ms');
    expect(formatDurationMs(850)).toBe('850ms');
    expect(formatDurationMs(2400)).toBe('2.4s');
    expect(formatDurationMs(72_000)).toBe('1m 12s');
    expect(formatDurationMs(60_000)).toBe('1m');
    expect(formatDurationMs(3_900_000)).toBe('1h 5m');
    expect(formatDurationMs(7_200_000)).toBe('2h');
  });

  it('null and negative → em dash', () => {
    expect(formatDurationMs(null)).toBe('—');
    expect(formatDurationMs(-5)).toBe('—');
  });
});

describe('numeric formatters', () => {
  it('formatNumber: exact grouping for tooltips', () => {
    expect(formatNumber(1250431)).toBe('1,250,431');
    expect(formatNumber(null)).toBe('—');
  });

  it('formatCompactNumber tiers', () => {
    expect(formatCompactNumber(1250431)).toBe('1.3M');
    expect(formatCompactNumber(12500)).toBe('12.5K');
    expect(formatCompactNumber(12000)).toBe('12K');
    expect(formatCompactNumber(850)).toBe('850');
    expect(formatCompactNumber(null)).toBe('—');
  });

  it('formatPercent: null → em dash, 0..1 → integer percent', () => {
    expect(formatPercent(null)).toBe('—');
    expect(formatPercent(0)).toBe('0%');
    expect(formatPercent(0.476)).toBe('48%');
    expect(formatPercent(1)).toBe('100%');
  });
});

describe('identifiers & timestamps', () => {
  it('shortId keeps first 8 chars with ellipsis beyond 9', () => {
    expect(shortId('abcdefghijk')).toBe('abcdefgh…');
    expect(shortId('123456789')).toBe('123456789');
    expect(shortId(null)).toBe('—');
  });

  it('formatLocalDateTime: missing/invalid → em dash', () => {
    expect(formatLocalDateTime(null)).toBe('—');
    expect(formatLocalDateTime('not-a-date')).toBe('—');
    expect(formatLocalDateTime('2026-08-22T08:00:00.000Z')).toMatch(/\d{2}[/\-.]\d{2},? \d{2}:\d{2}/);
  });

  it('isoTooltip passes the raw value through untouched', () => {
    expect(isoTooltip('2026-08-22T08:00:00.000Z')).toBe('2026-08-22T08:00:00.000Z');
    expect(isoTooltip(null)).toBeUndefined();
  });
});
