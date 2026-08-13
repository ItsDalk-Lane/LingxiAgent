import {
  TERMINAL_TAIL_DEFAULT_MAX_BYTES,
  TERMINAL_TAIL_DEFAULT_MAX_CHUNKS,
} from "../shared/terminal-ui-contract.ts";
import type { TerminalSessionManager } from "../lib/terminal/terminal-session-manager.ts";
import { wsSend } from "./ws-protocol.ts";

export const TERMINAL_OUTPUT_MAX_BATCH_DELAY_MS = 50;
export const TERMINAL_OUTPUT_MAX_BATCH_BYTES = 64 * 1024;

const LIFECYCLE_EVENT_TYPES = new Set([
  "terminal_started",
  "terminal_exited",
  "terminal_closed",
]);

interface TerminalWsBridgeOptions {
  terminalSessions: Partial<Pick<TerminalSessionManager, "list" | "readTail">>;
  resolveSessionId?: (sessionPath: string) => string | null;
  broadcast: (message: Record<string, unknown>) => void;
  send?: typeof wsSend;
  maxBatchDelay?: number;
  maxBatchBytes?: number;
}

function nonEmptyString(value) {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function normalizedSeq(value) {
  if (value === undefined || value === null || value === "") return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? Math.max(0, Math.floor(parsed)) : null;
}

function normalizeTerminal(terminal, { sessionId, sessionPath }) {
  return {
    ...terminal,
    sessionId,
    sessionPath,
  };
}

export function createTerminalWsBridge({
  terminalSessions,
  resolveSessionId = () => null,
  broadcast,
  send = wsSend,
  maxBatchDelay = TERMINAL_OUTPUT_MAX_BATCH_DELAY_MS,
  maxBatchBytes = TERMINAL_OUTPUT_MAX_BATCH_BYTES,
}: TerminalWsBridgeOptions) {
  const pendingByTerminal = new Map();

  function resolvedIdentity(sessionPath, terminal = null, requestedSessionId = null) {
    const normalizedPath = nonEmptyString(sessionPath) || nonEmptyString(terminal?.sessionPath);
    const resolvedId = normalizedPath ? nonEmptyString(resolveSessionId?.(normalizedPath)) : null;
    return {
      sessionPath: normalizedPath,
      sessionId: resolvedId || nonEmptyString(requestedSessionId) || nonEmptyString(terminal?.sessionId),
    };
  }

  function flush(terminalId) {
    const pending = pendingByTerminal.get(terminalId);
    if (!pending) return false;
    pendingByTerminal.delete(terminalId);
    if (pending.timer) clearTimeout(pending.timer);
    if (!pending.chunks.length || !pending.sessionPath) return false;
    broadcast({
      type: "terminal_output",
      terminalId,
      sessionId: pending.sessionId,
      sessionPath: pending.sessionPath,
      chunks: pending.chunks,
    });
    return true;
  }

  function queueOutput(event, sessionPath) {
    const id = nonEmptyString(event?.terminalId);
    const seq = normalizedSeq(event?.seq);
    const data = typeof event?.data === "string" ? event.data : "";
    if (!id || seq === null || !data) return;
    const identity = resolvedIdentity(sessionPath);
    if (!identity.sessionPath) return;
    let pending = pendingByTerminal.get(id);
    if (!pending) {
      pending = {
        terminalId: id,
        ...identity,
        chunks: [],
        bytes: 0,
        timer: null,
      };
      pendingByTerminal.set(id, pending);
    } else {
      pending.sessionPath = identity.sessionPath;
      pending.sessionId = identity.sessionId;
    }
    pending.chunks.push({ seq, data });
    pending.bytes += Buffer.byteLength(data, "utf8");
    if (pending.bytes >= maxBatchBytes) {
      flush(id);
      return;
    }
    if (!pending.timer) {
      pending.timer = setTimeout(() => flush(id), maxBatchDelay);
      pending.timer.unref?.();
    }
  }

  function broadcastState(event, sessionPath) {
    const terminal = event?.terminal;
    if (!terminal || typeof terminal !== "object") return;
    const identity = resolvedIdentity(sessionPath, terminal);
    if (!identity.sessionPath) return;
    broadcast({
      type: "terminal_state",
      sessionId: identity.sessionId,
      sessionPath: identity.sessionPath,
      terminal: normalizeTerminal(terminal, identity),
    });
  }

  function handleEvent(event, sessionPath) {
    if (event?.type === "terminal_output") {
      queueOutput(event, sessionPath);
      return true;
    }
    if (!LIFECYCLE_EVENT_TYPES.has(event?.type)) return false;
    const terminalId = nonEmptyString(event?.terminalId) || nonEmptyString(event?.terminal?.terminalId);
    if (terminalId && event.type !== "terminal_started") flush(terminalId);
    broadcastState(event, sessionPath);
    return true;
  }

  function sendSnapshot(ws, { sessionId = null, sessionPath }) {
    if (!terminalSessions || typeof terminalSessions.list !== "function") {
      throw new Error("terminal sessions unavailable");
    }
    const identity = resolvedIdentity(sessionPath, null, sessionId);
    if (!identity.sessionPath) throw new Error("sessionPath is required");
    const snapshot = terminalSessions.list(identity.sessionPath);
    const terminals = (Array.isArray(snapshot?.terminals) ? snapshot.terminals : []).map((terminal) => (
      normalizeTerminal(terminal, identity)
    ));
    send(ws, {
      type: "terminal_snapshot",
      sessionId: identity.sessionId,
      sessionPath: identity.sessionPath,
      terminals,
    });
  }

  function sendTail(ws, { sessionId = null, sessionPath, terminalId, sinceSeq = null }) {
    if (!terminalSessions || typeof terminalSessions.readTail !== "function") {
      throw new Error("terminal transcript tail unavailable");
    }
    const identity = resolvedIdentity(sessionPath, null, sessionId);
    const id = nonEmptyString(terminalId);
    if (!identity.sessionPath) throw new Error("sessionPath is required");
    if (!id) throw new Error("terminalId is required");
    const normalizedSince = normalizedSeq(sinceSeq);
    const result = terminalSessions.readTail({
      sessionPath: identity.sessionPath,
      terminalId: id,
      sinceSeq: normalizedSince,
      maxBytes: TERMINAL_TAIL_DEFAULT_MAX_BYTES,
      maxChunks: TERMINAL_TAIL_DEFAULT_MAX_CHUNKS,
    });
    const {
      output: _output,
      chunks,
      sinceSeq: returnedSinceSeq,
      lastSeq,
      truncated,
      ...terminal
    } = result;
    void _output;
    send(ws, {
      type: "terminal_tail",
      sessionId: identity.sessionId,
      sessionPath: identity.sessionPath,
      terminalId: id,
      terminal: normalizeTerminal(terminal, identity),
      chunks: Array.isArray(chunks) ? chunks : [],
      sinceSeq: returnedSinceSeq ?? normalizedSince,
      lastSeq: normalizedSeq(lastSeq) ?? normalizedSeq(terminal.seq) ?? 0,
      truncated: truncated === true,
    });
  }

  return {
    handleEvent,
    flush,
    sendSnapshot,
    sendTail,
  };
}
