// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ThinkingBlock } from '../../components/chat/ThinkingBlock';
import { useStore } from '../../stores';
import { clearDeferredHistoryContentCacheForTests } from '../../hooks/use-deferred-history-content';

describe('ThinkingBlock', () => {
  beforeEach(() => {
    window.t = ((key: string) => key) as typeof window.t;
    useStore.setState({ serverPort: '30141' } as never);
    clearDeferredHistoryContentCacheForTests();
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it('只在展开历史思考时读取完整内容', async () => {
    const fetchMock = vi.spyOn(globalThis, 'fetch').mockResolvedValue(new Response(JSON.stringify({
      id: 'reasoning-heavy',
      kind: 'assistant_segment',
      content: '完整思考末尾',
    }), { status: 200, headers: { 'Content-Type': 'application/json' } }));

    render(
      <ThinkingBlock
        content="思考预览"
        sealed
        sessionPath="/session/heavy.jsonl"
        deferred={{
          id: 'reasoning-heavy',
          kind: 'assistant_segment',
          size: 9_000,
          available: true,
        }}
      />,
    );

    expect(fetchMock).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText('thinking.done'));
    expect(screen.getByText('思考预览')).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getByText('完整思考末尾')).toBeInTheDocument();
    });
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});
