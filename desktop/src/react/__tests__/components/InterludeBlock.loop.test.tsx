// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import React from 'react';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { InterludeBlock } from '../../components/chat/InterludeBlock';
import { useStore } from '../../stores';

const { sendMock, getWebSocketMock } = vi.hoisted(() => ({
  sendMock: vi.fn(),
  getWebSocketMock: vi.fn(),
}));

vi.mock('../../services/websocket', () => ({
  getWebSocket: getWebSocketMock,
}));

function loopBlock(overrides: Record<string, unknown> = {}) {
  return {
    type: 'interlude',
    id: 'loop-1',
    variant: 'loop',
    status: 'success',
    text: '循环任务执行中',
    ...overrides,
  } as never;
}

// LoopControls 的身份来自渲染上下文透传的 sessionPath/agentId（H1 回归契约）：
//  locator 反解 sessionId 需要 currentSessionPath 或 sessions 列表能对上该 path。
const LOOP_SESSION = { sessionPath: '/session/a.jsonl', agentId: 'a1' };

function renderLoopBlock(overrides: Record<string, unknown> = {}) {
  return render(
    <InterludeBlock
      block={loopBlock({ turnCount: 0, maxTurns: 50, ...overrides })}
      sessionPath={LOOP_SESSION.sessionPath}
      agentId={LOOP_SESSION.agentId}
    />,
  );
}

function setLoopState(status: 'running' | 'paused' | 'stopped' | null) {
  useStore.setState({
    currentSessionId: 'sess_a',
    currentSessionPath: '/session/a.jsonl',
    currentAgentId: 'a1',
    loopStatusBySession: status
      ? { sess_a: { status, turnCount: 0, maxTurns: 50, pausedReason: null, prompt: 'x' } }
      : {},
  } as never);
}

