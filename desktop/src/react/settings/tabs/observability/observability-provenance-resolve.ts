/**
 * observability-provenance-resolve.ts — Semantic Locator 解析（Phase 9
 * §七十二～七十六）。
 *
 * **locator-only 纪律（§七十六 绝对红线）**：UI 只消费 provenance 里的
 * locator（root + path + UTF-16 span），从**同一 payload** 里按位置取值。
 * 绝对禁止内容搜索反推（在 payload JSON 里搜某段文字猜它属于 persona）。
 *
 * span 语义与 Phase 5 契约一致：JavaScript String 的 UTF-16 code unit
 * 闭开区间 [start, end)，即 String.prototype.slice(start, end)。代理对
 * （emoji 等）占 2 个 code unit——slice 语义天然正确。
 *
 * 解析结果三态（诚实，绝不伪造）：
 *   resolved   位置与 span 均有效，给出 slice 出的原文片段
 *   structural locator 指向了值但没有精确 span（identity-only/structural）——
 *              可以说「是什么」，不展示正文切片
 *   unavailable locator 在 payload 中落不到值（形状漂移/越界）——明说，不猜
 */
import type {
  ModelSemanticInputProvenance,
  ProviderPayloadLocator,
  SemanticInputLocator,
} from '../../../../../../shared/model-observability-api-contract.ts';

export type SemanticLocatorResolution =
  | { status: 'resolved'; text: string; containerPreview?: string }
  | { status: 'structural'; valueKind: string }
  | { status: 'unavailable'; reason: 'root_missing' | 'path_missing' | 'span_out_of_range' | 'not_text' };

function kindOfValue(value: unknown): string {
  if (value === null) return 'null';
  if (Array.isArray(value)) return 'array';
  return typeof value;
}

function navigatePath(root: unknown, path: Array<number | string> | undefined): { found: boolean; value: unknown } {
  let current: unknown = root;
  if (!path) return { found: true, value: current };
  for (const item of path) {
    if (Array.isArray(current) && typeof item === 'number') {
      if (item < 0 || item >= current.length) return { found: false, value: undefined };
      current = current[item];
    } else if (current !== null && typeof current === 'object' && typeof item === 'string') {
      if (!(item in (current as Record<string, unknown>))) return { found: false, value: undefined };
      current = (current as Record<string, unknown>)[item];
    } else {
      return { found: false, value: undefined };
    }
  }
  return { found: true, value: current };
}

/**
 * 从 semantic_request payload（sanitized 副本形状 {systemPrompt, messages,
 * tools, input, parameters, …}）按 locator 解析 section 内容。
 * payload 为 null（opaque/unavailable）时直接 unavailable。
 */
export function resolveSemanticLocator(
  payload: unknown,
  locator: SemanticInputLocator,
): SemanticLocatorResolution {
  if (payload === null || payload === undefined || typeof payload !== 'object' || Array.isArray(payload)) {
    return { status: 'unavailable', reason: 'root_missing' };
  }
  const container = (payload as Record<string, unknown>)[locator.root];
  if (container === undefined) return { status: 'unavailable', reason: 'root_missing' };
  const { found, value } = navigatePath(container, locator.path);
  if (!found) return { status: 'unavailable', reason: 'path_missing' };

  if (locator.span === null || locator.span === undefined) {
    // identity-only / 非 span 寻址根：诚实 structural，不展示正文切片。
    return { status: 'structural', valueKind: kindOfValue(value) };
  }
  if (typeof value !== 'string') {
    return { status: 'unavailable', reason: 'not_text' };
  }
  const { start, end } = locator.span;
  if (!Number.isInteger(start) || !Number.isInteger(end) || start < 0 || end < start || end > value.length) {
    return { status: 'unavailable', reason: 'span_out_of_range' };
  }
  // UTF-16 [start, end)：slice 语义与 Phase 5 契约一致（emoji 代理对安全）。
  return { status: 'resolved', text: value.slice(start, end), containerPreview: undefined };
}

/** providerLocator → provider request body（transport.body）的同纪律解析。 */
export function resolveProviderLocator(
  providerRequestPayload: unknown,
  locator: ProviderPayloadLocator,
): SemanticLocatorResolution {
  if (providerRequestPayload === null || typeof providerRequestPayload !== 'object' || Array.isArray(providerRequestPayload)) {
    return { status: 'unavailable', reason: 'root_missing' };
  }
  const transport = (providerRequestPayload as Record<string, unknown>).transport;
  const body = transport && typeof transport === 'object' && !Array.isArray(transport)
    ? (transport as Record<string, unknown>).body
    : undefined;
  if (body === undefined) return { status: 'unavailable', reason: 'root_missing' };
  const { found, value } = navigatePath(body, locator.path);
  if (!found) return { status: 'unavailable', reason: 'path_missing' };
  if (locator.span === null || locator.span === undefined) {
    return { status: 'structural', valueKind: kindOfValue(value) };
  }
  if (typeof value !== 'string') return { status: 'unavailable', reason: 'not_text' };
  const { start, end } = locator.span;
  if (!Number.isInteger(start) || !Number.isInteger(end) || start < 0 || end < start || end > value.length) {
    return { status: 'unavailable', reason: 'span_out_of_range' };
  }
  return { status: 'resolved', text: value.slice(start, end) };
}

/** provenance wire 形状的轻量守卫（payload 是 unknown，UI 不盲信）。 */
export function asSemanticInputProvenance(value: unknown): ModelSemanticInputProvenance | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  const record = value as Record<string, unknown>;
  if (typeof record.inputShape !== 'string' || !Array.isArray(record.sections)) return null;
  return value as ModelSemanticInputProvenance;
}
