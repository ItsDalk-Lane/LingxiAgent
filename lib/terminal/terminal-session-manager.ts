import fs from "fs";
import path from "path";
import { atomicWriteSync } from "../../shared/safe-fs.ts";
import { randomBytes } from "crypto";
import { assertExecutionCwd } from "../shell/execution-cwd.ts";
import {
  TERMINAL_TAIL_DEFAULT_MAX_BYTES,
  TERMINAL_TAIL_DEFAULT_MAX_CHUNKS,
  TERMINAL_TAIL_HARD_MAX_BYTES,
  TERMINAL_TAIL_HARD_MAX_CHUNKS,
} from "../../shared/terminal-ui-contract.ts";

const TERMINAL_ROOT = path.join(".ephemeral", "terminal-sessions");

function defaultNow() {
  return Date.now();
}

async function createDefaultBackend() {
  const mod = await import("./node-pty-backend.ts");
  return mod.createAsyncNodePtyBackend();
}

function terminalId() {
  return `term_${Date.now().toString(36)}_${randomBytes(4).toString("hex")}`;
}

function asNonEmptyString(value, name) {
  const text = typeof value === "string" ? value : "";
  if (!text.trim()) throw new Error(`${name} is required`);
  return text;
}

function normalizeString(value) {
  return typeof value === "string" ? value : "";
}

function publicEntry(entry, sessionId = null) {
  const toolCallId = normalizeString(entry.toolCallId);
  return {
    terminalId: entry.terminalId,
    ...(toolCallId ? { toolCallId } : {}),
    sessionId,
    sessionPath: entry.sessionPath,
    agentId: entry.agentId,
    cwd: entry.cwd,
    command: entry.command,
    label: entry.label,
    status: entry.status,
    seq: entry.seq,
    createdAt: entry.createdAt,
    lastActivityAt: entry.lastActivityAt,
    exitedAt: entry.exitedAt ?? null,
    exitCode: entry.exitCode ?? null,
    signal: entry.signal ?? null,
    transcriptPath: entry.transcriptPath,
  };
}

function boundedPositiveInteger(value, fallback, hardMax) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) return fallback;
  return Math.min(Math.floor(parsed), hardMax);
}

function normalizedSinceSeq(value) {
  if (value === undefined || value === null || value === "") return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? Math.max(0, Math.floor(parsed)) : null;
}

function utf8Tail(text, maxBytes) {
  const raw = Buffer.from(text, "utf8");
  if (raw.length <= maxBytes) return text;
  let start = raw.length - maxBytes;
  while (start < raw.length && (raw[start] & 0xc0) === 0x80) start += 1;
  return raw.subarray(start).toString("utf8");
}

export class TerminalSessionManager {

declare _backendPromise: any;

declare _bySession: any;

declare _createBackend: any;

declare _emitEvent: any;

declare _getSessionIdForPath: any;

declare _now: any;

declare _terminals: any;

declare lingxiHome: any;

declare root: any;
  constructor({
    lingxiHome,
    createBackend = createDefaultBackend,
    getSessionIdForPath = null,
    now = defaultNow,
    emitEvent = null,
  }: any = {}) {
    this.lingxiHome = asNonEmptyString(lingxiHome, "lingxiHome");
    this.root = path.join(this.lingxiHome, TERMINAL_ROOT);
    this._createBackend = createBackend;
    this._getSessionIdForPath = typeof getSessionIdForPath === "function" ? getSessionIdForPath : () => null;
    this._now = now;
    this._emitEvent = emitEvent;
    this._backendPromise = null;
    this._terminals = new Map();
    this._bySession = new Map();
    fs.mkdirSync(this.root, { recursive: true });
    this._loadPersistedTerminals();
  }

