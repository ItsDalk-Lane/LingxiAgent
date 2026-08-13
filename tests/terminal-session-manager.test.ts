import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import { TerminalSessionManager } from "../lib/terminal/terminal-session-manager.ts";

type CapturedTerminalEvent = {
  type: string;
  terminalId?: string;
  status?: string;
  seq?: number;
  data?: string;
  terminal?: {
    terminalId?: string;
    sessionId?: string | null;
    sessionPath?: string;
    status?: string;
    exitCode?: number | null;
    signal?: string | null;
    seq?: number;
  };
};

function makeFakeBackend() {
  const handles = [];
  return {
    handles,
    spawn: vi.fn((opts) => {
      const handle = {
        writes: [],
        killed: false,
        write(data) {
          this.writes.push(data);
          opts.onData(`echo:${data}`);
        },
        kill() {
          this.killed = true;
          opts.onExit({ exitCode: null, signal: "SIGTERM" });
        },
        emit(data) {
          opts.onData(data);
        },
        exit(exitCode = 0) {
          opts.onExit({ exitCode, signal: null });
        },
      };
      handles.push(handle);
      return handle;
    }),
  };
}

describe("TerminalSessionManager", () => {
  let tmpDir;
  let backend;

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-terminal-"));
    backend = makeFakeBackend();
  });

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  it("keeps terminal ownership scoped to the creating session", async () => {
    const manager = new TerminalSessionManager({
      lingxiHome: tmpDir,
      createBackend: () => backend,
      now: () => 1770000000000,
    });
    const sessionPath = path.join(tmpDir, "agents", "hana", "sessions", "s1.jsonl");
    const otherSessionPath = path.join(tmpDir, "agents", "hana", "sessions", "s2.jsonl");

    const started = await manager.start({
      toolCallId: "call-terminal-1",
      sessionPath,
      agentId: "hana",
      cwd: tmpDir,
      command: "npm run dev",
      label: "dev server",
    });
    backend.handles[0].emit("ready\n");

    expect(started).toMatchObject({
      sessionPath,
      agentId: "hana",
      cwd: tmpDir,
      command: "npm run dev",
      label: "dev server",
      status: "running",
      seq: 0,
    });
    expect(started.terminalId).toMatch(/^term_/);

    expect(manager.list(sessionPath).terminals).toHaveLength(1);
    expect(manager.list(sessionPath).terminals[0].toolCallId).toBe("call-terminal-1");
    expect(manager.list(otherSessionPath).terminals).toEqual([]);

    const read = manager.read({
      sessionPath,
      terminalId: started.terminalId,
      sinceSeq: 0,
    });
    expect(read).toMatchObject({
      terminalId: started.terminalId,
      status: "running",
      seq: 1,
      output: "ready\n",
    });

    expect(() => manager.read({
      sessionPath: otherSessionPath,
      terminalId: started.terminalId,
    })).toThrow(/belongs to another session/);
  });

  it("rejects start with a missing cwd before touching the pty backend", async () => {
    const manager = new TerminalSessionManager({
      lingxiHome: tmpDir,
      createBackend: () => backend,
      now: () => 1770000000000,
    });
    const sessionPath = path.join(tmpDir, "agents", "hana", "sessions", "s1.jsonl");
    const gone = path.join(tmpDir, "gone-workspace");

    await expect(
      manager.start({ sessionPath, agentId: "hana", cwd: gone }),
    ).rejects.toMatchObject({ code: "LINGXI_EXEC_CWD_MISSING", cwd: gone });

    expect(backend.spawn).not.toHaveBeenCalled();
    expect(manager.list(sessionPath).terminals).toEqual([]);
  });

  it("writes only to a running terminal owned by the same session", async () => {
    const manager = new TerminalSessionManager({
      lingxiHome: tmpDir,
      createBackend: () => backend,
    });
    const sessionPath = path.join(tmpDir, "agents", "hana", "sessions", "s1.jsonl");
    const started = await manager.start({ sessionPath, agentId: "hana", cwd: tmpDir });

    const written = manager.write({
      sessionPath,
      terminalId: started.terminalId,
      chars: "pwd\n",
    });

    expect(backend.handles[0].writes).toEqual(["pwd\n"]);
    expect(written.output).toBe("echo:pwd\n");
    expect(written.seq).toBe(1);

    manager.close({ sessionPath, terminalId: started.terminalId });

    expect(() => manager.write({
      sessionPath,
      terminalId: started.terminalId,
      chars: "date\n",
    })).toThrow(/is not running/);
  });

  it("closes live terminals for one session without touching another session", async () => {
    const manager = new TerminalSessionManager({
      lingxiHome: tmpDir,
      createBackend: () => backend,
    });
    const sessionPath = path.join(tmpDir, "agents", "hana", "sessions", "s1.jsonl");
    const otherSessionPath = path.join(tmpDir, "agents", "hana", "sessions", "s2.jsonl");
    const first = await manager.start({ sessionPath, agentId: "hana", cwd: tmpDir });
    const second = await manager.start({ sessionPath: otherSessionPath, agentId: "hana", cwd: tmpDir });

    const closed = manager.closeForSession(sessionPath);

    expect(closed).toHaveLength(1);
    expect(closed[0]).toMatchObject({
      terminalId: first.terminalId,
      status: "killed",
    });
    expect(backend.handles[0].killed).toBe(true);
    expect(backend.handles[1].killed).toBe(false);
    expect(manager.read({
      sessionPath: otherSessionPath,
      terminalId: second.terminalId,
    }).status).toBe("running");
  });

  it("closes live terminals through a moved session path with the same session id", async () => {
    const originalPath = path.join(tmpDir, "agents", "hana", "sessions", "original.jsonl");
    const movedPath = path.join(tmpDir, "agents", "hana", "sessions", "archived", "renamed.jsonl");
    const sessionId = "sess_terminal_stable";
    const manager = new TerminalSessionManager({
      lingxiHome: tmpDir,
      createBackend: () => backend,
      getSessionIdForPath: (sessionPath: string) => (
        sessionPath === originalPath || sessionPath === movedPath ? sessionId : null
      ),
    });
    const started = await manager.start({ sessionPath: originalPath, agentId: "hana", cwd: tmpDir });

    const closed = manager.closeForSession(movedPath);

    expect(closed).toHaveLength(1);
    expect(closed[0]).toMatchObject({
      terminalId: started.terminalId,
      sessionPath: movedPath,
      status: "killed",
    });
    expect(backend.handles[0].killed).toBe(true);
  });

  it("uses the backend dispose contract before falling back to kill", async () => {
    const manager = new TerminalSessionManager({
      lingxiHome: tmpDir,
      createBackend: () => backend,
    });
    const sessionPath = path.join(tmpDir, "agents", "hana", "sessions", "s1.jsonl");
    const started = await manager.start({ sessionPath, agentId: "hana", cwd: tmpDir });
    const handle = backend.handles[0];
    handle.dispose = vi.fn();

    manager.close({ sessionPath, terminalId: started.terminalId });

    expect(handle.dispose).toHaveBeenCalledWith({
      terminalId: started.terminalId,
      sessionPath,
      reason: "close",
    });
    expect(handle.killed).toBe(false);
  });

  it("marks previously running terminals stale after manager restart and preserves transcript", async () => {
    const sessionPath = path.join(tmpDir, "agents", "hana", "sessions", "s1.jsonl");
    const manager = new TerminalSessionManager({
      lingxiHome: tmpDir,
      createBackend: () => backend,
    });
    const started = await manager.start({ sessionPath, agentId: "hana", cwd: tmpDir });
    backend.handles[0].emit("line before restart\n");

    const restarted = new TerminalSessionManager({
      lingxiHome: tmpDir,
      createBackend: () => makeFakeBackend(),
    });

    const terminals = restarted.list(sessionPath).terminals;
    expect(terminals).toHaveLength(1);
    expect(terminals[0]).toMatchObject({
      terminalId: started.terminalId,
      sessionPath,
      status: "stale",
    });

    expect(restarted.read({
      sessionPath,
      terminalId: started.terminalId,
    }).output).toBe("line before restart\n");

    expect(restarted.readTail({
      sessionPath,
      terminalId: started.terminalId,
    })).toMatchObject({
      terminalId: started.terminalId,
      status: "stale",
      chunks: [{ seq: 1, data: "line before restart\n" }],
      truncated: false,
    });
  });

  it("emits complete lifecycle snapshots while keeping output events metadata-light", async () => {
    const events: Array<{ event: CapturedTerminalEvent; sessionPath: string }> = [];
    let now = 1770000000000;
    const manager = new TerminalSessionManager({
      lingxiHome: tmpDir,
      createBackend: () => backend,
      getSessionIdForPath: () => "sess_terminal",
      now: () => now++,
      emitEvent: (event, sessionPath) => events.push({ event, sessionPath }),
    });
    const sessionPath = path.join(tmpDir, "agents", "hana", "sessions", "events.jsonl");

    const started = await manager.start({
      sessionPath,
      agentId: "hana",
      cwd: tmpDir,
      command: "npm test",
      label: "tests",
    });
    backend.handles[0].emit("first\n");
    backend.handles[0].exit(0);

    expect(events.map(({ event }) => event.type)).toEqual([
      "terminal_started",
      "terminal_output",
      "terminal_exited",
    ]);
    expect(events[0]).toMatchObject({
      sessionPath,
      event: {
        terminalId: started.terminalId,
        terminal: {
          terminalId: started.terminalId,
          sessionId: "sess_terminal",
          sessionPath,
          agentId: "hana",
          cwd: tmpDir,
          command: "npm test",
          label: "tests",
          status: "running",
          seq: 0,
        },
      },
    });
    expect(events[1].event).toEqual({
      type: "terminal_output",
      terminalId: started.terminalId,
      status: "running",
      seq: 1,
      data: "first\n",
    });
    expect(events[1].event).not.toHaveProperty("terminal");
    expect(events[2].event.terminal).toMatchObject({
      terminalId: started.terminalId,
      sessionId: "sess_terminal",
      status: "exited",
      exitCode: 0,
      signal: null,
      seq: 1,
    });
  });

  it("preserves nonzero exits and killed state in lifecycle snapshots", async () => {
    const events: CapturedTerminalEvent[] = [];
    const manager = new TerminalSessionManager({
      lingxiHome: tmpDir,
      createBackend: () => backend,
      emitEvent: (event) => events.push(event),
    });
    const sessionPath = path.join(tmpDir, "agents", "hana", "sessions", "terminal-states.jsonl");
    const failed = await manager.start({ sessionPath, cwd: tmpDir });
    backend.handles[0].exit(7);
    const killed = await manager.start({ sessionPath, cwd: tmpDir });
    manager.close({ sessionPath, terminalId: killed.terminalId });
    backend.handles[1].exit(0);

    expect(manager.read({ sessionPath, terminalId: failed.terminalId })).toMatchObject({
      status: "exited",
      exitCode: 7,
    });
    expect(manager.read({ sessionPath, terminalId: killed.terminalId })).toMatchObject({
      status: "killed",
      exitCode: 0,
    });
    expect(events.filter((event) => event.type === "terminal_closed").at(-1)?.terminal).toMatchObject({
      terminalId: killed.terminalId,
      status: "killed",
    });
    expect(events.filter((event) => event.type === "terminal_exited").at(-1)?.terminal).toMatchObject({
      terminalId: killed.terminalId,
      status: "killed",
      exitCode: 0,
    });
  });

  it("reads a bounded transcript tail without crossing session ownership", async () => {
    const manager = new TerminalSessionManager({
      lingxiHome: tmpDir,
      createBackend: () => backend,
    });
    const sessionPath = path.join(tmpDir, "agents", "hana", "sessions", "tail.jsonl");
    const otherSessionPath = path.join(tmpDir, "agents", "hana", "sessions", "other.jsonl");
    const started = await manager.start({ sessionPath, cwd: tmpDir });
    backend.handles[0].emit("one\n");
    backend.handles[0].emit("two\n");
    backend.handles[0].emit("three\n");

    expect(manager.readTail({
      sessionPath,
      terminalId: started.terminalId,
      maxBytes: 64,
      maxChunks: 2,
    })).toMatchObject({
      chunks: [
        { seq: 2, data: "two\n" },
        { seq: 3, data: "three\n" },
      ],
      truncated: true,
      lastSeq: 3,
    });

    expect(manager.readTail({
      sessionPath,
      terminalId: started.terminalId,
      sinceSeq: 1,
      maxBytes: 64,
      maxChunks: 2,
    })).toMatchObject({
      chunks: [
        { seq: 2, data: "two\n" },
        { seq: 3, data: "three\n" },
      ],
      truncated: false,
      lastSeq: 3,
    });

    const byteBounded = manager.readTail({
      sessionPath,
      terminalId: started.terminalId,
      maxBytes: 6,
      maxChunks: 50,
    });
    expect(byteBounded.chunks).toEqual([{ seq: 3, data: "three\n" }]);
    expect(Buffer.byteLength(byteBounded.output, "utf8")).toBeLessThanOrEqual(6);
    expect(byteBounded.truncated).toBe(true);

    expect(() => manager.readTail({
      sessionPath: otherSessionPath,
      terminalId: started.terminalId,
    })).toThrow(/belongs to another session/);
  });
});