describe('InterludeBlock loop 轮次片段', () => {
  beforeEach(() => {
    Object.defineProperty(window, 'matchMedia', {
      writable: true,
      value: vi.fn().mockImplementation((query: string) => ({
        matches: false,
        media: query,
        onchange: null,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        addListener: vi.fn(),
        removeListener: vi.fn(),
        dispatchEvent: vi.fn(),
      })),
    });
    useStore.setState({
      currentSessionId: null,
      currentSessionPath: null,
      currentAgentId: null,
      loopStatusBySession: {},
    } as never);
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('variant=loop 且 turnCount=0 / maxTurns=50 时渲染"第 1/50 轮"', () => {
    render(<InterludeBlock block={loopBlock({ turnCount: 0, maxTurns: 50 })} />);

    expect(screen.getByText(/第 1\/50 轮/)).toBeInTheDocument();
  });

  it('turnCount 封顶：turnCount=49 / maxTurns=50 渲染"第 50/50 轮"', () => {
    render(<InterludeBlock block={loopBlock({ turnCount: 49, maxTurns: 50 })} />);

    expect(screen.getByText(/第 50\/50 轮/)).toBeInTheDocument();
  });

  it('maxTurns 缺失时不渲染轮次片段', () => {
    render(<InterludeBlock block={loopBlock({ turnCount: 3 })} />);

    expect(screen.getByText('循环任务执行中')).toBeInTheDocument();
    expect(screen.queryByText(/第 \d+\/\d+ 轮/)).toBeNull();
  });

  it('maxTurns 非 finite 时不渲染轮次片段', () => {
    render(<InterludeBlock block={loopBlock({ turnCount: 3, maxTurns: Number.POSITIVE_INFINITY })} />);

    expect(screen.queryByText(/第 \d+\/\d+ 轮/)).toBeNull();
  });

  it('非 loop variant 不渲染轮次片段', () => {
    render(<InterludeBlock block={loopBlock({ variant: 'deferred_result', turnCount: 0, maxTurns: 50 })} />);

    expect(screen.queryByText(/第 \d+\/\d+ 轮/)).toBeNull();
  });
});

describe('InterludeBlock LoopControls', () => {
  beforeEach(() => {
    Object.defineProperty(window, 'matchMedia', {
      writable: true,
      value: vi.fn().mockImplementation((query: string) => ({
        matches: false,
        media: query,
        onchange: null,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        addListener: vi.fn(),
        removeListener: vi.fn(),
        dispatchEvent: vi.fn(),
      })),
    });
    getWebSocketMock.mockReturnValue({ readyState: WebSocket.OPEN, send: sendMock });
    setLoopState(null);
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('running 时渲染暂停与停止按钮', () => {
    setLoopState('running');
    renderLoopBlock();

    expect(screen.getByTitle('暂停循环')).toBeInTheDocument();
    expect(screen.getByTitle('停止循环')).toBeInTheDocument();
    expect(screen.queryByTitle('恢复循环')).toBeNull();
  });

  it('paused 时渲染恢复与停止按钮', () => {
    setLoopState('paused');
    renderLoopBlock();

    expect(screen.getByTitle('恢复循环')).toBeInTheDocument();
    expect(screen.getByTitle('停止循环')).toBeInTheDocument();
    expect(screen.queryByTitle('暂停循环')).toBeNull();
  });

  it('stopped / 无状态时不渲染按钮', () => {
    setLoopState('stopped');
    const { unmount } = renderLoopBlock();

    expect(screen.queryByTitle('暂停循环')).toBeNull();
    expect(screen.queryByTitle('恢复循环')).toBeNull();
    expect(screen.queryByTitle('停止循环')).toBeNull();
    unmount();

    setLoopState(null);
    renderLoopBlock();

    expect(screen.queryByTitle('暂停循环')).toBeNull();
    expect(screen.queryByTitle('恢复循环')).toBeNull();
    expect(screen.queryByTitle('停止循环')).toBeNull();
  });

  it('点击停止通过 websocket 发送 /loop stop', async () => {
    setLoopState('running');
    renderLoopBlock();

    fireEvent.click(screen.getByTitle('停止循环'));

    await waitFor(() => {
      expect(sendMock).toHaveBeenCalledTimes(1);
    });
    expect(sendMock).toHaveBeenCalledWith(JSON.stringify({
      type: 'slash',
      text: '/loop stop',
      sessionPath: '/session/a.jsonl',
      agentId: 'a1',
    }));
  });

  it('点击暂停发送 /loop pause，paused 下点击恢复发送 /loop resume', async () => {
    setLoopState('running');
    const { unmount } = renderLoopBlock();

    fireEvent.click(screen.getByTitle('暂停循环'));
    await waitFor(() => {
      expect(sendMock).toHaveBeenCalledWith(JSON.stringify({
        type: 'slash',
        text: '/loop pause',
        sessionPath: '/session/a.jsonl',
        agentId: 'a1',
      }));
    });
    unmount();
    sendMock.mockClear();

    setLoopState('paused');
    renderLoopBlock();

    fireEvent.click(screen.getByTitle('恢复循环'));
    await waitFor(() => {
      expect(sendMock).toHaveBeenCalledWith(JSON.stringify({
        type: 'slash',
        text: '/loop resume',
        sessionPath: '/session/a.jsonl',
        agentId: 'a1',
      }));
    });
  });

  it('非 loop variant 不挂载控制按钮（即使 store 里有 running 状态）', () => {
    setLoopState('running');
    renderLoopBlock({ variant: 'deferred_result' });

    expect(screen.queryByTitle('暂停循环')).toBeNull();
    expect(screen.queryByTitle('停止循环')).toBeNull();
  });

  it('H1 回归：非所属会话的渲染上下文拿不到按钮，更不会误控主会话循环', () => {
    // 主会话有 running 循环；此时在子助手/频道预览（透传的是预览会话的 path）里渲染
    // loop interlude —— 预览会话解析不到自己的 sessionId/循环状态，必须不渲染按钮。
    setLoopState('running');
    render(
      <InterludeBlock
        block={loopBlock({ turnCount: 0, maxTurns: 50 })}
        sessionPath="/session/other-preview.jsonl"
        agentId="sub-1"
      />,
    );

    expect(screen.queryByTitle('暂停循环')).toBeNull();
    expect(screen.queryByTitle('停止循环')).toBeNull();
  });
});
