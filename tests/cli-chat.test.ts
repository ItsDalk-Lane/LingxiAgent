import { describe, expect, it, vi } from "vitest";
import {
  cliChatMessageMatchesSession,
  createCliChatAbortMessage,
  createCliChatPromptMessage,
  formatSessionLine,
  planCliInterrupt,
  printStatus,
  printSessions,
  reduceCliChatStreamIdentity,
  selectSession,
} from "../cli/chat.ts";

describe("CLI chat session helpers", () => {
  const sessions = [
    { path: "/a.json", title: "Alpha", agentName: "Hana", modified: "2026-05-19T10:00:00.000Z" },
    { path: "/b.json", firstMessage: "Beta first", agentId: "agent-b", modified: null },
  ];

  it("selects the latest session by default", () => {
    expect((selectSession as any)(sessions)).toBe(sessions[0]);
  });

  it("selects one-based session indices", () => {
    expect(selectSession(sessions, "2")).toBe(sessions[1]);
  });

  it("selects exact session paths", () => {
    expect(selectSession(sessions, "/b.json")).toBe(sessions[1]);
  });

  it("formats recent session rows for terminal display", () => {
    const line = formatSessionLine(sessions[0], 1);
    expect(line).toContain("Alpha");
    expect(line).toContain("Hana");
  });
});

describe("CLI status and list failure boundaries", () => {
  it("does not present a connection hint as authenticated identity", async () => {
    const output = vi.spyOn(console, "log").mockImplementation(() => {});
    try {
      await printStatus({
        health: async () => ({ agent: "Hana", version: "1.0" }),
        identity: async () => { throw new Error("unauthorized"); },
      } as any, { baseUrl: "http://127.0.0.1:1", source: "server-info" });
      const rendered = output.mock.calls.flat().join("\n");
      expect(rendered).toContain("unavailable (identity check failed)");
      expect(rendered).not.toContain("server-info");
    } finally {
      output.mockRestore();
    }
  });

  it("prints at most 20 sessions and preserves the empty-list message", async () => {
    const output = vi.spyOn(console, "log").mockImplementation(() => {});
    try {
      await printSessions({ sessions: async () => [] } as any);
      expect(output.mock.calls.flat().join("\n")).toContain("No sessions yet.");
      output.mockClear();
      await printSessions({ sessions: async () => Array.from({ length: 24 }, (_, index) => ({ path: `s${index}`, title: `S${index}` })) } as any);
      expect(output).toHaveBeenCalledTimes(20);
    } finally {
      output.mockRestore();
    }
  });
});

describe("standalone CLI session stream contract", () => {
  const identity = {
    sessionId: "sess-a",
    sessionPath: "/tmp/a.jsonl",
    streamId: "stream-a",
    isStreaming: true,
  };

  it("includes the immutable session identity in prompts", () => {
    expect(createCliChatPromptMessage(identity, "hello")).toEqual({
      type: "prompt",
      text: "hello",
      sessionId: "sess-a",
      sessionPath: "/tmp/a.jsonl",
    });
    expect(createCliChatPromptMessage({ sessionPath: "/tmp/a.jsonl" }, "hello")).toBeNull();
  });

  it("only builds abort requests when sessionId, sessionPath, and streamId are all present", () => {
    expect(createCliChatAbortMessage(identity)).toEqual({
      type: "abort",
      sessionId: "sess-a",
      sessionPath: "/tmp/a.jsonl",
      streamId: "stream-a",
    });
    expect(createCliChatAbortMessage({ ...identity, sessionId: null })).toBeNull();
    expect(createCliChatAbortMessage({ ...identity, sessionPath: null })).toBeNull();
    expect(createCliChatAbortMessage({ ...identity, streamId: null })).toBeNull();
  });

  it("keeps the active stream after abort_rejected and only clears it on a matching terminal event", () => {
    const rejected = reduceCliChatStreamIdentity(identity, {
      type: "abort_rejected",
      sessionId: "sess-a",
      sessionPath: "/tmp/a.jsonl",
      streamId: "stream-new",
      reason: "stale_stream",
    });
    expect(rejected).toEqual({ ...identity, streamId: "stream-new", isStreaming: true });

    const staleEnd = reduceCliChatStreamIdentity(rejected, {
      type: "turn_end",
      sessionId: "sess-a",
      sessionPath: "/tmp/a.jsonl",
      streamId: "stream-a",
    });
    expect(staleEnd).toEqual(rejected);

    const ended = reduceCliChatStreamIdentity(staleEnd, {
      type: "status",
      sessionId: "sess-a",
      sessionPath: "/tmp/a.jsonl",
      streamId: "stream-new",
      isStreaming: false,
    });
    expect(ended).toEqual({ ...identity, streamId: null, isStreaming: false });
  });

  it("does not let another session mutate the tracked stream", () => {
    expect(cliChatMessageMatchesSession(identity, {
      sessionId: "sess-b",
      sessionPath: "/tmp/b.jsonl",
    })).toBe(false);
    expect(reduceCliChatStreamIdentity(identity, {
      type: "status",
      sessionId: "sess-b",
      sessionPath: "/tmp/b.jsonl",
      streamId: "stream-b",
      isStreaming: false,
    })).toEqual(identity);
    expect(cliChatMessageMatchesSession(identity, { type: "text_delta", delta: "untagged" })).toBe(false);
    expect(cliChatMessageMatchesSession({ sessionPath: "/tmp/a.jsonl" }, {
      type: "text_delta", sessionId: "sess-b", delta: "wrong session with only an ID",
    })).toBe(false);
    expect(cliChatMessageMatchesSession({ sessionId: "sess-a" }, {
      type: "tool_start", sessionPath: "/tmp/b.jsonl", name: "wrong session tool",
    })).toBe(false);
    expect(cliChatMessageMatchesSession({ sessionPath: "/tmp/a.jsonl" }, {
      type: "error", sessionId: "sess-b", message: "wrong session error",
    })).toBe(false);
    expect(cliChatMessageMatchesSession({ sessionPath: "/tmp/a.jsonl" }, {
      type: "text_delta", sessionPath: "/tmp/a.jsonl", delta: "right session",
    })).toBe(true);
  });

  it("Ctrl+C targets only the active stream and waits for an unknown stream identity", () => {
    expect(planCliInterrupt({ ...identity, isStreaming: false })).toEqual({ kind: "exit" });
    expect(planCliInterrupt({ ...identity, streamId: null, pendingPrompt: true })).toEqual({ kind: "wait" });
    expect(planCliInterrupt(identity)).toEqual({
      kind: "abort",
      message: { type: "abort", sessionId: "sess-a", sessionPath: "/tmp/a.jsonl", streamId: "stream-a" },
    });
    expect(planCliInterrupt({ ...identity, abortRequestedStreamId: "stream-a" })).toEqual({ kind: "already-requested" });
  });
});