  async start({
    toolCallId = "",
    sessionPath,
    agentId = "",
    cwd,
    command = "",
    label = "",
    cols = 80,
    rows = 24,
    env,
  }: any = {}) {
    const normalizedSessionPath = asNonEmptyString(sessionPath, "sessionPath");
    const normalizedCwd = assertExecutionCwd(asNonEmptyString(cwd, "cwd"));
    const id = terminalId();
    const now = this._now();
    const entry = {
      terminalId: id,
      toolCallId: normalizeString(toolCallId),
      sessionPath: normalizedSessionPath,
      agentId: normalizeString(agentId),
      cwd: normalizedCwd,
      command: normalizeString(command),
      label: normalizeString(label),
      status: "running",
      seq: 0,
      createdAt: now,
      lastActivityAt: now,
      exitedAt: null,
      exitCode: null,
      signal: null,
      transcriptPath: this._transcriptPath(id),
      handle: null,
    };

    this._terminals.set(id, entry);
    this._index(entry);
    try {
      const backend = await this._getBackend();
      entry.handle = backend.spawn({
        terminalId: id,
        sessionPath: normalizedSessionPath,
        command: entry.command,
        cwd: normalizedCwd,
        cols,
        rows,
        env,
        onData: (data) => this._recordData(id, data),
        onExit: (result) => this._markExited(id, result),
      });
      this._persist(entry);
      this._emit("terminal_started", entry);
    } catch (err) {
      this._terminals.delete(id);
      this._bySession.get(this._sessionKeyForPath(normalizedSessionPath))?.delete(id);
      throw err;
    }
    return { ...this._publicEntry(entry), output: "" };
  }

  write({ sessionPath, terminalId, chars }: any = {}) {
    const entry = this._requireOwned({ sessionPath, terminalId });
    if (entry.status !== "running") {
      throw new Error(`terminal ${entry.terminalId} is not running`);
    }
    if (!entry.handle || typeof entry.handle.write !== "function") {
      throw new Error(`terminal ${entry.terminalId} has no live PTY handle`);
    }
    const sinceSeq = entry.seq;
    entry.handle.write(normalizeString(chars));
    return this.read({ sessionPath: entry.sessionPath, terminalId: entry.terminalId, sinceSeq });
  }

  read({ sessionPath, terminalId, sinceSeq = 0 }: any = {}) {
    const entry = this._requireOwned({ sessionPath, terminalId });
    const chunks = this._readTranscript(entry.transcriptPath, sinceSeq);
    return {
      ...this._publicEntry(entry),
      output: chunks.map((chunk) => chunk.data).join(""),
      chunks,
    };
  }

  /**
   * 只给界面使用的有界 transcript 读取。
   * 未传 sinceSeq 时返回最近窗口；传入后返回该序号之后仍能放进窗口的连续尾部。
   */
  readTail({
    sessionPath,
    terminalId,
    sinceSeq,
    maxBytes = TERMINAL_TAIL_DEFAULT_MAX_BYTES,
    maxChunks = TERMINAL_TAIL_DEFAULT_MAX_CHUNKS,
  }: any = {}) {
    const entry = this._requireOwned({ sessionPath, terminalId });
    const byteLimit = boundedPositiveInteger(
      maxBytes,
      TERMINAL_TAIL_DEFAULT_MAX_BYTES,
      TERMINAL_TAIL_HARD_MAX_BYTES,
    );
    const chunkLimit = boundedPositiveInteger(
      maxChunks,
      TERMINAL_TAIL_DEFAULT_MAX_CHUNKS,
      TERMINAL_TAIL_HARD_MAX_CHUNKS,
    );
    const normalizedSince = normalizedSinceSeq(sinceSeq);
    const tail = this._readTranscriptTail(entry.transcriptPath, {
      sinceSeq: normalizedSince,
      maxBytes: byteLimit,
      maxChunks: chunkLimit,
      lastSeq: entry.seq,
    });
    return {
      ...this._publicEntry(entry),
      output: tail.chunks.map((chunk) => chunk.data).join(""),
      chunks: tail.chunks,
      sinceSeq: normalizedSince,
      lastSeq: entry.seq,
      truncated: tail.truncated,
    };
  }

  close({ sessionPath, terminalId }: any = {}) {
    const entry = this._requireOwned({ sessionPath, terminalId });
    if (entry.status === "running") {
      entry.status = "killed";
      entry.exitedAt = this._now();
      entry.lastActivityAt = entry.exitedAt;
      try {
        if (typeof entry.handle?.dispose === "function") {
          entry.handle.dispose({
            terminalId: entry.terminalId,
            sessionPath: entry.sessionPath,
            reason: "close",
          });
        } else {
          entry.handle?.kill?.();
        }
      } finally {
        this._persist(entry);
        this._emit("terminal_closed", entry);
      }
    }
    return { ...this._publicEntry(entry), output: "" };
  }

  closeForSession(sessionPath) {
    const normalizedSessionPath = asNonEmptyString(sessionPath, "sessionPath");
    const ids = [...(this._bySession.get(this._sessionKeyForPath(normalizedSessionPath)) || [])];
    return ids.map((id) => this.close({
      sessionPath: normalizedSessionPath,
      terminalId: id,
    }));
  }

