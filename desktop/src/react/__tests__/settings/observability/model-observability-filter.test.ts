/**
 * Phase 9 统一 filter 状态测试 — dateRangeForState / buildCallFilterInput /
 * chips 模型（§十三～十六、§二十五）。
 */
import { describe, expect, it } from 'vitest';
import {
  buildCallFilterInput,
  clearAllFilterChips,
  dateBucketForGroupBy,
  dateRangeForState,
  DEFAULT_OBSERVABILITY_FILTER,
  listActiveFilterChips,
  localUtcOffsetMinutes,
  removeFilterChip,
  type ObservabilityFilterState,
} from '../../../settings/tabs/observability/model-observability-filter';

const NOW = new Date('2026-08-22T08:00:00.000Z');

describe('dateRangeForState (since inclusive / until exclusive, §四十四)', () => {
  it('default preset is last 7 days (§十五)', () => {
    expect(DEFAULT_OBSERVABILITY_FILTER.datePreset).toBe('7d');
  });

  it('presets produce since=now-duration and open-ended until', () => {
    expect(dateRangeForState({ datePreset: '24h', customSince: '', customUntil: '' }, NOW)).toEqual({
      since: '2026-08-21T08:00:00.000Z',
      until: null,
    });
    expect(dateRangeForState({ datePreset: '7d', customSince: '', customUntil: '' }, NOW)).toEqual({
      since: '2026-08-15T08:00:00.000Z',
      until: null,
    });
    expect(dateRangeForState({ datePreset: '30d', customSince: '', customUntil: '' }, NOW)).toEqual({
      since: '2026-07-23T08:00:00.000Z',
      until: null,
    });
  });

  it('all → no bounds; custom converts datetime-local to ISO', () => {
    expect(dateRangeForState({ datePreset: 'all', customSince: '', customUntil: '' }, NOW)).toEqual({
      since: null, until: null,
    });
    expect(dateRangeForState({
      datePreset: 'custom',
      customSince: '2026-08-01T09:30',
      customUntil: '2026-08-02T09:30',
    }, NOW).since).toBe(new Date('2026-08-01T09:30').toISOString());
  });

  it('custom invalid/empty input yields null rather than a guess', () => {
    expect(dateRangeForState({ datePreset: 'custom', customSince: 'not-a-date', customUntil: '' }, NOW))
      .toEqual({ since: null, until: null });
  });
});

describe('localUtcOffsetMinutes sign flip (§十六)', () => {
  it('is the negation of JS getTimezoneOffset for any environment TZ', () => {
    const probe = new Date('2026-08-22T12:00:00Z');
    expect(localUtcOffsetMinutes(probe)).toBe(-probe.getTimezoneOffset());
  });
});

describe('buildCallFilterInput (empty dimension = omitted, §十三)', () => {
  it('empty filter ("all" preset) produces an empty wire object', () => {
    const state: ObservabilityFilterState = { ...DEFAULT_OBSERVABILITY_FILTER, datePreset: 'all' };
    expect(buildCallFilterInput(state, NOW)).toEqual({});
  });

  it('maps multi UI keys to singular wire keys; categories alias subsystem (§十九)', () => {
    const state: ObservabilityFilterState = {
      ...DEFAULT_OBSERVABILITY_FILTER,
      datePreset: 'all',
      providers: ['openai'],
      modelIds: ['gpt-5'],
      categories: ['chat'],
      terminalStatuses: ['error'],
      operations: ['llm_call'],
      callPurposes: ['chat'],
      attributionKinds: ['agent'],
      inputShapes: ['chat_context'],
      provenancePrecisions: ['exact'],
      payloadAvailabilities: ['present'],
    };
    expect(buildCallFilterInput(state, NOW)).toEqual({
      provider: ['openai'],
      modelId: ['gpt-5'],
      subsystem: ['chat'],
      terminalStatus: ['error'],
      operation: ['llm_call'],
      callPurpose: ['chat'],
      attributionKind: ['agent'],
      inputShape: ['chat_context'],
      provenancePrecision: ['exact'],
      payloadAvailability: ['present'],
    });
  });

  it('exact ids trim and wrap into single-element arrays (§二十六)', () => {
    const state: ObservabilityFilterState = {
      ...DEFAULT_OBSERVABILITY_FILTER,
      datePreset: 'all',
      sessionId: '  sess-1  ',
      taskId: 'task-9',
    };
    expect(buildCallFilterInput(state, NOW)).toEqual({
      sessionId: ['sess-1'],
      taskId: ['task-9'],
    });
  });

  it('tri-state flags only enter wire when explicitly set (§二十四)', () => {
    const base: ObservabilityFilterState = { ...DEFAULT_OBSERVABILITY_FILTER, datePreset: 'all' };
    expect(buildCallFilterInput(base, NOW)).not.toHaveProperty('interruptedByRestart');
    expect(buildCallFilterInput({ ...base, interruptedByRestart: true }, NOW)).toEqual({ interruptedByRestart: true });
    expect(buildCallFilterInput({ ...base, hasPayload: false }, NOW)).toEqual({ hasPayload: false });
  });
});

describe('dateBucketForGroupBy', () => {
  it('only present when groupBy contains date; offset via localUtcOffsetMinutes', () => {
    expect(dateBucketForGroupBy(['provider'])).toBeUndefined();
    const bucket = dateBucketForGroupBy(['date', 'provider']);
    expect(bucket).toEqual({ bucket: 'day', utcOffsetMinutes: localUtcOffsetMinutes() });
  });
});

describe('filter chips (§二十五: individually removable + clear all)', () => {
  it('default filter produces no chips', () => {
    expect(listActiveFilterChips(DEFAULT_OBSERVABILITY_FILTER)).toEqual([]);
  });

  it('non-default date and every filter kind surface as chips', () => {
    const state: ObservabilityFilterState = {
      ...DEFAULT_OBSERVABILITY_FILTER,
      datePreset: '24h',
      providers: ['openai', 'anthropic'],
      sessionId: 'sess-1',
      interruptedByRestart: true,
    };
    const chips = listActiveFilterChips(state);
    expect(chips.map((c) => c.kind)).toEqual(['date', 'multi', 'multi', 'exact', 'flag']);
    expect(chips[1]).toMatchObject({ field: 'providers', value: 'openai' });
  });

  it('removeFilterChip clears exactly one dimension per kind', () => {
    const state: ObservabilityFilterState = {
      ...DEFAULT_OBSERVABILITY_FILTER,
      datePreset: '24h',
      providers: ['openai', 'anthropic'],
      sessionId: 'sess-1',
      hasPayload: true,
    };
    expect(removeFilterChip(state, { id: 'date', kind: 'date' }).datePreset).toBe('7d');
    const afterMulti = removeFilterChip(state, { id: 'multi:providers:openai', kind: 'multi', field: 'providers', value: 'openai' });
    expect(afterMulti.providers).toEqual(['anthropic']);
    expect(removeFilterChip(state, { id: 'exact:sessionId', kind: 'exact', field: 'sessionId' }).sessionId).toBe('');
    expect(removeFilterChip(state, { id: 'flag:hasPayload', kind: 'flag', field: 'hasPayload', value: true }).hasPayload).toBeNull();
  });

  it('clearAllFilterChips returns the default (7d) filter', () => {
    expect(clearAllFilterChips()).toEqual(DEFAULT_OBSERVABILITY_FILTER);
  });
});
