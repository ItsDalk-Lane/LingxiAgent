/**
 * model-observability-preferences.ts — Observability 用户 preference 的
 * canonical normalizer（Phase 8 §五十一～五十三）。
 *
 * 单一事实源：PreferencesManager getter / engine startup / persistence
 * coordinator 全部经 normalizeModelObservabilityPreferences()——禁止
 * PreferencesManager 一套 default、server startup 又一套、coordinator 第三套。
 *
 * 安全默认（§六十一）：enabled=false；用户开启 Observatory 后
 * persistTraceMetadata=true、persistPayloads=false、persistBlobs=false——
 * Payload/Blob 必须额外显式 opt-in，打开 Usage Observatory ≠ 自动永久保存
 * 所有 Prompt/Response。
 *
 * 持久化单位是 days（用户语义，§五十二）；转换为 persistence policy 时才
 * ×DAY_MS（内部毫秒不进 preferences.json）。
 */

import type { ModelObservabilityPersistencePolicy } from "./model-observability-persistence.ts";

const DAY_MS = 24 * 60 * 60 * 1000;

export type ModelObservabilityRetentionDays = {
  traceDays: number;
  payloadDays: number;
  blobDays: number;
};

export type ModelObservabilityUserPreference = {
  enabled: boolean;
  persistTraceMetadata: boolean;
  persistPayloads: boolean;
  persistBlobs: boolean;
  retention: ModelObservabilityRetentionDays;
};

/** 用户可见默认（§六十一；与 Phase 7 safe fallback retention 对齐）。 */
export const DEFAULT_MODEL_OBSERVABILITY_PREFERENCE: ModelObservabilityUserPreference = {
  enabled: false,
  persistTraceMetadata: true,
  persistPayloads: false,
  persistBlobs: false,
  retention: { traceDays: 180, payloadDays: 30, blobDays: 30 },
};

/** retention days 闭集边界（1..3650，防 0/负数/千年值）。 */
const MIN_DAYS = 1;
const MAX_DAYS = 3650;

function daysOrNull(value: unknown): number | null {
  const n = Number(value);
  if (!Number.isFinite(n) || n < MIN_DAYS || n > MAX_DAYS) return null;
  return Math.floor(n);
}

function daysOrFallback(value: unknown, fallback: number): number {
  return daysOrNull(value) ?? fallback;
}

/** canonical normalizer：任意输入（含损坏/陌生字段）→ 合法 preference。 */
export function normalizeModelObservabilityPreferences(input: unknown): ModelObservabilityUserPreference {
  const source = input && typeof input === "object" && !Array.isArray(input)
    ? input as Record<string, unknown>
    : {};
  const enabled = source.enabled === true;
  const defaults = DEFAULT_MODEL_OBSERVABILITY_PREFERENCE;
  const persistPayloads = enabled && source.persistPayloads === true;
  const persistTraceMetadata = enabled ? source.persistTraceMetadata !== false : false;
  const retentionSource = source.retention && typeof source.retention === "object" && !Array.isArray(source.retention)
    ? source.retention as Record<string, unknown>
    : {};
  return {
    enabled,
    persistTraceMetadata,
    persistPayloads,
    persistBlobs: enabled && persistPayloads && source.persistBlobs === true,
    retention: {
      traceDays: daysOrFallback(retentionSource.traceDays, defaults.retention.traceDays),
      payloadDays: daysOrFallback(retentionSource.payloadDays, defaults.retention.payloadDays),
      blobDays: daysOrFallback(retentionSource.blobDays, defaults.retention.blobDays),
    },
  };
}

/** preference → persistence policy（retention days → ms；结构开关语义一致）。 */
export function modelObservabilityPreferenceToPolicy(
  preference: ModelObservabilityUserPreference,
): ModelObservabilityPersistencePolicy {
  const normalized = normalizeModelObservabilityPreferences(preference);
  return {
    enabled: normalized.enabled,
    persistTraceMetadata: normalized.persistTraceMetadata,
    persistPayloads: normalized.persistPayloads,
    persistBlobs: normalized.persistBlobs,
    retention: {
      traceMaxAgeMs: normalized.retention.traceDays * DAY_MS,
      payloadMaxAgeMs: normalized.retention.payloadDays * DAY_MS,
      blobMaxAgeMs: normalized.retention.blobDays * DAY_MS,
      maxTraceRows: null,
      maxPayloadBytes: null,
      maxBlobBytes: null,
    },
  };
}

/** persistence policy → preference（settings API 返回 desired 状态用）。 */
export function modelObservabilityPolicyToPreference(
  policy: ModelObservabilityPersistencePolicy,
): ModelObservabilityUserPreference {
  return normalizeModelObservabilityPreferences(policy);
}
