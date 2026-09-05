/**
 * useTurnUsageStats — 按轮次窗口拉取 observability 用量并聚合成胶囊数据。
 *
 * 查询模式参考 settings/tabs/observability/model-observability-actions.ts 的
 * observabilityJson()（POST JSON；200 内嵌业务错误按失败处理），请求走聊天区
 * 惯用的 lingxiFetch。filter = sessionPath + since/until（账本口径：
 * since inclusive / until exclusive，绑定 started_at）。任何失败（网络、
 * 结构不符、无 usage 事实）一律返回 null——胶囊整体不渲染，绝不渲染 0。
 */

import { useEffect, useState } from 'react';
import type { ModelObservabilityCallListItem } from '../../../../../shared/model-observability-api-contract.ts';
import { lingxiFetch } from '../../hooks/use-hana-fetch';
import { aggregateTurnUsage, type TurnUsageStats, type TurnUsageWindow } from './turn-usage';

/** 单轮调用上限：一轮内的模型调用（含工具轮）远达不到该值，兜底防全量拉取。 */
const TURN_CALL_LIMIT = 200;

export interface TurnUsageStatsQuery {
  sessionPath: string;
  /** 轮次窗口（since/until + 可选展示用时 runMs）；null = 不查询。 */
  window: TurnUsageWindow | null;
  /** 完成轮才发起查询。 */
  enabled: boolean;
}

function iso(ms: number): string {
  return new Date(ms).toISOString();
}

function parseCallsPayload(data: unknown): ModelObservabilityCallListItem[] | null {
  if (!data || typeof data !== 'object' || Array.isArray(data)) return null;
  const calls = (data as { calls?: unknown }).calls;
  if (!Array.isArray(calls)) return null;
  return calls as ModelObservabilityCallListItem[];
}

export function useTurnUsageStats({ sessionPath, window, enabled }: TurnUsageStatsQuery): TurnUsageStats | null {
  const [stats, setStats] = useState<TurnUsageStats | null>(null);
  const startedAt = window?.startedAt ?? 0;
  const completedAt = window?.completedAt ?? 0;

  useEffect(() => {
    if (!enabled || !window) {
      setStats(null);
      return;
    }
    let cancelled = false;
    setStats(null);
    void (async () => {
      try {
        const res = await lingxiFetch('/api/model-observability/query/calls', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            filter: {
              sessionPath,
              since: iso(window.startedAt),
              until: iso(window.completedAt),
            },
            limit: TURN_CALL_LIMIT,
          }),
          throwOnHttpError: true,
        });
        const data: unknown = await res.json();
        if (cancelled) return;
        // 200 内嵌业务错误边界与 observabilityJson 对齐。
        if (data && typeof data === 'object' && !Array.isArray(data) && 'error' in data && (data as { error?: unknown }).error) {
          return;
        }
        const calls = parseCallsPayload(data);
        if (!calls) return;
        setStats(aggregateTurnUsage(calls, window));
      } catch {
        // 静默：无数据/请求失败 → 无胶囊（数据真实 > 覆盖面）。
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [completedAt, enabled, sessionPath, startedAt, window]);

  return stats;
}
