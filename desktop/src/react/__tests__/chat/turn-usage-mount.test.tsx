/**
 * 用量/用时胶囊 · AssistantMessage 挂载集成测试。
 *
 * 锁定真实渲染链路：完成轮投影（含/不含时间戳两种形态）→ useTurnUsageStats
 * 发起 query/calls → 账本返回 present 调用 → 页脚出现双胶囊；无数据/未完成轮
 * 不发请求也不渲染。网络层 mock 在 lingxiFetch 模块边界（不 mock 被测组件）。
 */
// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ModelObservabilityCallListItem } from '../../../../../shared/model-observability-api-contract.ts';

const lingxiFetchMock = vi.fn();
vi.mock('../../hooks/use-hana-fetch', () => ({
  lingxiFetch: (...args: unknown[]) => lingxiFetchMock(...args),
}));

import { AssistantMessage } from '../../components/chat/AssistantMessage';
import { useStore } from '../../stores';
import type { AssistantTurnProjection, ChatMessage } from '../../stores/chat-types';

const PATH = '/test/pill-mount.jsonl';

function callItem(startedAt: string): ModelObservabilityCallListItem {
  return {
    callId: 'mc_mount_1',
    traceId: 'mt_mount',
    parentCallId: null,
    startedAt,
    endedAt: startedAt,
    durationMs: 4000,
    terminalStatus: 'ok',
    model: { provider: 'openai', modelId: 'gpt-test', api: 'responses' },
    usage: {
      availability: 'present',
      status: 'ok',
      summary: {
        inputTokens: 1000,
        inputUncachedTokens: 250,
        outputTokens: 100,
        reasoningTokens: 30,
        cacheReadTokens: 750,
        cacheWriteTokens: 0,
        totalTokens: 1100,
        costTotal: null,
      },
    },
  } as unknown as ModelObservabilityCallListItem;
}

const T0 = Date.UTC(2026, 8, 5, 11, 36, 5, 0);

function projection(timestamps: boolean): AssistantTurnProjection {
  return {
    id: 'turn_mount',
    inputMessageId: null,
    assistantMessageIds: ['a1'],
    processBlockIds: [],
    answerBlockIds: ['b1'],
    resultBlockIds: [],
    controlBlockIds: [],
    status: 'completed',
    ...(timestamps ? { startedAt: T0, completedAt: T0 + 90_000 } : {}),
  };
}

function message(turn: AssistantTurnProjection): ChatMessage {
  return {
    id: 'a1',
    role: 'assistant',
    blocks: [{ id: 'b1', type: 'text', source: 'hello', html: '<p>hello</p>' }],
    timestamp: T0 + 92_000,
    turnProjection: turn,
  };
}

function renderAssistant(msg: ChatMessage) {
  return render(
    <AssistantMessage
      message={msg}
      showAvatar={false}
      sessionPath={PATH}
      agentDisplay={{ displayName: 'Lingxi', yuan: 'lingxi' } as never}
      isStreaming={false}
      isSelected={false}
      showTurnCompletionTime
    />,
  );
}

function queryBody(): Record<string, unknown> {
  const call = lingxiFetchMock.mock.calls[0]?.[1] as { body?: string } | undefined;
  return JSON.parse(call?.body ?? '{}');
}

beforeEach(() => {
  lingxiFetchMock.mockReset();
  // 隔离：邻居回退读的是共享 store 的 chatSessions，不清场会跨用例串台
  useStore.setState({ chatSessions: {}, sessionLocatorsById: {}, currentSessionId: null, currentSessionPath: null });
});

afterEach(() => {
  cleanup();
});

describe('AssistantMessage 用量/用时胶囊挂载（集成）', () => {
  it('完成轮投影带时间戳：发起 query/calls 且渲染双胶囊', async () => {
    lingxiFetchMock.mockResolvedValue({
      ok: true,
      json: async () => ({ calls: [callItem(new Date(T0 + 1000).toISOString())] }),
    });
    renderAssistant(message(projection(true)));

    const pill = await screen.findByTestId('turn-usage-pill', {}, { timeout: 3000 });
    expect(pill).toHaveTextContent('用量 1.1K tok');
    expect(screen.getByTestId('turn-time-pill')).toBeInTheDocument();

    expect(lingxiFetchMock).toHaveBeenCalledWith('/api/model-observability/query/calls', expect.objectContaining({ method: 'POST' }));
    const body = queryBody();
    expect((body.filter as Record<string, unknown>).sessionPath).toBe(PATH);
    expect((body.filter as Record<string, unknown>).since).toBe(new Date(T0).toISOString());
  });

  it('历史投影无时间戳：邻居回退（上一条 user 时刻~本条时刻）仍发起查询', async () => {
    lingxiFetchMock.mockResolvedValue({
      ok: true,
      json: async () => ({ calls: [callItem(new Date(T0 + 1000).toISOString())] }),
    });
    // 生产同款键形态：chatSessions 以 sessionScopedKey（sessionId 优先）为键，
    // locators 提供 path→sessionId 归属；锁定作用域查找不回退裸 path 也能命中。
    // a1 之后再放一条下一轮 user 消息：邻居窗口上界 = 其时刻−1（确定值）。
    const { useStore: store } = await import('../../stores');
    store.setState((state: Record<string, any>) => ({
      chatSessions: {
        ...state.chatSessions,
        sess_mount: {
          items: [
            { type: 'message', data: { id: 'u1', role: 'user', text: 'q', timestamp: T0 } },
            { type: 'message', data: { ...message(projection(false)) } },
            { type: 'message', data: { id: 'u2', role: 'user', text: 'next', timestamp: T0 + 120_000 } },
          ],
          hasMore: false,
          loadingMore: false,
        },
      },
      sessionLocatorsById: { sess_mount: { path: PATH } },
    }));

    renderAssistant(message(projection(false)));
    await screen.findByTestId('turn-usage-pill', {}, { timeout: 3000 });
    const body = queryBody();
    expect((body.filter as Record<string, unknown>).since).toBe(new Date(T0).toISOString());
    expect((body.filter as Record<string, unknown>).until).toBe(new Date(T0 + 120_000 - 1).toISOString());
  });

  it('账本无数据（calls 空）→ 不渲染胶囊也不渲染 0', async () => {
    lingxiFetchMock.mockResolvedValue({ ok: true, json: async () => ({ calls: [] }) });
    renderAssistant(message(projection(true)));
    await waitFor(() => { expect(lingxiFetchMock).toHaveBeenCalled(); }, { timeout: 3000 });
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(screen.queryByTestId('turn-usage-pill')).toBeNull();
  });

  it('streaming 轮不发请求、不渲染', async () => {
    const streaming = { ...message(projection(true)), turnProjection: { ...projection(true), status: 'streaming' as const } };
    renderAssistant(streaming);
    await new Promise((resolve) => setTimeout(resolve, 80));
    expect(lingxiFetchMock).not.toHaveBeenCalled();
    expect(screen.queryByTestId('turn-usage-pill')).toBeNull();
  });
});
