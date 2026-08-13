import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createChatRoute } from "../server/routes/chat.ts";

type HarnessSocket = {
  readyState: number;
  send: ReturnType<typeof vi.fn>;
};

type ChatWsHandlers = {
  onOpen: (event: unknown, ws: HarnessSocket) => void;
  onMessage: (event: { data: string }, ws: HarnessSocket) => void;
};

type TerminalSubscriber = (event: Record<string, unknown>, sessionPath: string) => void;

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

function makeHarness() {
  let createHandlers!: (context: Record<string, never>) => ChatWsHandlers;
  let subscriber!: TerminalSubscriber;
  const upgradeWebSocket = vi.fn((factory: (context: Record<string, never>) => ChatWsHandlers) => {
    createHandlers = factory;
    return () => new Response(null);
  });
  const terminalSessions = {
    list: vi.fn(() => ({ sessionPath: "/sessions/a.jsonl", terminals: [terminal()] })),
    readTail: vi.fn(() => ({
      ...terminal({ seq: 1 }),
      output: "ready\n",
      chunks: [{ seq: 1, data: "ready\n" }],
      sinceSeq: null,
      lastSeq: 1,
      truncated: false,
    })),
    close: vi.fn((input) => ({ ...terminal(), ...input, status: "killed" })),
  };
  const taskRegistry = {
    query: vi.fn((taskId) => ({ taskId, type: "subagent", parentSessionId: "sess_a", parentSessionPath: "/sessions/a.jsonl", status: "running" })),
    abort: vi.fn(() => "aborted"),
  };
  const hub = {
    subscribe: vi.fn((fn) => { subscriber = fn; }),
    send: vi.fn(async () => {}),
    eventBus: { emit: vi.fn() },
  };
  const engine = {
    agentName: "Hana",
    terminalSessions,
    taskRegistry,
    abortAllStreaming: vi.fn(async () => {}),
    getSessionByPath: vi.fn(() => ({ entries: [] })),
    getSessionIdForPath: vi.fn(() => "sess_a"),
    getSessionManifest: vi.fn(() => ({ currentLocator: { path: "/sessions/a.jsonl" } })),
    getRuntimeContext: vi.fn(() => ({ studioId: "studio_a" })),
    resolveSessionOwnership: vi.fn(() => ({ agentId: "hana", source: "manifest", agentDeleted: false })),
    isSessionStreaming: vi.fn(() => false),
    isSessionSwitching: vi.fn(() => false),
    steerSession: vi.fn(() => false),
    slashDispatcher: null,
  };
  createChatRoute(engine, hub, { upgradeWebSocket });
  const handlers = createHandlers({});
  const ws = { readyState: 1, send: vi.fn() };
  handlers.onOpen({}, ws);
  const payloads = () => ws.send.mock.calls.map(([raw]) => JSON.parse(raw));
  return { handlers, hub, subscriber, terminalSessions, taskRegistry, ws, payloads };
}

