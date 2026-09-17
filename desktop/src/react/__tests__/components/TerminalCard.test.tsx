// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import React from 'react';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const terminalClientMocks = vi.hoisted(() => ({
  requestTerminalSnapshot: vi.fn(() => true),
  requestTerminalTail: vi.fn(() => true),
}));

const controlMocks = vi.hoisted(() => ({
  stopTerminalProcess: vi.fn(async () => ({ ok: true })),
}));

const navigationMocks = vi.hoisted(() => ({
  navigateToChatCard: vi.fn(),
}));

vi.mock('../../services/terminal-client', () => terminalClientMocks);
vi.mock('../../services/background-process-control', () => controlMocks);
vi.mock('../../services/chat-card-navigation', () => navigationMocks);

import { TerminalCard } from '../../components/right-workspace/TerminalCard';
import { useStore } from '../../stores';

const translations: Record<string, string> = {
  'rightWorkspace.terminal.title': '终端进程',
  'rightWorkspace.terminal.running': '运行中',
  'rightWorkspace.terminal.done': '已完成',
  'rightWorkspace.terminal.failed': '失败',
  'rightWorkspace.terminal.aborted': '已终止',
  'rightWorkspace.terminal.stale': '已失效',
  'rightWorkspace.terminal.cwd': '工作目录',
  'rightWorkspace.terminal.exitCode': '退出码',
  'rightWorkspace.terminal.preview': '终端输出',
  'rightWorkspace.terminal.staleHint': 'Lingxi 重启后该 PTY 已不存在，但历史输出仍可查看。',
  'rightWorkspace.terminal.count': '{n} 个后台运行',
  'rightWorkspace.terminal.runningFor': '已运行 {text}',
  'rightWorkspace.terminal.stop': '停止',
  'rightWorkspace.process.stopping': '正在停止',
  'rightWorkspace.process.stopFailed': '停止失败，请重试',
};

function terminal(overrides: Record<string, unknown> = {}) {
  return {
    terminalId: 'term_1',
    toolCallId: 'call_1',
    sessionId: 'sess_a',
    sessionPath: '/sessions/a.jsonl',
    agentId: 'hana',
    cwd: '/workspace',
    command: 'wrapped command',
    label: 'friendly label',
    status: 'running' as const,
    seq: 0,
    createdAt: 1,
    lastActivityAt: 1,
    exitedAt: null,
    exitCode: null,
    signal: null,
    transcriptPath: '/state/term_1.jsonl',
    ...overrides,
  };
}

function setTerminals(terminals: ReturnType<typeof terminal>[]) {
  useStore.setState({
    currentSessionId: 'sess_a',
    currentSessionPath: '/sessions/a.jsonl',
    sessions: [{ sessionId: 'sess_a', path: '/sessions/a.jsonl' }],
    sessionLocatorsById: { sess_a: { path: '/sessions/a.jsonl' } },
    terminalsBySession: { sess_a: terminals },
  } as never);
}

