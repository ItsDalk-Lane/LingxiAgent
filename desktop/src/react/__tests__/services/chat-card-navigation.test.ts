import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  navigateToChatCard,
  subscribeChatCardNavigation,
} from '../../services/chat-card-navigation';
import { useStore } from '../../stores';

const cleanups: Array<() => void> = [];
afterEach(() => {
  while (cleanups.length) cleanups.pop()?.();
  vi.useRealTimers();
  useStore.setState({ currentSessionPath: null } as never);
});

describe('chat card navigation', () => {
  it('delivers the exact stable id to a mounted matching card', () => {
    const listener = vi.fn(() => true);
    cleanups.push(subscribeChatCardNavigation(listener));

    navigateToChatCard({ kind: 'terminal', ids: ['call_1', 'term_1'] });

    expect(listener).toHaveBeenCalledWith({ kind: 'terminal', ids: ['call_1', 'term_1'] });
  });

  it('keeps one pending request until a collapsed card mounts', () => {
    navigateToChatCard({ kind: 'subagent', ids: ['task_1'] });
    const listener = vi.fn(() => true);

    cleanups.push(subscribeChatCardNavigation(listener));

    expect(listener).toHaveBeenCalledWith({ kind: 'subagent', ids: ['task_1'] });
  });

  it('replays a session-scoped pending request only while that session is current', () => {
    useStore.setState({ currentSessionPath: '/sessions/a.jsonl' } as never);
    navigateToChatCard({ kind: 'terminal', ids: ['term_1'], sessionPath: '/sessions/a.jsonl' });

    // 会话切走后，迟到的卡片挂载不再消费旧会话的 pending。
    useStore.setState({ currentSessionPath: '/sessions/b.jsonl' } as never);
    const staleListener = vi.fn(() => false);
    cleanups.push(subscribeChatCardNavigation(staleListener));
    expect(staleListener).not.toHaveBeenCalled();

    // 切回原会话后新的导航照常 pending 并投递。
    navigateToChatCard({ kind: 'terminal', ids: ['term_2'], sessionPath: '/sessions/b.jsonl' });
    const listener = vi.fn(() => true);
    cleanups.push(subscribeChatCardNavigation(listener));
    expect(listener).toHaveBeenCalledWith({ kind: 'terminal', ids: ['term_2'], sessionPath: '/sessions/b.jsonl' });
  });

  it('expires a pending request after the ttl instead of delivering it forever', () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000_000);
    useStore.setState({ currentSessionPath: '/sessions/a.jsonl' } as never);

    navigateToChatCard({ kind: 'subagent', ids: ['task_1'], sessionPath: '/sessions/a.jsonl' });
    vi.setSystemTime(1_000_000 + 31_000);

    const listener = vi.fn(() => true);
    cleanups.push(subscribeChatCardNavigation(listener));
    expect(listener).not.toHaveBeenCalled();
  });

  it('delivers a session-scoped pending request while the session is still current', () => {
    useStore.setState({ currentSessionPath: '/sessions/a.jsonl' } as never);
    navigateToChatCard({ kind: 'terminal', ids: ['term_1'], sessionPath: '/sessions/a.jsonl' });

    const listener = vi.fn(() => true);
    cleanups.push(subscribeChatCardNavigation(listener));

    expect(listener).toHaveBeenCalledWith({ kind: 'terminal', ids: ['term_1'], sessionPath: '/sessions/a.jsonl' });
  });
});
