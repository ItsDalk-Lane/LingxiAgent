// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';

const fetchMock = vi.hoisted(() => vi.fn());
vi.mock('../../hooks/use-hana-fetch', () => ({ lingxiFetch: fetchMock }));

import { loadMobileSessions } from '../../mobile/mobile-init';

describe('mobile session response', () => {
  afterEach(() => {
    fetchMock.mockReset();
    useStore.setState({ sessions: [], currentSessionPath: null });
  });

  it.each([
    ['non-array', { sessions: [] }],
    ['invalid member', [{ title: 'missing path' }]],
  ])('keeps visible sessions when the server returns %s data', async (_kind, body) => {
    const previous = { path: '/sessions/previous.jsonl', title: 'Previous' };
    useStore.setState({ sessions: [previous] as never });
    fetchMock.mockResolvedValue({ json: async () => body });

    await expect(loadMobileSessions()).rejects.toThrow('mobile sessions response is incomplete');
    expect(useStore.getState().sessions).toEqual([previous]);
  });
});
