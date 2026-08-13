import { describe, expect, it, vi } from 'vitest';
import { createTerminalOutputStream } from '../../services/terminal-output-stream';
import type { TerminalOutputMessage, TerminalTailMessage } from '../../../../../shared/terminal-ui-contract.ts';

const ref = {
  terminalId: 'term_1',
  sessionId: 'sess_a',
  sessionPath: '/sessions/a.jsonl',
};

function output(
  chunks: Array<{ seq: number; data: string }>,
  overrides: Partial<TerminalOutputMessage> = {},
): TerminalOutputMessage {
  return { type: 'terminal_output', ...ref, chunks, ...overrides };
}

function tail(
  chunks: Array<{ seq: number; data: string }>,
  overrides: Partial<TerminalTailMessage> = {},
): TerminalTailMessage {
  return {
    type: 'terminal_tail',
    ...ref,
    terminal: {
      ...ref,
      agentId: 'hana',
      cwd: '/workspace',
      command: 'npm test',
      label: 'tests',
      status: 'running',
      seq: chunks.at(-1)?.seq || 0,
      createdAt: 1,
      lastActivityAt: 1,
      exitedAt: null,
      exitCode: null,
      signal: null,
      transcriptPath: '/state/term_1.jsonl',
    },
    chunks,
    sinceSeq: null,
    lastSeq: chunks.at(-1)?.seq || 0,
    truncated: false,
    ...overrides,
  };
}

describe('terminal output stream', () => {
  it('does not cache output while there is no subscriber', () => {
    const stream = createTerminalOutputStream();
    stream.handleChunks(output([{ seq: 1, data: 'dropped' }]));
    const onChunks = vi.fn();
    stream.subscribe(ref, { onChunks });
    stream.handleTail(tail([]));

    expect(onChunks).not.toHaveBeenCalled();
  });

  it('delivers ordered chunks and drops duplicate sequence numbers', () => {
    const stream = createTerminalOutputStream();
    const onChunks = vi.fn();
    stream.subscribe(ref, { onChunks });
    stream.handleTail(tail([{ seq: 1, data: 'a' }]));
    stream.handleChunks(output([
      { seq: 2, data: 'b' },
      { seq: 2, data: 'duplicate' },
      { seq: 3, data: 'c' },
    ]));

    expect(onChunks.mock.calls.flatMap(([delivery]) => delivery.chunks)).toEqual([
      { seq: 1, data: 'a' },
      { seq: 2, data: 'b' },
      { seq: 3, data: 'c' },
    ]);
  });

  it('merges live output received before the initial tail without loss or duplication', () => {
    const stream = createTerminalOutputStream();
    const onChunks = vi.fn();
    stream.subscribe(ref, { onChunks });
    stream.handleChunks(output([
      { seq: 2, data: 'live-b' },
      { seq: 3, data: 'live-c' },
    ]));
    expect(onChunks).not.toHaveBeenCalled();

    stream.handleTail(tail([
      { seq: 1, data: 'tail-a' },
      { seq: 2, data: 'tail-b' },
    ], { lastSeq: 3 }));

    expect(onChunks).toHaveBeenCalledTimes(1);
    expect(onChunks.mock.calls[0][0]).toEqual({
      chunks: [
        { seq: 1, data: 'tail-a' },
        { seq: 2, data: 'live-b' },
        { seq: 3, data: 'live-c' },
      ],
      reset: false,
    });
  });

  it('detects a gap, accepts a tail repair, and then drains buffered live output', () => {
    const stream = createTerminalOutputStream();
    const onChunks = vi.fn();
    const onGap = vi.fn();
    stream.subscribe(ref, { onChunks, onGap });
    stream.handleTail(tail([{ seq: 1, data: 'a' }]));
    stream.handleChunks(output([{ seq: 3, data: 'c' }]));

    expect(onGap).toHaveBeenCalledWith({ ...ref, lastSeq: 1, nextSeq: 3 });
    stream.handleTail(tail([
      { seq: 2, data: 'b' },
      { seq: 3, data: 'tail-c' },
    ], { sinceSeq: 1, lastSeq: 3 }));

    expect(onChunks.mock.calls.flatMap(([delivery]) => delivery.chunks)).toEqual([
      { seq: 1, data: 'a' },
      { seq: 2, data: 'b' },
      { seq: 3, data: 'c' },
    ]);
    expect(onGap).toHaveBeenCalledTimes(1);
  });

  it('isolates subscribers by stable session identity and terminal id', () => {
    const stream = createTerminalOutputStream();
    const onChunks = vi.fn();
    stream.subscribe(ref, { onChunks });
    stream.handleChunks(output([{ seq: 1, data: 'wrong' }], {
      sessionId: 'sess_b',
      sessionPath: '/sessions/b.jsonl',
    }));
    stream.handleTail(tail([]));

    expect(onChunks).not.toHaveBeenCalled();
  });

  it('bounds pre-tail pending live chunks to the server tail hard limit', () => {
    // 首个 tail 请求失败/丢失时 live 块会一直进 pending；无界增长会把 renderer 拖垮。
    // 超出服务端 tail 硬上限（2000 块）的部分本来也补不回来，只保留最新的。
    const stream = createTerminalOutputStream();
    const onChunks = vi.fn();
    stream.subscribe(ref, { onChunks });
    stream.handleChunks(output(
      Array.from({ length: 2500 }, (_, index) => ({ seq: index + 1, data: `chunk-${index + 1}` })),
    ));
    expect(onChunks).not.toHaveBeenCalled();

    stream.handleTail(tail([], { lastSeq: 2500 }));

    const delivered = onChunks.mock.calls.flatMap(([delivery]) => delivery.chunks);
    expect(delivered).toHaveLength(2000);
    expect(delivered[0]).toEqual({ seq: 501, data: 'chunk-501' });
    expect(delivered.at(-1)).toEqual({ seq: 2500, data: 'chunk-2500' });
  });
});
