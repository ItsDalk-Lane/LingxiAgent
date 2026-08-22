/**
 * model-observability-filter.ts — 统一 Filter 状态与 wire 转换（Phase 9
 * §十三～十六、§二十五）。
 *
 * 纯函数模块（无 React）：Filter Bar / Metrics / Groups / Ledger / Export 共享
 * 同一份 canonical appliedFilter（§十四），本文件是它的类型、默认值、
 * → wire ModelObservabilityCallFilterInput 的唯一转换点。
 *
 * 关键语义：
 *   - since inclusive / until exclusive（§四十四，全接口统一）。
 *   - dateBucket.utcOffsetMinutes 东半球为正；JS Date.getTimezoneOffset()
 *     符号相反（西为正）——localUtcOffsetMinutes() 是唯一换算点（§十六）。
 *   - 空数组/空字符串不进入 wire（少发字段 = 不过滤该维度）。
 */
import type {
  ModelObservabilityCallFilterInput,
  ModelObservabilityGroupByDimension,
} from '../../../../../../shared/model-observability-api-contract.ts';

export type ObservabilityDatePreset = '24h' | '7d' | '30d' | 'custom' | 'all';

export const OBSERVABILITY_DATE_PRESETS: readonly ObservabilityDatePreset[] = [
  '24h',
  '7d',
  '30d',
  'custom',
  'all',
];

/** 多值过滤字段的 UI 状态键（→ wire 字段映射见 MULTI_FIELD_WIRE）。 */
export type ObservabilityMultiFilterKey =
  | 'providers'
  | 'modelIds'
  | 'categories'
  | 'terminalStatuses'
  | 'operations'
  | 'callPurposes'
  | 'attributionKinds'
  | 'inputShapes'
  | 'provenancePrecisions'
  | 'payloadAvailabilities';

/** 精确 ID 过滤（高基数维度不用下拉，输入精确值；§二十六）。 */
export type ObservabilityExactFilterKey = 'sessionId' | 'conversationId' | 'agentId' | 'taskId';

export const OBSERVABILITY_EXACT_FILTER_KEYS: readonly ObservabilityExactFilterKey[] = [
  'sessionId',
  'conversationId',
  'agentId',
  'taskId',
];

export type ObservabilityFilterState = {
  datePreset: ObservabilityDatePreset;
  /** datetime-local 输入的 applied 值（'' = 未设置）；custom preset 专用。 */
  customSince: string;
  customUntil: string;
  providers: string[];
  modelIds: string[];
  /** category ≡ subsystem（§十九 wire alias，UI 只保留一份状态）。 */
  categories: string[];
  terminalStatuses: string[];
  operations: string[];
  callPurposes: string[];
  attributionKinds: string[];
  sessionId: string;
  conversationId: string;
  agentId: string;
  taskId: string;
  inputShapes: string[];
  provenancePrecisions: string[];
  payloadAvailabilities: string[];
  /** null = 不过滤；true/false = 显式过滤（§二十四 advanced filters）。 */
  interruptedByRestart: boolean | null;
  hasPayload: boolean | null;
};

/** 默认：Last 7 Days（§十五）。 */
export const DEFAULT_OBSERVABILITY_FILTER: ObservabilityFilterState = {
  datePreset: '7d',
  customSince: '',
  customUntil: '',
  providers: [],
  modelIds: [],
  categories: [],
  terminalStatuses: [],
  operations: [],
  callPurposes: [],
  attributionKinds: [],
  sessionId: '',
  conversationId: '',
  agentId: '',
  taskId: '',
  inputShapes: [],
  provenancePrecisions: [],
  payloadAvailabilities: [],
  interruptedByRestart: null,
  hasPayload: null,
};

export const DEFAULT_OBSERVABILITY_GROUP_BY: readonly ModelObservabilityGroupByDimension[] = ['date'];

/* ── 日期范围（since inclusive / until exclusive）──────────────────────── */

const PRESET_DURATION_MS: Record<Exclude<ObservabilityDatePreset, 'custom' | 'all'>, number> = {
  '24h': 24 * 60 * 60 * 1000,
  '7d': 7 * 24 * 60 * 60 * 1000,
  '30d': 30 * 24 * 60 * 60 * 1000,
};

function datetimeLocalToIso(value: string): string | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const time = new Date(trimmed).getTime();
  if (!Number.isFinite(time)) return null;
  return new Date(time).toISOString();
}

/**
 * 当前 filter 的日期范围。preset → since=now-时长、until=null（开口）；
 * custom → datetime-local 输入转 ISO（until 保持 exclusive 语义，§四十四）；
 * all → 无边界。
 */
export function dateRangeForState(
  state: Pick<ObservabilityFilterState, 'datePreset' | 'customSince' | 'customUntil'>,
  now: Date = new Date(),
): { since: string | null; until: string | null } {
  if (state.datePreset === 'all') return { since: null, until: null };
  if (state.datePreset === 'custom') {
    return {
      since: datetimeLocalToIso(state.customSince),
      until: datetimeLocalToIso(state.customUntil),
    };
  }
  const duration = PRESET_DURATION_MS[state.datePreset];
  return { since: new Date(now.getTime() - duration).toISOString(), until: null };
}

/**
 * dateBucket.utcOffsetMinutes：API 约定东半球为正（分钟）；JS
 * Date.getTimezoneOffset() 西半球为正——符号必须翻转（§十六）。
 */
