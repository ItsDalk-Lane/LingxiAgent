/**
 * useSessionCacheRate — 会话级缓存命中率（Context Ring 详情视图行）。
 *
 * 服务端一次聚合（POST /api/model-observability/query/aggregate，
 * filter = sessionPath，不拉明细），口径与逐轮用量胶囊一致：
 * cacheReadTokens ÷ inputTokens（账本 input_total = 未缓存输入
 * + 缓存读取 + 缓存写入，即 dsh 页脚同款「计费输入中由缓存 served 的份额」）。
 * 任何失败（网络、结构不符、无 usage 事实）一律返回 null——行整体不渲染，
 * 绝不渲染 0（数据真实 > 覆盖面）。
 */

import { useEffect, useState } from 'react';
import { lingxiFetch } from '../../hooks/use-hana-fetch';

export interface SessionCacheRateQuery {
  sessionPath: string | null;
  /** true 时才发起查询（详情视图打开时）。 */
  enabled: boolean;
  /** 变化时重查（轮次边界推进上下文用量即换新值）。 */
  refreshKey: number | null;
}

function parseOverall(data: unknown): Record<string, unknown> | null {
  if (!data || typeof data !== 'object' || Array.isArray(data)) return null;
  const overall = (data as { overall?: unknown }).overall;
  if (!overall || typeof overall !== 'object' || Array.isArray(overall)) return null;
  return overall as Record<string, unknown>;
}

function finiteNumber(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

/** 与逐轮胶囊同分母口径：缓存读取 ÷ 计费输入总量；分母非正不给值。 */
export function sessionCacheHitPercent(overall: Record<string, unknown>): number | null {
  const availability = overall.usageAggregateAvailability;
  if (availability !== 'complete' && availability !== 'partial') return null;
  const inputTokens = finiteNumber(overall.inputTokens);
  const cacheReadTokens = finiteNumber(overall.cacheReadTokens);
  if (inputTokens === null || cacheReadTokens === null || inputTokens <= 0) return null;
  return Math.min(100, Math.max(0, Math.round((cacheReadTokens / inputTokens) * 100)));
}

export function useSessionCacheRate({ sessionPath, enabled, refreshKey }: SessionCacheRateQuery): number | null {
  const [percent, setPercent] = useState<number | null>(null);

  useEffect(() => {
    if (!enabled || !sessionPath) {
      setPercent(null);
      return;
    }
    let cancelled = false;
    void (async () => {
      try {
        const res = await lingxiFetch('/api/model-observability/query/aggregate', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ filter: { sessionPath } }),
          throwOnHttpError: true,
        });
        const data: unknown = await res.json();
        if (cancelled) return;
        if (data && typeof data === 'object' && !Array.isArray(data) && 'error' in data && (data as { error?: unknown }).error) {
          return;
        }
        const overall = parseOverall(data);
        if (!overall) return;
        setPercent(sessionCacheHitPercent(overall));
      } catch {
        // 静默：无数据/请求失败 → 无该行（数据真实 > 覆盖面）。
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [enabled, refreshKey, sessionPath]);

  return percent;
}
