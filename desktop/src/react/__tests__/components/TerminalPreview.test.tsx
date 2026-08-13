// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import React from 'react';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const terminalClientMocks = vi.hoisted(() => ({
  requestTerminalSnapshot: vi.fn(() => true),
  requestTerminalTail: vi.fn(() => true),
}));

vi.mock('../../services/terminal-client', () => terminalClientMocks);

import { TerminalPreview } from '../../components/right-workspace/TerminalCard';
import { terminalOutputStream } from '../../services/terminal-output-stream';
import { useStore } from '../../stores';

class MockResizeObserver {
  static instances: MockResizeObserver[] = [];
  constructor(private readonly callback: ResizeObserverCallback) {
    MockResizeObserver.instances.push(this);
  }
  observe() {}
  disconnect() {}
  trigger() { this.callback([], this as unknown as ResizeObserver); }
}

function terminal(overrides: Record<string, unknown> = {}) {
  return {
    terminalId: 'term_preview',
    sessionId: 'sess_a',
    sessionPath: '/sessions/a.jsonl',
    agentId: 'hana',
    cwd: '/workspace',
    command: 'npm test',
    label: 'tests',
    status: 'running' as const,
    seq: 0,
    createdAt: 1,
    lastActivityAt: 1,
    exitedAt: null,
    exitCode: null,
    signal: null,
    transcriptPath: '/state/term_preview.jsonl',
    ...overrides,
  };
}

function setScrollMetrics(
  element: HTMLElement,
  metrics: { scrollHeight: number; clientHeight: number; scrollTop: number },
) {
  Object.defineProperty(element, 'scrollHeight', { configurable: true, get: () => metrics.scrollHeight });
  Object.defineProperty(element, 'clientHeight', { configurable: true, get: () => metrics.clientHeight });
  Object.defineProperty(element, 'scrollTop', {
    configurable: true,
    get: () => metrics.scrollTop,
    set: (value) => { metrics.scrollTop = value; },
  });
}