describe("chat terminal websocket integration", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("forwards lifecycle state and bounded output batches directly from terminal events", () => {
    const { subscriber, payloads } = makeHarness();

    subscriber({ type: "terminal_started", terminalId: "term_1", terminal: terminal() }, "/sessions/a.jsonl");
    subscriber({ type: "terminal_output", terminalId: "term_1", seq: 1, data: "a" }, "/sessions/a.jsonl");
    subscriber({ type: "terminal_output", terminalId: "term_1", seq: 2, data: "b" }, "/sessions/a.jsonl");
    vi.advanceTimersByTime(50);

    expect(payloads()).toEqual([
      {
        type: "terminal_state",
        sessionId: "sess_a",
        sessionPath: "/sessions/a.jsonl",
        terminal: terminal(),
        studioId: "studio_a",
      },
      {
        type: "terminal_output",
        terminalId: "term_1",
        sessionId: "sess_a",
        sessionPath: "/sessions/a.jsonl",
        chunks: [{ seq: 1, data: "a" }, { seq: 2, data: "b" }],
        studioId: "studio_a",
      },
    ]);
  });

  it("serves snapshot and tail requests through stable session identity", async () => {
    const { handlers, terminalSessions, ws, payloads } = makeHarness();

    handlers.onMessage({
      data: JSON.stringify({
        type: "terminal_snapshot_request",
        sessionId: "sess_a",
        sessionPath: "/sessions/a.jsonl",
      }),
    }, ws);
    handlers.onMessage({
      data: JSON.stringify({
        type: "terminal_tail_request",
        sessionId: "sess_a",
        sessionPath: "/sessions/a.jsonl",
        terminalId: "term_1",
      }),
    }, ws);
    await vi.runAllTimersAsync();

    expect(terminalSessions.list).toHaveBeenCalledWith("/sessions/a.jsonl");
    expect(terminalSessions.readTail).toHaveBeenCalledWith(expect.objectContaining({
      sessionPath: "/sessions/a.jsonl",
      terminalId: "term_1",
      sinceSeq: null,
    }));
    expect(payloads().map((message) => message.type)).toEqual([
      "terminal_snapshot",
      "terminal_tail",
    ]);
    expect(payloads()[0]).toMatchObject({
      sessionId: "sess_a",
      sessionPath: "/sessions/a.jsonl",
      terminals: [{ terminalId: "term_1" }],
    });
    expect(payloads()[1]).toMatchObject({
      sessionId: "sess_a",
      sessionPath: "/sessions/a.jsonl",
      terminalId: "term_1",
      chunks: [{ seq: 1, data: "ready\n" }],
    });
  });

  it("stops only a terminal owned by the requested stable session", async () => {
    const { handlers, terminalSessions, ws, payloads } = makeHarness();

    handlers.onMessage({
      data: JSON.stringify({
        type: "terminal_close_request",
        sessionId: "sess_a",
        sessionPath: "/sessions/a.jsonl",
        terminalId: "term_1",
      }),
    }, ws);
    await vi.runAllTimersAsync();

    expect(terminalSessions.close).toHaveBeenCalledWith({
      sessionPath: "/sessions/a.jsonl",
      terminalId: "term_1",
    });
    expect(payloads()).toContainEqual(expect.objectContaining({
      type: "terminal_close_result",
      terminalId: "term_1",
      status: "killed",
    }));
  });

  it("rejects terminal close when no terminal manager is wired", async () => {
    const { handlers, terminalSessions, ws, payloads } = makeHarness();
    // 引擎未接线终端管理器：不得把「什么都没发生」谎报成 already_stopped
    terminalSessions.close = undefined;

    handlers.onMessage({
      data: JSON.stringify({
        type: "terminal_close_request",
        sessionId: "sess_a",
        sessionPath: "/sessions/a.jsonl",
        terminalId: "term_1",
      }),
    }, ws);
    await vi.runAllTimersAsync();

    expect(payloads()).toContainEqual(expect.objectContaining({
      type: "terminal_close_result",
      terminalId: "term_1",
      status: "rejected",
      reason: "terminal_unavailable",
    }));
  });

  it("refuses to stop a subagent whose parent belongs to another session", async () => {
    const { handlers, taskRegistry, ws, payloads } = makeHarness();
    taskRegistry.query.mockReturnValue({
      taskId: "task-other",
      type: "subagent",
      parentSessionId: "sess_other",
      parentSessionPath: "/sessions/other.jsonl",
      status: "running",
    });

    handlers.onMessage({
      data: JSON.stringify({
        type: "subagent_stop_request",
        sessionId: "sess_a",
        sessionPath: "/sessions/a.jsonl",
        taskId: "task-other",
      }),
    }, ws);
    await vi.runAllTimersAsync();

    expect(taskRegistry.abort).not.toHaveBeenCalled();
    expect(payloads()).toContainEqual(expect.objectContaining({
      type: "subagent_stop_result",
      taskId: "task-other",
      status: "rejected",
      reason: "session_mismatch",
    }));
  });

  it("does not abort a subagent that has already reached a final state", async () => {
    const { handlers, taskRegistry, ws, payloads } = makeHarness();
    taskRegistry.query.mockReturnValue({
      taskId: "task-complete",
      type: "subagent",
      parentSessionId: "sess_a",
      parentSessionPath: "/sessions/a.jsonl",
      status: "completed",
    });

    handlers.onMessage({
      data: JSON.stringify({
        type: "subagent_stop_request",
        sessionId: "sess_a",
        sessionPath: "/sessions/a.jsonl",
        taskId: "task-complete",
      }),
    }, ws);
    await vi.runAllTimersAsync();

    expect(taskRegistry.abort).not.toHaveBeenCalled();
    expect(payloads()).toContainEqual(expect.objectContaining({
      type: "subagent_stop_result",
      taskId: "task-complete",
      status: "already_stopped",
    }));
  });
});
