/**
 * @vitest-environment jsdom
 *
 * facet 懒加载签名稳定性测试（§二十六回归锁定）：
 * 相对 preset 的 since 由父组件每次渲染按当前时刻重算（毫秒漂移），
 * 同一逻辑筛选绝不能因毫秒漂移 miss 缓存而 abort+refetch——那会让下拉
 * 空态文字在「加载选项…/无可选值」间高频闪烁。签名时间精度 = 分钟。
 */
import React from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, waitFor } from '@testing-library/react';
import { useObservabilityFacetOptions } from '../../../settings/tabs/observability/use-observability-facets';
import { queryObservabilityAggregate } from '../../../settings/tabs/observability/model-observability-actions';
import type {
  ModelObservabilityAggregateResult,
  ModelObservabilityCallFilterInput,
} from '../../../../../../shared/model-observability-api-contract.ts';

vi.mock('../../../settings/tabs/observability/model-observability-actions', () => ({
  queryObservabilityAggregate: vi.fn(),
}));

const aggregateMock = vi.mocked(queryObservabilityAggregate);

function Probe({ filter }: { filter: ModelObservabilityCallFilterInput }) {
  const { options, loading } = useObservabilityFacetOptions('callPurpose', true, filter);
  return <div data-testid="probe">{`${loading ? 'loading' : 'idle'}:${options.length}`}</div>;
}

beforeEach(() => {
  aggregateMock.mockReset();
  aggregateMock.mockResolvedValue({ groups: [] } as unknown as ModelObservabilityAggregateResult);
});

describe('useObservabilityFacetOptions signature stability', () => {
  it('millisecond-drifting since within one minute must not refetch', async () => {
    const base = new Date('2026-08-23T10:00:00.123Z').getTime();
    const { rerender } = render(<Probe filter={{ since: new Date(base).toISOString() }} />);
    await waitFor(() => expect(aggregateMock).toHaveBeenCalledTimes(1));
    for (let drift = 1; drift <= 5; drift += 1) {
      rerender(<Probe filter={{ since: new Date(base + drift).toISOString() }} />);
    }
    expect(aggregateMock).toHaveBeenCalledTimes(1);
  });

  it('minute boundary crossing refetches (cache key includes minute precision)', async () => {
    const base = new Date('2026-08-23T10:00:30.000Z').getTime();
    const { rerender } = render(<Probe filter={{ since: new Date(base).toISOString() }} />);
    await waitFor(() => expect(aggregateMock).toHaveBeenCalledTimes(1));
    rerender(<Probe filter={{ since: new Date(base + 61_000).toISOString() }} />);
    await waitFor(() => expect(aggregateMock).toHaveBeenCalledTimes(2));
  });

  it('logical filter change still invalidates cache', async () => {
    const since = '2026-08-23T10:00:00.000Z';
    const { rerender } = render(<Probe filter={{ since }} />);
    await waitFor(() => expect(aggregateMock).toHaveBeenCalledTimes(1));
    rerender(<Probe filter={{ since, provider: ['openai'] }} />);
    await waitFor(() => expect(aggregateMock).toHaveBeenCalledTimes(2));
  });
});
