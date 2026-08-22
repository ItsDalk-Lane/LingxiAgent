/**
 * @vitest-environment jsdom
 *
 * Phase 9 chip 文案测试 — observabilityChipLabel 是 FilterBar 与 ExportDialog
 * 共享的唯一文案源（§二十五/§一百零九）；字段键映射必须是显式映射，绝不能
 * 用 `replace(/s$/)` 这类猜测式去复数。
 */
import { beforeAll, describe, expect, it } from 'vitest';
import { observabilityChipLabel } from '../../../settings/tabs/observability/model-observability-labels';
import type {
  ObservabilityExactFilterKey,
  ObservabilityFilterChip,
  ObservabilityMultiFilterKey,
} from '../../../settings/tabs/observability/model-observability-filter';

beforeAll(() => {
  window.t = ((key: string) => `T:${key}`) as typeof window.t;
});

function multi(field: ObservabilityMultiFilterKey, value: string): ObservabilityFilterChip {
  return { id: `multi:${field}:${value}`, kind: 'multi', field, value };
}

describe('observabilityChipLabel', () => {
  it('maps every multi field to an existing singular filter label key', () => {
    const cases: Array<[ObservabilityMultiFilterKey, string, string]> = [
      ['providers', 'openai', 'settings.observability.filter.provider'],
      ['modelIds', 'gpt-5', 'settings.observability.filter.model'],
      ['categories', 'chat', 'settings.observability.filter.category'],
      // 显式映射的重点：-es 结尾复数绝不能被正则砍成 terminalStatuse。
      ['terminalStatuses', 'ok', 'settings.observability.filter.status'],
      ['operations', 'llm_call', 'settings.observability.filter.operation'],
      ['callPurposes', 'chat', 'settings.observability.filter.callPurpose'],
      ['attributionKinds', 'agent', 'settings.observability.filter.attributionKind'],
      ['inputShapes', 'chat_context', 'settings.observability.filter.inputShape'],
      ['provenancePrecisions', 'exact', 'settings.observability.filter.provenancePrecision'],
      ['payloadAvailabilities', 'present', 'settings.observability.filter.payloadAvailability'],
    ];
    for (const [field, value, expectedPrefix] of cases) {
      expect(observabilityChipLabel(multi(field, value)).startsWith(`T:${expectedPrefix}: `)).toBe(true);
    }
  });

  it('closed-set values are translated through values.* (not raw wire words)', () => {
    expect(observabilityChipLabel(multi('terminalStatuses', 'ok')))
      .toBe('T:settings.observability.filter.status: T:settings.observability.values.terminalStatus.ok');
    expect(observabilityChipLabel(multi('payloadAvailabilities', 'expired')))
      .toContain('T:settings.observability.values.payloadAvailability.expired');
  });

  it('exact and flag chips use their own label keys; flag appends tri-state', () => {
    const exact: ObservabilityFilterChip = { id: 'exact:sessionId', kind: 'exact', field: 'sessionId' as ObservabilityExactFilterKey };
    expect(observabilityChipLabel(exact)).toBe('T:settings.observability.filter.sessionId');
    expect(observabilityChipLabel({ id: 'flag:hasPayload', kind: 'flag', field: 'hasPayload', value: true }))
      .toBe('T:settings.observability.filter.hasPayload: T:settings.observability.tri.yes');
    expect(observabilityChipLabel({ id: 'flag:interruptedByRestart', kind: 'flag', field: 'interruptedByRestart', value: false }))
      .toBe('T:settings.observability.filter.interruptedByRestart: T:settings.observability.tri.no');
  });

  it('date chip label is just the date filter label', () => {
    expect(observabilityChipLabel({ id: 'date', kind: 'date' }))
      .toBe('T:settings.observability.filter.date');
  });
});