  closeAll() {
    const ids = [...this._terminals.keys()];
    return ids
      .map((id) => this._terminals.get(id))
      .filter(Boolean)
      .map((entry) => this.close({
        sessionPath: entry.sessionPath,
        terminalId: entry.terminalId,
      }));
  }

  list(sessionPath) {
    const normalizedSessionPath = asNonEmptyString(sessionPath, "sessionPath");
    const ids = this._bySession.get(this._sessionKeyForPath(normalizedSessionPath)) || new Set();
    const terminals = [...ids]
      .map((id) => this._terminals.get(id))
      .filter(Boolean)
      .map((entry) => this._entryMatchesSessionPath(entry, normalizedSessionPath)
        ? this._publicEntry({ ...entry, sessionPath: normalizedSessionPath })
        : this._publicEntry(entry))
      .sort((a, b) => a.createdAt - b.createdAt);
    return { sessionPath: normalizedSessionPath, terminals };
  }

  _getBackend() {
    if (!this._backendPromise) {
      this._backendPromise = Promise.resolve(this._createBackend());
    }
    return this._backendPromise;
  }

  _publicEntry(entry) {
    const sessionId = this._getSessionIdForPath?.(entry.sessionPath);
    const normalizedSessionId = typeof sessionId === "string" && sessionId.trim()
      ? sessionId.trim()
      : (typeof entry.sessionId === "string" && entry.sessionId.trim() ? entry.sessionId.trim() : null);
    return publicEntry(entry, normalizedSessionId);
  }

  _requireOwned({ sessionPath, terminalId }) {
    const id = asNonEmptyString(terminalId, "terminalId");
    const normalizedSessionPath = asNonEmptyString(sessionPath, "sessionPath");
    const entry = this._terminals.get(id);
    if (!entry) throw new Error(`terminal ${id} not found`);
    if (!this._entryMatchesSessionPath(entry, normalizedSessionPath)) {
      throw new Error(`terminal ${id} belongs to another session`);
    }
    if (entry.sessionPath !== normalizedSessionPath) {
      entry.sessionPath = normalizedSessionPath;
      this._persist(entry);
    }
    return entry;
  }

  _sessionKeyForPath(sessionPath) {
    const sessionId = this._getSessionIdForPath?.(sessionPath);
    return typeof sessionId === "string" && sessionId.trim() ? sessionId.trim() : sessionPath;
  }

  _entryMatchesSessionPath(entry, sessionPath) {
    return this._sessionKeyForPath(entry.sessionPath) === this._sessionKeyForPath(sessionPath);
  }

  _index(entry) {
    const key = this._sessionKeyForPath(entry.sessionPath);
    if (!this._bySession.has(key)) {
      this._bySession.set(key, new Set());
    }
    this._bySession.get(key).add(entry.terminalId);
  }

  _metadataPath(id) {
    return path.join(this.root, `${id}.json`);
  }

  _transcriptPath(id) {
    return path.join(this.root, `${id}.jsonl`);
  }

  _persist(entry) {
    fs.mkdirSync(this.root, { recursive: true });
    atomicWriteSync(this._metadataPath(entry.terminalId), JSON.stringify(this._publicEntry(entry), null, 2));
  }

  _appendTranscript(entry, data) {
    fs.mkdirSync(path.dirname(entry.transcriptPath), { recursive: true });
    fs.appendFileSync(entry.transcriptPath, JSON.stringify({
      seq: entry.seq,
      ts: entry.lastActivityAt,
      data,
    }) + "\n");
  }

  _recordData(id, data) {
    const entry = this._terminals.get(id);
    if (!entry) return;
    const text = Buffer.isBuffer(data) ? data.toString("utf8") : String(data ?? "");
    if (!text) return;
    entry.seq += 1;
    entry.lastActivityAt = this._now();
    this._appendTranscript(entry, text);
    this._persist(entry);
    this._emit("terminal_output", entry, { seq: entry.seq, data: text });
  }

  _markExited(id, result: any = {}) {
    const entry = this._terminals.get(id);
    if (!entry) return;
    if (entry.status === "running") {
      entry.status = "exited";
    }
    entry.exitCode = Number.isFinite(result.exitCode) ? result.exitCode : null;
    entry.signal = typeof result.signal === "string" ? result.signal : null;
    entry.exitedAt = this._now();
    entry.lastActivityAt = entry.exitedAt;
    entry.handle = null;
    this._persist(entry);
    this._emit("terminal_exited", entry);
  }

