/**
 * @vitest-environment jsdom
 */
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const switchSessionMock = vi.fn();
const locateSearchHitMock = vi.fn();

vi.mock('../../stores/session-actions', () => ({
  switchSession: (...args: unknown[]) => switchSessionMock(...args),
}));

vi.mock('../../stores/chat-find-actions', () => ({
  locateSearchHit: (...args: unknown[]) => locateSearchHitMock(...args),
}));

vi.mock('../../hooks/use-i18n', () => ({
  useI18n: () => ({ t: (key: string) => key }),
}));

import { SessionSearchItem } from '../../components/search/ChatSearchOverlay';

function makeResult(overrides: Record<string, unknown> = {}) {
  return {
    path: '/tmp/agents/hana/sessions/result.jsonl',
    title: 'Result title',
    firstMessage: 'hello',
    modified: '2026-05-22T08:00:00.000Z',
    messageCount: 2,
    agentId: 'hana',
    agentName: 'Hana',
    cwd: '/tmp/project',
    matchKind: 'content' as const,
    snippet: 'a snippet',
    ...overrides,
  };
}

describe('SessionSearchItem', () => {
  const onSelect = vi.fn();

  beforeEach(() => {
    switchSessionMock.mockReset();
    locateSearchHitMock.mockReset();
    onSelect.mockReset();
  });

  afterEach(() => {
    cleanup();
  });

  it('forwards a content match result with its query on click', () => {
    const result = makeResult({ matchKind: 'content' });
    render(
      <SessionSearchItem result={result as never} isActive={false} agents={[]} query="排查" onSelect={onSelect} />,
    );

    fireEvent.click(screen.getByText('Result title').closest('button')!);

    expect(onSelect).toHaveBeenCalledWith(result);
    // 点击路由（locateSearchHit/switchSession 分流）不在 item 内，由 ChatSearchOverlay 测试覆盖。
    expect(switchSessionMock).not.toHaveBeenCalled();
    expect(locateSearchHitMock).not.toHaveBeenCalled();
  });

  it('forwards a title match result on click and shows the snippet', () => {
    const result = makeResult({ matchKind: 'title' });
    render(
      <SessionSearchItem result={result as never} isActive={false} agents={[]} query="排查" onSelect={onSelect} />,
    );

    fireEvent.click(screen.getByText('Result title').closest('button')!);

    expect(onSelect).toHaveBeenCalledWith(result);
    expect(screen.getByText('a snippet')).toBeInTheDocument();
  });
});