describe('TerminalCard', () => {
  beforeEach(() => {
    window.t = ((key: string) => translations[key] || key) as typeof window.t;
    window.ResizeObserver = class { observe() {} disconnect() {} } as unknown as typeof ResizeObserver;
    window.matchMedia = vi.fn().mockReturnValue({
      matches: false,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    }) as unknown as typeof window.matchMedia;
    terminalClientMocks.requestTerminalSnapshot.mockClear();
    terminalClientMocks.requestTerminalTail.mockClear();
    controlMocks.stopTerminalProcess.mockClear();
    navigationMocks.navigateToChatCard.mockClear();
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('returns null when the current session has no terminal', () => {
    setTerminals([]);
    const { container } = render(<TerminalCard />);

    expect(container.firstChild).toBeNull();
    expect(terminalClientMocks.requestTerminalSnapshot).toHaveBeenCalledWith({
      sessionId: 'sess_a',
      sessionPath: '/sessions/a.jsonl',
    });
  });

  it('默认折叠只露标题行，展开后只显示运行中的终端', () => {
    setTerminals([
      terminal({ terminalId: 'running', label: '人类可读命令', createdAt: 5 }),
      terminal({ terminalId: 'done', label: '', command: 'done command', status: 'exited', exitCode: 0, createdAt: 4 }),
    ]);
    render(<TerminalCard />);

    // 默认折叠（用户裁决）：标题行在，行内容不挂载
    const header = screen.getByRole('button', { name: /终端进程/ });
    expect(header).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByTestId('terminal-name-running')).toBeNull();

    fireEvent.click(header);
    expect(header).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByTestId('terminal-name-running')).toHaveTextContent('人类可读命令');
    expect(screen.queryByTestId('terminal-name-done')).toBeNull();
    expect(screen.getByRole('button', { name: '停止' })).toBeInTheDocument();
  });

  it('shows every running terminal and no completed terminal', () => {
    const running = [
      terminal({ terminalId: 'running-old', label: 'running-old', createdAt: 20 }),
      terminal({ terminalId: 'running-new', label: 'running-new', createdAt: 21 }),
    ];
    const completed = Array.from({ length: 12 }, (_, index) => terminal({
      terminalId: `done-${index}`,
      label: `done-${index}`,
      status: 'exited',
      exitCode: 0,
      createdAt: index,
    }));
    setTerminals([...completed, ...running]);
    const { container } = render(<TerminalCard />);
    // 默认折叠：先展开标题行再数行
    fireEvent.click(screen.getByRole('button', { name: /终端进程/ }));
    const rows = Array.from(container.querySelectorAll('[data-terminal-row]'));

    expect(rows).toHaveLength(2);
    expect(rows.slice(0, 2).map((row) => row.getAttribute('data-terminal-id'))).toEqual([
      'running-new',
      'running-old',
    ]);
    expect(container.querySelector('[data-terminal-id="done-0"]')).toBeNull();
    expect(container.querySelector('[data-terminal-id="done-1"]')).toBeNull();
    expect(container.querySelector('[data-terminal-id="done-11"]')).toBeNull();
  });

  it('标题跳转到对应对话卡，停止按钮调用带会话身份的真实停止入口', async () => {
    setTerminals([terminal()]);
    render(<TerminalCard />);
    fireEvent.click(screen.getByRole('button', { name: /终端进程/ }));

    fireEvent.click(screen.getByTestId('terminal-name-term_1'));
    expect(navigationMocks.navigateToChatCard).toHaveBeenCalledWith({
      kind: 'terminal',
      ids: ['call_1', 'term_1'],
      sessionPath: '/sessions/a.jsonl',
    });

    fireEvent.click(screen.getByRole('button', { name: '停止' }));
    await waitFor(() => expect(controlMocks.stopTerminalProcess).toHaveBeenCalledWith({
      sessionId: 'sess_a',
      sessionPath: '/sessions/a.jsonl',
      terminalId: 'term_1',
    }));
  });

  it('resets the stopping state on a fallback timer when the authoritative event never arrives', async () => {
    setTerminals([terminal()]);
    render(<TerminalCard />);
    fireEvent.click(screen.getByRole('button', { name: /终端进程/ }));

    vi.useFakeTimers();
    try {
      fireEvent.click(screen.getByRole('button', { name: '停止' }));
      await act(async () => {});
      expect(controlMocks.stopTerminalProcess).toHaveBeenCalled();

      // 权威 terminal_state 事件丢失时按钮不能永久 disabled。
      expect(screen.getByRole('button', { name: '正在停止' })).toBeDisabled();

      await act(async () => {
        vi.advanceTimersByTime(30_000);
      });
      expect(screen.getByRole('button', { name: '停止' })).toBeEnabled();
    } finally {
      vi.useRealTimers();
    }
  });
});