describe('TerminalPreview', () => {
  beforeEach(() => {
    MockResizeObserver.instances = [];
    window.ResizeObserver = MockResizeObserver as unknown as typeof ResizeObserver;
    window.matchMedia = vi.fn().mockReturnValue({
      matches: false,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    }) as unknown as typeof window.matchMedia;
    window.requestAnimationFrame = ((callback: FrameRequestCallback) => {
      callback(16);
      return 1;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {}) as typeof window.cancelAnimationFrame;
    window.t = ((key: string) => key) as typeof window.t;
    useStore.setState({ wsState: 'disconnected' } as never);
    terminalClientMocks.requestTerminalTail.mockClear();
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
    useStore.setState({ wsState: 'disconnected' } as never);
  });

  it('keeps one ANSI parser across chunks and renders split color sequences correctly', () => {
    render(<TerminalPreview terminal={terminal()} />);

    act(() => {
      terminalOutputStream.handleTail({
        type: 'terminal_tail',
        sessionId: 'sess_a',
        sessionPath: '/sessions/a.jsonl',
        terminalId: 'term_preview',
        terminal: terminal({ seq: 2 }),
        chunks: [
          { seq: 1, data: '\u001b[3' },
          { seq: 2, data: '1mERROR' },
        ],
        sinceSeq: null,
        lastSeq: 2,
        truncated: false,
      });
    });

    const output = screen.getByTestId('terminal-output-term_preview');
    expect(output).toHaveTextContent('ERROR');
    expect(output.innerHTML).toContain('color:rgb(187,0,0)');
  });

  it('escapes unsafe HTML and never creates executable or terminal-supplied links', () => {
    render(<TerminalPreview terminal={terminal()} />);

    act(() => {
      terminalOutputStream.handleTail({
        type: 'terminal_tail',
        sessionId: 'sess_a',
        sessionPath: '/sessions/a.jsonl',
        terminalId: 'term_preview',
        terminal: terminal({ seq: 1 }),
        chunks: [{
          seq: 1,
          data: '<img src=x onerror=alert(1)><script>alert(2)</script>\u001b]8;;javascript:alert(3)\u0007click\u001b]8;;\u0007',
        }],
        sinceSeq: null,
        lastSeq: 1,
        truncated: false,
      });
    });

    const output = screen.getByTestId('terminal-output-term_preview');
    expect(output.querySelector('img')).toBeNull();
    expect(output.querySelector('script')).toBeNull();
    expect(output.querySelector('a')).toBeNull();
    expect(output).toHaveTextContent('<img src=x onerror=alert(1)>');
    expect(output).toHaveTextContent('<script>alert(2)</script>');
  });

  it('sticks to the bottom initially and stops auto-scroll after the user scrolls up', () => {
    render(<TerminalPreview terminal={terminal()} />);
    const scroller = screen.getByTestId('terminal-preview-term_preview');
    const metrics = { scrollHeight: 600, clientHeight: 300, scrollTop: 0 };
    setScrollMetrics(scroller, metrics);

    act(() => {
      MockResizeObserver.instances[0].trigger();
    });
    expect(metrics.scrollTop).toBe(300);

    act(() => {
      metrics.scrollTop = 100;
      fireEvent.scroll(scroller);
      terminalOutputStream.handleTail({
        type: 'terminal_tail',
        sessionId: 'sess_a',
        sessionPath: '/sessions/a.jsonl',
        terminalId: 'term_preview',
        terminal: terminal({ seq: 1 }),
        chunks: [{ seq: 1, data: 'new output\n' }],
        sinceSeq: null,
        lastSeq: 1,
        truncated: false,
      });
      metrics.scrollHeight = 700;
      MockResizeObserver.instances[0].trigger();
    });

    expect(metrics.scrollTop).toBe(100);
  });

  it('requests the tail only once the socket is connected, and retries after a reconnect', () => {
    render(<TerminalPreview terminal={terminal()} />);

    // socket 未 OPEN 时发出的 tail 请求会落空；挂载时不乱发，等连接建立。
    expect(terminalClientMocks.requestTerminalTail).not.toHaveBeenCalled();

    act(() => {
      useStore.setState({ wsState: 'connected' } as never);
    });
    expect(terminalClientMocks.requestTerminalTail).toHaveBeenCalledTimes(1);
    expect(terminalClientMocks.requestTerminalTail).toHaveBeenCalledWith({
      terminalId: 'term_preview',
      sessionId: 'sess_a',
      sessionPath: '/sessions/a.jsonl',
    });

    act(() => {
      useStore.setState({ wsState: 'reconnecting' } as never);
    });
    act(() => {
      useStore.setState({ wsState: 'connected' } as never);
    });
    expect(terminalClientMocks.requestTerminalTail).toHaveBeenCalledTimes(2);
  });

  it('bounds rendered preview chunks, dropping the oldest converted html', () => {
    useStore.setState({ wsState: 'connected' } as never);
    render(<TerminalPreview terminal={terminal()} />);

    act(() => {
      terminalOutputStream.handleTail({
        type: 'terminal_tail',
        sessionId: 'sess_a',
        sessionPath: '/sessions/a.jsonl',
        terminalId: 'term_preview',
        terminal: terminal({ seq: 600 }),
        chunks: Array.from({ length: 600 }, (_, index) => ({ seq: index + 1, data: `line-${index + 1}\n` })),
        sinceSeq: null,
        lastSeq: 600,
        truncated: false,
      });
    });

    const output = screen.getByTestId('terminal-output-term_preview');
    const renderedSeqs = Array.from(output.querySelectorAll('[data-terminal-seq]'))
      .map((el) => Number(el.getAttribute('data-terminal-seq')));
    expect(renderedSeqs).toHaveLength(500);
    expect(renderedSeqs[0]).toBe(101);
    expect(renderedSeqs.at(-1)).toBe(600);
  });
});