export function localUtcOffsetMinutes(date: Date = new Date()): number {
  return -date.getTimezoneOffset();
}

/* ── → wire filter input ──────────────────────────────────────────────── */

const MULTI_FIELD_WIRE: Record<ObservabilityMultiFilterKey, keyof ModelObservabilityCallFilterInput> = {
  providers: 'provider',
  modelIds: 'modelId',
  categories: 'subsystem',
  terminalStatuses: 'terminalStatus',
  operations: 'operation',
  callPurposes: 'callPurpose',
  attributionKinds: 'attributionKind',
  inputShapes: 'inputShape',
  provenancePrecisions: 'provenancePrecision',
  payloadAvailabilities: 'payloadAvailability',
};

const EXACT_FIELD_WIRE: Record<ObservabilityExactFilterKey, keyof ModelObservabilityCallFilterInput> = {
  sessionId: 'sessionId',
  conversationId: 'conversationId',
  agentId: 'agentId',
  taskId: 'taskId',
};

/**
 * appliedFilter → wire ModelObservabilityCallFilterInput。Metrics/Groups/Ledger/
 * Export 全部经此转换（§十四 单一 canonical appliedFilter）。空维度不输出。
 */
export function buildCallFilterInput(
  state: ObservabilityFilterState,
  now: Date = new Date(),
): ModelObservabilityCallFilterInput {
  const out: ModelObservabilityCallFilterInput = {};
  const { since, until } = dateRangeForState(state, now);
  if (since) out.since = since;
  if (until) out.until = until;
  for (const key of Object.keys(MULTI_FIELD_WIRE) as ObservabilityMultiFilterKey[]) {
    const values = state[key];
    if (values.length > 0) out[MULTI_FIELD_WIRE[key]] = [...values];
  }
  for (const key of OBSERVABILITY_EXACT_FILTER_KEYS) {
    const value = state[key].trim();
    if (value) out[EXACT_FIELD_WIRE[key]] = [value];
  }
  if (state.interruptedByRestart !== null) out.interruptedByRestart = state.interruptedByRestart;
  if (state.hasPayload !== null) out.hasPayload = state.hasPayload;
  return out;
}

/** groupBy 含 date 时的 bucket 参数（时区偏移唯一换算点）。 */
export function dateBucketForGroupBy(
  groupBy: readonly ModelObservabilityGroupByDimension[],
): { bucket: 'day'; utcOffsetMinutes: number } | undefined {
  if (!groupBy.includes('date')) return undefined;
  return { bucket: 'day', utcOffsetMinutes: localUtcOffsetMinutes() };
}

/* ── Filter Chips（§二十五：单独可删 + Clear All）───────────────────────── */

export type ObservabilityFilterChip =
  | { id: 'date'; kind: 'date' }
  | { id: string; kind: 'multi'; field: ObservabilityMultiFilterKey; value: string }
  | { id: string; kind: 'exact'; field: ObservabilityExactFilterKey }
  | { id: string; kind: 'flag'; field: 'interruptedByRestart' | 'hasPayload'; value: boolean };

/** 当前 applied filter 的活动 chip 列表（顺序稳定；date 只在非默认时出现）。 */
export function listActiveFilterChips(state: ObservabilityFilterState): ObservabilityFilterChip[] {
  const chips: ObservabilityFilterChip[] = [];
  if (state.datePreset !== DEFAULT_OBSERVABILITY_FILTER.datePreset
    || state.customSince !== '' || state.customUntil !== '') {
    chips.push({ id: 'date', kind: 'date' });
  }
  for (const key of Object.keys(MULTI_FIELD_WIRE) as ObservabilityMultiFilterKey[]) {
    for (const value of state[key]) {
      chips.push({ id: `multi:${key}:${value}`, kind: 'multi', field: key, value });
    }
  }
  for (const key of OBSERVABILITY_EXACT_FILTER_KEYS) {
    if (state[key].trim()) chips.push({ id: `exact:${key}`, kind: 'exact', field: key });
  }
  if (state.interruptedByRestart !== null) {
    chips.push({ id: 'flag:interruptedByRestart', kind: 'flag', field: 'interruptedByRestart', value: state.interruptedByRestart });
  }
  if (state.hasPayload !== null) {
    chips.push({ id: 'flag:hasPayload', kind: 'flag', field: 'hasPayload', value: state.hasPayload });
  }
  return chips;
}

/** 删除单个 chip → 新 state（不可变更新；调用方 setState）。 */
export function removeFilterChip(
  state: ObservabilityFilterState,
  chip: ObservabilityFilterChip,
): ObservabilityFilterState {
  switch (chip.kind) {
    case 'date':
      return { ...state, datePreset: DEFAULT_OBSERVABILITY_FILTER.datePreset, customSince: '', customUntil: '' };
    case 'multi':
      return { ...state, [chip.field]: state[chip.field].filter((value) => value !== chip.value) };
    case 'exact':
      return { ...state, [chip.field]: '' };
    case 'flag':
      return { ...state, [chip.field]: null };
  }
}

/** Clear All：回到默认 filter（默认含 7d preset，§十五）。 */
export function clearAllFilterChips(): ObservabilityFilterState {
  return { ...DEFAULT_OBSERVABILITY_FILTER };
}
