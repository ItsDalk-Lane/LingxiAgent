/**
 * model-observability-format.ts — Observatory 展示格式化（Phase 9 §一百五十三）。
 *
 * 从 legacy usage-ledger-model.ts 迁移的纯格式化器（formatCompactNumber /
 * formatCost / formatTime 语义），加上观测专用的新格式化器。迁移时按本轮
 * 契约修正：
 *   - costTotal=null 显示 "—"（未知），0 显示 "$0.00"（真实零值）——null≠0（§三十三）。
 *   - 小成本保精度：$0.0007 不四舍五入成 $0.00（§一百五十三）。
 *   - duration 人性化：850ms / 2.4s / 1m 12s（§一百五十三）。
 *   - token 紧凑格式（1.3M）+ 精确 tooltip（1,250,431）由 formatNumber 承担。
 *
 * 本文件纯函数、无 React/网络依赖，i18n 文案不在这里（调用方给 label）。
 */

const numberFormat = new Intl.NumberFormat();

/** 精确千分位（tooltip 用）：1,250,431。 */
export function formatNumber(value: number | null | undefined): string {
  if (typeof value !== 'number' || !Number.isFinite(value)) return '—';
  return numberFormat.format(Math.round(value));
}

function trimUnitNumber(value: number): string {
  const fixed = value.toFixed(1);
  return fixed.endsWith('.0') ? fixed.slice(0, -2) : fixed;
}

/** 紧凑格式：1.3M / 12K / 850。 */
export function formatCompactNumber(value: number | null | undefined): string {
  if (typeof value !== 'number' || !Number.isFinite(value)) return '—';
  const rounded = Math.round(value);
  const abs = Math.abs(rounded);
  if (abs >= 1_000_000) return `${trimUnitNumber(rounded / 1_000_000)}M`;
  if (abs >= 1_000) return `${trimUnitNumber(rounded / 1_000)}K`;
  return numberFormat.format(rounded);
}

/** null（未知/不适用）→ "—"；否则 0..1 → 百分比整数。 */
export function formatPercent(value: number | null | undefined): string {
  if (typeof value !== 'number' || !Number.isFinite(value)) return '—';
  return `${Math.round(value * 100)}%`;
}

/**
 * 成本：null → "—"（§三十三：绝不显示成 $0.00 伪装已知）；真实 0 → "$0.00"；
 * 小于一分钱保 4 位小数（$0.0007，§一百五十三）。
 */
export function formatCost(value: number | null | undefined): string {
  if (typeof value !== 'number' || !Number.isFinite(value)) return '—';
  if (value === 0) return '$0.00';
  if (Math.abs(value) < 0.01) return `$${value.toFixed(4)}`;
  return `$${value.toFixed(2)}`;
}

/** 时长人性化（§一百五十三）：850ms / 2.4s / 1m 12s / 1h 5m。 */
export function formatDurationMs(value: number | null | undefined): string {
  if (typeof value !== 'number' || !Number.isFinite(value) || value < 0) return '—';
  const ms = Math.round(value);
  if (ms < 1000) return `${ms}ms`;
  const seconds = ms / 1000;
  if (seconds < 60) return `${trimUnitNumber(seconds)}s`;
  const wholeMinutes = Math.floor(seconds / 60);
  const remainSeconds = Math.round(seconds % 60);
  if (wholeMinutes < 60) {
    return remainSeconds > 0 ? `${wholeMinutes}m ${remainSeconds}s` : `${wholeMinutes}m`;
  }
  const hours = Math.floor(wholeMinutes / 60);
  const remainMinutes = wholeMinutes % 60;
  return remainMinutes > 0 ? `${hours}h ${remainMinutes}m` : `${hours}h`;
}

const localDateTimeFormat = new Intl.DateTimeFormat(undefined, {
  month: '2-digit',
  day: '2-digit',
  hour: '2-digit',
  minute: '2-digit',
});

const localFullDateTimeFormat = new Intl.DateTimeFormat(undefined, {
  year: 'numeric',
  month: '2-digit',
  day: '2-digit',
  hour: '2-digit',
  minute: '2-digit',
  second: '2-digit',
});

const localAxisDateFormat = new Intl.DateTimeFormat(undefined, {
  month: 'short',
  day: 'numeric',
});

/** 折线图 X 轴标签：本地「8月22日 / Aug 22」。纯日期串按本地零点解析，避免 UTC 偏移串日；不合法则原样返回。 */
export function formatAxisDate(value: string | null | undefined): string {
  if (!value) return '—';
  const normalized = /^\d{4}-\d{2}-\d{2}$/.test(value) ? `${value}T00:00:00` : value;
  const time = Date.parse(normalized);
  if (!Number.isFinite(time)) return value;
  return localAxisDateFormat.format(new Date(time));
}

/** 列表时间：本地 MM-DD HH:mm；完整 ISO 由调用方放 title/tooltip（§一百五十三）。 */
export function formatLocalDateTime(value: string | null | undefined): string {
  if (!value) return '—';
  const time = Date.parse(value);
  if (!Number.isFinite(time)) return '—';
  return localDateTimeFormat.format(new Date(time));
}

/** 详情/Inspector 用的带秒完整本地时间。 */
export function formatLocalFullDateTime(value: string | null | undefined): string {
  if (!value) return '—';
  const time = Date.parse(value);
  if (!Number.isFinite(time)) return '—';
  return localFullDateTimeFormat.format(new Date(time));
}

/** tooltip 用 ISO 原文（不合法时原样返回，绝不静默吞）。 */
export function isoTooltip(value: string | null | undefined): string | undefined {
  return value ?? undefined;
}

/**
 * 短 ID：列表展示前 8 位 + 省略号；完整值放 title（§四十一）。
 * 不做语义解析（callId/traceId 的编码结构不属于 UI 契约）。
 */
export function shortId(value: string | null | undefined): string {
  if (!value) return '—';
  return value.length > 9 ? `${value.slice(0, 8)}…` : value;
}