  _readTranscript(transcriptPath, sinceSeq = 0) {
    if (!fs.existsSync(transcriptPath)) return [];
    const minSeq = Number.isFinite(Number(sinceSeq)) ? Number(sinceSeq) : 0;
    const raw = fs.readFileSync(transcriptPath, "utf8");
    const chunks = [];
    for (const line of raw.split(/\n/)) {
      if (!line.trim()) continue;
      try {
        const item = JSON.parse(line);
        if (Number(item.seq) > minSeq) chunks.push(item);
      } catch {}
    }
    return chunks;
  }

  _readTranscriptTail(transcriptPath, { sinceSeq, maxBytes, maxChunks, lastSeq }) {
    if (!fs.existsSync(transcriptPath)) return { chunks: [], truncated: false };
    const size = fs.statSync(transcriptPath).size;
    if (size <= 0) return { chunks: [], truncated: false };

    // JSON 转义会放大控制字符；扫描窗口仍有固定硬上限，不会随 transcript 总大小增长。
    const scanBudget = Math.min(
      size,
      Math.max(64 * 1024, maxBytes * 8) + Math.min(maxChunks * 256, 512 * 1024),
    );
    const start = Math.max(0, size - scanBudget);
    const fd = fs.openSync(transcriptPath, "r");
    let raw;
    try {
      const buffer = Buffer.alloc(scanBudget);
      const bytesRead = fs.readSync(fd, buffer, 0, scanBudget, start);
      raw = buffer.subarray(0, bytesRead).toString("utf8");
    } finally {
      fs.closeSync(fd);
    }

    let omittedBeforeScan = start > 0;
    if (omittedBeforeScan) {
      const firstNewline = raw.indexOf("\n");
      if (firstNewline < 0) return { chunks: [], truncated: lastSeq > (sinceSeq ?? 0) };
      raw = raw.slice(firstNewline + 1);
    }

    const parsed = [];
    for (const line of raw.split(/\n/)) {
      if (!line.trim()) continue;
      try {
        const item = JSON.parse(line);
        const seq = Number(item?.seq);
        if (!Number.isFinite(seq) || typeof item?.data !== "string") continue;
        if (sinceSeq !== null && seq <= sinceSeq) continue;
        parsed.push({ seq, data: item.data });
      } catch {}
    }
    parsed.sort((a, b) => a.seq - b.seq);

    const selected = [];
    let usedBytes = 0;
    let omittedByLimit = false;
    for (let index = parsed.length - 1; index >= 0; index -= 1) {
      const item = parsed[index];
      if (selected.length >= maxChunks) {
        omittedByLimit = true;
        break;
      }
      const remainingBytes = maxBytes - usedBytes;
      const itemBytes = Buffer.byteLength(item.data, "utf8");
      if (itemBytes > remainingBytes) {
        if (selected.length === 0 && remainingBytes > 0) {
          selected.unshift({
            seq: item.seq,
            data: utf8Tail(item.data, remainingBytes),
            truncatedStart: true,
          });
        }
        omittedByLimit = true;
        break;
      }
      selected.unshift(item);
      usedBytes += itemBytes;
    }

    if (sinceSeq !== null && omittedBeforeScan) {
      const firstParsedSeq = parsed[0]?.seq;
      omittedBeforeScan = lastSeq > sinceSeq
        && (!Number.isFinite(firstParsedSeq) || firstParsedSeq > sinceSeq + 1);
    }

    return {
      chunks: selected,
      truncated: omittedBeforeScan || omittedByLimit,
    };
  }

  _loadPersistedTerminals() {
    if (!fs.existsSync(this.root)) return;
    for (const file of fs.readdirSync(this.root)) {
      if (!file.endsWith(".json")) continue;
      try {
        const entry = JSON.parse(fs.readFileSync(path.join(this.root, file), "utf8"));
        if (!entry?.terminalId || !entry?.sessionPath) continue;
        const restored = {
          ...entry,
          status: entry.status === "running" ? "stale" : entry.status,
          handle: null,
          transcriptPath: entry.transcriptPath || this._transcriptPath(entry.terminalId),
        };
        this._terminals.set(restored.terminalId, restored);
        this._index(restored);
        if (restored.status !== entry.status) this._persist(restored);
      } catch {}
    }
  }

  _emit(type, entry, extra: any = {}) {
    const event: any = {
      type,
      terminalId: entry.terminalId,
      status: entry.status,
      seq: entry.seq,
      ...(extra || {}),
    };
    if (type === "terminal_started" || type === "terminal_exited" || type === "terminal_closed") {
      event.terminal = this._publicEntry(entry);
    }
    this._emitEvent?.(event, entry.sessionPath);
  }
}
