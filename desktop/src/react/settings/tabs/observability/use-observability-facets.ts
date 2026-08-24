/**
 * use-observability-facets.ts — facet 选项懒加载（Phase 9 §二十六）。
 *
 * facet 选项来自 aggregate API（groupBy 单维），绝不硬编码领域值列表；
 * 只在对应下拉打开时请求（active=false 不发请求）。不做前端大 DB——
 * 缓存以 (dimension, filter 签名) 为键，签名随 appliedFilter 变化自动失效。
 * 高基数维度（session/task/conversation/agent）不走 facet（§二十六：精确 ID
 * 输入 + 行上「按此值过滤」）。
 */
import { useEffect, useRef, useState } from 'react';
import type {
  ModelObservabilityCallFilterInput,
  ModelObservabilityGroupByDimension,
} from '../../../../../../shared/model-observability-api-contract.ts';
import { queryObservabilityAggregate } from './model-observability-actions';

/** 支持 facet 下拉的维度（闭集之外的可枚举维度）。 */
export type ObservabilityFacetDimension = Extract<
  ModelObservabilityGroupByDimension,
  'provider' | 'model' | 'category' | 'operation' | 'callPurpose' | 'attributionKind'
>;

function facetValueOf(
  dimension: ObservabilityFacetDimension,
  values: Record<string, string | null | undefined>,
): string | null {
  if (dimension === 'model') {
    // wire 的 modelId 与 provider 是两个独立过滤字段：facet 只产出裸 modelId
    // （同名跨 provider 时过滤同时命中——诚实反映 wire 语义，不合成复合键）。
    const modelId = values.modelId;
    return typeof modelId === 'string' && modelId !== '' ? modelId : null;
  }
  const value = values[dimension];
  return typeof value === 'string' && value !== '' ? value : null;
}

/**
 * signature 的时间精度：毫秒 → 分钟。
 *
 * 相对 preset（7d/24h/30d）的 since 由调用方每次渲染按当前时刻重算，毫秒级
 * 漂移；若直接 stringify 进缓存键，同一逻辑筛选会在每次父组件渲染时 miss
 * 缓存并 abort+refetch，下拉空态文字在「加载选项…/无可选值」间高频闪烁
 * （§二十六 缓存键随 appliedFilter 失效的本意是筛选变了才失效）。facet
 * 只枚举维度可选值，分钟级窗口差不改变选项集；查询本身仍用原始 filter。
 */
const SIGNATURE_TIME_PRECISION_MS = 60_000;

function roundIsoToSignaturePrecision(value: string): string {
  const time = new Date(value).getTime();
  if (!Number.isFinite(time)) return value;
  return new Date(Math.floor(time / SIGNATURE_TIME_PRECISION_MS) * SIGNATURE_TIME_PRECISION_MS).toISOString();
}

function facetSignature(filter: ModelObservabilityCallFilterInput): string {
  if (filter.since === undefined && filter.until === undefined) return JSON.stringify(filter);
  const stable = { ...filter };
  if (typeof stable.since === 'string') stable.since = roundIsoToSignaturePrecision(stable.since);
  if (typeof stable.until === 'string') stable.until = roundIsoToSignaturePrecision(stable.until);
  return JSON.stringify(stable);
}

export function useObservabilityFacetOptions(
  dimension: ObservabilityFacetDimension,
  active: boolean,
  filter: ModelObservabilityCallFilterInput,
): { options: string[]; loading: boolean } {
  const [options, setOptions] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const cacheRef = useRef(new Map<string, string[]>());
  // filter 签名：请求内容与 filter 绑定，filter 变了缓存自然 miss。
  const signature = facetSignature(filter);

  useEffect(() => {
    if (!active) return;
    const cacheKey = `${dimension}:${signature}`;
    const cached = cacheRef.current.get(cacheKey);
    if (cached) {
      setOptions(cached);
      setLoading(false);
      return;
    }
    const controller = new AbortController();
    setLoading(true);
    queryObservabilityAggregate(
      { filter, groupBy: [dimension] },
      { signal: controller.signal },
    ).then((result) => {
      const values: string[] = [];
      for (const bucket of result.groups) {
        const value = facetValueOf(dimension, bucket.values as Record<string, string | null | undefined>);
        if (value !== null && !values.includes(value)) values.push(value);
      }
      values.sort();
      // 缓存有界：filter 每次变化都会产生新签名，不允许无限堆积。
      if (cacheRef.current.size >= 24) cacheRef.current.clear();
      cacheRef.current.set(cacheKey, values);
      setOptions(values);
      setLoading(false);
    }).catch((error: unknown) => {
      if ((error as { name?: unknown })?.name === 'AbortError') return;
      // facet 加载失败不阻断过滤：下拉显示空态，chips 仍可移除既有选择。
      setOptions([]);
      setLoading(false);
    });
    return () => controller.abort();
  }, [dimension, active, signature]);

  return { options, loading };
}
