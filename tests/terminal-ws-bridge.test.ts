import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createTerminalWsBridge,
  TERMINAL_OUTPUT_MAX_BATCH_BYTES,
} from "../server/terminal-ws-bridge.ts";
import {
  TERMINAL_TAIL_DEFAULT_MAX_BYTES,
  TERMINAL_TAIL_DEFAULT_MAX_CHUNKS,
} from "../shared/terminal-ui-contract.ts";

function terminal(overrides: Record<string, unknown> = {}) {
  return {
    terminalId: "term_1",
    sessionId: "sess_a",
    sessionPath: "/sessions/a.jsonl",
    agentId: "hana",
    cwd: "/workspace",
    command: "npm test",
    label: "tests",
    status: "running",
    seq: 0,
    createdAt: 1,
    lastActivityAt: 1,
    exitedAt: null,
    exitCode: null,
    signal: null,
    transcriptPath: "/state/term_1.jsonl",
    ...overrides,
  };
}

describe("terminal websocket bridge", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("coalesces output for 50ms while preserving every chunk identity", () => {
    const broadcast = vi.fn();
    const bridge = createTerminalWsBridge({
      terminalSessions: {},
      resolveSessionId: () => "sess_a",
      broadcast,
    });

    bridge.handleEvent({ type: "terminal_output", terminalId: "term_1", seq: 101, data: "a" }, "/sessions/a.jsonl");
    bridge.handleEvent({ type: "terminal_output", terminalId: "term_1", seq: 102, data: "b" }, "/sessions/a.jsonl");
    vi.advanceTimersByTime(49);
    expect(broadcast).not.toHaveBeenCalled();

    vi.advanceTimersByTime(1);
    expect(broadcast).toHaveBeenCalledWith({
      type: "terminal_output",
      terminalId: "term_1",
      sessionId: "sess_a",
      sessionPath: "/sessions/a.jsonl",
      chunks: [
        { seq: 101, data: "a" },
        { seq: 102, data: "b" },
      ],
    });
  });

  it("flushes immediately when the byte threshold is reached", () => {
    const broadcast = vi.fn();
    const bridge = createTerminalWsBridge({
      terminalSessions: {},
      resolveSessionId: () => "sess_a",
      broadcast,
      maxBatchBytes: 5,
    });

    bridge.handleEvent({ type: "terminal_output", terminalId: "term_1", seq: 1, data: "你" }, "/sessions/a.jsonl");
    expect(broadcast).not.toHaveBeenCalled();
    bridge.handleEvent({ type: "terminal_output", terminalId: "term_1", seq: 2, data: "ab" }, "/sessions/a.jsonl");

    expect(broadcast).toHaveBeenCalledTimes(1);
    expect(broadcast.mock.calls[0][0].chunks).toEqual([
      { seq: 1, data: "你" },
      { seq: 2, data: "ab" },
    ]);
    expect(TERMINAL_OUTPUT_MAX_BATCH_BYTES).toBeGreaterThanOrEqual(32 * 1024);
    expect(TERMINAL_OUTPUT_MAX_BATCH_BYTES).toBeLessThanOrEqual(64 * 1024);
  });

  it.each(["terminal_exited", "terminal_closed"])("flushes pending output before %s state", (type) => {
    const broadcast = vi.fn();
    const bridge = createTerminalWsBridge({
      terminalSessions: {},
      resolveSessionId: () => "sess_a",
      broadcast,
    });
    bridge.handleEvent({
      type: "terminal_output",
      terminalId: "term_1",
      seq: 1,
      data: "last line\n",
    }, "/sessions/a.jsonl");

    bridge.handleEvent({
      type,
      terminalId: "term_1",
      terminal: terminal({ status: type === "terminal_closed" ? "killed" : "exited", seq: 1 }),
    }, "/sessions/a.jsonl");

    expect(broadcast.mock.calls.map(([message]) => message.type)).toEqual([
      "terminal_output",
      "terminal_state",
    ]);
    expect(broadcast.mock.calls[1][0].terminal).toMatchObject({
      terminalId: "term_1",
      sessionId: "sess_a",
      status: type === "terminal_closed" ? "killed" : "exited",
      seq: 1,
    });
    vi.advanceTimersByTime(50);
    expect(broadcast).toHaveBeenCalledTimes(2);
  });

  it("returns a session-scoped terminal snapshot to the requesting client", () => {
    const send = vi.fn();
    const list = vi.fn(() => ({
      sessionPath: "/sessions/a.jsonl",
      terminals: [terminal()],
    }));
    const bridge = createTerminalWsBridge({
      terminalSessions: { list },
      resolveSessionId: () => "sess_a",
      broadcast: vi.fn(),
      send,
    });
    const ws = {};

    bridge.sendSnapshot(ws, {
      sessionId: "sess_a",
      sessionPath: "/sessions/a.jsonl",
    });

    expect(list).toHaveBeenCalledWith("/sessions/a.jsonl");
    expect(send).toHaveBeenCalledWith(ws, {
      type: "terminal_snapshot",
      sessionId: "sess_a",
      sessionPath: "/sessions/a.jsonl",
      terminals: [terminal()],
    });
  });

  it("reads only the requested terminal tail with server-owned bounds", () => {
    const send = vi.fn();
    const readTail = vi.fn(() => ({
      ...terminal({ seq: 8 }),
      output: "tail\n",
      chunks: [{ seq: 8, data: "tail\n" }],
      sinceSeq: 7,
      lastSeq: 8,
      truncated: false,
    }));
    const bridge = createTerminalWsBridge({
      terminalSessions: { readTail },
      resolveSessionId: () => "sess_a",
      broadcast: vi.fn(),
      send,
    });
    const ws = {};

    bridge.sendTail(ws, {
      sessionId: "sess_a",
      sessionPath: "/sessions/a.jsonl",
      terminalId: "term_1",
      sinceSeq: 7,
    });

    expect(readTail).toHaveBeenCalledWith({
      sessionPath: "/sessions/a.jsonl",
      terminalId: "term_1",
      sinceSeq: 7,
      maxBytes: TERMINAL_TAIL_DEFAULT_MAX_BYTES,
      maxChunks: TERMINAL_TAIL_DEFAULT_MAX_CHUNKS,
    });
    expect(send).toHaveBeenCalledWith(ws, {
      type: "terminal_tail",
      sessionId: "sess_a",
      sessionPath: "/sessions/a.jsonl",
      terminalId: "term_1",
      terminal: terminal({ seq: 8 }),
      chunks: [{ seq: 8, data: "tail\n" }],
      sinceSeq: 7,
      lastSeq: 8,
      truncated: false,
    });
  });
});
