// @vitest-environment jsdom

import { describe, expect, it } from 'vitest';
import { sessionCacheHitPercent } from '../../components/input/use-session-cache-rate';

// 会话级缓存命中率口径：缓存读取 ÷ 计费输入总量（与逐轮胶囊一致）。
// 无事实 / 无观测投影 / 分母缺失一律 null——行不渲染，绝不渲染 0。

describe('sessionCacheHitPercent', () => {
  it('computes cacheRead ÷ billed input', () => {
    expect(sessionCacheHitPercent({
      usageAggregateAvailability: 'complete',
      inputTokens: 1000,
      cacheReadTokens: 800,
    })).toBe(80);
  });

  it('accepts partial aggregates (some calls lack usage facts)', () => {
    expect(sessionCacheHitPercent({
      usageAggregateAvailability: 'partial',
      inputTokens: 800,
      cacheReadTokens: 200,
    })).toBe(25);
  });

  it('returns null without usage facts or without accounting projection', () => {
    for (const availability of ['unknown', 'corrupt', 'projection_unavailable']) {
      expect(sessionCacheHitPercent({
        usageAggregateAvailability: availability,
        inputTokens: 1000,
        cacheReadTokens: 800,
      })).toBeNull();
    }
  });

  it('returns null on missing or non-positive denominators', () => {
    expect(sessionCacheHitPercent({
      usageAggregateAvailability: 'complete',
      inputTokens: null,
      cacheReadTokens: 800,
    })).toBeNull();
    expect(sessionCacheHitPercent({
      usageAggregateAvailability: 'complete',
      inputTokens: 0,
      cacheReadTokens: 0,
    })).toBeNull();
    expect(sessionCacheHitPercent({
      usageAggregateAvailability: 'complete',
      cacheReadTokens: 800,
    })).toBeNull();
  });

  it('clamps the ratio into 0..100', () => {
    expect(sessionCacheHitPercent({
      usageAggregateAvailability: 'complete',
      inputTokens: 100,
      cacheReadTokens: 200,
    })).toBe(100);
    expect(sessionCacheHitPercent({
      usageAggregateAvailability: 'complete',
      inputTokens: 100,
      cacheReadTokens: 0,
    })).toBe(0);
  });
});
